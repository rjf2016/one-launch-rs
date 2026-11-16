#![forbid(unsafe_code)]

// A recorded run is held to its Steam part only, because this build has no code yet for the rest: the other three launchers (their games, files, registry values and saved sign-ins), artwork, the vault probe and the log lines are left out of every comparison here.

use std::fs;

use ol_replay::{Replay, Setup};
use ol_testkit::vectors::difference;
use ol_testkit::{Scratch, oracle};
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const STEAM_REGISTRY: [&str; 2] = [
    r"HKCU\Software\Valve\Steam\",
    r"HKLM\SOFTWARE\WOW6432Node\Valve\Steam\",
];
const STEAM_FILE: &str = "steam/config/loginusers.vdf";

fn recorded(folder: &str, name: &str) -> Value {
    let path = oracle(&format!("l2/{folder}/{name}.json"));
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

// The version the expectations were recorded at, which a fresh database counts as seen.
fn pinned_version() -> String {
    fs::read_to_string(oracle("PIN"))
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("version = "))
        .expect("oracle/PIN names a version")
        .to_owned()
}

fn replay(name: &str) -> (Value, Value) {
    let rules = recorded("normalisation", name);
    let clock = OffsetDateTime::parse(rules["clock"].as_str().unwrap(), &Rfc3339).unwrap();
    let scratch = Scratch::new("replay");
    let version = pinned_version();
    let setup = Setup {
        scenarios: &oracle("fixtures/dev-only/fake-launchers"),
        fixtures: &oracle("fixtures/launchers"),
        scratch: scratch.path(),
        version: &version,
        clock,
    };
    let actual = Replay::run(&setup, &recorded("scripts", name), &rules)
        .unwrap_or_else(|error| panic!("{name} could not be replayed: {error}"));
    (actual, recorded("expected", name))
}

fn events(report: &Value, kind: &str) -> Vec<Value> {
    report["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["k"] == kind)
        .map(|event| {
            let mut event = event.clone();
            event.as_object_mut().unwrap().remove("seq");
            event
        })
        .collect()
}

// What of a final state this build can be held to: see the head of this file.
fn steam_slice(state: &Value, accounts: Accounts) -> Value {
    let tables = &state["db"]["tables"];
    let steam_games: Vec<&Value> = tables["games"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|game| game["launcher"] == "steam")
        .collect();
    let steam_registry: Vec<&Value> = state["registry"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|value| {
            let path = value["path"].as_str().unwrap();
            STEAM_REGISTRY.iter().any(|key| path.starts_with(key))
        })
        .collect();
    json!({
        "db": {
            "user_version": state["db"]["user_version"],
            "accounts": accounts_of(&tables["accounts"], accounts),
            "games": steam_games,
            "launch_history": tables["launch_history"],
            "settings": tables["settings"],
        },
        "journal": state["journal"],
        "backups": state["backups"],
        "loginusers": state["launcherFiles"][STEAM_FILE],
        "registry": steam_registry,
    })
}

#[derive(Clone, Copy, PartialEq)]
enum Accounts {
    All,
    // For a script whose other half imports accounts of a launcher this build has no adapter for.
    SteamOnly,
}

fn accounts_of(table: &Value, which: Accounts) -> Vec<&Value> {
    table
        .as_array()
        .unwrap()
        .iter()
        .filter(|account| which == Accounts::All || account["launcher"] == "steam")
        .collect()
}

fn check(name: &str) {
    check_with(name, Accounts::All);
}

fn check_with(name: &str, accounts: Accounts) {
    let (actual, expected) = replay(name);

    let (did, wanted) = (events(&actual, "plat"), events(&expected, "plat"));
    assert_eq!(
        difference(&did.clone().into(), &wanted.into()),
        None,
        "{name}: the platform events"
    );

    let did = events(&actual, "job");
    let mut from = 0;
    for (index, wanted) in events(&expected, "job").iter().enumerate() {
        let found = did[from..].iter().position(|event| event == wanted);
        let Some(found) = found else {
            let nearest = did.get(from).cloned().unwrap_or(Value::Null);
            panic!(
                "{name}: job event {index} of the expectation is not in the replay after its event {from}: {:?}",
                difference(&nearest, wanted)
            );
        };
        from += found + 1;
    }

    assert_eq!(
        difference(
            &steam_slice(&actual["final"], accounts),
            &steam_slice(&expected["final"], accounts)
        ),
        None,
        "{name}: the final state"
    );
}

#[test]
fn play_steam() {
    check("play-steam");
}

#[test]
fn rollback() {
    check("rollback");
}

#[test]
fn elevated() {
    check("elevated");
}

#[test]
fn already_active() {
    check("already-active");
}

#[test]
fn restore() {
    check("restore");
}

#[test]
fn stop() {
    check("stop");
}

#[test]
fn self_update() {
    check("self-update");
}

#[test]
fn preview() {
    check("preview");
}

#[test]
fn import() {
    check_with("import", Accounts::SteamOnly);
}

// The script goes on to Epic, which this build has no adapter for. Its Steam half is replayed: a session that has expired, the hand-over to the user, a stop, and a wait that runs out. Left out with the rest of the script: the final state. Left out of the platform events: the two copies to the clipboard, which this build does not have yet.
#[test]
fn handover_up_to_the_first_epic_step() {
    let name = "handover";
    let script = recorded("scripts", name);
    let steam_steps = script["steps"]
        .as_array()
        .unwrap()
        .iter()
        .position(|step| step.to_string().contains(":epic:"))
        .expect("the script turns to Epic");
    let rules = recorded("normalisation", name);
    let clock = OffsetDateTime::parse(rules["clock"].as_str().unwrap(), &Rfc3339).unwrap();
    let scratch = Scratch::new("replay");
    let version = pinned_version();
    let setup = Setup {
        scenarios: &oracle("fixtures/dev-only/fake-launchers"),
        fixtures: &oracle("fixtures/launchers"),
        scratch: scratch.path(),
        version: &version,
        clock,
    };
    let actual = Replay::run_first(&setup, &script, &rules, steam_steps).unwrap();
    let expected = recorded("expected", name);

    let all = expected["events"].as_array().unwrap();
    let steam_jobs = ["@job:1", "@job:2"];
    let epic_from = all
        .iter()
        .position(|event| {
            event["k"] == "job" && !steam_jobs.contains(&event["job"].as_str().unwrap())
        })
        .expect("the expectation turns to Epic");
    let before_epic = json!({ "events": all[..epic_from] });
    let wanted: Vec<Value> = events(&before_epic, "plat")
        .into_iter()
        .filter(|event| !event["op"].as_str().unwrap().starts_with("clip."))
        .collect();
    assert_eq!(
        difference(&events(&actual, "plat").into(), &wanted.into()),
        None,
        "{name}: the platform events"
    );
    let did = events(&actual, "job");
    let wanted = events(&before_epic, "job");
    assert!(
        wanted.len() > 20,
        "{name}: the expectation has the two Steam jobs"
    );
    let mut from = 0;
    for (index, wanted) in wanted.iter().enumerate() {
        let Some(found) = did[from..].iter().position(|event| event == wanted) else {
            let nearest = did.get(from).cloned().unwrap_or(Value::Null);
            panic!(
                "{name}: job event {index} of the expectation is not in the replay after its event {from}: {:?}",
                difference(&nearest, wanted)
            );
        };
        from += found + 1;
    }
}
