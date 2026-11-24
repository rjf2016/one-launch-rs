#![forbid(unsafe_code)]

// Every case of oracle/l1 for the window's close rule, the hide after a one-click Play and the start in the tray, run against the port.

use ol_services::window::{AfterPlay, close_request_action, hide_after_instant_play, start_hidden};
use ol_testkit::vectors::{Answer, run};
use serde_json::Value;

fn flag(value: &Value, name: &str) -> bool {
    value[name].as_bool().expect("a flag")
}

#[test]
fn a_close_closes_hides_or_asks_as_in_the_electron_app() {
    run("l1/core/closeRequestAction.json", &[], |case| {
        let state = &case.args[0];
        Answer::Out(
            close_request_action(
                flag(state, "quitting"),
                flag(state, "closeToTray"),
                flag(state, "switchBusy"),
            )
            .as_str()
            .into(),
        )
    });
}

#[test]
fn only_a_one_click_play_that_went_straight_to_the_game_hides_the_window() {
    run("l1/core/hideAfterInstantPlay.json", &[], |case| {
        let state = &case.args[0];
        Answer::Out(
            hide_after_instant_play(AfterPlay {
                instant: flag(state, "instant"),
                direct: flag(state, "direct"),
                done: flag(state, "done"),
                hand_off: flag(state, "handOff"),
                tray_present: flag(state, "trayPresent"),
                close_to_tray: flag(state, "closeToTray"),
                hide_to_tray: flag(state, "hideToTray"),
            })
            .into(),
        )
    });
}

#[test]
fn a_start_stays_in_the_tray_only_when_asked_and_the_tray_is_on() {
    run("l1/core/startHidden.json", &[], |case| {
        let input = &case.args[0];
        let args: Vec<&str> = input["argv"]
            .as_array()
            .expect("argv")
            .iter()
            .map(|arg| arg.as_str().expect("text"))
            .collect();
        Answer::Out(start_hidden(&args, flag(input, "closeToTray")).into())
    });
}
