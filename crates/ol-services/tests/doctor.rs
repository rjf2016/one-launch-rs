#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;

use ol_model::{Ciphertext, SecretKey, SettingKey};
use ol_platform_api::{DataProtector, ProtectError};
use ol_services::doctor::{Database, Key, Probe, Report, Tally, VaultState, examine, run};
use ol_store::{Store, database_path};
use ol_testkit::{Scratch, oracle, snapshot};
use ol_vault::{FakeVault, OsCryptVault, PROBE_VALUE, Secret, Vault};
use zeroize::Zeroizing;

const FIXTURE_REPORT: &str = "\
database: ok
schema version: 5
accounts: 9 (steam 2, epic 3, battlenet 2, riot 2)
games: 9 (8 installed)
settings: 8
secrets: 3
saved sign-ins: 3
vault key: none (the stand-in vault of the fixtures)
encrypted values: 9 of 9 decrypt (passwords 3 of 3, sign-ins 3 of 3, secrets 3 of 3)
vault probe: ok
result: ok
";

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

// Another Windows user's DPAPI: it unwraps what it wrapped itself, and nothing wrapped for the user the folder came from.
impl DataProtector for AnotherUser {
    fn protect(&self, plain: &[u8]) -> Result<Vec<u8>, ProtectError> {
        Ok([b"another-user".as_slice(), plain].concat())
    }

    fn unprotect(&self, protected: &[u8]) -> Result<Zeroizing<Vec<u8>>, ProtectError> {
        protected
            .strip_prefix(b"another-user".as_slice())
            .map(|plain| Zeroizing::new(plain.to_vec()))
            .ok_or(ProtectError::Unprotect)
    }
}

fn fixture() -> Scratch {
    Scratch::with_copy_of("doctor", &oracle("fixtures/data-folder"))
}

fn stand_in(data_dir: &Path) -> Report {
    examine(data_dir, &VaultState::StandIn(&FakeVault))
}

// Every text the fixture holds that says whose data it is, or is a stored value: read here, so none of it is written in this file.
fn telling_texts(data_dir: &Path) -> Vec<String> {
    let store = Store::open_read_only(&database_path(data_dir))
        .unwrap()
        .store;
    let mut texts = vec![PROBE_VALUE.to_owned()];
    for account in store.accounts().unwrap() {
        texts.extend([
            account.id,
            account.display_name,
            account.login,
            account.notes,
        ]);
        texts.extend(account.launcher_uid);
        texts.extend(account.tags);
    }
    for game in store.installed_games().unwrap() {
        texts.extend([game.id, game.launcher_game_id, game.name]);
        texts.extend(game.install_path);
    }
    let settings = store.settings().unwrap();
    texts.extend(
        settings
            .keys()
            .filter_map(|key| settings.get(key))
            .map(str::to_owned),
    );
    let stored = store.ciphertexts().unwrap();
    let values = stored
        .passwords
        .iter()
        .chain(&stored.sessions)
        .chain(stored.secrets.iter().map(|(_, value)| value));
    for value in values {
        texts.push(FakeVault.decrypt(value).unwrap().expose().to_owned());
    }
    texts.retain(|text| text.len() >= 4 && !["true", "false", "debug"].contains(&text.as_str()));
    assert!(texts.len() > 80, "only {} texts were found", texts.len());
    assert!(texts.iter().any(|text| text.ends_with("@example.com")));
    texts
}

#[test]
fn the_fixture_folder_is_reported_in_counts_and_left_byte_for_byte() {
    let scratch = fixture();
    let before = snapshot(scratch.path());
    assert!(before.len() > 40, "the fixture folder was not copied whole");

    let report = stand_in(scratch.path());
    assert_eq!(report.to_string(), FIXTURE_REPORT);
    assert!(report.passed());
    assert_eq!(
        report.database.as_ref().unwrap().encrypted(),
        Tally {
            found: 9,
            decrypted: 9
        }
    );
    assert!(before == snapshot(scratch.path()), "the folder changed");
}

#[test]
fn no_report_holds_a_name_a_login_an_id_or_a_stored_value() {
    let scratch = fixture();
    let telling = telling_texts(scratch.path());
    let other = Scratch::new("doctor");
    OsCryptVault::open_or_create(other.path(), &Reversible).unwrap();
    let other_key = OsCryptVault::open(other.path(), &Reversible).unwrap();
    let reports = [
        stand_in(scratch.path()),
        examine(scratch.path(), &VaultState::NoKey),
        examine(scratch.path(), &VaultState::Unwrapped(&other_key)),
        run(scratch.path(), &Reversible),
        run(scratch.path(), &AnotherUser),
    ];
    for report in reports {
        let shown = format!("{report}\n{report:?}\n{report:#?}");
        for text in &telling {
            assert!(
                !shown.contains(text.as_str()),
                "a report holds a text of the fixture"
            );
        }
        let folder = scratch.path().to_string_lossy();
        assert!(
            !shown.contains(folder.as_ref()),
            "a report holds the folder"
        );
    }
}

#[test]
fn without_the_key_nothing_decrypts_and_the_check_fails() {
    let scratch = fixture();
    let before = snapshot(scratch.path());
    for report in [
        examine(scratch.path(), &VaultState::NoKey),
        run(scratch.path(), &Reversible),
    ] {
        assert_eq!(report.key, Key::Missing);
        let database = report.database.as_ref().unwrap();
        assert_eq!(
            database.encrypted(),
            Tally {
                found: 9,
                decrypted: 0
            }
        );
        assert_eq!(database.probe, Probe::NotTried);
        assert!(!report.passed());
        let shown = report.to_string();
        assert!(shown.contains("vault key: failed (chromium\\Local State holds no vault key)\n"));
        assert!(shown.contains(
            "encrypted values: 0 of 9 decrypt (passwords 0 of 3, sign-ins 0 of 3, secrets 0 of 3)\n"
        ));
        assert!(shown.contains("vault probe: not tried (no vault key)\n"));
        assert!(shown.ends_with("result: FAILED\n"));
    }
    assert!(before == snapshot(scratch.path()), "the folder changed");
}

#[test]
fn another_key_opens_nothing() {
    let scratch = fixture();
    let other = Scratch::new("doctor");
    let other_key = OsCryptVault::open_or_create(other.path(), &Reversible).unwrap();
    let report = examine(scratch.path(), &VaultState::Unwrapped(&other_key));
    let database = report.database.as_ref().unwrap();
    assert_eq!(database.encrypted().decrypted, 0);
    assert_eq!(database.probe, Probe::Unreadable);
    assert!(!report.passed());
    assert!(
        report
            .to_string()
            .contains("vault key: ok\nencrypted values: 0 of 9 decrypt")
    );
    assert!(
        report
            .to_string()
            .contains("vault probe: failed (it does not decrypt)\nresult: FAILED\n")
    );
}

fn folder_with_a_key() -> (Scratch, OsCryptVault) {
    let scratch = Scratch::new("doctor");
    let vault = OsCryptVault::open_or_create(scratch.path(), &Reversible).unwrap();
    let (store, _) = Store::open(&database_path(scratch.path())).unwrap();
    store.set_setting(SettingKey::DryRun, "true").unwrap();
    for (key, text) in [
        (SecretKey::VaultProbe, PROBE_VALUE),
        (SecretKey::DiagnosticsRoundtrip, "placeholder-roundtrip"),
    ] {
        let sealed = vault.encrypt(&Secret::new(text.to_owned())).unwrap();
        store
            .put_secret(key, &sealed, "2026-01-02T03:04:05.678Z")
            .unwrap();
    }
    (scratch, vault)
}

#[test]
fn a_folder_written_here_under_a_wrapped_key_passes() {
    let (scratch, _) = folder_with_a_key();
    let before = snapshot(scratch.path());
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        ["chromium/", "chromium/Local State", "onelaunch.db"]
    );
    let report = run(scratch.path(), &Reversible);
    assert_eq!(
        report.to_string(),
        "\
database: ok
schema version: 5
accounts: 0 (steam 0, epic 0, battlenet 0, riot 0)
games: 0 (0 installed)
settings: 1
secrets: 2
saved sign-ins: 0
vault key: ok
encrypted values: 2 of 2 decrypt (passwords 0 of 0, sign-ins 0 of 0, secrets 2 of 2)
vault probe: ok
result: ok
"
    );
    assert!(report.passed());
    assert!(before == snapshot(scratch.path()), "the folder changed");

    let report = run(scratch.path(), &AnotherUser);
    assert!(matches!(report.key, Key::Failed(_)));
    assert!(!report.passed());
    assert!(report.to_string().contains(
        "vault key: failed (Windows did not unwrap the vault key: it belongs to another Windows user or another PC)\nencrypted values: 0 of 2 decrypt"
    ));
    assert!(before == snapshot(scratch.path()), "the folder changed");
}

#[test]
fn one_value_that_does_not_decrypt_fails_the_check() {
    let (scratch, vault) = folder_with_a_key();
    {
        let (store, _) = Store::open(&database_path(scratch.path())).unwrap();
        let sealed = store
            .secret(SecretKey::DiagnosticsRoundtrip)
            .unwrap()
            .unwrap();
        let mut changed = sealed.into_bytes();
        *changed.last_mut().unwrap() ^= 0x01;
        store
            .put_secret(
                SecretKey::DiagnosticsRoundtrip,
                &Ciphertext::new(changed),
                "2026-01-02T03:04:05.678Z",
            )
            .unwrap();
    }
    let report = run(scratch.path(), &Reversible);
    let database = report.database.as_ref().unwrap();
    assert_eq!(
        database.secret_values,
        Tally {
            found: 2,
            decrypted: 1
        }
    );
    assert_eq!(database.probe, Probe::Good);
    assert!(!report.passed());
    assert!(report.to_string().ends_with("result: FAILED\n"));

    let (store, _) = Store::open(&database_path(scratch.path())).unwrap();
    let wrong = vault
        .encrypt(&Secret::new("one-launch-vault-probe-v2".to_owned()))
        .unwrap();
    store
        .put_secret(SecretKey::VaultProbe, &wrong, "2026-01-02T03:04:05.678Z")
        .unwrap();
    let good = vault
        .encrypt(&Secret::new("placeholder".to_owned()))
        .unwrap();
    store
        .put_secret(
            SecretKey::DiagnosticsRoundtrip,
            &good,
            "2026-01-02T03:04:05.678Z",
        )
        .unwrap();
    drop(store);
    let report = run(scratch.path(), &Reversible);
    let database = report.database.as_ref().unwrap();
    assert_eq!(database.encrypted().decrypted, 2);
    assert_eq!(database.probe, Probe::Wrong);
    assert!(!report.passed());
    assert!(
        report
            .to_string()
            .contains("vault probe: failed (it decrypts to another text)\nresult: FAILED\n")
    );
}

#[test]
fn a_new_folder_with_nothing_encrypted_needs_no_key() {
    let scratch = Scratch::new("doctor");
    drop(Store::open(&database_path(scratch.path())).unwrap());
    let before = snapshot(scratch.path());
    let report = run(scratch.path(), &Reversible);
    assert_eq!(report.key, Key::Missing);
    assert!(report.passed());
    assert!(report.to_string().contains(
        "vault key: none yet (nothing is encrypted)\nencrypted values: 0 of 0 decrypt (passwords 0 of 0, sign-ins 0 of 0, secrets 0 of 0)\nvault probe: none stored\nresult: ok\n"
    ));
    assert!(before == snapshot(scratch.path()), "the folder changed");

    fs::create_dir(scratch.join("chromium")).unwrap();
    fs::write(scratch.join("chromium/Local State"), b"not json").unwrap();
    let report = run(scratch.path(), &Reversible);
    assert!(
        !report.passed(),
        "a key file that cannot be read is a failure"
    );
    assert!(report.to_string().contains(
        "vault key: failed (chromium\\Local State is not in the shape Chromium writes)\n"
    ));
}

#[test]
fn the_oldest_database_is_read_and_stays_as_it_is() {
    let scratch = Scratch::new("doctor");
    let file = database_path(scratch.path());
    fs::copy(oracle("fixtures/db/onelaunch-v1-oldest.db"), &file).unwrap();
    let before = snapshot(scratch.path());
    let report = stand_in(scratch.path());
    assert_eq!(
        report.to_string(),
        "\
database: ok
schema version: 1 (read through a copy in memory, migrated to 5; the file is unchanged)
accounts: 3 (steam 2, epic 1, battlenet 0, riot 0)
games: 1 (1 installed)
settings: 1
secrets: 1
saved sign-ins: 0
vault key: none (the stand-in vault of the fixtures)
encrypted values: 2 of 2 decrypt (passwords 1 of 1, sign-ins 0 of 0, secrets 1 of 1)
vault probe: none stored
result: ok
"
    );
    assert!(before == snapshot(scratch.path()), "the folder changed");
}

#[test]
fn a_folder_that_cannot_be_read_fails_with_the_reason() {
    let empty = Scratch::new("doctor");
    let report = stand_in(empty.path());
    assert_eq!(
        report.to_string(),
        "\
database: failed (There is no One Launch database in this folder.)
vault key: none (the stand-in vault of the fixtures)
result: FAILED
"
    );
    assert!(!report.passed());
    assert!(snapshot(empty.path()).is_empty());

    let running = fixture();
    fs::write(running.join("onelaunch.db-wal"), [0; 32]).unwrap();
    let before = snapshot(running.path());
    let report = stand_in(running.path());
    assert!(report.to_string().starts_with(
        "database: failed (The database has changes that are not in the file yet (onelaunch.db-wal is not empty). Quit One Launch, then copy the folder again.)\n"
    ));
    assert!(!report.passed());
    assert!(before == snapshot(running.path()), "the folder changed");

    let damaged = Scratch::new("doctor");
    fs::write(database_path(damaged.path()), [0x5a; 4096]).unwrap();
    let report = run(damaged.path(), &Reversible);
    assert!(report.to_string().starts_with(
        "database: failed (The One Launch database is damaged (file is not a database)."
    ));
    assert!(
        report.to_string().ends_with(
            "vault key: none (chromium\\Local State holds no vault key)\nresult: FAILED\n"
        )
    );
}

#[test]
fn a_report_of_a_failure_holds_no_folder_and_no_text_of_the_fixture() {
    use std::os::windows::fs::OpenOptionsExt;

    let telling = telling_texts(fixture().path());
    let check = |folder: &Path, report: Report, reason: &str| {
        let shown = format!("{report}\n{report:?}\n{report:#?}");
        assert!(
            shown.starts_with(&format!("database: failed ({reason}")),
            "{reason}"
        );
        assert!(!report.passed(), "{reason}");
        assert!(
            !shown.contains(folder.to_str().unwrap()),
            "{reason}: the folder is in the report"
        );
        for text in &telling {
            assert!(
                !shown.contains(text.as_str()),
                "{reason}: a text of the fixture"
            );
        }
    };

    let missing = Scratch::new("doctor");
    check(
        missing.path(),
        stand_in(&missing.join("no such folder")),
        "There is no One Launch database in this folder.",
    );

    let running = fixture();
    fs::write(running.join("onelaunch.db-journal"), [0; 32]).unwrap();
    check(
        running.path(),
        stand_in(running.path()),
        "The database has changes that are not in the file yet (onelaunch.db-journal is not empty).",
    );

    let cut_short = fixture();
    let file = database_path(cut_short.path());
    let whole = fs::read(&file).unwrap();
    fs::write(&file, &whole[..whole.len() / 2]).unwrap();
    check(
        cut_short.path(),
        stand_in(cut_short.path()),
        "The One Launch database is damaged (",
    );

    let held = fixture();
    let file = database_path(held.path());
    let handle = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&file)
        .unwrap();
    check(
        held.path(),
        stand_in(held.path()),
        "The One Launch database could not be opened (unable to open database file).",
    );
    drop(handle);

    let key_is_a_folder = fixture();
    fs::create_dir(key_is_a_folder.join("chromium").join("Local State")).unwrap();
    let report = run(key_is_a_folder.path(), &Reversible);
    let shown = format!("{report}\n{report:?}");
    assert!(shown.contains("vault key: failed (chromium\\Local State could not be read: "));
    assert!(!shown.contains(key_is_a_folder.path().to_str().unwrap()));
    assert!(!report.passed());
}

#[test]
fn rows_this_version_does_not_know_are_counted_apart() {
    let database = Database {
        schema_on_disk: 5,
        accounts: 7,
        accounts_by_launcher: [1, 2, 0, 1],
        games: 4,
        games_installed: 3,
        games_known: 2,
        settings: 6,
        settings_known: 4,
        secrets: 0,
        sign_ins: 0,
        passwords: Tally::default(),
        sign_in_blobs: Tally::default(),
        secret_values: Tally::default(),
        probe: Probe::Absent,
    };
    let report = Report {
        database: Ok(database),
        key: Key::Unwrapped,
    };
    let shown = report.to_string();
    assert!(
        shown.contains("accounts: 7 (steam 1, epic 2, battlenet 0, riot 1, other launchers 3)\n")
    );
    assert!(shown.contains("games: 4 (3 installed, 1 of launchers this version does not know)\n"));
    assert!(shown.contains("settings: 6 (2 this version does not know)\n"));
    assert!(report.passed());
}
