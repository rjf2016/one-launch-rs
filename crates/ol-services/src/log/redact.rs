// What the Electron app's redaction does to a line before it is written, rule for rule: JavaScript's idea of a space, a digit and a word boundary included, so the masks fall where its masks fall.

use std::borrow::Cow;
use std::fmt::Write as _;
use std::sync::LazyLock;

use ol_formats::js::{WHITESPACE_CLASS, utf16_len};
use ol_formats::js_json::{number_to_string, quote_text};
use ol_model::LogLevel;
use ol_model::redact::AccountNames;
use regex::{Captures, Regex};
use serde_json::{Map, Value};

pub const REDACTED: &str = "[REDACTED]";
pub const EMAIL_MASK: &str = "[EMAIL]";
pub const STEAM_ID_MASK: &str = "[STEAMID]";
pub const USER_MASK: &str = "[USER]";
// Message and fields are each cut to this many UTF-16 units, after redaction: cutting first could leave the start of a secret too short for its pattern to match.
pub const MAX_TEXT_LENGTH: usize = 8 * 1024;
const MAX_DEPTH: i64 = 6;

struct Rules {
    id_tail: Regex,
    key_value: Regex,
    ini_data: Regex,
    steam_login: Regex,
    jwt: Regex,
    email: Regex,
    steam_id64: Regex,
    uuid: Regex,
    profile_path: Regex,
    long_run: Regex,
    sensitive_key: Regex,
}

fn rule(pattern: &str) -> Regex {
    Regex::new(pattern).expect("a fixed pattern")
}

static RULES: LazyLock<Rules> = LazyLock::new(|| {
    let space = WHITESPACE_CLASS;
    let spaces = &WHITESPACE_CLASS[1..WHITESPACE_CLASS.len() - 1];
    let not_space = format!("[^{spaces}]");
    let boundary = r"(?-u:\b)";
    Rules {
        id_tail: rule("\u{2026}[0-9A-Za-z_-]{3,}"),
        key_value: rule(&format!(
            r#"{boundary}((?i-u:pass(?:word|wd|phrase)?|pwd|secret|token|api[_-]?key|apikey|authorization|cookie|session|refresh[_-]?token|access[_-]?token|credentials?|private[_-]?key)){boundary}(\\?["']?{space}*[=:]{space}*)(\\?["']?)[^{spaces}"',;&}}]+"#
        )),
        ini_data: rule(&format!(r"({boundary}Data{space}*={space}*){not_space}+")),
        steam_login: rule(&format!(
            r"((?i-u:-login){space}+{not_space}+{space}+){not_space}+"
        )),
        jwt: rule(r"(^|[^A-Za-z0-9_-])eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}"),
        email: rule(r"[A-Za-z0-9._%+-]{1,64}@[A-Za-z0-9-]{1,63}(?:\.[A-Za-z0-9-]{1,63})+"),
        steam_id64: rule(&format!(r"{boundary}7656119[0-9]{{10}}{boundary}")),
        uuid: rule(&format!(
            r"{boundary}[0-9A-Fa-f]{{8}}-[0-9A-Fa-f]{{4}}-[0-9A-Fa-f]{{4}}-[0-9A-Fa-f]{{4}}-[0-9A-Fa-f]{{12}}{boundary}"
        )),
        profile_path: rule(r#"([A-Za-z]:[\\/]+(?i-u:users)[\\/]+)[^\\/:*?"<>|\r\n]+"#),
        long_run: rule(r"[A-Za-z0-9+/_-]{48,}={0,2}"),
        sensitive_key: rule(
            r"\A(?i-u:pass(?:word|wd|phrase)?|pwd|secret|secret_enc|token|api[_-]?key|apikey|authorization|auth|cookie|session|session[_-]?(?:id|blob|data)|refresh[_-]?token|access[_-]?token|credentials?|private[_-]?key|remember[_-]?me|value_enc|data)\z",
        ),
    }
});

fn replaced<'a>(text: Cow<'a, str>, pattern: &Regex, with: &str) -> Cow<'a, str> {
    match pattern.replace_all(&text, with) {
        Cow::Borrowed(_) => text,
        Cow::Owned(changed) => Cow::Owned(changed),
    }
}

fn replaced_by<'a>(
    text: Cow<'a, str>,
    pattern: &Regex,
    with: impl FnMut(&Captures<'_>) -> String,
) -> Cow<'a, str> {
    match pattern.replace_all(&text, with) {
        Cow::Borrowed(_) => text,
        Cow::Owned(changed) => Cow::Owned(changed),
    }
}

// The Electron app's redactString: the names first, then each pattern in its order.
pub fn redact_string(names: &AccountNames, text: &str) -> String {
    let rules = &*RULES;
    let text = names.mask(text);
    // Beyond the Electron app: the app's own messages name an account by the end of a launcher's id, after an ellipsis.
    let text = replaced(text, &rules.id_tail, "\u{2026}[REDACTED]");
    let text = replaced(text, &rules.key_value, "${1}${2}${3}[REDACTED]");
    let text = replaced(text, &rules.ini_data, "${1}[REDACTED]");
    let text = replaced(text, &rules.steam_login, "${1}[REDACTED]");
    let text = replaced(text, &rules.jwt, "${1}[REDACTED]");
    let text = replaced(text, &rules.email, EMAIL_MASK);
    let text = replaced(text, &rules.steam_id64, STEAM_ID_MASK);
    let text = replaced_by(text, &rules.uuid, |found| {
        let id = &found[0];
        format!("[UUID\u{2026}{}]", &id[id.len() - 4..])
    });
    let text = replaced(text, &rules.profile_path, "${1}[USER]");
    let text = replaced_by(text, &rules.long_run, |found| {
        if found[0].bytes().any(|byte| byte.is_ascii_digit()) {
            REDACTED.to_owned()
        } else {
            found[0].to_owned()
        }
    });
    text.into_owned()
}

pub fn is_sensitive_key(key: &str) -> bool {
    RULES.sensitive_key.is_match(key)
}

// The Electron app's redactValue on what a log field can be: anything under a sensitive key is replaced whole, every string is redacted, and an object or a list deeper than six levels is cut.
pub fn redact_value(names: &AccountNames, value: &Value, depth: i64) -> Value {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        Value::String(text) => Value::String(redact_string(names, text)),
        Value::Array(_) | Value::Object(_) if depth >= MAX_DEPTH => {
            Value::String("[depth]".to_owned())
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| redact_value(names, item, depth + 1))
                .collect(),
        ),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, field)| {
                    let field = if is_sensitive_key(key) {
                        Value::String(REDACTED.to_owned())
                    } else {
                        redact_value(names, field, depth + 1)
                    };
                    (key.clone(), field)
                })
                .collect::<Map<String, Value>>(),
        ),
    }
}

// JSON.stringify of a value without an indent.
fn stringify(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => match (number.as_i64(), number.as_u64(), number.as_f64()) {
            (Some(whole), _, _) => {
                let _ = write!(out, "{whole}");
            }
            (_, Some(whole), _) => {
                let _ = write!(out, "{whole}");
            }
            (_, _, Some(float)) => out.push_str(&number_to_string(float)),
            _ => out.push_str("null"),
        },
        Value::String(text) => quote_text(out, text),
        Value::Array(items) => {
            out.push('[');
            for (at, item) in items.iter().enumerate() {
                if at > 0 {
                    out.push(',');
                }
                stringify(out, item);
            }
            out.push(']');
        }
        Value::Object(fields) => {
            out.push('{');
            for (at, (key, field)) in fields.iter().enumerate() {
                if at > 0 {
                    out.push(',');
                }
                quote_text(out, key);
                out.push(':');
                stringify(out, field);
            }
            out.push('}');
        }
    }
}

// Past the limit the text is cut and says how much was left out. A cut through a surrogate pair leaves U+FFFD, which is what the Electron app's file holds there.
fn clip(text: &str) -> Cow<'_, str> {
    let total = utf16_len(text);
    if total <= MAX_TEXT_LENGTH {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(MAX_TEXT_LENGTH + 32);
    let mut units = 0;
    for character in text.chars() {
        let width = character.len_utf16();
        if units + width > MAX_TEXT_LENGTH {
            if units < MAX_TEXT_LENGTH {
                out.push('\u{fffd}');
            }
            break;
        }
        out.push(character);
        units += width;
    }
    let _ = write!(
        out,
        "\u{2026} [{} more characters]",
        total - MAX_TEXT_LENGTH
    );
    Cow::Owned(out)
}

fn on_one_line(text: &str) -> Cow<'_, str> {
    if !text.contains(['\r', '\n']) {
        return Cow::Borrowed(text);
    }
    Cow::Owned(text.replace('\r', "\\r").replace('\n', "\\n"))
}

pub const fn level_label(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Debug => "DEBUG",
        LogLevel::Info => "INFO ",
        LogLevel::Warn => "WARN ",
        LogLevel::Error => "ERROR",
    }
}

// One line of the log: `<time> <LEVEL> [<scope>] <message>`, then a space and the fields as JSON when there are any. The message and the fields are redacted, then cut; the message is kept on one line. The time and the scope are the app's own and written as given.
pub fn format_line(
    names: &AccountNames,
    timestamp: &str,
    level: LogLevel,
    scope: &str,
    message: &str,
    fields: Option<&Value>,
) -> String {
    let redacted = redact_string(names, message);
    let mut line = format!(
        "{timestamp} {} [{scope}] {}",
        level_label(level),
        on_one_line(&clip(&redacted))
    );
    if let Some(fields) = fields {
        let mut json = String::new();
        stringify(&mut json, &redact_value(names, fields, 0));
        line.push(' ');
        line.push_str(&clip(&redact_string(names, &json)));
    }
    line.push('\n');
    line
}
