use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt as _, AnyElement, AnyView, App, ClickEvent, Context, Div, ElementId,
    FontWeight, SharedString, Stateful, Svg, Transformation, Window, div, percentage, px, svg,
};
use ol_model::LauncherId;
use ol_ui_kit::theme::{self, alpha, color};

use crate::assets;

const TURN: Duration = Duration::from_secs(1);

pub fn icon(path: &'static str, size: f32, hex: u32) -> Svg {
    svg()
        .path(path)
        .flex_none()
        .size(px(size))
        .text_color(color(hex))
}

fn turning(id: impl Into<ElementId>, glyph: Svg) -> AnyElement {
    glyph
        .with_animation(id, Animation::new(TURN).repeat(), |glyph, turned| {
            glyph.with_transformation(Transformation::rotate(percentage(turned)))
        })
        .into_any_element()
}

pub fn spinner(id: impl Into<ElementId>, size: f32, hex: u32) -> AnyElement {
    turning(id, icon(assets::SPINNER, size, hex))
}

pub fn page_title(title: &'static str, line: Option<&'static str>) -> Div {
    div()
        .flex()
        .flex_col()
        .min_w_0()
        .child(
            div()
                .text_size(px(20.))
                .line_height(px(28.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color(theme::TEXT_STRONG))
                .child(title),
        )
        .children(line.map(|line| {
            div()
                .mt(px(2.))
                .truncate()
                .text_color(color(theme::MUTED))
                .child(line)
        }))
}

pub fn narrow_page(header: Div) -> Div {
    div().size_full().px(px(32.)).child(
        div()
            .mx_auto()
            .w_full()
            .max_w(px(1040.))
            .pt(px(28.))
            .pb(px(20.))
            .child(header),
    )
}

struct Tip(SharedString);

impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // Below the pointer and not under it, where the Electron app's tooltips stand.
        div().pt(px(18.)).child(
            div()
                .px(px(8.))
                .py(px(4.))
                .rounded(px(6.))
                .border_1()
                .border_color(color(theme::BORDER_STRONG))
                .bg(color(theme::SURFACE_2))
                .font_family(theme::FONT)
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(color(theme::TEXT))
                .whitespace_nowrap()
                .child(self.0.clone()),
        )
    }
}

pub fn tip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text = text.into();
    move |_, cx| cx.new(|_| Tip(text.clone())).into()
}

pub fn launcher_hex(launcher: LauncherId) -> u32 {
    match launcher {
        LauncherId::Steam => theme::STEAM,
        LauncherId::Epic => theme::EPIC,
        LauncherId::Battlenet => theme::BATTLENET,
        LauncherId::Riot => theme::RIOT,
    }
}

pub fn launcher_dot(launcher: LauncherId) -> Div {
    div()
        .flex_none()
        .size(px(6.))
        .rounded_full()
        .bg(color(launcher_hex(launcher)))
}

pub fn launcher_badge(launcher: LauncherId) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .rounded(px(6.))
        .border_1()
        .border_color(color(theme::BORDER))
        .bg(color(theme::SURFACE_2))
        .px(px(8.))
        .py(px(2.))
        .text_size(px(12.))
        .line_height(px(16.))
        .font_weight(FontWeight::NORMAL)
        .text_color(color(theme::MUTED))
        .child(launcher_dot(launcher))
        .child(launcher.display_name())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

pub struct Button {
    id: ElementId,
    tone: Tone,
    label: SharedString,
    glyph: Option<&'static str>,
    turns: bool,
    disabled: bool,
    leads: bool,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, tone: Tone, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            tone,
            label: label.into(),
            glyph: None,
            turns: false,
            disabled: false,
            leads: false,
        }
    }

    pub fn glyph(mut self, glyph: &'static str) -> Self {
        self.glyph = Some(glyph);
        self
    }

    pub fn turning(mut self, turns: bool) -> Self {
        self.turns = turns;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    // The button Enter presses: it carries the ring the Electron app draws round the button that has the keyboard.
    pub fn leading(mut self, leads: bool) -> Self {
        self.leads = leads;
        self
    }

    pub fn on_click(
        self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> AnyElement {
        let (disabled, leads) = (self.disabled, self.leads && !self.disabled);
        let button = self
            .face()
            .when(!disabled, |button| button.on_click(handler));
        if !leads {
            return button.into_any_element();
        }
        div()
            .flex_none()
            .m(px(-4.))
            .p(px(2.))
            .rounded(px(12.))
            .border_2()
            .border_color(color(theme::ACCENT))
            .child(button)
            .into_any_element()
    }

    fn face(self) -> Stateful<Div> {
        let (text, glyph_hover) = match self.tone {
            Tone::Primary => (theme::TEXT_STRONG, theme::TEXT_STRONG),
            Tone::Secondary => (theme::TEXT, theme::TEXT),
            Tone::Ghost => (theme::MUTED, theme::TEXT),
            Tone::Danger => (theme::DANGER, theme::DANGER),
        };
        let bordered = matches!(self.tone, Tone::Secondary | Tone::Danger);
        let (tone, disabled) = (self.tone, self.disabled);
        let glyph = self.glyph.map(|glyph| {
            let glyph = icon(glyph, 16., text);
            if self.turns {
                turning("glyph", glyph)
            } else {
                glyph
                    .when(!disabled, |glyph| {
                        glyph.group_hover("button", move |style| {
                            style.text_color(color(glyph_hover))
                        })
                    })
                    .into_any_element()
            }
        });
        div()
            .id(self.id)
            .group("button")
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .px(px(12.))
            .py(px(6.))
            .rounded(px(8.))
            .text_size(px(14.))
            .line_height(px(20.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(color(text))
            .whitespace_nowrap()
            .when(bordered, |button| button.border_1())
            .map(|button| match tone {
                Tone::Primary => button.bg(color(theme::ACCENT)),
                Tone::Secondary => button
                    .border_color(color(theme::BORDER))
                    .bg(color(theme::SURFACE_2)),
                Tone::Ghost => button,
                Tone::Danger => button
                    .border_color(alpha(theme::DANGER, 0.4))
                    .bg(alpha(theme::DANGER, 0.1)),
            })
            .map(|button| {
                if disabled {
                    return button.opacity(0.5);
                }
                button.cursor_pointer().hover(move |style| match tone {
                    Tone::Primary => style.bg(color(theme::ACCENT_HOVER)),
                    Tone::Secondary => style
                        .bg(color(theme::SURFACE_3))
                        .border_color(color(theme::BORDER_STRONG)),
                    Tone::Ghost => style
                        .bg(color(theme::SURFACE_2))
                        .text_color(color(theme::TEXT)),
                    Tone::Danger => style
                        .bg(alpha(theme::DANGER, 0.2))
                        .border_color(alpha(theme::DANGER, 0.7)),
                })
            })
            .children(glyph)
            .child(self.label)
    }
}
