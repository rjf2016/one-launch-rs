// What the client's log says about its products: whether one has to be updated before it runs, whether a launch command made it start, and whether its game runs already.

use super::client_log::LogEvent;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProductState {
    // UpToDate, Update, or whatever else the client is doing with the product.
    pub operation: Option<String>,
    pub status: Option<String>,
    // The product's own flag. The state the client remembers from its last run can lack it.
    pub playable: Option<bool>,
}

// The last install state the client reported for each product, in the order the products first appear. The state it remembers at its start gives way to a fresh one when its servers say otherwise.
pub fn product_states(events: &[LogEvent]) -> Vec<(String, ProductState)> {
    let mut states: Vec<(String, ProductState)> = Vec::new();
    for event in events {
        let LogEvent::ProductState {
            codename,
            operation,
            status,
            playable,
        } = event
        else {
            continue;
        };
        let state = ProductState {
            operation: operation.clone(),
            status: status.clone(),
            playable: *playable,
        };
        match states.iter_mut().find(|(known, _)| known == codename) {
            Some((_, known)) => *known = state,
            None => states.push((codename.clone(), state)),
        }
    }
    states
}

// The client wants to update the product first: its Play button reads Update, and a launch command for it does nothing.
pub fn product_needs_update(state: Option<&ProductState>) -> bool {
    state
        .and_then(|state| state.operation.as_deref())
        .is_some_and(|operation| operation.to_lowercase() == "update")
}

// The client started the product's game. Builds before 2.52.12 said only that they took the launch request for its program code.
pub fn game_started(events: &[LogEvent], codename: &str, program_code: &str) -> bool {
    let (codename, program_code) = (codename.to_lowercase(), program_code.to_lowercase());
    events.iter().any(|event| match event {
        LogEvent::GameLaunching { codename: named } | LogEvent::GameRunning { codename: named } => {
            named.to_lowercase() == codename
        }
        LogEvent::LaunchRequest { code } => code.to_lowercase() == program_code,
        _ => false,
    })
}

// The client's last word on the game's process, whoever started it: seen launching or running, and its end not logged since.
pub fn game_running(events: &[LogEvent], codename: &str) -> bool {
    let wanted = codename.to_lowercase();
    let mut running = false;
    for event in events {
        let (named, now) = match event {
            LogEvent::GameLaunching { codename } | LogEvent::GameRunning { codename } => {
                (codename, true)
            }
            LogEvent::GameExited { codename } => (codename, false),
            _ => continue,
        };
        if named.to_lowercase() == wanted {
            running = now;
        }
    }
    running
}
