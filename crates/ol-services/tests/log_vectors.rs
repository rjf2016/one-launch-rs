#![forbid(unsafe_code)]

use ol_model::LogLevel;
use ol_model::redact::AccountNames;
use ol_services::log::redact::{format_line, is_sensitive_key, redact_string, redact_value};
use ol_testkit::vectors::{Answer, Case, UNPAIRED, Vectors, plain, run};
use serde_json::{Map, Value};

const NOT_TEXT: &str = "not text, which the Rust type rules out";
const NOT_A_LEVEL: &str = "a level other than the four, which the Rust type rules out";
const NOT_FINITE: &str = "NaN or Infinity, which a serde_json value cannot hold";
const NO_DEPTH: &str = "a depth that is not a whole number, which the Rust type rules out";
const JS_ONLY: &str = "a JavaScript value a serde_json value has no form for (a cycle, a shared object, bytes, a function, a symbol, a bigint, a Map, a typed array, a boxed string, a Date or a getter)";
const AS_WRITTEN: &str = "a name is also masked as it was written, where its lower case folds to something else (İ): the Electron app leaves it unmasked";
const PROTO: &str = "JavaScript's copy drops an own __proto__ key; Rust keeps it, and redacts what is under it like any other field";

fn names(arg: &Value) -> AccountNames {
    let mut names = AccountNames::new();
    let given: Vec<String> = arg
        .as_array()
        .expect("a list of names")
        .iter()
        .filter_map(|name| plain(name).as_str().map(str::to_owned))
        .collect();
    names.add(given);
    names
}

fn text(arg: &Value) -> Option<String> {
    plain(arg).as_str().map(str::to_owned)
}

// A JavaScript Error is given as the object redactValue makes of one, since Rust has none.
fn error_object(marker: &Map<String, Value>) -> Value {
    let message = marker["message"].as_str().unwrap_or_default().to_owned();
    let name = marker
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Error")
        .to_owned();
    let stack = match marker.get("stack") {
        Some(stack) => plain(stack),
        None => Value::String(format!("{name}: {message}")),
    };
    let mut error = Map::new();
    error.insert("name".to_owned(), Value::String(name));
    error.insert("message".to_owned(), Value::String(message));
    error.insert("stack".to_owned(), stack);
    if let Some(Value::String(code)) = marker.get("code") {
        error.insert("code".to_owned(), Value::String(code.clone()));
    }
    Value::Object(error)
}

fn built(value: &Value) -> Result<Value, &'static str> {
    match value {
        Value::Array(items) => items
            .iter()
            .map(built)
            .collect::<Result<_, _>>()
            .map(Value::Array),
        Value::Object(fields) => {
            if let Some(kind) = fields.get("%make").and_then(Value::as_str) {
                return match kind {
                    "error" => Ok(error_object(fields)),
                    _ => Err(JS_ONLY),
                };
            }
            if fields.len() == 1 {
                let (key, inner) = fields.iter().next().expect("one field");
                match (key.as_str(), inner) {
                    ("$number", Value::String(number)) if number != "-0" => return Err(NOT_FINITE),
                    ("$bytes" | "$date", _) => return Err(JS_ONLY),
                    ("$undefined" | "$number" | "$utf16", _) => return Ok(plain(value)),
                    _ => {}
                }
            }
            if fields.contains_key("__proto__") {
                return Err(PROTO);
            }
            // What is under a sensitive key is replaced unread, so one Rust cannot hold stands as null there.
            fields
                .iter()
                .map(|(key, inner)| {
                    let inner = if is_sensitive_key(key) {
                        Ok(Value::Null)
                    } else {
                        built(inner)
                    };
                    inner.map(|inner| (key.clone(), inner))
                })
                .collect::<Result<Map<_, _>, _>>()
                .map(Value::Object)
        }
        other => Ok(plain(other)),
    }
}

fn level(arg: &Value) -> Option<LogLevel> {
    arg.as_str().and_then(LogLevel::parse)
}

fn redact_string_case(case: &Case) -> Answer {
    if case.id == "names-dotless-i" {
        return Answer::Skip(AS_WRITTEN);
    }
    let Some(input) = text(&case.args[1]) else {
        return Answer::Skip(NOT_TEXT);
    };
    Answer::Out(Value::String(redact_string(&names(&case.args[0]), &input)))
}

fn redact_value_case(case: &Case) -> Answer {
    let value = match built(&case.args[1]) {
        Ok(value) => value,
        Err(reason) => return Answer::Skip(reason),
    };
    let depth = match &case.args[2] {
        Value::Number(depth) => match depth.as_i64() {
            Some(depth) => depth,
            None => return Answer::Skip(NO_DEPTH),
        },
        Value::Object(marker) if marker.contains_key("$undefined") => 0,
        _ => return Answer::Skip(NO_DEPTH),
    };
    Answer::Out(redact_value(&names(&case.args[0]), &value, depth))
}

fn format_line_case(case: &Case) -> Answer {
    let args = &case.args;
    let Some(level) = level(&args[2]) else {
        return Answer::Skip(NOT_A_LEVEL);
    };
    let (Some(timestamp), Some(scope), Some(message)) =
        (text(&args[1]), text(&args[3]), text(&args[4]))
    else {
        return Answer::Skip(NOT_TEXT);
    };
    let fields = match args.get(5) {
        None => None,
        Some(Value::Object(marker)) if marker.contains_key("$undefined") => None,
        Some(meta) => match built(meta) {
            Ok(meta) => Some(meta),
            Err(reason) => return Answer::Skip(reason),
        },
    };
    Answer::Out(Value::String(format_line(
        &names(&args[0]),
        &timestamp,
        level,
        &scope,
        &message,
        fields.as_ref(),
    )))
}

#[test]
fn redact_string_vectors() {
    run(
        "l1/core/redactString.json",
        &[
            ("names-dotless-i", AS_WRITTEN),
            ("names-lone-surrogate", UNPAIRED),
            ("lone-surrogates", UNPAIRED),
            ("not-a-string", NOT_TEXT),
            ("undefined-input", NOT_TEXT),
        ],
        redact_string_case,
    );
}

#[test]
fn redact_value_vectors() {
    run(
        "l1/core/redactValue.json",
        &[
            ("cycle", JS_ONLY),
            ("buffer", JS_ONLY),
            ("primitives", NOT_FINITE),
            ("function-and-symbol", JS_ONLY),
            ("bigint", JS_ONLY),
            ("shared-reference", JS_ONLY),
            ("array-cycle", JS_ONLY),
            ("map-and-typed-array", JS_ONLY),
            ("uint8-nested", JS_ONLY),
            ("depth-arg-nan", NO_DEPTH),
            ("getter-throws", JS_ONLY),
            ("to-json-not-called", JS_ONLY),
            ("proto-key", PROTO),
            ("lone-surrogate-key", UNPAIRED),
        ],
        redact_value_case,
    );
}

#[test]
fn format_line_vectors() {
    run(
        "l1/core/formatLine.json",
        &[
            ("redaction-failed", NOT_TEXT),
            ("cap-message-astral-straddle", UNPAIRED),
            ("meta-bigint", JS_ONLY),
            ("meta-getter-throws", JS_ONLY),
            ("meta-numbers", NOT_FINITE),
            ("meta-undefined-members", JS_ONLY),
            ("meta-string-escapes", UNPAIRED),
            ("level-non-ascii", NOT_A_LEVEL),
            ("level-astral", NOT_A_LEVEL),
            ("level-long", NOT_A_LEVEL),
            ("level-not-a-string", NOT_A_LEVEL),
            ("message-number", NOT_TEXT),
        ],
        format_line_case,
    );
}

#[test]
fn a_cut_through_a_surrogate_pair_writes_what_the_electron_file_holds() {
    let vectors = Vectors::load("l1/core/formatLine.json");
    let case = vectors
        .cases
        .iter()
        .find(|case| case.id == "cap-message-astral-straddle")
        .expect("the case is recorded");
    let recorded = case.out().expect("the case has an out");
    let units: Vec<u16> = recorded["$utf16"]
        .as_array()
        .expect("the line is recorded as UTF-16 units")
        .iter()
        .map(|unit| u16::try_from(unit.as_u64().expect("a unit")).expect("16 bits"))
        .collect();
    let on_disk = String::from_utf16_lossy(&units);
    let Answer::Out(Value::String(line)) = format_line_case(case) else {
        panic!("the case can be put to the port");
    };
    assert_eq!(line, on_disk);
    assert!(line.contains("\u{fffd}\u{2026} [1 more characters]"));
}
