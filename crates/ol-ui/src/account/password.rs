use std::time::Duration;

use gpui::prelude::*;
use gpui::{AnyElement, ClickEvent, Context, Div, SharedString, Window, div, px};
use ol_api::accounts::Secret;
use ol_api::clipboard::{CopyField, CopyResult};
use ol_api::{ApiError, Backend};
use ol_model::Account;
use ol_ui_kit::theme::{self, alpha, color};

use super::{AccountPage, Kind, PASSWORD_LINE};
use crate::assets;
use crate::widgets::{Button, Flash, FlashTone, Tone, field, icon, panel, row};

const COPY_FLASH_STAYS: Duration = Duration::from_secs(4);

pub(crate) fn copy_note(field: CopyField, copied: &CopyResult) -> String {
    let what = match field {
        CopyField::Login => "Login",
        CopyField::Password => "Password",
    };
    let seconds = (copied.clear_after.as_millis() + 500) / 1000;
    let history = if field == CopyField::Password && !copied.history_excluded {
        " \u{b7} Windows clipboard history may keep it"
    } else {
        ""
    };
    format!("{what} copied \u{b7} clipboard clears in {seconds}s while One Launch is open{history}")
}

// The first click of a double click opened the question; the second must not answer it.
pub(super) fn single(click: &ClickEvent) -> bool {
    click.click_count() == 1
}

#[derive(Default)]
pub(super) struct Keeping {
    pub editing: bool,
    pub busy: bool,
    pub error: Option<SharedString>,
    pub confirm_remove: bool,
}

impl AccountPage {
    fn edited_id(&self) -> Option<String> {
        match &self.kind {
            Kind::Edit { id } if self.account.is_some() => Some(id.clone()),
            _ => None,
        }
    }

    pub(super) fn password_open(&self) -> bool {
        matches!(self.kind, Kind::Create) || self.keeping.editing
    }

    pub(super) fn typed_new_password(&self, cx: &gpui::App) -> bool {
        self.keeping.editing
            && self
                .password
                .as_ref()
                .is_some_and(|password| !password.is_empty(cx))
    }

    fn edit_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.keeping.editing {
            return;
        }
        self.keeping.editing = true;
        self.keeping.confirm_remove = false;
        if let Some(password) = &self.password {
            password.focus(window, cx);
        }
        // The field has its place once the frame that shows it was drawn; a field that takes the keyboard is brought into sight, as a browser does.
        let this = cx.weak_entity();
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |_, cx| {
                this.update(cx, |this, cx| {
                    if let Some(password) = &this.password {
                        this.scroll.reveal(password.bounds(cx));
                        cx.notify();
                    }
                })
                .ok();
            });
        });
        cx.notify();
    }

    // Also when the page is left: nothing typed outlives the edit.
    pub(super) fn close_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.keeping.editing = false;
        self.keeping.error = None;
        self.show_password = false;
        if let Some(password) = &self.password {
            let focused = password.is_focused(window, cx);
            password.clear(window, cx);
            password.set_masked(true, window, cx);
            if focused {
                window.blur(cx);
            }
        }
        cx.notify();
    }

    pub(super) fn cancel_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.keeping.busy {
            self.close_password(window, cx);
        }
    }

    pub(super) fn store_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(id), Some(field)) = (self.edited_id(), &self.password) else {
            return;
        };
        if self.keeping.busy || !self.keeping.editing || field.is_empty(cx) {
            return;
        }
        let password = Secret::new(field.owned_text(cx));
        let had = self
            .account
            .as_ref()
            .is_some_and(|account| account.has_secret);
        self.keeping.busy = true;
        self.keeping.error = None;
        field.set_disabled(true, cx);
        let backend = self.backend.clone();
        cx.spawn_in(window, async move |this, cx| {
            let stored = backend.set_account_password(&id, password).await;
            this.update_in(cx, |this, window, cx| {
                this.password_stored(stored, had, window, cx);
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn password_stored(
        &mut self,
        stored: Result<Account, ApiError>,
        had: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.keeping.busy = false;
        if let Some(password) = &self.password {
            password.set_disabled(self.busy, cx);
        }
        match stored {
            Ok(account) => {
                self.take_account(account, cx);
                self.close_password(window, cx);
                self.flash(
                    if had {
                        "Password updated"
                    } else {
                        "Password stored"
                    },
                    None,
                    cx,
                );
            }
            Err(error) => self.keeping.error = Some(error.message.into()),
        }
        cx.notify();
    }

    fn remove_password(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.edited_id() else {
            return;
        };
        if self.keeping.busy {
            return;
        }
        self.keeping.busy = true;
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let removed = backend.clear_account_password(&id).await;
            this.update(cx, |this, cx| {
                this.keeping.busy = false;
                match removed {
                    Ok(account) => {
                        this.take_account(account, cx);
                        this.keeping.confirm_remove = false;
                        this.flash("Password removed", None, cx);
                    }
                    Err(error) => this.keeping.error = Some(error.message.into()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn copy(&mut self, field: CopyField, cx: &mut Context<Self>) {
        let Some(id) = self.edited_id() else {
            return;
        };
        let backend: Backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let copied = backend.copy_to_clipboard(&id, field).await;
            this.update(cx, |this, cx| {
                match copied {
                    Ok(copied) => {
                        this.flash(copy_note(field, &copied), Some(COPY_FLASH_STAYS), cx);
                    }
                    Err(error) => this.keeping.error = Some(error.message.into()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn flash(
        &mut self,
        text: impl Into<SharedString>,
        stays: Option<Duration>,
        cx: &mut Context<Self>,
    ) {
        self.flash = Some(Flash::new(
            text,
            FlashTone::Success,
            stays,
            |this: &mut Self| &mut this.flash,
            cx,
        ));
    }

    pub(super) fn eye(&self, cx: &mut Context<Self>) -> AnyElement {
        let shown = self.show_password;
        div()
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
            )
            .into_any_element()
    }

    pub(super) fn render_keeping(&self, window: &Window, cx: &mut Context<Self>) -> Option<Div> {
        let account = self.account.as_ref()?;
        let password = self.password.as_ref()?;
        let keeping = &self.keeping;
        let stored = account.has_secret;
        let actions = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .child(
                Button::new("copy-login", Tone::Secondary, "Copy login")
                    .glyph(assets::COPY)
                    .on_click(cx.listener(|this, _, _, cx| this.copy(CopyField::Login, cx))),
            )
            .child(
                Button::new("copy-password", Tone::Secondary, "Copy password")
                    .glyph(assets::COPY)
                    .disabled(!stored)
                    .on_click(cx.listener(|this, _, _, cx| this.copy(CopyField::Password, cx))),
            );
        let status = if stored {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_color(color(theme::SUCCESS))
                .child(icon(assets::SHIELD_CHECK, 16., theme::SUCCESS))
                .child("Stored \u{b7} encrypted")
        } else {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(icon(assets::KEY, 16., theme::MUTED))
                .child("Not stored")
        };
        let busy = keeping.busy;
        let below = if keeping.editing {
            let empty = password.is_empty(cx);
            div()
                .mt(px(12.))
                .flex()
                .flex_col()
                .gap(px(12.))
                .rounded(px(8.))
                .border_1()
                .border_color(color(theme::BORDER))
                .bg(alpha(theme::SURFACE_2, 0.5))
                .p(px(16.))
                .child(field(
                    if stored { "New password" } else { "Password" },
                    None,
                    keeping.error.clone(),
                    password.form_frame("password", Some(self.eye(cx)), window, cx),
                ))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            Button::new(
                                "password-store",
                                Tone::Primary,
                                if busy {
                                    "Saving\u{2026}"
                                } else if stored {
                                    "Update password"
                                } else {
                                    "Store password"
                                },
                            )
                            .glyph(assets::CHECK)
                            .disabled(empty || busy)
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.store_password(window, cx);
                                },
                            )),
                        )
                        .child(
                            Button::new("password-cancel", Tone::Ghost, "Cancel")
                                .disabled(busy)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.cancel_password(window, cx);
                                })),
                        ),
                )
        } else {
            let mut buttons: Vec<AnyElement> = vec![
                Button::new(
                    "password-edit",
                    Tone::Secondary,
                    if stored {
                        "Update password"
                    } else {
                        "Store password"
                    },
                )
                .glyph(assets::KEY)
                .on_click(cx.listener(|this, _, window, cx| this.edit_password(window, cx))),
            ];
            if stored && keeping.confirm_remove {
                buttons.push(
                    Button::new("password-remove-yes", Tone::Danger, "Confirm remove")
                        .disabled(busy)
                        .on_click(cx.listener(|this, click, _, cx| {
                            if single(click) {
                                this.remove_password(cx);
                            }
                        })),
                );
                buttons.push(
                    Button::new("password-remove-cancel", Tone::Ghost, "Cancel")
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.keeping.confirm_remove = false;
                            cx.notify();
                        })),
                );
            } else if stored {
                buttons.push(
                    Button::new("password-remove", Tone::Ghost, "Remove").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.keeping.confirm_remove = true;
                            cx.notify();
                        },
                    )),
                );
            }
            div()
                .mt(px(12.))
                .flex()
                .items_center()
                .gap(px(8.))
                .children(buttons)
                .children(keeping.error.clone().map(|error| {
                    div()
                        .min_w_0()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color(theme::DANGER))
                        .child(error)
                }))
        };
        Some(panel(
            "Password",
            Some(PASSWORD_LINE.into()),
            Some(actions.into_any_element()),
            div().child(row("Status", None, true, status)).child(below),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_after_a_copy_has_the_electron_apps_words() {
        let private = CopyResult {
            clear_after: Duration::from_secs(30),
            history_excluded: true,
        };
        let plain = CopyResult {
            history_excluded: false,
            ..private
        };
        assert_eq!(
            copy_note(CopyField::Login, &plain),
            "Login copied \u{b7} clipboard clears in 30s while One Launch is open"
        );
        assert_eq!(
            copy_note(CopyField::Password, &private),
            "Password copied \u{b7} clipboard clears in 30s while One Launch is open"
        );
        assert_eq!(
            copy_note(CopyField::Password, &plain),
            "Password copied \u{b7} clipboard clears in 30s while One Launch is open \u{b7} Windows clipboard history may keep it"
        );
        let rounded = CopyResult {
            clear_after: Duration::from_millis(29_500),
            ..plain
        };
        assert!(copy_note(CopyField::Login, &rounded).contains("in 30s"));
    }
}
