#![forbid(unsafe_code)]

// The recorded runs that turn on Battle.net, and those that became replayable with it. What a comparison takes and leaves out is in ol_replay::check.

use ol_replay::check::check;

#[test]
fn play_battlenet() {
    check("play-battlenet");
}

#[test]
fn game_running() {
    check("game-running");
}

#[test]
fn import() {
    check("import");
}

// The script reads the status of the launchers and of the artwork, which this build has no command for; a read leaves nothing behind, and the replay passes over those two.
#[test]
fn first_run() {
    check("first-run");
}
