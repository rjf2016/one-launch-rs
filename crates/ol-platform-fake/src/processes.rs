use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

use ol_platform_api::{ProcessArg, ProcessError, ProcessInfo, ProcessLister, ProcessRunner};
use serde_json::Value;

use crate::OpLog;

const SECRET_ARGUMENT: &str = "<password>";
// What a listener hears in place of arguments when a program is asked to close.
pub const CLOSE_ARGUMENT: &str = "<close>";

// How the fake process table behaves beyond "a started program runs".
#[derive(Clone, Default)]
pub struct ProcessKnobs {
    pub running: Vec<String>,
    pub table: Vec<ProcessInfo>,
    // How many looks at the process list a launcher asked to shut down stays in it.
    pub shutdown_polls: Option<u32>,
    // Images whose start never shows up as running.
    pub start_fails: Vec<String>,
    // Images Windows will not let the app end.
    pub elevated: Vec<String>,
    // An image that counts as running again after it was seen stopped this many times: something else started it.
    pub reappears_after_polls: BTreeMap<String, u32>,
}

struct State {
    running: BTreeSet<String>,
    table: Vec<ProcessInfo>,
    pending_shutdown: Option<(String, i64)>,
    shutdown_polls: u32,
    start_fails: BTreeSet<String>,
    elevated: BTreeSet<String>,
    reappears: BTreeMap<String, (u32, u32)>,
}

type StartListener = Arc<dyn Fn(&str, &[String]) + Send + Sync>;

// A process table in memory. Starting a program marks its image as running; starting one with -shutdown makes it go after a few looks; a kill ends it at once.
pub struct FakeProcesses {
    state: Mutex<State>,
    listeners: Mutex<Vec<StartListener>>,
    log: OpLog,
}

fn image_of(exe: &str) -> String {
    exe.rsplit(['\\', '/']).next().unwrap_or(exe).to_lowercase()
}

impl FakeProcesses {
    pub fn new(log: OpLog, knobs: ProcessKnobs) -> Self {
        let lower = |names: Vec<String>| names.iter().map(|name| name.to_lowercase()).collect();
        Self {
            state: Mutex::new(State {
                running: lower(knobs.running),
                table: knobs.table,
                pending_shutdown: None,
                shutdown_polls: knobs.shutdown_polls.unwrap_or(2),
                start_fails: lower(knobs.start_fails),
                elevated: lower(knobs.elevated),
                reappears: knobs
                    .reappears_after_polls
                    .into_iter()
                    .map(|(image, after)| (image.to_lowercase(), (after, 0)))
                    .collect(),
            }),
            listeners: Mutex::new(Vec::new()),
            log,
        }
    }

    // Hears of every start, as the launcher itself would: a simulator hangs its reactions here.
    pub fn on_start(&self, listener: impl Fn(&str, &[String]) + Send + Sync + 'static) {
        self.listeners
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(Arc::new(listener));
    }

    pub fn set_running(&self, image: &str, running: bool) {
        let mut state = self.state();
        if running {
            state.running.insert(image.to_lowercase());
        } else {
            state.running.remove(&image.to_lowercase());
        }
    }

    // A program that runs beside the launchers, such as a game: it is listed with its path, once.
    pub fn add_to_table(&self, name: &str, path: &str) {
        let mut state = self.state();
        let listed = |process: &ProcessInfo| process.name.eq_ignore_ascii_case(name);
        if !state.table.iter().any(listed) {
            let pid = 15_412 + u32::try_from(state.table.len()).unwrap_or(0);
            state.table.push(ProcessInfo {
                name: name.to_owned(),
                pid,
                path: Some(path.to_owned()),
            });
        }
    }

    fn listeners(&self) -> Vec<StartListener> {
        self.listeners
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl ProcessLister for FakeProcesses {
    fn is_running(&self, image: &str) -> Result<bool, ProcessError> {
        let image = image.to_lowercase();
        let mut state = self.state();
        if let Some((pending, polls)) = &mut state.pending_shutdown
            && *pending == image
        {
            *polls -= 1;
            if *polls <= 0 {
                state.running.remove(&image);
                state.pending_shutdown = None;
            }
        }
        if state.running.contains(&image) {
            return Ok(true);
        }
        let Some((after, stopped)) = state.reappears.get_mut(&image) else {
            return Ok(false);
        };
        *stopped += 1;
        if *stopped <= *after {
            return Ok(false);
        }
        state.reappears.remove(&image);
        state.running.insert(image);
        Ok(true)
    }

    fn list_processes(&self) -> Result<Vec<ProcessInfo>, ProcessError> {
        Ok(self.state().table.clone())
    }
}

impl ProcessRunner for FakeProcesses {
    fn start(&self, exe: &str, args: &[ProcessArg<'_>], _cwd: &str) -> Result<(), ProcessError> {
        let shown: Vec<Value> = args
            .iter()
            .map(|arg| match arg {
                ProcessArg::Plain(text) => (*text).into(),
                ProcessArg::Secret(_) => SECRET_ARGUMENT.into(),
            })
            .collect();
        self.log
            .record("proc.start", [("exe", exe.into()), ("args", shown.into())]);
        let passed: Vec<String> = args
            .iter()
            .map(|arg| match arg {
                ProcessArg::Plain(text) | ProcessArg::Secret(text) => (*text).to_owned(),
            })
            .collect();
        for listener in self.listeners() {
            listener(exe, &passed);
        }
        let image = image_of(exe);
        let mut state = self.state();
        if passed.iter().any(|arg| arg == "-shutdown") {
            let polls = i64::from(state.shutdown_polls);
            state.pending_shutdown = Some((image, polls));
        } else if !state.start_fails.contains(&image) {
            state.running.insert(image);
        }
        Ok(())
    }

    // The image goes after a few looks, as one started with -shutdown does.
    fn close(&self, image: &str) -> Result<(), ProcessError> {
        let lower = image.to_lowercase();
        if self.state().elevated.contains(&lower) {
            return Err(ProcessError::AccessDenied(image.to_owned()));
        }
        self.log.record(
            "proc.close",
            [("exe", image.into()), ("args", vec![CLOSE_ARGUMENT].into())],
        );
        for listener in self.listeners() {
            listener(image, &[CLOSE_ARGUMENT.to_owned()]);
        }
        let mut state = self.state();
        let polls = i64::from(state.shutdown_polls);
        state.pending_shutdown = Some((lower, polls));
        Ok(())
    }

    fn kill(&self, image: &str, tree: bool) -> Result<(), ProcessError> {
        self.log.record(
            "proc.kill",
            [
                ("exe", image.into()),
                ("args", Value::Array(Vec::new())),
                ("tree", tree.into()),
            ],
        );
        let lower = image.to_lowercase();
        let mut state = self.state();
        if state.elevated.contains(&lower) {
            return Err(ProcessError::AccessDenied(image.to_owned()));
        }
        state.running.remove(&lower);
        state.pending_shutdown = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const EXE: &str = r"D:\Scratch\launcher\Launcher.exe";

    fn processes(knobs: ProcessKnobs) -> (FakeProcesses, OpLog) {
        let log = OpLog::new();
        (FakeProcesses::new(log.clone(), knobs), log)
    }

    #[test]
    fn a_started_program_runs_and_one_asked_to_shut_down_goes_after_its_polls() {
        let (processes, log) = processes(ProcessKnobs {
            shutdown_polls: Some(3),
            ..ProcessKnobs::default()
        });
        assert!(!processes.is_running("launcher.exe").unwrap());
        processes.start(EXE, &[], r"D:\Scratch\launcher").unwrap();
        assert!(processes.is_running("LAUNCHER.EXE").unwrap());
        processes
            .start(
                EXE,
                &[ProcessArg::Plain("-shutdown")],
                r"D:\Scratch\launcher",
            )
            .unwrap();
        assert!(processes.is_running("launcher.exe").unwrap());
        assert!(processes.is_running("launcher.exe").unwrap());
        assert!(!processes.is_running("launcher.exe").unwrap());
        assert_eq!(
            log.snapshot(),
            [
                json!({ "k": "plat", "op": "proc.start", "exe": EXE, "args": [] }),
                json!({ "k": "plat", "op": "proc.start", "exe": EXE, "args": ["-shutdown"] }),
            ]
        );
    }

    #[test]
    fn a_secret_argument_reaches_the_launcher_and_not_the_log() {
        let (processes, log) = processes(ProcessKnobs::default());
        let heard = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&heard);
        processes.on_start(move |_, args| sink.lock().unwrap().extend(args.to_vec()));
        processes
            .start(
                EXE,
                &[
                    ProcessArg::Plain("-login"),
                    ProcessArg::Secret("placeholder-password-1"),
                ],
                "",
            )
            .unwrap();
        assert_eq!(*heard.lock().unwrap(), ["-login", "placeholder-password-1"]);
        assert!(!format!("{:?}", log.snapshot()).contains("placeholder"));
    }

    #[test]
    fn the_knobs_make_a_start_fail_a_kill_be_refused_and_a_program_come_back() {
        let (processes, log) = processes(ProcessKnobs {
            running: vec!["Elevated.exe".to_owned()],
            start_fails: vec!["launcher.exe".to_owned()],
            elevated: vec!["elevated.exe".to_owned()],
            reappears_after_polls: BTreeMap::from([("back.exe".to_owned(), 2)]),
            ..ProcessKnobs::default()
        });
        processes.start(EXE, &[], "").unwrap();
        assert!(!processes.is_running("launcher.exe").unwrap());
        let refused = processes.kill("elevated.exe", true).unwrap_err();
        assert_eq!(refused.code(), Some("PROCESS_ACCESS_DENIED"));
        assert!(processes.is_running("elevated.exe").unwrap());
        assert!(!processes.is_running("back.exe").unwrap());
        assert!(!processes.is_running("back.exe").unwrap());
        assert!(processes.is_running("back.exe").unwrap());
        processes.kill("back.exe", false).unwrap();
        assert!(!processes.is_running("back.exe").unwrap());
        assert_eq!(
            log.snapshot()[1],
            json!({ "k": "plat", "op": "proc.kill", "exe": "elevated.exe", "args": [], "tree": true })
        );
    }
}
