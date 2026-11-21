use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Div, Entity, FontWeight, Keystroke, Pixels, Stateful, Subscription, Svg,
    Window, WindowControlArea, div, px, svg,
};
use ol_api::Backend;
use ol_model::Sidebar;
use ol_ui_kit::theme::{self, alpha, color};

use crate::accounts::AccountsView;
use crate::assets;
use crate::host::LaunchHost;
use crate::library::LibraryView;
use crate::settings::SettingsView;
use crate::widgets::tip;

const SIDEBAR: f32 = 224.;
const SIDEBAR_COLLAPSED: f32 = 60.;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Library,
    Accounts,
    Settings,
}

impl Screen {
    const ALL: [Self; 3] = [Self::Library, Self::Accounts, Self::Settings];

    fn item(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            Self::Library => ("nav-library", assets::LIBRARY, "Library", "Ctrl+1"),
            Self::Accounts => ("nav-accounts", assets::ACCOUNTS, "Accounts", "Ctrl+2"),
            Self::Settings => ("nav-settings", assets::SETTINGS, "Settings", "Ctrl+3"),
        }
    }
}

// The Library lives as long as the window, so its posters stay decoded; the other screens are made afresh at each visit, as the Electron app's are.
enum Page {
    Library,
    Accounts(Entity<AccountsView>),
    Settings(Entity<SettingsView>),
}

impl Page {
    fn screen(&self) -> Screen {
        match self {
            Self::Library => Screen::Library,
            Self::Accounts(_) => Screen::Accounts,
            Self::Settings(_) => Screen::Settings,
        }
    }
}

pub struct Shell {
    backend: Backend,
    host: Entity<LaunchHost>,
    library: Entity<LibraryView>,
    page: Page,
    sidebar_open: bool,
    // The user has toggled the sidebar in this run: the stored choice, should it arrive later, does not undo that.
    toggled: bool,
    fake_launchers: bool,
    _keys: Subscription,
}

pub fn sidebar_width(open: bool) -> Pixels {
    px(if open { SIDEBAR } else { SIDEBAR_COLLAPSED })
}

impl Shell {
    pub fn new(
        backend: Backend,
        fake_launchers: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Asked before the Library asks for its games, so a stored open sidebar is in place before the first grid is laid out.
        cx.spawn({
            let backend = backend.clone();
            async move |this, cx| {
                let Ok(stored) = backend.sidebar().await else {
                    return;
                };
                this.update(cx, |this, cx| {
                    if !this.toggled && stored.is_open() != this.sidebar_open {
                        this.lay_out(stored.is_open(), cx);
                    }
                })
                .ok();
            }
        })
        .detach();
        let this = cx.weak_entity();
        let keys = cx.intercept_keystrokes(move |event, window, cx| {
            let taken = this
                .update(cx, |this, cx| {
                    this.on_keystroke(&event.keystroke, window, cx)
                })
                .unwrap_or(false);
            if taken {
                cx.stop_propagation();
            }
        });
        let host = cx.new(|cx| LaunchHost::new(backend.clone(), cx));
        Self {
            library: cx.new(|cx| LibraryView::new(backend.clone(), host.clone(), window, cx)),
            backend,
            host,
            page: Page::Library,
            sidebar_open: false,
            toggled: false,
            fake_launchers,
            _keys: keys,
        }
    }

    pub fn library(&self) -> &Entity<LibraryView> {
        &self.library
    }

    // Every way to another screen comes through here, so a later question about unsaved changes has one place to stand.
    fn navigate(&mut self, screen: Screen, window: &mut Window, cx: &mut Context<Self>) {
        let left = self.page.screen();
        if screen == left {
            return;
        }
        self.page = match screen {
            Screen::Library => Page::Library,
            Screen::Accounts => Page::Accounts(cx.new(|_| AccountsView)),
            Screen::Settings => Page::Settings(cx.new(|_| SettingsView)),
        };
        self.library.update(cx, |library, cx| {
            if screen == Screen::Library {
                library.arrive(window, cx);
            } else if left == Screen::Library {
                library.leave(window, cx);
            }
        });
        cx.notify();
    }

    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.toggled = true;
        let open = !self.sidebar_open;
        self.lay_out(open, cx);
        let sidebar = if open { Sidebar::Open } else { Sidebar::Folded };
        let backend = self.backend.clone();
        // A choice that could not be stored still holds for this run, as the Library's sort does.
        cx.spawn(async move |_, _| {
            let _ = backend.set_sidebar(sidebar).await;
        })
        .detach();
    }

    // The Library learns the width it lays out for in the same update as the sidebar, so the next frame draws both for it.
    fn lay_out(&mut self, open: bool, cx: &mut Context<Self>) {
        self.sidebar_open = open;
        let width = sidebar_width(open);
        self.library
            .update(cx, |library, cx| library.set_sidebar(width, cx));
        cx.notify();
    }

    fn on_keystroke(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let held = keystroke.modifiers;
        let only_control =
            held.control && !held.alt && !held.shift && !held.platform && !held.function;
        // As in the Electron app: neither a dialog nor the search field gives these keys up.
        if !only_control
            || self.host.read(cx).is_modal()
            || self.library.read(cx).typing(window, cx)
        {
            return false;
        }
        match keystroke.key.as_str() {
            "b" => self.toggle_sidebar(cx),
            "1" => self.navigate(Screen::Library, window, cx),
            "2" => self.navigate(Screen::Accounts, window, cx),
            "3" => self.navigate(Screen::Settings, window, cx),
            _ => return false,
        }
        true
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> Div {
        let open = self.sidebar_open;
        let (glyph, toggle_tip) = if open {
            (assets::COLLAPSE, "Collapse sidebar (Ctrl+B)")
        } else {
            (assets::EXPAND, "Expand sidebar (Ctrl+B)")
        };
        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(sidebar_width(self.sidebar_open))
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
                    .children(
                        Screen::ALL
                            .into_iter()
                            .map(|screen| self.nav_item(screen, open, cx)),
                    ),
            )
            // The Preview mode and Update ready cards stand in this column, above the button.
            .child(
                div()
                    .mt_auto()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .px(px(8.))
                    .pb(px(12.))
                    .child(self.render_toggle_row(open, glyph, toggle_tip, cx)),
            )
    }

    fn render_toggle_row(
        &self,
        open: bool,
        glyph: &'static str,
        toggle_tip: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .child(
                div()
                    .id("sidebar-toggle")
                    .group("sidebar-toggle")
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(px(36.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .hover(|style| style.bg(color(theme::SURFACE_2)))
                    .tooltip(tip(toggle_tip))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx)))
                    .child(
                        icon(glyph, 18., theme::FAINT).group_hover("sidebar-toggle", |style| {
                            style.text_color(color(theme::TEXT))
                        }),
                    ),
            )
            .when(open, |row| {
                row.child(
                    div()
                        .pr(px(8.))
                        .text_size(px(11.))
                        .text_color(color(theme::FAINT))
                        .whitespace_nowrap()
                        .child(concat!("v", env!("CARGO_PKG_VERSION"))),
                )
            })
    }

    fn nav_item(&self, screen: Screen, open: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let (id, glyph, label, shortcut) = screen.item();
        let active = screen == self.page.screen();
        let hex = if active {
            theme::TEXT_STRONG
        } else {
            theme::MUTED
        };
        div()
            .id(id)
            .group(id)
            .relative()
            .flex()
            .items_center()
            .gap(px(12.))
            .rounded(px(8.))
            .px(px(12.))
            .py(px(10.))
            .text_size(px(15.))
            .line_height(px(21.75))
            .text_color(color(hex))
            .whitespace_nowrap()
            .cursor_pointer()
            .tooltip(tip(format!("{label} ({shortcut})")))
            .on_click(cx.listener(move |this, _, window, cx| this.navigate(screen, window, cx)))
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
                    item.hover(|style| {
                        style
                            .bg(alpha(theme::SURFACE_2, 0.6))
                            .text_color(color(theme::TEXT))
                    })
                }
            })
            .child(icon(glyph, 20., hex).when(!active, |glyph| {
                glyph.group_hover(id, |style| style.text_color(color(theme::TEXT)))
            }))
            .when(open, |item| item.child(label))
    }

    fn render_screen(&self) -> AnyElement {
        match &self.page {
            Page::Library => self.library.clone().into_any_element(),
            Page::Accounts(accounts) => accounts.clone().into_any_element(),
            Page::Settings(settings) => settings.clone().into_any_element(),
        }
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(color(theme::BG))
            .text_color(color(theme::TEXT))
            .font_family(theme::FONT)
            .text_size(px(14.))
            .line_height(px(20.))
            .child(title_bar(window.is_maximized(), self.fake_launchers))
            .child(self.host.clone())
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_sidebar(cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_hidden()
                            .child(self.render_screen()),
                    ),
            )
    }
}

fn title_bar(maximized: bool, fake_launchers: bool) -> Div {
    let badge = fake_launchers.then(|| {
        div()
            .rounded(px(4.))
            .px(px(6.))
            .py(px(1.))
            .bg(alpha(theme::WARNING, 0.2))
            .text_size(px(10.))
            .line_height(px(15.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(color(theme::WARNING))
            .child("FAKE LAUNCHERS")
    });
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
        .children(badge);
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

fn icon(path: &'static str, size: f32, hex: u32) -> Svg {
    svg()
        .path(path)
        .flex_none()
        .size(px(size))
        .text_color(color(hex))
}
