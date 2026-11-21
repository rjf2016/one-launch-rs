use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ol_model::{
    ArtworkSource, Ciphertext, LauncherId, LibrarySort, SecretKey, SettingKey, Sidebar,
};
use ol_testkit::{Scratch, oracle, snapshot};
use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags};
use time::macros::datetime;

use crate::migrations::MIGRATIONS;
use crate::open::migrate;
use crate::{
    AccountUpdate, BACKUPS_TO_KEEP, DATABASE_FILE, MigrationReport, SCHEMA_VERSION, Store,
    StoreError, database_backups_dir, database_path, iso_timestamp,
};

type Master = Vec<(String, String, String, Option<String>)>;

fn master(conn: &Connection) -> Master {
    let mut statement = conn
        .prepare("SELECT type, name, tbl_name, sql FROM sqlite_master ORDER BY type, name")
        .unwrap();
    let rows = statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap();
    rows.map(Result::unwrap).collect()
}

fn recorded(file: &str) -> (u32, Master) {
    let text = fs::read_to_string(oracle(&format!("takeover/migrations/{file}"))).unwrap();
    let dump: serde_json::Value = serde_json::from_str(&text).unwrap();
    let text = |row: &serde_json::Value, field: &str| row[field].as_str().unwrap().to_owned();
    let master = dump["master"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                text(row, "type"),
                text(row, "name"),
                text(row, "tbl_name"),
                row["sql"].as_str().map(str::to_owned),
            )
        })
        .collect();
    (
        u32::try_from(dump["user_version"].as_u64().unwrap()).unwrap(),
        master,
    )
}

fn read_only(file: &Path) -> Connection {
    Connection::open_with_flags(file, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
}

fn version(conn: &Connection) -> i64 {
    conn.pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap()
}

fn rows(conn: &Connection, sql: &str) -> Vec<Vec<Value>> {
    let mut statement = conn.prepare(sql).unwrap();
    let width = statement.column_count();
    let rows = statement
        .query_map([], |row| (0..width).map(|index| row.get(index)).collect())
        .unwrap();
    rows.map(Result::unwrap).collect()
}

fn copy_of(scratch: &Scratch, fixture: &str) -> PathBuf {
    let file = database_path(scratch.path());
    fs::copy(oracle(fixture), &file).unwrap();
    file
}

#[test]
fn a_timestamp_reads_as_the_electron_app_writes_it() {
    assert_eq!(
        iso_timestamp(datetime!(2026-01-02 03:04:05.678_9 UTC)),
        "2026-01-02T03:04:05.678Z"
    );
    assert_eq!(
        iso_timestamp(datetime!(2026-01-02 03:04:05 +02:00)),
        "2026-01-02T01:04:05.000Z"
    );
}

fn data_folder() -> Scratch {
    Scratch::with_copy_of("store", &oracle("fixtures/data-folder"))
}

#[test]
fn each_step_leaves_the_schema_the_electron_app_leaves() {
    for step in 1..=MIGRATIONS.len() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        let report = migrate(&mut conn, &MIGRATIONS[..step]).unwrap();
        let (user_version, expected) = recorded(&format!("v{step}.json"));
        assert_eq!(report.to, user_version, "step {step}");
        assert_eq!(report.applied.len(), step);
        assert_eq!(master(&conn), expected, "step {step}");
        let on: i64 = conn
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .unwrap();
        assert_eq!(on, 1, "step {step} leaves foreign keys on");
    }
}

#[test]
fn a_new_file_gets_the_whole_schema_and_the_pragmas() {
    let scratch = Scratch::new("store");
    let file = database_path(scratch.path());
    let (store, report) = Store::open(&file).unwrap();
    assert_eq!(report.from, 0);
    assert_eq!(report.to, SCHEMA_VERSION);
    assert_eq!(
        report.applied,
        [
            "1_initial",
            "2_library_artwork",
            "3_launcher_sessions",
            "4_launcher_check_dropped",
            "5_artwork_launcher_source"
        ]
    );
    assert_eq!(master(&store.conn), recorded("v5.json").1);
    let pragma = |name: &str| -> Value {
        store
            .conn
            .pragma_query_value(None, name, |row| row.get(0))
            .unwrap()
    };
    assert_eq!(pragma("journal_mode"), Value::Text("wal".to_owned()));
    assert_eq!(pragma("synchronous"), Value::Integer(1));
    assert_eq!(pragma("foreign_keys"), Value::Integer(1));
    assert_eq!(pragma("busy_timeout"), Value::Integer(5_000));
    assert_eq!(pragma("temp_store"), Value::Integer(2));
    assert_eq!(store.row_counts().unwrap(), Default::default());
    drop(store);

    let (store, report) = Store::open(&file).unwrap();
    assert_eq!(
        report,
        MigrationReport {
            from: SCHEMA_VERSION,
            to: SCHEMA_VERSION,
            applied: Vec::new()
        }
    );
    assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
}

#[test]
fn the_oldest_database_ends_at_the_schema_of_a_new_one_with_its_rows() {
    let scratch = Scratch::new("store");
    let file = copy_of(&scratch, "fixtures/db/onelaunch-v1-oldest.db");
    let kept = [
        "SELECT id, launcher, display_name, login, secret_enc, launcher_uid, tags, notes, last_used_at, created_at, updated_at FROM accounts ORDER BY id",
        "SELECT id, launcher, launcher_game_id, name, install_path, installed, last_seen_at FROM games ORDER BY id",
        "SELECT game_id, source, grid_path, hero_path, logo_path, fetched_at FROM artwork ORDER BY game_id",
        "SELECT id, game_id, account_id, launched_at, result FROM launch_history ORDER BY id",
        "SELECT key, value FROM settings ORDER BY key",
        "SELECT key, value_enc, updated_at FROM secrets ORDER BY key",
    ];
    let before: Vec<_> = {
        let conn = read_only(&file);
        assert_eq!(version(&conn), 1);
        kept.iter().map(|sql| rows(&conn, sql)).collect()
    };
    assert_eq!(
        before.iter().map(Vec::len).collect::<Vec<_>>(),
        [3, 1, 1, 0, 1, 1]
    );

    let (store, report) = Store::open(&file).unwrap();
    assert_eq!(
        report,
        MigrationReport {
            from: 1,
            to: 5,
            applied: vec![
                "2_library_artwork".to_owned(),
                "3_launcher_sessions".to_owned(),
                "4_launcher_check_dropped".to_owned(),
                "5_artwork_launcher_source".to_owned(),
            ],
        }
    );
    let new = read_only(&oracle("fixtures/db/onelaunch-empty-v5.db"));
    assert_eq!(master(&store.conn), master(&new));
    assert_eq!(master(&store.conn), recorded("v1-oldest-migrated.json").1);
    assert_eq!(version(&store.conn), version(&new));
    let after: Vec<_> = kept.iter().map(|sql| rows(&store.conn, sql)).collect();
    assert!(before == after, "a row changed or went missing");
    assert_eq!(
        rows(
            &store.conn,
            "SELECT needs_update, last_played_at FROM games"
        ),
        [[Value::Integer(0), Value::Null]]
    );

    let accounts = store.accounts().unwrap();
    assert_eq!(accounts.len(), 3);
    assert_eq!(
        accounts.iter().filter(|account| account.has_secret).count(),
        1
    );
    assert_eq!(store.installed_games().unwrap().len(), 1);
}

#[test]
fn a_rebuild_that_orphans_a_row_is_rolled_back() {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    migrate(&mut conn, &MIGRATIONS[..3]).unwrap();
    conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
    conn.execute(
        "INSERT INTO artwork (game_id, source) VALUES (?1, 'manual')",
        ["no-such-game"],
    )
    .unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    let before = master(&conn);

    let error = migrate(&mut conn, &MIGRATIONS).unwrap_err();
    assert!(
        matches!(&error, StoreError::Orphans { label, count: 1 } if label == "4_launcher_check_dropped"),
        "{error}"
    );
    assert_eq!(
        error.to_string(),
        "Migration 4_launcher_check_dropped left 1 row(s) pointing at a missing parent"
    );
    assert_eq!(version(&conn), 3);
    assert_eq!(master(&conn), before);
    let on: i64 = conn
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .unwrap();
    assert_eq!(on, 1);
}

#[test]
fn a_rebuild_keeps_the_rows_that_point_at_the_rebuilt_table() {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    migrate(&mut conn, &MIGRATIONS[..3]).unwrap();
    conn.execute_batch(
        "INSERT INTO accounts (id, launcher, display_name, login, created_at, updated_at)
           VALUES ('account-1', 'epic', 'Placeholder', 'placeholder', 't', 't');
         INSERT INTO launcher_sessions (account_id, launcher, blob_enc, fingerprint, captured_at)
           VALUES ('account-1', 'epic', x'00', 'fingerprint', 't');
         INSERT INTO games (id, launcher, launcher_game_id, name, last_seen_at)
           VALUES ('game-1', 'epic', 'app', 'Placeholder Game', 't');
         INSERT INTO artwork (game_id, source) VALUES ('game-1', 'manual');
         INSERT INTO artwork_attempts (game_id, attempted_at, error) VALUES ('game-1', 't', 'none');",
    )
    .unwrap();
    migrate(&mut conn, &MIGRATIONS).unwrap();
    for table in ["launcher_sessions", "artwork", "artwork_attempts"] {
        let sql = format!("SELECT COUNT(*) FROM {table}");
        assert_eq!(rows(&conn, &sql), [[Value::Integer(1)]], "{table}");
    }
}

#[test]
fn a_database_from_a_newer_version_is_refused_and_keeps_its_version() {
    let scratch = Scratch::new("store");
    let file = copy_of(&scratch, "fixtures/db/onelaunch-empty-v5.db");
    Connection::open(&file)
        .unwrap()
        .pragma_update(None, "user_version", 6)
        .unwrap();
    let before = snapshot(scratch.path());
    let unread = Store::open_read_only(&file).err().unwrap();
    assert!(
        before == snapshot(scratch.path()),
        "the read-only open changed the file"
    );
    for error in [unread, Store::open(&file).err().unwrap()] {
        assert!(
            matches!(
                error,
                StoreError::TooNew {
                    found: 6,
                    supported: 5
                }
            ),
            "{error}"
        );
        assert_eq!(error.code(), "DB_TOO_NEW");
        assert_eq!(
            error.to_string(),
            "This database was created by a newer version of One Launch (schema 6; this version supports up to 5). Update One Launch, or restore an older copy of the database."
        );
    }
    assert_eq!(version(&read_only(&file)), 6);
}

#[test]
fn a_file_that_is_not_a_database_is_damaged() {
    let scratch = Scratch::new("store");
    let file = database_path(scratch.path());
    fs::write(&file, [0x5a; 8192]).unwrap();
    for error in [
        Store::open(&file).err().unwrap(),
        Store::open_read_only(&file).err().unwrap(),
    ] {
        assert!(matches!(error, StoreError::Corrupt(_)), "{error}");
        assert_eq!(error.code(), "DB_CORRUPT");
        assert_eq!(
            error.to_string(),
            "The One Launch database is damaged (file is not a database). Restore a copy from the backups\\db folder in the data directory, or move the file aside to start over."
        );
    }
    assert_eq!(fs::read(&file).unwrap(), [0x5a; 8192]);
}

#[test]
fn a_database_another_program_is_writing_is_in_use() {
    let scratch = Scratch::new("store");
    let file = copy_of(&scratch, "fixtures/db/onelaunch-empty-v5.db");
    let other = Connection::open(&file).unwrap();
    other.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let error = Store::open_within(&file, Duration::from_millis(50))
        .err()
        .unwrap();
    assert!(matches!(error, StoreError::Locked(_)), "{error}");
    assert_eq!(error.code(), "DB_UNAVAILABLE");
    assert!(
        error.to_string().starts_with(
            "The One Launch database is in use by another program (database is locked)."
        )
    );
    other.execute_batch("ROLLBACK").unwrap();
    drop(other);
    Store::open(&file).unwrap();
}

#[test]
fn a_file_that_cannot_be_written_is_read_only() {
    let scratch = Scratch::new("store");
    let file = copy_of(&scratch, "fixtures/db/onelaunch-empty-v5.db");
    let mut permissions = fs::metadata(&file).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&file, permissions.clone()).unwrap();
    let error = Store::open(&file).err().unwrap();
    #[expect(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    fs::set_permissions(&file, permissions).unwrap();
    assert!(matches!(error, StoreError::ReadOnly(_)), "{error}");
    assert_eq!(error.code(), "DB_UNAVAILABLE");
    assert!(
        error
            .to_string()
            .starts_with("The One Launch database is read-only (")
    );
}

#[test]
fn a_folder_that_is_not_there_cannot_be_opened() {
    let scratch = Scratch::new("store");
    let file = scratch.join("missing").join(DATABASE_FILE);
    let error = Store::open(&file).err().unwrap();
    assert!(matches!(error, StoreError::CannotOpen(_)), "{error}");
    assert_eq!(
        error.to_string(),
        "The One Launch database could not be opened (unable to open database file). Check that the data folder exists and is writable."
    );
    assert!(matches!(
        Store::open_read_only(&file).err().unwrap(),
        StoreError::NotFound
    ));
    assert!(!scratch.join("missing").exists());
}

#[test]
fn a_file_another_program_holds_cannot_be_opened_and_the_error_names_no_folder() {
    use std::os::windows::fs::OpenOptionsExt;

    let scratch = Scratch::new("store");
    let file = copy_of(&scratch, "fixtures/db/onelaunch-empty-v5.db");
    let before = snapshot(scratch.path());
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&file)
        .unwrap();
    for error in [
        Store::open_read_only(&file).err().unwrap(),
        Store::open(&file).err().unwrap(),
    ] {
        assert!(matches!(error, StoreError::CannotOpen(_)), "{error}");
        assert_eq!(
            error.to_string(),
            "The One Launch database could not be opened (unable to open database file). Check that the data folder exists and is writable."
        );
        assert!(!format!("{error:?}").contains(scratch.path().to_str().unwrap()));
    }
    drop(held);
    assert!(before == snapshot(scratch.path()), "the folder changed");
}

#[test]
fn the_fixture_folder_reads_with_the_counts_its_readme_states() {
    let scratch = data_folder();
    let (store, report) = Store::open(&database_path(scratch.path())).unwrap();
    assert!(report.applied.is_empty());
    let counts = store.row_counts().unwrap();
    assert_eq!(
        (
            counts.accounts,
            counts.games,
            counts.artwork,
            counts.artwork_attempts
        ),
        (9, 9, 5, 2)
    );
    assert_eq!(
        (
            counts.launch_history,
            counts.launcher_sessions,
            counts.settings,
            counts.secrets
        ),
        (8, 3, 8, 3)
    );

    let accounts = store.accounts().unwrap();
    assert_eq!(accounts.len(), 9);
    let of = |launcher| {
        accounts
            .iter()
            .filter(|account| account.launcher == launcher)
            .count()
    };
    assert_eq!(
        LauncherId::ALL.map(of),
        [2, 3, 2, 2],
        "steam, epic, battlenet, riot"
    );
    let order: Vec<_> = accounts
        .iter()
        .map(|account| {
            (
                account.launcher.as_str(),
                account.display_name.to_lowercase(),
            )
        })
        .collect();
    assert!(
        order.is_sorted(),
        "by launcher, then by name whatever its case"
    );
    assert_eq!(
        accounts.iter().filter(|account| account.has_secret).count(),
        3
    );
    assert_eq!(
        accounts
            .iter()
            .filter(|account| account.has_session)
            .count(),
        3
    );
    assert_eq!(
        accounts
            .iter()
            .filter(|account| !account.tags.is_empty())
            .count(),
        4
    );
    assert_eq!(
        accounts
            .iter()
            .filter(|account| !account.notes.is_empty())
            .count(),
        2
    );
    assert_eq!(
        accounts
            .iter()
            .filter(|account| account.last_used_at.is_some())
            .count(),
        2
    );
    assert_eq!(
        accounts
            .iter()
            .filter(|account| account.launcher_uid.is_some())
            .count(),
        8
    );
    for account in &accounts {
        assert_eq!(
            store.account_password(&account.id).unwrap().is_some(),
            account.has_secret
        );
        assert_eq!(
            store.session_blob(&account.id).unwrap().is_some(),
            account.has_session
        );
    }
    assert!(store.account_password("no-such-account").unwrap().is_none());

    let games = store.installed_games().unwrap();
    assert_eq!(games.len(), 8, "one of the nine is no longer installed");
    assert!(games.iter().all(|game| game.installed));
    let names: Vec<_> = games.iter().map(|game| game.name.to_lowercase()).collect();
    assert!(names.is_sorted());
    assert_eq!(games.iter().filter(|game| game.needs_update).count(), 2);
    let sources = |source| {
        games
            .iter()
            .filter(|game| {
                game.artwork
                    .as_ref()
                    .is_some_and(|art| art.source == source)
            })
            .count()
    };
    assert_eq!(
        games.iter().filter(|game| game.artwork.is_some()).count(),
        5
    );
    assert_eq!(ArtworkSource::ALL.map(sources), [1, 2, 1, 1]);
    for art in games.iter().filter_map(|game| game.artwork.as_ref()) {
        let grid = art.grid.as_deref().unwrap();
        assert!(scratch.join("artwork").join(grid).is_file());
        assert!(art.fetched_at.is_some());
    }
    assert_eq!(
        games
            .iter()
            .filter(|game| game.artwork_error.is_some())
            .count(),
        2
    );
    let launched: Vec<_> = games
        .iter()
        .filter_map(|game| game.last_launch.as_ref())
        .collect();
    assert_eq!(launched.len(), 4);
    for launch in launched {
        let account = accounts
            .iter()
            .find(|account| account.id == launch.account_id);
        assert_eq!(
            launch.account_name.as_deref(),
            account.map(|account| account.display_name.as_str())
        );
    }

    let settings = store.settings().unwrap();
    assert_eq!(settings.len(), 8);
    assert_eq!(settings.get_int(SettingKey::BackupsKeep, 20), 50);
    assert!(settings.get_bool(SettingKey::FirstRunDismissed, false));
    assert!(!settings.get_bool(SettingKey::SteamAutoLogin, true));
    assert_eq!(settings.get(SettingKey::LogLevel), Some("debug"));
    assert!(settings.get(SettingKey::WindowBounds).is_some());
    assert!(settings.get(SettingKey::DryRun).is_none());
    assert_eq!(
        store.setting(SettingKey::LogLevel).unwrap().as_deref(),
        Some("debug")
    );
    for key in SecretKey::ALL {
        assert!(store.secret(key).unwrap().is_some(), "{key:?}");
    }
    let all = store.ciphertexts().unwrap();
    assert_eq!(
        (all.passwords.len(), all.sessions.len(), all.secrets.len()),
        (3, 3, 3)
    );
}

#[test]
fn a_row_of_a_launcher_this_version_does_not_know_is_left_out() {
    let scratch = data_folder();
    let (store, _) = Store::open(&database_path(scratch.path())).unwrap();
    store
        .conn
        .execute_batch(
            "INSERT INTO accounts (id, launcher, display_name, login, created_at, updated_at)
               VALUES ('account-x', 'gog', 'Placeholder', 'placeholder', 't', 't');
             INSERT INTO games (id, launcher, launcher_game_id, name, last_seen_at)
               VALUES ('game-x', 'gog', 'app', 'Placeholder Game', 't');",
        )
        .unwrap();
    let counts = store.row_counts().unwrap();
    assert_eq!(counts.accounts, 10);
    assert_eq!(store.accounts().unwrap().len(), 9);
    assert_eq!(counts.games_installed, 9);
    assert_eq!(store.installed_games().unwrap().len(), 8);
}

#[test]
fn the_large_database_reads() {
    let scratch = Scratch::new("store");
    let file = copy_of(&scratch, "fixtures/db/onelaunch-large-v5.db");
    let (store, _) = Store::open(&file).unwrap();
    assert_eq!(store.accounts().unwrap().len(), 200);
    let games = store.installed_games().unwrap();
    assert_eq!(games.len(), 950);
    assert!(games.iter().any(|game| game.last_launch.is_some()));
}

#[test]
fn a_setting_and_a_secret_written_here_read_back_after_a_restart() {
    let scratch = data_folder();
    let file = database_path(scratch.path());
    let value = Ciphertext::new(vec![0x76, 0x31, 0x30, 0x00, 0xff]);
    {
        let (store, _) = Store::open(&file).unwrap();
        store.set_setting(SettingKey::DryRun, "true").unwrap();
        store.set_setting(SettingKey::LogLevel, "warn").unwrap();
        store.delete_setting(SettingKey::BackupsKeep).unwrap();
        store
            .put_secret(
                SecretKey::DiagnosticsRoundtrip,
                &value,
                "2026-01-02T03:04:05.678Z",
            )
            .unwrap();
    }
    let (store, _) = Store::open(&file).unwrap();
    let settings = store.settings().unwrap();
    assert_eq!(settings.len(), 8);
    assert!(settings.get_bool(SettingKey::DryRun, false));
    assert_eq!(settings.get(SettingKey::LogLevel), Some("warn"));
    assert_eq!(settings.get(SettingKey::BackupsKeep), None);
    assert!(store.secret(SecretKey::DiagnosticsRoundtrip).unwrap() == Some(value));
    assert_eq!(store.row_counts().unwrap().secrets, 3);
    assert_eq!(
        rows(
            &store.conn,
            "SELECT updated_at FROM secrets WHERE key = 'diagnostics.roundtrip'"
        ),
        [[Value::Text("2026-01-02T03:04:05.678Z".to_owned())]]
    );
}

#[test]
fn the_library_sort_is_a_row_the_electron_app_does_not_count_among_its_settings() {
    let scratch = data_folder();
    let file = database_path(scratch.path());
    {
        let (store, _) = Store::open(&file).unwrap();
        assert_eq!(store.library_sort().unwrap(), LibrarySort::LastPlayed);
        assert_eq!(store.preferences_stored().unwrap(), 0);
        store.set_library_sort(LibrarySort::Name).unwrap();
    }
    let (store, _) = Store::open(&file).unwrap();
    assert_eq!(store.library_sort().unwrap(), LibrarySort::Name);
    assert_eq!(store.preferences_stored().unwrap(), 1);
    assert_eq!(store.settings().unwrap().len(), 8);
    assert_eq!(store.row_counts().unwrap().settings, 9);
    assert_eq!(
        rows(
            &store.conn,
            "SELECT value FROM settings WHERE key = 'library.sort'"
        ),
        [[Value::Text("name".to_owned())]]
    );
    store.set_library_sort(LibrarySort::LastPlayed).unwrap();
    assert_eq!(store.library_sort().unwrap(), LibrarySort::LastPlayed);
    assert_eq!(store.row_counts().unwrap().settings, 9);
}

#[test]
fn the_sidebar_is_a_row_of_its_own_and_anything_but_expanded_reads_as_folded() {
    const OTHERS: &str = "SELECT key, value FROM settings WHERE key <> 'sidebar' ORDER BY key";
    let scratch = data_folder();
    let file = database_path(scratch.path());
    let before = {
        let (store, _) = Store::open(&file).unwrap();
        assert_eq!(store.sidebar().unwrap(), Sidebar::Folded);
        assert_eq!(store.preferences_stored().unwrap(), 0);
        store.set_sidebar(Sidebar::Open).unwrap();
        rows(&store.conn, OTHERS)
    };
    let (store, _) = Store::open(&file).unwrap();
    assert_eq!(store.sidebar().unwrap(), Sidebar::Open);
    assert_eq!(
        rows(
            &store.conn,
            "SELECT value FROM settings WHERE key = 'sidebar'"
        ),
        [[Value::Text("expanded".to_owned())]]
    );
    store.set_sidebar(Sidebar::Folded).unwrap();
    assert_eq!(store.sidebar().unwrap(), Sidebar::Folded);
    assert_eq!(
        rows(
            &store.conn,
            "SELECT value FROM settings WHERE key = 'sidebar'"
        ),
        [[Value::Text("collapsed".to_owned())]]
    );
    store
        .conn
        .execute(
            "UPDATE settings SET value = 'wide' WHERE key = 'sidebar'",
            [],
        )
        .unwrap();
    assert_eq!(store.sidebar().unwrap(), Sidebar::Folded);
    assert_eq!(store.preferences_stored().unwrap(), 1);
    assert_eq!(store.settings().unwrap().len(), 8);
    assert_eq!(store.row_counts().unwrap().settings, 9);
    assert_eq!(rows(&store.conn, OTHERS), before);
}

#[test]
fn an_empty_file_reads_as_a_database_with_nothing_in_it() {
    let scratch = Scratch::new("store");
    let file = database_path(scratch.path());
    fs::write(&file, b"").unwrap();
    {
        let opened = Store::open_read_only(&file).unwrap();
        assert_eq!(opened.schema_on_disk, 0);
        assert_eq!(opened.store.row_counts().unwrap(), Default::default());
        assert!(opened.store.accounts().unwrap().is_empty());
    }
    assert_eq!(snapshot(scratch.path()).len(), 1);
    assert!(fs::read(&file).unwrap().is_empty());
}

#[test]
fn a_read_only_open_changes_nothing_in_the_folder() {
    let scratch = data_folder();
    let before = snapshot(scratch.path());
    {
        let opened = Store::open_read_only(&database_path(scratch.path())).unwrap();
        assert_eq!(opened.schema_on_disk, 5);
        assert_eq!(
            rows(&opened.store.conn, "PRAGMA temp_store"),
            [[Value::Integer(2)]]
        );
        assert_eq!(opened.store.accounts().unwrap().len(), 9);
        assert_eq!(opened.store.installed_games().unwrap().len(), 8);
        assert_eq!(opened.store.ciphertexts().unwrap().secrets.len(), 3);
        let refused = opened.store.set_setting(SettingKey::DryRun, "true");
        assert!(refused.is_err());
        assert!(
            before == snapshot(scratch.path()),
            "the folder changed while the database was open"
        );
    }
    assert!(before == snapshot(scratch.path()), "the folder changed");
}

#[test]
fn an_old_database_is_read_through_a_copy_in_memory() {
    let scratch = Scratch::new("store");
    let file = copy_of(&scratch, "fixtures/db/onelaunch-v1-oldest.db");
    let before = snapshot(scratch.path());
    {
        let opened = Store::open_read_only(&file).unwrap();
        assert_eq!(opened.schema_on_disk, 1);
        assert_eq!(
            rows(&opened.store.conn, "PRAGMA temp_store"),
            [[Value::Integer(2)]]
        );
        assert_eq!(opened.store.schema_version().unwrap(), 5);
        assert_eq!(master(&opened.store.conn), recorded("v5.json").1);
        assert_eq!(opened.store.accounts().unwrap().len(), 3);
        assert_eq!(opened.store.installed_games().unwrap().len(), 1);
        assert_eq!(opened.store.ciphertexts().unwrap().passwords.len(), 1);
        assert_eq!(opened.store.row_counts().unwrap().launcher_sessions, 0);
        assert!(
            before == snapshot(scratch.path()),
            "the folder changed while the database was open"
        );
    }
    assert!(before == snapshot(scratch.path()), "the folder changed");
    assert_eq!(version(&read_only(&file)), 1);
}

#[test]
fn a_folder_with_an_odd_name_opens_both_ways() {
    let scratch = Scratch::new("store");
    for name in [
        "a b",
        "50%25 off",
        "tag #1",
        "what%3fnow",
        "ünïcødé 日本語",
        "file:x",
        "a&b=c",
    ] {
        let name = name.replace(':', ";");
        let folder = scratch.join(&name);
        fs::create_dir(&folder).unwrap();
        let file = database_path(&folder);
        fs::copy(oracle("fixtures/data-folder/onelaunch.db"), &file).unwrap();
        let before = snapshot(&folder);
        {
            let opened = Store::open_read_only(&file).unwrap();
            assert_eq!(opened.store.accounts().unwrap().len(), 9, "{name}");
        }
        assert!(before == snapshot(&folder), "{name}: the folder changed");
        assert_eq!(snapshot(scratch.path()).len(), before.len() + 1, "{name}");
        let (store, _) = Store::open(&file).unwrap();
        assert_eq!(store.accounts().unwrap().len(), 9, "{name}");
        drop(store);
        fs::remove_dir_all(&folder).unwrap();
    }

    let verbatim = PathBuf::from(format!(r"\\?\{}", scratch.path().display()));
    let file = database_path(&verbatim);
    fs::copy(oracle("fixtures/data-folder/onelaunch.db"), &file).unwrap();
    let opened = Store::open_read_only(&file).unwrap();
    assert_eq!(opened.store.accounts().unwrap().len(), 9);
    let slashes = scratch.path().to_str().unwrap().replace('\\', "/");
    let opened = Store::open_read_only(&database_path(Path::new(&slashes))).unwrap();
    assert_eq!(opened.store.accounts().unwrap().len(), 9);
}

#[test]
fn a_database_in_wal_mode_is_read_without_a_journal_being_made() {
    for fixture in [
        "fixtures/db/onelaunch-v1-oldest.db",
        "fixtures/data-folder/onelaunch.db",
    ] {
        let scratch = Scratch::new("store");
        let file = copy_of(&scratch, fixture);
        let on_disk = {
            let conn = Connection::open(&file).unwrap();
            conn.pragma_update(None, "journal_mode", "WAL").unwrap();
            version(&conn)
        };
        assert_eq!(
            fs::read(&file).unwrap()[18..20],
            [2, 2],
            "the header says WAL"
        );
        let before = snapshot(scratch.path());
        assert_eq!(
            before.len(),
            1,
            "closing the last connection removed the journal"
        );
        {
            let opened = Store::open_read_only(&file).unwrap();
            assert_eq!(i64::from(opened.schema_on_disk), on_disk);
            assert!(!opened.store.accounts().unwrap().is_empty());
            assert!(
                before == snapshot(scratch.path()),
                "a journal was made while the database was open"
            );
        }
        assert!(before == snapshot(scratch.path()), "the folder changed");
    }
}

#[test]
fn changes_still_in_the_journal_are_not_passed_over() {
    let scratch = Scratch::new("store");
    let live = Scratch::new("store");
    let file = database_path(live.path());
    let (store, _) = Store::open(&file).unwrap();
    store.set_setting(SettingKey::DryRun, "true").unwrap();
    for name in [DATABASE_FILE, "onelaunch.db-wal"] {
        fs::copy(live.join(name), scratch.join(name)).unwrap();
    }
    assert!(
        fs::metadata(scratch.join("onelaunch.db-wal"))
            .unwrap()
            .len()
            > 0
    );
    let before = snapshot(scratch.path());

    let error = Store::open_read_only(&database_path(scratch.path()))
        .err()
        .unwrap();
    assert!(
        matches!(error, StoreError::UnfinishedWrite("-wal")),
        "{error}"
    );
    assert!(before == snapshot(scratch.path()), "the folder changed");

    fs::write(scratch.join("onelaunch.db-wal"), b"").unwrap();
    fs::write(scratch.join("onelaunch.db-journal"), b"x").unwrap();
    let error = Store::open_read_only(&database_path(scratch.path()))
        .err()
        .unwrap();
    assert!(
        matches!(error, StoreError::UnfinishedWrite("-journal")),
        "{error}"
    );
}

#[test]
fn a_copy_is_taken_and_the_newest_five_are_kept() {
    let scratch = data_folder();
    let folder = database_backups_dir(scratch.path());
    let (store, _) = Store::open(&database_path(scratch.path())).unwrap();
    assert_eq!(fs::read_dir(&folder).unwrap().count(), 2);
    fs::write(folder.join("notes.txt"), b"kept").unwrap();
    fs::write(folder.join("onelaunch-2020.db-wal"), b"kept").unwrap();

    let first = store
        .backup(
            &folder,
            BACKUPS_TO_KEEP,
            datetime!(2026-12-01 10:20:30.456 UTC),
        )
        .unwrap();
    assert_eq!(first, folder.join("onelaunch-2026-12-01T10-20-30.456Z.db"));
    {
        let copy = Store::open_read_only(&first).unwrap().store.conn;
        assert_eq!(master(&copy), master(&store.conn));
        for table in ["accounts", "games", "secrets", "launcher_sessions"] {
            let sql = format!("SELECT * FROM {table} ORDER BY 1");
            assert!(rows(&copy, &sql) == rows(&store.conn, &sql), "{table}");
        }
    }

    for minute in 21..=24 {
        let now = datetime!(2026-12-01 10:00:00 UTC)
            .replace_minute(minute)
            .unwrap();
        store.backup(&folder, BACKUPS_TO_KEEP, now).unwrap();
    }
    let mut names: Vec<_> = fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "notes.txt",
            "onelaunch-2020.db-wal",
            "onelaunch-2026-12-01T10-20-30.456Z.db",
            "onelaunch-2026-12-01T10-21-00.000Z.db",
            "onelaunch-2026-12-01T10-22-00.000Z.db",
            "onelaunch-2026-12-01T10-23-00.000Z.db",
            "onelaunch-2026-12-01T10-24-00.000Z.db",
        ]
    );

    let copies = || {
        let mut names: Vec<_> = fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.ends_with(".db"))
            .collect();
        names.sort();
        names
    };
    let set_back = store
        .backup(&folder, BACKUPS_TO_KEEP, datetime!(2026-12-01 09:00:00 UTC))
        .unwrap();
    assert!(set_back.exists(), "the copy just made was pruned");
    assert_eq!(
        copies(),
        [
            "onelaunch-2026-12-01T09-00-00.000Z.db",
            "onelaunch-2026-12-01T10-21-00.000Z.db",
            "onelaunch-2026-12-01T10-22-00.000Z.db",
            "onelaunch-2026-12-01T10-23-00.000Z.db",
            "onelaunch-2026-12-01T10-24-00.000Z.db",
        ]
    );

    let last = store
        .backup(&folder, 0, datetime!(2026-12-01 08:00:00 UTC))
        .unwrap();
    assert!(last.exists());
    assert_eq!(
        copies(),
        ["onelaunch-2026-12-01T08-00-00.000Z.db"],
        "at least one copy stays, and it is the newest made"
    );
}

#[test]
fn a_copy_that_fails_leaves_no_file_and_takes_none_away() {
    let scratch = Scratch::new("store");
    let folder = database_backups_dir(scratch.path());
    fs::create_dir_all(&folder).unwrap();
    for minute in 1..=5 {
        let name = format!("onelaunch-2026-12-01T10-0{minute}-00.000Z.db");
        fs::write(folder.join(name), b"an earlier copy").unwrap();
    }
    let before = snapshot(&folder);

    let damaged = scratch.join("damaged.db");
    fs::write(&damaged, [0x5a; 8192]).unwrap();
    let store = Store {
        conn: Connection::open(&damaged).unwrap(),
    };
    let failed = store.backup(&folder, BACKUPS_TO_KEEP, datetime!(2026-12-01 11:00:00 UTC));
    assert!(failed.is_err());
    assert!(
        before == snapshot(&folder),
        "a failed copy changed the folder"
    );

    let (store, _) = Store::open(&database_path(scratch.path())).unwrap();
    let taken = folder.join("onelaunch-2026-12-01T10-05-00.000Z.db");
    let failed = store.backup(&folder, BACKUPS_TO_KEEP, datetime!(2026-12-01 10:05:00 UTC));
    assert!(
        failed.is_err(),
        "a file that is no database was copied over"
    );
    assert_eq!(fs::read(&taken).unwrap(), b"an earlier copy");
    assert!(
        before == snapshot(&folder),
        "a failed copy changed the folder"
    );
}

const EDITED_AT: &str = "2026-11-21T10:00:00.000Z";

fn id_of(store: &Store, launcher: LauncherId, login: &str) -> String {
    store
        .account_by_login(launcher, login)
        .unwrap()
        .expect("the fixture has this account")
        .id
}

fn last_played_as(store: &Store, account_id: &str) -> String {
    store
        .installed_games()
        .unwrap()
        .into_iter()
        .find(|game| {
            game.last_launch
                .as_ref()
                .is_some_and(|last| last.account_id == account_id)
        })
        .expect("the fixture played a game as this account")
        .id
}

fn account_rows(conn: &Connection) -> Vec<Vec<Value>> {
    rows(
        conn,
        "SELECT id, launcher, display_name, login, secret_enc, launcher_uid, tags, notes, last_used_at, created_at, updated_at
         FROM accounts ORDER BY id",
    )
}

fn with_changed(before: &[Vec<Value>], id: &str, changes: &[(usize, Value)]) -> Vec<Vec<Value>> {
    let mut after = before.to_vec();
    let row = after
        .iter_mut()
        .find(|row| row[0] == Value::Text(id.to_owned()))
        .expect("the fixture has this account");
    for (column, value) in changes {
        row[*column] = value.clone();
    }
    after
}

fn text(value: &str) -> Value {
    Value::Text(value.to_owned())
}

#[test]
fn an_edit_writes_the_columns_the_electron_app_writes_and_leaves_every_other_byte() {
    let scratch = data_folder();
    let (store, _) = Store::open(&database_path(scratch.path())).unwrap();
    let persona_one = id_of(&store, LauncherId::Steam, "user_1");
    let before = account_rows(&store.conn);
    let tags = [
        "quote\"d".to_owned(),
        r"back\slash".to_owned(),
        "é".to_owned(),
        "tab\there".to_owned(),
    ];
    store
        .update_account(
            &persona_one,
            &AccountUpdate {
                display_name: "Edited",
                login: "user_1b",
                launcher_uid: None,
                tags: Some(&tags),
                notes: "",
                now: EDITED_AT,
            },
        )
        .unwrap();
    assert_eq!(
        account_rows(&store.conn),
        with_changed(
            &before,
            &persona_one,
            &[
                (2, text("Edited")),
                (3, text("user_1b")),
                (5, Value::Null),
                (6, text(r#"["quote\"d","back\\slash","é","tab\there"]"#)),
                (7, text("")),
                (10, text(EDITED_AT)),
            ],
        )
    );

    store
        .update_account(
            &persona_one,
            &AccountUpdate {
                display_name: "Edited",
                login: "user_1b",
                launcher_uid: Some("uid-one"),
                tags: Some(&[]),
                notes: "line one\nline two",
                now: EDITED_AT,
            },
        )
        .unwrap();
    let [row] = &rows(
        &store.conn,
        &format!("SELECT launcher_uid, tags, notes FROM accounts WHERE id = '{persona_one}'"),
    )[..] else {
        panic!("one row");
    };
    assert_eq!(
        row,
        &[text("uid-one"), text("[]"), text("line one\nline two")]
    );

    let unreadable = r#"["\ud800"]"#;
    store
        .conn
        .execute(
            "UPDATE accounts SET tags = ?1 WHERE id = ?2",
            [unreadable, persona_one.as_str()],
        )
        .unwrap();
    store
        .update_account(
            &persona_one,
            &AccountUpdate {
                display_name: "Edited again",
                login: "user_1b",
                launcher_uid: Some("uid-one"),
                tags: None,
                notes: "line one\nline two",
                now: EDITED_AT,
            },
        )
        .unwrap();
    let [row] = &rows(
        &store.conn,
        &format!("SELECT display_name, tags FROM accounts WHERE id = '{persona_one}'"),
    )[..] else {
        panic!("one row");
    };
    assert_eq!(row, &[text("Edited again"), text(unreadable)]);
}

#[test]
fn a_password_is_set_and_cleared_in_its_column_alone() {
    let scratch = data_folder();
    let (store, _) = Store::open(&database_path(scratch.path())).unwrap();
    let persona_one = id_of(&store, LauncherId::Steam, "user_1");
    let before = account_rows(&store.conn);
    let sealed = Ciphertext::new(vec![0x76, 0x31, 0x30, 0x00, 0xff]);
    store
        .set_account_password(&persona_one, Some(&sealed), EDITED_AT)
        .unwrap();
    assert_eq!(
        account_rows(&store.conn),
        with_changed(
            &before,
            &persona_one,
            &[
                (4, Value::Blob(sealed.as_bytes().to_vec())),
                (10, text(EDITED_AT)),
            ],
        )
    );
    assert!(store.account_password(&persona_one).unwrap() == Some(sealed));
    assert!(store.account(&persona_one).unwrap().unwrap().has_secret);

    let cleared_at = "2026-11-21T10:00:01.000Z";
    store
        .set_account_password(&persona_one, None, cleared_at)
        .unwrap();
    assert_eq!(
        account_rows(&store.conn),
        with_changed(&before, &persona_one, &[(10, text(cleared_at))])
    );
    assert!(store.account_password(&persona_one).unwrap().is_none());
}

#[test]
fn a_deleted_account_takes_its_saved_sign_in_and_leaves_its_launches() {
    let scratch = data_folder();
    let (store, _) = Store::open(&database_path(scratch.path())).unwrap();
    let epic_alt = id_of(&store, LauncherId::Epic, "alt@example.com");
    let game = last_played_as(&store, &epic_alt);
    let before = account_rows(&store.conn);
    let history = rows(&store.conn, "SELECT * FROM launch_history ORDER BY id");
    let sessions = rows(
        &store.conn,
        &format!(
            "SELECT * FROM launcher_sessions WHERE account_id <> '{epic_alt}' ORDER BY account_id"
        ),
    );
    assert_eq!(sessions.len(), 2);
    assert!(store.session(&epic_alt).unwrap().is_some());
    let played = store.game(&game).unwrap().unwrap().last_launch.unwrap();
    assert_eq!(played.account_name.as_deref(), Some("Alt"));
    assert!(
        store
            .recent_launches(50)
            .unwrap()
            .iter()
            .any(|recent| recent.account_id == epic_alt)
    );

    assert!(store.delete_account(&epic_alt).unwrap());

    let mut expected = before;
    expected.retain(|row| row[0] != text(&epic_alt));
    assert_eq!(account_rows(&store.conn), expected);
    assert_eq!(
        rows(
            &store.conn,
            "SELECT * FROM launcher_sessions ORDER BY account_id"
        ),
        sessions
    );
    assert!(store.session_blob(&epic_alt).unwrap().is_none());
    assert_eq!(
        rows(&store.conn, "SELECT * FROM launch_history ORDER BY id"),
        history
    );
    let played = store.game(&game).unwrap().unwrap().last_launch.unwrap();
    assert_eq!(played.account_id, epic_alt);
    assert_eq!(played.account_name, None);
    assert!(
        store
            .recent_launches(50)
            .unwrap()
            .iter()
            .all(|recent| recent.account_id != epic_alt)
    );
    assert!(!store.delete_account(&epic_alt).unwrap());
}

#[test]
fn a_forgotten_sign_in_takes_its_row_alone() {
    let scratch = data_folder();
    let (store, _) = Store::open(&database_path(scratch.path())).unwrap();
    let epic_alt = id_of(&store, LauncherId::Epic, "alt@example.com");
    let accounts = account_rows(&store.conn);
    let history = rows(&store.conn, "SELECT * FROM launch_history ORDER BY id");
    let others = rows(
        &store.conn,
        &format!(
            "SELECT * FROM launcher_sessions WHERE account_id <> '{epic_alt}' ORDER BY account_id"
        ),
    );
    assert_eq!(others.len(), 2);
    assert!(store.session(&epic_alt).unwrap().is_some());

    assert!(store.delete_session(&epic_alt).unwrap());

    assert!(store.session(&epic_alt).unwrap().is_none());
    assert!(store.session_blob(&epic_alt).unwrap().is_none());
    assert_eq!(
        rows(
            &store.conn,
            "SELECT * FROM launcher_sessions ORDER BY account_id"
        ),
        others
    );
    assert_eq!(account_rows(&store.conn), accounts);
    assert_eq!(
        rows(&store.conn, "SELECT * FROM launch_history ORDER BY id"),
        history
    );
    assert!(!store.delete_session(&epic_alt).unwrap());
    assert!(!store.delete_session("no-such-account").unwrap());
}
