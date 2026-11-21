use std::time::SystemTime;

use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, SharedString, Window, div, px};
use ol_api::launch::sentence;
use ol_api::library::relative;
use ol_api::{ApiError, CapturedSignIn, SavedSignIn};
use ol_model::{Account, LauncherId, LauncherSession, LauncherSessionState};
use ol_ui_kit::theme::{self, color};

use super::password::single;
use super::{AccountPage, Kind};
use crate::assets;
use crate::widgets::{Button, Tone, icon, panel, row};

const DESCRIPTION: &str =
    "Saved the first time you play on this account, and put back whenever you switch to it.";

// The launchers that sign in from a session One Launch can save: the account's page shows the panel for these alone.
pub(super) fn launcher_name(launcher: LauncherId) -> Option<&'static str> {
    match launcher {
        LauncherId::Steam => None,
        LauncherId::Epic => Some("Epic Games Launcher"),
        LauncherId::Battlenet => Some("Battle.net"),
        LauncherId::Riot => Some("Riot Client"),
    }
}

#[derive(Default)]
pub(super) struct SignIn {
    asked: u64,
    loading: bool,
    found: Option<SavedSignIn>,
    error: Option<SharedString>,
    busy: bool,
    note: Option<SharedString>,
    failed: Option<SharedString>,
    confirm_forget: bool,
}

#[derive(Clone, Copy)]
enum Tint {
    Ok,
    Muted,
}

struct OnDisk {
    text: &'static str,
    tint: Tint,
    hint: Option<&'static str>,
}

fn on_disk(
    state: &LauncherSessionState,
    account: &Account,
    session: Option<&LauncherSession>,
) -> OnDisk {
    let plain = |text, tint| OnDisk {
        text,
        tint,
        hint: None,
    };
    if !state.remembered {
        return plain("Nobody", Tint::Muted);
    }
    let linked = state
        .linked_account_id
        .as_deref()
        .filter(|id| !id.is_empty());
    match linked {
        Some(id) if id == account.id => {
            let current = session.is_some_and(|session| {
                state.fingerprint.as_deref() == Some(session.fingerprint.as_str())
            });
            plain(
                if current {
                    "This account"
                } else {
                    "This account (newer than the saved sign-in; press Save again)"
                },
                Tint::Ok,
            )
        }
        Some(_) => plain("Another of your accounts", Tint::Muted),
        None => OnDisk {
            text: "An account not yet linked in One Launch",
            tint: Tint::Muted,
            hint: Some("If that is this account, press Save sign-in."),
        },
    }
}

fn small(hex: u32, text: impl Into<SharedString>) -> Div {
    div()
        .min_w_0()
        .text_size(px(12.))
        .line_height(px(16.))
        .text_color(color(hex))
        .child(text.into())
}

fn last_chars(text: &str, count: usize) -> &str {
    let from = text
        .char_indices()
        .rev()
        .nth(count - 1)
        .map_or(0, |(at, _)| at);
    &text[from..]
}

impl AccountPage {
    pub(super) fn load_sign_in(&mut self, cx: &mut Context<Self>) {
        let Kind::Edit { id } = &self.kind else {
            return;
        };
        if !self
            .account
            .as_ref()
            .is_some_and(|account| launcher_name(account.launcher).is_some())
        {
            return;
        }
        let sign_in = &mut self.sign_in;
        sign_in.asked += 1;
        sign_in.loading = true;
        sign_in.error = None;
        let (asked, backend, id) = (sign_in.asked, self.backend.clone(), id.clone());
        cx.spawn(async move |this, cx| {
            let found = backend.session(&id).await;
            this.update(cx, |this, cx| {
                let sign_in = &mut this.sign_in;
                // An answer to a question asked before the last one is passed over.
                if sign_in.asked != asked {
                    return;
                }
                sign_in.loading = false;
                match found {
                    Ok(found) => {
                        sign_in.found = Some(found);
                        sign_in.error = None;
                    }
                    Err(error) => sign_in.error = Some(error.message.into()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn save_sign_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Kind::Edit { id } = &self.kind else {
            return;
        };
        if self.sign_in.busy || self.account.is_none() {
            return;
        }
        self.sign_in.busy = true;
        self.sign_in.failed = None;
        self.sign_in.note = None;
        let (backend, id) = (self.backend.clone(), id.clone());
        cx.spawn_in(window, async move |this, cx| {
            let saved = backend.capture_session(&id).await;
            this.update_in(cx, |this, window, cx| this.sign_in_saved(saved, window, cx))
                .ok();
        })
        .detach();
        cx.notify();
    }

    fn sign_in_saved(
        &mut self,
        saved: Result<CapturedSignIn, ApiError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sign_in.busy = false;
        match saved {
            Ok(saved) => {
                self.take_linked(saved.account, window, cx);
                self.sign_in.note = Some("Sign-in saved".into());
                self.load_sign_in(cx);
            }
            Err(error) => self.sign_in.failed = Some(error.message.into()),
        }
        cx.notify();
    }

    fn forget_sign_in(&mut self, cx: &mut Context<Self>) {
        let Kind::Edit { id } = &self.kind else {
            return;
        };
        if self.sign_in.busy {
            return;
        }
        self.sign_in.busy = true;
        self.sign_in.failed = None;
        let (backend, id) = (self.backend.clone(), id.clone());
        cx.spawn(async move |this, cx| {
            let forgotten = backend.forget_session(&id).await;
            this.update(cx, |this, cx| {
                this.sign_in.busy = false;
                match forgotten {
                    Ok(()) => {
                        this.sign_in.confirm_forget = false;
                        this.sign_in.note = Some("Saved sign-in removed".into());
                        this.load_sign_in(cx);
                    }
                    Err(error) => this.sign_in.failed = Some(error.message.into()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(super) fn render_sign_in(&self, cx: &mut Context<Self>) -> Option<Div> {
        let account = self.account.as_ref()?;
        let name = launcher_name(account.launcher)?;
        let sign_in = &self.sign_in;
        let found = sign_in.found.as_ref();
        let session = found.and_then(|found| found.session.as_ref());
        let state = found.and_then(|found| found.state.as_ref());
        let first_load = found.is_none() && sign_in.error.is_none();
        let busy = sign_in.busy;
        let remembered = state.is_some_and(|state| state.remembered);
        let nothing_to_save = state.is_some_and(|state| !state.remembered);
        let now = SystemTime::now();
        let ago = |at: &str| relative(at, now).unwrap_or_default();

        let mut save = Button::new(
            "sign-in-save",
            Tone::Primary,
            if busy {
                "Saving\u{2026}"
            } else if session.is_some() {
                "Save again"
            } else {
                "Save sign-in"
            },
        )
        .glyph(assets::SAVE)
        .disabled(busy || !remembered);
        if nothing_to_save {
            save = save.tip(format!(
                "Nothing to save: {name} holds no sign-in right now."
            ));
        }
        let actions = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .child(
                Button::new("sign-in-recheck", Tone::Secondary, "Re-check")
                    .glyph(assets::RESCAN)
                    .turning(sign_in.loading)
                    .disabled(sign_in.loading || busy)
                    .on_click(cx.listener(|this, _, _, cx| this.load_sign_in(cx))),
            )
            .child(save.on_click(cx.listener(|this, _, window, cx| this.save_sign_in(window, cx))));

        let saved_hint: Option<SharedString> = match session {
            Some(session) => Some(
                session
                    .verified_at
                    .as_deref()
                    .map_or_else(
                        || "Not used for a switch yet.".to_owned(),
                        |at| format!("Last used for a switch {}.", ago(at)),
                    )
                    .into(),
            ),
            None if first_load && account.has_session => Some(" ".into()),
            None => None,
        };
        let marked = |glyph, hex, text: SharedString| {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_color(color(hex))
                .child(icon(glyph, 16., hex))
                .child(text)
                .into_any_element()
        };
        let saved: AnyElement = match session {
            Some(session) if session.stale_reason.is_some() => marked(
                assets::UPDATE,
                theme::WARNING,
                "Saved, but needs a fresh sign-in".into(),
            ),
            Some(session) => marked(
                assets::SHIELD_CHECK,
                theme::SUCCESS,
                format!("Saved {}", ago(&session.captured_at)).into(),
            ),
            None if first_load && account.has_session => {
                div().child("Checking\u{2026}").into_any_element()
            }
            None => marked(assets::SHIELD_OFF, theme::MUTED, "Not saved yet".into()),
        };
        let mut rows = vec![row("Saved sign-in", saved_hint, true, saved)];
        if let Some(reason) = session.and_then(|session| session.stale_reason.clone()) {
            rows.push(row(
                "Why",
                Some(
                    format!(
                        "Sign in to {name} once by hand; the next switch saves the fresh sign-in on its own."
                    )
                    .into(),
                ),
                false,
                small(theme::WARNING, reason),
            ));
        }
        let disk = state.map(|state| on_disk(state, account, session));
        let running = state.is_some_and(|state| state.launcher_running);
        let signed_in_hint = [
            running.then(|| format!("{name} is running.")),
            disk.as_ref().and_then(|disk| disk.hint).map(str::to_owned),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
        let failure = sign_in
            .error
            .clone()
            .or_else(|| found.and_then(|found| found.state_error.clone().map(Into::into)));
        let signed_in: AnyElement = match (failure, &disk) {
            (Some(failure), _) => div()
                .text_color(color(theme::DANGER))
                .child(failure)
                .into_any_element(),
            (None, Some(disk)) => div()
                .text_color(color(match disk.tint {
                    Tint::Ok => theme::SUCCESS,
                    Tint::Muted => theme::MUTED,
                }))
                .child(disk.text)
                .into_any_element(),
            (None, None) => div().child("Checking\u{2026}").into_any_element(),
        };
        rows.push(row(
            format!("{name} is signed in as"),
            (!signed_in_hint.is_empty()).then(|| signed_in_hint.into()),
            false,
            signed_in,
        ));
        if let Some(uid) = account
            .launcher_uid
            .as_deref()
            .filter(|uid| !uid.is_empty())
        {
            rows.push(row(
                format!("{name} account"),
                Some(
                    "Recorded when the launcher reported this account\u{2019}s sign-in. The full id is under Advanced."
                        .into(),
                ),
                false,
                format!("Linked to \u{2026}{}", last_chars(uid, 4)),
            ));
        }
        if let Some(last) = state
            .and_then(|state| state.last_sign_in.as_ref())
            .filter(|last| !last.ok)
        {
            let reason = last
                .reason
                .as_deref()
                .map_or_else(|| "Failed".to_owned(), sentence);
            rows.push(row(
                "Last sign-in",
                None,
                false,
                small(theme::WARNING, reason),
            ));
        }

        let mut buttons: Vec<AnyElement> = Vec::new();
        if session.is_some() {
            if sign_in.confirm_forget {
                buttons.push(
                    Button::new("sign-in-forget-yes", Tone::Danger, "Yes, forget it")
                        .glyph(assets::TRASH)
                        .disabled(busy)
                        .on_click(cx.listener(|this, click, _, cx| {
                            if single(click) {
                                this.forget_sign_in(cx);
                            }
                        })),
                );
                buttons.push(
                    Button::new("sign-in-forget-cancel", Tone::Ghost, "Cancel")
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.sign_in.confirm_forget = false;
                            cx.notify();
                        })),
                );
            } else {
                buttons.push(
                    Button::new("sign-in-forget", Tone::Ghost, "Forget saved sign-in")
                        .glyph(assets::TRASH)
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.sign_in.confirm_forget = true;
                            cx.notify();
                        })),
                );
            }
        }
        let footer = div()
            .mt(px(12.))
            .flex()
            .items_center()
            .gap(px(8.))
            .children(buttons)
            .children(sign_in.note.clone().map(|note| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(icon(assets::CHECK, 14., theme::SUCCESS))
                    .child(small(theme::SUCCESS, note))
            }))
            .children(
                sign_in
                    .failed
                    .clone()
                    .map(|failed| small(theme::DANGER, failed)),
            );
        Some(panel(
            "Saved sign-in",
            Some(DESCRIPTION.into()),
            Some(actions.into_any_element()),
            div().children(rows).child(footer),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_id_shows_its_last_four_characters() {
        assert_eq!(last_chars("0a1b2c3d", 4), "2c3d");
        assert_eq!(last_chars("ab", 4), "ab");
        assert_eq!(
            last_chars("x\u{e9}\u{e9}\u{e9}\u{e9}", 4),
            "\u{e9}\u{e9}\u{e9}\u{e9}"
        );
    }
}
