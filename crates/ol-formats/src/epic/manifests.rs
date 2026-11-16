// The Epic Games Launcher's library files: one <InstallationGuid>.item of JSON per installed app under <AppDataPath>\Manifests, and LauncherInstalled.dat as a second list of the same.

use serde_json::{Map, Value};
use thiserror::Error;

use crate::js;
use crate::winpath::normalize_launcher_path;

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum EpicManifestError {
    // `reason` is the JSON reader's own wording.
    #[error("{what}not valid JSON ({reason})")]
    NotJson { what: &'static str, reason: String },
    #[error("{0}")]
    Format(String),
}

impl EpicManifestError {
    pub const CODE: &'static str = "MANIFEST_FORMAT";
}

// <32 hex digits>.item
pub fn is_item_file(name: &str) -> bool {
    name.strip_suffix(".item")
        .is_some_and(|stem| stem.len() == 32 && stem.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

// No Debug: a game can be installed under the user's profile folder.
#[derive(Clone, PartialEq)]
pub struct EpicItemManifest {
    pub installation_guid: String,
    // Epic's own name for the app, such as "Sugar" for Rocket League: the library's launcher_game_id.
    pub app_name: String,
    pub display_name: String,
    pub catalog_namespace: Option<String>,
    pub catalog_item_id: Option<String>,
    pub install_location: String,
    // Relative to install_location.
    pub launch_executable: Option<String>,
    pub install_size: Option<f64>,
    pub incomplete: bool,
    // For an add-on, the app it belongs to.
    pub main_game_app_name: Option<String>,
    pub categories: Vec<String>,
}

fn text(fields: &Map<String, Value>, key: &str) -> Option<String> {
    fields
        .get(key)
        .and_then(Value::as_str)
        .map(js::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn object(json: &str, what: &'static str) -> Result<Value, EpicManifestError> {
    let json = json.strip_prefix('\u{feff}').unwrap_or(json);
    serde_json::from_str(json).map_err(|error| EpicManifestError::NotJson {
        what,
        reason: error.to_string(),
    })
}

// One .item file. Session ids, hashes and download addresses in it are not taken.
pub fn parse_item_manifest(json: &str) -> Result<EpicItemManifest, EpicManifestError> {
    let raw = object(json, "")?;
    let fields = raw
        .as_object()
        .ok_or_else(|| EpicManifestError::Format("not a JSON object".to_owned()))?;
    let app_name = text(fields, "AppName")
        .ok_or_else(|| EpicManifestError::Format("manifest has no AppName".to_owned()))?;
    let install_location = text(fields, "InstallLocation").ok_or_else(|| {
        EpicManifestError::Format(format!("manifest for {app_name} has no InstallLocation"))
    })?;
    Ok(EpicItemManifest {
        installation_guid: text(fields, "InstallationGuid").unwrap_or_default(),
        display_name: text(fields, "DisplayName").unwrap_or_else(|| app_name.clone()),
        catalog_namespace: text(fields, "CatalogNamespace"),
        catalog_item_id: text(fields, "CatalogItemId"),
        install_location: normalize_launcher_path(&install_location),
        launch_executable: text(fields, "LaunchExecutable")
            .map(|path| normalize_launcher_path(&path)),
        install_size: fields
            .get("InstallSize")
            .and_then(Value::as_f64)
            .filter(|size| size.is_finite() && *size >= 0.0),
        incomplete: fields.get("bIsIncompleteInstall") == Some(&Value::Bool(true)),
        main_game_app_name: text(fields, "MainGameAppName"),
        categories: fields
            .get("AppCategories")
            .and_then(Value::as_array)
            .map(|categories| {
                categories
                    .iter()
                    .filter_map(|category| category.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
        app_name,
    })
}

// A manifest the library shows: fully installed, a game or without a category, and no add-on of another app. An Unreal Engine install has the category "engines".
pub fn is_library_game(manifest: &EpicItemManifest) -> bool {
    let add_on = manifest
        .main_game_app_name
        .as_ref()
        .is_some_and(|main| *main != manifest.app_name);
    let other_kind = !manifest.categories.is_empty()
        && !manifest
            .categories
            .iter()
            .any(|category| category == "games");
    !manifest.incomplete && !add_on && !other_kind
}

#[derive(Clone, PartialEq, Eq)]
pub struct LauncherInstalledEntry {
    pub app_name: String,
    pub install_location: String,
    pub namespace_id: Option<String>,
    pub item_id: Option<String>,
}

pub fn parse_launcher_installed(
    json: &str,
) -> Result<Vec<LauncherInstalledEntry>, EpicManifestError> {
    let raw = object(json, "LauncherInstalled.dat is ")?;
    let list = raw
        .get("InstallationList")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            EpicManifestError::Format("LauncherInstalled.dat has no InstallationList".to_owned())
        })?;
    Ok(list
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|fields| {
            Some(LauncherInstalledEntry {
                app_name: text(fields, "AppName").or_else(|| text(fields, "ArtifactId"))?,
                install_location: normalize_launcher_path(&text(fields, "InstallLocation")?),
                namespace_id: text(fields, "NamespaceId"),
                item_id: text(fields, "ItemId"),
            })
        })
        .collect())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchUriForm {
    // What the launcher writes into its own desktop shortcuts.
    Catalog,
    AppName,
}

// Every byte of the text but A-Z a-z 0-9 - _ . ! ~ * ' ( ) as %XX.
fn uri_component(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

// The link that starts a game. Without both catalog ids the older form with the app's name alone is used.
pub fn launch_uri(
    app_name: &str,
    catalog_namespace: Option<&str>,
    catalog_item_id: Option<&str>,
    form: LaunchUriForm,
) -> String {
    fn filled(id: Option<&str>) -> Option<&str> {
        id.filter(|id| !id.is_empty())
    }
    let name = uri_component(app_name);
    let id = match (form, filled(catalog_namespace), filled(catalog_item_id)) {
        (LaunchUriForm::Catalog, Some(namespace), Some(item)) => {
            format!("{namespace}%3A{item}%3A{name}")
        }
        _ => name,
    };
    format!("com.epicgames.launcher://apps/{id}?action=launch&silent=true")
}
