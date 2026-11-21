use std::cmp::Reverse;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ol_model::{Game, LauncherId, LibrarySort};
use ol_store::{Store, StoreError};
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};

const ARTWORK_FOLDER: &str = "artwork";
const KINDS: [&str; 3] = ["grid", "hero", "logo"];
const EXTENSIONS: [&str; 4] = ["jpg", "png", "webp", "ico"];
const UNITS: [(&str, f64); 6] = [
    ("year", 365. * 24. * 3600.),
    ("month", 30. * 24. * 3600.),
    ("week", 7. * 24. * 3600.),
    ("day", 24. * 3600.),
    ("hour", 3600.),
    ("minute", 60.),
];

// No Debug: the file sits under the data folder, whose path can run through the user's profile.
#[derive(Clone, PartialEq, Eq)]
pub struct LibraryGame {
    pub game: Game,
    pub grid: Option<PathBuf>,
}

pub fn list(store: &Store, data_dir: &Path) -> Result<Vec<LibraryGame>, StoreError> {
    let artwork = data_dir.join(ARTWORK_FOLDER);
    Ok(store
        .installed_games()?
        .into_iter()
        .map(|game| {
            let grid = game
                .artwork
                .as_ref()
                .and_then(|art| art.grid.as_deref())
                .and_then(|stored| artwork_file(&artwork, stored));
            LibraryGame { game, grid }
        })
        .collect())
}

// Only "<game id>/<kind>.<extension>" is joined, so no stored path can name a file outside the artwork folder.
pub fn artwork_file(artwork_dir: &Path, stored: &str) -> Option<PathBuf> {
    let (game_id, file) = stored.split_once('/')?;
    let (kind, extension) = file.split_once('.')?;
    let hex = |part: &str, length: usize| {
        part.len() == length
            && part
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    };
    let mut parts = game_id.split('-');
    let shaped = [8, 4, 4, 4, 12]
        .into_iter()
        .all(|length| parts.next().is_some_and(|part| hex(part, length)))
        && parts.next().is_none();
    if !shaped || !KINDS.contains(&kind) || !EXTENSIONS.contains(&extension) {
        return None;
    }
    Some(artwork_dir.join(game_id).join(file))
}

pub fn last_played(game: &Game) -> Option<&str> {
    let by_launcher = game.last_played_at.as_deref().filter(|at| !at.is_empty());
    let from_here = game
        .last_launch
        .as_ref()
        .map(|launch| launch.launched_at.as_str());
    match (by_launcher, from_here) {
        (Some(by_launcher), Some(from_here)) => Some(by_launcher.max(from_here)),
        (by_launcher, from_here) => by_launcher.or(from_here),
    }
}

// Names compare by their lower-cased characters; the Electron app compares them by the rules of the system language.
pub fn visible(
    games: &[LibraryGame],
    query: &str,
    launcher: Option<LauncherId>,
    sort: LibrarySort,
) -> Vec<usize> {
    let wanted = query.trim().to_lowercase();
    let mut shown: Vec<(usize, String)> = games
        .iter()
        .enumerate()
        .filter(|(_, entry)| launcher.is_none_or(|launcher| entry.game.launcher == launcher))
        .map(|(index, entry)| (index, entry.game.name.to_lowercase()))
        .filter(|(_, name)| name.contains(&wanted))
        .collect();
    match sort {
        LibrarySort::Name => shown.sort_by(|a, b| a.1.cmp(&b.1)),
        LibrarySort::LastPlayed => shown.sort_by(|a, b| {
            let played = |index: usize| last_played(&games[index].game);
            let (first, second) = (played(a.0), played(b.0));
            (first.is_none(), Reverse(first), &a.1).cmp(&(second.is_none(), Reverse(second), &b.1))
        }),
    }
    shown.into_iter().map(|(index, _)| index).collect()
}

pub fn monogram(name: &str) -> String {
    name.split(|letter: char| !letter.is_alphanumeric())
        .filter_map(|word| word.chars().next())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect()
}

pub fn caption(game: &Game, now: SystemTime) -> String {
    let who = match &game.last_launch {
        Some(launch) => launch.account_name.as_deref().unwrap_or("removed account"),
        None => game.launcher.display_name(),
    };
    match last_played(game).and_then(|at| relative(at, now)) {
        Some(when) => format!("{who} \u{b7} {when}"),
        None => who.to_owned(),
    }
}

// English always, in the words the Electron app shows on a PC set to English.
pub fn relative(at: &str, now: SystemTime) -> Option<String> {
    let then = OffsetDateTime::parse(at, &Rfc3339).ok()?;
    let ahead = then - OffsetDateTime::from(now);
    let seconds = half_up(ahead.whole_milliseconds() as f64 / 1000.);
    let Some((unit, size)) = UNITS.into_iter().find(|(_, size)| seconds.abs() >= *size) else {
        return Some("just now".to_owned());
    };
    let count = half_up(seconds / size) as i64;
    let named = match (unit, count) {
        ("day", -1) => Some("yesterday".to_owned()),
        ("day", 1) => Some("tomorrow".to_owned()),
        ("year" | "month" | "week", -1) => Some(format!("last {unit}")),
        ("year" | "month" | "week", 1) => Some(format!("next {unit}")),
        _ => None,
    };
    Some(named.unwrap_or_else(|| {
        let plural = if count.abs() == 1 { "" } else { "s" };
        if count < 0 {
            format!("{} {unit}{plural} ago", -count)
        } else {
            format!("in {count} {unit}{plural}")
        }
    }))
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HourClock {
    Twelve,
    TwentyFour,
}

// English words always, with the clock of the regional settings (`triage.toml` X-02); `offset_of` gives seconds east of UTC for a moment.
pub fn date_time(
    at: &str,
    offset_of: impl Fn(i64) -> Option<i32>,
    clock: HourClock,
) -> Option<String> {
    let local = local(at, offset_of)?;
    Some(format!(
        "{} {}, {}, {}",
        MONTHS[usize::from(u8::from(local.month())) - 1],
        local.day(),
        local.year(),
        clock_time(&local, clock, false),
    ))
}

// The time alone, with its seconds, as the Launchers tab's "Checked" line gives it.
pub fn time_of_day(
    at: &str,
    offset_of: impl Fn(i64) -> Option<i32>,
    clock: HourClock,
) -> Option<String> {
    Some(clock_time(&local(at, offset_of)?, clock, true))
}

fn local(at: &str, offset_of: impl Fn(i64) -> Option<i32>) -> Option<OffsetDateTime> {
    let utc = OffsetDateTime::parse(at, &Rfc3339).ok()?;
    let offset = offset_of(utc.unix_timestamp())
        .and_then(|seconds| UtcOffset::from_whole_seconds(seconds).ok())
        .unwrap_or(UtcOffset::UTC);
    Some(utc.to_offset(offset))
}

fn clock_time(local: &OffsetDateTime, clock: HourClock, seconds: bool) -> String {
    let minutes = if seconds {
        format!("{:02}:{:02}", local.minute(), local.second())
    } else {
        format!("{:02}", local.minute())
    };
    match (clock, local.hour()) {
        (HourClock::TwentyFour, hour) => format!("{hour:02}:{minutes}"),
        (HourClock::Twelve, hour) => {
            let (hour, half) = match hour {
                0 => (12, "AM"),
                hour @ 1..=11 => (hour, "AM"),
                12 => (12, "PM"),
                hour => (hour - 12, "PM"),
            };
            format!("{hour}:{minutes}\u{202f}{half}")
        }
    }
}

// Halves go up, as the Electron app rounds them: -1.5 is -1.
fn half_up(value: f64) -> f64 {
    (value + 0.5).floor()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ol_model::LastLaunch;
    use time::macros::datetime;

    use super::*;

    fn game(name: &str, launcher: LauncherId) -> Game {
        Game {
            id: format!("id-of-{name}"),
            launcher,
            launcher_game_id: name.to_owned(),
            name: name.to_owned(),
            install_path: None,
            installed: true,
            last_seen_at: "2026-01-01T00:00:00.000Z".to_owned(),
            needs_update: false,
            last_played_at: None,
            artwork: None,
            artwork_error: None,
            last_launch: None,
        }
    }

    fn played(name: &str, by_launcher: Option<&str>, from_here: Option<&str>) -> LibraryGame {
        let mut game = game(name, LauncherId::Steam);
        game.last_played_at = by_launcher.map(str::to_owned);
        game.last_launch = from_here.map(|at| LastLaunch {
            account_id: "account-id".to_owned(),
            account_name: Some("Persona One".to_owned()),
            launched_at: at.to_owned(),
        });
        LibraryGame { game, grid: None }
    }

    fn names(games: &[LibraryGame], shown: &[usize]) -> Vec<String> {
        shown
            .iter()
            .map(|index| games[*index].game.name.clone())
            .collect()
    }

    fn now() -> SystemTime {
        SystemTime::from(datetime!(2026-11-06 16:46:23 UTC))
    }

    #[test]
    fn the_newer_of_the_two_records_is_the_last_played() {
        let early = "2026-01-02T03:04:05.000Z";
        let late = "2026-05-06T07:08:09.000Z";
        for (by_launcher, from_here, expected) in [
            (None, None, None),
            (Some(early), None, Some(early)),
            (None, Some(late), Some(late)),
            (Some(early), Some(late), Some(late)),
            (Some(late), Some(early), Some(late)),
            (Some(""), Some(early), Some(early)),
            (Some(""), None, None),
        ] {
            let entry = played("Game", by_launcher, from_here);
            assert_eq!(
                last_played(&entry.game),
                expected,
                "{by_launcher:?} {from_here:?}"
            );
        }
    }

    #[test]
    fn last_played_puts_the_newest_first_and_the_never_played_last_by_name() {
        let games = [
            played("delta", None, None),
            played("Alpha", Some("2026-01-01T00:00:00.000Z"), None),
            played("charlie", None, Some("2026-03-01T00:00:00.000Z")),
            played("Bravo", None, None),
            played("echo", Some("2026-03-01T00:00:00.000Z"), None),
            played(
                "foxtrot",
                Some("2025-01-01T00:00:00.000Z"),
                Some("2026-09-01T00:00:00.000Z"),
            ),
        ];
        let shown = visible(&games, "", None, LibrarySort::LastPlayed);
        assert_eq!(
            names(&games, &shown),
            ["foxtrot", "charlie", "echo", "Alpha", "Bravo", "delta"]
        );
        let shown = visible(&games, "", None, LibrarySort::Name);
        assert_eq!(
            names(&games, &shown),
            ["Alpha", "Bravo", "charlie", "delta", "echo", "foxtrot"]
        );
    }

    #[test]
    fn the_search_ignores_case_and_the_spaces_around_it() {
        let mut games = vec![
            played("Rocket League", None, None),
            played("Overwatch", None, None),
            played("ROCKETEER", None, None),
        ];
        games[1].game.launcher = LauncherId::Battlenet;
        let shown = |query: &str, launcher| {
            names(&games, &visible(&games, query, launcher, LibrarySort::Name))
        };
        assert_eq!(shown("  rocket ", None), ["Rocket League", "ROCKETEER"]);
        assert_eq!(shown("KET L", None), ["Rocket League"]);
        assert_eq!(shown("xyz", None), Vec::<String>::new());
        assert_eq!(shown("", None).len(), 3);
        assert_eq!(shown("", Some(LauncherId::Battlenet)), ["Overwatch"]);
        assert_eq!(
            shown("o", Some(LauncherId::Steam)),
            ["Rocket League", "ROCKETEER"]
        );
        assert_eq!(shown("", Some(LauncherId::Riot)), Vec::<String>::new());
    }

    #[test]
    fn a_monogram_is_the_first_letters_of_the_first_two_words() {
        for (name, expected) in [
            ("Counter-Strike 2", "CS"),
            ("Hearthstone", "H"),
            ("VALORANT", "V"),
            ("rocket league\u{ae}", "RL"),
            ("  the  witcher 3", "TW"),
            ("2048", "2"),
            ("\u{e9}clair noir", "\u{c9}N"),
            ("...", ""),
            ("", ""),
        ] {
            assert_eq!(monogram(name), expected, "{name}");
        }
    }

    #[test]
    fn a_time_reads_as_the_electron_app_words_it() {
        let ago = |seconds: u64| {
            let at = OffsetDateTime::from(now() - Duration::from_secs(seconds));
            relative(&ol_store::iso_timestamp(at), now()).unwrap()
        };
        let (minute, hour, day) = (60, 3600, 24 * 3600);
        for (seconds, expected) in [
            (0, "just now"),
            (59, "just now"),
            (60, "1 minute ago"),
            (89, "1 minute ago"),
            (90, "1 minute ago"),
            (91, "2 minutes ago"),
            (59 * minute, "59 minutes ago"),
            (hour, "1 hour ago"),
            (23 * hour, "23 hours ago"),
            (day, "yesterday"),
            (2 * day, "2 days ago"),
            (6 * day, "6 days ago"),
            (7 * day, "last week"),
            (3 * 7 * day, "3 weeks ago"),
            (30 * day, "last month"),
            (95 * day, "3 months ago"),
            (270 * day, "9 months ago"),
            (364 * day, "12 months ago"),
            (365 * day, "last year"),
            (800 * day, "2 years ago"),
        ] {
            assert_eq!(ago(seconds), expected, "{seconds} s");
        }
        let ahead = |seconds: u64| {
            let at = OffsetDateTime::from(now() + Duration::from_secs(seconds));
            relative(&ol_store::iso_timestamp(at), now()).unwrap()
        };
        for (seconds, expected) in [
            (30, "just now"),
            (2 * minute, "in 2 minutes"),
            (hour, "in 1 hour"),
            (day, "tomorrow"),
            (8 * day, "next week"),
            (400 * day, "next year"),
        ] {
            assert_eq!(ahead(seconds), expected, "{seconds} s");
        }
        assert_eq!(
            relative("2026-11-06T18:46:23+02:00", now()).unwrap(),
            "just now"
        );
        for odd in ["", "yesterday", "2026-11-06", "1762447583"] {
            assert_eq!(relative(odd, now()), None, "{odd}");
        }
    }

    #[test]
    fn the_caption_names_the_last_account_or_the_launcher() {
        let never = played("Game", None, None);
        assert_eq!(caption(&never.game, now()), "Steam");
        let by_launcher = played("Game", Some("2026-02-09T16:46:23.000Z"), None);
        assert_eq!(
            caption(&by_launcher.game, now()),
            "Steam \u{b7} 9 months ago"
        );
        let here = played("Game", None, Some("2026-11-06T16:46:00.000Z"));
        assert_eq!(caption(&here.game, now()), "Persona One \u{b7} just now");
        let mut removed = played("Game", None, Some("2025-11-01T00:00:00.000Z"));
        removed.game.last_launch.as_mut().unwrap().account_name = None;
        assert_eq!(
            caption(&removed.game, now()),
            "removed account \u{b7} last year"
        );
        let unreadable = played("Game", Some("some day"), None);
        assert_eq!(caption(&unreadable.game, now()), "Steam");
    }

    #[test]
    fn only_a_game_folder_and_a_known_file_name_leave_the_artwork_folder_joined() {
        let folder = Path::new(r"D:\data\artwork");
        let id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
        for file in [
            "grid.jpg",
            "grid.png",
            "grid.webp",
            "grid.ico",
            "hero.jpg",
            "logo.png",
        ] {
            assert_eq!(
                artwork_file(folder, &format!("{id}/{file}")),
                Some(folder.join(id).join(file)),
                "{file}"
            );
        }
        let (short, bare) = (&id[..id.len() - 1], id.replace('-', ""));
        for stored in [
            String::new(),
            "grid.jpg".to_owned(),
            id.to_owned(),
            format!("{id}/"),
            format!("{id}/grid"),
            format!("{id}/grid.gif"),
            format!("{id}/grid.jpg.tmp"),
            format!("{id}/cover.jpg"),
            format!("{id}/GRID.JPG"),
            format!("{id}/sub/grid.jpg"),
            format!("{id}/../grid.jpg"),
            format!(r"{id}\grid.jpg"),
            format!("{}/grid.jpg", id.to_uppercase()),
            format!("{short}/grid.jpg"),
            format!("{short}g/grid.jpg"),
            format!("{id}-0/grid.jpg"),
            format!("{bare}/grid.jpg"),
            "../grid.jpg".to_owned(),
            "../../onelaunch.db".to_owned(),
            "/grid.jpg".to_owned(),
            r"C:\Windows\grid.jpg".to_owned(),
            "C:/Windows/grid.jpg".to_owned(),
            format!(r"..\..\{id}/grid.jpg"),
            format!(" {id}/grid.jpg"),
            format!("{id}/grid.jpg "),
            format!("{id}/grid.jpg?v=1"),
        ] {
            assert_eq!(artwork_file(folder, &stored), None, "{stored:?}");
        }
    }

    #[test]
    fn a_date_and_time_read_in_english_words_with_the_clock_and_the_zone_given() {
        let utc = |_| Some(0);
        let twelve = HourClock::Twelve;
        assert_eq!(
            date_time("2026-11-06T16:46:23.000Z", utc, twelve).as_deref(),
            Some("Nov 6, 2026, 4:46\u{202f}PM")
        );
        assert_eq!(
            date_time("2026-11-06T16:46:23.000Z", utc, HourClock::TwentyFour).as_deref(),
            Some("Nov 6, 2026, 16:46")
        );
        assert_eq!(
            date_time("2026-01-01T00:05:00.000Z", utc, twelve).as_deref(),
            Some("Jan 1, 2026, 12:05\u{202f}AM")
        );
        assert_eq!(
            date_time("2026-01-01T00:05:00.000Z", utc, HourClock::TwentyFour).as_deref(),
            Some("Jan 1, 2026, 00:05")
        );
        assert_eq!(
            date_time("2026-07-04T12:00:00Z", |_| Some(-4 * 3600), twelve).as_deref(),
            Some("Jul 4, 2026, 8:00\u{202f}AM")
        );
        assert_eq!(
            date_time("2026-12-31T23:30:00Z", |_| Some(3600), twelve).as_deref(),
            Some("Jan 1, 2027, 12:30\u{202f}AM")
        );
        assert_eq!(
            date_time("2026-11-06T16:46:23Z", |_| None, twelve).as_deref(),
            Some("Nov 6, 2026, 4:46\u{202f}PM")
        );
        assert_eq!(date_time("not a time", utc, twelve), None);
    }

    #[test]
    fn a_time_of_day_keeps_its_seconds() {
        let utc = |_| Some(0);
        assert_eq!(
            time_of_day("2026-11-06T16:46:24.000Z", utc, HourClock::Twelve).as_deref(),
            Some("4:46:24\u{202f}PM")
        );
        assert_eq!(
            time_of_day("2026-11-06T00:05:09Z", utc, HourClock::Twelve).as_deref(),
            Some("12:05:09\u{202f}AM")
        );
        assert_eq!(
            time_of_day(
                "2026-11-06T16:46:24Z",
                |_| Some(3600),
                HourClock::TwentyFour
            )
            .as_deref(),
            Some("17:46:24")
        );
        assert_eq!(time_of_day("not a time", utc, HourClock::Twelve), None);
    }
}
