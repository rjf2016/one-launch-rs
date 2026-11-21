use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    AnyElement, ClickEvent, Context, DispatchPhase, Div, Entity, EventEmitter, FontWeight,
    Keystroke, MouseButton, MouseUpEvent, NavigationDirection, Pixels, SharedString, Stateful,
    Subscription, Svg, Window, WindowControlArea, canvas, div, px, svg,
};
use ol_api::Backend;
use ol_api::settings::QuitQuestion;
use ol_model::{Game, LaunchJob, LauncherId, SettingKey, Sidebar};
use ol_ui_kit::theme::{self, alpha, color};

use crate::account::AccountPage;
use crate::accounts::AccountsView;
use crate::assets;
use crate::host::{LaunchHost, PlayEnded, overlay};
use crate::library::LibraryView;
use crate::settings::{SettingsTab, SettingsView};
use crate::widgets::{Button, Tone, panel_shadow, spinner, tip};

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

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Route {
    Library,
    Accounts,
    // The Accounts list with the launcher's import dialog up.
    Import(LauncherId),
    NewAccount(LauncherId),
    // `created`: the page a Create account just opened, which says what comes next.
    Account {
        id: String,
        created: bool,
    },
    // The tab named, or the one last shown in this run.
    Settings(Option<SettingsTab>),
    // The Library with the game's launch dialog open, the account picked when one is named.
    Play {
        game: Box<Game>,
        account: Option<String>,
    },
}

pub(crate) struct Go {
    pub route: Route,
    pub guarded: bool,
    // The game whose Play sent the user off to add or import an account: the page this move opens may bring them back to it. Only the move that carries it has it.
    pub return_to: Option<Game>,
}

impl Go {
    pub fn to(route: Route) -> Self {
        Self {
            route,
            guarded: true,
            return_to: None,
        }
    }

    // A move a page makes once its work is done.
    pub fn done(route: Route) -> Self {
        Self {
            route,
            guarded: false,
            return_to: None,
        }
    }
}

#[derive(Clone)]
pub(crate) struct Question {
    pub title: &'static str,
    pub message: &'static str,
}

enum Asked {
    Leave {
        then: Route,
        return_to: Option<Box<Game>>,
    },
    // A quit while a switch runs; its first answer keeps the app running.
    Quit,
    // "Quit anyway" was answered: no answer is left, only the line that the switch is being stopped.
    Stopping,
}

struct Asking {
    title: SharedString,
    message: SharedString,
    detail: Option<SharedString>,
    keep: &'static str,
    act: &'static str,
    asked: Asked,
    // The second answer carries the ring, and Enter gives it.
    act_leads: bool,
}

// The user answered Quit anyway: the app is to quit.
pub struct QuitAnyway;

// A job the app followed has ended: the window may go to the tray.
pub struct Played {
    pub job: LaunchJob,
    pub instant: bool,
}

// The Library lives as long as the window, so its posters stay decoded; the other screens are made afresh at each visit, as the Electron app's are.
enum Page {
    Library,
    Accounts(Entity<AccountsView>),
    Account(Entity<AccountPage>),
    Settings(Entity<SettingsView>),
}

impl Page {
    fn screen(&self) -> Screen {
        match self {
            Self::Library => Screen::Library,
            Self::Accounts(_) | Self::Account(_) => Screen::Accounts,
            Self::Settings(_) => Screen::Settings,
        }
    }
}

pub struct Shell {
    backend: Backend,
    host: Entity<LaunchHost>,
    library: Entity<LibraryView>,
    page: Page,
    asking: Option<Asking>,
    sidebar_open: bool,
    // The user has toggled the sidebar in this run: the stored choice, should it arrive later, does not undo that.
    toggled: bool,
    fake_launchers: bool,
    settings_tab: Rc<Cell<SettingsTab>>,
    preview: bool,
    _page: Option<Subscription>,
    _keys: Subscription,
    _host: Subscription,
    _played: Subscription,
}

impl EventEmitter<QuitAnyway> for Shell {}

impl EventEmitter<Played> for Shell {}

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
        cx.spawn({
            let backend = backend.clone();
            async move |this, cx| {
                let Ok(settings) = backend.settings().await else {
                    return;
                };
                this.update(cx, |this, cx| {
                    this.preview = settings.toggle(SettingKey::DryRun);
                    cx.notify();
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
        let from_host = cx.subscribe_in(&host, window, Self::on_go);
        let played = cx.subscribe(&host, |_, _, ended: &PlayEnded, cx| {
            cx.emit(Played {
                job: ended.job.clone(),
                instant: ended.instant,
            });
        });
        Self {
            library: cx.new(|cx| LibraryView::new(backend.clone(), host.clone(), window, cx)),
            backend,
            host,
            page: Page::Library,
            asking: None,
            sidebar_open: false,
            toggled: false,
            fake_launchers,
            settings_tab: Rc::default(),
            preview: false,
            _page: None,
            _keys: keys,
            _host: from_host,
            _played: played,
        }
    }

    // A setting the backend stored: the sidebar's Preview mode card and the Settings page follow it.
    pub fn setting_changed(
        &mut self,
        key: SettingKey,
        value: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if key == SettingKey::DryRun {
            self.preview = value
                .as_deref()
                .is_some_and(|value| value == "true" || value == "1");
            cx.notify();
        }
        if let Page::Settings(page) = &self.page {
            page.update(cx, |page, cx| page.setting_changed(key, value, cx));
        }
    }

    // The question before a quit stops a running switch. Asked once at a time; it takes the place of a question about leaving a page.
    pub fn ask_before_quit(
        &mut self,
        question: QuitQuestion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(
            self.asking,
            Some(Asking {
                asked: Asked::Quit | Asked::Stopping,
                ..
            })
        ) {
            return;
        }
        self.asking = Some(Asking {
            title: question.title.into(),
            message: question.message.into(),
            detail: Some(question.detail.into()),
            keep: question.keep,
            act: question.quit,
            asked: Asked::Quit,
            act_leads: false,
        });
        window.blur(cx);
        cx.notify();
    }

    fn open_preview_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Page::Settings(page) = &self.page {
            page.update(cx, |page, cx| page.select_tab(SettingsTab::Advanced, cx));
            return;
        }
        self.go(
            Go::to(Route::Settings(Some(SettingsTab::Advanced))),
            window,
            cx,
        );
    }

    pub fn library(&self) -> &Entity<LibraryView> {
        &self.library
    }

    // The sidebar and Ctrl+1/2/3. Accounts goes back to the list from an account's page, as in the Electron app.
    fn navigate(&mut self, screen: Screen, window: &mut Window, cx: &mut Context<Self>) {
        let on_list = matches!(self.page, Page::Accounts(_));
        if screen == self.page.screen() && (screen != Screen::Accounts || on_list) {
            return;
        }
        let route = match screen {
            Screen::Library => Route::Library,
            Screen::Accounts => Route::Accounts,
            Screen::Settings => Route::Settings(None),
        };
        self.go(Go::to(route), window, cx);
    }

    // An unguarded move is one a page makes when a create, a delete or an import has finished: it wins over a question still open.
    fn go(&mut self, go: Go, window: &mut Window, cx: &mut Context<Self>) {
        if self.stopping() {
            return;
        }
        if self.asking.is_some() {
            if go.guarded {
                return;
            }
            self.asking = None;
        }
        if go.guarded
            && let Some(question) = self.question(cx)
        {
            self.asking = Some(Asking {
                title: question.title.into(),
                message: question.message.into(),
                detail: None,
                keep: "Stay",
                act: "Leave",
                asked: Asked::Leave {
                    then: go.route,
                    return_to: go.return_to.map(Box::new),
                },
                act_leads: false,
            });
            window.blur(cx);
            cx.notify();
            return;
        }
        self.enter(go.route, go.return_to, window, cx);
    }

    fn question(&self, cx: &Context<Self>) -> Option<Question> {
        match &self.page {
            Page::Account(page) => page.read(cx).leave_question(cx),
            _ => None,
        }
    }

    fn stopping(&self) -> bool {
        matches!(
            self.asking,
            Some(Asking {
                asked: Asked::Stopping,
                ..
            })
        )
    }

    fn answer(&mut self, act: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.stopping() {
            return;
        }
        let Some(asking) = self.asking.take() else {
            return;
        };
        match asking.asked {
            Asked::Leave { then, return_to } if act => {
                self.enter(then, return_to.map(|game| *game), window, cx);
            }
            Asked::Quit if act => {
                self.asking = Some(Asking {
                    title: "Stopping the switch".into(),
                    message:
                        "One Launch quits once the switch has stopped and put back what it had to."
                            .into(),
                    detail: None,
                    keep: "",
                    act: "",
                    asked: Asked::Stopping,
                    act_leads: false,
                });
                cx.emit(QuitAnyway);
            }
            _ => {}
        }
        cx.notify();
    }

    // Every page but the Library is made afresh, so the return target lives exactly as long as the page it was handed to.
    fn enter(
        &mut self,
        route: Route,
        return_to: Option<Game>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let left = self.page.screen();
        if let Page::Account(page) = &self.page {
            page.update(cx, |page, cx| page.leave(window, cx));
        }
        let backend = self.backend.clone();
        let mut play = None;
        self._page = None;
        self.page = match route {
            Route::Library => Page::Library,
            Route::Play { game, account } => {
                play = Some((*game, account));
                Page::Library
            }
            Route::Settings(tab) => {
                if let Some(tab) = tab {
                    self.settings_tab.set(tab);
                }
                let tab = Rc::clone(&self.settings_tab);
                Page::Settings(cx.new(|cx| SettingsView::new(backend, tab, cx)))
            }
            Route::Accounts => {
                let page = cx.new(|cx| AccountsView::new(backend, None, None, cx));
                self._page = Some(cx.subscribe_in(&page, window, Self::on_go));
                Page::Accounts(page)
            }
            Route::Import(launcher) => {
                let page = cx.new(|cx| AccountsView::new(backend, Some(launcher), return_to, cx));
                self._page = Some(cx.subscribe_in(&page, window, Self::on_go));
                Page::Accounts(page)
            }
            Route::NewAccount(launcher) => {
                let page =
                    cx.new(|cx| AccountPage::create(backend, launcher, return_to, window, cx));
                self._page = Some(cx.subscribe_in(&page, window, Self::on_go));
                Page::Account(page)
            }
            Route::Account { id, created } => {
                let host = self.host.clone();
                let page = cx.new(|cx| AccountPage::edit(backend, host, id, created, window, cx));
                self._page = Some(cx.subscribe_in(&page, window, Self::on_go));
                Page::Account(page)
            }
        };
        let screen = self.page.screen();
        self.library.update(cx, |library, cx| {
            if screen == Screen::Library && left != Screen::Library {
                library.arrive(window, cx);
            } else if screen != Screen::Library && left == Screen::Library {
                library.leave(window, cx);
            }
        });
        if let Some((game, account)) = play {
            self.host
                .update(cx, |host, cx| host.open(game, account, false, cx));
        }
        cx.notify();
    }

    fn on_go<E>(&mut self, _: &Entity<E>, go: &Go, window: &mut Window, cx: &mut Context<Self>) {
        self.go(
            Go {
                route: go.route.clone(),
                guarded: go.guarded,
                return_to: go.return_to.clone(),
            },
            window,
            cx,
        );
    }

    // The mouse's Back button and Alt+Left leave an account's page for the list; nowhere else has anywhere to go back to.
    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !matches!(self.page, Page::Account(_)) || self.modal(cx) {
            return false;
        }
        self.go(Go::to(Route::Accounts), window, cx);
        true
    }

    fn modal(&self, cx: &Context<Self>) -> bool {
        self.asking.is_some() || self.host.read(cx).is_modal() || self.importing(cx)
    }

    fn importing(&self, cx: &Context<Self>) -> bool {
        matches!(&self.page, Page::Accounts(page) if page.read(cx).importing())
    }

    fn typing(&self, window: &Window, cx: &Context<Self>) -> bool {
        match &self.page {
            Page::Library => self.library.read(cx).typing(window, cx),
            Page::Account(page) => page.read(cx).typing(window, cx),
            Page::Accounts(_) | Page::Settings(_) => false,
        }
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
        let none = !held.control && !held.alt && !held.shift && !held.platform && !held.function;
        if let Some(asking) = &mut self.asking {
            // The window's own keys, Alt+F4 among them, reach Windows only when nothing stops them.
            if held.alt || held.platform {
                return false;
            }
            match keystroke.key.as_str() {
                "escape" if none => self.answer(false, window, cx),
                "enter" if none => {
                    let act = asking.act_leads;
                    self.answer(act, window, cx);
                }
                "tab" => {
                    asking.act_leads = !asking.act_leads;
                    cx.notify();
                }
                _ => {}
            }
            return true;
        }
        if self.host.read(cx).is_modal() {
            return self
                .host
                .update(cx, |host, cx| host.on_keystroke(keystroke, cx));
        }
        if let Page::Accounts(page) = &self.page
            && page.read(cx).importing()
        {
            if held.alt || held.platform {
                return false;
            }
            page.update(cx, |page, cx| page.on_import_key(keystroke, cx));
            return true;
        }
        let only_alt = held.alt && !held.control && !held.shift && !held.platform && !held.function;
        if only_alt && keystroke.key == "left" {
            return self.back(window, cx);
        }
        let only_control =
            held.control && !held.alt && !held.shift && !held.platform && !held.function;
        if only_control && keystroke.key == "s" {
            if let Page::Account(page) = &self.page {
                page.update(cx, |page, cx| page.save(window, cx));
                return true;
            }
            return false;
        }
        // As in the Electron app: neither a dialog nor a text field gives these keys up.
        if only_control && !self.typing(window, cx) {
            let taken = match keystroke.key.as_str() {
                "b" => {
                    self.toggle_sidebar(cx);
                    true
                }
                "1" => {
                    self.navigate(Screen::Library, window, cx);
                    true
                }
                "2" => {
                    self.navigate(Screen::Accounts, window, cx);
                    true
                }
                "3" => {
                    self.navigate(Screen::Settings, window, cx);
                    true
                }
                _ => false,
            };
            // The Library's own Ctrl keys (Ctrl+F) come next; no other page has any.
            if taken || !matches!(self.page, Page::Library) {
                return taken;
            }
        }
        match &self.page {
            Page::Accounts(page) => page.update(cx, |page, cx| page.on_key(keystroke, cx)),
            Page::Account(page) => page.update(cx, |page, cx| page.on_key(keystroke, window, cx)),
            Page::Settings(page) => page.update(cx, |page, cx| page.on_key(keystroke, cx)),
            Page::Library => self.library.update(cx, |library, cx| {
                library.on_keystroke(keystroke, window, cx)
            }),
        }
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
            // The Update ready card is to stand in this column too, under the Preview mode card.
            .child(
                div()
                    .mt_auto()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .px(px(8.))
                    .pb(px(12.))
                    .children(self.preview.then(|| self.render_preview_card(open, cx)))
                    .child(self.render_toggle_row(open, glyph, toggle_tip, cx)),
            )
    }

    // The Electron app's Preview mode card: a click opens Settings on Advanced, where the switch is.
    fn render_preview_card(&self, open: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("preview-card")
            .flex()
            .items_start()
            .gap(px(8.))
            .rounded(px(8.))
            .border_1()
            .px(px(11.))
            .py(px(8.))
            .text_size(px(12.))
            .line_height(px(16.))
            .text_color(color(theme::WARNING))
            .whitespace_nowrap()
            .cursor_pointer()
            .map(|card| {
                if open {
                    card.border_color(alpha(theme::WARNING, 0.3))
                        .bg(alpha(theme::WARNING, 0.1))
                        .hover(|style| style.bg(alpha(theme::WARNING, 0.15)))
                } else {
                    card.border_color(gpui::transparent_black())
                        .hover(|style| style.bg(alpha(theme::WARNING, 0.1)))
                }
            })
            .tooltip(tip(
                "Preview mode is on: Play shows what a switch would do instead of doing it. Turn it off in Settings, or from the preview itself.",
            ))
            .on_click(cx.listener(|this, _, window, cx| this.open_preview_settings(window, cx)))
            .child(icon(assets::FLASK, 14., theme::WARNING).mt(px(1.)))
            .when(open, |card| {
                card.child(
                    div()
                        .w(px(164.))
                        .flex_none()
                        .whitespace_normal()
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .child("Preview mode is on"),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .line_height(px(15.))
                                .text_color(alpha(theme::WARNING, 0.8))
                                .child("Play shows what would happen, nothing changes."),
                        ),
                )
            })
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
            Page::Account(account) => account.clone().into_any_element(),
            Page::Settings(settings) => settings.clone().into_any_element(),
        }
    }

    fn render_question(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let asking = self.asking.as_ref()?;
        let card = div()
            .id("confirm")
            .occlude()
            .w_full()
            .max_w(px(448.))
            .rounded(px(12.))
            .border_1()
            .border_color(color(theme::BORDER))
            .bg(color(theme::SURFACE))
            .shadow(panel_shadow())
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .px(px(20.))
                    .py(px(16.))
                    .child(if matches!(asking.asked, Asked::Stopping) {
                        div()
                            .mt(px(2.))
                            .child(spinner("stopping", 20., theme::WARNING))
                            .into_any_element()
                    } else {
                        icon(assets::UPDATE, 20., theme::WARNING)
                            .mt(px(2.))
                            .into_any_element()
                    })
                    .child(
                        div()
                            .min_w_0()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(color(theme::TEXT_STRONG))
                                    .child(asking.title.clone()),
                            )
                            .child(
                                div()
                                    .mt(px(4.))
                                    .text_color(color(if asking.detail.is_some() {
                                        theme::TEXT
                                    } else {
                                        theme::MUTED
                                    }))
                                    .child(asking.message.clone()),
                            )
                            .children(asking.detail.clone().map(|detail| {
                                div()
                                    .mt(px(8.))
                                    .text_color(color(theme::MUTED))
                                    .child(detail)
                            })),
                    ),
            )
            .when(!matches!(asking.asked, Asked::Stopping), |card| {
                card.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(8.))
                        .border_t_1()
                        .border_color(color(theme::BORDER))
                        .px(px(20.))
                        .py(px(12.))
                        .child(
                            Button::new("stay", Tone::Ghost, asking.keep)
                                .leading(!asking.act_leads)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.answer(false, window, cx);
                                })),
                        )
                        .child(
                            Button::new("leave", Tone::Danger, asking.act)
                                .leading(asking.act_leads)
                                .on_click(cx.listener(|this, click: &ClickEvent, window, cx| {
                                    // The second click of a double click is not an answer.
                                    if click.click_count() == 1 {
                                        this.answer(true, window, cx);
                                    }
                                })),
                        ),
                )
            });
        Some(
            div()
                .id("confirm-backdrop")
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .p(px(24.))
                .bg(alpha(theme::BG, 0.7))
                .on_click(cx.listener(|this, click: &ClickEvent, window, cx| {
                    if click.click_count() == 1 {
                        this.answer(false, window, cx);
                    }
                }))
                .child(card),
        )
    }

    // Let go anywhere in the window, as a browser hears the mouse's Back button. The listener stands in every frame.
    fn render_back_button(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.weak_entity();
        canvas(
            |_, _, _| {},
            move |_, (), window, _| {
                window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble
                        && event.button == MouseButton::Navigate(NavigationDirection::Back)
                    {
                        this.update(cx, |this, cx| this.back(window, cx)).ok();
                    }
                });
            },
        )
        .absolute()
        .size_0()
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
            .children(
                self.render_question(cx)
                    .map(|question| overlay(window.viewport_size(), 4, question)),
            )
            .child(self.render_back_button(cx))
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
