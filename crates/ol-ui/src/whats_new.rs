use std::io::Cursor;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    BoxShadow, ClickEvent, Context, Div, EventEmitter, FontWeight, HighlightStyle, Keystroke,
    ObjectFit, RenderImage, ScrollEvent, Stateful, StyledText, Window, div, img, px,
};
use image::{Frame, ImageReader};
use ol_api::Backend;
use ol_api::whats_new::{Release, Tone, release_date};
use ol_model::SettingKey;
use ol_ui_kit::theme::{self, alpha, color};

use crate::assets;
use crate::page::{PageScroll, scrollbar};
use crate::widgets::{icon, panel_shadow, tip};

const CARD_WIDTH: f32 = 640.;
const LINE: f32 = 40.;
const PAGE_STEP: f32 = 0.875;

const PICTURES: &[(&str, &[u8])] = &[(
    "beta-3.png",
    include_bytes!("../../../oracle/assets/whats-new/beta-3.png"),
)];

pub struct WhatsNewDone;

pub struct WhatsNewCard {
    backend: Backend,
    release: &'static Release,
    version: String,
    list: PageScroll,
    picture: Option<(Arc<RenderImage>, f32)>,
    undecoded: bool,
    closing: bool,
}

impl EventEmitter<WhatsNewDone> for WhatsNewCard {}

fn decode(bytes: &'static [u8]) -> Option<(RenderImage, f32)> {
    let picture = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    let ratio = picture.width() as f32 / picture.height().max(1) as f32;
    let mut pixels = picture.into_rgba8();
    // GPUI's render images are BGRA.
    for pixel in pixels.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Some((RenderImage::new(vec![Frame::new(pixels)]), ratio))
}

impl WhatsNewCard {
    pub fn new(
        backend: Backend,
        release: &'static Release,
        version: String,
        cx: &mut Context<Self>,
    ) -> Self {
        let bytes = release.image.as_ref().and_then(|image| {
            PICTURES
                .iter()
                .find(|(file, _)| *file == image.file)
                .map(|(_, bytes)| *bytes)
        });
        if let Some(bytes) = bytes {
            let decoding = cx.background_executor().spawn(async move { decode(bytes) });
            cx.spawn(async move |this, cx| {
                let decoded = decoding.await;
                this.update(cx, |this, cx| {
                    match decoded {
                        Some((picture, ratio)) => this.picture = Some((Arc::new(picture), ratio)),
                        None => this.undecoded = true,
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        Self {
            backend,
            release,
            version,
            list: PageScroll::default(),
            picture: None,
            undecoded: false,
            closing: false,
        }
    }

    pub fn take_picture(&mut self) -> Option<Arc<RenderImage>> {
        self.picture.take().map(|(picture, _)| picture)
    }

    // A write that fails still closes the card: it then comes back at the next start.
    fn close(&mut self, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        self.closing = true;
        let (backend, version) = (self.backend.clone(), self.version.clone());
        cx.spawn(async move |this, cx| {
            let _ = backend
                .set_setting(SettingKey::WhatsNewLastSeen, version)
                .await;
            this.update(cx, |_, cx| cx.emit(WhatsNewDone)).ok();
        })
        .detach();
    }

    pub fn on_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let held = keystroke.modifiers;
        if held.alt || held.platform {
            return false;
        }
        if held.control || held.function {
            return true;
        }
        let page = f32::from(self.list.handle().bounds().size.height) * PAGE_STEP;
        let step = match (keystroke.key.as_str(), held.shift) {
            ("escape", false) => {
                self.close(cx);
                return true;
            }
            ("down", false) => LINE,
            ("up", false) => -LINE,
            ("pagedown", false) | ("space", false) => page,
            ("pageup", false) | ("space", true) => -page,
            ("home", false) => f32::MIN,
            ("end", false) => f32::MAX,
            _ => return true,
        };
        self.list.scroll_by(step);
        cx.notify();
        true
    }

    fn render_list(&self) -> Div {
        let tone = |tone: Option<Tone>| match tone.unwrap_or(Tone::Accent) {
            Tone::Accent => theme::ACCENT,
            Tone::Success => theme::SUCCESS,
            Tone::Warning => theme::WARNING,
            Tone::Danger => theme::DANGER,
            Tone::Steam => theme::STEAM,
            Tone::Epic => theme::EPIC,
            Tone::Battlenet => theme::BATTLENET,
            Tone::Riot => theme::RIOT,
        };
        let picture = match &self.picture {
            Some((picture, ratio)) => Some(
                div().mb(px(24.)).w_full().aspect_ratio(*ratio).child(
                    img(picture.clone())
                        .size_full()
                        .object_fit(ObjectFit::Contain)
                        .rounded(px(8.)),
                ),
            ),
            // As a browser shows an image's alt text in its place when the image cannot be read.
            None if self.undecoded => self
                .release
                .image
                .as_ref()
                .and_then(|image| image.alt)
                .map(|alt| div().mb(px(24.)).text_color(color(theme::MUTED)).child(alt)),
            None => None,
        };
        let intro = self.release.intro.map(|intro| {
            div()
                .mb(px(24.))
                .text_color(color(theme::TEXT))
                .child(intro)
        });
        let sections = self.release.sections.iter().map(|section| {
            let hex = tone(section.tone);
            div()
                .child(
                    div()
                        .mb(px(16.))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(color(hex))
                        .child(section.heading.to_uppercase())
                        .child(div().flex_1().h(px(1.)).bg(alpha(hex, 0.4))),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(14.))
                        .children(section.items.iter().map(|item| {
                            div()
                                .flex()
                                .gap(px(12.))
                                .line_height(px(22.75))
                                .child(
                                    div()
                                        .mt(px(8.))
                                        .size(px(6.))
                                        .flex_none()
                                        .rounded_full()
                                        .bg(color(theme::ACCENT))
                                        .shadow(vec![
                                            BoxShadow::new(
                                                px(0.),
                                                px(0.),
                                                color(theme::ACCENT).into(),
                                            )
                                            .blur_radius(px(8.)),
                                        ]),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_color(color(theme::MUTED))
                                        .child(
                                            StyledText::new(match item.detail {
                                                Some(detail) => format!("{} {detail}", item.lead),
                                                None => item.lead.to_owned(),
                                            })
                                            .with_highlights([(
                                                0..item.lead.len(),
                                                HighlightStyle {
                                                    color: Some(color(theme::TEXT_STRONG).into()),
                                                    font_weight: Some(FontWeight::SEMIBOLD),
                                                    ..Default::default()
                                                },
                                            )]),
                                        ),
                                )
                        })),
                )
        });
        div()
            .children(picture)
            .children(intro)
            .child(div().flex().flex_col().gap(px(32.)).children(sections))
    }

    pub fn render_card(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let header = div()
            .flex()
            .flex_none()
            .items_start()
            .justify_between()
            .gap(px(16.))
            .border_b_1()
            .border_color(alpha(theme::BORDER, 0.6))
            .px(px(32.))
            .pt(px(24.))
            .pb(px(16.))
            .child(
                div()
                    .child(
                        div()
                            .text_size(px(24.))
                            .line_height(px(32.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(color(theme::TEXT_STRONG))
                            .child("What's New"),
                    )
                    .child(
                        div()
                            .mt(px(2.))
                            .text_color(color(theme::MUTED))
                            .child(release_date(self.release.date)),
                    ),
            )
            .child(
                div()
                    .id("whats-new-close")
                    .group("whats-new-close")
                    .mt(px(-4.))
                    .mr(px(-8.))
                    .p(px(6.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|style| style.bg(color(theme::SURFACE_2)))
                    .tooltip(tip("Close (Esc)"))
                    .on_click(cx.listener(|this, _, _, cx| this.close(cx)))
                    .child(
                        icon(assets::CLEAR, 16., theme::MUTED)
                            .group_hover("whats-new-close", |style| {
                                style.text_color(color(theme::TEXT))
                            }),
                    ),
            );
        let list = div()
            .relative()
            .flex()
            .flex_col()
            .flex_initial()
            .min_h_0()
            .child(
                div()
                    .id("whats-new-list")
                    .flex_initial()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(self.list.handle())
                    .pl(px(32.))
                    .pr(px(32.))
                    .py(px(20.))
                    .child(self.render_list()),
            )
            .child(scrollbar(&self.list, true));
        let card = div()
            .id("whats-new")
            .occlude()
            .flex()
            .flex_col()
            .w_full()
            .max_w(px(CARD_WIDTH))
            .max_h_full()
            .overflow_hidden()
            .rounded(px(12.))
            .border_1()
            .border_color(color(theme::BORDER))
            .bg(color(theme::SURFACE))
            .shadow(panel_shadow())
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(header)
            .child(list);
        let wheeled = self.list.clone();
        div()
            .id("whats-new-backdrop")
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .p(px(24.))
            .bg(alpha(theme::BG, 0.9))
            // The card is all there is while it is up, as in the Electron app.
            .on_scroll(move |event: &ScrollEvent, window: &mut Window, _| {
                if wheeled.handle().bounds().contains(&event.position) {
                    return;
                }
                let delta = event.delta.pixel_delta(window.line_height());
                wheeled.scroll_by(-f32::from(delta.y));
                window.refresh();
            })
            .on_click(cx.listener(|this, click: &ClickEvent, _, cx| {
                if click.click_count() == 1 {
                    this.close(cx);
                }
            }))
            .child(card)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_picture_a_release_names_ships_and_reads() {
        for release in ol_api::whats_new::RELEASES {
            if let Some(image) = &release.image {
                let bytes = PICTURES
                    .iter()
                    .find(|(file, _)| *file == image.file)
                    .map(|(_, bytes)| *bytes)
                    .unwrap_or_else(|| panic!("{} is not shipped", image.file));
                assert!(decode(bytes).is_some(), "{}", image.file);
            }
        }
    }
}
