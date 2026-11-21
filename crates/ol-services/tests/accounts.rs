#![forbid(unsafe_code)]

// Making, changing and deleting accounts on a copy of the fixture folder, as `AccountsService` of the Electron app does (`test/accounts.test.ts`).

use ol_model::{Account, Ciphertext, LauncherId};
use ol_services::accounts::{self, AccountError, AccountPatch, NewAccount};
use ol_store::{Store, database_path};
use ol_testkit::{Scratch, oracle};
use ol_vault::{FakeVault, Secret, Vault, VaultError};

const NOW: &str = "2026-11-21T10:00:00.000Z";

struct Unavailable;

impl Vault for Unavailable {
    fn encrypt(&self, _: &Secret) -> Result<Ciphertext, VaultError> {
        Err(VaultError::Unavailable)
    }

    fn decrypt(&self, _: &Ciphertext) -> Result<Secret, VaultError> {
        Err(VaultError::Unavailable)
    }
}

struct Fixture {
    _scratch: Scratch,
    store: Store,
}

impl Fixture {
    fn id(&self, launcher: LauncherId, login: &str) -> String {
        self.store
            .account_by_login(launcher, login)
            .unwrap()
            .expect("the fixture has this account")
            .id
    }
}

fn fixture() -> Fixture {
    let scratch = Scratch::with_copy_of("accounts", &oracle("fixtures/data-folder"));
    let (store, _) = Store::open(&database_path(scratch.path())).unwrap();
    Fixture {
        _scratch: scratch,
        store,
    }
}

fn others(store: &Store, id: &str) -> Vec<(Account, Option<Vec<u8>>)> {
    store
        .accounts()
        .unwrap()
        .into_iter()
        .filter(|account| account.id != id)
        .map(|account| {
            let sealed = store
                .account_password(&account.id)
                .unwrap()
                .map(|sealed| sealed.as_bytes().to_vec());
            (account, sealed)
        })
        .collect()
}

fn refusal(result: Result<Account, AccountError>) -> (&'static str, String) {
    let error = result.expect_err("refused");
    (error.code(), error.to_string())
}

fn sealed(store: &Store, id: &str) -> Option<Vec<u8>> {
    store
        .account_password(id)
        .unwrap()
        .map(|sealed| sealed.as_bytes().to_vec())
}

#[test]
fn an_edit_changes_only_the_fields_in_the_patch() {
    let fixture = fixture();
    let store = &fixture.store;
    let persona_three = fixture.id(LauncherId::Steam, "user_3");
    let before = store.account(&persona_three).unwrap().unwrap();
    let password = sealed(store, &persona_three);
    let untouched = others(store, &persona_three);

    let edited = accounts::update(
        store,
        &persona_three,
        &AccountPatch {
            notes: Some("  changed  ".to_owned()),
            ..AccountPatch::default()
        },
        NOW,
    )
    .unwrap();
    assert_eq!(
        edited,
        Account {
            notes: "  changed  ".to_owned(),
            updated_at: NOW.to_owned(),
            ..before.clone()
        }
    );
    assert_eq!(sealed(store, &persona_three), password);

    let edited = accounts::update(
        store,
        &persona_three,
        &AccountPatch {
            display_name: Some("  Persona 3 ".to_owned()),
            login: Some(" user_3b ".to_owned()),
            launcher_uid: Some(None),
            tags: Some(vec![
                " smurf ".to_owned(),
                "SMURF".to_owned(),
                String::new(),
            ]),
            notes: None,
        },
        NOW,
    )
    .unwrap();
    assert_eq!(
        edited,
        Account {
            display_name: "Persona 3".to_owned(),
            login: "user_3b".to_owned(),
            launcher_uid: None,
            tags: vec!["smurf".to_owned()],
            notes: "  changed  ".to_owned(),
            updated_at: NOW.to_owned(),
            ..before.clone()
        }
    );
    let cleared = accounts::update(
        store,
        &persona_three,
        &AccountPatch {
            launcher_uid: Some(Some(" uid-three ".to_owned())),
            ..AccountPatch::default()
        },
        NOW,
    )
    .unwrap();
    assert_eq!(cleared.launcher_uid.as_deref(), Some("uid-three"));
    let blank = accounts::update(
        store,
        &persona_three,
        &AccountPatch {
            launcher_uid: Some(Some("   ".to_owned())),
            ..AccountPatch::default()
        },
        NOW,
    )
    .unwrap();
    assert_eq!(blank.launcher_uid, None);
    assert_eq!(sealed(store, &persona_three), password);
    assert_eq!(others(store, &persona_three), untouched);
}

#[test]
fn an_edit_that_breaks_a_rule_is_refused_and_changes_nothing() {
    let fixture = fixture();
    let store = &fixture.store;
    let epic_alt = fixture.id(LauncherId::Epic, "alt@example.com");
    let epic_main = fixture.id(LauncherId::Epic, "main@example.com");
    let riot_main = fixture.id(LauncherId::Riot, "riot_main");
    let before = store.accounts().unwrap();
    let patch = |patch: AccountPatch| refusal(accounts::update(store, &epic_alt, &patch, NOW));
    let bad = |message: &str| ("BAD_REQUEST", message.to_owned());

    assert_eq!(
        patch(AccountPatch {
            login: Some(" ".to_owned()),
            ..AccountPatch::default()
        }),
        bad("Login is required")
    );
    assert_eq!(
        patch(AccountPatch {
            display_name: Some(String::new()),
            ..AccountPatch::default()
        }),
        bad("Display name is required")
    );
    assert_eq!(
        patch(AccountPatch {
            display_name: Some("n".repeat(121)),
            ..AccountPatch::default()
        }),
        bad("Display name must be at most 120 characters")
    );
    assert_eq!(
        patch(AccountPatch {
            login: Some("l".repeat(257)),
            ..AccountPatch::default()
        }),
        bad("Login must be at most 256 characters")
    );
    assert_eq!(
        patch(AccountPatch {
            launcher_uid: Some(Some("1".repeat(65))),
            ..AccountPatch::default()
        }),
        bad("Launcher ID must be at most 64 characters")
    );
    assert_eq!(
        patch(AccountPatch {
            notes: Some("n".repeat(20_001)),
            ..AccountPatch::default()
        }),
        bad("Notes must be at most 20000 characters")
    );
    assert_eq!(
        patch(AccountPatch {
            login: Some("MAIN@example.com".to_owned()),
            ..AccountPatch::default()
        }),
        (
            "DUPLICATE",
            "\"Main\" already uses this login on Epic Games. Edit that account instead, or change its login first.".to_owned()
        )
    );
    assert_eq!(
        patch(AccountPatch {
            launcher_uid: Some(Some("33333333333333333333333333333333".to_owned())),
            ..AccountPatch::default()
        }),
        (
            "DUPLICATE",
            "\"Main\" is already linked to this Epic Games account id. Edit that account instead, or clear its id first.".to_owned()
        )
    );
    assert_eq!(
        refusal(accounts::update(
            store,
            "missing",
            &AccountPatch::default(),
            NOW
        )),
        ("NOT_FOUND", "Account missing does not exist".to_owned())
    );
    assert_eq!(store.accounts().unwrap(), before);

    let own = accounts::update(
        store,
        &epic_main,
        &AccountPatch {
            login: Some("MAIN@example.com".to_owned()),
            launcher_uid: Some(Some("33333333333333333333333333333333".to_owned())),
            ..AccountPatch::default()
        },
        NOW,
    )
    .unwrap();
    assert_eq!(own.login, "MAIN@example.com");
    let riot = accounts::update(
        store,
        &riot_main,
        &AccountPatch {
            login: Some("alt@example.com".to_owned()),
            ..AccountPatch::default()
        },
        NOW,
    )
    .unwrap();
    assert_eq!(riot.login, "alt@example.com");
}

#[test]
fn a_password_is_set_replaced_and_cleared_without_touching_anything_else() {
    let fixture = fixture();
    let store = &fixture.store;
    let epic_main = fixture.id(LauncherId::Epic, "main@example.com");
    let before = store.account(&epic_main).unwrap().unwrap();
    let untouched = others(store, &epic_main);
    assert!(!before.has_secret);

    let stored = accounts::set_password(
        store,
        &FakeVault,
        &epic_main,
        &Secret::new("placeholder-first".to_owned()),
        NOW,
    )
    .unwrap();
    assert_eq!(
        stored,
        Account {
            has_secret: true,
            updated_at: NOW.to_owned(),
            ..before.clone()
        }
    );
    let read = |store: &Store| {
        store
            .account_password(&epic_main)
            .unwrap()
            .map(|sealed| FakeVault.decrypt(&sealed).unwrap().expose().to_owned())
    };
    assert_eq!(read(store).as_deref(), Some("placeholder-first"));
    accounts::set_password(
        store,
        &FakeVault,
        &epic_main,
        &Secret::new("placeholder-second".to_owned()),
        NOW,
    )
    .unwrap();
    assert_eq!(read(store).as_deref(), Some("placeholder-second"));

    let set = |vault: &dyn Vault, id: &str, password: String| {
        refusal(accounts::set_password(
            store,
            vault,
            id,
            &Secret::new(password),
            NOW,
        ))
    };
    assert_eq!(
        set(&FakeVault, &epic_main, String::new()),
        ("BAD_REQUEST", "Password must not be empty".to_owned())
    );
    assert_eq!(
        set(&FakeVault, &epic_main, "p".repeat(1025)),
        (
            "BAD_REQUEST",
            "Password must be at most 1024 characters".to_owned()
        )
    );
    assert_eq!(
        set(&Unavailable, &epic_main, "placeholder-third".to_owned()),
        (
            "VAULT_UNAVAILABLE",
            "Secure storage is not available on this system. Secrets cannot be stored.".to_owned()
        )
    );
    assert_eq!(
        set(&FakeVault, "missing", String::new()),
        ("NOT_FOUND", "Account missing does not exist".to_owned())
    );
    assert_eq!(read(store).as_deref(), Some("placeholder-second"));
    assert!(
        accounts::set_password(
            store,
            &FakeVault,
            &epic_main,
            &Secret::new("p".repeat(1024)),
            NOW,
        )
        .is_ok()
    );

    let cleared_at = "2026-11-21T10:00:01.000Z";
    let cleared = accounts::clear_password(store, &epic_main, cleared_at).unwrap();
    assert_eq!(
        cleared,
        Account {
            updated_at: cleared_at.to_owned(),
            ..before
        }
    );
    assert_eq!(read(store), None);
    assert_eq!(
        refusal(accounts::clear_password(store, "missing", NOW)),
        ("NOT_FOUND", "Account missing does not exist".to_owned())
    );
    assert_eq!(others(store, &epic_main), untouched);
}

#[test]
fn a_deleted_account_is_gone_with_its_saved_sign_in_and_a_second_delete_is_refused() {
    let fixture = fixture();
    let store = &fixture.store;
    let riot_main = fixture.id(LauncherId::Riot, "riot_main");
    let untouched = others(store, &riot_main);
    assert!(store.session(&riot_main).unwrap().is_some());

    accounts::delete(store, &riot_main).unwrap();
    assert!(store.account(&riot_main).unwrap().is_none());
    assert!(store.session(&riot_main).unwrap().is_none());
    assert_eq!(others(store, &riot_main), untouched);
    let again = accounts::delete(store, &riot_main).err().unwrap();
    assert_eq!(
        (again.code(), again.to_string()),
        ("NOT_FOUND", format!("Account {riot_main} does not exist"))
    );
}

#[test]
fn a_new_account_keeps_its_tags_its_notes_its_id_and_its_password() {
    let fixture = fixture();
    let store = &fixture.store;
    let untouched = store.accounts().unwrap();
    let created = accounts::create(
        store,
        &FakeVault,
        &NewAccount {
            launcher: LauncherId::Battlenet,
            display_name: "  Third ".to_owned(),
            login: " third@example.com ".to_owned(),
            launcher_uid: Some(" 0c3d4e5f ".to_owned()),
            password: Some(Secret::new("placeholder-pass".to_owned())),
            tags: vec!["eu".to_owned(), " EU ".to_owned(), "support".to_owned()],
            notes: " Notes stay as typed. ".to_owned(),
        },
        NOW,
    )
    .unwrap();
    assert_eq!(
        created,
        Account {
            id: created.id.clone(),
            launcher: LauncherId::Battlenet,
            display_name: "Third".to_owned(),
            login: "third@example.com".to_owned(),
            has_secret: true,
            has_session: false,
            launcher_uid: Some("0c3d4e5f".to_owned()),
            tags: vec!["eu".to_owned(), "support".to_owned()],
            notes: " Notes stay as typed. ".to_owned(),
            last_used_at: None,
            created_at: NOW.to_owned(),
            updated_at: NOW.to_owned(),
        }
    );
    assert_eq!(created.id.len(), 36);
    assert_eq!(created.id.as_bytes()[14], b'4');
    let password = store.account_password(&created.id).unwrap().unwrap();
    assert!(FakeVault.decrypt(&password).unwrap().expose() == "placeholder-pass");
    assert_eq!(others(store, &created.id).len(), untouched.len());
    assert!(
        others(store, &created.id)
            .into_iter()
            .map(|(account, _)| account)
            .eq(untouched)
    );

    let unavailable = accounts::create(
        store,
        &Unavailable,
        &NewAccount {
            launcher: LauncherId::Steam,
            display_name: "x".to_owned(),
            login: "y".to_owned(),
            launcher_uid: None,
            password: Some(Secret::new("placeholder".to_owned())),
            tags: Vec::new(),
            notes: String::new(),
        },
        NOW,
    );
    assert_eq!(refusal(unavailable).0, "VAULT_UNAVAILABLE");
    assert!(
        store
            .account_by_login(LauncherId::Steam, "y")
            .unwrap()
            .is_none()
    );
    let empty_is_none = accounts::create(
        store,
        &Unavailable,
        &NewAccount {
            launcher: LauncherId::Steam,
            display_name: "x".to_owned(),
            login: "y".to_owned(),
            launcher_uid: None,
            password: Some(Secret::new(String::new())),
            tags: Vec::new(),
            notes: String::new(),
        },
        NOW,
    )
    .unwrap();
    assert!(!empty_is_none.has_secret);
}
