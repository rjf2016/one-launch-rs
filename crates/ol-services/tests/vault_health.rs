#![forbid(unsafe_code)]

// The probe the vault's state is read from: none stored reads as nothing to test against, one is stored once and never written over.

use ol_model::{Ciphertext, SecretKey};
use ol_services::install::{self, UpdateStatus, UpdateUnavailable};
use ol_services::vault_health::{
    EncryptionState, VaultAccess, VaultResetSummary, clear_stored_secrets, ensure_probe, status,
};
use ol_store::{Store, database_path};
use ol_testkit::Scratch;
use ol_vault::{FakeVault, Secret, Vault};

const NOW: &str = "2026-11-21T10:00:00.000Z";

fn store(scratch: &Scratch) -> Store {
    Store::open(&database_path(scratch.path())).unwrap().0
}

#[test]
fn the_probe_is_stored_once_and_a_wrong_one_is_never_written_over() {
    let scratch = Scratch::new("services-vault");
    let store = store(&scratch);
    let open = VaultAccess::Open(&FakeVault);
    assert_eq!(status(&store, open).unwrap().readable, None);
    assert_eq!(
        status(&store, open).unwrap().state(),
        EncryptionState::Working
    );
    assert!(!ensure_probe(&store, VaultAccess::Unavailable, NOW).unwrap());
    assert!(!ensure_probe(&store, VaultAccess::KeyLost, NOW).unwrap());
    assert_eq!(store.secret(SecretKey::VaultProbe).unwrap(), None);

    assert!(ensure_probe(&store, open, NOW).unwrap());
    assert!(!ensure_probe(&store, open, NOW).unwrap());
    assert_eq!(status(&store, open).unwrap().readable, Some(true));

    let wrong = FakeVault
        .encrypt(&Secret::new("another text".to_owned()))
        .unwrap();
    store
        .put_secret(SecretKey::VaultProbe, &wrong, NOW)
        .unwrap();
    assert!(!ensure_probe(&store, open, NOW).unwrap());
    assert_eq!(store.secret(SecretKey::VaultProbe).unwrap(), Some(wrong));
    assert_eq!(
        status(&store, open).unwrap().state(),
        EncryptionState::KeyLost
    );

    store
        .put_secret(SecretKey::VaultProbe, &Ciphertext::new(vec![0xa5]), NOW)
        .unwrap();
    assert_eq!(status(&store, open).unwrap().readable, Some(false));
    assert_eq!(
        status(&store, VaultAccess::Unavailable).unwrap().state(),
        EncryptionState::Unavailable
    );
}

#[test]
fn a_reset_of_an_empty_folder_counts_nothing() {
    let scratch = Scratch::new("services-vault");
    let removed = clear_stored_secrets(&store(&scratch), NOW).unwrap();
    assert_eq!(removed, VaultResetSummary::default());
    assert_eq!(
        removed.note(),
        "Removed 0 passwords, 0 saved sign-ins, 0 other secrets."
    );
    let one = VaultResetSummary {
        passwords: 1,
        sessions: 1,
        secrets: 1,
    };
    assert_eq!(
        one.note(),
        "Removed 1 password, 1 saved sign-in, 1 other secret."
    );
}

#[test]
fn a_development_copy_says_that_updates_and_start_with_windows_are_off() {
    let status = install::update_status();
    assert_eq!(
        status,
        UpdateStatus::Unavailable(UpdateUnavailable::Development)
    );
    assert_eq!(status.state_text(), "Off for this copy");
    assert!(!status.can_check());
    assert_eq!(
        UpdateUnavailable::Development.hint(),
        "Updates work only in the installed app."
    );
    let autostart = install::autostart_status();
    assert!(!autostart.available && !autostart.open_at_login);
    let refused = install::set_autostart(true).unwrap_err();
    assert_eq!(refused.code(), "UNAVAILABLE");
    assert_eq!(
        refused.to_string(),
        "Start with Windows is not available for this copy of One Launch"
    );
    assert_eq!(install::build_label(), "Development");
}
