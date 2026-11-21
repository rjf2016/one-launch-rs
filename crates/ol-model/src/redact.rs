// The names of the accounts on this PC, which the log masks wherever they appear: a login or a display name has no shape a pattern could catch. Whoever reads one from the database or a launcher file hands it over here first, so any line built from it later is already covered. Names are only ever added, as in the Electron app.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, PoisonError, RwLock};

pub const ACCOUNT_MASK: &str = "[ACCOUNT]";

const MIN_NAME_LENGTH: usize = 3;
const MAX_NAME_LENGTH: usize = 256;
const MAX_ACCOUNT_NAMES: usize = 2000;
const MIN_ID_LENGTH: usize = 6;
// Labels that identify nobody and are ordinary log words.
const NOT_A_NAME: [&str; 16] = [
    "steam",
    "epic",
    "epic games",
    "battle.net",
    "battlenet",
    "blizzard",
    "riot",
    "riot games",
    "riot client",
    "valorant",
    "main",
    "alt",
    "account",
    "default",
    "test",
    "user",
];

// A name as the log compares it: each character as JavaScript's case-insensitive match without the u flag sees it.
struct Name {
    units: usize,
    chars: Vec<char>,
}

// No Debug: it holds every name it masks.
#[derive(Default)]
pub struct AccountNames {
    stored: HashSet<String>,
    // By the first character, longest first and in the order given among names of one length, as the Electron app's alternation tries them.
    by_first: HashMap<char, Vec<Name>>,
}

static REGISTERED: LazyLock<RwLock<AccountNames>> = LazyLock::new(RwLock::default);

const fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

const fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

// JavaScript's Canonicalize for a case-insensitive pattern without the u flag: the upper case of one UTF-16 unit, unless that is more than one unit or takes a character outside ASCII into it.
fn canonical(c: char) -> char {
    if c.len_utf16() > 1 {
        return c;
    }
    let mut upper = c.to_uppercase();
    match (upper.next(), upper.next()) {
        (Some(single), None) if single.len_utf16() == 1 && (c.is_ascii() || !single.is_ascii()) => {
            single
        }
        _ => c,
    }
}

impl AccountNames {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.stored.is_empty()
    }

    pub fn len(&self) -> usize {
        self.stored.len()
    }

    // Trimmed and lower-cased; a name under 3 or over 256 UTF-16 units, a launcher word, a name already held and any name past the 2000th are passed over.
    pub fn add<I>(&mut self, names: I)
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        for raw in names {
            let name = raw.as_ref().trim_matches(is_js_space).to_lowercase();
            let units = name.encode_utf16().count();
            if !(MIN_NAME_LENGTH..=MAX_NAME_LENGTH).contains(&units)
                || NOT_A_NAME.contains(&name.as_str())
                || self.stored.contains(&name)
            {
                continue;
            }
            if self.stored.len() >= MAX_ACCOUNT_NAMES {
                return;
            }
            let chars: Vec<char> = name.chars().map(canonical).collect();
            let Some(&first) = chars.first() else {
                continue;
            };
            let bucket = self.by_first.entry(first).or_default();
            let at = bucket
                .iter()
                .position(|held| held.units < units)
                .unwrap_or(bucket.len());
            bucket.insert(at, Name { units, chars });
            self.stored.insert(name);
        }
    }

    // Every whole name, in any case, as [ACCOUNT]: a name is whole when no ASCII letter, digit or underscore touches it.
    pub fn mask<'a>(&self, text: &'a str) -> Cow<'a, str> {
        if self.stored.is_empty() {
            return Cow::Borrowed(text);
        }
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let folded: Vec<char> = chars.iter().map(|&(_, c)| canonical(c)).collect();
        let mut out = String::new();
        let mut copied = 0;
        let mut at = 0;
        while at < chars.len() {
            let whole_start = at == 0 || !is_word(chars[at - 1].1);
            let found = whole_start
                .then(|| self.by_first.get(&folded[at]))
                .flatten()
                .and_then(|names| {
                    names.iter().find_map(|name| {
                        let end = at + name.chars.len();
                        let fits = folded.get(at..end) == Some(name.chars.as_slice());
                        let whole_end = chars.get(end).is_none_or(|&(_, c)| !is_word(c));
                        (fits && whole_end).then_some(end)
                    })
                });
            match found {
                Some(end) => {
                    out.push_str(&text[copied..chars[at].0]);
                    out.push_str(ACCOUNT_MASK);
                    copied = chars.get(end).map_or(text.len(), |&(byte, _)| byte);
                    at = end;
                }
                None => at += 1,
            }
        }
        if copied == 0 && out.is_empty() {
            return Cow::Borrowed(text);
        }
        out.push_str(&text[copied..]);
        Cow::Owned(out)
    }
}

// Hands names to the log's redaction for the rest of the run.
pub fn register_account_names<I>(names: I)
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    REGISTERED
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .add(names);
}

// A launcher's id for an account is masked like a name, beyond what the Electron app does, once it is long enough not to be mistaken for a count.
pub fn register_account_ids<I>(ids: I)
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    let ids: Vec<I::Item> = ids
        .into_iter()
        .filter(|id| id.as_ref().trim().len() >= MIN_ID_LENGTH)
        .collect();
    register_account_names(ids);
}

pub fn with_registered_names<R>(read: impl FnOnce(&AccountNames) -> R) -> R {
    read(&REGISTERED.read().unwrap_or_else(PoisonError::into_inner))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kelvin_sign_and_a_long_s_never_fold_into_ascii() {
        assert_eq!(canonical('k'), 'K');
        assert_eq!(canonical('\u{212a}'), '\u{212a}');
        assert_eq!(canonical('\u{17f}'), '\u{17f}');
        assert_eq!(canonical('ß'), 'ß');
        assert_eq!(canonical('é'), 'É');
        assert_eq!(canonical('😀'), '😀');
    }

    #[test]
    fn a_name_is_masked_whole_and_in_any_case() {
        let mut names = AccountNames::new();
        names.add(["Bob", "gordon", "gordon freeman"]);
        assert_eq!(
            names.mask("BOB, bobsled, gordon freeman, gordon_x"),
            "[ACCOUNT], bobsled, [ACCOUNT], gordon_x"
        );
        assert!(matches!(names.mask("nothing here"), Cow::Borrowed(_)));
    }
}
