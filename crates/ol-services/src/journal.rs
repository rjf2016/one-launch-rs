// The record of a switch in progress, <data folder>\switch.json: written before the adapter is called, updated when the snapshot exists and at each step, removed when the switch settles. A file, not a table, so it outlives a damaged or locked database. No failure here stops a switch.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use ol_model::{LauncherId, RecoveryPhase};
use serde_json::{Value, json};

use crate::log::Log;

pub const JOURNAL_FILE: &str = "switch.json";

// Steps that run only once the launcher's files are written and it is being started or is signing in.
const AFTER_WRITES: [&str; 6] = ["start", "wait", "verify", "login", "manual", "recapture"];

// What is on disk while a switch runs. It names the account by One Launch's own id and the step by its fixed id: no login, no display name, no launcher account id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalEntry {
    pub job_id: String,
    pub launcher: LauncherId,
    pub account_id: String,
    pub started_at: String,
    pub updated_at: String,
    // None until the snapshot exists, which is always before the first launcher file or registry value is written.
    pub backup_id: Option<String>,
    pub step_id: Option<String>,
    // The switch failed and putting its snapshot back failed too: the next start offers the snapshot again.
    pub rollback_failed: bool,
}

impl JournalEntry {
    pub fn phase(&self) -> RecoveryPhase {
        if self.rollback_failed {
            RecoveryPhase::RollbackFailed
        } else if self
            .step_id
            .as_deref()
            .is_some_and(|step| AFTER_WRITES.contains(&step))
        {
            RecoveryPhase::SigningIn
        } else {
            RecoveryPhase::Writing
        }
    }

    fn to_json(&self) -> String {
        let entry = json!({
            "version": 1,
            "jobId": self.job_id,
            "launcher": self.launcher.as_str(),
            "accountId": self.account_id,
            "startedAt": self.started_at,
            "updatedAt": self.updated_at,
            "backupId": self.backup_id,
            "stepId": self.step_id,
            "rollbackFailed": self.rollback_failed,
        });
        serde_json::to_string_pretty(&entry).expect("a journal entry is plain JSON")
    }

    fn parse(text: &str) -> Option<Self> {
        let raw: Value = serde_json::from_str(text).ok()?;
        if !raw.is_object() || raw["version"] != 1 {
            return None;
        }
        let shaped = |value: &Value, max: usize, allowed: fn(u8) -> bool| {
            value
                .as_str()
                .filter(|text| (1..=max).contains(&text.len()) && text.bytes().all(allowed))
                .map(str::to_owned)
        };
        let id = |value: &Value| shaped(value, 64, |b| b.is_ascii_hexdigit() || b == b'-');
        let optional = |value: &Value, max: usize, allowed: fn(u8) -> bool| match value {
            Value::Null => Some(None),
            other => shaped(other, max, allowed).map(Some),
        };
        let started_at = raw["startedAt"]
            .as_str()
            .filter(|at| is_date(at))?
            .to_owned();
        if raw.get("backupId").is_none() || raw.get("stepId").is_none() {
            return None;
        }
        Some(Self {
            job_id: id(&raw["jobId"])?,
            launcher: LauncherId::parse(raw["launcher"].as_str()?)?,
            account_id: id(&raw["accountId"])?,
            updated_at: raw["updatedAt"]
                .as_str()
                .map_or_else(|| started_at.clone(), str::to_owned),
            started_at,
            backup_id: optional(&raw["backupId"], 64, |b| {
                b.is_ascii_alphanumeric() || b == b'.' || b == b'-'
            })?,
            step_id: optional(&raw["stepId"], 32, |b| b.is_ascii_lowercase() || b == b'-')?,
            rollback_failed: raw["rollbackFailed"] == true,
        })
    }
}

// The app writes its times as 2026-01-02T03:04:05.678Z, and only such a time is taken back.
fn is_date(text: &str) -> bool {
    let shape = "dddd-dd-ddTdd:dd:dd.dddZ";
    text.len() == shape.len()
        && text.bytes().zip(shape.bytes()).all(|(found, wanted)| {
            if wanted == b'd' {
                found.is_ascii_digit()
            } else {
                found == wanted
            }
        })
}

pub struct SwitchJournal {
    file: PathBuf,
    current: Option<JournalEntry>,
    log: Log,
}

impl SwitchJournal {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            file: data_dir.join(JOURNAL_FILE),
            current: None,
            log: Log::off(),
        }
    }

    #[must_use]
    pub fn logging_to(mut self, log: &Log) -> Self {
        self.log = log.child("journal");
        self
    }

    pub fn begin(&mut self, job_id: &str, launcher: LauncherId, account_id: &str, now: &str) {
        let entry = JournalEntry {
            job_id: job_id.to_owned(),
            launcher,
            account_id: account_id.to_owned(),
            started_at: now.to_owned(),
            updated_at: now.to_owned(),
            backup_id: None,
            step_id: None,
            rollback_failed: false,
        };
        self.write(&entry);
        self.current = Some(entry);
    }

    pub fn set_backup(&mut self, backup_id: &str, now: &str) {
        self.update(now, |entry| entry.backup_id = Some(backup_id.to_owned()));
    }

    pub fn set_step(&mut self, step_id: &str, now: &str) {
        self.update(now, |entry| entry.step_id = Some(step_id.to_owned()));
    }

    pub fn set_rollback_failed(&mut self, now: &str) {
        self.update(now, |entry| entry.rollback_failed = true);
    }

    // Writes only when something changed.
    fn update(&mut self, now: &str, change: impl FnOnce(&mut JournalEntry)) {
        let Some(entry) = &self.current else {
            return;
        };
        let mut next = entry.clone();
        change(&mut next);
        if next == *entry {
            return;
        }
        next.updated_at = now.to_owned();
        self.write(&next);
        self.current = Some(next);
    }

    // Removes the file. With a job id, only when the file still belongs to that job.
    pub fn clear(&mut self, job_id: Option<&str>) {
        if self
            .current
            .as_ref()
            .is_some_and(|entry| job_id.is_none_or(|id| entry.job_id == id))
        {
            self.current = None;
        }
        if let Some(job_id) = job_id
            && self.read().is_some_and(|on_disk| on_disk.job_id != job_id)
        {
            return;
        }
        if let Err(error) = fs::remove_file(&self.file)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            self.log.warn(
                "could not remove the switch journal",
                json!({ "error": error.to_string() }),
            );
        }
    }

    // The entry on disk, if there is one. A file that cannot be understood is moved aside, so it is met once and not at every start.
    pub fn read(&self) -> Option<JournalEntry> {
        if !self.file.exists() {
            return None;
        }
        let entry = fs::read_to_string(&self.file)
            .ok()
            .and_then(|text| JournalEntry::parse(&text));
        if entry.is_none() {
            self.log
                .warn("the switch journal could not be read; moving it aside", ());
            let mut aside = self.file.clone().into_os_string();
            aside.push(".unreadable");
            if let Err(error) = fs::rename(&self.file, aside) {
                self.log.warn(
                    "could not move the unreadable switch journal aside",
                    json!({ "error": error.to_string() }),
                );
            }
        }
        entry
    }

    // A temporary file, flushed to the disk, then moved over the journal: after a power cut the file is the old entry or the new one, never half of each.
    fn write(&self, entry: &JournalEntry) {
        let mut temporary = self.file.clone().into_os_string();
        temporary.push(format!(".{}.tmp", std::process::id()));
        let written = fs::File::create(&temporary)
            .and_then(|mut file| {
                file.write_all(entry.to_json().as_bytes())?;
                file.sync_all()
            })
            .and_then(|()| fs::rename(&temporary, &self.file));
        if let Err(error) = written {
            self.log.warn(
                "could not write the switch journal",
                json!({ "error": error.to_string() }),
            );
            let _ = fs::remove_file(&temporary);
        }
    }
}

// What the last run left behind. Read once at start, before any job can begin, so a journal written by a switch of this run is never taken for one to recover from.
#[derive(Default)]
pub struct Recovery {
    pending: Option<JournalEntry>,
    log: Log,
}

impl Recovery {
    pub fn load(data_dir: &Path) -> Self {
        Self::load_logged(data_dir, &Log::off())
    }

    pub fn load_logged(data_dir: &Path, log: &Log) -> Self {
        let mut journal = SwitchJournal::new(data_dir).logging_to(log);
        let Some(entry) = journal.read() else {
            return Self {
                pending: None,
                log: log.clone(),
            };
        };
        let root = log.clone();
        let log = log.child("recovery");
        if entry.backup_id.is_none() {
            log.info(
                "the last run ended during a switch before anything was written; nothing to recover",
                json!({ "jobId": entry.job_id, "stepId": entry.step_id }),
            );
            // The snapshot always comes before the first write, so without one nothing was changed.
            journal.clear(Some(&entry.job_id));
            return Self {
                pending: None,
                log: root,
            };
        }
        log.warn(
            "the last run ended during a switch; offering its snapshot",
            json!({
                "jobId": entry.job_id,
                "launcher": entry.launcher.as_str(),
                "accountId": entry.account_id,
                "backupId": entry.backup_id,
                "stepId": entry.step_id,
                "rollbackFailed": entry.rollback_failed,
            }),
        );
        Self {
            pending: Some(entry),
            log: root,
        }
    }

    pub fn pending(&self) -> Option<&JournalEntry> {
        self.pending.as_ref()
    }

    // The interrupted switch is dealt with, by a restore or by the user's leave: the journal goes.
    pub fn settle(&mut self, data_dir: &Path) {
        if let Some(entry) = self.pending.take() {
            SwitchJournal::new(data_dir)
                .logging_to(&self.log)
                .clear(Some(&entry.job_id));
        }
    }

    // A snapshot of this launcher was put back from Settings: whatever the journal said about the launcher is settled with it.
    pub fn restored_from_backups(&mut self, data_dir: &Path, launcher: LauncherId) {
        if self
            .pending
            .as_ref()
            .is_some_and(|entry| entry.launcher == launcher)
        {
            self.settle(data_dir);
        }
        let mut journal = SwitchJournal::new(data_dir).logging_to(&self.log);
        if let Some(on_disk) = journal.read().filter(|entry| entry.launcher == launcher) {
            journal.clear(Some(&on_disk.job_id));
        }
    }
}
#[cfg(test)]
mod tests {
    use ol_testkit::{Scratch, oracle};

    use super::*;

    fn fixture(name: &str) -> String {
        fs::read_to_string(oracle(&format!("fixtures/journal/{name}"))).unwrap()
    }

    #[test]
    fn each_recorded_journal_reads_and_is_written_back_byte_for_byte() {
        for (name, phase, has_backup) in [
            ("switch-before-writes.json", RecoveryPhase::Writing, false),
            ("switch-writing.json", RecoveryPhase::Writing, true),
            ("switch-signing-in.json", RecoveryPhase::SigningIn, true),
            (
                "switch-rollback-failed.json",
                RecoveryPhase::RollbackFailed,
                true,
            ),
        ] {
            let recorded = fixture(name);
            let entry = JournalEntry::parse(&recorded).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(entry.phase(), phase, "{name}");
            assert_eq!(entry.backup_id.is_some(), has_backup, "{name}");
            assert_eq!(entry.to_json(), recorded, "{name}");
        }
    }

    #[test]
    fn a_journal_goes_through_its_life_on_disk() {
        let scratch = Scratch::new("journal");
        let file = scratch.join(JOURNAL_FILE);
        let mut journal = SwitchJournal::new(scratch.path());
        assert_eq!(journal.read(), None);

        journal.begin(
            "0a1b",
            LauncherId::Steam,
            "2c3d",
            "2026-01-02T03:04:05.678Z",
        );
        let begun = journal.read().unwrap();
        assert_eq!(
            (begun.backup_id.as_deref(), begun.step_id.as_deref()),
            (None, None)
        );
        let first = fs::read(&file).unwrap();

        journal.set_step("close", "2026-01-02T03:04:06.000Z");
        journal.set_step("close", "2026-01-02T03:04:07.000Z");
        let stepped = journal.read().unwrap();
        assert_eq!(stepped.step_id.as_deref(), Some("close"));
        assert_eq!(stepped.updated_at, "2026-01-02T03:04:06.000Z");
        assert_eq!(stepped.started_at, "2026-01-02T03:04:05.678Z");
        assert_ne!(fs::read(&file).unwrap(), first);

        journal.set_backup("2026-01-02T03-04-08.000Z", "2026-01-02T03:04:08.000Z");
        journal.set_rollback_failed("2026-01-02T03:04:09.000Z");
        let failed = journal.read().unwrap();
        assert_eq!(failed.phase(), RecoveryPhase::RollbackFailed);
        assert_eq!(
            failed.backup_id.as_deref(),
            Some("2026-01-02T03-04-08.000Z")
        );

        journal.clear(Some("another-job"));
        assert!(file.exists());
        journal.clear(Some("0a1b"));
        assert!(!file.exists());
        journal.set_step("file", "2026-01-02T03:04:10.000Z");
        assert!(!file.exists(), "a cleared journal is not written again");
        let names: Vec<String> = ol_testkit::snapshot(scratch.path()).into_keys().collect();
        assert!(names.is_empty(), "{names:?}");
    }

    #[test]
    fn a_journal_that_cannot_be_understood_is_moved_aside_once() {
        let scratch = Scratch::new("journal");
        let file = scratch.join(JOURNAL_FILE);
        fs::write(&file, fixture("switch.json.unreadable")).unwrap();
        let journal = SwitchJournal::new(scratch.path());
        assert_eq!(journal.read(), None);
        assert!(!file.exists());
        assert!(scratch.join("switch.json.unreadable").exists());

        let recorded = fixture("switch-writing.json");
        for broken in [
            recorded.replace("\"version\": 1", "\"version\": 2"),
            recorded.replace("\"launcher\": \"steam\"", "\"launcher\": \"gog\""),
            recorded.replace("\"stepId\": \"file\"", "\"stepId\": \"File 1\""),
            recorded.replace("\"jobId\": \"", "\"jobId\": \"../"),
        ] {
            assert_ne!(broken, recorded);
            assert_eq!(JournalEntry::parse(&broken), None);
        }
    }
    #[test]
    fn a_journal_from_before_any_write_is_dropped_and_one_with_a_snapshot_is_offered() {
        let scratch = Scratch::new("journal");
        let file = scratch.join(JOURNAL_FILE);
        fs::write(&file, fixture("switch-before-writes.json")).unwrap();
        assert!(Recovery::load(scratch.path()).pending().is_none());
        assert!(!file.exists());

        fs::write(&file, fixture("switch-signing-in.json")).unwrap();
        let mut recovery = Recovery::load(scratch.path());
        assert_eq!(
            recovery.pending().map(JournalEntry::phase),
            Some(RecoveryPhase::SigningIn)
        );
        assert!(file.exists());
        recovery.restored_from_backups(scratch.path(), LauncherId::Epic);
        assert!(recovery.pending().is_some());
        recovery.restored_from_backups(scratch.path(), LauncherId::Steam);
        assert!(recovery.pending().is_none());
        assert!(!file.exists());
    }
}
