#![forbid(unsafe_code)]

use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures_lite::future::block_on;
use ol_api::Backend;
use ol_api::accounts::{NewAccount, Secret};
use ol_api::launchers::Launchers;
use ol_api::log::{LogLevel, LogOptions, Logger};
use ol_model::{LauncherId, SettingKey};
use ol_platform_api::{DataProtector, Platform, ProtectError, Zeroizing};
use ol_platform_fake::{
    FakeClock, FakeProcesses, FakeRegistry, FakeShell, OpLog, ProcessKnobs, ScratchFiles,
};
use ol_testkit::Scratch;
use ol_vault::FakeVault;

const VERSION: &str = "9.8.7";
const WAIT: Duration = Duration::from_secs(10);

#[test]
fn a_change_of_the_level_takes_effect_at_once() {
    let scratch = Scratch::new("api-log-level");
    let logger = Logger::for_data_folder(&scratch.join("data"), LogOptions::default());
    let backend = Backend::logging_to(logger.log()).start(scratch.join("data"), VERSION);
    block_on(backend.set_setting(SettingKey::LogLevel, "error".to_owned())).unwrap();
    block_on(backend.set_setting(SettingKey::DryRun, "true".to_owned())).unwrap();
    block_on(backend.set_setting(SettingKey::LogLevel, "debug".to_owned())).unwrap();
    block_on(backend.set_setting(SettingKey::DryRun, "false".to_owned())).unwrap();
    backend.shut_down();
    assert!(logger.flush(WAIT));
    let text = fs::read_to_string(logger.current_path()).unwrap();
    assert!(text.contains("[main] starting"));
    assert!(!text.contains(r#""key":"dryRun","value":"true""#));
    assert!(text.contains(r#"setting updated {"key":"logLevel","value":"debug"}"#));
    assert!(text.contains(r#""key":"dryRun","value":"false""#));
    assert_eq!(logger.log().level(), Some(LogLevel::Debug));
}

#[test]
fn the_stored_level_is_followed_from_the_first_read_of_the_database() {
    let scratch = Scratch::new("api-log-stored");
    let data = scratch.join("data");
    let first = Backend::start(data.clone(), VERSION);
    block_on(first.set_setting(SettingKey::LogLevel, "warn".to_owned())).unwrap();
    first.shut_down();

    let logger = Logger::for_data_folder(&data, LogOptions::default());
    let backend = Backend::logging_to(logger.log()).start(data.clone(), VERSION);
    block_on(backend.set_setting(SettingKey::DryRun, "true".to_owned())).unwrap();
    assert!(block_on(backend.set_setting(SettingKey::LogLevel, "loud".to_owned())).is_err());
    backend.shut_down();
    assert!(logger.flush(WAIT));
    let text = fs::read_to_string(logger.current_path()).unwrap();
    assert!(
        text.contains("[main] starting"),
        "the line before the read keeps the level the log was opened with"
    );
    assert!(!text.contains("setting updated"));
    assert!(text.contains("ERROR [main:ipc] ipc settings:set failed"));
    assert!(!text.contains("[main] exit"));
}

#[test]
fn a_log_that_cannot_be_written_leaves_the_backend_working() {
    let scratch = Scratch::new("api-log-blocked");
    let blocked = scratch.join("not-a-folder");
    fs::write(&blocked, "").unwrap();
    let logger = Logger::open(&blocked, LogOptions::default());
    let backend = Backend::logging_to(logger.log()).start(scratch.join("data"), VERSION);
    block_on(backend.set_setting(SettingKey::DryRun, "true".to_owned())).unwrap();
    let settings = block_on(backend.settings()).unwrap();
    assert_eq!(settings.get(SettingKey::DryRun), Some("true"));
    backend.shut_down();
    assert!(logger.flush(WAIT));
    assert_eq!(fs::read(&blocked).unwrap(), b"");
}

// This user's DPAPI stands in as a XOR; another user's wraps its own way and, once `opens` is set, reads this user's too.
struct Mine;

impl DataProtector for Mine {
    fn protect(&self, plain: &[u8]) -> Result<Vec<u8>, ProtectError> {
        Ok(plain.iter().map(|byte| byte ^ 0x5a).collect())
    }

    fn unprotect(&self, protected: &[u8]) -> Result<Zeroizing<Vec<u8>>, ProtectError> {
        Ok(Zeroizing::new(
            protected.iter().map(|byte| byte ^ 0x5a).collect(),
        ))
    }
}

struct Theirs(Arc<AtomicBool>);

impl DataProtector for Theirs {
    fn protect(&self, plain: &[u8]) -> Result<Vec<u8>, ProtectError> {
        Ok([b"theirs".as_slice(), plain].concat())
    }

    fn unprotect(&self, protected: &[u8]) -> Result<Zeroizing<Vec<u8>>, ProtectError> {
        match protected.strip_prefix(b"theirs".as_slice()) {
            Some(plain) => Ok(Zeroizing::new(plain.to_vec())),
            None if self.0.load(Ordering::SeqCst) => Mine.unprotect(protected),
            None => Err(ProtectError::Unprotect),
        }
    }
}

// A Windows that can neither wrap nor unwrap.
struct Broken;

impl DataProtector for Broken {
    fn protect(&self, _: &[u8]) -> Result<Vec<u8>, ProtectError> {
        Err(ProtectError::Protect)
    }

    fn unprotect(&self, _: &[u8]) -> Result<Zeroizing<Vec<u8>>, ProtectError> {
        Err(ProtectError::Unprotect)
    }
}

fn text(logger: &Logger) -> String {
    assert!(logger.flush(WAIT));
    fs::read_to_string(logger.current_path()).unwrap()
}

fn account_with_password(backend: &Backend) {
    block_on(backend.create_account(NewAccount {
        launcher: LauncherId::Steam,
        display_name: "Vault Keeper".to_owned(),
        login: "keeper_1".to_owned(),
        launcher_uid: None,
        password: Some(Secret::new("placeholder-pw-2".to_owned())),
        tags: Vec::new(),
        notes: String::new(),
    }))
    .unwrap();
}

#[test]
fn a_vault_that_is_not_there_or_a_folder_that_cannot_be_made_is_said_at_start() {
    let scratch = Scratch::new("api-log-start");
    let logger = Logger::open(&scratch.join("logs"), LogOptions::default());
    let backend = Backend::logging_to(logger.log()).start(scratch.join("data"), VERSION);
    block_on(backend.settings()).unwrap();
    let copies = scratch.join("data").join("backups").join("db");
    let _ = fs::remove_dir_all(&copies);
    fs::write(&copies, "").unwrap();
    assert!(block_on(backend.back_up_database()).is_err());
    assert!(block_on(backend.reset_vault()).is_err());
    backend.shut_down();
    fs::write(scratch.join("not-a-folder"), "").unwrap();
    let blocked = Backend::logging_to(logger.log()).start(scratch.join("not-a-folder"), VERSION);
    assert!(block_on(blocked.settings()).is_err());
    blocked.shut_down();
    let written = text(&logger);
    assert!(written.contains(
        "WARN  [main] secure storage unavailable; secrets cannot be stored on this system"
    ));
    assert!(written.contains("ERROR [main] the data folder could not be opened"));
    assert!(written.contains("ERROR [main:ipc] ipc settings:getAll failed"));
    assert!(written.contains("WARN  [main] database copy failed"));
    assert!(
        written.contains(r#"ERROR [main:ipc] ipc vault:reset failed {"code":"VAULT_UNAVAILABLE""#)
    );
}

#[test]
fn a_key_file_in_another_shape_is_said_before_the_start_stops() {
    let scratch = Scratch::new("api-log-key");
    let data = scratch.join("data");
    let local_state = ol_vault::local_state_path(&data);
    fs::create_dir_all(local_state.parent().unwrap()).unwrap();
    fs::write(&local_state, "not json").unwrap();
    let log = OpLog::new();
    let platform = Platform {
        registry: Arc::new(FakeRegistry::new(log.clone())),
        processes: Arc::new(FakeProcesses::new(log.clone(), ProcessKnobs::default())),
        files: Arc::new(ScratchFiles::new(&[scratch.path()], log.clone())),
        shell: Arc::new(FakeShell::new(log)),
        clock: Arc::new(FakeClock::starting_at(
            time::macros::datetime!(2026-11-06 16:46:23 UTC),
        )),
    };
    let logger = Logger::open(&scratch.join("logs"), LogOptions::default());
    let started = Backend::logging_to(logger.log()).start_on_this_pc(
        data,
        VERSION,
        Launchers::on(platform, Arc::new(FakeVault)),
        Box::new(Mine),
        None,
    );
    assert!(started.is_err());
    assert!(text(&logger).contains("ERROR [main] the vault key could not be opened"));
}

#[test]
fn a_lost_key_and_its_reset_are_logged() {
    let scratch = Scratch::new("api-log-reset");
    let data = scratch.join("data");
    let first = Backend::start_with_vault_of_data_folder(data.clone(), VERSION, Box::new(Mine));
    account_with_password(&first);
    first.shut_down();
    let (store, _) = ol_store::Store::open(&data.join("onelaunch.db")).unwrap();
    assert!(
        store
            .delete_secret(ol_model::SecretKey::VaultProbe)
            .unwrap()
    );
    drop(store);

    let logger = Logger::open(&scratch.join("logs"), LogOptions::default());
    let opens = Arc::new(AtomicBool::new(false));
    let lost = Backend::logging_to(logger.log()).start_with_vault_of_data_folder(
        data.clone(),
        VERSION,
        Box::new(Theirs(Arc::clone(&opens))),
    );
    block_on(lost.vault_status()).unwrap();
    opens.store(true, Ordering::SeqCst);
    block_on(lost.reset_vault()).unwrap();
    lost.shut_down();
    let readable = text(&logger);
    assert!(
        readable.find("INFO  [main:vault] vault probe stored")
            < readable.find("INFO  [main:vault] vault reset: the key opened after all"),
        "a key that opens after all gets its probe back, before the answer"
    );
    assert!(readable.contains("INFO  [main:vault] vault probe stored"));

    let backend = Backend::logging_to(logger.log()).start_with_vault_of_data_folder(
        data,
        VERSION,
        Box::new(Theirs(Arc::new(AtomicBool::new(false)))),
    );
    block_on(backend.reset_vault()).unwrap();
    assert_eq!(
        block_on(backend.reset_vault())
            .err()
            .map(|refused| refused.code),
        Some("BAD_REQUEST")
    );
    backend.shut_down();
    let cleared = text(&logger);
    let removed = [
        "WARN  [main:vault] vault reset: all stored secrets removed",
        "INFO  [main:vault] vault probe stored",
        "WARN  [main:ipc] vault reset by user",
    ]
    .map(|line| {
        cleared
            .rfind(line)
            .unwrap_or_else(|| panic!("the log has {line:?}"))
    });
    assert!(
        removed.windows(2).all(|pair| pair[0] < pair[1]),
        "the probe is stored between the reset and its answer, as in the Electron app"
    );

    let broken = Backend::logging_to(logger.log()).start_with_vault_of_data_folder(
        scratch.join("broken"),
        VERSION,
        Box::new(Broken),
    );
    assert!(block_on(broken.reset_vault()).is_err());
    broken.shut_down();
    let written = text(&logger);
    assert!(cleared.contains(r#"ERROR [main:ipc] ipc vault:reset failed {"code":"BAD_REQUEST""#));
    assert_eq!(
        written
            .matches("ERROR [main:ipc] ipc vault:reset failed")
            .count(),
        cleared
            .matches("ERROR [main:ipc] ipc vault:reset failed")
            .count()
            + 1,
        "a reset Windows cannot carry out is said"
    );
    for line in [
        "ERROR [main] stored secrets cannot be decrypted; the vault key was lost or belongs to another Windows user",
        "INFO  [main:vault] vault reset: the key opened after all; nothing removed",
        "WARN  [main:vault] vault reset: all stored secrets removed",
        "WARN  [main:ipc] vault reset by user",
    ] {
        assert!(written.contains(line), "the log has {line:?}");
    }
    assert!(!written.contains("placeholder-pw-2"));
}
