#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod args;
mod capture;
mod probe;

use std::cell::{Cell, RefCell};
use std::process::ExitCode;
use std::rc::Rc;

use gpui::{App, AsyncApp};
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
    gpui_platform::application()
        .with_assets(ol_ui::Assets)
        .run({
            let failed = failed.clone();
            move |cx| {
                if let Err(error) = start(args, failed.clone(), cx) {
                    eprintln!("OneLaunch: {error:#}");
                    failed.set(true);
                    cx.quit();
                }
            }
        });
    if failed.get() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn start(args: Args, failed: Rc<Cell<bool>>, cx: &mut App) -> anyhow::Result<()> {
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

    let tray = TraySlot::default();
    let on_event = {
        let (tray, cx) = (tray.clone(), cx.to_async());
        move |event| on_tray_event(event, &cx, tray.clone())
    };
    match Tray::new(TITLE, on_event) {
        Ok(icon) => {
            println!("OneLaunch: tray icon added");
            *tray.borrow_mut() = Some(icon);
        }
        Err(error) => {
            eprintln!("OneLaunch: tray icon unavailable; closing the window quits: {error}");
        }
    }

    let close_to_tray = tray.borrow().is_some();
    window.update(cx, |_, window, cx| {
        window.on_window_should_close(cx, move |_, _| {
            if !close_to_tray {
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
        quit(&tray, cx);
    })
    .detach();
    Ok(())
}

// The tray calls this from its window procedure, between two GPUI updates. The menu waits here and not in a foreground task, where it would hold up every other task while it is open.
fn on_tray_event(event: TrayEvent, cx: &AsyncApp, tray: TraySlot) {
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
        Some(2) => cx.spawn(async move |cx| quit(&tray, cx)).detach(),
        _ => {}
    }
}

fn show_window() {
    if let Some(window) = AppWindow::on_this_thread() {
        window.show_and_focus();
    }
}

fn quit(tray: &TraySlot, cx: &AsyncApp) {
    let icon = tray.borrow_mut().take();
    drop(icon);
    cx.update(|cx| cx.quit());
}
