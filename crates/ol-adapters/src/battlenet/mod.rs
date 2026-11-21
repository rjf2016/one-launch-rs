// The Battle.net adapter: everything it reads, and the three things it does to a process (ask the client's window to close, end the client, hand it a launch command). The client signs in from one token, a value under HKCU\Software\Blizzard Entertainment\Battle.net\UnifiedAuth that it writes again at every sign-in, and takes it only for the login that is first in Battle.net.config's SavedAccountNames. This module writes neither; the switch pipeline does that with what it plans.

mod health;
mod plan;

use std::sync::LazyLock;
use std::time::Duration;

use ol_formats::battlenet::client_log::{
    LogEvent, SignInOutcome, ended_signed_out, is_client_log_file, is_forwarder_log, last_sign_in,
    parse_log_events,
};
use ol_formats::battlenet::config::{
    ClientSettings, UserPrefs, game_last_played, parse_config, read_client_settings,
    read_user_prefs, round_trips,
};
use ol_formats::battlenet::product_state::{
    game_running, game_started, product_needs_update, product_states,
};
use ol_formats::battlenet::products::{
    LaunchCommand, LaunchForm, ProductCodes, ProductDbEntry, known_product, launch_command,
    parse_product_db, product_codes,
};
use ol_formats::battlenet::session::{TokenValue, UNIFIED_AUTH_KEY, fingerprint_of, is_token_hex};
use ol_formats::js::{self, WHITESPACE_CLASS};
use ol_formats::js_json::JsObject;
use ol_formats::winpath::{join, normalize_launcher_path};
use ol_model::{
    InstallSource, InstalledGame, LastSignIn, LauncherId, LauncherInstall, LauncherSessionState,
    RememberedAccount, RunningGame, RunningGameSource,
};
use ol_platform_api::{CancelToken, Hive, Platform, ProcessArg, RegistryData, RegistryNamedValue};
use ol_runtime::{Aborted, TimingOverrides, wait_until};
use regex::Regex;
use time::OffsetDateTime;

pub use crate::epic::{UNOWNED_SESSION_PIN, id_tail};
use crate::files::{
    MAX_LAUNCHER_FILE_BYTES, read_launcher_bytes, read_launcher_text, read_log_since,
};
use crate::running::game_with_running_process;
use crate::{AccountRef, AdapterError, Directory};

pub use plan::{PlanContext, STEP, close_message};

pub const EXE: &str = "Battle.net.exe";
// What the shortcuts start: it applies a pending update and then starts the client.
pub const LAUNCHER_EXE: &str = "Battle.net Launcher.exe";
pub const UNINSTALL_ROOT: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";
pub const UNINSTALL_KEY: &str =
    r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Battle.net";
pub const URI_COMMAND_KEY: &str = r"SOFTWARE\Classes\battlenet\shell\open\command";
pub const DEFAULT_DIR: &str = r"C:\Program Files (x86)\Battle.net";
pub const UNIFIED_AUTH_PATH: &str = r"HKCU\Software\Blizzard Entertainment\Battle.net\UnifiedAuth";
pub const CONFIG_FILE: &str = "Battle.net.config";

// What a search of the Uninstall key for Blizzard's products looks for.
const UNINSTALLER_TEXT: &str = "Blizzard Uninstaller";
const MAX_LOG_TAIL_BYTES: u64 = 8 * 1024 * 1024;
// A log's head, which says whether the file is a run of the client.
const LOG_HEAD_BYTES: u64 = 4096;

static UNINSTALLER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i-u:Blizzard Uninstaller\.exe)").expect("a fixed pattern"));
static UID_ARGUMENT: LazyLock<Regex> = LazyLock::new(|| {
    let word = WHITESPACE_CLASS.replacen('[', "[^", 1);
    Regex::new(&format!(r#"--uid=(?:"([^"]+)"|({word}+))"#)).expect("a fixed pattern")
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timing {
    pub poll: Duration,
    // The waits that read the log or a registry value look more often than those that read the process list.
    pub log_poll: Duration,
    // The client's own shutdown takes a second or two; with "close to tray" on it never exits.
    pub shutdown: Duration,
    pub kill_grace: Duration,
    // After it exits the client can come straight back for an update. How long to watch for that.
    pub restart_watch: Duration,
    // The same watch when the log shows the client shutting itself down after the window close.
    pub clean_exit_watch: Duration,
    // How long a client that started itself again gets to show its window before it is closed again.
    pub restart_settle: Duration,
    pub startup: Duration,
    pub sign_in: Duration,
    pub manual_sign_in: Duration,
    // The client writes the token at the sign-in itself; this is slack for the registry.
    pub capture: Duration,
    // The client takes a launch command only about a second after the sign-in.
    pub ready_grace: Duration,
    // How long the log is watched for the game's start after a launch command.
    pub launch_confirm: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            poll: Duration::from_millis(500),
            log_poll: Duration::from_millis(250),
            shutdown: Duration::from_secs(30),
            kill_grace: Duration::from_secs(10),
            restart_watch: Duration::from_secs(5),
            clean_exit_watch: Duration::from_millis(1500),
            restart_settle: Duration::from_secs(45),
            startup: Duration::from_secs(90),
            sign_in: Duration::from_secs(60),
            manual_sign_in: Duration::from_secs(600),
            capture: Duration::from_secs(10),
            ready_grace: Duration::from_secs(5),
            launch_confirm: Duration::from_secs(15),
        }
    }
}

impl Timing {
    pub fn with(overrides: &TimingOverrides) -> Self {
        let default = Self::default();
        let named = |name: &str, default: Duration| overrides.get(name).unwrap_or(default);
        Self {
            poll: named("pollMs", default.poll),
            log_poll: overrides.finer_poll("logPollMs", default.log_poll),
            shutdown: named("shutdownMs", default.shutdown),
            kill_grace: named("killGraceMs", default.kill_grace),
            restart_watch: named("restartWatchMs", default.restart_watch),
            clean_exit_watch: named("cleanExitWatchMs", default.clean_exit_watch),
            restart_settle: named("restartSettleMs", default.restart_settle),
            startup: named("startupMs", default.startup),
            sign_in: named("signInMs", default.sign_in),
            manual_sign_in: named("manualSignInMs", default.manual_sign_in),
            capture: named("captureMs", default.capture),
            ready_grace: named("readyGraceMs", default.ready_grace),
            launch_confirm: named("launchConfirmMs", default.launch_confirm),
        }
    }
}

// What the settings and the machine say, read once for the call in hand.
#[derive(Clone)]
pub struct BattlenetConfig {
    // The folder the user chose with Locate, tried before the registry.
    pub chosen_dir: Option<String>,
    pub default_dir: String,
    // %APPDATA%, %LOCALAPPDATA% and %ProgramData%.
    pub app_data: String,
    pub local_app_data: String,
    pub program_data: String,
    pub launch_form: LaunchForm,
    pub backups_to_keep: i64,
    pub backups_dir: String,
    pub timing: Timing,
}

pub struct BattlenetAdapter<'a> {
    pub(crate) platform: &'a Platform,
    pub(crate) config: &'a BattlenetConfig,
    pub(crate) directory: &'a dyn Directory,
}

// Battle.net.config as it is now. No Debug: it holds logins.
#[derive(Clone)]
pub struct ConfigState {
    pub path: String,
    // None when there is no file.
    pub text: Option<String>,
    pub doc: Option<JsObject>,
    pub client: Option<ClientSettings>,
    // None when the file is missing or is not JSON; false when it reads but writing it back would not give the same bytes.
    pub round_trips: Option<bool>,
    pub error: Option<String>,
}

// Where the client's log stood: the newest log, which a new run replaces, and its size.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct LogMark {
    pub file: Option<String>,
    pub size: u64,
}

// Whose the token is, and how that is known. No Debug: it holds the token.
#[derive(Clone)]
pub struct CurrentSession {
    // Every binary value under UnifiedAuth. The client keeps one.
    pub tokens: Vec<TokenValue>,
    // The one that counts: the one a saved sign-in matches, else the first.
    pub token: Option<TokenValue>,
    pub fingerprint: Option<String>,
    // While the client runs its own log decides. Closed, a saved sign-in with the token's fingerprint decides, then the last run's log: the client writes the token at the sign-in itself.
    pub owner: Option<AccountRef>,
    // The account the client last signed in, by its current log.
    pub logged_uid: Option<String>,
    pub last_sign_in: Option<SignInOutcome>,
    // The last run ended with a logout, so a token that is there is not that run's.
    pub signed_out: bool,
}

impl CurrentSession {
    pub const fn remembered(&self) -> bool {
        self.token.is_some()
    }
}

struct UninstallEntry {
    codename: String,
    name: String,
    install_path: Option<String>,
}

fn is_codename(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
}

// A per-account file is named by the account id in hex, one to eight digits.
fn user_config_id(name: &str) -> Option<String> {
    let cut = name.len().checked_sub(".config".len())?;
    let (id, extension) = (name.get(..cut)?, name.get(cut..)?);
    let named = extension.eq_ignore_ascii_case(".config")
        && (1..=8).contains(&id.len())
        && id.bytes().all(|byte| byte.is_ascii_hexdigit());
    named.then(|| id.to_lowercase())
}

fn text_value<'v>(values: &'v [RegistryNamedValue], name: &str) -> Option<&'v str> {
    values
        .iter()
        .find(|value| value.name.eq_ignore_ascii_case(name))
        .and_then(|value| match &value.data {
            RegistryData::Text(text) => Some(text.as_str()),
            _ => None,
        })
}

// A product the Agent installed: an Uninstall entry whose uninstaller is Blizzard's, named by its --uid. The client's own entry is not one.
fn uninstall_entry(values: &[RegistryNamedValue]) -> Option<UninstallEntry> {
    let uninstall =
        text_value(values, "UninstallString").filter(|text| UNINSTALLER.is_match(text))?;
    let found = UID_ARGUMENT.captures(uninstall)?;
    let codename = js::trim(found.get(1).or_else(|| found.get(2))?.as_str());
    if codename.is_empty() || codename == "battle.net" {
        return None;
    }
    let display_name = text_value(values, "DisplayName")
        .map(js::trim)
        .filter(|name| !name.is_empty());
    Some(UninstallEntry {
        name: match (display_name, known_product(codename)) {
            (Some(name), _) => name.to_owned(),
            (None, Some(known)) => known.name,
            (None, None) => codename.to_owned(),
        },
        install_path: text_value(values, "InstallLocation")
            .filter(|location| !js::trim(location).is_empty())
            .map(normalize_launcher_path),
        codename: codename.to_owned(),
    })
}

fn same_login(a: &str, b: &str) -> bool {
    js::trim(a).to_lowercase() == js::trim(b).to_lowercase()
}

impl<'a> BattlenetAdapter<'a> {
    pub const fn new(
        platform: &'a Platform,
        config: &'a BattlenetConfig,
        directory: &'a dyn Directory,
    ) -> Self {
        Self {
            platform,
            config,
            directory,
        }
    }

    pub fn directory(&self) -> &'a dyn Directory {
        self.directory
    }

    pub fn timing(&self) -> &Timing {
        &self.config.timing
    }

    pub fn config_dir(&self) -> String {
        join(&[&self.config.app_data, "Battle.net"])
    }

    pub fn config_path(&self) -> String {
        join(&[&self.config_dir(), CONFIG_FILE])
    }

    pub fn log_dir(&self) -> String {
        join(&[&self.config.local_app_data, "Battle.net", "Logs"])
    }

    pub fn product_db_path(&self) -> String {
        join(&[
            &self.config.program_data,
            "Battle.net",
            "Agent",
            "product.db",
        ])
    }

    pub fn chosen_install_dir(&self) -> Option<String> {
        self.config
            .chosen_dir
            .as_deref()
            .filter(|raw| !js::trim(raw).is_empty())
            .map(normalize_launcher_path)
    }

    fn install_at(&self, install_dir: String, source: InstallSource) -> Option<LauncherInstall> {
        let exe_path = join(&[&install_dir, EXE]);
        self.platform
            .files
            .exists(&exe_path)
            .then_some(LauncherInstall {
                launcher: LauncherId::Battlenet,
                install_dir,
                exe_path,
                source,
            })
    }

    // The first of these that holds Battle.net.exe: the folder chosen in Settings, the folder its uninstall entry names, the program its own link opens with, the usual place.
    pub fn detect_install(&self) -> Result<Option<LauncherInstall>, AdapterError> {
        let registry = &self.platform.registry;
        if let Some(chosen) = self
            .chosen_install_dir()
            .and_then(|chosen| self.install_at(chosen, InstallSource::User))
        {
            return Ok(Some(chosen));
        }
        let uninstall = registry.get_string(Hive::Hklm, UNINSTALL_KEY, "InstallLocation")?;
        if let Some(found) = uninstall
            .filter(|raw| !js::trim(raw).is_empty())
            .and_then(|raw| {
                self.install_at(normalize_launcher_path(&raw), InstallSource::RegistryHklm)
            })
        {
            return Ok(Some(found));
        }
        let command = registry.get_string(Hive::Hklm, URI_COMMAND_KEY, "")?;
        let quoted = command.as_deref().and_then(|command| {
            let rest = command.strip_prefix('"')?;
            let exe = &rest[..rest.find('"')?];
            (!exe.is_empty()).then_some(exe)
        });
        if let Some(found) = quoted.and_then(|exe| {
            let folder = join(&[&normalize_launcher_path(exe), ".."]);
            self.install_at(folder, InstallSource::RegistryHklm)
        }) {
            return Ok(Some(found));
        }
        Ok(self.install_at(self.config.default_dir.clone(), InstallSource::DefaultPath))
    }

    pub fn require_install(&self) -> Result<LauncherInstall, AdapterError> {
        self.detect_install()?
            .ok_or(AdapterError::NotInstalled(LauncherId::Battlenet))
    }

    // What a switch starts: the launcher beside the client when it is there, as the shortcuts do.
    pub fn start_exe(&self, install: &LauncherInstall) -> String {
        let launcher = join(&[&install.install_dir, LAUNCHER_EXE]);
        if self.platform.files.exists(&launcher) {
            launcher
        } else {
            install.exe_path.clone()
        }
    }

    // The file's text, and whether its bytes are that text exactly: a byte that is not UTF-8 reads as U+FFFD, and writing the text back would change it.
    pub fn read_config_text(&self) -> Result<Option<(String, bool)>, AdapterError> {
        let files = self.platform.files.as_ref();
        let path = self.config_path();
        if !files.exists(&path) {
            return Ok(None);
        }
        let bytes = read_launcher_bytes(files, &path, MAX_LAUNCHER_FILE_BYTES).map_err(
            |error| match error {
                AdapterError::LauncherFile { .. } => error,
                other => AdapterError::LauncherFile {
                    path: path.clone(),
                    message: format!("Could not read {CONFIG_FILE} ({other})"),
                },
            },
        )?;
        let exact = std::str::from_utf8(&bytes).is_ok();
        Ok(Some((String::from_utf8_lossy(&bytes).into_owned(), exact)))
    }

    pub fn read_config(&self) -> Result<ConfigState, AdapterError> {
        let mut state = ConfigState {
            path: self.config_path(),
            text: None,
            doc: None,
            client: None,
            round_trips: None,
            error: None,
        };
        let Some((text, exact)) = self.read_config_text()? else {
            return Ok(state);
        };
        match parse_config(&text) {
            Ok((doc, _)) => {
                state.client = Some(read_client_settings(&doc));
                state.doc = Some(doc);
                state.round_trips = Some(exact && round_trips(&text));
            }
            Err(error) => state.error = Some(error.to_string()),
        }
        state.text = Some(text);
        Ok(state)
    }

    // The per-account file that says what a window close does: the current account's, else the one written last.
    pub fn read_user_prefs(&self, uid: Option<&str>) -> Option<(String, UserPrefs)> {
        let files = self.platform.files.as_ref();
        let dir = self.config_dir();
        if !files.exists(&dir) {
            return None;
        }
        let mut names = files.list(&dir).ok()?;
        names.sort_by_cached_key(|name| name.to_lowercase());
        let mut candidates: Vec<(String, OffsetDateTime, bool)> = names
            .iter()
            .filter_map(|name| {
                let id = user_config_id(name)?;
                let path = join(&[&dir, name]);
                let written = files.modified(&path).ok()?;
                let own = uid.is_some_and(|uid| uid.to_lowercase() == id);
                Some((path, written, own))
            })
            .collect();
        candidates.sort_by(|a, b| b.2.cmp(&a.2).then(b.1.cmp(&a.1)));
        let (path, _, _) = candidates.into_iter().next()?;
        let text = read_launcher_text(files, &path, MAX_LAUNCHER_FILE_BYTES).ok()?;
        Some((path, read_user_prefs(&text).ok()?))
    }

    // Two small reads: which keys under Uninstall hold Blizzard's uninstaller, then each product's key for its name and folder. A key that cannot be read is passed over.
    fn uninstall_entries(&self) -> Result<Vec<UninstallEntry>, AdapterError> {
        let registry = &self.platform.registry;
        let keys = registry.keys_holding(Hive::Hklm, UNINSTALL_ROOT, UNINSTALLER_TEXT)?;
        Ok(keys
            .iter()
            .filter(|key| !key.contains('\\'))
            .filter_map(|key| {
                let values = registry
                    .list_values(Hive::Hklm, &format!("{UNINSTALL_ROOT}\\{key}"))
                    .ok()?;
                uninstall_entry(&values)
            })
            .collect())
    }

    fn product_db(&self) -> Vec<ProductDbEntry> {
        let files = self.platform.files.as_ref();
        let path = self.product_db_path();
        if !files.exists(&path) {
            return Vec::new();
        }
        read_launcher_bytes(files, &path, MAX_LAUNCHER_FILE_BYTES)
            .map(|bytes| parse_product_db(&bytes))
            .unwrap_or_default()
    }

    fn codes_for(codename: &str, db: &[ProductDbEntry]) -> Option<ProductCodes> {
        let from_db = db.iter().find(|entry| entry.codename == codename);
        product_codes(codename, from_db.map(|entry| entry.tact_code.as_str()))
    }

    pub fn list_installed_games(&self) -> Result<Vec<InstalledGame>, AdapterError> {
        self.require_install()?;
        let entries = self.uninstall_entries()?;
        let config = self.read_config()?;
        // The client's own word, from its newest run's log. Without a log every product reads as up to date.
        let states = product_states(&self.current_run_events());
        let files = self.platform.files.as_ref();
        let mut games: Vec<InstalledGame> = entries
            .into_iter()
            .filter_map(|entry| {
                let install_path = entry.install_path.filter(|path| files.exists(path))?;
                let state = states
                    .iter()
                    .find(|(codename, _)| *codename == entry.codename);
                let played = config
                    .doc
                    .as_ref()
                    .and_then(|doc| game_last_played(doc, &entry.codename));
                Some(InstalledGame {
                    launcher: LauncherId::Battlenet,
                    library_path: Some(join(&[&install_path, ".."])),
                    size_on_disk_bytes: None,
                    last_played_at: played
                        .and_then(|seconds| OffsetDateTime::from_unix_timestamp(seconds).ok())
                        .map(ol_store::iso_timestamp),
                    needs_update: product_needs_update(state.map(|(_, state)| state)),
                    launcher_game_id: entry.codename,
                    name: entry.name,
                    install_path,
                })
            })
            .collect();
        games.sort_by_cached_key(|game| game.name.to_lowercase());
        Ok(games)
    }

    // The current run's log: the newest battle.net-*.log by name that is a run of the client and not a forwarder's.
    fn newest_client_log(&self) -> Option<String> {
        let files = self.platform.files.as_ref();
        let dir = self.log_dir();
        if !files.exists(&dir) {
            return None;
        }
        let mut names = files.list(&dir).ok()?;
        names.retain(|name| is_client_log_file(name));
        names.sort_by_cached_key(|name| std::cmp::Reverse(name.to_lowercase()));
        names.iter().map(|name| join(&[&dir, name])).find(|path| {
            files
                .read_range(path, 0, LOG_HEAD_BYTES)
                .is_ok_and(|head| !is_forwarder_log(&String::from_utf8_lossy(&head)))
        })
    }

    pub fn log_mark(&self) -> LogMark {
        let file = self.newest_client_log();
        let size = file
            .as_deref()
            .and_then(|file| self.platform.files.size(file).ok());
        match size {
            Some(size) => LogMark { file, size },
            None => LogMark::default(),
        }
    }

    // What the client has logged since `mark`, or all of its log when it has started a new run since.
    pub fn events_since(&self, mark: &LogMark) -> Vec<LogEvent> {
        let Some(file) = self.newest_client_log() else {
            return Vec::new();
        };
        let same_run = mark
            .file
            .as_deref()
            .is_some_and(|marked| marked.to_lowercase() == file.to_lowercase());
        let from = if same_run { mark.size } else { 0 };
        let files = self.platform.files.as_ref();
        parse_log_events(&read_log_since(files, &file, from, MAX_LOG_TAIL_BYTES))
    }

    pub fn current_run_events(&self) -> Vec<LogEvent> {
        self.events_since(&LogMark::default())
    }

    // The binary values under UnifiedAuth. A value of another kind is not a token.
    pub fn read_tokens(&self) -> Result<Vec<TokenValue>, AdapterError> {
        let values = self
            .platform
            .registry
            .list_values(Hive::Hkcu, UNIFIED_AUTH_KEY)?;
        Ok(values
            .into_iter()
            .filter_map(|value| match value.data {
                RegistryData::Binary(hex) if is_token_hex(&hex) => Some(TokenValue {
                    name: value.name,
                    hex: hex.to_lowercase(),
                }),
                _ => None,
            })
            .collect())
    }

    // What the registry and the client's log say now, and whose the session is. Writes nothing.
    pub fn current_session(&self, running: bool) -> Result<CurrentSession, AdapterError> {
        let tokens = self.read_tokens()?;
        let events = self.current_run_events();
        let last_sign_in = last_sign_in(&events);
        let signed_out = ended_signed_out(&events);
        let logged_uid = match &last_sign_in {
            Some(SignInOutcome::SignedIn { account_id, .. }) if !account_id.is_empty() => {
                Some(account_id.clone())
            }
            _ => None,
        };
        let by_log = match &logged_uid {
            Some(uid) => self.directory.account_by_launcher_uid(uid)?,
            None => None,
        };

        let mut token = None;
        let mut stored_owner = None;
        for candidate in &tokens {
            let fingerprint = fingerprint_of(&candidate.hex);
            if let Some(stored) = self.directory.session_by_fingerprint(&fingerprint)? {
                stored_owner = self.directory.account_by_id(&stored.account_id)?;
                token = Some(candidate.clone());
                break;
            }
        }
        let token = token.or_else(|| tokens.first().cloned());

        let owner = if running {
            by_log
        } else if token.is_some() {
            stored_owner.or(by_log.filter(|_| !signed_out))
        } else {
            None
        };
        Ok(CurrentSession {
            fingerprint: token.as_ref().map(|token| fingerprint_of(&token.hex)),
            tokens,
            token,
            owner,
            logged_uid,
            last_sign_in,
            signed_out,
        })
    }

    pub fn session_state(&self) -> Result<LauncherSessionState, AdapterError> {
        self.require_install()?;
        let running = self.is_running()?;
        let current = self.current_session(running)?;
        let owner_uid = current
            .owner
            .as_ref()
            .and_then(|owner| owner.launcher_uid.clone());
        Ok(LauncherSessionState {
            launcher: LauncherId::Battlenet,
            remembered: current.remembered(),
            fingerprint: current.fingerprint.clone(),
            launcher_uid: if running {
                current.logged_uid.clone()
            } else if current.remembered() {
                owner_uid.or_else(|| current.logged_uid.clone().filter(|_| !current.signed_out))
            } else {
                None
            },
            linked_account_id: current.owner.as_ref().and_then(|owner| owner.id.clone()),
            launcher_running: running,
            last_sign_in: current.last_sign_in.as_ref().map(|last| match last {
                SignInOutcome::SignedIn { .. } => LastSignIn {
                    ok: true,
                    reason: None,
                },
                SignInOutcome::Failed { reason } => LastSignIn {
                    ok: false,
                    reason: Some(reason.clone()),
                },
            }),
        })
    }

    // The logins of SavedAccountNames. The client keeps no ids there, and one token for all of them, so only the login whose session it holds now is marked.
    pub fn list_remembered_accounts(&self) -> Result<Vec<RememberedAccount>, AdapterError> {
        self.require_install()?;
        let names = self
            .read_config()?
            .client
            .map(|client| client.saved_account_names)
            .unwrap_or_default();
        if names.is_empty() {
            return Ok(Vec::new());
        }
        let current = self.current_session(self.is_running()?)?;
        let owner_login = current.owner.map(|owner| owner.login);
        Ok(names
            .into_iter()
            .map(|login| RememberedAccount {
                launcher: LauncherId::Battlenet,
                launcher_uid: String::new(),
                persona_name: String::new(),
                remember_password: true,
                most_recent: owner_login
                    .as_deref()
                    .is_some_and(|owner| same_login(owner, &login)),
                last_login_at: None,
                session_may_be_expired: false,
                linked_account_id: None,
                login,
            })
            .collect())
    }

    pub fn is_running(&self) -> Result<bool, AdapterError> {
        Ok(self.platform.processes.is_running(EXE)?)
    }

    // A process that runs out of a game's folder counts.
    pub fn running_game(&self) -> Result<Option<RunningGame>, AdapterError> {
        if self.detect_install()?.is_none() {
            return Ok(None);
        }
        let Ok(processes) = self.platform.processes.list_processes() else {
            return Ok(None);
        };
        if processes.is_empty() {
            return Ok(None);
        }
        let games = self.list_installed_games()?;
        let files = self.platform.files.as_ref();
        Ok(
            game_with_running_process(files, &processes, &games).map(|game| RunningGame {
                launcher: LauncherId::Battlenet,
                launcher_game_id: game.launcher_game_id.clone(),
                name: game.name.clone(),
                source: RunningGameSource::Process,
            }),
        )
    }

    // Asks the client's window to close and waits for every Battle.net.exe to be gone, because the client writes its config files at the very end of its shutdown; then ends it, and not what it started: a game is the client's child and keeps running. A client that comes back by itself is given time to finish starting and closed the same way again.
    pub fn shutdown(
        &self,
        timeout: Option<Duration>,
        cancel: &CancelToken,
    ) -> Result<(), AdapterError> {
        self.require_install()?;
        if !self.is_running()? {
            return Ok(());
        }
        let timing = self.timing();
        let clock = self.platform.clock.as_ref();
        let processes = &self.platform.processes;
        let gone = || Ok::<_, AdapterError>(!self.is_running()?);
        let back = || self.is_running();
        for attempt in 1.. {
            let mark = self.log_mark();
            processes.close(EXE)?;
            let close_timeout = timeout.unwrap_or(timing.shutdown);
            if !wait_until(clock, cancel, close_timeout, timing.poll, gone)? {
                processes.kill(EXE, false)?;
                if !wait_until(clock, cancel, timing.kill_grace, timing.poll, gone)? {
                    return Err(AdapterError::Failed(format!(
                        "{EXE} is still running after a window close and a forced close"
                    )));
                }
            }
            let watch = if self.events_since(&mark).contains(&LogEvent::Exit) {
                timing.clean_exit_watch
            } else {
                timing.restart_watch
            };
            if !wait_until(clock, cancel, watch, timing.poll, back)? {
                return Ok(());
            }
            if attempt >= 3 {
                break;
            }
            let started_again = || {
                Ok::<_, AdapterError>(
                    self.log_mark().file != mark.file
                        && self.current_run_events().contains(&LogEvent::MainWindow),
                )
            };
            wait_until(
                clock,
                cancel,
                timing.restart_settle,
                timing.poll,
                started_again,
            )?;
        }
        Err(AdapterError::Failed(format!(
            "{EXE} keeps restarting itself after being closed"
        )))
    }

    // Refuses a game the client's own log says cannot start, sends the launch command, then watches the log for the game's start: a command the client does not act on leaves nothing else to tell by. Returns what was sent and what came of it.
    pub fn launch_game(
        &self,
        launcher_game_id: &str,
        name: &str,
        cancel: &CancelToken,
    ) -> Result<String, AdapterError> {
        if !is_codename(launcher_game_id) {
            return Err(AdapterError::Failed(format!(
                "Not a Battle.net product codename: {launcher_game_id}"
            )));
        }
        let install = self.require_install()?;
        let codes = Self::codes_for(launcher_game_id, &self.product_db()).ok_or_else(|| {
            AdapterError::Failed(format!(
                "No launch code is known for {name} ({launcher_game_id}); it is not in One Launch's product table or in product.db."
            ))
        })?;
        let events = self.current_run_events();
        let states = product_states(&events);
        let state = states
            .iter()
            .find(|(codename, _)| codename == launcher_game_id)
            .map(|(_, state)| state);
        if product_needs_update(state) {
            return Err(AdapterError::GameNotPlayable(format!(
                "{name} needs an update before it can start. Update it in the Battle.net window, then press Play again."
            )));
        }
        if let Some(state) = state.filter(|state| state.playable == Some(false)) {
            let status = state
                .status
                .as_deref()
                .filter(|status| !status.is_empty())
                .map(|status| format!(", {}", status.to_lowercase()))
                .unwrap_or_default();
            return Err(AdapterError::GameNotPlayable(format!(
                "{name} is not playable right now ({}{status}). Finish that in the Battle.net window, then press Play again.",
                state.operation.as_deref().unwrap_or("unknown state")
            )));
        }
        // The client drops a launch command for a game it sees running, without a word.
        if game_running(&events, launcher_game_id) {
            return Err(AdapterError::GameNotPlayable(format!(
                "{name} is already running. Close it, then press Play again."
            )));
        }

        let mark = self.log_mark();
        let sent = launch_command(&codes, self.config.launch_form);
        let command = match &sent {
            LaunchCommand::Uri(url) => {
                self.platform.shell.open_url(url)?;
                url.clone()
            }
            LaunchCommand::Exec(argument) => {
                self.platform.processes.start(
                    &install.exe_path,
                    &[ProcessArg::Plain(argument)],
                    &install.install_dir,
                )?;
                format!("\"{}\" {argument}", install.exe_path)
            }
        };
        let timing = self.timing();
        let started = wait_until(
            self.platform.clock.as_ref(),
            cancel,
            timing.launch_confirm,
            timing.log_poll,
            || {
                let since = self.events_since(&mark);
                Ok::<_, Aborted>(game_started(&since, launcher_game_id, &codes.program_code))
            },
        );
        match (started, sent) {
            (Err(Aborted), _) => Ok(format!("{command} (stopped watching for the game start)")),
            (Ok(true), _) => Ok(format!("{command} (Battle.net started the game)")),
            (Ok(false), LaunchCommand::Uri(_)) => Ok(format!(
                "{command} (this link only opens the game page: press Play there, or switch the Battle.net launch setting to the launch command)"
            )),
            (Ok(false), LaunchCommand::Exec(_)) => Err(AdapterError::Failed(format!(
                "{name} did not start within {} s of the launch command. Look at the Battle.net window: the game may need an update or a repair, or the client may be busy updating itself.",
                crate::steam::seconds(timing.launch_confirm)
            ))),
        }
    }

    // Whether the client, as it is now, remembers a session that no One Launch account owns. Only means something while the client is closed.
    pub fn holds_unowned_session(&self) -> Result<bool, AdapterError> {
        let session = self.current_session(false)?;
        let owned = session
            .owner
            .as_ref()
            .is_some_and(|owner| owner.id.is_some());
        Ok(session.remembered() && !owned)
    }
}
