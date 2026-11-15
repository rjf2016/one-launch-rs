use std::fmt;
use std::io;
use std::path::Path;

use aes_gcm::{Aes256Gcm, KeyInit};
use ol_model::Ciphertext;
use ol_platform_api::DataProtector;
use thiserror::Error;
use zeroize::Zeroizing;

use crate::codec::{self, NONCE_BYTES};
use crate::local_state::{self, local_state_path};
use crate::{REDACTED, Secret, Vault, VaultError};

const KEY_BYTES: usize = 32;

#[derive(Debug, Error)]
pub enum KeyError {
    #[error("chromium\\Local State holds no vault key")]
    Missing,
    #[error("chromium\\Local State could not be read: {0}")]
    Read(#[source] io::Error),
    #[error("chromium\\Local State is not in the shape Chromium writes")]
    Malformed,
    #[error(
        "Windows did not unwrap the vault key: it belongs to another Windows user or another PC"
    )]
    Unwrap,
    #[error("the vault key is not 32 bytes long")]
    WrongLength,
    #[error("no random bytes could be had for a new vault key")]
    Random,
    #[error("Windows did not wrap the new vault key")]
    Wrap,
    #[error("chromium\\Local State could not be written: {0}")]
    Write(#[source] io::Error),
}

// The vault of the Electron app: Chromium's os_crypt. One AES-256 key, wrapped by DPAPI in chromium\Local State, seals every value as "v10", a nonce, the ciphertext and the tag.
pub struct OsCryptVault {
    // In a box: moving the vault then moves a pointer, and does not leave a copy of the key behind at every stop. The cipher wipes its own when it is dropped.
    cipher: Box<Aes256Gcm>,
}

impl OsCryptVault {
    pub fn open(data_dir: &Path, protector: &dyn DataProtector) -> Result<Self, KeyError> {
        let wrapped =
            local_state::read_key(&local_state_path(data_dir))?.ok_or(KeyError::Missing)?;
        Self::unwrap(&wrapped, protector)
    }

    pub fn open_or_create(
        data_dir: &Path,
        protector: &dyn DataProtector,
    ) -> Result<Self, KeyError> {
        let file = local_state_path(data_dir);
        if let Some(wrapped) = local_state::read_key(&file)? {
            return Self::unwrap(&wrapped, protector);
        }
        let mut key = Zeroizing::new([0_u8; KEY_BYTES]);
        getrandom::fill(key.as_mut_slice()).map_err(|_| KeyError::Random)?;
        let wrapped = protector
            .protect(key.as_slice())
            .map_err(|_| KeyError::Wrap)?;
        local_state::write_key(&file, &wrapped)?;
        Self::with_key(key.as_slice())
    }

    fn unwrap(wrapped: &[u8], protector: &dyn DataProtector) -> Result<Self, KeyError> {
        let key = protector.unprotect(wrapped).map_err(|_| KeyError::Unwrap)?;
        Self::with_key(&key)
    }

    fn with_key(key: &[u8]) -> Result<Self, KeyError> {
        Aes256Gcm::new_from_slice(key)
            .map(|cipher| Self {
                cipher: Box::new(cipher),
            })
            .map_err(|_| KeyError::WrongLength)
    }
}

impl Vault for OsCryptVault {
    fn encrypt(&self, plaintext: &Secret) -> Result<Ciphertext, VaultError> {
        let mut nonce = [0_u8; NONCE_BYTES];
        getrandom::fill(&mut nonce).map_err(|_| VaultError::Unavailable)?;
        codec::seal(&self.cipher, &nonce, plaintext.expose().as_bytes()).map(Ciphertext::new)
    }

    fn decrypt(&self, ciphertext: &Ciphertext) -> Result<Secret, VaultError> {
        let plaintext = codec::open(&self.cipher, ciphertext.as_bytes())?;
        Secret::from_utf8(plaintext).ok_or(VaultError::Unreadable)
    }
}

impl fmt::Debug for OsCryptVault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OsCryptVault({REDACTED})")
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use ol_platform_api::ProtectError;
    use ol_testkit::{Scratch, oracle, snapshot};
    use serde_json::Value;

    use super::*;

    const MASK: u8 = 0xa7;

    struct Reversible;

    impl DataProtector for Reversible {
        fn protect(&self, plain: &[u8]) -> Result<Vec<u8>, ProtectError> {
            Ok(plain.iter().rev().map(|byte| byte ^ MASK).collect())
        }

        fn unprotect(&self, protected: &[u8]) -> Result<Zeroizing<Vec<u8>>, ProtectError> {
            Ok(Zeroizing::new(
                protected.iter().rev().map(|byte| byte ^ MASK).collect(),
            ))
        }
    }

    struct AnotherUser;

    impl DataProtector for AnotherUser {
        fn protect(&self, _: &[u8]) -> Result<Vec<u8>, ProtectError> {
            Err(ProtectError::Protect)
        }

        fn unprotect(&self, _: &[u8]) -> Result<Zeroizing<Vec<u8>>, ProtectError> {
            Err(ProtectError::Unprotect)
        }
    }

    fn vectors() -> Value {
        let text = fs::read_to_string(oracle("takeover/vault/portable-v10.json")).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    fn bytes(value: &Value) -> Vec<u8> {
        STANDARD.decode(value.as_str().unwrap()).unwrap()
    }

    fn vault(vectors: &Value, key: &str) -> OsCryptVault {
        OsCryptVault::with_key(&bytes(&vectors[key]["base64"])).unwrap()
    }

    fn secret(text: &str) -> Secret {
        Secret::new(text.to_owned())
    }

    fn write_local_state(data_dir: &Path, text: &str) {
        let file = local_state_path(data_dir);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }

    #[test]
    fn every_recorded_value_opens_and_seals_to_the_same_bytes() {
        let vectors = vectors();
        let vault = vault(&vectors, "key");
        let valid = vectors["valid"].as_array().unwrap();
        assert_eq!(valid.len(), 5);
        for vector in valid {
            let name = vector["name"].as_str().unwrap();
            let plaintext = vector["plaintext"].as_str().unwrap();
            let blob = bytes(&vector["blob"]);
            assert_eq!(blob.len() as u64, vector["blobBytes"].as_u64().unwrap());
            assert_eq!(blob.len(), plaintext.len() + 31, "{name}");

            let opened = vault.decrypt(&Ciphertext::new(blob.clone())).unwrap();
            assert!(opened.expose() == plaintext, "{name} opens to another text");

            let nonce: [u8; NONCE_BYTES] = bytes(&vector["nonce"]).try_into().unwrap();
            let sealed = codec::seal(&vault.cipher, &nonce, plaintext.as_bytes()).unwrap();
            assert!(sealed == blob, "{name} seals to other bytes");
        }
    }

    #[test]
    fn every_recorded_bad_value_is_refused() {
        let vectors = vectors();
        let failures = vectors["failures"].as_array().unwrap();
        assert_eq!(failures.len(), 12);
        for failure in failures {
            let name = failure["name"].as_str().unwrap();
            let vault = vault(&vectors, failure["key"].as_str().unwrap());
            let refused = vault.decrypt(&Ciphertext::new(bytes(&failure["blob"])));
            assert_eq!(refused.err(), Some(VaultError::Unreadable), "{name}");
        }
    }

    #[test]
    fn a_sealed_value_has_a_nonce_of_its_own_and_opens_again() {
        let vectors = vectors();
        let vault = vault(&vectors, "key");
        for text in ["", "placeholder-password-1", "plätzhalter-日本語-🎮"] {
            let first = vault.encrypt(&secret(text)).unwrap();
            let second = vault.encrypt(&secret(text)).unwrap();
            assert_eq!(first.as_bytes().len(), text.len() + 31);
            assert!(first.as_bytes().starts_with(b"v10"));
            assert!(first != second, "two values share a nonce");
            assert!(vault.decrypt(&first).unwrap().expose() == text);
            assert!(vault.decrypt(&second).unwrap().expose() == text);
        }
    }

    #[test]
    fn a_changed_byte_anywhere_is_refused() {
        let vectors = vectors();
        let vault = vault(&vectors, "key");
        let sealed = vault.encrypt(&secret("placeholder")).unwrap().into_bytes();
        for index in 0..sealed.len() {
            let mut changed = sealed.clone();
            changed[index] ^= 0x01;
            let refused = vault.decrypt(&Ciphertext::new(changed));
            assert_eq!(refused.err(), Some(VaultError::Unreadable), "byte {index}");
        }
        for length in 0..sealed.len() {
            let refused = vault.decrypt(&Ciphertext::new(sealed[..length].to_vec()));
            assert_eq!(
                refused.err(),
                Some(VaultError::Unreadable),
                "{length} bytes"
            );
        }
        let other = self::vault(&vectors, "otherKey");
        assert_eq!(
            other.decrypt(&Ciphertext::new(sealed)).err(),
            Some(VaultError::Unreadable)
        );
    }

    #[test]
    fn a_sealed_text_that_is_not_utf8_is_refused() {
        let vectors = vectors();
        let vault = vault(&vectors, "key");
        let sealed = codec::seal(&vault.cipher, &[7; NONCE_BYTES], &[0xff, 0xfe, 0xfd]).unwrap();
        assert_eq!(
            vault.decrypt(&Ciphertext::new(sealed)).err(),
            Some(VaultError::Unreadable)
        );
    }

    #[test]
    fn the_key_is_read_from_local_state_and_nothing_is_written() {
        let vectors = vectors();
        let key = bytes(&vectors["key"]["base64"]);
        let wrapped = [b"DPAPI".as_slice(), &Reversible.protect(&key).unwrap()].concat();
        let scratch = Scratch::new("vault");
        write_local_state(
            scratch.path(),
            &format!(
                r#"{{"browser":{{"enabled_labs_experiments":[]}},"os_crypt":{{"audit_enabled":true,"encrypted_key":"{}"}}}}"#,
                STANDARD.encode(wrapped)
            ),
        );
        let before = snapshot(scratch.path());

        let vault = OsCryptVault::open(scratch.path(), &Reversible).unwrap();
        let probe = &vectors["valid"][3];
        assert_eq!(probe["name"], "vault-probe");
        let opened = vault
            .decrypt(&Ciphertext::new(bytes(&probe["blob"])))
            .unwrap();
        assert!(opened.expose() == crate::PROBE_VALUE);
        OsCryptVault::open_or_create(scratch.path(), &Reversible).unwrap();
        assert!(before == snapshot(scratch.path()), "the folder changed");

        let refused = OsCryptVault::open(scratch.path(), &AnotherUser);
        assert!(matches!(refused, Err(KeyError::Unwrap)));
        let refused = OsCryptVault::open_or_create(scratch.path(), &AnotherUser);
        assert!(matches!(refused, Err(KeyError::Unwrap)));
        assert!(
            before == snapshot(scratch.path()),
            "a key that does not unwrap was replaced"
        );
    }

    #[test]
    fn a_folder_without_a_key_gets_one_only_when_asked() {
        let scratch = Scratch::new("vault");
        assert!(matches!(
            OsCryptVault::open(scratch.path(), &Reversible),
            Err(KeyError::Missing)
        ));
        assert!(snapshot(scratch.path()).is_empty());

        let first = OsCryptVault::open_or_create(scratch.path(), &Reversible).unwrap();
        let files = snapshot(scratch.path());
        assert_eq!(
            files.keys().collect::<Vec<_>>(),
            ["chromium/", "chromium/Local State"]
        );
        let state: Value = serde_json::from_slice(&files["chromium/Local State"]).unwrap();
        let wrapped = bytes(&state["os_crypt"]["encrypted_key"]);
        assert_eq!(wrapped.len(), 5 + KEY_BYTES);
        assert!(wrapped.starts_with(b"DPAPI"));

        let sealed = first.encrypt(&secret("placeholder")).unwrap();
        let again = OsCryptVault::open(scratch.path(), &Reversible).unwrap();
        assert!(again.decrypt(&sealed).unwrap().expose() == "placeholder");
        let other = Scratch::new("vault");
        let elsewhere = OsCryptVault::open_or_create(other.path(), &Reversible).unwrap();
        assert_eq!(
            elsewhere.decrypt(&sealed).err(),
            Some(VaultError::Unreadable),
            "two folders got the same key"
        );
    }

    #[test]
    fn a_new_key_goes_in_beside_what_chromium_keeps_there() {
        let scratch = Scratch::new("vault");
        write_local_state(
            scratch.path(),
            r#"{"zeta":{"b":1,"a":[1.5,"x"]},"os_crypt":{"audit_enabled":true},"alpha":null}"#,
        );
        OsCryptVault::open_or_create(scratch.path(), &Reversible).unwrap();
        let text = fs::read_to_string(local_state_path(scratch.path())).unwrap();
        let (kept, key) = text.split_once(r#","encrypted_key":""#).unwrap();
        assert_eq!(
            kept,
            r#"{"zeta":{"b":1,"a":[1.5,"x"]},"os_crypt":{"audit_enabled":true"#
        );
        assert!(key.ends_with(r#""},"alpha":null}"#));
        OsCryptVault::open(scratch.path(), &Reversible).unwrap();
        assert_eq!(
            snapshot(scratch.path()).len(),
            2,
            "no unfinished file is left"
        );
    }

    #[test]
    fn a_local_state_in_another_shape_is_refused_and_left_alone() {
        let wrapped = STANDARD.encode([b"DPAPI".as_slice(), &[1; 40]].concat());
        let short = STANDARD.encode([b"DPAPI".as_slice(), &[1; 31]].concat());
        let cases = [
            ("", KeyShape::Malformed),
            ("not json", KeyShape::Malformed),
            ("[]", KeyShape::Malformed),
            (r#"{"os_crypt":7}"#, KeyShape::Malformed),
            (r#"{"os_crypt":{"encrypted_key":7}}"#, KeyShape::Malformed),
            (
                r#"{"os_crypt":{"encrypted_key":"***"}}"#,
                KeyShape::Malformed,
            ),
            (r#"{"os_crypt":{"encrypted_key":""}}"#, KeyShape::Malformed),
            (
                r#"{"os_crypt":{"encrypted_key":"RFBBUEk="}}"#,
                KeyShape::Malformed,
            ),
            (
                r#"{"os_crypt":{"encrypted_key":"djEwAAAAAAAA"}}"#,
                KeyShape::Malformed,
            ),
            (
                &format!(r#"{{"os_crypt":{{"encrypted_key":"{short}"}}}}"#),
                KeyShape::WrongLength,
            ),
            (
                &format!(r#"{{"os_crypt":{{"encrypted_key":"{wrapped}"}}}}"#),
                KeyShape::WrongLength,
            ),
        ];
        for (text, expected) in cases {
            let scratch = Scratch::new("vault");
            write_local_state(scratch.path(), text);
            let before = snapshot(scratch.path());
            for opened in [
                OsCryptVault::open(scratch.path(), &Reversible),
                OsCryptVault::open_or_create(scratch.path(), &Reversible),
            ] {
                let found = match opened {
                    Err(KeyError::Malformed) => KeyShape::Malformed,
                    Err(KeyError::WrongLength) => KeyShape::WrongLength,
                    other => panic!("{text}: {:?}", other.map(|_| ())),
                };
                assert_eq!(found, expected, "{text}");
            }
            assert!(
                before == snapshot(scratch.path()),
                "{text}: the folder changed"
            );
        }
    }

    #[derive(Debug, PartialEq)]
    enum KeyShape {
        Malformed,
        WrongLength,
    }

    #[test]
    fn a_key_that_cannot_be_wrapped_leaves_no_file() {
        let scratch = Scratch::new("vault");
        let refused = OsCryptVault::open_or_create(scratch.path(), &AnotherUser);
        assert!(matches!(refused, Err(KeyError::Wrap)));
        assert!(snapshot(scratch.path()).is_empty());
    }

    #[test]
    fn debug_and_errors_show_nothing_of_the_key() {
        let vectors = vectors();
        let vault = vault(&vectors, "key");
        assert_eq!(format!("{vault:?}"), "OsCryptVault([REDACTED])");
        assert_eq!(VaultError::Unreadable.code(), "VAULT_UNREADABLE");
        assert_eq!(VaultError::Unavailable.code(), "VAULT_UNAVAILABLE");
        assert_eq!(
            VaultError::Unavailable.to_string(),
            "Secure storage is not available on this system. Secrets cannot be stored."
        );
        let recorded = vectors["failures"][0]["electron"]["app"]["message"]
            .as_str()
            .unwrap();
        assert_eq!(VaultError::Unreadable.to_string(), recorded);
    }

    #[test]
    fn the_cipher_wipes_its_key_when_it_is_dropped() {
        fn wipes_on_drop<T: zeroize::ZeroizeOnDrop>() {}
        wipes_on_drop::<Aes256Gcm>();
    }

    #[test]
    fn a_key_file_that_cannot_be_read_or_written_is_reported_without_the_folder() {
        let scratch = Scratch::new("vault");
        fs::create_dir_all(local_state_path(scratch.path())).unwrap();
        let before = snapshot(scratch.path());
        let folder = scratch.path().to_str().unwrap();
        for opened in [
            OsCryptVault::open(scratch.path(), &Reversible),
            OsCryptVault::open_or_create(scratch.path(), &Reversible),
        ] {
            let error = opened.err().unwrap();
            assert!(matches!(error, KeyError::Read(_)), "{error:?}");
            let shown = format!("{error} {error:?}");
            assert!(shown.starts_with("chromium\\Local State could not be read: "));
            assert!(!shown.contains(folder), "the folder is in the error");
        }
        assert!(before == snapshot(scratch.path()), "the folder changed");

        let blocked = Scratch::new("vault");
        fs::write(blocked.join("chromium"), b"a file where the folder goes").unwrap();
        let error = OsCryptVault::open_or_create(blocked.path(), &Reversible)
            .err()
            .unwrap();
        assert!(matches!(error, KeyError::Write(_)), "{error:?}");
        let shown = format!("{error} {error:?}");
        assert!(shown.starts_with("chromium\\Local State could not be written: "));
        assert!(!shown.contains(blocked.path().to_str().unwrap()));
        assert_eq!(
            snapshot(blocked.path()).len(),
            1,
            "something was left behind"
        );
    }
}
