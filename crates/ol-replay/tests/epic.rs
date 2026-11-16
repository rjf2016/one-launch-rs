#![forbid(unsafe_code)]

// The recorded runs that turn on Epic, each replayed whole and held to its recording. What a comparison takes and leaves out is in ol_replay::check.

use ol_replay::check::check;

#[test]
fn play_epic() {
    check("play-epic");
}

#[test]
fn handover() {
    check("handover");
}

#[test]
fn wrong_account() {
    check("wrong-account");
}

#[test]
fn quit_during_switch() {
    check("quit-during-switch");
}

#[test]
fn recovery() {
    check("recovery");
}
