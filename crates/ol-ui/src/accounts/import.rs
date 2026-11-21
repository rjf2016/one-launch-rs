// The import dialog: the accounts a launcher remembers, ticked to be made One Launch accounts, names and ids only.

use std::time::SystemTime;

use gpui::prelude::*;
use gpui::{
    ClickEvent, Context, Div, FontWeight, Keystroke, ScrollHandle, SharedString, Stateful, div, px,
};
use ol_api::ApiError;
use ol_api::launchers::ImportResult;
use ol_api::library::relative;
use ol_model::{LauncherId, RememberedAccount};
use ol_ui_kit::theme::{self, alpha, color};

use super::{AccountsView, IMPORTED_FLASH_STAYS};
use crate::assets;
use crate::shell::{Go, Route};
use crate::widgets::{Button, FlashTone, PillTone, Tone, icon, panel_shadow, pill};

pub(super) struct Import {
    key: u64,
    launcher: LauncherId,
    remembered: Remembered,
    // The keys of the entries ticked, in the order the launcher lists them.
    selected: Vec<String>,
    busy: bool,
    error: Option<SharedString>,
    body: ScrollHandle,
}

enum Remembered {
    Loading,
    Failed(SharedString),
    Ready(Vec<RememberedAccount>),
}

struct Copy {
    title: &'static str,
    subtitle: &'static str,
    reading: &'static str,
    empty: &'static str,
}

fn copy(launcher: LauncherId) -> Copy {
    match launcher {
        LauncherId::Battlenet => Copy {
            title: "Import Battle.net accounts",
            subtitle: "Logins in the client's saved-accounts list (Battle.net.config). Only the logins are imported; the account id is learned at the first confirmed sign-in.",
            reading: "Reading Battle.net.config\u{2026}",
            empty: "Battle.net has no saved account names on this PC yet.",
        },
        _ => Copy {
            title: "Import Steam accounts",
            subtitle: "Accounts Steam remembers on this PC. Only names and SteamIDs are imported.",
            reading: "Reading loginusers.vdf\u{2026}",
            empty: "Steam has not remembered any account on this PC yet.",
        },
    }
}

// What names an entry: the launcher's id for the account, or its login where the launcher records none.
fn key_of(entry: &RememberedAccount) -> &str {
    if entry.launcher_uid.is_empty() {
        &entry.login
    } else {
        &entry.launcher_uid
    }
}

impl Import {
    fn entries(&self) -> &[RememberedAccount] {
        match &self.remembered {
            Remembered::Ready(entries) => entries,
            Remembered::Loading | Remembered::Failed(_) => &[],
        }
    }

    fn importable(&self) -> usize {
        self.entries()
            .iter()
            .filter(|entry| entry.linked_account_id.is_none())
            .count()
    }

    fn loaded(&self) -> bool {
        !matches!(self.remembered, Remembered::Loading)
    }

    fn import_leads(&self) -> bool {
        !self.selected.is_empty()
    }
}

impl AccountsView {
    pub(super) fn open_import(&mut self, launcher: LauncherId, cx: &mut Context<Self>) {
        self.menu = None;
        self.imports += 1;
        let key = self.imports;
        self.import = Some(Import {
            key,
            launcher,
            remembered: Remembered::Loading,
            selected: Vec::new(),
            busy: false,
            error: None,
            body: ScrollHandle::new(),
        });
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let remembered = backend.remembered_accounts(launcher).await;
            this.update(cx, |this, cx| this.remembered(key, remembered, cx))
                .ok();
        })
        .detach();
        cx.notify();
    }

    fn remembered(
        &mut self,
        key: u64,
        remembered: Result<Vec<RememberedAccount>, ApiError>,
        cx: &mut Context<Self>,
    ) {
        let Some(import) = self.import.as_mut().filter(|import| import.key == key) else {
            return;
        };
        match remembered {
            Ok(entries) => {
                import.selected = entries
                    .iter()
                    .filter(|entry| entry.linked_account_id.is_none())
                    .map(|entry| key_of(entry).to_owned())
                    .collect();
                import.remembered = Remembered::Ready(entries);
            }
            Err(error) => import.remembered = Remembered::Failed(error.message.into()),
        }
        cx.notify();
    }

    fn toggle(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(import) = self.import.as_mut() else {
            return;
        };
        let Some(entry) = import.entries().get(index) else {
            return;
        };
        if entry.linked_account_id.is_some() {
            return;
        }
        let key = key_of(entry).to_owned();
        if import.selected.contains(&key) {
            import.selected.retain(|selected| *selected != key);
        } else {
            let order: Vec<String> = import
                .entries()
                .iter()
                .map(|entry| key_of(entry).to_owned())
                .filter(|entry| *entry == key || import.selected.contains(entry))
                .collect();
            import.selected = order;
        }
        cx.notify();
    }

    // A second press, or Enter with a click, finds the import under way and does nothing.
    fn run_import(&mut self, cx: &mut Context<Self>) {
        let Some(import) = self.import.as_mut() else {
            return;
        };
        if import.busy || import.selected.is_empty() {
            return;
        }
        import.busy = true;
        import.error = None;
        let (key, launcher, keys) = (import.key, import.launcher, import.selected.clone());
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let result = backend.import_accounts(launcher, keys).await;
            this.update(cx, |this, cx| this.imported(key, launcher, result, cx))
                .ok();
        })
        .detach();
        cx.notify();
    }

    // The answer counts whether or not the dialog is still up: it may have been closed while the import ran, as the Electron app allows.
    fn imported(
        &mut self,
        key: u64,
        launcher: LauncherId,
        result: Result<ImportResult, ApiError>,
        cx: &mut Context<Self>,
    ) {
        let open = self.import.as_mut().filter(|import| import.key == key);
        let imported = match result {
            Ok(result) => result.accounts,
            Err(error) => {
                match open {
                    Some(import) => {
                        import.busy = false;
                        import.error = Some(error.message.into());
                        cx.notify();
                    }
                    None => self.say(error.message, FlashTone::Error, None, cx),
                }
                return;
            }
        };
        if open.is_some() {
            self.import = None;
        }
        // Sent here by a game's Play: back to that game, with the one imported account picked, or the picker when there are several.
        let returns = self.return_to.as_ref().is_some_and(|game| {
            imported
                .iter()
                .any(|account| account.launcher == game.launcher)
        });
        if returns && let Some(game) = self.return_to.take() {
            let account = match imported.as_slice() {
                [only] => Some(only.id.clone()),
                _ => None,
            };
            cx.emit(Go::done(Route::Play {
                game: Box::new(game),
                account,
            }));
            return;
        }
        match imported.len() {
            0 => self.say(
                "Nothing new to import".to_owned(),
                FlashTone::Info,
                None,
                cx,
            ),
            count => self.say(
                format!(
                    "Imported {count} account{} from {}",
                    if count == 1 { "" } else { "s" },
                    launcher.display_name()
                ),
                FlashTone::Success,
                Some(IMPORTED_FLASH_STAYS),
                cx,
            ),
        }
        self.reload(cx);
    }

    // As in the Electron app, also while an import runs: its answer still comes to this page.
    fn close_import(&mut self, cx: &mut Context<Self>) {
        if self.import.take().is_some() {
            cx.notify();
        }
    }

    // Every key is the dialog's while it is up; the shell lets those held with Alt or the Windows key go on to Windows.
    pub fn on_import_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        let held = keystroke.modifiers;
        let plain = !held.control && !held.alt && !held.shift && !held.platform && !held.function;
        if !plain {
            return;
        }
        match keystroke.key.as_str() {
            "escape" => self.close_import(cx),
            "enter" => {
                let Some(import) = &self.import else {
                    return;
                };
                if !import.loaded() || import.busy {
                    return;
                }
                if import.import_leads() {
                    self.run_import(cx);
                } else {
                    self.close_import(cx);
                }
            }
            _ => {}
        }
    }

    fn render_entry(
        &self,
        index: usize,
        entry: &RememberedAccount,
        checked: bool,
        last: bool,
        now: SystemTime,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let linked = entry.linked_account_id.is_some();
        let name = if entry.persona_name.is_empty() {
            entry.login.clone()
        } else {
            entry.persona_name.clone()
        };
        let mut pills = Vec::new();
        if entry.most_recent {
            pills.push(pill(PillTone::Ok, None, "Signed in"));
        }
        if linked {
            pills.push(pill(PillTone::Info, None, "Already in One Launch"));
        }
        if !entry.remember_password {
            pills.push(pill(PillTone::Warn, None, "Remember me off"));
        } else if entry.session_may_be_expired {
            pills.push(pill(PillTone::Warn, None, "Session may have expired"));
        }
        let dot = || {
            div()
                .flex_none()
                .text_color(color(theme::FAINT))
                .child("\u{b7}")
        };
        let mut details = vec![div().min_w_0().truncate().child(entry.login.clone())];
        if entry.launcher_uid.is_empty() {
            details.push(dot());
            details.push(
                div()
                    .flex_none()
                    .font_family(theme::MONO)
                    .text_color(color(theme::FAINT))
                    .child(entry.launcher_uid.clone()),
            );
        }
        if let Some(when) = entry
            .last_login_at
            .as_deref()
            .and_then(|at| relative(at, now))
        {
            details.push(dot());
            details.push(
                div()
                    .flex_none()
                    .text_color(color(theme::FAINT))
                    .child(format!("last sign-in {when}")),
            );
        }
        let checkbox = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(16.))
            .rounded(px(3.))
            .border_1()
            .map(|checkbox| {
                if checked {
                    checkbox
                        .border_color(color(theme::ACCENT))
                        .bg(color(theme::ACCENT))
                        .child(icon(assets::CHECK, 12., theme::TEXT_STRONG))
                } else {
                    checkbox
                        .border_color(color(theme::BORDER_STRONG))
                        .bg(color(theme::SURFACE_2))
                }
            });
        div()
            .id(("remembered", index))
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(16.))
            .py(px(10.))
            .when(!last, |row| {
                row.border_b_1().border_color(alpha(theme::BORDER, 0.6))
            })
            .map(|row| {
                if linked {
                    row.opacity(0.6)
                } else {
                    row.cursor_pointer()
                        .hover(|style| style.bg(alpha(theme::SURFACE_2, 0.6)))
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle(index, cx)))
                }
            })
            .child(checkbox)
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
                                    .child(name),
                            )
                            .children(pills),
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
                            .children(details),
                    ),
            )
    }

    pub(super) fn render_import(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let import = self.import.as_ref()?;
        let copy = copy(import.launcher);
        let line = |text: SharedString, hex: u32| {
            div()
                .px(px(20.))
                .py(px(24.))
                .text_color(color(hex))
                .child(text)
        };
        let body: Vec<Stateful<Div>> = match &import.remembered {
            Remembered::Failed(message) => vec![
                line(
                    format!(
                        "Could not read {}'s remembered accounts: {message}",
                        import.launcher.display_name()
                    )
                    .into(),
                    theme::DANGER,
                )
                .id("import-failed"),
            ],
            Remembered::Loading => {
                vec![line(copy.reading.into(), theme::MUTED).id("import-reading")]
            }
            Remembered::Ready(entries) if entries.is_empty() => {
                vec![line(copy.empty.into(), theme::MUTED).id("import-empty")]
            }
            Remembered::Ready(entries) => {
                let now = SystemTime::now();
                let last = entries.len() - 1;
                entries
                    .iter()
                    .enumerate()
                    .map(|(index, entry)| {
                        let checked = import.selected.iter().any(|key| key == key_of(entry));
                        self.render_entry(index, entry, checked, index == last, now, cx)
                    })
                    .collect()
            }
        };
        let status: Div = match &import.error {
            Some(error) => div().text_color(color(theme::DANGER)).child(error.clone()),
            None if import.importable() == 0 && !import.entries().is_empty() => {
                div().child("Everything here is already in One Launch.")
            }
            None => div().child(format!("{} selected", import.selected.len())),
        };
        let busy = import.busy;
        let count = import.selected.len();
        let leads = import.loaded() && !busy;
        let header = div()
            .flex()
            .flex_none()
            .items_start()
            .justify_between()
            .gap(px(16.))
            .border_b_1()
            .border_color(color(theme::BORDER))
            .px(px(20.))
            .py(px(16.))
            .child(
                div()
                    .min_w_0()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(color(theme::TEXT_STRONG))
                            .child(copy.title),
                    )
                    .child(
                        div()
                            .mt(px(2.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color(theme::MUTED))
                            .child(copy.subtitle),
                    ),
            )
            .child(
                div()
                    .id("import-close")
                    .group("import-close")
                    .flex_none()
                    .p(px(4.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| this.close_import(cx)))
                    .child(
                        icon(assets::CLEAR, 16., theme::MUTED)
                            .group_hover("import-close", |style| {
                                style.text_color(color(theme::TEXT))
                            }),
                    ),
            );
        let footer = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .border_t_1()
            .border_color(color(theme::BORDER))
            .px(px(20.))
            .py(px(12.))
            .child(
                status
                    .min_w_0()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color(theme::MUTED)),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        Button::new("import-cancel", Tone::Ghost, "Cancel")
                            .disabled(busy)
                            .leading(leads && !import.import_leads())
                            .on_click(cx.listener(|this, _, _, cx| this.close_import(cx))),
                    )
                    .child(
                        Button::new(
                            "import-run",
                            Tone::Primary,
                            if busy {
                                "Importing\u{2026}".to_owned()
                            } else if count == 0 {
                                "Import".to_owned()
                            } else {
                                format!("Import {count}")
                            },
                        )
                        .glyph(if busy {
                            assets::CHECK
                        } else {
                            assets::DOWNLOAD
                        })
                        .disabled(busy || count == 0)
                        .leading(leads && import.import_leads())
                        .on_click(cx.listener(|this, _, _, cx| this.run_import(cx))),
                    ),
            );
        let card = div()
            .id("import-dialog")
            .occlude()
            .flex()
            .flex_col()
            .w_full()
            .max_w(px(672.))
            .max_h_full()
            .overflow_hidden()
            .rounded(px(12.))
            .border_1()
            .border_color(color(theme::BORDER))
            .bg(color(theme::SURFACE))
            .shadow(panel_shadow())
            .child(header)
            .child(
                div()
                    .id("import-body")
                    .flex_initial()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&import.body)
                    .children(body),
            )
            .child(footer);
        Some(
            div()
                .id("import-backdrop")
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .p(px(24.))
                .bg(alpha(theme::BG, 0.7))
                // The second click of a double-click on the menu item that opened the dialog does not close it again.
                .on_click(cx.listener(|this, click: &ClickEvent, _, cx| {
                    if click.click_count() == 1 {
                        this.close_import(cx);
                    }
                }))
                .child(card),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remembered(launcher_uid: &str, login: &str) -> RememberedAccount {
        RememberedAccount {
            launcher: LauncherId::Battlenet,
            launcher_uid: launcher_uid.to_owned(),
            login: login.to_owned(),
            persona_name: String::new(),
            remember_password: true,
            most_recent: false,
            last_login_at: None,
            session_may_be_expired: false,
            linked_account_id: None,
        }
    }

    #[test]
    fn an_entry_is_asked_for_by_the_launchers_id_or_by_its_login_where_the_launcher_keeps_none() {
        assert_eq!(
            key_of(&remembered("launcher-id-2", "user_2")),
            "launcher-id-2"
        );
        assert_eq!(
            key_of(&remembered("", "saved2@example.com")),
            "saved2@example.com"
        );
    }
}
