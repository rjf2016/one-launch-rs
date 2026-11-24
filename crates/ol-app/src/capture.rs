use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use gpui::{
    AnyWindowHandle, App, AsyncApp, Keystroke, Modifiers, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, NavigationDirection, Pixels, PlatformInput, Point, ScrollDelta,
    ScrollEvent, ScrollSource, TouchPhase, Window, WindowHandle, point, px,
};
use ol_platform_win::own_process;
use ol_platform_win::probe::{self, Area, OuterRect};
use ol_platform_win::window::AppWindow;
use ol_ui::{LibraryFrame, LibraryView, Shell};

use crate::Life;
use crate::args::CapturePlan;

const SETTLE: Duration = Duration::from_millis(1200);
const STEP: Duration = Duration::from_millis(400);
const FRAME: Duration = Duration::from_millis(16);
const SWEEP_STEP: f32 = 30.;
const LONGEST_WAIT: u64 = 60_000;
const MOST_SIDE: i32 = 8000;
const MOST_STEPS: usize = 2000;

pub async fn run(plan: &CapturePlan, life: &Rc<Life>, cx: &mut AsyncApp) -> anyhow::Result<()> {
    let window = life.window;
    let native = AppWindow::on_this_thread().context("the GPUI window was not found")?;
    pause(SETTLE, cx).await;

    if let Some(size) = plan.size {
        set_client_size(native, size)?;
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

    let mut logged = None;
    let mut kept = Vec::new();
    for step in &plan.keys {
        let step = match Step::parse(step)? {
            Step::Wait(time) => {
                pause(time, cx).await;
                continue;
            }
            Step::Size(size) => {
                set_client_size(native, size)?;
                pause(STEP, cx).await;
                continue;
            }
            Step::Resize { from, to, steps } => {
                let outer = logged.is_some().then_some(&mut kept);
                resize(native, window, (from, to, steps), outer, cx).await?;
                pause(STEP, cx).await;
                continue;
            }
            Step::Drag(from, to) => {
                drag(window, from, to, cx).await?;
                pause(STEP, cx).await;
                continue;
            }
            Step::Type(text) => {
                native.post_text(&text);
                pause(STEP, cx).await;
                continue;
            }
            pressed @ (Step::Close | Step::Minimize) => {
                let (width, _) = native.client_size().unwrap_or_default();
                let scale = native.scale();
                let at = |logical: f32| (logical * scale).round() as i32;
                let (area, from_right) = if matches!(pressed, Step::Close) {
                    (Area::Close, 23.)
                } else {
                    (Area::Minimize, 115.)
                };
                probe::click(native, area, width - at(from_right), at(20.));
                pause(STEP, cx).await;
                continue;
            }
            Step::TrayOpen => {
                crate::show_window();
                pause(STEP, cx).await;
                continue;
            }
            Step::TrayQuit => {
                crate::request_quit(life, cx);
                pause(STEP, cx).await;
                continue;
            }
            Step::LogFrames => {
                in_library(window, cx, |library, _, _| library.log_frames())?;
                kept.clear();
                logged = Some(Instant::now());
                continue;
            }
            Step::PrintFrames => {
                let mut frames = std::mem::take(&mut kept);
                frames.extend(in_library(window, cx, |library, _, _| {
                    library.take_frames()
                })?);
                let started = logged
                    .take()
                    .context("frames:print needs frames:log before it")?;
                for (index, frame) in frames.iter().enumerate() {
                    println!(
                        "OneLaunch: frame {index:>3} at {:>7.1} ms: drawn for {}x{}, pane {:.0} px, {} columns of {:.2} px",
                        frame.at.duration_since(started).as_secs_f32() * 1000.,
                        frame.viewport.width.as_f32(),
                        frame.viewport.height.as_f32(),
                        frame.pane,
                        frame.columns,
                        frame.card,
                    );
                }
                continue;
            }
            step => step,
        };
        let at = Instant::now();
        dispatch(window, cx, move |window, cx| step.take(window, cx))?;
        if let Some(started) = logged {
            println!(
                "OneLaunch: step at {:.1} ms",
                at.duration_since(started).as_secs_f32() * 1000.
            );
        }
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

// A step of --capture-keys, or several joined by & and taken in one go: a key press; the window's close or minimise button pressed through Windows (`close`, `minimize`), the tray's Open (`tray-open`) or Quit (`tray-quit`); text typed into whatever has the keyboard, with %2C for a comma; the pointer at a place of the client area, moved there, clicked with the left or the right button, clicked as the second click of a double click, dragged from there, turning the wheel there, with Shift held as Windows reports it, or the mouse's Back button pressed and let go there; a wait for something that takes its time, such as a fake switch; the client area set to a size, at once or in steps as an edge dragged by hand does; or the Library's frames logged from one step and printed at another.
enum Step {
    Key(Keystroke),
    Type(String),
    Move(Point<Pixels>),
    Click(Point<Pixels>),
    RightClick(Point<Pixels>),
    SecondClick(Point<Pixels>),
    Back(Point<Pixels>),
    Together(Vec<Step>),
    Wheel(Point<Pixels>, Point<f32>, Modifiers),
    Drag(Point<Pixels>, Point<Pixels>),
    Wait(Duration),
    Close,
    Minimize,
    TrayOpen,
    TrayQuit,
    Size((i32, i32)),
    Resize {
        from: (i32, i32),
        to: (i32, i32),
        steps: usize,
    },
    LogFrames,
    PrintFrames,
}

impl Step {
    fn parse(text: &str) -> anyhow::Result<Self> {
        if let Some(typed) = text.strip_prefix("type:").filter(|typed| !typed.is_empty()) {
            return Ok(Self::Type(typed.replace("%2C", ",")));
        }
        if text.contains('&') {
            let steps = text
                .split('&')
                .map(Self::parse)
                .collect::<anyhow::Result<Vec<_>>>()?;
            if steps.iter().any(|step| !step.at_once()) {
                anyhow::bail!(
                    "{text}: only keys, clicks, moves, the wheel and Back go together with &"
                );
            }
            return Ok(Self::Together(steps));
        }
        let Some((kind, rest)) = text.split_once(':') else {
            return match text {
                "close" => Ok(Self::Close),
                "minimize" => Ok(Self::Minimize),
                "tray-open" => Ok(Self::TrayOpen),
                "tray-quit" => Ok(Self::TrayQuit),
                _ => Self::key(text),
            };
        };
        let spot = |spot: &str| {
            spot.split_once('x')
                .and_then(|(x, y)| Some(point(px(x.parse().ok()?), px(y.parse().ok()?))))
        };
        let size = |size: &str| {
            size.split_once('x')
                .and_then(|(width, height)| Some((width.parse().ok()?, height.parse().ok()?)))
                .filter(|(width, height): &(i32, i32)| {
                    (1..=MOST_SIDE).contains(width) && (1..=MOST_SIDE).contains(height)
                })
        };
        let step = match kind {
            "wait" => rest
                .parse::<u64>()
                .ok()
                .filter(|milliseconds| *milliseconds <= LONGEST_WAIT)
                .map(|milliseconds| Self::Wait(Duration::from_millis(milliseconds))),
            "type" => (!rest.is_empty()).then(|| Self::Type(rest.replace("%2C", ","))),
            "move" => spot(rest).map(Self::Move),
            "click" => spot(rest).map(Self::Click),
            "right-click" => spot(rest).map(Self::RightClick),
            "second-click" => spot(rest).map(Self::SecondClick),
            "back" => spot(rest).map(Self::Back),
            "wheel" | "shift-wheel" => rest.split_once(':').and_then(|(at, lines)| {
                let lines = lines
                    .parse::<f32>()
                    .ok()
                    .filter(|lines| lines.abs() <= 100.)?;
                let shift = kind == "shift-wheel";
                let delta = if shift {
                    point(lines, 0.)
                } else {
                    point(0., lines)
                };
                let modifiers = Modifiers {
                    shift,
                    ..Modifiers::default()
                };
                Some(Self::Wheel(spot(at)?, delta, modifiers))
            }),
            "drag" => rest
                .split_once(':')
                .and_then(|(from, to)| Some(Self::Drag(spot(from)?, spot(to)?))),
            "size" => size(rest).map(Self::Size),
            "frames" => match rest {
                "log" => Some(Self::LogFrames),
                "print" => Some(Self::PrintFrames),
                _ => None,
            },
            "resize" => {
                let mut parts = rest.split(':');
                match (parts.next(), parts.next(), parts.next(), parts.next()) {
                    (Some(from), Some(to), Some(steps), None) => (|| {
                        let steps = steps
                            .parse::<usize>()
                            .ok()
                            .filter(|steps| (1..=MOST_STEPS).contains(steps))?;
                        Some(Self::Resize {
                            from: size(from)?,
                            to: size(to)?,
                            steps,
                        })
                    })(),
                    _ => None,
                }
            }
            _ => return Self::key(text),
        };
        step.with_context(|| {
            let form = match kind {
                "wait" => "wait:<milliseconds up to 60000>",
                "type" => "type:<text>",
                "move" | "click" | "right-click" | "second-click" | "back" => {
                    "move:<x>x<y>, click:<x>x<y>, right-click:<x>x<y>, second-click:<x>x<y> or back:<x>x<y>"
                }
                "wheel" | "shift-wheel" => "wheel:<x>x<y>:<lines, minus is down>",
                "drag" => "drag:<x>x<y>:<x>x<y>",
                "size" => "size:<width>x<height>",
                "frames" => "frames:log or frames:print",
                _ => "resize:<width>x<height>:<width>x<height>:<steps up to 2000>",
            };
            format!("{text} is not {form}")
        })
    }

    fn key(text: &str) -> anyhow::Result<Self> {
        Keystroke::parse(text.to_owned().leak())
            .map(Self::Key)
            .with_context(|| format!("{text} is not a keystroke"))
    }

    fn at_once(&self) -> bool {
        matches!(
            self,
            Self::Key(_)
                | Self::Move(_)
                | Self::Click(_)
                | Self::RightClick(_)
                | Self::SecondClick(_)
                | Self::Back(_)
                | Self::Wheel(..)
        )
    }

    fn take(self, window: &mut Window, cx: &mut App) {
        let clicks = if matches!(self, Self::SecondClick(_)) {
            2
        } else {
            1
        };
        let (position, button) = match self {
            Self::Together(steps) => {
                for step in steps {
                    step.take(window, cx);
                }
                return;
            }
            Self::Key(keystroke) => {
                window.dispatch_keystroke(keystroke, cx);
                return;
            }
            Self::Move(position) => (position, None),
            Self::Click(position) => (position, Some(MouseButton::Left)),
            Self::RightClick(position) => (position, Some(MouseButton::Right)),
            Self::SecondClick(position) => (position, Some(MouseButton::Left)),
            Self::Back(position) => (
                position,
                Some(MouseButton::Navigate(NavigationDirection::Back)),
            ),
            Self::Wheel(position, lines, modifiers) => {
                window.dispatch_event(
                    PlatformInput::MouseMove(MouseMoveEvent {
                        position,
                        pressed_button: None,
                        modifiers: Modifiers::default(),
                    }),
                    cx,
                );
                window.dispatch_event(
                    PlatformInput::Scroll(ScrollEvent {
                        position,
                        delta: ScrollDelta::Lines(lines),
                        modifiers,
                        touch_phase: TouchPhase::Moved,
                        source: ScrollSource::Pointer,
                    }),
                    cx,
                );
                return;
            }
            Self::Drag(..)
            | Self::Type(_)
            | Self::Wait(_)
            | Self::Close
            | Self::Minimize
            | Self::TrayOpen
            | Self::TrayQuit
            | Self::Size(_)
            | Self::Resize { .. }
            | Self::LogFrames
            | Self::PrintFrames => return,
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
        let Some(button) = button else {
            return;
        };
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button,
                position,
                modifiers,
                click_count: clicks,
                first_mouse: false,
            }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                button,
                position,
                modifiers,
                click_count: clicks,
            }),
            cx,
        );
    }
}

pub async fn pause(duration: Duration, cx: &AsyncApp) {
    cx.background_executor().timer(duration).await;
}

// Through the untyped handle: a key press can redraw the window, and the typed one holds the root view meanwhile.
fn dispatch(
    window: WindowHandle<Shell>,
    cx: &mut AsyncApp,
    update: impl FnOnce(&mut Window, &mut App),
) -> anyhow::Result<()> {
    AnyWindowHandle::from(window).update(cx, |_, window, cx| update(window, cx))
}

fn set_client_size(native: AppWindow, (width, height): (i32, i32)) -> anyhow::Result<()> {
    let client = native
        .client_size()
        .context("the window has no client area")?;
    let outer = probe::outer_rect(native).context("the window has no frame")?;
    probe::set_outer_rect(
        native,
        OuterRect {
            width: width + outer.width - client.0,
            height: height + outer.height - client.1,
            ..outer
        },
    );
    Ok(())
}

async fn drag(
    window: WindowHandle<Shell>,
    from: Point<Pixels>,
    to: Point<Pixels>,
    cx: &mut AsyncApp,
) -> anyhow::Result<()> {
    const MOVES: usize = 12;
    let modifiers = Modifiers::default();
    dispatch(window, cx, move |window, cx| {
        window.dispatch_event(
            PlatformInput::MouseMove(MouseMoveEvent {
                position: from,
                pressed_button: None,
                modifiers,
            }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position: from,
                modifiers,
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
    })?;
    for step in 1..=MOVES {
        pause(FRAME, cx).await;
        let share = step as f32 / MOVES as f32;
        let position = from + (to - from) * share;
        dispatch(window, cx, move |window, cx| {
            window.dispatch_event(
                PlatformInput::MouseMove(MouseMoveEvent {
                    position,
                    pressed_button: Some(MouseButton::Left),
                    modifiers,
                }),
                cx,
            );
        })?;
    }
    pause(FRAME, cx).await;
    dispatch(window, cx, move |window, cx| {
        window.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position: to,
                modifiers,
                click_count: 1,
            }),
            cx,
        );
    })
}

// Sets the client area from one size to the other a step a frame, as a window edge dragged by hand, and prints every frame the Library drew meanwhile: when, for which size, the size last set at that time, and the grid it laid out; a frames:log running around it keeps every frame, those of the resize included.
async fn resize(
    native: AppWindow,
    window: WindowHandle<Shell>,
    (from, to, steps): ((i32, i32), (i32, i32), usize),
    mut outer: Option<&mut Vec<LibraryFrame>>,
    cx: &mut AsyncApp,
) -> anyhow::Result<()> {
    set_client_size(native, from)?;
    pause(STEP, cx).await;
    let before = in_library(window, cx, |library, _, _| {
        let before = library.take_frames();
        library.log_frames();
        before
    })?;
    if let Some(outer) = outer.as_deref_mut() {
        outer.extend(before);
    }
    let started = Instant::now();
    let mut set = Vec::with_capacity(steps + 1);
    for step in 0..=steps {
        let share = step as f32 / steps as f32;
        let side = |from: i32, to: i32| from + ((to - from) as f32 * share).round() as i32;
        let size = (side(from.0, to.0), side(from.1, to.1));
        set_client_size(native, size)?;
        set.push((Instant::now(), size));
        pause(FRAME, cx).await;
    }
    pause(STEP, cx).await;
    let keep = outer.is_some();
    let frames = in_library(window, cx, move |library, _, _| {
        let frames = library.take_frames();
        if keep {
            library.log_frames();
        }
        frames
    })?;
    if let Some(outer) = outer {
        outer.extend_from_slice(&frames);
    }
    let scale = native.scale();
    let mut stale = 0;
    let mut layouts: Vec<((i32, i32), LibraryFrame)> = Vec::new();
    let mut differing = 0;
    for (index, frame) in frames.iter().enumerate() {
        let drawn = (
            (frame.viewport.width.as_f32() * scale).round() as i32,
            (frame.viewport.height.as_f32() * scale).round() as i32,
        );
        let last_set = set
            .iter()
            .take_while(|(at, _)| *at <= frame.at)
            .last()
            .map(|(_, size)| *size);
        let behind = last_set.is_some_and(|size| size != drawn);
        stale += usize::from(behind);
        match layouts.iter().find(|(size, _)| *size == drawn) {
            Some((_, earlier)) if !earlier.same_grid(frame) => differing += 1,
            Some(_) => {}
            None => layouts.push((drawn, *frame)),
        }
        println!(
            "OneLaunch: frame {index:>3} at {:>7.1} ms: drawn for {}x{}, last set {}, pane {:.0} px, {} columns of {:.2} px, gap {}{}",
            frame.at.duration_since(started).as_secs_f32() * 1000.,
            drawn.0,
            drawn.1,
            last_set.map_or("none".to_owned(), |(width, height)| format!(
                "{width}x{height}"
            )),
            frame.pane,
            frame.columns,
            frame.card,
            frame.gap,
            if behind { " BEHIND" } else { "" },
        );
    }
    println!(
        "OneLaunch: resize from {}x{} to {}x{} in {steps} steps: {} frames drawn, {stale} drawn for a size already replaced, {differing} laid out differently from an earlier frame of the same size",
        from.0,
        from.1,
        to.0,
        to.1,
        frames.len(),
    );
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pointer_and_size_steps_are_read_as_written() {
        let Ok(Step::Wheel(at, lines, held)) = Step::parse("wheel:700x450:-3") else {
            panic!("a wheel step");
        };
        assert_eq!(
            (at, lines, held.shift),
            (point(px(700.), px(450.)), point(0., -3.), false)
        );
        let Ok(Step::Wheel(_, lines, held)) = Step::parse("shift-wheel:10x20:2") else {
            panic!("a wheel step with Shift");
        };
        assert_eq!((lines, held.shift), (point(2., 0.), true));
        assert!(matches!(Step::parse("drag:1x2:3x4"), Ok(Step::Drag(..))));
        assert!(matches!(Step::parse("back:5x6"), Ok(Step::Back(_))));
        assert!(matches!(
            Step::parse("right-click:5x6"),
            Ok(Step::RightClick(_))
        ));
        assert!(matches!(
            Step::parse("second-click:5x6"),
            Ok(Step::SecondClick(_))
        ));
        assert!(matches!(
            Step::parse("click:1x2&escape"),
            Ok(Step::Together(steps)) if steps.len() == 2
        ));
        let Ok(Step::Type(text)) = Step::parse("type:gold%2C smurf") else {
            panic!("a step that types");
        };
        assert_eq!(text, "gold, smurf");
        let Ok(Step::Type(text)) = Step::parse("type:R&D") else {
            panic!("a step that types an ampersand");
        };
        assert_eq!(text, "R&D");
        assert!(matches!(Step::parse("frames:log"), Ok(Step::LogFrames)));
        assert!(matches!(Step::parse("close"), Ok(Step::Close)));
        assert!(matches!(Step::parse("minimize"), Ok(Step::Minimize)));
        assert!(matches!(Step::parse("tray-open"), Ok(Step::TrayOpen)));
        assert!(matches!(Step::parse("tray-quit"), Ok(Step::TrayQuit)));
        assert!(matches!(Step::parse("frames:print"), Ok(Step::PrintFrames)));
        assert!(matches!(
            Step::parse("size:1000x600"),
            Ok(Step::Size((1000, 600)))
        ));
        assert!(matches!(
            Step::parse("resize:1600x1150:1000x1150:200"),
            Ok(Step::Resize {
                from: (1600, 1150),
                to: (1000, 1150),
                steps: 200
            })
        ));
        for refused in [
            "wheel:700x450",
            "wheel:700x450:1000",
            "drag:1x2",
            "size:0x600",
            "size:1000",
            "resize:1600x1150:1000x1150",
            "resize:1600x1150:1000x1150:0",
            "resize:1600x1150:1000x1150:5000",
            "wait:61000",
            "type:",
            "click:1x2&wait:5",
            "back:5",
            "second-click:5",
            "frames:all",
        ] {
            assert!(Step::parse(refused).is_err(), "{refused}");
        }
    }
}
