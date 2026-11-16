#![forbid(unsafe_code)]

// Every case of oracle/l1/battlenet, run against the port. A case is left out only for a reason named here, and the test says which cases it left out.

use std::collections::HashMap;
use std::fs;

use ol_formats::battlenet::client_log::{
    LogEvent, SignInOutcome, ended_signed_out, is_forwarder_log, last_sign_in, line_time,
    mask_reason, parse_log_events,
};
use ol_formats::battlenet::config::{
    ConfigError, ConfigLayout, detect_layout, mask_login, parse_config, plan_saved_account_names,
    round_trips, serialize_config, split_saved_account_names,
};
use ol_formats::battlenet::product_state::{
    ProductState, game_running, game_started, product_needs_update, product_states,
};
use ol_formats::battlenet::products::{
    LaunchCommand, LaunchForm, PRODUCTS, ProductCodes, known_product, launch_command,
    parse_aggregate, parse_product_db, product_codes,
};
use ol_formats::battlenet::session::{
    BattlenetSessionBlob, TokenValue, parse_blob, serialize_blob,
};
use ol_formats::epic::ini::Eol;
use ol_formats::js_json::{self, JsObject, JsString, JsValue, JsonError, MAX_DEPTH};
use ol_testkit::oracle;
use ol_testkit::vectors::{Answer, Case, UNPAIRED, bytes, plain, run, text};
use serde_json::{Map, Value, json};

const NOT_TEXT: &str = "not text, which the Rust type rules out";
const NOT_A_DOCUMENT: &str = "not a document as JSON.parse gives one: it holds undefined";
const NOT_A_LAYOUT: &str =
    "an indent that is not a whole number from 0 up, which the Rust type rules out";
const NOT_EVENTS: &str = "not a list of log events, which the Rust type rules out";
const NOT_A_STATE: &str = "not a product state, which the Rust type rules out";
const NOT_A_BLOB: &str =
    "not a session as the app's own type holds one, which the Rust type rules out";
const NOT_A_FORM: &str = "a launch form that is neither of the two, which the Rust type rules out";
const NOT_CODES: &str = "not a product's codes, which the Rust type rules out";
const V8_WORDING: &str =
    "refused by both; the reason is the JavaScript engine's wording of a JSON error";
const TOO_DEEP: &str = "nested deeper than the 128 levels the JSON reader takes";
const PROTOTYPE_MEMBER: &str =
    "a name of JavaScript's Object.prototype, which the Electron app's table answers by accident";
const OBJECT_PROTOTYPE: [&str; 12] = [
    "__proto__",
    "constructor",
    "toString",
    "toLocaleString",
    "valueOf",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "__defineGetter__",
    "__defineSetter__",
    "__lookupGetter__",
    "__lookupSetter__",
];

// The cases of a seeded corpus that both sides refuse as JSON, by their numbers.
fn corpus(seed: u8, numbers: &[u16]) -> Vec<String> {
    numbers
        .iter()
        .map(|number| format!("corpus-{seed}-{number}"))
        .collect()
}

fn with_reason<'a>(ids: &'a [String], reason: &'static str) -> Vec<(&'a str, &'static str)> {
    ids.iter().map(|id| (id.as_str(), reason)).collect()
}

fn layout_json(layout: &ConfigLayout) -> Value {
    json!({
        "eol": layout.eol.as_str(),
        "indent": layout.indent,
        "trailingNewline": layout.trailing_newline,
        "escapeSlashes": layout.escape_slashes,
        "bom": layout.bom,
    })
}

fn layout_from(value: &Value) -> Option<ConfigLayout> {
    Some(ConfigLayout {
        eol: match value["eol"].as_str()? {
            "\r\n" => Eol::CrLf,
            "\n" => Eol::Lf,
            _ => return None,
        },
        indent: usize::try_from(value["indent"].as_u64()?).ok()?,
        trailing_newline: value["trailingNewline"].as_bool()?,
        escape_slashes: value["escapeSlashes"].as_bool()?,
        bom: value["bom"].as_bool()?,
    })
}

// The first argument of every case as it is written in the file. The recordings' own reader may round a long number to a neighbouring double, and a document's numbers have to be the ones recorded.
fn first_arguments(file: &str) -> HashMap<String, JsValue> {
    let recorded = fs::read_to_string(oracle(file)).unwrap();
    let root = js_json::parse(&recorded).unwrap_or_else(|error| panic!("{file}: {error}"));
    let cases = root
        .as_object()
        .and_then(|root| root.get("cases")?.as_array());
    cases
        .expect("a vector file has cases")
        .iter()
        .filter_map(|case| {
            let case = case.as_object()?;
            let id = case.get("id")?.as_string()?.to_string_lossy();
            Some((id, case.get("args")?.as_array()?.first()?.clone()))
        })
        .collect()
}

// A recorded value as the document it stands for. None when it holds undefined, which no parsed document does.
fn document(recorded: &JsValue) -> Option<JsValue> {
    Some(match recorded {
        JsValue::Array(items) => JsValue::Array(items.iter().map(document).collect::<Option<_>>()?),
        JsValue::Object(fields) => {
            let mut members = fields.members();
            let wrapper = match (members.next(), members.next()) {
                (Some((key, inner)), None) => Some((key.to_string_lossy(), inner)),
                _ => None,
            };
            match wrapper.as_ref().map(|(key, inner)| (key.as_str(), *inner)) {
                Some(("$undefined", _)) => return None,
                Some(("$number", JsValue::String(number))) => {
                    return Some(JsValue::Number(match number.to_string_lossy().as_str() {
                        "NaN" => f64::NAN,
                        "Infinity" => f64::INFINITY,
                        "-Infinity" => f64::NEG_INFINITY,
                        _ => -0.0,
                    }));
                }
                Some(("$utf16", JsValue::Array(units))) => {
                    let units = units
                        .iter()
                        .map(|unit| Some(unit.as_number()? as u16))
                        .collect::<Option<_>>()?;
                    return Some(JsValue::String(JsString::from_units(units)));
                }
                _ => {}
            }
            let mut object = JsObject::default();
            for (key, member) in fields.members() {
                object.insert(key, document(member)?);
            }
            JsValue::Object(object)
        }
        plain => plain.clone(),
    })
}

// A document as the recordings write one: a number no JSON holds stands in a wrapper.
fn document_json(value: &JsValue) -> Value {
    match value {
        JsValue::Null => Value::Null,
        JsValue::Bool(flag) => (*flag).into(),
        JsValue::Number(number) if number.is_nan() => json!({ "$number": "NaN" }),
        JsValue::Number(number) if number.is_infinite() => {
            json!({ "$number": if *number > 0.0 { "Infinity" } else { "-Infinity" } })
        }
        JsValue::Number(number) => (*number).into(),
        JsValue::String(text) => text.to_string_lossy().into(),
        JsValue::Array(items) => items.iter().map(document_json).collect(),
        JsValue::Object(object) => Value::Object(
            object
                .members()
                .map(|(key, member)| (key.to_string_lossy(), document_json(member)))
                .collect(),
        ),
    }
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

fn config_error(case: &Case, error: &ConfigError) -> Answer {
    let expects_not_json = case.error().is_some_and(|expected| {
        expected["message"]
            .as_str()
            .is_some_and(|message| message.starts_with("not valid JSON ("))
    });
    match error {
        ConfigError::NotJson(JsonError::TooDeep) => Answer::Skip(TOO_DEEP),
        ConfigError::NotJson(_) if expects_not_json => Answer::Skip(V8_WORDING),
        _ => Answer::Error(json!({
            "name": "BattlenetConfigError",
            "message": error.to_string(),
            "code": ConfigError::CODE,
        })),
    }
}

#[test]
fn detect_layout_cases() {
    run("l1/battlenet/detectLayout.json", &[], |case| {
        Answer::Out(layout_json(&detect_layout(&text(&case.args[0]))))
    });
}

#[test]
fn parse_config_cases() {
    let refused = corpus(
        11,
        &[
            0, 1, 2, 3, 4, 5, 6, 8, 9, 10, 11, 13, 16, 18, 19, 20, 21, 22, 24, 25, 26, 27, 28, 29,
            30, 32, 33, 34, 38, 39, 40, 42, 43, 44, 46, 50, 51, 52, 54, 55, 56, 58, 60, 62, 63, 64,
            67, 68, 70, 71, 72, 73, 74, 75, 78, 79, 80, 82, 83, 85, 88, 89, 90, 91, 92, 93, 95, 96,
            98, 99, 100, 101, 102, 103, 104, 106, 107, 108, 109, 110, 112, 115, 116, 119, 120, 121,
            122, 123, 124, 126, 129, 130, 132, 133, 134, 135, 136, 137, 138, 139, 140, 141, 142,
            143, 144, 145, 150, 151, 156, 159,
        ],
    );
    let mut skipped = vec![
        ("test-open-brace", V8_WORDING),
        ("empty", V8_WORDING),
        ("whitespace-only", V8_WORDING),
        ("cut-half", V8_WORDING),
        ("double-bom", V8_WORDING),
        ("utf16le-bom", V8_WORDING),
        ("escaped-lone-surrogate", UNPAIRED),
        ("escaped-lone-surrogate-upper", UNPAIRED),
        ("nbsp-before-object", V8_WORDING),
        ("feff-inside", V8_WORDING),
        ("trailing-comma", V8_WORDING),
        ("trailing-junk", V8_WORDING),
        ("single-quotes", V8_WORDING),
    ];
    skipped.extend(with_reason(&refused, V8_WORDING));
    run(
        "l1/battlenet/parseConfig.json",
        &skipped,
        |case| match parse_config(&text(&case.args[0])) {
            Ok((doc, layout)) => Answer::Out(json!({
                "doc": document_json(&JsValue::Object(doc)),
                "layout": layout_json(&layout),
            })),
            Err(error) => config_error(case, &error),
        },
    );
}

#[test]
fn serialize_config_cases() {
    let file = "l1/battlenet/serializeConfig.json";
    let documents = first_arguments(file);
    let skipped = [
        ("rt-escaped-lone-surrogate", UNPAIRED),
        ("rt-escaped-lone-surrogate-upper", UNPAIRED),
        ("layout-indent-negative", NOT_A_LAYOUT),
        ("layout-indent-fraction", NOT_A_LAYOUT),
        ("doc-undefined-values", NOT_A_DOCUMENT),
        ("doc-undefined-values-lf2", NOT_A_DOCUMENT),
        ("doc-lone-surrogates", UNPAIRED),
        ("doc-lone-surrogates-lf2", UNPAIRED),
    ];
    run(file, &skipped, |case| {
        let Some(layout) = layout_from(&case.args[1]) else {
            return Answer::Skip(NOT_A_LAYOUT);
        };
        match documents.get(&case.id).and_then(document) {
            Some(JsValue::Object(doc)) => Answer::Out(serialize_config(&doc, &layout).into()),
            _ => Answer::Skip(NOT_A_DOCUMENT),
        }
    });
}

#[test]
fn round_trips_cases() {
    run("l1/battlenet/roundTrips.json", &[], |case| {
        Answer::Out(round_trips(&text(&case.args[0])).into())
    });
}

#[test]
fn plan_saved_account_names_cases() {
    let refused = [
        corpus(
            21,
            &[
                0, 1, 2, 4, 5, 7, 10, 13, 15, 16, 17, 19, 20, 21, 22, 24, 26, 28, 29, 30, 32, 33,
                35, 36, 39,
            ],
        ),
        corpus(
            22,
            &[
                0, 1, 4, 5, 7, 9, 10, 11, 12, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 28,
                29, 30, 32, 34, 36, 38,
            ],
        ),
        corpus(
            23,
            &[
                0, 2, 3, 4, 5, 7, 8, 9, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 23, 25, 26, 27, 28,
                29, 30, 31, 32, 33, 34, 35, 37, 38,
            ],
        ),
    ]
    .concat();
    let mut skipped = vec![
        ("not-json", V8_WORDING),
        ("mask-astral-first", UNPAIRED),
        ("login-lone-surrogate", UNPAIRED),
    ];
    skipped.extend(with_reason(&refused, V8_WORDING));
    run(
        "l1/battlenet/planSavedAccountNames.json",
        &skipped,
        |case| {
            let Value::String(login) = plain(&case.args[1]) else {
                return Answer::Skip(NOT_TEXT);
            };
            match plan_saved_account_names(&text(&case.args[0]), &login) {
                Ok(write) => Answer::Out(json!({
                    "text": write.text,
                    "changes": write.changes.iter().map(|change| json!({
                        "where": change.place,
                        "before": change.before,
                        "after": change.after,
                    })).collect::<Vec<_>>(),
                    "noop": write.noop,
                    "before": write.before,
                    "after": write.after,
                })),
                Err(error) => config_error(case, &error),
            }
        },
    );
}

#[test]
fn mask_login_cases() {
    let skipped = [("astral-first", UNPAIRED), ("astral-tag", UNPAIRED)];
    run(
        "l1/battlenet/maskLogin.json",
        &skipped,
        |case| match plain(&case.args[0]) {
            Value::String(login) => Answer::Out(mask_login(&login).into()),
            _ => Answer::Skip(NOT_TEXT),
        },
    );
}

#[test]
fn split_saved_account_names_cases() {
    run(
        "l1/battlenet/splitSavedAccountNames.json",
        &[],
        |case| match plain(&case.args[0]) {
            Value::Null => Answer::Out(split_saved_account_names(None).into()),
            Value::String(raw) => Answer::Out(split_saved_account_names(Some(&raw)).into()),
            _ => Answer::Skip(NOT_TEXT),
        },
    );
}

fn event_json(event: &LogEvent) -> Value {
    match event {
        LogEvent::Started { build } => json!({ "kind": "started", "build": build }),
        LogEvent::SignInAttempt => json!({ "kind": "sign-in-attempt" }),
        LogEvent::SignedIn { account_id, at } => {
            json!({ "kind": "signed-in", "accountId": account_id, "at": at })
        }
        LogEvent::SignInFailed { reason } => json!({ "kind": "sign-in-failed", "reason": reason }),
        LogEvent::SignedOut => json!({ "kind": "signed-out" }),
        LogEvent::MainWindow => json!({ "kind": "main-window" }),
        LogEvent::LaunchRequest { code } => json!({ "kind": "launch-request", "code": code }),
        LogEvent::ProductState {
            codename,
            operation,
            status,
            playable,
        } => json!({
            "kind": "product-state",
            "codename": codename,
            "operation": operation,
            "status": status,
            "playable": playable,
        }),
        LogEvent::GameLaunching { codename } => {
            json!({ "kind": "game-launching", "codename": codename })
        }
        LogEvent::GameRunning { codename } => {
            json!({ "kind": "game-running", "codename": codename })
        }
        LogEvent::GameExited { codename } => json!({ "kind": "game-exited", "codename": codename }),
        LogEvent::CloseRequested => json!({ "kind": "close-requested" }),
        LogEvent::Exit => json!({ "kind": "exit" }),
    }
}

// Text, or nothing, where a type has an optional text. None for anything else.
fn optional_text(value: &Value) -> Option<Option<String>> {
    match value {
        Value::Null => Some(None),
        Value::String(text) => Some(Some(text.clone())),
        _ => None,
    }
}

fn optional_flag(value: &Value) -> Option<Option<bool>> {
    match value {
        Value::Null => Some(None),
        Value::Bool(flag) => Some(Some(*flag)),
        _ => None,
    }
}

fn event_from(value: &Value) -> Option<LogEvent> {
    let fields = value.as_object()?;
    let text = |name: &str| fields.get(name)?.as_str().map(str::to_owned);
    let codename = || text("codename");
    Some(match (fields.get("kind")?.as_str()?, fields.len()) {
        ("started", 2) => LogEvent::Started {
            build: optional_text(fields.get("build")?)?,
        },
        ("sign-in-attempt", 1) => LogEvent::SignInAttempt,
        ("signed-in", 3) => LogEvent::SignedIn {
            account_id: text("accountId")?,
            at: optional_text(fields.get("at")?)?,
        },
        ("sign-in-failed", 2) => LogEvent::SignInFailed {
            reason: text("reason")?,
        },
        ("signed-out", 1) => LogEvent::SignedOut,
        ("main-window", 1) => LogEvent::MainWindow,
        ("launch-request", 2) => LogEvent::LaunchRequest {
            code: text("code")?,
        },
        ("product-state", 5) => LogEvent::ProductState {
            codename: codename()?,
            operation: optional_text(fields.get("operation")?)?,
            status: optional_text(fields.get("status")?)?,
            playable: optional_flag(fields.get("playable")?)?,
        },
        ("game-launching", 2) => LogEvent::GameLaunching {
            codename: codename()?,
        },
        ("game-running", 2) => LogEvent::GameRunning {
            codename: codename()?,
        },
        ("game-exited", 2) => LogEvent::GameExited {
            codename: codename()?,
        },
        ("close-requested", 1) => LogEvent::CloseRequested,
        ("exit", 1) => LogEvent::Exit,
        _ => return None,
    })
}

fn events_from(arg: &Value) -> Option<Vec<LogEvent>> {
    plain(arg).as_array()?.iter().map(event_from).collect()
}

#[test]
fn parse_log_events_cases() {
    let skipped = [("failed-reason-astral-cut", UNPAIRED)];
    run("l1/battlenet/parseLogEvents.json", &skipped, |case| {
        Answer::Out(
            parse_log_events(&text(&case.args[0]))
                .iter()
                .map(event_json)
                .collect(),
        )
    });
}

#[test]
fn line_time_cases() {
    run("l1/battlenet/lineTime.json", &[], |case| {
        match plain(&case.args[0]) {
            Value::String(line) => Answer::Out(line_time(&line).into()),
            _ => Answer::Skip(NOT_TEXT),
        }
    });
}

#[test]
fn mask_reason_cases() {
    let skipped = [
        ("cap-length-astral-straddle", UNPAIRED),
        ("lone-surrogate", UNPAIRED),
    ];
    run(
        "l1/battlenet/maskReason.json",
        &skipped,
        |case| match plain(&case.args[0]) {
            Value::String(raw) => Answer::Out(mask_reason(&raw).into()),
            _ => Answer::Skip(NOT_TEXT),
        },
    );
}

#[test]
fn is_forwarder_log_cases() {
    run("l1/battlenet/isForwarderLog.json", &[], |case| {
        Answer::Out(is_forwarder_log(&text(&case.args[0])).into())
    });
}

#[test]
fn last_sign_in_cases() {
    run("l1/battlenet/lastSignIn.json", &[], |case| {
        let Some(events) = events_from(&case.args[0]) else {
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
fn ended_signed_out_cases() {
    run(
        "l1/battlenet/endedSignedOut.json",
        &[],
        |case| match events_from(&case.args[0]) {
            Some(events) => Answer::Out(ended_signed_out(&events).into()),
            None => Answer::Skip(NOT_EVENTS),
        },
    );
}

#[test]
fn product_states_cases() {
    run("l1/battlenet/productStates.json", &[], |case| {
        let Some(events) = events_from(&case.args[0]) else {
            return Answer::Skip(NOT_EVENTS);
        };
        Answer::Out(
            product_states(&events)
                .iter()
                .map(|(codename, state)| {
                    json!([codename, {
                        "operation": state.operation,
                        "status": state.status,
                        "playable": state.playable,
                    }])
                })
                .collect(),
        )
    });
}

fn state_from(value: &Value) -> Option<ProductState> {
    let fields = value.as_object().filter(|fields| fields.len() == 3)?;
    Some(ProductState {
        operation: optional_text(fields.get("operation")?)?,
        status: optional_text(fields.get("status")?)?,
        playable: optional_flag(fields.get("playable")?)?,
    })
}

#[test]
fn product_needs_update_cases() {
    run("l1/battlenet/productNeedsUpdate.json", &[], |case| {
        let state = match plain(&case.args[0]) {
            Value::Null => None,
            given => match state_from(&given) {
                Some(state) => Some(state),
                None => return Answer::Skip(NOT_A_STATE),
            },
        };
        Answer::Out(product_needs_update(state.as_ref()).into())
    });
}

#[test]
fn game_started_cases() {
    run("l1/battlenet/gameStarted.json", &[], |case| {
        let (Some(events), Value::String(codename), Value::String(program_code)) = (
            events_from(&case.args[0]),
            plain(&case.args[1]),
            plain(&case.args[2]),
        ) else {
            return Answer::Skip(NOT_EVENTS);
        };
        Answer::Out(game_started(&events, &codename, &program_code).into())
    });
}

#[test]
fn game_running_cases() {
    run("l1/battlenet/gameRunning.json", &[], |case| {
        let (Some(events), Value::String(codename)) =
            (events_from(&case.args[0]), plain(&case.args[1]))
        else {
            return Answer::Skip(NOT_EVENTS);
        };
        Answer::Out(game_running(&events, &codename).into())
    });
}

fn codes_json(codes: &ProductCodes) -> Value {
    let mut shown = json!({
        "programCode": codes.program_code,
        "tactCode": codes.tact_code,
        "name": codes.name,
        "verified": codes.verified,
    });
    if let Some(steam_app_id) = codes.steam_app_id {
        shown["steamAppId"] = steam_app_id.into();
    }
    shown
}

#[test]
fn the_product_table() {
    run("l1/battlenet/PRODUCTS.json", &[], |_| {
        let table: Map<String, Value> = PRODUCTS
            .iter()
            .map(|product| {
                let codes = known_product(product.0).expect("a row of the table");
                (product.0.to_owned(), codes_json(&codes))
            })
            .collect();
        Answer::Out(Value::Object(table))
    });
}

#[test]
fn product_codes_cases() {
    let skipped = [
        ("proto", PROTOTYPE_MEMBER),
        ("proto-tact", PROTOTYPE_MEMBER),
        ("constructor", PROTOTYPE_MEMBER),
        ("constructor-tact", PROTOTYPE_MEMBER),
        ("to-string", PROTOTYPE_MEMBER),
        ("has-own-property-unknown", PROTOTYPE_MEMBER),
    ];
    run("l1/battlenet/productCodes.json", &skipped, |case| {
        let Value::String(codename) = plain(&case.args[0]) else {
            return Answer::Skip(NOT_TEXT);
        };
        let from_db = match case.args.get(1).map(plain) {
            None | Some(Value::Null) => None,
            Some(Value::String(tact)) => Some(tact),
            Some(_) => return Answer::Skip(NOT_TEXT),
        };
        if OBJECT_PROTOTYPE.contains(&codename.as_str()) {
            return Answer::Skip(PROTOTYPE_MEMBER);
        }
        Answer::Out(
            product_codes(&codename, from_db.as_deref())
                .as_ref()
                .map_or(Value::Null, codes_json),
        )
    });
}

#[test]
fn parse_product_db_cases() {
    run("l1/battlenet/parseProductDb.json", &[], |case| {
        Answer::Out(
            parse_product_db(&bytes(&case.args[0]))
                .iter()
                .map(|entry| {
                    json!({
                        "codename": entry.codename,
                        "tactCode": entry.tact_code,
                        "installPath": entry.install_path,
                    })
                })
                .collect(),
        )
    });
}

#[test]
fn parse_aggregate_cases() {
    let skipped = [("lone-surrogate-name", UNPAIRED), ("nested-deep", TOO_DEEP)];
    run("l1/battlenet/parseAggregate.json", &skipped, |case| {
        let json = text(&case.args[0]);
        if depth(&json) > MAX_DEPTH {
            return Answer::Skip(TOO_DEEP);
        }
        match parse_aggregate(&json) {
            Ok(entries) => Answer::Out(
                entries
                    .iter()
                    .map(|entry| {
                        json!({
                            "name": entry.name,
                            "tactCode": entry.tact_code,
                            "launchUri": entry.launch_uri,
                            "iconPath": entry.icon_path,
                            "lastPlayedAt": entry.last_played_at,
                        })
                    })
                    .collect(),
            ),
            Err(error) => {
                Answer::Error(json!({ "name": "RangeError", "message": error.to_string() }))
            }
        }
    });
}

#[test]
fn launch_command_cases() {
    let skipped = [
        ("form-unknown", NOT_A_FORM),
        ("form-exec-upper", NOT_A_FORM),
        ("form-undefined", NOT_A_FORM),
        ("codes-lone-surrogate", UNPAIRED),
        ("codes-lone-surrogate-exec", UNPAIRED),
    ];
    run("l1/battlenet/launchCommand.json", &skipped, |case| {
        let codes = plain(&case.args[0]);
        let (Some(program_code), Some(tact_code)) =
            (codes["programCode"].as_str(), codes["tactCode"].as_str())
        else {
            return Answer::Skip(NOT_CODES);
        };
        let form = match case.args.get(1).map(plain) {
            Some(Value::String(form)) if form == "uri" => LaunchForm::Uri,
            Some(Value::String(form)) if form == "exec" => LaunchForm::Exec,
            _ => return Answer::Skip(NOT_A_FORM),
        };
        let codes = ProductCodes {
            program_code: program_code.to_owned(),
            tact_code: tact_code.to_owned(),
            name: String::new(),
            verified: false,
            steam_app_id: None,
        };
        Answer::Out(match launch_command(&codes, form) {
            LaunchCommand::Uri(url) => json!({ "kind": "uri", "url": url }),
            LaunchCommand::Exec(arg) => json!({ "kind": "exec", "arg": arg }),
        })
    });
}

// A session exactly as the app's own type holds it: version 1, its keys in the type's order and both values text.
fn blob_from(value: &Value) -> Option<BattlenetSessionBlob> {
    let keys = |value: &Value| -> Vec<String> {
        value
            .as_object()
            .map(|fields| fields.keys().cloned().collect())
            .unwrap_or_default()
    };
    let token = &value["unifiedAuth"];
    let shaped =
        keys(value) == ["v", "unifiedAuth"] && keys(token) == ["name", "hex"] && value["v"] == 1;
    shaped.then_some(())?;
    Some(BattlenetSessionBlob {
        unified_auth: TokenValue {
            name: token["name"].as_str()?.to_owned(),
            hex: token["hex"].as_str()?.to_owned(),
        },
    })
}

#[test]
fn serialize_blob_cases() {
    let skipped = [
        ("v2-with-cookies", NOT_A_BLOB),
        ("v2-without-cookies", NOT_A_BLOB),
        ("key-order", NOT_A_BLOB),
        ("undefined-member", NOT_A_BLOB),
        ("name-lone-surrogate", UNPAIRED),
    ];
    run(
        "l1/battlenet/serializeBlob.json",
        &skipped,
        |case| match blob_from(&case.args[0]) {
            Some(blob) => Answer::Out(serialize_blob(&blob).into()),
            None => Answer::Skip(NOT_A_BLOB),
        },
    );
}

#[test]
fn parse_blob_cases() {
    let skipped = [
        ("not-json", V8_WORDING),
        ("empty", V8_WORDING),
        ("truncated", V8_WORDING),
        ("bom", V8_WORDING),
        ("name-lone-surrogate", UNPAIRED),
    ];
    run("l1/battlenet/parseBlob.json", &skipped, |case| {
        let Value::String(json) = plain(&case.args[0]) else {
            return Answer::Skip(NOT_TEXT);
        };
        let expects_syntax_error = case
            .error()
            .is_some_and(|error| error["name"] == "SyntaxError");
        match parse_blob(&json) {
            Ok(blob) => Answer::Out(json!({
                "v": 1,
                "unifiedAuth": { "name": blob.unified_auth.name, "hex": blob.unified_auth.hex },
            })),
            Err(_) if expects_syntax_error => Answer::Skip(V8_WORDING),
            Err(error) => Answer::Error(json!({ "name": "Error", "message": error.to_string() })),
        }
    });
}
