// Steam's library files: <SteamPath>\steamapps\libraryfolders.vdf lists every library folder, and each library's steamapps\appmanifest_<appid>.acf describes one installed app.

use std::collections::HashSet;

use thiserror::Error;

use crate::js;
use crate::vdf::{self, VdfParseError, VdfValue};
use crate::winpath::{normalize_launcher_path, path_key};

const STATE_UPDATE_REQUIRED: i64 = 2;
const STATE_FULLY_INSTALLED: i64 = 4;

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ManifestError {
    #[error(transparent)]
    Vdf(#[from] VdfParseError),
    #[error("{0}")]
    Format(String),
}

impl ManifestError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Vdf(_) => VdfParseError::CODE,
            Self::Format(_) => "MANIFEST_FORMAT",
        }
    }
}

// The library folders in the file's order, each once. Takes the current form ("0" { "path" "D:\\Games\\Steam" }) and the one from before 2021 ("1" "D:\\Games\\Steam").
pub fn parse_library_folders(text: &str) -> Result<Vec<String>, ManifestError> {
    let root = vdf::parse(text)?;
    let block = root.block("libraryfolders").ok_or_else(|| {
        ManifestError::Format("libraryfolders.vdf has no \"libraryfolders\" block".to_owned())
    })?;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for (key, value) in block.entries() {
        if !js::is_ascii_digits(&key) {
            continue;
        }
        let raw = match value {
            VdfValue::Text(path) => Some(path.as_str()),
            VdfValue::Block(folder) => folder.string("path"),
        };
        let Some(raw) = raw.filter(|raw| !js::trim(raw).is_empty()) else {
            continue;
        };
        let path = normalize_launcher_path(raw);
        if seen.insert(path_key(&path)) {
            out.push(path);
        }
    }
    Ok(out)
}

// LastOwner, the SteamID64 of whoever bought the game, is left where it is: the library has no use for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SteamAppManifest {
    pub app_id: String,
    pub name: String,
    pub install_dir: String,
    pub state_flags: i64,
    pub last_played: Option<i64>,
    pub size_on_disk: Option<i64>,
    pub build_id: Option<String>,
}

pub fn parse_app_manifest(text: &str) -> Result<SteamAppManifest, ManifestError> {
    let root = vdf::parse(text)?;
    let state = root
        .block("AppState")
        .ok_or_else(|| ManifestError::Format("appmanifest has no \"AppState\" block".to_owned()))?;
    let app_id = state
        .string("appid")
        .map(js::trim)
        .filter(|id| js::is_ascii_digits(id))
        .ok_or_else(|| ManifestError::Format("appmanifest has no numeric appid".to_owned()))?;
    let install_dir = state
        .string("installdir")
        .map(js::trim)
        .filter(|dir| !dir.is_empty())
        .ok_or_else(|| ManifestError::Format(format!("appmanifest {app_id} has no installdir")))?;
    let name = state
        .string("name")
        .map(js::trim)
        .filter(|name| !name.is_empty())
        .map_or_else(|| format!("App {app_id}"), str::to_owned);
    Ok(SteamAppManifest {
        app_id: app_id.to_owned(),
        name,
        install_dir: install_dir.to_owned(),
        state_flags: state.int("StateFlags").unwrap_or(0),
        last_played: state.int("LastPlayed").filter(|played| *played > 0),
        size_on_disk: state.int("SizeOnDisk"),
        build_id: state.string("buildid").map(|id| js::trim(id).to_owned()),
    })
}

// The Electron app tests the flags as a 32-bit number, so a bit above the 32nd never counts.
const fn low_bits(state_flags: i64) -> i64 {
    state_flags & 0xffff_ffff
}

pub const fn is_fully_installed(state_flags: i64) -> bool {
    low_bits(state_flags) & STATE_FULLY_INSTALLED == STATE_FULLY_INSTALLED
}

pub const fn needs_update(state_flags: i64) -> bool {
    low_bits(state_flags) & STATE_UPDATE_REQUIRED == STATE_UPDATE_REQUIRED
}

// appmanifest_<digits>.acf, in any case.
pub fn is_app_manifest_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower
        .strip_prefix("appmanifest_")
        .and_then(|rest| rest.strip_suffix(".acf"))
        .is_some_and(js::is_ascii_digits)
        && name.is_ascii()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_manifest_name_is_a_manifest() {
        assert!(is_app_manifest_file("appmanifest_252950.acf"));
        assert!(is_app_manifest_file("AppManifest_1.ACF"));
        for name in [
            "appmanifest_.acf",
            "appmanifest_12a.acf",
            "appmanifest_1.acf.bak",
            "xappmanifest_1.acf",
            "libraryfolders.vdf",
        ] {
            assert!(!is_app_manifest_file(name), "{name}");
        }
    }
}
