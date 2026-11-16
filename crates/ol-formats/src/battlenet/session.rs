// Battle.net's remembered session: the REG_BINARY value under HKCU\Software\Blizzard Entertainment\Battle.net\UnifiedAuth, which the client writes again at every sign-in and deletes at a logout. Its bytes are sealed by Windows for this user, and whoever holds them is signed in all the same: nothing here prints them, and what a user is shown of them is their length and the start of their SHA-256.

use sha2::{Digest, Sha256};
use thiserror::Error;
use zeroize::Zeroize;

pub use crate::epic::session::short_fingerprint;
use crate::js_json::{JsObject, JsString, JsValue, parse, stringify};

pub const HKCU_KEY: &str = r"Software\Blizzard Entertainment\Battle.net";
pub const UNIFIED_AUTH_KEY: &str = r"Software\Blizzard Entertainment\Battle.net\UnifiedAuth";

// A value under UnifiedAuth: its name, and its bytes as lower-case hex digits. No Debug, and wiped when dropped.
#[derive(Clone, PartialEq, Eq)]
pub struct TokenValue {
    pub name: String,
    pub hex: String,
}

impl Drop for TokenValue {
    fn drop(&mut self) {
        self.hex.zeroize();
    }
}

// What One Launch keeps for an account.
#[derive(Clone, PartialEq, Eq)]
pub struct BattlenetSessionBlob {
    pub unified_auth: TokenValue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
#[error("Stored Battle.net session has an unexpected shape")]
pub struct BlobError;

// The SHA-256 of the hex digits in lower case, in hex.
pub fn fingerprint_of(hex: &str) -> String {
    Sha256::digest(hex.to_lowercase().as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

// Whole bytes as hex digits, and at least one.
pub fn is_token_hex(data: &str) -> bool {
    !data.is_empty()
        && data.len().is_multiple_of(2)
        && data.bytes().all(|byte| byte.is_ascii_hexdigit())
}

// The JSON that is encrypted and stored, byte for byte the Electron app's. It is the session: the caller seals it at once.
pub fn serialize_blob(blob: &BattlenetSessionBlob) -> String {
    let text = |text: &str| JsValue::String(JsString::from(text));
    let mut token = JsObject::default();
    token.insert(JsString::from("name"), text(&blob.unified_auth.name));
    token.insert(JsString::from("hex"), text(&blob.unified_auth.hex));
    let mut shaped = JsObject::default();
    shaped.insert(JsString::from("v"), JsValue::Number(1.0));
    shaped.insert(JsString::from("unifiedAuth"), JsValue::Object(token));
    let mut shaped = JsValue::Object(shaped);
    let written = stringify(&shaped, 0);
    shaped.wipe();
    written
}

fn read(raw: &JsValue) -> Option<BattlenetSessionBlob> {
    let root = raw.as_object()?;
    let version = root.get("v")?.as_number()?;
    let token = root.get("unifiedAuth")?.as_object()?;
    let name = String::from_utf16(token.get("name")?.as_string()?.units()).ok()?;
    let mut token = TokenValue {
        name,
        hex: token.get("hex")?.as_string()?.to_string_lossy(),
    };
    let shaped =
        (version == 1.0 || version == 2.0) && !token.name.is_empty() && is_token_hex(&token.hex);
    token.hex.make_ascii_lowercase();
    shaped.then_some(BattlenetSessionBlob {
        unified_auth: token,
    })
}

// Takes what the app stores and what a short-lived second version of it stored, which held more beside the token; the rest of that is dropped.
pub fn parse_blob(json: &str) -> Result<BattlenetSessionBlob, BlobError> {
    let mut raw = parse(json).map_err(|_| BlobError)?;
    let blob = read(&raw);
    raw.wipe();
    blob.ok_or(BlobError)
}

// A value under UnifiedAuth as a user may see it.
pub fn describe_token(value: Option<&TokenValue>) -> String {
    match value {
        None => "(absent)".to_owned(),
        Some(value) => format!(
            "({} bytes, fingerprint {})",
            value.hex.len() / 2,
            short_fingerprint(Some(&fingerprint_of(&value.hex)))
        ),
    }
}
