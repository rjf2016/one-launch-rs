#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod args;
mod capture;
#[cfg(feature = "fake")]
mod fake;
mod probe;

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

use gpui::{App, AsyncApp, Window};
use ol_api::Backend;
use ol_platform_win::data_folder::refuse_real_data_folder;
use ol_platform_win::dpapi::Dpapi;
use ol_platform_win::own_process;
use ol_platform_win::tray::{MenuEntry, Tray, TrayEvent};
use ol_platform_win::window::AppWindow;
use ol_ui::{LibraryLoaded, Presence};

use crate::args::{Args, Check, DoctorVault};

const TITLE: &str = "One Launch";
// As in the Electron app: no picture of a fake run can pass for a real one.
#[cfg(feature = "fake")]
const FAKE_TITLE: &str = "One Launch (fake launchers)";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const COPY_DELAY: Duration = Duration::from_millis(250);

type TraySlot = Rc<RefCell<Option<Tray>>>;

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
    let failed = Rc::new(Cell::new(false));
    let tray = TraySlot::default();
    gpui_platform::application()
        .with_assets(ol_ui::Assets)
        .run({
            let (backend, failed, tray) = (backend.clone(), failed.clone(), tray.clone());
            move |cx| {
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
            Backend::start(args.data_dir.clone(), VERSION),
            Machine::None,
        ));
    }
    let backend = Backend::start_with(
        args.data_dir.clone(),
        VERSION,
        real_launchers(&args.data_dir)?,
        None,
    );
    Ok((backend, Machine::None))
}

// This PC behind the launchers. The Riot Client is given no way to its local server: Play is left to the user, as the hand-over says.
#[cfg(not(feature = "fake"))]
fn real_launchers(data_dir: &Path) -> anyhow::Result<ol_api::launchers::Launchers> {
    use anyhow::Context as _;
    use ol_platform_win::machine;

    let mut launchers = ol_api::launchers::on_this_pc(machine::platform(), data_dir, &Dpapi)
        .context("the vault of --data-dir could not be opened")?;
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
    Ok(launchers)
}

#[cfg(feature = "fake")]
fn start_backend(args: &Args) -> anyhow::Result<(Backend, Machine)> {
    let Some(machine) = &args.fake else {
        return Ok((
            Backend::start(args.data_dir.clone(), VERSION),
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
    ol_ui::init(cx);
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
    let window = ol_ui::open_main_window(title, presence, fake_launchers, backend.clone(), cx)?;
    let library = window.update(cx, |shell, _, _| shell.library().clone())?;
    cx.subscribe(&library, move |_, _: &LibraryLoaded, cx| {
        let _ = window.update(cx, |_, window, _| {
            after_the_next_frame(window, "library shown")
        });
    })
    .detach();

    // In a task and not here: adding the icon waits on the shell, Windows may hand the GPUI window a message meanwhile, and GPUI cannot take one while the app is being updated.
    cx.spawn({
        let tray = tray.clone();
        async move |cx| add_tray_icon(title, &tray, cx)
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
        let tray = tray.clone();
        window.on_window_should_close(cx, move |_, _| {
            if tray.borrow().is_none() {
                return true;
            }
            if let Some(window) = AppWindow::on_this_thread() {
                window.hide();
            }
            false
        });
    })?;

    let Some(check) = args.check else {
        return Ok(());
    };
    // A run that ends before its report, say because the window closed, has failed.
    failed.set(true);
    cx.spawn(async move |cx| {
        let outcome = match &check {
            Check::Capture(plan) => capture::run(plan, window, cx).await.map(|()| true),
            Check::Probe => probe::run(&tray, &failed, cx).await,
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
        Err(error) => eprintln!("OneLaunch: database copy failed: {error}"),
    }
}

fn add_tray_icon(title: &str, tray: &TraySlot, cx: &AsyncApp) {
    let cx = cx.clone();
    match Tray::new(title, move |event| on_tray_event(event, &cx)) {
        Ok(icon) => {
            println!("OneLaunch: tray icon added");
            *tray.borrow_mut() = Some(icon);
        }
        Err(error) => {
            eprintln!("OneLaunch: tray icon unavailable; closing the window quits: {error}");
        }
    }
}

// The tray calls this from its window procedure, between two GPUI updates. The menu waits here and not in a foreground task, where it would hold up every other task while it is open.
fn on_tray_event(event: TrayEvent, cx: &AsyncApp) {
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
        Some(2) => cx.spawn(async |cx| quit(cx)).detach(),
        _ => {}
    }
}

fn show_window() {
    if let Some(window) = AppWindow::on_this_thread() {
        window.show_and_focus();
    }
}

fn quit(cx: &AsyncApp) {
    cx.update(|cx| cx.quit());
}
