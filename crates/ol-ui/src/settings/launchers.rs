use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{AnyElement, ClickEvent, Context, Div, SharedString, div, px};
use ol_api::ApiError;
use ol_api::library::time_of_day;
use ol_model::{HealthCheck, HealthStatus, LauncherId, LauncherStatus};
use ol_ui_kit::theme::{self, alpha, color};

use super::{Control, SettingsTab, SettingsView};
use crate::assets;
use crate::local_time;
use crate::picker::{Picked, pick_program};
use crate::widgets::{Button, PillTone, Tone, icon, launcher_dot, panel, pill, spot_of};

// The last answer of this run, shown at once on a later visit while a fresh one is read, as the Electron app keeps it in its store.
pub type KnownStatuses = Rc<RefCell<Option<Vec<LauncherStatus>>>>;

#[derive(Default)]
pub(super) struct Launchers {
    rows: Option<Vec<LauncherStatus>>,
    loading: bool,
    // Asked for while a read ran: one more read follows it, and the running one's answer is passed over.
    again: bool,
    error: Option<SharedString>,
    open: HashSet<LauncherId>,
    acts: HashMap<LauncherId, Act>,
    made: u64,
}

// A Locate or a Reset a row asked for. `stale`: it changed the folder, and the read it asked for has not answered yet.
#[derive(Default)]
struct Act {
    key: u64,
    busy: bool,
    stale: bool,
    error: Option<SharedString>,
}

impl Launchers {
    fn act(&self, launcher: LauncherId) -> Option<&Act> {
        self.acts.get(&launcher)
    }

    fn working(&self, launcher: LauncherId) -> bool {
        self.act(launcher)
            .is_some_and(|act| act.busy || (act.stale && self.loading))
    }

    fn rows(&self) -> &[LauncherStatus] {
        self.rows.as_deref().unwrap_or_default()
    }

    fn row(&self, launcher: LauncherId) -> Option<&LauncherStatus> {
        self.rows().iter().find(|row| row.launcher == launcher)
    }

    // The newest act of a row: an older one's answer finds another key.
    fn begin(&mut self, launcher: LauncherId) -> u64 {
        self.made += 1;
        let act = self.acts.entry(launcher).or_default();
        *act = Act {
            key: self.made,
            busy: true,
            stale: act.stale,
            error: None,
        };
        self.made
    }

    fn shows_locate(&self, row: &LauncherStatus) -> bool {
        row.implemented && (self.open.contains(&row.launcher) || row.install.is_none())
    }

    pub(super) fn controls(&self, controls: &mut Vec<Control>) {
        if !self.loading {
            controls.push(Control::Recheck);
        }
        for row in self.rows() {
            let launcher = row.launcher;
            if !row.implemented {
                continue;
            }
            controls.push(Control::Disclose(launcher));
            if self.working(launcher) {
                continue;
            }
            if self.shows_locate(row) {
                controls.push(Control::Locate(launcher));
            }
            if self.open.contains(&launcher) && row.chosen_path.is_some() {
                controls.push(Control::ResetPath(launcher));
            }
        }
    }
}

impl SettingsView {
    // As the Electron app mounts the panel afresh: every row folded, nothing a row asked for still counted, and a fresh read.
    pub(super) fn enter_launchers(&mut self, cx: &mut Context<Self>) {
        let launchers = &mut self.launchers;
        launchers.open.clear();
        launchers.acts.clear();
        if launchers.rows.is_none() {
            launchers.rows = self.known.borrow().clone();
        }
        self.read_statuses(cx);
    }

    // A status read passes over all four adapters on the backend's one thread, so reads never pile up there.
    pub(super) fn read_statuses(&mut self, cx: &mut Context<Self>) {
        let launchers = &mut self.launchers;
        if launchers.loading {
            launchers.again = true;
            return;
        }
        launchers.loading = true;
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let statuses = backend.launcher_statuses().await;
            this.update(cx, |this, cx| this.statuses_read(statuses, cx))
                .ok();
        })
        .detach();
        cx.notify();
    }

    fn statuses_read(
        &mut self,
        statuses: Result<Vec<LauncherStatus>, ApiError>,
        cx: &mut Context<Self>,
    ) {
        let launchers = &mut self.launchers;
        launchers.loading = false;
        if std::mem::take(&mut launchers.again) {
            self.read_statuses(cx);
            return;
        }
        match statuses {
            Ok(rows) => {
                *self.known.borrow_mut() = Some(rows.clone());
                launchers.rows = Some(rows);
                launchers.error = None;
            }
            Err(error) => launchers.error = Some(error.message.into()),
        }
        for act in launchers.acts.values_mut() {
            act.stale = false;
        }
        cx.notify();
    }

    pub(super) fn recheck(&mut self, cx: &mut Context<Self>) {
        if !self.launchers.loading {
            self.read_statuses(cx);
        }
    }

    pub(super) fn disclose(&mut self, launcher: LauncherId, cx: &mut Context<Self>) {
        let launchers = &mut self.launchers;
        if !launchers.row(launcher).is_some_and(|row| row.implemented) {
            return;
        }
        if !launchers.open.remove(&launcher) {
            launchers.open.insert(launcher);
        }
        cx.notify();
    }

    // Refused during a switch before the dialog opens, as the Electron app's requireIdle refuses it; the backend asks again once a file is chosen.
    pub(super) fn locate(&mut self, launcher: LauncherId, cx: &mut Context<Self>) {
        let launchers = &self.launchers;
        if launchers.working(launcher)
            || !launchers
                .row(launcher)
                .is_some_and(|row| launchers.shows_locate(row))
        {
            return;
        }
        let key = self.launchers.begin(launcher);
        if let Err(refused) = self.backend.require_idle() {
            self.path_changed(launcher, key, Some(Err(refused.message)), cx);
            return;
        }
        let picked = pick_program(cx);
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let located = match picked.await {
                Picked::Cancelled => None,
                Picked::Failed(error) => {
                    Some(Err(format!("Could not open the file dialog: {error}")))
                }
                Picked::Path(path) => Some(
                    backend
                        .locate_launcher(launcher, &path)
                        .await
                        .map(|_| ())
                        .map_err(|error| error.message),
                ),
            };
            this.update(cx, |this, cx| this.path_changed(launcher, key, located, cx))
                .ok();
        })
        .detach();
        cx.notify();
    }

    pub(super) fn reset_path(&mut self, launcher: LauncherId, cx: &mut Context<Self>) {
        let launchers = &self.launchers;
        if launchers.working(launcher)
            || !launchers.open.contains(&launcher)
            || !launchers
                .row(launcher)
                .is_some_and(|row| row.chosen_path.is_some())
        {
            return;
        }
        let key = self.launchers.begin(launcher);
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let reset = backend
                .reset_launcher_path(launcher)
                .await
                .map_err(|error| error.message);
            this.update(cx, |this, cx| {
                this.path_changed(launcher, key, Some(reset), cx)
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    // `None`: the dialog was cancelled. A changed folder is read again whichever visit of the tab asked for it; only the row that asked, and only its newest act, shows the answer.
    fn path_changed(
        &mut self,
        launcher: LauncherId,
        key: u64,
        changed: Option<Result<(), String>>,
        cx: &mut Context<Self>,
    ) {
        let succeeded = matches!(changed, Some(Ok(())));
        if let Some(act) = self
            .launchers
            .acts
            .get_mut(&launcher)
            .filter(|act| act.key == key)
        {
            act.busy = false;
            match changed {
                Some(Ok(())) => act.stale = true,
                Some(Err(message)) => act.error = Some(message.into()),
                None => {}
            }
        }
        if succeeded && self.tab.get() == SettingsTab::Launchers {
            self.read_statuses(cx);
        }
        cx.notify();
    }

    pub(super) fn render_launchers_panel(&self, cx: &mut Context<Self>) -> Div {
        let launchers = &self.launchers;
        let recheck = Button::new("recheck", Tone::Secondary, "Re-check")
            .glyph(assets::RESCAN)
            .turning(launchers.loading)
            .disabled(launchers.loading)
            .leading(self.ringed_on(Control::Recheck))
            .on_click(cx.listener(|this, _, _, cx| {
                this.clicked(Control::Recheck);
                this.recheck(cx);
            }));
        let recheck = self.track(Control::Recheck, recheck);
        let body = div()
            .children(launchers.error.clone().map(|error| {
                div()
                    .text_color(color(theme::DANGER))
                    .child(format!("Could not check launchers: {error}"))
            }))
            .map(|body| match &launchers.rows {
                Some(rows) => {
                    let last = rows.len().saturating_sub(1);
                    body.children(
                        rows.iter()
                            .enumerate()
                            .map(|(index, row)| self.render_row(index, row, index == last, cx)),
                    )
                }
                None if launchers.error.is_none() => body.children(
                    (0..LauncherId::ALL.len())
                        .map(|index| skeleton_row(index == 0, index == LauncherId::ALL.len() - 1)),
                ),
                None => body,
            });
        panel(
            "Launchers",
            Some("Which launchers are installed and set up the way One Launch expects.".into()),
            Some(recheck),
            body,
        )
    }

    fn render_row(
        &self,
        index: usize,
        row: &LauncherStatus,
        last: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let launchers = &self.launchers;
        let launcher = row.launcher;
        let open = launchers.open.contains(&launcher);
        let working = launchers.working(launcher);
        let error = launchers.act(launcher).and_then(|act| act.error.clone());
        let disclose = Control::Disclose(launcher);
        let chevron = if !row.implemented {
            div().flex_none().w(px(14.)).into_any_element()
        } else if open {
            icon(assets::CHEVRON_DOWN, 14., theme::FAINT).into_any_element()
        } else {
            icon(assets::CHEVRON_RIGHT, 14., theme::FAINT).into_any_element()
        };
        let place = row.install.as_ref().map_or_else(
            || SharedString::from("Not installed"),
            |install| install.install_dir.clone().into(),
        );
        // Ringed and tracked in place rather than wrapped, so the row's folder can still shrink to its ellipsis.
        let head = div()
            .id(("launcher-row", index))
            .relative()
            .flex()
            .min_w_0()
            .items_center()
            .gap(px(8.))
            .when(self.ringed_on(disclose), |head| {
                head.m(px(-4.))
                    .p(px(2.))
                    .border_2()
                    .border_color(color(theme::ACCENT))
                    .rounded(px(10.))
            })
            .children((self.focus == Some(disclose)).then(|| spot_of(&self.focused)))
            .when(row.implemented, |head| {
                head.cursor_pointer()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.clicked(disclose);
                        this.disclose(launcher, cx);
                    }))
            })
            .child(chevron)
            .child(launcher_dot(launcher))
            .child(
                div()
                    .min_w_0()
                    .child(div().text_color(color(theme::TEXT)).child(row.display_name))
                    .child(
                        div()
                            .truncate()
                            .font_family(theme::MONO)
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color(theme::MUTED))
                            .child(place),
                    ),
            );
        let mut pills: Vec<AnyElement> = Vec::new();
        if row.implemented && row.install.is_none() && !open {
            pills.push(self.locate_button(index, launcher, working, cx));
        }
        if row.attention.is_some() {
            pills.push(
                pill(PillTone::Fail, Some(assets::UPDATE), "Needs attention").into_any_element(),
            );
        }
        if let Some(health) = &row.health {
            pills.push(
                pill(health_tone(health.status), None, health.status.label()).into_any_element(),
            );
        }
        if let Some(running) = row.running {
            pills.push(
                pill(
                    PillTone::Info,
                    None,
                    if running { "Running" } else { "Not running" },
                )
                .into_any_element(),
            );
        }
        div()
            .py(px(10.))
            .when(index == 0, |row| row.pt_0())
            .when(last, |row| row.pb_0())
            .when(!last, |row| {
                row.border_b_1().border_color(alpha(theme::BORDER, 0.6))
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(24.))
                    .child(head)
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap(px(8.))
                            .children(pills),
                    ),
            )
            .when(!open, |block| {
                block.children(error.clone().map(|error| {
                    div()
                        .mt(px(4.))
                        .ml(px(22.))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color(theme::DANGER))
                        .child(error)
                }))
            })
            .when(open, |block| {
                block.child(self.render_details(index, row, working, error, cx))
            })
    }

    fn render_details(
        &self,
        index: usize,
        row: &LauncherStatus,
        working: bool,
        error: Option<SharedString>,
        cx: &mut Context<Self>,
    ) -> Div {
        let launcher = row.launcher;
        let small = |text: SharedString, hex: u32| {
            div()
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(color(hex))
                .child(text)
        };
        let note: SharedString = if working {
            "Checking…".into()
        } else {
            row.location_note().into()
        };
        let reset = row.chosen_path.is_some().then(|| {
            let control = Control::ResetPath(launcher);
            let button = Button::new(("reset-path", index), Tone::Ghost, "Reset")
                .disabled(working)
                .leading(self.ringed_on(control))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.clicked(control);
                    this.reset_path(launcher, cx);
                }));
            self.track(control, button)
        });
        let local = local_time(cx);
        let checked = row.health.as_ref().map(|health| {
            let at = time_of_day(&health.checked_at, local.offset, local.clock).unwrap_or_default();
            div()
                .mt(px(8.))
                .text_size(px(11.))
                .line_height(px(16.))
                .text_color(color(theme::FAINT))
                .child(format!("Checked {at}"))
        });
        let checks = row
            .health
            .as_ref()
            .map_or(&[][..], |health| &health.checks[..]);
        div()
            .mt(px(8.))
            .ml(px(22.))
            .rounded(px(8.))
            .border_1()
            .border_color(color(theme::BORDER))
            .bg(alpha(theme::SURFACE_2, 0.5))
            .px(px(12.))
            .py(px(8.))
            .children(
                row.attention
                    .clone()
                    .map(|attention| small(attention.into(), theme::DANGER).mb(px(8.))),
            )
            .child(
                div()
                    .mb(px(8.))
                    .pb(px(8.))
                    .border_b_1()
                    .border_color(alpha(theme::BORDER, 0.6))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(small(note, theme::MUTED).min_w_0())
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap(px(8.))
                            .child(self.locate_button(index, launcher, working, cx))
                            .children(reset),
                    ),
            )
            .children(error.map(|error| small(error, theme::DANGER).mb(px(8.))))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .children(checks.iter().map(render_check)),
            )
            .children(checked)
    }

    fn locate_button(
        &self,
        index: usize,
        launcher: LauncherId,
        working: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let control = Control::Locate(launcher);
        let button = Button::new(("locate", index), Tone::Secondary, "Locate…")
            .glyph(if working {
                assets::SPINNER
            } else {
                assets::FOLDER_SEARCH
            })
            .turning(working)
            .disabled(working)
            .leading(self.ringed_on(control))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.clicked(control);
                this.locate(launcher, cx);
            }));
        self.track(control, button)
    }
}

const fn health_tone(status: HealthStatus) -> PillTone {
    match status {
        HealthStatus::Ok => PillTone::Ok,
        HealthStatus::Warn => PillTone::Warn,
        HealthStatus::Fail => PillTone::Fail,
        HealthStatus::Absent => PillTone::Info,
    }
}

fn render_check(check: &HealthCheck) -> Div {
    let dot = match check.status {
        HealthStatus::Ok => theme::SUCCESS,
        HealthStatus::Warn => theme::WARNING,
        HealthStatus::Absent => theme::FAINT,
        HealthStatus::Fail => theme::DANGER,
    };
    div()
        .flex()
        .items_start()
        .gap(px(8.))
        .text_size(px(12.))
        .line_height(px(16.))
        .child(
            div()
                .flex_none()
                .mt(px(4.))
                .size(px(6.))
                .rounded_full()
                .bg(color(dot)),
        )
        .child(
            div()
                .flex_none()
                .w(px(160.))
                .text_color(color(theme::MUTED))
                .child(check.label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_color(color(theme::TEXT))
                .child(check.detail.clone()),
        )
}

// The height of a row, so the panel does not grow when the rows come.
fn skeleton_row(first: bool, last: bool) -> Div {
    let bar = |width: f32, height: f32, hex: u32| {
        div()
            .flex_none()
            .w(px(width))
            .h(px(height))
            .rounded(px(4.))
            .bg(color(hex))
    };
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(24.))
        .py(px(10.))
        .when(first, |row| row.pt_0())
        .when(last, |row| row.pb_0())
        .when(!last, |row| {
            row.border_b_1().border_color(alpha(theme::BORDER, 0.6))
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().flex_none().w(px(14.)))
                .child(
                    div()
                        .flex_none()
                        .size(px(8.))
                        .rounded_full()
                        .bg(color(theme::SURFACE_3)),
                )
                .child(
                    div()
                        .child(bar(96., 16., theme::SURFACE_3))
                        .child(bar(192., 12., theme::SURFACE_2).mt(px(4.))),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(bar(64., 20., theme::SURFACE_2).rounded(px(6.)))
                .child(bar(80., 20., theme::SURFACE_2).rounded(px(6.))),
        )
}
