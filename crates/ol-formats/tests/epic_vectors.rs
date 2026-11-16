#![forbid(unsafe_code)]

// Every case of oracle/l1/epic, run against the port. A case is left out only for a reason named here, and the test says which cases it left out.

use ol_formats::epic::ini::{Eol, IniLayout, join_ini, rewrite_section, split_ini};
use ol_formats::epic::launcher_log::{
    LogEvent, SignInOutcome, is_session_rejection, last_sign_in, parse_log_events,
};
use ol_formats::epic::manifests::{
    EpicManifestError, LaunchUriForm, launch_uri, parse_item_manifest, parse_launcher_installed,
};
use ol_formats::epic::session::{EpicSessionBlob, parse_blob, plan_session_write, serialize_blob};
use ol_testkit::vectors::{Answer, Case, UNPAIRED, plain, run, text};
use serde_json::{Value, json};

const NOT_TEXT: &str = "not text, which the Rust type rules out";
const NOT_A_BLOB: &str =
    "not a session as the app's own type holds one, which the Rust type rules out";
const NOT_AN_EOL: &str = "a line end that is neither CRLF nor LF, which the Rust type rules out";
const NOT_EVENTS: &str = "not a list of log events, which the Rust type rules out";
const V8_WORDING: &str =
    "refused by both; the reason is the JavaScript engine's wording of a JSON error";
const TOO_DEEP: &str = "nested deeper than the 128 levels the JSON reader takes";
const TOO_LARGE: &str = "a number past what the JSON reader takes";
const LOST_CHARACTER: &str = "a file with CRLF that ends in a bare LF, where the Electron app drops the character before that LF; the port keeps it, because every other byte is left alone (plan/GAPS.md)";
const LOSES_A_CHARACTER: [&str; 5] = [
    "mixed-crlf-then-lf-end",
    "mixed-crlf-then-lf-end-long",
    "mixed-eol-set",
    "mixed-eol-noop",
    "corpus-3-23",
];

fn loses_a_character(case: &Case) -> bool {
    LOSES_A_CHARACTER.contains(&case.id.as_str())
}

const SHOWN_WHOLE: &str = "a Data value of 8 characters or fewer, which the Electron app shows whole in a plan; the port shows its length, because a session value is never shown (plan/GAPS.md)";
const SHOWS_A_SHORT_VALUE: [&str; 8] = [
    "enable-lowercase",
    "data-short-plain",
    "describe-plain-at-8",
    "describe-base64-short",
    "adv-astral-length-over",
    "adv-astral-length-at",
    "adv-astral-length-straddle",
    "adv-fingerprint-non-ascii",
];

fn eol(layout: &IniLayout) -> &'static str {
    layout.eol.as_str()
}

fn layout_json(layout: &IniLayout) -> Value {
    json!({ "lines": layout.lines, "eol": eol(layout), "trailingNewline": layout.trailing_newline })
}

fn layout_from(value: &Value) -> Result<IniLayout, &'static str> {
    let lines = value["lines"]
        .as_array()
        .and_then(|lines| {
            lines
                .iter()
                .map(|line| line.as_str().map(str::to_owned))
                .collect()
        })
        .ok_or(NOT_TEXT)?;
    let eol = match value["eol"].as_str() {
        Some("\r\n") => Eol::CrLf,
        Some("\n") => Eol::Lf,
        _ => return Err(NOT_AN_EOL),
    };
    Ok(IniLayout {
        lines,
        eol,
        trailing_newline: value["trailingNewline"].as_bool().ok_or(NOT_TEXT)?,
    })
}

// A session exactly as the app's own type holds it: its keys in the type's order and every value of the type's kind.
fn blob_from(value: &Value) -> Option<EpicSessionBlob> {
    let keys = |value: &Value| -> Vec<String> {
        value
            .as_object()
            .map(|fields| fields.keys().cloned().collect())
            .unwrap_or_default()
    };
    let remember = &value["rememberMe"];
    let offline = match &value["offline"] {
        Value::Null => None,
        offline if keys(offline) == ["data"] => Some(offline["data"].as_str()?.to_owned()),
        _ => return None,
    };
    let shaped = keys(value) == ["v", "rememberMe", "offline"]
        && keys(remember) == ["enable", "data"]
        && value["v"] == 1;
    shaped.then_some(())?;
    Some(EpicSessionBlob {
        enable: remember["enable"].as_str()?.to_owned(),
        data: remember["data"].as_str()?.to_owned(),
        offline,
    })
}

fn blob_json(blob: &EpicSessionBlob) -> Value {
    json!({
        "v": 1,
        "rememberMe": { "enable": blob.enable, "data": blob.data },
        "offline": blob.offline.as_ref().map(|data| json!({ "data": data })),
    })
}

#[test]
fn split_ini_cases() {
    let skipped = [
        ("mixed-crlf-then-lf-end", LOST_CHARACTER),
        ("mixed-crlf-then-lf-end-long", LOST_CHARACTER),
    ];
    run("l1/epic/splitIni.json", &skipped, |case| {
        if loses_a_character(case) {
            return Answer::Skip(LOST_CHARACTER);
        }
        Answer::Out(layout_json(&split_ini(&text(&case.args[0]))))
    });
}

#[test]
fn join_ini_cases() {
    let skipped = [("adv-lone-surrogate-line", UNPAIRED)];
    run("l1/epic/joinIni.json", &skipped, |case| {
        match layout_from(&case.args[0]) {
            Ok(layout) => Answer::Out(join_ini(&layout).into()),
            Err(reason) => Answer::Skip(reason),
        }
    });
}

#[test]
fn join_ini_round_trips() {
    let skipped = [
        ("mixed-crlf-then-lf-end", LOST_CHARACTER),
        ("mixed-crlf-then-lf-end-long", LOST_CHARACTER),
    ];
    run("l1/epic/joinIni.roundtrip.json", &skipped, |case| {
        if loses_a_character(case) {
            return Answer::Skip(LOST_CHARACTER);
        }
        let before = text(&case.args[0]);
        let rejoined = join_ini(&split_ini(&before));
        Answer::Out(json!({ "rejoined": rejoined, "identical": rejoined == before }))
    });
}

#[test]
fn rewrite_section_cases() {
    let skipped = [
        ("mixed-eol-set", LOST_CHARACTER),
        ("mixed-eol-noop", LOST_CHARACTER),
        ("adv-lone-surrogate-value", UNPAIRED),
        ("corpus-3-23", LOST_CHARACTER),
    ];
    run("l1/epic/rewriteSection.json", &skipped, |case| {
        if loses_a_character(case) {
            return Answer::Skip(LOST_CHARACTER);
        }
        let (Value::String(name), Some(entries)) = (plain(&case.args[1]), case.args[2].as_object())
        else {
            return Answer::Skip(NOT_TEXT);
        };
        let Some(entries) = entries
            .iter()
            .map(|(key, value)| match value {
                Value::Null => Some((key.as_str(), None)),
                Value::String(value) => Some((key.as_str(), Some(value.as_str()))),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
        else {
            return Answer::Skip(NOT_TEXT);
        };
        let rewrite = rewrite_section(&text(&case.args[0]), &name, &entries);
        Answer::Out(json!({
            "text": rewrite.text,
            "changes": rewrite.changes.iter().map(|change| json!({
                "key": change.key,
                "line": change.line,
                "before": change.before,
                "after": change.after,
            })).collect::<Vec<_>>(),
            "noop": rewrite.noop,
        }))
    });
}

#[test]
fn a_file_with_crlf_that_ends_in_a_bare_lf_keeps_every_character_of_its_last_line() {
    let text = "[RememberMe]\r\nEnable=True\r\nData=old\r\n\r\n[Other]\r\nOther=42\n";
    let rewrite = rewrite_section(text, "RememberMe", &[("Data", Some("new"))]);
    assert_eq!(
        rewrite.text,
        "[RememberMe]\r\nEnable=True\r\nData=new\r\n\r\n[Other]\r\nOther=42\r\n"
    );
    assert_eq!(split_ini("[A]\r\nx=1\n").lines, ["[A]", "x=1"]);
    assert_eq!(split_ini("[A]\r\nx=1\r\n\n").lines, ["[A]", "x=1", ""]);
    assert_eq!(split_ini("[A]\r\nx=1\r\n").lines, ["[A]", "x=1"]);
}

#[test]
fn a_short_data_value_is_shown_by_its_length_and_never_whole() {
    let text = "[RememberMe]\r\nEnable=True\r\nData=abc\r\n\r\n[Offline]\r\nData=Zz9\r\n";
    let blob = EpicSessionBlob {
        enable: "True".to_owned(),
        data: "QUJDREVG".to_owned(),
        offline: Some("x1".to_owned()),
    };
    let shown = |write: ol_formats::epic::session::SessionWrite| {
        write
            .changes
            .iter()
            .map(|change| format!("{} -> {}", change.before, change.after))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        shown(plan_session_write(text, Some(&blob))),
        [
            "(3 characters) -> (8 characters)",
            "(3 characters) -> (2 characters)"
        ]
    );
    assert_eq!(
        shown(plan_session_write(text, None)),
        ["(3 characters) -> (removed)", "(3 characters) -> (removed)"]
    );
}

#[test]
fn plan_session_write_cases() {
    let skipped = [
        ("enable-lowercase", SHOWN_WHOLE),
        ("data-short-plain", SHOWN_WHOLE),
        ("describe-plain-at-8", SHOWN_WHOLE),
        ("describe-base64-short", SHOWN_WHOLE),
        ("adv-astral-length-over", SHOWN_WHOLE),
        ("adv-astral-length-at", SHOWN_WHOLE),
        ("adv-astral-length-straddle", SHOWN_WHOLE),
        ("adv-lone-surrogate-data", UNPAIRED),
        ("adv-fingerprint-non-ascii", SHOWN_WHOLE),
    ];
    run("l1/epic/planSessionWrite.json", &skipped, |case| {
        if SHOWS_A_SHORT_VALUE.contains(&case.id.as_str()) {
            return Answer::Skip(SHOWN_WHOLE);
        }
        let blob = match &case.args[1] {
            Value::Null => None,
            given => match blob_from(given) {
                Some(blob) => Some(blob),
                None => return Answer::Skip(NOT_A_BLOB),
            },
        };
        let write = plan_session_write(&text(&case.args[0]), blob.as_ref());
        Answer::Out(json!({
            "text": write.text,
            "changes": write.changes.iter().map(|change| json!({
                "where": change.place,
                "before": change.before,
                "after": change.after,
            })).collect::<Vec<_>>(),
            "noop": write.noop,
        }))
    });
}

#[test]
fn serialize_blob_cases() {
    run(
        "l1/epic/serializeBlob.json",
        &[
            ("key-order-as-given", NOT_A_BLOB),
            ("adv-lone-surrogate", UNPAIRED),
        ],
        |case| match blob_from(&case.args[0]) {
            Some(blob) => Answer::Out(serialize_blob(&blob).into()),
            None => Answer::Skip(NOT_A_BLOB),
        },
    );
}

// How deep the brackets of a JSON text go, outside its strings.
fn depth(json: &str) -> usize {
    let (mut deepest, mut now, mut quoted, mut escaped) = (0_usize, 0_usize, false, false);
    for c in json.chars() {
        match c {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            '[' | '{' if !quoted => {
                now += 1;
                deepest = deepest.max(now);
            }
            ']' | '}' if !quoted => now = now.saturating_sub(1),
            _ => {}
        }
    }
    deepest
}

fn expects_error_named(case: &Case, name: &str) -> bool {
    case.error().is_some_and(|error| error["name"] == name)
}

#[test]
fn parse_blob_cases() {
    let skipped = [
        ("syntax-empty", V8_WORDING),
        ("syntax-not-json", V8_WORDING),
        ("syntax-trailing-comma", V8_WORDING),
        ("syntax-truncated", V8_WORDING),
        ("syntax-single-quotes", V8_WORDING),
        ("syntax-trailing-junk", V8_WORDING),
        ("syntax-bom", V8_WORDING),
        ("adv-nbsp-around", V8_WORDING),
        ("adv-lone-surrogate-escape", UNPAIRED),
        ("adv-depth-129", TOO_DEEP),
        ("adv-depth-200", TOO_DEEP),
        ("adv-depth-5000", TOO_DEEP),
        ("adv-control-char", V8_WORDING),
    ];
    run("l1/epic/parseBlob.json", &skipped, |case| {
        let Value::String(json) = plain(&case.args[0]) else {
            return Answer::Skip(NOT_TEXT);
        };
        match parse_blob(&json) {
            Ok(blob) => Answer::Out(blob_json(&blob)),
            Err(_) if depth(&json) > 127 => Answer::Skip(TOO_DEEP),
            Err(_) if expects_error_named(case, "SyntaxError") => Answer::Skip(V8_WORDING),
            Err(error) => Answer::Error(json!({ "name": "Error", "message": error.to_string() })),
        }
    });
}

fn manifest_error(case: &Case, json: &str, error: &EpicManifestError) -> Answer {
    let EpicManifestError::NotJson { what, .. } = error else {
        return Answer::Error(json!({
            "name": "EpicManifestError",
            "message": error.to_string(),
            "code": EpicManifestError::CODE,
        }));
    };
    let says_not_json = case.error().is_some_and(|expected| {
        expected["message"]
            .as_str()
            .is_some_and(|message| message.starts_with(&format!("{what}not valid JSON (")))
    });
    if says_not_json {
        Answer::Skip(V8_WORDING)
    } else if depth(json) > 127 {
        Answer::Skip(TOO_DEEP)
    } else if error.to_string().contains("number out of range") {
        Answer::Skip(TOO_LARGE)
    } else {
        Answer::Error(json!({ "name": "EpicManifestError", "message": error.to_string() }))
    }
}

#[test]
fn parse_item_manifest_cases() {
    let skipped = [
        ("test-not-json", V8_WORDING),
        ("double-bom", V8_WORDING),
        ("empty", V8_WORDING),
        ("truncated", V8_WORDING),
        ("adv-size-huge", TOO_LARGE),
        ("adv-lone-surrogate", UNPAIRED),
        ("adv-depth-200", TOO_DEEP),
        ("adv-nbsp-before", V8_WORDING),
    ];
    run("l1/epic/parseItemManifest.json", &skipped, |case| {
        let json = text(&case.args[0]);
        match parse_item_manifest(&json) {
            Ok(manifest) => Answer::Out(json!({
                "installationGuid": manifest.installation_guid,
                "appName": manifest.app_name,
                "displayName": manifest.display_name,
                "catalogNamespace": manifest.catalog_namespace,
                "catalogItemId": manifest.catalog_item_id,
                "installLocation": manifest.install_location,
                "launchExecutable": manifest.launch_executable,
                "installSize": manifest.install_size,
                "incomplete": manifest.incomplete,
                "mainGameAppName": manifest.main_game_app_name,
                "categories": manifest.categories,
            })),
            Err(error) => manifest_error(case, &json, &error),
        }
    });
}

#[test]
fn parse_launcher_installed_cases() {
    let skipped = [
        ("double-bom", V8_WORDING),
        ("not-json", V8_WORDING),
        ("empty", V8_WORDING),
        ("truncated", V8_WORDING),
        ("adv-lone-surrogate", UNPAIRED),
    ];
    run("l1/epic/parseLauncherInstalled.json", &skipped, |case| {
        let json = text(&case.args[0]);
        match parse_launcher_installed(&json) {
            Ok(entries) => Answer::Out(
                entries
                    .iter()
                    .map(|entry| {
                        json!({
                            "appName": entry.app_name,
                            "installLocation": entry.install_location,
                            "namespaceId": entry.namespace_id,
                            "itemId": entry.item_id,
                        })
                    })
                    .collect(),
            ),
            Err(error) => manifest_error(case, &json, &error),
        }
    });
}

#[test]
fn launch_uri_cases() {
    let skipped = [
        ("adv-lone-high-surrogate", UNPAIRED),
        ("adv-lone-low-surrogate", UNPAIRED),
        ("adv-lone-surrogate-in-ids", UNPAIRED),
    ];
    run("l1/epic/launchUri.json", &skipped, |case| {
        let manifest = plain(&case.args[0]);
        let optional = |name: &str| match &manifest[name] {
            Value::Null => Some(None),
            Value::String(id) => Some(Some(id.clone())),
            _ => None,
        };
        let (Some(app_name), Some(namespace), Some(item), Value::String(form)) = (
            manifest["appName"].as_str(),
            optional("catalogNamespace"),
            optional("catalogItemId"),
            plain(&case.args[1]),
        ) else {
            return Answer::Skip(NOT_TEXT);
        };
        let form = if form == "catalog" {
            LaunchUriForm::Catalog
        } else {
            LaunchUriForm::AppName
        };
        Answer::Out(launch_uri(app_name, namespace.as_deref(), item.as_deref(), form).into())
    });
}

fn event_json(event: &LogEvent) -> Value {
    match event {
        LogEvent::AutoSignIn => json!({ "kind": "auto-sign-in" }),
        LogEvent::ManualSignIn => json!({ "kind": "manual-sign-in" }),
        LogEvent::SignedIn { account_id, at } => {
            json!({ "kind": "signed-in", "accountId": account_id, "at": at })
        }
        LogEvent::SignInFailed { reason } => json!({ "kind": "sign-in-failed", "reason": reason }),
        LogEvent::Ready => json!({ "kind": "ready" }),
        LogEvent::Restart => json!({ "kind": "restart" }),
        LogEvent::Exit => json!({ "kind": "exit" }),
    }
}

fn event_from(value: &Value) -> Option<LogEvent> {
    let text = |name: &str| value[name].as_str().map(str::to_owned);
    let fields = value.as_object()?.len();
    Some(match (value["kind"].as_str()?, fields) {
        ("auto-sign-in", 1) => LogEvent::AutoSignIn,
        ("manual-sign-in", 1) => LogEvent::ManualSignIn,
        ("signed-in", 3) => LogEvent::SignedIn {
            account_id: text("accountId")?,
            at: match &value["at"] {
                Value::Null => None,
                _ => Some(text("at")?),
            },
        },
        ("sign-in-failed", 2) => LogEvent::SignInFailed {
            reason: text("reason")?,
        },
        ("ready", 1) => LogEvent::Ready,
        ("restart", 1) => LogEvent::Restart,
        ("exit", 1) => LogEvent::Exit,
        _ => return None,
    })
}

#[test]
fn parse_log_events_cases() {
    run("l1/epic/parseLogEvents.json", &[], |case| {
        Answer::Out(
            parse_log_events(&text(&case.args[0]))
                .iter()
                .map(event_json)
                .collect(),
        )
    });
}

#[test]
fn last_sign_in_cases() {
    run("l1/epic/lastSignIn.json", &[], |case| {
        let events: Option<Vec<LogEvent>> = plain(&case.args[0])
            .as_array()
            .and_then(|events| events.iter().map(event_from).collect());
        let Some(events) = events else {
            return Answer::Skip(NOT_EVENTS);
        };
        Answer::Out(match last_sign_in(&events) {
            None => Value::Null,
            Some(SignInOutcome::SignedIn { account_id, at }) => {
                json!({ "ok": true, "accountId": account_id, "at": at })
            }
            Some(SignInOutcome::Failed { reason }) => json!({ "ok": false, "reason": reason }),
        })
    });
}

#[test]
fn is_session_rejection_cases() {
    run("l1/epic/isSessionRejection.json", &[], |case| {
        match plain(&case.args[0]) {
            Value::String(reason) => Answer::Out(is_session_rejection(&reason).into()),
            _ => Answer::Skip(NOT_TEXT),
        }
    });
}
