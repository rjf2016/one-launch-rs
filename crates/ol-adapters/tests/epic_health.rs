#![forbid(unsafe_code)]

// The Epic Games Launcher's health check and Locate on a machine of the test's own: a launcher folder, the fixture's manifest and LauncherInstalled.dat, and the fixture's log. No account and no saved sign-in.

use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use ol_adapters::epic::{EpicAdapter, EpicConfig, Timing};
use ol_adapters::health::Known;
use ol_adapters::{AccountRef, AdapterError, Directory};
use ol_formats::epic::manifests::LaunchUriForm;
use ol_model::{HealthCheck, HealthStatus, LauncherSession};
use ol_platform_api::{FileReader, FileWriter, Platform};
use ol_platform_fake::{
    FakeClock, FakeProcesses, FakeRegistry, FakeShell, OpLog, ProcessKnobs, ScratchFiles,
};
use ol_runtime::TimingOverrides;
use ol_testkit::{Scratch, oracle};
use ol_vault::Secret;
use time::OffsetDateTime;

struct Nobody;

fn only_read() -> AdapterError {
    AdapterError::Failed("a health check changes nothing".to_owned())
}

impl Directory for Nobody {
    fn account_by_launcher_uid(&self, _: &str) -> Result<Option<AccountRef>, AdapterError> {
        Ok(None)
    }
    fn account_by_id(&self, _: &str) -> Result<Option<AccountRef>, AdapterError> {
        Ok(None)
    }
    fn set_launcher_uid(&self, _: &str, _: &str) -> Result<(), AdapterError> {
        Err(only_read())
    }
    fn session(&self, _: &str) -> Result<Option<LauncherSession>, AdapterError> {
        Ok(None)
    }
    fn session_by_fingerprint(&self, _: &str) -> Result<Option<LauncherSession>, AdapterError> {
        Ok(None)
    }
    fn save_session(
        &self,
        _: &str,
        _: Option<&str>,
        _: &str,
        _: &Secret,
    ) -> Result<LauncherSession, AdapterError> {
        Err(only_read())
    }
    fn decrypt_session(&self, _: &str) -> Result<Option<Secret>, AdapterError> {
        Ok(None)
    }
    fn mark_session_verified(&self, _: &str) -> Result<(), AdapterError> {
        Err(only_read())
    }
    fn mark_session_stale(&self, _: &str, _: &str) -> Result<(), AdapterError> {
        Err(only_read())
    }
}

// The scratch folder, with one file that is there and cannot be read.
struct Unreadable {
    files: ScratchFiles,
    path: String,
}

impl Unreadable {
    fn check(&self, path: &str) -> io::Result<()> {
        if path.eq_ignore_ascii_case(&self.path) {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        Ok(())
    }
}

impl FileReader for Unreadable {
    fn exists(&self, path: &str) -> bool {
        self.files.exists(path)
    }
    fn size(&self, path: &str) -> io::Result<u64> {
        self.check(path)?;
        self.files.size(path)
    }
    fn modified(&self, path: &str) -> io::Result<OffsetDateTime> {
        self.files.modified(path)
    }
    fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        self.check(path)?;
        self.files.read(path)
    }
    fn read_range(&self, path: &str, from: u64, length: u64) -> io::Result<Vec<u8>> {
        self.check(path)?;
        self.files.read_range(path, from, length)
    }
    fn list(&self, folder: &str) -> io::Result<Vec<String>> {
        self.files.list(folder)
    }
    fn resolved(&self, folder: &str) -> Option<String> {
        self.files.resolved(folder)
    }
}

impl FileWriter for Unreadable {
    fn write_atomic(&self, _: &[(&str, &[u8])]) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }
}

struct World {
    scratch: Scratch,
    platform: Platform,
    config: EpicConfig,
}

fn fixture(name: &str) -> PathBuf {
    oracle(&format!("fixtures/launchers/epic/{name}"))
}

fn text(path: &std::path::Path) -> String {
    path.to_str().unwrap().to_owned()
}

impl World {
    fn launcher(&self) -> PathBuf {
        self.scratch.join("Epic").join("Launcher")
    }

    fn manifests(&self) -> PathBuf {
        let data = self.scratch.join("ProgramData").join("Epic");
        data.join("EpicGamesLauncher")
            .join("Data")
            .join("Manifests")
    }

    fn log(&self) -> PathBuf {
        let saved = self.scratch.join("Local").join("EpicGamesLauncher");
        saved
            .join("Saved")
            .join("Logs")
            .join("EpicGamesLauncher.log")
    }

    fn with_unreadable(mut self, path: &str) -> Self {
        let files = ScratchFiles::new(&[self.scratch.path()], OpLog::new());
        self.platform.files = Arc::new(Unreadable {
            files,
            path: path.to_owned(),
        });
        self
    }

    fn health(&self) -> Vec<HealthCheck> {
        let adapter = EpicAdapter::new(&self.platform, &self.config, &Nobody);
        adapter.health_checks(&Known {
            install: adapter.detect_install().unwrap(),
            running: Some(false),
        })
    }
}

fn world() -> World {
    let scratch = Scratch::new("epic-health");
    let root = text(scratch.path());
    let under = |parts: &[&str]| {
        parts
            .iter()
            .fold(scratch.path().to_path_buf(), |path, part| path.join(part))
    };
    let binaries = under(&["Epic", "Launcher", "Portal", "Binaries", "Win64"]);
    fs::create_dir_all(&binaries).unwrap();
    fs::write(binaries.join("EpicGamesLauncher.exe"), b"").unwrap();
    let manifests = under(&[
        "ProgramData",
        "Epic",
        "EpicGamesLauncher",
        "Data",
        "Manifests",
    ]);
    fs::create_dir_all(&manifests).unwrap();
    let item = "A0000000000000000000000000000001.item";
    fs::copy(fixture(item), manifests.join(item)).unwrap();
    let installed = under(&["ProgramData", "Epic", "UnrealEngineLauncher"]);
    fs::create_dir_all(&installed).unwrap();
    fs::copy(
        fixture("LauncherInstalled.dat"),
        installed.join("LauncherInstalled.dat"),
    )
    .unwrap();
    let logs = under(&["Local", "EpicGamesLauncher", "Saved", "Logs"]);
    fs::create_dir_all(&logs).unwrap();
    fs::copy(
        fixture("EpicGamesLauncher.log"),
        logs.join("EpicGamesLauncher.log"),
    )
    .unwrap();

    let log = OpLog::new();
    let platform = Platform {
        registry: Arc::new(FakeRegistry::new(log.clone())),
        processes: Arc::new(FakeProcesses::new(log.clone(), ProcessKnobs::default())),
        files: Arc::new(ScratchFiles::new(&[scratch.path()], log.clone())),
        shell: Arc::new(FakeShell::new(log)),
        clock: Arc::new(FakeClock::starting_at(OffsetDateTime::UNIX_EPOCH)),
    };
    let config = EpicConfig {
        chosen_dir: None,
        default_dir: format!("{root}\\Epic\\Launcher"),
        local_app_data: format!("{root}\\Local"),
        program_data: format!("{root}\\ProgramData"),
        launch_uri_form: LaunchUriForm::Catalog,
        backups_to_keep: 20,
        backups_dir: format!("{root}\\backups"),
        timing: Timing::with(&TimingOverrides::default()),
    };
    World {
        scratch,
        platform,
        config,
    }
}

fn check<'c>(checks: &'c [HealthCheck], id: &str) -> (HealthStatus, &'c str) {
    let check = checks.iter().find(|check| check.id == id).unwrap();
    (check.status, check.detail.as_str())
}

#[test]
fn launcher_installed_agrees_only_with_a_scan_that_read_every_manifest() {
    let world = world();
    let checks = world.health();
    assert_eq!(check(&checks, "manifests").0, HealthStatus::Ok);
    assert_eq!(
        check(&checks, "launcher-installed"),
        (HealthStatus::Ok, "1 entry, agrees with the manifests")
    );

    fs::write(
        world
            .manifests()
            .join("B0000000000000000000000000000002.item"),
        "not json",
    )
    .unwrap();
    let checks = world.health();
    let (status, detail) = check(&checks, "manifests");
    assert_eq!(status, HealthStatus::Warn, "{detail}");
    assert!(
        detail.contains(", 1 unreadable; 1 installed game"),
        "{detail}"
    );
    assert_eq!(
        check(&checks, "launcher-installed"),
        (
            HealthStatus::Warn,
            "1 entry; not every manifest could be read, so the cross-check is incomplete"
        )
    );
}

#[test]
fn a_manifests_folder_that_cannot_be_listed_leaves_the_cross_check_incomplete() {
    let world = world();
    fs::remove_dir_all(world.manifests()).unwrap();
    fs::write(world.manifests(), "").unwrap();
    let checks = world.health();
    assert_eq!(check(&checks, "manifests").0, HealthStatus::Fail);
    assert_eq!(
        check(&checks, "launcher-installed"),
        (
            HealthStatus::Warn,
            "1 entry; not every manifest could be read, so the cross-check is incomplete"
        )
    );
}

#[test]
fn a_log_that_cannot_be_read_is_not_called_readable() {
    let world = world();
    let checks = world.health();
    let (status, detail) = check(&checks, "log");
    assert_eq!(status, HealthStatus::Ok, "{detail}");
    assert!(detail.starts_with("Readable; "), "{detail}");

    let log = text(&world.log());
    let world = world.with_unreadable(&log);
    let checks = world.health();
    let (status, detail) = check(&checks, "log");
    assert_eq!(status, HealthStatus::Warn, "{detail}");
    assert!(detail.starts_with("Could not read "), "{detail}");
}

#[test]
fn locate_takes_the_picked_program_and_no_other() {
    let world = world();
    let adapter = EpicAdapter::new(&world.platform, &world.config, &Nobody);
    let other = world
        .launcher()
        .join("Other")
        .join("Binaries")
        .join("Win64");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("EpicGamesLauncher.exe"), b"").unwrap();
    assert!(
        adapter
            .install_from_exe(&text(&other.join("EpicGamesLauncher.exe")))
            .is_none()
    );
    let portal = world
        .launcher()
        .join("Portal")
        .join("Binaries")
        .join("Win64");
    assert!(
        adapter
            .install_from_exe(&text(&portal.join("EpicGamesLauncher.exe")))
            .is_some()
    );
    let shouted = text(&portal.join("EPICGAMESLAUNCHER.EXE")).replace("Portal", "PORTAL");
    assert!(adapter.install_from_exe(&shouted).is_some());
}
