// Holds the replay of a recorded run to its recording in oracle/l2: every platform event, the job events, and what the run left behind for the four launchers, their games' artwork among it. Left out of every comparison: the vault probe and the log lines, because nothing builds them yet, and the copies to the clipboard, which the replay does not make.
//
// One difference from the recordings is on purpose, on the owner's word of 2026-11-22: a Riot game is looked up like every other game and gets its cover from SteamGridDB once a key is stored, so the launcher's own icon is no longer stored in a cover's place. Where a recording ends with that stand-in, the replay is held to what the same lookup leaves without it: no artwork row for the game, and the reason of the miss in place of the stand-in's sentence. `without_the_stand_in` is that rule, and the only one.

use std::fs;

use ol_testkit::vectors::difference;
use ol_testkit::{Scratch, oracle};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::{Replay, Setup};

const LAUNCHERS: [&str; 4] = ["steam", "epic", "battlenet", "riot"];
const FILES: [&str; 4] = [
    "steam/config/loginusers.vdf",
    "localappdata/EpicGamesLauncher/Saved/Config/Windows/GameUserSettings.ini",
    "appdata/Battle.net/Battle.net.config",
    "localappdata/Riot Games/Riot Client/Data/RiotGamesPrivateSettings.yaml",
];
const SEALED: &str = "_enc";
const STAND_IN_SOURCE: &str = "launcher";
const STAND_IN_MISS: &str = "No cover found online; showing the game\u{2019}s own icon";
const MISS_WITHOUT_A_KEY: &str = "Not found on Steam; add a SteamGridDB API key in Settings";

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

// A script replayed, and what was recorded for it.
fn replay(name: &str) -> (Value, Value) {
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
    let actual = Replay::run(&setup, &script, &rules)
        .unwrap_or_else(|error| panic!("{name} could not be replayed: {error}"));
    (actual, recorded("expected", name))
}

// The events of one kind, without the copies to the clipboard, which the replay does not make.
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

// The recorded artwork rows as this build is held to them: see the head of this file. Answers the rows and how many stand-ins it took out.
fn without_the_stand_in(artwork: &Value, attempts: &Value) -> (Value, Value, usize) {
    let stood_in = |row: &&Value| row["source"] == STAND_IN_SOURCE;
    let rows = artwork.as_array().cloned().unwrap_or_default();
    let games: Vec<Value> = rows
        .iter()
        .filter(stood_in)
        .map(|row| row["game_id"].clone())
        .collect();
    let kept: Vec<Value> = rows.iter().filter(|row| !stood_in(row)).cloned().collect();
    let mut misses = attempts.as_array().cloned().unwrap_or_default();
    for miss in &mut misses {
        if games.contains(&miss["game_id"]) && miss["error"] == STAND_IN_MISS {
            miss["error"] = MISS_WITHOUT_A_KEY.into();
        }
    }
    (kept.into(), misses.into(), games.len())
}

// What of a final state this build can be held to: see the head of this file.
fn comparable(state: &Value) -> Value {
    let tables = &state["db"]["tables"];
    let (artwork, attempts, _) =
        without_the_stand_in(&tables["artwork"], &tables["artwork_attempts"]);
    let mut backups = Map::new();
    backups.insert("db".to_owned(), state["backups"]["db"].clone());
    for launcher in LAUNCHERS {
        backups.insert(launcher.to_owned(), state["backups"][launcher].clone());
    }
    let files: Map<String, Value> = FILES
        .iter()
        .map(|file| ((*file).to_owned(), state["launcherFiles"][*file].clone()))
        .collect();
    json!({
        "db": {
            "user_version": state["db"]["user_version"],
            "accounts": tables["accounts"],
            "artwork": artwork,
            "artwork_attempts": attempts,
            "games": tables["games"],
            "launch_history": tables["launch_history"],
            "launcher_sessions": tables["launcher_sessions"],
            "settings": tables["settings"],
        },
        "journal": state["journal"],
        "backups": backups,
        "launcherFiles": files,
        "registry": state["registry"],
    })
}

// A recording gives one digest of each content: of its bytes where every recording wrote the same, else of its projection, without what a scripted client stamps into it. A sealed value whose plain text holds such a stamp stands under its column's name without `_enc`. The replay knows both digests of everything, and here takes the shape the recording has.
fn shaped_like(actual: &mut Value, expected: &Value) {
    match (actual, expected) {
        (Value::Array(items), Value::Array(wanted)) => {
            for (item, wanted) in items.iter_mut().zip(wanted) {
                shaped_like(item, wanted);
            }
        }
        (Value::Object(fields), Value::Object(wanted)) => {
            let sealed: Vec<String> = fields
                .keys()
                .filter(|key| key.ends_with(SEALED))
                .cloned()
                .collect();
            for key in sealed {
                let plain_name = key.trim_end_matches(SEALED);
                let projected = !wanted.contains_key(&key) && wanted.contains_key(plain_name);
                if projected {
                    let value = fields.shift_remove(&key).unwrap_or_default();
                    let shown = json!({
                        "present": value["present"],
                        "projectionSha256": value["projectionSha256"],
                    });
                    fields.insert(plain_name.to_owned(), shown);
                } else if let Some(Value::Object(value)) = fields.get_mut(&key) {
                    value.shift_remove("projectionSha256");
                }
            }
            if fields.contains_key("sha256") && fields.contains_key("projectionSha256") {
                let projected =
                    wanted.contains_key("projectionSha256") && !wanted.contains_key("sha256");
                fields.shift_remove(if projected {
                    "sha256"
                } else {
                    "projectionSha256"
                });
            }
            for (key, value) in fields.iter_mut() {
                if let Some(wanted) = wanted.get(key) {
                    shaped_like(value, wanted);
                }
            }
        }
        _ => {}
    }
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
    let (actual, expected) = replay(name);
    check_events(name, &actual, expected["events"].as_array().unwrap());
    let expected = comparable(&expected["final"]);
    let mut actual = comparable(&actual["final"]);
    shaped_like(&mut actual, &expected);
    assert_eq!(
        difference(&actual, &expected),
        None,
        "{name}: the final state"
    );
}
