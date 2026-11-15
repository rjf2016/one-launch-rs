use rusqlite::ErrorCode;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error(
        "The One Launch database is damaged ({0}). Restore a copy from the backups\\db folder in the data directory, or move the file aside to start over."
    )]
    Corrupt(String),
    #[error(
        "The One Launch database is in use by another program ({0}). Close anything that has it open, such as a second One Launch, a database viewer, or a backup tool, and try again."
    )]
    Locked(String),
    #[error(
        "The One Launch database is read-only ({0}). Check the permissions on the data folder and the file, then try again."
    )]
    ReadOnly(String),
    #[error(
        "The One Launch database could not be opened ({0}). Check that the data folder exists and is writable."
    )]
    CannotOpen(String),
    #[error(
        "This database was created by a newer version of One Launch (schema {found}; this version supports up to {supported}). Update One Launch, or restore an older copy of the database."
    )]
    TooNew { found: u32, supported: u32 },
    #[error(
        "Migration {label} rebuilds tables, but foreign keys could not be switched off (is migrate() inside a transaction?)"
    )]
    ForeignKeysStillOn { label: String },
    #[error("Migration {label} left {count} row(s) pointing at a missing parent")]
    Orphans { label: String, count: usize },
    #[error("Migration {label} failed: {source}")]
    Migration {
        label: String,
        source: rusqlite::Error,
    },
    #[error("There is no One Launch database in this folder.")]
    NotFound,
    #[error(
        "The database has changes that are not in the file yet (onelaunch.db{0} is not empty). Quit One Launch, then copy the folder again."
    )]
    UnfinishedWrite(&'static str),
    #[error("The database could not be read or written: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("The copy of the database could not be {what}: {source}")]
    Copy {
        what: &'static str,
        source: std::io::Error,
    },
}

impl StoreError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Corrupt(_) => "DB_CORRUPT",
            Self::Locked(_) | Self::ReadOnly(_) | Self::CannotOpen(_) | Self::NotFound => {
                "DB_UNAVAILABLE"
            }
            Self::TooNew { .. } => "DB_TOO_NEW",
            Self::ForeignKeysStillOn { .. }
            | Self::Orphans { .. }
            | Self::Migration { .. }
            | Self::UnfinishedWrite(_)
            | Self::Sql(_)
            | Self::Copy { .. } => "INTERNAL",
        }
    }

    pub(crate) fn opening(error: rusqlite::Error) -> Self {
        let detail = error.to_string();
        Self::classified(error.sqlite_error_code(), detail)
    }

    // rusqlite puts the path into the text of a failed open, and a path can run through the user's profile folder.
    pub(crate) fn opening_file(error: rusqlite::Error) -> Self {
        let detail = match error.sqlite_error() {
            Some(sqlite) if sqlite.code == ErrorCode::CannotOpen => {
                "unable to open database file".to_owned()
            }
            Some(sqlite) => sqlite.to_string(),
            None => "the path cannot be used".to_owned(),
        };
        Self::classified(error.sqlite_error_code(), detail)
    }

    fn classified(code: Option<ErrorCode>, detail: String) -> Self {
        match code {
            Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => Self::Locked(detail),
            Some(ErrorCode::ReadOnly) => Self::ReadOnly(detail),
            Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase) => Self::Corrupt(detail),
            _ => Self::CannotOpen(detail),
        }
    }
}
