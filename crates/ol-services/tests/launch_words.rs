#![forbid(unsafe_code)]

// How a job that ended well is worded: "Launched" only where the launcher was seen starting the game, and "Sent" where the game was only handed to it. This differs from the Electron app on purpose; oracle/l1 holds its words.

use ol_model::{JobAccount, JobGame, JobState, LaunchJob, LauncherId};
use ol_services::phases::{banner_text, done_pill, sent_note};

const GAMES: [(LauncherId, &str); 4] = [
    (LauncherId::Steam, "Rocket League"),
    (LauncherId::Epic, "Rocket League®"),
    (LauncherId::Battlenet, "Overwatch"),
    (LauncherId::Riot, "VALORANT"),
];

fn done(launcher: LauncherId, game: Option<&str>) -> LaunchJob {
    LaunchJob {
        id: "job".to_owned(),
        state: JobState::Done,
        launcher,
        account: JobAccount {
            id: "account".to_owned(),
            display_name: "Persona Two".to_owned(),
            login: "login".to_owned(),
        },
        game: game.map(|name| JobGame {
            id: "game".to_owned(),
            name: name.to_owned(),
            launcher_game_id: "id".to_owned(),
        }),
        steps: Vec::new(),
        attention: None,
        direct: false,
        switched: true,
        game_seen: false,
        hand_off: None,
        backup_id: None,
        restored: None,
        error: None,
        started_at: String::new(),
        finished_at: None,
    }
}

#[test]
fn a_game_only_handed_to_its_launcher_is_sent_not_launched() {
    let expected = [
        (
            "Sent to Steam as Persona Two",
            "Steam starts Rocket League from here. If it has to update itself or the game first, the game starts after that.",
            "Asked Steam to start Rocket League as Persona Two",
        ),
        (
            "Sent to Epic Games Launcher as Persona Two",
            "Epic Games Launcher starts Rocket League® from here. If it has to update itself or the game first, the game starts after that.",
            "Asked Epic Games Launcher to start Rocket League® as Persona Two",
        ),
        (
            "Sent to Battle.net as Persona Two",
            "Battle.net starts Overwatch from here. If it has to update itself or the game first, the game starts after that.",
            "Asked Battle.net to start Overwatch as Persona Two",
        ),
        (
            "Sent to the Riot Client as Persona Two",
            "The Riot Client starts VALORANT from here. If it has to update itself or the game first, the game starts after that.",
            "Asked the Riot Client to start VALORANT as Persona Two",
        ),
    ];
    for ((launcher, game), (pill, note, banner)) in GAMES.into_iter().zip(expected) {
        let job = done(launcher, Some(game));
        assert_eq!(done_pill(&job), pill);
        assert_eq!(sent_note(&job).as_deref(), Some(note));
        assert_eq!(banner_text(&job), banner);
    }
}

#[test]
fn a_game_the_launcher_was_seen_starting_is_launched() {
    for (launcher, game) in GAMES {
        let job = LaunchJob {
            game_seen: true,
            ..done(launcher, Some(game))
        };
        assert_eq!(done_pill(&job), "Launched as Persona Two");
        assert_eq!(sent_note(&job), None);
        assert_eq!(banner_text(&job), format!("Launched {game} as Persona Two"));
    }
}

#[test]
fn a_hand_over_keeps_its_words() {
    let hand_off =
        "Press Play in the Riot Client window to start VALORANT; it is open on the game's page.";
    let job = LaunchJob {
        hand_off: Some(hand_off.to_owned()),
        ..done(LauncherId::Riot, Some("VALORANT"))
    };
    assert_eq!(done_pill(&job), "Switched to Persona Two");
    assert_eq!(sent_note(&job), None);
    assert_eq!(
        banner_text(&job),
        format!("Switched to Persona Two. {hand_off}")
    );
}

#[test]
fn a_plain_switch_keeps_its_words() {
    for (launcher, _) in GAMES {
        let job = done(launcher, None);
        assert_eq!(done_pill(&job), "Switched to Persona Two");
        assert_eq!(sent_note(&job), None);
        assert_eq!(banner_text(&job), "Switched to Persona Two");
    }
}

#[test]
fn a_job_that_did_not_end_well_has_no_note() {
    for state in [
        JobState::Running,
        JobState::Failed,
        JobState::Cancelled,
        JobState::NeedsSignIn,
    ] {
        let job = LaunchJob {
            state,
            ..done(LauncherId::Epic, Some("Rocket League®"))
        };
        assert_eq!(sent_note(&job), None);
        assert!(!banner_text(&job).starts_with("Asked"));
    }
}
