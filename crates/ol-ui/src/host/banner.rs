use gpui::prelude::*;
use gpui::{Context, Div, div, px};
use ol_api::launch::banner_text;
use ol_model::JobState;
use ol_ui_kit::theme::{self, alpha, color};

use super::LaunchHost;
use crate::assets;
use crate::widgets::{Button, Tone, icon, spinner};

impl LaunchHost {
    // A strip under the title bar for the job the app follows, while no launch dialog is open.
    pub(super) fn render_banner(&self, cx: &mut Context<Self>) -> Option<Div> {
        let job = &self.active.as_ref().filter(|_| self.request.is_none())?.job;
        let running = job.state == JobState::Running;
        let (hex, line, fill) = match job.state {
            JobState::Running => (theme::TEXT, theme::ACCENT, 0.16),
            JobState::Done => (theme::SUCCESS, theme::SUCCESS, 0.1),
            _ => (theme::WARNING, theme::WARNING, 0.1),
        };
        let mark = match job.state {
            JobState::Running if job.attention.is_some() => {
                icon(assets::KEY, 14., hex).into_any_element()
            }
            JobState::Running => spinner("banner-mark", 14., hex),
            JobState::Done => icon(assets::CHECK, 14., hex).into_any_element(),
            _ => icon(assets::FAILED, 14., hex).into_any_element(),
        };
        let banner = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(12.))
            .border_b_1()
            .border_color(alpha(line, 0.3))
            .bg(alpha(line, fill))
            .px(px(16.))
            .py(px(6.))
            .text_size(px(12.))
            .line_height(px(16.))
            .text_color(color(hex))
            .child(mark)
            .child(div().flex_1().min_w_0().truncate().child(banner_text(job)))
            .child(
                Button::new("banner-show", Tone::Ghost, "Show")
                    .on_click(cx.listener(|this, _, _, cx| this.show_job(cx))),
            );
        Some(banner.when(!running, |banner| {
            banner.child(
                div()
                    .id("banner-dismiss")
                    .flex_none()
                    .p(px(4.))
                    .rounded(px(4.))
                    .opacity(0.7)
                    .cursor_pointer()
                    .hover(|style| style.opacity(1.))
                    .on_click(cx.listener(|this, _, _, cx| this.forget_job(cx)))
                    .child(icon(assets::CLEAR, 14., hex)),
            )
        }))
    }
}
