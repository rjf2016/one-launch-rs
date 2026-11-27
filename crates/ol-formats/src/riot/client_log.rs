// The Riot Client's own log, %LOCALAPPDATA%\Riot Games\Riot Client\Logs\Riot Client Logs\<local time>_<pid>_Riot Client.log, one file for each run. The log never names the account, and its telemetry lines hold cookie and token values: those lines are passed over, and an event holds only what a strict pattern matched (the name of a state, a product id, a status word, an error code), never a line.

use std::sync::LazyLock;

use regex::Regex;

use crate::js::{self, WHITESPACE_CLASS as WS};
use crate::steam::login_log::MAX_LOG_LINE_LENGTH;

// A second RiotClientServices.exe, such as a shortcut used while the client runs, writes a few lines, hands its command line to the running client and exits. Its log is not a run.
pub const STUB_LOG_MAX_BYTES: u64 = 64 * 1024;
pub const STUB_LOG_MARK: &str = "ProcessSingleton: Did not acquire process singleton";
// How long a reason the client gives for not launching may be, in UTF-16 units.
const MAX_REASON_LENGTH: usize = 80;
pub const NOT_UP_TO_DATE: &str = "Product is not up to date";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SignedInHow {
    Restored,
    Manual,
    Unknown,
}

#[derive(Clone, PartialEq, Eq)]
pub enum LogEvent {
    Started {
        at: Option<String>,
        build: Option<String>,
    },
    // The client was started with --launch-product, which it repeats under "Command Line Arguments".
    LaunchRequest {
        product: String,
        patchline: Option<String>,
    },
    // True at a start with the arguments, false once the launch is handed over.
    ProductContext {
        product: String,
        patchline: String,
        auto_launch: bool,
    },
    // A step of the launch pipeline. WaitForSessionExit means the game runs.
    Lifecycle {
        from: String,
        to: String,
    },
    // The game is being started, by the client itself or by Play.
    LaunchStarting {
        product: String,
        patchline: String,
    },
    // The client shows the game's page in place of starting the game, and says why.
    LaunchDeclined {
        reason: String,
    },
    PatchState {
        product: String,
        patchline: String,
        status: String,
        launchable: Option<bool>,
    },
    // A state of the sign-in. PendingProductContext is signed in; PendingLoginStrategy is the login page.
    PslState {
        state: String,
    },
    // The user is signing in by hand.
    ManualSignIn,
    // Builds up to 116 said so in words.
    SignedIn {
        how: SignedInHow,
    },
    // The saved sign-in was refused.
    SessionRejected {
        detail: String,
    },
    // Another failure of the sign-in: the service or the network, which the saved sign-in may outlast.
    AuthError {
        error: String,
    },
    // Builds up to 116: started with no saved sign-in.
    NoSession,
    SignedOut,
    // The session file was written.
    SettingsWritten {
        key: String,
    },
    GameSessionEnded {
        product: String,
        patchline: String,
    },
    Quit,
    // The removal of the lock file, the client's last line of a clean exit.
    Exit,
}

fn pattern(text: &str) -> Regex {
    Regex::new(text).expect("a fixed pattern")
}

// A line as far as it goes without a line end of any kind.
const REST: &str = r"[^\n\r\u{2028}\u{2029}]*";
const ID: &str = "[a-z0-9_]+";

static NOT_WS: LazyLock<String> = LazyLock::new(|| WS.replacen('[', "[^", 1));

static CLIENT_LOG_FILE: LazyLock<Regex> = LazyLock::new(|| {
    pattern(
        r"^[0-9]{4}-[0-9]{2}-[0-9]{2}(?i-u:T)[0-9]{2}-[0-9]{2}-[0-9]{2}(?:\.[0-9]+)?_[0-9]+_(?i-u:Riot Client\.log)$",
    )
});
static TELEMETRY: LazyLock<Regex> = LazyLock::new(|| pattern("(?i-u:telemetry)"));
static LINE: LazyLock<Regex> = LazyLock::new(|| {
    pattern(&format!(
        r"^[0-9]{{6}}\.[0-9]{{3}}\|{WS}*([A-Z]+)\|{WS}?({REST})$"
    ))
});
static LAUNCH_ARG: LazyLock<Regex> = LazyLock::new(|| {
    pattern(&format!(
        r"^{WS}+(?i-u:--launch-(product|patchline)=({ID})){WS}*$"
    ))
});
static STARTED: LazyLock<Regex> = LazyLock::new(|| {
    pattern(
        r"^Logging started at ([0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]+)?)",
    )
});
static VERSION: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"^Application Version: ([0-9]+(?:\.[0-9]+){0,3})(?-u:\b)"));
static PSL_STATE: LazyLock<Regex> = LazyLock::new(|| {
    pattern(&format!(
        "riot-client-lifecycle: PSLProvider: PSL state changed to ([A-Za-z]+){WS}*$"
    ))
});
static PRODUCT_CONTEXT: LazyLock<Regex> = LazyLock::new(|| {
    pattern(&format!(
        r"(?i-u:riot-client-lifecycle: Setting product context to ({ID})\.({ID}), willAutoLaunch=(true|false))"
    ))
});
static LIFECYCLE: LazyLock<Regex> = LazyLock::new(|| {
    pattern(
        r"riot-client-lifecycle: Context #[0-9]+: Middleware transition \(([A-Za-z]+)\) -> \(([A-Za-z]+)\)",
    )
});
static LAUNCH_STARTING: LazyLock<Regex> = LazyLock::new(|| {
    pattern(&format!(
        "(?i-u:product-launcher: Launch starting: productId='({ID})' patchlineId='({ID})')"
    ))
});
// What the client says for a request handed to it while it runs.
static LAUNCH_DECLINED: LazyLock<Regex> = LazyLock::new(|| {
    pattern(
        r"riot-client-lifecycle: Client is already running\. Explicitly request focus instead of launching \(([^()]+)\)",
    )
});
// The same verdict at a start with the arguments. At a bare start its reason comes before any product context, and so never counts against one.
static BOOT_LAUNCH_VERDICT: LazyLock<Regex> = LazyLock::new(|| {
    pattern(r"riot-client-lifecycle: Attempting to launch default product with reason \(([^()]+)\)")
});
static PATCH_STATUS: LazyLock<Regex> = LazyLock::new(|| {
    pattern(&format!(
        "(?i-u:patch-proxy: HandleProductPatchlineStatusUpdate: productId: ({ID}), patchlineId: ({ID}), status: ([a-z]+))"
    ))
});
static PATCH_STATE_116: LazyLock<Regex> = LazyLock::new(|| {
    pattern(&format!(
        r"(?i-u:patch-proxy: ProductPatchline: \w+ state for ({ID})\.({ID}) changed from [a-z]+ to ([a-z]+), launchable: ([01]))"
    ))
});
static RSO_ERROR: LazyLock<Regex> = LazyLock::new(|| pattern("rso-auth: error '([a-z_]+)'"));
static INVALID_GRANT: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"Failed to create RSO authorization \(([0-9]{3})\) ([a-z_]+)"));
static SETTINGS_WRITTEN: LazyLock<Regex> = LazyLock::new(|| {
    pattern(&format!(
        "private-settings: Updated private settings for user {}+ with key ([A-Za-z0-9_.-]+)",
        NOT_WS.as_str()
    ))
});
static COOKIES_PERSISTED: LazyLock<Regex> = LazyLock::new(|| {
    pattern(&format!(
        "riot-login: Persisting ([0-9]+) RSO auth cookies: {REST} and ([0-9]+) RSO auth scopes"
    ))
});
static GAME_SESSION_ENDED: LazyLock<Regex> = LazyLock::new(|| {
    pattern(&format!(
        "(?i-u:product-launcher: Session for ({ID}) on patchline ({ID}) has ended)"
    ))
});

// Whether a file in the log folder is a run's log by its name, which holds the local time the run started.
pub fn is_client_log_file(name: &str) -> bool {
    CLIENT_LOG_FILE.is_match(name)
}

// The first reason in parentheses that is short enough to be one.
fn reason_in(found: &Regex, body: &str) -> Option<String> {
    found
        .captures_iter(body)
        .map(|found| found[1].to_owned())
        .find(|reason| js::utf16_len(reason) <= MAX_REASON_LENGTH)
}

fn settles_a_sign_in(event: &LogEvent) -> bool {
    matches!(
        event,
        LogEvent::SignedIn { .. }
            | LogEvent::SessionRejected { .. }
            | LogEvent::NoSession
            | LogEvent::SignedOut
    )
}

// Events the client logs several times in a row, a state or the device cookie written again, count once.
fn counts_once(event: &LogEvent) -> bool {
    matches!(
        event,
        LogEvent::LaunchRequest { .. }
            | LogEvent::ProductContext { .. }
            | LogEvent::PatchState { .. }
            | LogEvent::PslState { .. }
            | LogEvent::SignedOut
            | LogEvent::SettingsWritten { .. }
            | LogEvent::Quit
    )
}

fn push(events: &mut Vec<LogEvent>, event: LogEvent) {
    if counts_once(&event) && events.last() == Some(&event) {
        return;
    }
    events.push(event);
}

fn event_of(level: &str, body: &str, events: &mut Vec<LogEvent>) {
    let lower = |text: &str| text.to_ascii_lowercase();
    if let Some(found) = STARTED.captures(body) {
        push(
            events,
            LogEvent::Started {
                at: Some(found[1].to_owned()),
                build: None,
            },
        );
    } else if let Some(found) = VERSION.captures(body) {
        let version = Some(found[1].to_owned());
        match events.last_mut() {
            Some(LogEvent::Started { build, .. }) if build.is_none() => *build = version,
            _ => events.push(LogEvent::Started {
                at: None,
                build: version,
            }),
        }
    } else if let Some(found) = PSL_STATE.captures(body) {
        push(
            events,
            LogEvent::PslState {
                state: found[1].to_owned(),
            },
        );
    } else if body.contains("rso-auth: Using direct token exchange") {
        push(events, LogEvent::ManualSignIn);
    } else if body.contains("riot-client-auth: Deleted current RSO authorization")
        || body.contains("rso-auth: Successfully deleted RSO authorization")
    {
        push(events, LogEvent::SignedOut);
    } else if let (Some(found), "ERROR") = (RSO_ERROR.captures(body), level) {
        let refused = INVALID_GRANT
            .captures(body)
            .filter(|grant| &grant[2] == "invalid_grant");
        push(
            events,
            match refused {
                Some(grant) => LogEvent::SessionRejected {
                    detail: format!(
                        "the client refused the saved sign-in ({} invalid_grant)",
                        &grant[1]
                    ),
                },
                None => LogEvent::AuthError {
                    error: found[1].to_owned(),
                },
            },
        );
    } else if let Some(found) = SETTINGS_WRITTEN.captures(body) {
        push(
            events,
            LogEvent::SettingsWritten {
                key: found[1].to_owned(),
            },
        );
    } else if let Some(found) = PRODUCT_CONTEXT.captures(body) {
        push(
            events,
            LogEvent::ProductContext {
                product: lower(&found[1]),
                patchline: lower(&found[2]),
                auto_launch: found[3].eq_ignore_ascii_case("true"),
            },
        );
    } else if let Some(found) = LIFECYCLE.captures(body) {
        push(
            events,
            LogEvent::Lifecycle {
                from: found[1].to_owned(),
                to: found[2].to_owned(),
            },
        );
    } else if let Some(found) = LAUNCH_STARTING.captures(body) {
        push(
            events,
            LogEvent::LaunchStarting {
                product: lower(&found[1]),
                patchline: lower(&found[2]),
            },
        );
    } else if let Some(reason) =
        reason_in(&LAUNCH_DECLINED, body).or_else(|| reason_in(&BOOT_LAUNCH_VERDICT, body))
    {
        push(events, LogEvent::LaunchDeclined { reason });
    } else if let Some(found) = PATCH_STATUS.captures(body) {
        push(
            events,
            LogEvent::PatchState {
                product: lower(&found[1]),
                patchline: lower(&found[2]),
                status: found[3].to_owned(),
                launchable: None,
            },
        );
    } else if let Some(found) = PATCH_STATE_116.captures(body) {
        push(
            events,
            LogEvent::PatchState {
                product: lower(&found[1]),
                patchline: lower(&found[2]),
                status: found[3].to_owned(),
                launchable: Some(&found[4] == "1"),
            },
        );
    } else if let Some(found) = GAME_SESSION_ENDED.captures(body) {
        push(
            events,
            LogEvent::GameSessionEnded {
                product: lower(&found[1]),
                patchline: lower(&found[2]),
            },
        );
    } else if body.contains("riot-login: Successfully logged in with persistent session state") {
        push(
            events,
            LogEvent::SignedIn {
                how: SignedInHow::Restored,
            },
        );
    } else if body.contains("riot-login: Did not restore persistent session state: Failed") {
        push(
            events,
            LogEvent::SessionRejected {
                detail: "the client could not authenticate with the saved sign-in".to_owned(),
            },
        );
    } else if body
        .contains("riot-login: Did not restore persistent session state: No stored session exists")
    {
        push(events, LogEvent::NoSession);
    } else if let Some(found) = COOKIES_PERSISTED.captures(body) {
        // Builds up to 116 kept the session in cookies: a write with scopes is a finished sign-in, unless one was logged already, and one without is only the device cookie.
        let with_scopes = found[2].bytes().any(|digit| digit != b'0');
        let last = events.iter().rev().find(|event| settles_a_sign_in(event));
        if with_scopes && !matches!(last, Some(LogEvent::SignedIn { .. })) {
            push(
                events,
                LogEvent::SignedIn {
                    how: SignedInHow::Unknown,
                },
            );
        }
        push(
            events,
            LogEvent::SettingsWritten {
                key: "riot-login".to_owned(),
            },
        );
    } else if body.contains("riot-client-lifecycle: Attempting to quit") {
        push(events, LogEvent::Quit);
    } else if body.contains("ProcessSingleton: Removal of LockFile complete") {
        push(events, LogEvent::Exit);
    }
}

// The launch arguments read off the lines before, if they named a product.
fn flush(events: &mut Vec<LogEvent>, asked: &mut (Option<String>, Option<String>)) {
    if let (Some(product), patchline) = std::mem::take(asked) {
        push(events, LogEvent::LaunchRequest { product, patchline });
    }
}

pub fn parse_log_events(text: &str) -> Vec<LogEvent> {
    let mut events = Vec::new();
    let mut asked: (Option<String>, Option<String>) = (None, None);
    for line in js::lines(text) {
        let too_long =
            line.len() > MAX_LOG_LINE_LENGTH && js::utf16_len(line) > MAX_LOG_LINE_LENGTH;
        if too_long || TELEMETRY.is_match(line) {
            continue;
        }
        let Some(prefixed) = LINE.captures(line) else {
            // The command line is repeated one argument to an indented line.
            if let Some(argument) = LAUNCH_ARG.captures(line) {
                let value = Some(argument[2].to_ascii_lowercase());
                if argument[1].eq_ignore_ascii_case("product") {
                    asked.0 = value;
                } else {
                    asked.1 = value;
                }
            }
            continue;
        };
        flush(&mut events, &mut asked);
        event_of(&prefixed[1], &prefixed[2], &mut events);
    }
    flush(&mut events, &mut asked);
    events
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FailReason {
    // The saved sign-in was refused. Only this marks a stored copy stale.
    Rejected,
    SignedOut,
    NoSession,
    Error,
}

#[derive(Clone, PartialEq, Eq)]
pub enum SignInOutcome {
    SignedIn { how: SignedInHow },
    Failed { reason: FailReason, detail: String },
}

impl SignInOutcome {
    pub const fn is_signed_in(&self) -> bool {
        matches!(self, Self::SignedIn { .. })
    }
}

fn is_state(event: &LogEvent, name: &str) -> bool {
    matches!(event, LogEvent::PslState { state } if state == name)
}

// The events of the sign-in that ends at `end`: back to the PendingInitialization before it, of which there is one for each start and each sign-out.
fn attempt(events: &[LogEvent], end: usize) -> &[LogEvent] {
    let start = events[..end]
        .iter()
        .rposition(|event| is_state(event, "PendingInitialization"))
        .map_or(0, |at| at + 1);
    &events[start..end]
}

fn login_page_reason(events: &[LogEvent], end: usize) -> SignInOutcome {
    let (mut rejected, mut error) = (None, None);
    let (mut signed_out, mut restored) = (false, false);
    for event in attempt(events, end) {
        match event {
            LogEvent::SessionRejected { detail } => rejected = Some(detail.clone()),
            LogEvent::AuthError { error: code } => error = Some(code.clone()),
            LogEvent::SignedOut => signed_out = true,
            _ if is_state(event, "PendingCreateAuthorization") => signed_out = true,
            _ if is_state(event, "RestoreAuthorization") => restored = true,
            _ => {}
        }
    }
    let failed = |reason, detail: String| SignInOutcome::Failed { reason, detail };
    if let Some(detail) = rejected {
        failed(FailReason::Rejected, detail)
    } else if let Some(error) = error {
        failed(
            FailReason::Error,
            format!("the sign-in did not complete ({error})"),
        )
    } else if signed_out {
        failed(FailReason::SignedOut, "signed out".to_owned())
    } else if restored {
        failed(
            FailReason::Error,
            "the saved sign-in was not restored".to_owned(),
        )
    } else {
        failed(FailReason::NoSession, "no saved sign-in".to_owned())
    }
}

fn signed_in_how(events: &[LogEvent], end: usize) -> SignedInHow {
    let mut how = SignedInHow::Unknown;
    for event in attempt(events, end) {
        if *event == LogEvent::ManualSignIn || is_state(event, "LoginTokenExchange") {
            return SignedInHow::Manual;
        }
        if is_state(event, "RestoreAuthorization") {
            how = SignedInHow::Restored;
        }
    }
    how
}

// The last verdict the log gives on a sign-in, or None while the client is still at work or has not tried. The client ends on PendingProductContext, signed in, or on PendingLoginStrategy, its login page; why the page shows comes from the lines before it. A sign-out after a sign-in undoes it.
pub fn last_sign_in(events: &[LogEvent]) -> Option<SignInOutcome> {
    let failed = |reason, detail: &str| SignInOutcome::Failed {
        reason,
        detail: detail.to_owned(),
    };
    events
        .iter()
        .enumerate()
        .rev()
        .find_map(|(at, event)| match event {
            // PendingProductContextInitialization is the same point when the client was started with a product.
            LogEvent::PslState { state }
                if state == "PendingProductContext"
                    || state == "PendingProductContextInitialization" =>
            {
                Some(SignInOutcome::SignedIn {
                    how: signed_in_how(events, at),
                })
            }
            LogEvent::PslState { state } if state == "PendingLoginStrategy" => {
                Some(login_page_reason(events, at))
            }
            LogEvent::SignedIn { how } => Some(SignInOutcome::SignedIn { how: *how }),
            LogEvent::SessionRejected { detail } => Some(failed(FailReason::Rejected, detail)),
            LogEvent::NoSession => Some(failed(FailReason::NoSession, "no saved sign-in")),
            LogEvent::SignedOut => Some(failed(FailReason::SignedOut, "signed out")),
            _ => None,
        })
}

// The run has reached a verdict on the sign-in or the page of the terms, so a client that started itself again is far enough along to be closed again.
pub fn startup_finished(events: &[LogEvent]) -> bool {
    last_sign_in(events).is_some()
        || events
            .iter()
            .any(|event| is_state(event, "PendingEulaAcceptance"))
}

pub fn exited_cleanly(events: &[LogEvent]) -> bool {
    events.contains(&LogEvent::Exit)
}

#[derive(Clone, PartialEq, Eq)]
pub struct PatchState {
    pub status: String,
    pub launchable: Option<bool>,
}

// The latest patch state of each `<product>.<patchline>`.
pub fn patch_states(events: &[LogEvent]) -> Vec<(String, PatchState)> {
    let mut states: Vec<(String, PatchState)> = Vec::new();
    for event in events {
        let LogEvent::PatchState {
            product,
            patchline,
            status,
            launchable,
        } = event
        else {
            continue;
        };
        let id = format!("{product}.{patchline}");
        let state = PatchState {
            status: status.clone(),
            launchable: *launchable,
        };
        match states.iter_mut().find(|(known, _)| *known == id) {
            Some((_, known)) => *known = state,
            None => states.push((id, state)),
        }
    }
    states
}

// The client's last word on the product is that it has to be patched before it runs: its Play button reads Update, and a launch request only opens the product's page.
pub fn product_needs_update(state: &PatchState) -> bool {
    state.status.eq_ignore_ascii_case("outofdate") || state.launchable == Some(false)
}

pub fn products_needing_update(events: &[LogEvent]) -> Vec<String> {
    patch_states(events)
        .into_iter()
        .filter(|(_, state)| product_needs_update(state))
        .map(|(id, _)| id)
        .collect()
}

// The product a line names: a context set, or a launch begun. The steps of the pipeline and the refusals name none, so they belong to the product whose lines were the last to name one.
fn named_product(event: &LogEvent) -> Option<(&str, &str)> {
    match event {
        LogEvent::ProductContext {
            product, patchline, ..
        }
        | LogEvent::LaunchStarting { product, patchline } => Some((product, patchline)),
        _ => None,
    }
}

// Every event with whether the product's own lines were the last to name a product before it.
fn while_named<'e>(
    events: &'e [LogEvent],
    product: &'e str,
    patchline: &'e str,
) -> impl Iterator<Item = (&'e LogEvent, bool)> {
    let mut named = false;
    events.iter().map(move |event| {
        if let Some(names) = named_product(event) {
            named = names == (product, patchline);
            (event, false)
        } else {
            (event, named)
        }
    })
}

// The client acted on the launch request: it set the product's context or began the launch. The command line alone says only that it was asked.
pub fn launch_accepted(events: &[LogEvent], product: &str, patchline: &str) -> bool {
    events
        .iter()
        .any(|event| named_product(event) == Some((product, patchline)))
}

// The reason the client gave for showing the game's page in place of starting the game, if it did.
pub fn launch_declined(events: &[LogEvent], product: &str, patchline: &str) -> Option<String> {
    while_named(events, product, patchline)
        .filter_map(|(event, named)| match event {
            LogEvent::LaunchDeclined { reason } if named => Some(reason.clone()),
            _ => None,
        })
        .last()
}

// The game's page is up with its Play button ready: after the product's own lines, with no other product's since, the pipeline reached WaitForLaunch or the client said why it is not launching. "Product is not up to date" is no such reason: the client is still patching the game or something it needs, and is ready only later. The client's last word of these counts, so that refusal after the page takes it back.
pub fn launch_page_shown(events: &[LogEvent], product: &str, patchline: &str) -> bool {
    while_named(events, product, patchline)
        .filter_map(|(event, named)| match event {
            LogEvent::Lifecycle { to, .. } if named && to == "WaitForLaunch" => Some(true),
            LogEvent::LaunchDeclined { reason } if named => Some(reason != NOT_UP_TO_DATE),
            _ => None,
        })
        .last()
        .unwrap_or(false)
}

// The client started the product's game: the pipeline reached WaitForSessionExit after the product's own lines, with no other product's since, or the game ran to its end.
pub fn game_started(events: &[LogEvent], product: &str, patchline: &str) -> bool {
    while_named(events, product, patchline).any(|(event, named)| match event {
        LogEvent::Lifecycle { to, .. } => named && to == "WaitForSessionExit",
        LogEvent::GameSessionEnded {
            product: ended,
            patchline: on,
        } => ended == product && on == patchline,
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(at: u32, level: &str, text: &str) -> String {
        format!("{at:06}.000| {level:>6}| {text}")
    }

    fn psl(at: u32, state: &str) -> String {
        line(
            at,
            "ALWAYS",
            &format!("riot-client-lifecycle: PSLProvider: PSL state changed to {state}"),
        )
    }

    fn events(lines: &[String]) -> Vec<LogEvent> {
        parse_log_events(&lines.join("\r\n"))
    }

    fn start() -> Vec<String> {
        vec![
            psl(1, "PendingInitialization"),
            psl(2, "PendingClientId"),
            psl(3, "PendingPersistenceInitialization"),
        ]
    }

    fn outcome(lines: &[String]) -> Option<SignInOutcome> {
        last_sign_in(&events(lines))
    }

    fn failed(reason: FailReason, detail: &str) -> Option<SignInOutcome> {
        Some(SignInOutcome::Failed {
            reason,
            detail: detail.to_owned(),
        })
    }

    #[test]
    fn the_end_state_of_the_sign_in_decides_and_the_lines_before_it_say_why() {
        let mut restored = start();
        assert!(outcome(&restored).is_none());
        restored.extend([psl(4, "RestoreAuthorization"), psl(5, "PendingEulaData")]);
        assert!(outcome(&restored).is_none());
        assert!(!startup_finished(&events(&restored)));
        restored.push(psl(6, "PendingProductContext"));
        let signed_in = |how| Some(SignInOutcome::SignedIn { how });
        assert!(outcome(&restored) == signed_in(SignedInHow::Restored));
        assert!(startup_finished(&events(&restored)));

        let mut with_product = start();
        with_product.extend([
            psl(4, "RestoreAuthorization"),
            psl(5, "PendingProductContextInitialization"),
        ]);
        assert!(outcome(&with_product) == signed_in(SignedInHow::Restored));

        let mut login_page = start();
        login_page.push(psl(4, "PendingLoginStrategy"));
        assert!(outcome(&login_page) == failed(FailReason::NoSession, "no saved sign-in"));

        let mut by_hand = login_page.clone();
        by_hand.extend([
            line(5, "OKAY", "SDK: rso-auth: Using direct token exchange"),
            psl(6, "LoginTokenExchange"),
            psl(7, "PendingProductContext"),
        ]);
        assert!(outcome(&by_hand) == signed_in(SignedInHow::Manual));

        let mut refused = start();
        refused.extend([
            line(4, "ERROR", "SDK: rso-auth: error 'authorization_creation_failure_request_error': in plugin library function HandleAuthorizationCreate: Failed to create RSO authorization (401) invalid_grant:"),
            psl(5, "RestoreAuthorization"),
            psl(6, "PendingLoginStrategy"),
        ]);
        assert!(
            outcome(&refused)
                == failed(
                    FailReason::Rejected,
                    "the client refused the saved sign-in (401 invalid_grant)"
                )
        );

        let mut service = start();
        service.extend([
            line(4, "ERROR", "SDK: rso-auth: error 'request_timeout': in plugin library function HandleAuthorizationCreate: request failed"),
            psl(5, "RestoreAuthorization"),
            psl(6, "PendingLoginStrategy"),
        ]);
        assert!(
            outcome(&service)
                == failed(
                    FailReason::Error,
                    "the sign-in did not complete (request_timeout)"
                )
        );

        let mut signed_out = restored.clone();
        signed_out.extend([
            line(
                7,
                "OKAY",
                "SDK: riot-client-auth: Deleted current RSO authorization.",
            ),
            psl(8, "PendingInitialization"),
            psl(9, "PendingCreateAuthorization"),
            psl(10, "PendingLoginStrategy"),
        ]);
        assert!(outcome(&signed_out) == failed(FailReason::SignedOut, "signed out"));
    }

    #[test]
    fn a_telemetry_line_and_a_line_too_long_give_no_event() {
        let secret = "placeholder-cookie-value";
        let lines = [
            line(
                1,
                "OKAY",
                &format!(
                    "SDK: Telemetry: riot-client-lifecycle: PSLProvider: PSL state changed to {secret}"
                ),
            ),
            line(
                2,
                "OKAY",
                &format!(
                    "riot-client-lifecycle: PSLProvider: PSL state changed to {}",
                    "a".repeat(MAX_LOG_LINE_LENGTH)
                ),
            ),
            psl(3, "PendingClientId"),
            psl(4, "PendingClientId"),
        ];
        let found = events(&lines);
        assert_eq!(found.len(), 1);
        assert!(is_state(&found[0], "PendingClientId"));
    }

    #[test]
    fn a_page_and_a_start_count_for_the_product_whose_lines_named_one_last() {
        let context = |at, id: &str, auto: bool| {
            line(
                at,
                "ALWAYS",
                &format!(
                    "riot-client-lifecycle: Setting product context to {id}, willAutoLaunch={auto}"
                ),
            )
        };
        let step = |at, from: &str, to: &str| {
            line(
                at,
                "ALWAYS",
                &format!(
                    "riot-client-lifecycle: Context #1: Middleware transition ({from}) -> ({to})"
                ),
            )
        };
        let declined = line(
            5,
            "OKAY",
            "riot-client-lifecycle: Client is already running. Explicitly request focus instead of launching (Auto-launch is disabled)",
        );
        let mut lines = vec![
            line(1, "ALWAYS", "Command Line Arguments (Riot Client App):"),
            "    --launch-product=valorant".to_owned(),
            "    --launch-patchline=live".to_owned(),
            line(2, "OKAY", "ProcessSingleton: Acquired process singleton"),
        ];
        let asked = events(&lines);
        assert!(
            asked
                == [LogEvent::LaunchRequest {
                    product: "valorant".to_owned(),
                    patchline: Some("live".to_owned()),
                }]
        );
        assert!(!launch_accepted(&asked, "valorant", "live"));

        lines.extend([
            context(3, "valorant.live", false),
            step(4, "RestartRequired", "WaitForLaunch"),
        ]);
        let page = events(&lines);
        assert!(launch_accepted(&page, "valorant", "live"));
        assert!(launch_page_shown(&page, "valorant", "live"));
        assert!(!launch_page_shown(&page, "valorant", "pbe"));
        assert!(!game_started(&page, "valorant", "live"));
        assert_eq!(launch_declined(&page, "valorant", "live"), None);

        lines.push(declined);
        assert_eq!(
            launch_declined(&events(&lines), "valorant", "live").as_deref(),
            Some("Auto-launch is disabled")
        );

        let mut played = lines.clone();
        played.extend([
            line(
                6,
                "OKAY",
                "product-launcher: Launch starting: productId='valorant' patchlineId='live'",
            ),
            step(7, "WaitForLaunch", "WaitForSessionExit"),
        ]);
        assert!(game_started(&events(&played), "valorant", "live"));

        // Another product chosen in the client takes the steps that follow.
        let mut other = lines.clone();
        other.extend([
            context(6, "league_of_legends.live", false),
            step(7, "WaitForLaunch", "WaitForSessionExit"),
        ]);
        let other = events(&other);
        assert!(!game_started(&other, "valorant", "live"));
        assert!(game_started(&other, "league_of_legends", "live"));
    }

    #[test]
    fn a_refusal_because_the_product_is_not_up_to_date_does_not_show_the_page() {
        let step = |at, from: &str, to: &str| {
            line(
                at,
                "ALWAYS",
                &format!(
                    "riot-client-lifecycle: Context #1: Middleware transition ({from}) -> ({to})"
                ),
            )
        };
        let running = |at, reason: &str| {
            line(
                at,
                "OKAY",
                &format!(
                    "riot-client-lifecycle: Client is already running. Explicitly request focus instead of launching ({reason})"
                ),
            )
        };
        let mut lines = vec![
            line(
                1,
                "ALWAYS",
                "riot-client-lifecycle: Setting product context to valorant.live, willAutoLaunch=false",
            ),
            step(2, "AppRepairAndUpdate", "PatchStatus"),
            running(3, NOT_UP_TO_DATE),
        ];
        let updating = events(&lines);
        assert!(!launch_page_shown(&updating, "valorant", "live"));
        assert_eq!(
            launch_declined(&updating, "valorant", "live").as_deref(),
            Some(NOT_UP_TO_DATE)
        );

        let at_boot = [
            lines[0].clone(),
            line(
                2,
                "OKAY",
                "riot-client-lifecycle: Attempting to launch default product with reason (Product is not up to date)",
            ),
        ];
        assert!(!launch_page_shown(&events(&at_boot), "valorant", "live"));

        let mut ready = lines.clone();
        ready.push(step(4, "RestartRequired", "WaitForLaunch"));
        assert!(launch_page_shown(&events(&ready), "valorant", "live"));

        lines.push(running(4, "Auto-launch is disabled"));
        assert!(launch_page_shown(&events(&lines), "valorant", "live"));
    }

    fn context(at: u32, id: &str) -> String {
        line(
            at,
            "ALWAYS",
            &format!(
                "riot-client-lifecycle: Setting product context to {id}, willAutoLaunch=false"
            ),
        )
    }

    fn step(at: u32, from: &str, to: &str) -> String {
        line(
            at,
            "ALWAYS",
            &format!("riot-client-lifecycle: Context #1: Middleware transition ({from}) -> ({to})"),
        )
    }

    fn running(at: u32, reason: &str) -> String {
        line(
            at,
            "OKAY",
            &format!(
                "riot-client-lifecycle: Client is already running. Explicitly request focus instead of launching ({reason})"
            ),
        )
    }

    #[test]
    fn a_page_shown_and_then_refused_as_not_up_to_date_is_taken_back_until_it_is_shown_again() {
        let updated = [
            context(1, "valorant.live"),
            running(2, NOT_UP_TO_DATE),
            running(3, "Auto-launch is disabled"),
        ];
        assert!(launch_page_shown(&events(&updated), "valorant", "live"));

        let mut lines = vec![
            context(1, "valorant.live"),
            step(2, "RestartRequired", "WaitForLaunch"),
            running(3, "Auto-launch is disabled"),
            context(4, "valorant.live"),
            step(5, "AppRepairAndUpdate", "PatchStatus"),
            running(6, NOT_UP_TO_DATE),
        ];
        assert!(!launch_page_shown(&events(&lines), "valorant", "live"));

        lines.push(line(7, "OKAY", "Sentry Enabled"));
        assert!(!launch_page_shown(&events(&lines), "valorant", "live"));

        let mut shown_again = lines.clone();
        shown_again.push(step(8, "RestartRequired", "WaitForLaunch"));
        assert!(launch_page_shown(&events(&shown_again), "valorant", "live"));

        let mut declined_again = lines.clone();
        declined_again.push(running(8, "Auto-launch is disabled"));
        assert!(launch_page_shown(
            &events(&declined_again),
            "valorant",
            "live"
        ));
    }

    #[test]
    fn a_refusal_as_not_up_to_date_counts_only_for_the_product_whose_lines_named_one_last() {
        let mut lines = vec![
            context(1, "valorant.live"),
            step(2, "RestartRequired", "WaitForLaunch"),
            context(3, "league_of_legends.live"),
            running(4, NOT_UP_TO_DATE),
        ];
        let other_updating = events(&lines);
        assert!(launch_page_shown(&other_updating, "valorant", "live"));
        assert!(!launch_page_shown(
            &other_updating,
            "league_of_legends",
            "live"
        ));

        lines.extend([
            step(5, "RestartRequired", "WaitForLaunch"),
            context(6, "valorant.live"),
            running(7, NOT_UP_TO_DATE),
            context(8, "league_of_legends.live"),
        ]);
        let valorant_updating = events(&lines);
        assert!(!launch_page_shown(&valorant_updating, "valorant", "live"));
        assert!(launch_page_shown(
            &valorant_updating,
            "league_of_legends",
            "live"
        ));
    }

    // The one recorded run with an update: the client refuses with "Product is not up to date" while its patch state is DependencyUpdate, and is ready about 8 s later.
    #[test]
    fn in_the_recorded_run_the_page_is_shown_only_once_the_client_is_ready() {
        let file =
            ol_testkit::oracle("fixtures/launchers/riot/2026-10-07T17-26-29_10005_Riot Client.log");
        let text = String::from_utf8(std::fs::read(file).unwrap()).unwrap();
        let ready = text
            .find("Middleware transition (PatchStatus) -> (AgeRestriction)")
            .unwrap();
        let before = &text[..text[..ready].rfind('\n').unwrap()];
        let updating = parse_log_events(before);
        assert!(updating.iter().any(|event| matches!(
            event,
            LogEvent::PatchState { status, .. } if status == "DependencyUpdate"
        )));
        assert_eq!(
            launch_declined(&updating, "valorant", "live").as_deref(),
            Some(NOT_UP_TO_DATE)
        );
        assert!(!launch_page_shown(&updating, "valorant", "live"));
        assert!(products_needing_update(&updating).is_empty());

        let whole = parse_log_events(&text);
        assert!(launch_page_shown(&whole, "valorant", "live"));
        assert_eq!(
            launch_declined(&whole, "valorant", "live").as_deref(),
            Some("Auto-launch is disabled")
        );
    }

    #[test]
    fn the_latest_patch_state_of_a_product_says_whether_it_needs_an_update() {
        let status = |at, id: &str, status: &str| {
            line(
                at,
                "OKAY",
                &format!(
                    "patch-proxy: HandleProductPatchlineStatusUpdate: productId: {id}, patchlineId: live, status: {status}, progress: 0"
                ),
            )
        };
        let lines = [
            status(1, "valorant", "OutOfDate"),
            status(2, "bacon", "OutOfDate"),
            status(3, "valorant", "UpToDate"),
            line(
                4,
                "OKAY",
                "patch-proxy: ProductPatchline: UpdateCombinedProductStatus state for lion.live changed from CheckingForUpdate to Ready, launchable: 0",
            ),
        ];
        assert_eq!(
            products_needing_update(&events(&lines)),
            ["bacon.live", "lion.live"]
        );
    }

    #[test]
    fn a_run_log_is_known_by_its_name() {
        assert!(is_client_log_file(
            "2026-10-04T20-34-14_10001_Riot Client.log"
        ));
        assert!(is_client_log_file(
            "2026-10-04t20-34-14.123_1_riot client.LOG"
        ));
        assert!(!is_client_log_file("2026-10-04T20-34-14_Riot Client.log"));
        assert!(!is_client_log_file(
            "2026-10-04T20-34-14_10001_Riot Client.log.bak"
        ));
    }
}
