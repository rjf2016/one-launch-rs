use std::path::Path;
use std::time::Duration;

use rusqlite::backup::{Backup, StepResult};
use rusqlite::{Connection, OpenFlags};

use crate::migrations::{MIGRATIONS, Migration};
use crate::{SCHEMA_VERSION, Store, StoreError};

const BUSY_TIMEOUT: Duration = Duration::from_millis(5_000);
const UNFINISHED: [&str; 2] = ["-wal", "-journal"];

#[derive(Debug, PartialEq, Eq)]
pub struct MigrationReport {
    pub from: u32,
    pub to: u32,
    pub applied: Vec<String>,
}

pub struct Snapshot {
    pub store: Store,
    pub schema_on_disk: u32,
}

impl Store {
    pub fn open(file: &Path) -> Result<(Self, MigrationReport), StoreError> {
        Self::open_within(file, BUSY_TIMEOUT)
    }

    pub fn open_within(
        file: &Path,
        busy_timeout: Duration,
    ) -> Result<(Self, MigrationReport), StoreError> {
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let mut conn =
            Connection::open_with_flags(file, flags).map_err(StoreError::opening_file)?;
        prepare(&conn, busy_timeout)?;
        let report = migrate(&mut conn, &MIGRATIONS)?;
        Ok((Self { conn }, report))
    }

    // Nothing is written, not even a lock or a journal file: a database below the current schema is copied into memory and migrated there.
    pub fn open_read_only(file: &Path) -> Result<Snapshot, StoreError> {
        if !file.is_file() {
            return Err(StoreError::NotFound);
        }
        // An immutable open does not look at a journal, so what it holds would be left out without a word. One that cannot be looked at counts as holding something.
        for suffix in UNFINISHED {
            let mut beside = file.as_os_str().to_owned();
            beside.push(suffix);
            let empty = match std::fs::metadata(&beside) {
                Ok(journal) => journal.len() == 0,
                Err(error) => error.kind() == std::io::ErrorKind::NotFound,
            };
            if !empty {
                return Err(StoreError::UnfinishedWrite(suffix));
            }
        }
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = Connection::open_with_flags(immutable_uri(file)?, flags)
            .map_err(StoreError::opening_file)?;
        keep_out_of_temp(&conn)?;
        quick_check(&conn)?;
        let schema_on_disk = schema_version(&conn).map_err(opening)?;
        if schema_on_disk > SCHEMA_VERSION {
            return Err(StoreError::TooNew {
                found: schema_on_disk,
                supported: SCHEMA_VERSION,
            });
        }
        let conn = if schema_on_disk == SCHEMA_VERSION {
            conn
        } else {
            let mut copy = Connection::open_in_memory()?;
            let copied = Backup::new(&conn, &mut copy)
                .and_then(|backup| backup.step(-1))
                .map_err(StoreError::opening)?;
            if copied != StepResult::Done {
                return Err(StoreError::Locked(
                    "the copy in memory did not finish".to_owned(),
                ));
            }
            keep_out_of_temp(&copy)?;
            copy.pragma_update(None, "foreign_keys", "ON")?;
            migrate(&mut copy, &MIGRATIONS)?;
            copy
        };
        Ok(Snapshot {
            store: Self { conn },
            schema_on_disk,
        })
    }

    pub fn schema_version(&self) -> Result<u32, StoreError> {
        schema_version(&self.conn)
    }
}

fn opening(error: StoreError) -> StoreError {
    match error {
        StoreError::Sql(error) => StoreError::opening(error),
        other => other,
    }
}

fn immutable_uri(file: &Path) -> Result<String, StoreError> {
    let path = file
        .to_str()
        .ok_or_else(|| StoreError::CannotOpen("the path is not valid Unicode".to_owned()))?;
    // Two slashes after "file:" would start a host name, and a verbatim path takes no forward slash.
    let path = path
        .replace('/', "\\")
        .replace('%', "%25")
        .replace('?', "%3f")
        .replace('#', "%23");
    Ok(format!("file:{path}?mode=ro&immutable=1"))
}

fn prepare(conn: &Connection, busy_timeout: Duration) -> Result<(), StoreError> {
    let prepared = (|| {
        conn.busy_timeout(busy_timeout)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        keep_out_of_temp(conn)?;
        quick_check(conn)?;
        // A page write that is rolled back shows the file is ours to use: a lock held elsewhere fails as busy, a read-only file as read-only.
        let version = schema_version(conn)?;
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let written = conn.pragma_update(None, "user_version", version);
        let _ = conn.execute_batch("ROLLBACK");
        Ok(written?)
    })();
    prepared.map_err(opening)
}

// A sort or a statement journal that outgrows its memory would otherwise spill into a file in %TEMP%, rows and all. The Electron app leaves this at SQLite's default.
fn keep_out_of_temp(conn: &Connection) -> Result<(), StoreError> {
    Ok(conn.pragma_update(None, "temp_store", "MEMORY")?)
}

fn quick_check(conn: &Connection) -> Result<(), StoreError> {
    let found: String = conn
        .pragma_query_value(None, "quick_check", |row| row.get(0))
        .map_err(StoreError::opening)?;
    if found == "ok" {
        Ok(())
    } else {
        Err(StoreError::Corrupt(found))
    }
}

fn schema_version(conn: &Connection) -> Result<u32, StoreError> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    u32::try_from(version).map_err(|_| StoreError::Corrupt(format!("schema version {version}")))
}

pub(crate) fn migrate(
    conn: &mut Connection,
    migrations: &[Migration],
) -> Result<MigrationReport, StoreError> {
    let from = schema_version(conn)?;
    let supported = migrations.last().map_or(0, |last| last.version);
    if from > supported {
        return Err(StoreError::TooNew {
            found: from,
            supported,
        });
    }
    let mut applied = Vec::new();
    for migration in migrations.iter().filter(|step| step.version > from) {
        if migration.foreign_keys_off {
            apply_with_foreign_keys_off(conn, migration)?;
        } else {
            apply(conn, migration, false)?;
        }
        applied.push(migration.label());
    }
    Ok(MigrationReport {
        from,
        to: schema_version(conn)?,
        applied,
    })
}

fn apply(
    conn: &mut Connection,
    migration: &Migration,
    check_references: bool,
) -> Result<(), StoreError> {
    let failed = |source| StoreError::Migration {
        label: migration.label(),
        source,
    };
    let transaction = conn.transaction().map_err(failed)?;
    transaction.execute_batch(migration.up).map_err(failed)?;
    if check_references {
        let mut count = 0;
        transaction
            .pragma_query(None, "foreign_key_check", |_| {
                count += 1;
                Ok(())
            })
            .map_err(failed)?;
        if count > 0 {
            return Err(StoreError::Orphans {
                label: migration.label(),
                count,
            });
        }
    }
    transaction
        .pragma_update(None, "user_version", migration.version)
        .map_err(failed)?;
    transaction.commit().map_err(failed)
}

// Dropping a table that other rows point at deletes those rows while foreign keys are on, and the switch does nothing inside a transaction: so it is turned around the transaction, and every reference is checked before the commit.
fn apply_with_foreign_keys_off(
    conn: &mut Connection,
    migration: &Migration,
) -> Result<(), StoreError> {
    let failed = |source| StoreError::Migration {
        label: migration.label(),
        source,
    };
    conn.pragma_update(None, "foreign_keys", "OFF")
        .map_err(failed)?;
    let applied = match conn.pragma_query_value(None, "foreign_keys", |row| row.get::<_, i64>(0)) {
        Ok(0) => apply(conn, migration, true),
        Ok(_) => Err(StoreError::ForeignKeysStillOn {
            label: migration.label(),
        }),
        Err(error) => Err(failed(error)),
    };
    let restored = conn.pragma_update(None, "foreign_keys", "ON");
    applied?;
    restored.map_err(failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_steps_count_up_from_one() {
        for (index, migration) in MIGRATIONS.iter().enumerate() {
            assert_eq!(migration.version as usize, index + 1);
            assert!(!migration.name.is_empty());
            assert!(!migration.up.trim().is_empty());
        }
        assert_eq!(MIGRATIONS.last().unwrap().version, SCHEMA_VERSION);
    }

    #[test]
    fn a_path_is_escaped_for_the_uri() {
        let uri = immutable_uri(Path::new(r"D:\a b\50%#1?\onelaunch.db")).unwrap();
        assert_eq!(
            uri,
            r"file:D:\a b\50%25%231%3f\onelaunch.db?mode=ro&immutable=1"
        );
        let uri = immutable_uri(Path::new("//server/share/onelaunch.db")).unwrap();
        assert_eq!(uri, r"file:\\server\share\onelaunch.db?mode=ro&immutable=1");
    }
}
