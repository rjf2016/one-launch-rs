use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ol_formats::epic::session::{EpicSessionBlob, plan_session_write, read_session_from_ini};
use ol_platform_api::Clock;
use ol_platform_fake::{CLOSE_ARGUMENT, FakeClock, FakeProcesses, FakeRegistry, RegistryValue};
use time::format_description::well_known::Rfc3339;

// Where an Epic Games Launcher keeps what the app reads. The simulator has its own copy of these names: it stands in for the launcher, not for the app's idea of the launcher.
const EXE: &str = "EpicGamesLauncher.exe";
const UNINSTALL_LOCATION: &str = r"HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\{AEB35C6C-B6D4-4AA0-8452-DE699737B5F6}\InstallLocation";
const URI_COMMAND: &str = r"HKLM\SOFTWARE\Classes\com.epicgames.launcher\shell\open\command\";
const APP_DATA_PATH: &str = r"HKLM\SOFTWARE\WOW6432Node\Epic Games\EpicGamesLauncher\AppDataPath";

const EXPIRED_LINE: &str = "[..]FAccountService::OnLoginComplete - 0 FALSE  errormessage=Sorry the refresh token 'deadbeefdeadbeefdeadbeefdeadbeef' is invalid. Reason: TOKEN_EXPIRED errorcode=18036 errorpath=x";

#[derive(Clone, Default)]
pub struct EpicKnobs {
    // Sessions the launcher refuses at its start: a session's data, or the account id it signs in as.
    pub expired: Vec<String>,
    // The launcher writes a new session for the signed-in account at every start.
    pub rotate_on_start: bool,
    // The sign-in fails with this text without a look at the session: the service is down, say.
    pub sign_in_error: Option<String>,
    pub react_ms: u64,
    // Once on its sign-in page, the simulated user signs in by hand after this long.
    pub sign_in_when_asked_ms: Option<u64>,
    // The account id a sign-in by hand lands on.
    pub sign_in_as: Option<String>,
    // An account id the launcher is signed in as when the run begins.
    pub running_as: Option<String>,
}

struct State {
    // What "Epic's servers" know: the account each session signs in as.
    servers: HashMap<String, String>,
    // One count for log headers and new sessions, as the Electron app's simulator keeps it.
    counter: u32,
}

pub struct EpicSim {
    knobs: EpicKnobs,
    ini_path: PathBuf,
    log_path: PathBuf,
    clock: Arc<FakeClock>,
    state: Mutex<State>,
}

impl EpicSim {
    // The registry values an Epic install leaves behind.
    pub fn seed_registry(registry: &FakeRegistry, install_location: &str, program_data: &str) {
        let text = |value: String| RegistryValue::Text(value);
        registry.seed(UNINSTALL_LOCATION, text(format!("{install_location}\\")));
        registry.seed(
            URI_COMMAND,
            text(format!(
                "\"{install_location}\\Launcher\\Portal\\Binaries\\Win64\\{EXE}\" %1"
            )),
        );
        registry.seed(
            APP_DATA_PATH,
            text(format!("{program_data}\\Epic\\EpicGamesLauncher\\Data\\")),
        );
    }

    // Where the launcher's settings file is, for whoever has to know which session it holds before the simulator exists.
    pub fn settings_file(local_app_data: &str) -> PathBuf {
        [
            local_app_data,
            "EpicGamesLauncher",
            "Saved",
            "Config",
            "Windows",
            "GameUserSettings.ini",
        ]
        .iter()
        .collect()
    }

    pub fn attach(
        local_app_data: &str,
        processes: &FakeProcesses,
        clock: Arc<FakeClock>,
        servers: HashMap<String, String>,
        knobs: EpicKnobs,
    ) -> Arc<Self> {
        let sim = Arc::new(Self {
            knobs,
            ini_path: Self::settings_file(local_app_data),
            log_path: [
                local_app_data,
                "EpicGamesLauncher",
                "Saved",
                "Logs",
                "EpicGamesLauncher.log",
            ]
            .iter()
            .collect(),
            clock,
            state: Mutex::new(State {
                servers,
                counter: 0,
            }),
        });
        if let Some(uid) = &sim.knobs.running_as {
            processes.set_running(EXE, true);
            sim.write_log(&[
                sim.log_header(),
                "[..]FAccountService::AutoSignIn".to_owned(),
                format!("[..]FAccountService::OnLoginComplete - 0 TRUE {uid} "),
            ]);
        }
        let hears = Arc::clone(&sim);
        processes.on_start(move |exe, args| hears.on_start(exe, args));
        sim
    }

    fn on_start(self: &Arc<Self>, exe: &str, args: &[String]) {
        let image = exe.rsplit(['\\', '/']).next().unwrap_or(exe);
        let closing = args.first().map(String::as_str) == Some(CLOSE_ARGUMENT);
        if !image.eq_ignore_ascii_case(EXE) || closing {
            return;
        }
        if self.knobs.react_ms > 0 {
            let sim = Arc::clone(self);
            self.clock
                .schedule(Duration::from_millis(self.knobs.react_ms), move || {
                    sim.react();
                });
        } else {
            self.react();
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn next(&self) -> u32 {
        let mut state = self.state();
        state.counter += 1;
        state.counter
    }

    fn fresh_blob(&self) -> String {
        STANDARD.encode(format!("fresh-blob-{}-{}", self.next(), "x".repeat(40)))
    }

    // The launcher stamps the first line of its log with the time, which is what tells one run's log from the next.
    fn log_header(&self) -> String {
        let now = self
            .clock
            .now()
            .format(&Rfc3339)
            .expect("a date formats as RFC 3339");
        format!("Log file open, {now} run {}", self.next())
    }

    // Starts a new EpicGamesLauncher.log, as the launcher does at every start.
    fn write_log(&self, lines: &[String]) {
        let written = self
            .log_path
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| fs::write(&self.log_path, format!("{}\r\n", lines.join("\r\n"))));
        written.expect("the simulated launcher could not write its log");
    }

    fn append_log(&self, lines: &[String]) {
        let written = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .and_then(|mut log| log.write_all(format!("{}\r\n", lines.join("\r\n")).as_bytes()));
        written.expect("the simulated launcher could not write its log");
    }

    fn read_ini(&self) -> String {
        let bytes = fs::read(&self.ini_path).expect("the simulated launcher has a settings file");
        String::from_utf8_lossy(&bytes).into_owned()
    }

    // The launcher remembers a new session for this account.
    fn remember_fresh(&self, text: &str, uid: &str) {
        let fresh = self.fresh_blob();
        self.state().servers.insert(fresh.clone(), uid.to_owned());
        let blob = EpicSessionBlob {
            enable: "True".to_owned(),
            data: fresh,
            offline: None,
        };
        fs::write(&self.ini_path, plan_session_write(text, Some(&blob)).text)
            .expect("the simulated launcher could not write its settings file");
    }

    // Stands in for the user signing in on the launcher's page; the launcher then remembers a new session for that account.
    pub fn sign_in_by_hand(&self, account: Option<&str>) {
        let uid = account
            .or(self.knobs.sign_in_as.as_deref())
            .expect("a sign-in by hand needs an account id: pass one or set signInAs");
        self.append_log(&[
            "[..]FAccountService::SignIn (exchange code)".to_owned(),
            format!("[..]FAccountService::OnLoginComplete - 0 TRUE {uid} "),
        ]);
        self.remember_fresh(&self.read_ini(), uid);
    }

    fn react(self: &Arc<Self>) {
        let text = self.read_ini();
        let disk = read_session_from_ini(&text);
        let mut lines = vec![
            self.log_header(),
            "[..]FAccountService::AutoSignIn".to_owned(),
        ];
        let mut signed_in = false;
        if let (true, Some(data)) = (disk.remembered, &disk.data) {
            let known = self.state().servers.get(data).cloned();
            let refused = |uid: &String| {
                self.knobs.expired.contains(data) || self.knobs.expired.contains(uid)
            };
            match (&self.knobs.sign_in_error, known) {
                (Some(error), _) => lines.push(format!(
                    "[..]FAccountService::OnLoginComplete - 0 FALSE  errormessage={error}"
                )),
                (None, Some(uid)) if !refused(&uid) => {
                    lines.push(format!(
                        "[..]FAccountService::OnLoginComplete - 0 TRUE {uid} "
                    ));
                    signed_in = true;
                    if self.knobs.rotate_on_start {
                        self.remember_fresh(&text, &uid);
                    }
                }
                _ => lines.push(EXPIRED_LINE.to_owned()),
            }
        }
        self.write_log(&lines);
        if !signed_in && let Some(after) = self.knobs.sign_in_when_asked_ms {
            let sim = Arc::clone(self);
            self.clock.schedule(Duration::from_millis(after), move || {
                sim.sign_in_by_hand(None);
            });
        }
    }
}
