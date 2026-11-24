#![forbid(unsafe_code)]

mod backup;
mod error;
mod launchers;
mod migrations;
mod open;
mod read;
mod sessions;
#[cfg(test)]
mod tests;
mod write;

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use time::OffsetDateTime;
use time::macros::format_description;

pub use error::StoreError;
pub use launchers::{AccountUpdate, GameSync, NewAccount};
pub use open::{MigrationReport, Snapshot};
pub use read::{Ciphertexts, RowCounts};
pub use sessions::NewSession;
pub use write::{ClearedSecrets, Sealed};

pub const DATABASE_FILE: &str = "onelaunch.db";
pub const SCHEMA_VERSION: u32 = 5;
pub const BACKUPS_TO_KEEP: usize = 5;

pub struct Store {
    conn: Connection,
}

pub fn database_path(data_dir: &Path) -> PathBuf {
    data_dir.join(DATABASE_FILE)
}

pub fn database_backups_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("backups").join("db")
}

pub fn iso_timestamp(at: OffsetDateTime) -> String {
    let format =
        format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z");
    at.to_offset(time::UtcOffset::UTC)
        .format(format)
        .expect("a date formats with a fixed description")
}

pub fn iso_now() -> String {
    iso_timestamp(OffsetDateTime::now_utc())
}
