#![forbid(unsafe_code)]

// The backend's launcher commands for Battle.net on fake launchers, and where a saved token may be found afterwards: under the client's own registry key and, encrypted, in the database and a snapshot's manifest. Nowhere else.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

use futures_lite::future::block_on;
use ol_api::launchers::{
    BattlenetMachine, BattlenetTiming, EpicMachine, EpicTiming, Launchers, NewAccount, Secret,
    SteamTiming,
};
use ol_api::{ApiError, Backend, StartLaunch};
use ol_model::{JobState, JobStep, JobStepStatus, LaunchJob, LauncherId, SwitchPlan};
use ol_platform_fake::RegistryValue;
use ol_sim::FakeLaunchers;
use ol_testkit::vectors::sha256_hex;
use ol_testkit::{Scratch, oracle, snapshot};
use ol_vault::FakeVault;
use serde_json::{Value, json};
use time::macros::datetime;

const VERSION: &str = "9.8.7";
const SCENARIO: &str = "fixtures/dev-only/fake-launchers/all-four";
const CONFIG: &str = "appdata/Battle.net/Battle.net.config";
const UNIFIED_AUTH: &str = r"HKCU\Software\Blizzard Entertainment\Battle.net\UnifiedAuth\";
const MAIN: &str = "saved1@example.com";
const ALT: &str = "saved3@example.com";
const NEW: &str = "new@example.com";

struct Fakes {
    scratch: Scratch,
    fake: FakeLaunchers,
    backend: Backend,
    jobs: Receiver<LaunchJob>,
    // The job as it was last reported, where the job's own thread can read it.
    latest: Arc<Mutex<Option<LaunchJob>>>,
}

impl Fakes {
    fn config_file(&self) -> PathBuf {
        self.scratch.join("launchers").join(CONFIG)
    }

    // Stops the job the first time it waits while the named step runs, as the dialog's Stop does.
    fn stop_at(&self, step: &'static str) {
        let (backend, latest) = (self.backend.clone(), Arc::clone(&self.latest));
        self.fake.clock.before_sleep(move || {
            let job = latest.lock().unwrap().clone();
            let at_the_step = |job: &LaunchJob| {
                let running = |at: &JobStep| at.id == step && at.status == JobStepStatus::Running;
                job.steps.iter().any(running)
            };
            if let Some(job) = job.filter(at_the_step) {
                let _ = backend.cancel_launch(&job.id);
            }
        });
    }

    // The user signs in by hand as `uid` the first time the job waits while the named step runs. What that sign-in left under UnifiedAuth is kept for the test.
    fn sign_in_at(&self, step: &'static str, uid: &str) -> Arc<Mutex<Option<String>>> {
        let latest = Arc::clone(&self.latest);
        let client = Arc::clone(self.fake.battlenet.as_ref().unwrap());
        let registry = Arc::clone(&self.fake.registry);
        let (uid, done) = (uid.to_owned(), AtomicBool::new(false));
        let written = Arc::new(Mutex::new(None));
        let kept = Arc::clone(&written);
        self.fake.clock.before_sleep(move || {
            let job = latest.lock().unwrap().clone();
            let at_the_step = |job: &LaunchJob| {
                let running = |at: &JobStep| at.id == step && at.status == JobStepStatus::Running;
                job.steps.iter().any(running)
            };
            if job.is_some_and(|job| at_the_step(&job)) && !done.swap(true, Ordering::SeqCst) {
                client.sign_in_by_hand(Some(&uid));
                let token = registry
                    .dump()
                    .into_iter()
                    .find_map(|(path, value)| match value {
                        RegistryValue::Binary(hex) if path.starts_with(UNIFIED_AUTH) => Some(hex),
                        _ => None,
                    });
                *kept.lock().unwrap() = token.map(|hex| hex.to_lowercase());
            }
        });
        written
    }

    // The app quits and starts again on the same two folders, with a fake machine built afresh.
    fn restart(self) -> Self {
        self.backend.shut_down();
        start(self.scratch)
    }

    fn account(&self, login: &str) -> ol_model::Account {
        block_on(self.backend.accounts())
            .unwrap()
            .into_iter()
            .find(|account| account.launcher == LauncherId::Battlenet && account.login == login)
            .expect("the scenario seeds this account")
    }

    fn game(&self) -> ol_model::Game {
        let library = block_on(self.backend.library()).unwrap();
        let games = library.games.into_iter().map(|entry| entry.game);
        let mut found = games.filter(|game| game.launcher == LauncherId::Battlenet);
        found.next().expect("the scan found Battle.net's game")
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

    // The values under UnifiedAuth as the fake registry holds them: name and hex digits.
    fn tokens(&self) -> Vec<(String, String)> {
        let mut tokens: Vec<(String, String)> = self
            .fake
            .registry
            .dump()
            .into_iter()
            .filter_map(|(path, value)| match value {
                RegistryValue::Binary(hex) => Some((
                    path.strip_prefix(UNIFIED_AUTH)?.to_owned(),
                    hex.to_lowercase(),
                )),
                _ => None,
            })
            .collect();
        tokens.sort();
        tokens
    }
}

fn scenario() -> Value {
    let file = oracle(SCENARIO).join("scenario.json");
    serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap()
}

// The token the scenario saves for an account: a made-up one, and the thing that must not be found anywhere it does not belong.
fn seeded_token(login: &str) -> (String, String) {
    let scenario = scenario();
    let sessions = scenario["seed"]["sessions"].as_array().unwrap();
    let session = sessions
        .iter()
        .find(|session| session["launcher"] == "battlenet" && session["login"] == login)
        .expect("the scenario saves a sign-in for this account");
    let token = &session["blob"]["unifiedAuth"];
    let text = |name: &str| token[name].as_str().unwrap().to_owned();
    (text("name"), text("hex"))
}

// The token the scenario puts into the registry: the session the client remembers when a run begins.
fn remembered_token() -> (String, String) {
    let scenario = scenario();
    let (path, value) = scenario["registry"]
        .as_object()
        .unwrap()
        .iter()
        .find(|(path, _)| path.starts_with(UNIFIED_AUTH))
        .expect("the scenario's registry holds a token");
    let hex = value.as_str().unwrap().strip_prefix("binary:").unwrap();
    (path[UNIFIED_AUTH.len()..].to_owned(), hex.to_owned())
}

fn account_id_of(login: &str) -> String {
    let scenario = scenario();
    let accounts = scenario["seed"]["accounts"].as_array().unwrap();
    let account = accounts
        .iter()
        .find(|account| account["launcher"] == "battlenet" && account["login"] == login)
        .unwrap();
    account["launcherUid"].as_str().unwrap().to_owned()
}

fn fixture_config() -> Vec<u8> {
    fs::read(oracle("fixtures/launchers/battlenet/Battle.net.config")).unwrap()
}

// The logins of the fixture's SavedAccountNames, read off its line here and not by the code under test.
fn fixture_logins() -> Vec<String> {
    let text = String::from_utf8(fixture_config()).unwrap();
    let line = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("\"SavedAccountNames\": \""))
        .expect("the fixture has the list");
    let list = line.trim_end_matches(',').trim_end_matches('"');
    list.split(',').map(str::to_owned).collect()
}

// The scenario all-four laid down in a scratch folder, with its accounts and saved sign-ins seeded and its launchers scanned.
fn fakes(knobs: Option<&Value>) -> Fakes {
    let scratch = Scratch::new("api-battlenet");
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
    let latest = Arc::new(Mutex::new(None));
    let seen = Arc::clone(&latest);
    let backend = Backend::start_with(
        scratch.join("data"),
        VERSION,
        launchers,
        Some(Arc::new(move |job: &LaunchJob| {
            *seen.lock().unwrap() = Some(job.clone());
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
        latest,
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

// The files under a folder that hold `token` as hex digits, in either case.
fn files_holding(folder: &std::path::Path, token: &str) -> Vec<String> {
    snapshot(folder)
        .into_iter()
        .filter(|(_, bytes)| {
            holds(bytes, token.to_lowercase().as_bytes())
                || holds(bytes, token.to_uppercase().as_bytes())
        })
        .map(|(name, _)| name)
        .collect()
}

// A value as the stand-in vault of the fixtures stores it: each byte XOR 0x5a.
fn sealed_by_the_stand_in(text: &str) -> Vec<u8> {
    text.bytes().map(|byte| byte ^ 0x5a).collect()
}

// What the fake machine was asked to do to a process, a registry value or a launcher's file, in order.
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

fn detail(job: &LaunchJob, id: &str) -> String {
    let step = job.steps.iter().find(|step| step.id == id).unwrap();
    step.detail.clone()
}

#[test]
fn a_scan_finds_battlenets_game_and_the_client_lists_its_saved_logins() {
    let fakes = fakes(None);
    let recorded: Value = serde_json::from_str(
        &fs::read_to_string(oracle("l2/expected/play-battlenet.json")).unwrap(),
    )
    .unwrap();
    let recorded = recorded["final"]["db"]["tables"]["games"]
        .as_array()
        .unwrap()
        .iter()
        .find(|game| game["launcher"] == "battlenet")
        .unwrap()
        .clone();
    let game = fakes.game();
    assert_eq!(game.name, recorded["name"].as_str().unwrap());
    assert_eq!(
        game.launcher_game_id,
        recorded["launcher_game_id"].as_str().unwrap()
    );
    let folder = recorded["install_path"].as_str().unwrap();
    let below_the_scenario = folder.strip_prefix("{launchers}").unwrap();
    assert!(game.install_path.unwrap().ends_with(below_the_scenario));
    assert_eq!(game.needs_update, recorded["needs_update"] == 1);
    assert_eq!(
        game.last_played_at.as_deref(),
        recorded["last_played_at"].as_str()
    );
    // Hearthstone has an Uninstall entry too, and no folder: it is not installed.
    let from_files = block_on(fakes.backend.installed_games(LauncherId::Battlenet)).unwrap();
    assert_eq!(from_files.len(), 1);
    assert_eq!(from_files[0].launcher_game_id, game.launcher_game_id);

    let remembered = block_on(fakes.backend.remembered_accounts(LauncherId::Battlenet)).unwrap();
    let listed: Vec<&str> = remembered
        .iter()
        .map(|entry| entry.login.as_str())
        .collect();
    assert_eq!(listed, fixture_logins());
    assert!(remembered.iter().all(|entry| entry.launcher_uid.is_empty()));
    // The client's last run signed in as Main, and the token in the registry is that run's.
    let (main, alt) = (fakes.account(MAIN), fakes.account(ALT));
    let current: Vec<&str> = remembered
        .iter()
        .filter(|entry| entry.most_recent)
        .map(|entry| entry.login.as_str())
        .collect();
    assert_eq!(current, [MAIN]);
    let linked = |login: &str| {
        let entry = remembered.iter().find(|entry| entry.login == login);
        entry.unwrap().linked_account_id.clone()
    };
    assert_eq!(linked(MAIN), Some(main.id));
    assert_eq!(linked(ALT), Some(alt.id));
    assert_eq!(linked("saved2@example.com"), None);
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
        sha256_hex(seeded_token(ALT).1.as_bytes())
    );
    assert_eq!(session.launcher_uid, alt.launcher_uid);
    assert!(session.verified_at.is_none() && session.stale_reason.is_none());

    let state = saved
        .state
        .expect("the registry says what the client remembers");
    assert!(state.remembered && !state.launcher_running);
    assert_eq!(
        state.fingerprint,
        Some(sha256_hex(remembered_token().1.as_bytes()))
    );
    assert_eq!(state.linked_account_id.as_ref(), Some(&main.id));
    assert_eq!(state.launcher_uid, main.launcher_uid);
    assert!(state.last_sign_in.is_some_and(|last| last.ok));
    assert!(saved.state_error.is_none());
    fakes.backend.shut_down();
}

#[test]
fn a_saved_token_is_under_the_clients_key_and_encrypted_in_the_data_folder_and_nowhere_else() {
    let fakes = fakes(None);
    let alt = fakes.account(ALT);
    let (alts_name, saved_for_alt) = seeded_token(ALT);
    let (mains_name, remembered) = remembered_token();
    let secrets = [&saved_for_alt, &remembered];
    assert!(secrets.iter().all(|secret| secret.len() >= 40));
    assert_eq!(fakes.tokens(), [(mains_name.clone(), remembered.clone())]);

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
    said.push(format!("{:?}", fakes.fake.log.snapshot()));
    said.push(plan_text(
        &block_on(fakes.backend.switch_plan(&fakes.account(MAIN).id)).unwrap(),
    ));
    let saved = block_on(fakes.backend.session(&fakes.account(MAIN).id)).unwrap();
    let session = saved
        .session
        .expect("the switch saved the token it replaced");
    assert_eq!(session.fingerprint, sha256_hex(remembered.as_bytes()));
    said.push(format!(
        "{} {:?} {:?}",
        session.fingerprint, session.launcher_uid, saved.state_error
    ));
    // A restore is refused here, because the client runs, and the refusal says why.
    let snapshots = block_on(fakes.backend.backups(LauncherId::Battlenet)).unwrap();
    let refused = block_on(
        fakes
            .backend
            .restore_backup(LauncherId::Battlenet, &snapshots[0].id),
    )
    .err()
    .expect("a running client refuses a restore");
    said.push(format!("{refused:?} {refused}"));
    said.push(format!("{} {}", snapshots[0].reason, snapshots[0].path));
    let said = said.join("\n").to_lowercase();
    assert!(
        said.contains("fingerprint"),
        "the plans and the job speak of the token"
    );
    for secret in secrets {
        assert!(
            !said.contains(secret.as_str()),
            "a plan, a job, an error or the fake machine's log holds a token"
        );
    }
    fakes.backend.shut_down();

    // The client's own key holds Alt's token now, as it has to, and nothing else.
    assert_eq!(fakes.tokens(), [(alts_name, saved_for_alt.clone())]);

    // In the data folder a token is found only as the vault sealed it: in the database, and in the manifest of the snapshot.
    let data = fakes.scratch.join("data");
    for secret in secrets {
        assert_eq!(files_holding(&data, secret), Vec::<String>::new());
    }
    let database = fs::read(data.join("onelaunch.db")).unwrap();
    for stored in secrets {
        assert!(holds(&database, &sealed_by_the_stand_in(stored)));
    }
    let taken = snapshot(&data.join("backups").join("battlenet"));
    let manifests: Vec<&Vec<u8>> = taken
        .iter()
        .filter(|(name, _)| name.ends_with("manifest.json"))
        .map(|(_, bytes)| bytes)
        .collect();
    assert_eq!(manifests.len(), 1);
    let manifest: Value = serde_json::from_slice(manifests[0]).unwrap();
    let recorded: Value =
        serde_json::from_str(manifest["meta"]["unifiedAuth"].as_str().unwrap()).unwrap();
    assert_eq!(recorded.as_array().unwrap().len(), 1);
    assert_eq!(recorded[0]["name"], mains_name.as_str());
    assert!(
        recorded[0]["enc"]
            .as_str()
            .is_some_and(|enc| enc.len() >= 40)
    );

    // Under the scenario's folder a token is in the scenario itself, which is where the made-up ones came from, and in no file the app or the client wrote.
    let launchers = fakes.scratch.join("launchers");
    for secret in secrets {
        assert_eq!(files_holding(&launchers, secret), ["scenario.json"]);
    }
}

#[test]
fn a_switch_that_fails_after_its_writes_puts_the_token_and_the_list_back() {
    // Something else starts the client between the writes and the switch's own start.
    let knobs = json!({ "processes": { "reappearsAfterPolls": { "Battle.net.exe": 3 } } });
    let fakes = fakes(Some(&knobs));
    let before = fakes.tokens();
    let states = fakes.play(ALT);
    let last = states.last().unwrap();
    assert!(last.state == JobState::Failed);
    assert_eq!(last.restored, Some(true));
    assert!(last.backup_id.is_some());
    assert_eq!(
        last.error.as_deref(),
        Some(
            "Battle.net was started by something else while One Launch was switching accounts, so the switch was stopped. Try again."
        )
    );
    assert!(detail(last, "registry").starts_with("UnifiedAuth now holds the snapshot"));
    assert!(detail(last, "file").ends_with("is now first of 5 saved accounts."));
    let said = job_text(&states).to_lowercase();
    assert!(!said.contains(&seeded_token(ALT).1) && !said.contains(&remembered_token().1));

    assert_eq!(fakes.tokens(), before);
    assert!(fs::read(fakes.config_file()).unwrap() == fixture_config());
    assert!(block_on(fakes.backend.recovery()).unwrap().is_none());
    fakes.backend.shut_down();
}

#[test]
fn a_running_client_is_closed_by_its_window_once_the_user_has_said_yes() {
    let knobs = json!({ "knobs": { "battlenet": { "runningAs": account_id_of(MAIN) } } });
    let fakes = fakes(Some(&knobs));
    let (alt, game) = (fakes.account(ALT), fakes.game());
    let preflight = block_on(fakes.backend.launch_preflight(&alt.id, Some(&game.id))).unwrap();
    assert!(
        preflight.launcher_running && preflight.needs_confirmation && !preflight.already_active
    );
    assert_eq!(
        preflight.close_message.as_deref(),
        Some("Battle.net is running. Switching closes it.")
    );

    let unconfirmed = fakes.play_with(ALT, false).err().expect("a refusal");
    assert_eq!(unconfirmed.code, "CONFIRM_REQUIRED");
    assert!(asked_of_the_machine(&fakes).is_empty());

    let states = fakes.play_with(ALT, true).unwrap();
    let last = states.last().unwrap();
    assert!(last.state == JobState::Done && last.switched);
    assert_eq!(detail(last, "close"), "Client closed.");
    assert!(detail(last, "capture").starts_with("Saved Main's session (fingerprint "));
    assert_eq!(
        asked_of_the_machine(&fakes),
        [
            "proc.close",
            "reg.set",
            "reg.delete",
            "file.write",
            "proc.start",
            "proc.start"
        ]
    );
    fakes.backend.shut_down();
}

#[test]
fn a_client_that_runs_as_the_account_is_left_alone_and_the_game_starts_at_once() {
    let knobs = json!({ "knobs": { "battlenet": { "runningAs": account_id_of(MAIN) } } });
    let fakes = fakes(Some(&knobs));
    let (main, game) = (fakes.account(MAIN), fakes.game());
    let before = fakes.tokens();
    let preflight = block_on(fakes.backend.launch_preflight(&main.id, Some(&game.id))).unwrap();
    assert!(
        preflight.launcher_running && preflight.already_active && !preflight.needs_confirmation
    );

    let states = fakes.play(MAIN);
    let last = states.last().unwrap();
    assert!(last.state == JobState::Done && last.direct && last.backup_id.is_none());
    assert!(detail(last, "launch").ends_with("--exec=launch Pro (Battle.net started the game)"));
    assert_eq!(asked_of_the_machine(&fakes), ["proc.start"]);
    assert_eq!(fakes.tokens(), before);
    assert!(fs::read(fakes.config_file()).unwrap() == fixture_config());
    assert!(
        block_on(fakes.backend.backups(LauncherId::Battlenet))
            .unwrap()
            .is_empty()
    );
    fakes.backend.shut_down();
}

#[test]
fn an_account_without_a_saved_token_is_signed_in_by_hand_once_and_its_token_is_kept() {
    let signed_in_as = "0c3d4e5f";
    let knobs = json!({
        "knobs": { "battlenet": { "signInWhenAskedMs": 600, "signInAs": signed_in_as } },
    });
    let fakes = fakes(Some(&knobs));
    let new = fakes.account(NEW);
    assert!(!new.has_session && new.launcher_uid.is_none());
    let plan = block_on(fakes.backend.switch_plan(&new.id)).unwrap();
    assert!(plan.strategy == ol_model::SwitchStrategy::Credentials);

    let states = fakes.play(NEW);
    let last = states.last().unwrap();
    assert!(last.state == JobState::Done && last.attention.is_none());
    let asked = states.iter().find_map(|job| job.attention.clone());
    let asked = asked.expect("the job waited for the user");
    assert_eq!(asked.title, format!("Sign in to Battle.net as {NEW}"));
    assert!(asked.can_copy_password);
    assert_eq!(detail(last, "registry"), "UnifiedAuth cleared.");
    assert_eq!(
        detail(last, "file"),
        format!("{NEW} is now first of 6 saved accounts.")
    );
    assert_eq!(
        detail(last, "manual"),
        format!("Signed in as {NEW}. Linked to Battle.net account …4e5f.")
    );
    assert!(detail(last, "recapture").starts_with("Stored New's session (fingerprint "));

    // The token the client wrote at that sign-in is the account's now, and the job never spelled it out.
    let tokens = fakes.tokens();
    assert_eq!(tokens.len(), 1);
    let new = fakes.account(NEW);
    assert_eq!(new.launcher_uid.as_deref(), Some(signed_in_as));
    let saved = block_on(fakes.backend.session(&new.id)).unwrap();
    let session = saved.session.expect("the sign-in by hand was saved");
    assert_eq!(session.fingerprint, sha256_hex(tokens[0].1.as_bytes()));
    assert!(session.verified_at.is_some());
    assert!(!job_text(&states).to_lowercase().contains(&tokens[0].1));
    fakes.backend.shut_down();
    let data = fakes.scratch.join("data");
    assert_eq!(files_holding(&data, &tokens[0].1), Vec::<String>::new());
}

#[test]
fn a_failed_switch_is_not_put_back_under_a_client_that_will_not_close() {
    let probe = fakes(None);
    let main_uid = probe.account(MAIN).launcher_uid.unwrap();
    probe.backend.shut_down();
    let knobs = json!({
        "processes": { "elevated": ["Battle.net.exe"], "shutdownPolls": 100_000 },
        "knobs": { "battlenet": { "signInWhenAskedMs": 600, "signInAs": main_uid } },
        "timing": { "battlenet": { "shutdownMs": 1000, "killGraceMs": 500 } },
    });
    let fakes = fakes(Some(&knobs));
    let states = fakes.play(NEW);
    let last = states.last().unwrap();
    assert!(last.state == JobState::Failed);
    assert_eq!(last.restored, Some(false), "{:?}", last.error);
    let fakes = fakes.restart();
    assert!(block_on(fakes.backend.recovery()).unwrap().is_some());
    fakes.backend.shut_down();
}

#[test]
fn a_snapshot_puts_the_token_and_the_list_back_once_the_client_is_closed() {
    let fakes = fakes(None);
    let before = fakes.tokens();
    assert!(fakes.play(ALT).last().unwrap().state == JobState::Done);
    assert!(fs::read(fakes.config_file()).unwrap() != fixture_config());
    let taken = block_on(fakes.backend.backups(LauncherId::Battlenet)).unwrap();
    assert_eq!(taken.len(), 1);
    assert_eq!(taken[0].files, ["Battle.net.config"]);
    assert!(taken[0].pinned.is_none());
    let id = taken[0].id.clone();

    // The switch left the client running, and a running client writes its token and its config again.
    let refused = block_on(fakes.backend.restore_backup(LauncherId::Battlenet, &id))
        .err()
        .expect("a running client refuses a restore");
    assert_eq!(refused.code, "LAUNCHER_RUNNING");
    assert!(fakes.tokens() != before);

    // The user closes the client.
    fakes.fake.processes.set_running("Battle.net.exe", false);
    let restored = block_on(fakes.backend.restore_backup(LauncherId::Battlenet, &id)).unwrap();
    assert_eq!(restored.files.len(), 1);
    let (mains_name, remembered) = remembered_token();
    let fingerprint = sha256_hex(remembered.as_bytes());
    assert_eq!(
        restored.registry,
        [
            format!(
                "UnifiedAuth\\{mains_name} = ({} bytes, fingerprint {})",
                remembered.len() / 2,
                &fingerprint[..8]
            ),
            format!("UnifiedAuth\\{} (removed)", seeded_token(ALT).0),
        ]
    );
    assert_eq!(fakes.tokens(), before);
    assert!(fs::read(fakes.config_file()).unwrap() == fixture_config());
    // What was there before the restore was copied first, so the restore can be undone.
    let after = block_on(fakes.backend.backups(LauncherId::Battlenet)).unwrap();
    assert_eq!(after.len(), 2);
    assert_eq!(after[0].reason, format!("Before restoring {id}"));
    fakes.backend.shut_down();
}

#[test]
fn a_stop_while_the_client_closes_changes_nothing_and_one_after_its_start_puts_nothing_back() {
    let knobs = json!({ "knobs": { "battlenet": { "runningAs": account_id_of(MAIN) } } });
    let fakes = fakes(Some(&knobs));
    let before = fakes.tokens();
    fakes.stop_at("close");
    let states = fakes.play_with(ALT, true).unwrap();
    let last = states.last().unwrap();
    assert!(last.state == JobState::Cancelled && !last.switched);
    assert!(last.restored.is_none() && last.backup_id.is_none());
    assert_eq!(
        last.error.as_deref(),
        Some(
            "Stopped before Battle.net was started as saved3@example.com. Nothing had been changed."
        )
    );
    assert_eq!(detail(last, "close"), "Stopped.");
    assert_eq!(fakes.tokens(), before);
    assert!(fs::read(fakes.config_file()).unwrap() == fixture_config());
    fakes.backend.shut_down();

    // Once the client has been started again it reads what was written, so a stop leaves the writes standing.
    let fakes = self::fakes(None);
    fakes.stop_at("wait");
    let states = fakes.play(ALT);
    let last = states.last().unwrap();
    assert!(last.state == JobState::Cancelled && !last.switched);
    assert!(last.restored.is_none() && last.backup_id.is_some());
    assert_eq!(
        last.error.as_deref(),
        Some(
            "Stopped while Battle.net was starting as saved3@example.com. Nothing was rolled back: the client carries on signing in on its own."
        )
    );
    assert_eq!(fakes.tokens(), [seeded_token(ALT)]);
    assert!(block_on(fakes.backend.recovery()).unwrap().is_none());
    fakes.backend.shut_down();
}

#[test]
fn a_switch_the_last_run_did_not_finish_is_offered_and_put_back() {
    let fakes = fakes(None);
    let states = fakes.play(ALT);
    let job = states.last().unwrap();
    assert!(job.state == JobState::Done);
    let backup_id = job.backup_id.clone().expect("the switch took a snapshot");
    assert!(fs::read(fakes.config_file()).unwrap() != fixture_config());
    assert!(block_on(fakes.backend.recovery()).unwrap().is_none());

    // What a crash during the sign-in leaves: the journal of the fixtures, naming this run's launcher, account and snapshot.
    let mut journal: Value = serde_json::from_str(
        &fs::read_to_string(oracle("fixtures/journal/switch-signing-in.json")).unwrap(),
    )
    .unwrap();
    journal["launcher"] = "battlenet".into();
    journal["accountId"] = fakes.account(ALT).id.into();
    journal["backupId"] = backup_id.clone().into();
    let journal_file = fakes.scratch.join("data").join("switch.json");
    fakes.backend.shut_down();
    fs::write(&journal_file, journal.to_string()).unwrap();
    let fakes = start(fakes.scratch);

    let pending = block_on(fakes.backend.recovery())
        .unwrap()
        .expect("a recovery");
    assert!(pending.backup_id == backup_id && pending.snapshot_available);
    assert_eq!(pending.phase, ol_model::RecoveryPhase::SigningIn);
    assert_eq!(pending.launcher, LauncherId::Battlenet);
    assert_eq!(pending.account_name.as_deref(), Some("Alt"));

    let restored = block_on(fakes.backend.restore_recovery()).unwrap();
    assert!(restored.id == backup_id);
    assert_eq!(restored.files.len(), 1);
    assert_eq!(fakes.tokens(), [remembered_token()]);
    assert!(fs::read(fakes.config_file()).unwrap() == fixture_config());
    assert!(block_on(fakes.backend.recovery()).unwrap().is_none());
    assert!(!journal_file.exists());

    fs::write(&journal_file, journal.to_string()).unwrap();
    let fakes = fakes.restart();
    assert!(block_on(fakes.backend.recovery()).unwrap().is_some());
    block_on(fakes.backend.dismiss_recovery()).unwrap();
    assert!(block_on(fakes.backend.recovery()).unwrap().is_none());
    assert!(!journal_file.exists());
    fakes.backend.shut_down();
}

#[test]
fn a_client_signed_in_as_the_account_that_exits_before_the_switch_reaches_it_is_started_and_its_sign_in_confirmed()
 {
    let knobs = json!({ "knobs": { "battlenet": { "runningAs": account_id_of(MAIN) } } });
    let fakes = fakes(Some(&knobs));
    fakes.fake.processes.exit_once_seen("Battle.net.exe");
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
fn a_sign_in_that_lands_while_the_client_closes_is_saved_before_the_switch_takes_its_token_away() {
    // The client runs on an account no One Launch account is linked to, so the plan has no outgoing sign-in to save.
    let knobs = json!({ "knobs": { "battlenet": { "runningAs": "0f0f0f0f" } } });
    let fakes = fakes(Some(&knobs));
    let main = fakes.account(MAIN);
    assert!(
        block_on(fakes.backend.session(&main.id))
            .unwrap()
            .session
            .is_none()
    );
    let written = fakes.sign_in_at("close", &account_id_of(MAIN));

    let states = fakes.play_with(ALT, true).unwrap();
    let last = states.last().unwrap();
    assert!(last.state == JobState::Done, "{:?}", last.error);
    assert!(detail(last, "capture").starts_with("Saved Main's session (fingerprint "));
    let token = written.lock().unwrap().clone().expect("the user signed in");
    let saved = block_on(fakes.backend.session(&main.id)).unwrap().session;
    let saved = saved.expect("the sign-in made during the close was saved");
    assert_eq!(saved.fingerprint, sha256_hex(token.as_bytes()));
    assert_eq!(fakes.tokens(), [seeded_token(ALT)]);
    fakes.backend.shut_down();
}

#[test]
fn a_login_put_first_by_the_client_as_it_closes_is_put_back_behind_the_target() {
    // The list on disk has the target first; the client signed in as Main writes Main first as it exits.
    let fakes = fakes(Some(
        &json!({ "knobs": { "battlenet": { "runningAs": account_id_of(MAIN) } } }),
    ));
    let config = String::from_utf8(fixture_config()).unwrap();
    let logins = fixture_logins();
    let mut alt_first = vec![ALT.to_owned()];
    alt_first.extend(logins.iter().filter(|login| *login != ALT).cloned());
    fs::write(
        fakes.config_file(),
        config.replace(&logins.join(","), &alt_first.join(",")),
    )
    .unwrap();
    let plan = block_on(fakes.backend.switch_plan(&fakes.account(ALT).id)).unwrap();
    let file = plan.steps.iter().find(|step| step.id == "file").unwrap();
    assert!(file.skipped && file.detail.starts_with("No change:"));
    fakes.sign_in_at("close", &account_id_of(MAIN));

    let states = fakes.play_with(ALT, true).unwrap();
    let last = states.last().unwrap();
    assert!(last.state == JobState::Done, "{:?}", last.error);
    assert_eq!(
        detail(last, "file"),
        format!("{ALT} is now first of 5 saved accounts.")
    );
    assert!(detail(last, "verify").starts_with(&format!("Signed in as {ALT}.")));
    let after = String::from_utf8(fs::read(fakes.config_file()).unwrap()).unwrap();
    assert!(after.contains(&format!("\"SavedAccountNames\": \"{ALT},")));
    fakes.backend.shut_down();
}

#[test]
fn a_sign_in_by_hand_seen_at_the_first_look_keeps_its_token() {
    let signed_in_as = "0c3d4e5f";
    let knobs = json!({
        "knobs": { "battlenet": { "signInWhenAskedMs": 0, "signInAs": signed_in_as } },
    });
    let fakes = fakes(Some(&knobs));
    let new = fakes.account(NEW);
    let states = fakes.play(NEW);
    let last = states.last().unwrap();
    assert!(last.state == JobState::Done, "{:?}", last.error);
    assert_eq!(
        detail(last, "verify"),
        format!("Signed in as {NEW}. Linked to Battle.net account …4e5f.")
    );
    assert!(detail(last, "recapture").starts_with("Stored New's session (fingerprint "));
    let tokens = fakes.tokens();
    assert_eq!(tokens.len(), 1);
    let saved = block_on(fakes.backend.session(&new.id)).unwrap().session;
    let saved = saved.expect("the sign-in by hand was saved");
    assert_eq!(saved.fingerprint, sha256_hex(tokens[0].1.as_bytes()));
    assert!(saved.verified_at.is_some());
    fakes.backend.shut_down();
}

#[test]
fn a_value_that_was_under_unified_auth_before_the_client_started_is_never_saved_as_the_accounts() {
    let fakes = fakes(None);
    let (alt_name, alt_hex) = seeded_token(ALT);
    let (planted, _) = remembered_token();
    // UnifiedAuth holds Alt's own saved token and, beside it, a value another program left there; Alt's login is first, so the client signs in from Alt's.
    let registry = &fakes.fake.registry;
    registry.remove(&format!("{UNIFIED_AUTH}{planted}"));
    registry.seed(
        &format!("{UNIFIED_AUTH}{alt_name}"),
        RegistryValue::Binary(alt_hex.clone()),
    );
    let foreign = format!("01000000d08c9ddf00ff{}", "cd".repeat(24));
    registry.seed(
        &format!("{UNIFIED_AUTH}CC33DD44"),
        RegistryValue::Binary(foreign.clone()),
    );
    let config = String::from_utf8(fs::read(fakes.config_file()).unwrap()).unwrap();
    let reordered = config.replacen(
        "saved1@example.com,saved2@example.com,saved3@example.com",
        "saved3@example.com,saved1@example.com,saved2@example.com",
        1,
    );
    assert!(reordered != config);
    fs::write(fakes.config_file(), reordered).unwrap();

    let states = fakes.play(ALT);
    let last = states.last().unwrap();
    assert!(last.state == JobState::Done, "{:?}", last.error);
    assert_eq!(detail(last, "verify"), format!("Signed in as {ALT}."));
    assert_eq!(detail(last, "recapture"), "The stored session is current.");
    let saved = block_on(fakes.backend.session(&fakes.account(ALT).id)).unwrap();
    assert_eq!(
        saved.session.unwrap().fingerprint,
        sha256_hex(alt_hex.as_bytes())
    );
    fakes.backend.shut_down();
}

#[test]
fn a_sign_in_as_another_account_while_the_token_is_awaited_is_not_saved_for_the_first() {
    let first = "0c3d4e5f";
    let fakes = fakes(None);
    let new = fakes.account(NEW);
    // The client logs a sign-in by hand once it is up, before the switch's first look, and has not written its token yet; at the next look the user signs in as Main.
    let client = Arc::clone(fakes.fake.battlenet.as_ref().unwrap());
    let processes = Arc::clone(&fakes.fake.platform.processes);
    let logs = fakes
        .scratch
        .join("launchers")
        .join("localappdata/Battle.net/Logs");
    let main_uid = account_id_of(MAIN);
    let looks = std::sync::atomic::AtomicU32::new(0);
    fakes.fake.clock.before_sleep(move || {
        if !processes.is_running("Battle.net.exe").unwrap() {
            return;
        }
        match looks.fetch_add(1, Ordering::SeqCst) {
            0 => {
                let mut names: Vec<_> = fs::read_dir(&logs)
                    .unwrap()
                    .flatten()
                    .map(|entry| entry.path())
                    .collect();
                names.sort();
                let newest = names.pop().unwrap();
                let mut text = fs::read_to_string(&newest).unwrap();
                text.push_str(&format!(
                    "I 2026-09-13 03:21:00.000000 [BNLogin] {{Main}} Logged into Battle.net successfully. |bnet=1:0:{first}|game=2:100000000:0a1b2c3\r\n"
                ));
                fs::write(&newest, text).unwrap();
            }
            1 => client.sign_in_by_hand(Some(&main_uid)),
            _ => {}
        }
    });
    let states = fakes.play(NEW);
    let last = states.last().unwrap();
    assert_eq!(
        detail(last, "verify"),
        format!("Signed in as {NEW}. Linked to Battle.net account …4e5f.")
    );
    assert!(
        detail(last, "recapture").starts_with("Not saved: the client signed in as account "),
        "{}",
        detail(last, "recapture")
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
fn a_sign_in_as_another_account_while_the_token_is_awaited_ends_the_switch_as_a_wrong_account_and_starts_no_game()
 {
    let first = "0c3d4e5f";
    let fakes = fakes(None);
    let new = fakes.account(NEW);
    let before = fakes.tokens();
    // As above: a sign-in by hand is logged once the client is up, and at the next look the user signs in as Main. The hook never panics, so a failure here cannot strand the job.
    let client = Arc::clone(fakes.fake.battlenet.as_ref().unwrap());
    let processes = Arc::clone(&fakes.fake.platform.processes);
    let logs = fakes
        .scratch
        .join("launchers")
        .join("localappdata/Battle.net/Logs");
    let main_uid = account_id_of(MAIN);
    let looks = std::sync::atomic::AtomicU32::new(0);
    fakes.fake.clock.before_sleep(move || {
        if !processes.is_running("Battle.net.exe").unwrap_or(false) {
            return;
        }
        match looks.fetch_add(1, Ordering::SeqCst) {
            0 => {
                let mut names: Vec<_> = fs::read_dir(&logs)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|entry| entry.path())
                    .collect();
                names.sort();
                if let Some(newest) = names.pop()
                    && let Ok(mut text) = fs::read_to_string(&newest)
                {
                    text.push_str(&format!(
                        "I 2026-09-13 03:21:00.000000 [BNLogin] {{Main}} Logged into Battle.net successfully. |bnet=1:0:{first}|game=2:100000000:0a1b2c3\r\n"
                    ));
                    let _ = fs::write(&newest, text);
                }
            }
            1 => client.sign_in_by_hand(Some(&main_uid)),
            _ => {}
        }
    });
    let states = fakes.play(NEW);
    let last = states.last().unwrap();
    assert!(
        last.state == JobState::Failed && !last.switched,
        "{:?}",
        last.error
    );
    assert_eq!(
        last.error.as_deref(),
        Some(
            "Battle.net signed in as Main (account …2c3d) instead of New (…4e5f). Sign in to the client as new@example.com, or fix the account ID on the account page."
        )
    );
    let step = |id: &str| last.steps.iter().find(|step| step.id == id).unwrap();
    assert_eq!(
        detail(last, "verify"),
        format!("Signed in as {NEW}. Linked to Battle.net account …4e5f.")
    );
    assert_eq!(step("recapture").status, JobStepStatus::Failed);
    assert_eq!(
        detail(last, "recapture"),
        "Not saved: the client signed in as account …2c3d meanwhile."
    );
    assert_ne!(step("launch").status, JobStepStatus::Done);
    // The snapshot went back, as for a wrong account seen at the sign-in, and the client was started once, by the switch, and never for the game.
    assert_eq!(last.restored, Some(true));
    assert_eq!(fakes.tokens(), before);
    assert!(fs::read(fakes.config_file()).unwrap() == fixture_config());
    let asked = asked_of_the_machine(&fakes);
    assert_eq!(asked.iter().filter(|op| *op == "proc.start").count(), 1);
    assert!(
        block_on(fakes.backend.session(&new.id))
            .unwrap()
            .session
            .is_none()
    );
    fakes.backend.shut_down();
}
