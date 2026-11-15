#![forbid(unsafe_code)]
#![cfg(windows)]

use std::fs;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ol_model::Ciphertext;
use ol_platform_win::dpapi::Dpapi;
use ol_testkit::{Scratch, snapshot};
use ol_vault::{KeyError, OsCryptVault, Secret, Vault, VaultError, local_state_path};

const TEXT: &str = "placeholder-pässwörd-日本語-🎮";
const DPAPI_BLOB_START: [u8; 20] = [
    0x01, 0x00, 0x00, 0x00, 0xd0, 0x8c, 0x9d, 0xdf, 0x01, 0x15, 0xd1, 0x11, 0x8c, 0x7a, 0x00, 0xc0,
    0x4f, 0xc2, 0x97, 0xeb,
];

fn wrapped_key(scratch: &Scratch) -> Vec<u8> {
    let text = fs::read(local_state_path(scratch.path())).unwrap();
    let state: serde_json::Value = serde_json::from_slice(&text).unwrap();
    STANDARD
        .decode(state["os_crypt"]["encrypted_key"].as_str().unwrap())
        .unwrap()
}

#[test]
fn a_value_sealed_under_a_key_made_here_opens_again_and_a_changed_one_does_not() {
    let scratch = Scratch::new("dpapi");
    let sealed = {
        let vault = OsCryptVault::open_or_create(scratch.path(), &Dpapi).unwrap();
        vault.encrypt(&Secret::new(TEXT.to_owned())).unwrap()
    };
    assert_eq!(sealed.as_bytes().len(), TEXT.len() + 31);
    assert!(sealed.as_bytes().starts_with(b"v10"));
    assert!(
        !sealed
            .as_bytes()
            .windows(11)
            .any(|part| part == b"placeholder"),
        "the text is in the sealed value"
    );

    let key = wrapped_key(&scratch);
    assert!(key.starts_with(b"DPAPI"));
    assert!(key[5..].starts_with(&DPAPI_BLOB_START), "not a DPAPI blob");
    let before = snapshot(scratch.path());
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        ["chromium/", "chromium/Local State"]
    );

    let vault = OsCryptVault::open(scratch.path(), &Dpapi).unwrap();
    assert!(vault.decrypt(&sealed).unwrap().expose() == TEXT);
    let again = OsCryptVault::open_or_create(scratch.path(), &Dpapi).unwrap();
    assert!(again.decrypt(&sealed).unwrap().expose() == TEXT);
    assert!(
        before == snapshot(scratch.path()),
        "the key was written again"
    );

    let last = sealed.as_bytes().len() - 1;
    for index in [0, 3, 15, last] {
        let mut changed = sealed.as_bytes().to_vec();
        changed[index] ^= 0x01;
        assert_eq!(
            vault.decrypt(&Ciphertext::new(changed)).err(),
            Some(VaultError::Unreadable),
            "byte {index}"
        );
    }
    assert_eq!(
        vault
            .decrypt(&Ciphertext::new(sealed.as_bytes()[..last].to_vec()))
            .err(),
        Some(VaultError::Unreadable)
    );

    let other = Scratch::new("dpapi");
    let another_key = OsCryptVault::open_or_create(other.path(), &Dpapi).unwrap();
    assert_ne!(wrapped_key(&other), key);
    assert_eq!(
        another_key.decrypt(&sealed).err(),
        Some(VaultError::Unreadable)
    );
}

#[test]
fn a_wrapped_key_that_was_changed_does_not_unwrap_and_is_not_replaced() {
    let scratch = Scratch::new("dpapi");
    OsCryptVault::open_or_create(scratch.path(), &Dpapi).unwrap();
    let mut key = wrapped_key(&scratch);
    let last = key.len() - 1;
    key[last] ^= 0x01;
    let state = format!(
        r#"{{"os_crypt":{{"encrypted_key":"{}"}}}}"#,
        STANDARD.encode(key)
    );
    fs::write(local_state_path(scratch.path()), state).unwrap();
    let before = snapshot(scratch.path());
    for opened in [
        OsCryptVault::open(scratch.path(), &Dpapi),
        OsCryptVault::open_or_create(scratch.path(), &Dpapi),
    ] {
        assert!(matches!(opened, Err(KeyError::Unwrap)));
    }
    assert!(before == snapshot(scratch.path()), "the folder changed");
}
