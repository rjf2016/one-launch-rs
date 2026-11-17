#![forbid(unsafe_code)]

// Dry-run plans and preflights held against oracle/tests/goldens: what the Electron app's Riot adapter answered in its own tests. The account of each call comes from the golden; the test here only builds the machine those tests ran on: a client folder, the metadata of five products of which two are installed, the fixture's session file with Main signed in, three accounts of which Alt has a saved sign-in, and the short waits. Below them, the adapter's own rules on that machine: what it does to a process, and what a running game or a process list that cannot be read refuses.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ol_adapters::riot::{EXE, RiotAdapter, RiotConfig, Timing, WINDOW_EXE};
use ol_adapters::{AccountRef, AdapterError, Directory, GameRef};
use ol_model::{LauncherId, LauncherSession, SwitchPlan, SwitchPlanStep};
use ol_platform_api::{CancelToken, Platform};
use ol_platform_fake::{
    FakeClock, FakeProcesses, FakeRegistry, FakeShell, OpLog, ProcessKnobs, RegistryValue,
    ScratchFiles,
};
use ol_runtime::TimingOverrides;
use ol_sim::fake_session_file;
use ol_testkit::vectors::{difference, sha256_hex};
use ol_testkit::{Scratch, oracle, snapshot};
use ol_vault::Secret;
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const MAIN: &str = "@acct:riot:riot_main";
const ALT: &str = "@acct:riot:riot_alt";
const NEW: &str = "@acct:riot:riot_new";
const MAIN_UID: &str = "11111111-1111-4111-8111-111111111111";
const ALT_UID: &str = "33333333-3333-4333-8333-333333333333";
const UNINSTALL: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall";

// The accounts and saved sign-ins of the Electron tests, held in memory. A plan may read them and nothing else: every write is refused.
struct Book {
    accounts: Mutex<Vec<AccountRef>>,
    sessions: Vec<(LauncherSession, String)>,
}

fn only_read() -> AdapterError {
    AdapterError::Failed("a plan changes nothing".to_owned())
}

impl Directory for Book {
    fn account_by_launcher_uid(&self, uid: &str) -> Result<Option<AccountRef>, AdapterError> {
        let accounts = self.accounts.lock().unwrap();
        let held = |account: &&AccountRef| account.launcher_uid.as_deref() == Some(uid);
        Ok(accounts.iter().find(held).cloned())
    }

    fn account_by_id(&self, id: &str) -> Result<Option<AccountRef>, AdapterError> {
        let accounts = self.accounts.lock().unwrap();
        let named = |account: &&AccountRef| account.id.as_deref() == Some(id);
        Ok(accounts.iter().find(named).cloned())
    }

    fn set_launcher_uid(&self, _: &str, _: &str) -> Result<(), AdapterError> {
        Err(only_read())
    }

    fn session(&self, account_id: &str) -> Result<Option<LauncherSession>, AdapterError> {
        let of = |(session, _): &&(LauncherSession, String)| session.account_id == account_id;
        Ok(self
            .sessions
            .iter()
            .find(of)
            .map(|(session, _)| session.clone()))
    }

    fn session_by_fingerprint(
        &self,
        fingerprint: &str,
    ) -> Result<Option<LauncherSession>, AdapterError> {
        let with = |(session, _): &&(LauncherSession, String)| session.fingerprint == fingerprint;
        Ok(self
            .sessions
            .iter()
            .find(with)
            .map(|(session, _)| session.clone()))
    }

    fn save_session(
        &self,
        _: &str,
        _: Option<&str>,
        _: &str,
        _: &Secret,
    ) -> Result<LauncherSession, AdapterError> {
        Err(only_read())
    }

    fn decrypt_session(&self, _: &str) -> Result<Option<Secret>, AdapterError> {
        Err(only_read())
    }

    fn mark_session_verified(&self, _: &str) -> Result<(), AdapterError> {
        Err(only_read())
    }

    fn mark_session_stale(&self, _: &str, _: &str) -> Result<(), AdapterError> {
        Err(only_read())
    }
}

struct World {
    scratch: Scratch,
    log: OpLog,
    registry: Arc<FakeRegistry>,
    processes: Arc<FakeProcesses>,
    clock: Arc<FakeClock>,
    platform: Platform,
    config: RiotConfig,
    book: Book,
}

fn read_json(relative: &str) -> Value {
    serde_json::from_str(&fs::read_to_string(oracle(relative)).unwrap()).unwrap()
}

fn fixture(name: &str) -> Vec<u8> {
    fs::read(oracle(&format!("fixtures/launchers/riot/{name}"))).unwrap()
}

fn forward(path: &std::path::Path) -> String {
    path.to_str().unwrap().replace('\\', "/")
}

// The refresh token of Alt's saved copy, as the Electron tests make it up.
fn alt_token() -> String {
    format!("placeholder-refresh-token-for-alt-{}", "b".repeat(40))
}

impl World {
    fn data(&self) -> PathBuf {
        let local = self.scratch.join("Local");
        local.join("Riot Games").join("Riot Client")
    }

    fn session_file(&self) -> PathBuf {
        self.data()
            .join("Data")
            .join("RiotGamesPrivateSettings.yaml")
    }

    // A run's log that is newer than the one the machine starts with, so it is the current run.
    fn write_log(&self, text: &[u8]) {
        let logs = self.data().join("Logs").join("Riot Client Logs");
        let name = "2026-10-06T10-00-00.000_2000_Riot Client.log";
        fs::write(logs.join(name), text).unwrap();
    }

    fn adapter(&self) -> RiotAdapter<'_> {
        RiotAdapter::new(&self.platform, &self.config, &self.book, None)
    }

    // What the fake machine was asked to do, each as its operation, its image and, for an end, whether what the image started goes too.
    fn asked(&self) -> Vec<String> {
        let events = self.log.snapshot();
        let named = events.iter().map(|event| {
            let (op, exe) = (event["op"].as_str().unwrap(), &event["exe"]);
            match event.get("tree") {
                Some(tree) => format!("{op} {} tree={tree}", exe.as_str().unwrap()),
                None => format!("{op} {}", exe.as_str().unwrap_or_default()),
            }
        });
        named.collect()
    }
}

// A product's metadata folder as the Electron tests lay it down.
fn product(scratch: &Scratch, id: &str, install: Option<&PathBuf>, ok: bool, folder: bool) {
    let metadata = scratch
        .join("ProgramData")
        .join("Riot Games")
        .join("Metadata")
        .join(id);
    fs::create_dir_all(&metadata).unwrap();
    fs::write(metadata.join(format!("{id}.lockfile")), b"").unwrap();
    if let Some(install) = install {
        let lines = [
            "auto_patching_enabled_by_player: false".to_owned(),
            "patching_policy: \"manual\"".to_owned(),
            format!("product_install_full_path: \"{}\"", forward(install)),
            format!(
                "product_install_root: \"{}\"",
                forward(&scratch.join("Riot Games"))
            ),
            "should_repair: false".to_owned(),
            String::new(),
        ];
        fs::write(
            metadata.join(format!("{id}.product_settings.yaml")),
            lines.join("\n"),
        )
        .unwrap();
        if folder {
            fs::create_dir_all(install).unwrap();
        }
    }
    if ok {
        fs::write(metadata.join(format!("{id}.ok")), b"").unwrap();
    }
}

fn world_with(knobs: ProcessKnobs) -> World {
    let scratch = Scratch::new("riot-plans");
    let root = scratch.path().to_str().unwrap().to_owned();
    let games = scratch.join("Riot Games");
    let install = games.join("Riot Client");
    fs::create_dir_all(&install).unwrap();
    fs::write(install.join(EXE), b"").unwrap();
    let metadata = scratch
        .join("ProgramData")
        .join("Riot Games")
        .join("Metadata");
    for other in ["Riot Client", "league_of_legends.live.game_patch"] {
        fs::create_dir_all(metadata.join(other)).unwrap();
    }
    let valorant = games.join("VALORANT");
    product(
        &scratch,
        "valorant.live",
        Some(&valorant.join("live")),
        true,
        true,
    );
    product(
        &scratch,
        "valorant.pbe",
        Some(&valorant.join("pbe")),
        true,
        true,
    );
    let league = games.join("League of Legends");
    product(
        &scratch,
        "league_of_legends.live",
        Some(&league),
        false,
        false,
    );
    let tactics = games.join("Teamfight Tactics").join("Live");
    product(
        &scratch,
        "teamfighttactics.live",
        Some(&tactics),
        true,
        false,
    );
    product(&scratch, "bacon.live", None, false, false);
    let client = forward(&install.join(EXE));
    fs::write(
        scratch
            .join("ProgramData")
            .join("Riot Games")
            .join("RiotClientInstalls.json"),
        json!({ "rc_default": client }).to_string(),
    )
    .unwrap();

    let data = scratch.join("Local").join("Riot Games").join("Riot Client");
    for folder in ["Config", "Data"] {
        fs::create_dir_all(data.join(folder)).unwrap();
    }
    let logs = data.join("Logs").join("Riot Client Logs");
    fs::create_dir_all(&logs).unwrap();
    fs::write(
        data.join("Config").join("RiotClientSettings.yaml"),
        fixture("RiotClientSettings.yaml"),
    )
    .unwrap();
    fs::write(
        data.join("Data").join("RiotGamesPrivateSettings.yaml"),
        fixture("RiotGamesPrivateSettings.yaml"),
    )
    .unwrap();
    fs::write(
        logs.join("2026-10-05T20-00-00.000_1234_Riot Client.log"),
        b"Logging started\n",
    )
    .unwrap();

    let log = OpLog::new();
    let registry = Arc::new(FakeRegistry::new(log.clone()));
    let text = |value: String| RegistryValue::Text(value);
    registry.seed(
        &format!("{UNINSTALL}\\Riot Game Riot_Client.\\InstallLocation"),
        text(format!("{}/", forward(&install))),
    );
    registry.seed(
        &format!("{UNINSTALL}\\Riot Game valorant.live\\DisplayName"),
        text("VALORANT".to_owned()),
    );
    registry.seed(
        &format!("{UNINSTALL}\\Riot Game valorant.live\\InstallLocation"),
        text(forward(&valorant.join("live"))),
    );
    registry.seed(
        r"HKLM\SOFTWARE\Classes\riotclient\shell\open\command\",
        text(format!(
            "\"{}\" --app-command=\"%1\"",
            install.join(EXE).to_str().unwrap()
        )),
    );
    let processes = Arc::new(FakeProcesses::new(log.clone(), knobs));
    let clock = read_json("tests/capture.json")["clock"]
        .as_str()
        .map(|at| OffsetDateTime::parse(at, &Rfc3339).unwrap())
        .unwrap();
    let clock = Arc::new(FakeClock::starting_at(clock));
    let platform = Platform {
        registry: Arc::clone(&registry) as _,
        processes: Arc::clone(&processes) as _,
        files: Arc::new(ScratchFiles::new(&[scratch.path()], log.clone())),
        shell: Arc::new(FakeShell::new(log.clone())),
        clock: Arc::clone(&clock) as _,
    };
    let mut timing = TimingOverrides::default();
    for (name, milliseconds) in [
        ("pollMs", 1),
        ("shutdownMs", 40),
        ("killGraceMs", 20),
        ("windowCloseMs", 40),
        ("restartWatchMs", 25),
        ("cleanExitWatchMs", 5),
        ("restartSettleMs", 60),
        ("startupMs", 40),
        ("signInMs", 40),
        ("manualSignInMs", 80),
        ("captureMs", 40),
        ("readyGraceMs", 0),
        ("launchConfirmMs", 40),
        ("playConfirmMs", 40),
    ] {
        timing.set(name, milliseconds);
    }
    let config = RiotConfig {
        chosen_dir: None,
        default_dir: format!("{root}\\nowhere"),
        local_app_data: format!("{root}\\Local"),
        program_data: format!("{root}\\ProgramData"),
        press_play: true,
        backups_to_keep: 20,
        backups_dir: format!("{root}\\backups"),
        timing: Timing::with(&timing),
    };

    let account = |id: &str, name: &str, login: &str, uid: Option<&str>, has_secret| AccountRef {
        id: Some(id.to_owned()),
        display_name: name.to_owned(),
        login: login.to_owned(),
        launcher_uid: uid.map(str::to_owned),
        has_secret,
    };
    let saved = LauncherSession {
        account_id: ALT.to_owned(),
        launcher: LauncherId::Riot,
        launcher_uid: Some(ALT_UID.to_owned()),
        fingerprint: sha256_hex(alt_token().as_bytes()),
        captured_at: "<ts>".to_owned(),
        verified_at: None,
        stale_reason: None,
    };
    let book = Book {
        accounts: Mutex::new(vec![
            account(MAIN, "Main", "riot_main", Some(MAIN_UID), false),
            account(ALT, "Alt", "riot_alt", Some(ALT_UID), false),
            account(NEW, "New", "riot_new", None, true),
        ]),
        sessions: vec![(saved, fake_session_file(ALT_UID, &alt_token()))],
    };
    World {
        scratch,
        log,
        registry,
        processes,
        clock,
        platform,
        config,
        book,
    }
}

fn world() -> World {
    world_with(ProcessKnobs::default())
}

fn step_json(step: &SwitchPlanStep) -> Value {
    let changes: Vec<Value> = step
        .changes
        .iter()
        .map(|change| json!({ "where": change.place, "before": change.before, "after": change.after }))
        .collect();
    json!({
        "changes": changes,
        "skipped": step.skipped,
        "id": step.id,
        "kind": step.kind.as_str(),
        "title": step.title,
        "target": step.target,
        "detail": step.detail,
    })
}

fn plan_json(plan: &SwitchPlan) -> Value {
    json!({
        "launcher": plan.launcher.as_str(),
        "account": {
            "id": plan.account.id,
            "displayName": plan.account.display_name,
            "login": plan.account.login,
            "launcherUid": plan.account.launcher_uid,
        },
        "strategy": plan.strategy.as_str(),
        "current": {
            "mostRecentLogin": plan.most_recent_login,
            "autoLoginUser": plan.auto_login_user,
            "running": plan.running,
        },
        "steps": plan.steps.iter().map(step_json).collect::<Vec<_>>(),
        "warnings": plan.warnings,
        "notes": plan.notes,
        "executable": plan.executable,
        "generatedAt": "<ts>",
    })
}

// The test's own folder is {tmp} in a golden, however its letters are cased.
fn without_root(value: &mut Value, root: &str) {
    match value {
        Value::String(text) => {
            let lower = root.to_ascii_lowercase();
            while let Some(at) = text.to_ascii_lowercase().find(&lower) {
                text.replace_range(at..at + root.len(), "{tmp}");
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| without_root(item, root)),
        Value::Object(fields) => fields
            .values_mut()
            .for_each(|field| without_root(field, root)),
        _ => {}
    }
}

// What the adapter answered a call with, in a golden's shape: a plan, a preflight, or the refusal.
fn answer(world: &World, call: &Value) -> Result<Value, AdapterError> {
    let given = &call["args"][0];
    let text = |name: &str| given[name].as_str().map(str::to_owned);
    let account = AccountRef {
        id: text("id"),
        display_name: text("displayName").unwrap(),
        login: text("login").unwrap(),
        launcher_uid: text("launcherUid"),
        has_secret: given["hasSecret"].as_bool().unwrap(),
    };
    let asked_for = &call["args"][1]["game"];
    let game = asked_for.as_object().map(|_| GameRef {
        launcher_game_id: asked_for["launcherGameId"].as_str().unwrap(),
        name: asked_for["name"].as_str().unwrap(),
    });
    let adapter = world.adapter();
    if call["method"] == "preflight" {
        let preflight = adapter.preflight(&account, game)?;
        return Ok(json!({
            "plan": plan_json(&preflight.plan),
            "launcherRunning": preflight.launcher_running,
            "runningGame": preflight.running_game.map(|game| game.name),
            "alreadyActive": preflight.already_active,
            "closeMessage": preflight.close_message,
        }));
    }
    assert_eq!(call["method"], "switchAccount");
    assert_eq!(call["args"][1]["dryRun"], true);
    Ok(plan_json(&adapter.build_switch_plan(&account, game)?.0))
}

// Puts the calls of a golden that `before` has a machine for to the port, each on the machine prepared for it. A call it has None for is a real switch, which is not put here.
type Prepare<'p> = Option<&'p dyn Fn(&mut World)>;

fn check(golden: &str, before: &[Prepare<'_>]) {
    let recorded = read_json(&format!("tests/goldens/{golden}.json"));
    let calls = recorded["calls"].as_array().unwrap();
    assert_eq!(calls.len(), before.len(), "{golden}: a machine per call");
    let mut world = world();
    let root = world.scratch.path().to_str().unwrap().to_owned();
    for (index, (call, prepare)) in calls.iter().zip(before).enumerate() {
        let Some(prepare) = prepare else {
            assert_eq!(
                call["args"][1]["dryRun"], false,
                "{golden} call {index} is a real switch"
            );
            continue;
        };
        prepare(&mut world);
        let files = snapshot(world.scratch.path());
        let answered = answer(&world, call);
        assert!(
            snapshot(world.scratch.path()) == files,
            "{golden} call {index}: a plan writes nothing"
        );
        match (answered, call.get("error")) {
            (Ok(mut answered), None) => {
                without_root(&mut answered, &root);
                assert_eq!(
                    difference(&answered, &call["result"]),
                    None,
                    "{golden} call {index}"
                );
            }
            (Err(refused), Some(error)) => {
                let game = match &refused {
                    AdapterError::GameRunning { game, .. } => json!({
                        "launcher": game.launcher.as_str(),
                        "launcherGameId": game.launcher_game_id,
                        "name": game.name,
                        "source": "process",
                    }),
                    _ => Value::Null,
                };
                let refused = json!({
                    "message": refused.to_string(),
                    "runningGame": game,
                    "code": refused.code(),
                });
                let wanted = json!({
                    "message": error["message"],
                    "runningGame": error["runningGame"],
                    "code": error["code"],
                });
                assert_eq!(difference(&refused, &wanted), None, "{golden} call {index}");
            }
            (Ok(_), Some(_)) => panic!("{golden} call {index} was answered and is a refusal"),
            (Err(refused), None) => panic!("{golden} call {index} was refused: {refused}"),
        }
    }
    assert!(world.asked().is_empty(), "a plan asks nothing of a process");
}

fn as_it_is(_: &mut World) {}

fn client_running(world: &mut World) {
    world.processes.set_running(EXE, true);
}

#[test]
fn a_launch_that_follows_names_the_games_arguments_in_the_start_step() {
    check(
        "T-riot-adapter-fc186a14f5",
        &[Some(&as_it_is), Some(&as_it_is)],
    );
}

#[test]
fn the_saved_copy_path_with_the_change_it_makes_and_a_running_client_that_will_be_closed() {
    check(
        "T-riot-adapter-4fab74d271",
        &[Some(&client_running), Some(&as_it_is)],
    );
}

#[test]
fn an_account_whose_sign_in_the_file_holds_is_in_place_and_active_once_the_client_has_signed_in() {
    check(
        "T-riot-adapter-fc225608f6",
        &[
            Some(&as_it_is),
            Some(&client_running),
            Some(&|world: &mut World| {
                world.write_log(&fixture("2026-10-04T20-34-14_10001_Riot Client.log"));
            }),
            Some(&as_it_is),
        ],
    );
}

#[test]
fn the_sign_out_path_for_an_account_without_a_saved_sign_in_or_with_a_stale_one() {
    check(
        "T-riot-adapter-baacd57759",
        &[
            Some(&as_it_is),
            Some(&|world: &mut World| {
                let signed_out = fixture("RiotGamesPrivateSettings-signed-out.yaml");
                fs::write(world.session_file(), signed_out).unwrap();
            }),
            Some(&|world: &mut World| {
                world.book.sessions[0].0.stale_reason =
                    Some("the client refused the saved sign-in (401 invalid_grant)".to_owned());
            }),
        ],
    );
}

#[test]
fn a_plan_without_the_session_file_cannot_run_and_says_that_the_file_is_never_made() {
    check(
        "T-riot-adapter-9cc2befec5",
        &[
            Some(&|world: &mut World| fs::remove_file(world.session_file()).unwrap()),
            None,
        ],
    );
}

#[test]
fn a_running_riot_game_refuses_the_preflight_and_is_the_plans_first_warning() {
    check(
        "T-riot-adapter-785897eb5b",
        &[
            Some(&|world: &mut World| {
                world.processes.set_running(EXE, true);
                world.processes.add_name_to_table("VALORANT-Win64-Shipping");
            }),
            None,
            Some(&as_it_is),
        ],
    );
}

#[test]
fn a_sign_in_nobody_owns_is_said_to_be_kept_by_the_backup() {
    check(
        "T-riot-adapter-ccc4ee5207",
        &[
            Some(&|world: &mut World| {
                world.book.accounts.lock().unwrap()[0].launcher_uid = None;
            }),
            None,
        ],
    );
}

// What a plan may say of a sign-in: the end of its account id and the start of its fingerprint. Never a token, and never the Riot ID.
#[test]
fn a_plan_spells_out_no_token_and_no_riot_id() {
    let world = world();
    let adapter = world.adapter();
    let accounts = world.book.accounts.lock().unwrap().clone();
    let on_disk = String::from_utf8(fixture("RiotGamesPrivateSettings.yaml")).unwrap();
    let value_of = |key: &str| {
        let line = on_disk.lines().find(|line| line.trim().starts_with(key));
        let value = line.unwrap().split_once(": ").unwrap().1;
        value.trim().trim_matches('"').to_owned()
    };
    let secrets = [
        value_of("refresh_token:"),
        value_of("id_token:"),
        alt_token(),
    ];
    assert!(secrets.iter().all(|secret| secret.len() >= 40));
    for account in &accounts {
        let game = GameRef {
            launcher_game_id: "valorant.live",
            name: "VALORANT",
        };
        let plan = adapter.build_switch_plan(account, Some(game)).unwrap().0;
        let said = plan_json(&plan).to_string();
        assert!(said.contains("fingerprint") || account.id.as_deref() == Some(NEW));
        for secret in &secrets {
            assert!(!said.contains(secret.as_str()));
        }
        assert!(!said.contains("eyJ") && !said.contains("Placeholder"));
    }
    assert!(world.asked().is_empty());
}

#[test]
fn the_library_lists_what_is_installed_and_the_client_is_found_by_its_uninstall_entry() {
    let world = world();
    let adapter = world.adapter();
    let install = adapter.require_install().unwrap();
    assert!(install.source == ol_model::InstallSource::RegistryHkcu);
    assert!(
        install
            .exe_path
            .ends_with(r"Riot Games\Riot Client\RiotClientServices.exe")
    );

    let games = adapter.list_installed_games().unwrap();
    let listed: Vec<(&str, &str)> = games
        .iter()
        .map(|game| (game.launcher_game_id.as_str(), game.name.as_str()))
        .collect();
    // League has no mark of a finished install, Teamfight Tactics no folder, and Runeterra no settings.
    assert_eq!(
        listed,
        [
            ("valorant.live", "VALORANT"),
            ("valorant.pbe", "VALORANT (PBE)")
        ]
    );
    assert_eq!(
        games[0].last_played_at.as_deref(),
        Some("2025-10-09T08:53:20.000Z")
    );
    assert!(games.iter().all(|game| !game.needs_update));

    let state = adapter.session_state().unwrap();
    assert!(state.remembered && !state.launcher_running);
    assert_eq!(state.launcher_uid.as_deref(), Some(MAIN_UID));
    assert_eq!(state.linked_account_id.as_deref(), Some(MAIN));
    let remembered = adapter.list_remembered_accounts().unwrap();
    assert_eq!(remembered.len(), 1);
    assert_eq!(remembered[0].launcher_uid, MAIN_UID);
    assert_eq!(remembered[0].login, "riot_main");
    assert!(world.asked().is_empty());
    assert!(world.registry.dump().len() == 4);
}

#[test]
fn a_file_the_reader_does_not_know_is_an_error_and_never_a_client_that_is_signed_out() {
    let world = world();
    let accounts = world.book.accounts.lock().unwrap().clone();
    for (broken, reason) in [
        (
            b"psl:\r\n    authorization:\r\n        other: 1\r\n".to_vec(),
            "no psl.authorization.riot-client block",
        ),
        (
            b"psl:\r\n    authorization:\r\n        riot-client: {}\r\n".to_vec(),
            "riot-client is neither null nor a block",
        ),
        (
            b"psl:\r\n    authorization:\r\n        riot-client: null\r\nname: \xff\xfe\r\n"
                .to_vec(),
            "not UTF-8 text",
        ),
    ] {
        fs::write(world.session_file(), broken).unwrap();
        let adapter = world.adapter();
        let wanted = format!(
            "RiotGamesPrivateSettings.yaml is not in the shape One Launch knows ({reason}): {}",
            world.session_file().to_str().unwrap()
        );
        let refusals = [
            adapter.session_state().err(),
            adapter.list_remembered_accounts().err(),
            adapter.plan_switch(&accounts[1]).err(),
            adapter.preflight(&accounts[1], None).err(),
        ];
        for refused in refusals {
            let refused = refused.expect("a refusal");
            assert_eq!(refused.code(), "LAUNCHER_FILE");
            assert_eq!(refused.to_string(), wanted);
        }
    }
}

#[test]
fn the_client_is_closed_by_its_windows_first_the_core_and_then_what_it_leaves_behind() {
    let world = world();
    world.processes.set_running(EXE, true);
    world.processes.set_running(WINDOW_EXE, true);
    world.adapter().shutdown(None, &CancelToken::new()).unwrap();
    assert_eq!(
        world.asked(),
        [
            "proc.close RiotClientServices.exe",
            "proc.close Riot Client.exe"
        ]
    );
    let adapter = world.adapter();
    assert!(!adapter.is_running().unwrap());

    // With no core there is nothing to close but window processes left behind.
    let world = self::world();
    world.adapter().shutdown(None, &CancelToken::new()).unwrap();
    assert!(world.asked().is_empty());
    world.processes.set_running(WINDOW_EXE, true);
    world.adapter().shutdown(None, &CancelToken::new()).unwrap();
    assert_eq!(world.asked(), ["proc.close Riot Client.exe"]);
}

#[test]
fn a_client_that_ignores_its_window_close_is_ended_alone_and_never_with_what_it_started() {
    let world = world_with(ProcessKnobs {
        shutdown_polls: Some(u32::MAX),
        ..ProcessKnobs::default()
    });
    world.processes.set_running(EXE, true);
    world.processes.set_running(WINDOW_EXE, true);
    // Something the client started runs beside it. It is no game: beside a game the client is not closed at all.
    world
        .processes
        .set_running("RiotClientCrashHandler.exe", true);
    world.adapter().shutdown(None, &CancelToken::new()).unwrap();
    assert_eq!(
        world.asked(),
        [
            "proc.close RiotClientServices.exe",
            "proc.kill RiotClientServices.exe tree=false",
            "proc.close Riot Client.exe",
            "proc.kill Riot Client.exe tree=false",
        ]
    );
    let processes = &world.platform.processes;
    assert!(processes.is_running("RiotClientCrashHandler.exe").unwrap());
}

#[test]
fn nothing_of_the_client_is_closed_beside_a_riot_game_not_even_a_client_that_came_back() {
    let refusal =
        "VALORANT is running. Close it, then try again: One Launch never closes a Riot game.";
    let refused = |world: &World| {
        let refused = world.adapter().shutdown(None, &CancelToken::new());
        let refused = refused.expect_err("a refusal");
        assert_eq!(refused.code(), "GAME_RUNNING");
        assert_eq!(refused.to_string(), refusal);
    };

    let world = world();
    world.processes.set_running(EXE, true);
    world.processes.set_running(WINDOW_EXE, true);
    world.processes.add_name_to_table("VALORANT.exe");
    refused(&world);
    assert!(world.asked().is_empty());

    // The windows a client left behind are the client's too.
    let world = self::world();
    world.processes.set_running(WINDOW_EXE, true);
    world.processes.add_name_to_table("VALORANT.exe");
    refused(&world);
    assert!(world.asked().is_empty());

    // The client comes back by itself after its close, and a game starts meanwhile: the client is not closed a second time.
    let world = world_with(ProcessKnobs {
        reappears_after_polls: BTreeMap::from([(EXE.to_owned(), 1)]),
        ..ProcessKnobs::default()
    });
    world.processes.set_running(EXE, true);
    let processes = Arc::clone(&world.processes);
    world
        .clock
        .before_sleep(move || processes.add_name_to_table("VALORANT.exe"));
    refused(&world);
    assert_eq!(world.asked(), ["proc.close RiotClientServices.exe"]);
    assert!(world.adapter().is_running().unwrap());

    // Without the game the same client is closed again.
    let world = world_with(ProcessKnobs {
        reappears_after_polls: BTreeMap::from([(EXE.to_owned(), 1)]),
        ..ProcessKnobs::default()
    });
    world.processes.set_running(EXE, true);
    world.adapter().shutdown(None, &CancelToken::new()).unwrap();
    assert_eq!(
        world.asked(),
        [
            "proc.close RiotClientServices.exe",
            "proc.close RiotClientServices.exe"
        ]
    );
}

#[test]
fn a_riot_game_is_known_by_its_image_name_alone_and_refuses_a_launch() {
    let world = world();
    let adapter = world.adapter();
    assert!(adapter.find_running_game(None).unwrap().is_none());

    world.processes.add_name_to_table("LeagueClientUx");
    let game = adapter.find_running_game(None).unwrap().expect("a game");
    assert_eq!(
        (game.launcher_game_id.as_str(), game.name.as_str()),
        ("league_of_legends.live", "League of Legends")
    );
    let cancel = CancelToken::new();
    let refused = adapter
        .launch_game("valorant.live", "VALORANT", false, &cancel)
        .err()
        .expect("a refusal");
    assert_eq!(refused.code(), "GAME_RUNNING");
    assert!(refused.is_refusal());
    assert_eq!(
        refused.to_string(),
        "League of Legends is running. Close it, then press Play again: One Launch never closes a Riot game."
    );
    let same = adapter
        .launch_game(
            "league_of_legends.live",
            "League of Legends",
            false,
            &cancel,
        )
        .err()
        .expect("a refusal");
    assert_eq!(same.to_string(), "League of Legends is already running.");
    assert_eq!(
        adapter
            .launch_game("Riot Client", "x", false, &cancel)
            .err()
            .map(|refused| refused.to_string()),
        Some("Not a Riot product id: Riot Client".to_owned())
    );
    assert!(world.asked().is_empty());
}

#[test]
fn a_riot_game_under_its_short_name_counts_as_running() {
    let world = world();
    let adapter = world.adapter();
    world.processes.add_name_to_table("VGTRAY~1.EXE");
    assert!(adapter.find_running_game(None).unwrap().is_none());
    world.processes.add_name_to_table("VALORA~1.EXE");
    let game = adapter.find_running_game(None).unwrap().expect("a game");
    assert_eq!(game.launcher_game_id, "valorant.live");
}

#[test]
fn a_process_list_that_cannot_be_read_refuses_the_plan_the_preflight_and_the_launch() {
    let world = world();
    world.processes.fail_listings(true);
    let adapter = world.adapter();
    let alt = world.book.accounts.lock().unwrap()[1].clone();
    let cancel = CancelToken::new();
    assert!(adapter.find_running_game(None).is_err());
    assert!(adapter.plan_switch(&alt).is_err());
    assert!(adapter.preflight(&alt, None).is_err());
    let refused = adapter
        .launch_game("valorant.live", "VALORANT", false, &cancel)
        .err()
        .expect("a refusal");
    assert!(refused.to_string().contains("could not be read"));
    assert!(world.asked().is_empty());
    assert!(fs::read(world.session_file()).unwrap() == fixture("RiotGamesPrivateSettings.yaml"));
}
