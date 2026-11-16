// What stands between the app and its launcher adapters: the machine they run on, one adapter for the launcher a call names, the error every adapter failure is wrapped in, the accounts a launcher remembers and the library scan.

use std::fmt;
use std::path::Path;
use std::sync::Arc;

use ol_adapters::epic::{self, EpicAdapter, EpicConfig};
use ol_adapters::steam::{self, SteamAdapter, SteamConfig, SwitchPreflight, Timing};
use ol_adapters::{AccountRef, AdapterError, Directory};
use ol_formats::epic::manifests::LaunchUriForm;
use ol_model::{
    Account, InstalledGame, LauncherId, LauncherSessionState, LibraryScanResult, RememberedAccount,
    SettingKey, Settings, SwitchPlan,
};
use ol_platform_api::Platform;
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

// The machine the adapters run on and the vault that keeps passwords and saved sign-ins. A build with fake launchers hands in fakes; nothing else in the app can tell.
#[derive(Clone)]
pub struct Launchers {
    pub platform: Platform,
    pub vault: Arc<dyn Vault + Send + Sync>,
    pub steam_timing: Timing,
    // Where Steam is looked for when neither the settings nor the registry name a folder.
    pub steam_default_dir: String,
    pub epic: EpicMachine,
}

// What the settings say to each launcher's adapter, read once for the call in hand.
#[derive(Clone)]
pub struct Configs {
    pub steam: SteamConfig,
    pub epic: EpicConfig,
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
}

impl Adapter<'_> {
    pub const fn launcher(&self) -> LauncherId {
        match self {
            Self::Steam(_) => LauncherId::Steam,
            Self::Epic(_) => LauncherId::Epic,
        }
    }

    pub fn installed_games(&self) -> Result<Vec<InstalledGame>, AdapterError> {
        match self {
            Self::Steam(steam) => steam.list_installed_games(),
            Self::Epic(epic) => epic.list_installed_games(),
        }
    }

    pub fn remembered_accounts(&self) -> Result<Vec<RememberedAccount>, AdapterError> {
        match self {
            Self::Steam(steam) => steam.list_remembered_accounts(),
            Self::Epic(epic) => epic.list_remembered_accounts(),
        }
    }

    pub fn preflight(&self, account: &AccountRef) -> Result<SwitchPreflight, AdapterError> {
        match self {
            Self::Steam(steam) => steam.preflight(account),
            Self::Epic(epic) => epic.preflight(account),
        }
    }

    pub fn plan_switch(&self, account: &AccountRef) -> Result<SwitchPlan, AdapterError> {
        match self {
            Self::Steam(steam) => steam.plan_switch(account),
            Self::Epic(epic) => epic.plan_switch(account),
        }
    }

    // Starts a game, with the right account signed in already. Returns what was sent.
    pub fn launch_game(&self, launcher_game_id: &str) -> Result<String, AdapterError> {
        match self {
            Self::Steam(steam) => steam.launch_game(launcher_game_id),
            Self::Epic(epic) => epic.launch_game(launcher_game_id),
        }
    }

    // What the launcher's own files say about the session it remembers. None for a launcher that keeps a list of accounts instead.
    pub fn session_state(&self) -> Option<Result<LauncherSessionState, AdapterError>> {
        match self {
            Self::Steam(_) => None,
            Self::Epic(epic) => Some(epic.session_state()),
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
        }
    }

    // The launchers this build can scan, switch and start, in the order they are scanned.
    pub const IMPLEMENTED: [LauncherId; 2] = [LauncherId::Steam, LauncherId::Epic];

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
                backups_dir,
                timing: self.epic.timing,
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
            other => Err(ServiceError::NotImplemented(other)),
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

// Reads every launcher this build knows again and brings the games table in line. A launcher that is not installed counts as having no games; any other failure is reported on its entry and leaves its rows alone.
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
                .and_then(|adapter| installed_games(&adapter));
            match games {
                Ok(games) => sync(&games, true),
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
