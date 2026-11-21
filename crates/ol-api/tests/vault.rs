#![forbid(unsafe_code)]

// The Encryption panel: the vault's states, its self-test, and a reset that removes exactly what the Electron app's removes, on the fixtures' stand-in vault and on a data folder's own vault under a protector of the test's.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};

use futures_lite::future::block_on;
use ol_api::accounts::{NewAccount, Secret};
use ol_api::launchers::{Launchers, SteamTiming};
use ol_api::settings::{EncryptionState, VaultBackend, VaultHealth, VaultResetSummary};
use ol_api::{ApiError, Backend, StartLaunch};
use ol_model::{Account, Ciphertext, JobState, LaunchJob, LauncherId, SecretKey};
use ol_platform_api::{DataProtector, ProtectError, Zeroizing};
use ol_sim::FakeLaunchers;
use ol_store::{Store, database_path};
use ol_testkit::{Scratch, oracle, snapshot};
use ol_vault::{FakeVault, OsCryptVault, PROBE_VALUE, Vault, local_state_path};
use time::macros::datetime;

const VERSION: &str = "9.8.7";
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

// Another Windows user's DPAPI: it unwraps what it wrapped itself and nothing wrapped for the user the data folder came from.
struct AnotherUser;

const OWN_MARK: &[u8] = b"another-user";

impl DataProtector for AnotherUser {
    fn protect(&self, plain: &[u8]) -> Result<Vec<u8>, ProtectError> {
        Ok([OWN_MARK, plain].concat())
    }

    fn unprotect(&self, protected: &[u8]) -> Result<Zeroizing<Vec<u8>>, ProtectError> {
        protected
            .strip_prefix(OWN_MARK)
            .map(|plain| Zeroizing::new(plain.to_vec()))
            .ok_or(ProtectError::Unprotect)
    }
}

// Every account row as the database holds it, its sealed password among the columns; the saved sign-ins; the named secrets.
struct Rows {
    accounts: BTreeMap<String, (Account, Option<Vec<u8>>)>,
    sessions: u64,
    secrets: BTreeMap<String, Vec<u8>>,
    settings: Vec<(String, String)>,
    games: u64,
    history: u64,
}

fn rows(data_dir: &Path) -> Rows {
    let store = Store::open_read_only(&database_path(data_dir))
        .unwrap()
        .store;
    let accounts = store
        .accounts()
        .unwrap()
        .into_iter()
        .map(|account| {
            let sealed = store
                .account_password(&account.id)
                .unwrap()
                .map(Ciphertext::into_bytes);
            (account.id.clone(), (account, sealed))
        })
        .collect();
    let counts = store.row_counts().unwrap();
    Rows {
        accounts,
        sessions: counts.launcher_sessions,
        secrets: store
            .ciphertexts()
            .unwrap()
            .secrets
            .into_iter()
            .map(|(key, value)| (key, value.into_bytes()))
            .collect(),
        settings: store.settings_rows().unwrap(),
        games: counts.games,
        history: counts.launch_history,
    }
}

fn refusal<T>(answer: Result<T, ApiError>) -> (&'static str, String) {
    let error = answer.err().expect("a refusal");
    (error.code, error.message)
}

// What a reset leaves: each account as it was but for its password and, when it had one, the time of the change; no saved sign-in; one named secret, a new probe that opens with `vault`; every other row and every file under backups as they were.
fn assert_reset(
    before: &Rows,
    after: &Rows,
    backups: &BTreeMap<String, Vec<u8>>,
    data_dir: &Path,
    vault: &dyn Vault,
) {
    assert_eq!(after.accounts.len(), before.accounts.len());
    for (id, (account, sealed)) in &before.accounts {
        let (now, sealed_now) = &after.accounts[id];
        assert_eq!(*sealed_now, None, "{id} keeps a password");
        assert!(!now.has_secret);
        if sealed.is_some() {
            assert_ne!(now.updated_at, account.updated_at, "{id}");
        } else {
            assert_eq!(now.updated_at, account.updated_at, "{id}");
        }
        let mut was = account.clone();
        was.has_secret = false;
        was.has_session = false;
        was.updated_at.clone_from(&now.updated_at);
        assert!(*now == was, "{id} changed beyond its password");
    }
    assert_eq!(after.sessions, 0);
    assert_eq!(
        after.secrets.keys().collect::<Vec<_>>(),
        [SecretKey::VaultProbe.as_str()]
    );
    let probe = Ciphertext::new(after.secrets[SecretKey::VaultProbe.as_str()].clone());
    assert!(vault.decrypt(&probe).unwrap().expose() == PROBE_VALUE);
    assert_eq!(after.settings, before.settings);
    assert_eq!((after.games, after.history), (before.games, before.history));
    assert!(
        snapshot(&data_dir.join("backups")) == *backups,
        "a file under backups changed"
    );
}

#[test]
fn a_reset_on_the_stand_in_vault_removes_what_the_electron_app_removes_and_nothing_else() {
    let scratch = Scratch::with_copy_of("api-vault", &oracle("fixtures/data-folder"));
    let data_dir = scratch.path().to_path_buf();
    let before = rows(&data_dir);
    let with_password = before
        .accounts
        .values()
        .filter(|(_, sealed)| sealed.is_some())
        .count();
    assert_eq!(
        (with_password, before.sessions, before.secrets.len()),
        (3, 3, 3)
    );
    let backups = snapshot(&data_dir.join("backups"));
    let chromium = snapshot(&data_dir.join("chromium"));

    let platform = fake_machine(&scratch).platform;
    let backend = Backend::start_with(
        data_dir.clone(),
        VERSION,
        Launchers::on(platform, Arc::new(FakeVault)),
        None,
    );
    assert_eq!(
        block_on(backend.vault_status()).unwrap(),
        VaultHealth {
            available: true,
            backend: VaultBackend::Dpapi,
            readable: Some(true),
        }
    );
    let removed = block_on(backend.reset_vault()).unwrap();
    assert_eq!(
        removed,
        VaultResetSummary {
            passwords: 3,
            sessions: 3,
            secrets: 3,
        }
    );
    assert_eq!(
        removed.note(),
        "Removed 3 passwords, 3 saved sign-ins, 3 other secrets."
    );
    let status = block_on(backend.vault_status()).unwrap();
    assert_eq!(status.state(), EncryptionState::Working);
    backend.shut_down();

    assert_reset(&before, &rows(&data_dir), &backups, &data_dir, &FakeVault);
    assert!(
        snapshot(&data_dir.join("chromium")) == chromium,
        "chromium changed"
    );
}

// The all-four scenario laid down in the scratch folder, for a backend with launchers on the data folder there.
fn fake_machine(scratch: &Scratch) -> FakeLaunchers {
    let launchers_dir = scratch.join("launchers");
    ol_sim::lay_down(
        &oracle("fixtures/dev-only/fake-launchers/all-four"),
        &oracle("fixtures/launchers"),
        &launchers_dir,
    )
    .unwrap();
    ol_sim::materialize(
        &launchers_dir,
        scratch.path(),
        datetime!(2026-11-06 16:46:23 UTC),
    )
    .unwrap()
}

fn new_account(login: &str, password: &str) -> NewAccount {
    NewAccount {
        launcher: LauncherId::Epic,
        display_name: login.to_owned(),
        login: login.to_owned(),
        launcher_uid: None,
        password: Some(Secret::new(password.to_owned())),
        tags: Vec::new(),
        notes: String::new(),
    }
}

#[test]
fn a_reset_on_a_data_folders_own_vault_keeps_a_key_that_unwraps() {
    let scratch = Scratch::new("api-vault");
    let data_dir = scratch.join("data");
    let backend =
        Backend::start_with_vault_of_data_folder(data_dir.clone(), VERSION, Box::new(Reversible));
    for (login, password) in [
        ("one@example.com", "placeholder-1"),
        ("two@example.com", "placeholder-2"),
    ] {
        block_on(backend.create_account(new_account(login, password))).unwrap();
    }
    let mut plain = new_account("three@example.com", "unused");
    plain.password = None;
    block_on(backend.create_account(plain)).unwrap();
    block_on(backend.test_vault()).unwrap();
    backend.shut_down();
    let key = fs::read(local_state_path(&data_dir)).unwrap();
    let before = rows(&data_dir);
    assert_eq!(before.secrets.keys().collect::<Vec<_>>(), ["vault.probe"]);
    let backups = snapshot(&data_dir.join("backups"));

    let backend =
        Backend::start_with_vault_of_data_folder(data_dir.clone(), VERSION, Box::new(Reversible));
    let status = block_on(backend.vault_status()).unwrap();
    assert_eq!((status.available, status.readable), (true, Some(true)));
    let removed = block_on(backend.reset_vault()).unwrap();
    assert_eq!(
        removed,
        VaultResetSummary {
            passwords: 2,
            sessions: 0,
            secrets: 1,
        }
    );
    assert_eq!(
        removed.note(),
        "Removed 2 passwords, 0 saved sign-ins, 1 other secret."
    );
    backend.shut_down();

    assert!(
        fs::read(local_state_path(&data_dir)).unwrap() == key,
        "the key was replaced"
    );
    let vault = OsCryptVault::open(&data_dir, &Reversible).unwrap();
    assert_reset(&before, &rows(&data_dir), &backups, &data_dir, &vault);
}

#[test]
fn a_key_that_does_not_unwrap_is_replaced_by_a_reset_and_by_nothing_else() {
    let scratch = Scratch::new("api-vault");
    let data_dir = scratch.join("data");
    let backend =
        Backend::start_with_vault_of_data_folder(data_dir.clone(), VERSION, Box::new(Reversible));
    let account =
        block_on(backend.create_account(new_account("one@example.com", "placeholder-1"))).unwrap();
    backend.shut_down();
    let key = fs::read(local_state_path(&data_dir)).unwrap();
    let old = OsCryptVault::open(&data_dir, &Reversible).unwrap();
    let before = rows(&data_dir);
    let backups = snapshot(&data_dir.join("backups"));

    let backend =
        Backend::start_with_vault_of_data_folder(data_dir.clone(), VERSION, Box::new(AnotherUser));
    let status = block_on(backend.vault_status()).unwrap();
    assert_eq!(
        status,
        VaultHealth {
            available: true,
            backend: VaultBackend::Dpapi,
            readable: Some(false),
        }
    );
    assert_eq!(status.state(), EncryptionState::KeyLost);
    assert!(status.shows_reset());
    assert_eq!(
        refusal(block_on(backend.set_account_password(
            &account.id,
            Secret::new("placeholder-2".to_owned())
        )))
        .0,
        "VAULT_UNAVAILABLE"
    );
    assert_eq!(
        refusal(block_on(backend.test_vault())).0,
        "VAULT_UNREADABLE"
    );
    backend.shut_down();

    let backend =
        Backend::start_with_vault_of_data_folder(data_dir.clone(), VERSION, Box::new(AnotherUser));
    block_on(backend.accounts()).unwrap();
    backend.shut_down();
    assert!(
        fs::read(local_state_path(&data_dir)).unwrap() == key,
        "a start replaced the key"
    );
    assert_eq!(rows(&data_dir).secrets, before.secrets);

    let backend =
        Backend::start_with_vault_of_data_folder(data_dir.clone(), VERSION, Box::new(AnotherUser));
    let removed = block_on(backend.reset_vault()).unwrap();
    assert_eq!(
        removed,
        VaultResetSummary {
            passwords: 1,
            sessions: 0,
            secrets: 1,
        }
    );
    assert_eq!(
        block_on(backend.vault_status()).unwrap().state(),
        EncryptionState::Working
    );
    let stored = block_on(
        backend.set_account_password(&account.id, Secret::new("placeholder-2".to_owned())),
    )
    .unwrap();
    assert!(stored.has_secret);
    block_on(backend.clear_account_password(&account.id)).unwrap();
    block_on(backend.test_vault()).unwrap();
    backend.shut_down();

    let replaced = fs::read(local_state_path(&data_dir)).unwrap();
    assert!(
        replaced != key,
        "the key that does not unwrap is still there"
    );
    let new = OsCryptVault::replace_lost_key(&data_dir, &AnotherUser).unwrap();
    assert!(
        fs::read(local_state_path(&data_dir)).unwrap() == replaced,
        "the new key was replaced"
    );
    let after = rows(&data_dir);
    let probe = Ciphertext::new(after.secrets["vault.probe"].clone());
    assert!(
        old.decrypt(&probe).is_err(),
        "the probe is sealed with the old key"
    );
    assert_reset(&before, &after, &backups, &data_dir, &new);
}

#[test]
fn every_state_of_the_vault_is_reported_as_the_encryption_panel_reads_it() {
    let scratch = Scratch::new("api-vault");
    let backend = Backend::start(scratch.join("none"), VERSION);
    let status = block_on(backend.vault_status()).unwrap();
    assert_eq!(
        status,
        VaultHealth {
            available: false,
            backend: VaultBackend::None,
            readable: None,
        }
    );
    assert_eq!(status.state(), EncryptionState::Unavailable);
    assert_eq!(
        status.state().label(),
        "Unavailable · passwords and sign-ins cannot be stored"
    );
    assert_eq!(
        refusal(block_on(backend.test_vault())),
        (
            "VAULT_UNAVAILABLE",
            "Secure storage is not available on this system. Secrets cannot be stored.".to_owned()
        )
    );
    backend.shut_down();

    // The fixture's probe was sealed by the stand-in, which a key of this folder's own does not open: the key it was sealed with is lost.
    let copied = Scratch::with_copy_of("api-vault", &oracle("fixtures/data-folder"));
    fs::remove_dir_all(copied.join("chromium")).unwrap();
    let backend = Backend::start_with_vault_of_data_folder(
        copied.path().to_path_buf(),
        VERSION,
        Box::new(Reversible),
    );
    let status = block_on(backend.vault_status()).unwrap();
    assert_eq!((status.available, status.readable), (true, Some(false)));
    assert_eq!(status.state().label(), "Key lost");
    backend.shut_down();

    let fresh = Scratch::new("api-vault");
    let backend =
        Backend::start_with_vault_of_data_folder(fresh.join("data"), VERSION, Box::new(Reversible));
    let status = block_on(backend.vault_status()).unwrap();
    assert_eq!((status.available, status.readable), (true, Some(true)));
    assert_eq!(status.state().label(), "Working");
    assert!(!status.shows_reset());
    backend.shut_down();
}

#[test]
fn the_self_test_leaves_nothing_behind() {
    let scratch = Scratch::with_copy_of("api-vault", &oracle("fixtures/data-folder"));
    let data_dir = scratch.path().to_path_buf();
    let mut expected = rows(&data_dir).secrets;
    expected.remove(SecretKey::DiagnosticsRoundtrip.as_str());
    let backend = Backend::start_with(
        data_dir.clone(),
        VERSION,
        Launchers::on(fake_machine(&scratch).platform, Arc::new(FakeVault)),
        None,
    );
    block_on(backend.test_vault()).unwrap();
    block_on(backend.test_vault()).unwrap();
    backend.shut_down();
    assert_eq!(rows(&data_dir).secrets, expected);
}

#[test]
fn a_reset_waits_for_the_switch_that_runs() {
    let scratch = Scratch::new("api-vault");
    let fake = fake_machine(&scratch);
    let mut launchers = Launchers::on(fake.platform.clone(), Arc::new(FakeVault));
    launchers.steam_timing = SteamTiming::with(&fake.steam_timing);
    launchers
        .steam_default_dir
        .clone_from(&fake.steam_default_dir);
    let (started, running) = channel::<()>();
    let (release, released) = channel::<()>();
    let started = Mutex::new(Some(started));
    let released = Mutex::new(released);
    let (finished_tx, finished) = channel::<LaunchJob>();
    let finished_tx = Mutex::new(finished_tx);
    let backend = Backend::start_with(
        scratch.join("data"),
        VERSION,
        launchers,
        Some(Arc::new(move |job: &LaunchJob| {
            if std::thread::current().name() == Some("switch") {
                let first = started.lock().unwrap().take();
                if let Some(first) = first {
                    first.send(()).unwrap();
                    released.lock().unwrap().recv().unwrap();
                }
            }
            if job.state != JobState::Running {
                let _ = finished_tx.lock().unwrap().send(job.clone());
            }
        })),
    );
    let seeded = fake
        .seed
        .accounts
        .iter()
        .find(|account| account.launcher == LauncherId::Steam)
        .unwrap();
    let account = block_on(backend.create_account(NewAccount {
        launcher: seeded.launcher,
        display_name: seeded.display_name.clone(),
        login: seeded.login.clone(),
        launcher_uid: seeded.launcher_uid.clone(),
        password: Some(Secret::new("placeholder".to_owned())),
        tags: Vec::new(),
        notes: String::new(),
    }))
    .unwrap();
    block_on(backend.start_launch(StartLaunch {
        account_id: account.id.clone(),
        game_id: None,
        confirm_close: true,
    }))
    .unwrap();
    running.recv().unwrap();
    assert!(backend.is_busy());
    assert_eq!(
        refusal(block_on(backend.reset_vault())),
        (
            "BUSY",
            "Another switch or launch is still running".to_owned()
        )
    );
    let question = backend.quit_question().expect("a switch runs");
    assert_eq!(question.title, "A switch is still running");
    assert_eq!(
        question.message,
        format!("One Launch is still switching to {}.", seeded.display_name)
    );
    assert_eq!(
        question.detail,
        "Quitting stops the switch. If Steam has not been started again yet, its files are put back first; after that Steam carries on signing in on its own. Left alone, the switch finishes once the launcher reports the sign-in."
    );
    assert_eq!(
        (question.keep, question.quit),
        ("Keep running", "Quit anyway")
    );
    assert!(block_on(backend.account(&account.id)).unwrap().has_secret);
    release.send(()).unwrap();
    assert_ne!(finished.recv().unwrap().state, JobState::Running);
    assert!(!backend.is_busy());
    assert_eq!(backend.quit_question(), None);
    let removed = block_on(backend.reset_vault()).unwrap();
    assert_eq!(removed.passwords, 1);
    backend.shut_down();
}
