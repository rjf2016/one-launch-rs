#![forbid(unsafe_code)]

mod text_field;
pub mod theme;

pub use text_field::TextField;

pub fn init(cx: &mut gpui::App) {
    gpui_kit::base::init(cx);
}
