#![forbid(unsafe_code)]

// The recorded runs that turn on Steam alone, each replayed whole and held to its recording. What a comparison takes and leaves out is in ol_replay::check. The run that hands a Steam sign-in over to the user goes on to Epic and is in epic.rs; the one that imports accounts takes Battle.net's too and is in battlenet.rs.

use ol_replay::check::check;

#[test]
fn play_steam() {
    check("play-steam");
}

#[test]
fn rollback() {
    check("rollback");
}

#[test]
fn elevated() {
    check("elevated");
}

#[test]
fn already_active() {
    check("already-active");
}

#[test]
fn restore() {
    check("restore");
}

#[test]
fn stop() {
    check("stop");
}

#[test]
fn self_update() {
    check("self-update");
}

#[test]
fn preview() {
    check("preview");
}
