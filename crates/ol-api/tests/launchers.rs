#![forbid(unsafe_code)]

// The backend's launcher commands on fake launchers: what each answers, and what each refuses.

use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

use futures_lite::future::block_on;
use ol_api::launchers::{Launchers, NewAccount, Secret, SteamTiming};
use ol_api::{ApiError, Backend, StartLaunch};
use ol_model::{JobState, JobStepStatus, LaunchJob, LauncherId, SettingKey};
use ol_sim::FakeLaunchers;
use ol_testkit::vectors::Vectors;
use ol_testkit::{Scratch, oracle, snapshot};
use ol_vault::FakeVault;
use serde_json::{Value, json};
use time::macros::datetime;

const VERSION: &str = "9.8.7";

struct Fakes {
    scratch: Scratch,
    fake: FakeLaunchers,
    backend: Backend,
    jobs: Receiver<LaunchJob>,
}

impl Fakes {
    fn launchers_dir(&self) -> PathBuf {
        self.scratch.join("launchers")
    }

    fn account(&self, login: &str) -> String {
        block_on(self.backend.accounts())
            .unwrap()
            .into_iter()
            .find(|account| account.launcher == LauncherId::Steam && account.login == login)
            .map(|account| account.id)
            .expect("the scenario seeds this account")
    }

    fn game(&self) -> String {
        let library = block_on(self.backend.library()).unwrap();
        library.games[0].game.id.clone()
    }

    fn finished(&self) -> LaunchJob {
        loop {
            let job = self
                .jobs
                .recv()
                .expect("the job ends before its observer goes");
            if job.state != JobState::Running {
                return job;
            }
        }
    }

    fn seed_password(&self, login: &str) -> String {
        self.fake
            .seed
            .accounts
            .iter()
            .find(|account| account.login == login)
            .and_then(|account| account.password.clone())
            .expect("the scenario gives this account a password")
    }
}

// The scenario all-four laid down in a scratch folder, its Steam accounts seeded, the library not yet scanned.
fn fakes(knobs: Option<&Value>) -> Fakes {
    let scratch = Scratch::new("api-launchers");
    let launchers_dir = scratch.join("launchers");
    let data_dir = scratch.join("data");
    ol_sim::lay_down(
        &oracle("fixtures/dev-only/fake-launchers/all-four"),
        &oracle("fixtures/launchers"),
        &launchers_dir,
    )
    .unwrap();
    if let Some(knobs) = knobs {
        ol_sim::write_knobs(&launchers_dir, knobs).unwrap();
    }
    let fake = ol_sim::materialize(
        &launchers_dir,
        &data_dir,
        datetime!(2026-11-06 16:46:23 UTC),
    )
    .unwrap();
    let mut launchers = Launchers::on(fake.platform.clone(), Arc::new(FakeVault));
    launchers.steam_timing = SteamTiming::with(&fake.steam_timing);
    launchers
        .steam_default_dir
        .clone_from(&fake.steam_default_dir);
    let (changed, jobs) = channel();
    let changed = Mutex::new(changed);
    let backend = Backend::start_with(
        data_dir,
        VERSION,
        launchers,
        Some(Arc::new(move |job: &LaunchJob| {
            let _ = changed.lock().unwrap().send(job.clone());
        })),
    );
    for account in &fake.seed.accounts {
        if account.launcher == LauncherId::Steam {
            block_on(backend.create_account(NewAccount {
                launcher: account.launcher,
                display_name: account.display_name.clone(),
                login: account.login.clone(),
                launcher_uid: account.launcher_uid.clone(),
                password: account.password.clone().map(Secret::new),
                tags: Vec::new(),
                notes: String::new(),
            }))
            .unwrap();
        }
    }
    Fakes {
        scratch,
        fake,
        backend,
        jobs,
    }
}

fn refusal<T>(answer: Result<T, ApiError>) -> (&'static str, String) {
    let error = answer.err().expect("a refusal");
    (error.code, error.message)
}

// The game the recorded run of the Electron app found in the same scenario.
fn recorded_steam_game() -> Value {
    let expected: Value =
        serde_json::from_str(&fs::read_to_string(oracle("l2/expected/play-steam.json")).unwrap())
            .unwrap();
    expected["final"]["db"]["tables"]["games"]
        .as_array()
        .unwrap()
        .iter()
        .find(|game| game["launcher"] == "steam")
        .unwrap()
        .clone()
}

// The accounts the Electron app reads from the loginusers.vdf the scenario lays down.
fn recorded_login_users() -> Vec<Value> {
    Vectors::load("l1/steam/parseLoginUsers.json")
        .cases
        .iter()
        .find(|case| case.id == "fixture")
        .and_then(ol_testkit::vectors::Case::out)
        .and_then(|out| out.as_array().cloned())
        .expect("the fixture case lists its users")
}

#[test]
fn without_launchers_nothing_is_scanned_and_every_launcher_command_says_so() {
    let scratch = Scratch::with_copy_of("api-launchers", &oracle("fixtures/data-folder"));
    let backend = Backend::start(scratch.path().to_path_buf(), VERSION);
    let before = block_on(backend.library()).unwrap().games.len();
    assert!(block_on(backend.rescan()).unwrap().is_empty());
    assert_eq!(block_on(backend.library()).unwrap().games.len(), before);
    assert_eq!(
        refusal(block_on(backend.remembered_accounts(LauncherId::Steam))).0,
        "NOT_IMPLEMENTED"
    );
    let account = block_on(backend.accounts()).unwrap().remove(0).id;
    let start = StartLaunch {
        account_id: account,
        game_id: None,
        confirm_close: true,
    };
    assert_eq!(
        refusal(block_on(backend.start_launch(start))).0,
        "NOT_IMPLEMENTED"
    );
    backend.shut_down();
}

#[test]
fn a_scan_brings_steams_games_into_the_library_and_a_missing_steam_takes_them_out() {
    let fakes = fakes(None);
    let recorded = recorded_steam_game();
    assert!(block_on(fakes.backend.library()).unwrap().games.is_empty());

    let scanned = block_on(fakes.backend.rescan()).unwrap();
    assert_eq!(scanned.len(), 1);
    let steam = &scanned[0];
    assert_eq!(
        (
            steam.launcher,
            steam.added,
            steam.updated,
            steam.removed,
            steam.installed,
            &steam.error
        ),
        (LauncherId::Steam, 1, 0, 0, true, &None)
    );
    let library = block_on(fakes.backend.library()).unwrap();
    assert_eq!(library.games.len(), 1);
    let game = &library.games[0].game;
    assert_eq!(game.name, recorded["name"].as_str().unwrap());
    assert_eq!(
        game.launcher_game_id,
        recorded["launcher_game_id"].as_str().unwrap()
    );
    assert_eq!(game.needs_update, recorded["needs_update"] == 1);
    assert_eq!(
        game.last_played_at.as_deref(),
        recorded["last_played_at"].as_str()
    );
    let id = game.id.clone();

    let again = &block_on(fakes.backend.rescan()).unwrap()[0];
    assert_eq!((again.added, again.updated, again.removed), (0, 1, 0));
    assert_eq!(
        block_on(fakes.backend.library()).unwrap().games[0].game.id,
        id
    );

    let from_files = block_on(fakes.backend.installed_games(LauncherId::Steam)).unwrap();
    assert_eq!(from_files.len(), 1);
    assert_eq!(from_files[0].launcher_game_id, game.launcher_game_id);

    fs::remove_file(fakes.launchers_dir().join("steam").join("steam.exe")).unwrap();
    let gone = &block_on(fakes.backend.rescan()).unwrap()[0];
    assert_eq!(
        (gone.removed, gone.installed, &gone.error),
        (1, false, &None)
    );
    assert!(block_on(fakes.backend.library()).unwrap().games.is_empty());
    assert_eq!(
        refusal(block_on(fakes.backend.installed_games(LauncherId::Steam))),
        (
            "NOT_INSTALLED",
            "Steam: steam is not installed on this PC".to_owned()
        )
    );
    fakes.backend.shut_down();
}

#[test]
fn the_accounts_steam_remembers_are_listed_and_imported_once() {
    let fakes = fakes(None);
    let recorded = recorded_login_users();
    let remembered = block_on(fakes.backend.remembered_accounts(LauncherId::Steam)).unwrap();
    assert_eq!(remembered.len(), recorded.len());
    assert!(remembered[0].most_recent);
    for user in &recorded {
        let entry = remembered
            .iter()
            .find(|entry| Some(entry.launcher_uid.as_str()) == user["steamId64"].as_str())
            .expect("every remembered account is listed");
        assert_eq!(Some(entry.login.as_str()), user["accountName"].as_str());
        assert_eq!(
            Some(entry.persona_name.as_str()),
            user["personaName"].as_str()
        );
        assert_eq!(Some(entry.most_recent), user["mostRecent"].as_bool());
        assert_eq!(
            Some(entry.remember_password),
            user["rememberPassword"].as_bool()
        );
    }
    let linked = remembered
        .iter()
        .filter(|entry| entry.linked_account_id.is_some())
        .count();
    let seeded = block_on(fakes.backend.accounts()).unwrap().len();
    assert_eq!(linked, seeded);

    let new: Vec<String> = remembered
        .iter()
        .filter(|entry| entry.linked_account_id.is_none())
        .map(|entry| entry.launcher_uid.clone())
        .collect();
    assert!(!new.is_empty());
    let mut keys = new.clone();
    keys.push("no-such-account".to_owned());
    keys.push(new[0].clone());
    let imported = block_on(fakes.backend.import_accounts(LauncherId::Steam, keys)).unwrap();
    assert_eq!(imported.accounts.len(), new.len());
    for account in &imported.accounts {
        let entry = remembered
            .iter()
            .find(|entry| Some(&entry.launcher_uid) == account.launcher_uid.as_ref())
            .unwrap();
        assert!(account.display_name == entry.persona_name);
        assert!(account.login == entry.login);
        assert!(!account.has_secret);
    }
    assert!(
        imported.skipped
            == [(
                "no-such-account".to_owned(),
                "not remembered by the launcher"
            )]
    );
    assert_eq!(
        block_on(fakes.backend.accounts()).unwrap().len(),
        seeded + new.len()
    );

    let again = block_on(
        fakes
            .backend
            .import_accounts(LauncherId::Steam, new.clone()),
    )
    .unwrap();
    assert!(again.accounts.is_empty());
    assert!(
        again
            .skipped
            .iter()
            .all(|(_, reason)| *reason == "already in One Launch")
    );
    assert_eq!(again.skipped.len(), new.len());

    assert_eq!(
        refusal(block_on(
            fakes.backend.remembered_accounts(LauncherId::Epic)
        )),
        (
            "NOT_IMPLEMENTED",
            "The Epic Games adapter is not available yet".to_owned()
        )
    );
    fakes.backend.shut_down();
}

#[test]
fn a_switch_that_would_close_steam_asks_first_and_preview_mode_refuses_any() {
    let running = json!({ "knobs": { "steam": { "runningAs": "user_1" } } });
    let fakes = fakes(Some(&running));
    block_on(fakes.backend.rescan()).unwrap();
    let start = |confirm_close| StartLaunch {
        account_id: fakes.account("user_3"),
        game_id: Some(fakes.game()),
        confirm_close,
    };

    let preflight = block_on(
        fakes
            .backend
            .launch_preflight(&fakes.account("user_3"), Some(&fakes.game())),
    )
    .unwrap();
    assert!(
        preflight.launcher_running && preflight.needs_confirmation && !preflight.already_active
    );
    assert_eq!(
        preflight.close_message.as_deref(),
        Some("Steam is running. Switching closes it.")
    );
    assert_eq!(
        refusal(block_on(fakes.backend.start_launch(start(false)))),
        (
            "CONFIRM_REQUIRED",
            "Steam: Steam is running. Switching closes it.".to_owned()
        )
    );

    block_on(
        fakes
            .backend
            .set_setting(SettingKey::DryRun, "true".to_owned()),
    )
    .unwrap();
    assert_eq!(
        refusal(block_on(fakes.backend.start_launch(start(true)))).0,
        "DRY_RUN"
    );
    assert!(
        fakes.fake.log.snapshot().is_empty(),
        "a refusal touches nothing"
    );
    assert_eq!(
        refusal(fakes.backend.launch_status("no-such-job")).0,
        "NOT_FOUND"
    );
    fakes.backend.shut_down();
}

#[test]
fn a_stored_password_goes_to_steams_command_line_and_nowhere_else() {
    let expired = json!({ "knobs": { "steam": { "expiredSessions": ["user_3"] } } });
    let fakes = fakes(Some(&expired));
    block_on(fakes.backend.rescan()).unwrap();
    block_on(
        fakes
            .backend
            .set_setting(SettingKey::SteamAutoLogin, "true".to_owned()),
    )
    .unwrap();
    let password = fakes.seed_password("user_3");
    let heard = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&heard);
    fakes
        .fake
        .processes
        .on_start(move |_, args| sink.lock().unwrap().push(args.to_vec()));

    let start = StartLaunch {
        account_id: fakes.account("user_3"),
        game_id: Some(fakes.game()),
        confirm_close: false,
    };
    let started = block_on(fakes.backend.start_launch(start)).unwrap();
    let job = fakes.finished();
    assert_eq!(job.id, started.id);
    assert!(job.state == JobState::Done, "{:?}", job.error);
    let login = job.steps.iter().find(|step| step.id == "login").unwrap();
    assert!(login.status == JobStepStatus::Done && login.detail == "Steam started with -login.");
    assert_eq!(
        fakes.backend.launch_status(&job.id).unwrap().state,
        JobState::Done
    );

    let logins: Vec<Vec<String>> = heard
        .lock()
        .unwrap()
        .iter()
        .filter(|args| args.first().map(String::as_str) == Some("-login"))
        .cloned()
        .collect();
    assert_eq!(logins.len(), 1);
    assert!(logins[0][1..] == ["user_3".to_owned(), password.clone()]);
    let logged = serde_json::to_string(&fakes.fake.log.snapshot()).unwrap();
    assert!(logged.contains("<password>"));
    assert!(!logged.contains(&password));
    let told: String = job
        .steps
        .iter()
        .map(|step| format!("{} {}", step.title, step.detail))
        .chain(job.error.clone())
        .collect();
    assert!(!told.contains(&password));
    fakes.backend.shut_down();
    for (name, bytes) in snapshot(fakes.scratch.path()) {
        // The scenario is where the made-up password comes from.
        if name.ends_with("scenario.json") {
            continue;
        }
        assert!(
            !String::from_utf8_lossy(&bytes).contains(&password),
            "{name} holds the password"
        );
    }
}
