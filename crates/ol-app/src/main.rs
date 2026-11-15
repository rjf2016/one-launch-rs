#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod args;
mod capture;
mod probe;

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

use gpui::{App, AsyncApp};
use ol_api::Backend;
use ol_platform_win::data_folder::refuse_real_data_folder;
use ol_platform_win::dpapi::Dpapi;
use ol_platform_win::own_process;
use ol_platform_win::tray::{MenuEntry, Tray, TrayEvent};
use ol_platform_win::window::AppWindow;
use ol_ui::Presence;

use crate::args::{Args, Check, DoctorVault};

const TITLE: &str = "One Launch (GPUI proof)";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const COPY_DELAY: Duration = Duration::from_millis(250);

type TraySlot = Rc<RefCell<Option<Tray>>>;

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
    for (argument, path) in [Some(("--data-dir", args.data_dir.as_path())), capture]
        .into_iter()
        .flatten()
    {
        if let Err(refusal) = refuse_real_data_folder(argument, path) {
            eprintln!("OneLaunch: {refusal}");
            return ExitCode::from(2);
        }
    }
    if let Some(vault) = args.doctor {
        return doctor(&args.data_dir, vault);
    }
    // Started first: the database opens on its own thread while the window is being made.
    let backend = Backend::start(args.data_dir.clone(), VERSION);
    let failed = Rc::new(Cell::new(false));
    let tray = TraySlot::default();
    gpui_platform::application()
        .with_assets(ol_ui::Assets)
        .run({
            let (backend, failed, tray) = (backend.clone(), failed.clone(), tray.clone());
            move |cx| {
                if let Err(error) = start(args, backend, failed.clone(), tray, cx) {
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

fn start(
    args: Args,
    backend: Backend,
    failed: Rc<Cell<bool>>,
    tray: TraySlot,
    cx: &mut App,
) -> anyhow::Result<()> {
    ol_ui::init(cx);
    let presence = match &args.check {
        Some(Check::Capture(plan)) if !plan.focus => Presence::Background,
        Some(Check::Probe) => Presence::Background,
        _ => Presence::Foreground,
    };
    let window = ol_ui::open_main_window(TITLE, presence, backend.clone(), cx)?;

    // In a task and not here: adding the icon waits on the shell, Windows may hand the GPUI window a message meanwhile, and GPUI cannot take one while the app is being updated.
    cx.spawn({
        let tray = tray.clone();
        async move |cx| add_tray_icon(&tray, cx)
    })
    .detach();

    window.update(cx, |_, window, cx| {
        // These callbacks run before their frame is drawn, so it is the second one that follows the first frame shown.
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |_, cx| {
                if let Some(age) = own_process::age() {
                    println!(
                        "OneLaunch: first frame shown {} ms after the process started",
                        age.as_millis()
                    );
                }
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

// After the first paint and not before it, as in the Electron app: the copy must not hold up the window. Neither line names the folder, which can run through the user's profile.
async fn copy_database(backend: &Backend, cx: &AsyncApp) {
    capture::pause(COPY_DELAY, cx).await;
    match backend.back_up_database().await {
        Ok(()) => println!("OneLaunch: database copied"),
        Err(error) => eprintln!("OneLaunch: database copy failed: {error}"),
    }
}

fn add_tray_icon(tray: &TraySlot, cx: &AsyncApp) {
    let cx = cx.clone();
    match Tray::new(TITLE, move |event| on_tray_event(event, &cx)) {
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
