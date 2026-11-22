#![forbid(unsafe_code)]

use std::fs;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use ol_model::redact::{register_account_names, register_windows_user};
use ol_services::log::{LogOptions, Logger, log_panics};
use ol_testkit::Scratch;

const WAIT: Duration = Duration::from_secs(10);
const NAME: &str = "Persona Panicky";
const USER: &str = "panicky.user";
const PASSWORD: &str = "hunter2hunter2";

struct Exploding;

impl Iterator for Exploding {
    type Item = &'static str;

    fn next(&mut self) -> Option<&'static str> {
        panic!("{NAME} broke while the names were being written");
    }
}

fn text(logger: &Logger) -> String {
    assert!(logger.flush(WAIT));
    fs::read_to_string(logger.current_path()).unwrap_or_default()
}

// One test in its own binary: the hook is the process's.
#[test]
fn a_panic_is_one_masked_line_and_never_holds_up_the_crash() {
    let scratch = Scratch::new("log-panic");
    let logger = Arc::new(Logger::open(&scratch.join("logs"), LogOptions::default()));
    register_account_names([NAME]);
    register_windows_user(Some(&format!(r"C:\Users\{USER}")), Some(USER));
    log_panics(Arc::clone(&logger));

    let started = Instant::now();
    let thrown = thread::spawn(|| {
        panic!(r#"no session for {NAME} in C:\Users\{USER}\AppData, "password":"{PASSWORD}""#)
    })
    .join();
    assert!(thrown.is_err());
    let written = text(&logger);
    let line = written
        .lines()
        .find(|line| line.contains("ERROR [main] uncaught exception"))
        .expect("the panic was written");
    assert!(line.contains(r#""message":"no session for [ACCOUNT] in C:\\Users\\[USER]\\AppData"#));
    assert!(line.contains(r#""location":"crates"#) && line.contains("log_panic.rs:"));
    for kept_out in [NAME, USER, PASSWORD] {
        assert!(!written.contains(kept_out));
    }

    let held = thread::spawn(|| register_account_names(Exploding)).join();
    assert!(held.is_err());
    let written = text(&logger);
    assert_eq!(written.matches("uncaught exception").count(), 2);
    assert!(
        written
            .trim_end()
            .ends_with("ERROR [main] uncaught exception"),
        "a panic while the names are held is written without its fields"
    );
    assert!(!written.contains("broke while"));

    logger.close(WAIT);
    let after = thread::spawn(|| panic!("after the close")).join();
    assert!(after.is_err());
    assert!(!text_of(&logger).contains("after the close"));
    assert!(started.elapsed() < Duration::from_secs(5));
}

fn text_of(logger: &Logger) -> String {
    fs::read_to_string(logger.current_path()).unwrap_or_default()
}
