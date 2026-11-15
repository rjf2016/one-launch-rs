// Valve's text KeyValues format, which Steam uses for loginusers.vdf, libraryfolders.vdf and appmanifest_*.acf.

use std::collections::{BTreeMap, HashMap};

use thiserror::Error;

use crate::js;

const MAX_DEPTH: usize = 64;
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VdfValue {
    Text(String),
    Block(VdfObject),
}

// Keys come back in the order the Electron app sees them: whole numbers below 2^32 - 1 first, ascending, then the others as first written. A key written twice keeps its first place and its last value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VdfObject {
    indexed: BTreeMap<u32, VdfValue>,
    named: Vec<(String, VdfValue)>,
    places: HashMap<String, usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
#[error("{message} (line {line})")]
pub struct VdfParseError {
    pub message: String,
    pub line: u32,
}

impl VdfParseError {
    pub const CODE: &'static str = "VDF_PARSE";

    fn new(message: impl Into<String>, line: u32) -> Self {
        Self {
            message: message.into(),
            line,
        }
    }
}

fn index_of(key: &str) -> Option<u32> {
    let index: u32 = key.parse().ok()?;
    (index != u32::MAX && index.to_string() == key).then_some(index)
}

impl VdfObject {
    pub fn insert(&mut self, key: &str, value: VdfValue) {
        if let Some(index) = index_of(key) {
            self.indexed.insert(index, value);
        } else if let Some(place) = self.places.get(key) {
            self.named[*place].1 = value;
        } else {
            self.places.insert(key.to_owned(), self.named.len());
            self.named.push((key.to_owned(), value));
        }
    }

    pub fn entries(&self) -> impl Iterator<Item = (String, &VdfValue)> {
        let indexed = self
            .indexed
            .iter()
            .map(|(index, value)| (index.to_string(), value));
        let named = self.named.iter().map(|(key, value)| (key.clone(), value));
        indexed.chain(named)
    }

    pub fn len(&self) -> usize {
        self.indexed.len() + self.named.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn exact(&self, key: &str) -> Option<&VdfValue> {
        match index_of(key) {
            Some(index) => self.indexed.get(&index),
            None => self.places.get(key).map(|place| &self.named[*place].1),
        }
    }

    // The key as written wins; failing that, the first key that differs only in case.
    pub fn get(&self, key: &str) -> Option<&VdfValue> {
        if let Some(found) = self.exact(key) {
            return Some(found);
        }
        let lower = key.to_lowercase();
        self.named
            .iter()
            .find(|(named, _)| named.to_lowercase() == lower)
            .map(|(_, value)| value)
    }

    pub fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    pub fn string(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            VdfValue::Text(text) => Some(text),
            VdfValue::Block(_) => None,
        }
    }

    pub fn block(&self, key: &str) -> Option<&VdfObject> {
        match self.get(key)? {
            VdfValue::Text(_) => None,
            VdfValue::Block(block) => Some(block),
        }
    }

    // A whole number no larger than 2^53 - 1 either way, which is what the Electron app can hold exactly.
    pub fn int(&self, key: &str) -> Option<i64> {
        let text = js::trim(self.string(key)?);
        let (negative, digits) = match text.strip_prefix('-') {
            Some(digits) => (true, digits),
            None => (false, text),
        };
        if !js::is_ascii_digits(digits) {
            return None;
        }
        let significant = digits.trim_start_matches('0');
        if significant.len() > 16 {
            return None;
        }
        let magnitude: u64 = if significant.is_empty() {
            0
        } else {
            significant.parse().ok()?
        };
        if magnitude > MAX_SAFE_INTEGER {
            return None;
        }
        let value = i64::try_from(magnitude).ok()?;
        Some(if negative { -value } else { value })
    }

    pub fn flag(&self, key: &str) -> bool {
        self.string(key).is_some_and(|text| js::trim(text) == "1")
    }
}

enum Token {
    Text {
        value: String,
        quoted: bool,
        line: u32,
    },
    Open(u32),
    Close(u32),
}

fn tokenize(text: &str) -> Result<Vec<Token>, VdfParseError> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut tokens = Vec::new();
    let mut i = 0;
    let mut line = 1_u32;
    while i < n {
        let ch = chars[i];
        if ch == '\n' {
            line += 1;
            i += 1;
        } else if js::is_whitespace(ch) {
            // The Electron app skips a space, a tab, a carriage return and a byte order mark here, and never gets past any other blank character.
            i += 1;
        } else if ch == '/' && chars.get(i + 1) == Some(&'/') {
            while i < n && chars[i] != '\n' {
                i += 1;
            }
        } else if ch == '{' {
            tokens.push(Token::Open(line));
            i += 1;
        } else if ch == '}' {
            tokens.push(Token::Close(line));
            i += 1;
        } else if ch == '"' {
            let start_line = line;
            let unterminated = || VdfParseError::new("Unterminated string", start_line);
            let mut value = String::new();
            i += 1;
            loop {
                let c = *chars.get(i).ok_or_else(unterminated)?;
                if c == '"' {
                    i += 1;
                    break;
                }
                if c == '\\' {
                    match *chars.get(i + 1).ok_or_else(unterminated)? {
                        escaped @ ('\\' | '"') => value.push(escaped),
                        'n' => value.push('\n'),
                        't' => value.push('\t'),
                        'r' => value.push('\r'),
                        other => {
                            value.push(c);
                            value.push(other);
                        }
                    }
                    i += 2;
                    continue;
                }
                if c == '\n' {
                    line += 1;
                }
                value.push(c);
                i += 1;
            }
            tokens.push(Token::Text {
                value,
                quoted: true,
                line: start_line,
            });
        } else {
            let mut j = i;
            while j < n && !(js::is_whitespace(chars[j]) || matches!(chars[j], '{' | '}' | '"')) {
                j += 1;
            }
            tokens.push(Token::Text {
                value: chars[i..j].iter().collect(),
                quoted: false,
                line,
            });
            i = j;
        }
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn block(&mut self, opened_at: Option<u32>, depth: usize) -> Result<VdfObject, VdfParseError> {
        let mut into = VdfObject::default();
        while let Some(token) = self.tokens.get(self.pos) {
            let (key, line) = match token {
                Token::Close(line) => {
                    if opened_at.is_none() {
                        return Err(VdfParseError::new("Unexpected \"}\"", *line));
                    }
                    self.pos += 1;
                    return Ok(into);
                }
                Token::Open(line) => {
                    return Err(VdfParseError::new(
                        "Unexpected \"{\" where a key was expected",
                        *line,
                    ));
                }
                // A #base or #include line names another file: the directive goes, and its argument with it.
                Token::Text { value, quoted, .. } if !quoted && value.starts_with('#') => {
                    self.pos += 2;
                    continue;
                }
                Token::Text { value, line, .. } => (value.clone(), *line),
            };
            self.pos += 1;
            let no_value = || VdfParseError::new(format!("Key \"{key}\" has no value"), line);
            match self.tokens.get(self.pos) {
                Some(Token::Open(at)) => {
                    if depth >= MAX_DEPTH {
                        return Err(VdfParseError::new("Blocks are nested too deeply", *at));
                    }
                    let at = *at;
                    self.pos += 1;
                    let child = self.block(Some(at), depth + 1)?;
                    into.insert(&key, VdfValue::Block(child));
                }
                Some(Token::Text { value, .. }) => {
                    into.insert(&key, VdfValue::Text(value.clone()));
                    self.pos += 1;
                }
                Some(Token::Close(_)) | None => return Err(no_value()),
            }
            // A condition such as [$WIN32] may follow a value.
            if let Some(Token::Text { value, quoted, .. }) = self.tokens.get(self.pos)
                && !quoted
                && value.starts_with('[')
            {
                self.pos += 1;
            }
        }
        match opened_at {
            Some(line) => Err(VdfParseError::new("Missing \"}\" for block opened", line)),
            None => Ok(into),
        }
    }
}

pub fn parse(text: &str) -> Result<VdfObject, VdfParseError> {
    let mut parser = Parser {
        tokens: tokenize(text)?,
        pos: 0,
    };
    parser.block(None, 0)
}
