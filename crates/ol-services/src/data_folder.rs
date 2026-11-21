use std::fs;
use std::path::{Path, PathBuf};

use ol_model::SettingKey;
use ol_store::{
    BACKUPS_TO_KEEP, MigrationReport, Store, StoreError, database_backups_dir, database_path,
};
use thiserror::Error;
use time::OffsetDateTime;

const FOLDERS: [&str; 3] = ["artwork", "backups", "logs"];

#[derive(Debug, Error)]
pub enum OpenError {
    #[error(
        "The data folder could not be made ({0}). Check that its drive is there and that the folder can be written to."
    )]
    Folder(std::io::Error),
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl OpenError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Folder(_) => "INTERNAL",
            Self::Store(error) => error.code(),
        }
    }
}

pub fn open(data_dir: &Path, version: &str) -> Result<(Store, MigrationReport), OpenError> {
    fs::create_dir_all(data_dir).map_err(OpenError::Folder)?;
    for folder in FOLDERS {
        fs::create_dir_all(data_dir.join(folder)).map_err(OpenError::Folder)?;
    }
    let (store, migrated) = Store::open(&database_path(data_dir))?;
    // A database made by this start has nothing to announce, so its version counts as seen.
    if migrated.from == 0 && store.setting(SettingKey::WhatsNewLastSeen)?.is_none() {
        store.set_setting(SettingKey::WhatsNewLastSeen, version)?;
    }
    Ok((store, migrated))
}

// The folders Settings opens in Explorer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Folder {
    Data,
    Logs,
    Backups,
}

impl Folder {
    pub fn path(self, data_dir: &Path) -> PathBuf {
        match self {
            Self::Data => data_dir.to_path_buf(),
            Self::Logs => data_dir.join("logs"),
            Self::Backups => data_dir.join("backups"),
        }
    }
}

pub fn back_up(store: &Store, data_dir: &Path) -> Result<PathBuf, StoreError> {
    store.backup(
        &database_backups_dir(data_dir),
        BACKUPS_TO_KEEP,
        OffsetDateTime::now_utc(),
    )
}
