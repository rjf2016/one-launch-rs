#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;

use futures_lite::future::block_on;
use ol_api::library::visible;
use ol_api::{ApiError, Backend};
use ol_model::{LibrarySort, SettingKey, Sidebar};
use ol_store::{SCHEMA_VERSION, Store, database_backups_dir, database_path};
use ol_testkit::{Scratch, oracle, snapshot};

const VERSION: &str = "9.8.7";

fn fixture() -> Scratch {
    Scratch::with_copy_of("api", &oracle("fixtures/data-folder"))
}

fn files(folder: &Path) -> Vec<String> {
    snapshot(folder).into_keys().collect()
}

fn seen(data_dir: &Path) -> Option<String> {
    let opened = Store::open_read_only(&database_path(data_dir)).unwrap();
    opened.store.setting(SettingKey::WhatsNewLastSeen).unwrap()
}

#[test]
fn the_fixture_folder_hands_over_its_games_and_its_accounts() {
    let scratch = fixture();
    let backend = Backend::start(scratch.path().to_path_buf(), VERSION);
    let library = block_on(backend.library()).unwrap();
    assert_eq!(library.games.len(), 8);
    assert_eq!(library.sort, LibrarySort::LastPlayed);
    let artwork = scratch.join("artwork");
    let grids: Vec<_> = library
        .games
        .iter()
        .filter_map(|entry| entry.grid.as_deref())
        .collect();
    assert_eq!(grids.len(), 5);
    for grid in grids {
        assert!(grid.starts_with(&artwork));
        assert!(grid.is_file());
    }
    let shown = visible(&library.games, "", None, library.sort);
    assert_eq!(shown.len(), 8);
    let played = shown
        .iter()
        .take_while(|index| {
            let game = &library.games[**index].game;
            game.last_played_at.is_some() || game.last_launch.is_some()
        })
        .count();
    assert!(played > 0);
    assert!(shown[played..].iter().all(|index| {
        let game = &library.games[*index].game;
        game.last_played_at.is_none() && game.last_launch.is_none()
    }));
    assert_eq!(block_on(backend.accounts()).unwrap().len(), 9);
    backend.shut_down();
}

#[test]
fn a_folder_that_is_not_there_yet_gets_its_folders_and_a_database() {
    let scratch = Scratch::new("api");
    let data_dir = scratch.join("fresh").join("OneLaunch");
    let backend = Backend::start(data_dir.clone(), VERSION);
    let library = block_on(backend.library()).unwrap();
    assert!(library.games.is_empty());
    assert_eq!(library.sort, LibrarySort::LastPlayed);
    assert!(block_on(backend.accounts()).unwrap().is_empty());
    backend.shut_down();

    assert_eq!(
        files(&data_dir),
        ["artwork/", "backups/", "logs/", "onelaunch.db"],
        "closing folds the journal back into the one file"
    );
    let opened = Store::open_read_only(&database_path(&data_dir)).unwrap();
    assert_eq!(opened.schema_on_disk, SCHEMA_VERSION);
    assert_eq!(opened.store.row_counts().unwrap().settings, 1);
    assert_eq!(seen(&data_dir).as_deref(), Some(VERSION));
}

#[test]
fn a_database_that_was_there_keeps_what_it_has_seen() {
    let scratch = fixture();
    let before = seen(scratch.path());
    assert!(before.is_some());
    let backend = Backend::start(scratch.path().to_path_buf(), VERSION);
    block_on(backend.library()).unwrap();
    backend.shut_down();
    assert_eq!(seen(scratch.path()), before);

    let empty = Scratch::new("api");
    fs::copy(
        oracle("fixtures/db/onelaunch-empty-v5.db"),
        database_path(empty.path()),
    )
    .unwrap();
    let backend = Backend::start(empty.path().to_path_buf(), VERSION);
    block_on(backend.library()).unwrap();
    backend.shut_down();
    assert_eq!(seen(empty.path()), None);
}

#[test]
fn the_sort_that_was_chosen_is_there_after_a_restart() {
    let scratch = fixture();
    let backend = Backend::start(scratch.path().to_path_buf(), VERSION);
    block_on(backend.set_library_sort(LibrarySort::Name)).unwrap();
    backend.shut_down();
    let backend = Backend::start(scratch.path().to_path_buf(), VERSION);
    let library = block_on(backend.library()).unwrap();
    assert_eq!(library.sort, LibrarySort::Name);
    let names: Vec<_> = visible(&library.games, "", None, library.sort)
        .into_iter()
        .map(|index| library.games[index].game.name.to_lowercase())
        .collect();
    assert!(names.is_sorted());
    backend.shut_down();
}

#[test]
fn the_sidebar_that_was_opened_is_open_after_a_restart_and_folded_again_after_the_next() {
    let scratch = fixture();
    let restart = |sidebar: Option<Sidebar>| {
        let backend = Backend::start(scratch.path().to_path_buf(), VERSION);
        let read = block_on(backend.sidebar()).unwrap();
        if let Some(sidebar) = sidebar {
            block_on(backend.set_sidebar(sidebar)).unwrap();
        }
        backend.shut_down();
        read
    };
    assert_eq!(restart(Some(Sidebar::Open)), Sidebar::Folded);
    assert_eq!(restart(Some(Sidebar::Folded)), Sidebar::Open);
    assert_eq!(restart(None), Sidebar::Folded);
}

#[test]
fn the_copy_of_the_database_lands_in_its_folder() {
    let scratch = fixture();
    let copies = || {
        fs::read_dir(database_backups_dir(scratch.path()))
            .unwrap()
            .count()
    };
    assert_eq!(copies(), 2);
    let backend = Backend::start(scratch.path().to_path_buf(), VERSION);
    block_on(backend.back_up_database()).unwrap();
    backend.shut_down();
    assert_eq!(copies(), 3);
}

#[test]
fn a_database_that_cannot_be_opened_is_the_answer_to_every_request() {
    let too_new = Scratch::new("api");
    let mut bytes = fs::read(oracle("fixtures/db/onelaunch-empty-v5.db")).unwrap();
    // The schema version is the four bytes at 60 of the file's header.
    bytes[60..64].copy_from_slice(&6_u32.to_be_bytes());
    fs::write(database_path(too_new.path()), &bytes).unwrap();
    let damaged = Scratch::new("api");
    fs::write(database_path(damaged.path()), [0x5a; 8192]).unwrap();

    for (scratch, code, message) in [
        (
            &too_new,
            "DB_TOO_NEW",
            "This database was created by a newer version of One Launch (schema 6; this version supports up to 5). Update One Launch, or restore an older copy of the database.",
        ),
        (
            &damaged,
            "DB_CORRUPT",
            "The One Launch database is damaged (file is not a database). Restore a copy from the backups\\db folder in the data directory, or move the file aside to start over.",
        ),
    ] {
        let kept = fs::read(database_path(scratch.path())).unwrap();
        let backend = Backend::start(scratch.path().to_path_buf(), VERSION);
        let expected = ApiError {
            code,
            message: message.to_owned(),
        };
        assert_eq!(block_on(backend.library()).err(), Some(expected.clone()));
        assert_eq!(block_on(backend.accounts()).err(), Some(expected.clone()));
        assert_eq!(
            block_on(backend.set_library_sort(LibrarySort::Name)).err(),
            Some(expected.clone())
        );
        assert_eq!(
            block_on(backend.back_up_database()).err(),
            Some(expected.clone())
        );
        assert_eq!(expected.to_string(), message);
        backend.shut_down();
        // Opening switches the file to the write-ahead journal before it looks at the version, as the Electron app does; nothing past the header may move.
        let after = fs::read(database_path(scratch.path())).unwrap();
        assert!(after[100..] == kept[100..]);
        assert!(after[60..64] == kept[60..64]);
        assert!(!database_backups_dir(scratch.path()).exists());
    }
}

#[test]
fn a_request_after_the_end_is_refused_and_does_not_wait() {
    let scratch = Scratch::new("api");
    let backend = Backend::start(scratch.path().to_path_buf(), VERSION);
    let other = backend.clone();
    backend.shut_down();
    backend.shut_down();
    let refused = block_on(other.library()).err().unwrap();
    assert_eq!(refused.code, "INTERNAL");
}
