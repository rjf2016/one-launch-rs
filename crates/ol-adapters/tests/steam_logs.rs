#![forbid(unsafe_code)]

// The cases of oracle/l1 for the two Steam logs as they are read from disk: only what was added since a mark, and only the end of that.

use std::fs;

use ol_adapters::files::{MAX_TAIL_BYTES, read_log_since};
use ol_adapters::steam::{outcome_since, updating_since};
use ol_formats::steam::login_log::SignInOutcome;
use ol_platform_fake::{OpLog, ScratchFiles};
use ol_testkit::Scratch;
use ol_testkit::vectors::{Answer, Case, bytes, plain, run};
use serde_json::{Value, json};

// The log of a case, written where the port can read it; a case without a file gets the path of none.
fn with_log(case: &Case, name: &str, read: impl Fn(&ScratchFiles, &str, u64) -> Value) -> Answer {
    let scratch = Scratch::new("steam-log");
    let path = scratch.join(name);
    if let Some(file) = case.args[0].get("file") {
        fs::write(&path, bytes(file)).unwrap();
    }
    let files = ScratchFiles::new(&[scratch.path()], OpLog::new());
    let mark = case.args[1].as_u64().expect("a mark is a whole number");
    Answer::Out(read(&files, path.to_str().unwrap(), mark))
}

#[test]
fn outcome_since_cases() {
    run("l1/steam/outcomeSince.json", &[], |case| {
        with_log(
            case,
            "steamui_login.txt",
            |files, path, mark| match outcome_since(files, path, mark) {
                None => Value::Null,
                Some(SignInOutcome::Success) => json!({ "kind": "success" }),
                Some(SignInOutcome::Credentials { reason, cause }) => {
                    json!({ "kind": "credentials", "reason": reason, "cause": cause.as_str() })
                }
            },
        )
    });
}

#[test]
fn read_login_log_since_cases() {
    run("l1/steam/readLoginLogSince.json", &[], |case| {
        let max_bytes = plain(&case.args[2]).as_u64().unwrap_or(MAX_TAIL_BYTES);
        with_log(case, "steamui_login.txt", |files, path, mark| {
            read_log_since(files, path, mark, max_bytes).into()
        })
    });
}

#[test]
fn updating_since_cases() {
    run("l1/steam/updatingSince.json", &[], |case| {
        with_log(case, "bootstrap_log.txt", |files, path, mark| {
            updating_since(files, path, mark).into()
        })
    });
}
