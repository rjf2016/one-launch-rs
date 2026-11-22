use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, SharedString, Subscription, Window, div, px};
use ol_api::accounts::Secret;
use ol_api::artwork::{KEY_REFUSED, WebPage, looks_like_a_key};
use ol_model::{ArtworkStatus, LibraryScanResult, SettingKey};
use ol_ui_kit::theme::{self, color};
use ol_ui_kit::{FieldEvent, TextField};

use super::{Control, SettingsView};
use crate::assets;
use crate::widgets::{Button, Tone, icon, panel, setting_row, tip};

const FIELD_WIDTH: f32 = 224.;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Busy {
    Key,
    Refresh,
    Rescan,
}

pub(super) struct Artwork {
    // The queue as the backend last told it. None until its first word.
    status: Option<ArtworkStatus>,
    field: TextField,
    busy: Option<Busy>,
    message: Option<SharedString>,
    // How long the field's text was at its last change: a text that grows by more than a character at once was pasted.
    typed: usize,
    _field: Subscription,
}

fn games(count: usize) -> &'static str {
    if count == 1 { "game" } else { "games" }
}

impl Artwork {
    pub(super) fn new(window: &mut Window, cx: &mut Context<SettingsView>) -> Self {
        let field = TextField::new("Paste your key", true, window, cx).limited_to(4096, cx);
        let heard = field.on_event_in(window, cx, SettingsView::on_key_field);
        Self {
            status: None,
            field,
            busy: None,
            message: None,
            typed: 0,
            _field: heard,
        }
    }

    pub(super) fn heard(&mut self, status: ArtworkStatus) {
        self.status = Some(status);
    }

    fn has_key(&self) -> bool {
        self.status.is_some_and(|status| status.has_api_key)
    }

    pub(super) fn typing(&self, window: &Window, cx: &gpui::App) -> bool {
        self.field.is_focused(window, cx)
    }

    pub(super) fn focus_field(&self, window: &mut Window, cx: &mut gpui::App) {
        self.field.focus(window, cx);
    }

    // The controls Tab stops at, in the order they are drawn.
    pub(super) fn controls(&self, vault_works: bool, controls: &mut Vec<Control>) {
        if self.busy.is_none() {
            controls.extend([Control::ArtworkRescan, Control::ArtworkRefreshAll]);
        }
        if self.has_key() {
            if self.busy != Some(Busy::Key) {
                controls.push(Control::RemoveKey);
            }
        } else {
            controls.push(Control::GetKey);
            if vault_works && self.busy != Some(Busy::Key) {
                controls.push(Control::KeyField);
            }
        }
    }
}

impl SettingsView {
    fn artwork_busy(&mut self, busy: Option<Busy>, cx: &mut Context<Self>) {
        self.artwork.busy = busy;
        self.sync_key_field(cx);
        cx.notify();
    }

    // The field takes no key while the vault cannot seal one, nor while one is being stored.
    pub(super) fn sync_key_field(&mut self, cx: &mut Context<Self>) {
        let disabled = !self.vault_works() || self.artwork.busy == Some(Busy::Key);
        if self.artwork.field.is_disabled(cx) != disabled {
            self.artwork.field.set_disabled(disabled, cx);
        }
    }

    fn on_key_field(&mut self, event: FieldEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            FieldEvent::Changed => {
                let text = self.artwork.field.text(cx);
                let pasted = text.chars().count() > self.artwork.typed + 1;
                self.artwork.typed = text.chars().count();
                // A pasted key is stored at once, as in the Electron app.
                if pasted && looks_like_a_key(text.trim()) {
                    self.save_key(window, cx);
                }
            }
            FieldEvent::Enter => self.try_save_key(false, window, cx),
            FieldEvent::Blurred => self.try_save_key(true, window, cx),
        }
    }

    pub(super) fn leave_key_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.try_save_key(true, window, cx);
    }

    // Enter, or the keyboard leaving the field: what looks like a key is stored. What does not is said so after Enter, and left alone otherwise.
    fn try_save_key(&mut self, quiet: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.artwork.busy.is_some() {
            return;
        }
        let text = self.artwork.field.text(cx);
        let typed = text.trim();
        if typed.is_empty() {
            return;
        }
        if looks_like_a_key(typed) {
            self.save_key(window, cx);
        } else if !quiet {
            self.artwork.message = Some(KEY_REFUSED.into());
            cx.notify();
        }
    }

    // The key goes to the vault, and then every game without a cover is looked up again.
    fn save_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.artwork.busy.is_some() {
            return;
        }
        let typed = self.artwork.field.owned_text(cx);
        let key = Secret::new(if typed.trim().len() == typed.len() {
            typed
        } else {
            typed.trim().to_owned()
        });
        self.artwork.message = None;
        self.artwork_busy(Some(Busy::Key), cx);
        let backend = self.backend.clone();
        cx.spawn_in(window, async move |this, cx| {
            let stored = backend.set_steamgriddb_key(key).await;
            let message = match stored {
                Err(error) => Err(error.message),
                Ok(()) => Ok(match backend.refresh_artwork(None, false).await {
                    Ok(0) => "Key saved.".to_owned(),
                    Ok(queued) => format!(
                        "Key saved. Fetching artwork for {queued} {}.",
                        games(queued)
                    ),
                    Err(error) => error.message,
                }),
            };
            this.update_in(cx, |this, window, cx| {
                if message.is_ok() {
                    // Emptied without an undo step, so Ctrl+Z cannot bring the key back.
                    this.artwork.field.set_text("", window, cx);
                    this.artwork.typed = 0;
                    // The field gives way to the stored key's row, and comes back without the keyboard.
                    if this.artwork.field.is_focused(window, cx) {
                        window.blur(cx);
                    }
                }
                this.artwork.message = Some(message.unwrap_or_else(|error| error).into());
                this.artwork_busy(None, cx);
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn remove_key(&mut self, cx: &mut Context<Self>) {
        if self.artwork.busy.is_some() {
            return;
        }
        self.artwork.message = None;
        self.artwork_busy(Some(Busy::Key), cx);
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let removed = backend.remove_steamgriddb_key().await;
            this.update(cx, |this, cx| {
                this.artwork.message = Some(match removed {
                    Ok(()) => "Key removed.".into(),
                    Err(error) => error.message.into(),
                });
                this.artwork_busy(None, cx);
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn get_key(&mut self, cx: &mut Context<Self>) {
        self.artwork.message = None;
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            if let Err(error) = backend.open_page(WebPage::SteamGridDbApiKey).await {
                this.update(cx, |this, cx| {
                    this.artwork.message =
                        Some(format!("Could not open the browser: {}", error.message).into());
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
        cx.notify();
    }

    pub(super) fn refresh_all_artwork(&mut self, cx: &mut Context<Self>) {
        if self.artwork.busy.is_some() {
            return;
        }
        self.artwork.message = None;
        self.artwork_busy(Some(Busy::Refresh), cx);
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let queued = backend.refresh_artwork(None, true).await;
            this.update(cx, |this, cx| {
                this.artwork.message = Some(match queued {
                    Ok(queued) => format!(
                        "Re-fetching artwork for {queued} {}. Images you chose yourself are kept.",
                        games(queued)
                    )
                    .into(),
                    Err(error) => error.message.into(),
                });
                this.artwork_busy(None, cx);
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn rescan_for_artwork(&mut self, cx: &mut Context<Self>) {
        if self.artwork.busy.is_some() {
            return;
        }
        self.artwork.message = None;
        self.artwork_busy(Some(Busy::Rescan), cx);
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let scanned = backend.rescan().await;
            this.update(cx, |this, cx| {
                this.artwork.message = Some(match scanned {
                    Ok(scanned) => rescanned(&scanned).into(),
                    Err(error) => error.message.into(),
                });
                this.artwork_busy(None, cx);
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn render_artwork(&self, window: &Window, cx: &mut Context<Self>) -> Vec<Div> {
        let state = &self.artwork;
        let busy = state.busy.is_some();
        let rescan = Button::new("artwork-rescan", Tone::Secondary, "Rescan")
            .glyph(assets::RESCAN)
            .tip("Look for games added or removed since the last scan")
            .turning(state.busy == Some(Busy::Rescan))
            .disabled(busy)
            .leading(self.ringed_on(Control::ArtworkRescan))
            .on_click(cx.listener(|this, _, _, cx| {
                this.clicked(Control::ArtworkRescan);
                this.rescan_for_artwork(cx);
            }));
        let refresh_all = Button::new("artwork-refresh-all", Tone::Secondary, "Refresh all")
            .glyph(assets::IMAGES)
            .tip("Fetch every cover again; images you chose yourself are kept")
            .disabled(busy)
            .leading(self.ringed_on(Control::ArtworkRefreshAll))
            .on_click(cx.listener(|this, _, _, cx| {
                this.clicked(Control::ArtworkRefreshAll);
                this.refresh_all_artwork(cx);
            }));
        let actions = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .child(self.track(Control::ArtworkRescan, rescan))
            .child(self.track(Control::ArtworkRefreshAll, refresh_all))
            .into_any_element();

        let (key_hint, key_value) = if state.has_key() {
            (
                "Remove it to paste a different one.",
                self.render_stored_key(cx),
            )
        } else {
            (
                "Free. Press Get a key, sign in, paste it here.",
                self.render_key_field(window, cx),
            )
        };
        let queue = match state.status {
            None => div().child("\u{2026}"),
            Some(status) => {
                let active = status.busy();
                let mut line = String::new();
                if active > 0 {
                    line.push_str(&format!("{active} in progress \u{b7} "));
                }
                line.push_str(&format!("{} done", status.completed));
                if status.failed > 0 {
                    line.push_str(&format!(" \u{b7} {} not found", status.failed));
                }
                div()
                    .when(active > 0, |line| line.text_color(color(theme::TEXT)))
                    .child(line)
            }
        };
        let message = state.message.clone();
        let body = div()
            .child(setting_row(
                "SteamGridDB API key",
                Some(key_hint.into()),
                None,
                true,
                false,
                key_value,
            ))
            .child(self.toggle_row(
                SettingKey::ArtworkAutoFetch,
                "Fetch artwork automatically",
                "Looks up missing covers after each scan; only game names and ids are sent.",
                (false, false),
                cx,
            ))
            .child(setting_row(
                "Queue",
                Some("Background downloads since the app started.".into()),
                None,
                false,
                message.is_none(),
                queue,
            ))
            .children(message.map(|message| {
                div()
                    .pt(px(8.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color(theme::MUTED))
                    .child(message)
            }));
        vec![panel(
            "Artwork",
            Some("Covers come from Steam. Other games need a free SteamGridDB key.".into()),
            Some(actions),
            body,
        )]
    }

    // A key is stored: the field shows that and takes nothing, and the key itself is never read back into the window.
    fn render_stored_key(&self, cx: &mut Context<Self>) -> AnyElement {
        let masked = div()
            .flex()
            .flex_none()
            .items_center()
            .w(px(FIELD_WIDTH))
            .h(px(38.))
            .px(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(color(theme::BORDER))
            .bg(color(theme::SURFACE_2))
            .opacity(0.6)
            .text_left()
            .text_color(color(theme::FAINT))
            .child("\u{2022}".repeat(8));
        let control = Control::RemoveKey;
        let disabled = self.artwork.busy == Some(Busy::Key);
        let remove = div()
            .id("remove-artwork-key")
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(34.))
            .rounded(px(8.))
            .when(self.ringed_on(control), |remove| {
                remove.border_2().border_color(color(theme::ACCENT))
            })
            .map(|remove| {
                if disabled {
                    return remove.opacity(0.5);
                }
                remove
                    .cursor_pointer()
                    .hover(|style| style.bg(color(theme::SURFACE_2)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.clicked(control);
                        this.remove_key(cx);
                    }))
            })
            .tooltip(tip("Remove key"))
            .child(icon(assets::TRASH, 16., theme::MUTED));
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(masked)
            .child(self.track(control, remove.into_any_element()))
            .into_any_element()
    }

    fn render_key_field(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let get = Button::new("get-artwork-key", Tone::Secondary, "Get a key")
            .glyph(assets::EXTERNAL_LINK)
            .tip("Opens the SteamGridDB API page in your browser")
            .leading(self.ringed_on(Control::GetKey))
            .on_click(cx.listener(|this, _, _, cx| {
                this.clicked(Control::GetKey);
                this.get_key(cx);
            }));
        let field = self
            .artwork
            .field
            .form_frame("steamgriddb-key", None, window, cx)
            .flex_none()
            .w(px(FIELD_WIDTH))
            .text_left()
            .into_any_element();
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(self.track(Control::GetKey, get))
            .child(self.track(Control::KeyField, field))
            .into_any_element()
    }
}

// What the Electron app says after its Rescan.
fn rescanned(scanned: &[LibraryScanResult]) -> String {
    let added: usize = scanned.iter().map(|launcher| launcher.added).sum();
    let removed: usize = scanned.iter().map(|launcher| launcher.removed).sum();
    let failed: Vec<String> = scanned
        .iter()
        .filter_map(|launcher| {
            let error = launcher.error.as_ref()?;
            Some(format!("{}: {error}", launcher.launcher.display_name()))
        })
        .collect();
    let mut said = format!("Library rescanned: {added} added, {removed} removed.");
    if !failed.is_empty() {
        said.push_str(&format!(" Not scanned: {}", failed.join("; ")));
    }
    said
}

#[cfg(test)]
mod tests {
    use ol_model::LauncherId;

    use super::*;

    fn result(
        launcher: LauncherId,
        added: usize,
        removed: usize,
        error: Option<&str>,
    ) -> LibraryScanResult {
        LibraryScanResult {
            launcher,
            added,
            updated: 0,
            removed,
            installed: true,
            error: error.map(str::to_owned),
        }
    }

    #[test]
    fn a_rescan_says_what_it_added_and_removed_and_which_launcher_it_could_not_read() {
        assert_eq!(
            rescanned(&[
                result(LauncherId::Steam, 2, 0, None),
                result(LauncherId::Epic, 1, 3, None),
            ]),
            "Library rescanned: 3 added, 3 removed."
        );
        assert_eq!(
            rescanned(&[
                result(LauncherId::Steam, 0, 0, Some("no list")),
                result(LauncherId::Riot, 0, 0, Some("no folder")),
            ]),
            "Library rescanned: 0 added, 0 removed. Not scanned: Steam: no list; Riot Games: no folder"
        );
        assert_eq!(rescanned(&[]), "Library rescanned: 0 added, 0 removed.");
        assert_eq!((games(1), games(0), games(2)), ("game", "games", "games"));
    }
}
