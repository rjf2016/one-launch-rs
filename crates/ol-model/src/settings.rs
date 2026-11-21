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

    // Written by the app alone: the window's place, and a launcher's folder from a file the user picked, because One Launch starts the program found there.
    pub const MAIN_ONLY: [Self; 5] = [
        Self::WindowBounds,
        Self::SteamPath,
        Self::EpicPath,
        Self::BattlenetPath,
        Self::RiotPath,
    ];

    pub const TOGGLES: [Self; 9] = [
        Self::DryRun,
        Self::ArtworkAutoFetch,
        Self::SteamAutoLogin,
        Self::RiotPressPlay,
        Self::PlayHideToTray,
        Self::FirstRunDismissed,
        Self::WindowCloseToTray,
        Self::UpdatesAuto,
        Self::WhatsNewEnabled,
    ];

    pub fn is_main_only(self) -> bool {
        Self::MAIN_ONLY.contains(&self)
    }

    pub fn is_toggle(self) -> bool {
        Self::TOGGLES.contains(&self)
    }

    // What a toggle reads as while nothing is stored.
    pub const fn default_on(self) -> bool {
        !matches!(
            self,
            Self::DryRun | Self::SteamAutoLogin | Self::FirstRunDismissed
        )
    }
}

pub const MAX_SETTING_LENGTH: usize = 4096;
pub const DEFAULT_BACKUPS_TO_KEEP: i64 = 20;
pub const MAX_BACKUPS_TO_KEEP: i64 = 200;
const MAX_VERSION_LENGTH: usize = 64;

// Why a change of a setting is refused, in the words of the Electron app's settings:set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SettingRefusal {
    #[error("value exceeds {MAX_SETTING_LENGTH} characters")]
    TooLong,
    #[error("{} is not user-settable", .0.as_str())]
    NotUserSettable(SettingKey),
    #[error("logLevel must be debug, info, warn, or error")]
    LogLevel,
    #[error("epic.launchUri must be catalog or appname")]
    EpicLaunchUri,
    #[error("battlenet.launch must be uri or exec")]
    BattlenetLaunch,
    #[error("whatsNew.lastSeen must be a version")]
    Version,
    #[error("{} must be true or false", .0.as_str())]
    NotTrueOrFalse(SettingKey),
    #[error("backups.keep must be a whole number from 1 to {MAX_BACKUPS_TO_KEEP}")]
    BackupsKeep,
}

impl SettingRefusal {
    pub const CODE: &'static str = "BAD_REQUEST";

    pub const fn code(self) -> &'static str {
        Self::CODE
    }
}

// The rule of each key, checked in the Electron app's order. The length is counted in UTF-16 units, as a JavaScript string's is.
pub fn check_setting(key: SettingKey, value: &str) -> Result<(), SettingRefusal> {
    if value.encode_utf16().count() > MAX_SETTING_LENGTH {
        return Err(SettingRefusal::TooLong);
    }
    if key.is_main_only() {
        return Err(SettingRefusal::NotUserSettable(key));
    }
    let allowed = |values: &[&str]| values.contains(&value);
    match key {
        SettingKey::LogLevel if LogLevel::parse(value).is_none() => Err(SettingRefusal::LogLevel),
        SettingKey::EpicLaunchUri if !allowed(&["catalog", "appname"]) => {
            Err(SettingRefusal::EpicLaunchUri)
        }
        SettingKey::BattlenetLaunch if !allowed(&["uri", "exec"]) => {
            Err(SettingRefusal::BattlenetLaunch)
        }
        SettingKey::WhatsNewLastSeen if !is_version(value) => Err(SettingRefusal::Version),
        key if key.is_toggle() && !allowed(&["true", "false"]) => {
            Err(SettingRefusal::NotTrueOrFalse(key))
        }
        SettingKey::BackupsKeep if !is_backups_keep(value) => Err(SettingRefusal::BackupsKeep),
        _ => Ok(()),
    }
}

fn ascii_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

// `^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$`, at most 64 characters.
pub fn is_version(value: &str) -> bool {
    if value.len() > MAX_VERSION_LENGTH {
        return false;
    }
    let (core, pre) = match value.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (value, None),
    };
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| ascii_digits(part))
        && pre.is_none_or(|pre| {
            !pre.is_empty()
                && pre
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
        })
}

// One to three digits, from 1 to 200.
fn is_backups_keep(value: &str) -> bool {
    value.len() <= 3
        && ascii_digits(value)
        && value
            .parse::<i64>()
            .is_ok_and(|keep| (1..=MAX_BACKUPS_TO_KEEP).contains(&keep))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LogLevel {
    Debug,
    #[default]
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub const ALL: [Self; 4] = [Self::Debug, Self::Info, Self::Warn, Self::Error];

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|level| level.as_str() == text)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Debug => "Debug",
            Self::Info => "Info",
            Self::Warn => "Warn",
            Self::Error => "Error",
        }
    }
}

// The two choices of each dropdown of the Launching panel: the stored value and what the list says.
pub const EPIC_LAUNCH_URI_CHOICES: [(&str, &str); 2] = [
    ("catalog", "Catalog link (default)"),
    ("appname", "App name link"),
];
pub const BATTLENET_LAUNCH_CHOICES: [(&str, &str); 2] = [
    ("exec", "Launch command (default)"),
    ("uri", "battlenet:// link"),
];
// The Backups tab's list; a stored number outside it is offered too.
pub const BACKUPS_KEEP_CHOICES: [i64; 5] = [5, 10, 20, 50, 100];

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

    // As the app acts on a toggle: `true` or `1` is on, anything else stored is off, nothing stored is the key's default.
    pub fn toggle(&self, key: SettingKey) -> bool {
        self.get_bool(key, key.default_on())
    }

    pub fn log_level(&self) -> LogLevel {
        self.get(SettingKey::LogLevel)
            .and_then(LogLevel::parse)
            .unwrap_or_default()
    }

    // The stored value of the dropdown, or its first choice when anything else is stored.
    pub fn epic_launch_uri(&self) -> &'static str {
        self.choice(SettingKey::EpicLaunchUri, &EPIC_LAUNCH_URI_CHOICES)
    }

    pub fn battlenet_launch(&self) -> &'static str {
        self.choice(SettingKey::BattlenetLaunch, &BATTLENET_LAUNCH_CHOICES)
    }

    fn choice(&self, key: SettingKey, choices: &[(&'static str, &str); 2]) -> &'static str {
        let stored = self.get(key);
        choices
            .iter()
            .find(|(value, _)| Some(*value) == stored)
            .unwrap_or(&choices[0])
            .0
    }

    // What the Backups tab shows: the stored number when it is 1 or more, else the default.
    pub fn backups_keep(&self) -> i64 {
        self.get(SettingKey::BackupsKeep)
            .and_then(|value| value.parse::<i64>().ok())
            .filter(|keep| *keep >= 1)
            .unwrap_or(DEFAULT_BACKUPS_TO_KEEP)
    }

    pub fn backups_keep_choices(&self) -> Vec<i64> {
        let mut choices = BACKUPS_KEEP_CHOICES.to_vec();
        choices.push(self.backups_keep());
        choices.sort_unstable();
        choices.dedup();
        choices
    }

    pub fn launcher_path(&self, launcher: LauncherId) -> Option<&str> {
        self.get(SettingKey::launcher_path(launcher))
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
