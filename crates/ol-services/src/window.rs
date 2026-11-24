// The rules of the window that the settings decide: what a close does, when a one-click Play sends the window to the tray, when a start stays in the tray, and the question asked before quitting during a switch.

use ol_model::LaunchJob;

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
    // The job was started by a click on a card or on the Recent strip, not from the launch dialog.
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

// The question before a quit stops a running switch. Its first button keeps the app running, and is the answer to Escape and to a close of the question.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuitQuestion {
    pub title: &'static str,
    pub message: String,
    pub detail: String,
    pub keep: &'static str,
    pub quit: &'static str,
}

pub fn quit_question(job: &LaunchJob) -> QuitQuestion {
    let launcher = job.launcher.display_name();
    QuitQuestion {
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
