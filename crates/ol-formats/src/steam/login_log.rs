// Steam's own sign-in state log, <SteamPath>\logs\steamui_login.txt: plain text from the client's window, with no token or password in it. A line of interest ends "SetLoginState: WaitingForCredentials - Account Logon Denied".

use std::sync::LazyLock;

use regex::Regex;

use crate::js::{self, WHITESPACE_CLASS as WS};

// No launcher writes a log line near this long, so a longer one is passed over and one endless line cannot hold up a sign-in wait.
pub const MAX_LOG_LINE_LENGTH: usize = 16 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoginStateEvent {
    pub state: String,
    pub reason: String,
}

static STATE_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"SetLoginState:{WS}*([A-Za-z]+){WS}*-{WS}*([^\n\r\u{{2028}}\u{{2029}}]*?){WS}*$"
    ))
    .expect("a fixed pattern")
});

pub fn parse_login_states(text: &str) -> Vec<LoginStateEvent> {
    js::lines(text)
        .filter(|line| {
            line.len() <= MAX_LOG_LINE_LENGTH || js::utf16_len(line) <= MAX_LOG_LINE_LENGTH
        })
        .filter_map(|line| STATE_LINE.captures(line))
        .map(|found| LoginStateEvent {
            state: found[1].to_owned(),
            reason: found[2].to_owned(),
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignInCause {
    GuardEmail,
    GuardMobile,
    Password,
    Expired,
    RateLimit,
    None,
    Other,
}

impl SignInCause {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GuardEmail => "guard-email",
            Self::GuardMobile => "guard-mobile",
            Self::Password => "password",
            Self::Expired => "expired",
            Self::RateLimit => "rate-limit",
            Self::None => "none",
            Self::Other => "other",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignInOutcome {
    Success,
    Credentials { reason: String, cause: SignInCause },
}

// What Steam's wording on a WaitingForCredentials line asks of the user.
pub fn classify_reason(reason: &str) -> SignInCause {
    let reason = js::trim(reason).to_lowercase();
    let says = |words: &[&str]| words.iter().any(|word| reason.contains(word));
    if reason.is_empty() || reason == "ok" {
        SignInCause::None
    } else if says(&["two factor", "twofactor", "mobile"]) {
        SignInCause::GuardMobile
    } else if says(&["logon denied", "login denied", "email"]) {
        SignInCause::GuardEmail
    } else if says(&["password"]) {
        SignInCause::Password
    } else if says(&["expired"]) {
        SignInCause::Expired
    } else if says(&["rate limit", "throttle"]) {
        SignInCause::RateLimit
    } else {
        SignInCause::Other
    }
}

// The last state that settles anything, or None while Steam is still at work.
pub fn latest_outcome(events: &[LoginStateEvent]) -> Option<SignInOutcome> {
    events
        .iter()
        .rev()
        .find_map(|event| match event.state.as_str() {
            "Success" => Some(SignInOutcome::Success),
            "WaitingForCredentials" => Some(SignInOutcome::Credentials {
                reason: event.reason.clone(),
                cause: classify_reason(&event.reason),
            }),
            _ => None,
        })
}
