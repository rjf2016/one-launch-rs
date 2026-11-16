#![forbid(unsafe_code)]

// The recorded runs that turn on Battle.net, and those that became replayable with it. What a comparison takes and leaves out is in ol_replay::check.

use ol_replay::check::{check, check_around_the_unbuilt};

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

// The three scripts below go on to switch Riot. Each is replayed without its Riot calls, and held to every event outside its Riot job and to what it left behind for Steam, Epic and Battle.net.
#[test]
fn play_all_around_riot() {
    check_around_the_unbuilt("play-all");
}

#[test]
fn unowned_around_riot() {
    check_around_the_unbuilt("unowned");
}

#[test]
fn needs_update_around_riot() {
    check_around_the_unbuilt("needs-update");
}
