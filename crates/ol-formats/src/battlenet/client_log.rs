// The Battle.net client's own log, %LOCALAPPDATA%\Battle.net\Logs\battle.net-<UTC stamp>.log, one file for each run. The account id in a line such as "Logged into Battle.net successfully. |bnet=1:0:0a1b2c3d|" is what One Launch keeps as the account's launcher id. The client writes no token there; a login can stand in the text of a failure, and `mask_reason` takes it out.

use std::sync::LazyLock;

use regex::Regex;

use crate::js::{self, WHITESPACE_CLASS as WS};
use crate::steam::login_log::MAX_LOG_LINE_LENGTH;

// No Debug: an event can hold an account id.
#[derive(Clone, PartialEq, Eq)]
pub enum LogEvent {
    Started {
        build: Option<String>,
    },
    SignInAttempt,
    // `account_id` is empty when the line names none; `at` is the line's own time, in UTC.
    SignedIn {
        account_id: String,
        at: Option<String>,
    },
    SignInFailed {
        reason: String,
    },
    SignedOut,
    MainWindow,
    // Client builds before 2.52.12 said only that they took the request, by the product's program code.
    LaunchRequest {
        code: String,
    },
    // The client's install state for a product, logged at its start and whenever it changes.
    ProductState {
        codename: String,
        operation: Option<String>,
        status: Option<String>,
        playable: Option<bool>,
    },
    GameLaunching {
        codename: String,
    },
    GameRunning {
        codename: String,
    },
    GameExited {
        codename: String,
    },
    CloseRequested,
    Exit,
}

fn pattern(text: &str) -> Regex {
    Regex::new(text).expect("a fixed pattern")
}

// Everything that is not blank as JavaScript counts it.
static NOT_WS: LazyLock<String> = LazyLock::new(|| WS.replacen('[', "[^", 1));

const CODENAME: &str = "[A-Za-z0-9_.-]+";
// A line as far as it goes without a line end of any kind.
const REST: &str = r"[^\n\r\u{2028}\u{2029}]*";

static CLIENT_LOG_FILE: LazyLock<Regex> = LazyLock::new(|| {
    pattern(r"^(?i-u:battle\.net)-[0-9]{8}(?i-u:T)[0-9]{6}(?:\.[0-9]+)?(?i-u:\.log)$")
});
static LINE_TIME: LazyLock<Regex> = LazyLock::new(|| {
    pattern(&format!(
        r"^[A-Za-z0-9_]{WS}+([0-9]{{4}})-([0-9]{{2}})-([0-9]{{2}}) ([0-9]{{2}}):([0-9]{{2}}):([0-9]{{2}})\.([0-9]{{3}})"
    ))
});
static ACCOUNT_ID: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"\|(?i-u:bnet)=[0-9]+:[0-9]+:([0-9a-fA-F]{1,16})\|"));
static ENTITY_ID: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"(?i-u:entityId)=[0-9]+:[0-9]+:[0-9a-fA-F]{1,16}(?-u:\b)"));
static BUILD: LazyLock<Regex> = LazyLock::new(|| pattern(&format!("build ({}+)", NOT_WS.as_str())));
static ERROR_TEXT: LazyLock<Regex> = LazyLock::new(|| pattern(&format!("error=({REST})$")));
static UP_TO_THREAD: LazyLock<Regex> = LazyLock::new(|| pattern(&format!(r"^{REST}\}}{WS}*")));
static INSTALL_STATE: LazyLock<Regex> = LazyLock::new(|| {
    pattern(&format!(
        r"\[ProductState\] \{{Main\}} InstallState \(({CODENAME})\): ({REST})$"
    ))
});
static GAME_LAUNCHING_FIELD: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"(?-u:\b)GameLaunching=1(?-u:\b)"));
static GAME_RUNNING_FIELD: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"(?-u:\b)gameRunning=1(?-u:\b)"));
static GAME_EXITED_FIELD: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"(?-u:\b)gameRunning=0(?-u:\b)"));
static FIELDS: LazyLock<[Regex; 3]> = LazyLock::new(|| {
    ["playable", "currentOperation", "currentOperationStatus"]
        .map(|name| pattern(&format!(r"(?-u:\b){name}=({}+)", NOT_WS.as_str())))
});
static EXECUTE_STATUS: LazyLock<Regex> = LazyLock::new(|| {
    let word = NOT_WS.as_str();
    pattern(&format!(
        "Operation status changed: opType=Execute oldStatus={word}+ newStatus=({word}+) agentUid=({CODENAME})"
    ))
});
static LAUNCH_BINARY: LazyLock<Regex> = LazyLock::new(|| {
    pattern(&format!(
        r"\[GameLaunchController\] \{{Main\}} LaunchBinary: uid=({CODENAME})"
    ))
});
static GAME_IS_RUNNING: LazyLock<Regex> = LazyLock::new(|| {
    pattern(&format!(
        r"\[InstallManager\] \{{Main\}} Game is running: ({CODENAME})"
    ))
});
static LAUNCH_REQUEST: LazyLock<Regex> = LazyLock::new(|| {
    pattern(&format!(
        r"Launch request ({}+?)\.? isRunning=",
        NOT_WS.as_str()
    ))
});
static HEX_RUN: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?-u:\b)[0-9a-fA-F]{16,}(?-u:\b)"));

// Whether a file in the log folder is a client log by its name. The name holds the UTC time the run started.
pub fn is_client_log_file(name: &str) -> bool {
    CLIENT_LOG_FILE.is_match(name)
}

// A second Battle.net.exe started with --uri= or --exec= while the client runs writes a short log of its own and hands the command over. Such a file is not a run of the client.
pub fn is_forwarder_log(head: &str) -> bool {
    head.contains("IPC ShMem mode=client") || head.contains("Leaving because another instance")
}

// The UTC time at the head of a line, as 2026-09-13T03:20:55.149Z.
pub fn line_time(line: &str) -> Option<String> {
    let found = LINE_TIME.captures(line)?;
    Some(format!(
        "{}-{}-{}T{}:{}:{}.{}Z",
        &found[1], &found[2], &found[3], &found[4], &found[5], &found[6], &found[7]
    ))
}

fn in_address(character: char, also: &str) -> bool {
    character.is_ascii_alphanumeric() || character == '_' || also.contains(character)
}

// Where an email address that starts at `start` ends, if one starts there: name, @, a host part, a dot and the rest of the host.
fn address_end(characters: &[char], start: usize) -> Option<usize> {
    let run = |from: usize, also: &str| {
        let length = characters[from..]
            .iter()
            .take_while(|character| in_address(**character, also))
            .count();
        (length > 0).then_some(from + length)
    };
    let at = run(start, ".+-")?;
    (characters.get(at) == Some(&'@')).then_some(())?;
    let dot = run(at + 1, "-")?;
    (characters.get(dot) == Some(&'.')).then_some(())?;
    run(dot + 1, ".-")
}

// The text of a failure with every email address and every run of 16 or more hex digits taken out, cut to 200 characters.
pub fn mask_reason(raw: &str) -> String {
    let characters: Vec<char> = raw.chars().collect();
    let mut without_addresses = String::with_capacity(raw.len());
    let mut at = 0;
    while at < characters.len() {
        let starts_a_name = at == 0 || !in_address(characters[at - 1], ".+-");
        match starts_a_name
            .then(|| address_end(&characters, at))
            .flatten()
        {
            Some(end) => {
                without_addresses.push('…');
                at = end;
            }
            None => {
                without_addresses.push(characters[at]);
                at += 1;
            }
        }
    }
    let masked = HEX_RUN.replace_all(&without_addresses, "…");
    let mut kept = String::new();
    let mut units = 0;
    for character in js::trim(&masked).chars() {
        units += character.len_utf16();
        if units > 200 {
            break;
        }
        kept.push(character);
    }
    kept
}

fn product_state(codename: &str, fields: &str) -> Option<LogEvent> {
    let codename = codename.to_owned();
    if GAME_LAUNCHING_FIELD.is_match(fields) {
        return Some(LogEvent::GameLaunching { codename });
    }
    if GAME_RUNNING_FIELD.is_match(fields) {
        return Some(LogEvent::GameRunning { codename });
    }
    if GAME_EXITED_FIELD.is_match(fields) {
        return Some(LogEvent::GameExited { codename });
    }
    if !fields.contains("currentOperation=") {
        return None;
    }
    let field = |at: usize| FIELDS[at].captures(fields).map(|found| found[1].to_owned());
    // "playable=1" is the product's own flag. The regional entries write "playable:false", with a colon, and are not it.
    Some(LogEvent::ProductState {
        codename,
        playable: field(0).map(|playable| playable == "1"),
        operation: field(1),
        status: field(2),
    })
}

pub fn parse_log_events(text: &str) -> Vec<LogEvent> {
    let mut events = Vec::new();
    for line in js::lines(text) {
        if line.len() > MAX_LOG_LINE_LENGTH && js::utf16_len(line) > MAX_LOG_LINE_LENGTH {
            continue;
        }
        if line.contains("Logging started for Battle.net build") {
            events.push(LogEvent::Started {
                build: BUILD.captures(line).map(|found| found[1].to_owned()),
            });
        } else if line.contains("Logged into Battle.net successfully") {
            events.push(LogEvent::SignedIn {
                account_id: ACCOUNT_ID
                    .captures(line)
                    .map(|found| found[1].to_lowercase())
                    .unwrap_or_default(),
                at: line_time(line),
            });
        } else if (line.contains("Login triggered.") && ENTITY_ID.is_match(line))
            || line.contains("Login_QueueUpdate")
        {
            events.push(LogEvent::SignInAttempt);
        } else if line.contains("DeleteToken(): Deleting registry token") {
            // Logged at start when the token is not that of the account the login page remembers. The client then asks for that account's password.
            events.push(LogEvent::SignInFailed {
                reason: "the client discarded the token at start (its login page remembers a different account)".to_owned(),
            });
        } else if line.contains("Login failed.")
            || line.contains("OnLoginFailed()")
            || line.contains("External Challenge was canceled by user")
        {
            let reason = match ERROR_TEXT.captures(line) {
                Some(found) => found[1].to_owned(),
                None => UP_TO_THREAD.replace(line, "").into_owned(),
            };
            events.push(LogEvent::SignInFailed {
                reason: mask_reason(&reason),
            });
        } else if line.contains("Logout() : calling DeleteToken")
            || line.contains("DeleteToken() because !enabled")
        {
            events.push(LogEvent::SignedOut);
        } else if line.contains("Attempting to show main window") {
            events.push(LogEvent::MainWindow);
        } else if let Some(found) = INSTALL_STATE.captures(line) {
            events.extend(product_state(&found[1], &found[2]));
        } else if let Some(found) = EXECUTE_STATUS.captures(line) {
            let codename = found[2].to_owned();
            match &found[1] {
                "Pending" => events.push(LogEvent::GameLaunching { codename }),
                "On" => events.push(LogEvent::GameRunning { codename }),
                "Off" => events.push(LogEvent::GameExited { codename }),
                _ => {}
            }
        } else if let Some(found) = LAUNCH_BINARY.captures(line) {
            events.push(LogEvent::GameLaunching {
                codename: found[1].to_owned(),
            });
        } else if let Some(found) = GAME_IS_RUNNING.captures(line) {
            events.push(LogEvent::GameRunning {
                codename: found[1].to_owned(),
            });
        } else if let Some(found) = LAUNCH_REQUEST.captures(line) {
            events.push(LogEvent::LaunchRequest {
                code: found[1].to_owned(),
            });
        } else if line.contains("Frontend is requesting to close window") {
            events.push(LogEvent::CloseRequested);
        } else if line.contains("Shutting down battle.net interface")
            || line.contains("signaling application exit")
        {
            events.push(LogEvent::Exit);
        }
    }
    events
}

#[derive(Clone, PartialEq, Eq)]
pub enum SignInOutcome {
    SignedIn {
        account_id: String,
        at: Option<String>,
    },
    Failed {
        reason: String,
    },
}

// The last event that settles a sign-in, or None while the client is still at work or never tried. A sign-out after a sign-in undoes it.
pub fn last_sign_in(events: &[LogEvent]) -> Option<SignInOutcome> {
    events.iter().rev().find_map(|event| match event {
        LogEvent::SignedIn { account_id, at } => Some(SignInOutcome::SignedIn {
            account_id: account_id.clone(),
            at: at.clone(),
        }),
        LogEvent::SignInFailed { reason } => Some(SignInOutcome::Failed {
            reason: reason.clone(),
        }),
        LogEvent::SignedOut => Some(SignInOutcome::Failed {
            reason: "signed out".to_owned(),
        }),
        _ => None,
    })
}

// The run's last word on a sign-in is a sign-out, so the client left no session behind.
pub fn ended_signed_out(events: &[LogEvent]) -> bool {
    events
        .iter()
        .rev()
        .find_map(|event| match event {
            LogEvent::SignedOut => Some(true),
            LogEvent::SignedIn { .. } => Some(false),
            _ => None,
        })
        .unwrap_or(false)
}
