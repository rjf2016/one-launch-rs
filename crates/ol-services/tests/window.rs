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

#[test]
fn a_stored_place_is_used_only_while_its_title_bar_is_on_a_screen() {
    use ol_services::window::{Rect, placement_from};
    use ol_testkit::vectors::plain;

    let rect = |value: &Value| Rect {
        x: value["x"].as_f64().expect("x"),
        y: value["y"].as_f64().expect("y"),
        width: value["width"].as_f64().expect("width"),
        height: value["height"].as_f64().expect("height"),
    };
    run("l1/core/placementFrom.json", &[], |case| {
        let raw = plain(&case.args[0]);
        let areas: Vec<Rect> = plain(&case.args[1])
            .as_array()
            .expect("work areas")
            .iter()
            .map(rect)
            .collect();
        let placement = placement_from(raw.as_str(), &areas);
        let bounds = placement.bounds.map_or(Value::Null, |bounds| {
            serde_json::json!({
                "x": bounds.x,
                "y": bounds.y,
                "width": bounds.width,
                "height": bounds.height,
            })
        });
        Answer::Out(serde_json::json!({ "bounds": bounds, "maximized": placement.maximized }))
    });
}

#[test]
fn the_place_is_stored_as_the_electron_app_stores_it() {
    use ol_services::window::{bounds_setting, placement_from};

    let examples: Value = serde_json::from_str(
        &std::fs::read_to_string(ol_testkit::oracle(
            "fixtures/settings/window-bounds-examples.json",
        ))
        .expect("the examples"),
    )
    .expect("JSON");
    assert_eq!(
        bounds_setting(120, 80, 1400, 860, false),
        examples["normal"].as_str().expect("normal")
    );
    assert_eq!(
        bounds_setting(120, 80, 1400, 860, true),
        examples["maximizedWithNormalBounds"]
            .as_str()
            .expect("maximized")
    );
    let screen = [ol_services::window::Rect {
        x: 0.,
        y: 0.,
        width: 1920.,
        height: 1040.,
    }];
    for (example, kept) in [
        ("offScreenDroppedOnRead", false),
        ("malformedIgnored", false),
        ("tooSmallClampedOnRead", true),
    ] {
        let placement = placement_from(examples[example].as_str(), &screen);
        assert_eq!(placement.bounds.is_some(), kept, "{example}");
    }
}
