// Riot's products as the client keeps them under %ProgramData%\Riot Games, and the image names their games run as. A Riot game is recognised by its image name and by nothing else: no process of one is ever opened or asked for its path.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use crate::js::{self, WHITESPACE_CLASS as WS};
use crate::js_json::{JsValue, parse};

pub struct ProductInfo {
    pub name: &'static str,
    pub game_images: &'static [&'static str],
}

const LEAGUE_IMAGES: &[&str] = &[
    "LeagueClient.exe",
    "LeagueClientUx.exe",
    "LeagueClientUxRender.exe",
    "League of Legends.exe",
];

// League and Teamfight Tactics share one client, so a running League client counts for both.
pub const PRODUCTS: [(&str, ProductInfo); 5] = [
    (
        "valorant",
        ProductInfo {
            name: "VALORANT",
            game_images: &["VALORANT.exe", "VALORANT-Win64-Shipping.exe"],
        },
    ),
    (
        "league_of_legends",
        ProductInfo {
            name: "League of Legends",
            game_images: LEAGUE_IMAGES,
        },
    ),
    (
        "teamfighttactics",
        ProductInfo {
            name: "Teamfight Tactics",
            game_images: LEAGUE_IMAGES,
        },
    ),
    (
        "bacon",
        ProductInfo {
            name: "Legends of Runeterra",
            game_images: &["LoR.exe"],
        },
    ),
    (
        "lion",
        ProductInfo {
            name: "2XKO",
            game_images: &["2XKO.exe", "2XKO-Win64-Shipping.exe"],
        },
    ),
];

fn pattern(text: &str) -> Regex {
    Regex::new(text).expect("a fixed pattern")
}

// A line as far as it goes without a line end of any kind.
const REST: &str = r"[^\n\r\u{2028}\u{2029}]*";

static PRODUCT_FOLDER: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"^(?i-u:([a-z0-9_]+)\.([a-z0-9_]+))$"));
static TOP_LEVEL_SCALAR: LazyLock<Regex> =
    LazyLock::new(|| pattern(&format!(r"^([A-Za-z0-9_.-]+):[ \t]*({REST}?)[ \t]*$")));
static BLOCK_START: LazyLock<Regex> =
    LazyLock::new(|| pattern(&format!(r"^({WS}*)last-session-timestamp:[ \t]*$")));
static TIMESTAMP_LINE: LazyLock<Regex> = LazyLock::new(|| {
    pattern(&format!(
        r#"^({WS}*)([A-Za-z0-9_.-]+):[ \t]*"?([0-9]{{9,13}})"?[ \t]*$"#
    ))
});

fn known(product: &str) -> Option<&'static ProductInfo> {
    let found = PRODUCTS.iter().find(|(id, _)| *id == product);
    found.map(|(_, info)| info)
}

// An image name as a listing of names gives it: in lower case and without its `.exe`.
pub fn image_key(name: &str) -> String {
    let lower = name.to_lowercase();
    match lower.strip_suffix(".exe") {
        Some(stem) => stem.to_owned(),
        None => lower,
    }
}

// The product one of these process names belongs to, with or without `.exe`.
pub fn product_with_running_image<'n>(
    names: impl IntoIterator<Item = &'n str>,
) -> Option<&'static str> {
    let running: Vec<String> = names.into_iter().map(image_key).collect();
    let runs = |image: &&str| running.contains(&image_key(image));
    let found = PRODUCTS
        .iter()
        .find(|(_, info)| info.game_images.iter().any(runs));
    found.map(|(product, _)| *product)
}

// `<product>.<patchline>`: the metadata folder's name, and what the launch arguments are built from. Made only from a name of that shape, so neither part can hold anything but letters, digits and underscores.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RiotProductId {
    id: String,
    product: String,
    patchline: String,
}

impl RiotProductId {
    // The folder's name as it is written.
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn product(&self) -> &str {
        &self.product
    }

    pub fn patchline(&self) -> &str {
        &self.patchline
    }

    // `<product>.<patchline>` in lower case, as the client's log and settings name the product.
    pub fn key(&self) -> String {
        format!("{}.{}", self.product, self.patchline)
    }
}

// `valorant.live` gives its parts. `Riot Client` and `league_of_legends.live.game_patch` are not products.
pub fn parse_product_id(folder_name: &str) -> Option<RiotProductId> {
    let found = PRODUCT_FOLDER.captures(folder_name)?;
    Some(RiotProductId {
        id: folder_name.to_owned(),
        product: found[1].to_ascii_lowercase(),
        patchline: found[2].to_ascii_lowercase(),
    })
}

pub fn product_display_name(product: &str) -> &str {
    known(product).map_or(product, |info| info.name)
}

// The image names the product's game runs as. None for a product the table does not know.
pub fn game_images_of(product: &str) -> &'static [&'static str] {
    known(product).map_or(&[], |info| info.game_images)
}

// What the game's own shortcut passes to the client.
pub fn launch_args(id: &RiotProductId) -> [String; 2] {
    [
        format!("--launch-product={}", id.product),
        format!("--launch-patchline={}", id.patchline),
    ]
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RiotProductSettings {
    pub install_path: Option<String>,
    pub install_root: Option<String>,
    // The product this one shares an install with: League and Teamfight Tactics name each other.
    pub dependency: Option<String>,
}

fn unquote(value: &str) -> Option<&str> {
    let within = |quote: char| value.strip_prefix(quote)?.strip_suffix(quote);
    let text = within('"').or_else(|| within('\'')).unwrap_or(value);
    Some(text).filter(|text| !text.is_empty())
}

// The scalars at the top level of `<product>.<patchline>.product_settings.yaml`. Blocks below them are passed over.
pub fn parse_product_settings(text: &str) -> RiotProductSettings {
    let mut settings = RiotProductSettings::default();
    for line in js::lines(text) {
        if line.is_empty() || line.starts_with([' ', '\t', '#', '-']) {
            continue;
        }
        let Some(found) = TOP_LEVEL_SCALAR.captures(line) else {
            continue;
        };
        let value = unquote(&found[2]).map(str::to_owned);
        match &found[1] {
            "product_install_full_path" => settings.install_path = value,
            "product_install_root" => settings.install_root = value,
            "product_dependency" => settings.dependency = value,
            _ => {}
        }
    }
    settings
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RiotInstalls {
    // The client program Riot's own games are tied to.
    pub rc_default: Option<String>,
    pub rc_live: Option<String>,
    // Game folder and client program.
    pub associated_clients: Vec<(String, String)>,
}

fn filled(value: Option<&JsValue>) -> Option<String> {
    let text = value?.as_string()?.to_string_lossy();
    (!js::trim(&text).is_empty()).then_some(text)
}

// RiotClientInstalls.json. Anything that does not read says nothing.
pub fn parse_installs(text: &str) -> RiotInstalls {
    let Ok(parsed) = parse(text) else {
        return RiotInstalls::default();
    };
    let Some(root) = parsed.as_object() else {
        return RiotInstalls::default();
    };
    let mut associated_clients = Vec::new();
    match root.get("associated_client") {
        Some(JsValue::Object(folders)) => {
            for (folder, exe) in folders.members() {
                if let Some(exe) = filled(Some(exe)) {
                    associated_clients.push((folder.to_string_lossy(), exe));
                }
            }
        }
        Some(JsValue::Array(items)) => {
            for (at, exe) in items.iter().enumerate() {
                if let Some(exe) = filled(Some(exe)) {
                    associated_clients.push((at.to_string(), exe));
                }
            }
        }
        _ => {}
    }
    RiotInstalls {
        rc_default: filled(root.get("rc_default")),
        rc_live: filled(root.get("rc_live")),
        associated_clients,
    }
}

// `install.last-session-timestamp` of RiotClientSettings.yaml: each product id with the time of its last session. The values are seconds since 1970, or milliseconds from twelve digits on. Only that block is read.
pub fn parse_last_session_timestamps(text: &str) -> HashMap<String, String> {
    let mut times = HashMap::new();
    let mut lines = js::lines(text);
    let Some(indent) = lines.by_ref().find_map(|line| {
        let start = BLOCK_START.captures(line)?;
        Some(js::utf16_len(&start[1]))
    }) else {
        return times;
    };
    for line in lines {
        if js::trim(line).is_empty() {
            continue;
        }
        let Some(found) = TIMESTAMP_LINE
            .captures(line)
            .filter(|found| js::utf16_len(&found[1]) > indent)
        else {
            break;
        };
        let digits = &found[3];
        let Ok(number) = digits.parse::<f64>() else {
            break;
        };
        let milliseconds = if digits.len() >= 12 {
            number
        } else {
            number * 1000.0
        };
        if milliseconds > 0.0
            && let Some(at) = js::iso_from_millis(milliseconds)
        {
            times.insert(found[2].to_lowercase(), at);
        }
    }
    times
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_product_folder_is_told_from_the_rest_and_a_game_from_its_image_names() {
        let id = parse_product_id("valorant.live").unwrap();
        assert_eq!(
            (id.id(), id.product(), id.patchline()),
            ("valorant.live", "valorant", "live")
        );
        assert_eq!(
            launch_args(&id),
            ["--launch-product=valorant", "--launch-patchline=live"]
        );
        let shouted = parse_product_id("League_Of_Legends.PBE").unwrap();
        assert_eq!(shouted.id(), "League_Of_Legends.PBE");
        assert_eq!(
            (shouted.product(), shouted.patchline()),
            ("league_of_legends", "pbe")
        );
        for other in [
            "Riot Client",
            "league_of_legends.live.game_patch",
            "valorant",
            "a b.live",
            "valorant.live\n",
            "valorant.liv\u{212a}",
        ] {
            assert_eq!(parse_product_id(other), None, "{other:?}");
        }

        assert_eq!(
            product_with_running_image(["explorer", "valorant-win64-shipping"]),
            Some("valorant")
        );
        assert_eq!(
            product_with_running_image(["LEAGUECLIENTUX.EXE"]),
            Some("league_of_legends")
        );
        assert_eq!(
            product_with_running_image(["league of legends"]),
            Some("league_of_legends")
        );
        assert_eq!(product_with_running_image(["LoR.exe"]), Some("bacon"));
        assert_eq!(
            product_with_running_image(["riotclientservices", "vgtray"]),
            None
        );
        assert_eq!(product_display_name("bacon"), "Legends of Runeterra");
        assert_eq!(product_display_name("constructor"), "constructor");
        assert!(game_images_of("toString").is_empty());
    }

    #[test]
    fn the_last_sessions_are_read_from_their_block_and_nothing_else() {
        let text = "install:\r\n    globals:\r\n        region: \"NA\"\r\n    last-session-timestamp:\r\n        valorant.live: 1760000000\r\n\r\n        League_Of_Legends.live: \"1760000000123\"\r\n        bacon.live: 0\r\n        short.live: 12345\r\n    other: 1790000000\r\nlast-session-timestamp:\r\n    lion.live: 1790000000\r\n";
        let times = parse_last_session_timestamps(text);
        assert_eq!(times.len(), 2);
        assert_eq!(times["valorant.live"], "2025-10-09T08:53:20.000Z");
        assert_eq!(times["league_of_legends.live"], "2025-10-09T08:53:20.123Z");
        assert!(parse_last_session_timestamps("region: \"NA\"\n").is_empty());
    }
}
