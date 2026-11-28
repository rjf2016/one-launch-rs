#![forbid(unsafe_code)]

// The updater through the backend, on a release feed, an installer, a timer and a clock that exist only in the test: what a copy without a kit does, the background check and the button, automatic updates turned off and on, the install, and what a quit starts. The feed is the fake build's own, `ol_sim::updates`, signed by the fixtures' throwaway key; a test that needs a feed of its own wraps it.

use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use futures_lite::future::block_on;
use ol_api::launchers::{Launchers, SteamTiming};
use ol_api::log::{LogOptions, Logger};
use ol_api::settings::{UpdateStatus, UpdateStep, UpdateUnavailable};
use ol_api::updates::{Timer, TrustedKey, UpdatesKit};
use ol_api::{ApiError, Backend, StartLaunch};
use ol_model::{JobState, LaunchJob, LauncherId, SettingKey};
use ol_platform_api::http::HttpError;
use ol_platform_api::update_http::{UpdateAsk, UpdateRequest};
use ol_platform_api::{CancelToken, Clock};
use ol_platform_fake::update_http::{FakeAnswer, Part, ScriptedUpdates, in_parts};
use ol_platform_fake::{FakeClock, InstallerAsked, RecordingInstaller};
use ol_sim::updates::{INSTALLER, INSTALLER_NAME, UpdateStory, release_key, respond};
use ol_testkit::{Scratch, oracle};
use ol_vault::FakeVault;
use time::macros::datetime;

const VERSION: &str = "1.0.0-beta.3";
const RELEASED: &str = "1.0.0-beta.4";
const WAIT: Duration = Duration::from_secs(20);
const FIRST_CHECK: Duration = Duration::from_secs(15);

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

type Run = Box<dyn FnOnce() + Send>;

// Keeps what was armed, and fires one when the test says, cancelled or not: a timer that ends just as it is disarmed.
#[derive(Default)]
struct Timers(Mutex<Vec<(Duration, CancelToken, Option<Run>)>>);

impl Timer for Timers {
    fn after(&self, delay: Duration, cancel: CancelToken, run: Run) {
        lock(&self.0).push((delay, cancel, Some(run)));
    }
}

impl Timers {
    fn count(&self) -> usize {
        lock(&self.0).len()
    }

    // Armed, not disarmed and not yet fired.
    fn live(&self) -> Vec<Duration> {
        lock(&self.0)
            .iter()
            .filter(|(_, cancel, run)| run.is_some() && !cancel.is_cancelled())
            .map(|(delay, ..)| *delay)
            .collect()
    }

    fn fire(&self, at: usize) {
        let run = lock(&self.0)[at].2.take().expect("a timer not yet fired");
        run();
    }

    fn fire_the_live_one(&self) {
        let at = {
            let armed = lock(&self.0);
            let live: Vec<usize> = (0..armed.len())
                .filter(|at| armed[*at].2.is_some() && !armed[*at].1.is_cancelled())
                .collect();
            assert_eq!(live.len(), 1, "one timer is armed");
            live[0]
        };
        self.fire(at);
    }
}

type Feed = Box<dyn Fn(&UpdateRequest) -> Result<FakeAnswer, HttpError> + Send + Sync>;

struct Rig {
    scratch: Scratch,
    backend: Backend,
    logger: Logger,
    http: Arc<ScriptedUpdates>,
    installer: Arc<RecordingInstaller>,
    timers: Arc<Timers>,
    clock: Arc<FakeClock>,
}

fn story(story: UpdateStory) -> impl FnOnce(Arc<dyn Clock>) -> Feed {
    move |clock| Box::new(move |request| respond(story, VERSION, &clock, request))
}

fn keys() -> Vec<TrustedKey> {
    let (id, spki) = release_key();
    vec![TrustedKey { id, spki }]
}

impl Rig {
    fn new(name: &str, feed: impl FnOnce(Arc<dyn Clock>) -> Feed) -> Self {
        Self::starting(name, feed, |data_dir, logger| {
            Backend::logging_to(logger.log()).start(data_dir, VERSION)
        })
    }

    fn starting(
        name: &str,
        feed: impl FnOnce(Arc<dyn Clock>) -> Feed,
        start: impl FnOnce(PathBuf, &Logger) -> Backend,
    ) -> Self {
        let scratch = Scratch::new(name);
        let data_dir = scratch.join("data");
        let logger = Logger::for_data_folder(&data_dir, LogOptions::default());
        let backend = start(data_dir, &logger);
        let clock = Arc::new(FakeClock::starting_at(datetime!(2026-11-06 16:46:23 UTC)));
        let respond = feed(Arc::clone(&clock) as Arc<dyn Clock>);
        Self {
            scratch,
            backend,
            logger,
            http: Arc::new(ScriptedUpdates::new(respond)),
            installer: Arc::new(RecordingInstaller::new()),
            timers: Arc::default(),
            clock,
        }
    }

    fn kit(&self, this_copy: Result<(), UpdateUnavailable>) -> UpdatesKit {
        UpdatesKit {
            http: Arc::clone(&self.http) as _,
            installer: Arc::clone(&self.installer) as _,
            clock: Arc::clone(&self.clock) as _,
            timer: Arc::clone(&self.timers) as _,
            pending: self.pending(),
            keys: keys(),
            this_copy,
        }
    }

    // The kit of an installed copy, handed in and taken.
    fn handed_in(self) -> Self {
        self.backend.use_updates(self.kit(Ok(())));
        self.status();
        self
    }

    fn status(&self) -> UpdateStatus {
        block_on(self.backend.update_status_now()).unwrap()
    }

    fn check(&self) -> UpdateStatus {
        block_on(self.backend.check_updates()).unwrap()
    }

    fn set_auto(&self, on: bool) {
        let value = if on { "true" } else { "false" };
        block_on(
            self.backend
                .set_setting(SettingKey::UpdatesAuto, value.to_owned()),
        )
        .unwrap();
    }

    fn pending(&self) -> PathBuf {
        self.scratch.join("pending")
    }

    fn pending_files(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.pending())
            .map(|entries| {
                entries
                    .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    fn log_text(&self) -> String {
        assert!(self.logger.flush(WAIT));
        fs::read_to_string(self.logger.current_path()).unwrap()
    }

    fn starts(&self) -> Vec<InstallerAsked> {
        self.installer.asked()
    }

    // A verified update waits, found by the button.
    fn ready(self) -> Self {
        let heard = self.backend.update_changes();
        assert_eq!(self.check(), UpdateStatus::Checking);
        let seen = until(&heard, is_over);
        assert_eq!(
            seen.last(),
            Some(&UpdateStatus::Ready {
                version: RELEASED.to_owned()
            })
        );
        self
    }
}

fn is_over(status: &UpdateStatus) -> bool {
    !matches!(
        status,
        UpdateStatus::Idle | UpdateStatus::Checking | UpdateStatus::Downloading { .. }
    )
}

// Every status heard, up to and with the first `done` is true of.
fn until(
    heard: &async_channel::Receiver<UpdateStatus>,
    done: impl Fn(&UpdateStatus) -> bool,
) -> Vec<UpdateStatus> {
    let started = Instant::now();
    let mut seen = Vec::new();
    loop {
        match heard.try_recv() {
            Ok(status) => {
                let end = done(&status);
                seen.push(status);
                if end {
                    return seen;
                }
            }
            Err(_) => {
                assert!(started.elapsed() < WAIT, "heard only {seen:?}");
                thread::sleep(Duration::from_millis(2));
            }
        }
    }
}

fn refusal<T>(answer: Result<T, ApiError>) -> (&'static str, String) {
    let error = answer.err().expect("a refusal");
    (error.code, error.message)
}

fn not_ready() -> (&'static str, String) {
    (
        "NOT_READY",
        "No verified update has been downloaded yet".to_owned(),
    )
}

fn installed_quietly(rig: &Rig, args: &[&str]) -> Vec<InstallerAsked> {
    vec![InstallerAsked {
        installer: rig.pending().join(INSTALLER_NAME),
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
    }]
}

// The `ready` feed, but the installer's bytes wait after their first part until the test lets them go.
struct Held {
    paused: Receiver<()>,
    release: Sender<()>,
}

fn held_installer() -> (impl FnOnce(Arc<dyn Clock>) -> Feed, Held) {
    let (paused_tx, paused) = channel();
    let (release, released) = channel();
    let paused_tx = Mutex::new(paused_tx);
    let released = Arc::new(Mutex::new(released));
    let feed = move |clock: Arc<dyn Clock>| -> Feed {
        Box::new(move |request| {
            let asset =
                request.ask() == UpdateAsk::Installer && request.url().host() != "github.com";
            if !asset {
                return respond(UpdateStory::Ready, VERSION, &clock, request);
            }
            let paused = lock(&paused_tx).clone();
            let released = Arc::clone(&released);
            Ok(in_parts(
                Some(INSTALLER.len() as u64),
                vec![
                    Part::Bytes(INSTALLER[..10].to_vec()),
                    Part::pause(move || {
                        paused.send(()).unwrap();
                        let _ = lock(&released).recv();
                    }),
                    Part::Bytes(INSTALLER[10..].to_vec()),
                ],
            ))
        })
    };
    (feed, Held { paused, release })
}

fn wait_for(what: &str, done: impl Fn() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < WAIT, "{what}");
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn without_a_kit_the_copy_is_off_and_nothing_is_asked() {
    let rig = Rig::new("api-updates-no-kit", story(UpdateStory::Ready));
    let off = UpdateStatus::Unavailable(UpdateUnavailable::Development);
    let heard = rig.backend.update_changes();
    assert_eq!(rig.status(), off);
    assert_eq!(rig.backend.update_status(), off);
    assert_eq!(rig.check(), off);
    assert_eq!(refusal(block_on(rig.backend.install_update())), not_ready());
    rig.set_auto(false);
    rig.set_auto(true);
    assert_eq!(until(&heard, |_| true), [off]);
    assert!(heard.try_recv().is_err());
    rig.backend.update_crashed();
    rig.backend.shut_down();
    rig.backend.install_at_quit(0);
    assert!(rig.http.asked().is_empty());
    assert_eq!(rig.timers.count(), 0);
    assert!(rig.starts().is_empty());
    assert!(rig.log_text().contains("ipc updates:install refused"));
}

#[test]
fn a_kit_whose_copy_is_unavailable_asks_nothing_whatever_is_called() {
    let rig = Rig::new("api-updates-unavailable", story(UpdateStory::Ready));
    rig.backend
        .use_updates(rig.kit(Err(UpdateUnavailable::CustomDataFolder)));
    let off = UpdateStatus::Unavailable(UpdateUnavailable::CustomDataFolder);
    assert_eq!(rig.status(), off);
    assert_eq!(rig.check(), off);
    rig.set_auto(false);
    rig.set_auto(true);
    assert_eq!(refusal(block_on(rig.backend.install_update())), not_ready());
    assert_eq!(rig.status(), off);
    rig.backend.shut_down();
    rig.backend.install_at_quit(0);
    assert!(rig.http.asked().is_empty());
    assert_eq!(rig.timers.count(), 0);
    assert!(rig.starts().is_empty());
}

#[test]
fn a_copy_whose_version_is_not_a_semantic_version_is_not_installed_for_the_updater() {
    let rig = Rig::starting(
        "api-updates-odd-version",
        story(UpdateStory::Ready),
        |data_dir, logger| Backend::logging_to(logger.log()).start(data_dir, "a development build"),
    );
    rig.backend.use_updates(rig.kit(Ok(())));
    let off = UpdateStatus::Unavailable(UpdateUnavailable::NotInstalled);
    assert_eq!(rig.status(), off);
    assert_eq!(rig.check(), off);
    assert_eq!(rig.timers.count(), 0);
    assert!(rig.http.asked().is_empty());
    rig.backend.shut_down();
}

#[test]
fn a_second_kit_is_ignored() {
    let rig = Rig::new("api-updates-second-kit", story(UpdateStory::UpToDate)).handed_in();
    let other = Arc::new(ScriptedUpdates::new(|_| Err(HttpError::Timeout)));
    let mut kit = rig.kit(Err(UpdateUnavailable::AllUsersInstall));
    kit.http = Arc::clone(&other) as _;
    rig.backend.use_updates(kit);
    assert_eq!(rig.status(), UpdateStatus::Idle);
    assert_eq!(rig.timers.live(), [FIRST_CHECK]);
    let heard = rig.backend.update_changes();
    rig.check();
    let seen = until(&heard, is_over);
    assert!(matches!(seen.last(), Some(UpdateStatus::UpToDate { .. })));
    assert!(other.asked().is_empty());
    rig.backend.shut_down();
}

#[test]
fn the_background_check_runs_fifteen_seconds_after_the_kit_and_downloads_what_it_finds() {
    let rig = Rig::new("api-updates-background", story(UpdateStory::Ready)).handed_in();
    assert_eq!(rig.timers.live(), [FIRST_CHECK]);
    assert!(rig.http.asked().is_empty());
    let heard = rig.backend.update_changes();
    rig.timers.fire_the_live_one();
    let seen = until(&heard, is_over);
    let downloading = |percent| UpdateStatus::Downloading {
        version: RELEASED.to_owned(),
        percent,
    };
    assert_eq!(
        seen,
        [
            UpdateStatus::Idle,
            UpdateStatus::Checking,
            downloading(0),
            downloading(40),
            downloading(60),
            downloading(80),
            UpdateStatus::Ready {
                version: RELEASED.to_owned()
            },
        ]
    );
    assert_eq!(rig.timers.live(), Vec::<Duration>::new());
    assert_eq!(
        rig.timers.count(),
        1,
        "nothing is armed once an update waits"
    );
    assert_eq!(rig.pending_files(), [INSTALLER_NAME]);
    assert_eq!(rig.status(), seen[6]);
    let log = rig.log_text();
    assert!(
        log.contains("[main:updates] update check {\"origin\":\"background\"}"),
        "{log}"
    );
    assert!(log.contains("[main:updates] update ready"), "{log}");
    rig.backend.shut_down();
}

#[test]
fn the_button_answers_checking_at_once_and_the_watcher_hears_the_end() {
    let rig = Rig::new("api-updates-button", story(UpdateStory::UpToDate)).handed_in();
    let heard = rig.backend.update_changes();
    assert_eq!(rig.check(), UpdateStatus::Checking);
    let seen = until(&heard, is_over);
    assert_eq!(seen[..2], [UpdateStatus::Idle, UpdateStatus::Checking]);
    let UpdateStatus::UpToDate { checked_at } = &seen[2] else {
        panic!("{seen:?}");
    };
    assert_eq!(checked_at, "2026-11-06T16:46:23.000Z");
    assert_eq!(seen.len(), 3);
    assert_eq!(
        rig.timers.live(),
        [FIRST_CHECK],
        "the button leaves the timer as it was"
    );
    let watching_later = rig.backend.update_changes();
    assert_eq!(until(&watching_later, |_| true), [seen[2].clone()]);
    rig.backend.shut_down();
}

macro_rules! stories {
    ($($test:ident: $story:expr => $end:expr),* $(,)?) => {
        $(
            #[test]
            fn $test() {
                let story: UpdateStory = $story;
                let rig = Rig::new(&format!("api-updates-{}", story.name()), self::story(story)).handed_in();
                let heard = rig.backend.update_changes();
                assert_eq!(rig.check(), UpdateStatus::Checking);
                let seen = until(&heard, is_over);
                let end: UpdateStatus = $end;
                match (&end, seen.last().unwrap()) {
                    (UpdateStatus::UpToDate { .. }, UpdateStatus::UpToDate { .. }) => {}
                    (end, last) => assert_eq!(last, end, "{seen:?}"),
                }
                let percents: Vec<u8> = seen
                    .iter()
                    .filter_map(|status| match status {
                        UpdateStatus::Downloading { percent, .. } => Some(*percent),
                        _ => None,
                    })
                    .collect();
                if story == UpdateStory::Ready {
                    assert!(percents.iter().any(|percent| (1..100).contains(percent)), "{seen:?}");
                    assert_eq!(rig.pending_files(), [INSTALLER_NAME]);
                } else {
                    assert!(rig.pending_files().iter().all(|name| !name.starts_with("temp-")));
                }
                rig.backend.shut_down();
            }
        )*
    };
}

fn failed(step: UpdateStep, message: &str) -> UpdateStatus {
    UpdateStatus::Failed {
        step,
        message: message.to_owned(),
    }
}

stories! {
    the_story_ready_ends_ready: UpdateStory::Ready => UpdateStatus::Ready { version: RELEASED.to_owned() },
    the_story_up_to_date_ends_up_to_date: UpdateStory::UpToDate => UpdateStatus::UpToDate { checked_at: String::new() },
    the_story_unsigned_is_not_verified: UpdateStory::Unsigned => failed(UpdateStep::Verify, "the release is not signed"),
    the_story_bad_signature_is_not_verified: UpdateStory::BadSignature => failed(UpdateStep::Verify, "the release signature does not match"),
    the_story_hash_mismatch_fails_its_download: UpdateStory::HashMismatch => failed(UpdateStep::Download, "the downloaded file does not match the release"),
    the_story_no_manifest_fails_its_check: UpdateStory::NoManifest => failed(UpdateStep::Check, "the latest release has no update manifest"),
    the_story_http_503_fails_its_check: UpdateStory::Http503 => failed(UpdateStep::Check, "the update server answered HTTP 503"),
    the_story_offline_fails_its_check: UpdateStory::Offline => failed(UpdateStep::Check, "the update server could not be reached"),
}

#[test]
fn automatic_updates_turned_off_cancel_a_background_download_and_turned_on_arm_the_first_check() {
    let (feed, held) = held_installer();
    let rig = Rig::new("api-updates-auto-off", feed).handed_in();
    let heard = rig.backend.update_changes();
    rig.timers.fire_the_live_one();
    held.paused.recv_timeout(WAIT).unwrap();
    rig.set_auto(false);
    held.release.send(()).unwrap();
    assert_eq!(until(&heard, |_| true), [UpdateStatus::Idle]);
    let seen = until(&heard, |status| *status == UpdateStatus::Idle);
    assert!(seen.contains(&UpdateStatus::Downloading {
        version: RELEASED.to_owned(),
        percent: 0
    }));
    assert_eq!(rig.status(), UpdateStatus::Idle);
    assert_eq!(rig.pending_files(), Vec::<String>::new());
    assert_eq!(rig.timers.live(), Vec::<Duration>::new());
    assert!(
        rig.log_text()
            .contains("update download cancelled: automatic updates turned off")
    );

    rig.set_auto(true);
    assert_eq!(rig.timers.live(), [FIRST_CHECK]);
    rig.backend.shut_down();
}

#[test]
fn a_timer_disarmed_before_it_fired_changes_nothing_when_it_fires_late() {
    let rig = Rig::new("api-updates-late-timer", story(UpdateStory::UpToDate)).handed_in();
    assert_eq!(rig.timers.live(), [FIRST_CHECK]);
    rig.set_auto(false);
    rig.set_auto(true);
    assert_eq!(rig.timers.count(), 2);
    assert_eq!(rig.timers.live(), [FIRST_CHECK]);
    rig.timers.fire(0);
    assert_eq!(rig.status(), UpdateStatus::Idle);
    assert!(rig.http.asked().is_empty());
    assert_eq!(rig.timers.live(), [FIRST_CHECK]);

    let heard = rig.backend.update_changes();
    rig.timers.fire(1);
    let seen = until(&heard, is_over);
    assert!(matches!(seen.last(), Some(UpdateStatus::UpToDate { .. })));
    rig.backend.shut_down();
}

#[test]
fn an_install_is_refused_before_anything_is_verified() {
    let rig = Rig::new("api-updates-not-ready", story(UpdateStory::Ready)).handed_in();
    assert_eq!(refusal(block_on(rig.backend.install_update())), not_ready());
    let log = rig.log_text();
    assert!(
        log.contains(
            "[main:ipc] ipc updates:install refused {\"code\":\"NOT_READY\",\"message\":\"No verified update has been downloaded yet\"}"
        ),
        "{log}"
    );
    rig.backend.shut_down();
    rig.backend.install_at_quit(0);
    assert!(rig.starts().is_empty());
}

struct Switch {
    started: Mutex<Option<Sender<()>>>,
    release: Mutex<Receiver<()>>,
}

#[test]
fn an_install_waits_for_the_switch_that_runs() {
    let (started, running) = channel();
    let (release, released) = channel();
    let (finished_tx, finished) = channel::<LaunchJob>();
    let held = Arc::new(Switch {
        started: Mutex::new(Some(started)),
        release: Mutex::new(released),
    });
    let account_login = Arc::new(Mutex::new(None));
    let login = Arc::clone(&account_login);
    let rig = Rig::starting(
        "api-updates-busy",
        story(UpdateStory::Ready),
        move |data_dir, logger| {
            let launchers_dir = data_dir.parent().unwrap().join("launchers");
            ol_sim::lay_down(
                &oracle("fixtures/dev-only/fake-launchers/all-four"),
                &oracle("fixtures/launchers"),
                &launchers_dir,
            )
            .unwrap();
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
            let seeded = fake
                .seed
                .accounts
                .iter()
                .find(|account| account.launcher == LauncherId::Steam)
                .unwrap()
                .clone();
            *lock(&login) = Some(seeded);
            let finished_tx = Mutex::new(finished_tx);
            let observer = Arc::clone(&held);
            Backend::logging_to(logger.log()).start_with(
                data_dir,
                VERSION,
                launchers,
                Some(Arc::new(move |job: &LaunchJob| {
                    if thread::current().name() == Some("switch") {
                        let first = lock(&observer.started).take();
                        if let Some(first) = first {
                            first.send(()).unwrap();
                            lock(&observer.release).recv().unwrap();
                        }
                    }
                    if job.state != JobState::Running {
                        let _ = lock(&finished_tx).send(job.clone());
                    }
                })),
            )
        },
    )
    .handed_in()
    .ready();
    let seeded = lock(&account_login).take().unwrap();
    let account = block_on(rig.backend.create_account(ol_api::accounts::NewAccount {
        launcher: seeded.launcher,
        display_name: seeded.display_name.clone(),
        login: seeded.login.clone(),
        launcher_uid: seeded.launcher_uid.clone(),
        password: None,
        tags: Vec::new(),
        notes: String::new(),
    }))
    .unwrap();
    block_on(rig.backend.start_launch(StartLaunch {
        account_id: account.id,
        game_id: None,
        confirm_close: true,
    }))
    .unwrap();
    running.recv_timeout(WAIT).unwrap();

    assert_eq!(
        refusal(block_on(rig.backend.install_update())),
        (
            "BUSY",
            "Another switch or launch is still running".to_owned()
        )
    );
    assert!(
        rig.log_text()
            .contains("ipc updates:install refused {\"code\":\"BUSY\"")
    );
    release.send(()).unwrap();
    let job = finished.recv_timeout(WAIT).unwrap();
    assert_ne!(job.state, JobState::Running);

    block_on(rig.backend.install_update()).unwrap();
    rig.backend.shut_down();
    rig.backend.install_at_quit(0);
    assert_eq!(
        rig.starts(),
        installed_quietly(&rig, &["--updated", "/S", "--force-run"])
    );
}

#[test]
fn a_quit_starts_the_waiting_installer_once_and_quietly() {
    let rig = Rig::new("api-updates-quit", story(UpdateStory::Ready))
        .handed_in()
        .ready();
    rig.backend.shut_down();
    rig.backend.install_at_quit(0);
    assert_eq!(rig.starts(), installed_quietly(&rig, &["--updated", "/S"]));
    rig.backend.install_at_quit(0);
    assert_eq!(rig.starts().len(), 1);
    let log = rig.log_text();
    assert!(log.contains("[main:updates] update install {"), "{log}");
}

#[test]
fn restart_to_update_starts_the_installer_to_run_the_app_again() {
    let rig = Rig::new("api-updates-restart", story(UpdateStory::Ready))
        .handed_in()
        .ready();
    block_on(rig.backend.install_update()).unwrap();
    rig.backend.shut_down();
    rig.backend.install_at_quit(0);
    assert_eq!(
        rig.starts(),
        installed_quietly(&rig, &["--updated", "/S", "--force-run"])
    );
}

#[test]
fn a_quit_before_the_shut_down_starts_nothing_and_leaves_the_one_after_it() {
    let rig = Rig::new("api-updates-early-quit", story(UpdateStory::Ready))
        .handed_in()
        .ready();
    rig.backend.install_at_quit(0);
    assert!(rig.starts().is_empty());
    rig.backend.shut_down();
    rig.backend.install_at_quit(0);
    assert_eq!(rig.starts().len(), 1);
}

#[test]
fn a_failed_exit_a_crash_or_the_end_of_the_session_starts_nothing() {
    let rig = Rig::new("api-updates-exit-1", story(UpdateStory::Ready))
        .handed_in()
        .ready();
    rig.backend.shut_down();
    rig.backend.install_at_quit(1);
    rig.backend.install_at_quit(0);
    assert!(rig.starts().is_empty());

    let rig = Rig::new("api-updates-crash", story(UpdateStory::Ready))
        .handed_in()
        .ready();
    rig.backend.update_crashed();
    rig.backend.shut_down();
    rig.backend.install_at_quit(0);
    assert!(rig.starts().is_empty());
    assert!(
        rig.log_text()
            .contains("an unexpected error happened in this run")
    );

    let rig = Rig::new("api-updates-session", story(UpdateStory::Ready))
        .handed_in()
        .ready();
    rig.backend.update_session_ending();
    rig.backend.shut_down();
    rig.backend.install_at_quit(0);
    assert!(rig.starts().is_empty());
    assert!(
        rig.log_text()
            .contains("Windows is shutting down or signing out")
    );
}

#[test]
fn with_automatic_updates_off_an_update_waits_for_restart_to_update() {
    let rig = Rig::new("api-updates-auto-off-quit", story(UpdateStory::Ready));
    rig.set_auto(false);
    let rig = rig.handed_in();
    assert_eq!(rig.timers.count(), 0);
    let rig = rig.ready();
    rig.backend.shut_down();
    rig.backend.install_at_quit(0);
    assert!(rig.starts().is_empty());
    assert!(
        rig.log_text()
            .contains("update left for a restart: automatic updates are off")
    );
}

#[test]
fn a_shut_down_during_a_download_cancels_it_and_leaves_no_file() {
    let (feed, held) = held_installer();
    let rig = Rig::new("api-updates-quit-downloading", feed).handed_in();
    assert_eq!(rig.check(), UpdateStatus::Checking);
    held.paused.recv_timeout(WAIT).unwrap();
    assert_eq!(rig.pending_files(), [format!("temp-{INSTALLER_NAME}")]);
    rig.backend.shut_down();
    held.release.send(()).unwrap();
    wait_for("the download ends", || {
        rig.pending_files()
            .iter()
            .all(|name| !name.starts_with("temp-"))
    });
    assert_eq!(rig.pending_files(), Vec::<String>::new());
    assert_eq!(rig.timers.live(), Vec::<Duration>::new());
    rig.backend.install_at_quit(0);
    assert!(rig.starts().is_empty());
}

fn panicking(on: UpdateAsk) -> impl FnOnce(Arc<dyn Clock>) -> Feed {
    move |clock| {
        Box::new(move |request| {
            assert!(request.ask() != on, "the feed broke");
            respond(UpdateStory::Ready, VERSION, &clock, request)
        })
    }
}

#[test]
fn a_check_that_panics_fails_with_a_fixed_sentence_and_the_backend_goes_on() {
    let rig = Rig::new(
        "api-updates-check-panics",
        panicking(UpdateAsk::LatestRelease),
    )
    .handed_in();
    let heard = rig.backend.update_changes();
    assert_eq!(rig.check(), UpdateStatus::Checking);
    let seen = until(&heard, is_over);
    let failure = failed(UpdateStep::Check, "the update check stopped unexpectedly");
    assert_eq!(seen.last(), Some(&failure));
    assert_eq!(rig.status(), failure);
    block_on(rig.backend.settings()).unwrap();
    assert_eq!(rig.check(), UpdateStatus::Checking);
    assert_eq!(until(&heard, is_over).last(), Some(&failure));
    let log = rig.log_text();
    assert!(
        log.contains("ERROR [main:updates] update check crashed {\"error\":\"the update check stopped unexpectedly\"}"),
        "{log}"
    );
    rig.backend.shut_down();
}

#[test]
fn a_download_that_panics_fails_with_a_fixed_sentence() {
    let rig = Rig::new(
        "api-updates-download-panics",
        panicking(UpdateAsk::Installer),
    )
    .handed_in();
    let heard = rig.backend.update_changes();
    rig.check();
    let seen = until(&heard, is_over);
    assert_eq!(
        seen.last(),
        Some(&failed(
            UpdateStep::Download,
            "the update download stopped unexpectedly"
        ))
    );
    block_on(rig.backend.settings()).unwrap();
    assert!(rig.log_text().contains("update download crashed"));
    rig.backend.shut_down();
}
