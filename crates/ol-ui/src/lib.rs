#![forbid(unsafe_code)]

mod assets;
mod library;
mod posters;
mod shell;

use gpui::prelude::*;
use gpui::{
    App, SharedString, TitlebarOptions, WindowBounds, WindowHandle, WindowOptions, px, size,
};
use ol_api::Backend;

pub use assets::Assets;
pub use library::{LibraryLoaded, LibraryView, SweepReport};
pub use shell::Shell;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Foreground,
    // Shown without taking the keyboard from the owner, and drawn at the full frame rate all the same.
    Background,
}

pub fn init(cx: &mut App) {
    ol_ui_kit::init(cx);
}

pub fn open_main_window(
    title: impl Into<SharedString>,
    presence: Presence,
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
        cx.new(|cx| Shell::new(backend, window, cx))
    })
}
