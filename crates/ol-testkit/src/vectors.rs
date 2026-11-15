use std::fs;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::workspace_root;

// Stands where a recorded string holds half of a surrogate pair, which no Rust string can.
const UNPAIRED: char = '\u{fdd0}';

pub struct Vectors {
    pub function: String,
    pub cases: Vec<Case>,
}

pub struct Case {
    pub id: String,
    pub args: Vec<Value>,
    pub expected: Expected,
}

pub enum Expected {
    Out(Value),
    Error(Map<String, Value>),
}

impl Vectors {
    pub fn load(relative: &str) -> Self {
        let path = crate::oracle(relative);
        let text = fs::read_to_string(&path).expect("the vector file could not be read");
        let root: Value = serde_json::from_str(&without_unpaired_escapes(&text))
            .unwrap_or_else(|error| panic!("oracle/{relative} is not JSON: {error}"));
        let function = root["function"]
            .as_str()
            .expect("a vector file names its function")
            .to_owned();
        let cases = root["cases"]
            .as_array()
            .expect("a vector file has cases")
            .iter()
            .map(|case| Case {
                id: case["id"].as_str().expect("a case has an id").to_owned(),
                args: case["args"].as_array().expect("a case has args").clone(),
                expected: match (case.get("out"), case.get("error")) {
                    (Some(out), None) => Expected::Out(out.clone()),
                    (None, Some(Value::Object(error))) => Expected::Error(error.clone()),
                    _ => panic!("a case has an out or an error"),
                },
            })
            .collect();
        Self { function, cases }
    }
}

impl Case {
    // True when an argument or the result cannot be a Rust string: it holds an unpaired surrogate.
    pub fn has_unpaired_surrogate(&self) -> bool {
        let expected = match &self.expected {
            Expected::Out(out) => unpaired(out),
            Expected::Error(error) => error.values().any(unpaired),
        };
        expected || self.args.iter().any(unpaired)
    }

    pub fn out(&self) -> Option<Value> {
        match &self.expected {
            Expected::Out(out) => Some(plain(out)),
            Expected::Error(_) => None,
        }
    }

    pub fn error(&self) -> Option<&Map<String, Value>> {
        match &self.expected {
            Expected::Out(_) => None,
            Expected::Error(error) => Some(error),
        }
    }
}

pub fn bytes(arg: &Value) -> Vec<u8> {
    if let Some(text) = arg.as_str() {
        return text.as_bytes().to_vec();
    }
    if let Some(encoded) = arg.get("$bytes").and_then(Value::as_str) {
        return STANDARD.decode(encoded).expect("a $bytes value is base64");
    }
    if let Some(file) = arg.get("$file").and_then(Value::as_str) {
        let path = workspace_root().join("oracle").join(file);
        let read = fs::read(&path).unwrap_or_else(|_| panic!("oracle/{file} could not be read"));
        let recorded = arg["sha256"].as_str().expect("a $file names its digest");
        assert_eq!(
            sha256_hex(&read),
            recorded,
            "oracle/{file} is not the file recorded"
        );
        return read;
    }
    panic!("not bytes: {arg}");
}

// Text as the Electron app decodes a file it read: a BOM stays, and a byte that is not UTF-8 becomes U+FFFD.
pub fn text(arg: &Value) -> String {
    match plain(arg) {
        Value::String(text) => text,
        _ => String::from_utf8_lossy(&bytes(arg)).into_owned(),
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

// A value with its wrappers taken off where Rust has the thing itself: an absent value is null, text is a string and minus zero is zero.
pub fn plain(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(plain).collect()),
        Value::Object(fields) => {
            if fields.len() == 1 {
                let (key, inner) = fields.iter().next().expect("one field");
                match (key.as_str(), inner) {
                    ("$undefined", _) => return Value::Null,
                    ("$number", Value::String(number)) if number == "-0" => return 0.into(),
                    ("$date", Value::String(date)) => return Value::String(date.clone()),
                    ("$utf16", Value::Array(units)) => {
                        let units: Vec<u16> = units
                            .iter()
                            .map(|unit| {
                                u16::try_from(unit.as_u64().expect("a code unit")).expect("16 bits")
                            })
                            .collect();
                        if let Ok(text) = String::from_utf16(&units) {
                            return Value::String(text);
                        }
                        return value.clone();
                    }
                    _ => {}
                }
            }
            Value::Object(
                fields
                    .iter()
                    .map(|(key, inner)| (key.clone(), plain(inner)))
                    .collect(),
            )
        }
        other => other.clone(),
    }
}

fn unpaired(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains(UNPAIRED),
        Value::Array(items) => items.iter().any(unpaired),
        Value::Object(fields) => {
            if let Some(Value::Array(units)) = fields.get("$utf16") {
                let units: Vec<u16> = units
                    .iter()
                    .filter_map(|unit| u16::try_from(unit.as_u64()?).ok())
                    .collect();
                return String::from_utf16(&units).is_err();
            }
            fields
                .iter()
                .any(|(key, inner)| key.contains(UNPAIRED) || unpaired(inner))
        }
        _ => false,
    }
}

// serde_json refuses \ud800 without its other half, so the escape gives way to a marker before the file is parsed.
fn without_unpaired_escapes(json: &str) -> String {
    let bytes = json.as_bytes();
    let mut out = String::with_capacity(json.len());
    let mut from = 0;
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] != b'\\' {
            at += 1;
            continue;
        }
        let Some(unit) = escaped_unit(bytes, at) else {
            at += 2;
            continue;
        };
        let high = (0xd800..0xdc00).contains(&unit);
        let low = (0xdc00..0xe000).contains(&unit);
        let paired = high
            && escaped_unit(bytes, at + 6).is_some_and(|next| (0xdc00..0xe000).contains(&next));
        if paired {
            at += 12;
        } else if high || low {
            out.push_str(&json[from..at]);
            out.push_str("\\ufdd0");
            at += 6;
            from = at;
        } else {
            at += 6;
        }
    }
    out.push_str(&json[from..]);
    out
}

fn escaped_unit(bytes: &[u8], at: usize) -> Option<u32> {
    if bytes.get(at) != Some(&b'\\') || bytes.get(at + 1) != Some(&b'u') {
        return None;
    }
    let digits = std::str::from_utf8(bytes.get(at + 2..at + 6)?).ok()?;
    u32::from_str_radix(digits, 16).ok()
}

// The first place two values differ, with object keys compared in their order.
pub fn difference(actual: &Value, expected: &Value) -> Option<String> {
    differs(actual, expected, &mut String::new())
}

fn differs(actual: &Value, expected: &Value, path: &mut String) -> Option<String> {
    match (actual, expected) {
        (Value::Object(actual), Value::Object(expected)) => {
            let keys = |fields: &Map<String, Value>| fields.keys().cloned().collect::<Vec<_>>();
            if keys(actual) != keys(expected) {
                return Some(format!(
                    "{path}: keys {:?}, expected {:?}",
                    keys(actual),
                    keys(expected)
                ));
            }
            for (key, value) in actual {
                let length = path.len();
                path.push('/');
                path.push_str(key);
                if let Some(found) = differs(value, &expected[key], path) {
                    return Some(found);
                }
                path.truncate(length);
            }
            None
        }
        (Value::Array(actual), Value::Array(expected)) => {
            if actual.len() != expected.len() {
                return Some(format!(
                    "{path}: {} items, expected {}",
                    actual.len(),
                    expected.len()
                ));
            }
            for (index, (value, wanted)) in actual.iter().zip(expected).enumerate() {
                let length = path.len();
                path.push_str(&format!("/{index}"));
                if let Some(found) = differs(value, wanted, path) {
                    return Some(found);
                }
                path.truncate(length);
            }
            None
        }
        (Value::Number(actual), Value::Number(expected)) => (actual.as_f64() != expected.as_f64())
            .then(|| format!("{path}: {actual}, expected {expected}")),
        _ => (actual != expected).then(|| format!("{path}: {actual}, expected {expected}")),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn half_a_surrogate_pair_is_marked_and_a_whole_pair_is_kept() {
        let json = r#"{"a":"\ud800A","b":"😀","c":"x\\ud800","d":"\udc00"}"#;
        let parsed: Value = serde_json::from_str(&without_unpaired_escapes(json)).unwrap();
        assert!(unpaired(&parsed["a"]));
        assert_eq!(parsed["b"], "😀");
        assert_eq!(parsed["c"], "x\\ud800");
        assert!(unpaired(&parsed["d"]));
        assert!(unpaired(&json!({ "$utf16": [55296, 97] })));
        assert!(!unpaired(&json!({ "$utf16": [55357, 56832] })));
    }

    #[test]
    fn wrappers_come_off_where_rust_has_the_value() {
        let recorded = json!({
            "absent": { "$undefined": true },
            "zero": { "$number": "-0" },
            "text": { "$utf16": [104, 105] },
            "kept": { "$number": "NaN" }
        });
        assert_eq!(
            plain(&recorded),
            json!({ "absent": null, "zero": 0, "text": "hi", "kept": { "$number": "NaN" } })
        );
    }

    #[test]
    fn a_difference_names_its_place_and_key_order_counts() {
        assert_eq!(
            difference(&json!({ "a": [1, 2] }), &json!({ "a": [1, 2] })),
            None
        );
        assert_eq!(
            difference(&json!({ "a": [1, 3] }), &json!({ "a": [1, 2] })).as_deref(),
            Some("/a/1: 3, expected 2")
        );
        assert!(difference(&json!({ "a": 1, "b": 2 }), &json!({ "b": 2, "a": 1 })).is_some());
        assert_eq!(difference(&json!(1), &json!(1.0)), None);
    }
}
