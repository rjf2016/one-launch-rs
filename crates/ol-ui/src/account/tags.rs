use gpui::prelude::*;
use gpui::{Context, Div, Stateful, Window, div, px};
use ol_ui_kit::TextField;
use ol_ui_kit::theme::{self, alpha, color};

use super::AccountPage;
use crate::assets;
use crate::widgets::icon;

pub const MOST: usize = 32;
pub const LONGEST: usize = 40;

pub fn normalize(tags: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for raw in tags {
        let mut length = 0;
        let tag: String = raw
            .trim()
            .chars()
            .take_while(|letter| {
                length += letter.len_utf16();
                length <= LONGEST
            })
            .collect();
        if tag.is_empty() || !seen.insert(tag.to_lowercase()) {
            continue;
        }
        out.push(tag);
        if out.len() >= MOST {
            break;
        }
    }
    out
}

pub fn with_pending(tags: &[String], pending: &str) -> Vec<String> {
    normalize(
        tags.iter()
            .cloned()
            .chain(pending.split(',').map(str::to_owned)),
    )
}

impl AccountPage {
    pub(super) fn commit_tag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pending = self.tag_field.text(cx);
        if !super::blank(&pending) {
            self.tags = with_pending(&self.tags, &pending);
        }
        if !pending.is_empty() {
            self.tag_field.set_text("", window, cx);
        }
        self.tags_changed(window, cx);
    }

    pub(super) fn remove_tag(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index < self.tags.len() {
            self.tags.remove(index);
            self.tags_changed(window, cx);
        }
    }

    pub(super) fn tags_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let full = self.tags.len() >= MOST;
        let placeholder = if full {
            "Tag limit reached"
        } else if self.tags.is_empty() {
            "Add a tag"
        } else {
            ""
        };
        self.tag_field.set_placeholder(placeholder, window, cx);
        self.tag_field.set_disabled(self.busy || full, cx);
        cx.notify();
    }

    pub(super) fn render_tags(&self, window: &Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let field: &TextField = &self.tag_field;
        let focused = field.is_focused(window, cx);
        let disabled = self.busy;
        let chips = self.tags.iter().enumerate().map(|(index, tag)| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(4.))
                .rounded(px(6.))
                .bg(alpha(theme::ACCENT, 0.16))
                .px(px(8.))
                .py(px(2.))
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(color(theme::TEXT_STRONG))
                .child(tag.clone())
                .when(!disabled, |chip| {
                    chip.child(
                        div()
                            .id(("remove-tag", index))
                            .group("remove-tag")
                            .rounded(px(4.))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.remove_tag(index, window, cx);
                            }))
                            .child(
                                icon(assets::CLEAR, 12., theme::MUTED)
                                    .group_hover("remove-tag", |style| {
                                        style.text_color(color(theme::TEXT))
                                    }),
                            ),
                    )
                })
        });
        field
            .holder("tags", cx)
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .min_h(px(40.))
            .px(px(8.))
            .py(px(6.))
            .rounded(px(8.))
            .border_1()
            .border_color(color(if focused {
                theme::ACCENT
            } else {
                theme::BORDER
            }))
            .bg(color(theme::SURFACE_2))
            .when(disabled, |frame| frame.opacity(0.6))
            .children(chips)
            .child(
                div()
                    .flex_1()
                    .min_w(px(96.))
                    .px(px(4.))
                    .py(px(2.))
                    .line_height(px(20.))
                    .child(field.bare()),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owned(tags: &[&str]) -> Vec<String> {
        tags.iter().map(|tag| (*tag).to_owned()).collect()
    }

    #[test]
    fn a_tag_is_trimmed_cut_and_kept_once_as_the_electron_app_keeps_it() {
        assert_eq!(
            normalize(owned(&[" main ", "MAIN", "", "  ", "alt", &"x".repeat(50)])),
            owned(&["main", "alt", &"x".repeat(40)])
        );
        assert_eq!(
            normalize((0..40).map(|index| format!("tag-{index}"))).len(),
            32
        );
    }

    #[test]
    fn the_text_typed_is_split_at_its_commas() {
        assert_eq!(
            with_pending(&owned(&["eu"]), "gold, smurf,,EU "),
            owned(&["eu", "gold", "smurf"])
        );
        assert_eq!(with_pending(&owned(&["eu"]), "  "), owned(&["eu"]));
    }
}
