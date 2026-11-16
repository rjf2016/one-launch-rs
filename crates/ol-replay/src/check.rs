// Holds the replay of a recorded run to its recording in oracle/l2, as far as this build goes: Steam, Epic and Battle.net. Left out of every comparison, because nothing builds them yet: Riot's games, files, registry values and saved sign-ins, artwork, the vault probe, the log lines and the copies to the clipboard.

use std::fs;

use ol_model::LauncherId;
use ol_testkit::vectors::difference;
use ol_testkit::{Scratch, oracle};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::{Replay, Setup};

const BUILT: [&str; 3] = ["steam", "epic", "battlenet"];
const FILES: [&str; 3] = [
    "steam/config/loginusers.vdf",
    "localappdata/EpicGamesLauncher/Saved/Config/Windows/GameUserSettings.ini",
    "appdata/Battle.net/Battle.net.config",
];
// The registry values of the one launcher that is not built.
const NOT_BUILT_REGISTRY: [&str; 2] = [
    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Riot Game ",
    r"HKLM\SOFTWARE\Classes\riotclient\",
];
const NOT_BUILT: LauncherId = LauncherId::Riot;

pub fn recorded(folder: &str, name: &str) -> Value {
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

// A script replayed, whole or around the launcher that is not built; and what was recorded for the whole script.
fn replay(name: &str, around_the_unbuilt: bool) -> (Value, Value) {
    let rules = recorded("normalisation", name);
    let clock = OffsetDateTime::parse(rules["clock"].as_str().unwrap(), &Rfc3339).unwrap();
    let scratch = Scratch::new("replay-check");
    let version = pinned_version();
    let setup = Setup {
        scenarios: &oracle("fixtures/dev-only/fake-launchers"),
        fixtures: &oracle("fixtures/launchers"),
        scratch: scratch.path(),
        version: &version,
        clock,
    };
    let script = recorded("scripts", name);
    let actual = if around_the_unbuilt {
        Replay::run_without(&setup, &script, &rules, NOT_BUILT)
    } else {
        Replay::run(&setup, &script, &rules)
    };
    let actual = actual.unwrap_or_else(|error| panic!("{name} could not be replayed: {error}"));
    (actual, recorded("expected", name))
}

fn is_of_the_unbuilt(alias: &Value) -> bool {
    let alias = alias.as_str().unwrap_or_default();
    alias.split(':').nth(1) == Some(NOT_BUILT.as_str())
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
        .map(|event| {
            let mut event = event.clone();
            event.as_object_mut().unwrap().remove("seq");
            event
        })
        .collect()
}

// The jobs a script starts for an account of the launcher that is not built, by their aliases.
fn unbuilt_jobs(script: &Value) -> Vec<String> {
    let steps = script["steps"].as_array().unwrap();
    let starts = steps
        .iter()
        .filter(|step| step["channel"] == "launch:start");
    starts
        .enumerate()
        .filter(|(_, step)| is_of_the_unbuilt(&step["req"]["accountId"]))
        .map(|(index, _)| format!("@job:{}", index + 1))
        .collect()
}

// The events of a recording without those of the named jobs: everything from a job's first event to its last, which is what the app did for that job.
fn outside_jobs(events: &[Value], jobs: &[String]) -> Vec<Value> {
    let spans: Vec<(usize, usize)> = jobs
        .iter()
        .filter_map(|job| {
            let of_job = |event: &Value| event["job"] == job.as_str();
            Some((
                events.iter().position(of_job)?,
                events.iter().rposition(of_job)?,
            ))
        })
        .collect();
    events
        .iter()
        .enumerate()
        .filter(|(at, _)| !spans.iter().any(|(first, last)| first <= at && at <= last))
        .map(|(_, event)| event.clone())
        .collect()
}

fn rows(table: &Value, keep: impl Fn(&Value) -> bool) -> Vec<&Value> {
    table
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| keep(row))
        .collect()
}

// What of a final state this build can be held to: see the head of this file. `everyone` takes the accounts and launches of every launcher; without it those of the launcher that is not built are left out, for a run that went on to switch it.
fn built_slice(state: &Value, everyone: bool) -> Value {
    let tables = &state["db"]["tables"];
    let is_built = |row: &Value| BUILT.contains(&row["launcher"].as_str().unwrap());
    let mut backups = Map::new();
    backups.insert("db".to_owned(), state["backups"]["db"].clone());
    for launcher in BUILT {
        backups.insert(launcher.to_owned(), state["backups"][launcher].clone());
    }
    let files: Map<String, Value> = FILES
        .iter()
        .map(|file| ((*file).to_owned(), state["launcherFiles"][*file].clone()))
        .collect();
    let registry = rows(&state["registry"], |value| {
        let path = value["path"].as_str().unwrap();
        !NOT_BUILT_REGISTRY.iter().any(|key| path.starts_with(key))
    });
    json!({
        "db": {
            "user_version": state["db"]["user_version"],
            "accounts": rows(&tables["accounts"], |row| everyone || is_built(row)),
            "games": rows(&tables["games"], is_built),
            "launch_history": rows(&tables["launch_history"], |row| {
                everyone || !is_of_the_unbuilt(&row["account_id"])
            }),
            "launcher_sessions": rows(&tables["launcher_sessions"], is_built),
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
    let mut from = 0;
    for (index, wanted) in events(wanted, "job").iter().enumerate() {
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
pub fn check(name: &str) {
    let (actual, expected) = replay(name, false);
    check_events(name, &actual, expected["events"].as_array().unwrap());
    assert_eq!(
        difference(
            &built_slice(&actual["final"], true),
            &built_slice(&expected["final"], true)
        ),
        None,
        "{name}: the final state"
    );
}

// A script that also switches the launcher that is not built, replayed without the calls that name it: every event outside that launcher's jobs, and what the script left behind for the launchers that are built.
pub fn check_around_the_unbuilt(name: &str) {
    let jobs = unbuilt_jobs(&recorded("scripts", name));
    assert!(
        !jobs.is_empty(),
        "{name} starts a job this build cannot run"
    );
    let (actual, expected) = replay(name, true);
    let wanted = outside_jobs(expected["events"].as_array().unwrap(), &jobs);
    assert!(
        wanted.iter().any(|event| event["k"] == "job"),
        "{name}: the expectation has jobs this build runs"
    );
    check_events(name, &actual, &wanted);
    assert_eq!(
        difference(
            &built_slice(&actual["final"], false),
            &built_slice(&expected["final"], false)
        ),
        None,
        "{name}: the final state"
    );
}
