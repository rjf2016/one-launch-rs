// Steam's updater log, <SteamPath>\logs\bootstrap_log.txt, which tells a client that is updating itself from one that is stuck. steam.exe writes it before its window exists; it names no account.

use crate::js;

const UPDATE: [&str; 6] = [
    "Downloaded new manifest",
    "Downloading update",
    "Found pending update",
    "Installing update",
    "Extracting package",
    "Update complete, launching Steam",
];
const SETTLED: &str = "Verification complete";
const STARTUP: &str = "Startup - updater built";
const BACKGROUND: &str = "Background update loop";

// True while an update at start has not been closed by the restarted client's "Verification complete". A download in the background holds nothing up: it is applied at the next start.
pub fn update_in_progress(text: &str) -> bool {
    let mut updating = false;
    let mut background = false;
    for line in js::lines(text) {
        if line.contains(STARTUP) {
            background = false;
        } else if line.contains(BACKGROUND) {
            background = true;
        } else if line.contains(SETTLED) {
            updating = false;
        } else if !background && UPDATE.iter().any(|mark| line.contains(mark)) {
            updating = true;
        }
    }
    updating
}
