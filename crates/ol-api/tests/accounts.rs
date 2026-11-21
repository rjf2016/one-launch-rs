#![forbid(unsafe_code)]

// What the Accounts screen asks of the backend, on a copy of the fixture folder: one account, an edit, a delete and the stored password, on every kind of run.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use futures_lite::future::block_on;
use ol_api::accounts::{AccountPatch, NewAccount, Secret};
use ol_api::launchers::{Launchers, SteamTiming};
use ol_api::{ApiError, Backend, StartLaunch};
use ol_model::{Account, JobState, LaunchJob, LauncherId};
use ol_platform_api::{DataProtector, ProtectError, Zeroizing};
use ol_store::{Store, database_path};
use ol_testkit::{Scratch, oracle, snapshot};
use ol_vault::{FakeVault, OsCryptVault, Vault, local_state_path};
use time::macros::datetime;

const VERSION: &str = "9.8.7";
const MASK: u8 = 0xa7;
const UNAVAILABLE: &str =
    "Secure storage is not available on this system. Secrets cannot be stored.";

// Stands in for DPAPI: what it wraps it alone unwraps.
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

// The key belongs to another Windows user.
struct AnotherUser;

impl DataProtector for AnotherUser {
    fn protect(&self, _: &[u8]) -> Result<Vec<u8>, ProtectError> {
        Err(ProtectError::Protect)
    }

    fn unprotect(&self, _: &[u8]) -> Result<Zeroizing<Vec<u8>>, ProtectError> {
        Err(ProtectError::Unprotect)
    }
}

fn fixture() -> Scratch {
    Scratch::with_copy_of("api-accounts", &oracle("fixtures/data-folder"))
}

fn id_of(data_dir: &Path, launcher: LauncherId, login: &str) -> String {
    Store::open_read_only(&database_path(data_dir))
        .unwrap()
        .store
        .account_by_login(launcher, login)
        .unwrap()
        .expect("the fixture has this account")
        .id
}

fn refusal<T>(answer: Result<T, ApiError>) -> (&'static str, String) {
    let error = answer.err().expect("a refusal");
    (error.code, error.message)
}

fn new_account(login: &str, password: Option<&str>) -> NewAccount {
    NewAccount {
        launcher: LauncherId::Epic,
        display_name: "Third".to_owned(),
        login: login.to_owned(),
        launcher_uid: None,
        password: password.map(|password| Secret::new(password.to_owned())),
        tags: vec!["eu".to_owned()],
        notes: "Made from the form.".to_owned(),
    }
}

fn files(folder: &Path) -> BTreeSet<String> {
    snapshot(folder).into_keys().collect()
}

// The stored passwords as they are on disk, each opened with the vault given.
fn passwords(data_dir: &Path, vault: &dyn Vault) -> Vec<(String, Option<String>)> {
    let opened = Store::open_read_only(&database_path(data_dir)).unwrap();
    let store = opened.store;
    store
        .accounts()
        .unwrap()
        .into_iter()
        .map(|account| {
            let password = store.account_password(&account.id).unwrap().map(|sealed| {
                match vault.decrypt(&sealed) {
                    Ok(plain) => plain.expose().to_owned(),
                    Err(_) => "<sealed by another vault>".to_owned(),
                }
            });
            (account.login, password)
        })
        .collect()
}

fn sealed_bytes(data_dir: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    let opened = Store::open_read_only(&database_path(data_dir)).unwrap();
    let accounts = opened.store.accounts().unwrap();
    accounts
        .into_iter()
        .map(|account| {
            let sealed = opened
                .store
                .account_password(&account.id)
                .unwrap()
                .map(|sealed| sealed.as_bytes().to_vec());
            (account.id, sealed)
        })
        .collect()
}

#[test]
fn the_vault_of_the_data_folder_opens_at_start_and_what_it_seals_reads_back() {
    let scratch = fixture();
    let epic_main = id_of(scratch.path(), LauncherId::Epic, "main@example.com");
    let data_dir = scratch.path().to_path_buf();
    let before = files(&data_dir);
    let fixture_sealed = sealed_bytes(&data_dir);
    assert!(!local_state_path(&data_dir).exists());

    let backend =
        Backend::start_with_vault_of_data_folder(data_dir.clone(), VERSION, Box::new(Reversible));
    let created = block_on(backend.create_account(new_account(
        "third@example.com",
        Some("placeholder-created"),
    )))
    .unwrap();
    assert!(created.has_secret);
    let stored = block_on(
        backend.set_account_password(&epic_main, Secret::new("placeholder-set".to_owned())),
    )
    .unwrap();
    assert!(stored.has_secret);
    backend.shut_down();

    let made: Vec<String> = files(&data_dir).difference(&before).cloned().collect();
    assert_eq!(made, ["chromium/Local State"]);
    let key = fs::read(local_state_path(&data_dir)).unwrap();
    let vault = OsCryptVault::open(&data_dir, &Reversible).unwrap();
    let read = passwords(&data_dir, &vault);
    let of = |login: &str| {
        read.iter()
            .find(|(each, _)| each == login)
            .and_then(|(_, password)| password.clone())
    };
    assert_eq!(
        of("third@example.com").as_deref(),
        Some("placeholder-created")
    );
    assert_eq!(of("main@example.com").as_deref(), Some("placeholder-set"));

    let backend =
        Backend::start_with_vault_of_data_folder(data_dir.clone(), VERSION, Box::new(Reversible));
    block_on(backend.set_account_password(&epic_main, Secret::new("placeholder-again".to_owned())))
        .unwrap();
    let cleared = block_on(backend.clear_account_password(&created.id)).unwrap();
    assert!(!cleared.has_secret);
    backend.shut_down();
    assert!(
        fs::read(local_state_path(&data_dir)).unwrap() == key,
        "the key was replaced"
    );
    let read = passwords(&data_dir, &vault);
    let of = |login: &str| {
        read.iter()
            .find(|(each, _)| each == login)
            .and_then(|(_, password)| password.clone())
    };
    assert_eq!(of("main@example.com").as_deref(), Some("placeholder-again"));
    assert_eq!(of("third@example.com"), None);

    let mut after = sealed_bytes(&data_dir);
    assert_eq!(after.remove(&created.id), Some(None));
    let mut expected = fixture_sealed;
    let main = expected.get_mut(&epic_main).unwrap();
    assert_eq!(*main, None);
    *main = after[&epic_main].clone();
    assert!(main.is_some());
    assert_eq!(
        after, expected,
        "the fixture's own passwords are left as they were"
    );
}

#[test]
fn a_key_that_does_not_unwrap_leaves_the_vault_unavailable_and_is_never_replaced() {
    let scratch = fixture();
    let epic_main = id_of(scratch.path(), LauncherId::Epic, "main@example.com");
    let data_dir = scratch.path().to_path_buf();
    let backend =
        Backend::start_with_vault_of_data_folder(data_dir.clone(), VERSION, Box::new(Reversible));
    block_on(backend.accounts()).unwrap();
    backend.shut_down();
    let key = fs::read(local_state_path(&data_dir)).unwrap();

    let backend =
        Backend::start_with_vault_of_data_folder(data_dir.clone(), VERSION, Box::new(AnotherUser));
    assert_eq!(
        refusal(block_on(backend.set_account_password(
            &epic_main,
            Secret::new("placeholder".to_owned())
        ))),
        ("VAULT_UNAVAILABLE", UNAVAILABLE.to_owned())
    );
    assert_eq!(
        refusal(block_on(backend.create_account(new_account(
            "third@example.com",
            Some("placeholder")
        )))),
        ("VAULT_UNAVAILABLE", UNAVAILABLE.to_owned())
    );
    let created = block_on(backend.create_account(new_account("third@example.com", None))).unwrap();
    assert!(!created.has_secret);
    block_on(backend.update_account(
        &created.id,
        AccountPatch {
            notes: Some(String::new()),
            ..AccountPatch::default()
        },
    ))
    .unwrap();
    block_on(backend.delete_account(&created.id)).unwrap();
    backend.shut_down();
    assert!(fs::read(local_state_path(&data_dir)).unwrap() == key);
}

#[test]
fn without_a_vault_accounts_are_made_changed_and_deleted_and_no_password_is_stored() {
    let scratch = fixture();
    let persona_three = id_of(scratch.path(), LauncherId::Steam, "user_3");
    let data_dir = scratch.path().to_path_buf();
    let fixture_sealed = sealed_bytes(&data_dir);
    let backend = Backend::start(data_dir.clone(), VERSION);
    assert_eq!(
        refusal(block_on(backend.create_account(new_account(
            "third@example.com",
            Some("placeholder")
        )))),
        ("VAULT_UNAVAILABLE", UNAVAILABLE.to_owned())
    );
    assert_eq!(block_on(backend.accounts()).unwrap().len(), 9);
    let created = block_on(backend.create_account(new_account("third@example.com", None))).unwrap();
    assert_eq!(block_on(backend.account(&created.id)).unwrap(), created);
    assert_eq!(
        refusal(block_on(backend.set_account_password(
            &created.id,
            Secret::new("placeholder".to_owned())
        ))),
        ("VAULT_UNAVAILABLE", UNAVAILABLE.to_owned())
    );
    let edited = block_on(backend.update_account(
        &created.id,
        AccountPatch {
            display_name: Some(" Fourth ".to_owned()),
            launcher_uid: Some(Some("44444444444444444444444444444444".to_owned())),
            ..AccountPatch::default()
        },
    ))
    .unwrap();
    assert_eq!(edited.display_name, "Fourth");
    assert_eq!(
        edited.launcher_uid.as_deref(),
        Some("44444444444444444444444444444444")
    );
    assert!(edited.updated_at >= created.updated_at);
    let cleared = block_on(backend.clear_account_password(&persona_three)).unwrap();
    assert!(!cleared.has_secret);
    block_on(backend.delete_account(&created.id)).unwrap();
    assert_eq!(
        refusal(block_on(backend.account(&created.id))),
        (
            "NOT_FOUND",
            format!("Account {} does not exist", created.id)
        )
    );
    backend.shut_down();
    assert!(!local_state_path(&data_dir).exists());
    let mut expected = fixture_sealed;
    assert!(expected[&persona_three].is_some());
    expected.insert(persona_three.clone(), None);
    assert_eq!(sealed_bytes(&data_dir), expected);
}

#[test]
fn an_edit_and_a_delete_answer_with_the_electron_apps_refusals() {
    let scratch = fixture();
    let epic_alt = id_of(scratch.path(), LauncherId::Epic, "alt@example.com");
    let backend = Backend::start(scratch.path().to_path_buf(), VERSION);
    let patch =
        |id: &str, patch: AccountPatch| refusal(block_on(backend.update_account(id, patch)));
    assert_eq!(
        patch(
            &epic_alt,
            AccountPatch {
                login: Some("Main@Example.com".to_owned()),
                ..AccountPatch::default()
            }
        ),
        (
            "DUPLICATE",
            "\"Main\" already uses this login on Epic Games. Edit that account instead, or change its login first.".to_owned()
        )
    );
    assert_eq!(
        patch(
            &epic_alt,
            AccountPatch {
                display_name: Some("   ".to_owned()),
                ..AccountPatch::default()
            }
        ),
        ("BAD_REQUEST", "Display name is required".to_owned())
    );
    assert_eq!(
        patch("missing", AccountPatch::default()),
        ("NOT_FOUND", "Account missing does not exist".to_owned())
    );
    assert_eq!(
        refusal(block_on(backend.delete_account("missing"))),
        ("NOT_FOUND", "Account missing does not exist".to_owned())
    );
    assert_eq!(
        refusal(block_on(
            backend.set_account_password(&epic_alt, Secret::new(String::new()))
        )),
        ("BAD_REQUEST", "Password must not be empty".to_owned())
    );
    assert_eq!(
        refusal(block_on(backend.clear_account_password("missing"))),
        ("NOT_FOUND", "Account missing does not exist".to_owned())
    );
    backend.shut_down();
}

#[test]
fn a_deleted_account_takes_its_sign_in_and_its_games_show_it_as_removed() {
    let scratch = fixture();
    let epic_alt = id_of(scratch.path(), LauncherId::Epic, "alt@example.com");
    let backend = Backend::start(scratch.path().to_path_buf(), VERSION);
    let played = |backend: &Backend| {
        block_on(backend.library())
            .unwrap()
            .games
            .into_iter()
            .find_map(|entry| {
                entry
                    .game
                    .last_launch
                    .filter(|last| last.account_id == epic_alt)
            })
            .expect("the game was played")
    };
    assert_eq!(played(&backend).account_name.as_deref(), Some("Alt"));
    assert!(
        block_on(backend.session(&epic_alt))
            .unwrap()
            .session
            .is_some()
    );
    let games = block_on(backend.library()).unwrap().games.len();

    block_on(backend.delete_account(&epic_alt)).unwrap();

    let last = played(&backend);
    assert_eq!(last.account_id, epic_alt);
    assert_eq!(last.account_name, None);
    assert_eq!(block_on(backend.library()).unwrap().games.len(), games);
    assert_eq!(refusal(block_on(backend.session(&epic_alt))).0, "NOT_FOUND");
    assert!(
        block_on(backend.recent_launches(50))
            .unwrap()
            .iter()
            .all(|recent| recent.account_id != epic_alt)
    );
    assert_eq!(block_on(backend.accounts()).unwrap().len(), 8);
    backend.shut_down();
    let opened = Store::open_read_only(&database_path(scratch.path())).unwrap();
    assert!(opened.store.session_blob(&epic_alt).unwrap().is_none());
    assert_eq!(opened.store.row_counts().unwrap().launch_history, 8);
}

struct Held {
    started: Mutex<Option<Sender<()>>>,
    release: Mutex<Receiver<()>>,
}

#[test]
fn what_changes_an_account_waits_for_the_switch_that_runs() {
    let scratch = Scratch::new("api-accounts");
    let launchers_dir = scratch.join("launchers");
    ol_sim::lay_down(
        &oracle("fixtures/dev-only/fake-launchers/all-four"),
        &oracle("fixtures/launchers"),
        &launchers_dir,
    )
    .unwrap();
    let fake = ol_sim::materialize(
        &launchers_dir,
        &scratch.join("data"),
        datetime!(2026-11-06 16:46:23 UTC),
    )
    .unwrap();
    let mut launchers = Launchers::on(fake.platform.clone(), Arc::new(FakeVault));
    launchers.steam_timing = SteamTiming::with(&fake.steam_timing);
    launchers
        .steam_default_dir
        .clone_from(&fake.steam_default_dir);
    let (started, running) = channel();
    let (release, released) = channel();
    let held = Arc::new(Held {
        started: Mutex::new(Some(started)),
        release: Mutex::new(released),
    });
    let (finished_tx, finished) = channel::<LaunchJob>();
    let finished_tx = Mutex::new(finished_tx);
    let observer = Arc::clone(&held);
    let backend = Backend::start_with(
        scratch.join("data"),
        VERSION,
        launchers,
        Some(Arc::new(move |job: &LaunchJob| {
            // The switch is held at its first step until the test has asked what it means to ask.
            if std::thread::current().name() == Some("switch") {
                let first = observer.started.lock().unwrap().take();
                if let Some(first) = first {
                    first.send(()).unwrap();
                    observer.release.lock().unwrap().recv().unwrap();
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
    let account: Account = block_on(backend.create_account(NewAccount {
        launcher: seeded.launcher,
        display_name: seeded.display_name.clone(),
        login: seeded.login.clone(),
        launcher_uid: seeded.launcher_uid.clone(),
        password: None,
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
    let busy = (
        "BUSY",
        "Another switch or launch is still running".to_owned(),
    );
    assert_eq!(
        refusal(block_on(backend.update_account(
            &account.id,
            AccountPatch {
                notes: Some("changed".to_owned()),
                ..AccountPatch::default()
            }
        ))),
        busy
    );
    assert_eq!(refusal(block_on(backend.delete_account(&account.id))), busy);
    assert_eq!(
        refusal(block_on(backend.set_account_password(
            &account.id,
            Secret::new("placeholder".to_owned())
        ))),
        busy
    );
    assert_eq!(
        refusal(block_on(backend.clear_account_password(&account.id))),
        busy
    );
    assert_eq!(block_on(backend.account(&account.id)).unwrap().notes, "");
    release.send(()).unwrap();
    let job = finished.recv().unwrap();
    assert_ne!(job.state, JobState::Running);

    let edited = block_on(backend.update_account(
        &account.id,
        AccountPatch {
            notes: Some("changed".to_owned()),
            ..AccountPatch::default()
        },
    ))
    .unwrap();
    assert_eq!(edited.notes, "changed");
    block_on(backend.delete_account(&account.id)).unwrap();
    backend.shut_down();
}
