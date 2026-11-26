// Epic app names with the Steam id of the same game, for games Steam no longer sells: its search does not find them by name, and its servers still hold their artwork. A game Steam still sells is found by its name and has no row here.
const STEAM_APP_IDS: [(&str, &str); 1] = [
    // Rocket League, taken off the Steam store when it went free to play on Epic.
    ("Sugar", "252950"),
];

pub fn steam_app_id(app_name: &str) -> Option<&'static str> {
    STEAM_APP_IDS
        .iter()
        .find(|(name, _)| *name == app_name)
        .map(|(_, id)| *id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rocket_league_is_known_by_its_epic_name_as_written() {
        assert_eq!(steam_app_id("Sugar"), Some("252950"));
        assert_eq!(steam_app_id("sugar"), None);
        assert_eq!(steam_app_id("Fortnite"), None);
        assert_eq!(steam_app_id("constructor"), None);
    }
}
