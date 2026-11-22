mod import;

use std::time::{Duration, SystemTime};

use gpui::prelude::*;
use gpui::{
    Anchor, AnyElement, Context, Div, EventEmitter, FontWeight, Keystroke, SharedString, Stateful,
    Window, anchored, deferred, div, point, px,
};
use ol_api::launch::in_pick_order;
use ol_api::library::relative;
use ol_api::{ApiError, Backend};
use ol_model::{Account, Game, LauncherId};
use ol_ui_kit::theme::{self, alpha, color};

use crate::assets;
use crate::host::{can_import, overlay};
use crate::page::{PageScroll, scroll_page};
use crate::shell::{Go, Route};
use crate::widgets::{
    Button, Flash, FlashTone, Tone, icon, launcher_dot, menu_chevron, menu_shadow, page_title, tip,
};

use import::Import;

const MENU_GAP: f32 = 6.;
const IMPORTED_FLASH_STAYS: Duration = Duration::from_secs(4);

const IMPORT_ITEMS: [(LauncherId, &str, &str); 4] = [
    (LauncherId::Steam, "From Steam", "Logins Steam has saved"),
    (
        LauncherId::Battlenet,
        "From Battle.net",
        "Logins Battle.net has saved",
    ),
    (
        LauncherId::Epic,
        "Epic Games: add by hand",
        "Epic keeps no account list",
    ),
    (
        LauncherId::Riot,
        "Riot Games: add by hand",
        "The Riot Client keeps no account list",
    ),
];

pub struct AccountsView {
    backend: Backend,
    load: Load,
    reads: u64,
    menu: Option<OpenMenu>,
    over_trigger: Option<MenuSpot>,
    scroll: PageScroll,
    import: Option<Import>,
    imports: u64,
    return_to: Option<Game>,
    flash: Option<Flash>,
}

enum Load {
    Pending,
    Failed(SharedString),
    Ready(Vec<Account>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuKind {
    Import,
    Add,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuAt {
    Header,
    Empty,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct MenuSpot {
    kind: MenuKind,
    at: MenuAt,
}

impl MenuSpot {
    fn id(self) -> &'static str {
        match (self.kind, self.at) {
            (MenuKind::Import, MenuAt::Header) => "import-account",
            (MenuKind::Import, MenuAt::Empty) => "import-account-empty",
            (MenuKind::Add, MenuAt::Header) => "add-account",
            (MenuKind::Add, MenuAt::Empty) => "add-account-empty",
        }
    }
}

struct OpenMenu {
    spot: MenuSpot,
    highlight: Option<usize>,
}

impl EventEmitter<Go> for AccountsView {}

// One Launch can switch to it without asking the user to sign in again.
fn ready(account: &Account) -> bool {
    account.has_session || account.last_used_at.is_some()
}

fn note_preview(notes: &str) -> Option<&str> {
    notes.lines().map(str::trim).find(|line| !line.is_empty())
}

impl AccountsView {
    pub fn new(
        backend: Backend,
        import: Option<LauncherId>,
        return_to: Option<Game>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self {
            backend,
            load: Load::Pending,
            reads: 0,
            menu: None,
            over_trigger: None,
            scroll: PageScroll::default(),
            import: None,
            imports: 0,
            return_to,
            flash: None,
        };
        view.reload(cx);
        if let Some(launcher) = import.filter(|launcher| can_import(*launcher)) {
            view.open_import(launcher, cx);
        }
        view
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        self.reads += 1;
        let (read, backend) = (self.reads, self.backend.clone());
        cx.spawn(async move |this, cx| {
            let accounts = backend.accounts().await;
            this.update(cx, |this, cx| {
                if this.reads == read {
                    this.loaded(accounts, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn loaded(&mut self, accounts: Result<Vec<Account>, ApiError>, cx: &mut Context<Self>) {
        self.load = match accounts {
            Ok(accounts) => Load::Ready(accounts),
            Err(error) => Load::Failed(error.message.into()),
        };
        cx.notify();
    }

    pub fn importing(&self) -> bool {
        self.import.is_some()
    }

    fn say(
        &mut self,
        text: String,
        tone: FlashTone,
        stays: Option<Duration>,
        cx: &mut Context<Self>,
    ) {
        self.flash = Some(Flash::new(
            text,
            tone,
            stays,
            |this: &mut Self| &mut this.flash,
            cx,
        ));
        cx.notify();
    }

    fn toggle_menu(&mut self, spot: MenuSpot, cx: &mut Context<Self>) {
        let open = self.menu.as_ref().is_some_and(|menu| menu.spot == spot);
        self.menu = (!open).then_some(OpenMenu {
            spot,
            highlight: None,
        });
        cx.notify();
    }

    fn pick(&mut self, kind: MenuKind, option: usize, cx: &mut Context<Self>) {
        self.menu = None;
        cx.notify();
        let launcher = match kind {
            MenuKind::Add => LauncherId::ALL.get(option).copied(),
            MenuKind::Import => IMPORT_ITEMS.get(option).map(|(launcher, _, _)| *launcher),
        };
        let Some(launcher) = launcher else {
            return;
        };
        if kind == MenuKind::Import && can_import(launcher) {
            self.open_import(launcher, cx);
        } else {
            cx.emit(Go::to(Route::NewAccount(launcher)));
        }
    }

    pub fn on_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let held = keystroke.modifiers;
        if held.control || held.alt || held.platform || held.function {
            return false;
        }
        let Some(menu) = self.menu.as_mut() else {
            return false;
        };
        let last = match menu.spot.kind {
            MenuKind::Add => LauncherId::ALL.len() - 1,
            MenuKind::Import => IMPORT_ITEMS.len() - 1,
        };
        match keystroke.key.as_str() {
            "down" => menu.highlight = Some(menu.highlight.map_or(0, |at| (at + 1) % (last + 1))),
            "up" => {
                menu.highlight = Some(menu.highlight.map_or(last, |at| (at + last) % (last + 1)))
            }
            "home" => menu.highlight = Some(0),
            "end" => menu.highlight = Some(last),
            "enter" | "space" => {
                if let Some(option) = menu.highlight {
                    let kind = menu.spot.kind;
                    self.pick(kind, option, cx);
                }
            }
            "escape" | "tab" => self.menu = None,
            _ => return false,
        }
        cx.notify();
        true
    }

    fn open(&mut self, account: &Account, cx: &mut Context<Self>) {
        cx.emit(Go::to(Route::Account {
            id: account.id.clone(),
            created: false,
        }));
    }

    fn render_menu_button(
        &self,
        spot: MenuSpot,
        tone: Tone,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        let open = self.menu.as_ref().filter(|menu| menu.spot == spot);
        let (hex, opacity) = if tone == Tone::Primary {
            (theme::TEXT_STRONG, 0.7)
        } else {
            (theme::FAINT, 1.)
        };
        let glyph = match spot.kind {
            MenuKind::Import => assets::DOWNLOAD,
            MenuKind::Add => assets::PLUS,
        };
        let trigger = div()
            .id(spot.id())
            .on_hover(cx.listener(move |this, over: &bool, _, _| {
                if *over {
                    this.over_trigger = Some(spot);
                } else if this.over_trigger == Some(spot) {
                    this.over_trigger = None;
                }
            }))
            .child(
                Button::new("trigger", tone, label)
                    .glyph(glyph)
                    .trailing(menu_chevron(open.is_some(), hex, opacity))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_menu(spot, cx))),
            );
        div()
            .relative()
            .flex_none()
            .child(trigger)
            .children(open.map(|open| {
                let (anchor, place) = match spot.at {
                    MenuAt::Header => (Anchor::TopRight, div().absolute().top_full().right_0()),
                    MenuAt::Empty => (Anchor::TopLeft, div().absolute().top_full().left_0()),
                };
                place.child(
                    deferred(
                        anchored()
                            .anchor(anchor)
                            .offset(point(px(0.), px(MENU_GAP)))
                            .child(self.render_menu(spot, open.highlight, cx)),
                    )
                    .with_priority(1),
                )
            }))
    }

    fn render_menu(
        &self,
        spot: MenuSpot,
        highlight: Option<usize>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let items: Vec<(LauncherId, &'static str, Option<&'static str>)> = match spot.kind {
            MenuKind::Add => LauncherId::ALL
                .into_iter()
                .map(|launcher| (launcher, launcher.display_name(), None))
                .collect(),
            MenuKind::Import => IMPORT_ITEMS
                .into_iter()
                .map(|(launcher, label, hint)| (launcher, label, Some(hint)))
                .collect(),
        };
        let kind = spot.kind;
        div()
            .id("account-menu")
            .occlude()
            .flex()
            .flex_col()
            .min_w(px(192.))
            .p(px(4.))
            .rounded(px(8.))
            .border_1()
            .border_color(color(theme::BORDER))
            .bg(color(theme::SURFACE_2))
            .shadow(menu_shadow())
            .on_mouse_down_out(cx.listener(move |this, _, _, cx| {
                // A press on the button that opened the menu is that button's to answer: it closes it.
                if this.over_trigger != Some(spot) {
                    this.menu = None;
                    cx.notify();
                }
            }))
            .children(
                items
                    .into_iter()
                    .enumerate()
                    .map(|(option, (launcher, label, hint))| {
                        let lit = highlight == Some(option);
                        div()
                            .id(("launcher", option))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .py(px(6.))
                            .pl(px(10.))
                            .pr(px(12.))
                            .rounded(px(6.))
                            .whitespace_nowrap()
                            .cursor_pointer()
                            .text_color(color(if lit { theme::TEXT_STRONG } else { theme::TEXT }))
                            .when(lit, |row| row.bg(color(theme::SURFACE_3)))
                            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                                if let Some(menu) = this.menu.as_mut()
                                    && menu.highlight != Some(option)
                                {
                                    menu.highlight = Some(option);
                                    cx.notify();
                                }
                            }))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.pick(kind, option, cx)),
                            )
                            .child(launcher_dot(launcher))
                            .child(div().min_w_0().child(label).children(hint.map(|hint| {
                                div()
                                    .text_size(px(11.))
                                    .line_height(px(15.))
                                    .text_color(color(theme::FAINT))
                                    .child(hint)
                            })))
                    }),
            )
    }

    fn render_header(&self, cx: &mut Context<Self>) -> Div {
        let header = MenuAt::Header;
        div()
            .flex()
            .flex_wrap()
            .items_end()
            .justify_between()
            .gap_x(px(16.))
            .gap_y(px(12.))
            .pt(px(28.))
            .pb(px(20.))
            .child(div().flex_1().min_w(px(288.)).child(page_title(
                "Accounts",
                Some("The accounts you switch between, grouped by launcher."),
            )))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.))
                    .children(self.flash.as_ref().map(Flash::render))
                    .child(self.render_menu_button(
                        MenuSpot {
                            kind: MenuKind::Import,
                            at: header,
                        },
                        Tone::Secondary,
                        "Import",
                        cx,
                    ))
                    .child(self.render_menu_button(
                        MenuSpot {
                            kind: MenuKind::Add,
                            at: header,
                        },
                        Tone::Primary,
                        "Add account",
                        cx,
                    )),
            )
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> Div {
        let empty = MenuAt::Empty;
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .min_h(px(420.))
            .text_center()
            .child(
                div()
                    .mb(px(16.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(64.))
                    .rounded(px(16.))
                    .border_1()
                    .border_color(color(theme::BORDER))
                    .bg(color(theme::SURFACE))
                    .child(icon(assets::KEY, 28., theme::MUTED)),
            )
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(24.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(color(theme::TEXT_STRONG))
                    .child("No accounts yet"),
            )
            .child(
                div()
                    .mt(px(4.))
                    .max_w(px(384.))
                    .text_color(color(theme::MUTED))
                    .child("Add the accounts you use on each launcher, or import the ones Steam and Battle.net already know. Passwords are optional and stay on this PC."),
            )
            .child(
                div()
                    .mt(px(20.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(self.render_menu_button(
                        MenuSpot {
                            kind: MenuKind::Import,
                            at: empty,
                        },
                        Tone::Primary,
                        "Import accounts",
                        cx,
                    ))
                    .child(self.render_menu_button(
                        MenuSpot {
                            kind: MenuKind::Add,
                            at: empty,
                        },
                        Tone::Secondary,
                        "Add account",
                        cx,
                    )),
            )
    }

    fn render_groups(&self, accounts: &[Account], cx: &mut Context<Self>) -> Div {
        let now = SystemTime::now();
        div().flex().flex_col().gap(px(24.)).pb(px(24.)).children(
            LauncherId::ALL.into_iter().filter_map(|launcher| {
                let group = in_pick_order(
                    accounts
                        .iter()
                        .filter(|account| account.launcher == launcher)
                        .cloned()
                        .collect(),
                );
                if group.is_empty() {
                    return None;
                }
                // Each group's rows count from nought, so the group's id keeps a row's apart from those of the other groups.
                Some(
                    div()
                        .id(launcher.display_name())
                        .child(
                            div()
                                .mb(px(8.))
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .child(launcher_dot(launcher))
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(color(theme::MUTED))
                                        .child(launcher.display_name().to_uppercase()),
                                )
                                .child(
                                    div()
                                        .text_color(color(theme::FAINT))
                                        .child(group.len().to_string()),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .overflow_hidden()
                                .rounded(px(12.))
                                .border_1()
                                .border_color(color(theme::BORDER))
                                .bg(color(theme::SURFACE))
                                .children(group.iter().enumerate().map(|(index, account)| {
                                    self.render_row(index, account, now, cx)
                                })),
                        ),
                )
            }),
        )
    }

    fn render_row(
        &self,
        index: usize,
        account: &Account,
        now: SystemTime,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let ready = ready(account);
        let used = account.last_used_at.as_deref().filter(|at| !at.is_empty());
        let status = match used {
            Some(at) => format!("Used {}", relative(at, now).unwrap_or_default()),
            None if ready => "Sign-in saved".to_owned(),
            None => "Not used yet".to_owned(),
        };
        let badge = div()
            .id(("badge", index))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(36.))
            .rounded(px(8.))
            .border_1()
            .map(|badge| {
                if ready {
                    badge
                        .border_color(alpha(theme::ACCENT, 0.3))
                        .bg(alpha(theme::ACCENT, 0.16))
                        .tooltip(tip("Ready: switches without asking you to sign in"))
                        .child(icon(assets::KEY, 16., theme::ACCENT))
                } else {
                    badge
                        .border_color(color(theme::BORDER))
                        .bg(color(theme::SURFACE_2))
                        .tooltip(tip("The first switch will ask you to sign in once"))
                        .child(icon(assets::PERSON, 16., theme::FAINT))
                }
            });
        let tags = account.tags.iter().map(|tag| {
            div()
                .flex_none()
                .rounded(px(6.))
                .bg(color(theme::SURFACE_3))
                .px(px(6.))
                .py(px(2.))
                .text_size(px(11.))
                .line_height(px(15.))
                .text_color(color(theme::MUTED))
                .child(tag.clone())
        });
        let note = note_preview(&account.notes).map(|line| {
            [
                div()
                    .flex_none()
                    .text_color(color(theme::FAINT))
                    .child("\u{b7}")
                    .into_any_element(),
                div()
                    .flex()
                    .min_w_0()
                    .items_center()
                    .gap(px(4.))
                    .text_color(color(theme::FAINT))
                    .child(icon(assets::NOTE, 12., theme::FAINT))
                    .child(div().min_w_0().truncate().child(line.to_owned()))
                    .into_any_element(),
            ]
        });
        let opened = account.clone();
        div()
            .id(("account", index))
            .group("account-row")
            .flex()
            .items_center()
            .gap(px(16.))
            .px(px(16.))
            .py(px(12.))
            .cursor_pointer()
            .when(index > 0, |row| {
                row.border_t_1().border_color(color(theme::BORDER))
            })
            .hover(|style| style.bg(alpha(theme::SURFACE_2, 0.6)))
            .on_click(cx.listener(move |this, _, _, cx| this.open(&opened, cx)))
            .child(badge)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(color(theme::TEXT_STRONG))
                                    .child(account.display_name.clone()),
                            )
                            .children(tags),
                    )
                    .child(
                        div()
                            .mt(px(2.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color(theme::MUTED))
                            .child(div().min_w_0().truncate().child(account.login.clone()))
                            .children(note.into_iter().flatten()),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .items_end()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color(theme::FAINT))
                    .child(
                        div()
                            .when(used.is_some(), |line| line.text_color(color(theme::MUTED)))
                            .child(status),
                    )
                    .children(account.has_secret.then(|| {
                        div()
                            .id(("password", index))
                            .text_size(px(11.))
                            .line_height(px(15.))
                            .tooltip(tip("A password is stored for the Copy button"))
                            .child("Password stored")
                    })),
            )
            .child(
                icon(assets::CHEVRON_RIGHT, 16., theme::FAINT)
                    .group_hover("account-row", |style| style.text_color(color(theme::MUTED))),
            )
    }
}

impl Render for AccountsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body: AnyElement = match &self.load {
            Load::Pending => div().into_any_element(),
            Load::Failed(message) => div()
                .text_color(color(theme::DANGER))
                .child(format!("Could not load accounts: {message}"))
                .into_any_element(),
            Load::Ready(accounts) if accounts.is_empty() => {
                self.render_empty(cx).into_any_element()
            }
            Load::Ready(accounts) => {
                let accounts = accounts.clone();
                self.render_groups(&accounts, cx).into_any_element()
            }
        };
        let viewport = window.viewport_size();
        let dialog = self
            .render_import(cx)
            .map(|dialog| overlay(viewport, 2, dialog));
        scroll_page("accounts-page", &self.scroll, self.render_header(cx), body).children(dialog)
    }
}
