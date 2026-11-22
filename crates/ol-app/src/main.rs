#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod args;
mod boxes;
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

use gpui::{App, AsyncApp, Bounds, Task, Window, WindowBounds, WindowHandle, point, px, size};
use ol_api::Backend;
use ol_api::library::HourClock;
use ol_api::log::{Log, LogOptions, Logger, log_panics, register_windows_user};
use ol_api::settings::{
    AfterPlay, CloseAction, Folder, Rect, bounds_setting, close_request_action,
    hide_after_instant_play, placement_from, start_hidden,
};
use ol_model::{SettingKey, Settings};
use ol_platform_api::{Shell as FolderShell, ShellError};
use ol_platform_win::data_folder::refuse_real_data_folder;
use ol_platform_win::dpapi::Dpapi;
use ol_platform_win::instance::{self, Claim, Instance, InstanceEvent};
use ol_platform_win::own_process;
use ol_platform_win::shell::WindowsShell;
use ol_platform_win::tray::{MenuEntry, Tray, TrayEvent};
use ol_platform_win::window::AppWindow;
use ol_ui::{FilePicker, LibraryLoaded, Opening, Played, Presence, QuitAnyway, Shell};

use crate::args::{Args, Check, DoctorVault};
use crate::boxes::Boxes;

const TITLE: &str = "One Launch";
// As in the Electron app: no picture of a fake run can pass for a real one.
#[cfg(feature = "fake")]
const FAKE_TITLE: &str = "One Launch (fake launchers)";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const COPY_DELAY: Duration = Duration::from_millis(250);
const LOG_CLOSE: Duration = Duration::from_secs(2);
// How long a second start looks for the first copy's window, which may still be opening.
const SECOND_START_WAIT: Duration = Duration::from_secs(10);
// As the Electron app's: the place is stored once the window has kept still this long.
const BOUNDS_SETTLE: Duration = Duration::from_millis(500);
const DEFAULT_SIZE: (f32, f32) = (1280., 800.);

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

// The one place a run is handed the picker Locate asks with. A capture or a probe is run by agents, so it is answered from the run's steps and never opens a dialog on the owner's desktop.
fn hand_picker(args: &Args, set: impl FnOnce(FilePicker)) -> FilePicker {
    let picker = match Opener::for_run(args) {
        Opener::Windows => FilePicker::Windows,
        Opener::Recording => FilePicker::Scripted(Rc::default()),
    };
    set(picker.clone());
    picker
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
    // A capture or a probe: nothing takes the keyboard from the window that has it.
    quiet: bool,
    // Stored maximised and not shown so yet: a start in the tray, or a run that takes no focus. Stored maximised meanwhile, as the Electron app's pending maximize is.
    pending_maximize: Cell<bool>,
    bounds_timer: RefCell<Option<Task<()>>>,
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
    let instance = match claim(&args) {
        Ok(instance) => instance,
        Err(exit) => return exit,
    };
    let logger = open_log(&args.data_dir);
    let boxes = Boxes::for_run(&args);
    boxes::watch(boxes, &args.data_dir);
    let data_dir = args.data_dir.clone();
    let (exit, failure) = run(args, &logger.log(), instance.clone());
    logger.close(LOG_CLOSE);
    if let Some(reason) = failure {
        boxes::could_not_start(boxes, &reason, &data_dir);
    }
    drop(instance);
    if boxes::relaunch_asked() {
        boxes::relaunch();
    }
    exit
}

// Before the log, the database and the vault are opened, as the Electron app's single-instance lock: a second start on the same data folder brings the first copy's window forward and leaves with 0. A lock Windows will not make leaves this copy running without one.
fn claim(args: &Args) -> Result<Option<Rc<Instance>>, ExitCode> {
    match instance::claim(&args.data_dir) {
        Ok(Claim::First(held)) => Ok(Some(Rc::new(held))),
        Ok(Claim::Second) => {
            let quiet = args.check.is_some();
            let found = instance::bring_forward(&args.data_dir, quiet, SECOND_START_WAIT);
            println!(
                "OneLaunch: another copy runs on this data folder; {}",
                if found {
                    "its window was asked to come forward"
                } else {
                    "its window was not found"
                }
            );
            Err(ExitCode::SUCCESS)
        }
        Err(error) => {
            eprintln!("OneLaunch: {error}; this copy runs without it");
            Ok(None)
        }
    }
}

// The Windows user is registered before the first line, so no line can name them.
fn open_log(data_dir: &Path) -> Arc<Logger> {
    let folders = ol_platform_win::machine::folders();
    register_windows_user(
        folders.user_profile.as_deref(),
        folders.user_name.as_deref(),
    );
    let logger = Arc::new(Logger::for_data_folder(data_dir, LogOptions::default()));
    log_panics(Arc::clone(&logger));
    logger
}

fn run(args: Args, log: &Log, instance: Option<Rc<Instance>>) -> (ExitCode, Option<String>) {
    // Started first: the database opens on its own thread while the window is being made.
    let (backend, machine) = match start_backend(&args, log) {
        Ok(started) => started,
        Err(problem) => {
            eprintln!("OneLaunch: {problem:#}");
            return (ExitCode::from(2), None);
        }
    };
    let heard = use_clipboard(Board::for_run(&args).give(), &backend);
    let _recorded = hand_shell(&args, &backend);
    let picker = Rc::new(RefCell::new(None));
    let failed = Rc::new(Cell::new(false));
    let failure = Rc::new(RefCell::new(None));
    let tray = TraySlot::default();
    gpui_platform::application()
        .with_assets(ol_ui::Assets)
        .run({
            let started = Start {
                backend: backend.clone(),
                machine,
                failed: failed.clone(),
                failure: Rc::clone(&failure),
                tray: tray.clone(),
                instance,
            };
            let picker = Rc::clone(&picker);
            move |cx| {
                *picker.borrow_mut() = Some(hand_picker(&args, |given| cx.set_global(given)));
                start(args, started, cx);
            }
        });
    boxes::window_up(false);
    // Here and nowhere else: whichever way the message loop ended, the icon goes with it.
    let icon = tray.borrow_mut().take();
    if icon.is_some() {
        drop(icon);
        backend.window_log().tray_removed();
    }
    backend.shut_down();
    if let Some(heard) = heard {
        heard();
    }
    if let Some(picker) = picker.borrow().as_ref() {
        report_picks(picker);
    }
    let failure = failure.borrow_mut().take();
    let exit = if failed.get() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    };
    (exit, failure)
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
fn start_backend(args: &Args, log: &Log) -> anyhow::Result<(Backend, Machine)> {
    if !args.real_launchers {
        return Ok((
            Backend::logging_to(log.clone()).start_with_vault_of_data_folder(
                args.data_dir.clone(),
                VERSION,
                Box::new(Dpapi),
            ),
            Machine::None,
        ));
    }
    use anyhow::Context as _;

    // A key Windows does not unwrap does not stop the start: the vault comes up with its key lost, for the Encryption panel's reset.
    let backend = Backend::logging_to(log.clone())
        .start_on_this_pc(
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
fn start_backend(args: &Args, log: &Log) -> anyhow::Result<(Backend, Machine)> {
    let Some(machine) = &args.fake else {
        return Ok((
            Backend::logging_to(log.clone()).start_with_vault_of_data_folder(
                args.data_dir.clone(),
                VERSION,
                Box::new(Dpapi),
            ),
            Machine::None,
        ));
    };
    let (backend, seed) = fake::start(&args.data_dir, machine, VERSION, log)?;
    Ok((backend, Machine::Fake(seed)))
}

struct Start {
    backend: Backend,
    machine: Machine,
    failed: Rc<Cell<bool>>,
    failure: Rc<RefCell<Option<String>>>,
    tray: TraySlot,
    instance: Option<Rc<Instance>>,
}

fn start(args: Args, started: Start, cx: &mut App) {
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
    // As the Electron app's bootstrap: the data folder is read before the window is made, so it opens where it was and goes to the tray as the settings say, and a database that cannot be opened ends the start with its box.
    cx.spawn(async move |cx| {
        let Start {
            backend,
            machine,
            failed,
            failure,
            tray,
            instance,
        } = started;
        if let Err(error) = backend.settings().await {
            return fail_start(&backend, error.message, &failed, &failure, cx);
        }
        let title = match machine {
            Machine::None => TITLE,
            #[cfg(feature = "fake")]
            Machine::Fake(seed) => {
                // The code and not the sentence, which can name an account. Before the window, so the setup guide's question about accounts has the scenario's answer.
                if let Err(error) = fake::seed(&backend, seed).await {
                    eprintln!(
                        "OneLaunch: the scenario's accounts could not be made ({})",
                        error.code
                    );
                }
                FAKE_TITLE
            }
        };
        let settings = match backend.settings().await {
            Ok(settings) => settings,
            Err(error) => return fail_start(&backend, error.message, &failed, &failure, cx),
        };
        let opened = cx.update(|cx| {
            open(
                args,
                Opened {
                    backend: backend.clone(),
                    settings,
                    title,
                    failed: failed.clone(),
                    tray,
                    instance,
                },
                cx,
            )
        });
        if let Err(error) = opened {
            eprintln!("OneLaunch: {error:#}");
            fail_start(&backend, format!("{error:#}"), &failed, &failure, cx);
        }
    })
    .detach();
}

fn fail_start(
    backend: &Backend,
    reason: String,
    failed: &Cell<bool>,
    failure: &RefCell<Option<String>>,
    cx: &AsyncApp,
) {
    backend.window_log().fatal_during_startup();
    *failure.borrow_mut() = Some(reason);
    failed.set(true);
    quit(cx);
}

struct Opened {
    backend: Backend,
    settings: Settings,
    title: &'static str,
    failed: Rc<Cell<bool>>,
    tray: TraySlot,
    instance: Option<Rc<Instance>>,
}

fn work_areas(cx: &App) -> Vec<Rect> {
    cx.displays()
        .iter()
        .map(|display| {
            let area = display.visible_bounds();
            Rect {
                x: f64::from(area.origin.x.as_f32()),
                y: f64::from(area.origin.y.as_f32()),
                width: f64::from(area.size.width.as_f32()),
                height: f64::from(area.size.height.as_f32()),
            }
        })
        .collect()
}

fn open(args: Args, opened: Opened, cx: &mut App) -> anyhow::Result<()> {
    let Opened {
        backend,
        settings,
        title,
        failed,
        tray,
        instance,
    } = opened;
    let fake_launchers = title != TITLE;
    let presence = match &args.check {
        Some(Check::Capture(plan)) if !plan.focus => Presence::Background,
        Some(Check::Probe) => Presence::Background,
        _ => Presence::Foreground,
    };
    let quiet = presence == Presence::Background;
    let close_to_tray = settings.toggle(SettingKey::WindowCloseToTray);
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let hidden = start_hidden(&argv, close_to_tray);
    let placement = placement_from(settings.get(SettingKey::WindowBounds), &work_areas(cx));
    let restore = placement.bounds.map_or_else(
        || Bounds::centered(None, size(px(DEFAULT_SIZE.0), px(DEFAULT_SIZE.1)), cx),
        |stored| Bounds {
            origin: point(px(stored.x as f32), px(stored.y as f32)),
            size: size(px(stored.width as f32), px(stored.height as f32)),
        },
    );
    // Windows maximises only with the focus: a run that takes none, or a start in the tray, leaves it for the first show that may.
    let maximize_now = placement.maximized && !hidden && !quiet;
    // Heard from before the window opens, so no change made meanwhile is missed.
    let changes = backend.setting_changes();
    let window = ol_ui::open_main_window(
        Opening {
            title,
            presence,
            fake_launchers,
            developer: cfg!(feature = "fake"),
            bounds: restore,
            maximized: maximize_now,
            shown: !hidden,
        },
        backend.clone(),
        cx,
    )?;
    if hidden && let Some(native) = AppWindow::on_this_thread() {
        native.place_hidden(
            restore.origin.x.as_f32(),
            restore.origin.y.as_f32(),
            restore.size.width.as_f32(),
            restore.size.height.as_f32(),
        );
    }
    boxes::window_up(true);
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
        quiet,
        pending_maximize: Cell::new(placement.maximized && !maximize_now),
        bounds_timer: RefCell::new(None),
    });
    if let Some(instance) = &instance {
        let (life, cx) = (Rc::clone(&life), cx.to_async());
        instance.listen(move |event| on_instance_event(event, &life, &cx));
    }
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
                let (log, job) = (life.backend.window_log(), played.job.id.clone());
                cx.spawn(async move |_| {
                    if hide_window() {
                        log.hidden_after_instant_launch(&job);
                    }
                })
                .detach();
            }
        }
    })
    .detach();

    // In a task and not here: adding the icon waits on the shell, Windows may hand the GPUI window a message meanwhile, and GPUI cannot take one while the app is being updated. The icon is there while Minimize One Launch to System Tray is on, as in the Electron app.
    cx.spawn({
        let life = Rc::clone(&life);
        async move |cx| {
            for key in [SettingKey::WindowCloseToTray, SettingKey::PlayHideToTray] {
                life.heard(key, Some(&settings.toggle(key).to_string()));
            }
            if life.close_to_tray.get() {
                add_tray_icon(&life, cx);
            }
            // Without the icon nothing could bring back a window that never showed.
            if hidden {
                if life.tray.borrow().is_some() {
                    life.backend.window_log().started_in_tray();
                } else {
                    show_window(&life);
                }
            }
            while let Ok((key, value)) = changes.recv().await {
                life.heard(key, value.as_deref());
                if key == SettingKey::WindowCloseToTray {
                    let shown = life.tray.borrow().is_some();
                    if life.close_to_tray.get() && !shown {
                        add_tray_icon(&life, cx);
                    } else if !life.close_to_tray.get() && shown {
                        drop(life.tray.borrow_mut().take());
                        life.backend.window_log().tray_removed();
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
        cx.observe_window_bounds(window, {
            let life = Rc::clone(&life);
            move |_, _, cx| {
                let settled = Rc::clone(&life);
                let timer = cx.spawn(async move |_, cx| {
                    cx.background_executor().timer(BOUNDS_SETTLE).await;
                    let _ = settled
                        .window
                        .update(cx, |_, window, _| remember_bounds(&settled, window));
                });
                *life.bounds_timer.borrow_mut() = Some(timer);
            }
        })
        .detach();
        let life = Rc::clone(&life);
        window.on_window_should_close(cx, move |window, cx| {
            if life.stopping.get() {
                return false;
            }
            let tray = life.close_to_tray.get() && life.tray.borrow().is_some();
            let action = close_request_action(life.quitting.get(), tray, life.backend.is_busy());
            life.backend.window_log().close_requested(action);
            match action {
                CloseAction::Close => {
                    remember_bounds(&life, window);
                    true
                }
                CloseAction::Hide => {
                    remember_bounds(&life, window);
                    if hide_window() {
                        life.backend.window_log().hidden_to_tray();
                    }
                    false
                }
                // A start still being prepared has no job to ask about: it is stopped, and the window closes.
                CloseAction::Ask => match life.backend.quit_question() {
                    Some(question) => {
                        // A minimised window is brought back for its question, once GPUI is done with this close.
                        let shown = Rc::clone(&life);
                        cx.spawn(async move |_| {
                            show_window(&shown);
                        })
                        .detach();
                        shell.update(cx, |shell, cx| shell.ask_before_quit(question, window, cx));
                        false
                    }
                    None => {
                        remember_bounds(&life, window);
                        true
                    }
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
        quit_app(&life, cx);
    })
    .detach();
    Ok(())
}

// The place as the Electron app stores it, while the window can be seen: one hidden in the tray or minimised keeps the place it was last seen at, maximised or not.
fn remember_bounds(life: &Life, window: &Window) {
    let Some(native) = AppWindow::on_this_thread() else {
        return;
    };
    if !native.is_visible() || native.is_minimized() {
        return;
    }
    let (bounds, maximized) = match window.window_bounds() {
        WindowBounds::Maximized(bounds) => (bounds, true),
        WindowBounds::Windowed(bounds) | WindowBounds::Fullscreen(bounds) => (bounds, false),
    };
    let whole = |value: gpui::Pixels| value.as_f32().round() as i32;
    life.backend.remember_window_bounds(bounds_setting(
        whole(bounds.origin.x),
        whole(bounds.origin.y),
        whole(bounds.size.width),
        whole(bounds.size.height),
        maximized || life.pending_maximize.get(),
    ));
}

// Called from the message loop, between two GPUI updates, as the tray's handler is.
fn on_instance_event(event: InstanceEvent, life: &Rc<Life>, cx: &AsyncApp) {
    match event {
        InstanceEvent::Show { quiet } => {
            if bring_window_forward(life, !quiet) {
                println!(
                    "OneLaunch: a second start on this data folder brought the window forward"
                );
                life.backend.window_log().shown_for_second_start();
            }
        }
        InstanceEvent::Quit => {
            life.quitting.set(true);
            let life = Rc::clone(life);
            cx.spawn(async move |cx| quit_app(&life, cx)).detach();
        }
    }
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

// After the first paint and not before it, as in the Electron app: the copy must not hold up the window.
async fn copy_database(backend: &Backend, cx: &AsyncApp) {
    capture::pause(COPY_DELAY, cx).await;
    let _ = backend.back_up_database().await;
}

fn add_tray_icon(life: &Rc<Life>, cx: &AsyncApp) {
    let cx = cx.clone();
    let heard = Rc::clone(life);
    match Tray::new(life.title, move |event| on_tray_event(event, &heard, &cx)) {
        Ok(icon) => {
            *life.tray.borrow_mut() = Some(icon);
            life.backend.window_log().tray_added();
        }
        Err(error) => life
            .backend
            .window_log()
            .tray_unavailable(error.code(), error.hresult()),
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
        Some(0) => show_from_tray(life),
        Some(2) => {
            let life = Rc::clone(life);
            cx.spawn(async move |cx| request_quit(&life, cx)).detach();
        }
        _ => {}
    }
}

// Out of the tray or the taskbar; with the focus, and then maximised when that was left for the first show, unless the run takes no focus.
fn bring_window_forward(life: &Life, focus: bool) -> bool {
    let Some(window) = AppWindow::on_this_thread() else {
        return false;
    };
    let focus = focus && !life.quiet;
    let maximize = focus && life.pending_maximize.replace(false);
    window.bring_forward(maximize, focus);
    true
}

fn show_window(life: &Life) {
    bring_window_forward(life, true);
}

fn show_from_tray(life: &Life) {
    show_window(life);
    life.backend.window_log().shown_from_tray();
}

fn hide_window() -> bool {
    let Some(window) = AppWindow::on_this_thread() else {
        return false;
    };
    window.hide();
    true
}

// "Quit anyway": the window and the tray stay while the switch is asked to stop and puts back what it must; the app quits once the job has ended. The Electron app gives up after 10 s; this waits for the switch's own timeouts.
fn quit_after_the_switch(life: &Rc<Life>, cx: &mut App) {
    life.stopping.set(true);
    if life.backend.is_busy() {
        life.backend.window_log().stopping_for_quit();
    }
    if let Some(job) = life.backend.running_job() {
        let _ = life.backend.cancel_launch(&job.id);
    }
    let life = Rc::clone(life);
    cx.spawn(async move |cx| {
        while life.backend.is_busy() {
            capture::pause(Duration::from_millis(100), cx).await;
        }
        life.quitting.set(true);
        quit_app(&life, cx);
    })
    .detach();
}

// The tray's Quit: during a switch the window is shown and the question asked, as the Electron app's before-quit does.
fn request_quit(life: &Life, cx: &mut AsyncApp) {
    if life.stopping.get() {
        return show_window(life);
    }
    let Some(question) = life.backend.quit_question() else {
        life.quitting.set(true);
        return quit_app(life, cx);
    };
    show_window(life);
    let _ = life.window.update(cx, |shell, window, cx| {
        shell.ask_before_quit(question, window, cx);
    });
}

fn quit(cx: &AsyncApp) {
    cx.update(|cx| cx.quit());
}

// A quit the app itself asks for stores the window's place first, as the Electron app's close does.
fn quit_app(life: &Life, cx: &mut AsyncApp) {
    let _ = life
        .window
        .update(cx, |_, window, _| remember_bounds(life, window));
    quit(cx);
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
        let checks: [&[&str]; 6] = [
            &[r"--capture=D:\scratch\shot.png"],
            &[r"--capture=D:\scratch\shot.png", "--capture-keys=enter"],
            &[r"--capture=D:\scratch\shot.png", "--capture-size=960x600"],
            &[r"--capture=D:\scratch\shot.png", "--capture-focus"],
            &[r"--capture=D:\scratch\shot.png", "--capture-type=abc"],
            &["--probe"],
        ];
        let handed = |arguments: &[&str]| {
            let mut set = None;
            let kept = hand_picker(&parsed(arguments), |given| set = Some(given));
            (set.expect("the picker was handed on"), kept)
        };
        for check in checks {
            let (set, kept) = handed(check);
            assert!(
                matches!(set, FilePicker::Scripted(_)) && matches!(kept, FilePicker::Scripted(_)),
                "{check:?} could open the file dialog"
            );
        }
        let (set, _) = handed(&[]);
        assert!(matches!(set, FilePicker::Windows));
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
