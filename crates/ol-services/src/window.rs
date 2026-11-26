// The rules of the window that the settings decide: what a close does, when a one-click Play sends the window to the tray, when a start stays in the tray, and the question asked before quitting during a switch.

use ol_model::LaunchJob;
use serde_json::json;

use crate::log::Log;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseAction {
    Close,
    Hide,
    Ask,
}

impl CloseAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Close => "close",
            Self::Hide => "hide",
            Self::Ask => "ask",
        }
    }
}

// What a close of the window does. `close_to_tray` is the setting and a tray icon that is there; a quit already under way, or one the user confirmed, always closes.
pub const fn close_request_action(
    quitting: bool,
    close_to_tray: bool,
    switch_busy: bool,
) -> CloseAction {
    if quitting {
        CloseAction::Close
    } else if close_to_tray {
        CloseAction::Hide
    } else if switch_busy {
        CloseAction::Ask
    } else {
        CloseAction::Close
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AfterPlay {
    // The job was started by a click on the Recent strip, not from the launch dialog.
    pub instant: bool,
    pub direct: bool,
    pub done: bool,
    pub hand_off: bool,
    pub tray_present: bool,
    pub close_to_tray: bool,
    pub hide_to_tray: bool,
}

impl AfterPlay {
    // The flags of a finished job; the caller says how it was started and what the tray and the settings are.
    pub fn of(
        job: &LaunchJob,
        instant: bool,
        tray_present: bool,
        close_to_tray: bool,
        hide_to_tray: bool,
    ) -> Self {
        Self {
            instant,
            direct: job.direct,
            done: job.state == ol_model::JobState::Done,
            hand_off: job.hand_off.is_some(),
            tray_present,
            close_to_tray,
            hide_to_tray,
        }
    }
}

// Only a one-click Play that went straight to the game and ended well hides the window: a switch, a failure or a launch that still needs the user keeps it where its progress is.
pub const fn hide_after_instant_play(after: AfterPlay) -> bool {
    after.instant
        && after.direct
        && after.done
        && !after.hand_off
        && after.tray_present
        && after.close_to_tray
        && after.hide_to_tray
}

pub const START_HIDDEN_FLAG: &str = "--start-hidden";

// Without the tray there would be no way back to a window that never showed.
pub fn start_hidden<S: AsRef<str>>(args: &[S], close_to_tray: bool) -> bool {
    close_to_tray && args.iter().any(|arg| arg.as_ref() == START_HIDDEN_FLAG)
}

pub const MIN_WINDOW: (f64, f64) = (960., 600.);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Placement {
    pub bounds: Option<Rect>,
    pub maximized: bool,
}

pub fn placement_from(raw: Option<&str>, work_areas: &[Rect]) -> Placement {
    let Some(stored) = raw
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|value| value.as_object().cloned())
    else {
        return Placement::default();
    };
    let number = |key: &str| stored.get(key).and_then(serde_json::Value::as_f64);
    let bounds = match (number("x"), number("y"), number("width"), number("height")) {
        (Some(x), Some(y), Some(width), Some(height)) => Some(Rect {
            x,
            y,
            width: width.max(MIN_WINDOW.0),
            height: height.max(MIN_WINDOW.1),
        }),
        _ => None,
    };
    let on_screen = bounds.filter(|bounds| display_for(bounds, work_areas).is_some());
    Placement {
        bounds: on_screen,
        maximized: stored.get("maximized") == Some(&serde_json::Value::Bool(true)),
    }
}

const GRAB: f64 = 64.;

pub fn display_for(bounds: &Rect, work_areas: &[Rect]) -> Option<usize> {
    let grabbed = |area: &Rect| {
        bounds.x < area.x + area.width - GRAB
            && bounds.x + bounds.width > area.x + GRAB
            && bounds.y >= area.y - 8.
            && bounds.y < area.y + area.height - GRAB
    };
    let across =
        |area: &Rect| (bounds.x + bounds.width).min(area.x + area.width) - bounds.x.max(area.x);
    // Of two displays the title bar spans, the one that holds more of it, as Windows counts a window to the display with most of it.
    work_areas
        .iter()
        .enumerate()
        .filter(|(_, area)| grabbed(area))
        .max_by(|(at, a), (other, b)| across(a).total_cmp(&across(b)).then(other.cmp(at)))
        .map(|(at, _)| at)
}

// `placement_from` keeps any size, as its recorded cases want; Windows cuts a window larger than the screen.
pub fn fit(bounds: Rect, area: &Rect) -> Rect {
    let width = bounds.width.min(area.width).max(MIN_WINDOW.0);
    // A width cut from the right can take the whole title bar off the display `display_for` chose.
    let x = if bounds.x + width > area.x + GRAB {
        bounds.x
    } else {
        area.x
    };
    Rect {
        x,
        width,
        height: bounds.height.min(area.height).max(MIN_WINDOW.1),
        ..bounds
    }
}

// The Electron app's own key order.
pub fn bounds_setting(x: i32, y: i32, width: i32, height: i32, maximized: bool) -> String {
    format!(r#"{{"x":{x},"y":{y},"width":{width},"height":{height},"maximized":{maximized}}}"#)
}

// The question before a quit stops a running switch. Its first button keeps the app running, and is the answer to Escape and to a close of the question.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuitQuestion {
    pub job_id: String,
    pub title: &'static str,
    pub message: String,
    pub detail: String,
    pub keep: &'static str,
    pub quit: &'static str,
}

pub fn quit_question(job: &LaunchJob) -> QuitQuestion {
    let launcher = job.launcher.display_name();
    QuitQuestion {
        job_id: job.id.clone(),
        title: "A switch is still running",
        message: format!(
            "One Launch is still switching to {}.",
            job.account.display_name
        ),
        detail: format!(
            "Quitting stops the switch. If {launcher} has not been started again yet, its files are put back first; after that {launcher} carries on signing in on its own. Left alone, the switch finishes once the launcher reports the sign-in."
        ),
        keep: "Keep running",
        quit: "Quit anyway",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuitAnswer {
    Keep,
    Quit,
}

#[derive(Clone)]
pub struct WindowLog(Log);

impl WindowLog {
    pub fn new(log: &Log) -> Self {
        Self(log.clone())
    }

    pub fn close_requested(&self, action: CloseAction) {
        self.0.debug(
            "window close requested",
            json!({ "action": action.as_str() }),
        );
    }

    pub fn hidden_to_tray(&self) {
        self.0.info("window hidden to the tray", ());
    }

    pub fn hidden_after_instant_launch(&self, job_id: &str) {
        self.0.info(
            "window hidden to the tray after an instant launch",
            json!({ "jobId": job_id }),
        );
    }

    pub fn shown_from_tray(&self) {
        self.0.info("window shown from the tray", ());
    }

    pub fn tray_added(&self) {
        self.0.info("tray icon added", ());
    }

    pub fn tray_removed(&self) {
        self.0.info("tray icon removed", ());
    }

    // The code of the failure and Windows' number for it, not Windows' own sentence.
    pub fn tray_unavailable(&self, code: &str, hresult: Option<i32>) {
        let error = match hresult {
            Some(hresult) => {
                json!({ "code": code, "hresult": format!("{:#010x}", hresult.cast_unsigned()) })
            }
            None => json!({ "code": code }),
        };
        self.0.warn(
            "tray icon unavailable; closing the window quits",
            json!({ "error": error }),
        );
    }

    pub fn quit_asked(&self, job_id: &str) {
        self.0.info(
            "quit prompt shown during a switch",
            json!({ "jobId": job_id }),
        );
    }

    pub fn quit_answered(&self, answer: QuitAnswer) {
        let answer = match answer {
            QuitAnswer::Keep => "keep",
            QuitAnswer::Quit => "quit",
        };
        self.0
            .info("quit prompt answered", json!({ "answer": answer }));
    }

    pub fn stopping_for_quit(&self) {
        self.0.warn(
            "stopping the running switch",
            json!({ "reason": "quit confirmed during a switch" }),
        );
    }

    pub fn fatal_during_startup(&self) {
        self.0.error("fatal during startup", ());
    }

    pub fn started_in_tray(&self) {
        self.0.info("started in the tray", ());
    }

    pub fn shown_for_second_start(&self) {
        self.0.info("window shown for a second start", ());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_cut_to_its_display_keeps_its_title_bar_on_that_display() {
        let area = Rect {
            x: 0.,
            y: 0.,
            width: 1920.,
            height: 1040.,
        };
        let wide = Rect {
            x: -3000.,
            y: 100.,
            width: 3200.,
            height: 800.,
        };
        assert_eq!(display_for(&wide, &[area]), Some(0));
        let fitted = fit(wide, &area);
        assert_eq!(display_for(&fitted, &[area]), Some(0));
        assert_eq!((fitted.x, fitted.y, fitted.width), (0., 100., 1920.));
        let kept = Rect { x: 100., ..wide };
        assert_eq!(fit(kept, &area).x, 100.);
    }
}
