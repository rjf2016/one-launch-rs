use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, PoisonError, RwLock, TryLockError};

pub const ACCOUNT_MASK: &str = "[ACCOUNT]";
pub const USER_MASK: &str = "[USER]";

const MIN_NAME_LENGTH: usize = 3;
const MIN_USER_LENGTH: usize = 2;
const MAX_NAME_LENGTH: usize = 256;
const MAX_ACCOUNT_NAMES: usize = 2000;
const MIN_ID_LENGTH: usize = 6;
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

struct Name {
    units: usize,
    chars: Vec<char>,
    mask: &'static str,
}

#[derive(Default)]
pub struct AccountNames {
    stored: HashSet<String>,
    users: HashSet<String>,
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

// JavaScript's Canonicalize for a case-insensitive pattern without the u flag, which the Electron app's name pattern used.
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

fn folded(text: &str) -> Vec<char> {
    text.chars().map(canonical).collect()
}

impl AccountNames {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.by_first.is_empty()
    }

    pub fn len(&self) -> usize {
        self.stored.len()
    }

    fn insert(&mut self, chars: Vec<char>, mask: &'static str) {
        let Some(&first) = chars.first() else {
            return;
        };
        let units = chars.iter().map(|c| c.len_utf16()).sum();
        let bucket = self.by_first.entry(first).or_default();
        if bucket.iter().any(|held| held.chars == chars) {
            return;
        }
        let at = bucket
            .iter()
            .position(|held| held.units < units)
            .unwrap_or(bucket.len());
        bucket.insert(at, Name { units, chars, mask });
    }

    // A name whose lower case does not fold back to its own fold (İ, ẞ, the Kelvin, Ohm and Angstrom signs, ϴ) is also held as written, which the Electron app missed.
    fn insert_both(&mut self, written: &str, lower: &str, mask: &'static str) {
        self.insert(folded(lower), mask);
        let as_written = folded(written);
        if (1..=MAX_NAME_LENGTH).contains(&written.encode_utf16().count()) {
            self.insert(as_written, mask);
        }
    }

    pub fn add<I>(&mut self, names: I)
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        for raw in names {
            let written = raw.as_ref().trim_matches(is_js_space);
            let name = written.to_lowercase();
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
            self.insert_both(written, &name, ACCOUNT_MASK);
            self.stored.insert(name);
        }
    }

    pub fn add_user(&mut self, user: &str) {
        let written = user.trim_matches(is_js_space);
        let name = written.to_lowercase();
        let units = name.encode_utf16().count();
        if !(MIN_USER_LENGTH..=MAX_NAME_LENGTH).contains(&units) || !self.users.insert(name.clone())
        {
            return;
        }
        self.insert_both(written, &name, USER_MASK);
    }

    pub fn mask<'a>(&self, text: &'a str) -> Cow<'a, str> {
        if self.by_first.is_empty() {
            return Cow::Borrowed(text);
        }
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let folded: Vec<char> = chars.iter().map(|&(_, c)| canonical(c)).collect();
        let mut out = String::new();
        let mut copied = 0;
        let mut changed = false;
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
                        (fits && whole_end).then_some((end, name.mask))
                    })
                });
            match found {
                Some((end, mask)) => {
                    out.push_str(&text[copied..chars[at].0]);
                    out.push_str(mask);
                    copied = chars.get(end).map_or(text.len(), |&(byte, _)| byte);
                    changed = true;
                    at = end;
                }
                None => at += 1,
            }
        }
        if !changed {
            return Cow::Borrowed(text);
        }
        out.push_str(&text[copied..]);
        Cow::Owned(out)
    }
}

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

// Beyond the Electron app; long enough not to be mistaken for a count.
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

// The profile folder is masked by its last part, so it is found in any spelling of the path: raw, JSON-escaped or with forward slashes, on a share or a redirected folder.
pub fn register_windows_user(profile: Option<&str>, user_name: Option<&str>) {
    let mut names = REGISTERED.write().unwrap_or_else(PoisonError::into_inner);
    let leaf = profile.and_then(|profile| {
        profile
            .split(['\\', '/'])
            .rfind(|part| !part.trim().is_empty())
    });
    for user in leaf.into_iter().chain(user_name) {
        names.add_user(user);
    }
}

pub fn with_registered_names<R>(read: impl FnOnce(&AccountNames) -> R) -> R {
    read(&REGISTERED.read().unwrap_or_else(PoisonError::into_inner))
}

// For a panic, which may have struck while this thread held the names: never waits.
pub fn try_with_registered_names<R>(read: impl FnOnce(&AccountNames) -> R) -> Option<R> {
    match REGISTERED.try_read() {
        Ok(names) => Some(read(&names)),
        Err(TryLockError::Poisoned(poisoned)) => Some(read(&poisoned.into_inner())),
        Err(TryLockError::WouldBlock) => None,
    }
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
