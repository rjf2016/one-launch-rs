// The boxes the app shows before and beside its window, as the Electron app's: that it could not start, and that it hit an error inside it. A capture or a probe is run by agents, so it shows none on the owner's desktop: it says which box would have been shown, and answers as the box's last button does.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::ThreadId;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ol_platform_win::boxes::MessageBox;
use ol_platform_win::instance;

use crate::args::Args;

const CRASH_BOX_EVERY: Duration = Duration::from_secs(60);
// As the Electron app's Relaunch: the quit gets this long before the process is ended without it.
const QUIT_WAIT: Duration = Duration::from_secs(15);

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

    // The one place a box reaches Windows. Answers the index of the button pressed; a recorded box answers its last.
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

// Electron's error box: "Error" in the title bar, its sentence over the reason, the log and the data folder, and OK.
pub fn could_not_start(boxes: Boxes, reason: &str, data_dir: &Path) {
    let content = startup_text(reason, data_dir);
    boxes.show(&MessageBox {
        title: "Error",
        instruction: "One Launch could not start",
        content: &content,
        buttons: None,
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Crashed {
    // On the window's thread: nothing can run on after it.
    Window,
    Elsewhere,
}

pub struct CrashBox {
    pub content: String,
    pub buttons: &'static [&'static str],
}

// What the box says. Never the panic's message, which can quote the text being handled, a typed password among it: that the app stopped, and where the log is.
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

static WATCH: OnceLock<Watch> = OnceLock::new();
static WINDOW_UP: AtomicBool = AtomicBool::new(false);
static LAST_BOX: AtomicU64 = AtomicU64::new(0);
static RELAUNCH: AtomicBool = AtomicBool::new(false);

// From the window's first frame on, a panic shows the crash box. Called on the window's thread, after the log's own hook, which this one runs first.
pub fn watch(boxes: Boxes, data_dir: &Path) {
    let relaunch_args = std::env::args()
        .skip(1)
        .filter(|arg| arg != ol_api::settings::START_HIDDEN_FLAG)
        .collect();
    let _ = WATCH.set(Watch {
        boxes,
        data_dir: data_dir.to_path_buf(),
        window_thread: std::thread::current().id(),
        relaunch_args,
    });
    let logged = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        logged(info);
        after_panic();
    }));
}

pub fn window_up(up: bool) {
    WINDOW_UP.store(up, Ordering::SeqCst);
}

// Whether a Relaunch asked the app to quit: then the copy to start comes once the lock is let go.
pub fn relaunch_asked() -> bool {
    RELAUNCH.load(Ordering::SeqCst)
}

pub fn relaunch() {
    let Some(watch) = WATCH.get() else {
        return;
    };
    instance::release();
    ol_platform_win::own_process::start_again(&watch.relaunch_args);
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

fn after_panic() {
    let Some(watch) = WATCH.get() else {
        return;
    };
    let crashed = if std::thread::current().id() == watch.window_thread {
        Crashed::Window
    } else {
        Crashed::Elsewhere
    };
    let now = now_secs();
    let last = LAST_BOX.load(Ordering::SeqCst);
    let shows = WINDOW_UP.load(Ordering::SeqCst)
        && (last == 0 || now.saturating_sub(last) >= CRASH_BOX_EVERY.as_secs());
    let relaunch_now = if shows {
        LAST_BOX.store(now.max(1), Ordering::SeqCst);
        let text = crash_box(crashed, &watch.data_dir);
        let boxes = watch.boxes;
        // On a thread of its own and with no owner: the window's thread may be in the middle of an update, and a box there would hand it messages it cannot take.
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
        // Unwinding out of the window's thread would end the process with nothing said; it is ended here instead, the lock let go with it.
        Crashed::Window => {
            if relaunch_now {
                relaunch();
            }
            std::process::exit(1);
        }
        Crashed::Elsewhere if relaunch_now => {
            RELAUNCH.store(true, Ordering::SeqCst);
            if instance::ask_to_quit() {
                std::thread::sleep(QUIT_WAIT);
            }
            relaunch();
            std::process::exit(1);
        }
        Crashed::Elsewhere => {}
    }
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

    // `Boxes::Windows` is the only way to `ol_platform_win::boxes::show`; the crash hook and the start's box both go through `Boxes::show`.
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
        assert_eq!(Boxes::for_run(&parsed(&["--start-hidden"])), Boxes::Windows);
        let recorded = Boxes::Recording.show(&MessageBox {
            title: "t",
            instruction: "i",
            content: "c",
            buttons: Some(&["Relaunch", "Keep running"]),
        });
        assert_eq!(recorded, Some(1));
    }

    #[test]
    fn the_crash_box_never_carries_the_panics_message() {
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
    fn the_start_box_names_the_reason_the_log_and_the_data_folder() {
        assert_eq!(
            startup_text("The database is damaged.", Path::new(r"D:\scratch\data")),
            "The database is damaged.\n\nDetails are in the log:\nD:\\scratch\\data\\logs\\onelaunch.log\n\nData folder:\nD:\\scratch\\data"
        );
    }
}
