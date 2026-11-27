#![forbid(unsafe_code)]

// The backend's launcher commands for Epic on fake launchers, and where a saved sign-in may be found afterwards: in the launcher's own file and, encrypted, in the database and a snapshot. Nowhere else.

use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

use futures_lite::future::block_on;
use ol_api::accounts::AccountPatch;
use ol_api::launchers::{
    BattlenetMachine, BattlenetTiming, EpicMachine, EpicTiming, Launchers, NewAccount, Secret,
    SteamTiming,
};
use ol_api::{ApiError, Backend, StartLaunch};
use ol_model::{JobState, JobStepStatus, LaunchJob, LauncherId, SwitchPlan};
use ol_sim::FakeLaunchers;
use ol_testkit::vectors::sha256_hex;
use ol_testkit::{Scratch, oracle, snapshot};
use ol_vault::FakeVault;
use serde_json::{Value, json};
use time::macros::datetime;

const VERSION: &str = "9.8.7";
const SCENARIO: &str = "fixtures/dev-only/fake-launchers/all-four";
const SETTINGS: &str = "localappdata/EpicGamesLauncher/Saved/Config/Windows/GameUserSettings.ini";
const MAIN: &str = "main@example.com";
const ALT: &str = "alt@example.com";

struct Fakes {
    scratch: Scratch,
    fake: FakeLaunchers,
    backend: Backend,
    jobs: Receiver<LaunchJob>,
}

impl Fakes {
    fn settings_file(&self) -> PathBuf {
        self.scratch.join("launchers").join(SETTINGS)
    }

    fn account(&self, login: &str) -> ol_model::Account {
        block_on(self.backend.accounts())
            .unwrap()
            .into_iter()
            .find(|account| account.launcher == LauncherId::Epic && account.login == login)
            .expect("the scenario seeds this account")
    }

    fn game(&self) -> ol_model::Game {
        let library = block_on(self.backend.library()).unwrap();
        let games = library.games.into_iter().map(|entry| entry.game);
        let mut epics = games.filter(|game| game.launcher == LauncherId::Epic);
        epics.next().expect("the scan found Epic's game")
    }

    fn play(&self, login: &str) -> Vec<LaunchJob> {
        self.play_with(login, false)
            .expect("the switch is accepted")
    }

    // Every state the job went through, the last one last; or why the switch was not accepted.
    fn play_with(&self, login: &str, confirm_close: bool) -> Result<Vec<LaunchJob>, ApiError> {
        let start = StartLaunch {
            account_id: self.account(login).id,
            game_id: Some(self.game().id),
            confirm_close,
        };
        block_on(self.backend.start_launch(start))?;
        let mut states = Vec::new();
        loop {
            let job = self
                .jobs
                .recv()
                .expect("the job ends before its observer goes");
            let ended = job.state != JobState::Running;
            states.push(job);
            if ended {
                return Ok(states);
            }
        }
    }

    // The app quits and starts again on the same two folders, with a fake machine built afresh.
    fn restart(self) -> Self {
        self.backend.shut_down();
        start(self.scratch)
    }
}

fn scenario() -> Value {
    let file = oracle(SCENARIO).join("scenario.json");
    serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap()
}

// The session data the scenario saves for an account: a made-up one, and the thing that must not be found anywhere it does not belong.
fn seeded_session_data(login: &str) -> String {
    let scenario = scenario();
    let sessions = scenario["seed"]["sessions"].as_array().unwrap();
    let session = sessions
        .iter()
        .find(|session| session["launcher"] == "epic" && session["login"] == login)
        .expect("the scenario saves a sign-in for this account");
    session["blob"]["rememberMe"]["data"]
        .as_str()
        .unwrap()
        .to_owned()
}

// The value of a key in a section of the fixture's settings file, read line by line here and not by the code under test.
fn fixture_value(section: &str, key: &str) -> String {
    let text = String::from_utf8(fixture_settings()).unwrap();
    let mut lines = text
        .lines()
        .skip_while(|line| *line != format!("[{section}]"));
    lines
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .expect("the fixture has this value")
        .to_owned()
}

fn fixture_settings() -> Vec<u8> {
    fs::read(oracle("fixtures/launchers/epic/GameUserSettings.ini")).unwrap()
}

// The scenario all-four laid down in a scratch folder, with its accounts and saved sign-ins seeded and its launchers scanned.
fn fakes(knobs: Option<&Value>) -> Fakes {
    let scratch = Scratch::new("api-epic");
    let launchers_dir = scratch.join("launchers");
    ol_sim::lay_down(
        &oracle(SCENARIO),
        &oracle("fixtures/launchers"),
        &launchers_dir,
    )
    .unwrap();
    if let Some(knobs) = knobs {
        ol_sim::write_knobs(&launchers_dir, knobs).unwrap();
    }
    start(scratch)
}

fn start(scratch: Scratch) -> Fakes {
    let fake = ol_sim::materialize(
        &scratch.join("launchers"),
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
    let (changed, jobs) = channel();
    let changed = Mutex::new(changed);
    let backend = Backend::start_with(
        scratch.join("data"),
        VERSION,
        launchers,
        Some(Arc::new(move |job: &LaunchJob| {
            let _ = changed.lock().unwrap().send(job.clone());
        })),
    );
    if block_on(backend.accounts()).unwrap().is_empty() {
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
                .find(|account| {
                    account.launcher == session.launcher && account.login == session.login
                })
                .unwrap();
            block_on(backend.seed_session(
                &owner.id,
                Some(session.launcher_uid.clone()),
                Secret::new(session.blob.clone()),
            ))
            .unwrap();
        }
    }
    block_on(backend.rescan()).unwrap();
    Fakes {
        scratch,
        fake,
        backend,
        jobs,
    }
}

// Everything a plan says, as one text.
fn plan_text(plan: &SwitchPlan) -> String {
    let mut text = vec![plan.warnings.join("\n"), plan.notes.join("\n")];
    for step in &plan.steps {
        text.push(format!("{} {} {}", step.title, step.target, step.detail));
        for change in &step.changes {
            text.push(format!(
                "{} {} {}",
                change.place, change.before, change.after
            ));
        }
    }
    text.join("\n")
}

// Everything a job says, in every state it went through, as one text.
fn job_text(states: &[LaunchJob]) -> String {
    let mut text = Vec::new();
    for job in states {
        text.push(job.error.clone().unwrap_or_default());
        text.push(job.hand_off.clone().unwrap_or_default());
        for step in &job.steps {
            text.push(format!("{} {}", step.title, step.detail));
        }
        if let Some(attention) = &job.attention {
            text.push(format!(
                "{} {} {} {}",
                attention.title,
                attention.message,
                attention.steps.join(" "),
                attention.reason.clone().unwrap_or_default()
            ));
        }
    }
    text.join("\n")
}

fn holds(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

// The files under a folder that hold `needle`.
fn files_holding(folder: &std::path::Path, needle: &str) -> Vec<String> {
    snapshot(folder)
        .into_iter()
        .filter(|(_, bytes)| holds(bytes, needle.as_bytes()))
        .map(|(name, _)| name)
        .collect()
}

// A value as the stand-in vault of the fixtures stores it: each byte XOR 0x5a.
fn sealed_by_the_stand_in(text: &str) -> Vec<u8> {
    text.bytes().map(|byte| byte ^ 0x5a).collect()
}

#[test]
fn a_scan_finds_epics_game_and_the_launcher_says_who_signed_in_here() {
    let fakes = fakes(None);
    let recorded: Value =
        serde_json::from_str(&fs::read_to_string(oracle("l2/expected/play-epic.json")).unwrap())
            .unwrap();
    let recorded = recorded["final"]["db"]["tables"]["games"]
        .as_array()
        .unwrap()
        .iter()
        .find(|game| game["launcher"] == "epic")
        .unwrap()
        .clone();
    let game = fakes.game();
    assert_eq!(game.name, recorded["name"].as_str().unwrap());
    assert_eq!(
        game.launcher_game_id,
        recorded["launcher_game_id"].as_str().unwrap()
    );
    assert_eq!(
        game.install_path.as_deref(),
        recorded["install_path"].as_str()
    );
    assert_eq!(game.needs_update, recorded["needs_update"] == 1);
    assert_eq!(game.last_played_at, None);
    let from_files = block_on(fakes.backend.installed_games(LauncherId::Epic)).unwrap();
    assert_eq!(from_files.len(), 1);
    assert_eq!(from_files[0].launcher_game_id, game.launcher_game_id);

    // Every account that signed in on this PC has sections named [<32 hex digits>_...] in the settings file.
    let text = String::from_utf8(fixture_settings()).unwrap();
    let mut signed_in_here: Vec<String> = Vec::new();
    for line in text.lines() {
        let id = line.strip_prefix('[').and_then(|rest| rest.split_once('_'));
        if let Some((id, _)) = id
            && id.len() == 32
            && !signed_in_here.contains(&id.to_owned())
        {
            signed_in_here.push(id.to_owned());
        }
    }
    let remembered = block_on(fakes.backend.remembered_accounts(LauncherId::Epic)).unwrap();
    let mut listed: Vec<String> = remembered
        .iter()
        .map(|entry| entry.launcher_uid.clone())
        .collect();
    listed.sort();
    signed_in_here.sort();
    assert_eq!(listed, signed_in_here);

    // The launcher's log says the account "Main" signed in last, and the file it wrote then holds that session.
    let main = fakes.account(MAIN);
    let first = &remembered[0];
    assert!(first.most_recent && first.remember_password);
    assert_eq!(Some(&first.launcher_uid), main.launcher_uid.as_ref());
    assert_eq!(first.linked_account_id.as_ref(), Some(&main.id));
    assert!(first.login == main.login && first.persona_name == main.display_name);
    assert_eq!(
        remembered.iter().filter(|entry| entry.most_recent).count(),
        1
    );
    let unknown = remembered
        .iter()
        .find(|entry| entry.linked_account_id.is_none())
        .expect("an account that signed in here and is not in One Launch");
    assert!(unknown.login.is_empty() && unknown.persona_name.is_empty());
    fakes.backend.shut_down();
}

#[test]
fn what_is_saved_of_a_sign_in_is_its_digest_and_whose_it_is() {
    let fakes = fakes(None);
    let (main, alt) = (fakes.account(MAIN), fakes.account(ALT));
    assert!(alt.has_session && !main.has_session);

    let saved = block_on(fakes.backend.session(&alt.id)).unwrap();
    let session = saved.session.expect("the scenario saved a sign-in for Alt");
    assert_eq!(
        session.fingerprint,
        sha256_hex(seeded_session_data(ALT).as_bytes())
    );
    assert_eq!(session.launcher_uid, alt.launcher_uid);
    assert!(session.verified_at.is_none() && session.stale_reason.is_none());

    // What the launcher remembers now is Main's: its log says so, and the file was written after that sign-in.
    let state = saved.state.expect("Epic's files say what it remembers");
    assert!(state.remembered && !state.launcher_running);
    assert_eq!(
        state.fingerprint,
        Some(sha256_hex(fixture_value("RememberMe", "Data").as_bytes()))
    );
    assert_eq!(state.linked_account_id.as_ref(), Some(&main.id));
    assert_eq!(state.launcher_uid, main.launcher_uid);
    assert!(state.last_sign_in.is_some_and(|last| last.ok));
    assert!(saved.state_error.is_none());

    let steam = block_on(fakes.backend.accounts())
        .unwrap()
        .into_iter()
        .find(|account| account.launcher == LauncherId::Steam)
        .unwrap();
    let saved = block_on(fakes.backend.session(&steam.id)).unwrap();
    assert!(saved.session.is_none() && saved.state.is_none() && saved.state_error.is_none());
    fakes.backend.shut_down();
}

#[test]
fn a_settings_file_older_than_the_last_sign_in_names_no_one_as_remembered() {
    let fakes = fakes(None);
    let main = fakes.account(MAIN);
    assert!(!main.has_session);
    let file = fs::File::options()
        .write(true)
        .open(fakes.settings_file())
        .unwrap();
    file.set_modified(std::time::SystemTime::UNIX_EPOCH)
        .unwrap();
    drop(file);

    let state = block_on(fakes.backend.session(&main.id))
        .unwrap()
        .state
        .expect("Epic's files say what it remembers");
    assert!(state.remembered && !state.launcher_running);
    assert_eq!(state.linked_account_id, None);
    assert_eq!(state.launcher_uid, None);
    let remembered = block_on(fakes.backend.remembered_accounts(LauncherId::Epic)).unwrap();
    assert!(!remembered.is_empty());
    assert!(
        remembered
            .iter()
            .all(|entry| !entry.most_recent && !entry.remember_password)
    );
    fakes.backend.shut_down();
}

#[test]
fn a_saved_sign_in_is_in_the_launchers_file_and_encrypted_in_the_data_folder_and_nowhere_else() {
    let fakes = fakes(None);
    let alt = fakes.account(ALT);
    let saved_for_alt = seeded_session_data(ALT);
    let remembered = fixture_value("RememberMe", "Data");
    let offline = fixture_value("Offline", "Data");
    let secrets = [&saved_for_alt, &remembered, &offline];
    assert!(secrets.iter().all(|secret| secret.len() >= 40));

    let mut said = vec![
        plan_text(&block_on(fakes.backend.switch_plan(&alt.id)).unwrap()),
        plan_text(
            &block_on(
                fakes
                    .backend
                    .launch_preflight(&alt.id, Some(&fakes.game().id)),
            )
            .unwrap()
            .plan,
        ),
    ];
    let states = fakes.play(ALT);
    assert!(states.last().unwrap().state == JobState::Done);
    said.push(job_text(&states));
    said.push(json!(fakes.fake.log.snapshot()).to_string());
    said.push(plan_text(
        &block_on(fakes.backend.switch_plan(&fakes.account(MAIN).id)).unwrap(),
    ));
    let saved = block_on(fakes.backend.session(&fakes.account(MAIN).id)).unwrap();
    let session = saved
        .session
        .expect("the switch saved the session it replaced");
    assert_eq!(session.fingerprint, sha256_hex(remembered.as_bytes()));
    said.push(format!(
        "{} {:?} {:?}",
        session.fingerprint, session.launcher_uid, saved.state_error
    ));
    let said = said.join("\n");
    assert!(
        said.contains("fingerprint"),
        "the plans and the job speak of the session"
    );
    for secret in secrets {
        assert!(
            !said.contains(secret.as_str()),
            "a plan, a job or the fake machine's log holds a session"
        );
    }
    fakes.backend.shut_down();

    // The launcher's own file holds Alt's now, as it has to, and neither of the two it held before.
    let written = fs::read(fakes.settings_file()).unwrap();
    assert!(holds(&written, saved_for_alt.as_bytes()));
    assert!(!holds(&written, remembered.as_bytes()) && !holds(&written, offline.as_bytes()));

    // In the data folder a session is found only as the vault sealed it: in the database, and in the snapshot of the file.
    let data = fakes.scratch.join("data");
    for secret in secrets {
        assert_eq!(files_holding(&data, secret), Vec::<String>::new());
    }
    let database = fs::read(data.join("onelaunch.db")).unwrap();
    for stored in [&saved_for_alt, &remembered] {
        assert!(holds(&database, &sealed_by_the_stand_in(stored)));
    }
    let snapshots: Vec<String> = snapshot(&data.join("backups").join("epic"))
        .into_keys()
        .filter(|name| !name.ends_with('/') && !name.ends_with("manifest.json"))
        .collect();
    assert_eq!(snapshots.len(), 1);
    assert!(snapshots[0].ends_with("Config/Windows/GameUserSettings.ini.enc"));

    // Beside the launcher's file, the only other place under the scenario's folder is the scenario itself, which is where the made-up session came from.
    let launchers = fakes.scratch.join("launchers");
    assert_eq!(
        files_holding(&launchers, &saved_for_alt),
        [SETTINGS, "scenario.json"]
    );
}

#[test]
fn a_switch_that_fails_after_its_write_puts_the_file_back_from_its_encrypted_copy() {
    let knobs = json!({
        "processes": { "startFails": ["EpicGamesLauncher.exe"] },
        "timing": { "epic": { "startupMs": 1500 } },
    });
    let fakes = fakes(Some(&knobs));
    let states = fakes.play(ALT);
    let last = states.last().unwrap();
    assert!(last.state == JobState::Failed);
    assert_eq!(last.restored, Some(true));
    assert!(last.backup_id.is_some());
    let said = job_text(&states);
    assert!(said.contains("did not appear within 1.5 s"));
    for secret in [
        seeded_session_data(ALT),
        fixture_value("RememberMe", "Data"),
        fixture_value("Offline", "Data"),
    ] {
        assert!(!said.contains(&secret));
    }
    assert!(fs::read(fakes.settings_file()).unwrap() == fixture_settings());
    assert!(block_on(fakes.backend.recovery()).unwrap().is_none());
    fakes.backend.shut_down();
}

#[test]
fn a_failed_switch_is_not_put_back_under_a_launcher_that_will_not_close() {
    let probe = fakes(None);
    let main_uid = probe.account(MAIN).launcher_uid.unwrap();
    probe.backend.shut_down();
    let knobs = json!({
        "processes": { "elevated": ["EpicGamesLauncher.exe"], "shutdownPolls": 100_000 },
        "knobs": { "epic": { "signInWhenAskedMs": 2000, "signInAs": main_uid } },
        "timing": { "epic": { "shutdownMs": 1000, "killGraceMs": 500 } },
    });
    let fakes = fakes(Some(&knobs));
    let states = fakes.play("new@example.com");
    let last = states.last().unwrap();
    assert!(last.state == JobState::Failed);
    assert_eq!(last.restored, Some(false), "{:?}", last.error);
    let fakes = fakes.restart();
    assert!(block_on(fakes.backend.recovery()).unwrap().is_some());
    fakes.backend.shut_down();
}

// What the fake machine was asked to do to a process or to a launcher's file, in order.
fn asked_of_the_machine(fakes: &Fakes) -> Vec<String> {
    let events = fakes.fake.log.snapshot();
    let named = events.iter().filter_map(|event| {
        let op = event["op"].as_str()?;
        let file = event["path"].as_str().unwrap_or_default();
        let manifest = file.ends_with("manifest.json");
        (!manifest).then(|| op.to_owned())
    });
    named.collect()
}

fn account_id_of(login: &str) -> String {
    let scenario = scenario();
    let accounts = scenario["seed"]["accounts"].as_array().unwrap();
    let account = accounts
        .iter()
        .find(|account| account["launcher"] == "epic" && account["login"] == login)
        .unwrap();
    account["launcherUid"].as_str().unwrap().to_owned()
}

#[test]
fn a_running_launcher_is_closed_by_its_window_once_the_user_has_said_yes() {
    let knobs = json!({ "knobs": { "epic": { "runningAs": account_id_of(MAIN) } } });
    let fakes = fakes(Some(&knobs));
    let (alt, game) = (fakes.account(ALT), fakes.game());
    let preflight = block_on(fakes.backend.launch_preflight(&alt.id, Some(&game.id))).unwrap();
    assert!(
        preflight.launcher_running && preflight.needs_confirmation && !preflight.already_active
    );
    assert_eq!(
        preflight.close_message.as_deref(),
        Some("Epic Games Launcher is running. Switching closes it.")
    );

    let unconfirmed = fakes.play_with(ALT, false).err().expect("a refusal");
    assert_eq!(unconfirmed.code, "CONFIRM_REQUIRED");
    assert!(asked_of_the_machine(&fakes).is_empty());

    let states = fakes.play_with(ALT, true).unwrap();
    let last = states.last().unwrap();
    assert!(last.state == JobState::Done && last.switched);
    let detail = |id: &str| {
        let step = last.steps.iter().find(|step| step.id == id).unwrap();
        step.detail.clone()
    };
    assert_eq!(detail("close"), "Launcher closed.");
    assert!(detail("capture").starts_with("Saved Main's session (fingerprint "));
    assert_eq!(
        asked_of_the_machine(&fakes),
        ["proc.close", "file.write", "proc.start", "shell.open"]
    );
    assert!(
        block_on(fakes.backend.session(&fakes.account(MAIN).id))
            .unwrap()
            .session
            .is_some()
    );
    fakes.backend.shut_down();
}

#[test]
fn an_edit_after_the_dialog_s_check_makes_the_start_check_again() {
    let knobs = json!({ "knobs": { "epic": { "runningAs": account_id_of(ALT) } } });
    let fakes = fakes(Some(&knobs));
    let (alt, game) = (fakes.account(ALT), fakes.game());
    let preflight = block_on(fakes.backend.launch_preflight(&alt.id, Some(&game.id))).unwrap();
    assert!(preflight.already_active);

    block_on(fakes.backend.update_account(
        &alt.id,
        AccountPatch {
            launcher_uid: Some(Some("f".repeat(32))),
            ..AccountPatch::default()
        },
    ))
    .unwrap();
    let refused = fakes.play_with(ALT, false).err().expect("a refusal");
    assert_eq!(refused.code, "CONFIRM_REQUIRED");
    assert!(asked_of_the_machine(&fakes).is_empty());
    fakes.backend.shut_down();
}

#[test]
fn a_launcher_that_runs_as_the_account_is_left_alone_and_the_game_starts_at_once() {
    let knobs = json!({ "knobs": { "epic": { "runningAs": account_id_of(ALT) } } });
    let fakes = fakes(Some(&knobs));
    let (alt, game) = (fakes.account(ALT), fakes.game());
    let preflight = block_on(fakes.backend.launch_preflight(&alt.id, Some(&game.id))).unwrap();
    assert!(
        preflight.launcher_running && preflight.already_active && !preflight.needs_confirmation
    );

    let states = fakes.play(ALT);
    let last = states.last().unwrap();
    assert!(last.state == JobState::Done && last.direct && last.backup_id.is_none());
    assert!(!last.game_seen && last.hand_off.is_none());
    assert_eq!(asked_of_the_machine(&fakes), ["shell.open"]);
    assert!(fs::read(fakes.settings_file()).unwrap() == fixture_settings());
    assert!(
        block_on(fakes.backend.backups(LauncherId::Epic))
            .unwrap()
            .is_empty()
    );
    fakes.backend.shut_down();
}

#[test]
fn a_snapshot_goes_back_byte_for_byte_once_the_launcher_is_closed() {
    let fakes = fakes(None);
    assert!(fakes.play(ALT).last().unwrap().state == JobState::Done);
    assert!(fs::read(fakes.settings_file()).unwrap() != fixture_settings());
    let taken = block_on(fakes.backend.backups(LauncherId::Epic)).unwrap();
    assert_eq!(taken.len(), 1);
    assert_eq!(taken[0].files, ["Config/Windows/GameUserSettings.ini"]);
    assert!(taken[0].pinned.is_none());
    let id = taken[0].id.clone();

    // The switch left the launcher running, and a running launcher writes its file again when it exits.
    let refused = block_on(fakes.backend.restore_backup(LauncherId::Epic, &id))
        .err()
        .expect("a running launcher refuses a restore");
    assert_eq!(refused.code, "LAUNCHER_RUNNING");
    assert!(fs::read(fakes.settings_file()).unwrap() != fixture_settings());

    let fakes = fakes.restart();
    let restored = block_on(fakes.backend.restore_backup(LauncherId::Epic, &id)).unwrap();
    assert_eq!(restored.files.len(), 1);
    assert!(restored.registry.is_empty());
    assert!(fs::read(fakes.settings_file()).unwrap() == fixture_settings());
    // What was there before the restore was copied first, so the restore can be undone.
    let after = block_on(fakes.backend.backups(LauncherId::Epic)).unwrap();
    assert_eq!(after.len(), 2);
    assert_eq!(after[0].reason, format!("Before restoring {id}"));
    fakes.backend.shut_down();
}

// The fixture's settings file with a byte of Windows-1252 in it, as an editor that saves in the system's code page leaves one: the file is no longer UTF-8 text.
fn settings_not_utf8() -> Vec<u8> {
    let fixture = fixture_settings();
    let header = b"[Launcher]\r\n";
    let at = fixture
        .windows(header.len())
        .position(|window| window == header)
        .expect("the fixture opens with [Launcher]")
        + header.len();
    [
        &fixture[..at],
        b"InstallFolder=Jeux \xe9pic\r\n",
        &fixture[at..],
    ]
    .concat()
}

// A switch to Alt over a settings file that is not UTF-8 text: refused at the plan, with the launcher left as it is, the file's bytes as they were and no sign-in saved.
fn refused_over_a_settings_file_that_is_not_text(knobs: Option<&Value>) {
    let fakes = fakes(knobs);
    let broken = settings_not_utf8();
    fs::write(fakes.settings_file(), &broken).unwrap();
    let (main, alt, game) = (fakes.account(MAIN), fakes.account(ALT), fakes.game());
    assert!(!main.has_session);
    let alt_saved = block_on(fakes.backend.session(&alt.id))
        .unwrap()
        .session
        .expect("the scenario saved a sign-in for Alt");

    let preflight = block_on(fakes.backend.launch_preflight(&alt.id, Some(&game.id)))
        .err()
        .expect("the preflight is refused");
    assert_eq!(preflight.code, "LAUNCHER_FILE");
    assert!(preflight.message.starts_with(
        "Epic Games: GameUserSettings.ini holds bytes that are not UTF-8 text; One Launch will not rewrite it. Start Epic Games Launcher by hand and close it, so that it writes the file again, then try again: "
    ));
    let plan = block_on(fakes.backend.switch_plan(&alt.id))
        .err()
        .expect("the plan is refused");
    assert_eq!(plan, preflight);
    let refused = fakes
        .play_with(ALT, false)
        .err()
        .expect("the switch is refused at its start");
    assert_eq!(refused, preflight);
    // With the close said yes to beforehand the switch starts as a job, and its plan stops it before the first step.
    let states = fakes.play_with(ALT, true).unwrap();
    let last = states.last().unwrap();
    assert!(last.state == JobState::Failed && !last.switched);
    assert!(last.backup_id.is_none() && last.restored.is_none());
    assert_eq!(last.error.as_deref(), Some(preflight.message.as_str()));

    assert!(asked_of_the_machine(&fakes).is_empty());
    assert!(fs::read(fakes.settings_file()).unwrap() == broken);
    assert!(
        block_on(fakes.backend.backups(LauncherId::Epic))
            .unwrap()
            .is_empty()
    );
    let main_saved = block_on(fakes.backend.session(&main.id)).unwrap();
    assert!(main_saved.session.is_none() && main_saved.state.is_none());
    assert_eq!(main_saved.state_error, Some(preflight.message.clone()));
    let alt_now = block_on(fakes.backend.session(&alt.id))
        .unwrap()
        .session
        .unwrap();
    assert_eq!(alt_now.fingerprint, alt_saved.fingerprint);
    assert!(!fakes.account(MAIN).has_session);
    fakes.backend.shut_down();
    assert!(fs::read(fakes.settings_file()).unwrap() == broken);
}

#[test]
fn a_settings_file_that_is_not_utf8_text_is_never_rewritten_and_nothing_is_saved_from_it() {
    refused_over_a_settings_file_that_is_not_text(None);
}

#[test]
fn a_running_launcher_is_not_closed_for_a_settings_file_that_is_not_utf8_text() {
    let knobs = json!({ "knobs": { "epic": { "runningAs": account_id_of(MAIN) } } });
    refused_over_a_settings_file_that_is_not_text(Some(&knobs));
}

#[test]
fn a_launcher_signed_in_as_the_account_that_exits_before_the_switch_reaches_it_is_started_and_its_sign_in_confirmed()
 {
    let knobs = json!({ "knobs": { "epic": { "runningAs": account_id_of(MAIN) } } });
    let fakes = fakes(Some(&knobs));
    fakes.fake.processes.exit_once_seen("EpicGamesLauncher.exe");
    let states = fakes.play_with(MAIN, true).unwrap();
    let last = states.last().unwrap();
    assert!(last.state == JobState::Done, "{:?}", last.error);
    let step = |id: &str| last.steps.iter().find(|step| step.id == id).unwrap();
    assert_eq!(step("start").status, JobStepStatus::Done);
    assert_eq!(step("verify").status, JobStepStatus::Done);
    assert!(asked_of_the_machine(&fakes).contains(&"proc.start".to_owned()));
    fakes.backend.shut_down();
}

#[test]
fn a_sign_in_by_hand_seen_at_the_first_look_keeps_its_session() {
    let signed_in_as = "55555555555555555555555555555555";
    let fakes = fakes(None);
    let new = fakes.account("new@example.com");
    // The user signs in on the launcher's page as soon as it is up, before the switch first looks at its log.
    let launcher = Arc::clone(fakes.fake.epic.as_ref().unwrap());
    let processes = Arc::clone(&fakes.fake.platform.processes);
    let done = std::sync::atomic::AtomicBool::new(false);
    fakes.fake.clock.before_sleep(move || {
        let up = processes.is_running("EpicGamesLauncher.exe").unwrap();
        if up && !done.swap(true, std::sync::atomic::Ordering::SeqCst) {
            launcher.sign_in_by_hand(Some(signed_in_as));
        }
    });
    let states = fakes.play("new@example.com");
    let last = states.last().unwrap();
    assert!(last.state == JobState::Done, "{:?}", last.error);
    let step = |id: &str| last.steps.iter().find(|step| step.id == id).unwrap();
    assert_eq!(step("verify").status, JobStepStatus::Done);
    assert!(
        step("recapture")
            .detail
            .starts_with("Stored New's session (fingerprint ")
    );
    let saved = block_on(fakes.backend.session(&new.id)).unwrap().session;
    assert!(
        saved
            .expect("the sign-in by hand was saved")
            .verified_at
            .is_some()
    );
    fakes.backend.shut_down();
}

#[test]
fn a_sign_in_that_lands_while_the_launcher_closes_is_saved_before_the_switch_replaces_it() {
    // The launcher runs on an account no One Launch account is linked to, so the plan has no outgoing sign-in to save.
    let knobs = json!({ "knobs": { "epic": { "runningAs": "55555555555555555555555555555555" } } });
    let fakes = fakes(Some(&knobs));
    let main = fakes.account(MAIN);
    assert!(
        block_on(fakes.backend.session(&main.id))
            .unwrap()
            .session
            .is_none()
    );
    // The user signs in as Main while the switch waits for the launcher to close: the first wait of the switch.
    let launcher = Arc::clone(fakes.fake.epic.as_ref().unwrap());
    let main_uid = account_id_of(MAIN);
    let done = std::sync::atomic::AtomicBool::new(false);
    fakes.fake.clock.before_sleep(move || {
        if !done.swap(true, std::sync::atomic::Ordering::SeqCst) {
            launcher.sign_in_by_hand(Some(&main_uid));
        }
    });
    let states = fakes.play_with(ALT, true).unwrap();
    let last = states.last().unwrap();
    assert!(last.state == JobState::Done, "{:?}", last.error);
    let step = |id: &str| last.steps.iter().find(|step| step.id == id).unwrap();
    assert_eq!(step("close").status, JobStepStatus::Done);
    assert!(
        step("capture")
            .detail
            .starts_with("Saved Main's session (fingerprint ")
    );
    assert!(
        block_on(fakes.backend.session(&main.id))
            .unwrap()
            .session
            .is_some()
    );
    fakes.backend.shut_down();
}

#[test]
fn a_sign_in_as_another_account_while_the_session_is_awaited_is_not_saved_for_the_first() {
    let first = "55555555555555555555555555555555";
    let fakes = fakes(None);
    let new = fakes.account("new@example.com");
    // The launcher logs a sign-in by hand as soon as it is up, and has not written its session yet; at the next look the user signs in as Main.
    let launcher = Arc::clone(fakes.fake.epic.as_ref().unwrap());
    let processes = Arc::clone(&fakes.fake.platform.processes);
    let log = fakes
        .scratch
        .join("launchers")
        .join("localappdata/EpicGamesLauncher/Saved/Logs/EpicGamesLauncher.log");
    let main_uid = account_id_of(MAIN);
    let looks = std::sync::atomic::AtomicU32::new(0);
    fakes.fake.clock.before_sleep(move || {
        if !processes.is_running("EpicGamesLauncher.exe").unwrap() {
            return;
        }
        match looks.fetch_add(1, std::sync::atomic::Ordering::SeqCst) {
            0 => {
                let mut text = fs::read_to_string(&log).unwrap();
                text.push_str(&format!(
                    "[..]FAccountService::OnLoginComplete - 0 TRUE {first} \r\n"
                ));
                fs::write(&log, text).unwrap();
            }
            1 => launcher.sign_in_by_hand(Some(&main_uid)),
            _ => {}
        }
    });
    let states = fakes.play("new@example.com");
    let last = states.last().unwrap();
    let step = |id: &str| last.steps.iter().find(|step| step.id == id).unwrap();
    assert_eq!(step("verify").status, JobStepStatus::Done);
    assert!(
        step("recapture")
            .detail
            .starts_with("Not saved: the launcher signed in as account "),
        "{}",
        step("recapture").detail
    );
    assert!(
        block_on(fakes.backend.session(&new.id))
            .unwrap()
            .session
            .is_none()
    );
    fakes.backend.shut_down();
}

#[test]
fn a_sign_in_as_another_account_while_the_session_is_awaited_ends_the_switch_as_a_wrong_account_and_starts_no_game()
 {
    let first = "55555555555555555555555555555555";
    let fakes = fakes(None);
    let new = fakes.account("new@example.com");
    // As above: a sign-in by hand is logged as soon as the launcher is up, and at the next look the user signs in as Main. The hook never panics, so a failure here cannot strand the job.
    let launcher = Arc::clone(fakes.fake.epic.as_ref().unwrap());
    let processes = Arc::clone(&fakes.fake.platform.processes);
    let log = fakes
        .scratch
        .join("launchers")
        .join("localappdata/EpicGamesLauncher/Saved/Logs/EpicGamesLauncher.log");
    let main_uid = account_id_of(MAIN);
    let looks = std::sync::atomic::AtomicU32::new(0);
    fakes.fake.clock.before_sleep(move || {
        if !processes
            .is_running("EpicGamesLauncher.exe")
            .unwrap_or(false)
        {
            return;
        }
        match looks.fetch_add(1, std::sync::atomic::Ordering::SeqCst) {
            0 => {
                if let Ok(mut text) = fs::read_to_string(&log) {
                    text.push_str(&format!(
                        "[..]FAccountService::OnLoginComplete - 0 TRUE {first} \r\n"
                    ));
                    let _ = fs::write(&log, text);
                }
            }
            1 => launcher.sign_in_by_hand(Some(&main_uid)),
            _ => {}
        }
    });
    let states = fakes.play("new@example.com");
    let last = states.last().unwrap();
    assert!(
        last.state == JobState::Failed && !last.switched,
        "{:?}",
        last.error
    );
    assert_eq!(
        last.error.as_deref(),
        Some(
            "Epic Games Launcher signed in as Main (account …333333) instead of New (…555555). Sign in to the launcher as new@example.com, or fix the launcher account ID on the account page."
        )
    );
    let step = |id: &str| last.steps.iter().find(|step| step.id == id).unwrap();
    assert_eq!(step("verify").status, JobStepStatus::Done);
    assert_eq!(step("recapture").status, JobStepStatus::Failed);
    assert!(
        step("recapture")
            .detail
            .starts_with("Not saved: the launcher signed in as account …333333"),
        "{}",
        step("recapture").detail
    );
    assert_ne!(step("launch").status, JobStepStatus::Done);
    // The snapshot went back, as for a wrong account seen at the sign-in, and no game was started.
    assert_eq!(last.restored, Some(true));
    assert!(fs::read(fakes.settings_file()).unwrap() == fixture_settings());
    assert!(!asked_of_the_machine(&fakes).contains(&"shell.open".to_owned()));
    assert!(
        block_on(fakes.backend.session(&new.id))
            .unwrap()
            .session
            .is_none()
    );
    fakes.backend.shut_down();
}
