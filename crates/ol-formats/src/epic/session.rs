// Epic's remembered session: Enable and Data of [RememberMe] and Data of [Offline] in GameUserSettings.ini. The two Data values are opaque text the launcher signs in from, so whoever holds them is signed in: nothing here prints one, and what a user is shown of one is its length and, when it is long, the start of its SHA-256.

use ol_model::SwitchChange;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;
use zeroize::Zeroize;

use super::ini::{IniChange, read_value, rewrite_section};
use crate::js;

pub const REMEMBER_ME_SECTION: &str = "RememberMe";
pub const OFFLINE_SECTION: &str = "Offline";

// What One Launch keeps for an account. No Debug, and wiped when dropped.
#[derive(Clone, PartialEq, Eq)]
pub struct EpicSessionBlob {
    pub enable: String,
    pub data: String,
    pub offline: Option<String>,
}

impl Drop for EpicSessionBlob {
    fn drop(&mut self) {
        self.data.zeroize();
        self.offline.zeroize();
    }
}

// What the settings file holds now. No Debug: `data` and `offline_data` are the session.
#[derive(Clone, PartialEq, Eq)]
pub struct EpicSessionOnDisk {
    // Enable is True and Data is there: the launcher signs in on its own at its next start.
    pub remembered: bool,
    pub enable: Option<String>,
    pub data: Option<String>,
    pub offline_data: Option<String>,
    // SHA-256 of Data, in hex.
    pub fingerprint: Option<String>,
}

impl EpicSessionOnDisk {
    pub const fn absent() -> Self {
        Self {
            remembered: false,
            enable: None,
            data: None,
            offline_data: None,
            fingerprint: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
#[error("Stored Epic session has an unexpected shape")]
pub struct BlobError;

pub fn fingerprint_of(data: &str) -> String {
    Sha256::digest(data.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

// The first eight digits, for plans and the window.
pub fn short_fingerprint(fingerprint: Option<&str>) -> &str {
    match fingerprint {
        Some(fingerprint) if !fingerprint.is_empty() => fingerprint.get(..8).unwrap_or(fingerprint),
        _ => "(none)",
    }
}

pub fn read_session_from_ini(text: &str) -> EpicSessionOnDisk {
    let trimmed = |section: &str, key: &str| {
        read_value(text, section, key).map(|value| js::trim(&value).to_owned())
    };
    let filled = |value: Option<String>| value.filter(|value| !value.is_empty());
    let enable = trimmed(REMEMBER_ME_SECTION, "Enable");
    let data = filled(trimmed(REMEMBER_ME_SECTION, "Data"));
    EpicSessionOnDisk {
        remembered: data.is_some()
            && enable
                .as_deref()
                .is_some_and(|enable| enable.eq_ignore_ascii_case("true")),
        fingerprint: data.as_deref().map(fingerprint_of),
        offline_data: filled(trimmed(OFFLINE_SECTION, "Data")),
        enable,
        data,
    }
}

pub fn session_blob_from_ini(text: &str) -> Option<EpicSessionBlob> {
    let session = read_session_from_ini(text);
    if !session.remembered {
        return None;
    }
    Some(EpicSessionBlob {
        enable: session.enable.unwrap_or_else(|| "True".to_owned()),
        data: session.data?,
        offline: session.offline_data,
    })
}

fn wipe(value: &mut Value) {
    match value {
        Value::String(text) => text.zeroize(),
        Value::Array(items) => items.iter_mut().for_each(wipe),
        Value::Object(fields) => fields.values_mut().for_each(wipe),
        _ => {}
    }
}

// The JSON that is encrypted and stored, byte for byte the Electron app's. It is the session: the caller seals it at once.
pub fn serialize_blob(blob: &EpicSessionBlob) -> String {
    let mut shaped = json!({
        "v": 1,
        "rememberMe": { "enable": blob.enable, "data": blob.data },
        "offline": blob.offline.as_ref().map(|data| json!({ "data": data })),
    });
    let text = shaped.to_string();
    wipe(&mut shaped);
    text
}

pub fn parse_blob(json: &str) -> Result<EpicSessionBlob, BlobError> {
    let mut raw: Value = serde_json::from_str(json).map_err(|_| BlobError)?;
    let filled = |value: &Value| {
        value
            .as_str()
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    let remember = &raw["rememberMe"];
    let blob = match (raw["v"].as_f64(), filled(&remember["data"])) {
        (Some(1.0), Some(data)) => Ok(EpicSessionBlob {
            enable: remember["enable"].as_str().unwrap_or("True").to_owned(),
            data,
            offline: filled(&raw["offline"]["data"]),
        }),
        _ => Err(BlobError),
    };
    wipe(&mut raw);
    blob
}

const fn is_base64_character(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=')
}

// A Data value as a user may see it, never the value itself. A short one gets no fingerprint: trying every short text would find it from one.
fn describe(value: Option<&str>) -> String {
    let Some(value) = value else {
        return "(absent)".to_owned();
    };
    let length = js::utf16_len(value);
    if length >= 16 && value.chars().all(is_base64_character) {
        let fingerprint = fingerprint_of(value);
        format!(
            "({length} characters, fingerprint {})",
            short_fingerprint(Some(&fingerprint))
        )
    } else if length == 0 {
        String::new()
    } else {
        format!("({length} characters)")
    }
}

fn mask(section: &str, changes: &[IniChange]) -> Vec<SwitchChange> {
    changes
        .iter()
        .map(|change| {
            let is_data = change.key == "Data";
            SwitchChange {
                place: format!("[{section}] {}, line {}", change.key, change.line),
                before: match &change.before {
                    _ if is_data => describe(change.before.as_deref()),
                    Some(before) => before.clone(),
                    None => "(absent)".to_owned(),
                },
                after: match &change.after {
                    None => "(removed)".to_owned(),
                    Some(after) if is_data => describe(Some(after)),
                    Some(after) => after.clone(),
                },
            }
        })
        .collect()
}

pub struct SessionWrite {
    pub text: String,
    pub changes: Vec<SwitchChange>,
    pub noop: bool,
}

// GameUserSettings.ini with `blob` put in place, or with the launcher signed out when there is none: Data gone from both sections. Enable stays True either way: the launcher never turns it back on by itself, and without Data it opens on its sign-in page.
pub fn plan_session_write(text: &str, blob: Option<&EpicSessionBlob>) -> SessionWrite {
    let remember = rewrite_section(
        text,
        REMEMBER_ME_SECTION,
        &[
            ("Enable", Some("True")),
            ("Data", blob.map(|blob| blob.data.as_str())),
        ],
    );
    let offline = rewrite_section(
        &remember.text,
        OFFLINE_SECTION,
        &[("Data", blob.and_then(|blob| blob.offline.as_deref()))],
    );
    let mut changes = mask(REMEMBER_ME_SECTION, &remember.changes);
    changes.extend(mask(OFFLINE_SECTION, &offline.changes));
    SessionWrite {
        noop: offline.text == text,
        text: offline.text,
        changes,
    }
}
