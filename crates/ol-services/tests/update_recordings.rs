#![forbid(unsafe_code)]

// Each of the 23 recordings of the Electron app's update service in `oracle/l2/updates`, replayed step by step through the machine, with the real check and download on a scripted GitHub, a fake clock, a fake installer and a scratch pending folder.
//
// The recordings were made on a generic loopback feed, and the app asks GitHub. So a recorded `GET /latest.yml` stands for two requests here, `GET /rjf2016/one-launch/releases/latest` and then `GET /rjf2016/one-launch/releases/download/<tag>/latest.yml`, and a recorded `GET /<installer>` for `GET /rjf2016/one-launch/releases/download/<tag>/<installer>`. The scripted GitHub answers the latest release with `{"tag_name":"v1.0.0-beta.4"}` at once, without the redirects that have tests of their own in `update_check.rs` and `update_download.rs`. A feed that is not listening fails every request with a connection failure, and such a request never reached a feed, so it is not counted, as the recording counts none. A feed with `feed.status` answers every request with that status, so the check ends at its first request: a recorded `GET /latest.yml` answered with that status stands for the first of its two requests alone. `manifest: null` means the latest release is answered and `latest.yml` is a 404. Every request the port sends is a GET.
//
// Lines of the scope `main:updater` are electron-updater's or its test feed's and have no counterpart here; they are left out. `feed.keys` names the trusted keys by id, `[]` is none; the running version is the recording's `app.version`.

use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use ol_platform_api::http::HttpError;
use ol_platform_api::update_http::{ReleaseTag, UpdateAsk};
use ol_platform_api::{CancelToken, Clock};
use ol_platform_fake::update_http::{
    FakeAnswer, Part, ScriptedUpdates, file, in_parts, json, status,
};
use ol_platform_fake::{FakeClock, RecordingInstaller};
use ol_services::install::{UpdateStatus, UpdateStep};
use ol_services::log::{LogOptions, Logger};
use ol_services::updates::check::check;
use ol_services::updates::download::download;
use ol_services::updates::manifest::{ManifestFile, ReleaseKey};
use ol_services::updates::service::{UpdateEffect, Updates, UpdatesSetup};
use ol_testkit::{Scratch, oracle};
use semver::Version;
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const WAIT: Duration = Duration::from_secs(10);
const TAG: &str = "v1.0.0-beta.4";
const LATEST: &str = "/rjf2016/one-launch/releases/latest";
const RELEASE: &str = "/rjf2016/one-launch/releases/download/v1.0.0-beta.4";

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn status_json(status: &UpdateStatus) -> Value {
    match status {
        UpdateStatus::Unavailable(reason) => {
            json!({ "state": "unavailable", "reason": reason.as_str() })
        }
        UpdateStatus::Idle => json!({ "state": "idle" }),
        UpdateStatus::Checking => json!({ "state": "checking" }),
        UpdateStatus::UpToDate { checked_at } => {
            json!({ "state": "up-to-date", "checkedAt": checked_at })
        }
        UpdateStatus::Downloading { version, percent } => {
            json!({ "state": "downloading", "version": version, "percent": percent })
        }
        UpdateStatus::Ready { version } => json!({ "state": "ready", "version": version }),
        UpdateStatus::Failed { step, message } => json!({
            "state": "failed",
            "step": match step {
                UpdateStep::Check => "check",
                UpdateStep::Download => "download",
                UpdateStep::Verify => "verify",
            },
            "message": message,
        }),
    }
}

// `checkedAt` is any ISO time with milliseconds and a Z, as the recordings' `<ts>` is.
fn normalised(mut state: Value, condition: &str) -> Value {
    if let Some(at) = state.get_mut("checkedAt") {
        let text = at.as_str().unwrap().to_owned();
        assert!(
            text.len() == 24
                && text.ends_with('Z')
                && text.as_bytes()[19] == b'.'
                && OffsetDateTime::parse(&text, &Rfc3339).is_ok(),
            "{condition}: checkedAt {text:?} is not an ISO time with milliseconds"
        );
        *at = json!("<ts>");
    }
    state
}

fn is_downloading_at_100(state: &Value) -> bool {
    state["state"] == "downloading" && state["percent"] == 100
}

enum Work {
    Check,
    Download(ReleaseTag, ManifestFile),
}

// What the app's backend thread will be: it carries out the machine's effects and hands their answers back.
struct Shared {
    machine: Mutex<Updates>,
    seen: Mutex<Vec<Value>>,
    timers: Mutex<Vec<Value>>,
    armed: Mutex<Option<u64>>,
    token: Mutex<Option<CancelToken>>,
    work: Mutex<VecDeque<Work>>,
    quit: AtomicBool,
}

impl Shared {
    fn saw(&self, machine: &Updates) {
        lock(&self.seen).push(status_json(&machine.status()));
    }

    fn event(&self, event: impl FnOnce(&mut Updates) -> Vec<UpdateEffect>) {
        let effects = {
            let mut machine = lock(&self.machine);
            let effects = event(&mut machine);
            self.saw(&machine);
            effects
        };
        self.apply(effects);
    }

    fn apply(&self, effects: Vec<UpdateEffect>) {
        for effect in effects {
            match effect {
                UpdateEffect::Arm(delay) => {
                    let ms = u64::try_from(delay.as_millis()).unwrap();
                    let mut armed = lock(&self.armed);
                    assert!(armed.is_none(), "armed over an armed timer");
                    *armed = Some(ms);
                    lock(&self.timers).push(json!({ "ev": "arm", "ms": ms }));
                }
                UpdateEffect::Disarm => {
                    let ms = lock(&self.armed).take().expect("disarmed with none armed");
                    lock(&self.timers).push(json!({ "ev": "clear", "ms": ms }));
                }
                UpdateEffect::Check => lock(&self.work).push_back(Work::Check),
                UpdateEffect::Download { tag, file } => {
                    lock(&self.work).push_back(Work::Download(tag, file));
                }
                UpdateEffect::CancelDownload => lock(&self.token)
                    .as_ref()
                    .expect("a download to cancel")
                    .cancel(),
                UpdateEffect::Quit => self.quit.store(true, Ordering::SeqCst),
            }
        }
    }
}

struct Feed {
    condition: String,
    listening: bool,
    status: Option<u16>,
    manifest: Option<String>,
    installer: Vec<u8>,
}

struct Replay {
    condition: String,
    recording: Value,
    shared: Arc<Shared>,
    http: Arc<ScriptedUpdates>,
    clock: Arc<FakeClock>,
    reached: Arc<Mutex<Vec<String>>>,
    installer: RecordingInstaller,
    logger: Logger,
    pending: PathBuf,
    installer_name: Option<String>,
    _scratch: Scratch,
}

fn trusted(ids: &[Value]) -> Vec<(String, String)> {
    let keys: Value = serde_json::from_str(
        &fs::read_to_string(oracle("fixtures/updates/README-keys.json")).unwrap(),
    )
    .unwrap();
    ids.iter()
        .map(|id| {
            let id = id.as_str().unwrap();
            assert_eq!(keys["keyId"], id, "the one key the recordings name");
            (
                id.to_owned(),
                keys["throwawayPublicKeySpkiBase64"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
            )
        })
        .collect()
}

fn respond(
    feed: Feed,
    shared: &Arc<Shared>,
    clock: &Arc<FakeClock>,
    reached: &Arc<Mutex<Vec<String>>>,
) -> ScriptedUpdates {
    let (shared, clock, reached) = (Arc::clone(shared), Arc::clone(clock), Arc::clone(reached));
    let manifests = AtomicUsize::new(0);
    let installers = AtomicUsize::new(0);
    ScriptedUpdates::new(move |request| {
        if !feed.listening {
            return Err(HttpError::Failed("the connection was refused"));
        }
        lock(&reached).push(request.url().path_and_query().to_owned());
        if let Some(code) = feed.status {
            return Ok(status(code));
        }
        Ok(match request.ask() {
            UpdateAsk::LatestRelease => json(200, &format!("{{\"tag_name\":\"{TAG}\"}}")),
            UpdateAsk::Manifest => {
                let first = manifests.fetch_add(1, Ordering::SeqCst) == 0;
                match &feed.manifest {
                    None => status(404),
                    // "Automatic updates switched off while the background check waits for latest.yml": here between the check effect and its answer.
                    Some(text) if first && feed.condition == "auto-off-in-flight" => {
                        let shared = Arc::clone(&shared);
                        in_parts(
                            Some(text.len() as u64),
                            vec![
                                Part::run(move || shared.event(|machine| machine.set_auto(false))),
                                Part::Bytes(text.as_bytes().to_vec()),
                            ],
                        )
                    }
                    Some(text) => file(text.as_bytes().to_vec()),
                }
            }
            UpdateAsk::Installer => {
                let first = installers.fetch_add(1, Ordering::SeqCst) == 0;
                installer_answer(&feed, first, &shared, &clock)
            }
        })
    })
}

fn installer_answer(
    feed: &Feed,
    first: bool,
    shared: &Arc<Shared>,
    clock: &Arc<FakeClock>,
) -> FakeAnswer {
    let bytes = &feed.installer;
    let size = Some(bytes.len() as u64);
    match feed.condition.as_str() {
        // "the installer is sent in three parts: the first third, 1.2 s later the second third, then the rest once a progress report has been seen": 16 bytes, a wait of 1.2 s on the fake clock, 16 more, then the rest.
        "update-ready" => {
            let clock = Arc::clone(clock);
            in_parts(
                size,
                vec![
                    Part::Bytes(bytes[..16].to_vec()),
                    Part::pause(move || {
                        clock.sleep(Duration::from_millis(1200), &CancelToken::new())
                    }),
                    Part::Bytes(bytes[16..32].to_vec()),
                    Part::Bytes(bytes[32..].to_vec()),
                ],
            )
        }
        // "Automatic updates switched off while the background download waits for the installer": here after its first part, which cancels the download.
        "auto-off-during-download" if first => {
            let shared = Arc::clone(shared);
            in_parts(
                size,
                vec![
                    Part::Bytes(bytes[..16].to_vec()),
                    Part::run(move || shared.event(|machine| machine.set_auto(false))),
                    Part::Bytes(bytes[16..].to_vec()),
                ],
            )
        }
        _ => file(bytes.clone()),
    }
}

impl Replay {
    fn new(condition: &str) -> Self {
        let path = oracle(&format!("l2/updates/{condition}.json"));
        let recording: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        let app = &recording["app"];
        let feed = &recording["feed"];
        let version = Version::parse(app["version"].as_str().unwrap()).unwrap();
        let start = OffsetDateTime::parse(app["clock"].as_str().unwrap(), &Rfc3339).unwrap();
        let clock = Arc::new(FakeClock::starting_at(start));
        let scratch = Scratch::new(&format!("update-recording-{condition}"));
        let logger = Logger::open(&scratch.join("logs"), LogOptions::default());
        let keys = trusted(feed["keys"].as_array().unwrap());
        let keys: Vec<ReleaseKey<'_>> = keys
            .iter()
            .map(|(id, spki)| ReleaseKey { id, spki })
            .collect();
        let machine = Updates::new(UpdatesSetup {
            version,
            keys: &keys,
            auto: true,
            this_copy: Ok(()),
            log: &logger.log(),
            clock: Arc::clone(&clock) as Arc<dyn Clock>,
        });
        let shared = Arc::new(Shared {
            machine: Mutex::new(machine),
            seen: Mutex::default(),
            timers: Mutex::default(),
            armed: Mutex::default(),
            token: Mutex::default(),
            work: Mutex::default(),
            quit: AtomicBool::new(false),
        });
        let reached = Arc::new(Mutex::new(Vec::new()));
        let installer = &feed["installer"];
        let installer_name = installer["path"]
            .as_str()
            .map(|path| path.trim_start_matches('/').to_owned());
        let installer_bytes = installer["text"]
            .as_str()
            .map(|text| text.as_bytes().to_vec())
            .unwrap_or_default();
        if let Some(bytes) = installer["bytes"].as_u64() {
            assert_eq!(
                installer_bytes.len() as u64,
                bytes,
                "{condition}: installer text"
            );
        }
        let http = Arc::new(respond(
            Feed {
                condition: condition.to_owned(),
                listening: feed["listening"].as_bool().unwrap(),
                status: feed["status"]
                    .as_u64()
                    .map(|code| u16::try_from(code).unwrap()),
                manifest: feed["manifest"].as_str().map(str::to_owned),
                installer: installer_bytes,
            },
            &shared,
            &clock,
            &reached,
        ));
        let pending = scratch.join("update-cache").join("pending");
        Self {
            condition: condition.to_owned(),
            recording,
            shared,
            http,
            clock,
            reached,
            installer: RecordingInstaller::new(),
            logger,
            pending,
            installer_name,
            _scratch: scratch,
        }
    }

    fn drain(&self) {
        loop {
            let Some(work) = lock(&self.shared.work).pop_front() else {
                return;
            };
            match work {
                Work::Check => {
                    let running = lock(&self.shared.machine).running().clone();
                    let answer = check(&*self.http, &running);
                    self.shared.event(|machine| machine.checked(answer));
                }
                Work::Download(tag, release) => {
                    let token = CancelToken::new();
                    *lock(&self.shared.token) = Some(token.clone());
                    let shared = Arc::clone(&self.shared);
                    let answer = download(
                        &*self.http,
                        &*self.clock,
                        &self.pending,
                        &tag,
                        &release,
                        &mut |percent| {
                            let mut machine = lock(&shared.machine);
                            machine.progress(percent);
                            shared.saw(&machine);
                        },
                        &token,
                    );
                    *lock(&self.shared.token) = None;
                    self.shared.event(|machine| machine.downloaded(answer));
                }
            }
        }
    }

    fn fire(&self) {
        let ms = lock(&self.shared.armed)
            .take()
            .unwrap_or_else(|| panic!("{}: a timer to fire", self.condition));
        lock(&self.shared.timers).push(json!({ "ev": "fire", "ms": ms }));
        self.clock
            .sleep(Duration::from_millis(ms), &CancelToken::new());
        self.shared.event(Updates::timer_fired);
        self.drain();
    }

    fn quit(&self, exit_code: i32) {
        self.shared.event(Updates::stop);
        let mut machine = lock(&self.shared.machine);
        machine.install_at_quit(exit_code, &self.installer);
        self.shared.saw(&machine);
    }

    fn log_lines(&self) -> Vec<Value> {
        assert!(self.logger.flush(WAIT));
        fs::read_to_string(self.logger.current_path())
            .unwrap_or_default()
            .lines()
            .filter_map(parse_line)
            .collect()
    }

    fn expected_requests(&self, step: &Value) -> Vec<String> {
        let feed_status = self.recording["feed"]["status"].as_u64();
        step["requests"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|request| {
                assert_eq!(request["method"], "GET", "{}", self.condition);
                let path = request["path"].as_str().unwrap();
                if path == "/latest.yml" {
                    let both = vec![LATEST.to_owned(), format!("{RELEASE}/latest.yml")];
                    if feed_status.is_some() {
                        both[..1].to_vec()
                    } else {
                        both
                    }
                } else {
                    vec![format!("{RELEASE}{path}")]
                }
            })
            .collect()
    }

    fn pending_files(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.pending)
            .map(|entries| {
                entries
                    .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    fn run(&self) {
        let condition = &self.condition;
        let steps = self.recording["recorded"]["steps"].as_array().unwrap();
        let mut last_state: Option<Value> = None;
        let mut lines_before = 0;
        let mut reached_before = 0;
        {
            let machine = lock(&self.shared.machine);
            self.shared.saw(&machine);
        }
        for step in steps {
            let name = step["step"].as_str().unwrap();
            let at = format!("{condition}, step {name}");
            let mut answer = None;
            match name {
                "start" => self.shared.event(Updates::start),
                "timer" | "timer-again" => self.fire(),
                "manual" => {
                    let effects = {
                        let mut machine = lock(&self.shared.machine);
                        let (given, effects) = machine.check();
                        self.shared.saw(&machine);
                        answer = Some(status_json(&given));
                        effects
                    };
                    self.shared.apply(effects);
                    self.drain();
                }
                "auto-on" => self.shared.event(|machine| machine.set_auto(true)),
                "install" => {
                    self.shared
                        .event(|machine| machine.request_install().expect("a verified update"));
                    assert!(
                        self.shared.quit.load(Ordering::SeqCst),
                        "{at}: the quit is asked"
                    );
                    self.quit(0);
                }
                "quit" => self.quit(0),
                other => panic!("{at}: a step this replay does not know: {other}"),
            }
            if let Some(exit) = step.get("exit") {
                assert_eq!(exit, 0, "{at}: the exit code passed to the quit");
            }

            let seen: Vec<Value> = std::mem::take(&mut *lock(&self.shared.seen));
            let mut states = Vec::new();
            for state in seen {
                let state = normalised(state, condition);
                if is_downloading_at_100(&state) || last_state.as_ref() == Some(&state) {
                    continue;
                }
                last_state = Some(state.clone());
                states.push(state);
            }
            let recorded_states: Vec<Value> = step["states"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|state| !is_downloading_at_100(state))
                .cloned()
                .collect();
            assert_eq!(states, recorded_states, "{at}: states");

            let reached = lock(&self.reached).clone();
            assert_eq!(
                reached[reached_before..],
                self.expected_requests(step),
                "{at}: requests"
            );
            reached_before = reached.len();

            let timers = std::mem::take(&mut *lock(&self.shared.timers));
            assert_eq!(&Value::from(timers), &step["timers"], "{at}: timers");

            let lines = self.log_lines();
            let recorded_lines: Vec<Value> = step["log"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|line| line["scope"] == "main:updates" && line["exact"] == true)
                .map(|line| {
                    json!({
                        "level": line["level"],
                        "message": line["message"],
                        "meta": line.get("meta").cloned().unwrap_or(Value::Null),
                    })
                })
                .collect();
            assert_eq!(lines[lines_before..], recorded_lines, "{at}: log");
            lines_before = lines.len();

            if let Some(recorded) = step.get("answer").filter(|answer| !answer.is_null()) {
                assert_eq!(answer.as_ref(), Some(recorded), "{at}: answer");
            }
        }
        self.check_folder();
    }

    // What a run leaves in the folder is advisory by the README. Here the pending folder holds the installer alone after `ready`, without electron-updater's `update-info.json`, and nothing after a failure or a cancel.
    fn check_folder(&self) {
        let recorded = self.recording["recorded"]["dataFolder"]["updateCache"]
            .as_array()
            .unwrap();
        let ready = recorded.iter().any(|entry| {
            self.installer_name
                .as_ref()
                .is_some_and(|name| entry["path"] == format!("update-cache/pending/{name}"))
        });
        let expected: Vec<String> = if ready {
            vec![self.installer_name.clone().unwrap()]
        } else {
            Vec::new()
        };
        assert_eq!(
            self.pending_files(),
            expected,
            "{}: the pending folder",
            self.condition
        );
    }
}

fn parse_line(line: &str) -> Option<Value> {
    let (_, rest) = line.split_once(' ')?;
    let (level, rest) = rest.split_once(" [")?;
    let (scope, rest) = rest.split_once("] ")?;
    if scope != "main:updates" {
        return None;
    }
    let (message, meta) = match rest.find(" {") {
        Some(at) => (
            &rest[..at],
            serde_json::from_str::<Value>(&rest[at + 1..]).unwrap(),
        ),
        None => (rest, Value::Null),
    };
    Some(json!({ "level": level.trim(), "message": message, "meta": meta }))
}

fn replay(condition: &str) -> Replay {
    let replay = Replay::new(condition);
    replay.run();
    replay
}

macro_rules! recordings {
    ($($test:ident => $condition:literal),* $(,)?) => {
        $(
            #[test]
            fn $test() {
                replay($condition);
            }
        )*

        #[test]
        fn every_recording_is_replayed() {
            let mut named = vec![$($condition),*];
            named.sort_unstable();
            let mut on_disk: Vec<String> = fs::read_dir(oracle("l2/updates"))
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
                .map(|path| path.file_stem().unwrap().to_string_lossy().into_owned())
                .collect();
            on_disk.sort();
            assert_eq!(named, on_disk);
            assert_eq!(named.len(), 23, "the README lists 23 conditions");
        }
    };
}

recordings! {
    admin_rights => "admin-rights",
    auto_off_in_flight => "auto-off-in-flight",
    bad_signature => "bad-signature",
    feed_http_503 => "feed-http-503",
    file_count => "file-count",
    file_entry => "file-entry",
    file_name => "file-name",
    file_sha512 => "file-sha512",
    file_size => "file-size",
    hash_mismatch => "hash-mismatch",
    key_id_format => "key-id-format",
    network_failure => "network-failure",
    no_keys => "no-keys",
    no_manifest => "no-manifest",
    not_an_object => "not-an-object",
    signature_format => "signature-format",
    unsigned => "unsigned",
    up_to_date => "up-to-date",
    update_ready => "update-ready",
    version_format => "version-format",
    web_installer => "web-installer",
    wrong_key => "wrong-key",
    auto_off_during_download => "auto-off-during-download",
}

// The recorded installer request was held open until quit; here the cancel ends it and removes the temp file (triage X-18), and the request still counts as asked. The folder is empty right after the cancel.
#[test]
fn auto_off_during_download_ends_the_request_and_leaves_no_temp_file() {
    let replay = Replay::new("auto-off-during-download");
    replay.shared.event(Updates::start);
    replay.fire();
    assert_eq!(
        replay.shared.machine.lock().unwrap().status(),
        UpdateStatus::Idle
    );
    assert_eq!(
        *replay.reached.lock().unwrap(),
        [
            LATEST.to_owned(),
            format!("{RELEASE}/latest.yml"),
            format!("{RELEASE}/OneLaunch-1.0.0-beta.4-setup.exe"),
        ]
    );
    assert_eq!(replay.pending_files(), Vec::<String>::new());
}
