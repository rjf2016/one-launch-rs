use gpui::prelude::*;
use gpui::{Context, Window};

use crate::widgets::{narrow_page, page_title};

pub struct SettingsView;

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        narrow_page(page_title("Settings", None))
    }
}
