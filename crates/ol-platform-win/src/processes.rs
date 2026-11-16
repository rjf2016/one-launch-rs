// The process list, a close by window, an end, and a start, from the kernel's own process table: no process is opened to list it, to name its file or to close its windows, and one is opened only to end it. Which process may be touched is decided here, in plain functions; the calls to Windows sit behind `System`, so a made-up table can show that a Riot game, Vanguard and whatever they started are never handed to them.

mod command_line;
mod win32;

use std::collections::{BTreeSet, HashMap, HashSet};

use ol_platform_api::protected::{image_key, is_protected};
use ol_platform_api::{ProcessArg, ProcessError, ProcessInfo, ProcessLister, ProcessRunner};

use self::command_line::{Unwritable, command_line};
use self::win32::Win32;

// One row of the kernel's table: its image name and who started it, never more.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub pid: u32,
    pub parent: u32,
    // When it started, in 100 ns ticks since 1601: with the id, it names one process, as an id alone does not once Windows hands it out again.
    pub created: u64,
    pub name: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct TopWindow {
    pub handle: isize,
    pub pid: u32,
    pub owned: bool,
    pub visible: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Posted {
    Yes,
    Refused,
    Failed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ended {
    Yes,
    Gone,
    Refused,
    Failed(u32),
}

// What the port asks of Windows. Only `end` opens a process, and only `create` makes one.
pub(crate) trait System: Send + Sync {
    fn own_pid(&self) -> u32;
    fn snapshot(&self) -> Result<Vec<Entry>, ProcessError>;
    fn top_windows(&self) -> Vec<TopWindow>;
    fn post_close(&self, window: isize) -> Posted;
    // Each target is ended only if the process that has its id now started when the target did.
    fn end(&self, targets: &[Entry]) -> Vec<Ended>;
    fn image_paths(&self, pids: &[u32]) -> Vec<Option<String>>;
    fn create(&self, exe: &str, command_line: &mut [u16], cwd: &str) -> Result<(), u32>;
}

fn index(entries: &[Entry]) -> HashMap<u32, &Entry> {
    entries.iter().map(|entry| (entry.pid, entry)).collect()
}

// A parent id counts only when that process is older than the child: Windows hands out the ids of ended processes again.
fn parent_of<'a>(child: &Entry, by_pid: &HashMap<u32, &'a Entry>) -> Option<&'a Entry> {
    by_pid
        .get(&child.parent)
        .copied()
        .filter(|parent| parent.pid != child.pid && parent.created <= child.created)
}

fn lineage<'a>(entry: &'a Entry, by_pid: &HashMap<u32, &'a Entry>) -> Vec<&'a Entry> {
    let mut line = vec![entry];
    let mut at = entry;
    while let Some(parent) = parent_of(at, by_pid) {
        if line.iter().any(|seen| seen.pid == parent.pid) || line.len() > by_pid.len() {
            break;
        }
        line.push(parent);
        at = parent;
    }
    line
}

// The protected processes and everything they started, at any depth.
pub(crate) fn shielded(entries: &[Entry]) -> HashSet<u32> {
    let by_pid = index(entries);
    entries
        .iter()
        .filter(|entry| {
            lineage(entry, &by_pid)
                .iter()
                .any(|step| is_protected(&step.name))
        })
        .map(|entry| entry.pid)
        .collect()
}

fn named(entry: &Entry, image: &str) -> bool {
    image_key(&entry.name) == image_key(image)
}

// The processes a kill ends, the ones they started first. Never a protected process or one of its descendants, never this app or what it descends from.
pub(crate) fn end_targets(entries: &[Entry], image: &str, tree: bool, own_pid: u32) -> Vec<Entry> {
    let by_pid = index(entries);
    let shielded = shielded(entries);
    let mut spared: HashSet<u32> = by_pid
        .get(&own_pid)
        .map(|own| lineage(own, &by_pid).iter().map(|step| step.pid).collect())
        .unwrap_or_default();
    spared.insert(own_pid);
    let untouchable = |entry: &Entry| shielded.contains(&entry.pid) || spared.contains(&entry.pid);

    let mut targets = Vec::new();
    let mut visited = HashSet::new();
    for root in entries.iter().filter(|entry| named(entry, image)) {
        if untouchable(root) {
            continue;
        }
        let mut stack = vec![(root, false)];
        while let Some((node, expanded)) = stack.pop() {
            if expanded {
                targets.push(node.clone());
                continue;
            }
            if !visited.insert(node.pid) {
                continue;
            }
            stack.push((node, true));
            if tree {
                let children = entries.iter().filter(|child| {
                    !untouchable(child)
                        && parent_of(child, &by_pid).is_some_and(|parent| parent.pid == node.pid)
                });
                stack.extend(children.map(|child| (child, false)));
            }
        }
    }
    targets
}

// For each process of the image that was there before and after the windows were listed, the same process both times: its first visible window without an owner, as taskkill picks it, or else every hidden one without an owner.
pub(crate) fn close_targets(
    before: &[Entry],
    windows: &[TopWindow],
    after: &[Entry],
    image: &str,
    own_pid: u32,
) -> Vec<Vec<isize>> {
    let (shielded_before, shielded_after) = (shielded(before), shielded(after));
    let after_by_pid = index(after);
    before
        .iter()
        .filter(|entry| named(entry, image) && entry.pid != own_pid)
        .filter(|entry| !shielded_before.contains(&entry.pid))
        .filter(|entry| {
            after_by_pid.get(&entry.pid).is_some_and(|same| {
                same.created == entry.created
                    && named(same, image)
                    && !shielded_after.contains(&same.pid)
            })
        })
        .map(|entry| {
            let mut chosen = Vec::new();
            for window in windows
                .iter()
                .filter(|window| window.pid == entry.pid && !window.owned)
            {
                if window.visible {
                    chosen.clear();
                    chosen.push(window.handle);
                    break;
                }
                chosen.push(window.handle);
            }
            chosen
        })
        .collect()
}

// The processes whose file may be named: all but the protected ones and what they started.
pub(crate) fn path_wanted(entries: &[Entry]) -> Vec<bool> {
    let shielded = shielded(entries);
    entries
        .iter()
        .map(|entry| !shielded.contains(&entry.pid))
        .collect()
}

fn checked(image: &str) -> Result<(), ProcessError> {
    let plain =
        !image.trim().is_empty() && image.len() <= 260 && !image.contains(['\\', '/', ':', '\0']);
    if !plain {
        return Err(ProcessError::Failed(
            "One Launch was asked about a process by something that is not an image name"
                .to_owned(),
        ));
    }
    if is_protected(image) {
        return Err(refused(image));
    }
    Ok(())
}

fn refused(image: &str) -> ProcessError {
    ProcessError::Failed(format!(
        "One Launch never starts, closes or ends {image}: it is a Riot game or part of Vanguard"
    ))
}

fn image_of(exe: &str) -> &str {
    exe.rsplit(['\\', '/']).next().unwrap_or(exe)
}

// Windows drops a name's trailing dots and spaces, takes `name:stream` for the file, and follows a short name or a link to its file: each spelling is checked, and the file it lands on.
fn start_refused(exe: &str) -> bool {
    let given = image_of(exe);
    let landed = std::fs::canonicalize(exe).ok().and_then(|real| {
        real.file_name()
            .map(|name| name.to_string_lossy().into_owned())
    });
    given.contains(':')
        || is_protected(given.trim_end_matches(['.', ' ']))
        || landed.is_some_and(|name| is_protected(&name))
}

#[derive(Default)]
pub struct WindowsProcesses(Port<Win32>);

impl WindowsProcesses {
    pub const fn new() -> Self {
        Self(Port { system: Win32 })
    }
}

impl ProcessLister for WindowsProcesses {
    fn is_running(&self, image: &str) -> Result<bool, ProcessError> {
        self.0.is_running(image)
    }

    fn list_processes(&self) -> Result<Vec<ProcessInfo>, ProcessError> {
        self.0.list_processes()
    }

    fn running_names(&self) -> Result<Vec<String>, ProcessError> {
        self.0.running_names()
    }
}

impl ProcessRunner for WindowsProcesses {
    fn start(&self, exe: &str, args: &[ProcessArg<'_>], cwd: &str) -> Result<(), ProcessError> {
        self.0.start(exe, args, cwd)
    }

    fn close(&self, image: &str) -> Result<(), ProcessError> {
        self.0.close(image)
    }

    fn kill(&self, image: &str, tree: bool) -> Result<(), ProcessError> {
        self.0.kill(image, tree)
    }
}

#[derive(Default)]
pub(crate) struct Port<S> {
    system: S,
}

impl<S: System> ProcessLister for Port<S> {
    fn is_running(&self, image: &str) -> Result<bool, ProcessError> {
        let entries = self.system.snapshot()?;
        Ok(entries
            .iter()
            .any(|entry| !entry.name.is_empty() && named(entry, image)))
    }

    fn list_processes(&self) -> Result<Vec<ProcessInfo>, ProcessError> {
        let entries = self.system.snapshot()?;
        let wanted = path_wanted(&entries);
        let named: Vec<(Entry, bool)> = entries
            .into_iter()
            .zip(wanted)
            .filter(|(entry, _)| !entry.name.is_empty())
            .collect();
        let asked: Vec<u32> = named
            .iter()
            .filter(|(_, wanted)| *wanted)
            .map(|(entry, _)| entry.pid)
            .collect();
        let mut paths = self.system.image_paths(&asked).into_iter();
        Ok(named
            .into_iter()
            .map(|(entry, wanted)| ProcessInfo {
                path: if wanted { paths.next().flatten() } else { None },
                name: entry.name,
                pid: entry.pid,
            })
            .collect())
    }

    fn running_names(&self) -> Result<Vec<String>, ProcessError> {
        let entries = self.system.snapshot()?;
        let names: BTreeSet<String> = entries
            .iter()
            .filter(|entry| !entry.name.is_empty())
            .map(|entry| entry.name.to_lowercase())
            .collect();
        Ok(names.into_iter().collect())
    }
}

impl<S: System> ProcessRunner for Port<S> {
    fn start(&self, exe: &str, args: &[ProcessArg<'_>], cwd: &str) -> Result<(), ProcessError> {
        let image = image_of(exe);
        if start_refused(exe) {
            return Err(refused(image));
        }
        let mut line = command_line(exe, args).map_err(|unwritable| {
            ProcessError::Failed(match unwritable {
                Unwritable::QuotedProgram => {
                    format!("Could not start {image}: its path holds a quote")
                }
                Unwritable::Nul => format!("Could not start {image}: an argument holds a NUL"),
            })
        })?;
        self.system.create(exe, &mut line, cwd).map_err(|code| {
            ProcessError::Failed(format!("Could not start {image} (Windows error {code})"))
        })
    }

    fn close(&self, image: &str) -> Result<(), ProcessError> {
        checked(image)?;
        let before = self.system.snapshot()?;
        let windows = self.system.top_windows();
        let after = self.system.snapshot()?;
        let mut denied = false;
        for windows in close_targets(&before, &windows, &after, image, self.system.own_pid()) {
            let mut posted = false;
            let mut refused = false;
            for window in windows {
                match self.system.post_close(window) {
                    Posted::Yes => posted = true,
                    Posted::Refused => refused = true,
                    Posted::Failed => {}
                }
            }
            denied |= refused && !posted;
        }
        if denied {
            return Err(ProcessError::AccessDenied(image.to_owned()));
        }
        Ok(())
    }

    fn kill(&self, image: &str, tree: bool) -> Result<(), ProcessError> {
        checked(image)?;
        let entries = self.system.snapshot()?;
        let targets = end_targets(&entries, image, tree, self.system.own_pid());
        if targets.is_empty() {
            return Ok(());
        }
        let ended = self.system.end(&targets);
        let (mut denied, mut failed) = (false, None);
        for (target, ended) in targets.iter().zip(ended) {
            if !named(target, image) {
                continue;
            }
            match ended {
                Ended::Refused => denied = true,
                Ended::Failed(code) => failed = Some(code),
                Ended::Yes | Ended::Gone => {}
            }
        }
        if denied {
            return Err(ProcessError::AccessDenied(image.to_owned()));
        }
        if let Some(code) = failed {
            return Err(ProcessError::Failed(format!(
                "Could not end {image} (Windows error {code})"
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use ol_platform_api::protected::PROTECTED_IMAGES;

    use super::*;

    const OWN: u32 = 900;

    fn entry(pid: u32, parent: u32, created: u64, name: &str) -> Entry {
        Entry {
            pid,
            parent,
            created,
            name: name.to_owned(),
        }
    }

    // A PC with a Riot client and its game, Vanguard, Steam with a helper and a game, and this app under explorer; a reused id in it and a process started by the game.
    fn table() -> Vec<Entry> {
        vec![
            entry(4, 0, 1, "System"),
            entry(100, 4, 2, "services.exe"),
            entry(110, 100, 3, "vgc.exe"),
            entry(200, 4, 2, "explorer.exe"),
            entry(210, 200, 5, "vgtray.exe"),
            entry(OWN, 200, 9, "OneLaunch.exe"),
            entry(300, 200, 10, "RiotClientServices.exe"),
            entry(310, 300, 11, "Riot Client.exe"),
            entry(320, 300, 12, "VALORANT.exe"),
            entry(321, 320, 13, "VALORANT-Win64-Shipping.exe"),
            entry(322, 321, 14, "CrashReporter.exe"),
            entry(330, 300, 15, "RiotClientServices.exe"),
            entry(400, 200, 20, "steam.exe"),
            entry(410, 400, 21, "steamwebhelper.exe"),
            entry(411, 410, 22, "steamwebhelper.exe"),
            entry(420, 400, 23, "Game.exe"),
            // Started by a process with id 400 that ended long before this steam.exe took the id.
            entry(430, 400, 19, "Unrelated.exe"),
            entry(500, 322, 30, "steam.exe"),
        ]
    }

    #[derive(Default)]
    struct Calls {
        ended: Vec<u32>,
        posted: Vec<isize>,
        pathed: Vec<u32>,
        created: Vec<String>,
    }

    struct MadeUp {
        entries: Vec<Entry>,
        windows: Vec<TopWindow>,
        calls: Mutex<Calls>,
        refuse: bool,
    }

    impl MadeUp {
        fn new(entries: Vec<Entry>) -> Self {
            let windows = entries
                .iter()
                .flat_map(|entry| {
                    let handle = isize::try_from(entry.pid).unwrap() * 10;
                    [
                        TopWindow {
                            handle,
                            pid: entry.pid,
                            owned: false,
                            visible: false,
                        },
                        TopWindow {
                            handle: handle + 1,
                            pid: entry.pid,
                            owned: false,
                            visible: true,
                        },
                        TopWindow {
                            handle: handle + 2,
                            pid: entry.pid,
                            owned: true,
                            visible: true,
                        },
                    ]
                })
                .collect();
            Self {
                entries,
                windows,
                calls: Mutex::default(),
                refuse: false,
            }
        }

        fn calls(&self) -> std::sync::MutexGuard<'_, Calls> {
            self.calls.lock().unwrap()
        }
    }

    impl System for MadeUp {
        fn own_pid(&self) -> u32 {
            OWN
        }

        fn snapshot(&self) -> Result<Vec<Entry>, ProcessError> {
            Ok(self.entries.clone())
        }

        fn top_windows(&self) -> Vec<TopWindow> {
            self.windows.clone()
        }

        fn post_close(&self, window: isize) -> Posted {
            self.calls().posted.push(window);
            if self.refuse {
                Posted::Refused
            } else {
                Posted::Yes
            }
        }

        fn end(&self, targets: &[Entry]) -> Vec<Ended> {
            self.calls()
                .ended
                .extend(targets.iter().map(|target| target.pid));
            targets
                .iter()
                .map(|_| {
                    if self.refuse {
                        Ended::Refused
                    } else {
                        Ended::Yes
                    }
                })
                .collect()
        }

        fn image_paths(&self, pids: &[u32]) -> Vec<Option<String>> {
            self.calls().pathed.extend(pids);
            pids.iter()
                .map(|pid| Some(format!(r"D:\made-up\{pid}.exe")))
                .collect()
        }

        fn create(&self, exe: &str, _: &mut [u16], _: &str) -> Result<(), u32> {
            self.calls().created.push(exe.to_owned());
            Err(2)
        }
    }

    fn protected_pids() -> HashSet<u32> {
        HashSet::from([110, 210, 320, 321, 322, 500])
    }

    fn touched(calls: &Calls) -> HashSet<u32> {
        calls
            .ended
            .iter()
            .chain(&calls.pathed)
            .copied()
            .chain(
                calls
                    .posted
                    .iter()
                    .map(|window| u32::try_from(window / 10).unwrap()),
            )
            .collect()
    }

    #[test]
    fn the_shield_covers_riot_games_vanguard_and_whatever_they_started() {
        assert_eq!(shielded(&table()), protected_pids());
    }

    #[test]
    fn a_tree_end_takes_what_the_image_started_and_nothing_shielded_or_unrelated() {
        let pids =
            |targets: Vec<Entry>| -> Vec<u32> { targets.iter().map(|target| target.pid).collect() };
        let steam = pids(end_targets(&table(), "STEAM", true, OWN));
        assert_eq!(
            steam.iter().copied().collect::<HashSet<_>>(),
            HashSet::from([400, 410, 411, 420])
        );
        assert!(
            steam.iter().position(|pid| *pid == 411) < steam.iter().position(|pid| *pid == 410)
        );
        assert_eq!(steam.last(), Some(&400));
        assert_eq!(pids(end_targets(&table(), "steam.exe", false, OWN)), [400]);

        let client = pids(end_targets(&table(), "RiotClientServices.exe", true, OWN));
        assert_eq!(
            client.iter().copied().collect::<HashSet<_>>(),
            HashSet::from([300, 310, 330])
        );
        assert!(pids(end_targets(&table(), "explorer.exe", true, OWN)).is_empty());
        assert!(pids(end_targets(&table(), "OneLaunch.exe", true, OWN)).is_empty());
        for image in PROTECTED_IMAGES {
            assert!(
                end_targets(&table(), image, true, OWN).is_empty(),
                "{image}"
            );
        }
    }

    #[test]
    fn a_close_goes_to_the_visible_window_and_only_of_a_process_that_stayed_the_same() {
        let made_up = MadeUp::new(table());
        assert_eq!(
            close_targets(&table(), &made_up.windows, &table(), "Riot Client", OWN),
            [vec![3101]]
        );
        let hidden = [
            TopWindow {
                handle: 7,
                pid: 400,
                owned: false,
                visible: false,
            },
            TopWindow {
                handle: 8,
                pid: 400,
                owned: false,
                visible: false,
            },
        ];
        assert_eq!(
            close_targets(&table(), &hidden, &table(), "steam.exe", OWN),
            [vec![7, 8]]
        );

        let mut after = table();
        after
            .iter_mut()
            .find(|entry| entry.pid == 400)
            .unwrap()
            .created = 99;
        assert!(close_targets(&table(), &hidden, &after, "steam.exe", OWN).is_empty());
        let mut taken = table();
        let reused = taken.iter_mut().find(|entry| entry.pid == 400).unwrap();
        reused.name = "VALORANT.exe".to_owned();
        assert!(close_targets(&table(), &hidden, &taken, "steam.exe", OWN).is_empty());
        assert!(close_targets(&taken, &hidden, &table(), "steam.exe", OWN).is_empty());
    }

    #[test]
    fn whatever_the_caller_asks_no_protected_process_is_opened_named_or_sent_a_close() {
        let processes = Port {
            system: MadeUp::new(table()),
        };
        let mut images: Vec<String> = table().iter().map(|entry| entry.name.clone()).collect();
        images.extend(PROTECTED_IMAGES.iter().map(|image| (*image).to_owned()));
        images.extend(["valorant", "VGC", "", " ", r"C:\x\vgc.exe", "*"].map(str::to_owned));
        for image in &images {
            let _ = processes.close(image);
            let _ = processes.kill(image, true);
            let _ = processes.kill(image, false);
            let _ = processes.is_running(image);
        }
        let listed = processes.list_processes().unwrap();
        let _ = processes.running_names();

        let calls = processes.system.calls();
        assert!(!calls.ended.is_empty() && !calls.posted.is_empty() && !calls.pathed.is_empty());
        let touched = touched(&calls);
        for pid in protected_pids() {
            assert!(!touched.contains(&pid), "process {pid} was touched");
        }
        assert!(!calls.ended.contains(&OWN) && !calls.ended.contains(&200));
        assert!(!calls.posted.iter().any(|window| window / 10 == 900));
        for process in &listed {
            assert_eq!(
                process.path.is_none(),
                protected_pids().contains(&process.pid),
                "{}",
                process.pid
            );
        }
        assert_eq!(listed.len(), table().len());
    }

    #[test]
    fn a_protected_image_is_refused_before_anything_is_asked() {
        let processes = Port {
            system: MadeUp::new(table()),
        };
        for image in ["VALORANT.exe", "vgtray", "League of Legends.exe"] {
            let closed = processes.close(image).unwrap_err().to_string();
            assert!(closed.contains("Riot game or part of Vanguard"), "{closed}");
            assert!(processes.kill(image, false).is_err());
        }
        for exe in [
            r"D:\Riot Games\VALORANT\live\VALORANT.exe",
            r"D:\Riot Games\VALORANT\live\valorant",
            r"D:\Riot Games\VALORANT\live\VALORANT.exe. .",
            r"D:\Riot Games\VALORANT\live\VALORANT.exe::$DATA",
            r"D:/Riot Vanguard/vgtray.exe",
        ] {
            let started = processes.start(exe, &[], "").unwrap_err().to_string();
            assert!(
                started.contains("Riot game or part of Vanguard"),
                "{started}"
            );
        }
        let calls = processes.system.calls();
        assert!(calls.ended.is_empty() && calls.posted.is_empty() && calls.created.is_empty());
    }

    #[test]
    fn windows_refusing_a_close_or_an_end_is_access_denied() {
        let mut made_up = MadeUp::new(table());
        made_up.refuse = true;
        let processes = Port { system: made_up };
        let closed = processes.close("steam.exe").unwrap_err();
        assert_eq!(closed.code(), Some("PROCESS_ACCESS_DENIED"));
        let ended = processes.kill("steam.exe", true).unwrap_err();
        assert_eq!(ended, ProcessError::AccessDenied("steam.exe".to_owned()));
        assert!(processes.kill("nothing-runs-this.exe", true).is_ok());
    }

    #[test]
    fn a_secret_argument_is_in_no_error() {
        let processes = Port {
            system: MadeUp::new(table()),
        };
        for args in [
            &[
                ProcessArg::Plain("-login"),
                ProcessArg::Secret("placeholder-password-1"),
            ][..],
            &[ProcessArg::Secret("placeholder\0password")],
        ] {
            let refused = processes
                .start(r"D:\Steam\steam.exe", args, r"D:\Steam")
                .unwrap_err();
            let shown = format!("{refused} {refused:?}");
            assert!(!shown.contains("placeholder"), "{shown}");
            assert!(shown.contains("steam.exe"), "{shown}");
        }
    }

    #[test]
    fn the_names_are_every_image_in_lower_case_once() {
        let processes = Port {
            system: MadeUp::new(table()),
        };
        let names = processes.running_names().unwrap();
        assert!(names.contains(&"valorant-win64-shipping.exe".to_owned()));
        assert_eq!(names.iter().filter(|name| *name == "steam.exe").count(), 1);
        assert!(processes.is_running("VALORANT").unwrap());
        assert!(!processes.is_running("nothing.exe").unwrap());
    }
}
