use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt as _, AnyElement, AnyView, App, BoxShadow, ClickEvent, Context, Div,
    ElementId, FontWeight, SharedString, Stateful, Svg, Task, Transformation, Window, div, hsla,
    percentage, px, relative, svg,
};
use ol_model::LauncherId;
use ol_ui_kit::theme::{self, alpha, color};

use crate::assets;

const TURN: Duration = Duration::from_secs(1);
const FLASH_STAYS: Duration = Duration::from_secs(3);
const ERROR_FLASH_STAYS: Duration = Duration::from_secs(6);

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
    tip_of(text.into())
}

fn tip_of(text: SharedString) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
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
    trailing: Option<AnyElement>,
    text: Option<u32>,
    tip: Option<SharedString>,
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
            trailing: None,
            text: None,
            tip: None,
            turns: false,
            disabled: false,
            leads: false,
        }
    }

    pub fn glyph(mut self, glyph: &'static str) -> Self {
        self.glyph = Some(glyph);
        self
    }

    pub fn trailing(mut self, trailing: AnyElement) -> Self {
        self.trailing = Some(trailing);
        self
    }

    pub fn text(mut self, hex: u32) -> Self {
        self.text = Some(hex);
        self
    }

    pub fn tip(mut self, tip: impl Into<SharedString>) -> Self {
        self.tip = Some(tip.into());
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
        let (text, glyph_hover) = match (self.text, self.tone) {
            (Some(hex), _) => (hex, hex),
            (None, Tone::Primary) => (theme::TEXT_STRONG, theme::TEXT_STRONG),
            (None, Tone::Secondary) => (theme::TEXT, theme::TEXT),
            (None, Tone::Ghost) => (theme::MUTED, theme::TEXT),
            (None, Tone::Danger) => (theme::DANGER, theme::DANGER),
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
            .when_some(self.tip, |button, tip| button.tooltip(tip_of(tip)))
            .children(glyph)
            .child(self.label)
            .children(self.trailing)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FlashTone {
    Info,
    Success,
    Error,
}

pub struct Flash {
    text: SharedString,
    tone: FlashTone,
    _timer: Task<()>,
}

impl Flash {
    pub fn new<T: 'static>(
        text: impl Into<SharedString>,
        tone: FlashTone,
        stays: Option<Duration>,
        slot: fn(&mut T) -> &mut Option<Flash>,
        cx: &mut Context<T>,
    ) -> Self {
        let stays = stays.unwrap_or(if tone == FlashTone::Error {
            ERROR_FLASH_STAYS
        } else {
            FLASH_STAYS
        });
        let timer = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(stays).await;
            this.update(cx, |this, cx| {
                *slot(this) = None;
                cx.notify();
            })
            .ok();
        });
        Self {
            text: text.into(),
            tone,
            _timer: timer,
        }
    }

    pub fn render(&self) -> Div {
        div()
            .min_w_0()
            .text_size(px(12.))
            .line_height(px(16.))
            .text_color(color(match self.tone {
                FlashTone::Info => theme::MUTED,
                FlashTone::Success => theme::SUCCESS,
                FlashTone::Error => theme::DANGER,
            }))
            .child(self.text.clone())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PillTone {
    Ok,
    Warn,
    Fail,
    Info,
}

pub fn pill(tone: PillTone, glyph: Option<&'static str>, label: impl Into<SharedString>) -> Div {
    let (line, fill, hex) = match tone {
        PillTone::Ok => (
            alpha(theme::SUCCESS, 0.3),
            alpha(theme::SUCCESS, 0.1),
            theme::SUCCESS,
        ),
        PillTone::Warn => (
            alpha(theme::WARNING, 0.3),
            alpha(theme::WARNING, 0.1),
            theme::WARNING,
        ),
        PillTone::Fail => (
            alpha(theme::DANGER, 0.3),
            alpha(theme::DANGER, 0.1),
            theme::DANGER,
        ),
        PillTone::Info => (
            color(theme::BORDER).into(),
            color(theme::SURFACE_2).into(),
            theme::MUTED,
        ),
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(4.))
        .rounded(px(6.))
        .border_1()
        .border_color(line)
        .bg(fill)
        .px(px(8.))
        .py(px(2.))
        .text_size(px(12.))
        .line_height(px(16.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(color(hex))
        .children(glyph.map(|glyph| icon(glyph, 12., hex)))
        .child(label.into())
}

pub fn panel_shadow() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(1.), hsla(0., 0., 1., 0.03)).inset(),
        BoxShadow::new(px(0.), px(8.), hsla(0., 0., 0., 0.35)).blur_radius(px(24.)),
    ]
}

pub fn panel(
    title: &'static str,
    description: Option<SharedString>,
    actions: Option<AnyElement>,
    body: impl IntoElement,
) -> Div {
    div()
        .rounded(px(12.))
        .border_1()
        .border_color(color(theme::BORDER))
        .bg(color(theme::SURFACE))
        .shadow(panel_shadow())
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(16.))
                .border_b_1()
                .border_color(color(theme::BORDER))
                .px(px(20.))
                .py(px(16.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(color(theme::TEXT_STRONG))
                                .child(title),
                        )
                        .children(description.map(|line| {
                            div()
                                .mt(px(2.))
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(color(theme::MUTED))
                                .child(line)
                        })),
                )
                .children(actions),
        )
        .child(div().p(px(20.)).child(body))
}

pub fn field(
    label: impl Into<SharedString>,
    hint: Option<SharedString>,
    error: Option<SharedString>,
    control: impl IntoElement,
) -> Div {
    let (line, hex) = match (error, hint) {
        (Some(error), _) => (Some(error), theme::DANGER),
        (None, hint) => (hint, theme::FAINT),
    };
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .min_w_0()
        .child(
            div()
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(color(theme::MUTED))
                .child(label.into()),
        )
        .child(control)
        .children(line.map(|line| {
            div()
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(color(hex))
                .child(line)
        }))
}

// A line of a panel: what it is on the left, its value on the right. Each is followed by more of the panel, so each has the line under it.
pub fn row(
    label: impl Into<SharedString>,
    hint: Option<SharedString>,
    first: bool,
    value: impl IntoElement,
) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(24.))
        .py(px(10.))
        .when(first, |row| row.pt_0())
        .border_b_1()
        .border_color(alpha(theme::BORDER, 0.6))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(div().text_color(color(theme::TEXT)).child(label.into()))
                .children(hint.map(|hint| {
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color(theme::MUTED))
                        .child(hint)
                })),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .min_w_0()
                .max_w(relative(0.6))
                .items_center()
                .justify_end()
                .gap(px(8.))
                .text_right()
                .text_color(color(theme::MUTED))
                .child(value),
        )
}

pub fn menu_shadow() -> Vec<BoxShadow> {
    vec![BoxShadow::new(px(0.), px(8.), hsla(0., 0., 0., 0.35)).blur_radius(px(24.))]
}

pub fn menu_chevron(open: bool, hex: u32, opacity: f32) -> AnyElement {
    let chevron = icon(assets::CHEVRON_DOWN, 14., hex)
        .opacity(opacity)
        .mr(px(-2.));
    if open {
        chevron
            .with_transformation(Transformation::rotate(percentage(0.5)))
            .into_any_element()
    } else {
        chevron.into_any_element()
    }
}
