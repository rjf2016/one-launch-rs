use gpui::prelude::*;
use gpui::{Context, Window};

use crate::widgets::{narrow_page, page_title};

pub struct AccountsView;

impl Render for AccountsView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        narrow_page(page_title(
            "Accounts",
            Some("The accounts you switch between, grouped by launcher."),
        ))
    }
}
