#![forbid(unsafe_code)]

// Artwork through the backend, on fake launchers and an internet that exists only in the test: what a scan looks up, what a run without a client does, where the SteamGridDB key rests and where it never shows, and what a quit leaves.

use std::fs;
use std::path::Path;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_lite::future::block_on;
use ol_api::Backend;
use ol_api::launchers::{
    BattlenetMachine, BattlenetTiming, EpicMachine, EpicTiming, Launchers, RiotMachine, RiotTiming,
    Secret, SteamTiming,
};
use ol_api::log::{LogOptions, Logger};
use ol_model::{ArtworkKind, ArtworkSource, ArtworkStatus, Game, LauncherId, SettingKey};
use ol_platform_api::http::{IMAGE_HOSTS, LOOKUP_HOSTS};
use ol_platform_api::{Shell, WebPage};
use ol_platform_fake::http::status;
use ol_platform_fake::{FakeShell, OpLog, RecordingHttp};
use ol_sim::artwork::{REFUSED_KEY_PREFIX, respond, solid_png};
use ol_testkit::{Scratch, oracle, snapshot};
use ol_vault::FakeVault;
use time::macros::datetime;

const VERSION: &str = "9.8.7";
const KEY: &str = "placeholderkey0000000000";
const WAIT: Duration = Duration::from_secs(20);

struct Run {
    scratch: Scratch,
    backend: Backend,
    logger: Logger,
}

impl Run {
    // The scenario all-four on a scratch folder, not yet scanned and handed no client.
    fn new() -> Self {
        let scratch = Scratch::new("api-artwork");
        let launchers_dir = scratch.join("launchers");
        ol_sim::lay_down(
            &oracle("fixtures/dev-only/fake-launchers/all-four"),
            &oracle("fixtures/launchers"),
            &launchers_dir,
        )
        .unwrap();
        let data_dir = scratch.join("data");
        let fake = ol_sim::materialize(
            &launchers_dir,
            &data_dir,
            datetime!(2026-11-06 16:46:23 UTC),
        )
        .unwrap();
        let mut launchers = Launchers::on(fake.platform.clone(), Arc::new(FakeVault));
        launchers.steam_timing = SteamTiming::with(&fake.steam_timing);
        launchers
            .steam_default_dir
            .clone_from(&fake.steam_default_dir);
        launchers.epic = EpicMachine {
            timing: EpicTiming::with(&fake.epic_timing),
            default_dir: fake.epic_default_dir.clone(),
            local_app_data: fake.local_app_data.clone(),
            program_data: fake.program_data.clone(),
        };
        launchers.battlenet = BattlenetMachine {
            timing: BattlenetTiming::with(&fake.battlenet_timing),
            default_dir: fake.battlenet_default_dir.clone(),
            app_data: fake.app_data.clone(),
            local_app_data: fake.local_app_data.clone(),
            program_data: fake.program_data.clone(),
        };
        launchers.riot = RiotMachine::at(
            RiotTiming::with(&fake.riot_timing),
            fake.riot_default_dir.clone(),
            fake.local_app_data.clone(),
            fake.program_data.clone(),
        );
        let logger = Logger::for_data_folder(&data_dir, LogOptions::default());
        let backend =
            Backend::logging_to(logger.log()).start_with(data_dir, VERSION, launchers, None);
        Self {
            scratch,
            backend,
            logger,
        }
    }

    fn with_the_fake_internet() -> (Self, Arc<RecordingHttp>) {
        let run = Self::new();
        let http = Arc::new(RecordingHttp::new(respond));
        run.backend.use_http(Arc::clone(&http) as _);
        (run, http)
    }

    fn data_dir(&self) -> std::path::PathBuf {
        self.scratch.join("data")
    }

    fn status(&self) -> ArtworkStatus {
        block_on(self.backend.artwork_status()).unwrap()
    }

    // Waits until no lookup waits or runs.
    fn settled(&self) -> ArtworkStatus {
        let started = std::time::Instant::now();
        loop {
            let status = self.status();
            if status.busy() == 0 {
                return status;
            }
            assert!(started.elapsed() < WAIT, "the artwork queue did not drain");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn game(&self, launcher: LauncherId) -> Game {
        let library = block_on(self.backend.library()).unwrap();
        let found = library
            .games
            .into_iter()
            .find(|entry| entry.game.launcher == launcher);
        found.expect("the scan found this launcher's game").game
    }

    fn log_text(&self) -> String {
        assert!(self.logger.flush(WAIT));
        fs::read_to_string(self.logger.current_path()).unwrap()
    }
}

fn holds(folder: &Path, text: &str) -> Vec<String> {
    snapshot(folder)
        .into_iter()
        .filter(|(_, content)| {
            content
                .windows(text.len())
                .any(|window| window == text.as_bytes())
        })
        .map(|(name, _)| name)
        .collect()
}

#[test]
fn a_scan_looks_up_what_it_found_and_ends_as_the_recorded_runs_do_but_for_riots_icon() {
    let (run, http) = Run::with_the_fake_internet();
    let heard = run.backend.artwork_changes();
    assert_eq!(run.status(), ArtworkStatus::default());
    block_on(run.backend.rescan()).unwrap();
    let status = run.settled();
    assert_eq!(
        (
            status.completed,
            status.failed,
            status.scanned,
            status.has_api_key
        ),
        (3, 1, true, false)
    );
    assert!(
        status.revision >= 5,
        "a scan and four lookups moved the Library"
    );

    // What oracle/l2 records for every run on this scenario: Steam's three pictures for the three games Steam knows.
    for launcher in [LauncherId::Steam, LauncherId::Epic, LauncherId::Battlenet] {
        let game = run.game(launcher);
        let art = game.artwork.expect("a game Steam knows has its pictures");
        assert_eq!(art.source, ArtworkSource::SteamCdn, "{launcher:?}");
        assert_eq!(art.grid, Some(format!("{}/grid.png", game.id)));
        assert_eq!(art.hero, Some(format!("{}/hero.png", game.id)));
        assert_eq!(art.logo, Some(format!("{}/logo.png", game.id)));
        assert_eq!(game.artwork_error, None);
        let file = run
            .data_dir()
            .join("artwork")
            .join(&game.id)
            .join("grid.png");
        assert_eq!(fs::read(file).unwrap()[..8], solid_png(1, 1, [0; 3])[..8]);
    }
    // The recorded runs give VALORANT its own icon; here a Riot game is looked up like any other, and without a key that finds nothing.
    let valorant = run.game(LauncherId::Riot);
    assert_eq!(valorant.artwork, None);
    assert_eq!(
        valorant.artwork_error.as_deref(),
        Some("Not found on Steam; add a SteamGridDB API key in Settings")
    );

    let asked = http.asked();
    assert!(asked.iter().all(|asked| !asked.with_key));
    for asked in &asked {
        let host = asked.url.split('/').nth(2).unwrap();
        assert!(
            LOOKUP_HOSTS
                .into_iter()
                .chain(IMAGE_HOSTS)
                .any(|listed| listed == host),
            "{host}"
        );
    }
    assert_eq!(
        asked
            .iter()
            .filter(|asked| asked.url.contains("steamstatic.com"))
            .count(),
        9
    );
    assert_eq!(
        asked
            .iter()
            .filter(|asked| !asked.url.contains("steamstatic.com"))
            .map(|asked| asked.url.as_str())
            .collect::<Vec<_>>(),
        [
            "https://store.steampowered.com/api/storesearch/?term=VALORANT&l=english&cc=US",
            "https://steamcommunity.com/actions/SearchApps/VALORANT",
        ]
    );

    let mut last = None;
    while let Ok(status) = heard.try_recv() {
        last = Some(status);
    }
    assert_eq!(last, Some(status), "whoever watches heard the queue drain");
    let log = run.log_text();
    for line in ["artwork queued", "artwork cached", "artwork not found"] {
        assert!(log.contains(line), "{line}");
    }

    // A second scan asks nothing: the covers are there, and the miss is fresh.
    let before = http.asked().len();
    block_on(run.backend.rescan()).unwrap();
    assert_eq!(run.settled().completed, 3);
    assert_eq!(http.asked().len(), before);
    run.backend.shut_down();
}

#[test]
fn a_run_that_is_handed_no_client_looks_nothing_up() {
    let run = Run::new();
    block_on(run.backend.rescan()).unwrap();
    let status = run.status();
    assert_eq!((status.busy(), status.completed, status.failed), (0, 0, 0));
    assert!(status.scanned);
    let refused = block_on(run.backend.refresh_artwork(None, true)).unwrap_err();
    assert_eq!(refused.code, "UNAVAILABLE");
    let game = run.game(LauncherId::Steam);
    let refused = block_on(run.backend.refresh_artwork(Some(&game.id), true)).unwrap_err();
    assert_eq!(refused.code, "UNAVAILABLE");
    block_on(
        run.backend
            .set_setting(SettingKey::ArtworkAutoFetch, "false".to_owned()),
    )
    .unwrap();
    block_on(
        run.backend
            .set_setting(SettingKey::ArtworkAutoFetch, "true".to_owned()),
    )
    .unwrap();
    assert_eq!(run.status().busy(), 0);
    assert_eq!(game.artwork, None);
    assert_eq!(snapshot(&run.data_dir().join("artwork")).len(), 0);
    run.backend.shut_down();
}

#[test]
fn looking_up_by_itself_can_be_turned_off_and_turning_it_on_again_looks_up_what_is_missing() {
    let (run, http) = Run::with_the_fake_internet();
    let set = |value: &str| {
        block_on(
            run.backend
                .set_setting(SettingKey::ArtworkAutoFetch, value.to_owned()),
        )
        .unwrap();
    };
    set("false");
    block_on(run.backend.rescan()).unwrap();
    assert_eq!(run.settled().completed + run.status().failed, 0);
    assert!(http.asked().is_empty());
    set("true");
    let status = run.settled();
    assert_eq!((status.completed, status.failed), (3, 1));
    // Set to on where on is stored: nothing more is asked.
    let before = http.asked().len();
    set("true");
    run.settled();
    assert_eq!(http.asked().len(), before);
    // Asked for by the user, the lookups run whatever the setting says.
    set("false");
    assert_eq!(
        block_on(run.backend.refresh_artwork(None, true)).unwrap(),
        4
    );
    assert_eq!(run.settled().completed, 6);
    run.backend.shut_down();
}

#[test]
fn the_key_is_sealed_in_the_database_sent_to_steamgriddb_alone_and_named_nowhere() {
    let (run, http) = Run::with_the_fake_internet();
    block_on(run.backend.rescan()).unwrap();
    run.settled();
    for no_key in [
        "",
        "short",
        "https://www.steamgriddb.com/profile/preferences/api",
        "placeholderkey 000000000",
        "placeholderkey0000000000\r\nx-other: 1",
    ] {
        let refused = block_on(
            run.backend
                .set_steamgriddb_key(Secret::new(no_key.to_owned())),
        )
        .unwrap_err();
        assert_eq!(refused.code, "BAD_REQUEST", "{no_key:?}");
        assert_eq!(
            refused.message,
            "That does not look like a SteamGridDB API key. Paste the key itself."
        );
    }
    assert!(!run.status().has_api_key);

    block_on(run.backend.set_steamgriddb_key(Secret::new(KEY.to_owned()))).unwrap();
    assert!(run.status().has_api_key);
    assert_eq!(
        block_on(run.backend.refresh_artwork(None, false)).unwrap(),
        1
    );
    run.settled();
    let valorant = run.game(LauncherId::Riot);
    let art = valorant
        .artwork
        .expect("with a key SteamGridDB has a Riot game's cover");
    assert_eq!(art.source, ArtworkSource::SteamGridDb);
    assert_eq!(art.grid, Some(format!("{}/grid.png", valorant.id)));
    assert_eq!(valorant.artwork_error, None);

    let with_key: Vec<String> = http
        .asked()
        .into_iter()
        .filter(|asked| asked.with_key)
        .map(|asked| asked.url)
        .collect();
    assert_eq!(with_key.len(), 4);
    assert!(
        with_key
            .iter()
            .all(|url| url.starts_with("https://www.steamgriddb.com/api/v2/"))
    );
    assert!(http.urls().iter().all(|url| !url.contains(KEY)));

    block_on(run.backend.remove_steamgriddb_key()).unwrap();
    assert!(!run.status().has_api_key);
    let refused = format!("{REFUSED_KEY_PREFIX}0000000000000000");
    block_on(
        run.backend
            .set_steamgriddb_key(Secret::new(refused.clone())),
    )
    .unwrap();
    assert_eq!(
        block_on(run.backend.refresh_artwork(Some(&valorant.id), true)).unwrap(),
        1
    );
    run.settled();
    let valorant = run.game(LauncherId::Riot);
    assert_eq!(
        valorant.artwork_error.as_deref(),
        Some("SteamGridDB: the API key was rejected")
    );

    let log = run.log_text();
    assert!(log.contains("secret stored") && log.contains("secret deleted"));
    assert!(log.contains("steamgriddb.apiKey"));
    for key in [KEY, refused.as_str()] {
        assert!(!log.contains(key), "the log names the key");
    }
    run.backend.shut_down();
    run.logger.close(WAIT);
    // On disk the stand-in vault of the fixtures seals nothing well, so only the key that was removed can be looked for.
    assert_eq!(holds(&run.data_dir(), KEY), Vec::<String>::new());
    assert_eq!(holds(&run.data_dir(), &refused), Vec::<String>::new());
}

#[test]
fn a_picture_of_the_users_own_is_kept_through_the_backend() {
    let (run, http) = Run::with_the_fake_internet();
    block_on(run.backend.rescan()).unwrap();
    run.settled();
    let game = run.game(LauncherId::Steam);
    let chosen = run.scratch.join("chosen.png");
    fs::write(&chosen, solid_png(30, 45, [9, 9, 9])).unwrap();
    let revision = run.status().revision;
    let set = block_on(
        run.backend
            .set_manual_artwork(&game.id, ArtworkKind::Grid, chosen.clone()),
    )
    .unwrap();
    assert_eq!(
        set.artwork.map(|art| art.source),
        Some(ArtworkSource::Manual)
    );
    assert!(run.status().revision > revision);

    fs::write(&chosen, b"no picture at all, whatever it is called").unwrap();
    let refused = block_on(
        run.backend
            .set_manual_artwork(&game.id, ArtworkKind::Grid, chosen),
    )
    .unwrap_err();
    assert_eq!(refused.code, "BAD_IMAGE");

    let before = http.asked().len();
    assert_eq!(
        block_on(run.backend.refresh_artwork(None, true)).unwrap(),
        3
    );
    run.settled();
    // The Epic game shares Steam's id for Rocket League, so one request for that cover is its own.
    assert_eq!(
        http.asked()[before..]
            .iter()
            .filter(|asked| asked.url.contains("/252950/library_600x900"))
            .count(),
        1,
        "a refresh of all leaves the user's own picture alone"
    );
    assert_eq!(
        run.game(LauncherId::Steam).artwork.map(|art| art.source),
        Some(ArtworkSource::Manual)
    );
    assert!(run.log_text().contains("manual artwork set"));
    run.backend.shut_down();
}

#[test]
fn a_quit_does_not_wait_for_a_lookup_and_what_arrives_after_it_is_not_kept() {
    let run = Run::new();
    let (entered, entering) = channel();
    let (release, released) = channel::<()>();
    let entered: Mutex<Sender<()>> = Mutex::new(entered);
    let released = Mutex::new(released);
    let http = Arc::new(RecordingHttp::new(move |request| {
        let _ = entered.lock().unwrap().send(());
        let _ = released.lock().unwrap().recv();
        respond(request)
    }));
    run.backend.use_http(Arc::clone(&http) as _);
    block_on(run.backend.rescan()).unwrap();
    entering
        .recv_timeout(WAIT)
        .expect("a lookup asked something");
    let status = run.status();
    assert_eq!((status.running, status.queued), (2, 2));

    let quit = std::time::Instant::now();
    run.backend.shut_down();
    assert!(
        quit.elapsed() < Duration::from_secs(5),
        "the quit waited for a lookup"
    );
    drop(release);
    std::thread::sleep(Duration::from_millis(200));
    assert!(http.asked().len() <= 2, "a stopped lookup asked on");
    assert_eq!(snapshot(&run.data_dir().join("artwork")).len(), 0);
    let (store, _) = ol_store::Store::open(&ol_store::database_path(&run.data_dir())).unwrap();
    assert_eq!(store.artwork_game_ids().unwrap(), Vec::<String>::new());
}

#[test]
fn the_one_page_the_app_opens_in_a_browser_is_the_steamgriddb_key_page() {
    let run = Run::new();
    let refused = block_on(run.backend.open_page(WebPage::SteamGridDbApiKey)).unwrap_err();
    assert_eq!(refused.code, "OPEN_FAILED");
    let log = OpLog::new();
    run.backend
        .use_shell(Arc::new(FakeShell::new(log.clone())) as Arc<dyn Shell>);
    block_on(run.backend.open_page(WebPage::SteamGridDbApiKey)).unwrap();
    let opened = log.snapshot();
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0]["op"], "shell.page");
    assert_eq!(
        opened[0]["url"],
        "https://www.steamgriddb.com/profile/preferences/api"
    );
    run.backend.shut_down();
}

#[test]
fn a_client_that_answers_nothing_leaves_every_game_with_its_reason() {
    let run = Run::new();
    run.backend
        .use_http(Arc::new(RecordingHttp::new(|_| Ok(status(404)))));
    block_on(run.backend.rescan()).unwrap();
    let status = run.settled();
    assert_eq!((status.completed, status.failed), (0, 4));
    let steam = run.game(LauncherId::Steam);
    assert_eq!(steam.artwork, None);
    assert_eq!(
        steam.artwork_error.as_deref(),
        Some(
            "Steam CDN grid: HTTP 404; Steam CDN hero: HTTP 404; Steam CDN logo: HTTP 404; No SteamGridDB API key in Settings"
        )
    );
    run.backend.shut_down();
}
