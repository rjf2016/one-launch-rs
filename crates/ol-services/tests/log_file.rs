#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;
use std::time::Duration;

use ol_model::LogLevel;
use ol_model::redact::{register_account_names, register_windows_user};
use ol_services::log::file::FileSink;
use ol_services::log::{LogOptions, Logger};
use ol_testkit::Scratch;
use serde_json::json;

const WAIT: Duration = Duration::from_secs(10);

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

fn files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn the_file_is_moved_behind_the_others_before_a_line_would_take_it_past_the_limit() {
    let scratch = Scratch::new("log-rotate");
    let dir = scratch.join("logs");
    let mut sink = FileSink::new(&dir, 100, 2);
    let line = |n: usize| format!("{n:02}{}\n", "x".repeat(37));
    sink.write_lines(&[line(1), line(2)]).unwrap();
    assert_eq!(files(&dir), ["onelaunch.log"]);
    sink.write_lines(&[line(3)]).unwrap();
    assert_eq!(files(&dir), ["onelaunch.1.log", "onelaunch.log"]);
    assert_eq!(read(&dir.join("onelaunch.1.log")), line(1) + &line(2));
    assert_eq!(read(&dir.join("onelaunch.log")), line(3));
    sink.write_lines(&[line(4), line(5), line(6), line(7), line(8)])
        .unwrap();
    assert_eq!(
        files(&dir),
        ["onelaunch.1.log", "onelaunch.2.log", "onelaunch.log"]
    );
    assert_eq!(read(&dir.join("onelaunch.log")), line(7) + &line(8));
    assert_eq!(read(&dir.join("onelaunch.1.log")), line(5) + &line(6));
    assert_eq!(read(&dir.join("onelaunch.2.log")), line(3) + &line(4));
    sink.write_lines(&["y".repeat(150)]).unwrap();
    assert_eq!(read(&dir.join("onelaunch.log")), "y".repeat(150));
}

#[test]
fn a_new_run_counts_the_file_it_finds() {
    let scratch = Scratch::new("log-size");
    let dir = scratch.join("logs");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("onelaunch.log"), "z".repeat(90)).unwrap();
    let mut sink = FileSink::new(&dir, 100, 5);
    sink.write_lines(&["0123456789a".to_owned()]).unwrap();
    assert_eq!(read(&dir.join("onelaunch.1.log")), "z".repeat(90));
    assert_eq!(read(&dir.join("onelaunch.log")), "0123456789a");
}

#[test]
fn the_default_limits_are_the_electron_apps() {
    let options = LogOptions::default();
    assert_eq!((options.max_bytes, options.keep), (1024 * 1024, 5));
    assert_eq!(options.level, LogLevel::Info);
    assert!(!options.mirror);
}

#[test]
fn a_line_below_the_level_is_not_written_and_a_change_counts_from_the_next_line() {
    let scratch = Scratch::new("log-level");
    let logger = Logger::open(
        &scratch.join("logs"),
        LogOptions {
            level: LogLevel::Warn,
            ..LogOptions::default()
        },
    );
    let log = logger.log().child("test");
    log.debug("first debug", ());
    log.info("first info", ());
    log.warn("first warn", json!({ "count": 1 }));
    log.set_level(LogLevel::Debug);
    log.debug("second debug", ());
    log.set_level(LogLevel::Error);
    log.warn("second warn", ());
    log.error("second error", ());
    assert!(logger.flush(WAIT));
    let text = read(&logger.current_path());
    assert!(!text.contains("first debug") && !text.contains("first info"));
    assert!(text.contains(" WARN  [main:test] first warn {\"count\":1}\n"));
    assert!(text.contains(" DEBUG [main:test] second debug\n"));
    assert!(!text.contains("second warn"));
    assert!(text.contains(" ERROR [main:test] second error\n"));
    assert_eq!(log.level(), Some(LogLevel::Error));
}

#[test]
fn a_registered_name_is_masked_on_its_way_to_the_file() {
    let scratch = Scratch::new("log-names");
    let logger = Logger::open(&scratch.join("logs"), LogOptions::default());
    register_account_names(["Weekend_Smurf_17"]);
    logger.log().info(
        "signed in as weekend_smurf_17",
        json!({ "who": "WEEKEND_SMURF_17", "password": "hunter22" }),
    );
    assert!(logger.flush(WAIT));
    let text = read(&logger.current_path());
    assert!(
        text.contains("signed in as [ACCOUNT] {\"who\":\"[ACCOUNT]\",\"password\":\"[REDACTED]\"}")
    );
    assert!(!text.to_lowercase().contains("weekend_smurf_17"));
    assert!(!text.contains("hunter22"));
}

#[test]
fn a_folder_that_cannot_be_written_loses_the_lines_and_nothing_else() {
    let scratch = Scratch::new("log-unwritable");
    let blocked = scratch.join("logs");
    fs::write(&blocked, "a file where the folder should be").unwrap();
    let logger = Logger::open(&blocked, LogOptions::default());
    let log = logger.log();
    for n in 0..50 {
        log.error("cannot be written", json!({ "n": n }));
    }
    assert!(logger.flush(WAIT), "the writer is still there");
    assert_eq!(read(&blocked), "a file where the folder should be");
    log.info("after the failures", ());
    logger.close(WAIT);
    log.info("after the close", ());
    assert!(!logger.flush(WAIT));
}

#[test]
fn a_log_that_is_off_writes_nothing_and_takes_every_call() {
    let log = ol_services::log::Log::off();
    log.error("nowhere", json!({ "token": "x" }));
    assert!(log.is_off());
    assert_eq!(log.level(), None);
    assert_eq!(log.child("steam").scope(), "main:steam");
}

#[cfg(windows)]
#[test]
fn a_live_file_another_program_holds_is_not_rotated_and_no_older_file_is_lost() {
    use std::os::windows::fs::OpenOptionsExt;

    const SHARE_READ_AND_WRITE: u32 = 0x1 | 0x2;
    let scratch = Scratch::new("log-held");
    let dir = scratch.join("logs");
    fs::create_dir_all(&dir).unwrap();
    for n in 1..=5 {
        fs::write(
            dir.join(format!("onelaunch.{n}.log")),
            format!("older {n}\n"),
        )
        .unwrap();
    }
    let line = |n: usize| format!("{n:02}{}\n", "x".repeat(37));
    let mut sink = FileSink::new(&dir, 100, 5);
    sink.write_lines(&[line(1), line(2)]).unwrap();
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(SHARE_READ_AND_WRITE)
        .open(dir.join("onelaunch.log"))
        .unwrap();
    for n in 3..9 {
        sink.write_lines(&[line(n)]).unwrap();
    }
    for n in 1..=5 {
        assert_eq!(
            read(&dir.join(format!("onelaunch.{n}.log"))),
            format!("older {n}\n")
        );
    }
    assert_eq!(
        read(&dir.join("onelaunch.log")),
        (1..9).map(line).collect::<String>()
    );
    drop(held);
    sink.write_lines(&[line(9)]).unwrap();
    assert_eq!(read(&dir.join("onelaunch.log")), line(9));
    assert_eq!(
        read(&dir.join("onelaunch.1.log")),
        (1..9).map(line).collect::<String>()
    );
    assert_eq!(read(&dir.join("onelaunch.5.log")), "older 4\n");
}

#[cfg(windows)]
#[expect(
    clippy::disallowed_methods,
    reason = "std makes no junction, and a junction is what a test of a junction needs"
)]
fn junction(link: &Path, target: &Path) {
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .unwrap();
    assert!(made.status.success(), "mklink /J failed");
}

#[cfg(windows)]
#[test]
fn a_log_folder_or_file_that_is_a_link_is_not_written_through() {
    let scratch = Scratch::new("log-link");
    let away = scratch.join("away");
    fs::create_dir_all(&away).unwrap();
    let dir = scratch.join("logs");
    junction(&dir, &away);
    let logger = Logger::open(&dir, LogOptions::default());
    logger.log().error("through a junction", ());
    assert!(logger.flush(WAIT));
    assert_eq!(files(&away), Vec::<String>::new());

    let real = scratch.join("real-logs");
    fs::create_dir_all(&real).unwrap();
    junction(&real.join("onelaunch.log"), &away);
    let mut sink = FileSink::new(&real, 100, 5);
    assert!(sink.write_lines(&["refused\n".to_owned()]).is_err());
    assert_eq!(files(&away), Vec::<String>::new());
    let file_link = std::os::windows::fs::symlink_file(
        scratch.join("target.txt"),
        real.join("onelaunch.1.log"),
    );
    if file_link.is_ok() {
        fs::remove_dir(real.join("onelaunch.log")).unwrap();
        fs::write(real.join("onelaunch.log"), "x".repeat(99)).unwrap();
        let mut sink = FileSink::new(&real, 100, 5);
        sink.write_lines(&["rotate\n".to_owned()]).unwrap();
        assert!(!scratch.join("target.txt").exists());
        assert_eq!(
            read(&real.join("onelaunch.log")),
            "x".repeat(99) + "rotate\n"
        );
    }
}

#[test]
fn the_windows_user_registered_at_start_is_masked_on_a_share_too() {
    let scratch = Scratch::new("log-user");
    let logger = Logger::open(&scratch.join("logs"), LogOptions::default());
    register_windows_user(Some(r"\\server\redirect$\jo.smith"), Some("jsmith"));
    logger.log().info(
        "starting",
        json!({ "dataDir": r"\\server\redirect$\jo.smith\Documents\Games", "by": "JSMITH" }),
    );
    assert!(logger.flush(WAIT));
    let text = read(&logger.current_path());
    assert!(text.contains(r#"redirect$\\[USER]\\Documents"#), "{text}");
    assert!(!text.to_lowercase().contains("jo.smith") && !text.to_lowercase().contains("jsmith"));
}
