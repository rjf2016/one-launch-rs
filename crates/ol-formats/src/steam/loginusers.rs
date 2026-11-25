// <SteamPath>\config\loginusers.vdf: the accounts Steam remembers on this PC. Older clients wrote MostRecent where current ones write AutoLogin; the two mean the same.

use std::fmt;
use std::sync::LazyLock;

use ol_model::redact::{register_account_ids, register_account_names};
use regex::Regex;
use thiserror::Error;

use crate::js::{self, WHITESPACE_CLASS as WS};
use crate::vdf::{self, VdfObject, VdfParseError, VdfValue};

// SteamID64 of account 0: the id Steam keeps in ActiveUser is the SteamID64 less this.
const STEAMID64_BASE: i128 = 76_561_197_960_265_728;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MostRecentKey {
    AutoLogin,
    MostRecent,
}

impl MostRecentKey {
    pub const ALL: [Self; 2] = [Self::AutoLogin, Self::MostRecent];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AutoLogin => "AutoLogin",
            Self::MostRecent => "MostRecent",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct SteamLoginUser {
    pub steam_id64: String,
    pub account_name: String,
    pub persona_name: String,
    pub remember_password: bool,
    pub most_recent: bool,
    pub timestamp: Option<i64>,
    pub most_recent_keys: Vec<MostRecentKey>,
}

// The id and both names say whose account this is: none of them is printed.
impl fmt::Debug for SteamLoginUser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SteamLoginUser")
            .field("remember_password", &self.remember_password)
            .field("most_recent", &self.most_recent)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum LoginUsersError {
    #[error(transparent)]
    Vdf(#[from] VdfParseError),
    #[error("{0}")]
    Format(String),
}

impl LoginUsersError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Vdf(_) => VdfParseError::CODE,
            Self::Format(_) => "LOGINUSERS_FORMAT",
        }
    }
}

pub fn is_steam_id64(text: &str) -> bool {
    (15..=20).contains(&text.len()) && js::is_ascii_digits(text)
}

// The account id Steam keeps in ActiveProcess\ActiveUser for this SteamID64.
pub fn steam_id3(steam_id64: &str) -> Option<i128> {
    steam_id64
        .parse::<i128>()
        .ok()
        .map(|id| id - STEAMID64_BASE)
}

fn users_block(root: &VdfObject) -> Result<&VdfObject, LoginUsersError> {
    root.block("users")
        .ok_or_else(|| LoginUsersError::Format("loginusers.vdf has no \"users\" block".to_owned()))
}

// A block without a SteamID64 for a key or without an account name is left out, never guessed at.
pub fn parse_login_users(text: &str) -> Result<Vec<SteamLoginUser>, LoginUsersError> {
    let root = vdf::parse(text)?;
    Ok(read_users(&root)?
        .into_iter()
        .map(|(user, _)| user)
        .collect())
}

fn read_users(root: &VdfObject) -> Result<Vec<(SteamLoginUser, &VdfObject)>, LoginUsersError> {
    let mut out = Vec::new();
    for (id, entry) in users_block(root)?.entries() {
        let VdfValue::Block(entry) = entry else {
            continue;
        };
        if !is_steam_id64(&id) {
            continue;
        }
        let Some(account_name) = entry
            .string("AccountName")
            .map(js::trim)
            .filter(|name| !name.is_empty())
        else {
            continue;
        };
        let persona_name = entry
            .string("PersonaName")
            .map(js::trim)
            .unwrap_or_default();
        register_account_names([account_name, persona_name]);
        register_account_ids([id.as_str()]);
        let user = SteamLoginUser {
            steam_id64: id,
            account_name: account_name.to_owned(),
            persona_name: persona_name.to_owned(),
            remember_password: entry.flag("RememberPassword"),
            most_recent: MostRecentKey::ALL
                .iter()
                .any(|key| entry.flag(key.as_str())),
            timestamp: entry.int("Timestamp"),
            most_recent_keys: MostRecentKey::ALL
                .into_iter()
                .filter(|key| entry.string(key.as_str()).is_some())
                .collect(),
        };
        out.push((user, entry));
    }
    Ok(out)
}

// By the launcher's id when the account has one that Steam knows, else by login, whatever its case.
pub fn find_login_user<'a>(
    users: &'a [SteamLoginUser],
    launcher_uid: Option<&str>,
    login: &str,
) -> Option<&'a SteamLoginUser> {
    if let Some(uid) = launcher_uid.filter(|uid| !uid.is_empty())
        && let Some(by_id) = users.iter().find(|user| user.steam_id64 == uid)
    {
        return Some(by_id);
    }
    let login = js::trim(login).to_lowercase();
    users
        .iter()
        .find(|user| user.account_name.to_lowercase() == login)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineChange {
    // Counted from 1 in the file as it was; a line that is added names the line it goes in front of.
    pub line: usize,
    pub before: String,
    pub after: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct AutoLoginRewrite {
    pub text: String,
    pub changes: Vec<LineChange>,
    pub noop: bool,
}

static USER_KEY_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r#"^{WS}*"([0-9]{{15,20}})"{WS}*$"#)).expect("a fixed pattern")
});
static MOST_RECENT_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"^({WS}*)"((?i-u:AutoLogin|MostRecent))"({WS}+)"([^"]*)"({WS}*)$"#
    ))
    .expect("a fixed pattern")
});
static KEY_ONLY_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r#"^{WS}*"([^"]*)"{WS}*$"#)).expect("a fixed pattern"));
static ANY_KEY_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r#"^({WS}*)"[^"]*"({WS}+)"[^"]*"{WS}*$"#)).expect("a fixed pattern")
});

// The text of loginusers.vdf that makes `target` the account Steam signs in: every AutoLogin or MostRecent line becomes "1" in the target's block and "0" in every other, and a block with neither gets an AutoLogin line. It goes line by line, so every other byte of every line stays as it was.
pub fn plan_auto_login_rewrite(
    text: &str,
    target: &str,
) -> Result<AutoLoginRewrite, LoginUsersError> {
    let users = parse_login_users(text)?;
    if !users.iter().any(|user| user.steam_id64 == target) {
        return Err(LoginUsersError::Format(format!(
            "SteamID64 {target} is not in loginusers.vdf"
        )));
    }

    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut out: Vec<String> = Vec::new();
    let mut changes = Vec::new();
    let mut depth = 0_i64;
    // The reader takes the accounts from the top-level users block alone, so only its children are accounts here.
    let mut top_key: Option<String> = None;
    let mut in_users = false;
    let mut current_user: Option<String> = None;
    let mut saw_key_in_block = false;
    let mut indent = "\t\t".to_owned();
    let mut separator = "\t\t".to_owned();

    for (index, raw) in js::lines(text).enumerate() {
        let trimmed = js::trim(raw);
        if trimmed == "{" {
            if depth == 0 {
                in_users = top_key
                    .take()
                    .is_some_and(|key| key.eq_ignore_ascii_case("users"));
            }
            depth += 1;
            out.push(raw.to_owned());
            continue;
        }
        if trimmed == "}" {
            if depth == 2
                && let Some(user) = current_user.take()
                && !saw_key_in_block
            {
                let value = if user == target { "1" } else { "0" };
                let inserted = format!("{indent}\"AutoLogin\"{separator}\"{value}\"");
                changes.push(LineChange {
                    line: index + 1,
                    before: String::new(),
                    after: inserted.clone(),
                });
                out.push(inserted);
            }
            depth -= 1;
            out.push(raw.to_owned());
            continue;
        }
        if depth == 0 && !trimmed.is_empty() {
            top_key = KEY_ONLY_LINE.captures(raw).map(|found| found[1].to_owned());
        }
        if depth == 1 {
            if !in_users {
                current_user = None;
            } else if let Some(found) = USER_KEY_LINE.captures(raw) {
                current_user = Some(found[1].to_owned());
                saw_key_in_block = false;
            }
            out.push(raw.to_owned());
            continue;
        }
        if depth == 2
            && let Some(user) = &current_user
        {
            if let Some(found) = MOST_RECENT_LINE.captures(raw) {
                saw_key_in_block = true;
                let want = if user == target { "1" } else { "0" };
                if &found[4] != want {
                    let next = format!(
                        "{}\"{}\"{}\"{want}\"{}",
                        &found[1], &found[2], &found[3], &found[5]
                    );
                    changes.push(LineChange {
                        line: index + 1,
                        before: raw.to_owned(),
                        after: next.clone(),
                    });
                    out.push(next);
                    continue;
                }
            } else if let Some(found) = ANY_KEY_LINE.captures(raw) {
                indent = found[1].to_owned();
                separator = found[2].to_owned();
            }
        }
        out.push(raw.to_owned());
    }

    let noop = changes.is_empty();
    Ok(AutoLoginRewrite {
        text: out.join(eol),
        changes,
        noop,
    })
}

pub const UNWRITABLE_LAYOUT: &str = "loginusers.vdf is laid out in a way One Launch cannot rewrite. Start Steam and close it, so that it writes the file again, then try again.";

// Whether `text` reads as the rewrite promises: the target holds "1" in every AutoLogin or MostRecent it has, and at least one, while no other account holds "1" in either. The rewrite cannot say: on a layout the reader takes and the rewrite does not, such as a brace on the key's line or a comment after a flag, it leaves the line as it was, as the recorded cases hold.
pub fn marks_only(text: &str, target: &str) -> bool {
    let Ok(root) = vdf::parse(text) else {
        return false;
    };
    let Ok(users) = read_users(&root) else {
        return false;
    };
    users.iter().any(|(user, _)| user.steam_id64 == target)
        && users.iter().all(|(user, entry)| {
            if user.steam_id64 == target {
                !user.most_recent_keys.is_empty()
                    && user
                        .most_recent_keys
                        .iter()
                        .all(|key| entry.flag(key.as_str()))
            } else {
                !user.most_recent
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_account_shaped_key_outside_the_users_block_is_left_alone() {
        let text = "\"users\"\n{\n\t\"111111111111111\"\n\t{\n\t\t\"AccountName\"\t\t\"a\"\n\t\t\"AutoLogin\"\t\t\"0\"\n\t}\n}\n\"other\"\n{\n\t\"222222222222222\"\n\t{\n\t\t\"AutoLogin\"\t\t\"1\"\n\t}\n\t\"333333333333333\"\n\t{\n\t\t\"Name\"\t\t\"c\"\n\t}\n}\n";
        let rewrite = plan_auto_login_rewrite(text, "111111111111111").unwrap();
        assert_eq!(rewrite.changes.len(), 1);
        assert_eq!(
            rewrite.text,
            text.replacen("\"AutoLogin\"\t\t\"0\"", "\"AutoLogin\"\t\t\"1\"", 1)
        );
    }

    fn two_accounts(a: &str, b: &str) -> String {
        format!(
            "\"users\"\n{{\n\t\"111111111111111\"\n\t{{\n\t\t\"AccountName\"\t\t\"a\"\n{a}\t}}\n\t\"222222222222222\"\n\t{{\n\t\t\"AccountName\"\t\t\"b\"\n{b}\t}}\n}}\n"
        )
    }

    fn flags(auto_login: &str, most_recent: &str) -> String {
        format!("\t\t\"AutoLogin\"\t\t{auto_login}\n\t\t\"MostRecent\"\t\t{most_recent}\n")
    }

    fn guard_after_rewrite(text: &str) -> bool {
        let rewrite = plan_auto_login_rewrite(text, "222222222222222").unwrap();
        marks_only(&rewrite.text, "222222222222222")
    }

    #[test]
    fn a_flag_the_rewrite_leaves_on_either_side_fails_the_guard() {
        let off = flags("\"0\"", "\"0\"");
        for stuck in [flags("\"1\" // a", "\"0\""), flags("\"0\"", "\"1\" // a")] {
            assert!(!guard_after_rewrite(&two_accounts(&stuck, &off)));
        }
        for stuck in [flags("\"0\" // b", "\"0\""), flags("\"0\"", "\"0\" // b")] {
            assert!(!guard_after_rewrite(&two_accounts(&off, &stuck)));
        }
    }

    #[test]
    fn a_file_the_rewrite_handles_passes_the_guard_whatever_its_values() {
        let a = "\t\t\"autologin\"\t\t\" 1 \"\n\t\t\"MostRecent\"\t\t\"yes\"\n";
        let b = "\t\t\"AutoLogin\"\t\t\"\"\n";
        assert!(guard_after_rewrite(&two_accounts(a, b)));
        assert!(guard_after_rewrite(&two_accounts("", "")));
        assert!(guard_after_rewrite(&two_accounts(
            &flags("\"2\" // a", "\"0\""),
            b
        )));
    }
}
