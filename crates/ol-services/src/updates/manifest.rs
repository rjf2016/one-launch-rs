use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use base64::Engine;
use base64::alphabet;
use base64::engine::general_purpose::{GeneralPurpose, PAD, STANDARD};
use ed25519_dalek::{Signature, VerifyingKey};
use ol_formats::latest_yml::Yaml;
use sha2::{Digest, Sha256, Sha512};
use thiserror::Error;

pub const MANIFEST_DOMAIN: &str = "one-launch-update-v1";

const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

const ED25519_SPKI_PREFIX: [u8; 12] = [
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
];

// Node's Buffer.from(text, 'base64') ignores the unused low bits of the last character, so the signature decodes as it does there.
const NODE_BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    PAD.with_decode_allow_trailing_bits(true),
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReleaseKey<'a> {
    pub id: &'a str,
    pub spki: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestFile {
    pub url: String,
    pub sha512: String,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verified {
    pub version: String,
    pub file: ManifestFile,
    pub key_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefusalReason {
    NoKeys,
    Unsupported,
    Malformed,
    Unsigned,
    UnknownKey,
    BadSignature,
}

impl RefusalReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoKeys => "no-keys",
            Self::Unsupported => "unsupported",
            Self::Malformed => "malformed",
            Self::Unsigned => "unsigned",
            Self::UnknownKey => "unknown-key",
            Self::BadSignature => "bad-signature",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
#[error("{detail}")]
pub struct Refusal {
    pub reason: RefusalReason,
    pub detail: String,
}

fn refuse(reason: RefusalReason, detail: impl Into<String>) -> Result<Verified, Refusal> {
    Err(Refusal {
        reason,
        detail: detail.into(),
    })
}

pub fn canonical_payload(version: &str, file: &ManifestFile) -> String {
    format!(
        "{MANIFEST_DOMAIN}\n{version}\n{}\n{}\n{}\n",
        file.url, file.sha512, file.size
    )
}

pub fn key_id_of(spki_der: &[u8]) -> String {
    Sha256::digest(spki_der)
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn ed25519_public_key(spki_der: &[u8]) -> Option<VerifyingKey> {
    let bytes: &[u8; 32] = spki_der
        .strip_prefix(&ED25519_SPKI_PREFIX)?
        .try_into()
        .ok()?;
    VerifyingKey::from_bytes(bytes).ok()
}

pub fn verify_manifest(manifest: &Yaml, keys: &[ReleaseKey<'_>]) -> Result<Verified, Refusal> {
    use RefusalReason::{BadSignature, Malformed, NoKeys, UnknownKey, Unsigned, Unsupported};

    if keys.is_empty() {
        return refuse(NoKeys, "this build trusts no release key");
    }
    if !is_object(manifest) {
        return refuse(Malformed, "the release manifest is not an object");
    }

    if manifest
        .get("packages")
        .is_some_and(|packages| *packages != Yaml::Null)
    {
        return refuse(Unsupported, "the release uses a web installer");
    }
    let entry = match manifest.get("files") {
        Some(Yaml::List(files)) if files.len() == 1 => &files[0],
        _ => return refuse(Unsupported, "the release must list exactly one file"),
    };
    if !is_object(entry) {
        return refuse(Malformed, "the release file entry is not an object");
    }
    if entry.get("isAdminRightsRequired") == Some(&Yaml::Bool(true)) {
        return refuse(Unsupported, "the release asks for administrator rights");
    }

    let Some(version) = text(manifest.get("version")).filter(|text| plain_version(text)) else {
        return refuse(
            Malformed,
            "the release version is not a plain version number",
        );
    };
    let Some(url) = text(entry.get("url")).filter(|text| plain_file_name(text)) else {
        return refuse(Malformed, "the release file is not a plain .exe name");
    };
    let Some(sha512) = text(entry.get("sha512")).filter(|text| base64_of_64_bytes(text)) else {
        return refuse(Malformed, "the release file has no SHA-512");
    };
    let Some(size) = entry.get("size").and_then(plain_size) else {
        return refuse(Malformed, "the release file has no size");
    };

    let signature = manifest.get("signature");
    let key_id = manifest.get("keyId");
    if signature.is_none() && key_id.is_none() {
        return refuse(Unsigned, "the release is not signed");
    }
    let Some(signature) = text(signature).filter(|text| base64_of_64_bytes(text)) else {
        return refuse(Malformed, "the release signature is malformed");
    };
    let Some(key_id) = text(key_id).filter(|text| plain_key_id(text)) else {
        return refuse(Malformed, "the release key id is malformed");
    };

    let Some(key) = keys.iter().find(|key| key.id == key_id) else {
        return refuse(
            UnknownKey,
            format!("the release is signed with a key this build does not trust ({key_id})"),
        );
    };

    let file = ManifestFile {
        url: url.to_owned(),
        sha512: sha512.to_owned(),
        size,
    };
    if !signature_matches(key, &canonical_payload(version, &file), signature) {
        return refuse(BadSignature, "the release signature does not match");
    }
    Ok(Verified {
        version: version.to_owned(),
        file,
        key_id: key_id.to_owned(),
    })
}

fn is_object(value: &Yaml) -> bool {
    matches!(value, Yaml::Map(_) | Yaml::List(_))
}

fn text(value: Option<&Yaml>) -> Option<&str> {
    match value {
        Some(Yaml::Text(text)) => Some(text),
        _ => None,
    }
}

fn digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

fn plain_version(text: &str) -> bool {
    let (core, pre) = match text.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (text, None),
    };
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| digits(part))
        && pre.is_none_or(|pre| {
            !pre.is_empty()
                && pre
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
        })
}

fn plain_file_name(text: &str) -> bool {
    text.strip_suffix(".exe").is_some_and(|stem| {
        stem.bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
            && stem.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'_' || byte == b'-'
            })
    })
}

fn base64_of_64_bytes(text: &str) -> bool {
    text.len() == 88
        && text.strip_suffix("==").is_some_and(|body| {
            body.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'/')
        })
}

fn plain_key_id(text: &str) -> bool {
    text.len() == 16
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn plain_size(value: &Yaml) -> Option<u64> {
    let Yaml::Number(written) = value else {
        return None;
    };
    if !digits(written) || written.starts_with('0') {
        return None;
    }
    written
        .parse::<u64>()
        .ok()
        .filter(|size| *size <= MAX_SAFE_INTEGER)
}

fn signature_matches(key: &ReleaseKey<'_>, payload: &str, signature: &str) -> bool {
    let Some(public) = STANDARD
        .decode(key.spki)
        .ok()
        .and_then(|der| ed25519_public_key(&der))
    else {
        return false;
    };
    let Some(bytes) = NODE_BASE64
        .decode(signature)
        .ok()
        .and_then(|bytes| <[u8; 64]>::try_from(bytes).ok())
    else {
        return false;
    };
    public
        .verify_strict(payload.as_bytes(), &Signature::from_bytes(&bytes))
        .is_ok()
}

pub fn sha512_of_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha512::new();
    let mut buffer = vec![0; 1 << 20];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => hash.update(&buffer[..read]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(STANDARD.encode(hash.finalize()))
}
