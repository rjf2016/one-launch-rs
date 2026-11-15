#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod args;
mod capture;
mod probe;

use std::cell::{Cell, RefCell};
use std::process::ExitCode;
use std::rc::Rc;

use gpui::{App, AsyncApp};
use ol_platform_win::own_process;
use ol_platform_win::tray::{MenuEntry, Tray, TrayEvent};
use ol_platform_win::window::AppWindow;
use ol_ui::Presence;

use crate::args::{Args, Check};

const TITLE: &str = "One Launch (GPUI proof)";

type TraySlot = Rc<RefCell<Option<Tray>>>;

fn main() -> ExitCode {
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(problem) => {
            eprintln!("OneLaunch: {problem}");
            return ExitCode::from(2);
        }
    };
    let failed = Rc::new(Cell::new(false));
    let tray = TraySlot::default();
    gpui_platform::application()
        .with_assets(ol_ui::Assets)
        .run({
            let (failed, tray) = (failed.clone(), tray.clone());
            move |cx| {
                if let Err(error) = start(args, failed.clone(), tray, cx) {
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
    if failed.get() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn start(args: Args, failed: Rc<Cell<bool>>, tray: TraySlot, cx: &mut App) -> anyhow::Result<()> {
    println!(
        "OneLaunch: data folder {} (the proof stores nothing)",
        args.data_dir.display()
    );
    ol_ui::init(cx);
    let presence = match &args.check {
        Some(Check::Capture(plan)) if !plan.focus => Presence::Background,
        Some(Check::Probe) => Presence::Background,
        _ => Presence::Foreground,
    };
    let window = ol_ui::open_main_window(TITLE, presence, cx)?;

    // In a task and not here: adding the icon waits on the shell, Windows may hand the GPUI window a message meanwhile, and GPUI cannot take one while the app is being updated.
    cx.spawn({
        let tray = tray.clone();
        async move |cx| add_tray_icon(&tray, cx)
    })
    .detach();

    window.update(cx, |_, window, cx| {
        // These callbacks run before their frame is drawn, so it is the second one that follows the first frame shown.
        window.on_next_frame(|window, _| {
            window.on_next_frame(|_, _| {
                if let Some(age) = own_process::age() {
                    println!(
                        "OneLaunch: first frame shown {} ms after the process started",
                        age.as_millis()
                    );
                }
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
