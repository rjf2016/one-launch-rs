use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ol_formats::steam::loginusers::{SteamLoginUser, parse_login_users, steam_id3};
use ol_platform_api::{Hive, RegistryReader};
use ol_platform_fake::{FakeClock, FakeProcesses, FakeRegistry, RegistryValue};

// Where a Steam client keeps what the app reads. The simulator has its own copy of these names: it stands in for Steam, not for the app's idea of Steam.
const EXE: &str = "steam.exe";
const HKCU_KEY: &str = r"Software\Valve\Steam";
const ACTIVE_USER: &str = r"HKCU\Software\Valve\Steam\ActiveProcess\ActiveUser";
const INSTALL_PATH: &str = r"HKLM\SOFTWARE\WOW6432Node\Valve\Steam\InstallPath";

// The account id the simulator signs in for a login loginusers.vdf does not know.
const UNKNOWN_ACCOUNT_ID3: u32 = 424_242;

const UPDATER_STARTUP_LINE: &str = "Startup - updater built Sep  2 2026 18:32:43";
const UPDATE_LINES: [&str; 6] = [
    "Downloaded new manifest: /steam_client_win64 version 1788652215, installed version 1785799196, existing pending version 0",
    "Downloading update (12,842 of 212,314 KB)...",
    "Download complete.",
    "Found pending update",
    "Installing update...",
    "Extracting package...",
];
const UPDATED_LINES: [&str; 4] = [
    "Update complete, launching Steam...",
    "Shutdown",
    UPDATER_STARTUP_LINE,
    "Verification complete",
];

#[derive(Clone, Default)]
pub struct SteamKnobs {
    // Logins whose remembered session Steam rejects.
    pub expired_sessions: Vec<String>,
    pub wrong_password: bool,
    // What the sign-in log says after a -login start, such as "Account Logon Denied".
    pub login_verdict: Option<String>,
    // Steam updates itself for this long before it signs in; negative never finishes.
    pub self_update_ms: Option<i64>,
    pub react_ms: u64,
    // Once on its sign-in form, the simulated user signs in by hand after this long.
    pub sign_in_when_asked_ms: Option<u64>,
    pub sign_in_as: Option<u32>,
    // A login Steam is already signed in as when the run begins.
    pub running_as: Option<String>,
}

pub struct SteamSim {
    knobs: SteamKnobs,
    steam_dir: PathBuf,
    registry: Arc<FakeRegistry>,
    clock: Arc<FakeClock>,
    requested_id3: Mutex<Option<u32>>,
}

fn id3(user: &SteamLoginUser) -> Option<u32> {
    steam_id3(&user.steam_id64).and_then(|id| u32::try_from(id).ok())
}

impl SteamSim {
    // The registry values a Steam install leaves behind: its folder in Steam's own lower-case, forward-slash spelling, and the account it signs in.
    pub fn seed_registry(registry: &FakeRegistry, steam_dir: &str) {
        let spelled = steam_dir.replace('\\', "/").to_lowercase();
        let text = |value: String| RegistryValue::Text(value);
        registry.seed(
            &format!("HKCU\\{HKCU_KEY}\\SteamPath"),
            text(spelled.clone()),
        );
        registry.seed(
            &format!("HKCU\\{HKCU_KEY}\\SteamExe"),
            text(format!("{spelled}/{EXE}")),
        );
        registry.seed(
            &format!("HKCU\\{HKCU_KEY}\\AutoLoginUser"),
            text("user_1".to_owned()),
        );
        registry.seed(
            &format!("HKCU\\{HKCU_KEY}\\RunningAppID"),
            RegistryValue::Dword(0),
        );
        registry.seed(INSTALL_PATH, text(steam_dir.to_owned()));
    }

    pub fn attach(
        steam_dir: &str,
        registry: Arc<FakeRegistry>,
        processes: &FakeProcesses,
        clock: Arc<FakeClock>,
        knobs: SteamKnobs,
    ) -> Arc<Self> {
        let sim = Arc::new(Self {
            knobs,
            steam_dir: PathBuf::from(steam_dir),
            registry,
            clock,
            requested_id3: Mutex::new(None),
        });
        if let Some(login) = &sim.knobs.running_as {
            processes.set_running(EXE, true);
            let signed_in = sim
                .users()
                .iter()
                .find(|user| user.account_name.to_lowercase() == login.to_lowercase())
                .and_then(id3)
                .unwrap_or(UNKNOWN_ACCOUNT_ID3);
            sim.sign_in_by_hand(Some(signed_in));
        }
        let hears = Arc::clone(&sim);
        processes.on_start(move |exe, args| hears.on_start(exe, args));
        sim
    }

    fn on_start(self: &Arc<Self>, exe: &str, args: &[String]) {
        let image = exe.rsplit(['\\', '/']).next().unwrap_or(exe).to_lowercase();
        if image != EXE || args.iter().any(|arg| arg == "-shutdown") {
            return;
        }
        let sim = Arc::clone(self);
        let args = args.to_vec();
        let start = move || {
            let Some(update_ms) = sim.knobs.self_update_ms else {
                sim.react(&args);
                return;
            };
            sim.bootstrap_says(UPDATER_STARTUP_LINE);
            for line in UPDATE_LINES {
                sim.bootstrap_says(line);
            }
            let Ok(update_ms) = u64::try_from(update_ms) else {
                return;
            };
            let after = Arc::clone(&sim);
            sim.clock
                .schedule(Duration::from_millis(update_ms), move || {
                    for line in UPDATED_LINES {
                        after.bootstrap_says(line);
                    }
                    let then = Arc::clone(&after);
                    after
                        .clock
                        .schedule(Duration::from_millis(20), move || then.react(&args));
                });
        };
        if self.knobs.react_ms > 0 {
            self.clock
                .schedule(Duration::from_millis(self.knobs.react_ms), start);
        } else {
            start();
        }
    }

    fn users(&self) -> Vec<SteamLoginUser> {
        let file = self.steam_dir.join("config").join("loginusers.vdf");
        fs::read(file)
            .ok()
            .and_then(|bytes| parse_login_users(&String::from_utf8_lossy(&bytes)).ok())
            .unwrap_or_default()
    }

    fn append(&self, name: &str, line: &str) {
        let logs = self.steam_dir.join("logs");
        let written = fs::create_dir_all(&logs).and_then(|()| {
            fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(logs.join(name))?
                .write_all(line.as_bytes())
        });
        written.expect("the simulated Steam could not write its log");
    }

    // A line in steamui_login.txt, as the Steam client would add it.
    pub fn steam_says(&self, line: &str) {
        self.append(
            "steamui_login.txt",
            &format!("[2026-09-12 15:00:45] [ WaitingForServerResponse ] {line}\n"),
        );
    }

    // A line in bootstrap_log.txt, as steam.exe's updater would add it.
    pub fn bootstrap_says(&self, line: &str) {
        self.append(
            "bootstrap_log.txt",
            &format!("[2026-09-11 12:56:02] {line}\n"),
        );
    }

    fn set_active_user(&self, id: u32) {
        self.registry.seed(ACTIVE_USER, RegistryValue::Dword(id));
    }

    fn requested(&self) -> Option<u32> {
        *self
            .requested_id3
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn request(&self, id: Option<u32>) {
        *self
            .requested_id3
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = id;
    }

    // Stands in for the user finishing the sign-in in the Steam window.
    pub fn sign_in_by_hand(&self, account: Option<u32>) {
        let account = account
            .or(self.knobs.sign_in_as)
            .or_else(|| self.requested())
            .unwrap_or(UNKNOWN_ACCOUNT_ID3);
        self.steam_says("SetLoginState: Success - OK");
        self.set_active_user(account);
    }

    fn lands_on_form(self: &Arc<Self>) {
        if let Some(after) = self.knobs.sign_in_when_asked_ms {
            let sim = Arc::clone(self);
            self.clock.schedule(Duration::from_millis(after), move || {
                sim.sign_in_by_hand(None)
            });
        }
    }

    fn react(self: &Arc<Self>, args: &[String]) {
        let users = self.users();
        if args.first().map(String::as_str) == Some("-login") {
            let known = users
                .iter()
                .find(|user| Some(&user.account_name) == args.get(1))
                .and_then(id3);
            self.request(known);
            if args.len() == 2 {
                self.steam_says("SetLoginState: WaitingForCredentials - OK");
                self.set_active_user(0);
                self.lands_on_form();
                return;
            }
            if let Some(verdict) = &self.knobs.login_verdict {
                self.steam_says(&format!("SetLoginState: WaitingForCredentials - {verdict}"));
                self.set_active_user(0);
                self.lands_on_form();
                return;
            }
            if self.knobs.wrong_password {
                self.set_active_user(0);
                self.lands_on_form();
            } else {
                self.set_active_user(known.unwrap_or(UNKNOWN_ACCOUNT_ID3));
            }
            return;
        }
        let auto = self
            .registry
            .get_string(Hive::Hkcu, HKCU_KEY, "AutoLoginUser")
            .ok()
            .flatten()
            .unwrap_or_default()
            .to_lowercase();
        let user = users
            .iter()
            .find(|user| user.account_name.to_lowercase() == auto);
        self.request(user.and_then(id3));
        let signed_in = user
            .filter(|user| !self.knobs.expired_sessions.contains(&user.account_name))
            .and_then(id3);
        self.set_active_user(signed_in.unwrap_or(0));
        if signed_in.is_none() {
            self.lands_on_form();
        }
    }
}
