use std::time::Duration;

use anyhow::Context as _;
use gpui::{
    AnyWindowHandle, App, AsyncApp, Keystroke, Modifiers, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, PlatformInput, Point, Window, WindowHandle, point, px,
};
use ol_platform_win::own_process;
use ol_platform_win::probe::{self, OuterRect};
use ol_platform_win::window::AppWindow;
use ol_ui::{LibraryView, Shell};

use crate::args::CapturePlan;

const SETTLE: Duration = Duration::from_millis(1200);
const STEP: Duration = Duration::from_millis(400);
const SWEEP_STEP: f32 = 30.;
const LONGEST_WAIT: u64 = 60_000;

pub async fn run(
    plan: &CapturePlan,
    window: WindowHandle<Shell>,
    cx: &mut AsyncApp,
) -> anyhow::Result<()> {
    let native = AppWindow::on_this_thread().context("the GPUI window was not found")?;
    pause(SETTLE, cx).await;

    if let Some((width, height)) = plan.size {
        let client = native
            .client_size()
            .context("the window has no client area")?;
        let outer = probe::outer_rect(native).context("the window has no frame")?;
        let resized = OuterRect {
            width: width + outer.width - client.0,
            height: height + outer.height - client.1,
            ..outer
        };
        probe::set_outer_rect(native, resized);
        pause(STEP, cx).await;
    }

    if let Some(frames) = plan.sweep {
        in_library(window, cx, |library, window, cx| {
            library.start_sweep(frames, SWEEP_STEP, window, cx);
        })?;
        // A window that is hidden or minimised gets no frames, and the sweep would wait for ever.
        let mut polls_left = frames * 2 + 100;
        let report = loop {
            pause(Duration::from_millis(50), cx).await;
            if let Some(report) =
                in_library(window, cx, |library, _, _| library.take_sweep_report())?
            {
                break report;
            }
            polls_left = polls_left
                .checked_sub(1)
                .context("the sweep stopped getting frames")?;
        };
        println!(
            "OneLaunch: sweep of {} frames at {SWEEP_STEP} px a frame: median {:.2} ms, 95th {:.2} ms, longest {:.2} ms, {} over 1.5 x median",
            report.frames, report.median_ms, report.p95_ms, report.max_ms, report.late
        );
        pause(STEP, cx).await;
    }

    if let Some(row) = plan.row {
        in_library(window, cx, |library, _, cx| library.scroll_to_row(row, cx))?;
        pause(STEP, cx).await;
    }

    if let Some(text) = &plan.text {
        in_library(window, cx, |library, window, cx| {
            library.focus_search(window, cx);
        })?;
        pause(STEP, cx).await;
        native.post_text(text);
        pause(STEP, cx).await;
    }

    for step in &plan.keys {
        let step = match Step::parse(step)? {
            Step::Wait(time) => {
                pause(time, cx).await;
                continue;
            }
            step => step,
        };
        // Through the untyped handle: a key press can redraw the window, and the typed one holds the root view meanwhile.
        AnyWindowHandle::from(window).update(cx, |_, window, cx| step.take(window, cx))?;
        pause(STEP, cx).await;
    }

    let shot = native.capture()?;
    image::save_buffer(
        &plan.file,
        &shot.rgba,
        shot.width,
        shot.height,
        image::ExtendedColorType::Rgba8,
    )
    .with_context(|| format!("could not write {}", plan.file.display()))?;
    println!(
        "OneLaunch: captured {}x{} to {}",
        shot.width,
        shot.height,
        plan.file.display()
    );
    if let Some(used) = own_process::memory() {
        println!(
            "OneLaunch: working set {:.1} MB, private {:.1} MB",
            megabytes(used.working_set),
            megabytes(used.private)
        );
    }
    Ok(())
}

// A step of --capture-keys: a key press, the pointer at a place of the client area, moved there or clicked, or a wait for something that takes its time, such as a fake switch.
enum Step {
    Key(Keystroke),
    Move(Point<Pixels>),
    Click(Point<Pixels>),
    Wait(Duration),
}

impl Step {
    fn parse(text: &str) -> anyhow::Result<Self> {
        let (pointer, spot) = match text.split_once(':') {
            Some(("wait", milliseconds)) => {
                return milliseconds
                    .parse::<u64>()
                    .ok()
                    .filter(|milliseconds| *milliseconds <= LONGEST_WAIT)
                    .map(|milliseconds| Self::Wait(Duration::from_millis(milliseconds)))
                    .with_context(|| {
                        format!("{text} is not wait:<milliseconds up to {LONGEST_WAIT}>")
                    });
            }
            Some(pointer @ ("move" | "click", _)) => pointer,
            _ => {
                return Keystroke::parse(text.to_owned().leak())
                    .map(Self::Key)
                    .with_context(|| format!("{text} is not a keystroke"));
            }
        };
        let position = spot
            .split_once('x')
            .and_then(|(x, y)| Some(point(px(x.parse().ok()?), px(y.parse().ok()?))))
            .with_context(|| format!("{text} is not {pointer}:<x>x<y>"))?;
        Ok(if pointer == "click" {
            Self::Click(position)
        } else {
            Self::Move(position)
        })
    }

    fn take(self, window: &mut Window, cx: &mut App) {
        let (position, click) = match self {
            Self::Key(keystroke) => {
                window.dispatch_keystroke(keystroke, cx);
                return;
            }
            Self::Move(position) => (position, false),
            Self::Click(position) => (position, true),
            Self::Wait(_) => return,
        };
        let modifiers = Modifiers::default();
        window.dispatch_event(
            PlatformInput::MouseMove(MouseMoveEvent {
                position,
                pressed_button: None,
                modifiers,
            }),
            cx,
        );
        if !click {
            return;
        }
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position,
                modifiers,
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position,
                modifiers,
                click_count: 1,
            }),
            cx,
        );
    }
}

pub async fn pause(duration: Duration, cx: &AsyncApp) {
    cx.background_executor().timer(duration).await;
}

fn in_library<R>(
    window: WindowHandle<Shell>,
    cx: &mut AsyncApp,
    update: impl FnOnce(&mut LibraryView, &mut gpui::Window, &mut gpui::Context<LibraryView>) -> R,
) -> anyhow::Result<R> {
    window.update(cx, |shell, window, cx| {
        shell
            .library()
            .update(cx, |library, cx| update(library, window, cx))
    })
}

fn megabytes(bytes: usize) -> f64 {
    bytes as f64 / (1024. * 1024.)
}
