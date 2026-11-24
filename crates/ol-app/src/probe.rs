use std::cell::Cell;
use std::fmt::Debug;
use std::rc::Rc;
use std::time::Duration;

use anyhow::Context as _;
use gpui::AsyncApp;
use ol_platform_win::own_process;
use ol_platform_win::probe::{self, Area, MenuKey, OuterRect};
use ol_platform_win::tray::Tray;
use ol_platform_win::window::AppWindow;

use ol_model::SettingKey;

use crate::capture::pause;
use crate::{Life, TraySlot};

const SETTLE: Duration = Duration::from_millis(1200);
const BEAT: Duration = Duration::from_millis(700);
const IDLE: Duration = Duration::from_secs(5);
const QUIT: Duration = Duration::from_secs(3);

type Point = (i32, i32);

struct Report {
    window: AppWindow,
    failed: usize,
}

impl Report {
    fn check(&mut self, what: &str, passed: bool, seen: impl Debug) {
        if passed {
            println!("probe: ok    {what}");
        } else {
            self.failed += 1;
            println!("probe: FAIL  {what} (saw {seen:?})");
        }
    }

    fn skip(&self, what: &str) {
        println!("probe: skip  {what} (the step it follows failed)");
    }

    fn area(&mut self, what: &str, pointer: Point, at: Point, expected: Area) {
        let seen = probe::area_at(self.window, pointer, at);
        self.check(what, seen == expected, seen);
    }

    // As Windows does: the hit test decides which button the click is for.
    fn click(&self, at: Point) {
        let area = probe::area_at(self.window, at, at);
        probe::click(self.window, area, at.0, at.1);
    }

    fn double_click(&self, at: Point) {
        let area = probe::area_at(self.window, at, at);
        probe::double_click(self.window, area, at.0, at.1);
    }
}

struct Layout {
    width: i32,
    height: i32,
    scale: f32,
}

impl Layout {
    fn of(window: AppWindow, scale: f32) -> anyhow::Result<Self> {
        let (width, height) = window
            .client_size()
            .context("the window has no client area")?;
        Ok(Self {
            width,
            height,
            scale,
        })
    }

    fn px(&self, logical: f32) -> i32 {
        (logical * self.scale).round() as i32
    }

    fn middle(&self) -> Point {
        (self.width / 2, self.height / 2)
    }

    fn caption(&self) -> Point {
        (self.px(300.), self.px(20.))
    }

    fn button(&self, from_right: f32) -> Point {
        (self.width - self.px(46. * from_right - 23.), self.px(20.))
    }
}

pub async fn run(life: &Rc<Life>, failed: &Cell<bool>, cx: &mut AsyncApp) -> anyhow::Result<bool> {
    let tray = &life.tray;
    let window = AppWindow::on_this_thread().context("the GPUI window was not found")?;
    pause(SETTLE, cx).await;
    let mut report = Report { window, failed: 0 };

    let scale = window.scale();
    let layout = Layout::of(window, scale)?;
    println!(
        "probe: client area {}x{}, scale {scale}",
        layout.width, layout.height
    );
    print_cpu("shown and idle", cx).await;
    let (middle, caption) = (layout.middle(), layout.caption());
    let (minimize, maximize, close) = (layout.button(3.), layout.button(2.), layout.button(1.));
    let (right, bottom) = (layout.width + 3, layout.height + 3);

    report.area("content is client area", middle, middle, Area::Client);
    report.area(
        "the title bar is the caption",
        caption,
        caption,
        Area::Caption,
    );
    report.area("minimise button", minimize, minimize, Area::Minimize);
    report.area("maximise button", maximize, maximize, Area::Maximize);
    report.area("close button", close, close, Area::Close);
    report.area("the top edge resizes", caption, (caption.0, 1), Area::Top);
    report.area(
        "the top left corner resizes",
        middle,
        (-3, 1),
        Area::TopLeft,
    );
    report.area(
        "the top right corner resizes",
        middle,
        (right, 1),
        Area::TopRight,
    );
    report.area("the left edge resizes", middle, (-3, middle.1), Area::Left);
    report.area(
        "the right edge resizes",
        middle,
        (right, middle.1),
        Area::Right,
    );
    report.area(
        "the bottom edge resizes",
        middle,
        (middle.0, bottom),
        Area::Bottom,
    );
    report.area(
        "the bottom left corner resizes",
        middle,
        (-3, bottom),
        Area::BottomLeft,
    );
    report.area(
        "the bottom right corner resizes",
        middle,
        (right, bottom),
        Area::BottomRight,
    );

    let before = probe::outer_rect(window).context("the window has no frame")?;
    let small = OuterRect {
        width: 400,
        height: 300,
        ..before
    };
    probe::set_outer_rect(window, small);
    pause(BEAT, cx).await;
    let smallest = window.client_size();
    let least = Some((layout.px(960.), layout.px(600.)));
    report.check(
        "a 400x300 resize stops at 960x600",
        smallest == least,
        smallest,
    );
    probe::set_outer_rect(window, before);
    pause(BEAT, cx).await;

    report.click(maximize);
    pause(BEAT, cx).await;
    let is = window.is_maximized();
    report.check("the maximise button maximises", is, is);
    if is {
        let caption = Layout::of(window, scale)?.caption();
        report.area(
            "maximised: the top edge is caption",
            caption,
            (caption.0, 1),
            Area::Caption,
        );
        report.double_click(caption);
        pause(BEAT, cx).await;
        let is = window.is_maximized();
        report.check("a double-click on the title bar restores", !is, is);
    } else {
        report.skip("maximised: the top edge is caption");
        report.skip("a double-click on the title bar restores");
    }

    report.click(minimize);
    pause(BEAT, cx).await;
    let is = window.is_minimized();
    report.check("the minimise button minimises", is, is);
    if is {
        window.show_and_focus();
        pause(BEAT, cx).await;
        let is = window.is_minimized();
        report.check("showing the window restores it", !is, is);
    } else {
        report.skip("showing the window restores it");
    }

    let to_tray = |on: bool| {
        let backend = life.backend.clone();
        async move {
            backend
                .set_setting(SettingKey::WindowCloseToTray, on.to_string())
                .await
        }
    };
    let stored = to_tray(true).await;
    pause(BEAT, cx).await;
    let held = stored.is_ok() && with_tray(tray, probe::tray_is_shown) == Some(true);
    report.check("the shell holds the tray icon", held, held);
    let own = with_tray(tray, Tray::shows_app_icon) == Some(true);
    report.check("the tray icon is the app's own", own, own);

    let stored = to_tray(false).await;
    pause(BEAT, cx).await;
    let gone = stored.is_ok() && tray.borrow().is_none();
    report.check(
        "turning Minimize One Launch to System Tray off removes the icon at once",
        gone,
        gone,
    );
    let stored = to_tray(true).await;
    pause(BEAT, cx).await;
    let back = stored.is_ok() && with_tray(tray, probe::tray_is_shown) == Some(true);
    report.check("turning it on again puts the icon back", back, back);
    report.click(close);
    pause(BEAT, cx).await;
    let state = (
        AppWindow::on_this_thread() == Some(window),
        window.is_visible(),
    );
    report.check(
        "the close button hides the window and keeps it",
        state == (true, false),
        state,
    );
    if state == (true, false) {
        print_cpu("hidden in the tray", cx).await;
        with_tray(tray, probe::tray_click);
        pause(BEAT, cx).await;
        let is = window.is_visible();
        report.check("a tray click shows the window", is, is);
    } else {
        report.skip("a tray click shows the window");
    }
    println!(
        "probe: after the tray click the window {} the foreground window",
        if window.is_foreground() {
            "is"
        } else {
            "is not"
        }
    );

    with_tray(tray, probe::tray_menu);
    pause(BEAT, cx).await;
    let open = with_tray(tray, probe::tray_menu_is_open) == Some(true);
    report.check("a right-click on the tray icon opens its menu", open, open);
    if !open {
        println!("probe: {} failed", report.failed);
        return Ok(false);
    }

    println!(
        "probe: {} failed; now choosing Quit in the tray menu, which ends the app",
        report.failed
    );
    failed.set(report.failed != 0);
    with_tray(tray, |tray| {
        probe::tray_menu_key(tray, MenuKey::Up);
        probe::tray_menu_key(tray, MenuKey::Enter);
    });
    pause(QUIT, cx).await;
    probe::close_menu();
    println!("probe: FAIL  Quit in the tray menu ends the app (still running)");
    Ok(false)
}

fn with_tray<R>(tray: &TraySlot, act: impl FnOnce(&Tray) -> R) -> Option<R> {
    tray.borrow().as_ref().map(act)
}

async fn print_cpu(state: &str, cx: &AsyncApp) {
    let Some(before) = own_process::cpu_time() else {
        return;
    };
    pause(IDLE, cx).await;
    if let Some(after) = own_process::cpu_time() {
        println!(
            "probe: {state}, {} s took {:.1} ms of processor time",
            IDLE.as_secs(),
            after.saturating_sub(before).as_secs_f64() * 1000.
        );
    }
}
