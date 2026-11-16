// JSON as JavaScript reads and writes it: JSON.parse and JSON.stringify, down to the order of an object's keys, the spelling of a number and half a surrogate pair in a string. Battle.net.config is rewritten only when a read and a write give its text back, so both have to agree with the Electron app's to the character.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

use thiserror::Error;
use zeroize::Zeroize;

// As deep as the reader of every other launcher's JSON goes.
pub const MAX_DEPTH: usize = 128;
// JSON.stringify indents by ten spaces at most.
const MAX_INDENT: usize = 10;
// The largest key JavaScript counts as an array index, and so lists before every other key.
const MAX_ARRAY_INDEX: u64 = 4_294_967_294;

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum JsonError {
    #[error("unexpected end of JSON input")]
    End,
    #[error("unexpected character at line {line} column {column}")]
    Unexpected { line: usize, column: usize },
    #[error("nested deeper than {MAX_DEPTH} levels")]
    TooDeep,
}

// A JavaScript string: UTF-16 units, so half a surrogate pair survives a read and a write. No Debug: a launcher file holds logins.
#[derive(Clone, PartialEq, Eq, Hash, Default)]
pub struct JsString(Vec<u16>);

impl JsString {
    pub const fn from_units(units: Vec<u16>) -> Self {
        Self(units)
    }

    pub fn units(&self) -> &[u16] {
        &self.0
    }

    // The text, with U+FFFD where half a surrogate pair stands alone.
    pub fn to_string_lossy(&self) -> String {
        String::from_utf16_lossy(&self.0)
    }

    fn array_index(&self) -> Option<u32> {
        let digits = &self.0;
        let canonical = match digits.as_slice() {
            [] => false,
            [only] => (0x30..=0x39).contains(only),
            [first, ..] => *first != 0x30 && digits.iter().all(|unit| (0x30..=0x39).contains(unit)),
        };
        if !canonical || digits.len() > 10 {
            return None;
        }
        let number = digits
            .iter()
            .fold(0_u64, |number, unit| number * 10 + u64::from(unit - 0x30));
        (number <= MAX_ARRAY_INDEX).then(|| u32::try_from(number).ok())?
    }
}

impl From<&str> for JsString {
    fn from(text: &str) -> Self {
        Self(text.encode_utf16().collect())
    }
}

#[derive(Clone)]
pub enum JsValue {
    Null,
    Bool(bool),
    Number(f64),
    String(JsString),
    Array(Vec<Self>),
    Object(JsObject),
}

impl JsValue {
    pub const fn as_object(&self) -> Option<&JsObject> {
        match self {
            Self::Object(object) => Some(object),
            _ => None,
        }
    }

    pub const fn as_object_mut(&mut self) -> Option<&mut JsObject> {
        match self {
            Self::Object(object) => Some(object),
            _ => None,
        }
    }

    pub const fn as_string(&self) -> Option<&JsString> {
        match self {
            Self::String(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Self]> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }

    pub const fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(number) => Some(*number),
            _ => None,
        }
    }

    // Overwrites every text the value holds, for a document that held a sign-in.
    pub fn wipe(&mut self) {
        match self {
            Self::String(text) => text.0.zeroize(),
            Self::Array(items) => items.iter_mut().for_each(Self::wipe),
            Self::Object(object) => {
                object.indexed.values_mut().for_each(Self::wipe);
                object.named.iter_mut().for_each(|(_, value)| value.wipe());
            }
            Self::Null | Self::Bool(_) | Self::Number(_) => {}
        }
    }
}

// An object's members in the order JavaScript lists them: the keys that are array indices first, rising, then every other key in the order it was first set. A key set twice keeps its place and takes the later value.
#[derive(Clone, Default)]
pub struct JsObject {
    indexed: BTreeMap<u32, JsValue>,
    named: Vec<(JsString, JsValue)>,
    places: HashMap<JsString, usize>,
}

impl JsObject {
    pub fn is_empty(&self) -> bool {
        self.indexed.is_empty() && self.named.is_empty()
    }

    pub fn insert(&mut self, key: JsString, value: JsValue) {
        if let Some(index) = key.array_index() {
            self.indexed.insert(index, value);
        } else if let Some(place) = self.places.get(&key) {
            self.named[*place].1 = value;
        } else {
            self.places.insert(key.clone(), self.named.len());
            self.named.push((key, value));
        }
    }

    pub fn get(&self, key: &str) -> Option<&JsValue> {
        let key = JsString::from(key);
        match key.array_index() {
            Some(index) => self.indexed.get(&index),
            None => self.places.get(&key).map(|place| &self.named[*place].1),
        }
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut JsValue> {
        let key = JsString::from(key);
        match key.array_index() {
            Some(index) => self.indexed.get_mut(&index),
            None => self.places.get(&key).map(|place| &mut self.named[*place].1),
        }
    }

    pub fn members(&self) -> impl Iterator<Item = (JsString, &JsValue)> {
        let indexed = self
            .indexed
            .iter()
            .map(|(index, value)| (JsString::from(index.to_string().as_str()), value));
        let named = self.named.iter().map(|(key, value)| (key.clone(), value));
        indexed.chain(named)
    }
}

struct Parser<'a> {
    text: &'a str,
    at: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.at).copied()
    }

    fn unexpected(&self) -> JsonError {
        if self.at >= self.text.len() {
            return JsonError::End;
        }
        let before = &self.text[..self.at];
        let line_start = before.rfind('\n').map_or(0, |at| at + 1);
        JsonError::Unexpected {
            line: before.matches('\n').count() + 1,
            column: before[line_start..].chars().count() + 1,
        }
    }

    fn skip_blank(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), JsonError> {
        if self.peek() == Some(byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(self.unexpected())
        }
    }

    fn literal(&mut self, word: &str, value: JsValue) -> Result<JsValue, JsonError> {
        for byte in word.bytes() {
            self.expect(byte)?;
        }
        Ok(value)
    }

    fn digits(&mut self) -> Result<(), JsonError> {
        if !self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            return Err(self.unexpected());
        }
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.at += 1;
        }
        Ok(())
    }

    fn number(&mut self) -> Result<JsValue, JsonError> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        if self.peek() == Some(b'0') {
            self.at += 1;
        } else {
            self.digits()?;
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            self.digits()?;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            self.digits()?;
        }
        let number = self.text[start..self.at]
            .parse::<f64>()
            .map_err(|_| self.unexpected())?;
        Ok(JsValue::Number(number))
    }

    fn string(&mut self) -> Result<JsString, JsonError> {
        self.expect(b'"')?;
        let mut units = Vec::new();
        loop {
            let Some(byte) = self.peek() else {
                return Err(JsonError::End);
            };
            match byte {
                b'"' => {
                    self.at += 1;
                    return Ok(JsString(units));
                }
                b'\\' => {
                    self.at += 1;
                    let escaped = self.peek().ok_or(JsonError::End)?;
                    let unit = match escaped {
                        b'"' | b'\\' | b'/' => u16::from(escaped),
                        b'b' => 0x08,
                        b'f' => 0x0c,
                        b'n' => 0x0a,
                        b'r' => 0x0d,
                        b't' => 0x09,
                        b'u' => {
                            let mut unit = 0_u16;
                            for _ in 0..4 {
                                self.at += 1;
                                let digit = self
                                    .peek()
                                    .and_then(|byte| char::from(byte).to_digit(16))
                                    .ok_or_else(|| self.unexpected())?;
                                unit = unit * 16 + u16::try_from(digit).unwrap_or(0);
                            }
                            unit
                        }
                        _ => return Err(self.unexpected()),
                    };
                    self.at += 1;
                    units.push(unit);
                }
                0..0x20 => return Err(self.unexpected()),
                _ => {
                    let character = self.text[self.at..].chars().next().ok_or(JsonError::End)?;
                    self.at += character.len_utf8();
                    units.extend(character.encode_utf16(&mut [0; 2]).iter());
                }
            }
        }
    }

    fn value(&mut self, depth: usize) -> Result<JsValue, JsonError> {
        self.skip_blank();
        match self.peek() {
            Some(b'{') => {
                if depth >= MAX_DEPTH {
                    return Err(JsonError::TooDeep);
                }
                self.at += 1;
                let mut object = JsObject::default();
                self.skip_blank();
                if self.peek() == Some(b'}') {
                    self.at += 1;
                    return Ok(JsValue::Object(object));
                }
                loop {
                    self.skip_blank();
                    let key = self.string()?;
                    self.skip_blank();
                    self.expect(b':')?;
                    let value = self.value(depth + 1)?;
                    object.insert(key, value);
                    self.skip_blank();
                    match self.peek() {
                        Some(b',') => self.at += 1,
                        Some(b'}') => {
                            self.at += 1;
                            return Ok(JsValue::Object(object));
                        }
                        _ => return Err(self.unexpected()),
                    }
                }
            }
            Some(b'[') => {
                if depth >= MAX_DEPTH {
                    return Err(JsonError::TooDeep);
                }
                self.at += 1;
                let mut items = Vec::new();
                self.skip_blank();
                if self.peek() == Some(b']') {
                    self.at += 1;
                    return Ok(JsValue::Array(items));
                }
                loop {
                    items.push(self.value(depth + 1)?);
                    self.skip_blank();
                    match self.peek() {
                        Some(b',') => self.at += 1,
                        Some(b']') => {
                            self.at += 1;
                            return Ok(JsValue::Array(items));
                        }
                        _ => return Err(self.unexpected()),
                    }
                }
            }
            Some(b'"') => Ok(JsValue::String(self.string()?)),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(b't') => self.literal("true", JsValue::Bool(true)),
            Some(b'f') => self.literal("false", JsValue::Bool(false)),
            Some(b'n') => self.literal("null", JsValue::Null),
            _ => Err(self.unexpected()),
        }
    }
}

// What JSON.parse makes of a text: only space, tab, carriage return and line feed are blank, a number is a double, and a key given twice keeps the later value.
pub fn parse(text: &str) -> Result<JsValue, JsonError> {
    let mut parser = Parser { text, at: 0 };
    let value = parser.value(0)?;
    parser.skip_blank();
    if parser.at < text.len() {
        return Err(parser.unexpected());
    }
    Ok(value)
}

// A number as JavaScript prints it: the fewest digits that read back as the same double, in plain notation from 1e-6 up to 1e21 and with an exponent outside.
pub fn number_to_string(number: f64) -> String {
    if number.is_nan() {
        return "NaN".to_owned();
    }
    if number.is_infinite() {
        let sign = if number < 0.0 { "-" } else { "" };
        return format!("{sign}Infinity");
    }
    if number == 0.0 {
        return "0".to_owned();
    }
    let shortest = format!("{:e}", number.abs());
    let (mantissa, exponent) = shortest.split_once('e').unwrap_or((&shortest, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let count = i64::try_from(digits.len()).unwrap_or(i64::MAX);
    let point = exponent.parse::<i64>().unwrap_or(0) + 1;
    let mut out = String::new();
    if number < 0.0 {
        out.push('-');
    }
    if count <= point && point <= 21 {
        out.push_str(&digits);
        out.extend(std::iter::repeat_n(
            '0',
            usize::try_from(point - count).unwrap_or(0),
        ));
    } else if 0 < point && point <= 21 {
        let (whole, fraction) = digits.split_at(usize::try_from(point).unwrap_or(0));
        let _ = write!(out, "{whole}.{fraction}");
    } else if -6 < point && point <= 0 {
        out.push_str("0.");
        out.extend(std::iter::repeat_n(
            '0',
            usize::try_from(-point).unwrap_or(0),
        ));
        out.push_str(&digits);
    } else {
        let (first, rest) = digits.split_at(1);
        out.push_str(first);
        if !rest.is_empty() {
            let _ = write!(out, ".{rest}");
        }
        let sign = if point > 0 { '+' } else { '-' };
        let _ = write!(out, "e{sign}{}", (point - 1).abs());
    }
    out
}

fn quote(out: &mut String, text: &JsString) {
    out.push('"');
    for unit in char::decode_utf16(text.0.iter().copied()) {
        match unit {
            Ok('"') => out.push_str("\\\""),
            Ok('\\') => out.push_str("\\\\"),
            Ok('\u{8}') => out.push_str("\\b"),
            Ok('\u{c}') => out.push_str("\\f"),
            Ok('\n') => out.push_str("\\n"),
            Ok('\r') => out.push_str("\\r"),
            Ok('\t') => out.push_str("\\t"),
            Ok(control) if control < ' ' => {
                let _ = write!(out, "\\u{:04x}", u32::from(control));
            }
            Ok(character) => out.push(character),
            Err(half) => {
                let _ = write!(out, "\\u{:04x}", half.unpaired_surrogate());
            }
        }
    }
    out.push('"');
}

fn write_value(out: &mut String, value: &JsValue, gap: &str, indent: &mut String) {
    let open = |out: &mut String, indent: &mut String| {
        indent.push_str(gap);
        if !gap.is_empty() {
            out.push('\n');
            out.push_str(indent);
        }
    };
    let next = |out: &mut String, indent: &String| {
        out.push(',');
        if !gap.is_empty() {
            out.push('\n');
            out.push_str(indent);
        }
    };
    let close = |out: &mut String, indent: &mut String| {
        indent.truncate(indent.len() - gap.len());
        if !gap.is_empty() {
            out.push('\n');
            out.push_str(indent);
        }
    };
    match value {
        JsValue::Null => out.push_str("null"),
        JsValue::Bool(true) => out.push_str("true"),
        JsValue::Bool(false) => out.push_str("false"),
        JsValue::Number(number) if number.is_finite() => out.push_str(&number_to_string(*number)),
        JsValue::Number(_) => out.push_str("null"),
        JsValue::String(text) => quote(out, text),
        JsValue::Array(items) if items.is_empty() => out.push_str("[]"),
        JsValue::Array(items) => {
            out.push('[');
            open(out, indent);
            for (at, item) in items.iter().enumerate() {
                if at > 0 {
                    next(out, indent);
                }
                write_value(out, item, gap, indent);
            }
            close(out, indent);
            out.push(']');
        }
        JsValue::Object(object) if object.is_empty() => out.push_str("{}"),
        JsValue::Object(object) => {
            out.push('{');
            open(out, indent);
            for (at, (key, member)) in object.members().enumerate() {
                if at > 0 {
                    next(out, indent);
                }
                quote(out, &key);
                out.push(':');
                if !gap.is_empty() {
                    out.push(' ');
                }
                write_value(out, member, gap, indent);
            }
            close(out, indent);
            out.push('}');
        }
    }
}

// What JSON.stringify(value, null, indent) writes. Without an indent it is one line with nothing between its parts.
pub fn stringify(value: &JsValue, indent: usize) -> String {
    let gap = " ".repeat(indent.min(MAX_INDENT));
    let mut out = String::new();
    write_value(&mut out, value, &gap, &mut String::new());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rewritten(text: &str, indent: usize) -> String {
        stringify(&parse(text).unwrap(), indent)
    }

    #[test]
    fn numbers_are_spelled_as_javascript_spells_them() {
        let spelled = [
            (1e21, "1e+21"),
            (1e-7, "1e-7"),
            (0.1 + 0.2, "0.30000000000000004"),
            (5e-324, "5e-324"),
            (9_007_199_254_740_992.0, "9007199254740992"),
            (-0.0, "0"),
            (f64::MAX, "1.7976931348623157e+308"),
            (123_456_789_012_345_680_000.0, "123456789012345680000"),
            (-1.5, "-1.5"),
            (0.000_001, "0.000001"),
            (1.25e-7, "1.25e-7"),
            (100.0, "100"),
            (1.0e23, "1e+23"),
        ];
        for (number, text) in spelled {
            assert_eq!(number_to_string(number), text);
        }
        assert_eq!(rewritten("[1.0,-0,1E2,1e400]", 0), "[1,0,100,null]");
    }

    #[test]
    fn keys_that_are_array_indices_come_first_and_a_key_given_twice_keeps_its_place() {
        let text = r#"{"b":1,"10":2,"2":3,"01":4,"4294967295":5,"4294967294":6,"b":7}"#;
        assert_eq!(
            rewritten(text, 0),
            r#"{"2":3,"10":2,"4294967294":6,"b":7,"01":4,"4294967295":5}"#
        );
    }

    #[test]
    fn half_a_surrogate_pair_is_written_back_as_a_lower_case_escape() {
        assert_eq!(
            rewritten(r#"["\uD800x","\ud83d\ude00","\u0001\u007f\u2028/"]"#, 0),
            "[\"\\ud800x\",\"😀\",\"\\u0001\u{7f}\u{2028}/\"]"
        );
    }

    #[test]
    fn an_indent_puts_each_member_on_its_own_line_and_stops_at_ten_spaces() {
        assert_eq!(
            rewritten(r#"{"a":[1,{}],"b":{"c":[]}}"#, 2),
            "{\n  \"a\": [\n    1,\n    {}\n  ],\n  \"b\": {\n    \"c\": []\n  }\n}"
        );
        assert_eq!(rewritten("[1]", 12), format!("[\n{}1\n]", " ".repeat(10)));
    }

    #[test]
    fn what_javascript_refuses_is_refused() {
        for text in [
            "",
            " ",
            "{\"a\":1,}",
            "[1,]",
            "{'a':1}",
            "01",
            "1.",
            ".5",
            "+1",
            "\u{a0}{}",
            "\u{feff}{}",
            "{} x",
            "\"a\tb\"",
            "\"\\x41\"",
            "\"\\u12g4\"",
            "nul",
            "{\"a\" 1}",
        ] {
            assert!(parse(text).is_err(), "{text:?}");
        }
        assert_eq!(parse("").err(), Some(JsonError::End));
        assert_eq!(
            parse("{\n  \"a\": x").err().map(|error| error.to_string()),
            Some("unexpected character at line 2 column 8".to_owned())
        );
        let deep = format!("{}{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
        assert_eq!(parse(&deep).err(), Some(JsonError::TooDeep));
        let deepest = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
        assert!(parse(&deepest).is_ok());
    }
}
