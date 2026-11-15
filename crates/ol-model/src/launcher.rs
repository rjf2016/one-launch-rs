#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LauncherId {
    Steam,
    Epic,
    Battlenet,
    Riot,
}

impl LauncherId {
    pub const ALL: [Self; 4] = [Self::Steam, Self::Epic, Self::Battlenet, Self::Riot];

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|launcher| launcher.as_str() == text)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Steam => "steam",
            Self::Epic => "epic",
            Self::Battlenet => "battlenet",
            Self::Riot => "riot",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Steam => "Steam",
            Self::Epic => "Epic Games",
            Self::Battlenet => "Battle.net",
            Self::Riot => "Riot Games",
        }
    }

    // The program a switch closes and starts, as it reads in the middle of a sentence: Riot Games is the company, the client is what closes.
    pub const fn client_name(self) -> &'static str {
        match self {
            Self::Steam => "Steam",
            Self::Epic => "Epic Games Launcher",
            Self::Battlenet => "Battle.net",
            Self::Riot => "the Riot Client",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_id_reads_back() {
        for launcher in LauncherId::ALL {
            assert_eq!(LauncherId::parse(launcher.as_str()), Some(launcher));
        }
    }

    #[test]
    fn another_spelling_is_not_a_launcher() {
        for text in ["", "Steam", "steam ", "gog", "battle.net"] {
            assert_eq!(LauncherId::parse(text), None, "{text:?}");
        }
    }
}
