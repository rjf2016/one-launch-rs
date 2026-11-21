// Copies of launcher-owned files, taken under <data folder>\backups\<launcher>\<id>\ before a switch changes them. This module reads launcher files and writes only into the data folder: putting a copy back over a launcher's file is the switch pipeline's to do.

use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ol_formats::winpath::{is_absolute, same_path};
use ol_model::Ciphertext;
use ol_model::{BackupSnapshot, LauncherId};
use ol_platform_api::{Clock, Files};
use ol_vault::{Secret, Vault, VaultError};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;
use time::OffsetDateTime;
use zeroize::Zeroizing;

use crate::log::Log;

pub const DEFAULT_BACKUPS_TO_KEEP: i64 = 20;
// Pinned snapshots are kept beside the ordinary ones, the newest ten of them.
const PINNED_BACKUPS_TO_KEEP: usize = 10;
const MANIFEST_NAME: &str = "manifest.json";

#[derive(Error)]
pub enum BackupError {
    #[error("Invalid snapshot-relative path: {0}")]
    InvalidRelative(String),
    #[error("Snapshot {id} not found for {}", launcher.as_str())]
    NotFound { id: String, launcher: LauncherId },
    #[error("Snapshot {id} was taken for {taken_for}, not {}", launcher.as_str())]
    OtherLauncher {
        id: String,
        taken_for: String,
        launcher: LauncherId,
    },
    #[error("Snapshot {id} names a file that is not one of {}'s: {source_path}", launcher.as_str())]
    StrayFile {
        id: String,
        launcher: LauncherId,
        source_path: String,
    },
    #[error("Snapshot file is corrupt: {0}")]
    Corrupt(String),
    #[error("Snapshot meta has an unexpected shape")]
    MetaShape,
    #[error(transparent)]
    Vault(#[from] VaultError),
    #[error("The snapshot could not be read or written: {0}")]
    Io(#[from] std::io::Error),
}

impl BackupError {
    pub const CODE: &'static str = "BACKUP";
}

// The message can hold a path, so only the code is printed.
impl std::fmt::Debug for BackupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BackupError({})", Self::CODE)
    }
}

pub struct BackupFile<'a> {
    // The launcher's own file, by its full path.
    pub source: &'a str,
    // Where it lands inside the snapshot, such as config/loginusers.vdf.
    pub relative: &'a str,
    // The file holds a sign-in. Its copy is stored encrypted, as <relative>.enc, so that nothing under backups\ signs anyone in.
    pub sensitive: bool,
}

pub struct SnapshotRequest<'a> {
    pub launcher: LauncherId,
    // Shown to the user, so it names no secret.
    pub reason: &'a str,
    pub files: &'a [BackupFile<'a>],
    // Small values to put back with the files: registry values, for one.
    pub meta: Map<String, Value>,
    // A snapshot this one's clean-up must not remove: the one about to be put back.
    pub protect: Option<&'a str>,
    // Why the snapshot holds the only copy of something, which keeps it out of the usual clean-up. Shown to the user, so it names no secret.
    pub pinned: Option<&'a str>,
}

struct ManifestFile {
    relative: String,
    source: String,
    existed: bool,
    bytes: u64,
    sha256: Option<String>,
    encrypted: bool,
}

struct Manifest {
    launcher: String,
    created_at: String,
    reason: String,
    files: Vec<ManifestFile>,
    meta: Map<String, Value>,
    pinned: Option<String>,
}

// What a snapshot holds for the files that were there when it was taken, each checked against its recorded hash. A copy can be a sign-in out of the vault, so each is wiped when it is dropped.
pub struct Restorable {
    pub files: Vec<(String, Zeroizing<Vec<u8>>)>,
    pub meta: Map<String, Value>,
}

pub struct BackupService<'a> {
    root: String,
    keep: i64,
    files: &'a dyn Files,
    clock: &'a dyn Clock,
    vault: &'a dyn Vault,
    log: Log,
    launcher_log: Log,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn check_relative(relative: &str) -> Result<(), BackupError> {
    let bad_part = relative
        .split(['\\', '/'])
        .any(|part| part.is_empty() || part == "..");
    if relative.is_empty() || is_absolute(relative) || relative.contains(':') || bad_part {
        return Err(BackupError::InvalidRelative(relative.to_owned()));
    }
    Ok(())
}

// A snapshot's id is its time, so ids sort by age as plain text.
pub fn snapshot_id_for(at: OffsetDateTime) -> String {
    ol_store::iso_timestamp(at).replace(':', "-")
}

// What snapshot_id_for gives, with the "-2" a second snapshot in one millisecond gets. Nothing else is ever joined onto a path.
fn is_snapshot_id(id: &str) -> bool {
    let (stamp, counter) = match id.split_once("Z-") {
        Some((stamp, counter)) => (format!("{stamp}Z"), Some(counter)),
        None => (id.to_owned(), None),
    };
    let shape = "dddd-dd-ddTdd-dd-dd.dddZ";
    let stamped = stamp.len() == shape.len()
        && stamp.bytes().zip(shape.bytes()).all(|(found, wanted)| {
            if wanted == b'd' {
                found.is_ascii_digit()
            } else {
                found == wanted
            }
        });
    stamped
        && counter.is_none_or(|counter| {
            !counter.is_empty() && counter.bytes().all(|b| b.is_ascii_digit())
        })
}

fn manifest_file(value: &Value) -> Option<ManifestFile> {
    let sha256 = match &value["sha256"] {
        Value::Null if value.get("sha256").is_some() => None,
        Value::String(hash) => Some(hash.clone()),
        _ => return None,
    };
    Some(ManifestFile {
        relative: value["relative"].as_str()?.to_owned(),
        source: value["source"].as_str()?.to_owned(),
        existed: value["existed"].as_bool()?,
        bytes: value["bytes"].as_u64()?,
        sha256,
        encrypted: value["encrypted"] == true,
    })
}

impl<'a> BackupService<'a> {
    pub fn new(
        data_dir: &Path,
        keep: i64,
        files: &'a dyn Files,
        clock: &'a dyn Clock,
        vault: &'a dyn Vault,
    ) -> Self {
        Self {
            root: data_dir.join("backups").to_string_lossy().into_owned(),
            keep,
            files,
            clock,
            vault,
            log: Log::off(),
            launcher_log: Log::off(),
        }
    }

    // Snapshots taken, pruned, found unreadable and put back are logged under `main:backup`.
    #[must_use]
    pub fn logging_to(mut self, log: &Log) -> Self {
        self.log = log.child("backup");
        self.launcher_log = log.clone();
        self
    }

    // After a snapshot's files were written back, by a restore or a rollback: the two lines the Electron app's backup service and adapter write. Riot and Epic keep nothing in the registry, and their line says nothing of it.
    pub fn log_restored(
        &self,
        launcher: LauncherId,
        id: &str,
        files: usize,
        registry: Option<usize>,
    ) {
        self.log.warn(
            "restored launcher files from backup",
            json!({ "launcher": launcher.as_str(), "id": id, "files": files }),
        );
        let line = match launcher {
            LauncherId::Steam => "backup restored".to_owned(),
            other => format!("{} backup restored", other.as_str()),
        };
        let mut fields = json!({ "id": id, "files": files });
        if let Some(registry) = registry {
            fields["registry"] = registry.into();
        }
        self.launcher_log
            .child(launcher.as_str())
            .warn(&line, fields);
    }

    pub fn root(&self) -> &str {
        &self.root
    }

    // A small secret for a snapshot's meta, such as a registry value that signs someone in: the vault's ciphertext of it in base64, as the Electron app keeps it.
    pub fn seal(&self, secret: &Secret) -> Result<String, BackupError> {
        Ok(STANDARD.encode(self.vault.encrypt(secret)?.into_bytes()))
    }

    pub fn open(&self, sealed: &str) -> Result<Secret, BackupError> {
        let bytes = STANDARD
            .decode(sealed)
            .map_err(|_| BackupError::MetaShape)?;
        Ok(self.vault.decrypt(&Ciphertext::new(bytes))?)
    }

    fn launcher_dir(&self, launcher: LauncherId) -> PathBuf {
        Path::new(&self.root).join(launcher.as_str())
    }

    fn stored_path(folder: &Path, file: &ManifestFile) -> PathBuf {
        let mut path = folder.to_path_buf();
        path.extend(file.relative.split(['\\', '/']));
        if file.encrypted {
            path.as_mut_os_string().push(".enc");
        }
        path
    }

    pub fn snapshot(&self, request: &SnapshotRequest<'_>) -> Result<BackupSnapshot, BackupError> {
        for file in request.files {
            check_relative(file.relative)?;
        }
        let launcher_dir = self.launcher_dir(request.launcher);
        fs::create_dir_all(&launcher_dir)?;

        let created_at = self.clock.now();
        let stamp = snapshot_id_for(created_at);
        let mut id = stamp.clone();
        let mut counter = 2;
        while launcher_dir.join(&id).exists() {
            id = format!("{stamp}-{counter}");
            counter += 1;
        }
        let folder = launcher_dir.join(&id);
        fs::create_dir_all(&folder)?;

        let mut files = Vec::new();
        for file in request.files {
            let mut entry = ManifestFile {
                relative: file.relative.to_owned(),
                source: file.source.to_owned(),
                existed: false,
                bytes: 0,
                sha256: None,
                encrypted: false,
            };
            if self.files.exists(file.source) {
                // The file as it is can be a sign-in, so what was read is wiped once its copy is made.
                let bytes = Zeroizing::new(self.files.read(file.source)?);
                entry.encrypted = file.sensitive;
                // A copy that holds a sign-in is the vault's ciphertext of the file in base64, as the Electron app stores it.
                let kept = if file.sensitive {
                    let sealed = self.vault.encrypt(&Secret::new(STANDARD.encode(&*bytes)))?;
                    sealed.into_bytes()
                } else {
                    bytes.to_vec()
                };
                let stored = Self::stored_path(&folder, &entry);
                if let Some(parent) = stored.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&stored, &kept)?;
                entry.existed = true;
                entry.bytes = bytes.len() as u64;
                entry.sha256 = Some(sha256_hex(&kept));
            }
            files.push(entry);
        }

        let manifest = Manifest {
            launcher: request.launcher.as_str().to_owned(),
            created_at: ol_store::iso_timestamp(created_at),
            reason: request.reason.to_owned(),
            files,
            meta: request.meta.clone(),
            pinned: request
                .pinned
                .filter(|pinned| !pinned.is_empty())
                .map(str::to_owned),
        };
        let mut written = json!({
            "version": 1,
            "launcher": manifest.launcher,
            "createdAt": manifest.created_at,
            "reason": manifest.reason,
            "files": manifest.files.iter().map(|file| {
                let mut entry = json!({
                    "relative": file.relative,
                    "source": file.source,
                    "existed": file.existed,
                    "bytes": file.bytes,
                    "sha256": file.sha256,
                });
                if file.encrypted {
                    entry["encrypted"] = true.into();
                }
                entry
            }).collect::<Vec<_>>(),
            "meta": manifest.meta,
        });
        if let Some(pinned) = &manifest.pinned {
            written["pinned"] = pinned.as_str().into();
        }
        let text = serde_json::to_string_pretty(&written).expect("a manifest is plain JSON");
        let manifest_path = folder.join(MANIFEST_NAME).to_string_lossy().into_owned();
        self.files
            .write_atomic(&[(&manifest_path, text.as_bytes())])?;

        self.log.info(
            "backup snapshot written",
            json!({
                "launcher": request.launcher.as_str(),
                "id": id,
                "files": manifest.files.len(),
                "pinned": manifest.pinned.is_some(),
            }),
        );
        self.prune(request.launcher, &[Some(id.as_str()), request.protect])?;
        Ok(self.to_snapshot(request.launcher, &id, &folder, &manifest))
    }

    // The launcher's snapshots, newest first. A folder without a manifest that reads is not one.
    pub fn list(&self, launcher: LauncherId) -> Result<Vec<BackupSnapshot>, BackupError> {
        let launcher_dir = self.launcher_dir(launcher);
        if !launcher_dir.exists() {
            return Ok(Vec::new());
        }
        let mut snapshots = Vec::new();
        for entry in fs::read_dir(&launcher_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            match Self::read_manifest(&entry.path()) {
                Some(manifest) => {
                    snapshots.push(self.to_snapshot(launcher, &id, &entry.path(), &manifest));
                }
                None => self.log.warn(
                    "unreadable backup manifest",
                    json!({ "dir": entry.path().to_string_lossy() }),
                ),
            }
        }
        snapshots.sort_by(|a, b| b.id.cmp(&a.id));
        Ok(snapshots)
    }

    pub fn get(
        &self,
        launcher: LauncherId,
        id: &str,
    ) -> Result<Option<BackupSnapshot>, BackupError> {
        Ok(self
            .list(launcher)?
            .into_iter()
            .find(|snapshot| snapshot.id == id))
    }

    fn load(&self, launcher: LauncherId, id: &str) -> Result<(PathBuf, Manifest), BackupError> {
        let not_found = || BackupError::NotFound {
            id: id.to_owned(),
            launcher,
        };
        if !is_snapshot_id(id) {
            return Err(not_found());
        }
        let folder = self.launcher_dir(launcher).join(id);
        let manifest = Self::read_manifest(&folder).ok_or_else(not_found)?;
        if manifest.launcher != launcher.as_str() {
            return Err(BackupError::OtherLauncher {
                id: id.to_owned(),
                taken_for: manifest.launcher,
                launcher,
            });
        }
        Ok((folder, manifest))
    }

    // Removes the oldest ordinary snapshots beyond the number to keep and the oldest pinned ones beyond their own, never one of `protect`. Ids sort by age only while the clock goes forward, so the snapshot just taken is always among `protect`.
    fn prune(&self, launcher: LauncherId, protect: &[Option<&str>]) -> Result<(), BackupError> {
        let limit = usize::try_from(self.keep.max(1)).unwrap_or(1);
        let snapshots = self.list(launcher)?;
        let ordinary = snapshots
            .iter()
            .filter(|snapshot| snapshot.pinned.is_none())
            .skip(limit);
        let pinned = snapshots
            .iter()
            .filter(|snapshot| snapshot.pinned.is_some())
            .skip(PINNED_BACKUPS_TO_KEEP);
        let mut removed = 0;
        for snapshot in ordinary.chain(pinned) {
            if !protect.contains(&Some(snapshot.id.as_str())) {
                match fs::remove_dir_all(self.launcher_dir(launcher).join(&snapshot.id)) {
                    Ok(()) => removed += 1,
                    Err(error) => self.log.warn(
                        "could not remove an old backup",
                        json!({ "launcher": launcher.as_str(), "id": snapshot.id, "error": error.to_string() }),
                    ),
                }
            }
        }
        if removed > 0 {
            self.log.info(
                "pruned backups",
                json!({ "launcher": launcher.as_str(), "removed": removed, "keep": limit }),
            );
        }
        Ok(())
    }

    // Every copy of a snapshot, checked against its hash, with the path it was taken from. `targets` are the files the adapter owns now: a manifest is data on disk, so a path in it that is not one of them refuses the whole snapshot.
    pub fn read_for_restore(
        &self,
        launcher: LauncherId,
        id: &str,
        targets: &[&str],
    ) -> Result<Restorable, BackupError> {
        let (folder, manifest) = self.load(launcher, id)?;
        let existed: Vec<&ManifestFile> =
            manifest.files.iter().filter(|file| file.existed).collect();
        if let Some(stray) = existed.iter().find(|file| {
            !is_absolute(&file.source)
                || !targets.iter().any(|target| same_path(target, &file.source))
        }) {
            return Err(BackupError::StrayFile {
                id: id.to_owned(),
                launcher,
                source_path: stray.source.clone(),
            });
        }
        let mut files = Vec::new();
        for file in existed {
            check_relative(&file.relative)?;
            let bytes = fs::read(Self::stored_path(&folder, file))?;
            if file
                .sha256
                .as_deref()
                .is_some_and(|recorded| recorded != sha256_hex(&bytes))
            {
                return Err(BackupError::Corrupt(file.relative.clone()));
            }
            let bytes = if file.encrypted {
                let opened = self.vault.decrypt(&Ciphertext::new(bytes))?;
                STANDARD
                    .decode(opened.expose())
                    .map_err(|_| BackupError::Corrupt(file.relative.clone()))?
            } else {
                bytes
            };
            files.push((file.source.clone(), Zeroizing::new(bytes)));
        }
        Ok(Restorable {
            files,
            meta: manifest.meta,
        })
    }

    fn read_manifest(folder: &Path) -> Option<Manifest> {
        let parsed: Value =
            serde_json::from_slice(&fs::read(folder.join(MANIFEST_NAME)).ok()?).ok()?;
        if parsed["version"] != 1 {
            return None;
        }
        let files = parsed["files"]
            .as_array()?
            .iter()
            .map(manifest_file)
            .collect::<Option<Vec<_>>>()?;
        Some(Manifest {
            launcher: parsed["launcher"].as_str().unwrap_or_default().to_owned(),
            created_at: parsed["createdAt"].as_str()?.to_owned(),
            reason: parsed["reason"].as_str().unwrap_or_default().to_owned(),
            files,
            meta: parsed["meta"].as_object().cloned().unwrap_or_default(),
            pinned: parsed["pinned"]
                .as_str()
                .filter(|pinned| !pinned.is_empty())
                .map(str::to_owned),
        })
    }

    fn to_snapshot(
        &self,
        launcher: LauncherId,
        id: &str,
        folder: &Path,
        manifest: &Manifest,
    ) -> BackupSnapshot {
        BackupSnapshot {
            id: id.to_owned(),
            launcher,
            created_at: manifest.created_at.clone(),
            reason: manifest.reason.clone(),
            files: manifest
                .files
                .iter()
                .filter(|file| file.existed)
                .map(|file| file.relative.clone())
                .collect(),
            bytes: manifest.files.iter().map(|file| file.bytes).sum(),
            pinned: manifest.pinned.clone(),
            path: folder.to_string_lossy().into_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use ol_platform_fake::{FakeClock, OpLog, ScratchFiles};
    use ol_testkit::Scratch;
    use ol_vault::FakeVault;
    use time::macros::datetime;

    use super::*;

    #[test]
    fn a_snapshot_is_never_removed_by_its_own_clean_up() {
        let scratch = Scratch::new("backup-prune");
        let source = scratch.join("loginusers.vdf");
        fs::write(&source, "\"users\"\n{\n}\n").unwrap();
        let source = source.to_string_lossy().into_owned();
        let files = ScratchFiles::new(&[scratch.path()], OpLog::new());
        let clock = FakeClock::starting_at(datetime!(2026-11-06 16:46:23 UTC));
        let backups = BackupService::new(scratch.path(), 1, &files, &clock, &FakeVault);
        let request = SnapshotRequest {
            launcher: LauncherId::Steam,
            reason: "Before switching",
            files: &[BackupFile {
                source: &source,
                relative: "config/loginusers.vdf",
                sensitive: false,
            }],
            meta: Map::new(),
            protect: None,
            pinned: None,
        };
        for _ in 0..3 {
            let taken = backups.snapshot(&request).unwrap();
            assert!(
                backups.get(LauncherId::Steam, &taken.id).unwrap().is_some(),
                "the snapshot just taken is there to restore"
            );
        }
    }
}
