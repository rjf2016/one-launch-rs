use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ol_formats::battlenet::config::{parse_config, plan_saved_account_names, read_client_settings};
use ol_formats::battlenet::products::PRODUCTS;
use ol_formats::js_json::{JsString, JsValue, stringify};
use ol_platform_api::{Hive, RegistryData, RegistryReader};
use ol_platform_fake::{CLOSE_ARGUMENT, FakeClock, FakeProcesses, FakeRegistry, RegistryValue};

// Where a Battle.net client keeps what the app reads. The simulator has its own copy of these names: it stands in for the client, not for the app's idea of the client.
const EXE: &str = "Battle.net.exe";
const LAUNCHER_EXE: &str = "Battle.net Launcher.exe";
const UNINSTALL: &str = r"HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";
const URI_COMMAND: &str = r"HKLM\SOFTWARE\Classes\battlenet\shell\open\command\";
const UNIFIED_AUTH_KEY: &str = r"Software\Blizzard Entertainment\Battle.net\UnifiedAuth";
const TOKEN_NAME: &str = "AA11BB22";
const UNINSTALLER: &str =
    r#""C:\ProgramData\Battle.net\Agent\Blizzard Uninstaller.exe" --lang=enUS"#;

const LOGGING_STARTED_LINE: &str =
    "I 2026-09-13 03:20:48.092586 [Main] {Main} Logging started for Battle.net build 2.52.11.17778";
const MAIN_WINDOW_LINE: &str = "I 2026-09-13 03:20:55.118026 [Frontend] {Main} Attempting to show main window. isMinimizedOnStartup=false";
// What the client logs for a token of another login than the first in SavedAccountNames.
const TOKEN_DISCARDED_LINE: &str = "I 2026-09-13 03:20:54.000000 [BNLogin] {Main} BattleNetLogin::DeleteToken(): Deleting registry token because !m_lastEmailUsed.empty()";
const TOKEN_REFUSED_LINE: &str =
    "E 2026-09-13 03:20:54.000000 [BNLogin] {Main} Login failed. error=ERROR_AUTH_TOKEN_EXPIRED";
// The products the simulated client knows: the two the registry of a scenario installs.
const KNOWN_PRODUCTS: [&str; 2] = ["prometheus", "hs_beta"];

#[derive(Clone, Default)]
pub struct BattlenetKnobs {
    // Tokens the servers refuse at the client's start: a token's hex, or the account id it signs in as.
    pub rejected: Vec<String>,
    // The client writes a new token after every sign-in.
    pub rotate_on_sign_in: bool,
    pub react_ms: u64,
    // Once on its login page, the simulated user signs in by hand after this long.
    pub sign_in_when_asked_ms: Option<u64>,
    // The account id a sign-in by hand lands on.
    pub sign_in_as: Option<String>,
    // An account id the client is signed in as when the run begins.
    pub running_as: Option<String>,
    // Codenames the client wants to update before they run: a launch command for one only shows its page.
    pub updates_pending: Vec<String>,
    // Codenames whose game runs already, outside the client: every start of the client logs it running, and a launch command for one only shows its page.
    pub games_running: Vec<String>,
}

// The scenario's stand-ins for %APPDATA%, %LOCALAPPDATA% and %ProgramData%, and the folder its games are installed under.
pub struct BattlenetFolders<'a> {
    pub app_data: &'a str,
    pub local_app_data: &'a str,
    pub program_data: &'a str,
    pub games: &'a str,
}

// What a scenario tells the scripted client beside its knobs.
#[derive(Default)]
pub struct BattlenetScript {
    // What "Blizzard's servers" know: the account each token signs in as.
    pub servers: HashMap<String, String>,
    // The login each account id signs in with: the client takes a token only for the login that is first in its list.
    pub logins: HashMap<String, String>,
    // The account whose run the client's last log is.
    pub on_disk: Option<String>,
    pub knobs: BattlenetKnobs,
}

struct State {
    servers: HashMap<String, String>,
    runs: u32,
    tokens: u32,
}

pub struct BattlenetSim {
    knobs: BattlenetKnobs,
    logins: HashMap<String, String>,
    config_path: PathBuf,
    logs_dir: PathBuf,
    registry: Arc<FakeRegistry>,
    processes: Arc<FakeProcesses>,
    clock: Arc<FakeClock>,
    state: Mutex<State>,
}

fn product_name(codename: &str) -> String {
    let name = PRODUCTS
        .iter()
        .find(|product| product.0 == codename)
        .map_or(codename, |product| product.3);
    name.split_whitespace().collect()
}

fn signed_in_line(uid: &str) -> String {
    format!(
        "I 2026-09-13 03:21:00.000000 [BNLogin] {{Main}} Logged into Battle.net successfully. |bnet=1:0:{uid}|game=2:100000000:0a1b2c3"
    )
}

impl BattlenetSim {
    // The registry values a Battle.net install with Overwatch and Hearthstone leaves behind, and one entry of another vendor's beside them.
    pub fn seed_registry(registry: &FakeRegistry, install_dir: &str, games_dir: &str) {
        let text = |value: String| RegistryValue::Text(value);
        let product = |key: &str, name: &str, value: String| {
            registry.seed(&format!("{UNINSTALL}\\{key}\\{name}"), text(value));
        };
        product("Battle.net", "InstallLocation", install_dir.to_owned());
        product("Battle.net", "DisplayName", "Battle.net".to_owned());
        product(
            "Battle.net",
            "UninstallString",
            format!(r#"{UNINSTALLER} --uid=battle.net --displayname="Battle.net""#),
        );
        registry.seed(
            URI_COMMAND,
            text(format!(r#""{install_dir}\{EXE}" --uri="%1""#)),
        );
        for (name, codename) in [("Overwatch", "prometheus"), ("Hearthstone", "hs_beta")] {
            product(name, "DisplayName", name.to_owned());
            product(name, "InstallLocation", format!("{games_dir}\\{name}"));
            product(
                name,
                "UninstallString",
                format!(r#"{UNINSTALLER} --uid={codename} --displayname="{name}""#),
            );
        }
        product(
            "{AEB35C6C-B6D4-4AA0-8452-DE699737B5F6}",
            "UninstallString",
            "MsiExec.exe /X{AEB35C6C-B6D4-4AA0-8452-DE699737B5F6}".to_owned(),
        );
    }

    // A product.db laid out as the real one: each text behind a tag and a length byte.
    fn product_db() -> Vec<u8> {
        let records = [
            "agent",
            "agent",
            "C:/ProgramData/Battle.net/Agent",
            "prometheus",
            "pro",
            "C:/Users/someone/Games/Overwatch",
            "enUS",
        ];
        let mut bytes = Vec::new();
        for record in records {
            bytes.extend([0x0a, u8::try_from(record.len()).unwrap_or(0)]);
            bytes.extend(record.as_bytes());
        }
        bytes
    }

    pub fn attach(
        folders: &BattlenetFolders<'_>,
        registry: Arc<FakeRegistry>,
        processes: &Arc<FakeProcesses>,
        clock: Arc<FakeClock>,
        script: BattlenetScript,
    ) -> Arc<Self> {
        let sim = Arc::new(Self {
            knobs: script.knobs,
            logins: script.logins,
            config_path: [folders.app_data, "Battle.net", "Battle.net.config"]
                .iter()
                .collect(),
            logs_dir: [folders.local_app_data, "Battle.net", "Logs"]
                .iter()
                .collect(),
            registry,
            processes: Arc::clone(processes),
            clock,
            state: Mutex::new(State {
                servers: script.servers,
                runs: 0,
                tokens: 0,
            }),
        });
        for codename in &sim.knobs.games_running {
            let image = format!("{}.exe", product_name(codename));
            let program: PathBuf = [folders.games, &product_name(codename), "_retail_", &image]
                .iter()
                .collect();
            processes.add_to_table(&image, &program.to_string_lossy());
        }
        if let Some(uid) = &sim.knobs.running_as {
            processes.set_running(EXE, true);
            let mut lines = sim.opening_lines();
            lines.push(signed_in_line(uid));
            sim.write_log(&lines);
        }
        let db: PathBuf = [folders.program_data, "Battle.net", "Agent", "product.db"]
            .iter()
            .collect();
        if !db.exists() {
            Self::write(&db, &Self::product_db());
        }
        // The simulator owns every log: one laid down by a scenario would sort after its own and hide them. This is the run of the account the client's token belongs to.
        if let Some(uid) = &script.on_disk {
            let mut lines = vec![LOGGING_STARTED_LINE.to_owned()];
            lines.extend(sim.install_state_lines());
            lines.push(signed_in_line(uid));
            sim.write_log(&lines);
        }
        let hears = Arc::clone(&sim);
        processes.on_start(move |exe, args| hears.on_start(exe, args));
        sim
    }

    fn write(file: &Path, bytes: &[u8]) {
        let written = file
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| fs::write(file, bytes));
        written.expect("the simulated client could not write its files");
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn on_start(self: &Arc<Self>, exe: &str, args: &[String]) {
        let image = exe.rsplit(['\\', '/']).next().unwrap_or(exe);
        let first = args.first().map(String::as_str);
        let client = image.eq_ignore_ascii_case(EXE) || image.eq_ignore_ascii_case(LAUNCHER_EXE);
        if !client || first == Some(CLOSE_ARGUMENT) {
            return;
        }
        // A second Battle.net.exe hands its command to the running client, which logs what it does with it.
        if let Some(command) = first.filter(|first| first.starts_with("--exec=")) {
            self.append_log(&self.launch_lines(&command.replace("--exec=launch ", "")));
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

    fn fresh_token(&self) -> String {
        let mut state = self.state();
        state.tokens += 1;
        format!("01000000d08c9ddf{:04x}{}", state.tokens, "ab".repeat(24))
    }

    fn token_path(name: &str) -> String {
        format!("HKCU\\{UNIFIED_AUTH_KEY}\\{name}")
    }

    fn read_config(&self) -> Option<String> {
        fs::read(&self.config_path)
            .ok()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    }

    fn first_saved(&self) -> Option<String> {
        let (doc, _) = parse_config(&self.read_config()?).ok()?;
        read_client_settings(&doc)
            .saved_account_names
            .into_iter()
            .next()
    }

    // What the client does at every sign-in: the login that signed in goes to the front of SavedAccountNames. A file it cannot write back as it was is written in its own layout.
    fn client_signed_in(&self, uid: &str) {
        let (Some(text), Some(login)) = (self.read_config(), self.logins.get(uid)) else {
            return;
        };
        let rewritten = match plan_saved_account_names(&text, login) {
            Ok(write) => Some(write.text),
            Err(_) => parse_config(&text).ok().and_then(|(mut doc, _)| {
                let client = doc.get_mut("Client")?.as_object_mut()?;
                let saved = client
                    .get("SavedAccountNames")?
                    .as_string()?
                    .to_string_lossy();
                let mut names = vec![login.as_str()];
                names.extend(saved.split(',').filter(|name| name != login));
                client.insert(
                    JsString::from("SavedAccountNames"),
                    JsValue::String(JsString::from(names.join(",").as_str())),
                );
                Some(stringify(&JsValue::Object(doc), 4))
            }),
        };
        if let Some(rewritten) = rewritten {
            Self::write(&self.config_path, rewritten.as_bytes());
        }
    }

    fn current_tokens(&self) -> Vec<(String, String)> {
        let values = self.registry.list_values(Hive::Hkcu, UNIFIED_AUTH_KEY);
        values
            .unwrap_or_default()
            .into_iter()
            .filter_map(|value| match value.data {
                RegistryData::Binary(hex) => Some((value.name, hex.to_lowercase())),
                _ => None,
            })
            .collect()
    }

    // Starts a new log, as the client does at every start. Its name holds the time the run started, so the newest sorts last.
    fn write_log(&self, lines: &[String]) {
        let run = {
            let mut state = self.state();
            state.runs += 1;
            state.runs
        };
        let file = self
            .logs_dir
            .join(format!("battle.net-20260913T0320{run:02}.000000.log"));
        Self::write(&file, format!("{}\r\n", lines.join("\r\n")).as_bytes());
    }

    fn append_log(&self, lines: &[String]) {
        let newest = fs::read_dir(&self.logs_dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .filter(|name| name.starts_with("battle.net-"))
                    .max()
            })
            .ok()
            .flatten()
            .expect("the simulated client has a log to add to");
        let written = fs::OpenOptions::new()
            .append(true)
            .open(self.logs_dir.join(newest))
            .and_then(|mut log| log.write_all(format!("{}\r\n", lines.join("\r\n")).as_bytes()));
        written.expect("the simulated client could not write its log");
    }

    // What the client logs for each product it knows, right after its log opens: up to date and playable, or waiting for an update.
    fn install_state_lines(&self) -> Vec<String> {
        KNOWN_PRODUCTS
            .iter()
            .map(|codename| {
                if self.knobs.updates_pending.iter().any(|pending| pending == codename) {
                    format!(
                        "D 2026-09-13 03:20:49.000000 [ProductState] {{Main}} InstallState ({codename}): currentOperation=Update currentOperationStatus=New playable=0 regionalVersions=(us: {{playable:false config_key:placeholder}}) latestVersion=153619 currentStage=Setup"
                    )
                } else {
                    format!(
                        "D 2026-09-13 03:20:49.000000 [ProductState] {{Main}} InstallState ({codename}): currentOperation=UpToDate currentOperationStatus=Paused installed=1 hasInstallModificationOptions=1 installPath=C:/Users/someone/Games/{} sharedContainerSubpath=_retail_ playable=1 canCreateShortcut=1 selectedRegion=us",
                        product_name(codename)
                    )
                }
            })
            .collect()
    }

    // What a starting client logs for a game it finds running, a few seconds after its install states.
    fn running_game_lines(&self) -> Vec<String> {
        self.knobs
            .games_running
            .iter()
            .flat_map(|codename| {
                [
                    format!(
                        "D 2026-09-13 03:20:56.000000 [InstallInfo] {{Main}} Operation status changed: opType=Execute oldStatus=Off newStatus=On agentUid={codename}"
                    ),
                    format!(
                        "D 2026-09-13 03:20:56.000100 [ProductState] {{Main}} InstallState ({codename}): gameRunning=1 isGameProcessRunning=1"
                    ),
                ]
            })
            .collect()
    }

    fn opening_lines(&self) -> Vec<String> {
        let mut lines = vec![LOGGING_STARTED_LINE.to_owned()];
        lines.extend(self.install_state_lines());
        lines.extend(self.running_game_lines());
        lines
    }

    // What the client logs for --exec=launch <program code>: the game's start for a playable product it knows, and only the choice of its page for one that waits for an update, that runs already or that it does not know.
    fn launch_lines(&self, program_code: &str) -> Vec<String> {
        let mut lines = vec![
            "D 2026-09-13 04:21:00.000000 [CommandQueue] {Main} Received IPC Message: IPC_COMMAND"
                .to_owned(),
            format!(
                "D 2026-09-13 04:21:00.000100 [GameController] {{Main}} Selecting game family by id: {program_code}"
            ),
        ];
        let codename = PRODUCTS
            .iter()
            .find(|product| product.1.eq_ignore_ascii_case(program_code))
            .map(|product| product.0);
        let held = |list: &[String], codename: &str| list.iter().any(|held| held == codename);
        let Some(codename) = codename.filter(|codename| {
            KNOWN_PRODUCTS.contains(codename)
                && !held(&self.knobs.updates_pending, codename)
                && !held(&self.knobs.games_running, codename)
        }) else {
            return lines;
        };
        let exe = product_name(codename);
        lines.extend([
            format!(
                "I 2026-09-13 04:21:00.000200 [GameLaunchController] {{Main}} LaunchBinary: uid={codename} selectedRegion=US binaryType=game"
            ),
            format!(
                "D 2026-09-13 04:21:00.000300 [InstallInfo] {{Main}} Operation status changed: opType=Execute oldStatus=Off newStatus=Pending agentUid={codename}"
            ),
            format!(
                "D 2026-09-13 04:21:00.000400 [ProductState] {{Main}} InstallState ({codename}): GameLaunching=1"
            ),
            format!(
                "I 2026-09-13 04:21:00.600000 [InstallManager] {{Main}} Launched C:/Users/someone/Games/{exe}/_retail_/{exe}.exe with args: -uid {codename}, pid: 15412"
            ),
            format!(
                "D 2026-09-13 04:21:00.650000 [InstallInfo] {{Main}} Operation status changed: opType=Execute oldStatus=Pending newStatus=On agentUid={codename}"
            ),
            format!(
                "D 2026-09-13 04:21:00.650100 [InstallManager] {{Main}} Game is running: {codename}"
            ),
            format!(
                "D 2026-09-13 04:21:00.650200 [ProductState] {{Main}} InstallState ({codename}): gameRunning=1 isGameProcessRunning=1 GameLaunching=0"
            ),
        ]);
        lines
    }

    // Stands in for the user signing in on the client's login page. The client then writes a new token for that account.
    pub fn sign_in_by_hand(&self, account: Option<&str>) {
        let uid = account
            .or(self.knobs.sign_in_as.as_deref())
            .expect("a sign-in by hand needs an account id: pass one or set signInAs");
        let fresh = self.fresh_token();
        self.state().servers.insert(fresh.clone(), uid.to_owned());
        self.registry
            .seed(&Self::token_path(TOKEN_NAME), RegistryValue::Binary(fresh));
        self.client_signed_in(uid);
        self.append_log(&[
            format!(
                "I 2026-09-13 03:21:00.000000 [GSAccountProvider] {{Main}} Login triggered. entityId=1:0:{uid}"
            ),
            signed_in_line(uid),
        ]);
    }

    fn react(self: &Arc<Self>) {
        self.processes.set_running(EXE, true);
        let mut lines = self.opening_lines();
        lines.push(MAIN_WINDOW_LINE.to_owned());
        let mut signed_in = false;
        if let Some((name, hex)) = self.current_tokens().into_iter().next() {
            let known = self.state().servers.get(&hex).cloned();
            let first = self.first_saved();
            let for_first_login =
                |uid: &String| first.is_some() && first.as_ref() == self.logins.get(uid);
            let refused = |uid: &String| {
                let rejected = &self.knobs.rejected;
                rejected.iter().any(|token| token.to_lowercase() == hex) || rejected.contains(uid)
            };
            match known.filter(for_first_login) {
                None => {
                    lines.push(TOKEN_DISCARDED_LINE.to_owned());
                    self.registry.remove(&Self::token_path(&name));
                }
                Some(uid) if refused(&uid) => {
                    lines.push(TOKEN_REFUSED_LINE.to_owned());
                    self.registry.remove(&Self::token_path(&name));
                }
                Some(uid) => {
                    lines.push(signed_in_line(&uid));
                    self.client_signed_in(&uid);
                    signed_in = true;
                    if self.knobs.rotate_on_sign_in {
                        let fresh = self.fresh_token();
                        self.state().servers.insert(fresh.clone(), uid);
                        self.registry
                            .seed(&Self::token_path(&name), RegistryValue::Binary(fresh));
                    }
                }
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
