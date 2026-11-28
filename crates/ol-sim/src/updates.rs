// The release feed of a run on fake launchers, answered in this process: GitHub's latest release of One Launch, its `latest.yml` and its installer, as the story the scenario names tells them. The latest release and both files are answered with the redirects GitHub sends, so a fake run takes the path a real one takes. Nothing here opens a socket.

use std::sync::Arc;
use std::time::Duration;

use ol_platform_api::http::HttpError;
use ol_platform_api::update_http::{
    FEED_HOST, FEED_REPOSITORY, MANIFEST_FILE, UpdateAsk, UpdateRequest,
};
use ol_platform_api::{CancelToken, Clock};
use ol_platform_fake::update_http::{
    FakeAnswer, Part, ScriptedUpdates, file, in_parts, json, redirect, status,
};
use serde_json::Value;

const SIGNED: &str = include_str!("../../../oracle/fixtures/updates/latest.yml");
const UNSIGNED: &str = include_str!("../../../oracle/fixtures/updates/latest-unsigned.yml");
const KEYS: &str = include_str!("../../../oracle/fixtures/updates/README-keys.json");

pub const TAG: &str = "v1.0.0-beta.4";
pub const VERSION: &str = "1.0.0-beta.4";
pub const INSTALLER_NAME: &str = "OneLaunch-1.0.0-beta.4-setup.exe";
// The 50 bytes `latest.yml` signs.
pub const INSTALLER: &[u8] = b"placeholder installer bytes, not a real executable";
// As many bytes, and not the signed ones.
pub const TAMPERED: &[u8] = b"placeholder installer bytes, tampered, not signed.";
// How long the installer of `ready` waits between its parts: past the second a percent waits for.
pub const PAUSE: Duration = Duration::from_millis(1200);
const PART: usize = 10;
const ASSETS_HOST: &str = "release-assets.githubusercontent.com";
const ASSETS_PATH: &str = "/github-production-release-asset/104729/";
const MANIFEST: u32 = 2_871_301;
const SETUP: u32 = 2_871_302;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateStory {
    Ready,
    UpToDate,
    Unsigned,
    BadSignature,
    HashMismatch,
    NoManifest,
    Http503,
    Offline,
}

impl UpdateStory {
    pub const ALL: [Self; 8] = [
        Self::Ready,
        Self::UpToDate,
        Self::Unsigned,
        Self::BadSignature,
        Self::HashMismatch,
        Self::NoManifest,
        Self::Http503,
        Self::Offline,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::UpToDate => "up-to-date",
            Self::Unsigned => "unsigned",
            Self::BadSignature => "bad-signature",
            Self::HashMismatch => "hash-mismatch",
            Self::NoManifest => "no-manifest",
            Self::Http503 => "http-503",
            Self::Offline => "offline",
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|story| story.name() == name)
    }
}

// The fixture's throwaway key, which signed `latest.yml`: its id and its SPKI in base64.
pub fn release_key() -> (String, String) {
    let keys: Value = serde_json::from_str(KEYS).unwrap_or_default();
    let field = |name: &str| keys[name].as_str().unwrap_or_default().to_owned();
    (field("keyId"), field("throwawayPublicKeySpkiBase64"))
}

pub fn respond(
    story: UpdateStory,
    running: &str,
    clock: &Arc<dyn Clock>,
    request: &UpdateRequest,
) -> Result<FakeAnswer, HttpError> {
    match story {
        UpdateStory::Offline => return Err(HttpError::Failed("the connection was refused")),
        UpdateStory::Http503 => return Ok(status(503)),
        _ => {}
    }
    let url = request.url();
    let path = url.path_and_query();
    let on_github = url.host() == FEED_HOST;
    let download = format!("{FEED_REPOSITORY}/releases/download/{TAG}/");
    Ok(match (request.ask(), on_github) {
        (UpdateAsk::LatestRelease, true)
            if path == format!("{FEED_REPOSITORY}/releases/latest") =>
        {
            redirect(
                302,
                &format!("https://{FEED_HOST}{FEED_REPOSITORY}/releases/tag/{TAG}"),
            )
        }
        (UpdateAsk::LatestRelease, true)
            if path == format!("{FEED_REPOSITORY}/releases/tag/{TAG}") =>
        {
            json(200, &release())
        }
        (UpdateAsk::Manifest, true) if path == format!("{download}{MANIFEST_FILE}") => {
            if story == UpdateStory::NoManifest {
                status(404)
            } else {
                redirect(302, &asset(MANIFEST, MANIFEST_FILE))
            }
        }
        (UpdateAsk::Installer, true) if path == format!("{download}{INSTALLER_NAME}") => {
            redirect(302, &asset(SETUP, INSTALLER_NAME))
        }
        (UpdateAsk::Manifest, false) if is_asset(url.host(), path, MANIFEST) => {
            file(manifest(story, running).into_bytes())
        }
        (UpdateAsk::Installer, false) if is_asset(url.host(), path, SETUP) => {
            installer(story, clock)
        }
        _ => status(404),
    })
}

fn release() -> String {
    serde_json::json!({
        "tag_name": TAG,
        "name": format!("One Launch {VERSION}"),
        "draft": false,
        "prerelease": true,
    })
    .to_string()
}

// Where GitHub hands a release file out: a host of its file domain and a signed, short-lived query, here a made-up one.
fn asset(id: u32, name: &str) -> String {
    format!(
        "https://{ASSETS_HOST}{ASSETS_PATH}{id}?sp=r&sv=2018-11-09&sr=b&spr=https&se=2026-11-27T10%3A00%3A00Z&rscd=attachment%3B+filename%3D{name}&rsct=application%2Foctet-stream&sig=placeholdersignature%3D"
    )
}

fn is_asset(host: &str, path: &str, id: u32) -> bool {
    host == ASSETS_HOST
        && path
            .strip_prefix(ASSETS_PATH)
            .and_then(|rest| rest.split_once('?'))
            .is_some_and(|(at, _)| at == id.to_string())
}

fn manifest(story: UpdateStory, running: &str) -> String {
    let version = format!("version: {VERSION}");
    match story {
        UpdateStory::UpToDate => UNSIGNED.replacen(&version, &format!("version: {running}"), 1),
        UpdateStory::Unsigned => UNSIGNED.to_owned(),
        UpdateStory::BadSignature => SIGNED.replacen(&version, "version: 1.0.0-beta.5", 1),
        _ => SIGNED.to_owned(),
    }
}

fn installer(story: UpdateStory, clock: &Arc<dyn Clock>) -> FakeAnswer {
    match story {
        UpdateStory::HashMismatch => file(TAMPERED.to_vec()),
        UpdateStory::Ready => {
            let mut parts = Vec::new();
            for (at, part) in INSTALLER.chunks(PART).enumerate() {
                if at > 0 {
                    let clock = Arc::clone(clock);
                    parts.push(Part::pause(move || {
                        clock.sleep(PAUSE, &CancelToken::new());
                    }));
                }
                parts.push(Part::Bytes(part.to_vec()));
            }
            in_parts(Some(INSTALLER.len() as u64), parts)
        }
        _ => file(INSTALLER.to_vec()),
    }
}

// The feed of a run, for the updater's kit.
pub fn feed(story: UpdateStory, running: &str, clock: Arc<dyn Clock>) -> ScriptedUpdates {
    let running = running.to_owned();
    ScriptedUpdates::new(move |request| respond(story, &running, &clock, request))
}

#[cfg(test)]
mod tests {
    use ol_platform_api::update_http::{
        BodyEnd, InstallerName, ReleaseTag, UpdateHttp, UpdateRequest,
    };
    use ol_platform_fake::FakeClock;
    use serde_json::Value;
    use time::macros::datetime;

    use super::*;

    const WAIT: Duration = Duration::from_secs(30);
    const LIMIT: u64 = 64 * 1024;
    const RUNNING: &str = "1.0.0-beta.3";

    struct Asked {
        status: u16,
        body: Vec<u8>,
        hops: Vec<String>,
    }

    fn clock() -> Arc<FakeClock> {
        Arc::new(FakeClock::starting_at(datetime!(2026-11-06 16:46:23 UTC)))
    }

    fn tag() -> ReleaseTag {
        ReleaseTag::new(TAG).unwrap()
    }

    fn requests() -> [UpdateRequest; 3] {
        [
            UpdateRequest::latest_release(LIMIT, WAIT),
            UpdateRequest::manifest(&tag(), LIMIT, WAIT),
            UpdateRequest::installer(
                &tag(),
                &InstallerName::new(INSTALLER_NAME).unwrap(),
                50,
                WAIT,
            ),
        ]
    }

    // Follows every redirect the feed sends, each one where the updater's own rule lets it go.
    fn ask(http: &ScriptedUpdates, first: UpdateRequest) -> Result<Asked, HttpError> {
        let mut request = first;
        let mut hops = Vec::new();
        loop {
            let (status, location, body) = if request.ask() == UpdateAsk::Installer {
                let mut into = Vec::new();
                let answer = http
                    .download(&request, &mut into, &mut |_| {}, &CancelToken::new())
                    .map_err(|error| match error {
                        ol_platform_api::update_http::DownloadError::Network(error) => error,
                        other => panic!("{other:?}"),
                    })?;
                if (200..=299).contains(&answer.status) {
                    assert_eq!(answer.body, BodyEnd::Complete(into.len() as u64));
                }
                (answer.status, answer.location, into)
            } else {
                let answer = http.send(&request)?;
                (answer.status, answer.location, answer.body.unwrap())
            };
            if !(300..=399).contains(&status) {
                return Ok(Asked { status, body, hops });
            }
            let location = location.expect("a redirect names where it leads");
            request = request
                .redirected(&location)
                .unwrap_or_else(|| panic!("the updater would not follow {location}"));
            hops.push(request.url().to_string());
        }
    }

    fn story_feed(story: UpdateStory) -> (ScriptedUpdates, Arc<FakeClock>) {
        let clock = clock();
        (
            feed(story, RUNNING, Arc::clone(&clock) as Arc<dyn Clock>),
            clock,
        )
    }

    fn text(asked: &Asked) -> String {
        String::from_utf8(asked.body.clone()).unwrap()
    }

    #[test]
    fn every_story_has_a_name_it_is_found_by() {
        let names: Vec<&str> = UpdateStory::ALL.iter().map(|story| story.name()).collect();
        assert_eq!(
            names,
            [
                "ready",
                "up-to-date",
                "unsigned",
                "bad-signature",
                "hash-mismatch",
                "no-manifest",
                "http-503",
                "offline"
            ]
        );
        for story in UpdateStory::ALL {
            assert_eq!(UpdateStory::named(story.name()), Some(story));
        }
        for unknown in ["", "Ready", "ready ", "latest", "offline!"] {
            assert_eq!(UpdateStory::named(unknown), None, "{unknown:?}");
        }
    }

    #[test]
    fn the_key_is_the_one_that_signed_the_fixture() {
        let keys: Value = serde_json::from_str(KEYS).unwrap();
        let (id, spki) = release_key();
        assert_eq!(id, "3376dec30c48c71f");
        assert_eq!(Some(id.as_str()), keys["keyId"].as_str());
        assert_eq!(
            Some(spki.as_str()),
            keys["throwawayPublicKeySpkiBase64"].as_str()
        );
        assert!(SIGNED.contains(&format!("keyId: '{id}'")));
    }

    #[test]
    fn ready_answers_as_github_does_with_a_redirect_for_each_request() {
        let (http, _) = story_feed(UpdateStory::Ready);
        let [latest, manifest, installer] = requests();

        let release = ask(&http, latest).unwrap();
        assert_eq!(release.status, 200);
        assert_eq!(
            release.hops,
            ["https://github.com/rjf2016/one-launch/releases/tag/v1.0.0-beta.4"]
        );
        let json: Value = serde_json::from_slice(&release.body).unwrap();
        assert_eq!(json["tag_name"], TAG);

        let manifest = ask(&http, manifest).unwrap();
        assert_eq!((manifest.status, text(&manifest)), (200, SIGNED.to_owned()));
        assert_eq!(manifest.hops.len(), 1);
        assert!(
            manifest.hops[0].starts_with("https://release-assets.githubusercontent.com/")
                && manifest.hops[0].contains('?'),
            "{}",
            manifest.hops[0]
        );

        let installer = ask(&http, installer).unwrap();
        assert_eq!(
            (installer.status, installer.body.as_slice()),
            (200, INSTALLER)
        );
        assert_eq!(installer.hops.len(), 1);
        assert!(installer.hops[0].starts_with("https://release-assets.githubusercontent.com/"));
        assert!(installer.hops[0].contains(INSTALLER_NAME));
        assert_ne!(installer.hops[0], manifest.hops[0]);

        let urls = http.urls();
        assert_eq!(urls.len(), 6);
        for url in urls {
            assert!(
                url.starts_with("https://github.com/rjf2016/one-launch/releases/")
                    || url.starts_with("https://release-assets.githubusercontent.com/"),
                "{url}"
            );
        }
    }

    #[test]
    fn readys_installer_comes_in_parts_with_a_pause_on_the_clock_between_them() {
        let (http, clock) = story_feed(UpdateStory::Ready);
        let before = clock.now();
        let [_, _, installer] = requests();
        let mut steps = Vec::new();
        let mut into = Vec::new();
        let moved = http
            .download(&installer, &mut into, &mut |_| {}, &CancelToken::new())
            .unwrap();
        let there = installer.redirected(&moved.location.unwrap()).unwrap();
        let mut into = Vec::new();
        let answer = http
            .download(
                &there,
                &mut into,
                &mut |bytes| steps.push((bytes, clock.now())),
                &CancelToken::new(),
            )
            .unwrap();
        assert_eq!(answer.body, BodyEnd::Complete(50));
        assert_eq!(answer.declared, Some(50));
        assert_eq!(into, INSTALLER);
        let bytes: Vec<u64> = steps.iter().map(|(bytes, _)| *bytes).collect();
        assert_eq!(bytes, [10, 20, 30, 40, 50]);
        for (at, (_, when)) in steps.iter().enumerate() {
            assert_eq!(
                *when - before,
                PAUSE * u32::try_from(at).unwrap(),
                "part {at}"
            );
        }
    }

    #[test]
    fn up_to_date_serves_the_running_version_unsigned() {
        let (http, _) = story_feed(UpdateStory::UpToDate);
        let [_, manifest, _] = requests();
        let manifest = text(&ask(&http, manifest).unwrap());
        assert!(
            manifest.starts_with(&format!("version: {RUNNING}\n")),
            "{manifest}"
        );
        assert!(!manifest.contains("signature"));
        assert_eq!(manifest.replacen(RUNNING, VERSION, 1), UNSIGNED);
    }

    #[test]
    fn unsigned_serves_the_unsigned_fixture() {
        let (http, _) = story_feed(UpdateStory::Unsigned);
        let [_, manifest, installer] = requests();
        assert_eq!(text(&ask(&http, manifest).unwrap()), UNSIGNED);
        assert_eq!(ask(&http, installer).unwrap().body, INSTALLER);
    }

    #[test]
    fn bad_signature_serves_the_signed_fixture_with_a_greater_version() {
        let (http, _) = story_feed(UpdateStory::BadSignature);
        let [_, manifest, _] = requests();
        let manifest = text(&ask(&http, manifest).unwrap());
        assert!(
            manifest.starts_with("version: 1.0.0-beta.5\n"),
            "{manifest}"
        );
        assert_eq!(manifest.replacen("1.0.0-beta.5", VERSION, 1), SIGNED);
    }

    #[test]
    fn hash_mismatch_serves_the_signed_fixture_and_other_bytes_of_the_same_size() {
        let (http, _) = story_feed(UpdateStory::HashMismatch);
        let [_, manifest, installer] = requests();
        assert_eq!(text(&ask(&http, manifest).unwrap()), SIGNED);
        let bytes = ask(&http, installer).unwrap().body;
        assert_eq!(bytes, TAMPERED);
        assert_eq!(bytes.len(), INSTALLER.len());
        assert_ne!(bytes, INSTALLER);
    }

    #[test]
    fn no_manifest_has_a_release_and_no_latest_yml() {
        let (http, _) = story_feed(UpdateStory::NoManifest);
        let [latest, manifest, _] = requests();
        assert_eq!(ask(&http, latest).unwrap().status, 200);
        let manifest = ask(&http, manifest).unwrap();
        assert_eq!((manifest.status, manifest.hops.len()), (404, 0));
    }

    #[test]
    fn http_503_answers_every_request_so() {
        let (http, _) = story_feed(UpdateStory::Http503);
        for request in requests() {
            let asked = ask(&http, request).unwrap();
            assert_eq!((asked.status, asked.hops.len()), (503, 0));
        }
    }

    #[test]
    fn offline_reaches_nothing() {
        let (http, _) = story_feed(UpdateStory::Offline);
        for request in requests() {
            assert!(matches!(ask(&http, request), Err(HttpError::Failed(_))));
        }
    }
}
