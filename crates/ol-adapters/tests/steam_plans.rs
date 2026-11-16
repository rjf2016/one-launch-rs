#![forbid(unsafe_code)]

// Dry-run plans held against oracle/tests/goldens: what the Electron app's Steam adapter answered in its own tests. The account of each call comes from the golden; the test here only builds the machine those tests ran on, a Steam folder with the fixture's loginusers.vdf, the registry values a Steam install leaves, automatic sign-in on and the short waits.

use std::fs;
use std::sync::Arc;

use ol_adapters::steam::{AccountRef, SteamAdapter, SteamConfig, Timing};
use ol_model::{SwitchPlan, SwitchPlanStep};
use ol_platform_api::Platform;
use ol_platform_fake::{
    FakeClock, FakeProcesses, FakeRegistry, FakeShell, OpLog, ProcessKnobs, RegistryValue,
    ScratchFiles,
};
use ol_runtime::TimingOverrides;
use ol_sim::SteamSim;
use ol_testkit::vectors::difference;
use ol_testkit::{Scratch, oracle};
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const ACTIVE_USER: &str = r"HKCU\Software\Valve\Steam\ActiveProcess\ActiveUser";
const AUTO_LOGIN_USER: &str = r"HKCU\Software\Valve\Steam\AutoLoginUser";

struct World {
    scratch: Scratch,
    registry: Arc<FakeRegistry>,
    processes: Arc<FakeProcesses>,
    platform: Platform,
    config: SteamConfig,
}

fn read_json(relative: &str) -> Value {
    serde_json::from_str(&fs::read_to_string(oracle(relative)).unwrap()).unwrap()
}

fn world() -> World {
    let scratch = Scratch::new("steam-plans");
    let root = scratch.path().to_str().unwrap().to_owned();
    let steam_dir = scratch.join("Steam");
    fs::create_dir_all(steam_dir.join("config")).unwrap();
    fs::write(steam_dir.join("steam.exe"), b"").unwrap();
    fs::copy(
        oracle("fixtures/launchers/steam/loginusers.vdf"),
        steam_dir.join("config").join("loginusers.vdf"),
    )
    .unwrap();

    let log = OpLog::new();
    let registry = Arc::new(FakeRegistry::new(log.clone()));
    SteamSim::seed_registry(&registry, steam_dir.to_str().unwrap());
    let processes = Arc::new(FakeProcesses::new(log.clone(), ProcessKnobs::default()));
    // The results were recorded with the clock at this instant: a remembered session's age is part of a plan.
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
        ("pollMs", 1),
        ("shutdownMs", 40),
        ("killGraceMs", 20),
        ("startupMs", 40),
        ("signInMs", 40),
        ("guardMs", 40),
        ("manualSignInMs", 40),
        ("readyGraceMs", 0),
    ] {
        timing.set(name, milliseconds);
    }
    let config = SteamConfig {
        chosen_dir: None,
        default_dir: format!("{root}\\nowhere"),
        auto_login: true,
        backups_to_keep: 20,
        backups_dir: format!("{root}\\backups"),
        timing: Timing::with(&timing),
    };
    World {
        scratch,
        registry,
        processes,
        platform,
        config,
    }
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

// Puts each dry-run call of a golden to the port, on the machine `before` has prepared for that call.
fn check(golden: &str, before: &[&dyn Fn(&mut World)]) {
    let recorded = read_json(&format!("tests/goldens/{golden}.json"));
    let calls = recorded["calls"].as_array().unwrap();
    assert_eq!(
        calls.len(),
        before.len(),
        "{golden}: one machine state per call"
    );
    let mut world = world();
    let root = world.scratch.path().to_str().unwrap().to_owned();
    for (index, (call, prepare)) in calls.iter().zip(before).enumerate() {
        assert_eq!(call["method"], "switchAccount", "{golden} call {index}");
        assert_eq!(call["args"][1]["dryRun"], true, "{golden} call {index}");
        prepare(&mut world);
        let account = &call["args"][0];
        let text = |name: &str| account[name].as_str().map(str::to_owned);
        let account = AccountRef {
            id: text("id"),
            display_name: text("displayName").unwrap(),
            login: text("login").unwrap(),
            launcher_uid: text("launcherUid"),
            has_secret: account["hasSecret"].as_bool().unwrap(),
        };
        let plan = SteamAdapter::new(&world.platform, &world.config)
            .plan_switch(&account)
            .unwrap();
        let mut answered = plan_json(&plan);
        without_root(&mut answered, &root);
        assert_eq!(
            difference(&answered, &call["result"]),
            None,
            "{golden} call {index}"
        );
    }
    assert!(
        world.registry.dump().len() >= 5,
        "the registry the plans read is still there"
    );
}

fn as_it_is(_: &mut World) {}

fn steam_running(world: &mut World) {
    world.processes.set_running("steam.exe", true);
}

#[test]
fn the_remembered_path_with_its_exact_file_and_registry_changes() {
    check("T-steam-adapter-ea81f340c1", &[&steam_running]);
}

#[test]
fn an_account_that_is_already_active_is_a_no_op() {
    check(
        "T-steam-adapter-6e4253c8b5",
        &[&as_it_is, &|world: &mut World| {
            steam_running(world);
            let users = read_json("l1/steam/parseLoginUsers.json");
            let first = users["cases"][0]["out"][0]["steamId64"]
                .as_str()
                .and_then(ol_formats::steam::loginusers::steam_id3)
                .and_then(|id| u32::try_from(id).ok())
                .unwrap();
            world
                .registry
                .seed(ACTIVE_USER, RegistryValue::Dword(first));
        }],
    );
}

#[test]
fn the_credentials_path_for_an_account_steam_has_never_seen() {
    check("T-steam-adapter-0783e39980", &[&as_it_is, &as_it_is]);
}

#[test]
fn a_wrong_stored_id_is_matched_by_login_and_a_registry_that_disagrees_is_flagged() {
    check(
        "T-steam-adapter-73e6899565",
        &[&as_it_is, &|world: &mut World| {
            world
                .registry
                .seed(AUTO_LOGIN_USER, RegistryValue::Text("user_4".to_owned()));
        }],
    );
}

#[test]
fn with_automatic_sign_in_off_the_plan_names_no_password() {
    check(
        "T-steam-adapter-b1b3f90e68",
        &[&|world: &mut World| {
            world.config.auto_login = false;
        }],
    );
}

#[test]
fn a_file_whose_layout_the_rewrite_cannot_change_blocks_the_switch() {
    let world = world();
    let layout = "\"users\" {\n\t\"111111111111111\" {\n\t\t\"AccountName\"\t\t\"a\"\n\t\t\"RememberPassword\"\t\t\"1\"\n\t\t\"AutoLogin\"\t\t\"1\"\n\t}\n\t\"222222222222222\" {\n\t\t\"AccountName\"\t\t\"b\"\n\t\t\"RememberPassword\"\t\t\"1\"\n\t\t\"AutoLogin\"\t\t\"0\"\n\t}\n}\n";
    fs::write(
        world
            .scratch
            .join("Steam")
            .join("config")
            .join("loginusers.vdf"),
        layout,
    )
    .unwrap();
    world
        .registry
        .seed(AUTO_LOGIN_USER, RegistryValue::Text("b".to_owned()));
    let account = AccountRef {
        id: Some("account-b".to_owned()),
        display_name: "B".to_owned(),
        login: "b".to_owned(),
        launcher_uid: Some("222222222222222".to_owned()),
        has_secret: false,
    };
    let preflight = SteamAdapter::new(&world.platform, &world.config)
        .preflight(&account)
        .unwrap();
    assert!(!preflight.already_active);
    assert!(!preflight.plan.executable);
    assert_eq!(
        preflight.plan.warnings.first().map(String::as_str),
        Some(ol_formats::steam::loginusers::UNWRITABLE_LAYOUT)
    );
}
