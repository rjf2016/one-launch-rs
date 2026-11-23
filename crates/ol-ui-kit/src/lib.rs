#![forbid(unsafe_code)]

mod text_area;
mod text_field;
pub mod theme;

pub use text_area::TextArea;
pub use text_field::{FieldEvent, TextField};

pub fn init(cx: &mut gpui::App) {
    gpui_kit::base::init(cx);
}
