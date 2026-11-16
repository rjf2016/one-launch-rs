#![forbid(unsafe_code)]

// Every case of oracle/l1/riot, run against the port. A case is left out only for a reason named here, and the test says which cases it left out.

use ol_formats::riot::client_log::{LogEvent, SignedInHow, parse_log_events};
use ol_formats::riot::lockfile::parse_lockfile;
use ol_formats::riot::products::{
    launch_args, parse_installs, parse_product_id, parse_product_settings,
};
use ol_formats::riot::session::{SessionRead, plan_sign_out, read_session, splice_session_block};
use ol_testkit::vectors::{Answer, UNPAIRED, plain, run, text};
use serde_json::{Value, json};

const NOT_A_PRODUCT_ID: &str =
    "not a product id as a metadata folder's name gives one, which the Rust type rules out";
const NOT_LINES: &str = "a block body that is not lines of text, which the Rust type rules out";

fn error(message: impl std::fmt::Display) -> Answer {
    Answer::Error(json!({ "name": "Error", "message": message.to_string() }))
}

// A file whose lines do not all end the same way. The port keeps each line's own end; the Electron app ends every line of what it writes with CRLF once the file holds one.
fn has_mixed_line_ends(text: &str) -> bool {
    let bare = text
        .match_indices('\n')
        .any(|(at, _)| at == 0 || text.as_bytes()[at - 1] != b'\r');
    bare && text.contains("\r\n")
}

// A text with every line end made CRLF, as the Electron app joins the lines of such a file.
fn joined_with_crlf(text: &str) -> String {
    ol_formats::js::lines(text).collect::<Vec<_>>().join("\r\n")
}

// What the port wrote, as the recording has it: for a file with mixed line ends, with the line ends the Electron app would have given every line.
fn as_recorded(before: &str, written: String) -> String {
    if has_mixed_line_ends(before) {
        joined_with_crlf(&written)
    } else {
        written
    }
}

fn how(how: SignedInHow) -> &'static str {
    match how {
        SignedInHow::Restored => "restored",
        SignedInHow::Manual => "manual",
        SignedInHow::Unknown => "unknown",
    }
}

fn event_json(event: &LogEvent) -> Value {
    match event {
        LogEvent::Started { at, build } => json!({ "kind": "started", "at": at, "build": build }),
        LogEvent::LaunchRequest { product, patchline } => {
            json!({ "kind": "launch-request", "product": product, "patchline": patchline })
        }
        LogEvent::ProductContext {
            product,
            patchline,
            auto_launch,
        } => json!({
            "kind": "product-context",
            "product": product,
            "patchline": patchline,
            "autoLaunch": auto_launch,
        }),
        LogEvent::Lifecycle { from, to } => json!({ "kind": "lifecycle", "from": from, "to": to }),
        LogEvent::LaunchStarting { product, patchline } => {
            json!({ "kind": "launch-starting", "product": product, "patchline": patchline })
        }
        LogEvent::LaunchDeclined { reason } => {
            json!({ "kind": "launch-declined", "reason": reason })
        }
        LogEvent::PatchState {
            product,
            patchline,
            status,
            launchable,
        } => json!({
            "kind": "patch-state",
            "product": product,
            "patchline": patchline,
            "status": status,
            "launchable": launchable,
        }),
        LogEvent::PslState { state } => json!({ "kind": "psl-state", "state": state }),
        LogEvent::ManualSignIn => json!({ "kind": "manual-sign-in" }),
        LogEvent::SignedIn { how: by } => json!({ "kind": "signed-in", "how": how(*by) }),
        LogEvent::SessionRejected { detail } => {
            json!({ "kind": "session-rejected", "detail": detail })
        }
        LogEvent::AuthError { error } => json!({ "kind": "auth-error", "error": error }),
        LogEvent::NoSession => json!({ "kind": "no-session" }),
        LogEvent::SignedOut => json!({ "kind": "signed-out" }),
        LogEvent::SettingsWritten { key } => json!({ "kind": "settings-written", "key": key }),
        LogEvent::GameSessionEnded { product, patchline } => {
            json!({ "kind": "game-session-ended", "product": product, "patchline": patchline })
        }
        LogEvent::Quit => json!({ "kind": "quit" }),
        LogEvent::Exit => json!({ "kind": "exit" }),
    }
}

fn session_json(read: &SessionRead) -> Value {
    match read {
        SessionRead::SignedOut => json!({ "kind": "signed-out" }),
        SessionRead::Unexpected(reason) => {
            json!({ "kind": "unexpected", "reason": reason.to_string() })
        }
        SessionRead::Session(session) => json!({
            "kind": "session",
            "session": {
                "accountId": session.account_id,
                "riotId": session.riot_id.as_ref().map(|id| json!({
                    "gameName": id.game_name,
                    "tagLine": id.tag_line,
                })),
                "fingerprint": session.fingerprint,
                "dpopBound": session.dpop_bound,
                "writeCount": session.write_count,
            },
        }),
    }
}

#[test]
fn launch_args_cases() {
    let skipped = [
        ("unvalidated-parts", NOT_A_PRODUCT_ID),
        ("adv-astral", NOT_A_PRODUCT_ID),
        ("adv-lone-surrogate", UNPAIRED),
    ];
    run("l1/riot/launchArgs.json", &skipped, |case| {
        let id = &case.args[0];
        let part = |name: &str| id[name].as_str().unwrap_or_default().to_owned();
        let (product, patchline) = (part("product"), part("patchline"));
        match parse_product_id(&format!("{product}.{patchline}")) {
            Some(id) if id.product() == product && id.patchline() == patchline => {
                Answer::Out(json!(launch_args(&id)))
            }
            _ => Answer::Skip(NOT_A_PRODUCT_ID),
        }
    });
}

#[test]
fn parse_installs_cases() {
    let skipped = [("adv-json-lone-surrogate", UNPAIRED)];
    run("l1/riot/parseInstalls.json", &skipped, |case| {
        let installs = parse_installs(&text(&case.args[0]));
        Answer::Out(json!({
            "rcDefault": installs.rc_default,
            "rcLive": installs.rc_live,
            "associatedClients": installs.associated_clients,
        }))
    });
}

#[test]
fn parse_lockfile_cases() {
    run("l1/riot/parseLockfile.json", &[], |case| {
        Answer::Out(match parse_lockfile(&text(&case.args[0])) {
            Some(lock) => json!({ "port": lock.port, "password": lock.password.take().as_str() }),
            None => Value::Null,
        })
    });
}

#[test]
fn parse_log_events_cases() {
    run("l1/riot/parseLogEvents.json", &[], |case| {
        let events = parse_log_events(&text(&case.args[0]));
        Answer::Out(events.iter().map(event_json).collect())
    });
}

#[test]
fn parse_product_settings_cases() {
    run("l1/riot/parseProductSettings.json", &[], |case| {
        let settings = parse_product_settings(&text(&case.args[0]));
        Answer::Out(json!({
            "installPath": settings.install_path,
            "installRoot": settings.install_root,
            "dependency": settings.dependency,
        }))
    });
}

#[test]
fn read_session_cases() {
    let skipped = [("adv-name-lone-surrogate", UNPAIRED)];
    run("l1/riot/readSession.json", &skipped, |case| {
        Answer::Out(session_json(&read_session(&text(&case.args[0]))))
    });
}

#[test]
fn plan_sign_out_cases() {
    run("l1/riot/planSignOut.json", &[], |case| {
        let before = text(&case.args[0]);
        match plan_sign_out(&before) {
            // A file that is signed out already comes back as it is, mixed line ends and all.
            Ok(write) if write.noop => Answer::Out(json!({ "text": write.text, "noop": true })),
            Ok(write) => Answer::Out(json!({
                "text": as_recorded(&before, write.text),
                "noop": false,
            })),
            Err(left_alone) => error(left_alone),
        }
    });
}

#[test]
fn splice_session_block_cases() {
    let skipped = [("adv-lone-surrogate-body", UNPAIRED)];
    run("l1/riot/spliceSessionBlock.json", &skipped, |case| {
        let before = text(&case.args[0]);
        let body: Option<Vec<String>> = match plain(&case.args[1]) {
            Value::Null => None,
            Value::Array(lines) => {
                let lines: Option<Vec<String>> = lines
                    .iter()
                    .map(|line| line.as_str().map(str::to_owned))
                    .collect();
                match lines {
                    Some(lines) => Some(lines),
                    None => return Answer::Skip(NOT_LINES),
                }
            }
            _ => return Answer::Skip(NOT_LINES),
        };
        let lines: Option<Vec<&str>> = body
            .as_ref()
            .map(|lines| lines.iter().map(String::as_str).collect());
        match splice_session_block(&before, lines.as_deref()) {
            Ok(written) => Answer::Out(as_recorded(&before, written).into()),
            Err(refused) => error(refused),
        }
    });
}

// Six recorded files have mixed line ends: there the port is held to the recording with the line ends made alike, and to its own rule, that no byte outside the block changes, by the test beside the splice.
#[test]
fn the_cases_with_mixed_line_ends_are_the_six_known() {
    let mixed = |file: &str| -> Vec<String> {
        let cases = ol_testkit::vectors::Vectors::load(file).cases;
        let mixed = cases.into_iter().filter(|case| {
            !case.has_unpaired_surrogate() && has_mixed_line_ends(&text(&case.args[0]))
        });
        mixed.map(|case| case.id).collect()
    };
    assert_eq!(
        mixed("l1/riot/planSignOut.json"),
        [
            "mixed-eol",
            "mixed-eol-noop",
            "corpus-51-61",
            "corpus-51-64"
        ]
    );
    assert_eq!(
        mixed("l1/riot/spliceSessionBlock.json"),
        ["mixed-eol", "corpus-42-1"]
    );
}
