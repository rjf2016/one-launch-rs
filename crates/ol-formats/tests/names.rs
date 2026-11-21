#![forbid(unsafe_code)]

// The names a launcher file holds reach the log's redaction the moment the file is read, so a message built from one later is already covered.

use ol_formats::riot::session::{SessionRead, read_session};
use ol_formats::steam::loginusers::parse_login_users;
use ol_model::redact::with_registered_names;
use ol_testkit::oracle;

fn masked(text: &str) -> String {
    with_registered_names(|names| names.mask(text).into_owned())
}

#[test]
fn every_account_steam_remembers_is_masked_once_its_file_is_read() {
    let text = format!(
        "Steam signed in as user_4 (Persona Four, {}) instead of user_2.",
        format_args!("76561198{:09}", 4)
    );
    let text = text.as_str();
    assert_eq!(masked(text), text);
    let file = std::fs::read_to_string(oracle("fixtures/launchers/steam/loginusers.vdf")).unwrap();
    assert_eq!(parse_login_users(&file).unwrap().len(), 4);
    assert_eq!(
        masked(text),
        "Steam signed in as [ACCOUNT] ([ACCOUNT], [ACCOUNT]) instead of [ACCOUNT]."
    );
}

#[test]
fn the_riot_id_and_account_id_a_session_file_names_are_masked_once_it_is_read() {
    let file = std::fs::read_to_string(oracle(
        "fixtures/launchers/riot/RiotGamesPrivateSettings.yaml",
    ))
    .unwrap();
    let SessionRead::Session(session) = read_session(&file) else {
        panic!("the fixture holds a session");
    };
    let riot_id = session
        .riot_id
        .expect("the fixture's token names a Riot ID");
    let text = format!(
        "{} as {} ({})",
        riot_id.text(),
        riot_id.game_name,
        session.account_id
    );
    assert_eq!(masked(&text), "[ACCOUNT] as [ACCOUNT] ([ACCOUNT])");
}
