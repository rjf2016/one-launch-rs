#![forbid(unsafe_code)]

// How a job that ended well is worded: "Launched" only where the launcher was seen starting the game, and "Sent" where the game was only handed to it. This differs from the Electron app on purpose; oracle/l1 holds its words.

use ol_model::{JobAccount, JobGame, JobState, LaunchJob, LauncherId};
use ol_services::phases::{PhaseId, banner_text, done_pill, handed_over, sent_note, waits_for_you};

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

const HAND_OFF: &str =
    "Press Play in the Riot Client window to start VALORANT; it is open on the game's page.";

fn handed_over_job(direct: bool) -> LaunchJob {
    LaunchJob {
        direct,
        hand_off: Some(HAND_OFF.to_owned()),
        ..done(LauncherId::Riot, Some("VALORANT"))
    }
}

#[test]
fn a_hand_over_says_switched_only_when_the_job_switched() {
    let switched = handed_over_job(false);
    assert_eq!(done_pill(&switched), "Switched to Persona Two");
    assert_eq!(sent_note(&switched), None);
    assert_eq!(
        banner_text(&switched),
        format!("Switched to Persona Two. {HAND_OFF}")
    );

    let direct = handed_over_job(true);
    assert_eq!(done_pill(&direct), "Signed in as Persona Two");
    assert_eq!(sent_note(&direct), None);
    assert_eq!(
        banner_text(&direct),
        format!("Signed in as Persona Two. {HAND_OFF}")
    );
}

#[test]
fn the_launch_phase_of_a_hand_over_waits_for_you_and_nothing_else_does() {
    for direct in [false, true] {
        let job = handed_over_job(direct);
        assert!(handed_over(&job));
        for phase in PhaseId::ORDER {
            assert_eq!(waits_for_you(&job, phase), phase == PhaseId::Launch);
        }
    }
    for state in [
        JobState::Running,
        JobState::Failed,
        JobState::Cancelled,
        JobState::NeedsSignIn,
    ] {
        let job = LaunchJob {
            state,
            ..handed_over_job(false)
        };
        assert!(!handed_over(&job));
        assert!(!waits_for_you(&job, PhaseId::Launch));
    }
    for (launcher, game) in GAMES {
        for game_seen in [false, true] {
            let job = LaunchJob {
                game_seen,
                ..done(launcher, Some(game))
            };
            assert!(!handed_over(&job));
            assert!(!waits_for_you(&job, PhaseId::Launch));
        }
    }
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
