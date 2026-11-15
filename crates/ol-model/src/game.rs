use std::fmt;

use crate::LauncherId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ArtworkSource {
    SteamGridDb,
    SteamCdn,
    Manual,
    Launcher,
}

impl ArtworkSource {
    pub const ALL: [Self; 4] = [
        Self::SteamGridDb,
        Self::SteamCdn,
        Self::Manual,
        Self::Launcher,
    ];

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|source| source.as_str() == text)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SteamGridDb => "steamgriddb",
            Self::SteamCdn => "steamcdn",
            Self::Manual => "manual",
            Self::Launcher => "launcher",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameArtwork {
    pub source: ArtworkSource,
    pub grid: Option<String>,
    pub hero: Option<String>,
    pub logo: Option<String>,
    pub fetched_at: Option<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct LastLaunch {
    pub account_id: String,
    pub account_name: Option<String>,
    pub launched_at: String,
}

impl fmt::Debug for LastLaunch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LastLaunch")
            .field("launched_at", &self.launched_at)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Game {
    pub id: String,
    pub launcher: LauncherId,
    pub launcher_game_id: String,
    pub name: String,
    pub install_path: Option<String>,
    pub installed: bool,
    pub last_seen_at: String,
    pub needs_update: bool,
    pub last_played_at: Option<String>,
    pub artwork: Option<GameArtwork>,
    pub artwork_error: Option<String>,
    pub last_launch: Option<LastLaunch>,
}

// An install path can run through the user's profile folder, and the last launch names an account.
impl fmt::Debug for Game {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Game")
            .field("launcher", &self.launcher)
            .field("launcher_game_id", &self.launcher_game_id)
            .field("name", &self.name)
            .field("installed", &self.installed)
            .field("needs_update", &self.needs_update)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_source_reads_back() {
        for source in ArtworkSource::ALL {
            assert_eq!(ArtworkSource::parse(source.as_str()), Some(source));
        }
        assert_eq!(ArtworkSource::parse("gog"), None);
    }

    #[test]
    fn debug_shows_no_path_and_no_account() {
        let game = Game {
            id: "game-id".to_owned(),
            launcher: LauncherId::Steam,
            launcher_game_id: "252950".to_owned(),
            name: "Some Game".to_owned(),
            install_path: Some(r"D:\Games\somewhere".to_owned()),
            installed: true,
            last_seen_at: "2026-01-02T03:04:05.678Z".to_owned(),
            needs_update: false,
            last_played_at: None,
            artwork: None,
            artwork_error: None,
            last_launch: Some(LastLaunch {
                account_id: "account-id".to_owned(),
                account_name: Some("Display Name".to_owned()),
                launched_at: "2026-01-02T03:04:05.678Z".to_owned(),
            }),
        };
        let shown = format!("{game:?} {:?}", game.last_launch);
        for hidden in ["somewhere", "account-id", "Display Name"] {
            assert!(!shown.contains(hidden), "{hidden}");
        }
        assert!(shown.contains("Some Game"));
    }
}
