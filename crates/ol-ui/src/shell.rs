use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    AnyElement, ClickEvent, Context, DispatchPhase, Div, Entity, EventEmitter, FocusHandle,
    FontWeight, Keystroke, MouseButton, MouseUpEvent, NavigationDirection, Pixels, SharedString,
    Stateful, Subscription, Svg, Window, WindowControlArea, canvas, div, px, svg,
};
use ol_api::Backend;
use ol_api::settings::{QuitAnswer, QuitQuestion, UpdateStatus};
use ol_api::whats_new::{AtStart, Release, at_start, shows_setup_guide};
use ol_model::{Game, LaunchJob, LauncherId, SettingKey, Sidebar};
use ol_ui_kit::theme::{self, alpha, color};

use crate::account::AccountPage;
use crate::accounts::AccountsView;
use crate::assets;
use crate::guide::{GuideDone, GuideExit, SetupGuide};
use crate::host::{Ask, LaunchHost, PlayEnded, overlay};
use crate::library::{LibraryView, OpenArtworkSettings};
use crate::settings::{KnownStatuses, SettingsAsk, SettingsTab, SettingsView};
use crate::whats_new::{WhatsNewCard, WhatsNewDone};
use crate::widgets::{Appeared, Button, Tone, panel_shadow, spinner, tip};

const SIDEBAR: f32 = 224.;
const SIDEBAR_COLLAPSED: f32 = 60.;
// The Library gets this long to paint first, as in the Electron app.
const WHATS_NEW_DELAY: Duration = Duration::from_millis(400);

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
    Import(LauncherId),
    NewAccount(LauncherId),
    // `created`: the page a Create account just opened, which says what comes next.
    Account {
        id: String,
        created: bool,
    },
    // The tab named, or the one last shown in this run.
    Settings(Option<SettingsTab>),
    Play {
        game: Box<Game>,
        account: Option<String>,
    },
}

pub(crate) struct Go {
    pub route: Route,
    pub guarded: bool,
    // Only the move that carries it has it, so it lasts one hop.
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
    // Restart to update, asked from a page with unsaved edits.
    Restart(RestartFrom),
    // A quit while a switch runs; its first answer keeps the app running.
    Quit,
    // "Quit anyway" was answered: no answer is left, only the line that the switch is being stopped.
    Stopping,
}

impl Asked {
    // A question about quitting during a switch stays until it is answered, whatever page a finished import or delete moves to.
    const fn gives_way_to_a_finished_move(&self) -> bool {
        matches!(self, Self::Leave { .. })
    }
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
    appeared: Appeared,
    had_keyboard: Option<FocusHandle>,
}

// The user answered Quit anyway: the app is to quit.
pub struct QuitAnyway;

// The backend took Restart to update: the app is to quit for real, and the installer starts at its end.
pub struct RestartToUpdate;

// Where Restart to update was pressed, which is where a refusal is told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RestartFrom {
    Card,
    Settings,
}

// Shared with the Settings page: whether a restart is under way, and whether a switch or launch runs, either of which keeps Restart to update from being pressed.
#[derive(Default)]
pub(crate) struct RestartGate {
    restarting: Cell<bool>,
    running: Cell<bool>,
}

impl RestartGate {
    pub fn restarting(&self) -> bool {
        self.restarting.get()
    }

    pub fn running(&self) -> bool {
        self.running.get()
    }

    pub fn shut(&self) -> bool {
        self.restarting() || self.running()
    }
}

// The Update ready card's title and second line, and whether that line is a refusal.
fn card_lines(
    version: &str,
    restarting: bool,
    refusal: Option<&str>,
) -> (&'static str, String, bool) {
    let title = if restarting {
        "Restarting…"
    } else {
        "Update ready"
    };
    match refusal {
        Some(message) if !restarting => (title, format!("Could not restart: {message}"), true),
        _ => (title, format!("Restart to install {version}"), false),
    }
}

fn card_tip(version: &str, running: bool) -> String {
    if running {
        format!(
            "One Launch {version} is ready. Restart to install it once the switch has finished."
        )
    } else {
        format!("One Launch {version} is ready. Restart to install it.")
    }
}

// A job the app followed has ended: the window may go to the tray.
pub struct Played {
    pub job: LaunchJob,
}

enum Welcome {
    Guide {
        guide: Entity<SetupGuide>,
        _done: Subscription,
    },
    WhatsNew {
        card: Entity<WhatsNewCard>,
        _done: Subscription,
    },
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
    developer: bool,
    welcome: Option<Welcome>,
    settings_tab: Rc<Cell<SettingsTab>>,
    launcher_statuses: KnownStatuses,
    preview: bool,
    update: Option<UpdateStatus>,
    gate: Rc<RestartGate>,
    card_refusal: Option<SharedString>,
    _page: Option<Subscription>,
    _keys: Subscription,
    _host: Subscription,
    _played: Subscription,
    _library: Subscription,
    _running: Subscription,
}

impl EventEmitter<QuitAnyway> for Shell {}

impl EventEmitter<RestartToUpdate> for Shell {}

impl EventEmitter<Played> for Shell {}

pub fn sidebar_width(open: bool) -> Pixels {
    px(if open { SIDEBAR } else { SIDEBAR_COLLAPSED })
}

impl Shell {
    pub fn new(
        backend: Backend,
        fake_launchers: bool,
        developer: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Decided apart, as by the Electron app's two hosts.
        cx.spawn_in(window, {
            let backend = backend.clone();
            async move |this, cx| {
                let (Ok(settings), Ok(accounts)) =
                    (backend.settings().await, backend.accounts().await)
                else {
                    return;
                };
                if shows_setup_guide(&settings, accounts.len()) {
                    this.update_in(cx, |this, window, cx| this.open_guide(window, cx))
                        .ok();
                }
                let recovery = matches!(
                    backend.recovery().await,
                    Ok(Some(found)) if backend.can_reach(found.launcher)
                );
                let version = backend.app_info().version;
                match at_start(&settings, &version, recovery) {
                    AtStart::Nothing => {}
                    AtStart::RecordSeen => {
                        let _ = backend
                            .set_setting(SettingKey::WhatsNewLastSeen, version)
                            .await;
                    }
                    AtStart::Show(release) => {
                        cx.background_executor().timer(WHATS_NEW_DELAY).await;
                        this.update_in(cx, |this, window, cx| {
                            if this.welcome.is_none() {
                                this.open_whats_new(release, window, cx);
                            }
                        })
                        .ok();
                    }
                }
            }
        })
        .detach();
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
        // The sidebar's Update ready card follows the updater for as long as the window lives.
        cx.spawn({
            let changes = backend.update_changes();
            async move |this, cx| {
                while let Ok(status) = changes.recv().await {
                    let heard = this.update(cx, |this, cx| this.update_heard(status, cx));
                    if heard.is_err() {
                        break;
                    }
                }
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
        let running = cx.observe(&host, |this: &mut Self, host, cx| {
            let running = host.read(cx).job_running();
            if this.gate.running.replace(running) != running {
                cx.notify();
            }
        });
        let played = cx.subscribe(&host, |_, _, ended: &PlayEnded, cx| {
            cx.emit(Played {
                job: ended.job.clone(),
            });
        });
        let library = cx.new(|cx| LibraryView::new(backend.clone(), host.clone(), window, cx));
        let artwork_settings = cx.subscribe_in(
            &library,
            window,
            |this, _, _: &OpenArtworkSettings, window, cx| {
                this.open_settings_on(SettingsTab::Artwork, window, cx);
            },
        );
        Self {
            library,
            backend,
            host,
            page: Page::Library,
            asking: None,
            sidebar_open: false,
            toggled: false,
            fake_launchers,
            developer,
            welcome: None,
            settings_tab: Rc::default(),
            launcher_statuses: KnownStatuses::default(),
            preview: false,
            update: None,
            gate: Rc::default(),
            card_refusal: None,
            _page: None,
            _keys: keys,
            _host: from_host,
            _played: played,
            _library: artwork_settings,
            _running: running,
        }
    }

    fn update_heard(&mut self, status: UpdateStatus, cx: &mut Context<Self>) {
        if !matches!(status, UpdateStatus::Ready { .. }) {
            self.card_refusal = None;
        }
        self.update = Some(status);
        cx.notify();
    }

    // As the Electron app's restartToUpdate: the page with unsaved edits has its say first, then the backend is asked; the app quits once it has said yes.
    fn restart_to_update(
        &mut self,
        from: RestartFrom,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.gate.shut() || self.stopping() {
            return;
        }
        if from == RestartFrom::Card {
            self.card_refusal = None;
        }
        self.gate.restarting.set(true);
        cx.notify();
        if let Some(question) = self.question(cx) {
            self.asking = Some(Asking {
                title: question.title.into(),
                message: question.message.into(),
                detail: None,
                keep: "Stay",
                act: "Leave",
                asked: Asked::Restart(from),
                act_leads: false,
                appeared: Appeared::now(),
                had_keyboard: window.focused(cx),
            });
            window.blur(cx);
            return;
        }
        self.install_update(from, cx);
    }

    fn install_update(&mut self, from: RestartFrom, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let installed = backend.install_update().await;
            this.update(cx, |this, cx| match installed {
                Ok(()) => cx.emit(RestartToUpdate),
                Err(error) => this.restart_refused(from, error.message.into(), cx),
            })
            .ok();
        })
        .detach();
    }

    fn restart_refused(
        &mut self,
        from: RestartFrom,
        message: SharedString,
        cx: &mut Context<Self>,
    ) {
        self.gate.restarting.set(false);
        match from {
            RestartFrom::Card => self.card_refusal = Some(message),
            RestartFrom::Settings => {
                if let Page::Settings(page) = &self.page {
                    page.update(cx, |page, cx| page.restart_refused(&message, cx));
                }
            }
        }
        cx.notify();
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

    pub fn close_finished_job(&mut self, job_id: &str, cx: &mut Context<Self>) {
        self.host
            .update(cx, |host, cx| host.close_finished_job(job_id, cx));
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
        if matches!(
            self.asking,
            Some(Asking {
                asked: Asked::Restart(_),
                ..
            })
        ) {
            self.gate.restarting.set(false);
        }
        self.backend.window_log().quit_asked(&question.job_id);
        self.asking = Some(Asking {
            title: question.title.into(),
            message: question.message.into(),
            detail: Some(question.detail.into()),
            keep: question.keep,
            act: question.quit,
            asked: Asked::Quit,
            act_leads: false,
            appeared: Appeared::now(),
            had_keyboard: window.focused(cx),
        });
        window.blur(cx);
        cx.notify();
    }

    fn open_preview_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_settings_on(SettingsTab::Advanced, window, cx);
    }

    // Settings on one of its tabs, from anywhere: the sidebar's Preview mode card, the Library's link to the artwork key.
    fn open_settings_on(&mut self, tab: SettingsTab, window: &mut Window, cx: &mut Context<Self>) {
        if let Page::Settings(page) = &self.page {
            page.update(cx, |page, cx| page.select_tab(tab, cx));
            return;
        }
        self.go(Go::to(Route::Settings(Some(tab))), window, cx);
    }

    pub fn library(&self) -> &Entity<LibraryView> {
        &self.library
    }

    fn open_guide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.welcome.is_some() {
            return;
        }
        let (backend, library) = (self.backend.clone(), self.library.clone());
        let guide = cx.new(|cx| SetupGuide::new(backend, library, cx));
        let done = cx.subscribe_in(&guide, window, |this, _, done: &GuideDone, window, cx| {
            this.welcome = None;
            let route = match done.0 {
                GuideExit::Stay => None,
                GuideExit::Library => Some(Route::Library),
                GuideExit::Accounts => Some(Route::Accounts),
                GuideExit::NewAccount(launcher) => Some(Route::NewAccount(launcher)),
            };
            if let Some(route) = route {
                this.go(Go::to(route), window, cx);
            }
            cx.notify();
        });
        self.welcome = Some(Welcome::Guide { guide, _done: done });
        window.blur(cx);
        cx.notify();
    }

    fn open_whats_new(
        &mut self,
        release: &'static Release,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.welcome.is_some() {
            return;
        }
        let (backend, version) = (self.backend.clone(), self.backend.app_info().version);
        let card = cx.new(|cx| WhatsNewCard::new(backend, release, version, cx));
        let done = cx.subscribe_in(&card, window, |this, card, _: &WhatsNewDone, window, cx| {
            this.welcome = None;
            if let Some(picture) = card.update(cx, |card, _| card.take_picture()) {
                cx.drop_image(picture, Some(window));
            }
            cx.notify();
        });
        self.welcome = Some(Welcome::WhatsNew { card, _done: done });
        window.blur(cx);
        cx.notify();
    }

    fn on_settings_ask(
        &mut self,
        _: &Entity<SettingsView>,
        ask: &SettingsAsk,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match ask {
            SettingsAsk::SetupGuide => self.open_guide(window, cx),
            SettingsAsk::WhatsNew(release) => self.open_whats_new(release, window, cx),
            SettingsAsk::RestartToUpdate => {
                self.restart_to_update(RestartFrom::Settings, window, cx);
            }
        }
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
        if let Some(asking) = &self.asking {
            if go.guarded {
                return;
            }
            if asking.asked.gives_way_to_a_finished_move() {
                self.asking = None;
            }
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
                appeared: Appeared::now(),
                had_keyboard: window.focused(cx),
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
            Asked::Restart(from) if act => self.install_update(from, cx),
            Asked::Quit if act => {
                self.backend.window_log().quit_answered(QuitAnswer::Quit);
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
                    appeared: Appeared::now(),
                    had_keyboard: None,
                });
                cx.emit(QuitAnyway);
            }
            other => {
                if matches!(other, Asked::Quit) {
                    self.backend.window_log().quit_answered(QuitAnswer::Keep);
                }
                if matches!(other, Asked::Restart(_)) {
                    self.gate.restarting.set(false);
                }
                if let Some(field) = &asking.had_keyboard {
                    window.focus(field, cx);
                }
            }
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
        if let Some(asking) = &mut self.asking {
            asking.had_keyboard = None;
        }
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
                let known = Rc::clone(&self.launcher_statuses);
                let gate = Rc::clone(&self.gate);
                let developer = self.developer;
                let page = cx
                    .new(|cx| SettingsView::new(backend, tab, known, gate, developer, window, cx));
                self._page = Some(cx.subscribe_in(&page, window, Self::on_settings_ask));
                Page::Settings(page)
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
                .update(cx, |host, cx| host.open(game, account, Ask::ListOrJob, cx));
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
        self.asking.is_some()
            || self.welcome.is_some()
            || self.host.read(cx).is_modal()
            || self.importing(cx)
            || self.restoring(cx)
    }

    fn importing(&self, cx: &Context<Self>) -> bool {
        matches!(&self.page, Page::Accounts(page) if page.read(cx).importing())
    }

    fn restoring(&self, cx: &Context<Self>) -> bool {
        matches!(&self.page, Page::Settings(page) if page.read(cx).restoring())
    }

    fn typing(&self, window: &Window, cx: &Context<Self>) -> bool {
        match &self.page {
            Page::Library => self.library.read(cx).typing(window, cx),
            Page::Account(page) => page.read(cx).typing(window, cx),
            Page::Settings(page) => page.read(cx).typing(window, cx),
            Page::Accounts(_) => false,
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
        if self.host.read(cx).recovery_up() {
            return self
                .host
                .update(cx, |host, cx| host.on_keystroke(keystroke, cx));
        }
        match &self.welcome {
            Some(Welcome::Guide { guide, .. }) => {
                return guide.update(cx, |guide, cx| guide.on_key(keystroke, cx));
            }
            Some(Welcome::WhatsNew { card, .. }) => {
                return card.update(cx, |card, cx| card.on_key(keystroke, cx));
            }
            None => {}
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
        if let Page::Settings(page) = &self.page
            && page.read(cx).restoring()
        {
            if held.alt || held.platform {
                return false;
            }
            page.update(cx, |page, cx| page.on_restore_key(keystroke, cx));
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
        self.on_page_key(keystroke, window, cx)
    }

    fn on_page_key(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match &self.page {
            Page::Accounts(page) => page.update(cx, |page, cx| page.on_key(keystroke, cx)),
            Page::Account(page) => page.update(cx, |page, cx| page.on_key(keystroke, window, cx)),
            Page::Settings(page) => page.update(cx, |page, cx| page.on_key(keystroke, window, cx)),
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
            .child(
                div()
                    .mt_auto()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .px(px(8.))
                    .pb(px(12.))
                    .children(self.preview.then(|| self.render_preview_card(open, cx)))
                    .children(self.render_update_card(open, cx))
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

    // The Electron app's Update ready card: shown while a verified update waits, and never dismissed.
    fn render_update_card(&self, open: bool, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let Some(UpdateStatus::Ready { version }) = &self.update else {
            return None;
        };
        let shut = self.gate.shut();
        let (title, line, refused) = card_lines(
            version,
            self.gate.restarting(),
            self.card_refusal.as_deref(),
        );
        Some(
            div()
                .id("update-card")
                .flex()
                .items_start()
                .gap(px(8.))
                .rounded(px(8.))
                .border_1()
                .px(px(11.))
                .py(px(8.))
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(color(theme::TEXT_STRONG))
                .whitespace_nowrap()
                .map(|card| {
                    if open {
                        card.border_color(alpha(theme::ACCENT, 0.3))
                            .bg(alpha(theme::ACCENT, 0.16))
                            .when(!shut, |card| {
                                card.hover(|style| style.border_color(alpha(theme::ACCENT, 0.5)))
                            })
                    } else {
                        card.border_color(gpui::transparent_black())
                            .when(!shut, |card| {
                                card.hover(|style| style.bg(alpha(theme::ACCENT, 0.16)))
                            })
                    }
                })
                .when(shut, |card| card.opacity(0.6))
                .when(!shut, |card| {
                    card.cursor_pointer()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.restart_to_update(RestartFrom::Card, window, cx);
                        }))
                })
                .tooltip(tip(card_tip(version, self.gate.running())))
                .child(icon(assets::UPDATE_READY, 14., theme::ACCENT_HOVER).mt(px(2.)))
                .when(open, |card| {
                    card.child(
                        div()
                            .w(px(164.))
                            .flex_none()
                            .whitespace_normal()
                            .child(div().font_weight(FontWeight::MEDIUM).child(title))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .line_height(px(15.))
                                    .text_color(color(if refused {
                                        theme::DANGER
                                    } else {
                                        theme::MUTED
                                    }))
                                    .child(line),
                            ),
                    )
                }),
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
                                    if this
                                        .asking
                                        .as_ref()
                                        .is_some_and(|asking| asking.appeared.takes(click))
                                    {
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

    fn render_welcome(&self, window: &Window, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let card = match self.welcome.as_ref()? {
            Welcome::Guide { guide, .. } => guide.update(cx, |guide, cx| guide.render_card(cx)),
            Welcome::WhatsNew { card, .. } => card.update(cx, |card, cx| card.render_card(cx)),
        };
        Some(overlay(window.viewport_size(), 3, card))
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
            .children(self.render_welcome(window, cx))
            .children(
                self.render_question(cx)
                    .map(|question| overlay(window.viewport_size(), 5, question)),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_update_card_says_what_the_electron_card_says() {
        assert_eq!(
            card_lines("1.0.0-beta.4", false, None),
            (
                "Update ready",
                "Restart to install 1.0.0-beta.4".to_owned(),
                false
            )
        );
        assert_eq!(
            card_lines("1.0.0-beta.4", true, None),
            (
                "Restarting…",
                "Restart to install 1.0.0-beta.4".to_owned(),
                false
            )
        );
        assert_eq!(
            card_lines(
                "1.0.0-beta.4",
                false,
                Some("Another switch or launch is still running")
            ),
            (
                "Update ready",
                "Could not restart: Another switch or launch is still running".to_owned(),
                true
            )
        );
        assert_eq!(
            card_tip("1.0.0-beta.4", false),
            "One Launch 1.0.0-beta.4 is ready. Restart to install it."
        );
        assert_eq!(
            card_tip("1.0.0-beta.4", true),
            "One Launch 1.0.0-beta.4 is ready. Restart to install it once the switch has finished."
        );
    }

    #[test]
    fn a_restart_is_kept_from_being_pressed_while_one_is_under_way_or_a_switch_runs() {
        let gate = RestartGate::default();
        assert!(!gate.shut());
        gate.running.set(true);
        assert!(gate.shut() && !gate.restarting());
        gate.running.set(false);
        gate.restarting.set(true);
        assert!(gate.shut() && !gate.running());
    }

    #[test]
    fn only_a_leave_question_gives_way_to_a_finished_move() {
        let leave = Asked::Leave {
            then: Route::Library,
            return_to: None,
        };
        assert!(leave.gives_way_to_a_finished_move());
        assert!(!Asked::Quit.gives_way_to_a_finished_move());
        assert!(!Asked::Restart(RestartFrom::Card).gives_way_to_a_finished_move());
        assert!(!Asked::Stopping.gives_way_to_a_finished_move());
    }
}
