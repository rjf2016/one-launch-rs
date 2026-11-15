use std::fmt;

use ol_formats::js;
use ol_model::{Account, LauncherId};
use ol_store::{NewAccount as NewRow, Store, StoreError};
use ol_vault::{Secret, Vault, VaultError};
use thiserror::Error;
use uuid::Uuid;

const DISPLAY_NAME_MAX: usize = 120;
const LOGIN_MAX: usize = 256;
const LAUNCHER_UID_MAX: usize = 64;
const PASSWORD_MAX: usize = 1024;
const NOTES_MAX: usize = 20_000;
const TAG_COUNT_MAX: usize = 32;
const TAG_LENGTH_MAX: usize = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DuplicateField {
    Login,
    LauncherUid,
}

#[derive(Error)]
pub enum AccountError {
    #[error("Account {0} does not exist")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    // Two accounts of one launcher with the same login or the same launcher id cannot both be right, and a launcher's sign-in is told apart by exactly those.
    #[error("{}", duplicate_message(*.field, .display_name, *.launcher))]
    Duplicate {
        field: DuplicateField,
        display_name: String,
        launcher: LauncherId,
    },
    #[error(transparent)]
    Vault(#[from] VaultError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

fn duplicate_message(field: DuplicateField, display_name: &str, launcher: LauncherId) -> String {
    let launcher = launcher.display_name();
    match field {
        DuplicateField::Login => format!(
            "\"{display_name}\" already uses this login on {launcher}. Edit that account instead, or change its login first."
        ),
        DuplicateField::LauncherUid => format!(
            "\"{display_name}\" is already linked to this {launcher} account id. Edit that account instead, or clear its id first."
        ),
    }
}

impl AccountError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "NOT_FOUND",
            Self::Invalid(_) => "BAD_REQUEST",
            Self::Duplicate { .. } => "DUPLICATE",
            Self::Vault(error) => error.code(),
            Self::Store(error) => error.code(),
        }
    }
}

// The message can name an account, so only the code is printed.
impl fmt::Debug for AccountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AccountError({})", self.code())
    }
}

// No Debug: it names an account and can carry its password.
pub struct NewAccount {
    pub launcher: LauncherId,
    pub display_name: String,
    pub login: String,
    pub launcher_uid: Option<String>,
    pub password: Option<Secret>,
    pub tags: Vec<String>,
    pub notes: String,
}

fn invalid(message: String) -> AccountError {
    AccountError::Invalid(message)
}

fn require_text(value: &str, name: &str, max: usize) -> Result<String, AccountError> {
    let value = js::trim(value);
    if value.is_empty() {
        return Err(invalid(format!("{name} is required")));
    }
    if js::utf16_len(value) > max {
        return Err(invalid(format!("{name} must be at most {max} characters")));
    }
    Ok(value.to_owned())
}

fn optional_uid(value: Option<&str>) -> Result<Option<String>, AccountError> {
    let Some(value) = value.map(js::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if js::utf16_len(value) > LAUNCHER_UID_MAX {
        return Err(invalid(format!(
            "Launcher ID must be at most {LAUNCHER_UID_MAX} characters"
        )));
    }
    Ok(Some(value.to_owned()))
}

// Trimmed, cut to length, without blanks and without a second copy of a tag that differs only in case.
fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for raw in tags {
        let mut length = 0;
        let tag: String = js::trim(raw)
            .chars()
            .take_while(|c| {
                length += c.len_utf16();
                length <= TAG_LENGTH_MAX
            })
            .collect();
        if tag.is_empty() || !seen.insert(tag.to_lowercase()) {
            continue;
        }
        out.push(tag);
        if out.len() >= TAG_COUNT_MAX {
            break;
        }
    }
    out
}

fn check_unique(
    store: &Store,
    launcher: LauncherId,
    login: &str,
    launcher_uid: Option<&str>,
    own_id: Option<&str>,
) -> Result<(), AccountError> {
    let duplicate = |field, existing: Account| AccountError::Duplicate {
        field,
        display_name: existing.display_name,
        launcher: existing.launcher,
    };
    if let Some(by_login) = store.account_by_login(launcher, login)?
        && Some(by_login.id.as_str()) != own_id
    {
        return Err(duplicate(DuplicateField::Login, by_login));
    }
    if let Some(uid) = launcher_uid {
        let uid = uid.to_lowercase();
        let by_uid = store.accounts_of(launcher)?.into_iter().find(|account| {
            Some(account.id.as_str()) != own_id
                && account
                    .launcher_uid
                    .as_ref()
                    .is_some_and(|other| other.to_lowercase() == uid)
        });
        if let Some(by_uid) = by_uid {
            return Err(duplicate(DuplicateField::LauncherUid, by_uid));
        }
    }
    Ok(())
}

pub fn require(store: &Store, id: &str) -> Result<Account, AccountError> {
    store
        .account(id)?
        .ok_or_else(|| AccountError::NotFound(id.to_owned()))
}

// The password is encrypted before the database is touched, so a vault that fails leaves no half-made row.
pub fn create(
    store: &Store,
    vault: &dyn Vault,
    input: &NewAccount,
    now: &str,
) -> Result<Account, AccountError> {
    let display_name = require_text(&input.display_name, "Display name", DISPLAY_NAME_MAX)?;
    let login = require_text(&input.login, "Login", LOGIN_MAX)?;
    let launcher_uid = optional_uid(input.launcher_uid.as_deref())?;
    let tags = normalize_tags(&input.tags);
    if js::utf16_len(&input.notes) > NOTES_MAX {
        return Err(invalid(format!(
            "Notes must be at most {NOTES_MAX} characters"
        )));
    }
    let password = input
        .password
        .as_ref()
        .filter(|password| !password.expose().is_empty());
    if password.is_some_and(|password| js::utf16_len(password.expose()) > PASSWORD_MAX) {
        return Err(invalid(format!(
            "Password must be at most {PASSWORD_MAX} characters"
        )));
    }
    check_unique(store, input.launcher, &login, launcher_uid.as_deref(), None)?;
    let sealed = password
        .map(|password| vault.encrypt(password))
        .transpose()?;

    let id = Uuid::new_v4().to_string();
    store.insert_account(&NewRow {
        id: &id,
        launcher: input.launcher,
        display_name: &display_name,
        login: &login,
        password: sealed.as_ref(),
        launcher_uid: launcher_uid.as_deref(),
        tags: &tags,
        notes: &input.notes,
        now,
    })?;
    require(store, &id)
}

// Records the launcher's own id on an account. A second account with that id is refused, so nothing is ever linked under an id another account holds.
pub fn set_launcher_uid(
    store: &Store,
    id: &str,
    launcher_uid: Option<&str>,
    now: &str,
) -> Result<Account, AccountError> {
    let current = require(store, id)?;
    let launcher_uid = optional_uid(launcher_uid)?;
    check_unique(
        store,
        current.launcher,
        &current.login,
        launcher_uid.as_deref(),
        Some(id),
    )?;
    store.set_account_launcher_uid(id, launcher_uid.as_deref(), now)?;
    require(store, id)
}

// The order of a list to pick an account from: the one used last first, then those never used by name. Names compare by their lower-cased characters; the Electron app compares them by the rules of the system language.
pub fn in_pick_order(mut accounts: Vec<Account>) -> Vec<Account> {
    accounts.sort_by_cached_key(|account| {
        match account.last_used_at.clone().filter(|at| !at.is_empty()) {
            Some(used) => (false, Some(std::cmp::Reverse(used)), String::new()),
            None => (true, None, account.display_name.to_lowercase()),
        }
    });
    accounts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_are_trimmed_cut_and_kept_once() {
        let tags: Vec<String> = [" main ", "MAIN", "", "  ", "alt", &"x".repeat(50)]
            .iter()
            .map(|tag| (*tag).to_owned())
            .collect();
        assert_eq!(normalize_tags(&tags), ["main", "alt", &"x".repeat(40)]);
        let many: Vec<String> = (0..40).map(|index| format!("tag-{index}")).collect();
        assert_eq!(normalize_tags(&many).len(), 32);
    }

    #[test]
    fn a_name_is_required_and_has_a_length() {
        let failed = |value: &str| require_text(value, "Login", 4).err().map(|e| e.to_string());
        assert_eq!(failed("  "), Some("Login is required".to_owned()));
        assert_eq!(
            failed("abcde"),
            Some("Login must be at most 4 characters".to_owned())
        );
        assert_eq!(
            require_text(" abcd ", "Login", 4).ok().as_deref(),
            Some("abcd")
        );
        assert_eq!(optional_uid(Some("  ")).ok(), Some(None));
        assert_eq!(
            optional_uid(Some(&"1".repeat(65))).err().map(|e| e.code()),
            Some("BAD_REQUEST")
        );
    }
}
