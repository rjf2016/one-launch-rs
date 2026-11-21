#![forbid(unsafe_code)]

// A whole run on the fake launchers with the log at debug: a scan, an import, a switch and a launch on each of the four launchers and a second one after it, a refusal, a failure and a rename. Then the log is searched for every login, name, account id, password, token and saved sign-in the scenario and its launcher files hold. None may be there.

use std::fs;
use std::path::Path;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

use futures_lite::future::block_on;
use ol_api::accounts::AccountPatch;
use ol_api::launchers::{
    BattlenetMachine, BattlenetTiming, EpicMachine, EpicTiming, Launchers, NewAccount, RiotMachine,
    RiotTiming, Secret, SteamTiming,
};
use ol_api::log::{LogLevel, LogOptions, Logger};
use ol_api::{Backend, StartLaunch};
use ol_model::{JobState, LaunchJob, LauncherId, SettingKey};
use ol_testkit::{Scratch, oracle};
use ol_vault::FakeVault;
use serde_json::Value;
use time::macros::datetime;

const VERSION: &str = "9.8.7";
const SCENARIO: &str = "fixtures/dev-only/fake-launchers/all-four";
// Words the redaction leaves alone on purpose, as the Electron app's does: they name nobody.
const NOT_A_NAME: [&str; 4] = ["main", "alt", "true", "false"];

struct Run {
    backend: Backend,
    jobs: Receiver<LaunchJob>,
}

impl Run {
    fn account(&self, launcher: LauncherId, login: &str) -> ol_model::Account {
        block_on(self.backend.accounts())
            .unwrap()
            .into_iter()
            .find(|account| account.launcher == launcher && account.login == login)
            .expect("the scenario seeds this account")
    }

    fn game(&self, launcher: LauncherId) -> ol_model::Game {
        let library = block_on(self.backend.library()).unwrap();
        library
            .games
            .into_iter()
            .map(|entry| entry.game)
            .find(|game| game.launcher == launcher)
            .expect("the scan found the launcher's game")
    }

    // Starts a switch with the launcher's game and waits until it ends; None when it was refused.
    fn play(&self, launcher: LauncherId, login: &str) -> Option<LaunchJob> {
        let start = StartLaunch {
            account_id: self.account(launcher, login).id,
            game_id: Some(self.game(launcher).id),
            confirm_close: true,
        };
        block_on(self.backend.start_launch(start)).ok()?;
        loop {
            let job = self.jobs.recv().expect("the job ends");
            if job.state != JobState::Running {
                return Some(job);
            }
        }
    }
}

fn start(scratch: &Scratch, log: &Logger) -> Run {
    let launchers_dir = scratch.join("launchers");
    ol_sim::lay_down(
        &oracle(SCENARIO),
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
    launchers.epic = EpicMachine {
        timing: EpicTiming::with(&fake.epic_timing),
        default_dir: fake.epic_default_dir.clone(),
        local_app_data: fake.local_app_data.clone(),
        program_data: fake.program_data.clone(),
    };
    launchers.battlenet = BattlenetMachine {
        timing: BattlenetTiming::with(&fake.battlenet_timing),
        default_dir: fake.battlenet_default_dir.clone(),
        app_data: fake.app_data.clone(),
        local_app_data: fake.local_app_data.clone(),
        program_data: fake.program_data.clone(),
    };
    launchers.riot = RiotMachine::at(
        RiotTiming::with(&fake.riot_timing),
        fake.riot_default_dir.clone(),
        fake.local_app_data.clone(),
        fake.program_data.clone(),
    )
    .with_local_launch(fake.riot_launch.clone());
    let (changed, jobs) = channel();
    let changed = Mutex::new(changed);
    let backend = Backend::logging_to(log.log()).start_with(
        scratch.join("data"),
        VERSION,
        launchers,
        Some(Arc::new(move |job: &LaunchJob| {
            let _ = changed.lock().unwrap().send(job.clone());
        })),
    );
    for account in &fake.seed.accounts {
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
    let accounts = block_on(backend.accounts()).unwrap();
    for session in &fake.seed.sessions {
        let owner = accounts
            .iter()
            .find(|account| account.launcher == session.launcher && account.login == session.login)
            .unwrap();
        block_on(backend.seed_session(
            &owner.id,
            Some(session.launcher_uid.clone()),
            Secret::new(session.blob.clone()),
        ))
        .unwrap();
    }
    block_on(backend.rescan()).unwrap();
    Run { backend, jobs }
}

fn strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => out.push(text.clone()),
        Value::Array(items) => items.iter().for_each(|item| strings(item, out)),
        Value::Object(fields) => fields.values().for_each(|field| strings(field, out)),
        _ => {}
    }
}

// What the code under test must never write, read off the fixtures here and not by the code under test.
fn secrets() -> Vec<String> {
    let scenario: Value =
        serde_json::from_str(&fs::read_to_string(oracle(SCENARIO).join("scenario.json")).unwrap())
            .unwrap();
    let mut found = Vec::new();
    for account in scenario["seed"]["accounts"].as_array().unwrap() {
        for key in ["displayName", "login", "launcherUid", "password"] {
            strings(&account[key], &mut found);
        }
    }
    for session in scenario["seed"]["sessions"].as_array().unwrap() {
        for key in ["login", "launcherUid", "refreshToken", "blob"] {
            strings(&session[key], &mut found);
        }
    }
    for (path, value) in scenario["registry"].as_object().unwrap() {
        found.extend(path.rsplit('\\').next().map(str::to_owned));
        strings(value, &mut found);
    }
    strings(&scenario["onDisk"], &mut found);

    let fixture =
        |relative: &str| fs::read_to_string(oracle("fixtures/launchers").join(relative)).unwrap();
    let quoted = |line: &str| -> Vec<String> {
        line.split('"')
            .skip(1)
            .step_by(2)
            .map(str::to_owned)
            .collect()
    };
    for line in fixture("steam/loginusers.vdf").lines() {
        let line = line.trim();
        if line.starts_with("\"AccountName\"") || line.starts_with("\"PersonaName\"") {
            found.extend(quoted(line).into_iter().skip(1));
        } else if line.starts_with("\"7656119") {
            found.extend(quoted(line));
        }
    }
    let config = fixture("battlenet/Battle.net.config");
    let saved = config
        .lines()
        .find_map(|line| line.trim().strip_prefix("\"SavedAccountNames\": \""))
        .unwrap();
    found.extend(
        saved
            .trim_end_matches(['"', ','])
            .split(',')
            .map(str::to_owned),
    );
    for line in fixture("epic/GameUserSettings.ini").lines() {
        if let Some(data) = line.strip_prefix("Data=") {
            found.push(data.to_owned());
        }
    }
    let riot = fixture("riot/RiotGamesPrivateSettings.yaml");
    for line in riot.lines() {
        for value in quoted(line) {
            if value.len() >= 16 && !value.contains("placeholder.example") {
                found.push(value);
            }
        }
    }
    let id_token = riot
        .lines()
        .find_map(|line| line.trim().strip_prefix("id_token: \""))
        .unwrap()
        .trim_end_matches('"');
    let payload = base64url(id_token.split('.').nth(1).unwrap());
    let claims: Value = serde_json::from_slice(&payload).unwrap();
    strings(&claims["sub"], &mut found);
    let (game_name, tag_line) = (
        claims["acct"]["game_name"].as_str().unwrap(),
        claims["acct"]["tag_line"].as_str().unwrap(),
    );
    found.push(game_name.to_owned());
    found.push(format!("{game_name}#{tag_line}"));

    found.retain(|value| value.len() >= 3 && !NOT_A_NAME.contains(&value.to_lowercase().as_str()));
    found.sort();
    found.dedup();
    found
}

fn base64url(text: &str) -> Vec<u8> {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut bits = 0_u32;
    let mut count = 0;
    let mut out = Vec::new();
    for byte in text.bytes().filter(|byte| *byte != b'=') {
        let value = alphabet.iter().position(|a| *a == byte).expect("base64url") as u32;
        bits = (bits << 6) | value;
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    out
}

fn read_logs(dir: &Path) -> String {
    let mut text = String::new();
    for entry in fs::read_dir(dir).unwrap() {
        text.push_str(&fs::read_to_string(entry.unwrap().path()).unwrap());
    }
    text
}

#[test]
fn a_whole_run_on_each_launcher_leaves_no_secret_in_the_log() {
    let scratch = Scratch::new("api-log-switch");
    let logger = Logger::for_data_folder(
        &scratch.join("data"),
        LogOptions {
            level: LogLevel::Debug,
            ..LogOptions::default()
        },
    );
    let run = start(&scratch, &logger);
    block_on(
        run.backend
            .set_setting(SettingKey::LogLevel, "debug".to_owned()),
    )
    .unwrap();
    assert!(
        block_on(
            run.backend
                .set_setting(SettingKey::LogLevel, "loud".to_owned())
        )
        .is_err()
    );

    let remembered = block_on(run.backend.remembered_accounts(LauncherId::Steam)).unwrap();
    let keys: Vec<String> = remembered.iter().map(ol_services_key).collect();
    block_on(run.backend.import_accounts(LauncherId::Steam, keys)).unwrap();
    let duplicate = block_on(run.backend.create_account(NewAccount {
        launcher: LauncherId::Steam,
        display_name: "Persona One again".to_owned(),
        login: "user_1".to_owned(),
        launcher_uid: None,
        password: None,
        tags: Vec::new(),
        notes: String::new(),
    }));
    assert_eq!(
        duplicate.err().map(|refused| refused.code),
        Some("DUPLICATE")
    );

    let plays = [
        (LauncherId::Steam, ["user_3", "user_1"]),
        (LauncherId::Epic, ["alt@example.com", "main@example.com"]),
        (
            LauncherId::Battlenet,
            ["saved3@example.com", "saved1@example.com"],
        ),
        (LauncherId::Riot, ["riot_alt", "riot_main"]),
    ];
    let mut ended = Vec::new();
    for (launcher, logins) in plays {
        for login in logins {
            let plan = block_on(run.backend.switch_plan(&run.account(launcher, login).id));
            assert!(plan.is_ok(), "{} plans a switch", launcher.as_str());
            ended.extend(run.play(launcher, login).map(|job| (launcher, job.state)));
        }
    }
    assert!(
        ended
            .iter()
            .any(|(launcher, state)| *launcher == LauncherId::Steam && *state == JobState::Done),
        "a Steam switch went through: {:?}",
        ended
    );
    let renamed = run.account(LauncherId::Riot, "riot_new");
    block_on(run.backend.update_account(
        &renamed.id,
        AccountPatch {
            display_name: Some("Gordon Renamed".to_owned()),
            login: None,
            launcher_uid: None,
            tags: None,
            notes: None,
        },
    ))
    .unwrap();
    let _ = block_on(run.backend.session(&renamed.id));
    run.backend.shut_down();
    assert!(logger.flush(std::time::Duration::from_secs(10)));
    logger.close(std::time::Duration::from_secs(10));

    let log = read_logs(&scratch.join("data").join("logs"));
    let lower = log.to_lowercase();
    for expected in [
        "[main] starting",
        "[main:steam] switch started",
        "[main:epic] epic switch started",
        "[main:battlenet] battlenet switch started",
        "[main:riot] riot switch started",
        "[main:launch] launch job finished",
        "[main:launch] launch step finished",
        "[main:scan] library rescanned",
        "[main:backup] backup snapshot written",
        "[main:ipc] imported launcher accounts",
        "ipc accounts:create refused",
        "ipc settings:set failed",
        "[main] exit",
        "[ACCOUNT]",
    ] {
        assert!(log.contains(expected), "the log has {expected:?}:\n{log}");
    }
    let secrets = secrets();
    assert!(
        secrets.len() >= 40,
        "the fixtures give {} values to look for",
        secrets.len()
    );
    let leaked: Vec<String> = secrets
        .into_iter()
        .filter(|secret| lower.contains(&secret.to_lowercase()))
        .collect();
    assert!(
        leaked.is_empty(),
        "the log holds {} value(s) it must not; the first is {} characters long",
        leaked.len(),
        leaked[0].len()
    );
    let mut after_users = lower.split("users");
    after_users.next();
    for rest in after_users {
        let name = rest.trim_start_matches(['\\', '/']);
        assert!(
            rest == name || name.starts_with("[user]"),
            "a profile path in the log names its user"
        );
    }
}

fn ol_services_key(entry: &ol_model::RememberedAccount) -> String {
    if entry.launcher_uid.is_empty() {
        entry.login.clone()
    } else {
        entry.launcher_uid.clone()
    }
}
