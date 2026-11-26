// The Riot Games adapter: everything it reads, and the few things it does to a process (start the client as its own shortcut does, ask the client's windows to close, end the client alone). The Riot Client signs in from one file, RiotGamesPrivateSettings.yaml, which names the account it holds a sign-in for. This module writes neither that file nor anything else of the launcher's: it is handed the files and the registry to read only, and the switch pipeline writes what it plans.
//
// Riot's rules come before convenience. A Riot game and Vanguard are known by image name alone and are never closed, ended, opened or asked for a path; a running game refuses a switch, a restore and a launch, and a process listing that fails refuses too; nothing goes to Riot's servers, and the one request to the client's own local server is the launch request its Play button sends.

mod health;
mod plan;
mod processes;

use std::cmp::Reverse;
use std::collections::HashMap;
use std::time::Duration;

use ol_formats::js;
use ol_formats::riot::client_log::{
    FailReason, LogEvent, NOT_UP_TO_DATE, STUB_LOG_MARK, STUB_LOG_MAX_BYTES, SignInOutcome,
    SignedInHow, exited_cleanly, game_started, is_client_log_file, last_sign_in, launch_accepted,
    launch_declined, launch_page_shown, parse_log_events, products_needing_update,
    startup_finished,
};
use ol_formats::riot::products::{
    RiotInstalls, RiotProductId, RiotProductSettings, game_images_of, image_key, launch_args,
    parse_installs, parse_last_session_timestamps, parse_product_id, parse_product_settings,
    product_display_name, product_with_running_image,
};
use ol_formats::riot::session::{RiotSession, RiotSessionBlob, SessionRead, read_session};
use ol_formats::winpath::{join, normalize_launcher_path};
use ol_model::{
    InstallSource, InstalledGame, LastSignIn, LauncherId, LauncherInstall, LauncherSessionState,
    RememberedAccount, RunningGame, RunningGameSource,
};
use ol_platform_api::{
    CancelToken, Clock, FileReader, Hive, Platform, RegistryReader, RiotLaunchRequest,
    RiotLocalLaunch,
};
use ol_runtime::{TimingOverrides, wait_until};
use time::OffsetDateTime;

pub use crate::epic::{UNOWNED_SESSION_PIN, id_tail};
use crate::files::{
    MAX_LAUNCHER_FILE_BYTES, read_launcher_bytes, read_launcher_text, read_log_since,
};
use crate::steam::seconds;
use crate::{AccountRef, AdapterError, Directory, Launched};

pub use plan::{CLOSE_MESSAGE, PlanContext, STEP};
pub use processes::ClientImage;
use processes::ClientProcesses;

pub const EXE: &str = ClientImage::Core.exe();
pub const WINDOW_EXE: &str = ClientImage::Window.exe();
// Riot writes its uninstall entries for the user who installed. The client's own key ends in a dot.
pub const UNINSTALL_ROOT: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
pub const CLIENT_UNINSTALL_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Riot Game Riot_Client.";
pub const URI_COMMAND_KEY: &str = r"SOFTWARE\Classes\riotclient\shell\open\command";
pub const INSTALLS_FILE: &str = "RiotClientInstalls.json";
pub const DEFAULT_DIR: &str = r"C:\Riot Games\Riot Client";
// The one launcher file a switch writes.
pub const SESSION_FILE: &str = "RiotGamesPrivateSettings.yaml";
// Where it lands inside a snapshot.
pub const SESSION_FILE_RELATIVE: &str = "Data/RiotGamesPrivateSettings.yaml";

const MAX_LOG_TAIL_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timing {
    pub poll: Duration,
    // The waits that read the log or the session file look more often than those that read the process list.
    pub log_poll: Duration,
    // How often a watch for a game's start lists the processes; the log is read at every look.
    pub process_poll: Duration,
    // The client's one window, the tray icon's, swallows the close on the current build: a short polite try, then the client is ended.
    pub shutdown: Duration,
    pub kill_grace: Duration,
    // The window processes once the client is gone: those with a window get the close, the helpers go with their parent.
    pub window_close: Duration,
    // How long to watch for the client coming back after it exits, as it does for an update of itself.
    pub restart_watch: Duration,
    // The same watch once the log shows the lock file removed, or after the client was ended.
    pub clean_exit_watch: Duration,
    // How long a client that started itself again gets to reach a verdict on its sign-in before it is closed again.
    pub restart_settle: Duration,
    pub startup: Duration,
    pub sign_in: Duration,
    pub manual_sign_in: Duration,
    // How long the client gets to write the fresh session file after a sign-in.
    pub capture: Duration,
    pub ready_grace: Duration,
    // How long the log is watched after the client was handed a game.
    pub launch_confirm: Duration,
    // How long the game gets to start after One Launch's own Play.
    pub play_confirm: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            poll: Duration::from_millis(500),
            log_poll: Duration::from_millis(250),
            process_poll: Duration::from_secs(1),
            shutdown: Duration::from_secs(3),
            kill_grace: Duration::from_secs(10),
            window_close: Duration::from_secs(10),
            restart_watch: Duration::from_secs(5),
            clean_exit_watch: Duration::from_millis(1500),
            restart_settle: Duration::from_secs(45),
            startup: Duration::from_secs(60),
            sign_in: Duration::from_secs(45),
            manual_sign_in: Duration::from_secs(600),
            capture: Duration::from_secs(15),
            ready_grace: Duration::from_secs(2),
            launch_confirm: Duration::from_secs(90),
            play_confirm: Duration::from_secs(30),
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
            process_poll: overrides.finer_poll("processPollMs", default.process_poll),
            shutdown: named("shutdownMs", default.shutdown),
            kill_grace: named("killGraceMs", default.kill_grace),
            window_close: named("windowCloseMs", default.window_close),
            restart_watch: named("restartWatchMs", default.restart_watch),
            clean_exit_watch: named("cleanExitWatchMs", default.clean_exit_watch),
            restart_settle: named("restartSettleMs", default.restart_settle),
            startup: named("startupMs", default.startup),
            sign_in: named("signInMs", default.sign_in),
            manual_sign_in: named("manualSignInMs", default.manual_sign_in),
            capture: named("captureMs", default.capture),
            ready_grace: named("readyGraceMs", default.ready_grace),
            launch_confirm: named("launchConfirmMs", default.launch_confirm),
            play_confirm: named("playConfirmMs", default.play_confirm),
        }
    }
}

// What the settings and the machine say, read once for the call in hand.
#[derive(Clone)]
pub struct RiotConfig {
    // The folder the user chose with Locate, tried before the registry.
    pub chosen_dir: Option<String>,
    pub default_dir: String,
    // %LOCALAPPDATA% and %ProgramData%.
    pub local_app_data: String,
    pub program_data: String,
    // Once the client shows the game's page, press Play for the user.
    pub press_play: bool,
    pub backups_to_keep: i64,
    pub backups_dir: String,
    pub timing: Timing,
}

pub struct RiotAdapter<'a> {
    pub(crate) files: &'a dyn FileReader,
    pub(crate) registry: &'a dyn RegistryReader,
    processes: ClientProcesses<'a>,
    pub(crate) clock: &'a dyn Clock,
    pub(crate) config: &'a RiotConfig,
    pub(crate) directory: &'a dyn Directory,
    // The way to the client's own local server, for the one request `launch_game` sends through it. None in a build that has no such way.
    local_launch: Option<&'a dyn RiotLocalLaunch>,
}

// Where the client's log stood: the newest run's log, which a new run replaces, and its size.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct LogMark {
    pub file: Option<String>,
    pub size: u64,
}

// The session file as it is now, and what its block says. No Debug: `file` is the sign-in.
pub struct SessionFile {
    pub file: RiotSessionBlob,
    pub read: SessionRead,
}

// The session file as it is now and whose sign-in it holds. No Debug: `file` is the sign-in.
pub struct CurrentSession {
    // None when there is no file. Wiped when dropped.
    pub file: Option<RiotSessionBlob>,
    pub session: Option<RiotSession>,
    // The One Launch account the sign-in belongs to: by a saved copy's fingerprint first, then by the account id the file itself names.
    pub owner: Option<AccountRef>,
}

struct ProductRecord {
    id: RiotProductId,
    settings: Option<RiotProductSettings>,
    // Riot's own mark that the install finished. A settings file alone is also there for a product that was never installed.
    ok: bool,
    install_path: Option<String>,
}

fn table_name(id: &RiotProductId) -> String {
    let name = product_display_name(id.product());
    if id.patchline() == "live" {
        name.to_owned()
    } else {
        format!("{name} ({})", id.patchline().to_uppercase())
    }
}

// The log's verdict as an account's page shows it. A closed client whose last run signed in by hand, and whose file holds no sign-in, kept nothing: "Stay signed in" was not ticked. A start without a saved sign-in is not a try at one.
fn session_last_sign_in(
    last: Option<SignInOutcome>,
    running: bool,
    remembered: bool,
) -> Option<LastSignIn> {
    match last? {
        SignInOutcome::SignedIn {
            how: SignedInHow::Manual,
        } if !running && !remembered => Some(LastSignIn {
            ok: false,
            reason: Some("the last sign-in was not kept because “Stay signed in” was not ticked. Sign in to the Riot Client again with it ticked.".to_owned()),
        }),
        SignInOutcome::SignedIn { .. } => Some(LastSignIn {
            ok: true,
            reason: None,
        }),
        SignInOutcome::Failed {
            reason: FailReason::NoSession,
            ..
        } => None,
        SignInOutcome::Failed { detail, .. } => Some(LastSignIn {
            ok: false,
            reason: Some(detail),
        }),
    }
}

// Whether one of a product's games runs, asked at most once in `process_poll`: in between the answer is no, and the log is what each look reads.
struct GameWatch<'w, 'a> {
    riot: &'w RiotAdapter<'a>,
    images: &'static [&'static str],
    looked: Option<OffsetDateTime>,
}

impl GameWatch<'_, '_> {
    fn is_up(&mut self) -> Result<bool, AdapterError> {
        let now = self.riot.clock.now();
        if self
            .looked
            .is_some_and(|at| now - at < self.riot.config.timing.process_poll)
        {
            return Ok(false);
        }
        self.looked = Some(now);
        let names = self.riot.processes.names()?;
        Ok(self
            .images
            .iter()
            .any(|image| names.contains(&image_key(image))))
    }
}

enum PlayOutcome {
    Started,
    Declined(String),
    Silent(String),
    OutOfDate,
    Stopped,
}

impl<'a> RiotAdapter<'a> {
    pub fn new(
        platform: &'a Platform,
        config: &'a RiotConfig,
        directory: &'a dyn Directory,
        local_launch: Option<&'a dyn RiotLocalLaunch>,
    ) -> Self {
        Self {
            files: platform.files.as_ref(),
            registry: platform.registry.as_ref(),
            processes: ClientProcesses::new(platform.processes.as_ref()),
            clock: platform.clock.as_ref(),
            config,
            directory,
            local_launch,
        }
    }

    pub fn directory(&self) -> &'a dyn Directory {
        self.directory
    }

    pub fn timing(&self) -> &Timing {
        &self.config.timing
    }

    // The client's own folder, one for each Windows user.
    pub fn data_dir(&self) -> String {
        join(&[&self.config.local_app_data, "Riot Games", "Riot Client"])
    }

    pub fn log_dir(&self) -> String {
        join(&[&self.data_dir(), "Logs", "Riot Client Logs"])
    }

    // Handed to the launch request and to nothing else: this module never opens the file.
    fn lockfile_path(&self) -> String {
        join(&[&self.data_dir(), "Config", "lockfile"])
    }

    pub fn session_file_path(&self) -> String {
        join(&[&self.data_dir(), "Data", SESSION_FILE])
    }

    pub fn settings_path(&self) -> String {
        join(&[&self.data_dir(), "Config", "RiotClientSettings.yaml"])
    }

    pub fn metadata_dir(&self) -> String {
        join(&[&self.config.program_data, "Riot Games", "Metadata"])
    }

    pub fn installs_path(&self) -> String {
        join(&[&self.config.program_data, "Riot Games", INSTALLS_FILE])
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
        self.files.exists(&exe_path).then_some(LauncherInstall {
            launcher: LauncherId::Riot,
            install_dir,
            exe_path,
            source,
        })
    }

    // RiotClientInstalls.json, or None when it is missing or does not read.
    fn installs(&self) -> Option<RiotInstalls> {
        let path = self.installs_path();
        if !self.files.exists(&path) {
            return None;
        }
        let text = read_launcher_text(self.files, &path, MAX_LAUNCHER_FILE_BYTES).ok()?;
        Some(parse_installs(&text))
    }

    // The first of these that holds RiotClientServices.exe: the folder chosen in Settings, the folder the client's uninstall entry names, the program RiotClientInstalls.json names, the program the client's own link opens with, the usual place.
    pub fn detect_install(&self) -> Result<Option<LauncherInstall>, AdapterError> {
        if let Some(chosen) = self
            .chosen_install_dir()
            .and_then(|chosen| self.install_at(chosen, InstallSource::User))
        {
            return Ok(Some(chosen));
        }
        let uninstall =
            self.registry
                .get_string(Hive::Hkcu, CLIENT_UNINSTALL_KEY, "InstallLocation")?;
        if let Some(found) = uninstall
            .filter(|raw| !js::trim(raw).is_empty())
            .and_then(|raw| {
                self.install_at(normalize_launcher_path(&raw), InstallSource::RegistryHkcu)
            })
        {
            return Ok(Some(found));
        }
        let installs = self.installs().unwrap_or_default();
        for exe in [installs.rc_default, installs.rc_live]
            .into_iter()
            .flatten()
        {
            let folder = join(&[&normalize_launcher_path(&exe), ".."]);
            if let Some(found) = self.install_at(folder, InstallSource::LauncherFile) {
                return Ok(Some(found));
            }
        }
        for (hive, source) in [
            (Hive::Hklm, InstallSource::RegistryHklm),
            (Hive::Hkcu, InstallSource::RegistryHkcu),
        ] {
            let command = self.registry.get_string(hive, URI_COMMAND_KEY, "")?;
            let quoted = command.as_deref().and_then(|command| {
                let rest = command.strip_prefix('"')?;
                let exe = &rest[..rest.find('"')?];
                (!exe.is_empty()).then_some(exe)
            });
            if let Some(found) = quoted.and_then(|exe| {
                let folder = join(&[&normalize_launcher_path(exe), ".."]);
                self.install_at(folder, source)
            }) {
                return Ok(Some(found));
            }
        }
        Ok(self.install_at(self.config.default_dir.clone(), InstallSource::DefaultPath))
    }

    pub fn require_install(&self) -> Result<LauncherInstall, AdapterError> {
        self.detect_install()?
            .ok_or(AdapterError::NotInstalled(LauncherId::Riot))
    }

    // One record for each `<product>.<patchline>` folder under Metadata, installed or not.
    fn products(&self) -> Vec<ProductRecord> {
        self.listed_products().0
    }

    // The products, and why the list can be short: a metadata folder that could not be listed gives none, and a product's settings file that could not be read gives no install folder for it.
    fn listed_products(&self) -> (Vec<ProductRecord>, Option<String>) {
        let dir = self.metadata_dir();
        if !self.files.exists(&dir) {
            return (Vec::new(), None);
        }
        let names = match self.files.list(&dir) {
            Ok(names) => names,
            Err(error) => return (Vec::new(), Some(format!("Could not list {dir}: {error}"))),
        };
        let mut unread = None;
        let mut records: Vec<ProductRecord> = names
            .iter()
            .filter_map(|name| {
                let id = parse_product_id(name)?;
                let folder = join(&[&dir, name]);
                let settings_path = join(&[&folder, &format!("{name}.product_settings.yaml")]);
                let settings = if self.files.exists(&settings_path) {
                    match read_launcher_text(self.files, &settings_path, MAX_LAUNCHER_FILE_BYTES) {
                        Ok(text) => Some(parse_product_settings(&text)),
                        Err(error) => {
                            unread.get_or_insert_with(|| {
                                format!("Could not read {settings_path}: {error}")
                            });
                            None
                        }
                    }
                } else {
                    None
                };
                Some(ProductRecord {
                    ok: self.files.exists(&join(&[&folder, &format!("{name}.ok")])),
                    install_path: settings
                        .as_ref()
                        .and_then(|settings| settings.install_path.as_deref())
                        .map(normalize_launcher_path),
                    settings,
                    id,
                })
            })
            .collect();
        records.sort_by(|a, b| a.id.id().cmp(b.id.id()));
        (records, unread)
    }

    // What Windows' list of apps calls the product, from its uninstall entry.
    fn uninstall_display_name(&self, id: &str) -> Option<String> {
        let key = format!("{UNINSTALL_ROOT}\\Riot Game {id}");
        let name = self
            .registry
            .get_string(Hive::Hkcu, &key, "DisplayName")
            .ok()??;
        Some(js::trim(&name).to_owned()).filter(|name| !name.is_empty())
    }

    fn last_session_timestamps(&self) -> HashMap<String, String> {
        let path = self.settings_path();
        if !self.files.exists(&path) {
            return HashMap::new();
        }
        read_launcher_text(self.files, &path, MAX_LAUNCHER_FILE_BYTES)
            .map(|text| parse_last_session_timestamps(&text))
            .unwrap_or_default()
    }

    // A product counts as installed when Riot's own mark is there and its folder is.
    fn games_from(&self, records: &[ProductRecord], out_of_date: &[String]) -> Vec<InstalledGame> {
        let last_sessions = self.last_session_timestamps();
        let mut games: Vec<InstalledGame> = records
            .iter()
            .filter_map(|record| {
                let install_path = record
                    .install_path
                    .clone()
                    .filter(|path| record.ok && self.files.exists(path))?;
                let key = record.id.key();
                let root = record
                    .settings
                    .as_ref()
                    .and_then(|settings| settings.install_root.as_deref());
                Some(InstalledGame {
                    launcher: LauncherId::Riot,
                    launcher_game_id: record.id.id().to_owned(),
                    name: self
                        .uninstall_display_name(record.id.id())
                        .unwrap_or_else(|| table_name(&record.id)),
                    library_path: Some(match root {
                        Some(root) => normalize_launcher_path(root),
                        None => join(&[&install_path, ".."]),
                    }),
                    size_on_disk_bytes: None,
                    last_played_at: last_sessions.get(&key).cloned(),
                    needs_update: out_of_date.contains(&key),
                    install_path,
                })
            })
            .collect();
        games.sort_by_cached_key(|game| game.name.to_lowercase());
        games
    }

    // The client logs every product's patch state at each start and again when it changes, so the newest run's last word on a product stands until the next run.
    pub fn list_installed_games(&self) -> Result<Vec<InstalledGame>, AdapterError> {
        Ok(self.scan_installed_games()?.0)
    }

    // The games, and why the list can be short.
    pub fn scan_installed_games(
        &self,
    ) -> Result<(Vec<InstalledGame>, Option<String>), AdapterError> {
        self.require_install()?;
        let out_of_date = products_needing_update(&self.current_run_events());
        let (products, unread) = self.listed_products();
        Ok((self.games_from(&products, &out_of_date), unread))
    }

    // The current run's log: the newest by name, which holds the local time the run started, that is not the few lines a second RiotClientServices.exe leaves behind.
    fn newest_client_log(&self) -> Option<String> {
        let dir = self.log_dir();
        if !self.files.exists(&dir) {
            return None;
        }
        let mut names = self.files.list(&dir).ok()?;
        names.retain(|name| is_client_log_file(name));
        names.sort_by_cached_key(|name| Reverse(name.to_lowercase()));
        let is_stub = |path: &String| {
            let Ok(size) = self.files.size(path) else {
                return false;
            };
            size <= STUB_LOG_MAX_BYTES
                && self
                    .files
                    .read_range(path, 0, size)
                    .is_ok_and(|head| String::from_utf8_lossy(&head).contains(STUB_LOG_MARK))
        };
        names
            .iter()
            .map(|name| join(&[&dir, name]))
            .find(|path| !is_stub(path))
    }

    pub fn log_mark(&self) -> LogMark {
        let file = self.newest_client_log();
        let size = file.as_deref().and_then(|file| self.files.size(file).ok());
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
        parse_log_events(&read_log_since(self.files, &file, from, MAX_LOG_TAIL_BYTES))
    }

    pub fn current_run_events(&self) -> Vec<LogEvent> {
        self.events_since(&LogMark::default())
    }

    // The session file and what its block says, or None when there is no file. A file that cannot be read, or that is not UTF-8 text, is an error: it is never taken for a client that is signed out.
    pub fn read_session_file(&self) -> Result<Option<SessionFile>, AdapterError> {
        let path = self.session_file_path();
        if !self.files.exists(&path) {
            return Ok(None);
        }
        let refused = |message: String| AdapterError::LauncherFile {
            path: path.clone(),
            message,
        };
        let bytes =
            read_launcher_bytes(self.files, &path, MAX_LAUNCHER_FILE_BYTES).map_err(|error| {
                match error {
                    AdapterError::LauncherFile { .. } => error,
                    other => refused(format!("Could not read {SESSION_FILE} ({other})")),
                }
            })?;
        let file = RiotSessionBlob::from_bytes(bytes).map_err(|not_text| {
            refused(format!(
                "{SESSION_FILE} is not in the shape One Launch knows ({not_text})"
            ))
        })?;
        let read = read_session(&file.file);
        Ok(Some(SessionFile { file, read }))
    }

    // What the session file holds now and whose sign-in it is. Reads the file only. A file in a shape the reader does not know is an error, not a sign-out.
    pub fn current_session(&self) -> Result<CurrentSession, AdapterError> {
        self.session_in(self.read_session_file()?)
    }

    // What a read of the session file holds and whose sign-in it is, for a caller that needs to know which read it was.
    pub fn session_in(&self, read: Option<SessionFile>) -> Result<CurrentSession, AdapterError> {
        let Some(SessionFile { file, read }) = read else {
            return Ok(CurrentSession {
                file: None,
                session: None,
                owner: None,
            });
        };
        let session = match read {
            SessionRead::Unexpected(reason) => {
                return Err(AdapterError::LauncherFile {
                    path: self.session_file_path(),
                    message: format!(
                        "{SESSION_FILE} is not in the shape One Launch knows ({reason})"
                    ),
                });
            }
            SessionRead::SignedOut => None,
            SessionRead::Session(session) => Some(session),
        };
        let mut owner = None;
        if let Some(session) = &session {
            if let Some(stored) = self
                .directory
                .session_by_fingerprint(&session.fingerprint)?
            {
                owner = self.directory.account_by_id(&stored.account_id)?;
            }
            if owner.is_none() {
                owner = self
                    .directory
                    .account_by_launcher_uid(&session.account_id)?;
            }
        }
        Ok(CurrentSession {
            file: Some(file),
            session,
            owner,
        })
    }

    pub fn session_state(&self) -> Result<LauncherSessionState, AdapterError> {
        self.require_install()?;
        let running = self.is_running()?;
        let current = self.current_session()?;
        let remembered = current.session.is_some();
        Ok(LauncherSessionState {
            launcher: LauncherId::Riot,
            remembered,
            fingerprint: current
                .session
                .as_ref()
                .map(|session| session.fingerprint.clone()),
            launcher_uid: current
                .session
                .as_ref()
                .map(|session| session.account_id.clone()),
            linked_account_id: current.owner.as_ref().and_then(|owner| owner.id.clone()),
            launcher_running: running,
            last_sign_in: session_last_sign_in(
                last_sign_in(&self.current_run_events()),
                running,
                remembered,
            ),
        })
    }

    // The one account the session file names, when it holds a sign-in. The file records no login, and the client keeps no list of past sign-ins.
    pub fn list_remembered_accounts(&self) -> Result<Vec<RememberedAccount>, AdapterError> {
        self.require_install()?;
        let current = self.current_session()?;
        let Some(session) = &current.session else {
            return Ok(Vec::new());
        };
        let owner = current.owner.as_ref();
        Ok(vec![RememberedAccount {
            launcher: LauncherId::Riot,
            launcher_uid: session.account_id.clone(),
            login: owner.map(|owner| owner.login.clone()).unwrap_or_default(),
            persona_name: match (owner, &session.riot_id) {
                (Some(owner), _) => owner.display_name.clone(),
                (None, Some(riot_id)) => riot_id.text(),
                (None, None) => String::new(),
            },
            remember_password: true,
            most_recent: true,
            last_login_at: None,
            session_may_be_expired: false,
            linked_account_id: None,
        }])
    }

    pub fn is_running(&self) -> Result<bool, AdapterError> {
        self.processes.is_running(ClientImage::Core)
    }

    // The installed games without the read of the log: the check for a running game only needs their names.
    fn installed_games_or_none(&self) -> Vec<InstalledGame> {
        match self.detect_install() {
            Ok(Some(_)) => self.games_from(&self.products(), &[]),
            _ => Vec::new(),
        }
    }

    // The Riot game that runs, known by its image name alone. A listing that fails is an error: a launch, a switch or a restore then refuses, and never goes on past a game it could not see. `names` is a listing already taken.
    pub fn find_running_game(
        &self,
        names: Option<&[String]>,
    ) -> Result<Option<RunningGame>, AdapterError> {
        let listed;
        let names = match names {
            Some(names) => names,
            None => {
                listed = self.processes.names()?;
                &listed
            }
        };
        let Some(product) = product_with_running_image(names.iter().map(String::as_str)) else {
            return Ok(None);
        };
        let games = self.installed_games_or_none();
        let prefix = format!("{product}.");
        let game = games
            .iter()
            .find(|game| game.launcher_game_id.starts_with(&prefix));
        Ok(Some(RunningGame {
            launcher: LauncherId::Riot,
            launcher_game_id: game.map_or_else(
                || format!("{product}.live"),
                |game| game.launcher_game_id.clone(),
            ),
            name: game.map_or_else(
                || product_display_name(product).to_owned(),
                |game| game.name.clone(),
            ),
            source: RunningGameSource::Process,
        }))
    }

    // No switch and no restore while a Riot game runs, and no offer to close it: closing the client in the middle of a match is a leave.
    pub fn game_running_error(game: RunningGame, action: &str) -> AdapterError {
        AdapterError::GameRunning {
            message: format!(
                "{} is running. Close it, then {action}: One Launch never closes a Riot game.",
                game.name
            ),
            game,
        }
    }

    fn close_and_wait(
        &self,
        image: ClientImage,
        timeout: Duration,
        cancel: &CancelToken,
    ) -> Result<bool, AdapterError> {
        let timing = self.timing();
        let gone = || Ok::<_, AdapterError>(!self.processes.is_running(image)?);
        self.no_game_runs()?;
        self.processes.close(image)?;
        if wait_until(self.clock, cancel, timeout, timing.poll, gone)? {
            return Ok(false);
        }
        // A window close that was ignored is followed by the end of the image's own processes, never of what they started: a running game is the client's child. A game can have started during the wait.
        self.no_game_runs()?;
        self.processes.end(image)?;
        if !wait_until(self.clock, cancel, timing.kill_grace, timing.poll, gone)? {
            return Err(AdapterError::Failed(format!(
                "{} is still running after a window close and a forced close",
                image.exe()
            )));
        }
        Ok(true)
    }

    // The window processes, once the client is gone or was never there. The tray icon lingers until the pointer passes over it; the processes are what counts.
    fn close_windows(&self, cancel: &CancelToken) -> Result<(), AdapterError> {
        if !self.processes.is_running(ClientImage::Window)? {
            return Ok(());
        }
        self.close_and_wait(ClientImage::Window, self.timing().window_close, cancel)?;
        Ok(())
    }

    // Whether the client comes back after it exits, as it does for an update of itself. The watch is cut short the moment the log gains the removal of the lock file, whenever that line lands, and from the start after the client was ended: an ended client has no updater left to bring it back.
    fn watch_for_restart(
        &self,
        mark: &LogMark,
        ended: bool,
        cancel: &CancelToken,
    ) -> Result<bool, AdapterError> {
        let timing = self.timing();
        let started = self.clock.now();
        let mut short = ended;
        loop {
            if cancel.is_cancelled() {
                return Err(AdapterError::Aborted);
            }
            if self.is_running()? {
                return Ok(true);
            }
            short = short || exited_cleanly(&self.events_since(mark));
            let watch = if short {
                timing.clean_exit_watch
            } else {
                timing.restart_watch
            };
            if self.clock.now() - started >= watch {
                return Ok(false);
            }
            self.clock.sleep(timing.poll, cancel);
        }
    }

    // The look a write of the session file rests on, both answers from one listing: a Riot game that runs is an error, and so is a listing that cannot be read; otherwise whether the client runs.
    pub fn client_runs_beside_no_game(&self, action: &str) -> Result<bool, AdapterError> {
        let names = self.processes.names()?;
        if let Some(game) = self.find_running_game(Some(&names))? {
            return Err(Self::game_running_error(game, action));
        }
        Ok(names.contains(&image_key(EXE)))
    }

    fn no_game_runs(&self) -> Result<(), AdapterError> {
        match self.find_running_game(None)? {
            Some(game) => Err(Self::game_running_error(game, "try again")),
            None => Ok(()),
        }
    }

    // Closes the client's own processes and nothing else: a window close for the client, which is ended alone when it ignores the close, then the same for the window processes it leaves behind. A client that comes back by itself is given time to finish starting and is closed again, three times at most. A Riot game is looked for directly before every close and every end, the later ones included, because a game can start while a close is waited for: beside a game nothing is closed or ended.
    pub fn shutdown(
        &self,
        timeout: Option<Duration>,
        cancel: &CancelToken,
    ) -> Result<(), AdapterError> {
        self.require_install()?;
        if !self.is_running()? {
            self.no_game_runs()?;
            return self.close_windows(cancel);
        }
        let timing = self.timing();
        for attempt in 1.. {
            let mark = self.log_mark();
            let ended = self.close_and_wait(
                ClientImage::Core,
                timeout.unwrap_or(timing.shutdown),
                cancel,
            )?;
            self.close_windows(cancel)?;
            if !self.watch_for_restart(&mark, ended, cancel)? {
                return Ok(());
            }
            if attempt >= 3 {
                break;
            }
            let started_again = || {
                Ok::<_, AdapterError>(
                    self.log_mark().file != mark.file
                        && startup_finished(&self.current_run_events()),
                )
            };
            wait_until(
                self.clock,
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

    // Starts the client the way its own shortcut does: with no arguments, or with a game's two launch arguments. Nothing else is ever put on its command line.
    pub fn start_client(
        &self,
        install: &LauncherInstall,
        game: Option<&RiotProductId>,
    ) -> Result<(), AdapterError> {
        let args = game.map(|game| launch_args(game).to_vec());
        self.processes.start(
            &install.exe_path,
            &args.unwrap_or_default(),
            &join(&[&install.exe_path, ".."]),
        )
    }

    // Presses Play for the user: the launch request the client's own Play button sends to its local server, then the same watch as for a start the client made on its own. Never the reason a launch fails: a request the client refused or did not act on leaves the hand-over as it was, with what happened.
    fn press_play(
        &self,
        channel: &dyn RiotLocalLaunch,
        id: &RiotProductId,
        read: &dyn Fn() -> Vec<LogEvent>,
        cancel: &CancelToken,
    ) -> Result<PlayOutcome, AdapterError> {
        let timing = self.timing();
        let sent = channel.launch(&RiotLaunchRequest {
            product: id.product(),
            patchline: id.patchline(),
            lockfile_path: &self.lockfile_path(),
            cancel,
        });
        if let Err(refused) = sent {
            return Ok(if cancel.is_cancelled() {
                PlayOutcome::Stopped
            } else {
                PlayOutcome::Declined(format!("the launch request was not taken: {refused}"))
            });
        }
        let key = id.key();
        let mut out_of_date = false;
        let mut game = GameWatch {
            riot: self,
            images: game_images_of(id.product()),
            looked: None,
        };
        let waited = wait_until(
            self.clock,
            cancel,
            timing.play_confirm,
            timing.log_poll,
            || {
                let events = read();
                if products_needing_update(&events).contains(&key) {
                    out_of_date = true;
                    return Ok(true);
                }
                Ok::<_, AdapterError>(
                    game_started(&events, id.product(), id.patchline()) || game.is_up()?,
                )
            },
        );
        match waited {
            Err(AdapterError::Aborted) => Ok(PlayOutcome::Stopped),
            Err(other) => Err(other),
            Ok(_) if out_of_date => Ok(PlayOutcome::OutOfDate),
            Ok(true) => Ok(PlayOutcome::Started),
            Ok(false) => Ok(PlayOutcome::Silent(format!(
                "the Riot Client took the launch request but did not start the game within {} s",
                seconds(timing.play_confirm)
            ))),
        }
    }

    // Hands the game to the client the way the game's own shortcut does: RiotClientServices.exe with --launch-product and --launch-patchline, to the running client or to start it. When the switch has just started the client with these arguments and the run's log shows the product, nothing is sent. Riot has turned auto-launch off, so the client opens the game's page and waits for Play: once the page is up Play is pressed for the user when the setting is on, and otherwise, or when that did not take, the launch hands over to the user. Refused before anything is touched while a Riot game runs and when the client's newest log says the product needs an update.
    pub fn launch_game(
        &self,
        launcher_game_id: &str,
        name: &str,
        started_by_switch: bool,
        cancel: &CancelToken,
    ) -> Result<Launched, AdapterError> {
        let id = parse_product_id(launcher_game_id).ok_or_else(|| {
            AdapterError::Failed(format!("Not a Riot product id: {launcher_game_id}"))
        })?;
        let install = self.require_install()?;
        let names = self.processes.names()?;
        if let Some(running) = self.find_running_game(Some(&names))? {
            let same = running.launcher_game_id.split('.').next() == Some(id.product());
            return Err(if same {
                AdapterError::GameRunning {
                    message: format!("{} is already running.", running.name),
                    game: running,
                }
            } else {
                Self::game_running_error(running, "press Play again")
            });
        }
        let key = id.key();
        let (product, patchline) = (id.product(), id.patchline());
        let needs_update = || {
            AdapterError::GameNotPlayable(format!(
                "{name} needs an update before it can start. Update it in the Riot Client window, then press Play again."
            ))
        };
        if products_needing_update(&self.current_run_events()).contains(&key) {
            return Err(needs_update());
        }

        let timing = self.timing();
        let command = format!("\"{}\" {}", install.exe_path, launch_args(&id).join(" "));
        let client_running = names.contains(&image_key(EXE));
        let underway = started_by_switch
            && client_running
            && launch_accepted(&self.current_run_events(), product, patchline);
        let how = if underway {
            "the switch started the client with these arguments; "
        } else {
            ""
        };
        let mark = self.log_mark();
        let read = || {
            if underway {
                self.current_run_events()
            } else {
                self.events_since(&mark)
            }
        };
        let channel = self.local_launch.filter(|_| self.config.press_play);
        if !underway {
            self.start_client(&install, Some(&id))?;
        }

        let (mut out_of_date, mut page_shown) = (false, false);
        let mut login_page = None;
        let mut game = GameWatch {
            riot: self,
            images: game_images_of(product),
            looked: None,
        };
        let waited = wait_until(
            self.clock,
            cancel,
            timing.launch_confirm,
            timing.log_poll,
            || {
                let events = read();
                if products_needing_update(&events).contains(&key) {
                    out_of_date = true;
                    return Ok(true);
                }
                if game_started(&events, product, patchline) || game.is_up()? {
                    return Ok(true);
                }
                if launch_page_shown(&events, product, patchline) {
                    page_shown = true;
                    return Ok(true);
                }
                if let Some(SignInOutcome::Failed { detail, .. }) = last_sign_in(&events) {
                    login_page = Some(detail);
                    return Ok(true);
                }
                Ok::<_, AdapterError>(false)
            },
        );
        let (seen, stopped) = match waited {
            Ok(seen) => (seen, false),
            Err(AdapterError::Aborted) => (false, true),
            Err(other) => return Err(other),
        };
        let events = read();
        let accepted = launch_accepted(&events, product, patchline);
        let declined = launch_declined(&events, product, patchline);
        let play = match channel {
            Some(channel) if page_shown => Some(self.press_play(channel, &id, &read, cancel)?),
            _ => None,
        };
        if out_of_date || matches!(play, Some(PlayOutcome::OutOfDate)) {
            return Err(needs_update());
        }
        if let Some(detail) = login_page {
            return Err(AdapterError::Failed(format!(
                "The Riot Client opened on its login page instead of signing in ({detail}), so {name} was not started. Sign in there with “Stay signed in” ticked and press Play; One Launch saves the sign-in at the next switch."
            )));
        }
        let said = |what: &str| format!("{command} ({how}{what})");
        if matches!(play, Some(PlayOutcome::Started)) {
            return Ok(Launched::started(said(
                "One Launch pressed Play and the Riot Client started the game",
            )));
        }
        if stopped || matches!(play, Some(PlayOutcome::Stopped)) {
            return Ok(Launched::sent(said("stopped watching for the game start")));
        }
        if page_shown {
            let why = declined
                .map(|reason| format!(": {reason}"))
                .unwrap_or_default();
            let pressed = match &play {
                Some(PlayOutcome::Declined(reason) | PlayOutcome::Silent(reason)) => {
                    format!("; {reason}")
                }
                _ => String::new(),
            };
            let hand_off = if matches!(play, Some(PlayOutcome::Silent(_))) {
                format!(
                    "One Launch pressed Play, but {name} has not started yet. Look at the Riot Client window: it may be updating the game or something it needs first."
                )
            } else {
                format!(
                    "Press Play in the Riot Client window to start {name}; it is open on the game's page."
                )
            };
            return Ok(Launched {
                hand_off: Some(hand_off),
                ..Launched::sent(said(&format!(
                    "the Riot Client opened the game's page{why}{pressed}"
                )))
            });
        }
        if seen {
            return Ok(Launched::started(said("the Riot Client started the game")));
        }
        let waited_for = seconds(timing.launch_confirm);
        if declined.as_deref() == Some(NOT_UP_TO_DATE) {
            return Err(AdapterError::Failed(format!(
                "{name} did not start within {waited_for} s: the Riot Client is still updating it or something it needs. When the Riot Client has finished, press Play there."
            )));
        }
        Err(AdapterError::Failed(if accepted {
            format!(
                "{name} did not start within {waited_for} s. Look at the Riot Client window: the game may need an update or a repair, or the client may be waiting for you."
            )
        } else {
            format!(
                "The Riot Client did not act on the launch within {waited_for} s. Look at the Riot Client window: it may be updating itself or asking you to sign in."
            )
        }))
    }

    // Whether the session file, as it is now, holds a sign-in that no One Launch account owns. Only means something while the client is closed. A file that cannot be read counts as holding none.
    pub fn holds_unowned_session(&self) -> bool {
        self.current_session().is_ok_and(|current| {
            let owned = current.owner.is_some_and(|owner| owner.id.is_some());
            current.session.is_some() && !owned
        })
    }
}
