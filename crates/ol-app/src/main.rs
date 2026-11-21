#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod args;
mod capture;
#[cfg(feature = "fake")]
mod fake;
mod probe;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{App, AsyncApp, Window, WindowHandle};
use ol_api::Backend;
use ol_api::library::HourClock;
use ol_api::settings::{
    AfterPlay, CloseAction, Folder, close_request_action, hide_after_instant_play,
};
use ol_model::SettingKey;
use ol_platform_api::{Shell as FolderShell, ShellError};
use ol_platform_win::data_folder::refuse_real_data_folder;
use ol_platform_win::dpapi::Dpapi;
use ol_platform_win::own_process;
use ol_platform_win::shell::WindowsShell;
use ol_platform_win::tray::{MenuEntry, Tray, TrayEvent};
use ol_platform_win::window::AppWindow;
use ol_ui::{FilePicker, LibraryLoaded, Played, Presence, QuitAnyway, Shell};

use crate::args::{Args, Check, DoctorVault};

const TITLE: &str = "One Launch";
// As in the Electron app: no picture of a fake run can pass for a real one.
#[cfg(feature = "fake")]
const FAKE_TITLE: &str = "One Launch (fake launchers)";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const COPY_DELAY: Duration = Duration::from_millis(250);

type TraySlot = Rc<RefCell<Option<Tray>>>;

// The clipboard a run copies to. A capture or a probe is run by agents, so it never reaches the real one, neither through the backend nor through a text field's keys.
#[derive(Debug, PartialEq, Eq)]
enum Board {
    Windows,
    #[cfg(feature = "fake")]
    Recording,
    None,
}

impl Board {
    fn for_run(args: &Args) -> Self {
        match (&args.check, &args.doctor) {
            (None, None) => Self::Windows,
            #[cfg(feature = "fake")]
            (Some(_), _) => Self::Recording,
            _ => Self::None,
        }
    }

    fn keys_reach_it(&self) -> bool {
        matches!(self, Self::Windows)
    }

    fn give(&self) -> Given {
        match self {
            Self::Windows => Given::Windows(ol_platform_win::clipboard::WindowsClipboard),
            #[cfg(feature = "fake")]
            Self::Recording => {
                let log = ol_platform_fake::OpLog::new();
                Given::Recording(
                    Arc::new(ol_platform_fake::RecordingClipboard::new(log.clone())),
                    log,
                )
            }
            Self::None => Given::None,
        }
    }
}

enum Given {
    Windows(ol_platform_win::clipboard::WindowsClipboard),
    #[cfg(feature = "fake")]
    Recording(
        Arc<ol_platform_fake::RecordingClipboard>,
        ol_platform_fake::OpLog,
    ),
    None,
}

fn use_clipboard(given: Given, backend: &Backend) -> Option<Box<dyn FnOnce()>> {
    let timer = || {
        Arc::new(ol_api::clipboard::SleepingTimer::new(Arc::new(
            ol_platform_win::clock::SystemClock,
        )))
    };
    match given {
        Given::Windows(clipboard) => {
            backend.use_clipboard(Arc::new(clipboard), timer());
            None
        }
        #[cfg(feature = "fake")]
        Given::Recording(clipboard, log) => {
            backend.use_clipboard(clipboard, timer());
            println!("OneLaunch: clipboard: a recording one, in this process only");
            Some(Box::new(move || {
                for event in log.snapshot() {
                    println!("OneLaunch: clipboard {event}");
                }
            }))
        }
        Given::None => None,
    }
}

// What shows a folder in Explorer. A capture or a probe is run by agents, so it records the folder and opens nothing on the owner's desktop.
#[derive(Debug, PartialEq, Eq)]
enum Opener {
    Windows,
    Recording,
}

impl Opener {
    fn for_run(args: &Args) -> Self {
        match (&args.check, &args.doctor) {
            (None, None) => Self::Windows,
            _ => Self::Recording,
        }
    }
}

// The one place a run's backend is handed its shell. Answers the recording shell when that is the one given.
fn hand_shell(args: &Args, backend: &Backend) -> Option<Arc<RecordingShell>> {
    match Opener::for_run(args) {
        Opener::Windows => {
            backend.use_shell(Arc::new(WindowsShell));
            None
        }
        Opener::Recording => {
            let shell = Arc::new(RecordingShell {
                folders: [
                    (Folder::Data, "the data folder"),
                    (Folder::Logs, "logs"),
                    (Folder::Backups, "backups"),
                ]
                .map(|(folder, name)| (backend.folder(folder), name))
                .to_vec(),
                asked: std::sync::Mutex::default(),
            });
            backend.use_shell(Arc::clone(&shell) as Arc<dyn FolderShell>);
            Some(shell)
        }
    }
}

// Opens nothing: it says which of the data folder's folders it was asked for, by name and not by path, which can run through the user's profile.
struct RecordingShell {
    folders: Vec<(PathBuf, &'static str)>,
    asked: std::sync::Mutex<Vec<&'static str>>,
}

impl FolderShell for RecordingShell {
    fn open_url(&self, _: &str) -> Result<(), ShellError> {
        Err(ShellError("This run opens no links.".to_owned()))
    }

    fn open_folder(&self, folder: &str) -> Result<(), ShellError> {
        let name = self
            .folders
            .iter()
            .find(|(path, _)| path.as_os_str() == folder)
            .map_or("another folder", |(_, name)| name);
        println!("OneLaunch: folder recorded and not opened: {name}");
        if let Ok(mut asked) = self.asked.lock() {
            asked.push(name);
        }
        Ok(())
    }
}

// What Locate asks for a launcher's program with. A capture, a probe or the doctor is run by agents, so it is answered from the run's steps and never opens a dialog on the owner's desktop.
fn file_picker(args: &Args) -> FilePicker {
    match Opener::for_run(args) {
        Opener::Windows => FilePicker::Windows,
        Opener::Recording => FilePicker::Scripted(Rc::default()),
    }
}

fn report_picks(picker: &FilePicker) {
    if let FilePicker::Scripted(script) = picker {
        let asked = script.borrow().asked();
        if asked > 0 {
            println!(
                "OneLaunch: the file dialog was asked for {asked} time(s), answered from the run's steps and not opened"
            );
        }
    }
}

// What the window's close, the tray and the end of a Play act on: the settings that decide them, kept as the backend reports each change.
struct Life {
    backend: Backend,
    window: WindowHandle<Shell>,
    tray: TraySlot,
    title: &'static str,
    close_to_tray: Cell<bool>,
    hide_to_tray: Cell<bool>,
    quitting: Cell<bool>,
    // "Quit anyway" was answered: the switch is being stopped, and the app quits when it has.
    stopping: Cell<bool>,
}

impl Life {
    fn heard(&self, key: SettingKey, value: Option<&str>) {
        let on = value.is_none_or(|value| value == "true" || value == "1");
        match key {
            SettingKey::WindowCloseToTray => self.close_to_tray.set(on),
            SettingKey::PlayHideToTray => self.hide_to_tray.set(on),
            _ => {}
        }
    }
}

// What the launchers run on: nothing yet in the app itself, a scenario folder in a build with the fake launchers.
enum Machine {
    None,
    #[cfg(feature = "fake")]
    Fake(ol_sim::Seed),
}

fn main() -> ExitCode {
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(problem) => {
            eprintln!("OneLaunch: {problem}");
            return ExitCode::from(2);
        }
    };
    // Before anything is opened, made or read: the installed app's own folder is not for this build, and a capture is the other path it writes to.
    let capture = match &args.check {
        Some(Check::Capture(plan)) => Some(("--capture", plan.file.as_path())),
        _ => None,
    };
    let written = [
        Some(("--data-dir", args.data_dir.as_path())),
        capture,
        fake_folder(&args),
    ];
    for (argument, path) in written.into_iter().flatten() {
        if let Err(refusal) = refuse_real_data_folder(argument, path) {
            eprintln!("OneLaunch: {refusal}");
            return ExitCode::from(2);
        }
    }
    if let Some(vault) = args.doctor {
        return doctor(&args.data_dir, vault);
    }
    // Started first: the database opens on its own thread while the window is being made.
    let (backend, machine) = match start_backend(&args) {
        Ok(started) => started,
        Err(problem) => {
            eprintln!("OneLaunch: {problem:#}");
            return ExitCode::from(2);
        }
    };
    let heard = use_clipboard(Board::for_run(&args).give(), &backend);
    let _recorded = hand_shell(&args, &backend);
    let picker = file_picker(&args);
    let failed = Rc::new(Cell::new(false));
    let tray = TraySlot::default();
    gpui_platform::application()
        .with_assets(ol_ui::Assets)
        .run({
            let (backend, failed, tray) = (backend.clone(), failed.clone(), tray.clone());
            let picker = picker.clone();
            move |cx| {
                cx.set_global(picker);
                if let Err(error) = start(args, backend, machine, failed.clone(), tray, cx) {
                    eprintln!("OneLaunch: {error:#}");
                    failed.set(true);
                    cx.quit();
                }
            }
        });
    // Here and nowhere else: whichever way the message loop ended, the icon goes with it.
    let icon = tray.borrow_mut().take();
    if icon.is_some() {
        drop(icon);
        println!("OneLaunch: tray icon removed");
    }
    backend.shut_down();
    if let Some(heard) = heard {
        heard();
    }
    report_picks(&picker);
    if failed.get() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

// Counts and ok or failed, never a value: the report can be shown to anyone.
fn doctor(data_dir: &Path, vault: DoctorVault) -> ExitCode {
    let report = match vault {
        DoctorVault::OfThisUser => ol_api::doctor::run(data_dir, &Dpapi),
        #[cfg(feature = "fake")]
        DoctorVault::OfTheFixtures => ol_api::doctor::run_on_fixtures(data_dir),
    };
    print!(
        "OneLaunch doctor
{report}"
    );
    if report.passed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

// The fake machine writes under its folder, so that folder is held to the rule of the data folder.
#[cfg(feature = "fake")]
fn fake_folder(args: &Args) -> Option<(&'static str, &Path)> {
    args.fake
        .as_ref()
        .map(|machine| ("--fake-launchers", machine.folder.as_path()))
}

#[cfg(not(feature = "fake"))]
fn fake_folder(_: &Args) -> Option<(&'static str, &Path)> {
    None
}

#[cfg(not(feature = "fake"))]
fn start_backend(args: &Args) -> anyhow::Result<(Backend, Machine)> {
    if !args.real_launchers {
        return Ok((
            Backend::start_with_vault_of_data_folder(
                args.data_dir.clone(),
                VERSION,
                Box::new(Dpapi),
            ),
            Machine::None,
        ));
    }
    use anyhow::Context as _;

    // A key Windows does not unwrap does not stop the start: the vault comes up with its key lost, for the Encryption panel's reset.
    let backend = Backend::start_on_this_pc(
        args.data_dir.clone(),
        VERSION,
        real_launchers(),
        Box::new(Dpapi),
        None,
    )
    .context("the vault of --data-dir could not be opened")?;
    Ok((backend, Machine::None))
}

// This PC behind the launchers. The Riot Client is given no way to its local server: Play is left to the user, as the hand-over says.
#[cfg(not(feature = "fake"))]
fn real_launchers() -> ol_api::launchers::Launchers {
    use ol_platform_win::machine;

    let mut launchers = ol_api::launchers::on_this_pc(machine::platform());
    let folders = machine::folders();
    if let Some(app_data) = folders.app_data {
        launchers.battlenet.app_data = app_data;
    }
    if let Some(local_app_data) = folders.local_app_data {
        launchers.epic.local_app_data.clone_from(&local_app_data);
        launchers
            .battlenet
            .local_app_data
            .clone_from(&local_app_data);
        launchers.riot.local_app_data = local_app_data;
    }
    if let Some(program_data) = folders.program_data {
        launchers.epic.program_data.clone_from(&program_data);
        launchers.battlenet.program_data.clone_from(&program_data);
        launchers.riot.program_data = program_data;
    }
    launchers
}

#[cfg(feature = "fake")]
fn start_backend(args: &Args) -> anyhow::Result<(Backend, Machine)> {
    let Some(machine) = &args.fake else {
        return Ok((
            Backend::start_with_vault_of_data_folder(
                args.data_dir.clone(),
                VERSION,
                Box::new(Dpapi),
            ),
            Machine::None,
        ));
    };
    let (backend, seed) = fake::start(&args.data_dir, machine, VERSION)?;
    Ok((backend, Machine::Fake(seed)))
}

fn start(
    args: Args,
    backend: Backend,
    machine: Machine,
    failed: Rc<Cell<bool>>,
    tray: TraySlot,
    cx: &mut App,
) -> anyhow::Result<()> {
    let clock = if ol_platform_win::clock::uses_24_hour_clock() {
        HourClock::TwentyFour
    } else {
        HourClock::Twelve
    };
    ol_ui::init(
        cx,
        ol_ui::LocalTime {
            offset: ol_platform_win::clock::local_offset,
            clock,
        },
    );
    if !Board::for_run(&args).keys_reach_it() {
        ol_ui_kit::refuse_clipboard_keys(cx);
    }
    let title = match machine {
        Machine::None => TITLE,
        #[cfg(feature = "fake")]
        Machine::Fake(seed) => {
            let backend = backend.clone();
            cx.spawn(async move |_| {
                // The code and not the sentence, which can name an account.
                if let Err(error) = fake::seed(&backend, seed).await {
                    eprintln!(
                        "OneLaunch: the scenario's accounts could not be made ({})",
                        error.code
                    );
                }
            })
            .detach();
            FAKE_TITLE
        }
    };
    let fake_launchers = title != TITLE;
    let presence = match &args.check {
        Some(Check::Capture(plan)) if !plan.focus => Presence::Background,
        Some(Check::Probe) => Presence::Background,
        _ => Presence::Foreground,
    };
    // Heard from before the window opens, so no change made meanwhile is missed.
    let changes = backend.setting_changes();
    let window = ol_ui::open_main_window(title, presence, fake_launchers, backend.clone(), cx)?;
    let library = window.update(cx, |shell, _, _| shell.library().clone())?;
    cx.subscribe(&library, move |_, _: &LibraryLoaded, cx| {
        let _ = window.update(cx, |_, window, _| {
            after_the_next_frame(window, "library shown")
        });
    })
    .detach();
    let shell = window.update(cx, |_, _, cx| cx.entity())?;
    let life = Rc::new(Life {
        backend: backend.clone(),
        window,
        tray: tray.clone(),
        title,
        close_to_tray: Cell::new(SettingKey::WindowCloseToTray.default_on()),
        hide_to_tray: Cell::new(SettingKey::PlayHideToTray.default_on()),
        quitting: Cell::new(false),
        stopping: Cell::new(false),
    });
    cx.subscribe(&shell, {
        let life = Rc::clone(&life);
        move |_, _: &QuitAnyway, cx| quit_after_the_switch(&life, cx)
    })
    .detach();
    cx.subscribe(&shell, {
        let life = Rc::clone(&life);
        move |_, played: &Played, cx| {
            let after = AfterPlay::of(
                &played.job,
                played.instant,
                life.tray.borrow().is_some(),
                life.close_to_tray.get(),
                life.hide_to_tray.get(),
            );
            if hide_after_instant_play(after) {
                cx.spawn(async |_| hide_window("after an instant launch"))
                    .detach();
            }
        }
    })
    .detach();

    // In a task and not here: adding the icon waits on the shell, Windows may hand the GPUI window a message meanwhile, and GPUI cannot take one while the app is being updated. The icon is there while Minimize One Launch to System Tray is on, as in the Electron app.
    cx.spawn({
        let life = Rc::clone(&life);
        async move |cx| {
            if let Ok(settings) = life.backend.settings().await {
                for key in [SettingKey::WindowCloseToTray, SettingKey::PlayHideToTray] {
                    life.heard(key, Some(&settings.toggle(key).to_string()));
                }
            }
            if life.close_to_tray.get() {
                add_tray_icon(&life, cx);
            }
            while let Ok((key, value)) = changes.recv().await {
                life.heard(key, value.as_deref());
                if key == SettingKey::WindowCloseToTray {
                    let shown = life.tray.borrow().is_some();
                    if life.close_to_tray.get() && !shown {
                        add_tray_icon(&life, cx);
                    } else if !life.close_to_tray.get() && shown {
                        drop(life.tray.borrow_mut().take());
                        println!("OneLaunch: tray icon removed");
                    }
                }
                let _ = life.window.update(cx, |shell, _, cx| {
                    shell.setting_changed(key, value, cx);
                });
            }
        }
    })
    .detach();

    window.update(cx, |_, window, cx| {
        after_the_next_frame(window, "first frame shown");
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |_, cx| {
                cx.spawn(async move |cx| copy_database(&backend, cx).await)
                    .detach();
            });
        });
        let life = Rc::clone(&life);
        window.on_window_should_close(cx, move |window, cx| {
            if life.stopping.get() {
                return false;
            }
            let tray = life.close_to_tray.get() && life.tray.borrow().is_some();
            match close_request_action(life.quitting.get(), tray, life.backend.is_busy()) {
                CloseAction::Close => true,
                CloseAction::Hide => {
                    hide_window("by the close button");
                    false
                }
                // A start still being prepared has no job to ask about: it is stopped, and the window closes.
                CloseAction::Ask => match life.backend.quit_question() {
                    Some(question) => {
                        // A minimised window is brought back for its question, once GPUI is done with this close.
                        cx.spawn(async |_| show_window()).detach();
                        shell.update(cx, |shell, cx| shell.ask_before_quit(question, window, cx));
                        false
                    }
                    None => true,
                },
            }
        });
    })?;

    let Some(check) = args.check else {
        return Ok(());
    };
    // A run that ends before its report, say because the window closed, has failed.
    failed.set(true);
    cx.spawn(async move |cx| {
        let outcome = match &check {
            Check::Capture(plan) => capture::run(plan, &life, cx).await.map(|()| true),
            Check::Probe => probe::run(&life, &failed, cx).await,
        };
        match outcome {
            Ok(passed) => failed.set(!passed),
            Err(error) => {
                eprintln!("OneLaunch: {error:#}");
                failed.set(true);
            }
        }
        quit(cx);
    })
    .detach();
    Ok(())
}

// A callback of a frame runs before the frame is drawn, so it is the second one that follows the frame shown.
fn after_the_next_frame(window: &mut Window, what: &'static str) {
    window.on_next_frame(move |window, _| {
        window.on_next_frame(move |_, _| {
            if let Some(age) = own_process::age() {
                println!(
                    "OneLaunch: {what} {} ms after the process started",
                    age.as_millis()
                );
            }
        });
    });
}

// After the first paint and not before it, as in the Electron app: the copy must not hold up the window. Neither line names the folder, which can run through the user's profile.
async fn copy_database(backend: &Backend, cx: &AsyncApp) {
    capture::pause(COPY_DELAY, cx).await;
    match backend.back_up_database().await {
        Ok(()) => println!("OneLaunch: database copied"),
        Err(error) => eprintln!("OneLaunch: database copy failed ({})", error.code),
    }
}

fn add_tray_icon(life: &Rc<Life>, cx: &AsyncApp) {
    let cx = cx.clone();
    let heard = Rc::clone(life);
    match Tray::new(life.title, move |event| on_tray_event(event, &heard, &cx)) {
        Ok(icon) => {
            println!("OneLaunch: tray icon added");
            *life.tray.borrow_mut() = Some(icon);
        }
        Err(error) => {
            eprintln!("OneLaunch: tray icon unavailable; closing the window quits: {error}");
        }
    }
}

// The tray calls this from its window procedure, between two GPUI updates. The menu waits here and not in a foreground task, where it would hold up every other task while it is open.
fn on_tray_event(event: TrayEvent, life: &Rc<Life>, cx: &AsyncApp) {
    let chosen = match event {
        TrayEvent::Activate => Some(0),
        TrayEvent::Menu(anchor) => anchor.show(&[
            MenuEntry::Item("Open One Launch"),
            MenuEntry::Separator,
            MenuEntry::Item("Quit One Launch"),
        ]),
    };
    match chosen {
        Some(0) => show_window(),
        Some(2) => {
            let life = Rc::clone(life);
            cx.spawn(async move |cx| request_quit(&life, cx)).detach();
        }
        _ => {}
    }
}

fn show_window() {
    if let Some(window) = AppWindow::on_this_thread() {
        window.show_and_focus();
    }
}

fn hide_window(why: &str) {
    if let Some(window) = AppWindow::on_this_thread() {
        window.hide();
        println!("OneLaunch: window hidden to the tray {why}");
    }
}

// "Quit anyway": the window and the tray stay while the switch is asked to stop and puts back what it must; the app quits once the job has ended. The Electron app gives up after 10 s; this waits for the switch's own timeouts.
fn quit_after_the_switch(life: &Rc<Life>, cx: &mut App) {
    life.stopping.set(true);
    let running = life.backend.running_job();
    println!(
        "OneLaunch: quit anyway: {}",
        if running.is_some() {
            "stopping the switch first"
        } else {
            "no switch runs any more"
        }
    );
    if let Some(job) = running {
        let _ = life.backend.cancel_launch(&job.id);
    }
    let life = Rc::clone(life);
    cx.spawn(async move |cx| {
        while life.backend.is_busy() {
            capture::pause(Duration::from_millis(100), cx).await;
        }
        life.quitting.set(true);
        println!("OneLaunch: the switch has stopped; quitting");
        quit(cx);
    })
    .detach();
}

// The tray's Quit: during a switch the window is shown and the question asked, as the Electron app's before-quit does.
fn request_quit(life: &Life, cx: &mut AsyncApp) {
    if life.stopping.get() {
        return show_window();
    }
    let Some(question) = life.backend.quit_question() else {
        life.quitting.set(true);
        return quit(cx);
    };
    show_window();
    let _ = life.window.update(cx, |shell, window, cx| {
        shell.ask_before_quit(question, window, cx);
    });
}

fn quit(cx: &AsyncApp) {
    cx.update(|cx| cx.quit());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(arguments: &[&str]) -> Board {
        let arguments = [r"--data-dir=D:\scratch\data"]
            .iter()
            .chain(arguments)
            .map(|argument| (*argument).to_owned());
        Board::for_run(&Args::parse(arguments).unwrap())
    }

    #[test]
    fn a_capture_or_a_probe_never_gets_the_windows_clipboard() {
        let checks: [&[&str]; 6] = [
            &[r"--capture=D:\scratch\shot.png"],
            &[r"--capture=D:\scratch\shot.png", "--capture-keys=ctrl-c"],
            &[r"--capture=D:\scratch\shot.png", "--capture-size=960x600"],
            &[r"--capture=D:\scratch\shot.png", "--capture-focus"],
            &[r"--capture=D:\scratch\shot.png", "--capture-type=abc"],
            &["--probe"],
        ];
        for check in checks {
            let board = board(check);
            assert_ne!(board, Board::Windows, "{check:?}");
            assert!(!board.keys_reach_it(), "{check:?}");
            assert!(!matches!(board.give(), Given::Windows(_)), "{check:?}");
            #[cfg(feature = "fake")]
            assert!(matches!(board.give(), Given::Recording(..)), "{check:?}");
            #[cfg(not(feature = "fake"))]
            assert!(matches!(board.give(), Given::None), "{check:?}");
            #[cfg(feature = "fake")]
            assert_eq!(board, Board::Recording, "{check:?}");
            #[cfg(not(feature = "fake"))]
            assert_eq!(board, Board::None, "{check:?}");
        }
        assert_eq!(board(&["--doctor"]), Board::None);
    }

    // Polls a future on this thread until it is ready.
    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        struct Unpark(std::thread::Thread);

        impl std::task::Wake for Unpark {
            fn wake(self: Arc<Self>) {
                self.0.unpark();
            }
        }

        let mut future = std::pin::pin!(future);
        let waker = Arc::new(Unpark(std::thread::current())).into();
        let mut context = std::task::Context::from_waker(&waker);
        loop {
            if let std::task::Poll::Ready(output) = future.as_mut().poll(&mut context) {
                return output;
            }
            std::thread::park();
        }
    }

    // What `main` hands the backend: in a capture or a probe the folders asked for reach the recording shell and nothing else.
    #[test]
    fn a_capture_or_a_probe_never_opens_a_folder_in_explorer() {
        let checks: [&[&str]; 6] = [
            &[r"--capture=D:\scratch\shot.png"],
            &[r"--capture=D:\scratch\shot.png", "--capture-keys=enter"],
            &[r"--capture=D:\scratch\shot.png", "--capture-size=960x600"],
            &[r"--capture=D:\scratch\shot.png", "--capture-focus"],
            &[r"--capture=D:\scratch\shot.png", "--capture-type=abc"],
            &["--probe"],
        ];
        let scratch = ol_testkit::Scratch::new("app-shell");
        for check in checks {
            let backend = Backend::start(scratch.join("data"), VERSION);
            let recorded = hand_shell(&parsed(check), &backend)
                .unwrap_or_else(|| panic!("{check:?} was handed the Windows shell"));
            for folder in [Folder::Data, Folder::Logs, Folder::Backups] {
                block_on(backend.open_folder(folder)).unwrap();
            }
            backend.shut_down();
            assert_eq!(
                *recorded.asked.lock().unwrap(),
                ["the data folder", "logs", "backups"],
                "{check:?}"
            );
            assert!(recorded.open_url("steam://rungameid/1").is_err());
        }
        assert_eq!(Opener::for_run(&parsed(&["--doctor"])), Opener::Recording);
        assert_eq!(Opener::for_run(&parsed(&[])), Opener::Windows);
    }

    // Locate's dialog is GPUI's `prompt_for_paths`, which only `FilePicker::Windows` reaches.
    #[test]
    fn a_capture_or_a_probe_never_opens_the_file_dialog() {
        let checks: [&[&str]; 7] = [
            &[r"--capture=D:\scratch\shot.png"],
            &[r"--capture=D:\scratch\shot.png", "--capture-keys=enter"],
            &[r"--capture=D:\scratch\shot.png", "--capture-size=960x600"],
            &[r"--capture=D:\scratch\shot.png", "--capture-focus"],
            &[r"--capture=D:\scratch\shot.png", "--capture-type=abc"],
            &["--probe"],
            &["--doctor"],
        ];
        for check in checks {
            assert!(
                matches!(file_picker(&parsed(check)), FilePicker::Scripted(_)),
                "{check:?} could open the file dialog"
            );
        }
        assert!(matches!(file_picker(&parsed(&[])), FilePicker::Windows));
    }

    fn parsed(arguments: &[&str]) -> Args {
        let arguments = [r"--data-dir=D:\scratch\data"]
            .iter()
            .chain(arguments)
            .map(|argument| (*argument).to_owned());
        Args::parse(arguments).unwrap()
    }

    #[test]
    fn a_run_a_person_uses_gets_the_windows_clipboard() {
        assert_eq!(board(&[]), Board::Windows);
        assert!(board(&[]).keys_reach_it());
        assert!(matches!(board(&[]).give(), Given::Windows(_)));
        #[cfg(not(feature = "fake"))]
        assert_eq!(board(&["--real-launchers"]), Board::Windows);
        #[cfg(feature = "fake")]
        assert_eq!(
            board(&[
                r"--fake-launchers=D:\scratch\launchers",
                r"--fake-scenario=D:\scratch\scenario",
                r"--fake-fixtures=D:\scratch\fixtures",
            ]),
            Board::Windows
        );
    }
}
