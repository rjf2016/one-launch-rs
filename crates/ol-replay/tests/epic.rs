#![forbid(unsafe_code)]

// The recorded runs that turn on Epic, each replayed whole and held to its recording. What a comparison takes and leaves out is in ol_replay::check.

use ol_replay::check::{check, recorded};
use ol_replay::{Replay, Setup};
use ol_testkit::{Scratch, oracle};
use serde_json::{Value, json};
use time::macros::datetime;

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

// What a run of `steps` on the scenario of recovery leaves in the fake registry.
fn registry_after(steps: Value) -> Value {
    let scratch = Scratch::new("replay-crash");
    let setup = Setup {
        scenarios: &oracle("fixtures/dev-only/fake-launchers"),
        fixtures: &oracle("fixtures/launchers"),
        scratch: scratch.path(),
        version: "9.8.7",
        clock: datetime!(2026-11-06 16:46:23 UTC),
    };
    let script = json!({ "fake": "all-four", "steps": steps });
    let report = Replay::run(&setup, &script, &recorded("normalisation", "recovery")).unwrap();
    report["final"]["registry"].clone()
}

#[test]
fn a_run_that_ends_in_a_crash_reports_the_registry_the_app_left() {
    let quit = registry_after(json!([{ "op": "snapshot" }]));
    assert!(quit.as_array().is_some_and(|values| !values.is_empty()));
    assert_eq!(registry_after(json!([{ "op": "crash" }])), quit);
}
