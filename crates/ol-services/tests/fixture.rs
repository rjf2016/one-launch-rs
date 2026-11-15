#![forbid(unsafe_code)]

use ol_model::{LauncherId, SecretKey};
use ol_store::{Store, database_path};
use ol_testkit::{Scratch, oracle};
use ol_vault::{FakeVault, PROBE_VALUE, Vault};
use serde_json::Value;

#[test]
fn the_secrets_the_passwords_and_the_saved_sign_ins_of_the_fixture_decrypt() {
    let scratch = Scratch::with_copy_of("fixture", &oracle("fixtures/data-folder"));
    let (store, _) = Store::open(&database_path(scratch.path())).unwrap();
    let secret = |key| {
        let stored = store.secret(key).unwrap().unwrap();
        let opened = FakeVault.decrypt(&stored).unwrap();
        assert!(
            FakeVault.encrypt(&opened).unwrap() == stored,
            "{key:?} does not seal back to what is stored"
        );
        opened
    };
    assert!(secret(SecretKey::VaultProbe).expose() == PROBE_VALUE);
    assert!(secret(SecretKey::DiagnosticsRoundtrip).expose() == "roundtrip-ok");
    let api_key = secret(SecretKey::SteamGridDbApiKey);
    assert_eq!(api_key.expose().len(), 37);
    assert!(api_key.expose().starts_with("placeholder"));

    let accounts = store.accounts().unwrap();
    let mut passwords = 0;
    let mut sign_ins = Vec::new();
    for account in &accounts {
        if let Some(stored) = store.account_password(&account.id).unwrap() {
            let password = FakeVault.decrypt(&stored).unwrap();
            assert!(!password.expose().is_empty());
            assert_eq!(password.expose().len(), stored.as_bytes().len());
            passwords += 1;
        }
        if let Some(stored) = store.session_blob(&account.id).unwrap() {
            let blob = FakeVault.decrypt(&stored).unwrap();
            let blob: Value = serde_json::from_str(blob.expose()).unwrap();
            assert_eq!(blob["v"], 1);
            let part = match account.launcher {
                LauncherId::Epic => &blob["rememberMe"]["data"],
                LauncherId::Battlenet => &blob["unifiedAuth"]["hex"],
                LauncherId::Riot => &blob["file"],
                LauncherId::Steam => &Value::Null,
            };
            assert!(part.as_str().is_some_and(|text| !text.is_empty()));
            sign_ins.push(account.launcher);
        }
    }
    assert_eq!(passwords, 3);
    sign_ins.sort();
    assert_eq!(
        sign_ins,
        [LauncherId::Epic, LauncherId::Battlenet, LauncherId::Riot]
    );

    let all = store.ciphertexts().unwrap();
    let opened = all
        .passwords
        .iter()
        .chain(&all.sessions)
        .chain(all.secrets.iter().map(|(_, value)| value))
        .filter(|value| FakeVault.decrypt(value).is_ok())
        .count();
    assert_eq!(opened, 9);
}
