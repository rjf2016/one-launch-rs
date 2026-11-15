#![forbid(unsafe_code)]

// The Steam replay scripts of oracle/l2, run through the backend on fake launchers and held against what the Electron app did.
//
// Compared in full: every platform event, in order; the job events, the expectation's being a subsequence of the replay's; and of the final state the schema version, the accounts, the launch history, the settings, the journal, the backups, Steam's games, Steam's loginusers.vdf and Steam's registry values.
//
// Left out, because this build has no code for it yet: the other three launchers (their games, files and registry values, and their saved sign-ins), artwork, the vault probe and the log lines.

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
fn steam_slice(state: &Value) -> Value {
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
            "accounts": tables["accounts"],
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

fn check(name: &str) {
    let (actual, expected) = replay(name);

    let (did, wanted) = (events(&actual, "plat"), events(&expected, "plat"));
    assert_eq!(
        difference(&did.clone().into(), &wanted.into()),
        None,
        "{name}: the platform events"
    );
    assert!(
        !did.is_empty(),
        "{name}: the run asked nothing of the machine"
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
            &steam_slice(&actual["final"]),
            &steam_slice(&expected["final"])
        ),
        None,
        "{name}: the final state"
    );
}

#[test]
fn play_steam() {
    check("play-steam");
}
