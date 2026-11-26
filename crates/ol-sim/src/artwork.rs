// The internet of a run on fake launchers, answered in this process. Steam's searches answer the name asked as a game of its own, except for Riot's games, which Steam does not sell; Steam's image servers and SteamGridDB's answer every picture with a small PNG of one colour, taken from the address; the SteamGridDB API answers from the recordings in oracle/fixtures/external/steamgriddb. Nothing here opens a socket.

use ol_formats::riot::products::PRODUCTS;
use ol_platform_api::http::{HttpError, HttpRequest, HttpResponse, KEY_HEADER};
use ol_platform_fake::http::{json, picture, status};

const UNAUTHORIZED: &str =
    include_str!("../../../oracle/fixtures/external/steamgriddb/unauthorized.json");
const AUTOCOMPLETE: &str =
    include_str!("../../../oracle/fixtures/external/steamgriddb/autocomplete-rocket-league.json");
const GRIDS: &str =
    include_str!("../../../oracle/fixtures/external/steamgriddb/grids-steam-252950.json");
const HEROES: &str =
    include_str!("../../../oracle/fixtures/external/steamgriddb/heroes-game-5267.json");
const EMPTY: &str = include_str!("../../../oracle/fixtures/external/steamgriddb/empty.json");

// A key that begins so is refused, so a run can show what a refused key looks like. Any other key is taken.
pub const REFUSED_KEY_PREFIX: &str = "rejected";

pub fn respond(request: &HttpRequest) -> Result<HttpResponse, HttpError> {
    let url = request.url();
    let path = url.path_and_query();
    Ok(match url.host() {
        "store.steampowered.com" | "steamcommunity.com" => search(path),
        "www.steamgriddb.com" => steamgriddb(request, path),
        host if host.ends_with("steamstatic.com") || host == "cdn2.steamgriddb.com" => {
            let hash = hash(&url.to_string());
            let colour = [
                40 + (hash % 120) as u8,
                40 + ((hash >> 8) % 120) as u8,
                60 + ((hash >> 16) % 140) as u8,
            ];
            picture("image/png", solid_png(60, 90, colour))
        }
        _ => status(404),
    })
}

fn search(path: &str) -> HttpResponse {
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let term = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("term="))
        .or_else(|| path.rsplit('/').next())
        .map(percent_decoded)
        .unwrap_or_default();
    let not_on_steam = PRODUCTS
        .iter()
        .any(|(_, product)| product.name.eq_ignore_ascii_case(&term));
    if not_on_steam {
        return json(200, r#"{"items":[]}"#);
    }
    let id = 100_000 + hash(&term) % 900_000;
    let found = serde_json::json!({ "items": [{ "type": "app", "id": id, "name": term }] });
    json(200, &found.to_string())
}

fn steamgriddb(request: &HttpRequest, path: &str) -> HttpResponse {
    let key = request
        .headers()
        .into_iter()
        .find(|(name, _)| *name == KEY_HEADER)
        .and_then(|(_, value)| value.strip_prefix("Bearer ").map(str::to_owned));
    if key.is_none_or(|key| key.starts_with(REFUSED_KEY_PREFIX)) {
        return json(401, UNAUTHORIZED);
    }
    let recorded = match path.strip_prefix("/api/v2/") {
        Some(call) if call.starts_with("search/autocomplete/") => AUTOCOMPLETE,
        Some(call) if call.starts_with("grids/") => GRIDS,
        Some(call) if call.starts_with("heroes/") => HEROES,
        Some(_) => EMPTY,
        None => return status(404),
    };
    json(200, recorded)
}

fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = bytes
            .get(at + 1..at + 3)
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        match (bytes[at], hex) {
            (b'%', Some(byte)) => {
                decoded.push(byte);
                at += 3;
            }
            (byte, _) => {
                decoded.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

// The Electron app's simulator hashes the same way, so a game gets the same made-up id and colour in both.
fn hash(text: &str) -> u32 {
    let mut hash: u32 = 2_166_136_261;
    for letter in text.chars() {
        let mut units = [0; 2];
        let first = letter.encode_utf16(&mut units)[0];
        hash = (hash ^ u32::from(first)).wrapping_mul(16_777_619);
    }
    hash
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                0xedb8_8320 ^ (crc >> 1)
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut low, mut high) = (1_u32, 0_u32);
    for byte in bytes {
        low = (low + u32::from(*byte)) % 65_521;
        high = (high + low) % 65_521;
    }
    (high << 16) | low
}

fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let body = [kind.as_slice(), data].concat();
    let mut chunk = (data.len() as u32).to_be_bytes().to_vec();
    chunk.extend(&body);
    chunk.extend(crc32(&body).to_be_bytes());
    chunk
}

// A whole 8-bit RGB PNG of one colour, its pixels stored without compression, so the app checks, decodes, stores and draws real picture bytes.
pub fn solid_png(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
    let mut header = Vec::new();
    header.extend(width.to_be_bytes());
    header.extend(height.to_be_bytes());
    header.extend([8, 2, 0, 0, 0]);
    let mut row = vec![0];
    for _ in 0..width {
        row.extend(rgb);
    }
    let raw = row.repeat(height as usize);
    let mut packed = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65_535).peekable();
    while let Some(block) = blocks.next() {
        let length = block.len() as u16;
        packed.push(u8::from(blocks.peek().is_none()));
        packed.extend(length.to_le_bytes());
        packed.extend((!length).to_le_bytes());
        packed.extend(block);
    }
    packed.extend(adler32(&raw).to_be_bytes());
    [
        vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a],
        chunk(b"IHDR", &header),
        chunk(b"IDAT", &packed),
        chunk(b"IEND", &[]),
    ]
    .concat()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ol_platform_api::http::ArtworkUrl;
    use serde_json::Value;

    use super::*;

    const WAIT: Duration = Duration::from_secs(15);

    fn lookup(url: &str) -> HttpRequest {
        HttpRequest::lookup(url, 1024 * 1024, WAIT).unwrap()
    }

    fn body(answer: &HttpResponse) -> Value {
        serde_json::from_slice(answer.body.as_ref().unwrap()).unwrap()
    }

    #[test]
    fn steam_knows_every_game_but_riots() {
        let found = respond(&lookup(
            "https://store.steampowered.com/api/storesearch/?term=Rocket%20League&l=english&cc=US",
        ))
        .unwrap();
        let items = body(&found);
        assert_eq!(items["items"][0]["name"], "Rocket League");
        assert_eq!(items["items"][0]["type"], "app");
        let id = items["items"][0]["id"].as_u64().unwrap();
        assert!((100_000..1_000_000).contains(&id));
        let by_path = respond(&lookup(
            "https://steamcommunity.com/actions/SearchApps/Rocket%20League",
        ))
        .unwrap();
        assert_eq!(body(&by_path)["items"][0]["id"].as_u64(), Some(id));
        for name in ["VALORANT", "valorant", "League%20of%20Legends", "2XKO"] {
            let none = respond(&lookup(&format!(
                "https://store.steampowered.com/api/storesearch/?term={name}&l=english&cc=US"
            )))
            .unwrap();
            assert_eq!(body(&none)["items"], serde_json::json!([]), "{name}");
        }
    }

    // The Electron app's simulator hashes with FNV-1a over UTF-16 units; these are that hash's published sums.
    #[test]
    fn a_name_hashes_as_in_the_electron_apps_simulator() {
        assert_eq!(hash(""), 2_166_136_261);
        assert_eq!(hash("a"), 0xe40c_292c);
        assert_eq!(hash("foobar"), 0xbf9c_f968);
    }

    #[test]
    fn a_picture_is_a_png_the_app_decodes() {
        for host in [
            "cdn.cloudflare.steamstatic.com",
            "shared.cloudflare.steamstatic.com",
            "shared.steamstatic.com",
            "cdn2.steamgriddb.com",
        ] {
            let url = ArtworkUrl::image(&format!("https://{host}/steam/apps/1/logo.png")).unwrap();
            let answer = respond(&HttpRequest::image(url, 1024 * 1024, WAIT).unwrap()).unwrap();
            assert_eq!(answer.content_type.as_deref(), Some("image/png"));
            let bytes = answer.body.unwrap();
            assert_eq!(bytes[..8], [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
            assert_eq!(bytes[16..24], [0, 0, 0, 60, 0, 0, 0, 90], "{host}");
        }
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
        assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
        let large = solid_png(300, 300, [1, 2, 3]);
        assert!(large.len() > 270_000);
    }

    #[test]
    fn steamgriddb_answers_its_recordings_to_a_key_and_refuses_without_one() {
        let api = "https://www.steamgriddb.com/api/v2";
        let without = respond(&lookup(&format!("{api}/grids/steam/252950"))).unwrap();
        assert_eq!(without.status, 401);
        assert_eq!(body(&without)["success"], false);
        let refused = respond(
            &lookup(&format!("{api}/grids/steam/252950"))
                .with_key("rejected0000000000000000")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(refused.status, 401);
        let asked = |call: &str| {
            let request = lookup(&format!("{api}/{call}"))
                .with_key("placeholderkey0000000000")
                .unwrap();
            body(&respond(&request).unwrap())
        };
        assert_eq!(asked("search/autocomplete/VALORANT")["data"][0]["id"], 5267);
        assert_eq!(
            asked("grids/game/5267?dimensions=600x900")["data"][0]["url"],
            "https://cdn2.steamgriddb.com/grid/aaaa1111.png"
        );
        assert_eq!(
            asked("heroes/game/5267")["data"][0]["url"],
            "https://cdn2.steamgriddb.com/hero/cccc3333.jpg"
        );
        assert_eq!(asked("logos/game/5267")["data"], serde_json::json!([]));
    }
}
