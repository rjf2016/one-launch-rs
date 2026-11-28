use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use thiserror::Error;

pub const MAX_BYTES: usize = 64 * 1024;
pub const MAX_DEPTH: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Yaml {
    Null,
    Bool(bool),
    Number(String),
    Text(String),
    List(Vec<Yaml>),
    Map(Vec<(String, Yaml)>),
}

impl Yaml {
    pub fn get(&self, key: &str) -> Option<&Yaml> {
        match self {
            Self::Map(entries) => entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum Unreadable {
    #[error("the file is larger than {MAX_BYTES} bytes")]
    TooLarge,
    #[error("line {0} nests deeper than {MAX_DEPTH} levels")]
    TooDeep(usize),
    #[error("line {0} holds a tab")]
    Tab(usize),
    #[error("line {0} holds a control character")]
    ControlCharacter(usize),
    #[error("line {0} holds a carriage return that ends no line")]
    CarriageReturn(usize),
    #[error("line {0} holds a comment")]
    Comment(usize),
    #[error("line {0} holds a directive or a document marker")]
    Document(usize),
    #[error("line {0} holds an anchor, an alias or a tag")]
    NodeProperty(usize),
    #[error("line {0} starts a block scalar")]
    BlockScalar(usize),
    #[error("line {0} holds a flow collection")]
    Flow(usize),
    #[error("line {0} holds a complex key")]
    ComplexKey(usize),
    #[error("line {0} holds a key that is not text")]
    KeyNotText(usize),
    #[error("line {0} holds a merge key")]
    Merge(usize),
    #[error("line {0} holds a timestamp")]
    Timestamp(usize),
    #[error("line {0} repeats a key")]
    DuplicateKey(usize),
    #[error("line {0} holds a quoted scalar that does not end on it")]
    UnclosedQuote(usize),
    #[error("line {0} holds an escape that is not allowed")]
    Escape(usize),
    #[error("line {0} is not indented as its block")]
    Indentation(usize),
    #[error("line {0} is not a key, a list item or a value")]
    Syntax(usize),
}

static FLOAT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?:[-+]?[0-9]+(?:\.[0-9]*)?(?:[eE][-+]?[0-9]+)?|\.[0-9]+(?:[eE][-+]?[0-9]+)?|[-+]?\.(?:inf|Inf|INF)|\.(?:nan|NaN|NAN))$",
    )
    .expect("a fixed pattern")
});

static FLOAT_SPECIAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:[-+]?\.(?:inf|Inf|INF)|\.(?:nan|NaN|NAN))$").expect("a fixed pattern")
});

static DATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}$").expect("a fixed pattern"));

static TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^[0-9]{4}-[0-9]{1,2}-[0-9]{1,2}(?:[Tt]|[ \t]+)[0-9]{1,2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]*)?(?:[ \t]*(?:Z|[-+][0-9]{1,2}(?::[0-9]{2})?))?$",
    )
    .expect("a fixed pattern")
});

pub fn read(bytes: &[u8]) -> Result<Yaml, Unreadable> {
    if bytes.len() > MAX_BYTES {
        return Err(Unreadable::TooLarge);
    }
    let decoded = String::from_utf8_lossy(bytes);
    let text = decoded.strip_prefix('\u{feff}').unwrap_or(&decoded);
    let mut parser = Parser {
        lines: lines(text)?,
        at: 0,
    };
    let value = parser.block(None, 1)?;
    match parser.lines.get(parser.at) {
        Some(line) => Err(Unreadable::Indentation(line.number)),
        None => Ok(value),
    }
}

struct Line<'a> {
    number: usize,
    indent: usize,
    text: &'a str,
}

fn lines(text: &str) -> Result<Vec<Line<'_>>, Unreadable> {
    let mut lines = Vec::new();
    for (index, raw) in text.split('\n').enumerate() {
        let number = index + 1;
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        for c in raw.chars() {
            match c {
                '\r' => return Err(Unreadable::CarriageReturn(number)),
                '\t' => return Err(Unreadable::Tab(number)),
                c if not_printable(c) => return Err(Unreadable::ControlCharacter(number)),
                _ => {}
            }
        }
        let body = raw.trim_start_matches(' ');
        let indent = raw.len() - body.len();
        let body = body.trim_end_matches(' ');
        if body.is_empty() {
            continue;
        }
        if body.starts_with('#') {
            return Err(Unreadable::Comment(number));
        }
        if indent == 0 && document_line(body) {
            return Err(Unreadable::Document(number));
        }
        lines.push(Line {
            number,
            indent,
            text: body,
        });
    }
    Ok(lines)
}

fn not_printable(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{2028}' | '\u{2029}' | '\u{feff}' | '\u{fffe}' | '\u{ffff}'
        )
}

fn document_line(body: &str) -> bool {
    body.starts_with('%')
        || ["---", "..."].iter().any(|marker| {
            body.strip_prefix(marker)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
        })
}

enum Kind<'a> {
    Item(&'a str),
    Entry(String, &'a str),
    Scalar,
}

fn kind(text: &str, number: usize) -> Result<Kind<'_>, Unreadable> {
    if text == "-" {
        return Ok(Kind::Item(""));
    }
    if let Some(rest) = text.strip_prefix("- ") {
        return Ok(Kind::Item(rest));
    }
    if text.starts_with(['\'', '"']) {
        let (key, after) = quoted(text, number)?;
        let trimmed = after.trim_start_matches(' ');
        if trimmed.is_empty() {
            return Ok(Kind::Scalar);
        }
        if let Some(value) = trimmed.strip_prefix(':')
            && (value.is_empty() || value.starts_with(' '))
        {
            return Ok(Kind::Entry(key, value.trim_start_matches(' ')));
        }
        return Err(after_quote(after, number));
    }
    if text.starts_with('[') {
        return Ok(Kind::Scalar);
    }
    plain_start(text, number)?;
    let Some(colon) = separator(text) else {
        return Ok(Kind::Scalar);
    };
    let key = text[..colon].trim_end_matches(' ');
    plain_body(key, number)?;
    match resolve(key, number)? {
        Yaml::Text(key) => Ok(Kind::Entry(key, text[colon + 1..].trim_start_matches(' '))),
        _ => Err(Unreadable::KeyNotText(number)),
    }
}

fn separator(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    (0..bytes.len())
        .find(|&at| bytes[at] == b':' && bytes.get(at + 1).is_none_or(|&next| next == b' '))
}

fn after_quote(after: &str, number: usize) -> Unreadable {
    if after.starts_with(' ') && after.trim_start_matches(' ').starts_with('#') {
        Unreadable::Comment(number)
    } else {
        Unreadable::Syntax(number)
    }
}

fn plain_start(text: &str, number: usize) -> Result<(), Unreadable> {
    let mut chars = text.chars();
    let first = chars.next();
    let second = chars.next();
    match first {
        Some('&' | '*' | '!') => Err(Unreadable::NodeProperty(number)),
        Some('|' | '>') => Err(Unreadable::BlockScalar(number)),
        Some('[' | '{') => Err(Unreadable::Flow(number)),
        Some('?') => Err(Unreadable::ComplexKey(number)),
        Some('#') => Err(Unreadable::Comment(number)),
        Some('-') if second.is_none_or(|c| c == ' ') => Err(Unreadable::Syntax(number)),
        Some(':' | ',' | ']' | '}' | '%' | '@' | '`') => Err(Unreadable::Syntax(number)),
        _ => Ok(()),
    }
}

fn plain_body(text: &str, number: usize) -> Result<(), Unreadable> {
    if text.contains(" #") {
        return Err(Unreadable::Comment(number));
    }
    if text.contains(": ") || text.ends_with(':') {
        return Err(Unreadable::Syntax(number));
    }
    Ok(())
}

fn inline(text: &str, number: usize) -> Result<Yaml, Unreadable> {
    if text.starts_with(['\'', '"']) {
        let (value, after) = quoted(text, number)?;
        return if after.trim_start_matches(' ').is_empty() {
            Ok(Yaml::Text(value))
        } else {
            Err(after_quote(after, number))
        };
    }
    if let Some(inner) = text.strip_prefix('[') {
        return if inner.trim_start_matches(' ') == "]" {
            Ok(Yaml::List(Vec::new()))
        } else {
            Err(Unreadable::Flow(number))
        };
    }
    plain_start(text, number)?;
    plain_body(text, number)?;
    resolve(text, number)
}

fn resolve(text: &str, number: usize) -> Result<Yaml, Unreadable> {
    match text {
        "~" | "null" | "Null" | "NULL" => return Ok(Yaml::Null),
        "true" | "True" | "TRUE" => return Ok(Yaml::Bool(true)),
        "false" | "False" | "FALSE" => return Ok(Yaml::Bool(false)),
        "<<" => return Err(Unreadable::Merge(number)),
        _ => {}
    }
    if integer(text) || float(text) {
        return Ok(Yaml::Number(text.to_owned()));
    }
    if DATE.is_match(text) || TIMESTAMP.is_match(text) {
        return Err(Unreadable::Timestamp(number));
    }
    Ok(Yaml::Text(text.to_owned()))
}

// js-yaml 4 reads a whole number too large for a double as text, so finiteness decides here as it does there.
fn integer(text: &str) -> bool {
    let unsigned = text.strip_prefix(['-', '+']).unwrap_or(text);
    if unsigned == "0" {
        return true;
    }
    let radix = match unsigned.get(..2) {
        Some("0b") => Some(2),
        Some("0x") => Some(16),
        Some("0o") => Some(8),
        _ => None,
    };
    if let Some(radix) = radix {
        let digits = &unsigned[2..];
        return !digits.is_empty()
            && digits.chars().all(|c| c.is_digit(radix))
            && digits
                .chars()
                .filter_map(|c| c.to_digit(radix))
                .fold(0.0_f64, |value, digit| {
                    value * f64::from(radix) + f64::from(digit)
                })
                .is_finite();
    }
    !unsigned.is_empty()
        && unsigned.bytes().all(|b| b.is_ascii_digit())
        && text.parse::<f64>().is_ok_and(f64::is_finite)
}

fn float(text: &str) -> bool {
    FLOAT.is_match(text)
        && (text.parse::<f64>().is_ok_and(f64::is_finite) || FLOAT_SPECIAL.is_match(text))
}

fn quoted(text: &str, number: usize) -> Result<(String, &str), Unreadable> {
    let mut chars = text.char_indices();
    let quote = chars.next().map(|(_, c)| c);
    let mut value = String::new();
    while let Some((at, c)) = chars.next() {
        match (quote, c) {
            (Some('\''), '\'') => {
                if text[at + 1..].starts_with('\'') {
                    chars.next();
                    value.push('\'');
                } else {
                    return Ok((value, &text[at + 1..]));
                }
            }
            (Some('"'), '"') => return Ok((value, &text[at + 1..])),
            (Some('"'), '\\') => {
                let Some((_, escape)) = chars.next() else {
                    return Err(Unreadable::UnclosedQuote(number));
                };
                value.push(unescape(escape, &mut chars).ok_or(Unreadable::Escape(number))?);
            }
            _ => value.push(c),
        }
    }
    Err(Unreadable::UnclosedQuote(number))
}

fn unescape(escape: char, chars: &mut std::str::CharIndices<'_>) -> Option<char> {
    let digits = match escape {
        '0' => return Some('\0'),
        'a' => return Some('\u{7}'),
        'b' => return Some('\u{8}'),
        't' => return Some('\t'),
        'n' => return Some('\n'),
        'v' => return Some('\u{b}'),
        'f' => return Some('\u{c}'),
        'r' => return Some('\r'),
        'e' => return Some('\u{1b}'),
        ' ' | '"' | '/' | '\\' => return Some(escape),
        'N' => return Some('\u{85}'),
        '_' => return Some('\u{a0}'),
        'L' => return Some('\u{2028}'),
        'P' => return Some('\u{2029}'),
        'x' => 2,
        'u' => 4,
        'U' => 8,
        _ => return None,
    };
    let mut code = 0_u32;
    for _ in 0..digits {
        code = code * 16 + chars.next()?.1.to_digit(16)?;
    }
    char::from_u32(code)
}

struct Parser<'a> {
    lines: Vec<Line<'a>>,
    at: usize,
}

impl Parser<'_> {
    fn block(&mut self, parent: Option<usize>, depth: usize) -> Result<Yaml, Unreadable> {
        let Some(line) = self.lines.get(self.at) else {
            return Ok(Yaml::Null);
        };
        if parent.is_some_and(|parent| line.indent <= parent) {
            return Ok(Yaml::Null);
        }
        let (indent, number, text) = (line.indent, line.number, line.text);
        if depth > MAX_DEPTH {
            return Err(Unreadable::TooDeep(number));
        }
        match kind(text, number)? {
            Kind::Item(_) => self.list(indent, depth),
            Kind::Entry(..) => self.map(indent, depth),
            Kind::Scalar => {
                self.at += 1;
                let value = inline(text, number)?;
                self.after_scalar(parent)?;
                Ok(value)
            }
        }
    }

    fn after_scalar(&self, limit: Option<usize>) -> Result<(), Unreadable> {
        match self.lines.get(self.at) {
            Some(next) if limit.is_none_or(|limit| next.indent > limit) => {
                Err(Unreadable::Indentation(next.number))
            }
            _ => Ok(()),
        }
    }

    fn list(&mut self, indent: usize, depth: usize) -> Result<Yaml, Unreadable> {
        let mut items = Vec::new();
        while let Some(line) = self.lines.get(self.at) {
            if line.indent < indent {
                break;
            }
            let (number, text) = (line.number, line.text);
            if line.indent > indent {
                return Err(Unreadable::Indentation(number));
            }
            let Kind::Item(rest) = kind(text, number)? else {
                return Err(Unreadable::Syntax(number));
            };
            let content = rest.trim_start_matches(' ');
            if content.is_empty() {
                self.at += 1;
            } else {
                self.lines[self.at] = Line {
                    number,
                    indent: indent + text.len() - content.len(),
                    text: content,
                };
            }
            items.push(self.block(Some(indent), depth + 1)?);
        }
        Ok(Yaml::List(items))
    }

    fn map(&mut self, indent: usize, depth: usize) -> Result<Yaml, Unreadable> {
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        while let Some(line) = self.lines.get(self.at) {
            if line.indent < indent {
                break;
            }
            let (number, text) = (line.number, line.text);
            if line.indent > indent {
                return Err(Unreadable::Indentation(number));
            }
            let Kind::Entry(key, value) = kind(text, number)? else {
                return Err(Unreadable::Syntax(number));
            };
            if !seen.insert(key.clone()) {
                return Err(Unreadable::DuplicateKey(number));
            }
            self.at += 1;
            let value = if value.is_empty() {
                self.block(Some(indent), depth + 1)?
            } else {
                let value = inline(value, number)?;
                self.after_scalar(Some(indent))?;
                value
            };
            entries.push((key, value));
        }
        Ok(Yaml::Map(entries))
    }
}
