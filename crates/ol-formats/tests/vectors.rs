#![forbid(unsafe_code)]

// Every case of oracle/l1 for the Steam text formats and the path rules, run against the port. A case is left out only for a reason named here, and the test says which cases it left out.

use ol_formats::steam::bootstrap_log::update_in_progress;
use ol_formats::steam::login_log::{
    LoginStateEvent, SignInOutcome, classify_reason, latest_outcome, parse_login_states,
};
use ol_formats::steam::loginusers::{
    LoginUsersError, MostRecentKey, SteamLoginUser, find_login_user, parse_login_users,
    plan_auto_login_rewrite,
};
use ol_formats::steam::manifests::{
    ManifestError, is_fully_installed, needs_update, parse_app_manifest, parse_library_folders,
};
use ol_formats::vdf::{self, VdfObject, VdfParseError, VdfValue};
use ol_formats::winpath::{normalize_launcher_path, path_key};
use ol_testkit::vectors::{Case, Vectors, difference, plain, text};
use serde_json::{Value, json};

const UNPAIRED: &str = "half a surrogate pair, which no Rust string holds";
const NOT_TEXT: &str = "not text, which the Rust type rules out";
const NOT_WHOLE: &str = "not a whole number, which the Rust type rules out";

enum Answer {
    Out(Value),
    Error(Value),
    Skip(&'static str),
}

fn run(file: &str, skipped: &[(&str, &str)], call: impl Fn(&Case) -> Answer) {
    let vectors = Vectors::load(file);
    let mut failures = Vec::new();
    let mut left_out = Vec::new();
    let mut passed = 0;
    for case in &vectors.cases {
        let answer = if case.has_unpaired_surrogate() {
            Answer::Skip(UNPAIRED)
        } else {
            call(case)
        };
        let failure = match (&answer, case.out(), case.error()) {
            (Answer::Skip(reason), _, _) => {
                left_out.push((case.id.as_str(), *reason));
                continue;
            }
            (Answer::Out(actual), Some(expected), _) => difference(actual, &expected),
            (Answer::Error(actual), _, Some(expected)) => {
                (actual != &Value::Object(expected.clone())).then(|| {
                    format!(
                        "error {actual}, expected {}",
                        Value::Object(expected.clone())
                    )
                })
            }
            (Answer::Out(actual), None, Some(expected)) => Some(format!(
                "returned {actual}, expected the error {}",
                Value::Object(expected.clone())
            )),
            (Answer::Error(actual), Some(expected), _) => {
                Some(format!("failed with {actual}, expected {expected}"))
            }
            _ => unreachable!("a case has an out or an error"),
        };
        match failure {
            Some(failure) => failures.push(format!("{}: {failure}", case.id)),
            None => passed += 1,
        }
    }
    println!(
        "{}: {} cases, {passed} passed, {} left out",
        vectors.function,
        vectors.cases.len(),
        left_out.len()
    );
    assert!(
        failures.is_empty(),
        "{} of {} cases of {file} differ:\n{}",
        failures.len(),
        vectors.cases.len(),
        failures.join("\n")
    );
    assert_eq!(left_out, skipped, "the cases of {file} left out");
}

fn vdf_error(error: &VdfParseError) -> Value {
    json!({ "name": "VdfParseError", "message": error.to_string(), "line": error.line, "code": VdfParseError::CODE })
}

fn login_users_error(error: &LoginUsersError) -> Answer {
    Answer::Error(match error {
        LoginUsersError::Vdf(error) => vdf_error(error),
        LoginUsersError::Format(_) => {
            json!({ "name": "LoginUsersFormatError", "message": error.to_string(), "code": error.code() })
        }
    })
}

fn manifest_error(error: &ManifestError) -> Answer {
    Answer::Error(match error {
        ManifestError::Vdf(error) => vdf_error(error),
        ManifestError::Format(_) => {
            json!({ "name": "ManifestFormatError", "message": error.to_string(), "code": error.code() })
        }
    })
}

fn object_json(object: &VdfObject) -> Value {
    Value::Object(
        object
            .entries()
            .map(|(key, value)| {
                let value = match value {
                    VdfValue::Text(text) => Value::String(text.clone()),
                    VdfValue::Block(block) => object_json(block),
                };
                (key, value)
            })
            .collect(),
    )
}

fn object_from(value: &Value) -> Option<VdfObject> {
    let mut object = VdfObject::default();
    for (key, value) in value.as_object()? {
        let value = match value {
            Value::String(text) => VdfValue::Text(text.clone()),
            Value::Object(_) => VdfValue::Block(object_from(value)?),
            _ => return None,
        };
        object.insert(key, value);
    }
    Some(object)
}

fn user_json(user: &SteamLoginUser) -> Value {
    json!({
        "steamId64": user.steam_id64,
        "accountName": user.account_name,
        "personaName": user.persona_name,
        "rememberPassword": user.remember_password,
        "mostRecent": user.most_recent,
        "timestamp": user.timestamp,
        "mostRecentKeys": user.most_recent_keys.iter().map(|key| key.as_str()).collect::<Vec<_>>(),
    })
}

fn user_from(value: &Value) -> Option<SteamLoginUser> {
    let keys = value["mostRecentKeys"]
        .as_array()?
        .iter()
        .map(|key| {
            MostRecentKey::ALL
                .into_iter()
                .find(|known| Some(known.as_str()) == key.as_str())
        })
        .collect::<Option<Vec<_>>>()?;
    Some(SteamLoginUser {
        steam_id64: value["steamId64"].as_str()?.to_owned(),
        account_name: value["accountName"].as_str()?.to_owned(),
        persona_name: value["personaName"].as_str()?.to_owned(),
        remember_password: value["rememberPassword"].as_bool()?,
        most_recent: value["mostRecent"].as_bool()?,
        timestamp: match &value["timestamp"] {
            Value::Null => None,
            other => Some(other.as_i64()?),
        },
        most_recent_keys: keys,
    })
}

fn lookup(file: &str, skipped: &[(&str, &str)], answer: impl Fn(&VdfObject, &str) -> Value) {
    run(file, skipped, |case| {
        let (Some(object), Value::String(key)) = (object_from(&case.args[0]), plain(&case.args[1]))
        else {
            return Answer::Skip(NOT_TEXT);
        };
        Answer::Out(answer(&object, &key))
    });
}

fn whole(arg: &Value) -> Option<i64> {
    let number = plain(arg);
    number.as_i64().or_else(|| {
        let float = number.as_f64()?;
        (float.fract() == 0.0 && float.abs() < 9.0e15).then_some(float as i64)
    })
}

#[test]
fn parse_vdf() {
    run("l1/steam/parseVdf.json", &[], |case| {
        match vdf::parse(&text(&case.args[0])) {
            Ok(object) => Answer::Out(object_json(&object)),
            Err(error) => Answer::Error(vdf_error(&error)),
        }
    });
}

#[test]
fn vdf_get() {
    lookup(
        "l1/steam/vdfGet.json",
        &[("lone-surrogate-key", UNPAIRED)],
        |object, key| match object.get(key) {
            Some(VdfValue::Text(text)) => Value::String(text.clone()),
            Some(VdfValue::Block(block)) => object_json(block),
            None => Value::Null,
        },
    );
}

#[test]
fn vdf_has() {
    lookup(
        "l1/steam/vdfHas.json",
        &[("lone-surrogate-key", UNPAIRED)],
        |object, key| object.has(key).into(),
    );
}

#[test]
fn vdf_string() {
    lookup(
        "l1/steam/vdfString.json",
        &[("lone-surrogate-key", UNPAIRED)],
        |object, key| object.string(key).into(),
    );
}

#[test]
fn vdf_object() {
    lookup(
        "l1/steam/vdfObject.json",
        &[("lone-surrogate-key", UNPAIRED)],
        |object, key| object.block(key).map_or(Value::Null, object_json),
    );
}

#[test]
fn vdf_int() {
    let skipped = [("lone-surrogate-key", UNPAIRED)];
    lookup("l1/steam/vdfInt.json", &skipped, |object, key| {
        object.int(key).into()
    });
}

#[test]
fn vdf_flag() {
    lookup(
        "l1/steam/vdfFlag.json",
        &[("lone-surrogate-key", UNPAIRED)],
        |object, key| object.flag(key).into(),
    );
}

#[test]
fn parse_login_users_cases() {
    run(
        "l1/steam/parseLoginUsers.json",
        &[],
        |case| match parse_login_users(&text(&case.args[0])) {
            Ok(users) => Answer::Out(users.iter().map(user_json).collect()),
            Err(error) => login_users_error(&error),
        },
    );
}

#[test]
fn find_login_user_cases() {
    run("l1/steam/findLoginUser.json", &[], |case| {
        let Some(users) = case.args[0]
            .as_array()
            .and_then(|users| users.iter().map(user_from).collect::<Option<Vec<_>>>())
        else {
            return Answer::Skip(NOT_TEXT);
        };
        let reference = plain(&case.args[1]);
        let (uid, Some(login)) = (
            reference["launcherUid"].as_str(),
            reference["login"].as_str(),
        ) else {
            return Answer::Skip(NOT_TEXT);
        };
        Answer::Out(find_login_user(&users, uid, login).map_or(Value::Null, user_json))
    });
}

#[test]
fn plan_auto_login_rewrite_cases() {
    let skipped = [("undefined-target", NOT_TEXT)];
    run("l1/steam/planAutoLoginRewrite.json", &skipped, |case| {
        let Value::String(target) = plain(&case.args[1]) else {
            return Answer::Skip(NOT_TEXT);
        };
        match plan_auto_login_rewrite(&text(&case.args[0]), &target) {
            Ok(rewrite) => Answer::Out(json!({
                "text": rewrite.text,
                "changes": rewrite.changes.iter().map(|change| json!({
                    "line": change.line,
                    "before": change.before,
                    "after": change.after,
                })).collect::<Vec<_>>(),
                "noop": rewrite.noop,
            })),
            Err(error) => login_users_error(&error),
        }
    });
}

#[test]
fn parse_library_folders_cases() {
    run(
        "l1/steam/parseLibraryFolders.json",
        &[],
        |case| match parse_library_folders(&text(&case.args[0])) {
            Ok(folders) => Answer::Out(folders.into()),
            Err(error) => manifest_error(&error),
        },
    );
}

#[test]
fn parse_app_manifest_cases() {
    run(
        "l1/steam/parseAppManifest.json",
        &[],
        |case| match parse_app_manifest(&text(&case.args[0])) {
            Ok(manifest) => Answer::Out(json!({
                "appId": manifest.app_id,
                "name": manifest.name,
                "installDir": manifest.install_dir,
                "stateFlags": manifest.state_flags,
                "lastPlayed": manifest.last_played,
                "sizeOnDisk": manifest.size_on_disk,
                "buildId": manifest.build_id,
            })),
            Err(error) => manifest_error(&error),
        },
    );
}

fn flags(file: &str, skipped: &[(&str, &str)], test: fn(i64) -> bool) {
    run(file, skipped, |case| match whole(&case.args[0]) {
        Some(flags) => Answer::Out(test(flags).into()),
        None => Answer::Skip(NOT_WHOLE),
    });
}

#[test]
fn is_fully_installed_cases() {
    flags(
        "l1/steam/isFullyInstalled.json",
        &[("flags-fraction", NOT_WHOLE)],
        is_fully_installed,
    );
}

#[test]
fn needs_update_cases() {
    flags(
        "l1/steam/needsUpdate.json",
        &[("flags-fraction", NOT_WHOLE)],
        needs_update,
    );
}

fn event_json(event: &LoginStateEvent) -> Value {
    json!({ "state": event.state, "reason": event.reason })
}

#[test]
fn parse_login_states_cases() {
    run("l1/steam/parseLoginStates.json", &[], |case| {
        Answer::Out(
            parse_login_states(&text(&case.args[0]))
                .iter()
                .map(event_json)
                .collect(),
        )
    });
}

#[test]
fn classify_reason_cases() {
    run(
        "l1/steam/classifyReason.json",
        &[("lone-surrogate", UNPAIRED)],
        |case| match plain(&case.args[0]) {
            Value::String(reason) => Answer::Out(classify_reason(&reason).as_str().into()),
            _ => Answer::Skip(NOT_TEXT),
        },
    );
}

pub fn outcome_json(outcome: Option<SignInOutcome>) -> Value {
    match outcome {
        None => Value::Null,
        Some(SignInOutcome::Success) => json!({ "kind": "success" }),
        Some(SignInOutcome::Credentials { reason, cause }) => {
            json!({ "kind": "credentials", "reason": reason, "cause": cause.as_str() })
        }
    }
}

#[test]
fn latest_outcome_cases() {
    run("l1/steam/latestOutcome.json", &[], |case| {
        let events: Option<Vec<LoginStateEvent>> = case.args[0].as_array().and_then(|events| {
            events
                .iter()
                .map(|event| {
                    Some(LoginStateEvent {
                        state: event["state"].as_str()?.to_owned(),
                        reason: event["reason"].as_str()?.to_owned(),
                    })
                })
                .collect()
        });
        match events {
            Some(events) => Answer::Out(outcome_json(latest_outcome(&events))),
            None => Answer::Skip(NOT_TEXT),
        }
    });
}

#[test]
fn update_in_progress_cases() {
    run("l1/steam/updateInProgress.json", &[], |case| {
        Answer::Out(update_in_progress(&text(&case.args[0])).into())
    });
}

fn path(file: &str, call: fn(&str) -> String) {
    run(
        file,
        &[("lone-surrogate", UNPAIRED), ("not-a-string", NOT_TEXT)],
        |case| match plain(&case.args[0]) {
            Value::String(path) => Answer::Out(call(&path).into()),
            _ => Answer::Skip(NOT_TEXT),
        },
    );
}

#[test]
fn normalize_launcher_path_cases() {
    path(
        "l1/core/normalizeLauncherPath.json",
        normalize_launcher_path,
    );
}

#[test]
fn path_key_cases() {
    path("l1/core/pathKey.json", path_key);
}

#[test]
fn the_keys_of_a_block_come_back_numbers_first() {
    let mut object = VdfObject::default();
    for key in ["b", "10", "a", "2", "007", "b"] {
        object.insert(key, VdfValue::Text(key.to_owned()));
    }
    let keys: Vec<String> = object.entries().map(|(key, _)| key).collect();
    assert_eq!(keys, ["2", "10", "b", "a", "007"]);
}
