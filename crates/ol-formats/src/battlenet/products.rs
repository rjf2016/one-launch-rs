// Battle.net's products. Each has three names: the codename (prometheus: the --uid of its Uninstall entry and its key in the Games section of Battle.net.config), the program code (Pro: what a launch command names) and the TACT code (pro: product.db and battlenet://game/pro).

use thiserror::Error;

use crate::js;
use crate::js_json::{self, JsString, JsValue};
use crate::winpath::normalize_launcher_path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProductCodes {
    pub program_code: String,
    pub tact_code: String,
    pub name: String,
    // Seen on a real installation, with the client's log, the registry and product.db agreeing.
    pub verified: bool,
    // The same game's Steam id when Steam sells it too, which lets its artwork come from Steam.
    pub steam_app_id: Option<&'static str>,
}

type Product = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    bool,
    Option<&'static str>,
);

// Codename, program code, TACT code, name. Only the first two rows were seen on a real installation; the rest are the codes in common use, taken when nothing else names the product.
pub const PRODUCTS: [Product; 22] = [
    (
        "prometheus",
        "Pro",
        "pro",
        "Overwatch",
        true,
        Some("2357570"),
    ),
    ("hs_beta", "WTCG", "hsb", "Hearthstone", true, None),
    ("wow", "WoW", "wow", "World of Warcraft", false, None),
    (
        "wow_classic",
        "WoWC",
        "wow_classic",
        "World of Warcraft Classic",
        false,
        None,
    ),
    (
        "wow_classic_era",
        "WoWC",
        "wow_classic_era",
        "World of Warcraft Classic Era",
        false,
        None,
    ),
    ("diablo3", "D3", "d3", "Diablo III", false, None),
    (
        "fenris",
        "Fen",
        "fenris",
        "Diablo IV",
        false,
        Some("2344520"),
    ),
    ("osi", "OSI", "osi", "Diablo II: Resurrected", false, None),
    ("anbs", "ANBS", "anbs", "Diablo Immortal", false, None),
    ("s2", "S2", "s2", "StarCraft II", false, None),
    ("s1", "S1", "s1", "StarCraft", false, None),
    ("heroes", "Hero", "hero", "Heroes of the Storm", false, None),
    ("w3", "W3", "w3", "Warcraft III: Reforged", false, None),
    (
        "odin",
        "ODIN",
        "odin",
        "Call of Duty: Modern Warfare",
        false,
        None,
    ),
    (
        "zeus",
        "ZEUS",
        "zeus",
        "Call of Duty: Black Ops Cold War",
        false,
        Some("1985810"),
    ),
    (
        "fore",
        "FORE",
        "fore",
        "Call of Duty: Vanguard",
        false,
        Some("1985820"),
    ),
    (
        "lazr",
        "LAZR",
        "lazr",
        "Call of Duty: Modern Warfare II",
        false,
        Some("1938090"),
    ),
    (
        "auks",
        "AUKS",
        "auks",
        "Call of Duty",
        false,
        Some("1938090"),
    ),
    (
        "viper",
        "VIPR",
        "viper",
        "Call of Duty: Black Ops 4",
        false,
        None,
    ),
    (
        "rtro",
        "RTRO",
        "rtro",
        "Blizzard Arcade Collection",
        false,
        None,
    ),
    ("wlby", "WLBY", "wlby", "Warcraft III", false, None),
    ("gryphon", "GRY", "gryphon", "Warcraft Rumble", false, None),
];

// A product's row of the table.
pub fn known_product(codename: &str) -> Option<ProductCodes> {
    PRODUCTS
        .iter()
        .find(|product| product.0 == codename)
        .map(|product| ProductCodes {
            program_code: product.1.to_owned(),
            tact_code: product.2.to_owned(),
            name: product.3.to_owned(),
            verified: product.4,
            steam_app_id: product.5,
        })
}

// A product's codes: its row of the table with product.db's TACT code where the two differ, or, for a product the table lacks, a guess from product.db's TACT code.
pub fn product_codes(codename: &str, tact_from_db: Option<&str>) -> Option<ProductCodes> {
    let from_db = tact_from_db.filter(|tact| !tact.is_empty());
    match (known_product(codename), from_db) {
        (Some(known), Some(tact)) => Some(ProductCodes {
            tact_code: tact.to_owned(),
            ..known
        }),
        (Some(known), None) => Some(known),
        (None, Some(tact)) => Some(ProductCodes {
            program_code: tact.to_uppercase(),
            tact_code: tact.to_owned(),
            name: codename.to_owned(),
            verified: false,
            steam_app_id: None,
        }),
        (None, None) => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProductDbEntry {
    pub codename: String,
    pub tact_code: String,
    pub install_path: String,
}

// More products than Battle.net has ever had: a file that gives more is not a product.db.
const MAX_PRODUCTS: usize = 512;

fn is_name(text: &str, longest: usize, also: &[u8]) -> bool {
    let bytes = text.as_bytes();
    (2..=longest).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || also.contains(byte))
}

// The part of a text from its first drive letter, colon and separator on, when something follows them.
fn path_tail(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    (0..bytes.len().saturating_sub(3))
        .find(|at| {
            bytes[*at].is_ascii_alphabetic()
                && bytes[at + 1] == b':'
                && matches!(bytes[at + 2], b'\\' | b'/')
        })
        .map(|at| &text[at..])
}

// %ProgramData%\Battle.net\Agent\product.db is a protobuf in which each install record holds the codename, the TACT code and the install folder as three texts in a row. They are taken from the runs of printable characters without decoding the message; a length byte that happens to be printable can stick to the front of a text, which is why a path is looked for inside its run.
pub fn parse_product_db(bytes: &[u8]) -> Vec<ProductDbEntry> {
    let runs: Vec<&str> = bytes
        .split(|byte| !(0x20..0x7f).contains(byte))
        .filter(|run| run.len() >= 2)
        .filter_map(|run| std::str::from_utf8(run).ok())
        .collect();
    let mut entries: Vec<ProductDbEntry> = Vec::new();
    for triple in runs.windows(3) {
        if entries.len() >= MAX_PRODUCTS {
            break;
        }
        let (codename, tact) = (triple[0], triple[1]);
        let Some(path) = path_tail(triple[2]) else {
            continue;
        };
        let named = is_name(codename, 32, b"_.") && is_name(tact, 16, b"_");
        let known = entries.iter().any(|entry| entry.codename == codename);
        if !named || known || codename == "agent" || codename == "battle.net" {
            continue;
        }
        entries.push(ProductDbEntry {
            codename: codename.to_owned(),
            tact_code: tact.to_owned(),
            install_path: normalize_launcher_path(path),
        });
    }
    entries
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AggregateEntry {
    pub name: String,
    pub tact_code: String,
    pub launch_uri: Option<String>,
    pub icon_path: Option<String>,
    pub last_played_at: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
#[error("Invalid time value")]
pub struct InvalidTime;

// %ProgramData%\Battle.net\Agent\aggregate.json, the list the client keeps for the Xbox app. It can lag behind what is installed. A file that is not JSON holds nothing.
pub fn parse_aggregate(text: &str) -> Result<Vec<AggregateEntry>, InvalidTime> {
    let json = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Ok(root) = js_json::parse(json) else {
        return Ok(Vec::new());
    };
    let items = root
        .as_object()
        .and_then(|root| root.get("installed"))
        .and_then(JsValue::as_array)
        .unwrap_or_default();
    let mut entries = Vec::new();
    for item in items.iter().filter_map(JsValue::as_object) {
        let text = |name: &str| {
            item.get(name)
                .and_then(JsValue::as_string)
                .map(JsString::to_string_lossy)
        };
        let Some(tact) = text("product_id").filter(|tact| !tact.is_empty()) else {
            continue;
        };
        let played = item
            .get("last_played_timestamp")
            .and_then(JsValue::as_number)
            .filter(|milliseconds| *milliseconds > 0.0);
        entries.push(AggregateEntry {
            name: text("name").unwrap_or_else(|| tact.clone()),
            launch_uri: text("launch_uri"),
            icon_path: text("icon_path").map(|path| normalize_launcher_path(&path)),
            tact_code: tact,
            last_played_at: match played {
                Some(milliseconds) => Some(js::iso_from_millis(milliseconds).ok_or(InvalidTime)?),
                None => None,
            },
        });
    }
    Ok(entries)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchForm {
    // The link the client itself gives out. In a running client it only shows the game's page.
    Uri,
    // What a second Battle.net.exe hands to the running client, which then starts the game.
    Exec,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchCommand {
    Uri(String),
    Exec(String),
}

pub fn launch_command(codes: &ProductCodes, form: LaunchForm) -> LaunchCommand {
    match form {
        LaunchForm::Exec => LaunchCommand::Exec(format!("--exec=launch {}", codes.program_code)),
        LaunchForm::Uri => LaunchCommand::Uri(format!("battlenet://game/{}", codes.tact_code)),
    }
}
