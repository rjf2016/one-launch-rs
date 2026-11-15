// The launcher's own log, %LOCALAPPDATA%\EpicGamesLauncher\Saved\Logs\EpicGamesLauncher.log, read for how a sign-in ended. The launcher leaves tokens out of it itself: a line taken here, such as "FAccountService::OnLoginComplete - 0 TRUE <account id>", holds an account id at most. FAccountService::SignOut is written at every normal exit and says nothing about the remembered session, so it is passed over.

use std::sync::LazyLock;

use regex::Regex;

use crate::js::{self, WHITESPACE_CLASS as WS};
use crate::steam::login_log::MAX_LOG_LINE_LENGTH;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogEvent {
    AutoSignIn,
    ManualSignIn,
    // `account_id` is empty when the line names none; `at` is the line's own time, in UTC.
    SignedIn {
        account_id: String,
        at: Option<String>,
    },
    SignInFailed {
        reason: String,
    },
    // The launcher has finished starting.
    Ready,
    // The launcher says it will start itself again after it exits.
    Restart,
    Exit,
}

static LOGIN_COMPLETE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"FAccountService::OnLoginComplete - [0-9]+ (TRUE|FALSE){WS}*([^\n\r\u{{2028}}\u{{2029}}]*)$"
    ))
    .expect("a fixed pattern")
});
static LINE_TIME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^\[([0-9]{4})\.([0-9]{2})\.([0-9]{2})-([0-9]{2})\.([0-9]{2})\.([0-9]{2}):([0-9]{3})\]",
    )
    .expect("a fixed pattern")
});
static TOKEN_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("[0-9a-fA-F]{32}").expect("a fixed pattern"));
static WHITESPACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!("{WS}+")).expect("a fixed pattern"));
static SESSION_REJECTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i-u)TOKEN_EXPIRED|refresh token|invalid_grant|token (is |was |has )?(invalid|expired|revoked)",
    )
    .expect("a fixed pattern")
});

// The time at the head of a line, [2026.09.13-00.09.05:354], as 2026-09-13T00:09:05.354Z.
pub fn line_time(line: &str) -> Option<String> {
    let found = LINE_TIME.captures(line)?;
    Some(format!(
        "{}-{}-{}T{}:{}:{}.{}Z",
        &found[1], &found[2], &found[3], &found[4], &found[5], &found[6], &found[7]
    ))
}

// Why a sign-in failed, with every run of 32 hex digits (a token's id) and everything from errorpath= on taken out.
pub fn mask_reason(raw: &str) -> String {
    let text = raw.strip_prefix("errormessage=").unwrap_or(raw);
    let kept = text.find("errorpath=").map_or(text, |cut| &text[..cut]);
    js::trim(&TOKEN_ID.replace_all(kept, "…")).to_owned()
}

fn is_account_id(text: &str) -> bool {
    text.len() == 32 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn parse_log_events(text: &str) -> Vec<LogEvent> {
    let mut events = Vec::new();
    for line in js::lines(text) {
        if line.len() > MAX_LOG_LINE_LENGTH && js::utf16_len(line) > MAX_LOG_LINE_LENGTH {
            continue;
        }
        if line.contains("FAccountService::AutoSignIn") {
            events.push(LogEvent::AutoSignIn);
        } else if line.contains("FAccountService::SignIn") {
            events.push(LogEvent::ManualSignIn);
        } else if let Some(found) = LOGIN_COMPLETE.captures(line) {
            if &found[1] == "TRUE" {
                let id = WHITESPACE
                    .split(js::trim(&found[2]))
                    .next()
                    .unwrap_or_default();
                events.push(LogEvent::SignedIn {
                    account_id: if is_account_id(id) {
                        id.to_lowercase()
                    } else {
                        String::new()
                    },
                    at: line_time(line),
                });
            } else {
                events.push(LogEvent::SignInFailed {
                    reason: mask_reason(&found[2]),
                });
            }
        } else if line.contains("LogInit: AllCheckComplete") {
            events.push(LogEvent::Ready);
        } else if line.contains("Application: Restarting") && line.contains("EpicGamesUpdater.exe")
        {
            events.push(LogEvent::Restart);
        } else if line.contains("LogCore: Engine exit requested")
            || line.starts_with("Log file closed")
        {
            events.push(LogEvent::Exit);
        }
    }
    events
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignInOutcome {
    SignedIn {
        account_id: String,
        at: Option<String>,
    },
    Failed {
        reason: String,
    },
}

// The last event that settles a sign-in, or None while the launcher is still at work or never tried.
pub fn last_sign_in(events: &[LogEvent]) -> Option<SignInOutcome> {
    events.iter().rev().find_map(|event| match event {
        LogEvent::SignedIn { account_id, at } => Some(SignInOutcome::SignedIn {
            account_id: account_id.clone(),
            at: at.clone(),
        }),
        LogEvent::SignInFailed { reason } => Some(SignInOutcome::Failed {
            reason: reason.clone(),
        }),
        _ => None,
    })
}

// The launcher refused the remembered session itself, as opposed to a failure of the service or the network that the same session outlives.
pub fn is_session_rejection(reason: &str) -> bool {
    SESSION_REJECTION.is_match(reason)
}

pub fn announced_restart(events: &[LogEvent]) -> bool {
    events.contains(&LogEvent::Restart)
}

pub fn startup_finished(events: &[LogEvent]) -> bool {
    events.contains(&LogEvent::Ready)
}
