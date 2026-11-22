#![forbid(unsafe_code)]

use std::fs;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use ol_model::redact::{register_account_names, register_windows_user};
use ol_services::log::{LogOptions, Logger, log_panics};
use ol_testkit::Scratch;

const WAIT: Duration = Duration::from_secs(10);
const NAME: &str = "Persona Panicky";
const USER: &str = "panicky.user";
const SHAPELESS: &str = "correcthorsebatterystaple";

struct Exploding;

impl Iterator for Exploding {
    type Item = &'static str;

    fn next(&mut self) -> Option<&'static str> {
        panic!("{SHAPELESS} broke while the names were being written");
    }
}

fn text(logger: &Logger) -> String {
    assert!(logger.flush(WAIT));
    fs::read_to_string(logger.current_path()).unwrap_or_default()
}

fn panic_on(name: &str, panics: impl FnOnce() + Send + 'static) {
    let thrown = thread::Builder::new()
        .name(name.to_owned())
        .spawn(panics)
        .unwrap()
        .join();
    assert!(thrown.is_err());
}

// One test in its own binary: the hook is the process's.
#[test]
fn a_panic_is_written_with_its_place_and_thread_and_never_its_message() {
    let scratch = Scratch::new("log-panic");
    let logger = Arc::new(Logger::open(&scratch.join("logs"), LogOptions::default()));
    register_account_names([NAME]);
    register_windows_user(Some(&format!(r"C:\Users\{USER}")), Some(USER));
    log_panics(Arc::clone(&logger));

    panic_on("worker", || {
        panic!(r"no session for {NAME} in C:\Users\{USER}\AppData, typed {SHAPELESS}")
    });
    let written = text(&logger);
    let line = written
        .lines()
        .find(|line| line.contains("ERROR [main] uncaught exception "))
        .expect("the panic was written");
    assert!(
        line.ends_with(r#""thread":"worker"}"#)
            && line.contains(r#"{"location":"crates\\ol-services\\tests\\log_panic.rs:"#),
        "{line}"
    );
    for kept_out in [NAME, USER, SHAPELESS, "no session"] {
        assert!(!written.contains(kept_out));
    }

    panic_on("registering", || register_account_names(Exploding));
    let written = text(&logger);
    let line = written
        .lines()
        .rfind(|line| line.contains("uncaught exception"))
        .unwrap();
    assert_ne!(written.matches("uncaught exception").count(), 1);
    assert!(
        line.contains("log_panic.rs:") && line.ends_with(r#""thread":"registering"}"#),
        "a panic while its thread holds the names is written with its place: {line}"
    );
    assert!(!written.contains(SHAPELESS));
}
