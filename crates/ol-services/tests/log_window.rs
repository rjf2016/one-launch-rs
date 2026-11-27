#![forbid(unsafe_code)]

use std::fs;
use std::time::Duration;

use ol_model::LogLevel;
use ol_services::log::{LogOptions, Logger};
use ol_services::window::{AfterPlay, CloseAction, QuitAnswer, WindowLog};
use ol_testkit::{Scratch, oracle};

const WAIT: Duration = Duration::from_secs(10);

fn lines(logger: &Logger) -> Vec<String> {
    assert!(logger.flush(WAIT));
    fs::read_to_string(logger.current_path())
        .unwrap()
        .lines()
        .map(|line| line.split_once(' ').unwrap().1.to_owned())
        .collect()
}

fn job_id() -> String {
    let journal: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(oracle("fixtures/journal/switch-before-writes.json")).unwrap(),
    )
    .unwrap();
    journal["jobId"].as_str().unwrap().to_owned()
}

fn every_line(window: &WindowLog) {
    let job = job_id();
    window.close_requested(CloseAction::Hide);
    window.hidden_to_tray();
    window.shown_from_tray();
    window.hidden_after_launch(&job);
    window.kept_after_launch(
        &job,
        AfterPlay {
            direct: false,
            done: true,
            hand_off: false,
            tray_present: true,
            close_to_tray: true,
            hide_to_tray: true,
        },
    );
    window.tray_added();
    window.tray_removed();
    window.tray_unavailable("TRAY_WINDOW", Some(0x8007_0005_u32.cast_signed()));
    window.tray_unavailable("TRAY_REFUSED", None);
    window.quit_asked(&job);
    window.quit_answered(QuitAnswer::Keep);
    window.quit_answered(QuitAnswer::Quit);
    window.stopping_for_quit();
    window.fatal_during_startup();
    window.started_in_tray();
    window.shown_for_second_start();
}

#[test]
fn the_window_the_tray_and_a_quit_are_written_with_facts_alone() {
    let scratch = Scratch::new("log-window");
    let logger = Logger::open(
        &scratch.join("logs"),
        LogOptions {
            level: LogLevel::Debug,
            ..LogOptions::default()
        },
    );
    every_line(&WindowLog::new(&logger.log()));
    assert_eq!(
        lines(&logger),
        [
            r#"DEBUG [main] window close requested {"action":"hide"}"#,
            "INFO  [main] window hidden to the tray",
            "INFO  [main] window shown from the tray",
            r#"INFO  [main] window hidden to the tray after a launch {"jobId":"[UUID…[REDACTED]]"}"#,
            r#"INFO  [main] window kept after a launch {"jobId":"[UUID…[REDACTED]]","direct":false,"handOff":false,"tray":true,"closeToTray":true}"#,
            "INFO  [main] tray icon added",
            "INFO  [main] tray icon removed",
            r#"WARN  [main] tray icon unavailable; closing the window quits {"error":{"code":"TRAY_WINDOW","hresult":"0x80070005"}}"#,
            r#"WARN  [main] tray icon unavailable; closing the window quits {"error":{"code":"TRAY_REFUSED"}}"#,
            r#"INFO  [main] quit prompt shown during a switch {"jobId":"[UUID…[REDACTED]]"}"#,
            r#"INFO  [main] quit prompt answered {"answer":"keep"}"#,
            r#"INFO  [main] quit prompt answered {"answer":"quit"}"#,
            r#"WARN  [main] stopping the running switch {"reason":"quit confirmed during a switch"}"#,
            "ERROR [main] fatal during startup",
            "INFO  [main] started in the tray",
            "INFO  [main] window shown for a second start",
        ]
    );
}

#[test]
fn the_close_buttons_action_is_written_only_at_debug() {
    let scratch = Scratch::new("log-window-info");
    let logger = Logger::open(&scratch.join("logs"), LogOptions::default());
    every_line(&WindowLog::new(&logger.log()));
    let written = lines(&logger);
    assert_eq!(written.len(), 15);
    assert!(!written.iter().any(|line| line.contains("close requested")));
    logger.log().set_level(LogLevel::Error);
    every_line(&WindowLog::new(&logger.log()));
    let written = lines(&logger);
    assert_eq!(written.len(), 16);
    assert_eq!(written[15], "ERROR [main] fatal during startup");
}
