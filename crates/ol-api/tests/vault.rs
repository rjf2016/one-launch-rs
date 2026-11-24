#![forbid(unsafe_code)]

// The Encryption panel: the vault's states, its self-test, and a reset that removes exactly what the Electron app's removes, on the fixtures' stand-in vault and on a data folder's own vault under a protector of the test's.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_lite::future::block_on;
use ol_api::accounts::{NewAccount, Secret};
use ol_api::launchers::{EpicMachine, EpicTiming, Launchers, SteamTiming};
use ol_api::settings::{
    EncryptionState, KEY_READABLE, VaultBackend, VaultHealth, VaultReset, VaultResetSummary,
};
use ol_api::{ApiError, Backend, StartLaunch};
use ol_model::{Account, Ciphertext, JobState, LaunchJob, LauncherId, SecretKey};
use ol_platform_api::{DataProtector, ProtectError, Zeroizing};
use ol_sim::FakeLaunchers;
use ol_store::{Store, database_path};
use ol_testkit::{Scratch, oracle, snapshot};
use ol_vault::{FakeVault, OsCryptVault, PROBE_VALUE, Recovered, Vault, local_state_path};
use time::macros::datetime;

const VERSION: &str = "9.8.7";
const MASK: u8 = 0xa7;
const WAIT: Duration = Duration::from_secs(30);

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

// This user's DPAPI, which for a time unwraps nothing but what it wrapped itself: the key of the folder then reads as lost, and opens again once `refusing` is off.
#[derive(Clone)]
struct Moody {
    refusing: Arc<AtomicBool>,
    made: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl Moody {
    fn new() -> Self {
        Self {
            refusing: Arc::default(),
            made: Arc::default(),
        }
    }

    fn refuse(&self, refusing: bool) {
        self.refusing.store(refusing, Ordering::SeqCst);
    }
}

impl DataProtector for Moody {
    fn protect(&self, plain: &[u8]) -> Result<Vec<u8>, ProtectError> {
        let wrapped = Reversible.protect(plain)?;
        self.made.lock().unwrap().push(wrapped.clone());
        Ok(wrapped)
    }

    fn unprotect(&self, protected: &[u8]) -> Result<Zeroizing<Vec<u8>>, ProtectError> {
        let own = self
            .made
            .lock()
            .unwrap()
            .iter()
            .any(|made| made == protected);
        if self.refusing.load(Ordering::SeqCst) && !own {
            return Err(ProtectError::Unprotect);
        }
        Reversible.unprotect(protected)
    }
}

// This user's DPAPI while Windows wraps and unwraps nothing at all, its own values included, until `down` is off.
#[derive(Clone)]
struct Down(Arc<AtomicBool>);

impl DataProtector for Down {
    fn protect(&self, plain: &[u8]) -> Result<Vec<u8>, ProtectError> {
        if self.0.load(Ordering::SeqCst) {
            return Err(ProtectError::Protect);
        }
        Reversible.protect(plain)
    }

    fn unprotect(&self, protected: &[u8]) -> Result<Zeroizing<Vec<u8>>, ProtectError> {
        if self.0.load(Ordering::SeqCst) {
            return Err(ProtectError::Unprotect);
        }
        Reversible.unprotect(protected)
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
fn a_reset_on_the_stand_in_vault_removes_only_what_it_cannot_open() {
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
    let start = || {
        Backend::start_with(
            data_dir.clone(),
            VERSION,
            Launchers::on(platform.clone(), Arc::new(FakeVault)),
            None,
        )
    };
    let backend = start();
    assert_eq!(
        block_on(backend.vault_status()).unwrap(),
        VaultHealth {
            available: true,
            backend: VaultBackend::Dpapi,
            readable: Some(true),
        }
    );
    assert_eq!(
        refusal(block_on(backend.reset_vault())),
        (
            "BAD_REQUEST",
            "Your stored passwords and saved sign-ins can be read, so nothing was removed."
                .to_owned()
        )
    );
    backend.shut_down();
    let after_refusal = rows(&data_dir);
    assert_eq!(after_refusal.secrets, before.secrets);
    assert_eq!(after_refusal.sessions, before.sessions);

    // The probe as a key that is lost would have left it: the stand-in opens it to another text.
    let (store, _) = Store::open(&database_path(&data_dir)).unwrap();
    let lost = FakeVault
        .encrypt(&Secret::new("sealed with a key that is gone".to_owned()))
        .unwrap();
    store
        .put_secret(SecretKey::VaultProbe, &lost, "2026-11-06T16:46:23.000Z")
        .unwrap();
    drop(store);
    let before = rows(&data_dir);
    let backend = start();
    assert_eq!(
        block_on(backend.vault_status()).unwrap().state(),
        EncryptionState::KeyLost
    );
    let removed = block_on(backend.reset_vault()).unwrap().summary().unwrap();
    assert_eq!(
        removed,
        VaultResetSummary {
            passwords: 0,
            sessions: 0,
            secrets: 1,
            kept: 8,
        }
    );
    assert_eq!(
        removed.note(),
        "Removed 0 passwords, 0 saved sign-ins, 1 other secret. Kept 8 that can still be read."
    );
    let status = block_on(backend.vault_status()).unwrap();
    assert_eq!(status.state(), EncryptionState::Working);
    backend.shut_down();

    let after = rows(&data_dir);
    for (id, (_, sealed)) in &before.accounts {
        assert_eq!(
            after.accounts[id].1, *sealed,
            "{id} lost a password that opens"
        );
    }
    assert_eq!(after.sessions, before.sessions);
    assert_eq!(
        after.secrets.keys().collect::<Vec<_>>(),
        before.secrets.keys().collect::<Vec<_>>()
    );
    let probe = Ciphertext::new(after.secrets[SecretKey::VaultProbe.as_str()].clone());
    assert!(FakeVault.decrypt(&probe).unwrap().expose() == PROBE_VALUE);
    assert!(snapshot(&data_dir.join("backups")) == backups);
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
fn a_reset_of_a_data_folders_own_vault_that_reads_is_refused_and_removes_nothing() {
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
    assert_eq!(
        refusal(block_on(backend.reset_vault())),
        (
            "BAD_REQUEST",
            "Your stored passwords and saved sign-ins can be read, so nothing was removed."
                .to_owned()
        )
    );
    backend.shut_down();

    assert!(
        fs::read(local_state_path(&data_dir)).unwrap() == key,
        "the key was replaced"
    );
    let after = rows(&data_dir);
    assert_eq!(after.accounts.len(), before.accounts.len());
    for (id, (_, sealed)) in &before.accounts {
        assert_eq!(after.accounts[id].1, *sealed, "{id} lost its password");
    }
    assert_eq!(after.secrets, before.secrets);
    assert!(snapshot(&data_dir.join("backups")) == backups);
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
    let removed = block_on(backend.reset_vault()).unwrap().summary().unwrap();
    assert_eq!(
        removed,
        VaultResetSummary {
            passwords: 1,
            sessions: 0,
            secrets: 1,
            kept: 0,
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
    let Ok(Recovered::Opened(new)) = OsCryptVault::replace_lost_key(&data_dir, &AnotherUser) else {
        panic!("the new key does not open");
    };
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
    running
        .recv_timeout(WAIT)
        .expect("the switch did not start");
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
    assert_ne!(
        finished
            .recv_timeout(WAIT)
            .expect("the switch did not report a final state")
            .state,
        JobState::Running
    );
    assert!(!backend.is_busy());
    assert_eq!(backend.quit_question(), None);
    assert_eq!(refusal(block_on(backend.reset_vault())).0, "BAD_REQUEST");
    assert!(block_on(backend.account(&account.id)).unwrap().has_secret);
    backend.shut_down();
}

// A start with the launchers of this PC, as `--real-launchers` makes it, on a folder whose key Windows does not unwrap for this user: it comes up, with the accounts readable, nothing that needs a secret possible and the key left as it is, until the reset replaces the key for the backend and the launchers alike.
#[test]
fn a_start_on_this_pc_with_a_key_that_does_not_unwrap_comes_up_with_the_key_lost() {
    let scratch = Scratch::new("api-vault");
    let data_dir = scratch.join("data");
    let fake = fake_machine(&scratch);
    let seeded = fake
        .seed
        .sessions
        .iter()
        .find(|session| session.launcher == LauncherId::Epic)
        .unwrap();
    let owner = fake
        .seed
        .accounts
        .iter()
        .find(|account| account.launcher == LauncherId::Epic && account.login == seeded.login)
        .unwrap();
    let start = |protector: Box<dyn DataProtector + Send>| {
        Backend::start_on_this_pc(
            data_dir.clone(),
            VERSION,
            ol_api::launchers::on_this_pc(fake.platform.clone()),
            protector,
            None,
        )
    };
    let seed = |backend: &Backend, account: &Account| {
        block_on(backend.seed_session(
            &account.id,
            Some(seeded.launcher_uid.clone()),
            Secret::new(seeded.blob.clone()),
        ))
    };

    let backend = start(Box::new(Reversible)).unwrap();
    let mut first = new_account(&owner.login, "placeholder-1");
    first.display_name.clone_from(&owner.display_name);
    let account = block_on(backend.create_account(first)).unwrap();
    seed(&backend, &account).unwrap();
    assert!(block_on(backend.account(&account.id)).unwrap().has_session);
    backend.shut_down();
    let key = fs::read(local_state_path(&data_dir)).unwrap();
    let before = rows(&data_dir);
    assert_eq!(
        (before.sessions, before.secrets.len()),
        (1, 1),
        "a saved sign-in and the probe"
    );

    let backend = start(Box::new(AnotherUser)).expect("a key that does not unwrap stops the start");
    let status = block_on(backend.vault_status()).unwrap();
    assert_eq!(status.state(), EncryptionState::KeyLost);
    assert!(status.shows_reset());
    let accounts = block_on(backend.accounts()).unwrap();
    assert_eq!(accounts.len(), 1);
    assert!(accounts[0].has_secret && accounts[0].has_session);
    assert_eq!(
        refusal(block_on(backend.set_account_password(
            &account.id,
            Secret::new("placeholder-2".to_owned())
        )))
        .0,
        "VAULT_UNAVAILABLE"
    );
    assert!(
        seed(&backend, &account).is_err(),
        "the launchers sealed with no key"
    );
    assert_eq!(
        refusal(block_on(backend.test_vault())).0,
        "VAULT_UNREADABLE"
    );
    backend.shut_down();
    assert!(
        fs::read(local_state_path(&data_dir)).unwrap() == key,
        "a start replaced the key"
    );
    assert_eq!(rows(&data_dir).secrets, before.secrets);
    assert_eq!(rows(&data_dir).sessions, 1);

    let backend = start(Box::new(AnotherUser)).unwrap();
    let removed = block_on(backend.reset_vault()).unwrap().summary().unwrap();
    assert_eq!(
        removed,
        VaultResetSummary {
            passwords: 1,
            sessions: 1,
            secrets: 1,
            kept: 0,
        }
    );
    assert_eq!(
        block_on(backend.vault_status()).unwrap().state(),
        EncryptionState::Working
    );
    seed(&backend, &account).expect("the launchers seal with the new key");
    assert!(block_on(backend.account(&account.id)).unwrap().has_session);
    block_on(backend.set_account_password(&account.id, Secret::new("placeholder-2".to_owned())))
        .unwrap();
    backend.shut_down();

    assert!(
        fs::read(local_state_path(&data_dir)).unwrap() != key,
        "the key that does not unwrap is still there"
    );
    let new = OsCryptVault::open(&data_dir, &AnotherUser).unwrap();
    let store = Store::open_read_only(&database_path(&data_dir))
        .unwrap()
        .store;
    let sealed = store.ciphertexts().unwrap();
    assert_eq!(sealed.sessions.len(), 1);
    assert!(
        new.decrypt(&sealed.sessions[0]).unwrap().expose() == seeded.blob,
        "the saved sign-in is not sealed with the new key"
    );
    assert_eq!(sealed.passwords.len(), 1);
    assert!(new.decrypt(&sealed.passwords[0]).unwrap().expose() == "placeholder-2");
}

#[test]
fn a_fault_of_the_key_file_other_than_a_lost_key_still_stops_a_start_on_this_pc() {
    let scratch = Scratch::new("api-vault");
    let data_dir = scratch.join("data");
    let fake = fake_machine(&scratch);
    let file = local_state_path(&data_dir);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, "not json").unwrap();
    let refused = Backend::start_on_this_pc(
        data_dir.clone(),
        VERSION,
        ol_api::launchers::on_this_pc(fake.platform.clone()),
        Box::new(Reversible),
        None,
    );
    assert!(refused.is_err());
    assert!(fs::read(&file).unwrap() == b"not json");
}

// A key that Windows unwraps again by the time the user asks for the reset is a vault that works: nothing is removed, the key stays, and the answer says so.
#[test]
fn a_key_that_unwraps_on_the_resets_own_try_clears_nothing() {
    let scratch = Scratch::new("api-vault");
    let data_dir = scratch.join("data");
    let windows = Moody::new();
    let backend = Backend::start_with_vault_of_data_folder(
        data_dir.clone(),
        VERSION,
        Box::new(windows.clone()),
    );
    let account =
        block_on(backend.create_account(new_account("one@example.com", "placeholder-1"))).unwrap();
    backend.shut_down();
    let key = fs::read(local_state_path(&data_dir)).unwrap();
    let before = rows(&data_dir);

    let windows = Moody::new();
    windows.refuse(true);
    let backend = Backend::start_with_vault_of_data_folder(
        data_dir.clone(),
        VERSION,
        Box::new(windows.clone()),
    );
    assert_eq!(
        block_on(backend.vault_status()).unwrap().state(),
        EncryptionState::KeyLost
    );
    windows.refuse(false);
    let answer = block_on(backend.reset_vault()).unwrap();
    assert_eq!(answer, VaultReset::KeyReadable);
    assert_eq!(answer.note(), KEY_READABLE);
    assert_eq!(
        block_on(backend.vault_status()).unwrap().state(),
        EncryptionState::Working
    );
    assert!(block_on(backend.account(&account.id)).unwrap().has_secret);
    block_on(backend.test_vault()).unwrap();
    backend.shut_down();

    assert!(
        fs::read(local_state_path(&data_dir)).unwrap() == key,
        "the key was replaced"
    );
    let after = rows(&data_dir);
    assert_eq!(after.secrets, before.secrets);
    assert_eq!(
        after.accounts[&account.id].1,
        before.accounts[&account.id].1
    );
}

// A Windows that unwraps nothing for a while, its own values among them, is no lost key: the vault reads as unavailable, no reset is offered or carried out, and once Windows works again the next look at the vault finds it working. A key that read as lost is looked at again the same way.
#[test]
fn a_fault_of_windows_that_clears_offers_no_reset_and_the_vault_is_back_at_the_next_look() {
    let scratch = Scratch::new("api-vault");
    let data_dir = scratch.join("data");
    let backend =
        Backend::start_with_vault_of_data_folder(data_dir.clone(), VERSION, Box::new(Reversible));
    let account =
        block_on(backend.create_account(new_account("one@example.com", "placeholder-1"))).unwrap();
    backend.shut_down();
    let key = fs::read(local_state_path(&data_dir)).unwrap();
    let before = rows(&data_dir);

    let down = Arc::new(AtomicBool::new(true));
    let backend = Backend::start_with_vault_of_data_folder(
        data_dir.clone(),
        VERSION,
        Box::new(Down(Arc::clone(&down))),
    );
    let status = block_on(backend.vault_status()).unwrap();
    assert_eq!(status.state(), EncryptionState::Unavailable);
    assert!(!status.shows_reset());
    assert_eq!(
        refusal(block_on(backend.reset_vault())),
        (
            "VAULT_UNAVAILABLE",
            "Windows cannot encrypt or decrypt anything just now, so nothing was removed. Try again later."
                .to_owned()
        )
    );
    down.store(false, Ordering::SeqCst);
    assert_eq!(
        block_on(backend.vault_status()).unwrap().state(),
        EncryptionState::Working
    );
    assert!(block_on(backend.account(&account.id)).unwrap().has_secret);
    block_on(backend.test_vault()).unwrap();
    backend.shut_down();

    let windows = Moody::new();
    windows.refuse(true);
    let backend = Backend::start_with_vault_of_data_folder(
        data_dir.clone(),
        VERSION,
        Box::new(windows.clone()),
    );
    let status = block_on(backend.vault_status()).unwrap();
    assert!(status.shows_reset());
    windows.refuse(false);
    let status = block_on(backend.vault_status()).unwrap();
    assert_eq!(status.state(), EncryptionState::Working);
    assert!(!status.shows_reset());
    backend.shut_down();

    assert!(fs::read(local_state_path(&data_dir)).unwrap() == key);
    let after = rows(&data_dir);
    assert_eq!(after.secrets, before.secrets);
    assert_eq!(
        after.accounts[&account.id].1,
        before.accounts[&account.id].1
    );
}

// After a reset of a lost key on this PC, everything is sealed with the new key: a password stored by the backend, a sign-in seeded through the launchers, and the sign-in a switch saves on its own thread, each opened with a vault opened afresh from the folder.
#[test]
fn after_a_reset_the_switchs_own_thread_seals_with_the_new_key() {
    let scratch = Scratch::new("api-vault");
    let data_dir = scratch.join("data");
    let launchers_dir = scratch.join("launchers");
    ol_sim::lay_down(
        &oracle("fixtures/dev-only/fake-launchers/all-four"),
        &oracle("fixtures/launchers"),
        &launchers_dir,
    )
    .unwrap();
    let start = |protector: Box<dyn DataProtector + Send>| {
        let fake = ol_sim::materialize(
            &launchers_dir,
            &data_dir,
            datetime!(2026-11-06 16:46:23 UTC),
        )
        .unwrap();
        let mut launchers = ol_api::launchers::on_this_pc(fake.platform.clone());
        launchers.epic = EpicMachine {
            timing: EpicTiming::with(&fake.epic_timing),
            default_dir: fake.epic_default_dir.clone(),
            local_app_data: fake.local_app_data.clone(),
            program_data: fake.program_data.clone(),
        };
        let (changed, jobs) = channel::<LaunchJob>();
        let changed = Mutex::new(changed);
        let backend = Backend::start_on_this_pc(
            data_dir.clone(),
            VERSION,
            launchers,
            protector,
            Some(Arc::new(move |job: &LaunchJob| {
                let _ = changed.lock().unwrap().send(job.clone());
            })),
        )
        .unwrap();
        (backend, fake, jobs)
    };
    let seed_epic = |backend: &Backend, fake: &FakeLaunchers, login: &str| {
        let session = fake
            .seed
            .sessions
            .iter()
            .find(|session| session.launcher == LauncherId::Epic && session.login == login)
            .unwrap();
        let owner = block_on(backend.accounts())
            .unwrap()
            .into_iter()
            .find(|account| account.launcher == LauncherId::Epic && account.login == login)
            .unwrap();
        block_on(backend.seed_session(
            &owner.id,
            Some(session.launcher_uid.clone()),
            Secret::new(session.blob.clone()),
        ))
        .unwrap();
        owner
    };

    let (backend, fake, _) = start(Box::new(Reversible));
    for account in fake
        .seed
        .accounts
        .iter()
        .filter(|account| account.launcher == LauncherId::Epic)
    {
        block_on(backend.create_account(NewAccount {
            launcher: account.launcher,
            display_name: account.display_name.clone(),
            login: account.login.clone(),
            launcher_uid: account.launcher_uid.clone(),
            password: Some(Secret::new("placeholder-old".to_owned())),
            tags: Vec::new(),
            notes: String::new(),
        }))
        .unwrap();
    }
    backend.shut_down();

    let (backend, fake, jobs) = start(Box::new(AnotherUser));
    assert_eq!(
        block_on(backend.vault_status()).unwrap().state(),
        EncryptionState::KeyLost
    );
    block_on(backend.reset_vault()).unwrap().summary().unwrap();
    let alt = seed_epic(&backend, &fake, "alt@example.com");
    block_on(backend.set_account_password(&alt.id, Secret::new("placeholder-new".to_owned())))
        .unwrap();
    block_on(backend.rescan()).unwrap();
    let game = block_on(backend.library())
        .unwrap()
        .games
        .into_iter()
        .map(|entry| entry.game)
        .find(|game| game.launcher == LauncherId::Epic)
        .unwrap();
    block_on(backend.start_launch(StartLaunch {
        account_id: alt.id.clone(),
        game_id: Some(game.id),
        confirm_close: true,
    }))
    .unwrap();
    let last = loop {
        let job = jobs
            .recv_timeout(WAIT)
            .expect("the switch did not report a final state");
        if job.state != JobState::Running {
            break job;
        }
    };
    assert_eq!(last.state, JobState::Done, "{:?}", last.error);
    backend.shut_down();

    let new = OsCryptVault::open(&data_dir, &AnotherUser).unwrap();
    let sealed = Store::open_read_only(&database_path(&data_dir))
        .unwrap()
        .store
        .ciphertexts()
        .unwrap();
    assert_eq!(
        sealed.sessions.len(),
        2,
        "the switch saved the sign-in it replaced beside Alt's"
    );
    for session in &sealed.sessions {
        new.decrypt(session)
            .expect("a saved sign-in is not sealed with the new key");
    }
    assert_eq!(sealed.passwords.len(), 1);
    assert!(new.decrypt(&sealed.passwords[0]).unwrap().expose() == "placeholder-new");
}

// The key file went missing and a start made a new key: the old probe reads as lost and the banner shows, while what was stored since is sealed with the new key. A reset removes what the old key sealed and keeps what the new one did.
#[test]
fn a_reset_after_the_key_file_went_missing_keeps_what_the_new_key_sealed() {
    let scratch = Scratch::new("api-vault");
    let data_dir = scratch.join("data");
    let fake = fake_machine(&scratch);
    let start = || {
        Backend::start_on_this_pc(
            data_dir.clone(),
            VERSION,
            ol_api::launchers::on_this_pc(fake.platform.clone()),
            Box::new(Reversible),
            None,
        )
        .unwrap()
    };
    // The scenario saves one Epic sign-in, Alt's; both accounts are given it.
    let epic = |_: &str| {
        fake.seed
            .sessions
            .iter()
            .find(|session| session.launcher == LauncherId::Epic)
            .unwrap()
    };
    let store_both = |backend: &Backend, login: &str| {
        let account = block_on(backend.create_account(new_account(login, "placeholder"))).unwrap();
        let session = epic(login);
        block_on(backend.seed_session(
            &account.id,
            Some(session.launcher_uid.clone()),
            Secret::new(session.blob.clone()),
        ))
        .unwrap();
        account
    };

    let backend = start();
    let old = store_both(&backend, "main@example.com");
    backend.shut_down();
    fs::remove_dir_all(data_dir.join("chromium")).unwrap();

    let backend = start();
    assert_eq!(
        block_on(backend.vault_status()).unwrap().state(),
        EncryptionState::KeyLost
    );
    let new = store_both(&backend, "alt@example.com");
    let removed = block_on(backend.reset_vault()).unwrap().summary().unwrap();
    assert_eq!(
        removed,
        VaultResetSummary {
            passwords: 1,
            sessions: 1,
            secrets: 1,
            kept: 2,
        }
    );
    assert_eq!(
        block_on(backend.vault_status()).unwrap().state(),
        EncryptionState::Working
    );
    let old = block_on(backend.account(&old.id)).unwrap();
    assert!(
        !old.has_secret && !old.has_session,
        "what the lost key sealed stays"
    );
    let new = block_on(backend.account(&new.id)).unwrap();
    assert!(
        new.has_secret && new.has_session,
        "what the new key sealed went"
    );
    backend.shut_down();

    let vault = OsCryptVault::open(&data_dir, &Reversible).unwrap();
    let sealed = Store::open_read_only(&database_path(&data_dir))
        .unwrap()
        .store
        .ciphertexts()
        .unwrap();
    assert!(vault.decrypt(&sealed.passwords[0]).unwrap().expose() == "placeholder");
    assert!(vault.decrypt(&sealed.sessions[0]).unwrap().expose() == epic("alt@example.com").blob);
}

// A folder without a key while Windows wraps nothing: the start comes up with the vault unavailable and nothing written, and the next look makes the key once Windows works.
#[test]
fn a_start_on_this_pc_while_windows_wraps_nothing_comes_up_and_makes_the_key_later() {
    let scratch = Scratch::new("api-vault");
    let data_dir = scratch.join("data");
    let fake = fake_machine(&scratch);
    let down = Arc::new(AtomicBool::new(true));
    let backend = Backend::start_on_this_pc(
        data_dir.clone(),
        VERSION,
        ol_api::launchers::on_this_pc(fake.platform.clone()),
        Box::new(Down(Arc::clone(&down))),
        None,
    )
    .expect("a Windows that wraps nothing stops the start");
    let status = block_on(backend.vault_status()).unwrap();
    assert_eq!(status.state(), EncryptionState::Unavailable);
    assert!(!status.shows_reset());
    assert!(
        !local_state_path(&data_dir).exists(),
        "a key file was written"
    );
    down.store(false, Ordering::SeqCst);
    assert_eq!(
        block_on(backend.vault_status()).unwrap().state(),
        EncryptionState::Working
    );
    backend.shut_down();
    OsCryptVault::open(&data_dir, &Reversible).unwrap();
}

// A switch that needs the vault while its key is lost is refused before anything of the launcher is closed, started or written.
#[test]
fn a_switch_that_needs_the_vault_is_refused_before_the_launcher_is_touched_while_it_is_shut() {
    let scratch = Scratch::new("api-vault");
    let data_dir = scratch.join("data");
    let fake = fake_machine(&scratch);
    let start = |protector: Box<dyn DataProtector + Send>| {
        let mut launchers = ol_api::launchers::on_this_pc(fake.platform.clone());
        launchers.epic = EpicMachine {
            timing: EpicTiming::with(&fake.epic_timing),
            default_dir: fake.epic_default_dir.clone(),
            local_app_data: fake.local_app_data.clone(),
            program_data: fake.program_data.clone(),
        };
        Backend::start_on_this_pc(data_dir.clone(), VERSION, launchers, protector, None).unwrap()
    };
    let backend = start(Box::new(Reversible));
    let account =
        block_on(backend.create_account(new_account("alt@example.com", "placeholder"))).unwrap();
    block_on(backend.rescan()).unwrap();
    backend.shut_down();

    let backend = start(Box::new(AnotherUser));
    let game = block_on(backend.library())
        .unwrap()
        .games
        .into_iter()
        .map(|entry| entry.game)
        .find(|game| game.launcher == LauncherId::Epic)
        .unwrap();
    let before = fake.log.snapshot().len();
    let preflight = block_on(backend.launch_preflight(&account.id, Some(&game.id))).unwrap();
    let blocked = preflight
        .blocked
        .expect("the preflight lets the switch through");
    assert!(blocked.contains("Settings › Advanced"), "{blocked}");
    let refused = block_on(backend.start_launch(StartLaunch {
        account_id: account.id.clone(),
        game_id: Some(game.id.clone()),
        confirm_close: true,
    }));
    assert_eq!(
        refused.err().map(|error| error.code),
        Some("VAULT_UNAVAILABLE")
    );
    assert!(!backend.is_busy());
    let acted: Vec<String> = fake.log.snapshot()[before..]
        .iter()
        .filter_map(|event| event["op"].as_str().map(str::to_owned))
        .filter(|op| {
            ["proc.", "reg.", "file.", "shell."]
                .iter()
                .any(|kind| op.starts_with(kind))
        })
        .collect();
    assert_eq!(acted, Vec::<String>::new());
    backend.shut_down();
}

// This user's DPAPI refusing the folder's key for as many tries as `refusals` holds, while it unwraps what it wrapped itself all along.
#[derive(Clone)]
struct Refuses {
    refusals: Arc<AtomicUsize>,
    made: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl DataProtector for Refuses {
    fn protect(&self, plain: &[u8]) -> Result<Vec<u8>, ProtectError> {
        let wrapped = Reversible.protect(plain)?;
        self.made.lock().unwrap().push(wrapped.clone());
        Ok(wrapped)
    }

    fn unprotect(&self, protected: &[u8]) -> Result<Zeroizing<Vec<u8>>, ProtectError> {
        let own = self
            .made
            .lock()
            .unwrap()
            .iter()
            .any(|made| made == protected);
        let refused = !own
            && self
                .refusals
                .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                    left.checked_sub(1)
                })
                .is_ok();
        if refused {
            return Err(ProtectError::Unprotect);
        }
        Reversible.unprotect(protected)
    }
}

fn delete_probe(data_dir: &Path) {
    let store = Store::open(&database_path(data_dir)).unwrap().0;
    assert!(store.delete_secret(SecretKey::VaultProbe).unwrap());
}

// A lost key with no probe stored: the reset stores one no key opens before it replaces the key, so a clear that failed would still read as lost at the next start; the clear removes it with the rest, and the vault then reads as working, after a start too.
#[test]
fn a_reset_of_a_lost_key_with_no_probe_finishes_and_leaves_a_probe_that_opens() {
    let scratch = Scratch::new("api-vault");
    let data_dir = scratch.join("data");
    let backend =
        Backend::start_with_vault_of_data_folder(data_dir.clone(), VERSION, Box::new(Reversible));
    let account =
        block_on(backend.create_account(new_account("one@example.com", "placeholder-1"))).unwrap();
    backend.shut_down();
    delete_probe(&data_dir);

    let backend =
        Backend::start_with_vault_of_data_folder(data_dir.clone(), VERSION, Box::new(AnotherUser));
    assert_eq!(
        block_on(backend.vault_status()).unwrap().state(),
        EncryptionState::KeyLost
    );
    let removed = block_on(backend.reset_vault()).unwrap().summary().unwrap();
    assert_eq!(
        (removed.passwords, removed.sessions, removed.kept),
        (1, 0, 0)
    );
    assert_eq!(
        block_on(backend.vault_status()).unwrap().readable,
        Some(true)
    );
    assert!(!block_on(backend.account(&account.id)).unwrap().has_secret);
    backend.shut_down();

    let backend =
        Backend::start_with_vault_of_data_folder(data_dir.clone(), VERSION, Box::new(AnotherUser));
    assert_eq!(
        block_on(backend.vault_status()).unwrap().state(),
        EncryptionState::Working
    );
    backend.shut_down();
    let new = OsCryptVault::open(&data_dir, &AnotherUser).unwrap();
    let after = rows(&data_dir);
    assert_eq!(
        after.secrets.keys().collect::<Vec<_>>(),
        [SecretKey::VaultProbe.as_str()]
    );
    let probe = Ciphertext::new(after.secrets[SecretKey::VaultProbe.as_str()].clone());
    assert!(new.decrypt(&probe).unwrap().expose() == PROBE_VALUE);
}

// A key that opens only on the reset's own try, beside a probe it does not open: the reset does not answer that the secrets can be read, but clears what the key does not open and keeps what it does.
#[test]
fn a_key_that_opens_late_beside_a_probe_it_does_not_open_is_cleared_not_called_readable() {
    let scratch = Scratch::new("api-vault");
    let data_dir = scratch.join("data");
    let backend =
        Backend::start_with_vault_of_data_folder(data_dir.clone(), VERSION, Box::new(Reversible));
    let account =
        block_on(backend.create_account(new_account("one@example.com", "placeholder-1"))).unwrap();
    backend.shut_down();
    let key = fs::read(local_state_path(&data_dir)).unwrap();
    Store::open(&database_path(&data_dir))
        .unwrap()
        .0
        .put_secret(
            SecretKey::VaultProbe,
            &Ciphertext::new(vec![0xa5]),
            "2026-11-06T16:46:23.000Z",
        )
        .unwrap();

    let windows = Refuses {
        refusals: Arc::new(AtomicUsize::new(usize::MAX)),
        made: Arc::default(),
    };
    let backend = Backend::start_with_vault_of_data_folder(
        data_dir.clone(),
        VERSION,
        Box::new(windows.clone()),
    );
    assert_eq!(
        block_on(backend.vault_status()).unwrap().state(),
        EncryptionState::KeyLost
    );
    windows.refusals.store(2, Ordering::SeqCst);
    let answer = block_on(backend.reset_vault()).unwrap();
    assert_eq!(windows.refusals.load(Ordering::SeqCst), 0);
    assert_eq!(
        answer.summary(),
        Some(VaultResetSummary {
            passwords: 0,
            sessions: 0,
            secrets: 1,
            kept: 1,
        })
    );
    assert_eq!(
        block_on(backend.vault_status()).unwrap().readable,
        Some(true)
    );
    assert!(block_on(backend.account(&account.id)).unwrap().has_secret);
    backend.shut_down();
    assert!(
        fs::read(local_state_path(&data_dir)).unwrap() == key,
        "the key was replaced"
    );
}

// The key file went missing and a start made a new key: a switch to an account whose saved sign-in the old key sealed would close Epic and stop at that sign-in, so it is refused before anything of the launcher is closed, started or written, and the preflight says so.
#[test]
fn a_switch_whose_saved_sign_in_the_key_does_not_open_is_refused_before_the_launcher_is_touched() {
    let scratch = Scratch::new("api-vault");
    let data_dir = scratch.join("data");
    let fake = fake_machine(&scratch);
    let start = || {
        let mut launchers = ol_api::launchers::on_this_pc(fake.platform.clone());
        launchers.epic = EpicMachine {
            timing: EpicTiming::with(&fake.epic_timing),
            default_dir: fake.epic_default_dir.clone(),
            local_app_data: fake.local_app_data.clone(),
            program_data: fake.program_data.clone(),
        };
        Backend::start_on_this_pc(
            data_dir.clone(),
            VERSION,
            launchers,
            Box::new(Reversible),
            None,
        )
        .unwrap()
    };
    let backend = start();
    let account =
        block_on(backend.create_account(new_account("alt@example.com", "placeholder"))).unwrap();
    let session = fake
        .seed
        .sessions
        .iter()
        .find(|session| session.launcher == LauncherId::Epic)
        .unwrap();
    block_on(backend.seed_session(
        &account.id,
        Some(session.launcher_uid.clone()),
        Secret::new(session.blob.clone()),
    ))
    .unwrap();
    block_on(backend.rescan()).unwrap();
    backend.shut_down();
    fs::remove_dir_all(data_dir.join("chromium")).unwrap();

    let backend = start();
    assert_eq!(
        block_on(backend.vault_status()).unwrap().state(),
        EncryptionState::KeyLost
    );
    let game = block_on(backend.library())
        .unwrap()
        .games
        .into_iter()
        .map(|entry| entry.game)
        .find(|game| game.launcher == LauncherId::Epic)
        .unwrap();
    let before = fake.log.snapshot().len();
    let preflight = block_on(backend.launch_preflight(&account.id, Some(&game.id))).unwrap();
    let blocked = preflight
        .blocked
        .expect("the preflight lets the switch through");
    assert!(blocked.contains("saved sign-in"), "{blocked}");
    let refused = block_on(backend.start_launch(StartLaunch {
        account_id: account.id.clone(),
        game_id: Some(game.id.clone()),
        confirm_close: true,
    }));
    assert_eq!(
        refused.err().map(|error| error.code),
        Some("VAULT_UNREADABLE")
    );
    assert!(!backend.is_busy());
    let acted: Vec<String> = fake.log.snapshot()[before..]
        .iter()
        .filter_map(|event| event["op"].as_str().map(str::to_owned))
        .filter(|op| {
            ["proc.", "reg.", "file.", "shell."]
                .iter()
                .any(|kind| op.starts_with(kind))
        })
        .collect();
    assert_eq!(acted, Vec::<String>::new());
    backend.shut_down();
}
