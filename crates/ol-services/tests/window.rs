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
    for example in ["offScreenDroppedOnRead", "malformedIgnored"] {
        let placement = placement_from(examples[example].as_str(), &screen);
        assert_eq!(placement.bounds, None, "{example}");
    }
    let raised = placement_from(examples["tooSmallClampedOnRead"].as_str(), &screen)
        .bounds
        .expect("a small window on the screen is kept");
    assert_eq!(
        (raised.x, raised.y, raised.width, raised.height),
        (0., 0., 960., 600.)
    );
}

// Two displays, the second to the left of the first and at another size: a place is found on the display its title bar lies on, and a size larger than that display is cut to it.
#[test]
fn a_place_opens_on_the_display_it_names_at_a_size_that_display_holds() {
    use ol_services::window::{Rect, display_for, fit, placement_from};

    let rect = |x: f64, y: f64, width: f64, height: f64| Rect {
        x,
        y,
        width,
        height,
    };
    let areas = [rect(0., 0., 1920., 1040.), rect(-1280., 100., 1280., 984.)];
    let left = placement_from(
        Some(r#"{"x":-1200,"y":150,"width":1100,"height":700,"maximized":true}"#),
        &areas,
    );
    let bounds = left.bounds.expect("on the second display");
    assert!(left.maximized);
    assert_eq!(display_for(&bounds, &areas), Some(1));
    assert_eq!(fit(bounds, &areas[1]), bounds);
    let half_off = placement_from(
        Some(r#"{"x":-1250,"y":150,"width":1280,"height":800}"#),
        &areas[..1],
    );
    assert_eq!(half_off.bounds, None);
    let straddling = placement_from(
        Some(r#"{"x":-1250,"y":150,"width":1280,"height":800}"#),
        &areas,
    )
    .bounds
    .expect("its title bar is on the second display");
    assert_eq!(display_for(&straddling, &areas), Some(1));
    let spanning = rect(-100., 150., 1280., 800.);
    assert_eq!(display_for(&spanning, &areas), Some(0));
    let mostly_left = rect(-1100., 150., 1280., 800.);
    assert_eq!(display_for(&mostly_left, &areas), Some(1));
    let huge = placement_from(
        Some(r#"{"x":100,"y":100,"width":3e9,"height":800}"#),
        &areas,
    )
    .bounds
    .expect("the rule keeps any size");
    assert_eq!(huge.width, 3e9);
    let fitted = fit(huge, &areas[display_for(&huge, &areas).unwrap()]);
    assert_eq!((fitted.x, fitted.width, fitted.height), (100., 1920., 800.));
    let tiny = rect(0., 0., 800., 500.);
    assert_eq!(fit(rect(10., 10., 960., 600.), &tiny).width, 960.);
}
