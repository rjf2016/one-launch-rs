#![forbid(unsafe_code)]

use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signer, SigningKey};
use ol_formats::latest_yml::Yaml;
use ol_platform_api::Clock;
use ol_platform_api::update_http::ReleaseTag;
use ol_platform_fake::{FakeClock, InstallerAsked, RecordingInstaller};
use ol_services::install::{UpdateStatus, UpdateStep, UpdateUnavailable};
use ol_services::log::{LogOptions, Logger};
use ol_services::updates::check::Checked;
use ol_services::updates::download::Fetched;
use ol_services::updates::failure::UpdateFailure;
use ol_services::updates::manifest::{ManifestFile, ReleaseKey, canonical_payload, key_id_of};
use ol_services::updates::service::{
    CHECK_EVERY, FIRST_CHECK, UpdateEffect, Updates, UpdatesSetup, running_version,
};
use ol_testkit::Scratch;
use semver::Version;
use serde_json::{Value, json};
use sha2::{Digest, Sha512};
use time::macros::datetime;

const WAIT: Duration = Duration::from_secs(10);
const NAME: &str = "OneLaunch-1.0.0-beta.2-setup.exe";
const VERSION: &str = "1.0.0-beta.2";
const BYTES: &[u8] = b"v2";

const ED25519_SPKI_PREFIX: [u8; 12] = [
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
];

struct Pair {
    signing: SigningKey,
    spki: String,
    id: String,
}

impl Pair {
    fn new(seed: u8) -> Self {
        let signing = SigningKey::from_bytes(&[seed; 32]);
        let mut der = ED25519_SPKI_PREFIX.to_vec();
        der.extend(signing.verifying_key().to_bytes());
        Self {
            signing,
            spki: STANDARD.encode(&der),
            id: key_id_of(&der),
        }
    }

    fn key(&self) -> ReleaseKey<'_> {
        ReleaseKey {
            id: &self.id,
            spki: &self.spki,
        }
    }
}

fn sha512(bytes: &[u8]) -> String {
    STANDARD.encode(Sha512::digest(bytes))
}

fn release_file() -> ManifestFile {
    ManifestFile {
        url: NAME.to_owned(),
        sha512: sha512(BYTES),
        size: BYTES.len() as u64,
    }
}

fn text(value: &str) -> Yaml {
    Yaml::Text(value.to_owned())
}

fn manifest(signer: Option<&Pair>) -> Yaml {
    let file = release_file();
    let mut entries = vec![
        ("version".to_owned(), text(VERSION)),
        (
            "files".to_owned(),
            Yaml::List(vec![Yaml::Map(vec![
                ("url".to_owned(), text(&file.url)),
                ("sha512".to_owned(), text(&file.sha512)),
                ("size".to_owned(), Yaml::Number(file.size.to_string())),
            ])]),
        ),
    ];
    if let Some(signer) = signer {
        let signature = signer
            .signing
            .sign(canonical_payload(VERSION, &file).as_bytes());
        entries.push(("keyId".to_owned(), text(&signer.id)));
        entries.push((
            "signature".to_owned(),
            Yaml::Text(STANDARD.encode(signature.to_bytes())),
        ));
    }
    Yaml::Map(entries)
}

fn newer(manifest: Yaml) -> Result<Checked, UpdateFailure> {
    Ok(Checked::Newer {
        tag: ReleaseTag::new("v1.0.0-beta.2").unwrap(),
        version: Version::parse(VERSION).unwrap(),
        manifest,
    })
}

#[derive(Clone, Debug, PartialEq)]
struct Line {
    level: String,
    message: String,
    meta: Option<Value>,
}

fn parse(line: &str) -> Option<Line> {
    let (_, rest) = line.split_once(' ')?;
    let (level, rest) = rest.split_once(" [")?;
    let (scope, rest) = rest.split_once("] ")?;
    if scope != "main:updates" {
        return None;
    }
    let (message, meta) = match rest.find(" {") {
        Some(at) => (
            &rest[..at],
            Some(serde_json::from_str(&rest[at + 1..]).unwrap()),
        ),
        None => (rest, None),
    };
    Some(Line {
        level: level.trim().to_owned(),
        message: message.to_owned(),
        meta,
    })
}

fn line(level: &str, message: &str, meta: Option<Value>) -> Line {
    Line {
        level: level.to_owned(),
        message: message.to_owned(),
        meta,
    }
}

struct Download {
    tag: ReleaseTag,
    file: ManifestFile,
    cancelled: bool,
}

enum Answer {
    Checked(Result<Checked, UpdateFailure>),
    Downloaded(Result<Fetched, UpdateFailure>),
}

type CheckAnswer = Box<dyn FnMut() -> Result<Checked, UpdateFailure>>;

// The Electron tests' fake engine and fake timers: a check is answered at the next settle by `answer`, a download waits for the test, a cancel answers its download at once as cancelled, and the one timer fires when `advance` passes it.
struct Rig {
    updates: Updates,
    scratch: Scratch,
    logger: Logger,
    installer: RecordingInstaller,
    answer: CheckAnswer,
    checks: usize,
    downloads: Vec<Download>,
    quits: usize,
    now: Duration,
    due: Option<Duration>,
    queue: VecDeque<Answer>,
}

struct Options {
    auto: bool,
    this_copy: Result<(), UpdateUnavailable>,
    installer: RecordingInstaller,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            auto: true,
            this_copy: Ok(()),
            installer: RecordingInstaller::new(),
        }
    }
}

fn setup(options: Options) -> Rig {
    let scratch = Scratch::new("update-service");
    let logger = Logger::open(&scratch.join("logs"), LogOptions::default());
    let signer = Pair::new(7);
    let clock: Arc<dyn Clock> =
        Arc::new(FakeClock::starting_at(datetime!(2026-10-01 12:00:00 UTC)));
    let updates = Updates::new(UpdatesSetup {
        version: Version::parse("1.0.0-beta.1").unwrap(),
        keys: &[signer.key()],
        auto: options.auto,
        this_copy: options.this_copy,
        log: &logger.log(),
        clock,
    });
    Rig {
        updates,
        scratch,
        logger,
        installer: options.installer,
        answer: Box::new(|| Ok(Checked::UpToDate)),
        checks: 0,
        downloads: Vec::new(),
        quits: 0,
        now: Duration::ZERO,
        due: None,
        queue: VecDeque::new(),
    }
}

fn signed() -> Result<Checked, UpdateFailure> {
    newer(manifest(Some(&Pair::new(7))))
}

impl Rig {
    fn path(&self) -> PathBuf {
        self.scratch.join("pending").join(NAME)
    }

    fn lines(&self) -> Vec<Line> {
        assert!(self.logger.flush(WAIT));
        fs::read_to_string(self.logger.current_path())
            .unwrap_or_default()
            .lines()
            .filter_map(parse)
            .collect()
    }

    fn levels(&self, level: &str) -> Vec<String> {
        self.lines()
            .into_iter()
            .filter(|line| line.level == level)
            .map(|line| line.message)
            .collect()
    }

    fn apply(&mut self, effects: Vec<UpdateEffect>) {
        for effect in effects {
            match effect {
                UpdateEffect::Arm(delay) => {
                    assert!(self.due.is_none(), "armed over an armed timer");
                    self.due = Some(self.now + delay);
                }
                UpdateEffect::Disarm => {
                    assert!(self.due.take().is_some(), "disarmed with no timer armed");
                }
                UpdateEffect::Check => {
                    self.checks += 1;
                    let answer = (self.answer)();
                    self.queue.push_back(Answer::Checked(answer));
                }
                UpdateEffect::Download { tag, file } => self.downloads.push(Download {
                    tag,
                    file,
                    cancelled: false,
                }),
                UpdateEffect::CancelDownload => {
                    let last = self.downloads.last_mut().expect("a download to cancel");
                    assert!(!last.cancelled, "cancelled twice");
                    last.cancelled = true;
                    self.queue
                        .push_back(Answer::Downloaded(Ok(Fetched::Cancelled)));
                }
                UpdateEffect::Quit => self.quits += 1,
            }
        }
    }

    fn settle(&mut self) {
        while let Some(answer) = self.queue.pop_front() {
            let effects = match answer {
                Answer::Checked(result) => self.updates.checked(result),
                Answer::Downloaded(result) => self.updates.downloaded(result),
            };
            self.apply(effects);
        }
    }

    fn advance(&mut self, by: Duration) {
        let until = self.now + by;
        while let Some(due) = self.due.filter(|due| *due <= until) {
            self.now = due;
            self.due = None;
            let effects = self.updates.timer_fired();
            self.apply(effects);
            self.settle();
        }
        self.now = until;
    }

    fn start(&mut self) {
        let effects = self.updates.start();
        self.apply(effects);
        self.settle();
    }

    fn check(&mut self) -> UpdateStatus {
        let (answer, effects) = self.updates.check();
        self.apply(effects);
        answer
    }

    fn set_auto(&mut self, on: bool) {
        let effects = self.updates.set_auto(on);
        self.apply(effects);
    }

    fn stop(&mut self) {
        let effects = self.updates.stop();
        self.apply(effects);
    }

    fn resolve(&mut self, result: Result<Fetched, UpdateFailure>) {
        let effects = self.updates.downloaded(result);
        self.apply(effects);
        self.settle();
    }

    fn resolve_ready(&mut self) {
        let path = self.path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, BYTES).unwrap();
        self.resolve(Ok(Fetched::Ready {
            path,
            sha512: sha512(BYTES),
        }));
    }

    fn request_install(&mut self) {
        let effects = self.updates.request_install().expect("a verified update");
        self.apply(effects);
    }

    fn install_at_quit(&mut self, exit_code: i32) {
        self.updates.install_at_quit(exit_code, &self.installer);
    }

    fn installs(&self) -> Vec<bool> {
        self.installer
            .asked()
            .iter()
            .map(|asked| asked.args.iter().any(|arg| arg == "--force-run"))
            .collect()
    }
}

fn ready_rig(options: Options) -> Rig {
    let mut rig = setup(options);
    rig.answer = Box::new(signed);
    rig.check();
    rig.settle();
    rig.resolve_ready();
    assert_eq!(
        rig.updates.status(),
        UpdateStatus::Ready {
            version: VERSION.to_owned()
        }
    );
    rig
}

fn auto_off() -> Options {
    Options {
        auto: false,
        ..Options::default()
    }
}

mod when_it_checks {
    use super::*;

    #[test]
    fn checks_nothing_on_its_own_while_automatic_updates_are_off_but_the_button_still_checks() {
        let mut rig = setup(auto_off());
        rig.start();
        rig.advance(CHECK_EVERY * 2);
        assert_eq!(rig.checks, 0);
        assert_eq!(rig.check(), UpdateStatus::Checking);
        rig.settle();
        assert_eq!(rig.checks, 1);
        assert_eq!(
            rig.updates.status(),
            UpdateStatus::UpToDate {
                checked_at: "2026-10-01T12:00:00.000Z".to_owned()
            }
        );
    }

    #[test]
    fn checks_15_s_after_the_start_then_every_6_hours() {
        let mut rig = setup(Options::default());
        rig.start();
        rig.advance(FIRST_CHECK - Duration::from_millis(1));
        assert_eq!(rig.checks, 0);
        rig.advance(Duration::from_millis(1));
        assert_eq!(rig.checks, 1);
        rig.advance(CHECK_EVERY);
        assert_eq!(rig.checks, 2);
        assert_eq!(FIRST_CHECK, Duration::from_millis(15_000));
        assert_eq!(CHECK_EVERY, Duration::from_millis(21_600_000));
    }

    #[test]
    fn starts_checking_when_automatic_updates_are_turned_on_and_stops_when_they_are_turned_off() {
        let mut rig = setup(auto_off());
        rig.start();
        rig.set_auto(true);
        rig.advance(FIRST_CHECK);
        assert_eq!(rig.checks, 1);
        rig.set_auto(false);
        rig.advance(CHECK_EVERY * 2);
        assert_eq!(rig.checks, 1);
    }

    #[test]
    fn never_loads_the_updater_for_a_copy_that_cannot_update_itself() {
        let unavailable = UpdateStatus::Unavailable(UpdateUnavailable::CustomDataFolder);
        let mut rig = setup(Options {
            this_copy: Err(UpdateUnavailable::CustomDataFolder),
            ..Options::default()
        });
        assert!(rig.updates.start().is_empty());
        assert_eq!(rig.updates.status(), unavailable);
        let (answer, effects) = rig.updates.check();
        assert_eq!((answer, effects), (unavailable.clone(), Vec::new()));
        assert!(rig.updates.set_auto(false).is_empty());
        assert!(rig.updates.set_auto(true).is_empty());
        assert!(rig.updates.timer_fired().is_empty());
        assert!(rig.updates.stop().is_empty());
        rig.advance(CHECK_EVERY * 2);
        assert_eq!(rig.checks, 0);
        assert!(rig.downloads.is_empty());
        assert_eq!(rig.updates.status(), unavailable);
        assert_eq!(
            rig.lines(),
            [line(
                "INFO",
                "updates unavailable",
                Some(json!({ "reason": "custom-data-folder" }))
            )]
        );
    }

    #[test]
    fn stops_checking_once_a_verified_update_is_waiting() {
        let mut rig = ready_rig(Options::default());
        rig.start();
        rig.advance(CHECK_EVERY * 3);
        assert_eq!(rig.checks, 1);
        assert_eq!(
            rig.check(),
            UpdateStatus::Ready {
                version: VERSION.to_owned()
            }
        );
        assert_eq!(rig.checks, 1);
    }

    #[test]
    fn stop_ends_the_schedule_and_cancels_a_download() {
        let mut rig = setup(Options::default());
        rig.answer = Box::new(signed);
        rig.start();
        rig.advance(FIRST_CHECK);
        assert_eq!(rig.downloads.len(), 1);
        rig.stop();
        rig.settle();
        assert!(rig.downloads[0].cancelled);
        rig.advance(CHECK_EVERY * 2);
        assert_eq!(rig.checks, 1);
    }
}

mod what_it_downloads {
    use super::*;

    #[test]
    fn downloads_a_signed_update_reports_progress_and_is_ready_once_the_bytes_hash_to_the_signed_value()
     {
        let mut rig = setup(Options::default());
        rig.answer = Box::new(signed);
        rig.check();
        rig.settle();
        let downloading = |percent| UpdateStatus::Downloading {
            version: VERSION.to_owned(),
            percent,
        };
        assert_eq!(rig.updates.status(), downloading(0));
        assert_eq!(rig.downloads[0].tag.as_str(), "v1.0.0-beta.2");
        assert_eq!(rig.downloads[0].file, release_file());
        rig.updates.progress(42);
        assert_eq!(rig.updates.status(), downloading(42));
        rig.resolve_ready();
        assert_eq!(
            rig.updates.status(),
            UpdateStatus::Ready {
                version: VERSION.to_owned()
            }
        );
        assert_eq!(rig.levels("ERROR"), Vec::<String>::new());
    }

    #[test]
    fn never_downloads_an_unsigned_update_or_one_signed_with_another_key() {
        let mut rig = setup(Options::default());
        rig.answer = Box::new(|| newer(manifest(None)));
        rig.check();
        rig.settle();
        assert_eq!(
            rig.updates.status(),
            UpdateStatus::Failed {
                step: UpdateStep::Verify,
                message: "the release is not signed".to_owned()
            }
        );

        rig.answer = Box::new(|| newer(manifest(Some(&Pair::new(9)))));
        rig.check();
        rig.settle();
        assert!(matches!(
            rig.updates.status(),
            UpdateStatus::Failed {
                step: UpdateStep::Verify,
                ..
            }
        ));
        assert!(rig.downloads.is_empty());
        assert_eq!(
            rig.levels("WARN"),
            ["update verify failed", "update verify failed"]
        );
        assert_eq!(rig.levels("ERROR"), Vec::<String>::new());
    }

    #[test]
    fn refuses_a_download_whose_bytes_do_not_hash_to_the_signed_value() {
        let mut rig = setup(Options::default());
        rig.answer = Box::new(signed);
        rig.check();
        rig.settle();
        let other = sha512(b"something else");
        rig.resolve(Ok(Fetched::Ready {
            path: rig.path(),
            sha512: other.clone(),
        }));
        assert_eq!(
            rig.updates.status(),
            UpdateStatus::Failed {
                step: UpdateStep::Verify,
                message: "the downloaded file does not match the signed release".to_owned()
            }
        );
        assert_eq!(
            rig.lines().last(),
            Some(&line(
                "WARN",
                "update verify failed",
                Some(json!({
                    "version": VERSION,
                    "sha512": &other[..12],
                    "message": "the downloaded file does not match the signed release",
                }))
            ))
        );
        rig.install_at_quit(0);
        assert_eq!(rig.installs(), Vec::<bool>::new());
    }

    #[test]
    fn turning_automatic_updates_off_cancels_a_background_download_not_one_the_user_asked_for() {
        let mut background = setup(Options::default());
        background.answer = Box::new(signed);
        background.start();
        background.advance(FIRST_CHECK);
        background.set_auto(false);
        background.settle();
        assert!(background.downloads[0].cancelled);
        assert_eq!(background.updates.status(), UpdateStatus::Idle);
        assert!(background.lines().contains(&line(
            "INFO",
            "update download cancelled: automatic updates turned off",
            None
        )));

        let mut manual = setup(Options::default());
        manual.answer = Box::new(signed);
        manual.check();
        manual.settle();
        manual.set_auto(false);
        assert!(!manual.downloads[0].cancelled);
    }

    #[test]
    fn reports_failures_as_warnings_in_plain_words_never_as_errors() {
        let mut rig = setup(Options::default());
        rig.answer = Box::new(|| Err(UpdateFailure::Unreachable));
        rig.check();
        rig.settle();
        assert_eq!(
            rig.updates.status(),
            UpdateStatus::Failed {
                step: UpdateStep::Check,
                message: "the update server could not be reached".to_owned()
            }
        );

        rig.answer = Box::new(signed);
        rig.check();
        rig.settle();
        rig.resolve(Err(UpdateFailure::Status(503)));
        assert_eq!(
            rig.updates.status(),
            UpdateStatus::Failed {
                step: UpdateStep::Download,
                message: "the update server answered HTTP 503".to_owned()
            }
        );
        assert_eq!(rig.levels("ERROR"), Vec::<String>::new());
        assert_eq!(
            rig.levels("WARN"),
            ["update check failed", "update download failed"]
        );
    }
}

mod when_it_installs {
    use super::*;

    #[test]
    fn installs_at_quit_when_automatic_updates_are_on_without_starting_one_launch_again() {
        let mut rig = ready_rig(Options::default());
        rig.install_at_quit(0);
        assert_eq!(rig.installs(), [false]);
    }

    #[test]
    fn restart_to_update_quits_then_installs_and_starts_one_launch_again() {
        let mut rig = ready_rig(auto_off());
        rig.request_install();
        assert_eq!(rig.quits, 1);
        rig.install_at_quit(0);
        assert_eq!(rig.installs(), [true]);
    }

    #[test]
    fn waits_for_restart_to_update_when_automatic_updates_are_off() {
        let mut rig = ready_rig(auto_off());
        rig.install_at_quit(0);
        assert_eq!(rig.installs(), Vec::<bool>::new());
        assert_eq!(
            rig.lines().last(),
            Some(&line(
                "INFO",
                "update left for a restart: automatic updates are off",
                Some(json!({ "version": VERSION }))
            ))
        );
    }

    #[test]
    fn never_installs_during_a_windows_shutdown_after_a_crash_or_on_a_failed_exit() {
        let mut shutdown = ready_rig(Options::default());
        shutdown.updates.on_session_end();
        shutdown.install_at_quit(0);
        assert_eq!(shutdown.installs(), Vec::<bool>::new());
        assert_eq!(
            shutdown.lines().last(),
            Some(&line(
                "INFO",
                "update not installed at quit",
                Some(
                    json!({ "version": VERSION, "reason": "Windows is shutting down or signing out" })
                )
            ))
        );

        let mut crash = ready_rig(Options::default());
        crash.updates.on_crash();
        crash.request_install();
        crash.install_at_quit(0);
        assert_eq!(crash.installs(), Vec::<bool>::new());
        assert_eq!(
            crash.lines().last(),
            Some(&line(
                "INFO",
                "update not installed at quit",
                Some(
                    json!({ "version": VERSION, "reason": "an unexpected error happened in this run" })
                )
            ))
        );

        let mut failed = ready_rig(Options::default());
        failed.install_at_quit(1);
        assert_eq!(failed.installs(), Vec::<bool>::new());
        assert_eq!(
            failed.lines().last(),
            Some(&line(
                "INFO",
                "update not installed at quit",
                Some(json!({ "version": VERSION, "exitCode": 1 }))
            ))
        );
    }

    #[test]
    fn hashes_the_file_again_right_before_running_it() {
        let mut rig = ready_rig(Options::default());
        fs::write(rig.path(), b"tampered").unwrap();
        rig.install_at_quit(0);
        assert_eq!(rig.installs(), Vec::<bool>::new());
        assert_eq!(
            rig.lines().last(),
            Some(&line(
                "WARN",
                "update install refused: the downloaded file changed after it was verified",
                Some(json!({ "version": VERSION }))
            ))
        );
    }

    #[test]
    fn refuses_an_install_request_before_anything_is_verified() {
        let mut rig = setup(Options::default());
        let refused = rig.updates.request_install().unwrap_err();
        assert_eq!(refused.code(), "NOT_READY");
        assert_eq!(
            refused.to_string(),
            "No verified update has been downloaded yet"
        );
        assert_eq!(rig.quits, 0);
        assert!(rig.lines().is_empty());
    }
}

mod the_installer {
    use super::*;

    fn started(rig: &Rig) -> InstallerAsked {
        let asked = rig.installer.asked();
        assert_eq!(asked.len(), 1, "the installer is started once");
        asked[0].clone()
    }

    #[test]
    fn without_a_restart_it_is_started_updated_and_silent_and_nothing_more() {
        let mut rig = ready_rig(Options::default());
        rig.install_at_quit(0);
        assert_eq!(
            started(&rig),
            InstallerAsked {
                installer: rig.path(),
                args: vec!["--updated".to_owned(), "/S".to_owned()],
            }
        );
        assert_eq!(
            rig.lines().last(),
            Some(&line(
                "INFO",
                "update install",
                Some(json!({ "version": VERSION, "relaunch": false, "started": true }))
            ))
        );
    }

    #[test]
    fn after_restart_to_update_force_run_comes_last() {
        let mut rig = ready_rig(Options::default());
        rig.request_install();
        rig.install_at_quit(0);
        assert_eq!(
            started(&rig),
            InstallerAsked {
                installer: rig.path(),
                args: vec![
                    "--updated".to_owned(),
                    "/S".to_owned(),
                    "--force-run".to_owned()
                ],
            }
        );
        assert_eq!(
            rig.lines().last(),
            Some(&line(
                "INFO",
                "update install",
                Some(json!({ "version": VERSION, "relaunch": true, "started": true }))
            ))
        );
    }

    #[test]
    fn a_start_windows_refused_is_written_as_not_started() {
        let mut rig = ready_rig(Options {
            installer: RecordingInstaller::refusing(),
            ..Options::default()
        });
        rig.install_at_quit(0);
        assert_eq!(started(&rig).args, ["--updated", "/S"]);
        assert_eq!(
            rig.lines().last(),
            Some(&line(
                "INFO",
                "update install",
                Some(json!({ "version": VERSION, "relaunch": false, "started": false }))
            ))
        );
    }

    #[test]
    fn nothing_is_started_at_quit_without_a_verified_update() {
        let mut rig = setup(Options::default());
        rig.request_install_refused();
        rig.install_at_quit(0);
        assert!(rig.installer.asked().is_empty());
        assert!(rig.lines().is_empty());
    }

    #[test]
    fn an_installer_that_cannot_be_read_at_quit_is_not_started() {
        let mut rig = ready_rig(Options::default());
        fs::remove_file(rig.path()).unwrap();
        rig.install_at_quit(0);
        assert!(rig.installer.asked().is_empty());
        let last = rig.lines().pop().unwrap();
        assert_eq!(
            (last.level.as_str(), last.message.as_str()),
            (
                "WARN",
                "update install refused: the downloaded file cannot be read"
            )
        );
        let meta = last.meta.unwrap();
        assert_eq!(meta["version"], VERSION);
        assert!(meta["error"].is_string());
    }

    #[test]
    fn a_failed_exit_after_restart_to_update_is_not_installed_either() {
        let mut rig = ready_rig(auto_off());
        rig.request_install();
        rig.install_at_quit(3);
        assert!(rig.installer.asked().is_empty());
    }
}

impl Rig {
    fn request_install_refused(&mut self) {
        assert!(self.updates.request_install().is_err());
    }
}

mod after_stop {
    use super::*;

    #[test]
    fn answers_that_come_after_stop_change_nothing() {
        let mut rig = setup(Options::default());
        rig.answer = Box::new(signed);
        rig.check();
        rig.stop();
        let checking = rig.updates.status();
        assert_eq!(checking, UpdateStatus::Checking);
        assert!(rig.updates.checked(signed()).is_empty());
        rig.queue.clear();
        assert_eq!(rig.updates.status(), checking);
        assert!(rig.downloads.is_empty());

        let mut rig = setup(Options::default());
        rig.answer = Box::new(signed);
        rig.start();
        rig.advance(FIRST_CHECK);
        rig.stop();
        rig.queue.clear();
        let downloading = rig.updates.status();
        rig.resolve_ready();
        assert_eq!(rig.updates.status(), downloading);
        assert!(rig.updates.request_install().is_err());
        rig.install_at_quit(0);
        assert!(rig.installer.asked().is_empty());
        assert_eq!(rig.updates.check().0, downloading);
        assert!(rig.updates.timer_fired().is_empty());
        assert!(rig.updates.set_auto(false).is_empty());
        rig.updates.progress(50);
        assert_eq!(rig.updates.status(), downloading);
    }

    #[test]
    fn a_timer_that_fires_during_a_manual_check_waits_for_it_then_arms_six_hours() {
        let mut rig = setup(Options::default());
        rig.start();
        rig.check();
        let effects = rig.updates.timer_fired();
        assert!(effects.is_empty(), "nothing starts while a check runs");
        rig.due = None;
        assert_eq!(rig.checks, 1);
        rig.settle();
        assert_eq!(rig.due, Some(CHECK_EVERY));
    }
}

#[test]
fn a_copy_whose_own_version_is_not_a_semantic_version_gets_no_updates() {
    let installed: Result<(), UpdateUnavailable> = Ok(());
    let (version, this_copy) = running_version("1.0.0-beta.3", installed);
    assert_eq!(
        (version, this_copy),
        (Version::parse("1.0.0-beta.3").unwrap(), Ok(()))
    );
    for odd in ["", "v1.0.0", "1.0", "not a version", " 1.0.0"] {
        let (_, this_copy) = running_version(odd, installed);
        assert_eq!(this_copy, Err(UpdateUnavailable::NotInstalled), "{odd:?}");
    }
    let development = Err(UpdateUnavailable::Development);
    assert_eq!(running_version("1.0.0", development).1, development);
    assert_eq!(running_version("odd", development).1, development);
}
