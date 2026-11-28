#![forbid(unsafe_code)]

// Dry-run plans and preflights held against oracle/tests/goldens: what the Electron app's Battle.net adapter answered in its own tests. The account of each call comes from the golden; the test here only builds the machine those tests ran on: a client folder, the fixture's Battle.net.config and per-account file, the registry values an install leaves, a token under UnifiedAuth, three accounts of which two have a saved token, and the short waits.

use std::fs;
use std::sync::{Arc, Mutex};

use ol_adapters::battlenet::{BattlenetAdapter, BattlenetConfig, Timing};
use ol_adapters::health::Known;
use ol_adapters::{AccountRef, AdapterError, Directory};
use ol_formats::battlenet::products::LaunchForm;
use ol_model::{
    HealthCheck, HealthStatus, LauncherId, LauncherSession, SwitchPlan, SwitchPlanStep,
};
use ol_platform_api::Platform;
use ol_platform_fake::{
    FakeClock, FakeProcesses, FakeRegistry, FakeShell, OpLog, ProcessKnobs, RegistryValue,
    ScratchFiles,
};
use ol_runtime::TimingOverrides;
use ol_sim::BattlenetSim;
use ol_testkit::vectors::{difference, sha256_hex};
use ol_testkit::{Scratch, oracle};
use ol_vault::Secret;
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const TOKEN: &str = r"HKCU\Software\Blizzard Entertainment\Battle.net\UnifiedAuth\AA11BB22";
const MAIN: &str = "@acct:battlenet:saved1@example.com";
const ALT: &str = "@acct:battlenet:saved3@example.com";
const NEW: &str = "@acct:battlenet:new@example.com";
const STARTED_LINE: &str =
    "I 2026-09-13 03:20:48.092586 [Main] {Main} Logging started for Battle.net build 2.52.11.17778";
const GOLDENS: [&str; 6] = [
    "T-battlenet-adapter-4549663a63",
    "T-battlenet-adapter-5633fa792b",
    "T-battlenet-adapter-e6c4326a9e",
    "T-battlenet-adapter-bee0c90ffa",
    "T-battlenet-adapter-71c22bba0a",
    "T-battlenet-adapter-e5a389b67a",
];
const NEVER_RUN: [&str; 2] = [" (reg add /t REG_BINARY)", " (reg delete)"];
const COMMANDS: [&str; 5] = ["reg add", "reg delete", "reg.exe", "powershell", "cmd"];

// The accounts and saved tokens of the Electron tests, held in memory. A plan may read them and nothing else: every write is refused.
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

    fn decrypt_session(&self, account_id: &str) -> Result<Option<Secret>, AdapterError> {
        let of = |(session, _): &&(LauncherSession, String)| session.account_id == account_id;
        let stored = self.sessions.iter().find(of);
        Ok(stored.map(|(_, blob)| Secret::new(blob.clone())))
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
    registry: Arc<FakeRegistry>,
    processes: Arc<FakeProcesses>,
    platform: Platform,
    config: BattlenetConfig,
    book: Book,
}

fn read_json(relative: &str) -> Value {
    serde_json::from_str(&fs::read_to_string(oracle(relative)).unwrap()).unwrap()
}

fn fixture(name: &str) -> String {
    fs::read_to_string(oracle(&format!("fixtures/launchers/battlenet/{name}"))).unwrap()
}

// The nth token the Electron tests' scripted client makes up.
fn token(number: u32) -> String {
    format!("01000000d08c9ddf{number:04x}{}", "ab".repeat(24))
}

fn signed_in_line(uid: &str) -> String {
    format!(
        "I 2026-09-13 03:21:00.000000 [BNLogin] {{Main}} Logged into Battle.net successfully. |bnet=1:0:{uid}|game=2:100000000:0a1b2c3"
    )
}

impl World {
    fn config_file(&self) -> std::path::PathBuf {
        let folder = self.scratch.join("Roaming").join("Battle.net");
        folder.join("Battle.net.config")
    }

    // The client's log of its nth run, as the scripted client names them.
    fn write_log(&self, run: u32, text: &str) {
        let logs = self.scratch.join("Local").join("Battle.net").join("Logs");
        fs::create_dir_all(&logs).unwrap();
        let name = format!("battle.net-20260913T0320{run:02}.000000.log");
        fs::write(logs.join(name), text).unwrap();
    }
}

fn world() -> World {
    let scratch = Scratch::new("battlenet-plans");
    let root = scratch.path().to_str().unwrap().to_owned();
    let install = scratch.join("Battle.net");
    let settings = scratch.join("Roaming").join("Battle.net");
    for folder in [
        &install,
        &settings,
        &scratch.join("Games").join("Overwatch"),
    ] {
        fs::create_dir_all(folder).unwrap();
    }
    for program in ["Battle.net.exe", "Battle.net Launcher.exe"] {
        fs::write(install.join(program), b"").unwrap();
    }
    for file in ["Battle.net.config", "0a1b2c3d.config"] {
        fs::write(settings.join(file), fixture(file)).unwrap();
    }

    let log = OpLog::new();
    let registry = Arc::new(FakeRegistry::new(log.clone()));
    BattlenetSim::seed_registry(
        &registry,
        install.to_str().unwrap(),
        &format!("{root}\\Games"),
    );
    registry.seed(TOKEN, RegistryValue::Binary(token(1)));
    let processes = Arc::new(FakeProcesses::new(log.clone(), ProcessKnobs::default()));
    let clock = read_json("tests/capture.json")["clock"]
        .as_str()
        .map(|at| OffsetDateTime::parse(at, &Rfc3339).unwrap())
        .unwrap();
    let platform = Platform {
        registry: Arc::clone(&registry) as _,
        processes: Arc::clone(&processes) as _,
        files: Arc::new(ScratchFiles::new(&[scratch.path()], log.clone())),
        shell: Arc::new(FakeShell::new(log)),
        clock: Arc::new(FakeClock::starting_at(clock)),
    };
    let mut timing = TimingOverrides::default();
    for (name, milliseconds) in [
        ("pollMs", 5),
        ("shutdownMs", 200),
        ("killGraceMs", 100),
        ("restartWatchMs", 20),
        ("restartSettleMs", 100),
        ("startupMs", 500),
        ("signInMs", 500),
        ("manualSignInMs", 1_000),
        ("captureMs", 300),
        ("readyGraceMs", 0),
        ("launchConfirmMs", 50),
    ] {
        timing.set(name, milliseconds);
    }
    let config = BattlenetConfig {
        chosen_dir: None,
        default_dir: format!("{root}\\nowhere"),
        app_data: format!("{root}\\Roaming"),
        local_app_data: format!("{root}\\Local"),
        program_data: format!("{root}\\ProgramData"),
        launch_form: LaunchForm::Uri,
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
    let saved = |id: &str, uid: &str, name: &str, hex: String| {
        let session = LauncherSession {
            account_id: id.to_owned(),
            launcher: LauncherId::Battlenet,
            launcher_uid: Some(uid.to_owned()),
            fingerprint: sha256_hex(hex.as_bytes()),
            captured_at: "<ts>".to_owned(),
            verified_at: None,
            stale_reason: None,
        };
        let blob = json!({ "v": 1, "unifiedAuth": { "name": name, "hex": hex } });
        (session, blob.to_string())
    };
    let book = Book {
        accounts: Mutex::new(vec![
            account(MAIN, "Main", "saved1@example.com", Some("0a1b2c3d"), true),
            account(ALT, "Alt", "saved3@example.com", Some("0b2c3d4e"), false),
            account(NEW, "New", "new@example.com", None, true),
        ]),
        sessions: vec![
            saved(MAIN, "0a1b2c3d", "AA11BB22", token(1)),
            saved(ALT, "0b2c3d4e", "B1B1B1B1", token(2)),
        ],
    };
    let world = World {
        scratch,
        registry,
        processes,
        platform,
        config,
        book,
    };
    world.write_log(1, &fixture("battle.net-20260913T032048.091643.log"));
    world
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

// The Electron app ran reg.exe for these two steps; this build writes through the registry API, the owner's word of 2026-11-27, see plan/GAPS.md under Battle.net.
fn without_the_commands_this_build_never_runs(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            if let Some(Value::Array(steps)) = fields.get_mut("steps") {
                for step in steps {
                    if let Some(Value::String(detail)) = step.get_mut("detail") {
                        for fragment in NEVER_RUN {
                            *detail = detail.replace(fragment, "");
                        }
                    }
                }
            }
            fields
                .values_mut()
                .for_each(without_the_commands_this_build_never_runs);
        }
        Value::Array(items) => items
            .iter_mut()
            .for_each(without_the_commands_this_build_never_runs),
        _ => {}
    }
}

fn step_details(value: &Value, into: &mut Vec<String>) {
    match value {
        Value::Object(fields) => {
            if let Some(Value::Array(steps)) = fields.get("steps") {
                let details = steps.iter().filter_map(|step| step["detail"].as_str());
                into.extend(details.map(str::to_owned));
            }
            fields.values().for_each(|field| step_details(field, into));
        }
        Value::Array(items) => items.iter().for_each(|item| step_details(item, into)),
        _ => {}
    }
}

// Puts the first calls of a golden to the port, each on the machine `before` has prepared for it. The calls after them are real switches and restores, which are not put here.
fn check(golden: &str, before: &[&dyn Fn(&mut World)]) {
    let recorded = read_json(&format!("tests/goldens/{golden}.json"));
    let calls = recorded["calls"].as_array().unwrap();
    assert!(calls.len() >= before.len(), "{golden}: a machine per call");
    let mut world = world();
    let root = world.scratch.path().to_str().unwrap().to_owned();
    for (index, (call, prepare)) in calls.iter().zip(before).enumerate() {
        prepare(&mut world);
        let preflight = call["method"] == "preflight";
        if !preflight {
            assert_eq!(call["method"], "switchAccount", "{golden} call {index}");
            assert_eq!(call["args"][1]["dryRun"], true, "{golden} call {index}");
        }
        let given = &call["args"][0];
        let text = |name: &str| given[name].as_str().map(str::to_owned);
        let account = AccountRef {
            id: text("id"),
            display_name: text("displayName").unwrap(),
            login: text("login").unwrap(),
            launcher_uid: text("launcherUid"),
            has_secret: given["hasSecret"].as_bool().unwrap(),
        };
        let adapter = BattlenetAdapter::new(&world.platform, &world.config, &world.book);
        let mut answered = if preflight {
            let preflight = adapter.preflight(&account).unwrap();
            json!({
                "plan": plan_json(&preflight.plan),
                "launcherRunning": preflight.launcher_running,
                "runningGame": preflight.running_game.map(|game| game.name),
                "alreadyActive": preflight.already_active,
                "closeMessage": preflight.close_message,
            })
        } else {
            plan_json(&adapter.plan_switch(&account).unwrap())
        };
        without_root(&mut answered, &root);
        let mut expected = call["result"].clone();
        without_the_commands_this_build_never_runs(&mut expected);
        assert_eq!(
            difference(&answered, &expected),
            None,
            "{golden} call {index}"
        );
    }
    assert!(
        world.registry.dump().len() >= 10,
        "the registry the plans read is still there"
    );
}

fn as_it_is(_: &mut World) {}

fn client_running(world: &mut World) {
    world.processes.set_running("Battle.net.exe", true);
}

// The config with one line changed, as the Electron tests change it.
fn config_with(world: &World, from: &str, to: &str) {
    let changed = fixture("Battle.net.config").replace(from, to);
    assert!(changed.contains(to), "the fixture has the line");
    fs::write(world.config_file(), changed).unwrap();
}

#[test]
fn the_remembered_path_with_its_exact_registry_and_file_changes() {
    check("T-battlenet-adapter-4549663a63", &[&as_it_is]);
}

#[test]
fn the_credentials_path_and_an_account_whose_token_is_in_place_or_whose_client_runs() {
    check(
        "T-battlenet-adapter-5633fa792b",
        &[&as_it_is, &as_it_is, &client_running, &as_it_is, &as_it_is],
    );
}

#[test]
fn settings_the_plan_never_writes_are_warned_about_and_a_file_it_cannot_write_back_is_left() {
    check(
        "T-battlenet-adapter-e6c4326a9e",
        &[
            &|world: &mut World| {
                config_with(world, "\"AutoLogin\": \"true\"", "\"AutoLogin\": \"false\"");
                // A short account id names its file without padding, and the file of the account that signed in last decides.
                let settings = world.config_file().parent().unwrap().to_path_buf();
                fs::remove_file(settings.join("0a1b2c3d.config")).unwrap();
                let hides = fixture("0a1b2c3d.config")
                    .replace("\"HideOnClose\": \"false\"", "\"HideOnClose\": \"true\"");
                fs::write(settings.join("b7c8.config"), hides).unwrap();
                world.book.accounts.lock().unwrap()[0].launcher_uid = Some("b7c8".to_owned());
                let run = format!("{STARTED_LINE}\r\n{}\r\n", signed_in_line("b7c8"));
                world.write_log(2, &run);
            },
            &|world: &mut World| {
                config_with(
                    world,
                    "\"AutoLogin\": \"true\"",
                    "\"AutoLogin\":   \"true\"",
                );
            },
            &|world: &mut World| fs::remove_file(world.config_file()).unwrap(),
        ],
    );
}

#[test]
fn a_token_nobody_owns_is_said_to_be_kept_by_the_backup() {
    check(
        "T-battlenet-adapter-bee0c90ffa",
        &[&|world: &mut World| {
            world.registry.seed(TOKEN, RegistryValue::Binary(token(3)));
            let run = format!("{STARTED_LINE}\r\n{}\r\n", signed_in_line("0f0f0f0f"));
            world.write_log(2, &run);
        }],
    );
}

#[test]
fn a_list_that_cannot_be_rewritten_is_said_to_end_in_a_password_prompt() {
    check(
        "T-battlenet-adapter-71c22bba0a",
        &[&|world: &mut World| {
            config_with(
                world,
                "\"AutoLogin\": \"true\"",
                "\"AutoLogin\":   \"true\"",
            );
        }],
    );
}

#[test]
fn a_login_that_is_first_already_leaves_the_file_step_out() {
    check(
        "T-battlenet-adapter-e5a389b67a",
        &[&|world: &mut World| {
            let logins = "saved1@example.com,saved2@example.com,saved3@example.com";
            let moved = "saved3@example.com,saved1@example.com,saved2@example.com";
            config_with(world, logins, moved);
        }],
    );
}

#[test]
fn the_recordings_still_name_the_commands_the_rule_takes_out() {
    let mut recorded = Vec::new();
    let mut ruled = Vec::new();
    for golden in GOLDENS {
        let calls = read_json(&format!("tests/goldens/{golden}.json"))["calls"].clone();
        let results: Vec<Value> = calls
            .as_array()
            .unwrap()
            .iter()
            .map(|call| call["result"].clone())
            .collect();
        let mut results = Value::Array(results);
        step_details(&results, &mut recorded);
        without_the_commands_this_build_never_runs(&mut results);
        step_details(&results, &mut ruled);
    }
    for fragment in NEVER_RUN {
        let found = recorded.iter().filter(|detail| detail.contains(fragment));
        assert!(found.count() >= 1, "a recording names{fragment}");
    }
    assert_eq!(recorded.len(), ruled.len());
    for detail in &ruled {
        assert!(!detail.contains("reg add"), "{detail}");
        assert!(!detail.contains("reg delete"), "{detail}");
    }
}

#[test]
fn the_plans_name_no_command() {
    let mut details = Vec::new();
    for running in [false, true] {
        let mut world = world();
        if running {
            client_running(&mut world);
        }
        let accounts = world.book.accounts.lock().unwrap().clone();
        let adapter = BattlenetAdapter::new(&world.platform, &world.config, &world.book);
        for account in &accounts {
            let plan = adapter.plan_switch(account).unwrap();
            let preflight = adapter.preflight(account).unwrap();
            for steps in [&plan.steps, &preflight.plan.steps] {
                details.extend(steps.iter().map(|step| step.detail.clone()));
            }
        }
    }
    let restores = "write it as the REG_BINARY value under UnifiedAuth";
    assert!(details.iter().any(|detail| detail.contains(restores)));
    let signs_out = "Remove the UnifiedAuth value";
    assert!(details.iter().any(|detail| detail.starts_with(signs_out)));
    for detail in &details {
        let lower = detail.to_ascii_lowercase();
        for command in COMMANDS {
            assert!(!lower.contains(command), "{command}: {detail}");
        }
    }
}

fn health(world: &World) -> Vec<HealthCheck> {
    let adapter = BattlenetAdapter::new(&world.platform, &world.config, &world.book);
    let install = adapter.detect_install().unwrap();
    adapter.health_checks(&Known {
        install,
        running: None,
    })
}

fn health_check<'c>(checks: &'c [HealthCheck], id: &str) -> (HealthStatus, &'c str) {
    let check = checks.iter().find(|check| check.id == id).unwrap();
    (check.status, check.detail.as_str())
}

#[test]
fn a_config_that_cannot_be_read_leaves_the_products_their_install_folders() {
    let world = world();
    fs::remove_file(world.config_file()).unwrap();
    fs::create_dir(world.config_file()).unwrap();
    let checks = health(&world);
    assert_eq!(health_check(&checks, "config").0, HealthStatus::Fail);
    let (status, detail) = health_check(&checks, "uninstall");
    assert_eq!(status, HealthStatus::Ok, "{detail}");
    assert!(
        detail.ends_with("; 1 without an install folder"),
        "{detail}"
    );
}

#[test]
fn a_product_db_that_cannot_be_read_is_a_warning_and_the_built_in_codes_stay() {
    let world = world();
    let agent = world
        .scratch
        .join("ProgramData")
        .join("Battle.net")
        .join("Agent");
    fs::create_dir_all(agent.join("product.db")).unwrap();
    let checks = health(&world);
    let (status, detail) = health_check(&checks, "product-db");
    assert_eq!(status, HealthStatus::Warn, "{detail}");
    assert!(detail.starts_with("Could not read "), "{detail}");
    assert!(
        detail.contains("Launch codes come from the built-in table only: prometheus → "),
        "{detail}"
    );
    assert!(!detail.contains("0 install records"), "{detail}");
}

#[test]
fn a_log_folder_without_a_client_log_is_not_called_readable() {
    let world = world();
    let checks = health(&world);
    let (status, detail) = health_check(&checks, "log");
    assert_eq!(status, HealthStatus::Ok, "{detail}");
    assert!(detail.starts_with("Readable; "), "{detail}");

    let logs = world.scratch.join("Local").join("Battle.net").join("Logs");
    for entry in fs::read_dir(&logs).unwrap() {
        fs::remove_file(entry.unwrap().path()).unwrap();
    }
    let checks = health(&world);
    let (status, detail) = health_check(&checks, "log");
    assert_eq!(status, HealthStatus::Warn, "{detail}");
    assert!(detail.starts_with("No client log in "), "{detail}");
}

#[test]
fn a_saved_token_is_said_to_be_its_owners_whoever_the_log_names() {
    let mut world = world();
    client_running(&mut world);
    let run = format!(
        "{STARTED_LINE}
{}
",
        signed_in_line("0b2c3d4e")
    );
    world.write_log(2, &run);
    let checks = health(&world);
    let (_, detail) = health_check(&checks, "token");
    assert!(
        detail.contains("; belongs to Main; the client log names Alt ("),
        "{detail}"
    );

    world.registry.seed(TOKEN, RegistryValue::Binary(token(9)));
    let checks = health(&world);
    let (_, detail) = health_check(&checks, "token");
    assert!(
        detail.contains("; matches no saved sign-in; the client log names Alt ("),
        "{detail}"
    );
}

#[test]
fn locate_takes_either_program_only_when_the_picked_one_is_there() {
    let world = world();
    let adapter = BattlenetAdapter::new(&world.platform, &world.config, &world.book);
    let install = world.scratch.join("Battle.net");
    let picked = |name: &str| install.join(name).to_str().unwrap().to_owned();
    assert!(
        adapter
            .install_from_exe(&picked("Battle.net Launcher.exe"))
            .is_some()
    );
    assert!(
        adapter
            .install_from_exe(&picked("battle.net.exe"))
            .is_some()
    );
    fs::remove_file(install.join("Battle.net Launcher.exe")).unwrap();
    assert!(
        adapter
            .install_from_exe(&picked("Battle.net Launcher.exe"))
            .is_none()
    );
    assert!(
        adapter
            .install_from_exe(&picked("Battle.net.exe"))
            .is_some()
    );
}
