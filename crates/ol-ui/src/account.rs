mod tags;

use std::time::{Duration, SystemTime};

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Div, EventEmitter, FontWeight, Keystroke, SharedString, Subscription,
    Window, div, px,
};
use ol_api::accounts::{AccountPatch, NewAccount, Secret};
use ol_api::library::{date_time, relative};
use ol_api::{ApiError, Backend};
use ol_model::{Account, LauncherId};
use ol_ui_kit::theme::{self, color};
use ol_ui_kit::{FieldEvent, TextArea, TextField};

use crate::assets;
use crate::local_offset;
use crate::page::{PageScroll, scroll_page};
use crate::shell::{Go, Question, Route};
use crate::widgets::{Button, Flash, FlashTone, Tone, field, icon, launcher_badge, panel};

const NAME_MOST: usize = 120;
const LOGIN_MOST: usize = 256;
const UID_MOST: usize = 64;
const PASSWORD_MOST: usize = 1024;
const NOTES_MOST: usize = 20_000;
const NOTES_ROWS: usize = 5;
const NOTES_MOST_ROWS: usize = 12;
const CREATED_FLASH_STAYS: Duration = Duration::from_secs(8);
const PASSWORD_LINE: &str = "Optional. Switching does not need it; it is only there so you can copy it when a launcher asks you to sign in. Stored encrypted on this PC.";

#[derive(Clone, PartialEq, Eq)]
enum Kind {
    Create,
    Edit { id: String },
}

#[derive(Clone, Default, PartialEq, Eq)]
struct Draft {
    name: String,
    login: String,
    uid: String,
    tags: Vec<String>,
    notes: String,
}

impl Draft {
    fn of(account: &Account) -> Self {
        Self {
            name: account.display_name.clone(),
            login: account.login.clone(),
            uid: account.launcher_uid.clone().unwrap_or_default(),
            tags: account.tags.clone(),
            notes: account.notes.clone(),
        }
    }
}

#[derive(Default)]
struct Errors {
    name: Option<SharedString>,
    login: Option<SharedString>,
    notes: Option<SharedString>,
}

impl Errors {
    fn any(&self) -> bool {
        self.name.is_some() || self.login.is_some() || self.notes.is_some()
    }
}

fn blank(text: &str) -> bool {
    text.trim_matches(|letter: char| letter.is_whitespace() || letter == '\u{feff}')
        .is_empty()
}

fn validate(draft: &Draft) -> Errors {
    Errors {
        name: blank(&draft.name).then(|| "Display name is required.".into()),
        login: blank(&draft.login).then(|| "Login is required.".into()),
        notes: (draft.notes.encode_utf16().count() > NOTES_MOST)
            .then(|| format!("Notes must be at most {NOTES_MOST} characters.").into()),
    }
}

fn first_play(launcher: LauncherId) -> &'static str {
    match launcher {
        LauncherId::Steam => {
            "First Play: Steam asks you to sign in once; keep \u{201c}Remember me\u{201d} ticked."
        }
        LauncherId::Epic => "First Play: Epic Games Launcher asks you to sign in once.",
        LauncherId::Battlenet => {
            "First Play: Battle.net asks you to sign in once; leave \u{201c}Keep me logged in\u{201d} on."
        }
        LauncherId::Riot => {
            "First Play: the Riot Client asks you to sign in once; tick \u{201c}Stay signed in\u{201d}."
        }
    }
}

fn example(launcher: LauncherId) -> (&'static str, &'static str) {
    match launcher {
        LauncherId::Steam => (
            "CS2 alt",
            "Prime \u{b7} owns Rust and CS2 \u{b7} Family Sharing on",
        ),
        LauncherId::Epic => (
            "Fortnite main",
            "Chapter 1 skins \u{b7} free games claimed every week",
        ),
        LauncherId::Battlenet => (
            "OW Smurf #12",
            "Gold 3 \u{b7} bought 2024-03 \u{b7} has the OW1 skins",
        ),
        LauncherId::Riot => (
            "VALORANT alt",
            "Plat 2 \u{b7} EU West \u{b7} has the Reaver bundle",
        ),
    }
}

fn login_hint(launcher: LauncherId) -> &'static str {
    match launcher {
        LauncherId::Steam => "Your Steam account name, not your profile name.",
        LauncherId::Epic => "The email you sign in to Epic Games with.",
        LauncherId::Riot => "The username you sign in to the Riot Client with.",
        LauncherId::Battlenet => "The email you sign in to Battle.net with.",
    }
}

fn login_placeholder(launcher: LauncherId) -> &'static str {
    match launcher {
        LauncherId::Steam => "account name",
        LauncherId::Riot => "username",
        LauncherId::Epic | LauncherId::Battlenet => "you@example.com",
    }
}

fn uid_label(launcher: LauncherId) -> &'static str {
    match launcher {
        LauncherId::Steam => "SteamID64",
        LauncherId::Epic => "Epic account id",
        LauncherId::Riot => "Riot account id",
        LauncherId::Battlenet => "Battle.net account id",
    }
}

fn uid_hint(launcher: LauncherId) -> &'static str {
    match launcher {
        LauncherId::Steam => {
            "Filled in when you import the account from Steam. Only needed if Steam\u{2019}s remembered name and this account disagree."
        }
        _ => {
            "Filled in by itself the first time One Launch sees this account signed in. You normally never type it."
        }
    }
}

fn uid_placeholder(launcher: LauncherId) -> &'static str {
    match launcher {
        LauncherId::Steam => "7656119\u{2026}",
        LauncherId::Epic => "32 hex characters",
        LauncherId::Riot => "36 characters, with dashes",
        LauncherId::Battlenet => "8 hex characters",
    }
}

pub struct AccountPage {
    backend: Backend,
    kind: Kind,
    launcher: Option<LauncherId>,
    account: Option<Account>,
    load_error: Option<SharedString>,
    name: TextField,
    login: TextField,
    uid: TextField,
    notes: TextArea,
    tags: Vec<String>,
    tag_field: TextField,
    // Only the form that adds an account has one: an account's page has a panel of its own for it, not built yet.
    password: Option<TextField>,
    show_password: bool,
    baseline: Draft,
    touched: bool,
    busy: bool,
    error: Option<SharedString>,
    flash: Option<Flash>,
    advanced: bool,
    confirm_delete: bool,
    scroll: PageScroll,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Go> for AccountPage {}

impl AccountPage {
    pub fn create(
        backend: Backend,
        launcher: LauncherId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut page = Self::new(backend, Kind::Create, window, cx);
        let password = TextField::new("", true, window, cx).limited_to(PASSWORD_MOST, cx);
        page._subscriptions
            .push(password.on_event_in(window, cx, |_, _, _, cx| cx.notify()));
        page.password = Some(password);
        page.set_launcher(launcher, window, cx);
        page.name.focus(window, cx);
        page
    }

    pub fn edit(
        backend: Backend,
        id: String,
        created: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.spawn_in(window, {
            let (backend, id) = (backend.clone(), id.clone());
            async move |this, cx| {
                let account = backend.account(&id).await;
                this.update_in(cx, |this, window, cx| {
                    this.loaded(account, created, window, cx);
                })
                .ok();
            }
        })
        .detach();
        Self::new(backend, Kind::Edit { id }, window, cx)
    }

    fn new(backend: Backend, kind: Kind, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = TextField::new("", false, window, cx).limited_to(NAME_MOST, cx);
        let login = TextField::new("", false, window, cx).limited_to(LOGIN_MOST, cx);
        let uid = TextField::new("", false, window, cx).limited_to(UID_MOST, cx);
        let tag_field =
            TextField::new("Add a tag", false, window, cx).limited_to(tags::LONGEST, cx);
        let notes = TextArea::new("", NOTES_ROWS, NOTES_MOST_ROWS, window, cx);
        let changed = |_: &mut Self, _: FieldEvent, _: &mut Window, cx: &mut Context<Self>| {
            cx.notify();
        };
        let subscriptions = vec![
            name.on_event_in(window, cx, changed),
            login.on_event_in(window, cx, changed),
            uid.on_event_in(window, cx, changed),
            notes.on_event_in(window, cx, changed),
            tag_field.on_event_in(window, cx, Self::on_tag_field),
        ];
        Self {
            backend,
            kind,
            launcher: None,
            account: None,
            load_error: None,
            name,
            login,
            uid,
            notes,
            tags: Vec::new(),
            tag_field,
            password: None,
            show_password: false,
            baseline: Draft::default(),
            touched: false,
            busy: false,
            error: None,
            flash: None,
            advanced: false,
            confirm_delete: false,
            scroll: PageScroll::default(),
            _subscriptions: subscriptions,
        }
    }

    fn on_tag_field(&mut self, event: FieldEvent, window: &mut Window, cx: &mut Context<Self>) {
        let pending = self.tag_field.text(cx);
        match event {
            FieldEvent::Enter => self.commit_tag(window, cx),
            FieldEvent::Changed if pending.contains(',') => self.commit_tag(window, cx),
            FieldEvent::Blurred if !blank(&pending) => self.commit_tag(window, cx),
            FieldEvent::Changed | FieldEvent::Blurred => cx.notify(),
        }
    }

    fn set_launcher(&mut self, launcher: LauncherId, window: &mut Window, cx: &mut Context<Self>) {
        self.launcher = Some(launcher);
        let (name, notes) = example(launcher);
        self.name.set_placeholder(name, window, cx);
        self.login
            .set_placeholder(login_placeholder(launcher), window, cx);
        self.uid
            .set_placeholder(uid_placeholder(launcher), window, cx);
        self.notes.set_placeholder(notes, window, cx);
    }

    fn loaded(
        &mut self,
        account: Result<Account, ApiError>,
        created: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match account {
            Ok(account) => {
                self.set_launcher(account.launcher, window, cx);
                self.show(&account, window, cx);
                self.account = Some(account);
                if created {
                    self.flash = Some(Flash::new(
                        "Account created. Pick a game in the Library and press Play.",
                        FlashTone::Success,
                        Some(CREATED_FLASH_STAYS),
                        |this: &mut Self| &mut this.flash,
                        cx,
                    ));
                }
            }
            Err(error) => self.load_error = Some(error.message.into()),
        }
        cx.notify();
    }

    fn show(&mut self, account: &Account, window: &mut Window, cx: &mut Context<Self>) {
        let draft = Draft::of(account);
        self.fill(&draft, window, cx);
        self.baseline = draft;
    }

    fn fill(&mut self, draft: &Draft, window: &mut Window, cx: &mut Context<Self>) {
        self.name.set_text(draft.name.clone(), window, cx);
        self.login.set_text(draft.login.clone(), window, cx);
        self.uid.set_text(draft.uid.clone(), window, cx);
        self.notes.set_text(draft.notes.clone(), window, cx);
        self.tags.clone_from(&draft.tags);
        self.tags_changed(window, cx);
    }

    fn draft(&self, cx: &App) -> Draft {
        Draft {
            name: self.name.text(cx).to_string(),
            login: self.login.text(cx).to_string(),
            uid: self.uid.text(cx).to_string(),
            tags: self.tags.clone(),
            notes: self.notes.text(cx).to_string(),
        }
    }

    fn typed_password(&self, cx: &App) -> bool {
        self.password
            .as_ref()
            .is_some_and(|password| !password.text(cx).is_empty())
    }

    fn dirty(&self, cx: &App) -> bool {
        match self.kind {
            Kind::Create => self.draft(cx) != Draft::default() || self.typed_password(cx),
            Kind::Edit { .. } => self.account.is_some() && self.draft(cx) != self.baseline,
        }
    }

    // The errors shown: on the form that adds an account after the first try, on an account's page while it is changed.
    fn errors(&self, cx: &App) -> Errors {
        let shown = match self.kind {
            Kind::Create => self.touched,
            Kind::Edit { .. } => self.dirty(cx),
        };
        if shown {
            validate(&self.draft(cx))
        } else {
            Errors::default()
        }
    }

    pub fn leave_question(&self, cx: &App) -> Option<Question> {
        match self.kind {
            // While the account is being made nothing is lost by leaving, as in the Electron app.
            Kind::Create if self.busy => None,
            Kind::Create if self.dirty(cx) => Some(Question {
                title: "Leave without creating this account?",
                message: "What you typed will be lost.",
            }),
            Kind::Edit { .. } if self.dirty(cx) => Some(Question {
                title: "Leave without saving?",
                message: "Your changes to this account will be lost.",
            }),
            _ => None,
        }
    }

    pub fn typing(&self, window: &Window, cx: &App) -> bool {
        self.name.is_focused(window, cx)
            || self.login.is_focused(window, cx)
            || self.tag_field.is_focused(window, cx)
            || self.notes.is_focused(window, cx)
            || self.uid.is_focused(window, cx)
            || self
                .password
                .as_ref()
                .is_some_and(|password| password.is_focused(window, cx))
    }

    fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        self.busy = busy;
        for field in [&self.name, &self.login, &self.uid] {
            field.set_disabled(busy, cx);
        }
        if let Some(password) = &self.password {
            password.set_disabled(busy, cx);
        }
        self.notes.set_disabled(busy, cx);
        self.tag_field
            .set_disabled(busy || self.tags.len() >= tags::MOST, cx);
        cx.notify();
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        cx.emit(Go {
            route: Route::Accounts,
            guarded: true,
        });
    }

    fn create_account(&mut self, cx: &mut Context<Self>) {
        let Some(launcher) = self.launcher else {
            return;
        };
        if self.busy {
            return;
        }
        self.touched = true;
        let draft = self.draft(cx);
        if validate(&draft).any() {
            return cx.notify();
        }
        let password = self
            .password
            .as_ref()
            .map(|field| field.text(cx))
            .filter(|password| !password.is_empty())
            .map(|password| Secret::new(password.to_string()));
        let account = NewAccount {
            launcher,
            display_name: draft.name,
            login: draft.login,
            launcher_uid: Some(draft.uid).filter(|uid| !uid.is_empty()),
            password,
            tags: tags::normalize(draft.tags),
            notes: draft.notes,
        };
        self.error = None;
        self.set_busy(true, cx);
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let made = backend.create_account(account).await;
            this.update(cx, |this, cx| match made {
                Ok(account) => cx.emit(Go {
                    route: Route::Account {
                        id: account.id,
                        created: true,
                    },
                    guarded: false,
                }),
                Err(error) => {
                    this.error = Some(error.message.into());
                    this.set_busy(false, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Kind::Edit { id } = &self.kind else {
            return;
        };
        let id = id.clone();
        let draft = self.draft(cx);
        if self.account.is_none() || !self.dirty(cx) || self.busy || validate(&draft).any() {
            return;
        }
        let was = &self.baseline;
        let patch = AccountPatch {
            display_name: (draft.name != was.name).then(|| draft.name.clone()),
            login: (draft.login != was.login).then(|| draft.login.clone()),
            launcher_uid: (draft.uid != was.uid)
                .then(|| Some(draft.uid.clone()).filter(|uid| !uid.is_empty())),
            tags: (draft.tags != was.tags).then(|| draft.tags.clone()),
            notes: (draft.notes != was.notes).then(|| draft.notes.clone()),
        };
        self.error = None;
        self.set_busy(true, cx);
        let backend = self.backend.clone();
        cx.spawn_in(window, async move |this, cx| {
            let saved = backend.update_account(&id, patch).await;
            this.update_in(cx, |this, window, cx| this.saved(saved, window, cx))
                .ok();
        })
        .detach();
    }

    fn saved(
        &mut self,
        saved: Result<Account, ApiError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_busy(false, cx);
        match saved {
            Ok(account) => {
                self.show(&account, window, cx);
                self.account = Some(account);
                self.flash = Some(Flash::new(
                    "Saved",
                    FlashTone::Success,
                    None,
                    |this: &mut Self| &mut this.flash,
                    cx,
                ));
            }
            Err(error) => self.error = Some(error.message.into()),
        }
        cx.notify();
    }

    fn discard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let baseline = self.baseline.clone();
        self.fill(&baseline, window, cx);
        self.tag_field.set_text("", window, cx);
        cx.notify();
    }

    fn delete(&mut self, cx: &mut Context<Self>) {
        let Kind::Edit { id } = &self.kind else {
            return;
        };
        if self.busy {
            return;
        }
        let (backend, id) = (self.backend.clone(), id.clone());
        self.set_busy(true, cx);
        cx.spawn(async move |this, cx| {
            let deleted = backend.delete_account(&id).await;
            this.update(cx, |this, cx| match deleted {
                Ok(()) => cx.emit(Go {
                    route: Route::Accounts,
                    guarded: false,
                }),
                Err(error) => {
                    this.error = Some(error.message.into());
                    this.set_busy(false, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn stops(&self) -> Vec<Stop<'_>> {
        let mut stops = vec![
            Stop::Line(&self.name),
            Stop::Line(&self.login),
            Stop::Line(&self.tag_field),
            Stop::Lines(&self.notes),
        ];
        stops.extend(self.password.as_ref().map(Stop::Line));
        if self.advanced {
            stops.push(Stop::Line(&self.uid));
        }
        stops
    }

    pub fn on_key(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let held = keystroke.modifiers;
        let plain = !held.control && !held.alt && !held.platform && !held.function;
        if !plain {
            return false;
        }
        let in_tags = self.tag_field.is_focused(window, cx);
        match keystroke.key.as_str() {
            "tab" if in_tags && !held.shift && !blank(&self.tag_field.text(cx)) => {
                self.commit_tag(window, cx);
                true
            }
            "backspace"
                if in_tags
                    && !held.shift
                    && self.tag_field.text(cx).is_empty()
                    && !self.tags.is_empty() =>
            {
                let last = self.tags.len() - 1;
                self.remove_tag(last, window, cx);
                true
            }
            "tab" => {
                let stops = self.stops();
                let Some(at) = stops.iter().position(|stop| stop.is_focused(window, cx)) else {
                    return false;
                };
                let next = if held.shift {
                    (at + stops.len() - 1) % stops.len()
                } else {
                    (at + 1) % stops.len()
                };
                let next = stops[next].clone();
                next.focus(window, cx);
                self.scroll.reveal(next.bounds(cx));
                cx.notify();
                true
            }
            _ => false,
        }
    }

    fn render_header(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let launcher = self.launcher;
        let title: SharedString = match (&self.kind, &self.account, launcher) {
            (Kind::Edit { .. }, Some(account), _) => account.display_name.clone().into(),
            (Kind::Create, _, Some(launcher)) => {
                format!("New {} account", launcher.display_name()).into()
            }
            _ => SharedString::default(),
        };
        let line = self.account.as_ref().map(|account| {
            let now = SystemTime::now();
            let used = account
                .last_used_at
                .as_deref()
                .filter(|at| !at.is_empty())
                .map_or_else(
                    || "Never used from One Launch".to_owned(),
                    |at| format!("Last used {}", relative(at, now).unwrap_or_default()),
                );
            let added = date_time(&account.created_at, local_offset(cx)).unwrap_or_default();
            div()
                .mt(px(2.))
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(color(theme::MUTED))
                .child(format!("{used} \u{b7} Added {added}"))
        });
        let left = div()
            .min_w_0()
            .child(
                div().mb(px(4.)).ml(px(-12.)).flex().child(
                    Button::new("back", Tone::Ghost, "Accounts")
                        .glyph(assets::BACK)
                        .text(theme::TEXT)
                        .on_click(cx.listener(|this, _, _, cx| this.back(cx))),
                ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(20.))
                            .line_height(px(28.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(color(theme::TEXT_STRONG))
                            .child(title),
                    )
                    .children(launcher.map(launcher_badge)),
            )
            .children(line);
        let error = self.error.clone().map(|error| {
            div()
                .min_w_0()
                .max_w(px(420.))
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(color(theme::DANGER))
                .child(error)
        });
        let busy = self.busy;
        let mut actions: Vec<AnyElement> = Vec::new();
        match self.kind {
            Kind::Create => {
                actions.push(
                    Button::new("cancel", Tone::Ghost, "Cancel")
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, _, cx| this.back(cx))),
                );
                actions.push(
                    Button::new(
                        "create",
                        Tone::Primary,
                        if busy {
                            "Creating\u{2026}"
                        } else {
                            "Create account"
                        },
                    )
                    .glyph(assets::CHECK)
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, _, cx| this.create_account(cx))),
                );
            }
            Kind::Edit { .. } => {
                let dirty = self.dirty(cx);
                if dirty {
                    actions.push(
                        Button::new("discard", Tone::Ghost, "Discard")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, window, cx| this.discard(window, cx))),
                    );
                }
                let invalid = self.errors(cx).any();
                actions.push(
                    Button::new(
                        "save",
                        Tone::Primary,
                        if busy {
                            "Saving\u{2026}"
                        } else {
                            "Save changes"
                        },
                    )
                    .glyph(assets::CHECK)
                    .disabled(!dirty || busy || invalid)
                    .tip(if dirty {
                        "Save changes (Ctrl+S)"
                    } else {
                        "No unsaved changes"
                    })
                    .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                );
            }
        }
        let _ = window;
        div()
            .flex()
            .items_end()
            .justify_between()
            .gap(px(16.))
            .pt(px(28.))
            .pb(px(20.))
            .child(left)
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(8.))
                    .children(self.flash.as_ref().map(Flash::render))
                    .children(error)
                    .children(actions),
            )
    }

    fn render_form(&self, launcher: LauncherId, window: &Window, cx: &mut Context<Self>) -> Div {
        let errors = self.errors(cx);
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .gap(px(16.))
                    .child(
                        field(
                            "Display name",
                            Some("How this account is shown in One Launch.".into()),
                            errors.name,
                            self.name.form_frame("name", None, window, cx),
                        )
                        .flex_1(),
                    )
                    .child(
                        field(
                            "Login",
                            Some(login_hint(launcher).into()),
                            errors.login,
                            self.login.form_frame("login", None, window, cx),
                        )
                        .flex_1(),
                    ),
            )
            .child(field(
                "Tags",
                Some("Press Enter to add one. Rank, role, region, anything.".into()),
                None,
                self.render_tags(window, cx),
            ))
            .child(field(
                "Notes",
                Some("Anything you want to remember about this account.".into()),
                errors.notes,
                self.notes.frame("notes", window, cx),
            ))
    }

    fn render_password(&self, window: &Window, cx: &mut Context<Self>) -> Option<Div> {
        let password = self.password.as_ref()?;
        let shown = self.show_password;
        let eye = div()
            .id("eye")
            .group("eye")
            .flex_none()
            .p(px(4.))
            .mr(px(-4.))
            .rounded(px(4.))
            .cursor_pointer()
            .tooltip(crate::widgets::tip(if shown {
                "Hide password"
            } else {
                "Show password"
            }))
            .on_click(cx.listener(|this, _, window, cx| {
                this.show_password = !this.show_password;
                if let Some(password) = &this.password {
                    password.set_masked(!this.show_password, window, cx);
                }
                cx.notify();
            }))
            .child(
                icon(
                    if shown { assets::HIDE } else { assets::EYE },
                    16.,
                    theme::MUTED,
                )
                .group_hover("eye", |style| style.text_color(color(theme::TEXT))),
            );
        Some(panel(
            "Password",
            Some(PASSWORD_LINE.into()),
            None,
            field(
                "Password",
                None,
                None,
                password.form_frame("password", Some(eye.into_any_element()), window, cx),
            ),
        ))
    }

    fn render_advanced(
        &self,
        launcher: LauncherId,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let open = self.advanced;
        div()
            .child(
                div()
                    .id("advanced")
                    .group("advanced")
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(8.))
                    .px(px(4.))
                    .py(px(8.))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.advanced = !this.advanced;
                        cx.notify();
                    }))
                    .child(icon(
                        if open {
                            assets::CHEVRON_DOWN
                        } else {
                            assets::CHEVRON_RIGHT
                        },
                        16.,
                        theme::FAINT,
                    ))
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(color(theme::TEXT))
                            .group_hover("advanced", |style| {
                                style.text_color(color(theme::TEXT_STRONG))
                            })
                            .child("Advanced"),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color(theme::FAINT))
                            .child("Launcher account id"),
                    ),
            )
            .when(open, |section| {
                section.child(
                    div().mt(px(8.)).child(panel(
                        "Launcher account id",
                        None,
                        None,
                        field(
                            uid_label(launcher),
                            Some(uid_hint(launcher).into()),
                            None,
                            self.uid
                                .form_frame("uid", None, window, cx)
                                .font_family(theme::MONO),
                        ),
                    )),
                )
            })
    }

    fn render_danger(&self, cx: &mut Context<Self>) -> Option<Div> {
        let account = self.account.as_ref()?;
        let busy = self.busy;
        let buttons: Vec<AnyElement> = if self.confirm_delete {
            vec![
                Button::new(
                    "delete-yes",
                    Tone::Danger,
                    format!("Yes, delete \u{201c}{}\u{201d}", account.display_name),
                )
                .glyph(assets::TRASH)
                .disabled(busy)
                .on_click(cx.listener(|this, _, _, cx| this.delete(cx))),
                Button::new("delete-cancel", Tone::Ghost, "Cancel")
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.confirm_delete = false;
                        cx.notify();
                    })),
            ]
        } else {
            vec![
                Button::new("delete", Tone::Danger, "Delete account")
                    .glyph(assets::TRASH)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.confirm_delete = true;
                        cx.notify();
                    })),
            ]
        };
        Some(panel(
            "Danger zone",
            Some("Deleting removes the account, its password, and its saved sign-in from One Launch only. Nothing changes in the launcher.".into()),
            None,
            div().flex().items_center().gap(px(8.)).children(buttons),
        ))
    }
}

#[derive(Clone)]
enum Stop<'a> {
    Line(&'a TextField),
    Lines(&'a TextArea),
}

impl Stop<'_> {
    fn is_focused(&self, window: &Window, cx: &gpui::App) -> bool {
        match self {
            Self::Line(field) => field.is_focused(window, cx),
            Self::Lines(field) => field.is_focused(window, cx),
        }
    }

    fn bounds(&self, cx: &App) -> gpui::Bounds<gpui::Pixels> {
        match self {
            Self::Line(field) => field.bounds(cx),
            Self::Lines(field) => field.bounds(cx),
        }
    }

    fn focus(&self, window: &mut Window, cx: &mut gpui::App) {
        match self {
            Self::Line(field) => field.focus(window, cx),
            Self::Lines(field) => field.focus(window, cx),
        }
    }
}

impl Render for AccountPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(error) = &self.load_error {
            let header = div().pt(px(28.)).mb(px(16.)).ml(px(-12.)).flex().child(
                Button::new("back", Tone::Ghost, "Accounts")
                    .glyph(assets::BACK)
                    .text(theme::TEXT)
                    .on_click(cx.listener(|this, _, _, cx| this.back(cx))),
            );
            let body = div()
                .text_color(color(theme::DANGER))
                .child(format!("Could not load this account: {error}"));
            return scroll_page("account-page", &self.scroll, header, body).into_any_element();
        }
        let Some(launcher) = self.launcher else {
            return div().into_any_element();
        };
        let details = panel(
            "Details",
            matches!(self.kind, Kind::Create).then(|| first_play(launcher).into()),
            None,
            self.render_form(launcher, window, cx),
        );
        let body = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .pb(px(24.))
            .child(details)
            .children(self.render_password(window, cx))
            .child(self.render_advanced(launcher, window, cx))
            .children(self.render_danger(cx));
        scroll_page(
            "account-page",
            &self.scroll,
            self.render_header(window, cx),
            body,
        )
        .into_any_element()
    }
}
