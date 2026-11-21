use std::time::SystemTime;

use gpui::prelude::*;
use gpui::{
    Anchor, AnyElement, Context, Div, EventEmitter, FontWeight, Keystroke, SharedString, Stateful,
    Window, anchored, deferred, div, point, px,
};
use ol_api::launch::in_pick_order;
use ol_api::library::relative;
use ol_api::{ApiError, Backend};
use ol_model::{Account, LauncherId};
use ol_ui_kit::theme::{self, alpha, color};

use crate::assets;
use crate::page::{PageScroll, scroll_page};
use crate::shell::{Go, Route};
use crate::widgets::{
    Button, Tone, icon, launcher_dot, menu_chevron, menu_shadow, page_title, tip,
};

const MENU_GAP: f32 = 6.;

pub struct AccountsView {
    load: Load,
    menu: Option<OpenMenu>,
    over_trigger: Option<MenuAt>,
    scroll: PageScroll,
}

enum Load {
    Pending,
    Failed(SharedString),
    Ready(Vec<Account>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuAt {
    Header,
    Empty,
}

struct OpenMenu {
    at: MenuAt,
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
    pub fn new(backend: Backend, cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |this, cx| {
            let accounts = backend.accounts().await;
            this.update(cx, |this, cx| this.loaded(accounts, cx)).ok();
        })
        .detach();
        Self {
            load: Load::Pending,
            menu: None,
            over_trigger: None,
            scroll: PageScroll::default(),
        }
    }

    fn loaded(&mut self, accounts: Result<Vec<Account>, ApiError>, cx: &mut Context<Self>) {
        self.load = match accounts {
            Ok(accounts) => Load::Ready(accounts),
            Err(error) => Load::Failed(error.message.into()),
        };
        cx.notify();
    }

    fn toggle_menu(&mut self, at: MenuAt, cx: &mut Context<Self>) {
        let open = self.menu.as_ref().is_some_and(|menu| menu.at == at);
        self.menu = (!open).then_some(OpenMenu {
            at,
            highlight: None,
        });
        cx.notify();
    }

    fn pick(&mut self, option: usize, cx: &mut Context<Self>) {
        self.menu = None;
        cx.notify();
        if let Some(launcher) = LauncherId::ALL.get(option).copied() {
            cx.emit(Go {
                route: Route::NewAccount(launcher),
                guarded: true,
            });
        }
    }

    pub fn on_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(menu) = self.menu.as_mut() else {
            return false;
        };
        let last = LauncherId::ALL.len() - 1;
        match keystroke.key.as_str() {
            "down" => menu.highlight = Some(menu.highlight.map_or(0, |at| (at + 1) % (last + 1))),
            "up" => {
                menu.highlight = Some(menu.highlight.map_or(last, |at| (at + last) % (last + 1)))
            }
            "home" => menu.highlight = Some(0),
            "end" => menu.highlight = Some(last),
            "enter" | "space" => {
                if let Some(option) = menu.highlight {
                    self.pick(option, cx);
                }
            }
            "escape" | "tab" => self.menu = None,
            _ => return false,
        }
        cx.notify();
        true
    }

    fn open(&mut self, account: &Account, cx: &mut Context<Self>) {
        cx.emit(Go {
            route: Route::Account {
                id: account.id.clone(),
                created: false,
            },
            guarded: true,
        });
    }

    fn render_add_menu(
        &self,
        at: MenuAt,
        tone: Tone,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        let open = self.menu.as_ref().filter(|menu| menu.at == at);
        let (hex, opacity) = if tone == Tone::Primary {
            (theme::TEXT_STRONG, 0.7)
        } else {
            (theme::FAINT, 1.)
        };
        let id = match at {
            MenuAt::Header => "add-account",
            MenuAt::Empty => "add-account-empty",
        };
        let trigger = div()
            .id(id)
            .on_hover(cx.listener(move |this, over: &bool, _, _| {
                if *over {
                    this.over_trigger = Some(at);
                } else if this.over_trigger == Some(at) {
                    this.over_trigger = None;
                }
            }))
            .child(
                Button::new("trigger", tone, label)
                    .glyph(assets::PLUS)
                    .trailing(menu_chevron(open.is_some(), hex, opacity))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_menu(at, cx))),
            );
        div()
            .relative()
            .flex_none()
            .child(trigger)
            .children(open.map(|open| {
                let (anchor, place) = match at {
                    MenuAt::Header => (Anchor::TopRight, div().absolute().top_full().right_0()),
                    MenuAt::Empty => (Anchor::TopLeft, div().absolute().top_full().left_0()),
                };
                place.child(
                    deferred(
                        anchored()
                            .anchor(anchor)
                            .offset(point(px(0.), px(MENU_GAP)))
                            .child(self.render_menu(at, open.highlight, cx)),
                    )
                    .with_priority(1),
                )
            }))
    }

    fn render_menu(
        &self,
        at: MenuAt,
        highlight: Option<usize>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        div()
            .id("add-menu")
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
                if this.over_trigger != Some(at) {
                    this.menu = None;
                    cx.notify();
                }
            }))
            .children(
                LauncherId::ALL
                    .into_iter()
                    .enumerate()
                    .map(|(option, launcher)| {
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
                            .on_click(cx.listener(move |this, _, _, cx| this.pick(option, cx)))
                            .child(launcher_dot(launcher))
                            .child(launcher.display_name())
                    }),
            )
    }

    fn render_header(&self, cx: &mut Context<Self>) -> Div {
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
                    .child(self.render_add_menu(MenuAt::Header, Tone::Primary, "Add account", cx)),
            )
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> Div {
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
                    .child(self.render_add_menu(MenuAt::Empty, Tone::Secondary, "Add account", cx)),
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        scroll_page("accounts-page", &self.scroll, self.render_header(cx), body)
    }
}
