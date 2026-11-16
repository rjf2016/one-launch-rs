// %APPDATA%\Battle.net\Battle.net.config and the per-account <hex>.config files beside it: JSON as Qt writes it, with a four-space indent, CRLF, a line end after the last brace and every forward slash escaped as "\/". The layout is read off the file and written back the same way, and a file is rewritten only when reading and writing it unchanged gives its text back.

use ol_model::SwitchChange;
use thiserror::Error;

use crate::epic::ini::Eol;
use crate::js;
use crate::js_json::{JsObject, JsString, JsValue, JsonError, parse, stringify};

const BOM: char = '\u{feff}';
const COMMA: u16 = b',' as u16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfigLayout {
    pub eol: Eol,
    pub indent: usize,
    pub trailing_newline: bool,
    pub escape_slashes: bool,
    pub bom: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ConfigError {
    #[error("not valid JSON ({0})")]
    NotJson(JsonError),
    #[error("not a JSON object")]
    NotAnObject,
    #[error(
        "Battle.net.config does not round-trip through the JSON writer, so One Launch will not rewrite it"
    )]
    NoRoundTrip,
    #[error("Battle.net.config has no Client section")]
    NoClientSection,
}

impl ConfigError {
    pub const CODE: &'static str = "CONFIG_FORMAT";
}

pub fn detect_layout(text: &str) -> ConfigLayout {
    // The indent is that of the first line that starts with spaces and a quote.
    let indent = text.match_indices('\n').find_map(|(at, _)| {
        let rest = &text[at + 1..];
        let spaces = rest.len() - rest.trim_start_matches(' ').len();
        (spaces > 0 && rest[spaces..].starts_with('"')).then_some(spaces)
    });
    ConfigLayout {
        eol: if text.contains("\r\n") {
            Eol::CrLf
        } else {
            Eol::Lf
        },
        indent: indent.unwrap_or(4),
        trailing_newline: text.ends_with('\n'),
        escape_slashes: text.contains("\\/"),
        bom: text.starts_with(BOM),
    }
}

pub fn parse_config(text: &str) -> Result<(JsObject, ConfigLayout), ConfigError> {
    let json = text.strip_prefix(BOM).unwrap_or(text);
    match parse(json).map_err(ConfigError::NotJson)? {
        JsValue::Object(doc) => Ok((doc, detect_layout(text))),
        _ => Err(ConfigError::NotAnObject),
    }
}

fn serialize(doc: JsValue, layout: &ConfigLayout) -> String {
    let mut text = stringify(&doc, layout.indent);
    // The writer never escapes a slash by itself, so every one it wrote is inside a string.
    if layout.escape_slashes {
        text = text.replace('/', "\\/");
    }
    if layout.eol == Eol::CrLf {
        text = text.replace('\n', "\r\n");
    }
    if layout.trailing_newline {
        text.push_str(layout.eol.as_str());
    }
    if layout.bom {
        text.insert(0, BOM);
    }
    text
}

pub fn serialize_config(doc: &JsObject, layout: &ConfigLayout) -> String {
    serialize(JsValue::Object(doc.clone()), layout)
}

// Reading the file and writing it back unchanged gives the same text, so changing one value changes nothing else.
pub fn round_trips(text: &str) -> bool {
    parse_config(text).is_ok_and(|(doc, layout)| serialize(JsValue::Object(doc), &layout) == text)
}

fn section<'a>(doc: &'a JsObject, path: &[&str]) -> Option<&'a JsObject> {
    path.iter()
        .try_fold(doc, |object, key| object.get(key)?.as_object())
}

fn text_of<'a>(object: Option<&'a JsObject>, key: &str) -> Option<&'a JsString> {
    object?.get(key)?.as_string()
}

fn trimmed(units: &[u16]) -> &[u16] {
    let blank = |unit: &u16| char::from_u32(u32::from(*unit)).is_some_and(js::is_whitespace);
    let start = units
        .iter()
        .position(|unit| !blank(unit))
        .unwrap_or(units.len());
    let end = units
        .iter()
        .rposition(|unit| !blank(unit))
        .map_or(start, |at| at + 1);
    &units[start..end]
}

fn split_names(raw: &[u16]) -> Vec<&[u16]> {
    raw.split(|unit| *unit == COMMA)
        .map(trimmed)
        .filter(|name| !name.is_empty())
        .collect()
}

// The logins of Client.SavedAccountNames, which the client writes in lower case with commas between them.
pub fn split_saved_account_names(raw: Option<&str>) -> Vec<String> {
    let units: Vec<u16> = raw.unwrap_or_default().encode_utf16().collect();
    split_names(&units)
        .into_iter()
        .map(String::from_utf16_lossy)
        .collect()
}

// No Debug: the logins.
#[derive(Clone, PartialEq, Eq)]
pub struct ClientSettings {
    // "true" or "false" as written: the client's "Keep me logged in". One Launch never writes it.
    pub auto_login: Option<String>,
    pub saved_account_names: Vec<String>,
    pub version: Option<String>,
}

pub fn read_client_settings(doc: &JsObject) -> ClientSettings {
    let client = section(doc, &["Client"]);
    let text = |object, key| text_of(object, key).map(JsString::to_string_lossy);
    ClientSettings {
        auto_login: text(client, "AutoLogin"),
        saved_account_names: text_of(client, "SavedAccountNames")
            .map(|names| {
                split_names(names.units())
                    .into_iter()
                    .map(String::from_utf16_lossy)
                    .collect()
            })
            .unwrap_or_default(),
        version: text(
            section(doc, &["Client", "Version", "Release"]),
            "LastBuildVersion",
        ),
    }
}

// When a game was last played, in seconds since 1970, from Games.<codename>.LastPlayed. Being listed there does not mean the game is installed.
pub fn game_last_played(doc: &JsObject, codename: &str) -> Option<i64> {
    let games = section(doc, &["Games"])?;
    let raw = text_of(games.get(codename)?.as_object(), "LastPlayed")?.to_string_lossy();
    let seconds = ((9..=11).contains(&raw.len()) && js::is_ascii_digits(&raw))
        .then(|| raw.parse::<i64>().ok())??;
    (seconds > 0).then_some(seconds)
}

// What a window close does for the account that wrote a per-account config file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserPrefs {
    pub hide_on_close: Option<bool>,
    pub display_exit_prompt: Option<bool>,
}

pub fn read_user_prefs(text: &str) -> Result<UserPrefs, ConfigError> {
    let (doc, _) = parse_config(text)?;
    let client = section(&doc, &["User", "Client"]);
    let flag =
        |key| text_of(client, key).map(|value| value.to_string_lossy().to_lowercase() == "true");
    Ok(UserPrefs {
        hide_on_close: flag("HideOnClose"),
        display_exit_prompt: flag("DisplayExitPrompt"),
    })
}

// A login with everything but its first character hidden before the @: a plan names the login it switches to in full and the others in outline.
pub fn mask_login(login: &str) -> String {
    let Some(first) = login.chars().next() else {
        return String::new();
    };
    match login.find('@') {
        Some(at) if at > 0 => format!("{first}…{}", &login[at..]),
        _ if js::utf16_len(login) <= 2 => login.to_owned(),
        _ => format!("{first}…"),
    }
}

// No Debug: the logins.
pub struct SavedNamesWrite {
    // The file to write.
    pub text: String,
    pub changes: Vec<SwitchChange>,
    pub noop: bool,
    pub before: Vec<String>,
    pub after: Vec<String>,
}

// The file with `login` first in Client.SavedAccountNames: moved there, or added when it is not in the list, the others keeping their order. The client moves the login that signs in to the front and, at its start, takes the UnifiedAuth token only of the login that is first. A file whose layout cannot be written back exactly is refused.
pub fn plan_saved_account_names(text: &str, login: &str) -> Result<SavedNamesWrite, ConfigError> {
    let (mut doc, layout) = parse_config(text)?;
    if serialize_config(&doc, &layout) != text {
        return Err(ConfigError::NoRoundTrip);
    }
    let client = doc
        .get_mut("Client")
        .and_then(JsValue::as_object_mut)
        .ok_or(ConfigError::NoClientSection)?;
    let raw = text_of(Some(client), "SavedAccountNames")
        .cloned()
        .unwrap_or_default();
    let names = split_names(raw.units());
    let shown = |names: &[&[u16]]| -> Vec<String> {
        names
            .iter()
            .map(|name| String::from_utf16_lossy(name))
            .collect()
    };
    let wanted = js::trim(login).to_lowercase();
    let is_wanted = |name: &&[u16]| String::from_utf16_lossy(name).to_lowercase() == wanted;
    let before = shown(&names);
    if names.first().is_some_and(is_wanted) {
        return Ok(SavedNamesWrite {
            text: text.to_owned(),
            changes: Vec::new(),
            noop: true,
            after: before.clone(),
            before,
        });
    }
    let wanted_units: Vec<u16> = wanted.encode_utf16().collect();
    let mut reordered: Vec<&[u16]> = vec![
        names
            .iter()
            .find(|name| is_wanted(name))
            .copied()
            .unwrap_or(&wanted_units),
    ];
    reordered.extend(names.iter().filter(|name| !is_wanted(name)));
    let joined = reordered.join(&COMMA);
    let after = shown(&reordered);
    client.insert(
        JsString::from("SavedAccountNames"),
        JsValue::String(JsString::from_units(joined)),
    );
    let next = serialize_config(&doc, &layout);
    let list = |names: &[String]| {
        if names.is_empty() {
            return "(empty)".to_owned();
        }
        let masked: Vec<String> = names
            .iter()
            .map(|name| {
                if name.to_lowercase() == wanted {
                    name.clone()
                } else {
                    mask_login(name)
                }
            })
            .collect();
        masked.join(", ")
    };
    Ok(SavedNamesWrite {
        changes: vec![SwitchChange {
            place: "Client.SavedAccountNames".to_owned(),
            before: list(&before),
            after: list(&after),
        }],
        noop: next == text,
        text: next,
        before,
        after,
    })
}
