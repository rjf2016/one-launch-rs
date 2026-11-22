#![forbid(unsafe_code)]

// The settings table after each kind of write, on a copy of the fixture data folder: the rows the Electron app stores, and no other row changed.

use std::collections::BTreeMap;
use std::path::Path;

use futures_lite::future::block_on;
use ol_api::{ApiError, Backend, SettingChange};
use ol_model::{LauncherId, LibrarySort, LogLevel, SettingKey, Sidebar};
use ol_store::{Store, database_path};
use ol_testkit::{Scratch, oracle};

const VERSION: &str = "9.8.7";

// What the backend said it changed, taken when the test looks.
struct Heard(async_channel::Receiver<SettingChange>);

impl Heard {
    fn taken(&self) -> Vec<SettingChange> {
        std::iter::from_fn(|| self.0.try_recv().ok()).collect()
    }
}

fn rows(data_dir: &Path) -> BTreeMap<String, String> {
    Store::open_read_only(&database_path(data_dir))
        .unwrap()
        .store
        .settings_rows()
        .unwrap()
        .into_iter()
        .collect()
}

// The fixture with two rows of this app's own beside the Electron app's nineteen keys, which the Electron app neither reads nor returns.
fn fixture() -> Scratch {
    let scratch = Scratch::with_copy_of("api-settings", &oracle("fixtures/data-folder"));
    let (store, _) = Store::open(&database_path(scratch.path())).unwrap();
    store.set_library_sort(LibrarySort::Name).unwrap();
    store.set_sidebar(Sidebar::Open).unwrap();
    drop(store);
    scratch
}

fn listening(backend: &Backend) -> Heard {
    Heard(backend.setting_changes())
}

fn refusal<T>(answer: Result<T, ApiError>) -> (&'static str, String) {
    let error = answer.err().expect("a refusal");
    (error.code, error.message)
}

#[test]
fn each_write_stores_the_row_the_electron_app_stores_and_changes_no_other() {
    let scratch = fixture();
    let data_dir = scratch.path().to_path_buf();
    let before = rows(&data_dir);
    assert_eq!(before.get("logLevel").map(String::as_str), Some("debug"));
    assert!(!before.contains_key("dryRun"));

    let backend = Backend::start(data_dir.clone(), VERSION);
    let heard = listening(&backend);
    let writes = [
        (SettingKey::DryRun, "true"),
        (SettingKey::LogLevel, "info"),
        (SettingKey::BackupsKeep, "7"),
        (SettingKey::RiotPressPlay, "false"),
        (SettingKey::EpicLaunchUri, "appname"),
        (SettingKey::BattlenetLaunch, "uri"),
        (SettingKey::WhatsNewLastSeen, "2.0.0-beta.1"),
        (SettingKey::WindowCloseToTray, "false"),
        (SettingKey::DryRun, "false"),
    ];
    for (key, value) in writes {
        block_on(backend.set_setting(key, value.to_owned())).unwrap();
    }
    let settings = block_on(backend.settings()).unwrap();
    backend.shut_down();

    let mut expected = before;
    for (key, value) in writes {
        expected.insert(key.as_str().to_owned(), value.to_owned());
    }
    assert_eq!(rows(&data_dir), expected);
    assert_eq!(expected["library.sort"], "name");
    assert_eq!(expected["sidebar"], "expanded");
    assert_eq!(
        heard.taken(),
        writes
            .iter()
            .map(|(key, value)| (*key, Some((*value).to_owned())))
            .collect::<Vec<_>>()
    );

    assert_eq!(settings.len(), 12);
    assert!(!settings.toggle(SettingKey::DryRun));
    assert!(!settings.toggle(SettingKey::RiotPressPlay));
    assert!(!settings.toggle(SettingKey::WindowCloseToTray));
    assert!(settings.toggle(SettingKey::PlayHideToTray));
    assert!(settings.toggle(SettingKey::UpdatesAuto));
    assert!(settings.toggle(SettingKey::FirstRunDismissed));
    assert!(!settings.toggle(SettingKey::SteamAutoLogin));
    assert_eq!(settings.log_level(), LogLevel::Info);
    assert_eq!(settings.backups_keep(), 7);
    assert_eq!(settings.backups_keep_choices(), [5, 7, 10, 20, 50, 100]);
    assert_eq!(settings.epic_launch_uri(), "appname");
    assert_eq!(settings.battlenet_launch(), "uri");
    assert_eq!(
        settings.launcher_path(LauncherId::Steam),
        Some(r"D:\Games\Steam")
    );
    assert_eq!(
        settings.get(SettingKey::WhatsNewLastSeen),
        Some("2.0.0-beta.1")
    );
}

#[test]
fn each_rule_refuses_in_the_electron_apps_words_and_nothing_is_stored() {
    let scratch = fixture();
    let data_dir = scratch.path().to_path_buf();
    let before = rows(&data_dir);
    let backend = Backend::start(data_dir.clone(), VERSION);
    let heard = listening(&backend);
    let refused = [
        (
            SettingKey::SteamPath,
            "D:\\Elsewhere",
            "steam.path is not user-settable",
        ),
        (
            SettingKey::WindowBounds,
            "{}",
            "window.bounds is not user-settable",
        ),
        (SettingKey::DryRun, "yes", "dryRun must be true or false"),
        (
            SettingKey::WindowCloseToTray,
            "1",
            "window.closeToTray must be true or false",
        ),
        (
            SettingKey::LogLevel,
            "verbose",
            "logLevel must be debug, info, warn, or error",
        ),
        (
            SettingKey::EpicLaunchUri,
            "uri",
            "epic.launchUri must be catalog or appname",
        ),
        (
            SettingKey::BattlenetLaunch,
            "catalog",
            "battlenet.launch must be uri or exec",
        ),
        (
            SettingKey::WhatsNewLastSeen,
            "v1.0.0",
            "whatsNew.lastSeen must be a version",
        ),
        (
            SettingKey::BackupsKeep,
            "0",
            "backups.keep must be a whole number from 1 to 200",
        ),
        (
            SettingKey::BackupsKeep,
            "201",
            "backups.keep must be a whole number from 1 to 200",
        ),
    ];
    for (key, value, message) in refused {
        assert_eq!(
            refusal(block_on(backend.set_setting(key, value.to_owned()))),
            ("BAD_REQUEST", message.to_owned()),
            "{key:?} {value}"
        );
    }
    assert_eq!(
        refusal(block_on(
            backend.set_setting(SettingKey::LogLevel, "x".repeat(4097))
        )),
        ("BAD_REQUEST", "value exceeds 4096 characters".to_owned())
    );
    backend.shut_down();
    assert_eq!(rows(&data_dir), before);
    assert!(heard.taken().is_empty());
}

#[test]
fn a_fresh_folder_reads_every_default() {
    let scratch = Scratch::new("api-settings");
    let backend = Backend::start(scratch.join("data"), VERSION);
    let settings = block_on(backend.settings()).unwrap();
    backend.shut_down();
    assert_eq!(
        settings.keys().collect::<Vec<_>>(),
        [SettingKey::WhatsNewLastSeen]
    );
    assert_eq!(settings.get(SettingKey::WhatsNewLastSeen), Some(VERSION));
    for key in SettingKey::TOGGLES {
        let on = !matches!(
            key,
            SettingKey::DryRun | SettingKey::SteamAutoLogin | SettingKey::FirstRunDismissed
        );
        assert_eq!(settings.toggle(key), on, "{key:?}");
    }
    assert_eq!(settings.log_level(), LogLevel::Info);
    assert_eq!(settings.backups_keep(), 20);
    assert_eq!(settings.backups_keep_choices(), [5, 10, 20, 50, 100]);
    assert_eq!(settings.epic_launch_uri(), "catalog");
    assert_eq!(settings.battlenet_launch(), "exec");
    assert_eq!(settings.launcher_path(LauncherId::Riot), None);
}

#[test]
// As the Electron app's seedableSettingKey: a key only the app sets is passed over, any other is stored as it is given.
fn the_fake_launchers_seed_is_stored_as_it_is_given_but_for_the_keys_only_the_app_sets() {
    let scratch = fixture();
    let data_dir = scratch.path().to_path_buf();
    let mut expected = rows(&data_dir);
    let backend = Backend::start(data_dir.clone(), VERSION);
    let heard = listening(&backend);
    block_on(backend.seed_setting(SettingKey::EpicPath, r"D:\Seeded".to_owned())).unwrap();
    block_on(backend.seed_setting(SettingKey::DryRun, "1".to_owned())).unwrap();
    let settings = block_on(backend.settings()).unwrap();
    backend.shut_down();
    expected.insert("dryRun".to_owned(), "1".to_owned());
    assert_eq!(rows(&data_dir), expected);
    assert!(heard.taken().is_empty());
    assert!(settings.toggle(SettingKey::DryRun));
}

// `window.bounds` is the app's own: refused to settings:set, stored by the window's own call, with nothing said to the settings' listeners, and stored before the backend shuts down even when it was handed over just before.
#[test]
fn the_windows_place_is_stored_by_the_app_alone_and_as_given() {
    let scratch = fixture();
    let data_dir = scratch.path().to_path_buf();
    let before = rows(&data_dir);
    let backend = Backend::start(data_dir.clone(), VERSION);
    let heard = listening(&backend);
    let refused = refusal(block_on(
        backend.set_setting(SettingKey::WindowBounds, "{}".to_owned()),
    ));
    assert_eq!(
        refused,
        (
            "BAD_REQUEST",
            "window.bounds is not user-settable".to_owned()
        )
    );
    let place = ol_api::settings::bounds_setting(-1800, 40, 1280, 800, true);
    backend.remember_window_bounds(place.clone());
    backend.shut_down();
    let after = rows(&data_dir);
    assert_eq!(after.get("window.bounds"), Some(&place));
    let mut others = after.clone();
    others.remove("window.bounds");
    let mut was = before.clone();
    was.remove("window.bounds");
    assert_eq!(others, was);
    assert!(heard.taken().is_empty());
}
