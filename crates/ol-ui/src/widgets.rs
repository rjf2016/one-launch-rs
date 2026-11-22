use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt as _, AnyElement, AnyView, App, Bounds, BoxShadow, ClickEvent, Context,
    Div, ElementId, FontWeight, MouseDownEvent, Pixels, Point, SharedString, Size, Stateful, Svg,
    Task, Transformation, Window, canvas, div, hsla, percentage, point, px, relative, svg,
};
use ol_model::LauncherId;
use ol_ui_kit::theme::{self, alpha, color};

use crate::assets;

const TURN: Duration = Duration::from_secs(1);
const FLASH_STAYS: Duration = Duration::from_secs(3);
const ERROR_FLASH_STAYS: Duration = Duration::from_secs(6);
// Windows' double-click time as it ships, read from no setting.
const SECOND_CLICK: Duration = Duration::from_millis(500);

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

// When a confirmation came up, so the second click of the double click that brought it is not taken for a yes, and later quick clicks are.
#[derive(Clone, Copy, Debug)]
pub struct Appeared(Instant);

impl Appeared {
    pub fn now() -> Self {
        Self(Instant::now())
    }

    pub fn takes(self, click: &ClickEvent) -> bool {
        takes_click(click.click_count(), self.0.elapsed())
    }
}

fn takes_click(count: usize, since: Duration) -> bool {
    count <= 1 || since >= SECOND_CLICK
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

// The ring the Electron app draws round what has the keyboard (`:focus-visible`: 2 px of accent, 2 px out), drawn without moving anything.
pub fn ringed(child: impl IntoElement, radius: Option<f32>) -> Div {
    div()
        .flex_none()
        .m(px(-4.))
        .p(px(2.))
        .border_2()
        .border_color(color(theme::ACCENT))
        .map(|ring| match radius {
            Some(radius) => ring.rounded(px(radius + 4.)),
            None => ring.rounded_full(),
        })
        .child(child)
}

fn knob_shadow() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(1.), hsla(0., 0., 0., 0.1)).blur_radius(px(3.)),
        BoxShadow::new(px(0.), px(1.), hsla(0., 0., 0., 0.1))
            .blur_radius(px(2.))
            .spread_radius(px(-1.)),
    ]
}

// The Electron app's Toggle: a switch 44 by 24 px whose knob stands right while it is on.
pub fn switch(
    id: impl Into<ElementId>,
    on: bool,
    disabled: bool,
    ring: bool,
    toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let face = div()
        .id(id)
        .relative()
        .flex_none()
        .w(px(44.))
        .h(px(24.))
        .rounded_full()
        .border_1()
        .map(|face| {
            if on {
                face.border_color(color(theme::ACCENT))
                    .bg(color(theme::ACCENT))
            } else {
                face.border_color(color(theme::BORDER_STRONG))
                    .bg(color(theme::SURFACE_3))
            }
        })
        .map(|face| {
            if disabled {
                face.opacity(0.5).cursor_not_allowed()
            } else {
                face.cursor_pointer().on_click(toggle)
            }
        })
        .child(
            div()
                .absolute()
                .top(px(2.))
                .left(px(if on { 22. } else { 2. }))
                .size(px(18.))
                .rounded_full()
                .bg(color(theme::TEXT_STRONG))
                .shadow(knob_shadow()),
        );
    if ring && !disabled {
        ringed(face, None).into_any_element()
    } else {
        face.into_any_element()
    }
}

// The Electron app's TabList: text tabs over a line, the chosen one underlined in accent.
pub fn tab_strip(
    labels: &[&'static str],
    chosen: usize,
    ring: bool,
    choose: impl Fn(&usize, &mut Window, &mut App) + 'static,
) -> Div {
    let choose = Rc::new(choose);
    div()
        .flex()
        .gap(px(4.))
        .border_b_1()
        .border_color(color(theme::BORDER))
        .children(labels.iter().enumerate().map(|(index, label)| {
            let picked = index == chosen;
            let choose = Rc::clone(&choose);
            let tab = div()
                .id(("settings-tab", index))
                .px(px(12.))
                .py(px(8.))
                .border_b_2()
                .text_size(px(14.))
                .line_height(px(20.))
                .whitespace_nowrap()
                .cursor_pointer()
                .map(|tab| {
                    if picked {
                        tab.border_color(color(theme::ACCENT))
                            .text_color(color(theme::TEXT_STRONG))
                    } else {
                        tab.border_color(gpui::transparent_black())
                            .text_color(color(theme::MUTED))
                            .hover(|style| style.text_color(color(theme::TEXT)))
                    }
                })
                .on_click(move |_, window, cx| choose(&index, window, cx))
                .child(*label);
            let tab = if picked && ring {
                ringed(tab, Some(0.)).into_any_element()
            } else {
                tab.into_any_element()
            };
            div().mb(px(-1.)).child(tab)
        }))
}

// Where the trigger of a dropdown was last drawn, in the window, for the menu to open against.
pub type Spot = Rc<Cell<Option<Bounds<Pixels>>>>;

pub fn spot_of(spot: &Spot) -> impl IntoElement + use<> {
    let spot = Rc::clone(spot);
    canvas(move |bounds, _, _| spot.set(Some(bounds)), |_, (), _, _| {})
        .absolute()
        .top_0()
        .left_0()
        .size_full()
}

// The Electron app's Dropdown trigger: the chosen value and a chevron, which turns while the menu is open.
pub fn dropdown_trigger(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    open: bool,
    disabled: bool,
    ring: bool,
    spot: &Spot,
    press: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let chevron = icon(assets::CHEVRON_DOWN, 16., theme::FAINT).ml(px(2.));
    let chevron = if open {
        chevron
            .with_transformation(Transformation::rotate(percentage(0.5)))
            .into_any_element()
    } else {
        chevron.into_any_element()
    };
    let trigger = div()
        .id(id)
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .px(px(12.))
        .py(px(6.))
        .rounded(px(8.))
        .border_1()
        .border_color(color(if open {
            theme::BORDER_STRONG
        } else {
            theme::BORDER
        }))
        .bg(color(theme::SURFACE_2))
        .text_size(px(14.))
        .line_height(px(20.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(color(theme::TEXT))
        .whitespace_nowrap()
        .map(|trigger| {
            if disabled {
                trigger.opacity(0.6).cursor_not_allowed()
            } else {
                trigger
                    .cursor_pointer()
                    .hover(|style| {
                        style
                            .border_color(color(theme::BORDER_STRONG))
                            .bg(color(theme::SURFACE_3))
                    })
                    .on_click(press)
            }
        })
        .child(spot_of(spot))
        .child(label.into())
        .child(chevron);
    if ring && !disabled {
        ringed(trigger, Some(8.)).into_any_element()
    } else {
        trigger.into_any_element()
    }
}

const MENU_ROW: f32 = 32.;
const MENU_LEAST: f32 = 176.;
const MENU_GAP: f32 = 6.;
const MENU_EDGE: f32 = 8.;

// As the Electron app places its menu: under the trigger when it fits, else over it, its left edge on the trigger's and kept 8 px inside the window.
pub fn dropdown_place(
    trigger: Bounds<Pixels>,
    viewport: Size<Pixels>,
    rows: usize,
) -> Point<Pixels> {
    let height = MENU_ROW * rows as f32 + 10.;
    let width = f32::from(trigger.size.width).max(MENU_LEAST);
    let left = f32::from(trigger.left())
        .min(f32::from(viewport.width) - width - MENU_EDGE)
        .max(MENU_EDGE);
    let below = f32::from(trigger.bottom()) + MENU_GAP;
    let top = if below + height <= f32::from(viewport.height) - MENU_EDGE {
        below
    } else {
        f32::from(trigger.top()) - MENU_GAP - height
    };
    point(px(left), px(top))
}

pub struct DropdownMenu {
    pub labels: Vec<SharedString>,
    pub chosen: usize,
    pub lit: usize,
    pub least: Pixels,
}

// The Electron app's Dropdown list: the row under the pointer or the arrows lit, a check by the chosen one.
pub fn dropdown_menu(
    menu: DropdownMenu,
    light: impl Fn(&usize, &mut Window, &mut App) + 'static,
    pick: impl Fn(&usize, &mut Window, &mut App) + 'static,
    away: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let (light, pick) = (Rc::new(light), Rc::new(pick));
    div()
        .id("dropdown-menu")
        .occlude()
        .flex()
        .flex_col()
        .min_w(menu.least.max(px(MENU_LEAST)))
        .p(px(4.))
        .rounded(px(8.))
        .border_1()
        .border_color(color(theme::BORDER))
        .bg(color(theme::SURFACE_2))
        .shadow(panel_shadow())
        .font_family(theme::FONT)
        .on_mouse_down_out(away)
        .children(menu.labels.into_iter().enumerate().map(|(index, label)| {
            let (lit, chosen) = (index == menu.lit, index == menu.chosen);
            let (light, pick) = (Rc::clone(&light), Rc::clone(&pick));
            div()
                .id(("dropdown-option", index))
                .flex()
                .items_center()
                .gap(px(8.))
                .py(px(6.))
                .pl(px(10.))
                .pr(px(8.))
                .rounded(px(6.))
                .text_size(px(14.))
                .line_height(px(20.))
                .whitespace_nowrap()
                .cursor_pointer()
                .text_color(color(if lit || chosen {
                    theme::TEXT_STRONG
                } else {
                    theme::TEXT
                }))
                .when(lit, |row| row.bg(color(theme::SURFACE_3)))
                .on_mouse_move(move |_, window, cx| light(&index, window, cx))
                .on_click(move |_, window, cx| pick(&index, window, cx))
                .child(div().flex_1().child(label))
                .child(
                    icon(assets::CHECK, 14., theme::ACCENT)
                        .when(!chosen, |check| check.invisible()),
                )
        }))
}

// A labelled row inside a panel, as the Electron app's Row: the label and hint on the left, a refusal under them, the control or value on the right.
pub fn setting_row(
    label: impl Into<SharedString>,
    hint: Option<SharedString>,
    error: Option<SharedString>,
    first: bool,
    last: bool,
    value: impl IntoElement,
) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(24.))
        .py(px(10.))
        .when(first, |row| row.pt_0())
        .when(last, |row| row.pb_0())
        .when(!last, |row| {
            row.border_b_1().border_color(alpha(theme::BORDER, 0.6))
        })
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
                }))
                .children(error.map(|error| {
                    div()
                        .mt(px(4.))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color(theme::DANGER))
                        .child(SharedString::from(format!(
                            "Could not save the setting: {error}"
                        )))
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

#[cfg(test)]
mod tests {
    use gpui::{Bounds, point, px, size};

    use std::time::Duration;

    use super::{SECOND_CLICK, dropdown_place, takes_click};

    #[test]
    fn a_confirmation_ignores_only_the_second_click_of_the_double_click_that_brought_it() {
        assert!(takes_click(1, Duration::ZERO));
        assert!(!takes_click(2, Duration::from_millis(120)));
        assert!(!takes_click(3, SECOND_CLICK - Duration::from_millis(1)));
        assert!(takes_click(2, SECOND_CLICK));
        assert!(takes_click(3, Duration::from_secs(2)));
    }

    fn trigger(x: f32, y: f32, width: f32) -> Bounds<gpui::Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(32.)))
    }

    // Where the Electron app's shots put the menus: 154 under Epic launch link, 161 under Log level and kept 8 px inside the window.
    #[test]
    fn a_menu_opens_where_the_electron_app_opens_it() {
        let window = size(px(1280.), px(800.));
        assert_eq!(
            dropdown_place(trigger(1029., 321., 186.), window, 2),
            point(px(1029.), px(359.))
        );
        assert_eq!(
            dropdown_place(trigger(1138., 565., 77.), window, 4),
            point(px(1096.), px(603.))
        );
        assert_eq!(
            dropdown_place(trigger(1086., 640., 77.), window, 4),
            point(px(1086.), px(496.))
        );
        assert_eq!(
            dropdown_place(trigger(1086., 620., 77.), window, 4),
            point(px(1086.), px(476.))
        );
        assert_eq!(
            dropdown_place(trigger(2., 100., 77.), window, 2),
            point(px(8.), px(138.))
        );
    }
}
