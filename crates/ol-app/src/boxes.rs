use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::ThreadId;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ol_api::Backend;
use ol_api::log::Logger;
use ol_platform_win::boxes::MessageBox;
use ol_platform_win::instance::QuitAsker;

use crate::args::{Args, RELAUNCHED};

const CRASH_BOX_EVERY: Duration = Duration::from_secs(60);
const QUIT_WAIT: Duration = Duration::from_secs(15);
const SHUTDOWN_WAIT: Duration = Duration::from_secs(5);
const LOG_CLOSE: Duration = Duration::from_secs(2);
const LOOK_EVERY: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boxes {
    Windows,
    Recording,
}

impl Boxes {
    pub fn for_run(args: &Args) -> Self {
        match (&args.check, &args.doctor) {
            (None, None) => Self::Windows,
            _ => Self::Recording,
        }
    }

    // The one place a box reaches Windows.
    fn show(self, message: &MessageBox<'_>) -> Option<usize> {
        match self {
            Self::Windows => ol_platform_win::boxes::show(message),
            Self::Recording => {
                println!(
                    "OneLaunch: box recorded and not shown: {} / {}",
                    message.title, message.instruction
                );
                message
                    .buttons
                    .map(|buttons| buttons.len().saturating_sub(1))
            }
        }
    }
}

fn log_file(data_dir: &Path) -> PathBuf {
    data_dir
        .join("logs")
        .join(format!("{}.log", ol_api::log::LOG_NAME))
}

pub fn startup_text(reason: &str, data_dir: &Path) -> String {
    format!(
        "{reason}\n\nDetails are in the log:\n{}\n\nData folder:\n{}",
        log_file(data_dir).display(),
        data_dir.display()
    )
}

pub fn could_not_start(boxes: Boxes, reason: &str, data_dir: &Path) {
    let content = startup_text(reason, data_dir);
    boxes.show(&MessageBox {
        title: "Error",
        instruction: "One Launch could not start",
        content: &content,
        buttons: None,
    });
}

pub const NOT_ANSWERING: &str = "Another copy of One Launch runs on this data folder and did not answer. Look for it in the notification area, or end it in Task Manager, then start One Launch again.";

pub const NOT_RELAUNCHED: &str = "One Launch closed to start again, and the new copy could not be started. Start One Launch again.";

pub const STARTING_FAILED: &str = "One Launch hit an error inside it while starting, and stopped.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Crashed {
    Window,
    Elsewhere,
}

pub struct CrashBox {
    pub content: String,
    pub buttons: &'static [&'static str],
}

pub fn crash_box(crashed: Crashed, data_dir: &Path) -> CrashBox {
    let log = log_file(data_dir);
    match crashed {
        Crashed::Window => CrashBox {
            content: format!(
                "One Launch has to close.\n\nThe details are in {}. Relaunching starts it again.",
                log.display()
            ),
            buttons: &["Relaunch", "Close"],
        },
        Crashed::Elsewhere => CrashBox {
            content: format!(
                "One Launch stopped what it was doing.\n\nThe details are in {}. Relaunching is the safe choice; keeping it running may leave things half-done.",
                log.display()
            ),
            buttons: &["Relaunch", "Keep running"],
        },
    }
}

struct Watch {
    boxes: Boxes,
    data_dir: PathBuf,
    window_thread: ThreadId,
    relaunch_args: Vec<String>,
}

struct Ending {
    backend: Backend,
    logger: Arc<Logger>,
}

static WATCH: OnceLock<Watch> = OnceLock::new();
static ENDING: OnceLock<Ending> = OnceLock::new();
static QUIT: OnceLock<QuitAsker> = OnceLock::new();
static QUIT_HEARD: AtomicBool = AtomicBool::new(false);
static WINDOW_UP: AtomicBool = AtomicBool::new(false);
static LAST_BOX: AtomicU64 = AtomicU64::new(0);
static RELAUNCH: AtomicBool = AtomicBool::new(false);

thread_local! {
    static WINDOW_END: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
}

// The new copy waits for this one's lock, which goes only when this process ends.
pub fn relaunch_arguments(arguments: impl Iterator<Item = String>) -> Vec<String> {
    arguments
        .filter(|argument| {
            argument != ol_api::settings::START_HIDDEN_FLAG && argument != RELAUNCHED
        })
        .chain([RELAUNCHED.to_owned()])
        .collect()
}

pub fn watch(boxes: Boxes, data_dir: &Path) {
    let _ = WATCH.set(Watch {
        boxes,
        data_dir: data_dir.to_path_buf(),
        window_thread: std::thread::current().id(),
        relaunch_args: relaunch_arguments(std::env::args().skip(1)),
    });
    let logged = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        logged(info);
        after_panic();
    }));
}

pub fn attach(backend: Backend, logger: Arc<Logger>) {
    let _ = ENDING.set(Ending { backend, logger });
}

pub fn window_up(up: bool) {
    WINDOW_UP.store(up, Ordering::SeqCst);
}

pub fn on_window_thread_end(end: impl FnOnce() + 'static) {
    WINDOW_END.with(|slot| {
        if let Ok(mut slot) = slot.try_borrow_mut() {
            *slot = Some(Box::new(end));
        }
    });
}

// Inside the process only: nothing outside it can ask the app to quit.
pub fn quit_through(asker: QuitAsker) {
    let _ = QUIT.set(asker);
}

pub fn quit_heard() {
    QUIT_HEARD.store(true, Ordering::SeqCst);
}

pub fn relaunch_asked() -> bool {
    RELAUNCH.load(Ordering::SeqCst)
}

pub fn relaunch() {
    if let Some(watch) = WATCH.get() {
        relaunch_with(
            &watch.relaunch_args,
            ol_platform_win::own_process::start_again,
            || {
                let (boxes, data_dir) = (watch.boxes, watch.data_dir.clone());
                // Off the window's thread, as the crash box: a crash ends the app from inside a GPUI update.
                let _ = std::thread::spawn(move || {
                    could_not_start(boxes, NOT_RELAUNCHED, &data_dir);
                })
                .join();
            },
        );
    }
}

fn relaunch_with(
    arguments: &[String],
    start: impl FnOnce(&[String]) -> bool,
    not_started: impl FnOnce(),
) {
    if !start(arguments) {
        not_started();
    }
}

fn admitted(last_box: &AtomicU64, now: u64) -> bool {
    let mut last = last_box.load(Ordering::SeqCst);
    loop {
        if last != 0 && now.saturating_sub(last) < CRASH_BOX_EVERY.as_secs() {
            return false;
        }
        match last_box.compare_exchange(last, now.max(1), Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return true,
            Err(seen) => last = seen,
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

fn switch_runs() -> bool {
    ENDING.get().is_some_and(|ending| ending.backend.is_busy())
}

fn after_panic() {
    // A run that goes on after a crash installs nothing at its quit.
    if let Some(ending) = ENDING.get() {
        ending.backend.update_crashed();
    }
    let Some(watch) = WATCH.get() else {
        return;
    };
    let crashed = if std::thread::current().id() == watch.window_thread {
        Crashed::Window
    } else {
        Crashed::Elsewhere
    };
    let up = WINDOW_UP.load(Ordering::SeqCst);
    if !up {
        if crashed == Crashed::Window {
            could_not_start(watch.boxes, STARTING_FAILED, &watch.data_dir);
            end_here(false);
        }
        return;
    }
    let relaunch_now = if admitted(&LAST_BOX, now_secs()) {
        let text = crash_box(crashed, &watch.data_dir);
        let boxes = watch.boxes;
        // A box on the window's thread mid-update would hand GPUI messages it cannot take.
        let answer = std::thread::spawn(move || {
            boxes.show(&MessageBox {
                title: "One Launch hit an unexpected error",
                instruction: "Something went wrong inside One Launch.",
                content: &text.content,
                buttons: Some(text.buttons),
            })
        })
        .join()
        .ok()
        .flatten();
        answer == Some(0)
    } else {
        false
    };
    match crashed {
        Crashed::Window => end_here(relaunch_now),
        Crashed::Elsewhere if relaunch_now => {
            RELAUNCH.store(true, Ordering::SeqCst);
            let asked = QUIT.get().is_some_and(|quit| quit.ask());
            wait_for_the_quit(asked);
        }
        Crashed::Elsewhere => {}
    }
}

// Only a window thread that does not answer while no switch runs is overtaken.
fn wait_for_the_quit(asked: bool) {
    let started = Instant::now();
    loop {
        if asked && QUIT_HEARD.load(Ordering::SeqCst) {
            return;
        }
        if started.elapsed() >= QUIT_WAIT && !switch_runs() {
            end_here(true);
        }
        std::thread::sleep(LOOK_EVERY);
    }
}

// A switch being stopped or put back is waited for, however long its own timeouts run.
fn end_here(relaunch_after: bool) -> ! {
    WINDOW_END.with(|slot| {
        if let Some(end) = slot.try_borrow_mut().ok().and_then(|mut slot| slot.take()) {
            end();
        }
    });
    if let Some(ending) = ENDING.get() {
        let backend = ending.backend.clone();
        let done = Arc::new(AtomicBool::new(false));
        let finished = Arc::clone(&done);
        let _ = std::thread::Builder::new()
            .name("shutdown".to_owned())
            .spawn(move || {
                backend.shut_down();
                finished.store(true, Ordering::SeqCst);
            });
        let started = Instant::now();
        while !done.load(Ordering::SeqCst)
            && (ending.backend.is_busy() || started.elapsed() < SHUTDOWN_WAIT)
        {
            std::thread::sleep(LOOK_EVERY);
        }
        ending.logger.close(LOG_CLOSE);
    }
    if relaunch_after {
        relaunch();
    }
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(arguments: &[&str]) -> Args {
        let arguments = [r"--data-dir=D:\scratch\data"]
            .iter()
            .chain(arguments)
            .map(|argument| (*argument).to_owned());
        Args::parse(arguments).unwrap()
    }

    // `Boxes::Windows` is the only way to `ol_platform_win::boxes::show`; the crash hook and every start's box go through `Boxes::show`.
    #[test]
    fn a_capture_or_a_probe_never_shows_a_native_box() {
        let checks: [&[&str]; 7] = [
            &[r"--capture=D:\scratch\shot.png"],
            &[r"--capture=D:\scratch\shot.png", "--capture-keys=enter"],
            &[r"--capture=D:\scratch\shot.png", "--capture-size=960x600"],
            &[r"--capture=D:\scratch\shot.png", "--capture-focus"],
            &[r"--capture=D:\scratch\shot.png", "--capture-type=abc"],
            &[r"--capture=D:\scratch\shot.png", "--start-hidden"],
            &["--probe"],
        ];
        for check in checks {
            assert_eq!(
                Boxes::for_run(&parsed(check)),
                Boxes::Recording,
                "{check:?}"
            );
        }
        assert_eq!(Boxes::for_run(&parsed(&["--doctor"])), Boxes::Recording);
        assert_eq!(Boxes::for_run(&parsed(&[])), Boxes::Windows);
        assert_eq!(
            Boxes::for_run(&parsed(&["--start-hidden", RELAUNCHED])),
            Boxes::Windows
        );
        let recorded = Boxes::Recording.show(&MessageBox {
            title: "t",
            instruction: "i",
            content: "c",
            buttons: Some(&["Relaunch", "Keep running"]),
        });
        assert_eq!(recorded, Some(1));
    }

    #[test]
    fn the_crash_box_offers_relaunch_and_what_the_thread_that_stopped_allows() {
        let data_dir = Path::new(r"D:\scratch\data");
        for crashed in [Crashed::Window, Crashed::Elsewhere] {
            let text = crash_box(crashed, data_dir);
            assert!(
                text.content.contains(r"D:\scratch\data\logs\onelaunch.log"),
                "{}",
                text.content
            );
            assert_eq!(text.buttons[0], "Relaunch");
        }
        assert_eq!(crash_box(Crashed::Window, data_dir).buttons[1], "Close");
        assert_eq!(
            crash_box(Crashed::Elsewhere, data_dir).buttons[1],
            "Keep running"
        );
    }

    #[test]
    fn a_relaunch_starts_the_app_as_it_was_started_but_shown_and_says_it_is_one() {
        let arguments =
            [r"--data-dir=D:\scratch\data", "--start-hidden", RELAUNCHED].map(str::to_owned);
        assert_eq!(
            relaunch_arguments(arguments.into_iter()),
            [r"--data-dir=D:\scratch\data", RELAUNCHED]
        );
    }

    #[test]
    fn a_relaunch_that_starts_no_copy_says_so() {
        let said = std::cell::Cell::new(0);
        let arguments = [RELAUNCHED.to_owned()];
        relaunch_with(
            &arguments,
            |given| {
                assert_eq!(given, [RELAUNCHED]);
                false
            },
            || said.set(said.get() + 1),
        );
        assert_eq!(said.get(), 1);
        relaunch_with(&arguments, |_| true, || said.set(said.get() + 1));
        assert_eq!(said.get(), 1);
    }

    #[test]
    fn of_panics_together_one_box_is_shown_and_the_next_after_a_minute() {
        let every = CRASH_BOX_EVERY.as_secs();
        for _ in 0..200 {
            let last = AtomicU64::new(0);
            let together = std::sync::Barrier::new(16);
            let shown = std::thread::scope(|scope| {
                let panics: Vec<_> = (0..16)
                    .map(|_| {
                        scope.spawn(|| {
                            together.wait();
                            admitted(&last, 1000)
                        })
                    })
                    .collect();
                panics
                    .into_iter()
                    .filter_map(|panic| panic.join().ok())
                    .filter(|shown| *shown)
                    .count()
            });
            assert_eq!(shown, 1);
        }
        let last = AtomicU64::new(0);
        assert!(admitted(&last, 0));
        assert!(!admitted(&last, every - 1));
        assert!(admitted(&last, every + 1));
        assert!(!admitted(&last, every + 2));
        assert!(admitted(&last, 2 * every + 1));
    }

    #[test]
    fn the_start_box_names_the_reason_the_log_and_the_data_folder() {
        assert_eq!(
            startup_text("The database is damaged.", Path::new(r"D:\scratch\data")),
            "The database is damaged.\n\nDetails are in the log:\nD:\\scratch\\data\\logs\\onelaunch.log\n\nData folder:\nD:\\scratch\\data"
        );
    }
}
