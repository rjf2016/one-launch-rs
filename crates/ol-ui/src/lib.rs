#![forbid(unsafe_code)]

mod account;
mod accounts;
mod assets;
mod follow;
mod host;
mod library;
mod page;
mod posters;
mod settings;
mod shell;
mod widgets;

use gpui::prelude::*;
use gpui::{
    App, SharedString, TitlebarOptions, WindowBounds, WindowHandle, WindowOptions, px, size,
};
use ol_api::Backend;
use ol_api::library::HourClock;

pub use assets::Assets;
pub use library::{LibraryFrame, LibraryLoaded, LibraryView, SweepReport};
pub use shell::{Played, QuitAnyway, Shell};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Foreground,
    // Shown without taking the keyboard from the owner, and drawn at the full frame rate all the same.
    Background,
}

#[derive(Clone, Copy)]
pub struct LocalTime {
    pub offset: fn(i64) -> Option<i32>,
    pub clock: HourClock,
}

impl gpui::Global for LocalTime {}

pub fn init(cx: &mut App, local_time: LocalTime) {
    ol_ui_kit::init(cx);
    cx.set_global(local_time);
}

pub(crate) fn local_time(cx: &App) -> LocalTime {
    cx.try_global::<LocalTime>().copied().unwrap_or(LocalTime {
        offset: |_| None,
        clock: HourClock::Twelve,
    })
}

// `fake_launchers` says so in the title bar, so that no picture of a fake run passes for a real one.
pub fn open_main_window(
    title: impl Into<SharedString>,
    presence: Presence,
    fake_launchers: bool,
    backend: Backend,
    cx: &mut App,
) -> gpui::Result<WindowHandle<Shell>> {
    let defaults = WindowOptions::default();
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(1280.), px(800.)), cx)),
        titlebar: Some(TitlebarOptions {
            title: Some(title.into()),
            appears_transparent: true,
            traffic_light_position: None,
        }),
        window_min_size: Some(size(px(960.), px(600.))),
        focus: presence == Presence::Foreground,
        inactive_frame_interval: defaults
            .inactive_frame_interval
            .filter(|_| presence == Presence::Foreground),
        ..defaults
    };
    cx.open_window(options, |window, cx| {
        cx.new(|cx| Shell::new(backend, fake_launchers, window, cx))
    })
}
