// The Epic Games Launcher adapter: everything it reads, and the three things it does to a process (ask the launcher to close, end it, hand a game link to the shell). Epic keeps no list of accounts: it signs in from the session it remembers in GameUserSettings.ini. This module writes no launcher file; the switch pipeline does that with what it plans.

mod plan;

use std::time::Duration;

use ol_formats::epic::ini::section_names;
use ol_formats::epic::launcher_log::{
    LogEvent, SignInOutcome, announced_restart, last_sign_in, parse_log_events, startup_finished,
};
use ol_formats::epic::manifests::{
    EpicItemManifest, LaunchUriForm, is_item_file, is_library_game, launch_uri, parse_item_manifest,
};
use ol_formats::epic::session::{EpicSessionOnDisk, read_session_from_ini};
use ol_formats::js;
use ol_formats::winpath::{join, normalize_launcher_path};
use ol_model::{
    InstallSource, InstalledGame, LastSignIn, LauncherId, LauncherInstall, LauncherSessionState,
    RememberedAccount, RunningGame, RunningGameSource,
};
use ol_platform_api::{CancelToken, Hive, Platform};
use ol_runtime::{TimingOverrides, wait_until};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::files::{
    MAX_LAUNCHER_FILE_BYTES, read_launcher_bytes, read_launcher_text, read_log_since,
};
use crate::running::game_with_running_process;
use crate::{AccountRef, AdapterError, Directory};

pub use plan::{PlanContext, STEP, close_message};

pub const EXE: &str = "EpicGamesLauncher.exe";
pub const HKLM_KEY: &str = r"SOFTWARE\WOW6432Node\Epic Games\EpicGamesLauncher";
pub const UNINSTALL_KEY: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\{AEB35C6C-B6D4-4AA0-8452-DE699737B5F6}";
pub const URI_COMMAND_KEY: &str = r"SOFTWARE\Classes\com.epicgames.launcher\shell\open\command";
pub const DEFAULT_DIR: &str = r"C:\Program Files (x86)\Epic Games\Launcher";
// The launcher kept its settings under Config\Windows for years and then moved to Config\WindowsEditor, so each folder that holds the file counts: the newest is read, all are copied and written.
pub const SETTINGS_FOLDERS: [&str; 2] = ["Windows", "WindowsEditor"];
// Why a snapshot is kept out of the usual clean-up.
pub const UNOWNED_SESSION_PIN: &str = "Holds a sign-in that no One Launch account owns";

const SETTINGS_FILE: &str = "GameUserSettings.ini";
const MAX_LOG_TAIL_BYTES: u64 = 8 * 1024 * 1024;
// Settings files written this close together count as written at once, and the later folder wins.
const SAME_WRITE: time::Duration = time::Duration::seconds(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timing {
    pub poll: Duration,
    // The waits that read the log or the settings file look more often than those that read the process list.
    pub log_poll: Duration,
    pub shutdown: Duration,
    pub kill_grace: Duration,
    // After it exits the launcher may hand over to its updater and come straight back. How long to watch for that.
    pub restart_watch: Duration,
    // The same watch when the log shows a clean exit without a word about a restart.
    pub clean_exit_watch: Duration,
    // How long a launcher that started itself again gets to finish starting before it is closed again.
    pub restart_settle: Duration,
    pub startup: Duration,
    pub sign_in: Duration,
    pub manual_sign_in: Duration,
    // How long the launcher gets to write the fresh session after a sign-in by hand.
    pub capture: Duration,
    pub ready_grace: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            poll: Duration::from_millis(500),
            log_poll: Duration::from_millis(250),
            shutdown: Duration::from_secs(20),
            kill_grace: Duration::from_secs(10),
            restart_watch: Duration::from_secs(8),
            clean_exit_watch: Duration::from_millis(1500),
            restart_settle: Duration::from_secs(45),
            startup: Duration::from_secs(60),
            sign_in: Duration::from_secs(45),
            manual_sign_in: Duration::from_secs(600),
            capture: Duration::from_secs(15),
            ready_grace: Duration::from_secs(2),
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
        }
    }
}

// What the settings and the machine say, read once for the call in hand.
#[derive(Clone)]
pub struct EpicConfig {
    // The folder the user chose with Locate, tried before the registry.
    pub chosen_dir: Option<String>,
    pub default_dir: String,
    // %LOCALAPPDATA% and %ProgramData%.
    pub local_app_data: String,
    pub program_data: String,
    pub launch_uri_form: LaunchUriForm,
    pub backups_to_keep: i64,
    pub backups_dir: String,
    pub timing: Timing,
}

pub struct EpicAdapter<'a> {
    pub(crate) platform: &'a Platform,
    pub(crate) config: &'a EpicConfig,
    pub(crate) directory: &'a dyn Directory,
}

// Where the launcher's log stood: its size and its first line, which names the run. The launcher starts a new log at every start.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct LogMark {
    pub size: u64,
    pub opened: Option<String>,
}

// Whose the session on disk is, and how that is known. No Debug: it holds the session.
#[derive(Clone)]
pub struct CurrentSession {
    pub disk: EpicSessionOnDisk,
    // While the launcher runs its own log decides, because the file can lag behind a sign-in by hand. Closed, the file's fingerprint decides, then the last run's log.
    pub owner: Option<AccountRef>,
    // The Epic account the launcher last signed in, by its current log.
    pub logged_uid: Option<String>,
    pub last_sign_in: Option<SignInOutcome>,
    // The launcher wrote the file after its last sign-in, so the file is that sign-in's.
    pub disk_current: bool,
}

impl CurrentSession {
    // The account the log names, when the file on disk is that sign-in's: a closed launcher's file older than the sign-in may be someone else's.
    fn uid_of_the_file(&self, running: bool) -> Option<String> {
        self.logged_uid
            .clone()
            .filter(|_| running || self.disk_current)
    }
}

// The end of an account id, enough to tell two apart in a sentence without spelling one out.
pub fn id_tail(uid: Option<&str>, length: usize) -> String {
    match uid.filter(|uid| !uid.is_empty()) {
        Some(uid) => {
            let skipped = uid.chars().count().saturating_sub(length);
            format!("…{}", uid.chars().skip(skipped).collect::<String>())
        }
        None => "(unknown id)".to_owned(),
    }
}

// A settings file's bytes as text, or why One Launch will not rewrite it. Unreal turns a config file into UTF-16 once a character outside ASCII lands in it, and such a file reads as empty; any other byte that is not UTF-8 would be written back as U+FFFD, and the file holds the sign-in.
pub fn settings_text<'b>(path: &str, bytes: &'b [u8]) -> Result<&'b str, AdapterError> {
    let refused = |message: String| AdapterError::LauncherFile {
        path: path.to_owned(),
        message,
    };
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) || bytes.contains(&0) {
        return Err(refused(format!(
            "{SETTINGS_FILE} is not UTF-8 (UTF-16 or binary content); One Launch will not rewrite it"
        )));
    }
    std::str::from_utf8(bytes).map_err(|_| {
        refused(format!(
            "{SETTINGS_FILE} holds bytes that are not UTF-8 text; One Launch will not rewrite it. Start Epic Games Launcher by hand and close it, so that it writes the file again, then try again"
        ))
    })
}

fn is_app_name(id: &str) -> bool {
    (1..=128).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
}

// The 32 hex digits a section such as [<id>_General] starts with.
fn account_of_section(name: &str) -> Option<String> {
    let id = name.get(..32)?;
    (name.as_bytes().get(32) == Some(&b'_') && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| id.to_lowercase())
}

impl<'a> EpicAdapter<'a> {
    pub const fn new(
        platform: &'a Platform,
        config: &'a EpicConfig,
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

    pub fn settings_path_in(&self, folder: &str) -> String {
        join(&[
            &self.config.local_app_data,
            "EpicGamesLauncher",
            "Saved",
            "Config",
            folder,
            SETTINGS_FILE,
        ])
    }

    // Every settings file there is, the newest first.
    pub fn settings_paths(&self) -> Vec<String> {
        let files = self.platform.files.as_ref();
        let mut found: Vec<(String, OffsetDateTime, usize)> = SETTINGS_FOLDERS
            .iter()
            .enumerate()
            .filter_map(|(order, folder)| {
                let path = self.settings_path_in(folder);
                let written = files.exists(&path).then(|| files.modified(&path).ok())??;
                Some((path, written, order))
            })
            .collect();
        found.sort_by(|a, b| {
            if (a.1 - b.1).abs() < SAME_WRITE {
                b.2.cmp(&a.2)
            } else {
                b.1.cmp(&a.1)
            }
        });
        found.into_iter().map(|(path, _, _)| path).collect()
    }

    // The settings file the launcher uses: the one written last, or where it has always been when there is none.
    pub fn settings_path(&self) -> String {
        self.settings_paths()
            .into_iter()
            .next()
            .unwrap_or_else(|| self.settings_path_in(SETTINGS_FOLDERS[0]))
    }

    // Where a settings file lands inside a snapshot.
    pub fn settings_relative(path: &str) -> String {
        let lower = path.to_lowercase();
        let folder = SETTINGS_FOLDERS
            .iter()
            .find(|folder| {
                lower.ends_with(&format!(
                    "config\\{}\\{}",
                    folder.to_lowercase(),
                    SETTINGS_FILE.to_lowercase()
                ))
            })
            .unwrap_or(&SETTINGS_FOLDERS[0]);
        format!("Config/{folder}/{SETTINGS_FILE}")
    }

    pub fn log_path(&self) -> String {
        join(&[
            &self.config.local_app_data,
            "EpicGamesLauncher",
            "Saved",
            "Logs",
            "EpicGamesLauncher.log",
        ])
    }

    pub fn manifests_dir(&self) -> Result<String, AdapterError> {
        let raw = self
            .platform
            .registry
            .get_string(Hive::Hklm, HKLM_KEY, "AppDataPath")?;
        let data_dir = match raw.filter(|raw| !js::trim(raw).is_empty()) {
            Some(raw) => normalize_launcher_path(&raw),
            None => join(&[
                &self.config.program_data,
                "Epic",
                "EpicGamesLauncher",
                "Data",
            ]),
        };
        Ok(join(&[&data_dir, "Manifests"]))
    }

    pub fn chosen_install_dir(&self) -> Option<String> {
        self.config
            .chosen_dir
            .as_deref()
            .filter(|raw| !js::trim(raw).is_empty())
            .map(normalize_launcher_path)
    }

    fn install_at(&self, install_dir: String, source: InstallSource) -> Option<LauncherInstall> {
        let exe_path = join(&[&install_dir, "Portal", "Binaries", "Win64", EXE]);
        self.platform
            .files
            .exists(&exe_path)
            .then_some(LauncherInstall {
                launcher: LauncherId::Epic,
                install_dir,
                exe_path,
                source,
            })
    }

    // The first of these that holds the launcher: the folder chosen in Settings, the folder its uninstall entry names, the program its own link opens with, the usual place.
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
                let folder = join(&[&normalize_launcher_path(&raw), "Launcher"]);
                self.install_at(folder, InstallSource::RegistryHklm)
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
            let folder = join(&[&normalize_launcher_path(exe), "..", "..", "..", ".."]);
            self.install_at(folder, InstallSource::RegistryHklm)
        }) {
            return Ok(Some(found));
        }
        Ok(self.install_at(self.config.default_dir.clone(), InstallSource::DefaultPath))
    }

    pub fn require_install(&self) -> Result<LauncherInstall, AdapterError> {
        self.detect_install()?
            .ok_or(AdapterError::NotInstalled(LauncherId::Epic))
    }

    // Every .item file that reads, in the order of their names. One that does not read is passed over.
    fn read_manifests(&self) -> Result<Vec<EpicItemManifest>, AdapterError> {
        let files = self.platform.files.as_ref();
        let dir = self.manifests_dir()?;
        if !files.exists(&dir) {
            return Ok(Vec::new());
        }
        let mut names = files.list(&dir).map_err(|error| AdapterError::io(&error))?;
        names.retain(|name| is_item_file(name));
        names.sort_by_cached_key(|name| name.to_lowercase());
        Ok(names
            .iter()
            .filter_map(|name| {
                let text = read_launcher_text(files, &join(&[&dir, name]), MAX_LAUNCHER_FILE_BYTES)
                    .ok()?;
                parse_item_manifest(&text).ok()
            })
            .collect())
    }

    pub fn list_installed_games(&self) -> Result<Vec<InstalledGame>, AdapterError> {
        self.require_install()?;
        let mut games: Vec<InstalledGame> = Vec::new();
        for manifest in self.read_manifests()? {
            let known = games
                .iter()
                .any(|game| game.launcher_game_id == manifest.app_name);
            if known || !is_library_game(&manifest) {
                continue;
            }
            games.push(InstalledGame {
                launcher: LauncherId::Epic,
                library_path: Some(join(&[&manifest.install_location, ".."])),
                size_on_disk_bytes: manifest
                    .install_size
                    .filter(|size| size.fract() == 0.0 && *size < 9.0e15)
                    .map(|size| size as i64),
                last_played_at: None,
                needs_update: false,
                launcher_game_id: manifest.app_name,
                name: manifest.display_name,
                install_path: manifest.install_location,
            });
        }
        games.sort_by_cached_key(|game| game.name.to_lowercase());
        Ok(games)
    }

    // The settings file as text, or None when there is none. A file that is not UTF-8 text is refused (see `settings_text`).
    pub fn read_ini(&self) -> Result<Option<String>, AdapterError> {
        let files = self.platform.files.as_ref();
        let file = self.settings_path();
        if !files.exists(&file) {
            return Ok(None);
        }
        let bytes = read_launcher_bytes(files, &file, MAX_LAUNCHER_FILE_BYTES).map_err(
            |error| match error {
                AdapterError::LauncherFile { .. } => error,
                other => AdapterError::LauncherFile {
                    path: file.clone(),
                    message: format!("Could not read {SETTINGS_FILE} ({other})"),
                },
            },
        )?;
        settings_text(&file, &bytes).map(|text| Some(text.to_owned()))
    }

    fn first_log_line(&self, path: &str) -> Option<String> {
        let head = self.platform.files.read_range(path, 0, 256).ok()?;
        let head = String::from_utf8_lossy(&head);
        let line = head.split('\n').next().unwrap_or_default();
        let line = line.strip_suffix('\r').unwrap_or(line);
        (!line.is_empty()).then(|| line.to_owned())
    }

    pub fn log_mark(&self) -> LogMark {
        let files = self.platform.files.as_ref();
        let path = self.log_path();
        match files
            .exists(&path)
            .then(|| files.size(&path).ok())
            .flatten()
        {
            Some(size) => LogMark {
                size,
                opened: self.first_log_line(&path),
            },
            None => LogMark::default(),
        }
    }

    // What the launcher has logged since `mark`, or all of its log when it has started a new one since.
    pub fn events_since(&self, mark: &LogMark) -> Vec<LogEvent> {
        let files = self.platform.files.as_ref();
        let path = self.log_path();
        if !files.exists(&path) {
            return Vec::new();
        }
        let Ok(size) = files.size(&path) else {
            return Vec::new();
        };
        let opened = self.first_log_line(&path);
        let same_run = opened.is_some() && opened == mark.opened && size >= mark.size;
        let from = if same_run { mark.size } else { 0 };
        parse_log_events(&read_log_since(files, &path, from, MAX_LOG_TAIL_BYTES))
    }

    pub fn last_sign_in_in_log(&self) -> Option<SignInOutcome> {
        last_sign_in(&self.events_since(&LogMark::default()))
    }

    // What is on disk now and whose it is. Reads the settings file's time and the launcher's log; writes nothing.
    pub fn current_session(
        &self,
        ini_text: Option<&str>,
        running: bool,
    ) -> Result<CurrentSession, AdapterError> {
        let disk = ini_text.map_or_else(EpicSessionOnDisk::absent, read_session_from_ini);
        let last_sign_in = self.last_sign_in_in_log();
        let (logged_uid, signed_in_at) = match &last_sign_in {
            Some(SignInOutcome::SignedIn { account_id, at }) => (
                Some(account_id.clone()).filter(|id| !id.is_empty()),
                at.as_deref()
                    .map(|at| OffsetDateTime::parse(at, &Rfc3339).ok()),
            ),
            _ => (None, None),
        };
        let by_log = match &logged_uid {
            Some(uid) => self.directory.account_by_launcher_uid(uid)?,
            None => None,
        };
        let written_at = self.platform.files.modified(&self.settings_path()).ok();
        let disk_current = ini_text.is_some()
            && match (signed_in_at, written_at) {
                (None, _) | (_, None) => true,
                (Some(signed_in), Some(written)) => signed_in.is_some_and(|at| written >= at),
            };
        let mut owner = None;
        if running {
            owner = by_log;
        } else if let (true, Some(fingerprint)) = (disk.remembered, &disk.fingerprint) {
            if let Some(stored) = self.directory.session_by_fingerprint(fingerprint)? {
                owner = self.directory.account_by_id(&stored.account_id)?;
            }
            if owner.is_none() && disk_current {
                owner = by_log;
            }
        }
        Ok(CurrentSession {
            disk,
            owner,
            logged_uid,
            last_sign_in,
            disk_current,
        })
    }

    pub fn session_state(&self) -> Result<LauncherSessionState, AdapterError> {
        let running = self.is_running()?;
        let current = self.current_session(self.read_ini()?.as_deref(), running)?;
        let owner_uid = || {
            current
                .owner
                .as_ref()
                .and_then(|owner| owner.launcher_uid.clone())
        };
        Ok(LauncherSessionState {
            launcher: LauncherId::Epic,
            remembered: current.disk.remembered,
            fingerprint: current
                .disk
                .fingerprint
                .clone()
                .filter(|_| current.disk.remembered),
            launcher_uid: if running {
                current.logged_uid.clone()
            } else if current.disk.remembered {
                owner_uid().or_else(|| current.uid_of_the_file(running))
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

    // Every account that has signed in on this PC has sections named [<id>_...]. Only the session of the current one is remembered.
    pub fn list_remembered_accounts(&self) -> Result<Vec<RememberedAccount>, AdapterError> {
        self.require_install()?;
        let Some(text) = self.read_ini()? else {
            return Ok(Vec::new());
        };
        let mut ids: Vec<String> = Vec::new();
        for id in section_names(&text)
            .iter()
            .filter_map(|name| account_of_section(name))
        {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        let running = self.is_running()?;
        let current = self.current_session(Some(&text), running)?;
        let active = current
            .disk
            .remembered
            .then(|| {
                current
                    .owner
                    .as_ref()
                    .and_then(|owner| owner.launcher_uid.clone())
                    .or_else(|| current.uid_of_the_file(running))
            })
            .flatten()
            .map(|uid| uid.to_lowercase());
        let mut accounts = Vec::new();
        for uid in ids {
            let linked = self.directory.account_by_launcher_uid(&uid)?;
            let is_active = active.as_deref() == Some(uid.as_str());
            accounts.push(RememberedAccount {
                launcher: LauncherId::Epic,
                login: linked
                    .as_ref()
                    .map(|linked| linked.login.clone())
                    .unwrap_or_default(),
                persona_name: linked.map(|linked| linked.display_name).unwrap_or_default(),
                remember_password: is_active,
                most_recent: is_active,
                last_login_at: None,
                session_may_be_expired: false,
                linked_account_id: None,
                launcher_uid: uid,
            });
        }
        accounts.sort_by_key(|account| !account.most_recent);
        Ok(accounts)
    }

    pub fn is_running(&self) -> Result<bool, AdapterError> {
        Ok(self.platform.processes.is_running(EXE)?)
    }

    // Epic keeps no word of what it runs: a process that runs out of a game's folder counts.
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
                launcher: LauncherId::Epic,
                launcher_game_id: game.launcher_game_id.clone(),
                name: game.name.clone(),
                source: RunningGameSource::Process,
            }),
        )
    }

    // Asks the launcher's window to close, waits, then ends it and what it started. A launcher that hands over to its updater and comes back is given time to finish starting and closed the same way again, so it is never ended while it writes its files. Fails when the launcher still runs after that.
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
                processes.kill(EXE, true)?;
                if !wait_until(clock, cancel, timing.kill_grace, timing.poll, gone)? {
                    return Err(AdapterError::Failed(format!(
                        "{EXE} is still running after a window close and a forced close"
                    )));
                }
            }
            let events = self.events_since(&mark);
            let restarting = announced_restart(&events);
            let watch = if restarting {
                timing.restart_settle
            } else if events.contains(&LogEvent::Exit) {
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
                    self.log_mark().opened != mark.opened
                        && startup_finished(&self.events_since(&LogMark::default())),
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

    // Hands the game's com.epicgames.launcher:// link to the shell, as the launcher's own shortcuts do. Returns what was sent.
    pub fn launch_game(&self, launcher_game_id: &str) -> Result<String, AdapterError> {
        if !is_app_name(launcher_game_id) {
            return Err(AdapterError::Failed(format!(
                "Not an Epic app name: {launcher_game_id}"
            )));
        }
        let manifests = self.read_manifests()?;
        let manifest = manifests
            .iter()
            .find(|manifest| manifest.app_name == launcher_game_id);
        let url = launch_uri(
            launcher_game_id,
            manifest.and_then(|manifest| manifest.catalog_namespace.as_deref()),
            manifest.and_then(|manifest| manifest.catalog_item_id.as_deref()),
            self.config.launch_uri_form,
        );
        self.platform.shell.open_url(&url)?;
        Ok(url)
    }

    // Whether the settings file, as it is now, remembers a session that no One Launch account owns. Only means something while the launcher is closed.
    pub fn holds_unowned_session(&self) -> Result<bool, AdapterError> {
        let Some(text) = self.read_ini()? else {
            return Ok(false);
        };
        let session = self.current_session(Some(&text), false)?;
        let owned = session.owner.is_some_and(|owner| owner.id.is_some());
        Ok(session.disk.remembered && !owned)
    }
}
