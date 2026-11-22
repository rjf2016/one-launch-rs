// The setup guide: five steps for a data folder nobody has set up, also opened again from Settings › General. The words and the rules are the Electron app's `SetupGuide.tsx` and `SetupGuideSteps.tsx`.

use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    AnyElement, BoxShadow, ClickEvent, Context, Div, Entity, EventEmitter, FontWeight, Keystroke,
    ScrollHandle, SharedString, Stateful, Task, div, hsla, linear_color_stop, linear_gradient, px,
    relative,
};
use ol_api::launch::{PhaseId, PhaseNames, phase_label};
use ol_api::launchers::ImportResult;
use ol_api::{ApiError, Backend};
use ol_model::{LauncherId, LauncherStatus, RememberedAccount, SettingKey};
use ol_ui_kit::theme::{self, alpha, color};

use crate::assets;
use crate::library::LibraryView;
use crate::picker::{Picked, pick_program};
use crate::widgets::{
    Appeared, Button, Flash, FlashTone, PillTone, Tone, icon, launcher_hex, panel_shadow, pill,
    spinner,
};

const STEPS: [&str; 5] = [
    "Welcome",
    "Your launchers",
    "How Play works",
    "Your accounts",
    "Ready",
];
const WELCOME: usize = 0;
const LAUNCHERS: usize = 1;
const PLAY: usize = 2;
const ACCOUNTS: usize = 3;
const READY: usize = 4;
// The two that keep a list of the accounts that signed in on the PC.
const IMPORT_LAUNCHERS: [LauncherId; 2] = [LauncherId::Steam, LauncherId::Battlenet];
const TICK: Duration = Duration::from_millis(850);
// The account in the welcome's picture changes every three ticks, close to the Electron app's 2.5 s.
const SWAP_TICKS: u32 = 3;
const DEMO_STAGES: usize = 5 + 1 + 3;
const IMPORTED_FLASH_STAYS: Duration = Duration::from_secs(4);
const CARD_WIDTH: f32 = 832.;
const CARD_HEIGHT: f32 = 580.;

// Where the guide leaves to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuideExit {
    Stay,
    Library,
    Accounts,
    NewAccount(LauncherId),
}

// The guide is done: `firstRun.dismissed` was written, or could not be, and the guide is to close and go where the user chose.
pub struct GuideDone(pub GuideExit);

enum Statuses {
    Loading,
    Failed(SharedString),
    Ready(Vec<LauncherStatus>),
}

enum Remembered {
    Loading(Vec<RememberedAccount>),
    Failed(SharedString),
    Ready(Vec<RememberedAccount>),
}

impl Remembered {
    fn entries(&self) -> &[RememberedAccount] {
        match self {
            Self::Loading(entries) | Self::Ready(entries) => entries,
            Self::Failed(_) => &[],
        }
    }
}

type Key = (LauncherId, String);

fn key_of(launcher: LauncherId, entry: &RememberedAccount) -> Key {
    let id = if entry.launcher_uid.is_empty() {
        &entry.login
    } else {
        &entry.launcher_uid
    };
    (launcher, id.clone())
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

enum Primary {
    Next,
    ImportThenNext,
    Finish(GuideExit),
}

pub struct SetupGuide {
    backend: Backend,
    library: Entity<LibraryView>,
    step: usize,
    statuses: Statuses,
    reads: u64,
    games: Option<usize>,
    preview: bool,
    accounts: Option<usize>,
    remembered: Vec<(LauncherId, Remembered)>,
    selected: Vec<Key>,
    imported: Vec<Key>,
    importing: bool,
    closing: bool,
    by_hand: Option<LauncherId>,
    locating: Option<LauncherId>,
    locate_errors: Vec<(LauncherId, SharedString)>,
    flash: Option<Flash>,
    ticks: u32,
    body: ScrollHandle,
    appeared: Appeared,
    _ticking: Task<()>,
}

impl EventEmitter<GuideDone> for SetupGuide {}

impl SetupGuide {
    pub fn new(backend: Backend, library: Entity<LibraryView>, cx: &mut Context<Self>) -> Self {
        let ticking = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                if this.update(cx, |this, cx| this.tick(cx)).is_err() {
                    break;
                }
            }
        });
        let mut guide = Self {
            backend,
            library,
            step: WELCOME,
            statuses: Statuses::Loading,
            reads: 0,
            games: None,
            preview: false,
            accounts: None,
            remembered: Vec::new(),
            selected: Vec::new(),
            imported: Vec::new(),
            importing: false,
            closing: false,
            by_hand: None,
            locating: None,
            locate_errors: Vec::new(),
            flash: None,
            ticks: 0,
            body: ScrollHandle::new(),
            appeared: Appeared::now(),
            _ticking: ticking,
        };
        guide.read_statuses(cx);
        let backend = guide.backend.clone();
        cx.spawn(async move |this, cx| {
            if let Ok(settings) = backend.settings().await {
                this.update(cx, |this, cx| {
                    this.preview = settings.toggle(SettingKey::DryRun);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
        guide
    }

    fn busy(&self) -> bool {
        self.importing || self.closing
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        self.ticks = self.ticks.wrapping_add(1);
        if self.games.is_none() && self.library.read(cx).scanned() {
            self.count_games(cx);
        }
        if matches!(self.step, WELCOME | PLAY) {
            cx.notify();
        }
    }

    fn count_games(&mut self, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            if let Ok(library) = backend.library().await {
                this.update(cx, |this, cx| {
                    this.games = Some(library.games.len());
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    // The launchers' status, and what the two that keep a list remember once it is in. An answer to an earlier read is dropped.
    fn read_statuses(&mut self, cx: &mut Context<Self>) {
        self.reads += 1;
        let (read, backend) = (self.reads, self.backend.clone());
        cx.spawn(async move |this, cx| {
            let statuses = backend.launcher_statuses().await;
            this.update(cx, |this, cx| this.statuses_read(read, statuses, cx))
                .ok();
        })
        .detach();
    }

    fn statuses_read(
        &mut self,
        read: u64,
        statuses: Result<Vec<LauncherStatus>, ApiError>,
        cx: &mut Context<Self>,
    ) {
        if read != self.reads {
            return;
        }
        let statuses = match statuses {
            Ok(statuses) => statuses,
            Err(error) => {
                self.statuses = Statuses::Failed(error.message.into());
                return cx.notify();
            }
        };
        for launcher in IMPORT_LAUNCHERS {
            let installed = statuses
                .iter()
                .any(|status| status.launcher == launcher && status.install.is_some());
            if !installed {
                continue;
            }
            let kept = self
                .remembered
                .iter()
                .position(|(held, _)| *held == launcher)
                .map(|at| self.remembered.remove(at).1.entries().to_vec())
                .unwrap_or_default();
            self.remembered.push((launcher, Remembered::Loading(kept)));
            let backend = self.backend.clone();
            cx.spawn(async move |this, cx| {
                let answer = backend.remembered_accounts(launcher).await;
                this.update(cx, |this, cx| {
                    this.remembered_read(read, launcher, answer, cx)
                })
                .ok();
            })
            .detach();
        }
        self.remembered.sort_by_key(|(launcher, _)| {
            IMPORT_LAUNCHERS.iter().position(|known| known == launcher)
        });
        self.statuses = Statuses::Ready(statuses);
        cx.notify();
    }

    fn remembered_read(
        &mut self,
        read: u64,
        launcher: LauncherId,
        answer: Result<Vec<RememberedAccount>, ApiError>,
        cx: &mut Context<Self>,
    ) {
        if read != self.reads {
            return;
        }
        let Some((_, list)) = self
            .remembered
            .iter_mut()
            .find(|(held, _)| *held == launcher)
        else {
            return;
        };
        match answer {
            Ok(entries) => {
                self.selected.retain(|(held, _)| *held != launcher);
                for entry in &entries {
                    let key = key_of(launcher, entry);
                    if entry.linked_account_id.is_none() && !self.imported.contains(&key) {
                        self.selected.push(key);
                    }
                }
                *list = Remembered::Ready(entries);
            }
            Err(error) => *list = Remembered::Failed(error.message.into()),
        }
        cx.notify();
    }

    fn installed(&self) -> Option<&[LauncherStatus]> {
        match &self.statuses {
            Statuses::Ready(statuses) => Some(statuses),
            Statuses::Loading | Statuses::Failed(_) => None,
        }
    }

    fn found(&self) -> Option<usize> {
        self.installed().map(|statuses| {
            statuses
                .iter()
                .filter(|status| status.install.is_some())
                .count()
        })
    }

    fn locate(&mut self, launcher: LauncherId, cx: &mut Context<Self>) {
        if self.locating.is_some() || self.busy() {
            return;
        }
        self.locate_errors.retain(|(held, _)| *held != launcher);
        if let Err(refused) = self.backend.require_idle() {
            self.locate_errors.push((launcher, refused.message.into()));
            return cx.notify();
        }
        self.locating = Some(launcher);
        let picked = pick_program(cx);
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let located = match picked.await {
                Picked::Cancelled => Ok(false),
                Picked::Failed(error) => Err(format!("Could not open the file dialog: {error}")),
                Picked::Path(path) => backend
                    .locate_launcher(launcher, &path)
                    .await
                    .map(|_| true)
                    .map_err(|error| error.message),
            };
            this.update(cx, |this, cx| {
                this.locating = None;
                match located {
                    Ok(true) => {
                        this.read_statuses(cx);
                        this.count_games(cx);
                    }
                    Ok(false) => {}
                    Err(error) => this.locate_errors.push((launcher, error.into())),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn toggle(&mut self, key: Key, cx: &mut Context<Self>) {
        if self.importing || self.imported.contains(&key) {
            return;
        }
        if let Some(at) = self.selected.iter().position(|held| *held == key) {
            self.selected.remove(at);
        } else {
            self.selected.push(key);
        }
        cx.notify();
    }

    // Imports what is ticked, launcher by launcher. `then` moves on once every import went through.
    fn run_import(&mut self, then: bool, cx: &mut Context<Self>) {
        if self.busy() || self.selected.is_empty() {
            return;
        }
        self.importing = true;
        self.flash = None;
        let chosen: Vec<(LauncherId, Vec<String>)> = IMPORT_LAUNCHERS
            .iter()
            .map(|launcher| {
                let ids = self
                    .selected
                    .iter()
                    .filter(|(held, _)| held == launcher)
                    .map(|(_, id)| id.clone())
                    .collect::<Vec<_>>();
                (*launcher, ids)
            })
            .filter(|(_, ids)| !ids.is_empty())
            .collect();
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let mut total = 0;
            let mut failure = None;
            for (launcher, ids) in chosen {
                let result: Result<ImportResult, ApiError> =
                    backend.import_accounts(launcher, ids.clone()).await;
                match result {
                    Ok(result) => {
                        total += result.accounts.len();
                        this.update(cx, |this, _| {
                            let keys: Vec<Key> = ids.into_iter().map(|id| (launcher, id)).collect();
                            this.selected.retain(|key| !keys.contains(key));
                            this.imported.extend(keys);
                        })
                        .ok();
                    }
                    Err(error) => {
                        failure = Some(error.message);
                        break;
                    }
                }
            }
            this.update(cx, |this, cx| {
                this.importing = false;
                this.flash = Some(match &failure {
                    Some(error) => Flash::new(
                        format!("Could not import: {error}"),
                        FlashTone::Error,
                        None,
                        |this: &mut Self| &mut this.flash,
                        cx,
                    ),
                    None => Flash::new(
                        if total == 0 {
                            "Nothing new to import".to_owned()
                        } else {
                            format!("Imported {}", plural(total, "account", "accounts"))
                        },
                        FlashTone::Success,
                        Some(IMPORTED_FLASH_STAYS),
                        |this: &mut Self| &mut this.flash,
                        cx,
                    ),
                });
                if failure.is_none() && then && this.step == ACCOUNTS {
                    this.go(READY, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn go(&mut self, step: usize, cx: &mut Context<Self>) {
        if self.busy() || step == self.step || step >= STEPS.len() {
            return;
        }
        self.step = step;
        self.body.set_offset(gpui::point(px(0.), px(0.)));
        self.appeared = Appeared::now();
        if step == READY {
            self.accounts = None;
            let backend = self.backend.clone();
            cx.spawn(async move |this, cx| {
                let accounts = backend.accounts().await;
                this.update(cx, |this, cx| {
                    if this.step == READY {
                        this.accounts = accounts.ok().map(|accounts| accounts.len());
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
        cx.notify();
    }

    // Every way out writes that the guide was seen, then closes it, also when the write failed: it then comes back at the next start.
    fn finish(&mut self, exit: GuideExit, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        self.closing = true;
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let _ = backend
                .set_setting(SettingKey::FirstRunDismissed, "true".to_owned())
                .await;
            this.update(cx, |_, cx| cx.emit(GuideDone(exit))).ok();
        })
        .detach();
        cx.notify();
    }

    fn primary(&self) -> (SharedString, &'static str, Primary) {
        if self.step == READY {
            return match self.by_hand {
                Some(launcher) => (
                    format!("Add a {} account", launcher.display_name()).into(),
                    assets::USER_PLUS,
                    Primary::Finish(GuideExit::NewAccount(launcher)),
                ),
                None => (
                    "Open Library".into(),
                    assets::LIBRARY,
                    Primary::Finish(GuideExit::Library),
                ),
            };
        }
        let pending = if self.step == ACCOUNTS {
            self.selected.len()
        } else {
            0
        };
        if pending > 0 {
            return (
                format!("Import {pending} and continue").into(),
                assets::ARROW_RIGHT,
                Primary::ImportThenNext,
            );
        }
        let label = if self.step == WELCOME {
            "Get started"
        } else {
            "Continue"
        };
        (label.into(), assets::ARROW_RIGHT, Primary::Next)
    }

    fn secondary(&self) -> Option<(&'static str, GuideExit)> {
        if self.step != READY {
            return Some(("Skip", GuideExit::Stay));
        }
        if self.by_hand.is_some() {
            return Some(("Open Library", GuideExit::Library));
        }
        (self.accounts == Some(0)).then_some(("Go to Accounts", GuideExit::Accounts))
    }

    fn press(&mut self, action: Primary, cx: &mut Context<Self>) {
        match action {
            Primary::Next => self.go(self.step + 1, cx),
            Primary::ImportThenNext => self.run_import(true, cx),
            Primary::Finish(exit) => self.finish(exit, cx),
        }
    }

    // The Shell hands each key here while the guide is up, after the question and the recovery dialog: every key is the guide's but those held with Alt or the Windows key, which go on to Windows.
    pub fn on_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let held = keystroke.modifiers;
        if held.alt || held.platform {
            return false;
        }
        let plain = !held.control && !held.shift && !held.function;
        match keystroke.key.as_str() {
            "escape" if plain => self.finish(GuideExit::Stay, cx),
            "enter" if plain && !self.busy() => {
                let (_, _, action) = self.primary();
                self.press(action, cx);
            }
            _ => {}
        }
        true
    }

    fn render_dots(&self) -> Div {
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .children((0..STEPS.len()).map(|at| {
                        let (width, fill) = if at == self.step {
                            (28., color(theme::ACCENT).into())
                        } else if at < self.step {
                            (10., alpha(theme::ACCENT, 0.5))
                        } else {
                            (10., color(theme::BORDER_STRONG).into())
                        };
                        div().h(px(6.)).w(px(width)).rounded_full().bg(fill)
                    })),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color(theme::FAINT))
                    .child(format!("{} of {}", self.step + 1, STEPS.len())),
            )
    }

    fn render_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        match self.step {
            WELCOME => self.render_welcome().into_any_element(),
            LAUNCHERS => self.render_launchers(cx).into_any_element(),
            PLAY => self.render_play().into_any_element(),
            ACCOUNTS => self.render_accounts(cx).into_any_element(),
            _ => self.render_ready().into_any_element(),
        }
    }

    fn render_welcome(&self) -> Div {
        let account = if (self.ticks / SWAP_TICKS).is_multiple_of(2) {
            "Main"
        } else {
            "Alt"
        };
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .min_h(relative(1.))
            .text_center()
            .child(
                div()
                    .mb(px(20.))
                    .rounded_full()
                    .shadow(vec![
                        BoxShadow::new(px(0.), px(0.), alpha(theme::ACCENT, 0.25))
                            .blur_radius(px(40.))
                            .spread_radius(px(8.)),
                    ])
                    .child(icon(assets::BRAND, 64., theme::ACCENT)),
            )
            .child(
                div()
                    .text_size(px(24.))
                    .line_height(px(32.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(color(theme::TEXT_STRONG))
                    .child("Welcome to One Launch"),
            )
            .child(
                div()
                    .mt(px(8.))
                    .max_w(px(448.))
                    .text_color(color(theme::MUTED))
                    .child("Launch any game, from any launcher, on any account."),
            )
            .child(div().mt(px(28.)).child(flow_art(account)))
            .child(
                div()
                    .mt(px(24.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color(theme::MUTED))
                    .child(icon(assets::SHIELD_CHECK, 14., theme::SUCCESS))
                    .child(
                        "Everything stays on this PC. There is no account, no sync, no telemetry.",
                    ),
            )
    }

    fn render_launchers(&self, cx: &mut Context<Self>) -> Div {
        let found = self.found();
        let lead = match found {
            None => "One Launch is checking which launchers are installed on this PC.".to_owned(),
            Some(0) => "No launcher was found on this PC. Install one, or point One Launch at one that is installed somewhere unusual.".to_owned(),
            Some(count) => format!(
                "One Launch found {} on this PC. Point it at any other that is installed somewhere unusual.",
                if count == 1 { "this launcher" } else { "these launchers" }
            ),
        };
        let list: AnyElement = match &self.statuses {
            Statuses::Failed(error) => div()
                .mt(px(20.))
                .text_color(color(theme::DANGER))
                .child(format!("Could not check launchers: {error}"))
                .into_any_element(),
            Statuses::Loading => boxed(
                LauncherId::ALL
                    .iter()
                    .enumerate()
                    .map(|(at, _)| skeleton_row(at == 0)),
            )
            .mt(px(20.))
            .into_any_element(),
            Statuses::Ready(statuses) => boxed(
                statuses
                    .iter()
                    .enumerate()
                    .map(|(at, status)| self.launcher_row(at, status, cx)),
            )
            .mt(px(20.))
            .into_any_element(),
        };
        let games = match self.games {
            None => div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child("Looking for installed games")
                .child(spinner("games-spinner", 14., theme::MUTED)),
            Some(count) => div()
                .flex()
                .gap(px(4.))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(color(theme::TEXT_STRONG))
                        .child(count.to_string()),
                )
                .child(if count == 1 {
                    "installed game found"
                } else {
                    "installed games found"
                }),
        };
        div()
            .flex()
            .flex_col()
            .min_h(relative(1.))
            .child(title("Your launchers", lead))
            .child(list)
            .child(
                div()
                    .mt(px(16.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_color(color(theme::MUTED))
                    .child(icon(assets::GAMEPAD, 16., theme::ACCENT))
                    .child(games),
            )
    }

    fn launcher_row(&self, at: usize, status: &LauncherStatus, cx: &mut Context<Self>) -> Div {
        let launcher = status.launcher;
        let error = self
            .locate_errors
            .iter()
            .find(|(held, _)| *held == launcher)
            .map(|(_, error)| error.clone());
        let place = match &status.install {
            Some(install) => div()
                .truncate()
                .font_family(theme::MONO)
                .text_color(color(theme::MUTED))
                .child(install.install_dir.clone()),
            None => div().text_color(color(theme::FAINT)).child(format!(
                "Not found. If it is installed, point One Launch at {}.",
                launcher.exe_name()
            )),
        };
        let working = self.locating == Some(launcher);
        let right: AnyElement = if status.install.is_some() {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.))
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(color(theme::SUCCESS))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(20.))
                        .rounded_full()
                        .bg(alpha(theme::SUCCESS, 0.15))
                        .child(icon(assets::CHECK, 12., theme::SUCCESS)),
                )
                .child("Found")
                .into_any_element()
        } else {
            Button::new(("guide-locate", at), Tone::Secondary, "Locate…")
                .glyph(if working {
                    assets::SPINNER
                } else {
                    assets::FOLDER_SEARCH
                })
                .turning(working)
                .disabled(working || self.busy())
                .on_click(cx.listener(move |this, _, _, cx| this.locate(launcher, cx)))
        };
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(16.))
            .py(px(10.))
            .when(at > 0, |row| {
                row.border_t_1().border_color(alpha(theme::BORDER, 0.6))
            })
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(px(36.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color(theme::BORDER))
                    .bg(color(theme::SURFACE_2))
                    .child(
                        div()
                            .size(px(10.))
                            .rounded_full()
                            .bg(color(launcher_hex(launcher))),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(color(theme::TEXT_STRONG))
                            .child(status.display_name),
                    )
                    .child(div().text_size(px(12.)).line_height(px(16.)).child(place))
                    .children(error.map(|error| {
                        div()
                            .mt(px(4.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color(theme::DANGER))
                            .child(error)
                    })),
            )
            .child(right)
    }

    fn render_play(&self) -> Div {
        let point = |glyph: &'static str, heading: &'static str, text: &'static str| {
            div()
                .flex()
                .items_start()
                .gap(px(12.))
                .child(
                    div()
                        .mt(px(2.))
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .size(px(32.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(alpha(theme::ACCENT, 0.3))
                        .bg(alpha(theme::ACCENT, 0.16))
                        .child(icon(glyph, 16., theme::ACCENT)),
                )
                .child(
                    div()
                        .min_w_0()
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(color(theme::TEXT_STRONG))
                                .child(heading),
                        )
                        .child(
                            div()
                                .mt(px(2.))
                                .text_size(px(12.))
                                .line_height(px(19.5))
                                .text_color(color(theme::MUTED))
                                .child(text),
                        ),
                )
        };
        let left = div()
            .flex_1()
            .min_w_0()
            .child(title(
                "Play switches accounts for you",
                "Press Play on a game, pick the account to play it on, and One Launch does the rest.".to_owned(),
            ))
            .child(
                div()
                    .mt(px(20.))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(point(
                        assets::REPEAT,
                        "The launcher is switched for you",
                        "One Launch closes it, signs it in as the account you picked, starts it again and launches the game.",
                    ))
                    .child(point(
                        assets::KEY,
                        "Sign in once per account",
                        "The first time you play on an account you may have to sign in once, in the launcher's own window. One Launch waits for you and carries on.",
                    ))
                    .child(point(
                        assets::SHIELD_CHECK,
                        "Backed up before every switch",
                        "The launcher's sign-in files are saved first and put back if anything fails. Settings › Backups can restore one by hand.",
                    )),
            )
            .when(self.preview, |left| {
                left.child(
                    div()
                        .mt(px(20.))
                        .flex()
                        .items_start()
                        .gap(px(8.))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color(theme::MUTED))
                        .child(icon(assets::FLASK, 14., theme::WARNING).mt(px(2.)))
                        .child("Preview mode is on, so Play only shows what a switch would do and changes nothing. Settings › Advanced turns it off."),
                )
            });
        div()
            .flex()
            .items_center()
            .gap(px(32.))
            .min_h(relative(1.))
            .child(left)
            .child(
                div()
                    .w(px(272.))
                    .flex_none()
                    .child(phase_demo(self.ticks as usize % DEMO_STAGES)),
            )
    }

    fn render_accounts(&self, cx: &mut Context<Self>) -> Div {
        let lead = "Import the ones your launchers already remember, or add them by hand. Passwords are optional and stay on this PC.".to_owned();
        let statuses = self.installed();
        let lists = if statuses.is_some() && self.remembered.is_empty() {
            div()
                .mt(px(20.))
                .rounded(px(12.))
                .border_1()
                .border_color(color(theme::BORDER))
                .bg(alpha(theme::SURFACE_2, 0.4))
                .px(px(16.))
                .py(px(12.))
                .text_color(color(theme::MUTED))
                .child(no_import_text(statuses.unwrap_or_default()))
        } else {
            let loading = statuses.is_none()
                || self
                    .remembered
                    .iter()
                    .any(|(_, list)| matches!(list, Remembered::Loading(_)));
            let entries: usize = self
                .remembered
                .iter()
                .map(|(_, list)| list.entries().len())
                .sum();
            let selectable: usize = self
                .remembered
                .iter()
                .map(|(launcher, list)| {
                    list.entries()
                        .iter()
                        .filter(|entry| {
                            entry.linked_account_id.is_none()
                                && !self.imported.contains(&key_of(*launcher, entry))
                        })
                        .count()
                })
                .sum();
            let count = self.selected.len();
            let summary = if statuses.is_none() {
                "Checking your launchers…".to_owned()
            } else if loading {
                "Reading what your launchers remember…".to_owned()
            } else if selectable == 0 {
                if entries == 0 {
                    "Your launchers have not remembered any account on this PC yet.".to_owned()
                } else {
                    "Everything here is in One Launch.".to_owned()
                }
            } else {
                format!("{count} of {selectable} selected")
            };
            let import_label = if self.importing {
                "Importing…".to_owned()
            } else if count == 0 {
                "Import".to_owned()
            } else {
                format!("Import {}", plural(count, "account", "accounts"))
            };
            let by_hand_only = statuses.map(by_hand_only_text).unwrap_or_default();
            div()
                .mt(px(20.))
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(color(theme::MUTED))
                                .child(summary),
                        )
                        .child(
                            Button::new("guide-import", Tone::Primary, import_label)
                                .glyph(if self.importing {
                                    assets::SPINNER
                                } else {
                                    assets::CHECK
                                })
                                .turning(self.importing)
                                .disabled(self.importing || count == 0 || self.closing)
                                .on_click(cx.listener(|this, _, _, cx| this.run_import(false, cx))),
                        ),
                )
                .children(
                    self.remembered
                        .iter()
                        .map(|(launcher, list)| self.remembered_section(*launcher, list, cx)),
                )
                .when(!by_hand_only.is_empty(), |lists| {
                    lists.child(
                        div()
                            .px(px(4.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color(theme::MUTED))
                            .child(by_hand_only),
                    )
                })
        };
        div()
            .flex()
            .flex_col()
            .min_h(relative(1.))
            .child(title("Add your accounts", lead))
            .child(lists)
            .child(
                div()
                    .mt(px(24.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(color(theme::MUTED))
                            .child("ADD BY HAND"),
                    )
                    .child(
                        div()
                            .mt(px(4.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color(theme::FAINT))
                            .child("Pick a launcher; its account form opens when you finish the guide."),
                    )
                    .child(
                        div()
                            .mt(px(10.))
                            .flex()
                            .flex_wrap()
                            .gap(px(8.))
                            .children(LauncherId::ALL.iter().map(|launcher| {
                                self.by_hand_chip(*launcher, cx)
                            })),
                    ),
            )
    }

    fn by_hand_chip(&self, launcher: LauncherId, cx: &mut Context<Self>) -> Stateful<Div> {
        let on = self.by_hand == Some(launcher);
        div()
            .id(("by-hand", launcher as usize))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(8.))
            .border_1()
            .px(px(12.))
            .py(px(6.))
            .font_weight(FontWeight::MEDIUM)
            .whitespace_nowrap()
            .cursor_pointer()
            .map(|chip| {
                if on {
                    chip.border_color(color(theme::ACCENT))
                        .bg(alpha(theme::ACCENT, 0.16))
                        .text_color(color(theme::TEXT_STRONG))
                } else {
                    chip.border_color(color(theme::BORDER))
                        .bg(color(theme::SURFACE_2))
                        .text_color(color(theme::TEXT))
                        .hover(|style| {
                            style
                                .border_color(color(theme::BORDER_STRONG))
                                .bg(color(theme::SURFACE_3))
                        })
                }
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.busy() {
                    this.by_hand = if on { None } else { Some(launcher) };
                    cx.notify();
                }
            }))
            .child(
                div()
                    .size(px(8.))
                    .rounded_full()
                    .bg(color(launcher_hex(launcher))),
            )
            .child(launcher.display_name())
            .when(on, |chip| {
                chip.child(icon(assets::CHECK, 14., theme::ACCENT))
            })
    }

    fn remembered_section(
        &self,
        launcher: LauncherId,
        list: &Remembered,
        cx: &mut Context<Self>,
    ) -> Div {
        let name = launcher.display_name();
        let count = list.entries().len();
        let said = match list {
            Remembered::Loading(_) => "reading…".to_owned(),
            Remembered::Failed(_) => String::new(),
            Remembered::Ready(_) => format!("remembers {}", plural(count, "account", "accounts")),
        };
        let line = |text: String, hex: u32| {
            div()
                .px(px(16.))
                .py(px(12.))
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(color(hex))
                .child(text)
        };
        let body: Vec<AnyElement> = match list {
            Remembered::Failed(error) => vec![
                line(
                    format!("Could not read {name}'s remembered accounts: {error}"),
                    theme::DANGER,
                )
                .into_any_element(),
            ],
            Remembered::Loading(entries) if entries.is_empty() => {
                vec![skeleton_lines().into_any_element()]
            }
            _ if count == 0 => vec![
                line(
                    format!("{name} has not remembered any account on this PC yet."),
                    theme::MUTED,
                )
                .into_any_element(),
            ],
            _ => list
                .entries()
                .iter()
                .enumerate()
                .map(|(at, entry)| {
                    self.remembered_row(launcher, at, entry, at + 1 == count, cx)
                        .into_any_element()
                })
                .collect(),
        };
        div()
            .overflow_hidden()
            .rounded(px(12.))
            .border_1()
            .border_color(color(theme::BORDER))
            .bg(alpha(theme::SURFACE_2, 0.4))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(16.))
                    .py(px(8.))
                    .border_b_1()
                    .border_color(alpha(theme::BORDER, 0.6))
                    .child(
                        div()
                            .size(px(8.))
                            .rounded_full()
                            .bg(color(launcher_hex(launcher))),
                    )
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(color(theme::TEXT_STRONG))
                            .child(name),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color(theme::MUTED))
                            .child(said),
                    ),
            )
            .children(body)
    }

    fn remembered_row(
        &self,
        launcher: LauncherId,
        at: usize,
        entry: &RememberedAccount,
        last: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let key = key_of(launcher, entry);
        let linked = entry.linked_account_id.is_some();
        let imported = self.imported.contains(&key);
        let locked = linked || imported;
        let checked = imported || self.selected.contains(&key);
        let name = if entry.persona_name.is_empty() {
            entry.login.clone()
        } else {
            entry.persona_name.clone()
        };
        let badge = if imported {
            Some(pill(PillTone::Ok, Some(assets::CHECK), "Imported"))
        } else if linked {
            Some(pill(PillTone::Info, None, "Already in One Launch"))
        } else if entry.most_recent {
            Some(pill(PillTone::Ok, None, "Signed in"))
        } else {
            None
        };
        div()
            .id(("guide-remembered", launcher as usize * 10_000 + at))
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(16.))
            .py(px(8.))
            .when(!last, |row| {
                row.border_b_1().border_color(alpha(theme::BORDER, 0.6))
            })
            .when(linked, |row| row.opacity(0.6))
            .when(!locked, |row| {
                row.cursor_pointer()
                    .hover(|style| style.bg(alpha(theme::SURFACE_2, 0.6)))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle(key.clone(), cx)))
            })
            .child(checkbox(checked, locked))
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
                            .children(badge),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color(theme::MUTED))
                            .child(entry.login.clone()),
                    ),
            )
    }

    fn render_ready(&self) -> Div {
        let ready = self.accounts.unwrap_or(0) > 0;
        let lead = if ready {
            "Pick a game in the Library, pick the account to play it on, and press Play.".to_owned()
        } else if let Some(launcher) = self.by_hand {
            format!(
                "Add your {} account next. Then pick a game in the Library, pick the account, and press Play.",
                launcher.display_name()
            )
        } else {
            "Add an account from the Accounts page whenever you like. Then pick a game in the Library, pick the account, and press Play.".to_owned()
        };
        let stat = |label: &'static str, value: Option<usize>| {
            div()
                .flex_1()
                .rounded(px(12.))
                .border_1()
                .border_color(color(theme::BORDER))
                .bg(alpha(theme::SURFACE_2, 0.5))
                .px(px(12.))
                .py(px(12.))
                .child(
                    div()
                        .text_size(px(11.))
                        .line_height(px(16.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(color(theme::MUTED))
                        .child(label),
                )
                .child(
                    div()
                        .mt(px(2.))
                        .text_size(px(20.))
                        .line_height(px(28.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(color(theme::TEXT_STRONG))
                        .child(value.map_or("…".to_owned(), |value| value.to_string())),
                )
        };
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .min_h(relative(1.))
            .text_center()
            .child(
                div()
                    .mb(px(16.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(56.))
                    .rounded_full()
                    .bg(alpha(theme::SUCCESS, 0.15))
                    .child(icon(assets::CHECK, 28., theme::SUCCESS)),
            )
            .child(
                div()
                    .text_size(px(24.))
                    .line_height(px(32.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(color(theme::TEXT_STRONG))
                    .child(if ready {
                        "You're all set"
                    } else {
                        "Almost there"
                    }),
            )
            .child(
                div()
                    .mt(px(8.))
                    .max_w(px(448.))
                    .text_color(color(theme::MUTED))
                    .child(lead),
            )
            .child(
                div()
                    .mt(px(28.))
                    .w_full()
                    .max_w(px(448.))
                    .flex()
                    .gap(px(12.))
                    .child(stat("LAUNCHERS FOUND", self.found()))
                    .child(stat("GAMES FOUND", self.games))
                    .child(stat("ACCOUNTS", self.accounts)),
            )
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> Div {
        let busy = self.busy();
        let (label, glyph, _) = self.primary();
        let left: AnyElement = if self.step == WELCOME {
            div()
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(color(theme::FAINT))
                .child("Takes about a minute.")
                .into_any_element()
        } else {
            Button::new("guide-back", Tone::Ghost, "Back")
                .glyph(assets::BACK)
                .disabled(busy)
                .on_click(cx.listener(|this, _, _, cx| {
                    if this.step > 0 {
                        this.go(this.step - 1, cx);
                    }
                }))
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .border_t_1()
            .border_color(color(theme::BORDER))
            .px(px(32.))
            .py(px(14.))
            .child(
                div()
                    .flex()
                    .min_w_0()
                    .items_center()
                    .gap(px(12.))
                    .child(left)
                    .children(self.flash.as_ref().map(|flash| flash.render().truncate())),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.))
                    .children(self.secondary().map(|(label, exit)| {
                        Button::new("guide-secondary", Tone::Ghost, label)
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, _, cx| this.finish(exit, cx)))
                    }))
                    .child(
                        Button::new("guide-primary", Tone::Primary, label)
                            .glyph(glyph)
                            .disabled(busy)
                            .leading(true)
                            .on_click(cx.listener(|this, click: &ClickEvent, _, cx| {
                                // The second click of the double click on Continue is not the next step's.
                                if !this.appeared.takes(click) {
                                    return;
                                }
                                let (_, _, action) = this.primary();
                                this.press(action, cx);
                            })),
                    ),
            )
    }

    pub fn render_card(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let busy = self.busy();
        let body = self.render_body(cx);
        let card = div()
            .id("setup-guide")
            .occlude()
            .relative()
            .flex()
            .flex_col()
            .w_full()
            .max_w(px(CARD_WIDTH))
            .h(px(CARD_HEIGHT))
            .max_h_full()
            .overflow_hidden()
            .rounded(px(12.))
            .border_1()
            .border_color(color(theme::BORDER))
            .bg(color(theme::SURFACE))
            .shadow(panel_shadow())
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w_full()
                    .h(px(240.))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(alpha(theme::ACCENT, 0.16), 0.),
                        linear_color_stop(alpha(theme::ACCENT, 0.), 1.),
                    )),
            )
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_between()
                    .gap(px(16.))
                    .px(px(32.))
                    .pt(px(16.))
                    .child(self.render_dots())
                    .child(
                        div()
                            .id("guide-skip")
                            .group("guide-skip")
                            .mr(px(-8.))
                            .p(px(6.))
                            .rounded(px(6.))
                            .map(|close| {
                                if busy {
                                    close.opacity(0.5)
                                } else {
                                    close
                                        .cursor_pointer()
                                        .hover(|style| style.bg(color(theme::SURFACE_2)))
                                        .tooltip(crate::widgets::tip("Skip setup (Esc)"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.finish(GuideExit::Stay, cx);
                                        }))
                                }
                            })
                            .child(
                                icon(assets::CLEAR, 16., theme::MUTED)
                                    .group_hover("guide-skip", |style| {
                                        style.text_color(color(theme::TEXT))
                                    }),
                            ),
                    ),
            )
            .child(
                div()
                    .id("guide-body")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.body)
                    .px(px(32.))
                    .py(px(20.))
                    .child(body),
            )
            .child(self.render_footer(cx).relative());
        // The backdrop takes no click, as the Electron app's: the guide is left by its own buttons.
        div()
            .id("guide-backdrop")
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .p(px(24.))
            .bg(alpha(theme::BG, 0.9))
            .child(card)
    }
}

fn title(heading: &'static str, lead: String) -> Div {
    div()
        .child(
            div()
                .text_size(px(20.))
                .line_height(px(28.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color(theme::TEXT_STRONG))
                .child(heading),
        )
        .child(
            div()
                .mt(px(6.))
                .max_w(px(672.))
                .text_color(color(theme::MUTED))
                .child(lead),
        )
}

fn boxed(rows: impl Iterator<Item = Div>) -> Div {
    div()
        .rounded(px(12.))
        .border_1()
        .border_color(color(theme::BORDER))
        .bg(alpha(theme::SURFACE_2, 0.4))
        .children(rows)
}

fn skeleton_row(first: bool) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .px(px(16.))
        .py(px(10.))
        .when(!first, |row| {
            row.border_t_1().border_color(alpha(theme::BORDER, 0.6))
        })
        .child(
            div()
                .size(px(36.))
                .flex_none()
                .rounded(px(8.))
                .bg(color(theme::SURFACE_2)),
        )
        .child(skeleton_lines().p_0())
}

fn skeleton_lines() -> Div {
    div()
        .flex_1()
        .px(px(16.))
        .py(px(10.))
        .child(
            div()
                .h(px(16.))
                .w(px(96.))
                .rounded(px(4.))
                .bg(color(theme::SURFACE_3)),
        )
        .child(
            div()
                .mt(px(4.))
                .h(px(12.))
                .w(px(224.))
                .rounded(px(4.))
                .bg(color(theme::SURFACE_2)),
        )
}

fn checkbox(checked: bool, locked: bool) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(16.))
        .rounded(px(3.))
        .border_1()
        .when(locked && !checked, |checkbox| checkbox.opacity(0.5))
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
        })
}

// The installed launchers that keep no account list, in one sentence; empty when every installed one keeps one.
fn by_hand_only_text(statuses: &[LauncherStatus]) -> String {
    let names: Vec<&str> = statuses
        .iter()
        .filter(|status| status.install.is_some() && !IMPORT_LAUNCHERS.contains(&status.launcher))
        .map(|status| status.launcher.display_name())
        .collect();
    match names.as_slice() {
        [] => String::new(),
        [one] => format!("{one} keeps no account list. Add it under Add by hand."),
        [rest @ .., last] => format!(
            "{} and {last} keep no account list. Add those under Add by hand.",
            rest.join(", ")
        ),
    }
}

fn no_import_text(statuses: &[LauncherStatus]) -> String {
    if statuses.iter().all(|status| status.install.is_none()) {
        return "No launcher was found on this PC, so there is nothing to import yet. Accounts can still be added by hand.".to_owned();
    }
    by_hand_only_text(statuses)
}

// The product in one picture: a game, an account that keeps changing, and Play. Decorative.
fn flow_art(account: &'static str) -> Div {
    let beat = |label: &'static str, art: AnyElement| {
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(10.))
            .child(art)
            .child(
                div()
                    .text_size(px(11.))
                    .line_height(px(16.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(color(theme::MUTED))
                    .child(label),
            )
    };
    let poster = div()
        .relative()
        .flex()
        .items_end()
        .justify_center()
        .h(px(72.))
        .w(px(48.))
        .overflow_hidden()
        .rounded(px(6.))
        .border_1()
        .border_color(color(theme::BORDER))
        .bg(linear_gradient(
            180.,
            linear_color_stop(color(theme::SURFACE_2), 0.),
            linear_color_stop(color(theme::SURFACE_3), 1.),
        ))
        .p(px(4.))
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(18.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(hsla(0., 0., 1., 0.1))
                .child("CS"),
        )
        .child(
            div()
                .relative()
                .text_size(px(9.))
                .line_height(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color(theme::TEXT_STRONG))
                .child("CS2"),
        );
    let chip = div().flex().items_center().h(px(72.)).child(
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(alpha(theme::ACCENT, 0.3))
            .bg(alpha(theme::ACCENT, 0.16))
            .px(px(12.))
            .py(px(8.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(color(theme::TEXT_STRONG))
            .child(icon(assets::KEY, 16., theme::ACCENT))
            .child(div().w(px(40.)).text_left().child(account)),
    );
    let play = div()
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .h(px(72.))
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .size(px(44.))
                .rounded_full()
                .bg(color(theme::ACCENT))
                .child(icon(assets::PLAY, 20., theme::TEXT_STRONG)),
        );
    let chevron = || icon(assets::CHEVRON_RIGHT, 16., theme::FAINT);
    div()
        .flex()
        .items_center()
        .gap(px(16.))
        .rounded(px(16.))
        .border_1()
        .border_color(color(theme::BORDER))
        .bg(alpha(theme::SURFACE_2, 0.5))
        .px(px(28.))
        .py(px(20.))
        .child(beat("Pick a game", poster.into_any_element()))
        .child(chevron())
        .child(beat("Pick an account", chip.into_any_element()))
        .child(chevron())
        .child(beat("Play", play.into_any_element()))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DemoStatus {
    Pending,
    Running,
    Done,
}

fn demo_status(stage: usize, index: usize) -> DemoStatus {
    if stage == 0 {
        DemoStatus::Pending
    } else if stage > PhaseId::ORDER.len() || index + 1 < stage {
        DemoStatus::Done
    } else if index + 1 == stage {
        DemoStatus::Running
    } else {
        DemoStatus::Pending
    }
}

// The launch dialog's phases playing themselves: all pending, then each in turn, then all done for three ticks. Decorative.
fn phase_demo(stage: usize) -> Div {
    let names = PhaseNames {
        launcher: "Steam",
        account: "Alt",
        game: Some("Counter-Strike 2"),
    };
    div()
        .rounded(px(12.))
        .border_1()
        .border_color(color(theme::BORDER))
        .bg(alpha(theme::SURFACE_2, 0.5))
        .px(px(16.))
        .py(px(14.))
        .child(
            div()
                .mb(px(12.))
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(color(theme::MUTED))
                .child(icon(assets::PLAY, 14., theme::ACCENT))
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(color(theme::TEXT))
                        .child("Counter-Strike 2"),
                )
                .child(div().text_color(color(theme::FAINT)).child("·"))
                .child("as Alt"),
        )
        .child(div().flex().flex_col().gap(px(10.)).children(
            PhaseId::ORDER.iter().enumerate().map(|(index, id)| {
                let status = demo_status(stage, index);
                let mark: AnyElement = match status {
                    DemoStatus::Done => div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(16.))
                        .rounded_full()
                        .bg(alpha(theme::SUCCESS, 0.15))
                        .child(icon(assets::CHECK, 12., theme::SUCCESS))
                        .into_any_element(),
                    DemoStatus::Running => spinner(("demo-spinner", index), 16., theme::ACCENT),
                    DemoStatus::Pending => div()
                        .size(px(14.))
                        .rounded_full()
                        .border_1()
                        .border_color(color(theme::BORDER_STRONG))
                        .into_any_element(),
                };
                let label = div().child(phase_label(*id, &names));
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .size(px(16.))
                            .child(mark),
                    )
                    .child(match status {
                        DemoStatus::Pending => label.text_color(color(theme::FAINT)),
                        DemoStatus::Running => label
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(color(theme::TEXT_STRONG)),
                        DemoStatus::Done => label.text_color(color(theme::TEXT)),
                    })
            }),
        ))
        .child(
            div()
                .mt(px(12.))
                .text_size(px(11.))
                .line_height(px(16.))
                .text_color(color(theme::FAINT))
                .child("What you see when you press Play."),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_demo_runs_each_phase_in_turn_and_holds_all_done() {
        assert_eq!(demo_status(0, 0), DemoStatus::Pending);
        assert_eq!(demo_status(1, 0), DemoStatus::Running);
        assert_eq!(demo_status(1, 1), DemoStatus::Pending);
        assert_eq!(demo_status(3, 1), DemoStatus::Done);
        assert_eq!(demo_status(3, 2), DemoStatus::Running);
        assert_eq!(demo_status(6, 4), DemoStatus::Done);
        assert_eq!(demo_status(8, 0), DemoStatus::Done);
    }
}
