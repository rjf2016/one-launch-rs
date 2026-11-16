use gpui::prelude::*;
use gpui::{
    Context, Div, Entity, FontWeight, Pixels, Stateful, Svg, Window, WindowControlArea, div, px,
    svg,
};
use ol_api::Backend;
use ol_ui_kit::theme::{self, alpha, color};

use crate::assets;
use crate::library::LibraryView;

const SIDEBAR: f32 = 224.;
const SIDEBAR_COLLAPSED: f32 = 60.;
const NARROW: f32 = 1100.;

pub struct Shell {
    library: Entity<LibraryView>,
}

pub fn sidebar_width(viewport: Pixels) -> Pixels {
    px(if viewport < px(NARROW) {
        SIDEBAR_COLLAPSED
    } else {
        SIDEBAR
    })
}

impl Shell {
    pub fn new(backend: Backend, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            library: cx.new(|cx| LibraryView::new(backend, window, cx)),
        }
    }

    pub fn library(&self) -> &Entity<LibraryView> {
        &self.library
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let collapsed = window.viewport_size().width < px(NARROW);
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(color(theme::BG))
            .text_color(color(theme::TEXT))
            .font_family(theme::FONT)
            .text_size(px(14.))
            .line_height(px(20.))
            .child(title_bar(window.is_maximized()))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(sidebar(collapsed))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_hidden()
                            .child(self.library.clone()),
                    ),
            )
    }
}

fn title_bar(maximized: bool) -> Div {
    let badge = div()
        .rounded(px(4.))
        .px(px(6.))
        .py(px(1.))
        .bg(alpha(theme::WARNING, 0.2))
        .text_size(px(10.))
        .line_height(px(15.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(color(theme::WARNING))
        .child("GPUI PROOF");
    let drag = div()
        .id("title-drag")
        .window_control_area(WindowControlArea::Drag)
        .flex()
        .flex_1()
        .h_full()
        .items_center()
        .gap(px(8.))
        .pl(px(12.))
        .child(icon(assets::BRAND, 16., theme::ACCENT))
        .child(
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color(theme::MUTED))
                .child("One Launch"),
        )
        .child(badge);
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(36.))
        .bg(color(theme::SURFACE))
        .border_b_1()
        .border_color(alpha(theme::BORDER, 0.5))
        .child(drag)
        .child(caption_button(
            "minimize",
            assets::MINIMIZE,
            WindowControlArea::Min,
            theme::SURFACE_3,
        ))
        .child(caption_button(
            "maximize",
            if maximized {
                assets::RESTORE
            } else {
                assets::MAXIMIZE
            },
            WindowControlArea::Max,
            theme::SURFACE_3,
        ))
        .child(caption_button(
            "close",
            assets::CLOSE,
            WindowControlArea::Close,
            theme::CLOSE_HOVER,
        ))
}

// No click handler: GPUI's window procedure answers the hit test for these areas and acts on the click itself, and Windows shows its snap flyout only over a maximise button it was told about that way.
fn caption_button(
    id: &'static str,
    glyph: &'static str,
    area: WindowControlArea,
    hover_hex: u32,
) -> Stateful<Div> {
    div()
        .id(id)
        .group(id)
        .window_control_area(area)
        .occlude()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .w(px(46.))
        .h_full()
        .hover(move |style| style.bg(color(hover_hex)))
        .child(
            icon(glyph, 10., theme::MUTED)
                .group_hover(id, |style| style.text_color(color(theme::TEXT_STRONG))),
        )
}

fn sidebar(collapsed: bool) -> Div {
    let width = if collapsed {
        SIDEBAR_COLLAPSED
    } else {
        SIDEBAR
    };
    div()
        .flex()
        .flex_col()
        .flex_none()
        .w(px(width))
        .h_full()
        .overflow_hidden()
        .bg(color(theme::SURFACE))
        .border_r_1()
        .border_color(color(theme::BORDER))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .px(px(8.))
                .pt(px(20.))
                .child(nav_item(
                    "nav-library",
                    assets::LIBRARY,
                    "Library",
                    true,
                    collapsed,
                ))
                .child(nav_item(
                    "nav-accounts",
                    assets::ACCOUNTS,
                    "Accounts",
                    false,
                    collapsed,
                ))
                .child(nav_item(
                    "nav-settings",
                    assets::SETTINGS,
                    "Settings",
                    false,
                    collapsed,
                )),
        )
        .child(
            div()
                .mt_auto()
                .flex()
                .items_center()
                .justify_between()
                .px(px(8.))
                .pb(px(12.))
                .child(
                    div()
                        .id("sidebar-toggle")
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .size(px(36.))
                        .rounded(px(8.))
                        .cursor_pointer()
                        .hover(|style| style.bg(color(theme::SURFACE_2)))
                        .child(icon(assets::COLLAPSE, 18., theme::FAINT)),
                )
                .when(!collapsed, |row| {
                    row.child(
                        div()
                            .pr(px(8.))
                            .text_size(px(11.))
                            .text_color(color(theme::FAINT))
                            .child(concat!("v", env!("CARGO_PKG_VERSION"))),
                    )
                }),
        )
}

fn nav_item(
    id: &'static str,
    glyph: &'static str,
    label: &'static str,
    active: bool,
    collapsed: bool,
) -> Stateful<Div> {
    let hex = if active {
        theme::TEXT_STRONG
    } else {
        theme::MUTED
    };
    div()
        .id(id)
        .relative()
        .flex()
        .items_center()
        .gap(px(12.))
        .rounded(px(8.))
        .px(px(12.))
        .py(px(10.))
        .text_size(px(15.))
        .line_height(px(22.5))
        .text_color(color(hex))
        .whitespace_nowrap()
        .cursor_pointer()
        .map(|item| {
            if active {
                item.bg(color(theme::SURFACE_2)).child(
                    div()
                        .absolute()
                        .left_0()
                        .top(px(10.))
                        .bottom(px(10.))
                        .w(px(2.))
                        .rounded_full()
                        .bg(color(theme::ACCENT)),
                )
            } else {
                item.hover(|style| style.bg(alpha(theme::SURFACE_2, 0.6)))
            }
        })
        .child(icon(glyph, 20., hex))
        .when(!collapsed, |item| item.child(label))
}

fn icon(path: &'static str, size: f32, hex: u32) -> Svg {
    svg()
        .path(path)
        .flex_none()
        .size(px(size))
        .text_color(color(hex))
}
