// What stands between the app and its launcher adapters: the machine they run on, one adapter for the launcher a call names, the error every adapter failure is wrapped in, the accounts a launcher remembers and the library scan.

use std::fmt;
use std::path::Path;
use std::sync::Arc;

use ol_adapters::battlenet::{self, BattlenetAdapter, BattlenetConfig};
use ol_adapters::epic::{self, EpicAdapter, EpicConfig};
use ol_adapters::health::Known;
use ol_adapters::riot::{self, Launched, RiotAdapter, RiotConfig};
use ol_adapters::steam::{self, SteamAdapter, SteamConfig, SwitchPreflight, Timing};
use ol_adapters::{AccountRef, AdapterError, Directory, GameRef};
use ol_formats::battlenet::products::LaunchForm;
use ol_formats::epic::manifests::LaunchUriForm;
use ol_model::{
    Account, HealthCheck, HealthReport, HealthStatus, InstalledGame, JobGame, LauncherId,
    LauncherInstall, LauncherSessionState, LauncherStatus, LibraryScanResult, RememberedAccount,
    SettingKey, Settings, SwitchPlan,
};
use ol_platform_api::{CancelToken, Platform, RiotLocalLaunch};
use ol_store::{Store, StoreError};
use ol_vault::Vault;
use thiserror::Error;
use uuid::Uuid;

use crate::accounts::{self, AccountError, NewAccount};
use crate::backup::{BackupError, BackupService, DEFAULT_BACKUPS_TO_KEEP};
use crate::sessions::StoreDirectory;

// Where the Epic Games Launcher keeps its files on this PC, and how long a switch waits for it.
#[derive(Clone)]
pub struct EpicMachine {
    pub timing: epic::Timing,
    pub default_dir: String,
    // %LOCALAPPDATA% and %ProgramData%.
    pub local_app_data: String,
    pub program_data: String,
}

// The two folders where Windows has them when it names none, which is what the Electron app falls back to. Whoever builds the machine puts the real ones in: the environment is read by ol-platform-win, not here.
impl Default for EpicMachine {
    fn default() -> Self {
        Self {
            timing: epic::Timing::default(),
            default_dir: epic::DEFAULT_DIR.to_owned(),
            local_app_data: r"C:\AppData\Local".to_owned(),
            program_data: r"C:\ProgramData".to_owned(),
        }
    }
}

// Where Battle.net keeps its files on this PC, and how long a switch waits for it.
#[derive(Clone)]
pub struct BattlenetMachine {
    pub timing: battlenet::Timing,
    pub default_dir: String,
    // %APPDATA%, %LOCALAPPDATA% and %ProgramData%.
    pub app_data: String,
    pub local_app_data: String,
    pub program_data: String,
}

// As for Epic: the folders where Windows has them when it names none, until whoever builds the machine puts the real ones in.
impl Default for BattlenetMachine {
    fn default() -> Self {
        Self {
            timing: battlenet::Timing::default(),
            default_dir: battlenet::DEFAULT_DIR.to_owned(),
            app_data: r"C:\AppData\Roaming".to_owned(),
            local_app_data: r"C:\AppData\Local".to_owned(),
            program_data: r"C:\ProgramData".to_owned(),
        }
    }
}

// Where the Riot Client keeps its files on this PC, how long a switch waits for it, and the way to its own local server.
#[derive(Clone)]
pub struct RiotMachine {
    pub timing: riot::Timing,
    pub default_dir: String,
    // %LOCALAPPDATA% and %ProgramData%.
    pub local_app_data: String,
    pub program_data: String,
    // Sends the client's local server the launch request its Play button sends. None on a machine that has no such way: Play is then left to the user. Nothing reads it but `Launchers::adapter`, which hands it to the Riot adapter.
    local_launch: Option<Arc<dyn RiotLocalLaunch>>,
}

impl RiotMachine {
    pub fn at(
        timing: riot::Timing,
        default_dir: String,
        local_app_data: String,
        program_data: String,
    ) -> Self {
        Self {
            timing,
            default_dir,
            local_app_data,
            program_data,
            local_launch: None,
        }
    }

    // The same machine with a way to the client's own local server.
    pub fn with_local_launch(mut self, channel: Arc<dyn RiotLocalLaunch>) -> Self {
        self.local_launch = Some(channel);
        self
    }
}

// As for Epic: the folders where Windows has them when it names none, until whoever builds the machine puts the real ones in.
impl Default for RiotMachine {
    fn default() -> Self {
        Self::at(
            riot::Timing::default(),
            riot::DEFAULT_DIR.to_owned(),
            r"C:\AppData\Local".to_owned(),
            r"C:\ProgramData".to_owned(),
        )
    }
}

// The machine the adapters run on and the vault that keeps passwords and saved sign-ins. A build with fake launchers hands in fakes; nothing else in the app can tell.
#[derive(Clone)]
pub struct Launchers {
    pub platform: Platform,
    pub vault: Arc<dyn Vault + Send + Sync>,
    pub steam_timing: Timing,
    // Where Steam is looked for when neither the settings nor the registry name a folder.
    pub steam_default_dir: String,
    pub epic: EpicMachine,
    pub battlenet: BattlenetMachine,
    pub riot: RiotMachine,
}

// What the settings say to each launcher's adapter, read once for the call in hand.
#[derive(Clone)]
pub struct Configs {
    pub steam: SteamConfig,
    pub epic: EpicConfig,
    pub battlenet: BattlenetConfig,
    pub riot: RiotConfig,
}

impl Configs {
    pub const fn backups_to_keep(&self) -> i64 {
        self.steam.backups_to_keep
    }
}

// The adapter of the launcher a call is for.
pub enum Adapter<'a> {
    Steam(SteamAdapter<'a>),
    Epic(EpicAdapter<'a>),
    Battlenet(BattlenetAdapter<'a>),
    Riot(RiotAdapter<'a>),
}

impl Adapter<'_> {
    pub const fn launcher(&self) -> LauncherId {
        match self {
            Self::Steam(_) => LauncherId::Steam,
            Self::Epic(_) => LauncherId::Epic,
            Self::Battlenet(_) => LauncherId::Battlenet,
            Self::Riot(_) => LauncherId::Riot,
        }
    }

    pub fn installed_games(&self) -> Result<Vec<InstalledGame>, AdapterError> {
        match self {
            Self::Steam(steam) => steam.list_installed_games(),
            Self::Epic(epic) => epic.list_installed_games(),
            Self::Battlenet(battlenet) => battlenet.list_installed_games(),
            Self::Riot(riot) => riot.list_installed_games(),
        }
    }

    // The games, and why the list can be short of some that are installed.
    fn scanned_games(&self) -> Result<(Vec<InstalledGame>, Option<String>), AdapterError> {
        match self {
            Self::Steam(steam) => steam.scan_installed_games(),
            Self::Epic(epic) => Ok((epic.list_installed_games()?, None)),
            Self::Battlenet(battlenet) => Ok((battlenet.list_installed_games()?, None)),
            Self::Riot(riot) => riot.scan_installed_games(),
        }
    }

    pub fn remembered_accounts(&self) -> Result<Vec<RememberedAccount>, AdapterError> {
        match self {
            Self::Steam(steam) => steam.list_remembered_accounts(),
            Self::Epic(epic) => epic.list_remembered_accounts(),
            Self::Battlenet(battlenet) => battlenet.list_remembered_accounts(),
            Self::Riot(riot) => riot.list_remembered_accounts(),
        }
    }

    // `game` is the game a launch follows the switch with, which changes how Riot's client is started.
    pub fn preflight(
        &self,
        account: &AccountRef,
        game: Option<GameRef<'_>>,
    ) -> Result<SwitchPreflight, AdapterError> {
        match self {
            Self::Steam(steam) => steam.preflight(account),
            Self::Epic(epic) => epic.preflight(account),
            Self::Battlenet(battlenet) => battlenet.preflight(account),
            Self::Riot(riot) => riot.preflight(account, game),
        }
    }

    pub fn plan_switch(&self, account: &AccountRef) -> Result<SwitchPlan, AdapterError> {
        match self {
            Self::Steam(steam) => steam.plan_switch(account),
            Self::Epic(epic) => epic.plan_switch(account),
            Self::Battlenet(battlenet) => battlenet.plan_switch(account),
            Self::Riot(riot) => riot.plan_switch(account),
        }
    }

    // Starts a game, with the right account signed in already. Returns what was sent, and what is left for the user to do when the launcher only shows the game's page. A launcher that is watched for the game's start stops watching when `cancel` is set. `started_by_switch` says that the switch has just started the launcher for this game.
    pub fn launch_game(
        &self,
        game: &JobGame,
        started_by_switch: bool,
        cancel: &CancelToken,
    ) -> Result<Launched, AdapterError> {
        let sent = |command: String| Launched {
            command,
            hand_off: None,
        };
        match self {
            Self::Steam(steam) => steam.launch_game(&game.launcher_game_id).map(sent),
            Self::Epic(epic) => epic.launch_game(&game.launcher_game_id).map(sent),
            Self::Battlenet(battlenet) => battlenet
                .launch_game(&game.launcher_game_id, &game.name, cancel)
                .map(sent),
            Self::Riot(riot) => riot.launch_game(
                &game.launcher_game_id,
                &game.name,
                started_by_switch,
                cancel,
            ),
        }
    }

    pub fn chosen_install_dir(&self) -> Option<String> {
        match self {
            Self::Steam(steam) => steam.chosen_install_dir(),
            Self::Epic(epic) => epic.chosen_install_dir(),
            Self::Battlenet(battlenet) => battlenet.chosen_install_dir(),
            Self::Riot(riot) => riot.chosen_install_dir(),
        }
    }

    pub fn detect_install(&self) -> Result<Option<LauncherInstall>, AdapterError> {
        match self {
            Self::Steam(steam) => steam.detect_install(),
            Self::Epic(epic) => epic.detect_install(),
            Self::Battlenet(battlenet) => battlenet.detect_install(),
            Self::Riot(riot) => riot.detect_install(),
        }
    }

    pub fn is_running(&self) -> Result<bool, AdapterError> {
        match self {
            Self::Steam(steam) => steam.is_running(),
            Self::Epic(epic) => epic.is_running(),
            Self::Battlenet(battlenet) => battlenet.is_running(),
            Self::Riot(riot) => riot.is_running(),
        }
    }

    // The install a program the user picked belongs to, when it is the launcher's own and is there. Reads only.
    pub fn install_from_exe(&self, exe: &str) -> Option<LauncherInstall> {
        match self {
            Self::Steam(steam) => steam.install_from_exe(exe),
            Self::Epic(epic) => epic.install_from_exe(exe),
            Self::Battlenet(battlenet) => battlenet.install_from_exe(exe),
            Self::Riot(riot) => riot.install_from_exe(exe),
        }
    }

    pub fn health_checks(&self, known: &Known) -> Vec<HealthCheck> {
        match self {
            Self::Steam(steam) => steam.health_checks(known),
            Self::Epic(epic) => epic.health_checks(known),
            Self::Battlenet(battlenet) => battlenet.health_checks(known),
            Self::Riot(riot) => riot.health_checks(known),
        }
    }

    // What the launcher's own files say about the session it remembers. None for a launcher that keeps a list of accounts instead.
    pub fn session_state(&self) -> Option<Result<LauncherSessionState, AdapterError>> {
        match self {
            Self::Steam(_) => None,
            Self::Epic(epic) => Some(epic.session_state()),
            Self::Battlenet(battlenet) => Some(battlenet.session_state()),
            Self::Riot(riot) => Some(riot.session_state()),
        }
    }
}

impl Launchers {
    pub fn on(platform: Platform, vault: Arc<dyn Vault + Send + Sync>) -> Self {
        Self {
            platform,
            vault,
            steam_timing: Timing::default(),
            steam_default_dir: steam::DEFAULT_DIR.to_owned(),
            epic: EpicMachine::default(),
            battlenet: BattlenetMachine::default(),
            riot: RiotMachine::default(),
        }
    }

    // The launchers this build can scan, switch and start, in the order they are scanned.
    pub const IMPLEMENTED: [LauncherId; 4] = LauncherId::ALL;

    pub fn configs(&self, settings: &Settings, data_dir: &Path) -> Configs {
        let backups_to_keep = settings.get_int(SettingKey::BackupsKeep, DEFAULT_BACKUPS_TO_KEEP);
        let backups_dir = data_dir.join("backups").to_string_lossy().into_owned();
        Configs {
            steam: SteamConfig {
                chosen_dir: settings.get(SettingKey::SteamPath).map(str::to_owned),
                default_dir: self.steam_default_dir.clone(),
                auto_login: settings.get_bool(SettingKey::SteamAutoLogin, false),
                backups_to_keep,
                backups_dir: backups_dir.clone(),
                timing: self.steam_timing,
            },
            epic: EpicConfig {
                chosen_dir: settings.get(SettingKey::EpicPath).map(str::to_owned),
                default_dir: self.epic.default_dir.clone(),
                local_app_data: self.epic.local_app_data.clone(),
                program_data: self.epic.program_data.clone(),
                launch_uri_form: if settings.get(SettingKey::EpicLaunchUri) == Some("appname") {
                    LaunchUriForm::AppName
                } else {
                    LaunchUriForm::Catalog
                },
                backups_to_keep,
                backups_dir: backups_dir.clone(),
                timing: self.epic.timing,
            },
            battlenet: BattlenetConfig {
                chosen_dir: settings.get(SettingKey::BattlenetPath).map(str::to_owned),
                default_dir: self.battlenet.default_dir.clone(),
                app_data: self.battlenet.app_data.clone(),
                local_app_data: self.battlenet.local_app_data.clone(),
                program_data: self.battlenet.program_data.clone(),
                launch_form: if settings.get(SettingKey::BattlenetLaunch) == Some("uri") {
                    LaunchForm::Uri
                } else {
                    LaunchForm::Exec
                },
                backups_to_keep,
                backups_dir: backups_dir.clone(),
                timing: self.battlenet.timing,
            },
            riot: RiotConfig {
                chosen_dir: settings.get(SettingKey::RiotPath).map(str::to_owned),
                default_dir: self.riot.default_dir.clone(),
                local_app_data: self.riot.local_app_data.clone(),
                program_data: self.riot.program_data.clone(),
                press_play: settings.get_bool(SettingKey::RiotPressPlay, true),
                backups_to_keep,
                backups_dir,
                timing: self.riot.timing,
            },
        }
    }

    // `directory` answers the adapter's questions about One Launch's own accounts of that launcher.
    pub fn adapter<'a>(
        &'a self,
        launcher: LauncherId,
        configs: &'a Configs,
        directory: &'a dyn Directory,
    ) -> Result<Adapter<'a>, ServiceError> {
        match launcher {
            LauncherId::Steam => Ok(Adapter::Steam(SteamAdapter::new(
                &self.platform,
                &configs.steam,
            ))),
            LauncherId::Epic => Ok(Adapter::Epic(EpicAdapter::new(
                &self.platform,
                &configs.epic,
                directory,
            ))),
            LauncherId::Battlenet => Ok(Adapter::Battlenet(BattlenetAdapter::new(
                &self.platform,
                &configs.battlenet,
                directory,
            ))),
            LauncherId::Riot => Ok(Adapter::Riot(RiotAdapter::new(
                &self.platform,
                &configs.riot,
                directory,
                self.riot.local_launch.as_deref(),
            ))),
        }
    }

    pub fn directory<'a>(&'a self, store: &'a Store, launcher: LauncherId) -> StoreDirectory<'a> {
        StoreDirectory {
            store,
            vault: self.vault.as_ref(),
            clock: self.platform.clock.as_ref(),
            launcher,
        }
    }

    pub fn backups<'a>(&'a self, data_dir: &Path, configs: &Configs) -> BackupService<'a> {
        BackupService::new(
            data_dir,
            configs.backups_to_keep(),
            self.platform.files.as_ref(),
            self.platform.clock.as_ref(),
            self.vault.as_ref(),
        )
    }
}

#[derive(Error)]
pub enum ServiceError {
    // An adapter's failure, with the launcher named in front as the user reads it.
    #[error("{}: {source}", launcher.display_name())]
    Adapter {
        launcher: LauncherId,
        source: AdapterError,
    },
    #[error("{}: {source}", launcher.display_name())]
    Backup {
        launcher: LauncherId,
        source: BackupError,
    },
    #[error("The {} adapter is not available yet", .0.display_name())]
    NotImplemented(LauncherId),
    #[error(transparent)]
    Account(#[from] AccountError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("Game {0} does not exist")]
    GameNotFound(String),
    #[error("{0}")]
    Mismatch(String),
    #[error("Another switch or launch is still running")]
    Busy,
    #[error(
        "Preview mode is on. Turn it off in Settings, or from the preview, to switch accounts for real."
    )]
    DryRun,
    #[error("No launch job {0}")]
    JobNotFound(String),
    #[error("That snapshot no longer exists")]
    SnapshotGone,
    #[error("There is no interrupted switch to recover")]
    NothingToRecover,
    #[error("That file is not {}. Choose {} in the folder {} is installed in.", .0.display_name(), .0.exe_name(), .0.display_name())]
    NotALauncher(LauncherId),
    #[error(r"Choose the program by its full path on a lettered drive, such as C:\Program Files\{}.", .0.exe_name())]
    NotAPlainPath(LauncherId),
    #[error(
        "One Launch cannot read or store passwords and saved sign-ins right now, so it does not start this switch: it would close {} and stop halfway. Settings › Advanced › Encryption says why.",
        .0.display_name()
    )]
    VaultClosed(LauncherId),
}

impl ServiceError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Adapter { source, .. } => source.code(),
            Self::Backup { .. } => BackupError::CODE,
            Self::NotImplemented(_) => "NOT_IMPLEMENTED",
            Self::Account(error) => error.code(),
            Self::Store(error) => error.code(),
            Self::GameNotFound(_)
            | Self::JobNotFound(_)
            | Self::SnapshotGone
            | Self::NothingToRecover => "NOT_FOUND",
            Self::Mismatch(_) => "BAD_REQUEST",
            Self::Busy => "BUSY",
            Self::DryRun => "DRY_RUN",
            Self::NotALauncher(_) => "NOT_A_LAUNCHER",
            Self::NotAPlainPath(_) => "BAD_REQUEST",
            Self::VaultClosed(_) => "VAULT_UNAVAILABLE",
        }
    }

    pub fn adapter(launcher: LauncherId) -> impl Fn(AdapterError) -> Self {
        move |source| Self::Adapter { launcher, source }
    }

    pub fn backup(launcher: LauncherId) -> impl Fn(BackupError) -> Self {
        move |source| Self::Backup { launcher, source }
    }
}

// The message can name an account or a folder, so only the code is printed.
impl fmt::Debug for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ServiceError({})", self.code())
    }
}

pub fn require_implemented(launcher: LauncherId) -> Result<(), ServiceError> {
    if Launchers::IMPLEMENTED.contains(&launcher) {
        Ok(())
    } else {
        Err(ServiceError::NotImplemented(launcher))
    }
}

pub fn installed_games(adapter: &Adapter<'_>) -> Result<Vec<InstalledGame>, ServiceError> {
    adapter
        .installed_games()
        .map_err(ServiceError::adapter(adapter.launcher()))
}

// The accounts a launcher remembers, each with the One Launch account that goes with it: the one with the same launcher id, else the one with the same login.
pub fn remembered_accounts(
    store: &Store,
    adapter: &Adapter<'_>,
) -> Result<Vec<RememberedAccount>, ServiceError> {
    let launcher = adapter.launcher();
    let mut entries = adapter
        .remembered_accounts()
        .map_err(ServiceError::adapter(launcher))?;
    for entry in &mut entries {
        let mut linked = None;
        if !entry.launcher_uid.is_empty() {
            linked = store.account_by_launcher_uid(launcher, &entry.launcher_uid)?;
        }
        if linked.is_none() && !entry.login.is_empty() {
            linked = store.account_by_login(launcher, ol_formats::js::trim(&entry.login))?;
        }
        entry.linked_account_id = linked.map(|account| account.id);
    }
    Ok(entries)
}

// What names a remembered account: the launcher's id for it, or its login when the launcher keeps no id.
pub fn remembered_key(entry: &RememberedAccount) -> &str {
    if entry.launcher_uid.is_empty() {
        &entry.login
    } else {
        &entry.launcher_uid
    }
}

pub struct ImportResult {
    // Accounts made or newly linked by this import.
    pub accounts: Vec<Account>,
    pub skipped: Vec<(String, &'static str)>,
}

// Makes One Launch accounts for the remembered accounts named by `keys`: names and ids only, never a password. An account that already has the login gets the launcher's id filled in instead.
pub fn import_accounts(
    store: &Store,
    vault: &dyn Vault,
    adapter: &Adapter<'_>,
    keys: &[String],
    now: &str,
) -> Result<ImportResult, ServiceError> {
    let remembered = remembered_accounts(store, adapter)?;
    let mut result = ImportResult {
        accounts: Vec::new(),
        skipped: Vec::new(),
    };
    let mut seen = std::collections::HashSet::new();
    for key in keys.iter().filter(|key| seen.insert(key.as_str())) {
        let mut skip = |reason| result.skipped.push((key.clone(), reason));
        let Some(entry) = remembered
            .iter()
            .find(|entry| remembered_key(entry) == key.as_str())
        else {
            skip("not remembered by the launcher");
            continue;
        };
        match &entry.linked_account_id {
            None if entry.login.is_empty() => {
                skip("the launcher does not record the login for this account");
            }
            Some(linked) => {
                let unlinked = store
                    .account(linked)?
                    .filter(|account| account.launcher_uid.as_deref().is_none_or(str::is_empty));
                match unlinked {
                    Some(account) if !entry.launcher_uid.is_empty() => {
                        result.accounts.push(accounts::set_launcher_uid(
                            store,
                            &account.id,
                            Some(&entry.launcher_uid),
                            now,
                        )?);
                    }
                    _ => skip("already in One Launch"),
                }
            }
            None => {
                let display_name = if entry.persona_name.is_empty() {
                    &entry.login
                } else {
                    &entry.persona_name
                };
                let new = NewAccount {
                    launcher: entry.launcher,
                    display_name: display_name.clone(),
                    login: entry.login.clone(),
                    launcher_uid: Some(entry.launcher_uid.clone()).filter(|uid| !uid.is_empty()),
                    password: None,
                    tags: Vec::new(),
                    notes: String::new(),
                };
                result
                    .accounts
                    .push(accounts::create(store, vault, &new, now)?);
            }
        }
    }
    Ok(result)
}

// Reads every launcher this build knows again and brings the games table in line. A launcher that is not installed counts as having no games; any other failure, and a list that could not be read whole, is reported on its entry and leaves its rows alone.
pub fn scan_library(
    store: &Store,
    launchers: &Launchers,
    configs: &Configs,
    now: &str,
) -> Vec<LibraryScanResult> {
    Launchers::IMPLEMENTED
        .into_iter()
        .map(|launcher| {
            let sync = |games: &[InstalledGame], installed: bool| match store.sync_games(
                launcher,
                games,
                now,
                || Uuid::new_v4().to_string(),
            ) {
                Ok(sync) => LibraryScanResult {
                    launcher,
                    added: sync.added,
                    updated: sync.updated,
                    removed: sync.removed,
                    installed,
                    error: None,
                },
                Err(error) => failed(launcher, error.to_string()),
            };
            let directory = launchers.directory(store, launcher);
            let games = launchers
                .adapter(launcher, configs, &directory)
                .and_then(|adapter| {
                    adapter
                        .scanned_games()
                        .map_err(ServiceError::adapter(launcher))
                });
            match games {
                Ok((games, None)) => sync(&games, true),
                // A short list would take out every game it misses, so the rows stay until a scan can read it all.
                Ok((_, Some(unread))) => failed(
                    launcher,
                    format!("{unread}. Nothing in the Library was changed."),
                ),
                Err(error) if error.code() == "NOT_INSTALLED" => sync(&[], false),
                Err(error) => failed(launcher, error.to_string()),
            }
        })
        .collect()
}

fn failed(launcher: LauncherId, error: String) -> LibraryScanResult {
    LibraryScanResult {
        launcher,
        added: 0,
        updated: 0,
        removed: 0,
        installed: true,
        error: Some(error),
    }
}

// What the Launchers tab shows of one launcher: the folder chosen with Locate, the install found, whether it runs, and its health. Whatever fails on the way is the row's attention, and what was found before it stays.
pub fn status(
    store: &Store,
    launchers: &Launchers,
    configs: &Configs,
    data_dir: &Path,
    launcher: LauncherId,
) -> LauncherStatus {
    let mut status = LauncherStatus {
        implemented: true,
        ..LauncherStatus::unreachable(launcher)
    };
    let directory = launchers.directory(store, launcher);
    let adapter = match launchers.adapter(launcher, configs, &directory) {
        Ok(adapter) => adapter,
        Err(error) => {
            status.attention = Some(error.to_string());
            return status;
        }
    };
    status.chosen_path = adapter.chosen_install_dir();
    let pass = (|| {
        status.install = adapter.detect_install()?;
        if status.install.is_some() {
            status.running = Some(adapter.is_running()?);
        }
        Ok::<_, AdapterError>(())
    })();
    if let Err(error) = pass {
        status.attention = Some(error.to_string());
        return status;
    }
    let known = Known {
        install: status.install.clone(),
        running: status.running,
    };
    let mut checks = adapter.health_checks(&known);
    if status.install.is_some() {
        if launcher != LauncherId::Steam {
            checks.push(sessions_check(store, launcher));
        }
        checks.push(backups_check(launchers, configs, data_dir, launcher));
    }
    let checked_at = ol_store::iso_timestamp(launchers.platform.clock.now());
    let mut report = HealthReport::of(launcher, checks, checked_at);
    if status.install.is_none() {
        report.status = HealthStatus::Absent;
    }
    status.health = Some(report);
    status
}

fn status_if(warn: bool) -> HealthStatus {
    if warn {
        HealthStatus::Warn
    } else {
        HealthStatus::Ok
    }
}

fn sessions_check(store: &Store, launcher: LauncherId) -> HealthCheck {
    let check = |status, detail| HealthCheck {
        id: "sessions",
        label: "Session snapshots",
        status,
        detail,
    };
    match store.sessions_of(launcher) {
        Ok(sessions) => {
            let stale = sessions
                .iter()
                .filter(|session| session.stale_reason.is_some())
                .count();
            let tail = if stale > 0 {
                format!(", {stale} marked stale")
            } else {
                String::new()
            };
            check(
                status_if(stale > 0),
                format!("{} stored{tail}", steam::plural(sessions.len(), "snapshot")),
            )
        }
        Err(error) => check(
            HealthStatus::Warn,
            format!("Could not read the saved sign-ins: {error}"),
        ),
    }
}

fn backups_check(
    launchers: &Launchers,
    configs: &Configs,
    data_dir: &Path,
    launcher: LauncherId,
) -> HealthCheck {
    let check = |status, detail| HealthCheck {
        id: "backups",
        label: "Backups",
        status,
        detail,
    };
    match launchers.backups(data_dir, configs).list(launcher) {
        Ok(snapshots) if snapshots.is_empty() => {
            check(HealthStatus::Ok, "No snapshots yet".to_owned())
        }
        Ok(snapshots) => check(
            HealthStatus::Ok,
            format!(
                "{} available for restore",
                steam::plural(snapshots.len(), "snapshot")
            ),
        ),
        Err(error) => check(
            HealthStatus::Warn,
            format!("Could not list the snapshots: {error}"),
        ),
    }
}

// The install a program the user picked belongs to, or the refusal that names the program to pick. Reads only; storing the folder is the caller's.
pub fn locate(adapter: &Adapter<'_>, exe: &str) -> Result<LauncherInstall, ServiceError> {
    if !on_a_lettered_drive(exe) {
        return Err(ServiceError::NotAPlainPath(adapter.launcher()));
    }
    adapter
        .install_from_exe(exe)
        .ok_or(ServiceError::NotALauncher(adapter.launcher()))
}

// The folder found is stored and its program started later, from wherever the app then runs: a relative path, a share or a device path is refused, as for the app's own folders.
fn on_a_lettered_drive(exe: &str) -> bool {
    use std::path::{Component, Prefix};
    let mut parts = Path::new(exe).components();
    matches!(
        (parts.next(), parts.next()),
        (Some(Component::Prefix(prefix)), Some(Component::RootDir))
            if matches!(prefix.kind(), Prefix::Disk(_))
    )
}
