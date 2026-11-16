// The only code in the app that writes a launcher-owned file or registry value: the thirteen functions from write_login_users to write_restored, each through a writer port. The four modules under switch/ read, wait and decide, and come here to write.

pub mod battlenet;
pub mod epic;
pub mod riot;
pub mod steam;

use std::collections::HashMap;

use ol_adapters::battlenet::BattlenetAdapter;
use ol_adapters::epic::{EpicAdapter, SETTINGS_FOLDERS};
use ol_adapters::riot::RiotAdapter;
use ol_adapters::steam::{HKCU_KEY, SteamAdapter};
use ol_adapters::{AccountRef, AdapterError, GameRef};
use ol_formats::battlenet::session::{TokenValue, UNIFIED_AUTH_KEY, describe_token};
use ol_model::{
    JobStep, JobStepStatus, LaunchAttention, LauncherId, PlanAccount, RestoreResult, SwitchPlan,
    SwitchStrategy,
};
use ol_platform_api::{
    CancelToken, FileWriter, Files, Hive, Platform, RegistryError, RegistryWriter,
};
use ol_runtime::Aborted;
use ol_vault::Secret;
use serde_json::{Map, Value};
use time::OffsetDateTime;

use crate::backup::{BackupError, BackupService, Restorable};
use crate::launchers::{Adapter, ServiceError};

const SNAPSHOT_RELATIVE: &str = "config/loginusers.vdf";
const AUTO_LOGIN_USER: &str = "AutoLoginUser";
const REMEMBER_PASSWORD: &str = "RememberPassword";

// Steam: loginusers.vdf with every AutoLogin or MostRecent line set for the target and nothing else changed.
fn write_login_users(files: &dyn FileWriter, path: &str, text: &str) -> Result<(), Thrown> {
    files
        .write_atomic(&[(path, text.as_bytes())])
        .map_err(|error| Thrown::Failed(error.to_string()))
}

// Steam: HKCU\Software\Valve\Steam\AutoLoginUser, the account Steam signs in at its next start.
fn set_auto_login_user(registry: &dyn RegistryWriter, account_name: &str) -> Result<(), Thrown> {
    Ok(registry.set_string(Hive::Hkcu, HKCU_KEY, AUTO_LOGIN_USER, account_name)?)
}

// Steam: HKCU\Software\Valve\Steam\RememberPassword = 1.
fn set_remember_password(registry: &dyn RegistryWriter) -> Result<(), Thrown> {
    Ok(registry.set_dword(Hive::Hkcu, HKCU_KEY, REMEMBER_PASSWORD, 1)?)
}

// Steam: puts a snapshot back. loginusers.vdf of the install in use now, then the two registry values as the snapshot recorded them, a value it recorded as absent being removed.
fn restore_steam(
    steam: &SteamAdapter<'_>,
    platform: &Platform,
    backups: &BackupService<'_>,
    id: &str,
) -> Result<RestoreResult, RestoreError> {
    let install = steam.require_install()?;
    let target = SteamAdapter::login_users_path(&install);
    let restorable = backups.read_for_restore(LauncherId::Steam, id, &[&target])?;
    write_restored(platform.files.as_ref(), &restorable)?;

    let registry: &dyn RegistryWriter = platform.registry.as_ref();
    let mut lines = Vec::new();
    if let Some(value) = restorable.meta.get(AUTO_LOGIN_USER) {
        if let Some(name) = value.as_str() {
            registry.set_string(Hive::Hkcu, HKCU_KEY, AUTO_LOGIN_USER, name)?;
            lines.push(format!("{AUTO_LOGIN_USER} = {name}"));
        } else {
            registry.delete_value(Hive::Hkcu, HKCU_KEY, AUTO_LOGIN_USER)?;
            lines.push(format!("{AUTO_LOGIN_USER} (removed)"));
        }
    }
    if let Some(value) = restorable.meta.get(REMEMBER_PASSWORD) {
        if let Some(number) = value.as_u64().and_then(|number| u32::try_from(number).ok()) {
            registry.set_dword(Hive::Hkcu, HKCU_KEY, REMEMBER_PASSWORD, number)?;
            lines.push(format!("{REMEMBER_PASSWORD} = {number}"));
        } else {
            registry.delete_value(Hive::Hkcu, HKCU_KEY, REMEMBER_PASSWORD)?;
            lines.push(format!("{REMEMBER_PASSWORD} (removed)"));
        }
    }
    Ok(RestoreResult {
        launcher: LauncherId::Steam,
        id: id.to_owned(),
        files: restorable.files.into_iter().map(|(path, _)| path).collect(),
        registry: lines,
    })
}

// Epic: GameUserSettings.ini with Enable and Data of [RememberMe] and Data of [Offline] set or removed, and every other byte as it was.
fn write_game_user_settings(files: &dyn FileWriter, path: &str, text: &str) -> Result<(), Thrown> {
    files
        .write_atomic(&[(path, text.as_bytes())])
        .map_err(|error| Thrown::Failed(error.to_string()))
}

// Epic: puts a snapshot back. Each GameUserSettings.ini the snapshot holds goes over the file it was copied from, which has to be one of the launcher's two; Epic keeps nothing of a sign-in in the registry.
fn restore_epic(
    epic: &EpicAdapter<'_>,
    platform: &Platform,
    backups: &BackupService<'_>,
    id: &str,
) -> Result<RestoreResult, RestoreError> {
    let targets = SETTINGS_FOLDERS.map(|folder| epic.settings_path_in(folder));
    let targets: Vec<&str> = targets.iter().map(String::as_str).collect();
    let restorable = backups.read_for_restore(LauncherId::Epic, id, &targets)?;
    write_restored(platform.files.as_ref(), &restorable)?;
    Ok(RestoreResult {
        launcher: LauncherId::Epic,
        id: id.to_owned(),
        files: restorable.files.into_iter().map(|(path, _)| path).collect(),
        registry: Vec::new(),
    })
}

// Battle.net: one value under HKCU\Software\Blizzard Entertainment\Battle.net\UnifiedAuth, the token the client signs in from at its next start.
fn set_unified_auth(
    registry: &dyn RegistryWriter,
    token: &TokenValue,
) -> Result<(), RegistryError> {
    registry.set_binary(Hive::Hkcu, UNIFIED_AUTH_KEY, &token.name, &token.hex)
}

// Battle.net: removes one value under UnifiedAuth, as the client's own logout does.
fn delete_unified_auth(registry: &dyn RegistryWriter, name: &str) -> Result<(), RegistryError> {
    registry.delete_value(Hive::Hkcu, UNIFIED_AUTH_KEY, name)
}

// Battle.net: Battle.net.config with the target login first in Client.SavedAccountNames and every other byte as it was.
fn write_battlenet_config(files: &dyn FileWriter, path: &str, text: &str) -> Result<(), Thrown> {
    files
        .write_atomic(&[(path, text.as_bytes())])
        .map_err(|error| Thrown::Failed(error.to_string()))
}

// Battle.net: puts a snapshot back. Battle.net.config goes over the client's own, then UnifiedAuth is made to hold exactly the values the snapshot recorded: each is set, and any other value there is removed. The recorded values are opened before anything is written, so a vault that cannot open them leaves everything as it is.
fn restore_battlenet(
    battlenet: &BattlenetAdapter<'_>,
    platform: &Platform,
    backups: &BackupService<'_>,
    id: &str,
) -> Result<RestoreResult, RestoreError> {
    let target = battlenet.config_path();
    let restorable = backups.read_for_restore(LauncherId::Battlenet, id, &[&target])?;
    let wanted = match restorable.meta.get(battlenet::UNIFIED_AUTH_META) {
        Some(recorded) => Some(battlenet::open_tokens(backups, recorded)?),
        None => None,
    };
    write_restored(platform.files.as_ref(), &restorable)?;

    let registry: &dyn RegistryWriter = platform.registry.as_ref();
    let mut lines = Vec::new();
    if let Some(wanted) = &wanted {
        let existing = battlenet.read_tokens()?;
        for token in wanted {
            set_unified_auth(registry, token)?;
            lines.push(format!(
                "UnifiedAuth\\{} = {}",
                token.name,
                describe_token(Some(token))
            ));
        }
        let kept = |name: &str| {
            wanted
                .iter()
                .any(|token| token.name.to_lowercase() == name.to_lowercase())
        };
        for token in existing.iter().filter(|token| !kept(&token.name)) {
            delete_unified_auth(registry, &token.name)?;
            lines.push(format!("UnifiedAuth\\{} (removed)", token.name));
        }
    }
    Ok(RestoreResult {
        launcher: LauncherId::Battlenet,
        id: id.to_owned(),
        files: restorable.files.into_iter().map(|(path, _)| path).collect(),
        registry: lines,
    })
}

// Riot: RiotGamesPrivateSettings.yaml, whole: the account's saved copy of the file as the client left it, the file with its session block set to null, or a snapshot's copy of it. It is the one launcher file a Riot switch writes, and it is never made where there is none: the client writes it at its first start, with the mark Riot knows this PC by. `ready` shows that the client was seen closed and that the sign-in the file held was handed to the store for its owner first.
fn write_riot_session(
    files: &dyn Files,
    path: &str,
    content: &[u8],
    _ready: &riot::ReadyToWrite,
) -> std::io::Result<()> {
    if !files.exists(path) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "{path} is missing. Start the Riot Client once by hand so it writes its settings file, then try again."
            ),
        ));
    }
    files.write_atomic(&[(path, content)])
}

// Riot: puts a snapshot back: its copy of the session file over the client's own, and nothing else. Riot keeps nothing of a sign-in in the registry.
fn restore_riot(
    riot: &RiotAdapter<'_>,
    platform: &Platform,
    backups: &BackupService<'_>,
    id: &str,
    ready: &riot::ReadyToWrite,
) -> Result<RestoreResult, RestoreError> {
    let target = riot.session_file_path();
    let restorable = backups.read_for_restore(LauncherId::Riot, id, &[&target])?;
    for (path, content) in &restorable.files {
        write_riot_session(platform.files.as_ref(), path, content, ready)
            .map_err(BackupError::Io)?;
    }
    Ok(RestoreResult {
        launcher: LauncherId::Riot,
        id: id.to_owned(),
        files: restorable.files.into_iter().map(|(path, _)| path).collect(),
        registry: Vec::new(),
    })
}

// The files of a snapshot, each over the launcher file it was copied from, all or none. Only the restore functions of Steam, Epic and Battle.net call it.
fn write_restored(files: &dyn FileWriter, restorable: &Restorable) -> Result<(), RestoreError> {
    let copies: Vec<(&str, &[u8])> = restorable
        .files
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
        .collect();
    Ok(files.write_atomic(&copies).map_err(BackupError::Io)?)
}

enum RestoreError {
    Adapter(AdapterError),
    Backup(BackupError),
}

impl From<AdapterError> for RestoreError {
    fn from(error: AdapterError) -> Self {
        Self::Adapter(error)
    }
}

impl From<BackupError> for RestoreError {
    fn from(error: BackupError) -> Self {
        Self::Backup(error)
    }
}

impl From<ol_platform_api::RegistryError> for RestoreError {
    fn from(error: ol_platform_api::RegistryError) -> Self {
        Self::Adapter(error.into())
    }
}

impl RestoreError {
    fn message(&self) -> String {
        match self {
            Self::Adapter(error) => error.to_string(),
            Self::Backup(error) => error.to_string(),
        }
    }

    fn into_service(self, launcher: LauncherId) -> ServiceError {
        match self {
            Self::Adapter(source) => ServiceError::adapter(launcher)(source),
            Self::Backup(source) => ServiceError::backup(launcher)(source),
        }
    }
}

fn snapshot_meta(auto_login_user: Option<&str>, remember: Option<u32>) -> Map<String, Value> {
    let mut meta = Map::new();
    meta.insert(AUTO_LOGIN_USER.to_owned(), auto_login_user.into());
    meta.insert(REMEMBER_PASSWORD.to_owned(), remember.into());
    meta
}

// What the caller of a switch hears while it runs, and the one thing the switch may ask for.
pub trait SwitchSink {
    // The whole step list, each time a step changes.
    fn progress(&mut self, steps: &[JobStep]);
    // The snapshot's id, once, before the first launcher file or registry value is written.
    fn backup(&mut self, backup_id: &str);
    // The switch starts or stops waiting for the user in the launcher's window.
    fn attention(&mut self, attention: Option<&LaunchAttention>);
    // The account's password, decrypted at the moment it is needed. None when none is stored.
    fn password(&mut self) -> Result<Option<Secret>, String>;
}

pub struct SwitchOptions<'a> {
    // The user has said that the launcher, and any game running under it, may be closed.
    pub confirm_close: bool,
    pub cancel: &'a CancelToken,
    // The game a launch follows the switch with. Only Riot's client is started differently for one.
    pub game: Option<GameRef<'a>>,
}

// No Debug: it names the account.
pub struct SwitchResult {
    pub account: PlanAccount,
    pub strategy: SwitchStrategy,
    pub ok: bool,
    // The launcher asked for a sign-in by hand and the wait for it ran out. Nothing is put back.
    pub needs_sign_in: bool,
    pub cancelled: bool,
    pub backup_id: Option<String>,
    // After a failure: whether the snapshot was put back. None when nothing had been changed.
    pub restored: Option<bool>,
    pub error: Option<String>,
    pub steps: Vec<JobStep>,
    pub started_at: String,
    pub finished_at: String,
    // From when the launcher the switch started takes a game. Only a job that goes on to a game waits for it.
    pub ready_at: Option<OffsetDateTime>,
    // The game whose launch arguments the launcher was started with, by the launcher's id for it: the launch that follows sends nothing more.
    pub started_with_game: Option<String>,
}

// Why a step of the switch stopped it.
enum Thrown {
    Aborted,
    Failed(String),
}

impl From<AdapterError> for Thrown {
    fn from(error: AdapterError) -> Self {
        match error {
            AdapterError::Aborted => Self::Aborted,
            other => Self::Failed(other.to_string()),
        }
    }
}

impl From<Aborted> for Thrown {
    fn from(_: Aborted) -> Self {
        Self::Aborted
    }
}

impl From<ol_platform_api::RegistryError> for Thrown {
    fn from(error: ol_platform_api::RegistryError) -> Self {
        Self::Failed(error.to_string())
    }
}

impl From<ol_platform_api::ProcessError> for Thrown {
    fn from(error: ol_platform_api::ProcessError) -> Self {
        Self::Failed(error.to_string())
    }
}

// Makes `account` the one its launcher signs in. A refusal before anything is touched is an error; from the first step on, whatever happens comes back as a result that says what was put back.
pub fn run_switch(
    adapter: &Adapter<'_>,
    platform: &Platform,
    backups: &BackupService<'_>,
    account: &AccountRef,
    options: &SwitchOptions<'_>,
    sink: &mut dyn SwitchSink,
) -> Result<SwitchResult, ServiceError> {
    match adapter {
        Adapter::Steam(steam) => {
            steam::run_switch(steam, platform, backups, account, options, sink)
        }
        Adapter::Epic(epic) => epic::run_switch(epic, platform, backups, account, options, sink),
        Adapter::Battlenet(battlenet) => {
            battlenet::run_switch(battlenet, platform, backups, account, options, sink)
        }
        Adapter::Riot(riot) => riot::run_switch(riot, platform, backups, account, options, sink),
    }
}

// Puts a snapshot of a launcher's files back, from Settings or after a crash. Refused while the launcher runs; what is there now is snapshotted first, so restoring the wrong one can be undone.
pub fn restore_backup(
    adapter: &Adapter<'_>,
    platform: &Platform,
    backups: &BackupService<'_>,
    id: &str,
) -> Result<RestoreResult, ServiceError> {
    match adapter {
        Adapter::Steam(steam) => steam::restore_backup(steam, platform, backups, id),
        Adapter::Epic(epic) => epic::restore_backup(epic, platform, backups, id),
        Adapter::Battlenet(battlenet) => {
            battlenet::restore_backup(battlenet, platform, backups, id)
        }
        Adapter::Riot(riot) => riot::restore_backup(riot, platform, backups, id),
    }
}

// A plan's steps as a job reports them, and where each is in the list. A step the plan passes over keeps the plan's reason.
fn job_steps(plan: &SwitchPlan) -> (Vec<JobStep>, HashMap<&'static str, usize>) {
    let steps = plan
        .steps
        .iter()
        .map(|step| JobStep {
            id: step.id,
            title: step.title.clone(),
            status: if step.skipped {
                JobStepStatus::Skipped
            } else {
                JobStepStatus::Pending
            },
            detail: if step.skipped {
                step.detail.clone()
            } else {
                String::new()
            },
        })
        .collect();
    let index = plan
        .steps
        .iter()
        .enumerate()
        .map(|(at, step)| (step.id, at))
        .collect();
    (steps, index)
}
