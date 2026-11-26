#![forbid(unsafe_code)]

// The artwork lookups against an internet that exists only in the test: every address asked is written down, and SteamGridDB answers from the recordings in oracle/fixtures/external. No test here opens a socket.

use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use image::{DynamicImage, GrayAlphaImage, GrayImage, ImageFormat, Luma, LumaA, Rgb, RgbImage};
use ol_model::{ArtworkKind, ArtworkSource, InstalledGame, LauncherId, SettingKey};
use ol_platform_api::CancelToken;
use ol_platform_api::http::{
    HttpClient, HttpError, HttpResponse, IMAGE_HOSTS, KEY_HOST, LOOKUP_HOSTS, TooLarge,
};
use ol_platform_fake::RecordingHttp;
use ol_platform_fake::http::{json, picture, redirect, status};
use ol_services::artwork::fetch::{MAX_DOWNLOAD_BYTES, MAX_JSON_BYTES};
use ol_services::artwork::{
    Artwork, ArtworkError, KeySource, MAX_MANUAL_BYTES, Outcome, Prepared, decode, fetch,
    is_steam_placeholder, sniff_image,
};
use ol_services::log::Log;
use ol_sim::artwork::solid_png;
use ol_store::{ArtworkRow, Store, database_path};
use ol_testkit::vectors::{Vectors, bytes};
use ol_testkit::{Scratch, oracle};
use ol_vault::Secret;
use time::OffsetDateTime;
use time::macros::datetime;

const KEY: &str = "placeholderkey0000000000";
const NOW: OffsetDateTime = datetime!(2026-11-22 10:00:00 UTC);
const STAMP: &str = "2026-11-22T10:00:00.000Z";
const CDN: &str = "https://cdn.cloudflare.steamstatic.com/steam/apps";
const SHARED: &str = "https://shared.cloudflare.steamstatic.com/store_item_assets/steam/apps";
const SGDB: &str = "https://www.steamgriddb.com/api/v2";
const FILTER: &str = "types=static&nsfw=false&humor=false";

fn recorded(name: &str) -> String {
    fs::read_to_string(oracle(&format!("fixtures/external/steamgriddb/{name}"))).unwrap()
}

fn png() -> Vec<u8> {
    solid_png(60, 90, [200, 60, 70])
}

fn encoded(format: ImageFormat) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(RgbImage::from_pixel(12, 18, image::Rgb([10, 120, 200])))
        .write_to(&mut bytes, format)
        .unwrap();
    bytes.into_inner()
}

struct Keys {
    key: Result<Option<&'static str>, &'static str>,
    asked: AtomicUsize,
}

impl Keys {
    fn none() -> Self {
        Self::holding(Ok(None))
    }

    fn stored() -> Self {
        Self::holding(Ok(Some(KEY)))
    }

    fn holding(key: Result<Option<&'static str>, &'static str>) -> Self {
        Self {
            key,
            asked: AtomicUsize::new(0),
        }
    }

    fn asked(&self) -> usize {
        self.asked.load(Ordering::SeqCst)
    }
}

impl KeySource for Keys {
    fn api_key(&self) -> Result<Option<Secret>, String> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        match self.key {
            Ok(key) => Ok(key.map(|key| Secret::new(key.to_owned()))),
            Err(reason) => Err(reason.to_owned()),
        }
    }
}

// Answers the addresses it was given and 404 to every other.
#[derive(Default)]
struct Web(Mutex<HashMap<String, Result<HttpResponse, HttpError>>>);

impl Web {
    fn with(self, url: impl Into<String>, answer: HttpResponse) -> Self {
        self.0.lock().unwrap().insert(url.into(), Ok(answer));
        self
    }

    fn failing(self, url: impl Into<String>, error: HttpError) -> Self {
        self.0.lock().unwrap().insert(url.into(), Err(error));
        self
    }

    fn steam_pictures(mut self, app_id: &str) -> Self {
        for file in ["library_600x900.jpg", "library_hero.jpg", "logo.png"] {
            self = self.with(
                format!("{CDN}/{app_id}/{file}"),
                picture("image/png", png()),
            );
        }
        self
    }

    fn http(self) -> RecordingHttp {
        RecordingHttp::new(move |request| {
            let answers = self.0.lock().unwrap();
            answers
                .get(&request.url().to_string())
                .cloned()
                .unwrap_or_else(|| Ok(status(404)))
        })
    }
}

struct Library {
    scratch: Scratch,
    store: Store,
    art: Artwork,
    now: OffsetDateTime,
}

impl Library {
    fn new() -> Self {
        let scratch = Scratch::new("artwork");
        let (store, _) = Store::open(&database_path(scratch.path())).unwrap();
        let art = Artwork::new(scratch.path(), &Log::off());
        Self {
            scratch,
            store,
            art,
            now: NOW,
        }
    }

    fn install(&self, launcher: LauncherId, launcher_game_id: &str, name: &str) -> String {
        let mut games: Vec<InstalledGame> = self
            .store
            .installed_games()
            .unwrap()
            .into_iter()
            .filter(|game| game.launcher == launcher)
            .map(|game| installed(launcher, &game.launcher_game_id, &game.name))
            .collect();
        games.push(installed(launcher, launcher_game_id, name));
        self.store
            .sync_games(launcher, &games, STAMP, || uuid::Uuid::new_v4().to_string())
            .unwrap();
        self.id_of(name)
    }

    fn uninstall_all(&self, launcher: LauncherId) {
        self.store
            .sync_games(launcher, &[], STAMP, || unreachable!())
            .unwrap();
    }

    fn id_of(&self, name: &str) -> String {
        self.store
            .installed_games()
            .unwrap()
            .into_iter()
            .find(|game| game.name == name)
            .unwrap_or_else(|| panic!("{name} is installed"))
            .id
    }

    fn folder(&self, game_id: &str) -> PathBuf {
        self.scratch.join("artwork").join(game_id)
    }

    fn files(&self, game_id: &str) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.folder(game_id))
            .map(|entries| {
                entries
                    .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    fn row(&self, game_id: &str) -> Option<ArtworkRow> {
        self.store.artwork_row(game_id).unwrap()
    }

    fn miss(&self, game_id: &str) -> Option<String> {
        self.store
            .game(game_id)
            .unwrap()
            .and_then(|game| game.artwork_error)
    }

    // Runs the queue dry on this thread, as the backend does across its threads.
    fn run(&mut self, http: &dyn HttpClient, keys: &dyn KeySource) -> Vec<Outcome> {
        let cancel = CancelToken::new();
        let mut outcomes = Vec::new();
        while let Some((id, force)) = self.art.queue.take_next() {
            let outcome = match self.art.prepare(&self.store, &id, force).unwrap() {
                Prepared::Nothing => Outcome::Done,
                Prepared::Lookup(job) => {
                    let fetched = fetch(http, keys, &job.wanted, &cancel);
                    self.art.finish(&self.store, &job, fetched, self.now).0
                }
            };
            self.art.queue.finish(&id, outcome);
            outcomes.push(outcome);
        }
        outcomes
    }

    fn look_up(&mut self, game_id: &str, http: &RecordingHttp, keys: &Keys) -> Outcome {
        assert_eq!(
            self.art
                .refresh(&self.store, Some(game_id), true, self.now)
                .unwrap(),
            1
        );
        self.run(http, keys)[0]
    }
}

fn installed(launcher: LauncherId, launcher_game_id: &str, name: &str) -> InstalledGame {
    InstalledGame {
        launcher,
        launcher_game_id: launcher_game_id.to_owned(),
        name: name.to_owned(),
        install_path: format!(r"D:\Games\{launcher_game_id}"),
        library_path: None,
        size_on_disk_bytes: None,
        last_played_at: None,
        needs_update: false,
    }
}

fn steam_row(game_id: &str) -> ArtworkRow {
    ArtworkRow {
        source: "steamcdn".to_owned(),
        grid_path: Some(format!("{game_id}/grid.png")),
        hero_path: Some(format!("{game_id}/hero.png")),
        logo_path: Some(format!("{game_id}/logo.png")),
        fetched_at: Some(STAMP.to_owned()),
    }
}

#[test]
fn a_steam_game_takes_its_three_pictures_from_steams_servers_and_nothing_else_is_asked() {
    let mut library = Library::new();
    let id = library.install(LauncherId::Steam, "252950", "Rocket League");
    let http = Web::default().steam_pictures("252950").http();
    let keys = Keys::stored();
    assert_eq!(
        library.art.enqueue_after_scan(&library.store, NOW).unwrap(),
        1
    );
    assert_eq!(library.run(&http, &keys), [Outcome::Done]);
    assert_eq!(
        http.urls(),
        [
            format!("{CDN}/252950/library_600x900.jpg"),
            format!("{CDN}/252950/library_hero.jpg"),
            format!("{CDN}/252950/logo.png"),
        ]
    );
    assert!(http.asked().iter().all(|asked| !asked.with_key));
    assert_eq!(keys.asked(), 0, "the key is read only when it is needed");
    assert_eq!(library.row(&id), Some(steam_row(&id)));
    assert_eq!(library.files(&id), ["grid.png", "hero.png", "logo.png"]);
    assert_eq!(
        fs::read(library.folder(&id).join("grid.png")).unwrap(),
        png()
    );
    assert_eq!(library.miss(&id), None);
    assert_eq!(
        library.art.enqueue_after_scan(&library.store, NOW).unwrap(),
        0
    );
}

#[test]
fn a_battlenet_and_a_delisted_epic_game_are_known_by_their_steam_id_without_a_search() {
    let mut library = Library::new();
    let overwatch = library.install(LauncherId::Battlenet, "prometheus", "Overwatch");
    let rocket = library.install(LauncherId::Epic, "Sugar", "Rocket League\u{ae}");
    let steam_id = ol_formats::battlenet::products::known_product("prometheus")
        .and_then(|product| product.steam_app_id)
        .expect("the table has Overwatch's Steam id");
    let http = Web::default()
        .steam_pictures(steam_id)
        .steam_pictures("252950")
        .http();
    library.art.enqueue_after_scan(&library.store, NOW).unwrap();
    assert_eq!(
        library.run(&http, &Keys::none()),
        [Outcome::Done, Outcome::Done]
    );
    assert_eq!(http.urls().len(), 6);
    assert!(http.urls().iter().all(|url| url.starts_with(CDN)));
    assert_eq!(library.row(&overwatch), Some(steam_row(&overwatch)));
    assert_eq!(library.row(&rocket), Some(steam_row(&rocket)));
}

#[test]
fn a_game_without_a_known_id_is_searched_by_name_with_the_electron_apps_two_addresses() {
    let mut library = Library::new();
    let id = library.install(
        LauncherId::Epic,
        "Fortnite",
        "Fall Guys\u{ae}: Ultimate Knockout\u{2122} ",
    );
    let store_search = "https://store.steampowered.com/api/storesearch/?term=Fall%20Guys%3A%20Ultimate%20Knockout&l=english&cc=US";
    let community_search =
        "https://steamcommunity.com/actions/SearchApps/Fall%20Guys%3A%20Ultimate%20Knockout";
    let http = Web::default()
        .with(
            community_search,
            json(
                200,
                r#"[{"appid":"999","name":"Fall Guys Soundtrack"},{"appid":"1097150","name":"fall guys - ultimate knockout"}]"#,
            ),
        )
        .steam_pictures("1097150")
        .http();
    assert_eq!(library.look_up(&id, &http, &Keys::none()), Outcome::Done);
    assert_eq!(
        http.urls()[..3],
        [
            store_search.to_owned(),
            community_search.to_owned(),
            format!("{CDN}/1097150/library_600x900.jpg"),
        ]
    );
    assert_eq!(http.urls().len(), 5);
    assert_eq!(library.row(&id), Some(steam_row(&id)));
}

#[test]
fn only_a_result_with_the_games_own_name_gives_a_steam_id() {
    let store_search = |term: &str| {
        format!("https://store.steampowered.com/api/storesearch/?term={term}&l=english&cc=US")
    };
    for (items, taken) in [
        (
            r#"{"items":[{"type":"app","id":10,"name":"Overwatch 2"}]}"#,
            false,
        ),
        (
            r#"{"items":[{"type":"app","id":10,"name":"Overwatch"}]}"#,
            true,
        ),
        (r#"{"items":[{"id":10,"name":"OVERWATCH\u00ae"}]}"#, true),
        (
            r#"{"items":[{"type":"dlc","id":10,"name":"Overwatch"}]}"#,
            false,
        ),
        (
            r#"{"items":[{"type":"app","id":"ten","name":"Overwatch"}]}"#,
            false,
        ),
        (
            r#"{"items":[{"type":"app","id":-10,"name":"Overwatch"}]}"#,
            false,
        ),
        (
            r#"{"items":[{"type":"app","id":null,"appid":10,"name":"Overwatch"}]}"#,
            true,
        ),
        (
            r#"{"items":[null,7,"x",{"type":"app","name":"Overwatch"}]}"#,
            false,
        ),
        (r#"{"items":{"0":{"id":10,"name":"Overwatch"}}}"#, false),
        (r#"[{"appid":"10","name":"Overwatch"}]"#, true),
        ("null", false),
        ("{}", false),
    ] {
        let mut library = Library::new();
        let id = library.install(LauncherId::Epic, "Some", "Overwatch");
        let http = Web::default()
            .with(store_search("Overwatch"), json(200, items))
            .steam_pictures("10")
            .http();
        let outcome = library.look_up(&id, &http, &Keys::none());
        assert_eq!(outcome == Outcome::Done, taken, "{items}");
        assert_eq!(
            library.miss(&id).as_deref(),
            (!taken).then_some("Not found on Steam; add a SteamGridDB API key in Settings"),
            "{items}"
        );
    }
}

#[test]
fn the_second_steam_host_is_asked_only_after_a_404_and_its_redirect_is_followed_by_hand() {
    let mut library = Library::new();
    let id = library.install(LauncherId::Steam, "440", "Team Fortress 2");
    let landed = "https://shared.steamstatic.com/store_item_assets/steam/apps/440/logo.png";
    let http = Web::default()
        .with(
            format!("{CDN}/440/library_600x900.jpg"),
            picture("image/jpeg", encoded(ImageFormat::Jpeg)),
        )
        .with(format!("{CDN}/440/library_hero.jpg"), status(500))
        .with(format!("{SHARED}/440/logo.png"), redirect(302, landed))
        .with(landed, picture("IMAGE/WebP", encoded(ImageFormat::WebP)))
        .http();
    assert_eq!(library.look_up(&id, &http, &Keys::none()), Outcome::Done);
    assert_eq!(
        http.urls(),
        [
            format!("{CDN}/440/library_600x900.jpg"),
            format!("{CDN}/440/library_hero.jpg"),
            format!("{CDN}/440/logo.png"),
            format!("{SHARED}/440/logo.png"),
            landed.to_owned(),
        ]
    );
    let row = library.row(&id).unwrap();
    assert_eq!(row.grid_path, Some(format!("{id}/grid.jpg")));
    assert_eq!(row.hero_path, None);
    assert_eq!(row.logo_path, Some(format!("{id}/logo.webp")));
    assert_eq!(library.files(&id), ["grid.jpg", "logo.webp"]);
    assert_eq!(library.miss(&id), None);
}

#[test]
fn with_a_key_steamgriddb_is_asked_for_what_steam_lacks_and_the_key_goes_nowhere_else() {
    let mut library = Library::new();
    let id = library.install(LauncherId::Riot, "valorant.live", "VALORANT");
    let autocomplete = format!("{SGDB}/search/autocomplete/VALORANT");
    let grids = format!("{SGDB}/grids/game/5267?dimensions=600x900&{FILTER}");
    let heroes = format!("{SGDB}/heroes/game/5267?{FILTER}");
    let logos = format!("{SGDB}/logos/game/5267?{FILTER}");
    let http = Web::default()
        .with(
            "https://store.steampowered.com/api/storesearch/?term=VALORANT&l=english&cc=US",
            json(200, r#"{"items":[]}"#),
        )
        .with(
            &autocomplete,
            json(200, &recorded("autocomplete-rocket-league.json")),
        )
        .with(&grids, json(200, &recorded("grids-steam-252950.json")))
        .with(&heroes, json(200, &recorded("heroes-game-5267.json")))
        .with(&logos, json(200, &recorded("empty.json")))
        .with(
            "https://cdn2.steamgriddb.com/grid/aaaa1111.png",
            picture("image/png", png()),
        )
        .with(
            "https://cdn2.steamgriddb.com/hero/cccc3333.jpg",
            picture("image/jpeg", encoded(ImageFormat::Jpeg)),
        )
        .http();
    let keys = Keys::stored();
    assert_eq!(library.look_up(&id, &http, &keys), Outcome::Done);
    assert_eq!(keys.asked(), 1);
    assert_eq!(
        http.urls(),
        [
            "https://store.steampowered.com/api/storesearch/?term=VALORANT&l=english&cc=US"
                .to_owned(),
            "https://steamcommunity.com/actions/SearchApps/VALORANT".to_owned(),
            autocomplete,
            grids,
            "https://cdn2.steamgriddb.com/grid/aaaa1111.png".to_owned(),
            heroes,
            "https://cdn2.steamgriddb.com/hero/cccc3333.jpg".to_owned(),
            logos,
        ]
    );
    for asked in http.asked() {
        assert_eq!(asked.with_key, asked.url.starts_with(SGDB), "{}", asked.url);
        assert!(!asked.url.contains(KEY), "{}", asked.url);
        let host = asked.url.split('/').nth(2).unwrap().to_owned();
        assert!(
            LOOKUP_HOSTS
                .into_iter()
                .chain(IMAGE_HOSTS)
                .any(|listed| listed == host),
            "{host}"
        );
        assert!(!host.contains("riot"), "{host}");
    }
    assert_eq!(
        library.row(&id),
        Some(ArtworkRow {
            source: "steamgriddb".to_owned(),
            grid_path: Some(format!("{id}/grid.png")),
            hero_path: Some(format!("{id}/hero.jpg")),
            logo_path: None,
            fetched_at: Some(STAMP.to_owned()),
        })
    );
    assert_eq!(library.miss(&id), None);
}

#[test]
fn with_a_steam_id_steamgriddb_is_asked_by_that_id_and_only_for_the_kinds_still_missing() {
    let mut library = Library::new();
    let id = library.install(LauncherId::Steam, "252950", "Rocket League");
    let grids = format!("{SGDB}/grids/steam/252950?dimensions=600x900&{FILTER}");
    let http = Web::default()
        .with(
            format!("{CDN}/252950/library_hero.jpg"),
            picture("image/png", png()),
        )
        .with(
            format!("{CDN}/252950/logo.png"),
            picture("image/png", png()),
        )
        .with(&grids, json(200, &recorded("grids-steam-252950.json")))
        .with(
            "https://cdn2.steamgriddb.com/grid/aaaa1111.png",
            picture("image/png", png()),
        )
        .http();
    assert_eq!(library.look_up(&id, &http, &Keys::stored()), Outcome::Done);
    let asked = http.urls();
    assert_eq!(
        asked[asked.len() - 2..],
        [
            grids,
            "https://cdn2.steamgriddb.com/grid/aaaa1111.png".to_owned()
        ]
    );
    assert!(
        !asked
            .iter()
            .any(|url| url.contains("autocomplete") || url.contains("/heroes/"))
    );
    // The cover's source names the row, and the kinds are told in the order found.
    assert_eq!(library.row(&id).unwrap().source, "steamgriddb");
}

#[test]
fn what_steamgriddb_refuses_is_told_in_the_electron_apps_words_and_never_with_the_key() {
    let autocomplete = format!("{SGDB}/search/autocomplete/Some%20Game");
    let echoing = format!(r#"{{"success":false,"errors":["Bad key {KEY}","quota",5]}}"#);
    for (answer, miss) in [
        (
            json(401, &recorded("unauthorized.json")),
            "SteamGridDB: the API key was rejected",
        ),
        (status(403), "SteamGridDB: the API key was rejected"),
        (status(429), "SteamGridDB: rate limited, try again later"),
        (status(500), "SteamGridDB: HTTP 500"),
        (
            redirect(302, "https://example.com/"),
            "SteamGridDB: fetch failed",
        ),
        (
            json(200, r#"{"success":false}"#),
            "SteamGridDB: request failed",
        ),
        (json(200, "[]"), "SteamGridDB: request failed"),
        (json(200, &echoing), "SteamGridDB: Bad key [key], quota, 5"),
        (status(404), "SteamGridDB: no match for \"Some Game\""),
        (
            json(200, &recorded("empty.json")),
            "SteamGridDB: no match for \"Some Game\"",
        ),
        (
            json(200, r#"{"success":true,"data":[{"id":"5267"}]}"#),
            "SteamGridDB: no match for \"Some Game\"",
        ),
    ] {
        let mut library = Library::new();
        let id = library.install(LauncherId::Epic, "Some", "Some Game");
        let http = Web::default().with(&autocomplete, answer).http();
        assert_eq!(
            library.look_up(&id, &http, &Keys::stored()),
            Outcome::Failed
        );
        let told = library.miss(&id).unwrap();
        assert_eq!(told, miss);
        assert!(!told.contains(KEY));
        assert_eq!(library.row(&id), None);
    }
    let mut library = Library::new();
    let id = library.install(LauncherId::Epic, "Some", "Some Game");
    let http = Web::default()
        .failing(&autocomplete, HttpError::Timeout)
        .http();
    library.look_up(&id, &http, &Keys::stored());
    assert_eq!(
        library.miss(&id).as_deref(),
        Some("SteamGridDB: The operation was aborted due to timeout")
    );
}

#[test]
fn a_key_the_api_repeats_across_the_cut_is_kept_in_no_part() {
    let autocomplete = format!("{SGDB}/search/autocomplete/Some%20Game");
    for before in ["e".repeat(190), "\u{1f600}".repeat(95)] {
        let echoing = format!(r#"{{"success":false,"errors":["{before}{KEY}"]}}"#);
        let mut library = Library::new();
        let id = library.install(LauncherId::Epic, "Some", "Some Game");
        let http = Web::default()
            .with(&autocomplete, json(200, &echoing))
            .http();
        assert_eq!(
            library.look_up(&id, &http, &Keys::stored()),
            Outcome::Failed
        );
        let told = library.miss(&id).unwrap();
        for most in 4..=KEY.len() {
            assert!(!told.contains(&KEY[..most]), "{told}");
        }
        assert_eq!(told, format!("SteamGridDB: {before}[key]"));
    }
}

#[test]
fn a_picture_steamgriddb_names_on_another_host_is_never_asked_for() {
    let mut library = Library::new();
    let id = library.install(LauncherId::Steam, "7", "Seven");
    let assets = |url: &str| {
        json(
            200,
            &format!(r#"{{"success":true,"data":[{{"url":"{url}"}}]}}"#),
        )
    };
    let http = Web::default()
        .with(
            format!("{SGDB}/grids/steam/7?dimensions=600x900&{FILTER}"),
            assets("https://example.com/grid.png"),
        )
        .with(
            format!("{SGDB}/heroes/steam/7?{FILTER}"),
            assets("http://cdn2.steamgriddb.com/hero.png"),
        )
        .with(
            format!("{SGDB}/logos/steam/7?{FILTER}"),
            assets("https://cdn2.steamgriddb.com:8443/logo.png"),
        )
        .http();
    assert_eq!(
        library.look_up(&id, &http, &Keys::stored()),
        Outcome::Failed
    );
    let told = library.miss(&id).unwrap();
    for kind in ["grid", "hero", "logo"] {
        assert!(
            told.contains(&format!(
                "SteamGridDB {kind}: the image is not on an artwork host"
            )),
            "{told}"
        );
    }
    assert!(
        http.urls()
            .iter()
            .all(|url| !url.contains("example.com") && !url.contains("8443"))
    );
    assert_eq!(http.urls().len(), 6 + 3);
}

#[test]
fn a_redirect_is_followed_three_times_at_most_and_only_among_the_image_hosts() {
    let grid = format!("{CDN}/1/library_600x900.jpg");
    let hop = |n: u32| format!("https://shared.steamstatic.com/hop/{n}.png");
    let cases: Vec<(Web, &str, usize)> = vec![
        (
            Web::default().with(&grid, redirect(302, "https://example.com/grid.png")),
            "Steam CDN grid: redirected away from the artwork hosts",
            1,
        ),
        (
            Web::default().with(
                &grid,
                redirect(301, "http://shared.steamstatic.com/grid.png"),
            ),
            "Steam CDN grid: redirected away from the artwork hosts",
            1,
        ),
        (
            Web::default().with(
                &grid,
                redirect(307, "https://www.steamgriddb.com/api/v2/grids/steam/1"),
            ),
            "Steam CDN grid: redirected away from the artwork hosts",
            1,
        ),
        (
            Web::default().with(&grid, redirect(308, "https://127.0.0.1/grid.png")),
            "Steam CDN grid: redirected away from the artwork hosts",
            1,
        ),
        (
            Web::default().with(&grid, status(302)),
            "Steam CDN grid: HTTP 302",
            1,
        ),
        (
            Web::default()
                .with(&grid, redirect(302, &hop(1)))
                .with(hop(1), redirect(303, &hop(2)))
                .with(hop(2), redirect(307, &hop(3)))
                .with(hop(3), redirect(308, &hop(4)))
                .with(hop(4), picture("image/png", png())),
            "Steam CDN grid: too many redirects",
            4,
        ),
    ];
    for (web, miss, asked_for_grid) in cases {
        let mut library = Library::new();
        let id = library.install(LauncherId::Steam, "1", "One");
        let http = web.http();
        library.look_up(&id, &http, &Keys::none());
        let told = library.miss(&id).unwrap();
        assert!(told.starts_with(miss), "{told}");
        let grids = http
            .urls()
            .iter()
            .take_while(|url| !url.contains("library_hero"))
            .count();
        assert_eq!(grids, asked_for_grid, "{miss}");
        assert!(
            http.urls()
                .iter()
                .all(|url| !url.contains("example.com") && !url.contains("127.0.0.1"))
        );
    }
    let mut library = Library::new();
    let id = library.install(LauncherId::Steam, "1", "One");
    let http = Web::default()
        .with(&grid, redirect(302, &hop(1)))
        .with(hop(1), redirect(302, "/hop/2.png"))
        .with(hop(2), redirect(302, "3.png"))
        .with(hop(3), picture("image/png", png()))
        .http();
    assert_eq!(library.look_up(&id, &http, &Keys::none()), Outcome::Done);
    assert_eq!(
        library.row(&id).unwrap().grid_path,
        Some(format!("{id}/grid.png"))
    );
}

#[test]
fn a_download_that_is_not_a_whole_picture_within_the_limits_is_not_kept() {
    let grid = format!("{CDN}/1/library_600x900.jpg");
    let mut cut_short = png();
    cut_short.truncate(60);
    let mut header_off_its_checksum = png();
    header_off_its_checksum[16..24].copy_from_slice(&[0, 1, 0x86, 0xa0, 0, 1, 0x86, 0xa0]);
    let mut renamed = b"GIF89a".to_vec();
    renamed.extend([0; 64]);
    let over = HttpResponse {
        body: Err(TooLarge::Declared(MAX_DOWNLOAD_BYTES + 1)),
        ..picture("image/png", Vec::new())
    };
    let cases = [
        (
            picture("text/html", png()),
            "Steam CDN grid: not an image (text/html)".to_owned(),
        ),
        (
            HttpResponse {
                content_type: None,
                ..picture("", png())
            },
            "Steam CDN grid: not an image (no content type)".to_owned(),
        ),
        (
            over,
            format!(
                "Steam CDN grid: too large ({} bytes)",
                MAX_DOWNLOAD_BYTES + 1
            ),
        ),
        (
            picture("image/png", vec![0; MAX_DOWNLOAD_BYTES as usize + 1]),
            format!("Steam CDN grid: too large (over {MAX_DOWNLOAD_BYTES} bytes)"),
        ),
        (
            picture("image/gif", renamed),
            "Steam CDN grid: unrecognised image data".to_owned(),
        ),
        (
            picture("image/png", b"<html>".to_vec()),
            "Steam CDN grid: unrecognised image data".to_owned(),
        ),
        (
            picture("image/png", cut_short),
            "Steam CDN grid: unrecognised image data".to_owned(),
        ),
        (
            picture("image/png", header_off_its_checksum),
            "Steam CDN grid: unrecognised image data".to_owned(),
        ),
        (
            picture("image/png", solid_png(8193, 1, [200, 60, 70])),
            "Steam CDN grid: unrecognised image data".to_owned(),
        ),
        (
            picture("image/png", solid_png(1, 8193, [200, 60, 70])),
            "Steam CDN grid: unrecognised image data".to_owned(),
        ),
        (status(503), "Steam CDN grid: HTTP 503".to_owned()),
    ];
    for (answer, miss) in cases {
        let mut library = Library::new();
        let id = library.install(LauncherId::Steam, "1", "One");
        let http = Web::default().with(&grid, answer).http();
        assert_eq!(
            library.look_up(&id, &http, &Keys::none()),
            Outcome::Failed,
            "{miss}"
        );
        let told = library.miss(&id).unwrap();
        assert!(told.starts_with(&miss), "{told}");
        assert_eq!(library.files(&id), Vec::<String>::new(), "{miss}");
        assert_eq!(library.row(&id), None);
    }
}

#[test]
fn a_picture_as_wide_or_as_tall_as_the_library_draws_is_kept() {
    for (width, height) in [(8192, 1), (1, 8192)] {
        let mut library = Library::new();
        let id = library.install(LauncherId::Steam, "1", "One");
        let http = Web::default()
            .with(
                format!("{CDN}/1/library_600x900.jpg"),
                picture("image/png", solid_png(width, height, [200, 60, 70])),
            )
            .http();
        assert_eq!(
            library.look_up(&id, &http, &Keys::none()),
            Outcome::Done,
            "{width}x{height}"
        );
        assert_eq!(
            library.row(&id).unwrap().grid_path,
            Some(format!("{id}/grid.png"))
        );
    }
}

#[test]
fn a_search_answer_past_its_limit_or_not_json_is_a_reason_and_the_next_search_is_asked() {
    let mut library = Library::new();
    let id = library.install(LauncherId::Epic, "Some", "Some Game");
    let store_search =
        "https://store.steampowered.com/api/storesearch/?term=Some%20Game&l=english&cc=US";
    let http = Web::default()
        .with(
            store_search,
            json(200, &"x".repeat(MAX_JSON_BYTES as usize + 1)),
        )
        .http();
    library.look_up(&id, &http, &Keys::none());
    assert_eq!(
        library.miss(&id).unwrap(),
        format!(
            "Steam search: too large (over {MAX_JSON_BYTES} bytes); Not found on Steam; add a SteamGridDB API key in Settings"
        )
    );
    assert_eq!(http.urls().len(), 2);

    let http = Web::default()
        .with(store_search, json(200, "<!DOCTYPE html>"))
        .failing(
            "https://steamcommunity.com/actions/SearchApps/Some%20Game",
            HttpError::Failed("the connection failed"),
        )
        .http();
    library.look_up(&id, &http, &Keys::none());
    assert_eq!(
        library.miss(&id).unwrap(),
        "Steam search: fetch failed; Not found on Steam; add a SteamGridDB API key in Settings"
    );
    let http = Web::default()
        .with(store_search, redirect(301, "https://example.com/"))
        .http();
    library.look_up(&id, &http, &Keys::none());
    assert!(
        library
            .miss(&id)
            .unwrap()
            .starts_with("Steam search: fetch failed")
    );
    assert!(http.urls().iter().all(|url| !url.contains("example.com")));
}

#[test]
fn a_miss_is_not_asked_again_by_itself_for_seven_days() {
    let mut library = Library::new();
    let id = library.install(LauncherId::Steam, "1", "One");
    let http = Web::default().http();
    library.art.enqueue_after_scan(&library.store, NOW).unwrap();
    assert_eq!(library.run(&http, &Keys::none()), [Outcome::Failed]);
    assert_eq!(
        library.miss(&id).as_deref(),
        Some(
            "Steam CDN grid: HTTP 404; Steam CDN hero: HTTP 404; Steam CDN logo: HTTP 404; No SteamGridDB API key in Settings"
        )
    );
    let later = |days: i64, seconds: i64| {
        NOW + time::Duration::days(days) + time::Duration::seconds(seconds)
    };
    assert_eq!(
        library
            .art
            .enqueue_after_scan(&library.store, later(6, 86_399))
            .unwrap(),
        0
    );
    assert_eq!(
        library
            .art
            .refresh(&library.store, None, false, later(1, 0))
            .unwrap(),
        1
    );
    library.now = later(1, 0);
    library.run(&http, &Keys::none());
    assert_eq!(
        library.store.artwork_attempt(&id).unwrap().as_deref(),
        Some("2026-11-23T10:00:00.000Z")
    );
    assert_eq!(
        library
            .art
            .enqueue_after_scan(&library.store, later(7, 86_399))
            .unwrap(),
        0
    );
    assert_eq!(
        library
            .art
            .enqueue_after_scan(&library.store, later(8, 0))
            .unwrap(),
        1
    );
    library.now = later(8, 0);
    library.run(&http, &Keys::none());

    library
        .store
        .set_setting(SettingKey::ArtworkAutoFetch, "false")
        .unwrap();
    assert_eq!(
        library
            .art
            .enqueue_after_scan(&library.store, later(30, 0))
            .unwrap(),
        0
    );
    assert_eq!(
        library
            .art
            .refresh(&library.store, None, false, later(30, 0))
            .unwrap(),
        1
    );
}

#[test]
fn a_key_that_cannot_be_read_ends_the_lookup_with_nothing_kept() {
    let mut library = Library::new();
    let id = library.install(LauncherId::Steam, "1", "One");
    let http = Web::default()
        .with(
            format!("{CDN}/1/library_600x900.jpg"),
            picture("image/png", png()),
        )
        .http();
    let keys = Keys::holding(Err("the vault cannot read it"));
    assert_eq!(library.look_up(&id, &http, &keys), Outcome::Failed);
    assert_eq!(library.row(&id), None);
    assert_eq!(library.miss(&id), None);
    assert_eq!(library.files(&id), Vec::<String>::new());
}

#[test]
fn a_stopped_lookup_asks_nothing_more() {
    let library = Library::new();
    let id = library.install(LauncherId::Epic, "Some", "Some Game");
    library.art.prepare(&library.store, &id, false).unwrap();
    let Prepared::Lookup(job) = library.art.prepare(&library.store, &id, false).unwrap() else {
        panic!("a game without pictures is looked up");
    };
    let http = Web::default().http();
    let cancel = CancelToken::new();
    cancel.cancel();
    let keys = Keys::stored();
    let fetched = fetch(&http, &keys, &job.wanted, &cancel);
    assert!(fetched.found.is_empty());
    assert_eq!(http.urls(), Vec::<String>::new());
    assert_eq!(keys.asked(), 0);
}

#[test]
fn a_picture_of_the_users_own_is_checked_kept_and_not_looked_up_over() {
    let mut library = Library::new();
    let id = library.install(LauncherId::Steam, "252950", "Rocket League");
    let other = library.install(LauncherId::Steam, "440", "Team Fortress 2");
    let chosen = library.scratch.join("chosen.jpg");
    fs::write(&chosen, encoded(ImageFormat::Jpeg)).unwrap();
    let game = library
        .art
        .set_manual(&library.store, &id, ArtworkKind::Grid, &chosen, NOW)
        .unwrap();
    let art = game.artwork.unwrap();
    assert_eq!(art.source, ArtworkSource::Manual);
    assert_eq!(art.grid, Some(format!("{id}/grid.jpg")));
    assert_eq!(library.files(&id), ["grid.jpg"]);

    let http = Web::default()
        .steam_pictures("252950")
        .steam_pictures("440")
        .http();
    assert_eq!(
        library.art.enqueue_after_scan(&library.store, NOW).unwrap(),
        1
    );
    assert_eq!(
        library
            .art
            .refresh(&library.store, None, true, NOW)
            .unwrap(),
        0
    );
    assert_eq!(
        library
            .art
            .refresh(&library.store, Some(&id), false, NOW)
            .unwrap(),
        1
    );
    library.run(&http, &Keys::none());
    assert!(http.urls().iter().all(|url| url.contains("/440/")));
    assert_eq!(library.row(&id).unwrap().source, "manual");
    assert_eq!(library.row(&other), Some(steam_row(&other)));

    assert_eq!(library.look_up(&id, &http, &Keys::none()), Outcome::Done);
    assert_eq!(library.row(&id), Some(steam_row(&id)));
    assert_eq!(library.files(&id), ["grid.png", "hero.png", "logo.png"]);
}

#[test]
fn a_file_that_is_too_large_or_no_picture_is_refused_as_the_users_own() {
    let mut library = Library::new();
    let id = library.install(LauncherId::Steam, "252950", "Rocket League");
    let refused = |library: &mut Library, name: &str, content: &[u8]| {
        let file = library.scratch.join(name);
        fs::write(&file, content).unwrap();
        library
            .art
            .set_manual(&library.store, &id, ArtworkKind::Grid, &file, NOW)
            .map(|_| ())
            .unwrap_err()
    };
    let mut cut_short = png();
    cut_short.truncate(60);
    for (name, content) in [
        ("notes.png", b"just some text, long enough".to_vec()),
        ("cut.png", cut_short),
        ("empty.png", Vec::new()),
        ("wide.png", solid_png(8193, 1, [200, 60, 70])),
    ] {
        let error = refused(&mut library, name, &content);
        assert!(matches!(error, ArtworkError::BadImage), "{name}");
        assert_eq!(error.code(), "BAD_IMAGE");
        assert_eq!(
            error.to_string(),
            "That file is not a JPEG, PNG, or WebP image"
        );
    }
    let large = refused(
        &mut library,
        "large.png",
        &vec![0; MAX_MANUAL_BYTES as usize + 1],
    );
    assert_eq!(large.code(), "TOO_LARGE");
    assert_eq!(large.to_string(), "That image is larger than 20 MB");
    let missing = library
        .art
        .set_manual(
            &library.store,
            &id,
            ArtworkKind::Grid,
            &library.scratch.join("gone.png"),
            NOW,
        )
        .unwrap_err();
    assert!(matches!(missing, ArtworkError::Unreadable(_)));
    assert!(!missing.to_string().contains("gone.png"));
    let unknown = library
        .art
        .set_manual(
            &library.store,
            "no-such-game",
            ArtworkKind::Grid,
            &library.scratch.join("gone.png"),
            NOW,
        )
        .unwrap_err();
    assert_eq!(unknown.code(), "NOT_FOUND");
    assert_eq!(library.row(&id), None);
    assert_eq!(library.files(&id), Vec::<String>::new());
}

#[test]
fn a_lookup_the_users_own_picture_overtook_writes_nothing() {
    let mut library = Library::new();
    let id = library.install(LauncherId::Steam, "252950", "Rocket League");
    library
        .art
        .refresh(&library.store, Some(&id), true, NOW)
        .unwrap();
    let (queued, force) = library.art.queue.take_next().unwrap();
    let Prepared::Lookup(job) = library.art.prepare(&library.store, &queued, force).unwrap() else {
        panic!("a game without pictures is looked up");
    };
    let http = Web::default().steam_pictures("252950").http();
    let fetched = fetch(&http, &Keys::none(), &job.wanted, &CancelToken::new());
    assert_eq!(fetched.found.len(), 3);

    let chosen = library.scratch.join("chosen.png");
    fs::write(&chosen, png()).unwrap();
    library
        .art
        .set_manual(&library.store, &id, ArtworkKind::Grid, &chosen, NOW)
        .unwrap();
    assert_eq!(
        library.art.finish(&library.store, &job, fetched, NOW),
        (Outcome::Done, false)
    );
    let row = library.row(&id).unwrap();
    assert_eq!((row.source.as_str(), row.hero_path), ("manual", None));
    assert_eq!(library.files(&id), ["grid.png"]);
}

#[test]
fn a_picture_of_the_users_own_chosen_while_a_refresh_of_all_waits_is_kept() {
    let mut library = Library::new();
    let id = library.install(LauncherId::Steam, "252950", "Rocket League");
    let other = library.install(LauncherId::Steam, "440", "Team Fortress 2");
    assert_eq!(
        library
            .art
            .refresh(&library.store, None, true, NOW)
            .unwrap(),
        2
    );
    let chosen = library.scratch.join("chosen.jpg");
    fs::write(&chosen, encoded(ImageFormat::Jpeg)).unwrap();
    library
        .art
        .set_manual(&library.store, &id, ArtworkKind::Grid, &chosen, NOW)
        .unwrap();

    let http = Web::default()
        .steam_pictures("252950")
        .steam_pictures("440")
        .http();
    library.run(&http, &Keys::none());
    assert!(http.urls().iter().all(|url| url.contains("/440/")));
    let row = library.row(&id).unwrap();
    assert_eq!(
        (row.source.as_str(), row.grid_path),
        ("manual", Some(format!("{id}/grid.jpg")))
    );
    assert_eq!(library.files(&id), ["grid.jpg"]);
    assert_eq!(library.row(&other), Some(steam_row(&other)));
}

#[test]
fn a_refresh_of_one_game_that_waits_behind_a_refresh_of_all_still_replaces_the_users_own() {
    let mut library = Library::new();
    let id = library.install(LauncherId::Steam, "252950", "Rocket League");
    library.install(LauncherId::Steam, "440", "Team Fortress 2");
    assert_eq!(
        library
            .art
            .refresh(&library.store, None, true, NOW)
            .unwrap(),
        2
    );
    let chosen = library.scratch.join("chosen.jpg");
    fs::write(&chosen, encoded(ImageFormat::Jpeg)).unwrap();
    library
        .art
        .set_manual(&library.store, &id, ArtworkKind::Grid, &chosen, NOW)
        .unwrap();
    assert_eq!(
        library
            .art
            .refresh(&library.store, Some(&id), true, NOW)
            .unwrap(),
        0
    );

    let http = Web::default()
        .steam_pictures("252950")
        .steam_pictures("440")
        .http();
    library.run(&http, &Keys::none());
    assert_eq!(library.row(&id), Some(steam_row(&id)));
    assert_eq!(library.files(&id), ["grid.png", "hero.png", "logo.png"]);
}

#[test]
fn after_a_quit_a_lookup_on_its_way_writes_nothing() {
    let mut library = Library::new();
    let id = library.install(LauncherId::Steam, "252950", "Rocket League");
    library
        .art
        .refresh(&library.store, Some(&id), false, NOW)
        .unwrap();
    let (queued, force) = library.art.queue.take_next().unwrap();
    let Prepared::Lookup(job) = library.art.prepare(&library.store, &queued, force).unwrap() else {
        panic!("a game without pictures is looked up");
    };
    let http = Web::default().steam_pictures("252950").http();
    let fetched = fetch(&http, &Keys::none(), &job.wanted, &CancelToken::new());
    library.art.queue.stop();
    assert_eq!(
        library.art.finish(&library.store, &job, fetched, NOW),
        (Outcome::Done, false)
    );
    assert_eq!(library.row(&id), None);
    assert_eq!(library.files(&id), Vec::<String>::new());
    assert_eq!(
        library
            .art
            .refresh(&library.store, Some(&id), true, NOW)
            .unwrap(),
        0
    );
}

#[test]
fn only_what_is_missing_on_disk_is_asked_for_again_and_a_kind_keeps_one_file() {
    let mut library = Library::new();
    let id = library.install(LauncherId::Steam, "252950", "Rocket League");
    let http = Web::default().steam_pictures("252950").http();
    library.look_up(&id, &http, &Keys::none());
    fs::remove_file(library.folder(&id).join("hero.png")).unwrap();
    fs::write(
        library.folder(&id).join("hero.jpg.1234.tmp"),
        b"left by a crash",
    )
    .unwrap();

    let jpeg = Web::default()
        .with(
            format!("{CDN}/252950/library_hero.jpg"),
            picture("image/jpeg", encoded(ImageFormat::Jpeg)),
        )
        .http();
    assert_eq!(
        library
            .art
            .refresh(&library.store, Some(&id), false, NOW)
            .unwrap(),
        1
    );
    assert_eq!(library.run(&jpeg, &Keys::none()), [Outcome::Done]);
    assert_eq!(jpeg.urls(), [format!("{CDN}/252950/library_hero.jpg")]);
    let row = library.row(&id).unwrap();
    assert_eq!(row.hero_path, Some(format!("{id}/hero.jpg")));
    assert_eq!(row.grid_path, Some(format!("{id}/grid.png")));
    assert_eq!(
        library.files(&id),
        ["grid.png", "hero.jpg", "hero.jpg.1234.tmp", "logo.png"]
    );

    fs::write(library.folder(&id).join("grid.webp"), b"stale").unwrap();
    library.look_up(&id, &http, &Keys::none());
    assert_eq!(
        library.files(&id),
        ["grid.png", "hero.jpg.1234.tmp", "hero.png", "logo.png"]
    );
}

#[test]
fn a_refresh_that_cannot_write_every_picture_leaves_the_row_and_its_files_as_they_were() {
    let mut library = Library::new();
    let id = library.install(LauncherId::Steam, "252950", "Rocket League");
    library.look_up(
        &id,
        &Web::default().steam_pictures("252950").http(),
        &Keys::none(),
    );
    fs::create_dir(library.folder(&id).join("hero.jpg")).unwrap();
    let jpeg = Web::default()
        .with(
            format!("{CDN}/252950/library_600x900.jpg"),
            picture("image/jpeg", encoded(ImageFormat::Jpeg)),
        )
        .with(
            format!("{CDN}/252950/library_hero.jpg"),
            picture("image/jpeg", encoded(ImageFormat::Jpeg)),
        )
        .http();
    assert_eq!(library.look_up(&id, &jpeg, &Keys::none()), Outcome::Failed);
    let row = library.row(&id).unwrap();
    assert_eq!(row, steam_row(&id));
    for named in [row.grid_path, row.hero_path, row.logo_path] {
        let named = named.unwrap();
        assert!(
            library.scratch.join("artwork").join(&named).is_file(),
            "{named}"
        );
    }
    assert_eq!(
        fs::read(library.folder(&id).join("grid.png")).unwrap(),
        png()
    );
    assert_eq!(
        library.files(&id),
        ["grid.png", "hero.jpg", "hero.png", "logo.png"]
    );
}

#[test]
fn a_launchers_icon_an_earlier_version_stored_stands_in_until_a_cover_is_found() {
    let mut library = Library::new();
    let id = library.install(LauncherId::Riot, "valorant.live", "VALORANT");
    fs::create_dir_all(library.folder(&id)).unwrap();
    fs::write(library.folder(&id).join("grid.ico"), b"an icon").unwrap();
    let stand_in = ArtworkRow {
        source: "launcher".to_owned(),
        grid_path: Some(format!("{id}/grid.ico")),
        hero_path: None,
        logo_path: None,
        fetched_at: Some("2026-11-01T00:00:00.000Z".to_owned()),
    };
    library.store.put_artwork(&id, &stand_in).unwrap();

    assert_eq!(
        library.art.enqueue_after_scan(&library.store, NOW).unwrap(),
        1
    );
    assert_eq!(
        library.run(&Web::default().http(), &Keys::none()),
        [Outcome::Failed]
    );
    assert_eq!(library.row(&id), Some(stand_in.clone()));
    assert_eq!(library.files(&id), ["grid.ico"]);
    assert_eq!(
        library.miss(&id).as_deref(),
        Some("Not found on Steam; add a SteamGridDB API key in Settings")
    );

    let hero_only = Web::default()
        .with(
            format!("{SGDB}/search/autocomplete/VALORANT"),
            json(200, &recorded("autocomplete-rocket-league.json")),
        )
        .with(
            format!("{SGDB}/heroes/game/5267?{FILTER}"),
            json(200, &recorded("heroes-game-5267.json")),
        )
        .with(
            "https://cdn2.steamgriddb.com/hero/cccc3333.jpg",
            picture("image/png", png()),
        )
        .http();
    assert_eq!(
        library
            .art
            .refresh(&library.store, None, false, NOW)
            .unwrap(),
        1
    );
    assert_eq!(library.run(&hero_only, &Keys::stored()), [Outcome::Done]);
    let row = library.row(&id).unwrap();
    assert_eq!(row.source, "launcher");
    assert_eq!(row.grid_path, stand_in.grid_path);
    assert_eq!(row.hero_path, Some(format!("{id}/hero.png")));
    assert_eq!(
        library.miss(&id).as_deref(),
        Some("SteamGridDB: no grid; SteamGridDB: no logo")
    );

    let cover = Web::default()
        .with(
            format!("{SGDB}/search/autocomplete/VALORANT"),
            json(200, &recorded("autocomplete-rocket-league.json")),
        )
        .with(
            format!("{SGDB}/grids/game/5267?dimensions=600x900&{FILTER}"),
            json(200, &recorded("grids-steam-252950.json")),
        )
        .with(
            "https://cdn2.steamgriddb.com/grid/aaaa1111.png",
            picture("image/png", png()),
        )
        .http();
    assert_eq!(
        library
            .art
            .refresh(&library.store, None, false, NOW)
            .unwrap(),
        1
    );
    assert_eq!(library.run(&cover, &Keys::stored()), [Outcome::Done]);
    let row = library.row(&id).unwrap();
    assert_eq!(row.source, "steamgriddb");
    assert_eq!(row.grid_path, Some(format!("{id}/grid.png")));
    assert_eq!(row.hero_path, Some(format!("{id}/hero.png")));
    assert_eq!(library.files(&id), ["grid.png", "hero.png"]);
    assert_eq!(library.miss(&id), None);
}

fn steam_hero_only(app_id: &str) -> RecordingHttp {
    Web::default()
        .with(
            format!("{CDN}/{app_id}/library_hero.jpg"),
            picture("image/jpeg", encoded(ImageFormat::Jpeg)),
        )
        .http()
}

#[test]
fn a_forced_refresh_that_finds_no_cover_keeps_the_one_there() {
    let mut library = Library::new();
    let id = library.install(LauncherId::Steam, "252950", "Rocket League");
    let http = Web::default().steam_pictures("252950").http();
    library.look_up(&id, &http, &Keys::none());
    assert_eq!(library.row(&id), Some(steam_row(&id)));

    assert_eq!(
        library.look_up(&id, &steam_hero_only("252950"), &Keys::none()),
        Outcome::Done
    );
    assert_eq!(
        library.row(&id),
        Some(ArtworkRow {
            hero_path: Some(format!("{id}/hero.jpg")),
            ..steam_row(&id)
        })
    );
    assert_eq!(library.files(&id), ["grid.png", "hero.jpg", "logo.png"]);
    assert_eq!(library.miss(&id), None);
}

#[test]
fn a_forced_refresh_keeps_the_users_own_cover_when_only_a_hero_comes_back() {
    let mut library = Library::new();
    let id = library.install(LauncherId::Steam, "252950", "Rocket League");
    let chosen = library.scratch.join("chosen.jpg");
    fs::write(&chosen, encoded(ImageFormat::Jpeg)).unwrap();
    library
        .art
        .set_manual(&library.store, &id, ArtworkKind::Grid, &chosen, NOW)
        .unwrap();

    assert_eq!(
        library.look_up(&id, &steam_hero_only("252950"), &Keys::none()),
        Outcome::Done
    );
    let row = library.row(&id).unwrap();
    assert_eq!(row.source, "manual");
    assert_eq!(row.grid_path, Some(format!("{id}/grid.jpg")));
    assert_eq!(row.hero_path, Some(format!("{id}/hero.jpg")));
    assert_eq!(library.files(&id), ["grid.jpg", "hero.jpg"]);
    assert_eq!(
        library
            .art
            .refresh(&library.store, None, true, NOW)
            .unwrap(),
        0,
        "a refresh of all still leaves the user's own picture alone"
    );
}

#[test]
fn a_forced_refresh_keeps_a_stand_in_until_a_cover_is_found() {
    let mut library = Library::new();
    let id = library.install(LauncherId::Riot, "valorant.live", "VALORANT");
    fs::create_dir_all(library.folder(&id)).unwrap();
    fs::write(library.folder(&id).join("grid.ico"), b"an icon").unwrap();
    library
        .store
        .put_artwork(
            &id,
            &ArtworkRow {
                source: "launcher".to_owned(),
                grid_path: Some(format!("{id}/grid.ico")),
                hero_path: None,
                logo_path: None,
                fetched_at: Some("2026-11-01T00:00:00.000Z".to_owned()),
            },
        )
        .unwrap();
    let autocomplete = format!("{SGDB}/search/autocomplete/VALORANT");
    let hero_only = Web::default()
        .with(
            &autocomplete,
            json(200, &recorded("autocomplete-rocket-league.json")),
        )
        .with(
            format!("{SGDB}/heroes/game/5267?{FILTER}"),
            json(200, &recorded("heroes-game-5267.json")),
        )
        .with(
            "https://cdn2.steamgriddb.com/hero/cccc3333.jpg",
            picture("image/png", png()),
        )
        .http();
    assert_eq!(
        library.look_up(&id, &hero_only, &Keys::stored()),
        Outcome::Done
    );
    let row = library.row(&id).unwrap();
    assert_eq!(row.source, "launcher");
    assert_eq!(row.grid_path, Some(format!("{id}/grid.ico")));
    assert_eq!(row.hero_path, Some(format!("{id}/hero.png")));
    assert_eq!(library.files(&id), ["grid.ico", "hero.png"]);
    assert_eq!(
        library.miss(&id).as_deref(),
        Some("SteamGridDB: no grid; SteamGridDB: no logo")
    );

    let cover = Web::default()
        .with(
            &autocomplete,
            json(200, &recorded("autocomplete-rocket-league.json")),
        )
        .with(
            format!("{SGDB}/grids/game/5267?dimensions=600x900&{FILTER}"),
            json(200, &recorded("grids-steam-252950.json")),
        )
        .with(
            "https://cdn2.steamgriddb.com/grid/aaaa1111.png",
            picture("image/png", png()),
        )
        .http();
    assert_eq!(library.look_up(&id, &cover, &Keys::stored()), Outcome::Done);
    let row = library.row(&id).unwrap();
    assert_eq!(row.source, "steamgriddb");
    assert_eq!(row.grid_path, Some(format!("{id}/grid.png")));
    assert_eq!(row.hero_path, Some(format!("{id}/hero.png")));
    assert_eq!(library.files(&id), ["grid.png", "hero.png"]);
    assert_eq!(library.miss(&id), None);
}

#[test]
fn the_cache_of_a_game_that_is_gone_is_removed_and_nothing_else() {
    let mut library = Library::new();
    let kept = library.install(LauncherId::Steam, "252950", "Rocket League");
    let gone = library.install(LauncherId::Epic, "Some", "Some Game");
    let http = Web::default().steam_pictures("252950").http();
    library
        .art
        .refresh(&library.store, None, false, NOW)
        .unwrap();
    library.run(&http, &Keys::none());
    fs::create_dir_all(library.folder(&gone)).unwrap();
    fs::write(library.folder(&gone).join("grid.png"), png()).unwrap();
    let stray = library.scratch.join("artwork").join("notes.txt");
    fs::write(&stray, b"a file is left alone").unwrap();
    assert!(library.miss(&gone).is_some());

    assert_eq!(library.art.prune(&library.store).unwrap(), 0);
    library.uninstall_all(LauncherId::Epic);
    library
        .art
        .queue
        .enqueue(std::slice::from_ref(&gone), false);
    assert_eq!(
        library.art.prune(&library.store).unwrap(),
        0,
        "a queued game stays"
    );
    library.run(&http, &Keys::none());
    assert_eq!(library.art.prune(&library.store).unwrap(), 1);
    assert!(!library.folder(&gone).exists());
    assert!(library.store.artwork_attempt(&gone).unwrap().is_none());
    assert!(stray.is_file());
    assert_eq!(library.row(&kept), Some(steam_row(&kept)));
    assert_eq!(library.files(&kept), ["grid.png", "hero.png", "logo.png"]);
    assert_eq!(library.art.prune(&library.store).unwrap(), 0);
}

#[test]
fn a_game_whose_id_is_no_folder_name_of_the_cache_gets_no_file() {
    let mut library = Library::new();
    library
        .store
        .sync_games(
            LauncherId::Steam,
            &[installed(LauncherId::Steam, "252950", "Rocket League")],
            STAMP,
            || r"..\..\outside".to_owned(),
        )
        .unwrap();
    let id = library.id_of("Rocket League");
    let http = Web::default().steam_pictures("252950").http();
    assert_eq!(library.look_up(&id, &http, &Keys::none()), Outcome::Failed);
    assert_eq!(library.row(&id), None);
    assert!(!library.scratch.join("outside").exists());
    assert!(
        !library
            .scratch
            .path()
            .parent()
            .unwrap()
            .join("outside")
            .exists()
    );
}

#[test]
fn a_refresh_of_a_game_that_does_not_exist_is_refused() {
    let mut library = Library::new();
    let error = library
        .art
        .refresh(&library.store, Some("no-such-game"), true, NOW)
        .unwrap_err();
    assert_eq!(error.code(), "NOT_FOUND");
    assert_eq!(error.to_string(), "Game no-such-game does not exist");
}

// Every recorded case of the Electron app's sniffImage.
#[test]
fn bytes_are_told_apart_as_the_electron_app_tells_them() {
    let vectors = Vectors::load("l1/core/sniffImage.json");
    for case in &vectors.cases {
        let sniffed = sniff_image(&bytes(&case.args[0])).map(|ext| ext.as_str().to_owned());
        let expected = case.out().and_then(|out| out.as_str().map(str::to_owned));
        assert_eq!(sniffed, expected, "{}", case.id);
    }
    assert_eq!(vectors.cases.len(), 43);
    assert_eq!(
        sniff_image(&encoded(ImageFormat::Jpeg)).map(|ext| ext.as_str()),
        Some("jpg")
    );
    assert_eq!(
        sniff_image(&encoded(ImageFormat::WebP)).map(|ext| ext.as_str()),
        Some("webp")
    );
    assert_eq!(sniff_image(&png()).map(|ext| ext.as_str()), Some("png"));
    assert_eq!(KEY_HOST, "www.steamgriddb.com");
}

// Made up in the shape of what Steam's servers answer for a picture a game never had: one gray, a lighter line at the edge, and for the logo an edge a little transparent.
fn gray_cover() -> DynamicImage {
    DynamicImage::ImageLuma8(GrayImage::from_fn(30, 45, |x, y| {
        let edge = x == 0 || y == 0 || x == 29 || y == 44;
        Luma([if edge { 88 } else { 75 }])
    }))
}

fn gray_logo() -> DynamicImage {
    DynamicImage::ImageLumaA8(GrayAlphaImage::from_fn(64, 36, |x, y| {
        let edge = x == 0 || y == 0 || x == 63 || y == 35;
        LumaA([75, if edge { 248 } else { 255 }])
    }))
}

fn bytes_of(picture: &DynamicImage, format: ImageFormat) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    picture.write_to(&mut bytes, format).unwrap();
    bytes.into_inner()
}

fn judged(bytes: &[u8]) -> bool {
    let ext = sniff_image(bytes).unwrap();
    is_steam_placeholder(&decode(bytes, ext).unwrap())
}

const APP: &str = "2000000";
const LANDED: &str = "https://shared.steamstatic.com/store_item_assets/steam/apps/2000000";

// Steam's two hosts answer the cover and the logo with the placeholder, the second through its redirect, and the hero with a picture.
fn steam_placeholders() -> Web {
    let cover = picture("image/jpeg", bytes_of(&gray_cover(), ImageFormat::Jpeg));
    let logo = picture("image/png", bytes_of(&gray_logo(), ImageFormat::Png));
    let mut web = Web::default().with(
        format!("{CDN}/{APP}/library_hero.jpg"),
        picture("image/png", png()),
    );
    for (file, answer) in [("library_600x900.jpg", cover), ("logo.png", logo)] {
        web = web
            .with(format!("{CDN}/{APP}/{file}"), answer.clone())
            .with(
                format!("{SHARED}/{APP}/{file}"),
                redirect(301, &format!("{LANDED}/{file}")),
            )
            .with(format!("{LANDED}/{file}"), answer);
    }
    web
}

#[test]
fn steams_placeholder_is_one_gray_and_a_picture_is_not() {
    let noisy = DynamicImage::ImageRgb8(RgbImage::from_fn(30, 45, |x, y| {
        let wobble = |seed: u32| (75 + (x * seed + y * 13) % 7) as u8;
        Rgb([wobble(3), wobble(5), wobble(11)])
    }));
    for (name, bytes) in [
        ("flat gray", bytes_of(&gray_cover(), ImageFormat::Png)),
        (
            "flat gray as a JPEG",
            bytes_of(&gray_cover(), ImageFormat::Jpeg),
        ),
        ("gray with noise", bytes_of(&noisy, ImageFormat::Png)),
        (
            "gray with noise as a JPEG",
            bytes_of(&noisy, ImageFormat::Jpeg),
        ),
        ("gray and alpha", bytes_of(&gray_logo(), ImageFormat::Png)),
        (
            "a gray just short of a tint",
            solid_png(30, 45, [75, 75, 83]),
        ),
    ] {
        assert!(judged(&bytes), "{name} is the placeholder");
    }

    let two_tone = DynamicImage::ImageLuma8(GrayImage::from_fn(30, 45, |_, y| {
        Luma([if y < 20 { 75 } else { 200 }])
    }));
    let cover = DynamicImage::ImageRgb8(RgbImage::from_fn(30, 45, |x, y| {
        Rgb([(x * 8) as u8, (y * 5) as u8, 128])
    }));
    let faded_logo = DynamicImage::ImageLumaA8(GrayAlphaImage::from_fn(64, 36, |x, _| {
        LumaA([75, if x < 32 { 0 } else { 255 }])
    }));
    for (name, bytes) in [
        ("one colour that is not gray", png()),
        ("a gray with a tint", solid_png(30, 45, [75, 75, 84])),
        ("two grays far apart", bytes_of(&two_tone, ImageFormat::Png)),
        ("a cover", bytes_of(&cover, ImageFormat::Jpeg)),
        (
            "a logo on a transparent ground",
            bytes_of(&faded_logo, ImageFormat::Png),
        ),
    ] {
        assert!(!judged(&bytes), "{name} is a picture");
    }
}

#[test]
fn steams_placeholder_is_no_picture_and_steamgriddb_is_asked_in_its_place() {
    let mut library = Library::new();
    let id = library.install(LauncherId::Steam, APP, "Made Up Game");
    let http = steam_placeholders().http();
    assert_eq!(library.look_up(&id, &http, &Keys::none()), Outcome::Done);
    assert_eq!(
        http.urls(),
        [
            format!("{CDN}/{APP}/library_600x900.jpg"),
            format!("{SHARED}/{APP}/library_600x900.jpg"),
            format!("{LANDED}/library_600x900.jpg"),
            format!("{CDN}/{APP}/library_hero.jpg"),
            format!("{CDN}/{APP}/logo.png"),
            format!("{SHARED}/{APP}/logo.png"),
            format!("{LANDED}/logo.png"),
        ]
    );
    assert_eq!(
        library.row(&id),
        Some(ArtworkRow {
            grid_path: None,
            logo_path: None,
            ..steam_row(&id)
        })
    );
    assert_eq!(library.files(&id), ["hero.png"]);

    let grids = format!("{SGDB}/grids/steam/{APP}?dimensions=600x900&{FILTER}");
    let logos = format!("{SGDB}/logos/steam/{APP}?{FILTER}");
    let http = steam_placeholders()
        .with(&grids, json(200, &recorded("grids-steam-252950.json")))
        .with(
            "https://cdn2.steamgriddb.com/grid/aaaa1111.png",
            picture("image/png", png()),
        )
        .http();
    assert_eq!(library.look_up(&id, &http, &Keys::stored()), Outcome::Done);
    assert_eq!(
        http.urls()[7..],
        [
            grids,
            "https://cdn2.steamgriddb.com/grid/aaaa1111.png".to_owned(),
            logos,
        ]
    );
    assert_eq!(
        library.row(&id),
        Some(ArtworkRow {
            source: "steamgriddb".to_owned(),
            logo_path: None,
            ..steam_row(&id)
        })
    );
    assert_eq!(library.files(&id), ["grid.png", "hero.png"]);
}

#[test]
fn a_placeholder_an_earlier_lookup_kept_goes_at_the_next_scan_and_is_looked_up_again_once() {
    let mut library = Library::new();
    let id = library.install(LauncherId::Steam, APP, "Made Up Game");
    let gray = library.install(LauncherId::Steam, "2000001", "Made Up Gray Game");
    for (game, files) in [
        (
            &id,
            vec![
                ("grid.jpg", bytes_of(&gray_cover(), ImageFormat::Jpeg)),
                ("hero.png", png()),
                ("logo.png", bytes_of(&gray_logo(), ImageFormat::Png)),
            ],
        ),
        (
            &gray,
            vec![("grid.jpg", bytes_of(&gray_cover(), ImageFormat::Jpeg))],
        ),
    ] {
        fs::create_dir_all(library.folder(game)).unwrap();
        for (name, bytes) in files {
            fs::write(library.folder(game).join(name), bytes).unwrap();
        }
    }
    let kept = ArtworkRow {
        grid_path: Some(format!("{id}/grid.jpg")),
        ..steam_row(&id)
    };
    library.store.put_artwork(&id, &kept).unwrap();
    library
        .store
        .put_artwork(
            &gray,
            &ArtworkRow {
                grid_path: Some(format!("{gray}/grid.jpg")),
                hero_path: None,
                logo_path: None,
                ..steam_row(&gray)
            },
        )
        .unwrap();
    assert_eq!(
        library.art.enqueue_after_scan(&library.store, NOW).unwrap(),
        0,
        "before the check the placeholder counts as a cover"
    );

    assert_eq!(library.art.prune(&library.store).unwrap(), 2);
    let healed = ArtworkRow {
        grid_path: None,
        logo_path: None,
        ..kept
    };
    assert_eq!(library.row(&id), Some(healed.clone()));
    assert_eq!(library.files(&id), ["hero.png"]);
    assert_eq!(library.row(&gray), None);
    assert!(library.files(&gray).is_empty());

    assert_eq!(
        library.art.enqueue_after_scan(&library.store, NOW).unwrap(),
        2
    );
    let http = steam_placeholders().http();
    assert_eq!(
        library.run(&http, &Keys::none()),
        [Outcome::Failed, Outcome::Failed]
    );
    assert_eq!(library.row(&id), Some(healed));
    assert_eq!(library.files(&id), ["hero.png"]);
    assert_eq!(
        library.miss(&id).as_deref(),
        Some(
            "Steam CDN grid: only Steam's gray placeholder; Steam CDN logo: only Steam's gray placeholder; No SteamGridDB API key in Settings"
        )
    );

    assert_eq!(library.art.prune(&library.store).unwrap(), 0);
    assert_eq!(
        library.art.enqueue_after_scan(&library.store, NOW).unwrap(),
        0
    );
}

#[test]
fn a_gray_picture_the_user_chose_or_steamgriddb_gave_is_kept() {
    let mut library = Library::new();
    let chosen_for = library.install(LauncherId::Steam, APP, "Made Up Game");
    let given_to = library.install(LauncherId::Riot, "valorant.live", "VALORANT");
    let chosen = library.scratch.join("chosen.jpg");
    fs::write(&chosen, bytes_of(&gray_cover(), ImageFormat::Jpeg)).unwrap();
    library
        .art
        .set_manual(&library.store, &chosen_for, ArtworkKind::Grid, &chosen, NOW)
        .unwrap();
    let http = Web::default()
        .with(
            format!("{SGDB}/search/autocomplete/VALORANT"),
            json(200, &recorded("autocomplete-rocket-league.json")),
        )
        .with(
            format!("{SGDB}/grids/game/5267?dimensions=600x900&{FILTER}"),
            json(200, &recorded("grids-steam-252950.json")),
        )
        .with(
            "https://cdn2.steamgriddb.com/grid/aaaa1111.png",
            picture("image/png", bytes_of(&gray_cover(), ImageFormat::Png)),
        )
        .http();
    assert_eq!(
        library.look_up(&given_to, &http, &Keys::stored()),
        Outcome::Done
    );

    assert_eq!(library.art.prune(&library.store).unwrap(), 0);
    let chosen_row = library.row(&chosen_for).unwrap();
    assert_eq!(chosen_row.source, "manual");
    assert_eq!(chosen_row.grid_path, Some(format!("{chosen_for}/grid.jpg")));
    assert_eq!(library.files(&chosen_for), ["grid.jpg"]);
    let given_row = library.row(&given_to).unwrap();
    assert_eq!(given_row.source, "steamgriddb");
    assert_eq!(given_row.grid_path, Some(format!("{given_to}/grid.png")));
    assert_eq!(library.files(&given_to), ["grid.png"]);
}
