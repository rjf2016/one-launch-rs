// What stands between the app and its launcher adapters: the machine they run on, the error every adapter failure is wrapped in, the accounts a launcher remembers and the library scan.

use std::fmt;
use std::path::Path;
use std::sync::Arc;

use ol_adapters::AdapterError;
use ol_adapters::steam::{self, SteamAdapter, SteamConfig, Timing};
use ol_model::{
    Account, InstalledGame, LauncherId, LibraryScanResult, RememberedAccount, SettingKey, Settings,
};
use ol_platform_api::Platform;
use ol_store::{Store, StoreError};
use ol_vault::Vault;
use thiserror::Error;
use uuid::Uuid;

use crate::accounts::{self, AccountError, NewAccount};
use crate::backup::{BackupError, DEFAULT_BACKUPS_TO_KEEP};

// The machine the adapters run on and the vault that keeps passwords. A build with fake launchers hands in fakes; nothing else in the app can tell.
#[derive(Clone)]
pub struct Launchers {
    pub platform: Platform,
    pub vault: Arc<dyn Vault + Send + Sync>,
    pub steam_timing: Timing,
    // Where Steam is looked for when neither the settings nor the registry name a folder.
    pub steam_default_dir: String,
}

impl Launchers {
    pub fn on(platform: Platform, vault: Arc<dyn Vault + Send + Sync>) -> Self {
        Self {
            platform,
            vault,
            steam_timing: Timing::default(),
            steam_default_dir: steam::DEFAULT_DIR.to_owned(),
        }
    }

    // The launchers this build can scan, switch and start, in the order they are scanned.
    pub const IMPLEMENTED: [LauncherId; 1] = [LauncherId::Steam];

    pub fn steam_config(&self, settings: &Settings, data_dir: &Path) -> SteamConfig {
        SteamConfig {
            chosen_dir: settings.get(SettingKey::SteamPath).map(str::to_owned),
            default_dir: self.steam_default_dir.clone(),
            auto_login: settings.get_bool(SettingKey::SteamAutoLogin, false),
            backups_to_keep: settings.get_int(SettingKey::BackupsKeep, DEFAULT_BACKUPS_TO_KEEP),
            backups_dir: data_dir.join("backups").to_string_lossy().into_owned(),
            timing: self.steam_timing,
        }
    }

    pub fn steam<'a>(&'a self, config: &'a SteamConfig) -> SteamAdapter<'a> {
        SteamAdapter::new(&self.platform, config)
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

pub fn installed_games(steam: &SteamAdapter<'_>) -> Result<Vec<InstalledGame>, ServiceError> {
    steam
        .list_installed_games()
        .map_err(ServiceError::adapter(LauncherId::Steam))
}

// The accounts Steam remembers, each with the One Launch account that goes with it: the one with the same launcher id, else the one with the same login.
pub fn remembered_accounts(
    store: &Store,
    steam: &SteamAdapter<'_>,
) -> Result<Vec<RememberedAccount>, ServiceError> {
    let launcher = LauncherId::Steam;
    let mut entries = steam
        .list_remembered_accounts()
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
    steam: &SteamAdapter<'_>,
    keys: &[String],
    now: &str,
) -> Result<ImportResult, ServiceError> {
    let remembered = remembered_accounts(store, steam)?;
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
pub fn scan_library(store: &Store, steam: &SteamAdapter<'_>, now: &str) -> Vec<LibraryScanResult> {
    let launcher = LauncherId::Steam;
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
    let result = match installed_games(steam) {
        Ok(games) => sync(&games, true),
        Err(error) if error.code() == "NOT_INSTALLED" => sync(&[], false),
        Err(error) => failed(launcher, error.to_string()),
    };
    vec![result]
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
