use std::collections::HashMap;

use gpui::prelude::*;
use gpui::{
    AnyElement, ClickEvent, Context, Div, ElementId, Keystroke, SharedString, Stateful, div, px,
};
use ol_api::ApiError;
use ol_api::library::date_time;
use ol_api::settings::Folder;
use ol_model::{BackupSnapshot, LauncherId, RestoreResult, SettingKey};
use ol_ui_kit::theme::{self, alpha, color};

use super::{Control, Pick, Refused, SettingsTab, SettingsView};
use crate::assets;
use crate::local_time;
use crate::widgets::{
    Appeared, Button, Tone, icon, launcher_dot, panel, panel_shadow, setting_row, tip,
};

#[derive(Default)]
pub(super) struct Backups {
    lists: HashMap<LauncherId, List>,
    // Every snapshot id seen, so the keyboard can stay on one snapshot's button while the lists change.
    seen: Vec<(LauncherId, String)>,
    seen_at: HashMap<LauncherId, HashMap<String, usize>>,
    dialog: Option<Restoring>,
    dialogs: u64,
    outcome: Option<Outcome>,
    visit: u64,
}

#[derive(Default)]
struct List {
    snapshots: Option<Vec<BackupSnapshot>>,
    error: Option<SharedString>,
    loading: bool,
    // Asked for while a read ran: one more read follows it, and the running one's answer is passed over.
    again: bool,
    open: bool,
}

struct Restoring {
    key: u64,
    snapshot: BackupSnapshot,
    busy: bool,
    error: Option<SharedString>,
    // Cancel leads until Tab moves the ring, as the Electron app's dialog puts the keyboard on its first button.
    restore_leads: bool,
    appeared: Appeared,
}

enum Outcome {
    Restored(RestoreResult),
    Failed(SharedString),
}

impl Backups {
    fn list(&self, launcher: LauncherId) -> Option<&List> {
        self.lists.get(&launcher)
    }

    fn shown(&self, launcher: LauncherId) -> &[BackupSnapshot] {
        self.list(launcher)
            .and_then(|list| list.snapshots.as_deref())
            .unwrap_or_default()
    }

    fn seen_at(&self, launcher: LauncherId, id: &str) -> Option<usize> {
        self.seen_at.get(&launcher)?.get(id).copied()
    }

    fn see(&mut self, launcher: LauncherId, id: &str) {
        let known = self.seen_at.entry(launcher).or_default();
        if !known.contains_key(id) {
            known.insert(id.to_owned(), self.seen.len());
            self.seen.push((launcher, id.to_owned()));
        }
    }

    pub(super) fn controls(&self, controls: &mut Vec<Control>) {
        for launcher in LauncherId::ALL {
            controls.push(Control::Section(launcher));
            if !self.list(launcher).is_some_and(|list| list.open) {
                continue;
            }
            controls.extend(
                self.shown(launcher)
                    .iter()
                    .filter_map(|snapshot| self.seen_at(launcher, &snapshot.id))
                    .map(|seen| Control::Restore(launcher, seen)),
            );
        }
    }
}

fn plural(count: usize, one: &str) -> String {
    format!("{count} {one}{}", if count == 1 { "" } else { "s" })
}

fn size(bytes: u64) -> String {
    const KB: f64 = 1024.;
    let value = bytes as f64;
    if bytes < 1024 {
        format!("{bytes} B")
    } else if value < KB * KB {
        format!("{:.1} KB", value / KB)
    } else {
        format!("{:.1} MB", value / (KB * KB))
    }
}

fn restored_line(result: &RestoreResult) -> String {
    let registry = if result.registry.is_empty() {
        String::new()
    } else {
        format!(", registry {}", result.registry.join(", "))
    };
    format!(
        "Restored snapshot {}: {}{registry}. Start the launcher to check it signed in as expected.",
        result.id,
        plural(result.files.len(), "file")
    )
}

impl SettingsView {
    // As the Electron app mounts the panel afresh: every section folded, no restore's answer from before, and every list read.
    pub(super) fn enter_backups(&mut self, cx: &mut Context<Self>) {
        let backups = &mut self.backups;
        backups.visit += 1;
        backups.dialog = None;
        backups.outcome = None;
        for list in backups.lists.values_mut() {
            list.open = false;
        }
        self.list_backups(None, cx);
    }

    // Every launcher's list, or one. A list being read is read once more after it, and no more, however often it is asked for.
    pub(super) fn list_backups(&mut self, only: Option<LauncherId>, cx: &mut Context<Self>) {
        for launcher in LauncherId::ALL {
            if only.is_none_or(|only| only == launcher) {
                self.read_list(launcher, cx);
            }
        }
        cx.notify();
    }

    fn read_list(&mut self, launcher: LauncherId, cx: &mut Context<Self>) {
        let list = self.backups.lists.entry(launcher).or_default();
        if list.loading {
            list.again = true;
            return;
        }
        list.loading = true;
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let listed = backend.backups(launcher).await;
            this.update(cx, |this, cx| this.listed(launcher, listed, cx))
                .ok();
        })
        .detach();
    }

    fn listed(
        &mut self,
        launcher: LauncherId,
        listed: Result<Vec<BackupSnapshot>, ApiError>,
        cx: &mut Context<Self>,
    ) {
        let backups = &mut self.backups;
        let Some(list) = backups.lists.get_mut(&launcher) else {
            return;
        };
        list.loading = false;
        if std::mem::take(&mut list.again) {
            self.read_list(launcher, cx);
            return;
        }
        match listed {
            Ok(snapshots) => {
                for snapshot in &snapshots {
                    backups.see(launcher, &snapshot.id);
                }
                if let Some(list) = backups.lists.get_mut(&launcher) {
                    list.snapshots = Some(snapshots);
                    list.error = None;
                }
            }
            Err(error) => {
                list.snapshots = None;
                list.error = Some(error.message.into());
            }
        }
        cx.notify();
    }

    pub(super) fn toggle_section(&mut self, launcher: LauncherId, cx: &mut Context<Self>) {
        let list = self.backups.lists.entry(launcher).or_default();
        list.open = !list.open;
        cx.notify();
    }

    pub(super) fn ask_restore(
        &mut self,
        launcher: LauncherId,
        seen: usize,
        cx: &mut Context<Self>,
    ) {
        let backups = &mut self.backups;
        let Some((_, id)) = backups.seen.get(seen) else {
            return;
        };
        let Some(snapshot) = backups
            .shown(launcher)
            .iter()
            .find(|snapshot| snapshot.id == *id)
            .cloned()
        else {
            return;
        };
        backups.dialogs += 1;
        backups.dialog = Some(Restoring {
            key: backups.dialogs,
            snapshot,
            busy: false,
            error: None,
            restore_leads: false,
            appeared: Appeared::now(),
        });
        cx.notify();
    }

    pub fn restoring(&self) -> bool {
        self.backups.dialog.is_some()
    }

    // As the Electron app's backdrop does, also while the restore runs: its answer still comes to the page.
    fn close_restore(&mut self, cx: &mut Context<Self>) {
        if self.backups.dialog.take().is_some() {
            cx.notify();
        }
    }

    // A second press, or Enter with a click, finds the restore under way and does nothing.
    fn restore(&mut self, cx: &mut Context<Self>) {
        let visit = self.backups.visit;
        let Some(dialog) = self.backups.dialog.as_mut() else {
            return;
        };
        if dialog.busy {
            return;
        }
        dialog.busy = true;
        dialog.error = None;
        let (key, launcher, id) = (
            dialog.key,
            dialog.snapshot.launcher,
            dialog.snapshot.id.clone(),
        );
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let restored = backend.restore_backup(launcher, &id).await;
            this.update(cx, |this, cx| {
                this.restored(visit, key, launcher, restored, cx)
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    // The dialog that asked shows a refusal; once it is closed, the panel does. A visit of the tab after the one that asked shows neither, and lists the launcher again.
    fn restored(
        &mut self,
        visit: u64,
        key: u64,
        launcher: LauncherId,
        restored: Result<RestoreResult, ApiError>,
        cx: &mut Context<Self>,
    ) {
        let backups = &mut self.backups;
        let succeeded = restored.is_ok();
        if visit == backups.visit {
            let asked = backups.dialog.as_mut().filter(|dialog| dialog.key == key);
            match (restored, asked) {
                (Ok(result), asked) => {
                    if asked.is_some() {
                        backups.dialog = None;
                    }
                    backups.outcome = Some(Outcome::Restored(result));
                }
                (Err(error), Some(dialog)) => {
                    dialog.busy = false;
                    dialog.error = Some(error.message.into());
                }
                (Err(error), None) => {
                    backups.outcome = Some(Outcome::Failed(error.message.into()));
                }
            }
        }
        if succeeded && self.tab.get() == SettingsTab::Backups {
            self.list_backups(Some(launcher), cx);
        }
        cx.notify();
    }

    // Every key is the dialog's while it is up; the shell lets those held with Alt or the Windows key go on to Windows.
    pub fn on_restore_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        let held = keystroke.modifiers;
        if held.control || held.alt || held.platform || held.function {
            return;
        }
        let Some(dialog) = self.backups.dialog.as_mut() else {
            return;
        };
        match keystroke.key.as_str() {
            "tab" => {
                dialog.restore_leads = !dialog.restore_leads;
                cx.notify();
            }
            "escape" if !held.shift => self.close_restore(cx),
            "enter" | "space" if !held.shift => {
                if dialog.busy {
                    return;
                }
                if dialog.restore_leads {
                    self.restore(cx);
                } else {
                    self.close_restore(cx);
                }
            }
            _ => {}
        }
    }

    pub(super) fn open_backups_folder(&mut self, cx: &mut Context<Self>) {
        self.open_folder(Folder::Backups, cx);
    }

    pub(super) fn render_backups(&mut self, cx: &mut Context<Self>) -> Vec<Div> {
        let keep = self.render_pick(Pick::Keep, cx);
        let backups = &self.backups;
        let refresh = Button::new("backups-refresh", Tone::Secondary, "Refresh")
            .glyph(assets::RESCAN)
            .leading(self.ringed_on(Control::Refresh))
            .on_click(cx.listener(|this, _, _, cx| {
                this.clicked(Control::Refresh);
                this.list_backups(None, cx);
            }));
        let open = Control::Open(Folder::Backups);
        let open_folder = Button::new("backups-open", Tone::Secondary, "Open folder")
            .glyph(assets::FOLDER_OPEN)
            .leading(self.ringed_on(open))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.clicked(open);
                this.open_backups_folder(cx);
            }));
        let actions = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .child(self.track(Control::Refresh, refresh))
            .child(self.track(open, open_folder))
            .into_any_element();
        let line = |text: String, hex: u32| {
            div()
                .mb(px(12.))
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(color(hex))
                .child(text)
        };
        let outcome = backups.outcome.as_ref().map(|outcome| match outcome {
            Outcome::Restored(result) => div()
                .mb(px(12.))
                .rounded(px(8.))
                .border_1()
                .border_color(alpha(theme::SUCCESS, 0.3))
                .bg(alpha(theme::SUCCESS, 0.1))
                .px(px(12.))
                .py(px(8.))
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(color(theme::SUCCESS))
                .child(restored_line(result)),
            Outcome::Failed(error) => line(format!("Could not restore: {error}"), theme::DANGER),
        });
        let body = div()
            .children(
                self.folder_error
                    .clone()
                    .map(|error| line(format!("Could not open the folder: {error}"), theme::DANGER)),
            )
            .children(outcome)
            .children(
                self.refusal(Refused::Setting(SettingKey::BackupsKeep))
                    .map(|error| line(format!("Could not save the setting: {error}"), theme::DANGER)),
            )
            .child(setting_row(
                "Snapshots to keep",
                Some("Per launcher. Older ones go as new ones are taken; Kept snapshots never count.".into()),
                None,
                true,
                false,
                keep,
            ))
            .child(
                div().mb(px(-12.)).children(
                    LauncherId::ALL
                        .into_iter()
                        .enumerate()
                        .map(|(index, launcher)| self.render_section(index, launcher, cx)),
                ),
            );
        vec![panel(
            "Backups",
            Some("The launcher files One Launch changes, saved before every switch.".into()),
            Some(actions),
            body,
        )]
    }

    fn render_section(&self, index: usize, launcher: LauncherId, cx: &mut Context<Self>) -> Div {
        let backups = &self.backups;
        let list = backups.list(launcher);
        let open = list.is_some_and(|list| list.open);
        let snapshots = backups.shown(launcher);
        let count = if list
            .is_none_or(|list| list.loading && list.snapshots.is_none() && list.error.is_none())
        {
            "…".to_owned()
        } else {
            plural(snapshots.len(), "snapshot")
        };
        let control = Control::Section(launcher);
        let chevron = if open {
            assets::CHEVRON_DOWN
        } else {
            assets::CHEVRON_RIGHT
        };
        let head = div()
            .id(("backups-section", index))
            .flex()
            .w_full()
            .items_center()
            .gap(px(8.))
            .py(px(12.))
            .rounded(px(6.))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.clicked(control);
                this.toggle_section(launcher, cx);
            }))
            .child(icon(chevron, 14., theme::FAINT))
            .child(launcher_dot(launcher))
            .child(
                div()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(color(theme::TEXT))
                    .child(launcher.display_name()),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color(theme::FAINT))
                    .child(count),
            );
        let head = if self.ringed_on(control) {
            crate::widgets::ringed(head, Some(6.)).into_any_element()
        } else {
            head.into_any_element()
        };
        let small = |text: SharedString, hex: u32| {
            div()
                .pb(px(12.))
                .pl(px(22.))
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(color(hex))
                .child(text)
        };
        let body: Option<AnyElement> = match list {
            _ if !open => None,
            Some(List { error: Some(error), .. }) => Some(
                small(format!("Could not list backups: {error}").into(), theme::DANGER)
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .into_any_element(),
            ),
            Some(list) if snapshots.is_empty() => (!list.loading).then(|| {
                small(
                    "No snapshots yet. The first one is taken automatically before the first real switch.".into(),
                    theme::MUTED,
                )
                .into_any_element()
            }),
            None => None,
            Some(_) => {
                let last = snapshots.len() - 1;
                Some(
                    div()
                        .mb(px(8.))
                        .pl(px(22.))
                        .children(snapshots.iter().enumerate().map(|(row, snapshot)| {
                            self.render_snapshot(snapshot, row == last, cx)
                        }))
                        .into_any_element(),
                )
            }
        };
        div()
            .when(index + 1 < LauncherId::ALL.len(), |section| {
                section.border_b_1().border_color(alpha(theme::BORDER, 0.6))
            })
            .child(self.track(control, head))
            .children(body)
    }

    fn render_snapshot(
        &self,
        snapshot: &BackupSnapshot,
        last: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let launcher = snapshot.launcher;
        let local = local_time(cx);
        let when = date_time(&snapshot.created_at, local.offset, local.clock).unwrap_or_default();
        let kept = snapshot.pinned.clone().map(|pinned| {
            div()
                .id(SharedString::from(format!(
                    "kept-{}-{}",
                    launcher.as_str(),
                    snapshot.id
                )))
                .rounded(px(6.))
                .border_1()
                .border_color(alpha(theme::WARNING, 0.4))
                .bg(alpha(theme::WARNING, 0.1))
                .px(px(6.))
                .py(px(2.))
                .text_size(px(11.))
                .line_height(px(14.))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(color(theme::WARNING))
                .tooltip(tip(pinned))
                .child("Kept")
        });
        let reason = if snapshot.reason.is_empty() {
            "Snapshot"
        } else {
            snapshot.reason.as_str()
        };
        let meta = format!(
            "{}{reason} · {} · {} · ",
            snapshot
                .pinned
                .as_ref()
                .map_or_else(String::new, |pinned| format!("{pinned} · ")),
            plural(snapshot.files.len(), "file"),
            size(snapshot.bytes),
        );
        let seen = self.backups.seen_at(launcher, &snapshot.id);
        let button = seen.map(|seen| {
            let control = Control::Restore(launcher, seen);
            let id: ElementId =
                SharedString::from(format!("restore-{}-{}", launcher.as_str(), snapshot.id)).into();
            let button = Button::new(id, Tone::Secondary, "Restore")
                .glyph(assets::RETRY)
                .leading(self.ringed_on(control))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.clicked(control);
                    this.ask_restore(launcher, seen, cx);
                }));
            self.track(control, button)
        });
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .py(px(8.))
            .when(!last, |row| {
                row.border_b_1().border_color(alpha(theme::BORDER, 0.6))
            })
            .child(
                div()
                    .min_w_0()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_color(color(theme::TEXT))
                            .child(when)
                            .children(kept),
                    )
                    .child(
                        div()
                            .flex()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color(theme::MUTED))
                            .child(div().flex_none().child(meta))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .font_family(theme::MONO)
                                    .text_color(color(theme::FAINT))
                                    .child(snapshot.id.clone()),
                            ),
                    ),
            )
            .children(button)
    }

    pub(super) fn render_restore(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let dialog = self.backups.dialog.as_ref()?;
        let snapshot = &dialog.snapshot;
        let launcher = snapshot.launcher;
        let name = launcher.display_name();
        let local = local_time(cx);
        let when = date_time(&snapshot.created_at, local.offset, local.clock).unwrap_or_default();
        let mut files: Vec<String> = snapshot.files.clone();
        if launcher == LauncherId::Steam {
            files.push(
                r"HKCU\Software\Valve\Steam\AutoLoginUser and RememberPassword (as recorded)"
                    .to_owned(),
            );
        }
        let warning = if launcher == LauncherId::Steam {
            "Steam must be closed first. It rewrites loginusers.vdf when it exits, which would undo the restore.".to_owned()
        } else {
            format!(
                "{name} must be closed first. It rewrites its settings file while it runs, which would undo the restore."
            )
        };
        let busy = dialog.busy;
        let header = div()
            .border_b_1()
            .border_color(color(theme::BORDER))
            .px(px(20.))
            .py(px(16.))
            .child(
                div()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(color(theme::TEXT_STRONG))
                    .child(format!("Restore {name} files from {when}?")),
            )
            .child(
                div()
                    .mt(px(2.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color(theme::MUTED))
                    .child(if snapshot.reason.is_empty() {
                        "Snapshot".to_owned()
                    } else {
                        snapshot.reason.clone()
                    }),
            );
        let body = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .px(px(20.))
            .py(px(16.))
            .text_color(color(theme::TEXT))
            .child("This overwrites the current copies with the ones in the snapshot:")
            .child(
                div()
                    .flex()
                    .flex_col()
                    .pl(px(8.))
                    .font_family(theme::MONO)
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color(theme::MUTED))
                    .children(files.into_iter().map(|file| {
                        div()
                            .flex()
                            .items_start()
                            .gap(px(8.))
                            .child(div().flex_none().child("•"))
                            .child(div().flex_1().min_w_0().child(file))
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(alpha(theme::WARNING, 0.3))
                    .bg(alpha(theme::WARNING, 0.1))
                    .px(px(12.))
                    .py(px(8.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color(theme::WARNING))
                    .child(icon(assets::UPDATE, 14., theme::WARNING).mt(px(1.)))
                    .child(div().flex_1().min_w_0().child(warning)),
            )
            .children(dialog.error.clone().map(|error| {
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color(theme::DANGER))
                    .child(error)
            }));
        let footer = div()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(8.))
            .border_t_1()
            .border_color(color(theme::BORDER))
            .px(px(20.))
            .py(px(12.))
            .child(
                Button::new("restore-cancel", Tone::Ghost, "Cancel")
                    .disabled(busy)
                    .leading(!busy && !dialog.restore_leads)
                    .on_click(cx.listener(|this, _, _, cx| this.close_restore(cx))),
            )
            .child(
                Button::new(
                    "restore-confirm",
                    Tone::Danger,
                    if busy {
                        "Restoring…"
                    } else {
                        "Restore this snapshot"
                    },
                )
                .glyph(assets::RETRY)
                .disabled(busy)
                .leading(!busy && dialog.restore_leads)
                .on_click(cx.listener(|this, click: &ClickEvent, _, cx| {
                    if this
                        .backups
                        .dialog
                        .as_ref()
                        .is_some_and(|dialog| dialog.appeared.takes(click))
                    {
                        this.restore(cx);
                    }
                })),
            );
        let card = div()
            .id("restore-dialog")
            .occlude()
            .flex()
            .flex_col()
            .w_full()
            .max_w(px(512.))
            .max_h_full()
            .overflow_hidden()
            .rounded(px(12.))
            .border_1()
            .border_color(color(theme::BORDER))
            .bg(color(theme::SURFACE))
            .shadow(panel_shadow())
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(header)
            .child(body)
            .child(footer);
        Some(
            div()
                .id("restore-backdrop")
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .p(px(24.))
                .bg(alpha(theme::BG, 0.7))
                .on_click(cx.listener(|this, click: &ClickEvent, _, cx| {
                    if click.click_count() == 1 {
                        this.close_restore(cx);
                    }
                }))
                .child(card),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_counts_read_as_the_electron_app_writes_them() {
        assert_eq!(size(875), "875 B");
        assert_eq!(size(2048), "2.0 KB");
        assert_eq!(size(1023), "1023 B");
        assert_eq!(size(5 * 1024 * 1024 + 512 * 1024), "5.5 MB");
        assert_eq!(plural(1, "file"), "1 file");
        assert_eq!(plural(0, "snapshot"), "0 snapshots");
    }

    #[test]
    fn a_restore_names_its_files_and_what_it_put_back_in_the_registry() {
        let mut result = RestoreResult {
            launcher: LauncherId::Steam,
            id: "2026-11-06T16-46-25.998Z".to_owned(),
            files: vec!["config/loginusers.vdf".to_owned()],
            registry: vec![
                "AutoLoginUser = user_1".to_owned(),
                "RememberPassword (removed)".to_owned(),
            ],
        };
        assert_eq!(
            restored_line(&result),
            "Restored snapshot 2026-11-06T16-46-25.998Z: 1 file, registry AutoLoginUser = user_1, RememberPassword (removed). Start the launcher to check it signed in as expected."
        );
        result.registry.clear();
        result.files.push("other".to_owned());
        assert_eq!(
            restored_line(&result),
            "Restored snapshot 2026-11-06T16-46-25.998Z: 2 files. Start the launcher to check it signed in as expected."
        );
    }
}
