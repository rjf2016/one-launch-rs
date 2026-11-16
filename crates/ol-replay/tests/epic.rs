#![forbid(unsafe_code)]

// The recorded runs that turn on Epic, held to what this build has: Steam and Epic. Left out of every comparison here, because nothing builds them yet: Battle.net's and Riot's games, files, registry values and saved sign-ins, artwork, the vault probe, the log lines and the copy to the clipboard.

use std::fs;

use ol_replay::{Replay, Setup};
use ol_testkit::vectors::difference;
use ol_testkit::{Scratch, oracle};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const BUILT: [&str; 2] = ["steam", "epic"];
const FILES: [&str; 2] = [
    "steam/config/loginusers.vdf",
    "localappdata/EpicGamesLauncher/Saved/Config/Windows/GameUserSettings.ini",
];
const REGISTRY: [&str; 5] = [
    r"HKCU\Software\Valve\Steam\",
    r"HKLM\SOFTWARE\WOW6432Node\Valve\Steam\",
    r"HKLM\SOFTWARE\Classes\com.epicgames.launcher\",
    r"HKLM\SOFTWARE\WOW6432Node\Epic Games\",
    r"HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\{AEB35C6C-B6D4-4AA0-8452-DE699737B5F6}\InstallLocation",
];

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

// The first `steps` steps of a script, or all of them, replayed; and what was recorded for the whole script.
fn replay(name: &str, steps: Option<usize>) -> (Value, Value) {
    let rules = recorded("normalisation", name);
    let clock = OffsetDateTime::parse(rules["clock"].as_str().unwrap(), &Rfc3339).unwrap();
    let scratch = Scratch::new("replay-epic");
    let version = pinned_version();
    let setup = Setup {
        scenarios: &oracle("fixtures/dev-only/fake-launchers"),
        fixtures: &oracle("fixtures/launchers"),
        scratch: scratch.path(),
        version: &version,
        clock,
    };
    let script = recorded("scripts", name);
    let actual = Replay::run_first(&setup, &script, &rules, steps.unwrap_or(usize::MAX))
        .unwrap_or_else(|error| panic!("{name} could not be replayed: {error}"));
    (actual, recorded("expected", name))
}

fn without_seq(event: &Value) -> Value {
    let mut event = event.clone();
    event.as_object_mut().unwrap().remove("seq");
    event
}

// The events of one kind, without the copies to the clipboard, which this build does not make.
fn events(all: &[Value], kind: &str) -> Vec<Value> {
    all.iter()
        .filter(|event| event["k"] == kind)
        .filter(|event| {
            !event["op"]
                .as_str()
                .is_some_and(|op| op.starts_with("clip."))
        })
        .map(without_seq)
        .collect()
}

fn rows_of_built_launchers(table: &Value) -> Vec<&Value> {
    table
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| BUILT.contains(&row["launcher"].as_str().unwrap()))
        .collect()
}

// What of a final state this build can be held to: see the head of this file.
fn built_slice(state: &Value) -> Value {
    let tables = &state["db"]["tables"];
    let mut backups = Map::new();
    backups.insert("db".to_owned(), state["backups"]["db"].clone());
    for launcher in BUILT {
        backups.insert(launcher.to_owned(), state["backups"][launcher].clone());
    }
    let files: Map<String, Value> = FILES
        .iter()
        .map(|file| ((*file).to_owned(), state["launcherFiles"][*file].clone()))
        .collect();
    let registry: Vec<&Value> = state["registry"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|value| {
            let path = value["path"].as_str().unwrap();
            REGISTRY.iter().any(|key| path.starts_with(key))
        })
        .collect();
    json!({
        "db": {
            "user_version": state["db"]["user_version"],
            "accounts": tables["accounts"],
            "games": rows_of_built_launchers(&tables["games"]),
            "launch_history": tables["launch_history"],
            "launcher_sessions": rows_of_built_launchers(&tables["launcher_sessions"]),
            "settings": tables["settings"],
        },
        "journal": state["journal"],
        "backups": backups,
        "launcherFiles": files,
        "registry": registry,
    })
}

// The platform events are the recorded ones in their order, and the recorded job events are among the replay's in their order.
fn check_events(name: &str, actual: &Value, wanted: &[Value]) {
    let did = actual["events"].as_array().unwrap();
    assert_eq!(
        difference(&events(did, "plat").into(), &events(wanted, "plat").into()),
        None,
        "{name}: the platform events"
    );
    let did = events(did, "job");
    let wanted = events(wanted, "job");
    assert!(!wanted.is_empty(), "{name}: the expectation has job events");
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

// A whole script: its events and what it left behind.
fn check(name: &str) {
    let (actual, expected) = replay(name, None);
    check_events(name, &actual, expected["events"].as_array().unwrap());
    assert_eq!(
        difference(
            &built_slice(&actual["final"]),
            &built_slice(&expected["final"])
        ),
        None,
        "{name}: the final state"
    );
}

// The part of a script that comes before a launcher this build has no adapter for: the events up to that launcher's first job. What the whole script left behind is not compared.
fn check_up_to(name: &str, launcher: &str, jobs: usize) {
    let script = recorded("scripts", name);
    let steps = script["steps"]
        .as_array()
        .unwrap()
        .iter()
        .position(|step| step.to_string().contains(&format!(":{launcher}:")))
        .expect("the script turns to the other launcher");
    let (actual, expected) = replay(name, Some(steps));
    let all = expected["events"].as_array().unwrap();
    let built: Vec<String> = (1..=jobs).map(|job| format!("@job:{job}")).collect();
    let end = all
        .iter()
        .position(|event| {
            event["k"] == "job" && !built.contains(&event["job"].as_str().unwrap().to_owned())
        })
        .expect("the expectation turns to the other launcher");
    check_events(name, &actual, &all[..end]);
}

#[test]
fn play_epic() {
    check("play-epic");
}

#[test]
fn handover() {
    check("handover");
}

#[test]
fn wrong_account() {
    check("wrong-account");
}

#[test]
fn quit_during_switch() {
    check("quit-during-switch");
}

#[test]
fn recovery() {
    check("recovery");
}

#[test]
fn play_all_up_to_battlenet() {
    check_up_to("play-all", "battlenet", 2);
}

#[test]
fn unowned_up_to_battlenet() {
    check_up_to("unowned", "battlenet", 1);
}
