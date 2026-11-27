// The part of an artwork lookup that asks the internet, in the Electron app's order: Steam's own id for the game or Steam's two searches, Steam's image servers for each kind that is missing, and SteamGridDB for what is left when a key is stored. Every game takes this one way, a Riot game too: SteamGridDB has their covers. It runs on a thread of its own, holds what it downloads in memory and writes nothing: only a game's name and its Steam or SteamGridDB id ever leave, and the key in its header.

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use ol_formats::js;
use ol_model::{ArtworkKind, ArtworkSource, LauncherId};
use ol_platform_api::CancelToken;
use ol_platform_api::http::{
    ArtworkUrl, HttpClient, HttpError, HttpRequest, HttpResponse, KEY_HOST, TooLarge,
};
use ol_vault::Secret;
use regex::Regex;
use serde_json::Value;

use super::{Download, decode, is_steam_placeholder, sniff_image};

pub const TIMEOUT: Duration = Duration::from_secs(15);
pub const MAX_DOWNLOAD_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_JSON_BYTES: u64 = 1024 * 1024;
pub const MAX_IMAGE_REDIRECTS: usize = 3;

const STEAM_CDN: [&str; 2] = [
    "https://cdn.cloudflare.steamstatic.com/steam/apps",
    "https://shared.cloudflare.steamstatic.com/store_item_assets/steam/apps",
];
const SGDB_API: &str = "https://www.steamgriddb.com/api/v2";
const SGDB_FILTER: &str = "types=static&nsfw=false&humor=false";
const REDIRECTS: [u16; 5] = [301, 302, 303, 307, 308];
const STOPPED: &str = "stopped";
const PLACEHOLDER: &str = "only Steam's gray placeholder";

static NOT_A_WORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[^\p{L}\p{N}]+").expect("the pattern is fixed"));

const fn steam_file(kind: ArtworkKind) -> &'static str {
    match kind {
        ArtworkKind::Grid => "library_600x900.jpg",
        ArtworkKind::Hero => "library_hero.jpg",
        ArtworkKind::Logo => "logo.png",
    }
}

const fn sgdb_type(kind: ArtworkKind) -> &'static str {
    match kind {
        ArtworkKind::Grid => "grids",
        ArtworkKind::Hero => "heroes",
        ArtworkKind::Logo => "logos",
    }
}

// The SteamGridDB key out of the vault, asked for at the moment a lookup needs it and not before. An error is why it could not be read.
pub trait KeySource: Send + Sync {
    fn api_key(&self) -> Result<Option<Secret>, String>;
}

// The game as a lookup needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wanted {
    pub game_id: String,
    pub name: String,
    pub launcher: LauncherId,
    // The Steam id known without asking: the game's own, or the one a launcher's table gives.
    pub app_id: Option<String>,
    pub missing: Vec<ArtworkKind>,
}

#[derive(Default)]
pub struct Fetched {
    // In the order found: Steam's kinds first, then SteamGridDB's.
    pub found: Vec<(ArtworkKind, Download, ArtworkSource)>,
    pub problems: Vec<String>,
    // Why the lookup could not run to its end; nothing of it is kept then.
    pub crashed: Option<String>,
}

impl Fetched {
    fn has(&self, kind: ArtworkKind) -> bool {
        self.found.iter().any(|(found, ..)| *found == kind)
    }
}

pub fn fetch(
    http: &dyn HttpClient,
    keys: &dyn KeySource,
    wanted: &Wanted,
    cancel: &CancelToken,
) -> Fetched {
    let asking = Asking { http, cancel };
    let mut fetched = Fetched::default();
    let mut app_id = wanted.app_id.clone();
    if app_id.is_none() && wanted.launcher != LauncherId::Steam {
        let (found, error) = asking.steam_app_id(&wanted.name);
        app_id = found;
        if let Some(error) = error {
            fetched.problems.push(format!("Steam search: {error}"));
        }
    }
    if let Some(app_id) = &app_id {
        for kind in &wanted.missing {
            match asking.steam_picture(app_id, *kind) {
                Ok(image) => fetched.found.push((*kind, image, ArtworkSource::SteamCdn)),
                Err(error) => fetched
                    .problems
                    .push(format!("Steam CDN {}: {error}", kind.as_str())),
            }
        }
    }
    let still: Vec<ArtworkKind> = wanted
        .missing
        .iter()
        .copied()
        .filter(|kind| !fetched.has(*kind))
        .collect();
    if !still.is_empty() && !cancel.is_cancelled() {
        match keys.api_key() {
            Err(reason) => {
                return Fetched {
                    crashed: Some(reason),
                    ..Fetched::default()
                };
            }
            Ok(Some(key)) => {
                asking.ask_steamgriddb(wanted, app_id.as_deref(), &still, &key, &mut fetched)
            }
            Ok(None) if !fetched.has(ArtworkKind::Grid) => fetched.problems.push(
                if app_id.is_some() {
                    "No SteamGridDB API key in Settings"
                } else {
                    "Not found on Steam; add a SteamGridDB API key in Settings"
                }
                .to_owned(),
            ),
            Ok(None) => {}
        }
    }
    fetched
}

struct Asking<'a> {
    http: &'a dyn HttpClient,
    cancel: &'a CancelToken,
}

impl Asking<'_> {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
        if self.cancel.is_cancelled() {
            return Err(STOPPED.to_owned());
        }
        self.http.send(request).map_err(|error| error.to_string())
    }

    // A game's name to its Steam id, taken only from a result whose name is the game's, case, punctuation and trademark signs aside: a looser match would put another game's cover on the card.
    fn steam_app_id(&self, name: &str) -> (Option<String>, Option<String>) {
        let want = normalize_name(name);
        if want.is_empty() {
            return (None, None);
        }
        let term = encode_uri_component(&search_term(name));
        let searches = [
            format!("https://store.steampowered.com/api/storesearch/?term={term}&l=english&cc=US"),
            format!("https://steamcommunity.com/actions/SearchApps/{term}"),
        ];
        let mut last_error = None;
        for url in searches {
            match self.search(&url, &want) {
                Ok(Some(app_id)) => return (Some(app_id), None),
                Ok(None) => {}
                Err(error) => last_error = Some(error),
            }
        }
        (None, last_error)
    }

    fn search(&self, url: &str, want: &str) -> Result<Option<String>, String> {
        let request =
            HttpRequest::lookup(url, MAX_JSON_BYTES, TIMEOUT).map_err(|error| error.to_string())?;
        let answer = self.send(&request)?;
        if answer.status == 404 {
            return Ok(None);
        }
        let body = json_of(answer)?;
        let items = match &body {
            Value::Array(items) => items,
            other => match other.get("items") {
                Some(Value::Array(items)) => items,
                _ => return Ok(None),
            },
        };
        for item in items.iter().filter(|item| item.is_object()) {
            if item
                .get("type")
                .is_some_and(|kind| kind.is_string() && kind != "app")
            {
                continue;
            }
            let id = match item.get("id") {
                None | Some(Value::Null) => item.get("appid"),
                id => id,
            };
            let named = item
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|label| !label.is_empty() && normalize_name(label) == want);
            if named && let Some(app_id) = id.and_then(digits) {
                return Ok(Some(app_id));
            }
        }
        Ok(None)
    }

    // The second host is asked only when the first has no such file, or only its gray placeholder.
    fn steam_picture(&self, app_id: &str, kind: ArtworkKind) -> Result<Download, String> {
        let mut last_error = String::new();
        for host in STEAM_CDN {
            let url = format!(
                "{host}/{}/{}",
                encode_uri_component(app_id),
                steam_file(kind)
            );
            match self.download(&url, ArtworkSource::SteamCdn) {
                Ok(image) => return Ok(image),
                Err(error) => {
                    let gone = error.starts_with("HTTP 404") || error == PLACEHOLDER;
                    last_error = error;
                    if !gone {
                        break;
                    }
                }
            }
        }
        Err(last_error)
    }

    fn ask_steamgriddb(
        &self,
        wanted: &Wanted,
        app_id: Option<&str>,
        kinds: &[ArtworkKind],
        key: &Secret,
        fetched: &mut Fetched,
    ) {
        let mut problems = Vec::new();
        if let Err(error) = self.steamgriddb(wanted, app_id, kinds, key, fetched, &mut problems) {
            problems.push(format!("SteamGridDB: {error}"));
        }
        // What the API answers is kept as the reason; should it ever repeat the key, the key is not kept with it.
        let key = key.expose();
        fetched
            .problems
            .extend(problems.into_iter().map(|problem| redacted(problem, key)));
    }

    fn steamgriddb(
        &self,
        wanted: &Wanted,
        app_id: Option<&str>,
        kinds: &[ArtworkKind],
        key: &Secret,
        fetched: &mut Fetched,
        problems: &mut Vec<String>,
    ) -> Result<(), String> {
        let by_game = match app_id {
            Some(app_id) => format!("steam/{}", encode_uri_component(app_id)),
            None => {
                let name = encode_uri_component(&wanted.name);
                let hits = self.api(&format!("{SGDB_API}/search/autocomplete/{name}"), key)?;
                let Some(id) = first_of(&hits)
                    .and_then(|hit| hit.get("id"))
                    .and_then(number)
                else {
                    problems.push(format!("SteamGridDB: no match for \"{}\"", wanted.name));
                    return Ok(());
                };
                format!("game/{id}")
            }
        };
        for kind in kinds {
            let filter = match kind {
                ArtworkKind::Grid => format!("dimensions=600x900&{SGDB_FILTER}"),
                ArtworkKind::Hero | ArtworkKind::Logo => SGDB_FILTER.to_owned(),
            };
            let assets = self.api(
                &format!("{SGDB_API}/{}/{by_game}?{filter}", sgdb_type(*kind)),
                key,
            )?;
            let kind_name = kind.as_str();
            let image_url = first_of(&assets)
                .and_then(|asset| asset.get("url"))
                .and_then(Value::as_str)
                .filter(|url| !url.is_empty());
            let Some(image_url) = image_url else {
                problems.push(format!("SteamGridDB: no {kind_name}"));
                continue;
            };
            if ArtworkUrl::image(image_url).is_none() {
                problems.push(format!(
                    "SteamGridDB {kind_name}: the image is not on an artwork host"
                ));
                continue;
            }
            match self.download(image_url, ArtworkSource::SteamGridDb) {
                Ok(image) => fetched
                    .found
                    .push((*kind, image, ArtworkSource::SteamGridDb)),
                Err(error) => problems.push(format!("SteamGridDB {kind_name}: {error}")),
            }
        }
        Ok(())
    }

    // One call of the SteamGridDB API, answered with its `data`.
    fn api(&self, url: &str, key: &Secret) -> Result<Value, String> {
        debug_assert!(url.contains(KEY_HOST));
        let rejected = || "the API key was rejected".to_owned();
        let request = HttpRequest::lookup(url, MAX_JSON_BYTES, TIMEOUT)
            .map_err(|error| error.to_string())?
            .with_key(key.expose())
            .map_err(|_| rejected())?;
        let answer = self.send(&request)?;
        match answer.status {
            401 | 403 => return Err(rejected()),
            404 => return Ok(Value::Array(Vec::new())),
            429 => return Err("rate limited, try again later".to_owned()),
            _ => {}
        }
        let mut body = json_of(answer)?;
        if body.get("success") != Some(&Value::Bool(true)) {
            return Err(match body.get("errors") {
                Some(Value::Array(errors)) => {
                    let said: Vec<String> = errors.iter().map(js_string).collect();
                    // Redacted before the cut, which could otherwise leave a start of the key too short to be found.
                    cut_utf16(&redacted(said.join(", "), key.expose()), 200).to_owned()
                }
                _ => "request failed".to_owned(),
            });
        }
        Ok(body.get_mut("data").map(Value::take).unwrap_or(Value::Null))
    }

    // A picture, asked without the key. Redirects are followed by hand, a few at most and only among the image hosts, within one time limit for the whole chain; what comes back must say it is an image, stay within the size limit, start as a JPEG, PNG or WebP and decode as one, and from Steam's servers be more than their placeholder.
    fn download(&self, url: &str, from: ArtworkSource) -> Result<Download, String> {
        let started = Instant::now();
        let mut next = ArtworkUrl::image(url).ok_or("not an artwork host")?;
        let mut hops = 0;
        let answer = loop {
            let left = TIMEOUT.saturating_sub(started.elapsed());
            if left.is_zero() {
                return Err(HttpError::Timeout.to_string());
            }
            let request = HttpRequest::image(next.clone(), MAX_DOWNLOAD_BYTES, left)
                .map_err(|error| error.to_string())?;
            let answer = self.send(&request)?;
            if !REDIRECTS.contains(&answer.status) {
                break answer;
            }
            let location = answer.location.as_deref().unwrap_or_default();
            if location.is_empty() {
                return Err(format!("HTTP {}", answer.status));
            }
            if hops >= MAX_IMAGE_REDIRECTS {
                return Err("too many redirects".to_owned());
            }
            next = next
                .redirected(location)
                .ok_or("redirected away from the artwork hosts")?;
            hops += 1;
        };
        if !answer.is_ok() {
            return Err(format!("HTTP {}", answer.status));
        }
        let content_type = answer.content_type.unwrap_or_default().to_lowercase();
        if !content_type.starts_with("image/") {
            let said = match content_type.is_empty() {
                true => "no content type",
                false => &content_type,
            };
            return Err(format!("not an image ({said})"));
        }
        let bytes = answer
            .body
            .map_err(|large| too_large(large, MAX_DOWNLOAD_BYTES))?;
        let ext = sniff_image(&bytes).ok_or("unrecognised image data")?;
        let picture = decode(&bytes, ext).ok_or("unrecognised image data")?;
        if from == ArtworkSource::SteamCdn && is_steam_placeholder(&picture) {
            return Err(PLACEHOLDER.to_owned());
        }
        Ok(Download { bytes, ext })
    }
}

fn redacted(text: String, key: &str) -> String {
    match key.is_empty() {
        true => text,
        false => text.replace(key, "[key]"),
    }
}

fn too_large(large: TooLarge, limit: u64) -> String {
    match large {
        TooLarge::Declared(declared) => format!("too large ({declared} bytes)"),
        TooLarge::Read => format!("too large (over {limit} bytes)"),
    }
}

// A lookup's answer as JSON. A redirect is an error here, as the Electron app asks its fetch to treat it.
fn json_of(answer: HttpResponse) -> Result<Value, String> {
    if REDIRECTS.contains(&answer.status) {
        return Err(HttpError::Failed("redirected").to_string());
    }
    if !answer.is_ok() {
        return Err(format!("HTTP {}", answer.status));
    }
    let bytes = answer
        .body
        .map_err(|large| too_large(large, MAX_JSON_BYTES))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("the answer is not JSON ({error})"))
}

fn first_of(data: &Value) -> Option<&Value> {
    data.as_array()?.first().filter(|first| first.is_object())
}

// A number as JavaScript writes it into a string, for the whole numbers an id is.
fn number(value: &Value) -> Option<String> {
    let number = value.as_number()?;
    if let Some(whole) = number.as_i64() {
        return Some(whole.to_string());
    }
    if let Some(whole) = number.as_u64() {
        return Some(whole.to_string());
    }
    let float = number.as_f64().filter(|float| float.is_finite())?;
    Some(if float.fract() == 0. && float.abs() < 1e21 {
        format!("{float:.0}")
    } else {
        float.to_string()
    })
}

// A Steam id out of a search result: a whole number from 0 up, or a string of digits.
fn digits(id: &Value) -> Option<String> {
    let text = match id {
        Value::String(text) => text.clone(),
        Value::Number(_) => number(id)?,
        _ => return None,
    };
    let text = if text == "-0" { "0".to_owned() } else { text };
    js::is_ascii_digits(&text).then_some(text)
}

// What JavaScript's String() makes of a value, for the errors the API lists.
fn js_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "null".to_owned(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(_) => number(value).unwrap_or_default(),
        Value::Object(_) => "[object Object]".to_owned(),
        Value::Array(items) => items
            .iter()
            .map(|item| match item {
                Value::Null => String::new(),
                item => js_string(item),
            })
            .collect::<Vec<_>>()
            .join(","),
    }
}

// Lower-case words alone, so "Rocket League®" and "rocket league" are one name and "Overwatch 2" and "Overwatch" are two.
pub fn normalize_name(name: &str) -> String {
    let plain = without_marks(&name.to_lowercase());
    NOT_A_WORD
        .replace_all(&plain, " ")
        .trim_matches(' ')
        .to_owned()
}

fn search_term(name: &str) -> String {
    js::trim(&without_marks(name)).to_owned()
}

fn without_marks(text: &str) -> String {
    text.replace(['\u{ae}', '\u{2122}', '\u{a9}'], "")
}

// JavaScript's encodeURIComponent.
pub fn encode_uri_component(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
            )
        {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

// At most `most` UTF-16 units, as JavaScript's slice counts them, and never half a character.
pub fn cut_utf16(text: &str, most: usize) -> &str {
    let mut units = 0;
    for (at, letter) in text.char_indices() {
        units += letter.len_utf16();
        if units > most {
            return &text[..at];
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_name_compares_by_its_lower_case_words() {
        for (name, normal) in [
            ("Rocket League\u{ae}", "rocket league"),
            ("  ROCKET   League\u{2122} ", "rocket league"),
            ("Counter-Strike 2", "counter strike 2"),
            ("Overwatch\u{a9} 2", "overwatch 2"),
            (
                "Tom Clancy's Rainbow Six\u{ae} Siege",
                "tom clancy s rainbow six siege",
            ),
            ("\u{c9}clair: Noir!", "\u{e9}clair noir"),
            ("\u{2462} Three", "\u{2462} three"),
            ("...", ""),
            ("\u{ae}", ""),
            ("", ""),
        ] {
            assert_eq!(normalize_name(name), normal, "{name}");
        }
        assert_ne!(normalize_name("Overwatch 2"), normalize_name("Overwatch"));
    }

    #[test]
    fn a_name_goes_into_an_address_as_javascript_encodes_it() {
        for (text, encoded) in [
            ("Rocket League", "Rocket%20League"),
            ("Tom Clancy's (R6)!", "Tom%20Clancy's%20(R6)!"),
            ("a+b&c=d/e?f#g", "a%2Bb%26c%3Dd%2Fe%3Ff%23g"),
            ("\u{c9}clair", "%C3%89clair"),
            ("100%", "100%25"),
            ("a.b-c_d~e*f", "a.b-c_d~e*f"),
            ("\u{1f600}", "%F0%9F%98%80"),
            ("a\r\nb", "a%0D%0Ab"),
            ("", ""),
        ] {
            assert_eq!(encode_uri_component(text), encoded, "{text}");
        }
        assert_eq!(search_term(" Rocket League\u{ae}\u{a0}"), "Rocket League");
    }

    #[test]
    fn an_id_is_taken_as_javascript_reads_it() {
        for (id, taken) in [
            (json!(252_950), Some("252950")),
            (json!("252950"), Some("252950")),
            (json!(0), Some("0")),
            (json!(252_950.0), Some("252950")),
            (json!(-0.0), Some("0")),
            (json!(-5), None),
            (json!(1.5), None),
            (json!("12a"), None),
            (json!(""), None),
            (json!(" 12"), None),
            (json!("\u{0663}"), None),
            (json!(null), None),
            (json!(true), None),
            (json!([1]), None),
        ] {
            assert_eq!(digits(&id).as_deref(), taken, "{id}");
        }
        assert_eq!(number(&json!(5267)).as_deref(), Some("5267"));
        assert_eq!(number(&json!(5267.0)).as_deref(), Some("5267"));
        assert_eq!(number(&json!(52.5)).as_deref(), Some("52.5"));
        assert_eq!(number(&json!("5267")), None);
    }

    #[test]
    fn a_cut_counts_utf16_units_and_keeps_characters_whole() {
        assert_eq!(cut_utf16("abcdef", 3), "abc");
        assert_eq!(cut_utf16("abc", 3), "abc");
        assert_eq!(cut_utf16("ab\u{1f600}cd", 3), "ab");
        assert_eq!(cut_utf16("ab\u{1f600}cd", 4), "ab\u{1f600}");
        assert_eq!(cut_utf16("", 3), "");
    }

    #[test]
    fn the_errors_of_the_api_read_as_javascript_would_write_them() {
        let said: Vec<String> = [
            json!("Unauthorized"),
            json!(404),
            json!(null),
            json!(false),
            json!({"a": 1}),
            json!(["a", null, 2]),
        ]
        .iter()
        .map(js_string)
        .collect();
        assert_eq!(
            said,
            [
                "Unauthorized",
                "404",
                "null",
                "false",
                "[object Object]",
                "a,,2"
            ]
        );
    }
}
