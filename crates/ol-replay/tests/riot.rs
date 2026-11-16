#![forbid(unsafe_code)]

// The recorded runs that turn on Riot, and the three that switch every launcher in one session, each replayed whole and held to its recording. What a comparison takes and leaves out is in ol_replay::check.

use ol_replay::check::check;

#[test]
fn play_riot() {
    check("play-riot");
}

#[test]
fn riot_stay_signed_in() {
    check("riot-stay-signed-in");
}

#[test]
fn play_all() {
    check("play-all");
}

#[test]
fn unowned() {
    check("unowned");
}

#[test]
fn needs_update() {
    check("needs-update");
}
