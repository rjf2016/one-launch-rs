use std::collections::BTreeMap;
use std::fmt;

use crate::LauncherId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SettingKey {
    DryRun,
    LogLevel,
    ArtworkAutoFetch,
    BackupsKeep,
    EpicLaunchUri,
    BattlenetLaunch,
    SteamAutoLogin,
    RiotPressPlay,
    PlayHideToTray,
    SteamPath,
    EpicPath,
    BattlenetPath,
    RiotPath,
    WindowBounds,
    WindowCloseToTray,
    FirstRunDismissed,
    UpdatesAuto,
    WhatsNewEnabled,
    WhatsNewLastSeen,
}

impl SettingKey {
    pub const ALL: [Self; 19] = [
        Self::DryRun,
        Self::LogLevel,
        Self::ArtworkAutoFetch,
        Self::BackupsKeep,
        Self::EpicLaunchUri,
        Self::BattlenetLaunch,
        Self::SteamAutoLogin,
        Self::RiotPressPlay,
        Self::PlayHideToTray,
        Self::SteamPath,
        Self::EpicPath,
        Self::BattlenetPath,
        Self::RiotPath,
        Self::WindowBounds,
        Self::WindowCloseToTray,
        Self::FirstRunDismissed,
        Self::UpdatesAuto,
        Self::WhatsNewEnabled,
        Self::WhatsNewLastSeen,
    ];

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|key| key.as_str() == text)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DryRun => "dryRun",
            Self::LogLevel => "logLevel",
            Self::ArtworkAutoFetch => "artwork.autoFetch",
            Self::BackupsKeep => "backups.keep",
            Self::EpicLaunchUri => "epic.launchUri",
            Self::BattlenetLaunch => "battlenet.launch",
            Self::SteamAutoLogin => "steam.autoLogin",
            Self::RiotPressPlay => "riot.pressPlay",
            Self::PlayHideToTray => "play.hideToTray",
            Self::SteamPath => "steam.path",
            Self::EpicPath => "epic.path",
            Self::BattlenetPath => "battlenet.path",
            Self::RiotPath => "riot.path",
            Self::WindowBounds => "window.bounds",
            Self::WindowCloseToTray => "window.closeToTray",
            Self::FirstRunDismissed => "firstRun.dismissed",
            Self::UpdatesAuto => "updates.auto",
            Self::WhatsNewEnabled => "whatsNew.enabled",
            Self::WhatsNewLastSeen => "whatsNew.lastSeen",
        }
    }

    pub const fn launcher_path(launcher: LauncherId) -> Self {
        match launcher {
            LauncherId::Steam => Self::SteamPath,
            LauncherId::Epic => Self::EpicPath,
            LauncherId::Battlenet => Self::BattlenetPath,
            LauncherId::Riot => Self::RiotPath,
        }
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct Settings(BTreeMap<SettingKey, String>);

impl Settings {
    pub fn get(&self, key: SettingKey) -> Option<&str> {
        self.0.get(&key).map(String::as_str)
    }

    pub fn get_bool(&self, key: SettingKey, fallback: bool) -> bool {
        self.get(key)
            .map_or(fallback, |value| value == "true" || value == "1")
    }

    pub fn get_int(&self, key: SettingKey, fallback: i64) -> i64 {
        self.get(key)
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(fallback)
    }

    pub fn insert(&mut self, key: SettingKey, value: String) {
        self.0.insert(key, value);
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn keys(&self) -> impl Iterator<Item = SettingKey> + '_ {
        self.0.keys().copied()
    }
}

// A launcher path can run through the user's profile folder, so the values stay out.
impl fmt::Debug for Settings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set()
            .entries(self.0.keys().map(|key| key.as_str()))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_reads_back() {
        for key in SettingKey::ALL {
            assert_eq!(SettingKey::parse(key.as_str()), Some(key));
        }
        assert_eq!(SettingKey::parse("dryrun"), None);
    }

    #[test]
    fn the_keys_are_the_ones_the_electron_app_stores() {
        for file in ["settings-non-defaults.json", "settings-defaults.json"] {
            let path = ol_testkit::oracle(&format!("fixtures/settings/{file}"));
            let recorded: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            let recorded: Vec<&str> = recorded
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .filter(|key| *key != "_note")
                .collect();
            let known: Vec<&str> = SettingKey::ALL.iter().map(|key| key.as_str()).collect();
            assert_eq!(known, recorded, "{file}");
        }
    }

    #[test]
    fn a_missing_or_odd_value_falls_back() {
        let mut settings = Settings::default();
        assert!(settings.get_bool(SettingKey::DryRun, true));
        assert_eq!(settings.get_int(SettingKey::BackupsKeep, 20), 20);
        settings.insert(SettingKey::DryRun, "1".to_owned());
        settings.insert(SettingKey::UpdatesAuto, "yes".to_owned());
        settings.insert(SettingKey::BackupsKeep, "many".to_owned());
        assert!(settings.get_bool(SettingKey::DryRun, false));
        assert!(!settings.get_bool(SettingKey::UpdatesAuto, true));
        assert_eq!(settings.get_int(SettingKey::BackupsKeep, 20), 20);
        settings.insert(SettingKey::BackupsKeep, "35".to_owned());
        assert_eq!(settings.get_int(SettingKey::BackupsKeep, 20), 35);
    }

    #[test]
    fn debug_lists_keys_only() {
        let mut settings = Settings::default();
        settings.insert(SettingKey::SteamPath, r"D:\somewhere\Steam".to_owned());
        let shown = format!("{settings:?}");
        assert!(shown.contains("steam.path"));
        assert!(!shown.contains("somewhere"));
    }
}
