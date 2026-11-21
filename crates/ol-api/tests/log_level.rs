#![forbid(unsafe_code)]

// The backend's log follows the logLevel setting: the stored one from its first read of the database, and a change at once. A log that cannot be written changes nothing else.

use std::fs;
use std::time::Duration;

use futures_lite::future::block_on;
use ol_api::Backend;
use ol_api::log::{LogLevel, LogOptions, Logger};
use ol_model::SettingKey;
use ol_testkit::Scratch;

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
