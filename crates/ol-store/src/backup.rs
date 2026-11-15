use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use rusqlite::backup::{Backup, StepResult};
use time::OffsetDateTime;

use crate::{Store, StoreError, iso_timestamp};

const PREFIX: &str = "onelaunch-";
const SUFFIX: &str = ".db";

impl Store {
    pub fn backup(
        &self,
        folder: &Path,
        keep: usize,
        now: OffsetDateTime,
    ) -> Result<PathBuf, StoreError> {
        let failed = |what| move |source| StoreError::Copy { what, source };
        fs::create_dir_all(folder).map_err(failed("made"))?;
        let copy = folder.join(format!(
            "{PREFIX}{}{SUFFIX}",
            iso_timestamp(now).replace(':', "-")
        ));
        let was_there = copy.exists();
        let copied = (|| {
            let mut target = Connection::open(&copy).map_err(StoreError::opening_file)?;
            match Backup::new(&self.conn, &mut target)?.step(-1)? {
                StepResult::Done => Ok(()),
                _ => Err(StoreError::Locked("the copy did not finish".to_owned())),
            }
        })();
        if let Err(error) = copied {
            // Left behind, the empty file would count among the newest and push a whole copy out.
            if !was_there {
                let _ = fs::remove_file(&copy);
            }
            return Err(error);
        }

        // The copy just made stays whatever its name says: after the clock is set back it sorts as the oldest, and would be pruned at once.
        let made = copy.file_name();
        let mut earlier = Vec::new();
        for entry in fs::read_dir(folder).map_err(failed("listed"))? {
            let name = entry.map_err(failed("listed"))?.file_name();
            if is_a_copy(&name.to_string_lossy()) && Some(name.as_os_str()) != made {
                earlier.push(name);
            }
        }
        earlier.sort();
        earlier.reverse();
        for name in earlier.into_iter().skip(keep.max(1) - 1) {
            fs::remove_file(folder.join(name)).map_err(failed("pruned"))?;
        }
        Ok(copy)
    }
}

fn is_a_copy(name: &str) -> bool {
    name.len() >= PREFIX.len() + SUFFIX.len() && name.starts_with(PREFIX) && name.ends_with(SUFFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_copies_are_pruned() {
        for name in [
            "onelaunch-2026-11-07T03-23-28.612Z.db",
            "onelaunch-.db",
            "onelaunch-x.db",
        ] {
            assert!(is_a_copy(name), "{name}");
        }
        for name in [
            "onelaunch.db",
            "onelaunch-.d",
            "onelaunch-x.db-wal",
            "Onelaunch-x.db",
            "my-onelaunch-x.db",
        ] {
            assert!(!is_a_copy(name), "{name}");
        }
    }
}
