#![forbid(unsafe_code)]

// The backend's launcher commands for Riot on fake launchers, and Riot's rules held on them: a running Riot game or a process list that cannot be read refuses everything and nothing is touched; the session file is never made; what the file holds is stored for its owner before it is overwritten; and a sign-in is found afterwards in the client's own file and, sealed, in the database and a snapshot's encrypted copy, and nowhere else.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

use futures_lite::future::block_on;
use ol_api::launchers::{
    BattlenetMachine, BattlenetTiming, EpicMachine, EpicTiming, Launchers, NewAccount, RiotMachine,
    RiotTiming, Secret, SteamTiming,
};
use ol_api::{ApiError, Backend, StartLaunch};
use ol_model::{
    JobState, JobStep, JobStepStatus, LaunchJob, LauncherId, SettingKey, SwitchPlan, SwitchStrategy,
};
use ol_sim::FakeLaunchers;
use ol_testkit::vectors::sha256_hex;
use ol_testkit::{Scratch, oracle, snapshot};
use ol_vault::FakeVault;
use serde_json::{Value, json};
use time::macros::datetime;

const VERSION: &str = "9.8.7";
const SCENARIO: &str = "fixtures/dev-only/fake-launchers/all-four";
const SESSION_FILE: &str = "localappdata/Riot Games/Riot Client/Data/RiotGamesPrivateSettings.yaml";
const LOCKFILE: &str = "localappdata/Riot Games/Riot Client/Config/lockfile";
const EXE: &str = "RiotClientServices.exe";
const MAIN: &str = "riot_main";
const ALT: &str = "riot_alt";
const NEW: &str = "riot_new";
const MAIN_UID: &str = "11111111-1111-4111-8111-111111111111";
const ALT_UID: &str = "33333333-3333-4333-8333-333333333333";
const NEW_UID: &str = "44444444-4444-4444-8444-444444444444";
// A Riot ID no other text of a run can hold by chance.
const GAME_NAME: &str = "Quillfeather";
const TAG_LINE: &str = "QF77";
const HAND_OFF: &str =
    "Press Play in the Riot Client window to start VALORANT; it is open on the game's page.";

struct Fakes {
    scratch: Scratch,
    fake: FakeLaunchers,
    backend: Backend,
    jobs: Receiver<LaunchJob>,
    // The job as it was last reported, where the job's own thread can read it.
    latest: Arc<Mutex<Option<LaunchJob>>>,
}

impl Fakes {
    fn launchers(&self) -> PathBuf {
        self.scratch.join("launchers")
    }

    fn session_file(&self) -> PathBuf {
        self.launchers().join(SESSION_FILE)
    }

    fn on_disk(&self) -> String {
        String::from_utf8(fs::read(self.session_file()).unwrap()).unwrap()
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

    fn account(&self, login: &str) -> ol_model::Account {
        block_on(self.backend.accounts())
            .unwrap()
            .into_iter()
            .find(|account| account.launcher == LauncherId::Riot && account.login == login)
            .expect("the scenario seeds this account")
    }

    fn game(&self) -> ol_model::Game {
        let library = block_on(self.backend.library()).unwrap();
        let games = library.games.into_iter().map(|entry| entry.game);
        let mut found = games.filter(|game| game.launcher == LauncherId::Riot);
        found.next().expect("the scan found Riot's game")
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

    // The fingerprint of what is saved for an account, if anything is.
    fn saved(&self, login: &str) -> Option<String> {
        let saved = block_on(self.backend.session(&self.account(login).id)).unwrap();
        saved.session.map(|session| session.fingerprint)
    }

    // "Start Riot games for you" turned off: the client is handed the game and nothing is pressed, so no game runs afterwards.
    fn without_the_press(self) -> Self {
        let off = "false".to_owned();
        block_on(self.backend.set_setting(SettingKey::RiotPressPlay, off)).unwrap();
        self
    }

    // The user exits the client from its tray icon.
    fn client_exits(&self) {
        for image in [EXE, "Riot Client.exe"] {
            self.fake.processes.set_running(image, false);
        }
    }
}

fn scenario() -> Value {
    let file = oracle(SCENARIO).join("scenario.json");
    serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap()
}

// The refresh token the scenario saves for Alt: a made-up one.
fn alts_token() -> String {
    let scenario = scenario();
    let sessions = scenario["seed"]["sessions"].as_array().unwrap();
    let session = sessions
        .iter()
        .find(|session| session["launcher"] == "riot" && session["login"] == ALT)
        .expect("the scenario saves a sign-in for Alt");
    session["refreshToken"].as_str().unwrap().to_owned()
}

fn mains_token() -> String {
    format!("made-up-token-of-riot-main-{}", "m".repeat(48))
}

fn mains_id_token() -> String {
    ol_sim::fake_id_token(MAIN_UID, GAME_NAME, TAG_LINE)
}

fn fixture_file() -> String {
    let file = oracle("fixtures/launchers/riot/RiotGamesPrivateSettings.yaml");
    String::from_utf8(fs::read(file).unwrap()).unwrap()
}

// The client's file with a recognisable sign-in for Main in it: a refresh token, an id token and a Riot ID that nothing else in a run can hold by chance. The rest of the file is the fixture's, its device cookie and the cookies of older builds among it.
fn planted_file() -> String {
    let block =
        ol_sim::session_block_lines(&mains_id_token(), &mains_token(), 3, 1_760_000_000_000);
    ol_sim::session_file_with(&fixture_file(), &block)
}

// The n-th refresh token the scripted client makes up.
fn clients_token(number: u32) -> String {
    format!("placeholder-refresh-{number}-{}", "x".repeat(48))
}

// The scenario all-four laid down in a scratch folder, with Main's planted sign-in in the client's file, its accounts and saved sign-ins seeded and its launchers scanned.
fn fakes(knobs: Option<&Value>) -> Fakes {
    fakes_with(knobs, &|_| {})
}

fn fakes_with(knobs: Option<&Value>, prepare: &dyn Fn(&Path)) -> Fakes {
    let scratch = Scratch::new("api-riot");
    let launchers_dir = scratch.join("launchers");
    ol_sim::lay_down(
        &oracle(SCENARIO),
        &oracle("fixtures/launchers"),
        &launchers_dir,
    )
    .unwrap();
    fs::write(launchers_dir.join(SESSION_FILE), planted_file()).unwrap();
    prepare(&launchers_dir);
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
    launchers.riot = RiotMachine::at(
        RiotTiming::with(&fake.riot_timing),
        fake.riot_default_dir.clone(),
        fake.local_app_data.clone(),
        fake.program_data.clone(),
    )
    .with_local_launch(fake.riot_launch.clone());
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
    text.push(format!(
        "{:?} {:?}",
        plan.account.launcher_uid, plan.most_recent_login
    ));
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

// The files under a folder that hold a text, as it is written or in one case throughout.
fn files_holding(folder: &Path, text: &str) -> Vec<String> {
    snapshot(folder)
        .into_iter()
        .filter(|(_, bytes)| {
            [text.to_owned(), text.to_lowercase(), text.to_uppercase()]
                .iter()
                .any(|spelling| holds(bytes, spelling.as_bytes()))
        })
        .map(|(name, _)| name)
        .collect()
}

// A value as the stand-in vault of the fixtures stores it: each byte XOR 0x5a.
fn sealed_by_the_stand_in(text: &str) -> Vec<u8> {
    text.bytes().map(|byte| byte ^ 0x5a).collect()
}

// Whether a file holds a text as the stand-in vault sealed it. The database keeps a long value on more than one page, so of a text cut by a page's end one half is found whole.
fn holds_sealed(haystack: &[u8], text: &str) -> bool {
    let sealed = sealed_by_the_stand_in(text);
    let (first, second) = sealed.split_at(sealed.len() / 2);
    holds(haystack, first) || holds(haystack, second)
}

fn base64(bytes: &[u8]) -> String {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let held = chunk
            .iter()
            .fold(0_u32, |held, byte| (held << 8) | u32::from(*byte))
            << (8 * (3 - chunk.len()));
        for at in 0..4 {
            if at <= chunk.len() {
                out.push(alphabet[(held >> (18 - 6 * at)) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

// What the fake machine was asked to do to a process or a launcher's file, in order. A snapshot's manifest is the app's own file.
fn asked_of_the_machine(fakes: &Fakes) -> Vec<String> {
    let events = fakes.fake.log.snapshot();
    let named = events.iter().filter_map(|event| {
        let op = event["op"].as_str()?;
        let file = event["path"].as_str().unwrap_or_default();
        let manifest = file.ends_with("manifest.json");
        (!manifest).then(|| match event.get("tree") {
            Some(tree) => format!("{op} tree={tree}"),
            None => op.to_owned(),
        })
    });
    named.collect()
}

fn detail(job: &LaunchJob, id: &str) -> String {
    let step = job.steps.iter().find(|step| step.id == id).unwrap();
    step.detail.clone()
}

fn refusal<T>(answer: Result<T, ApiError>) -> (&'static str, String) {
    let error = answer.err().expect("a refusal");
    (error.code, error.message)
}

#[test]
fn a_scan_finds_riots_game_and_the_client_names_the_account_whose_sign_in_it_holds() {
    let fakes = fakes(None);
    let recorded: Value =
        serde_json::from_str(&fs::read_to_string(oracle("l2/expected/play-riot.json")).unwrap())
            .unwrap();
    let recorded = recorded["final"]["db"]["tables"]["games"]
        .as_array()
        .unwrap()
        .iter()
        .find(|game| game["launcher"] == "riot")
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
    assert!(fakes.backend.can_reach(LauncherId::Riot));

    // The file names one account by its id, and no login: the client keeps no list of past sign-ins.
    let remembered = block_on(fakes.backend.remembered_accounts(LauncherId::Riot)).unwrap();
    assert_eq!(remembered.len(), 1);
    let main = fakes.account(MAIN);
    assert_eq!(remembered[0].launcher_uid, MAIN_UID);
    assert_eq!(remembered[0].login, MAIN);
    assert_eq!(remembered[0].persona_name, main.display_name);
    assert_eq!(remembered[0].linked_account_id, Some(main.id));
    fakes.backend.shut_down();
}

#[test]
fn what_is_saved_of_a_sign_in_is_its_digest_and_whose_it_is() {
    let fakes = fakes(None);
    let (main, alt) = (fakes.account(MAIN), fakes.account(ALT));
    assert!(alt.has_session && !main.has_session);

    let saved = block_on(fakes.backend.session(&alt.id)).unwrap();
    let session = saved.session.expect("the scenario saved a sign-in for Alt");
    assert_eq!(session.fingerprint, sha256_hex(alts_token().as_bytes()));
    assert_eq!(session.launcher_uid.as_deref(), Some(ALT_UID));
    assert!(session.verified_at.is_none() && session.stale_reason.is_none());

    let state = saved.state.expect("the file says whose sign-in it holds");
    assert!(state.remembered && !state.launcher_running);
    assert_eq!(
        state.fingerprint,
        Some(sha256_hex(mains_token().as_bytes()))
    );
    assert_eq!(state.launcher_uid.as_deref(), Some(MAIN_UID));
    assert_eq!(state.linked_account_id.as_ref(), Some(&main.id));
    assert!(state.last_sign_in.is_some_and(|last| last.ok));
    assert!(saved.state_error.is_none());
    fakes.backend.shut_down();
}

#[test]
fn a_sign_in_is_in_the_clients_own_file_and_sealed_in_the_data_folder_and_nowhere_else() {
    // The client keeps the token it is given, so that what the switch wrote can be looked for.
    let knobs = json!({ "knobs": { "riot": { "rotateOnStart": false } } });
    let fakes = fakes(Some(&knobs));
    let (alt, game) = (fakes.account(ALT), fakes.game());
    let planted = planted_file();
    let alts_id_token = ol_sim::fake_id_token(ALT_UID, "Placeholder", "0000");
    let payload = |token: &str| token.split('.').nth(1).unwrap().to_owned();
    // The device cookie and the cookies of older builds, which are in the file too.
    let cookies: Vec<String> = fixture_file()
        .lines()
        .filter_map(|line| line.trim().strip_prefix("value: \""))
        .map(|value| value.trim_end_matches('"').to_owned())
        .filter(|value| value.len() >= 40)
        .collect();
    assert!(cookies.len() >= 3);
    let mut secrets = vec![
        mains_token(),
        mains_id_token(),
        payload(&mains_id_token()),
        alts_token(),
        alts_id_token.clone(),
        payload(&alts_id_token),
    ];
    secrets.extend(cookies);
    // Every one of them is in Main's file or in the copy saved for Alt. The Riot ID is in the id token, which names it in base64: a token made for another name is another token.
    let alts_file = ol_sim::fake_session_file(ALT_UID, &alts_token());
    for secret in &secrets {
        assert!(planted.contains(secret) || alts_file.contains(secret));
    }
    assert!(mains_id_token() != ol_sim::fake_id_token(MAIN_UID, "Placeholder", "0000"));
    secrets.extend([GAME_NAME.to_owned(), TAG_LINE.to_owned()]);

    let mut said = vec![
        plan_text(&block_on(fakes.backend.switch_plan(&alt.id)).unwrap()),
        plan_text(
            &block_on(fakes.backend.launch_preflight(&alt.id, Some(&game.id)))
                .unwrap()
                .plan,
        ),
    ];
    let states = fakes.play(ALT);
    let last = states.last().unwrap();
    assert!(last.state == JobState::Done);
    assert!(detail(last, "capture").starts_with("Saved Riot Main's sign-in (fingerprint "));
    assert!(detail(last, "file").contains("now holds the saved copy (fingerprint "));
    said.push(job_text(&states));
    said.push(json!(fakes.fake.log.snapshot()).to_string());
    said.push(format!("{:?}", fakes.fake.log.snapshot()));
    let main = fakes.account(MAIN);
    said.push(plan_text(
        &block_on(fakes.backend.switch_plan(&main.id)).unwrap(),
    ));
    let saved = block_on(fakes.backend.session(&main.id)).unwrap();
    let session = saved
        .session
        .expect("the switch saved the sign-in it replaced");
    assert_eq!(session.fingerprint, sha256_hex(mains_token().as_bytes()));
    said.push(format!(
        "{} {:?} {:?}",
        session.fingerprint, session.launcher_uid, saved.state_error
    ));
    // A restore is refused here, because the client runs, and the refusal says why.
    let snapshots = block_on(fakes.backend.backups(LauncherId::Riot)).unwrap();
    let refused = block_on(
        fakes
            .backend
            .restore_backup(LauncherId::Riot, &snapshots[0].id),
    )
    .err()
    .expect("a running client refuses a restore");
    said.push(format!("{refused:?} {refused}"));
    said.push(format!("{} {}", snapshots[0].reason, snapshots[0].path));
    let said = said.join("\n");
    assert!(
        said.contains("fingerprint") && said.contains("…1111"),
        "the plans and the job speak of the sign-in"
    );
    for secret in &secrets {
        let found = [secret.clone(), secret.to_lowercase()]
            .iter()
            .any(|spelling| said.contains(spelling) || said.to_lowercase().contains(spelling));
        assert!(
            !found,
            "a plan, a job, an error or the fake machine's log holds a part of a sign-in"
        );
    }
    fakes.backend.shut_down();

    // The client's own file holds Alt's saved copy now, whole, as it has to.
    assert!(fakes.on_disk() == ol_sim::fake_session_file(ALT_UID, &alts_token()));

    // In the data folder nothing of a sign-in is found in the clear.
    let data = fakes.scratch.join("data");
    for secret in &secrets {
        assert_eq!(files_holding(&data, secret), Vec::<String>::new());
    }
    // It is there as the vault sealed it: both accounts' files in the database,
    let database = fs::read(data.join("onelaunch.db")).unwrap();
    for stored in [mains_token(), mains_id_token(), alts_token(), alts_id_token] {
        assert!(holds_sealed(&database, &stored));
    }
    // and the file the switch replaced in the snapshot, as an encrypted copy and no other.
    let taken = snapshot(&data.join("backups").join("riot"));
    let files: Vec<&str> = taken
        .keys()
        .map(String::as_str)
        .filter(|name| !name.ends_with('/'))
        .map(|name| name.split_once('/').unwrap().1)
        .collect();
    assert_eq!(
        files,
        ["Data/RiotGamesPrivateSettings.yaml.enc", "manifest.json"]
    );
    let copy = taken
        .iter()
        .find(|(name, _)| name.ends_with(".enc"))
        .map(|(_, bytes)| bytes.clone())
        .unwrap();
    assert!(copy == sealed_by_the_stand_in(&base64(planted.as_bytes())));
    let manifest = taken
        .iter()
        .find(|(name, _)| name.ends_with("manifest.json"))
        .map(|(_, bytes)| serde_json::from_slice::<Value>(bytes).unwrap())
        .unwrap();
    assert_eq!(manifest["files"][0]["encrypted"], true);
    assert_eq!(manifest["meta"], json!({}));

    // Under the scenario's folder Alt's token is in the client's own file, where the switch put it, and in the scenario, where the made-up one came from. Main's is in no file there any more.
    let launchers = fakes.launchers();
    assert_eq!(
        files_holding(&launchers, &alts_token()),
        [SESSION_FILE, "scenario.json"]
    );
    assert_eq!(
        files_holding(&launchers, &mains_token()),
        Vec::<String>::new()
    );
    assert_eq!(files_holding(&launchers, GAME_NAME), Vec::<String>::new());
}

#[test]
fn a_running_riot_game_refuses_the_switch_and_the_restore_and_nothing_is_written_or_closed() {
    let running = json!({ "knobs": { "riot": { "runningAs": MAIN_UID, "rotateOnStart": false } } });
    let fakes = fakes(Some(&running));
    let (alt, game) = (fakes.account(ALT), fakes.game());
    // The game is listed by its name alone, as it is to an app that never asks for a path.
    fakes
        .fake
        .processes
        .add_name_to_table("VALORANT-Win64-Shipping.exe");
    let message = "Riot Games: VALORANT is running. Close it, then switch: One Launch never closes a Riot game.";
    let preflight = block_on(fakes.backend.launch_preflight(&alt.id, Some(&game.id)));
    assert_eq!(refusal(preflight), ("GAME_RUNNING", message.to_owned()));
    // Asked first, the switch is refused before a job is made. Told that the client may be closed, a job is made and refuses before its first step, in the same words: there is no yes that closes a Riot game.
    let refused = fakes.play_with(ALT, false);
    assert_eq!(refusal(refused), ("GAME_RUNNING", message.to_owned()));
    let states = fakes.play_with(ALT, true).unwrap();
    let last = states.last().unwrap();
    assert!(last.state == JobState::Failed && !last.switched && last.backup_id.is_none());
    assert_eq!(last.error.as_deref(), Some(message));
    // The account the client already runs as is refused the same way: the game is not looked past.
    assert_eq!(refusal(fakes.play_with(MAIN, false)).0, "GAME_RUNNING");
    let plan = block_on(fakes.backend.switch_plan(&alt.id)).unwrap();
    assert_eq!(
        plan.warnings[0],
        "VALORANT is running. Close it first: One Launch never closes a Riot game, and no switch runs while one does."
    );
    assert!(asked_of_the_machine(&fakes).is_empty());
    assert!(fakes.on_disk() == planted_file());
    assert!(
        block_on(fakes.backend.backups(LauncherId::Riot))
            .unwrap()
            .is_empty()
    );
    assert!(fakes.saved(MAIN).is_none());
    assert!(fakes.fake.platform.processes.is_running(EXE).unwrap());
    fakes.backend.shut_down();

    // A restore: the client is closed, as a restore asks, and a game runs.
    let fakes = self::fakes(None).without_the_press();
    assert!(fakes.play(ALT).last().unwrap().state == JobState::Done);
    let id = block_on(fakes.backend.backups(LauncherId::Riot)).unwrap()[0]
        .id
        .clone();
    fakes.client_exits();
    fakes.fake.processes.add_name_to_table("LeagueClientUx.exe");
    let (asked, on_disk) = (asked_of_the_machine(&fakes), fakes.on_disk());
    let refused = block_on(fakes.backend.restore_backup(LauncherId::Riot, &id));
    assert_eq!(
        refusal(refused),
        (
            "GAME_RUNNING",
            "Riot Games: League of Legends is running. Close it, then restore: One Launch never closes a Riot game."
                .to_owned()
        )
    );
    assert_eq!(asked_of_the_machine(&fakes), asked);
    assert!(fakes.on_disk() == on_disk);
    assert_eq!(
        block_on(fakes.backend.backups(LauncherId::Riot))
            .unwrap()
            .len(),
        1
    );
    fakes.backend.shut_down();
}

#[test]
fn a_process_list_that_cannot_be_read_refuses_the_switch_the_launch_and_the_restore() {
    let fakes = fakes(None).without_the_press();
    let (alt, game) = (fakes.account(ALT), fakes.game());
    assert!(fakes.play(ALT).last().unwrap().state == JobState::Done);
    let id = block_on(fakes.backend.backups(LauncherId::Riot)).unwrap()[0]
        .id
        .clone();
    fakes.client_exits();
    let (asked, on_disk) = (asked_of_the_machine(&fakes), fakes.on_disk());

    fakes.fake.processes.fail_listings(true);
    let unread = |refused: (&'static str, String)| {
        assert!(refused.1.contains("could not be read"), "{}", refused.1);
    };
    unread(refusal(block_on(
        fakes.backend.launch_preflight(&alt.id, Some(&game.id)),
    )));
    unread(refusal(block_on(fakes.backend.switch_plan(&alt.id))));
    unread(refusal(fakes.play_with(MAIN, false)));
    // Alt's sign-in is in place, so nothing would be written: a game that could not be seen refuses the start all the same.
    unread(refusal(fakes.play_with(ALT, false)));
    let states = fakes.play_with(MAIN, true).unwrap();
    let last = states.last().unwrap();
    assert!(last.state == JobState::Failed && !last.switched && last.backup_id.is_none());
    assert!(
        last.error
            .as_deref()
            .is_some_and(|error| error.contains("could not be read"))
    );
    unread(refusal(block_on(
        fakes.backend.restore_backup(LauncherId::Riot, &id),
    )));
    assert_eq!(asked_of_the_machine(&fakes), asked);
    assert!(fakes.on_disk() == on_disk);
    assert_eq!(
        block_on(fakes.backend.backups(LauncherId::Riot))
            .unwrap()
            .len(),
        1
    );
    fakes.backend.shut_down();
}

#[test]
fn the_session_file_is_never_made_where_there_is_none() {
    let fakes = fakes(None);
    let alt = fakes.account(ALT);
    fs::remove_file(fakes.session_file()).unwrap();
    let missing = format!(
        "Riot Games: {} does not exist. One Launch never creates it: start the Riot Client once by hand (it writes the file at its first start), then try again.",
        fakes.session_file().to_str().unwrap().replace('/', "\\")
    );
    let preflight = block_on(fakes.backend.launch_preflight(&alt.id, None)).unwrap();
    assert!(!preflight.plan.executable);
    assert_eq!(
        preflight
            .blocked
            .map(|blocked| format!("Riot Games: {blocked}")),
        Some(missing.clone())
    );
    // A start is taken as a job, as it is for any plan that cannot run, and the job refuses before its first step.
    let states = fakes.play(ALT);
    let last = states.last().unwrap();
    assert!(last.state == JobState::Failed && !last.switched && last.backup_id.is_none());
    assert_eq!(last.error, Some(missing));
    assert!(!fakes.session_file().exists());
    assert!(asked_of_the_machine(&fakes).is_empty());
    fakes.backend.shut_down();

    // A snapshot is not put back over a file that is gone either: the client makes the file, nobody else.
    let fakes = self::fakes(None).without_the_press();
    assert!(fakes.play(ALT).last().unwrap().state == JobState::Done);
    let id = block_on(fakes.backend.backups(LauncherId::Riot)).unwrap()[0]
        .id
        .clone();
    fakes.client_exits();
    fs::remove_file(fakes.session_file()).unwrap();
    let asked = asked_of_the_machine(&fakes);
    let refused = refusal(block_on(
        fakes.backend.restore_backup(LauncherId::Riot, &id),
    ));
    assert_eq!(refused.0, "BACKUP");
    assert!(
        refused
            .1
            .contains("is missing. Start the Riot Client once by hand")
    );
    assert!(!fakes.session_file().exists());
    assert_eq!(asked_of_the_machine(&fakes), asked);
    fakes.backend.shut_down();
}

#[test]
fn what_the_file_holds_is_stored_for_its_owner_before_a_rollback_overwrites_it() {
    // New has no saved sign-in, so the client is signed out and asks; the user signs in as Main, which is another account's.
    let knobs = json!({
        "knobs": { "riot": { "signInWhenAskedMs": 600, "signInAs": MAIN_UID } },
    });
    let fakes = fakes(Some(&knobs));
    assert!(fakes.saved(MAIN).is_none());
    let states = fakes.play(NEW);
    let last = states.last().unwrap();
    assert!(last.state == JobState::Failed);
    assert_eq!(last.restored, Some(true));
    assert_eq!(
        last.error.as_deref(),
        Some(
            "The Riot Client signed in as Riot Main (Riot account …1111), which is another One Launch account, instead of Riot New. Sign in to the client as riot_new."
        )
    );
    assert_eq!(
        detail(last, "capture"),
        format!(
            "Saved Riot Main's sign-in (fingerprint {}).",
            &sha256_hex(mains_token().as_bytes())[..8]
        )
    );
    assert_eq!(
        detail(last, "file"),
        "RiotGamesPrivateSettings.yaml now holds the client's signed-out state; everything else in it is as it was."
    );
    assert_eq!(detail(last, "manual"), "Signed in as a different account.");

    // The sign-in the user made by hand was in the file when the rollback began. It is Main's and newer than the one saved before the switch, so it is the one that is kept: an older token is never the one presented later.
    assert_eq!(
        fakes.saved(MAIN),
        Some(sha256_hex(clients_token(1).as_bytes()))
    );
    // The file is back as the snapshot took it, and the client was closed by its windows for that.
    assert!(fakes.on_disk() == planted_file());
    assert_eq!(
        asked_of_the_machine(&fakes),
        [
            "file.write",
            "proc.start",
            "proc.close",
            "proc.close",
            "file.write"
        ]
    );
    assert!(fakes.account(NEW).launcher_uid.is_none() && fakes.saved(NEW).is_none());
    let said = job_text(&states);
    for secret in [mains_token(), clients_token(1), GAME_NAME.to_owned()] {
        assert!(!said.contains(&secret));
    }
    assert!(block_on(fakes.backend.recovery()).unwrap().is_none());
    fakes.backend.shut_down();
}

#[test]
fn the_lockfiles_password_is_in_no_plan_job_error_or_log_and_play_is_pressed_through_the_port() {
    let password = format!("made-up-lockfile-password-{}", "p".repeat(24));
    let lockfile = format!("Riot Client:4242:51234:{password}:https");
    let fakes = fakes_with(None, &|launchers| {
        fs::write(launchers.join(LOCKFILE), &lockfile).unwrap();
    });
    let (alt, game) = (fakes.account(ALT), fakes.game());
    let mut said = vec![
        plan_text(&block_on(fakes.backend.switch_plan(&alt.id)).unwrap()),
        plan_text(
            &block_on(fakes.backend.launch_preflight(&alt.id, Some(&game.id)))
                .unwrap()
                .plan,
        ),
    ];
    let states = fakes.play(ALT);
    let last = states.last().unwrap();
    assert!(last.state == JobState::Done && last.hand_off.is_none());
    assert!(detail(last, "launch").ends_with(
        "--launch-product=valorant --launch-patchline=live (the switch started the client with these arguments; One Launch pressed Play and the Riot Client started the game)"
    ));
    // One request, by the product's id, and nothing else was sent anywhere.
    let log = fakes.fake.log.snapshot();
    let requests: Vec<&Value> = log
        .iter()
        .filter(|event| event["op"] == "riot.launch")
        .collect();
    assert_eq!(
        requests,
        [&json!({ "k": "plat", "op": "riot.launch", "id": "valorant.live" })]
    );
    assert!(!log.iter().any(|event| event["op"] == "shell.open"));
    said.push(job_text(&states));
    said.push(json!(log).to_string());
    said.push(format!("{log:?}"));
    assert!(!said.join("\n").contains(&password));
    fakes.backend.shut_down();
    assert_eq!(
        files_holding(&fakes.scratch.join("data"), &password),
        Vec::<String>::new()
    );
    assert_eq!(files_holding(&fakes.launchers(), &password), [LOCKFILE]);
    assert!(fs::read_to_string(fakes.launchers().join(LOCKFILE)).unwrap() == lockfile);
}

#[test]
fn with_the_setting_off_or_a_request_the_client_refuses_the_launch_hands_play_over_to_the_user() {
    let fakes = fakes(None);
    block_on(
        fakes
            .backend
            .set_setting(SettingKey::RiotPressPlay, "false".to_owned()),
    )
    .unwrap();
    let states = fakes.play(ALT);
    let last = states.last().unwrap();
    assert!(last.state == JobState::Done && last.switched);
    assert_eq!(last.hand_off.as_deref(), Some(HAND_OFF));
    assert!(detail(last, "launch").ends_with(
        "(the switch started the client with these arguments; the Riot Client opened the game's page: Auto-launch is disabled)"
    ));
    assert_eq!(
        detail(last, "start"),
        "Client started with VALORANT's launch arguments."
    );
    let sent = |fakes: &Fakes| {
        let log = fakes.fake.log.snapshot();
        log.iter()
            .filter(|event| event["op"] == "riot.launch")
            .count()
    };
    assert_eq!(sent(&fakes), 0);
    fakes.backend.shut_down();

    let refuses = json!({ "knobs": { "riot": { "pressPlayFails": true } } });
    let fakes = self::fakes(Some(&refuses));
    let states = fakes.play(ALT);
    let last = states.last().unwrap();
    assert!(last.state == JobState::Done);
    assert_eq!(last.hand_off.as_deref(), Some(HAND_OFF));
    assert!(detail(last, "launch").ends_with(
        "the Riot Client opened the game's page: Auto-launch is disabled; the launch request was not taken: the Riot Client answered the launch request with HTTP 500)"
    ));
    assert_eq!(sent(&fakes), 1);
    fakes.backend.shut_down();
}

#[test]
fn a_running_client_is_closed_by_its_windows_once_the_user_has_said_yes() {
    let knobs = json!({ "knobs": { "riot": { "runningAs": MAIN_UID } } });
    let fakes = fakes(Some(&knobs));
    let (alt, game) = (fakes.account(ALT), fakes.game());
    let preflight = block_on(fakes.backend.launch_preflight(&alt.id, Some(&game.id))).unwrap();
    assert!(
        preflight.launcher_running && preflight.needs_confirmation && !preflight.already_active
    );
    assert!(preflight.running_game.is_none());
    assert_eq!(
        preflight.close_message.as_deref(),
        Some("The Riot Client is running. Switching closes it.")
    );

    let unconfirmed = fakes.play_with(ALT, false).err().expect("a refusal");
    assert_eq!(unconfirmed.code, "CONFIRM_REQUIRED");
    assert!(asked_of_the_machine(&fakes).is_empty());

    let states = fakes.play_with(ALT, true).unwrap();
    let last = states.last().unwrap();
    assert!(last.state == JobState::Done && last.switched);
    assert_eq!(detail(last, "close"), "Client closed.");
    assert!(detail(last, "capture").starts_with("Saved Riot Main's sign-in (fingerprint "));
    // The window close for the client, then for the window processes it leaves behind: nothing is ended.
    assert_eq!(
        asked_of_the_machine(&fakes),
        [
            "proc.close",
            "proc.close",
            "file.write",
            "proc.start",
            "riot.launch"
        ]
    );
    fakes.backend.shut_down();
}

#[test]
fn a_client_that_runs_as_the_account_is_left_alone_and_the_game_is_handed_to_it() {
    let knobs = json!({ "knobs": { "riot": { "runningAs": MAIN_UID } } });
    let fakes = fakes(Some(&knobs));
    let (main, game) = (fakes.account(MAIN), fakes.game());
    let before = fakes.on_disk();
    let preflight = block_on(fakes.backend.launch_preflight(&main.id, Some(&game.id))).unwrap();
    assert!(
        preflight.launcher_running && preflight.already_active && !preflight.needs_confirmation
    );

    let states = fakes.play(MAIN);
    let last = states.last().unwrap();
    assert!(last.state == JobState::Done && last.direct && last.backup_id.is_none());
    assert!(detail(last, "launch").ends_with(
        "--launch-product=valorant --launch-patchline=live (One Launch pressed Play and the Riot Client started the game)"
    ));
    assert_eq!(asked_of_the_machine(&fakes), ["proc.start", "riot.launch"]);
    assert!(fakes.on_disk() == before);
    assert!(
        block_on(fakes.backend.backups(LauncherId::Riot))
            .unwrap()
            .is_empty()
    );
    fakes.backend.shut_down();
}

#[test]
fn an_account_without_a_saved_sign_in_is_signed_in_by_hand_once_and_its_sign_in_is_kept() {
    let knobs = json!({
        "knobs": { "riot": { "signInWhenAskedMs": 600, "signInAs": NEW_UID } },
    });
    let fakes = fakes(Some(&knobs));
    let new = fakes.account(NEW);
    assert!(!new.has_session && new.launcher_uid.is_none());
    let plan = block_on(fakes.backend.switch_plan(&new.id)).unwrap();
    assert!(plan.strategy == SwitchStrategy::Credentials);

    let states = fakes.play(NEW);
    let last = states.last().unwrap();
    assert!(last.state == JobState::Done && last.attention.is_none());
    let asked = states.iter().find_map(|job| job.attention.clone());
    let asked = asked.expect("the job waited for the user");
    assert_eq!(asked.title, format!("Sign in to the Riot Client as {NEW}"));
    assert_eq!(asked.message, "No sign-in is saved for this account yet.");
    assert_eq!(
        asked.steps[1],
        "Tick “Stay signed in” before you sign in, or the client forgets the account when it closes."
    );
    assert!(asked.can_copy_password);
    assert_eq!(detail(last, "verify"), "Waiting for you.");
    assert_eq!(
        detail(last, "manual"),
        format!("Signed in as {NEW}. Linked to Riot account …4444.")
    );
    let fresh = sha256_hex(clients_token(1).as_bytes());
    assert_eq!(
        detail(last, "recapture"),
        format!("Stored Riot New's sign-in (fingerprint {}).", &fresh[..8])
    );

    // The file the client wrote at that sign-in is the account's now, and the job never spelled its token out.
    let new = fakes.account(NEW);
    assert_eq!(new.launcher_uid.as_deref(), Some(NEW_UID));
    let saved = block_on(fakes.backend.session(&new.id)).unwrap();
    let session = saved.session.expect("the sign-in by hand was saved");
    assert_eq!(session.fingerprint, fresh);
    assert!(session.verified_at.is_some());
    assert!(fakes.on_disk().contains(&clients_token(1)));
    assert!(!job_text(&states).contains(&clients_token(1)));
    fakes.backend.shut_down();
    let data = fakes.scratch.join("data");
    assert_eq!(
        files_holding(&data, &clients_token(1)),
        Vec::<String>::new()
    );
}

#[test]
fn a_saved_sign_in_the_client_refuses_is_marked_stale_and_replaced_by_the_one_made_by_hand() {
    let knobs = json!({
        "knobs": { "riot": { "rejected": [ALT_UID], "signInWhenAskedMs": 600, "signInAs": ALT_UID } },
    });
    let fakes = fakes(Some(&knobs));
    let states = fakes.play(ALT);
    let last = states.last().unwrap();
    assert!(last.state == JobState::Done);
    assert_eq!(
        detail(last, "verify"),
        "The client ended on its login page: the client refused the saved sign-in (401 invalid_grant)."
    );
    let asked = states.iter().find_map(|job| job.attention.clone());
    let asked = asked.expect("the job waited for the user");
    assert_eq!(
        asked.title,
        format!("The Riot Client is asking you to sign in as {ALT}")
    );
    assert_eq!(asked.message, "The client refused its saved sign-in.");
    assert_eq!(detail(last, "manual"), format!("Signed in as {ALT}."));
    let saved = block_on(fakes.backend.session(&fakes.account(ALT).id)).unwrap();
    let session = saved.session.expect("the fresh sign-in was saved");
    assert_eq!(session.fingerprint, sha256_hex(clients_token(1).as_bytes()));
    assert!(session.stale_reason.is_none() && session.verified_at.is_some());
    fakes.backend.shut_down();
}

#[test]
fn a_switch_that_fails_after_its_write_puts_the_file_back() {
    // Something else starts the client between the write and the switch's own start.
    let knobs = json!({ "processes": { "reappearsAfterPolls": { "RiotClientServices.exe": 1 } } });
    let fakes = fakes(Some(&knobs));
    let states = fakes.play(ALT);
    let last = states.last().unwrap();
    assert!(last.state == JobState::Failed);
    assert_eq!(last.restored, Some(true));
    assert!(last.backup_id.is_some());
    assert_eq!(
        last.error.as_deref(),
        Some(
            "The Riot Client was started by something else while One Launch was switching accounts, so the switch was stopped. Try again."
        )
    );
    assert!(detail(last, "file").contains("now holds the saved copy"));
    assert!(fakes.on_disk() == planted_file());
    assert_eq!(
        fakes.saved(MAIN),
        Some(sha256_hex(mains_token().as_bytes()))
    );
    let said = job_text(&states);
    assert!(!said.contains(&alts_token()) && !said.contains(&mains_token()));
    assert!(block_on(fakes.backend.recovery()).unwrap().is_none());
    fakes.backend.shut_down();
}

#[test]
fn a_snapshot_puts_the_file_back_once_the_client_is_closed_and_keeps_what_it_replaces() {
    let fakes = fakes(None).without_the_press();
    assert!(fakes.play(ALT).last().unwrap().state == JobState::Done);
    let taken = block_on(fakes.backend.backups(LauncherId::Riot)).unwrap();
    assert_eq!(taken.len(), 1);
    assert_eq!(taken[0].files, ["Data/RiotGamesPrivateSettings.yaml"]);
    assert!(taken[0].pinned.is_none());
    let id = taken[0].id.clone();

    // The switch left the client running, and a running client writes its file again.
    let refused = block_on(fakes.backend.restore_backup(LauncherId::Riot, &id));
    assert_eq!(
        refusal(refused),
        (
            "LAUNCHER_RUNNING",
            "Riot Games: Exit the Riot Client from its tray icon before restoring: it rewrites RiotGamesPrivateSettings.yaml while it runs, which would undo the restore."
                .to_owned()
        )
    );

    fakes.client_exits();
    // The client replaced Alt's token when it started: that newer sign-in is what the file holds now.
    let rotated = sha256_hex(clients_token(1).as_bytes());
    assert!(fakes.on_disk().contains(&clients_token(1)));
    assert_eq!(fakes.saved(ALT), Some(sha256_hex(alts_token().as_bytes())));
    let restored = block_on(fakes.backend.restore_backup(LauncherId::Riot, &id)).unwrap();
    assert_eq!(restored.files.len(), 1);
    assert!(restored.registry.is_empty());
    assert!(fakes.on_disk() == planted_file());
    // It went to Alt before the file was overwritten,
    assert_eq!(fakes.saved(ALT), Some(rotated));
    // and what was there was copied first, so the restore can be undone.
    let after = block_on(fakes.backend.backups(LauncherId::Riot)).unwrap();
    assert_eq!(after.len(), 2);
    assert_eq!(after[0].reason, format!("Before restoring {id}"));
    fakes.backend.shut_down();
}

#[test]
fn a_stop_while_the_client_closes_changes_nothing_and_one_after_its_start_puts_nothing_back() {
    let knobs = json!({ "knobs": { "riot": { "runningAs": MAIN_UID } } });
    let fakes = fakes(Some(&knobs));
    let before = fakes.on_disk();
    fakes.stop_at("close");
    let states = fakes.play_with(ALT, true).unwrap();
    let last = states.last().unwrap();
    assert!(last.state == JobState::Cancelled && !last.switched);
    assert!(last.restored.is_none() && last.backup_id.is_none());
    assert_eq!(
        last.error.as_deref(),
        Some("Stopped before the Riot Client was started as riot_alt. Nothing had been changed.")
    );
    assert_eq!(detail(last, "close"), "Stopped.");
    assert!(fakes.on_disk() == before);
    fakes.backend.shut_down();

    // Once the client has been started again it reads what was written, so a stop leaves the write standing.
    let fakes = self::fakes(None);
    fakes.stop_at("verify");
    let states = fakes.play(ALT);
    let last = states.last().unwrap();
    assert!(last.state == JobState::Cancelled && !last.switched);
    assert!(last.restored.is_none() && last.backup_id.is_some());
    assert_eq!(
        last.error.as_deref(),
        Some(
            "Stopped while the Riot Client was starting as riot_alt. Nothing was rolled back: the client carries on signing in on its own."
        )
    );
    assert_eq!(detail(last, "verify"), "Cancelled.");
    assert!(fakes.on_disk() == ol_sim::fake_session_file(ALT_UID, &alts_token()));
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
    assert!(fakes.on_disk() != planted_file());

    // What a crash during the sign-in leaves: the journal of the fixtures, naming this run's launcher, account and snapshot.
    let mut journal: Value = serde_json::from_str(
        &fs::read_to_string(oracle("fixtures/journal/switch-signing-in.json")).unwrap(),
    )
    .unwrap();
    journal["launcher"] = "riot".into();
    journal["accountId"] = fakes.account(ALT).id.into();
    journal["backupId"] = backup_id.clone().into();
    let journal_file = fakes.scratch.join("data").join("switch.json");
    fakes.backend.shut_down();
    fs::write(&journal_file, journal.to_string()).unwrap();
    // The fake machine is built afresh, as after a crash: no client runs.
    let fakes = start(fakes.scratch);

    let pending = block_on(fakes.backend.recovery())
        .unwrap()
        .expect("a recovery");
    assert!(pending.backup_id == backup_id && pending.snapshot_available);
    assert_eq!(pending.launcher, LauncherId::Riot);
    assert_eq!(pending.account_name.as_deref(), Some("Riot Alt"));

    let restored = block_on(fakes.backend.restore_recovery()).unwrap();
    assert!(restored.id == backup_id);
    assert_eq!(restored.files.len(), 1);
    assert!(fakes.on_disk() == planted_file());
    assert!(block_on(fakes.backend.recovery()).unwrap().is_none());
    assert!(!journal_file.exists());
    fakes.backend.shut_down();
}
