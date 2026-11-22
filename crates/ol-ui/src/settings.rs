use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    AnyElement, ClickEvent, Context, Div, Keystroke, MouseDownEvent, Pixels, SharedString, Size,
    StyledText, TextRun, Window, anchored, deferred, div, font, px,
};
use ol_api::settings::{
    AutostartStatus, EncryptionState, Folder, RESET_BANNER, RESET_WARNING, UpdateStatus,
    VaultBackend, VaultHealth,
};
use ol_api::{ApiError, AppInfo, Backend};
use ol_model::{
    BATTLENET_LAUNCH_CHOICES, DEFAULT_BACKUPS_TO_KEEP, EPIC_LAUNCH_URI_CHOICES, LauncherId,
    LogLevel, SettingKey, Settings,
};
use ol_ui_kit::theme::{self, alpha, color};

use crate::assets;
use crate::host::overlay;
use crate::page::{PageScroll, scroll_page};
use crate::widgets::{
    Appeared, Button, DropdownMenu, Spot, Tone, dropdown_menu, dropdown_place, dropdown_trigger,
    icon, page_title, panel, setting_row, spot_of, switch, tab_strip,
};

mod backups;
mod launchers;

pub use launchers::KnownStatuses;

// The tabs this build shows, in the Electron app's order. Artwork comes with its screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettingsTab {
    #[default]
    General,
    Launchers,
    Backups,
    Advanced,
}

impl SettingsTab {
    const SHOWN: [Self; 4] = [
        Self::General,
        Self::Launchers,
        Self::Backups,
        Self::Advanced,
    ];
    const LABELS: [&'static str; 4] = ["General", "Launchers", "Backups", "Advanced"];

    fn index(self) -> usize {
        Self::SHOWN.iter().position(|tab| *tab == self).unwrap_or(0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Pick {
    EpicLaunch,
    BattlenetLaunch,
    LogLevel,
    Keep,
}

impl Pick {
    const fn key(self) -> SettingKey {
        match self {
            Self::EpicLaunch => SettingKey::EpicLaunchUri,
            Self::BattlenetLaunch => SettingKey::BattlenetLaunch,
            Self::LogLevel => SettingKey::LogLevel,
            Self::Keep => SettingKey::BackupsKeep,
        }
    }

    const fn id(self) -> &'static str {
        match self {
            Self::EpicLaunch => "epic-launch-link",
            Self::BattlenetLaunch => "battlenet-launch",
            Self::LogLevel => "log-level",
            Self::Keep => "backups-keep",
        }
    }

    // The stored value and what the list says. A number of snapshots stored outside the list is offered too, as in the Electron app.
    fn choices(self, settings: &Settings) -> Vec<(String, String)> {
        let owned = |choices: &[(&str, &str)]| {
            choices
                .iter()
                .map(|(value, label)| ((*value).to_owned(), (*label).to_owned()))
                .collect()
        };
        match self {
            Self::EpicLaunch => owned(&EPIC_LAUNCH_URI_CHOICES),
            Self::BattlenetLaunch => owned(&BATTLENET_LAUNCH_CHOICES),
            Self::LogLevel => LogLevel::ALL
                .iter()
                .map(|level| (level.as_str().to_owned(), level.label().to_owned()))
                .collect(),
            Self::Keep => settings
                .backups_keep_choices()
                .into_iter()
                .map(|keep| {
                    let label = if keep == DEFAULT_BACKUPS_TO_KEEP {
                        format!("{keep} (default)")
                    } else {
                        keep.to_string()
                    };
                    (keep.to_string(), label)
                })
                .collect(),
        }
    }

    fn current(self, settings: &Settings) -> String {
        match self {
            Self::EpicLaunch => settings.epic_launch_uri().to_owned(),
            Self::BattlenetLaunch => settings.battlenet_launch().to_owned(),
            Self::LogLevel => settings.log_level().as_str().to_owned(),
            Self::Keep => settings.backups_keep().to_string(),
        }
    }

    fn chosen(self, settings: &Settings) -> usize {
        let current = self.current(settings);
        self.choices(settings)
            .iter()
            .position(|(value, _)| *value == current)
            .unwrap_or(0)
    }
}

// What the keyboard can be on, in the order Tab goes through it. A snapshot's Restore is named by where its id stands among those seen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Control {
    Tabs,
    AskReset,
    YesReset,
    CancelReset,
    Toggle(SettingKey),
    Pick(Pick),
    RunTest,
    Open(Folder),
    Recheck,
    Disclose(LauncherId),
    Locate(LauncherId),
    ResetPath(LauncherId),
    Refresh,
    Section(LauncherId),
    Restore(LauncherId, usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Refused {
    Setting(SettingKey),
    Autostart,
}

enum SelfTest {
    Idle,
    Running,
    Passed(u128),
    Failed(SharedString),
}

// Writes of one setting not answered yet, and the newest of them: only its answer may say the setting was refused.
struct Pending {
    count: u32,
    newest: u64,
}

pub struct SettingsView {
    backend: Backend,
    tab: Rc<Cell<SettingsTab>>,
    scroll: PageScroll,
    settings: Option<Settings>,
    reads: u64,
    writes: u64,
    pending: HashMap<SettingKey, Pending>,
    refused: Option<(Refused, SharedString)>,
    vault: Option<VaultHealth>,
    vault_reads: u64,
    test: SelfTest,
    tests: u64,
    confirm_reset: Option<Appeared>,
    resetting: bool,
    reset_note: Option<SharedString>,
    folder_error: Option<SharedString>,
    info: AppInfo,
    update: UpdateStatus,
    autostart: AutostartStatus,
    menu: Option<(Pick, usize)>,
    spots: HashMap<Pick, Spot>,
    viewport: Option<Size<Pixels>>,
    focus: Option<Control>,
    ring: bool,
    reveal: bool,
    focused: Spot,
    known: KnownStatuses,
    launchers: launchers::Launchers,
    backups: backups::Backups,
}

impl SettingsView {
    pub fn new(
        backend: Backend,
        tab: Rc<Cell<SettingsTab>>,
        known: KnownStatuses,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self {
            info: backend.app_info(),
            update: backend.update_status(),
            autostart: backend.autostart_status(),
            backend,
            tab,
            scroll: PageScroll::default(),
            settings: None,
            reads: 0,
            writes: 0,
            pending: HashMap::new(),
            refused: None,
            vault: None,
            vault_reads: 0,
            test: SelfTest::Idle,
            tests: 0,
            confirm_reset: None,
            resetting: false,
            reset_note: None,
            folder_error: None,
            menu: None,
            spots: HashMap::new(),
            viewport: None,
            focus: None,
            ring: false,
            reveal: false,
            focused: Spot::default(),
            known,
            launchers: launchers::Launchers::default(),
            backups: backups::Backups::default(),
        };
        view.reload(cx);
        view.read_vault(cx);
        view.enter_tab(cx);
        view
    }

    fn enter_tab(&mut self, cx: &mut Context<Self>) {
        match self.tab.get() {
            SettingsTab::Launchers => self.enter_launchers(cx),
            SettingsTab::Backups => self.enter_backups(cx),
            SettingsTab::General | SettingsTab::Advanced => {}
        }
    }

    pub fn select_tab(&mut self, tab: SettingsTab, cx: &mut Context<Self>) {
        if self.tab.get() != tab {
            self.tab.set(tab);
            self.scroll.to_top();
            self.menu = None;
            self.folder_error = None;
            self.enter_tab(cx);
        }
        // A control of the tab that was shown is no longer drawn: the keyboard leaves it, unless it is on the tabs themselves.
        if self.focus != Some(Control::Tabs) {
            self.focus = None;
        }
        cx.notify();
    }

    // A setting stored elsewhere, from the launch dialog's Turn preview off, say. A write of this page's own that is still out decides that setting.
    pub fn setting_changed(
        &mut self,
        key: SettingKey,
        value: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.pending.contains_key(&key) {
            return;
        }
        match (value, &mut self.settings) {
            (Some(value), Some(settings)) => {
                settings.insert(key, value);
                cx.notify();
            }
            _ => self.reload(cx),
        }
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        self.reads += 1;
        let (read, backend) = (self.reads, self.backend.clone());
        cx.spawn(async move |this, cx| {
            let settings = backend.settings().await;
            this.update(cx, |this, cx| this.loaded(read, settings, cx))
                .ok();
        })
        .detach();
    }

    fn loaded(&mut self, read: u64, settings: Result<Settings, ApiError>, cx: &mut Context<Self>) {
        let Ok(mut fresh) = settings else {
            return;
        };
        if read != self.reads {
            return;
        }
        if let Some(shown) = &self.settings {
            for key in self.pending.keys() {
                if let Some(value) = shown.get(*key) {
                    fresh.insert(*key, value.to_owned());
                }
            }
        }
        self.settings = Some(fresh);
        cx.notify();
    }

    fn read_vault(&mut self, cx: &mut Context<Self>) {
        self.vault_reads += 1;
        let (read, backend) = (self.vault_reads, self.backend.clone());
        cx.spawn(async move |this, cx| {
            let status = backend.vault_status().await;
            this.update(cx, |this, cx| {
                if this.vault_reads == read {
                    this.vault = Some(status.unwrap_or(VaultHealth {
                        available: false,
                        backend: VaultBackend::None,
                        readable: None,
                    }));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn on(&self, key: SettingKey) -> bool {
        self.settings
            .as_ref()
            .map_or_else(|| key.default_on(), |settings| settings.toggle(key))
    }

    // Hide One Launch after Play shows off, and takes no change, while the tray setting is off.
    fn shown_on(&self, key: SettingKey) -> bool {
        self.on(key)
            && (key != SettingKey::PlayHideToTray || self.on(SettingKey::WindowCloseToTray))
    }

    fn can_change(&self, key: SettingKey) -> bool {
        self.settings.is_some()
            && (key != SettingKey::PlayHideToTray || self.on(SettingKey::WindowCloseToTray))
    }

    fn toggle(&mut self, key: SettingKey, cx: &mut Context<Self>) {
        if !self.can_change(key) {
            return;
        }
        let next = !self.shown_on(key);
        self.write(key, next.to_string(), cx);
    }

    // Shown at once and stored at once, as in the Electron app; a refusal says so under the row and the page reads the settings again.
    fn write(&mut self, key: SettingKey, value: String, cx: &mut Context<Self>) {
        let Some(settings) = &mut self.settings else {
            return;
        };
        self.refused = None;
        settings.insert(key, value.clone());
        self.writes += 1;
        let write = self.writes;
        let pending = self.pending.entry(key).or_insert(Pending {
            count: 0,
            newest: write,
        });
        pending.count += 1;
        pending.newest = write;
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let stored = backend.set_setting(key, value).await;
            this.update(cx, |this, cx| this.written(key, write, stored, cx))
                .ok();
        })
        .detach();
        cx.notify();
    }

    fn written(
        &mut self,
        key: SettingKey,
        write: u64,
        stored: Result<(), ApiError>,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = self.pending.get_mut(&key) else {
            return;
        };
        pending.count = pending.count.saturating_sub(1);
        let newest = pending.newest == write;
        if pending.count == 0 {
            self.pending.remove(&key);
        }
        if let Err(error) = stored
            && newest
        {
            self.refused = Some((Refused::Setting(key), error.message.into()));
            self.reload(cx);
        }
        cx.notify();
    }

    fn set_autostart(&mut self, cx: &mut Context<Self>) {
        if !self.autostart.available {
            return;
        }
        self.refused = None;
        if let Err(error) = self.backend.set_autostart(!self.autostart.open_at_login) {
            self.refused = Some((Refused::Autostart, error.message.into()));
        }
        self.autostart = self.backend.autostart_status();
        cx.notify();
    }

    fn open_menu(&mut self, pick: Pick, cx: &mut Context<Self>) {
        let Some(settings) = &self.settings else {
            return;
        };
        self.menu = match self.menu {
            Some((open, _)) if open == pick => None,
            _ => Some((pick, pick.chosen(settings))),
        };
        cx.notify();
    }

    // Choosing the value already chosen changes nothing, as in the Electron app.
    fn choose(&mut self, pick: Pick, index: usize, cx: &mut Context<Self>) {
        self.menu = None;
        self.focus = Some(Control::Pick(pick));
        cx.notify();
        let Some(settings) = &self.settings else {
            return;
        };
        let Some((value, _)) = pick.choices(settings).into_iter().nth(index) else {
            return;
        };
        if value != pick.current(settings) {
            self.write(pick.key(), value, cx);
        }
    }

    fn vault_works(&self) -> bool {
        self.vault.is_some_and(|vault| vault.available)
    }

    fn run_test(&mut self, cx: &mut Context<Self>) {
        if matches!(self.test, SelfTest::Running) || !self.vault_works() {
            return;
        }
        self.test = SelfTest::Running;
        self.tests += 1;
        let (test, backend) = (self.tests, self.backend.clone());
        cx.spawn(async move |this, cx| {
            let tested = backend.test_vault().await;
            this.update(cx, |this, cx| {
                if this.tests == test {
                    this.test = match tested {
                        Ok(took) => SelfTest::Passed(took.as_millis()),
                        Err(error) => SelfTest::Failed(error.message.into()),
                    };
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn shows_reset(&self) -> bool {
        self.vault.is_some_and(VaultHealth::shows_reset)
    }

    fn ask_reset(&mut self, ask: bool, cx: &mut Context<Self>) {
        if self.resetting || !self.shows_reset() {
            return;
        }
        self.confirm_reset = ask.then(Appeared::now);
        // The button that had the keyboard is gone, as in the Electron app: the next Enter presses nothing.
        self.focus = None;
        cx.notify();
    }

    // Enter and a click together reset once.
    fn reset(&mut self, cx: &mut Context<Self>) {
        if self.resetting || self.confirm_reset.is_none() || !self.shows_reset() {
            return;
        }
        self.resetting = true;
        self.reset_note = None;
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let reset = backend.reset_vault().await;
            this.update(cx, |this, cx| {
                this.resetting = false;
                match reset {
                    Ok(reset) => {
                        this.reset_note = Some(reset.note().into());
                        this.confirm_reset = None;
                    }
                    Err(error) => this.reset_note = Some(error.message.into()),
                }
                this.read_vault(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    // A refusal is told on the tab that asked, while it is shown.
    fn open_folder(&mut self, folder: Folder, cx: &mut Context<Self>) {
        self.folder_error = None;
        let (backend, tab) = (self.backend.clone(), self.tab.get());
        cx.spawn(async move |this, cx| {
            let opened = backend.open_folder(folder).await;
            this.update(cx, |this, cx| {
                if let Err(error) = opened
                    && this.tab.get() == tab
                {
                    this.folder_error = Some(error.message.into());
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn controls(&self) -> Vec<Control> {
        let mut controls = vec![Control::Tabs];
        if self.shows_reset() && !self.resetting {
            if self.confirm_reset.is_some() {
                controls.extend([Control::YesReset, Control::CancelReset]);
            } else {
                controls.push(Control::AskReset);
            }
        }
        let toggles = |keys: &[SettingKey]| {
            keys.iter()
                .filter(|key| self.can_change(**key))
                .map(|key| Control::Toggle(*key))
                .collect::<Vec<_>>()
        };
        let loaded = self.settings.is_some();
        match self.tab.get() {
            SettingsTab::General => controls.extend(toggles(&[
                SettingKey::WindowCloseToTray,
                SettingKey::UpdatesAuto,
            ])),
            SettingsTab::Launchers => {
                self.launchers.controls(&mut controls);
                controls.extend(toggles(&[
                    SettingKey::SteamAutoLogin,
                    SettingKey::RiotPressPlay,
                    SettingKey::PlayHideToTray,
                ]));
            }
            SettingsTab::Backups => {
                controls.extend([Control::Refresh, Control::Open(Folder::Backups)]);
                if loaded {
                    controls.push(Control::Pick(Pick::Keep));
                }
                self.backups.controls(&mut controls);
            }
            SettingsTab::Advanced => {
                controls.extend(toggles(&[SettingKey::DryRun]));
                if loaded {
                    controls.extend([
                        Control::Pick(Pick::EpicLaunch),
                        Control::Pick(Pick::BattlenetLaunch),
                    ]);
                }
                if self.vault_works() && !matches!(self.test, SelfTest::Running) {
                    controls.push(Control::RunTest);
                }
                controls.extend([Control::Open(Folder::Logs), Control::Open(Folder::Data)]);
                if loaded {
                    controls.push(Control::Pick(Pick::LogLevel));
                }
            }
        }
        controls
    }

    fn step_focus(&mut self, back: bool) {
        let controls = self.controls();
        let at = self
            .focus
            .and_then(|focus| controls.iter().position(|control| *control == focus));
        let count = controls.len();
        let next = match (at, back) {
            (None, false) => 0,
            (None, true) => count - 1,
            (Some(at), false) => (at + 1) % count,
            (Some(at), true) => (at + count - 1) % count,
        };
        self.focus = Some(controls[next]);
        self.ring = true;
        self.reveal = true;
    }

    // Keys held with Ctrl, Alt or the Windows key are the window's or the shell's.
    pub fn on_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let held = keystroke.modifiers;
        if held.control || held.alt || held.platform || held.function {
            return false;
        }
        let key = keystroke.key.as_str();
        if let Some((pick, lit)) = self.menu {
            let last = self
                .settings
                .as_ref()
                .map_or(0, |settings| pick.choices(settings).len().saturating_sub(1));
            match key {
                "down" => self.menu = Some((pick, (lit + 1).min(last))),
                "up" => self.menu = Some((pick, lit.saturating_sub(1))),
                "home" => self.menu = Some((pick, 0)),
                "end" => self.menu = Some((pick, last)),
                "enter" | "space" => self.choose(pick, lit, cx),
                "escape" | "tab" => {
                    self.menu = None;
                    self.focus = Some(Control::Pick(pick));
                    self.ring = true;
                }
                _ => {}
            }
            cx.notify();
            return true;
        }
        if key == "tab" {
            self.step_focus(held.shift);
            cx.notify();
            return true;
        }
        if held.shift {
            return false;
        }
        let Some(focus) = self.focus else {
            return false;
        };
        // A key acts only on a control that is drawn and can be used.
        if !self.controls().contains(&focus) {
            return false;
        }
        let press = matches!(key, "enter" | "space");
        match focus {
            Control::Tabs => {
                let at = self.tab.get().index();
                let count = SettingsTab::SHOWN.len();
                let to = match key {
                    "right" => (at + 1) % count,
                    "left" => (at + count - 1) % count,
                    "home" => 0,
                    "end" => count - 1,
                    _ => return false,
                };
                self.ring = true;
                self.select_tab(SettingsTab::SHOWN[to], cx);
            }
            Control::Toggle(setting) if press => self.toggle(setting, cx),
            Control::Pick(pick) if press || matches!(key, "down" | "up") => {
                if press || self.menu.is_none() {
                    self.open_menu(pick, cx);
                }
            }
            Control::RunTest if press => self.run_test(cx),
            Control::Open(folder) if press => self.open_folder(folder, cx),
            Control::AskReset if press => self.ask_reset(true, cx),
            Control::YesReset if press => self.reset(cx),
            Control::CancelReset if press => self.ask_reset(false, cx),
            Control::Recheck if press => self.recheck(cx),
            Control::Disclose(launcher) if press => self.disclose(launcher, cx),
            Control::Locate(launcher) if press => self.locate(launcher, cx),
            Control::ResetPath(launcher) if press => self.reset_path(launcher, cx),
            Control::Refresh if press => self.list_backups(None, cx),
            Control::Section(launcher) if press => self.toggle_section(launcher, cx),
            Control::Restore(launcher, seen) if press => self.ask_restore(launcher, seen, cx),
            _ => return false,
        }
        cx.notify();
        true
    }

    fn clicked(&mut self, control: Control) {
        self.focus = Some(control);
        self.ring = false;
    }

    fn ringed_on(&self, control: Control) -> bool {
        self.ring && self.focus == Some(control)
    }

    // The control that has the keyboard reports where it is drawn, so Tab can bring it into sight.
    fn track(&self, control: Control, element: AnyElement) -> AnyElement {
        if self.focus == Some(control) {
            div()
                .relative()
                .flex_none()
                .child(element)
                .child(spot_of(&self.focused))
                .into_any_element()
        } else {
            element
        }
    }

    fn render_switch(&self, key: SettingKey, cx: &mut Context<Self>) -> AnyElement {
        let control = Control::Toggle(key);
        let element = switch(
            key.as_str(),
            self.shown_on(key),
            !self.can_change(key),
            self.ringed_on(control),
            cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.clicked(control);
                this.toggle(key, cx);
            }),
        );
        self.track(control, element)
    }

    fn refusal(&self, row: Refused) -> Option<SharedString> {
        self.refused
            .as_ref()
            .filter(|(refused, _)| *refused == row)
            .map(|(_, message)| message.clone())
    }

    fn toggle_row(
        &self,
        key: SettingKey,
        label: &'static str,
        hint: &'static str,
        place: (bool, bool),
        cx: &mut Context<Self>,
    ) -> Div {
        setting_row(
            label,
            Some(hint.into()),
            self.refusal(Refused::Setting(key)),
            place.0,
            place.1,
            self.render_switch(key, cx),
        )
    }

    fn render_pick(&mut self, pick: Pick, cx: &mut Context<Self>) -> AnyElement {
        let control = Control::Pick(pick);
        let spot = self.spots.entry(pick).or_default().clone();
        let label = self
            .settings
            .as_ref()
            .and_then(|settings| {
                pick.choices(settings)
                    .into_iter()
                    .nth(pick.chosen(settings))
                    .map(|(_, label)| label)
            })
            .unwrap_or_default();
        let element = dropdown_trigger(
            pick.id(),
            label,
            self.menu.is_some_and(|(open, _)| open == pick),
            self.settings.is_none(),
            self.ringed_on(control),
            &spot,
            cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.clicked(control);
                this.open_menu(pick, cx);
            }),
        );
        self.track(control, element)
    }

    fn render_menu(&self, viewport: Size<Pixels>, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (pick, lit) = self.menu?;
        let settings = self.settings.as_ref()?;
        let trigger = self.spots.get(&pick)?.get()?;
        let choices = pick.choices(settings);
        let at = dropdown_place(trigger, viewport, choices.len());
        let spot = self.spots.get(&pick)?.clone();
        let menu = dropdown_menu(
            DropdownMenu {
                labels: choices.into_iter().map(|(_, label)| label.into()).collect(),
                chosen: pick.chosen(settings),
                lit,
                least: trigger.size.width,
            },
            cx.listener(move |this, index: &usize, _, cx| {
                if let Some((open, lit)) = &mut this.menu
                    && *open == pick
                    && *lit != *index
                {
                    *lit = *index;
                    cx.notify();
                }
            }),
            cx.listener(move |this, index: &usize, _, cx| this.choose(pick, *index, cx)),
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                // A press on the trigger is the trigger's to answer: it closes the menu.
                if spot
                    .get()
                    .is_some_and(|bounds| bounds.contains(&event.position))
                {
                    return;
                }
                this.menu = None;
                cx.notify();
            }),
        );
        Some(
            deferred(anchored().position(at).child(menu))
                .with_priority(1)
                .into_any_element(),
        )
    }

    fn render_header(&self, cx: &mut Context<Self>) -> Div {
        let at = self.tab.get().index();
        div()
            .pt(px(28.))
            .pb(px(20.))
            .bg(color(theme::BG))
            .child(page_title("Settings", None))
            .child(div().mt(px(20.)).child(tab_strip(
                &SettingsTab::LABELS,
                at,
                self.ringed_on(Control::Tabs),
                cx.listener(|this, index: &usize, _, cx| {
                    this.clicked(Control::Tabs);
                    this.select_tab(SettingsTab::SHOWN[*index], cx);
                }),
            )))
    }

    fn render_banner(&self, cx: &mut Context<Self>) -> Option<Div> {
        let shows = self.shows_reset();
        if !shows {
            // Once the secrets read again, the reset's answer stands alone: the banner's sentence would say the opposite.
            return self.reset_note.clone().map(|note| {
                div()
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color(theme::BORDER))
                    .bg(color(theme::SURFACE))
                    .px(px(16.))
                    .py(px(12.))
                    .text_color(color(theme::TEXT))
                    .child(
                        if self.vault_works() {
                            icon(assets::SHIELD_CHECK, 16., theme::SUCCESS)
                        } else {
                            icon(assets::SHIELD_OFF, 16., theme::FAINT)
                        }
                        .mt(px(2.)),
                    )
                    .child(div().flex_1().min_w_0().child(note))
            });
        }
        let (before, after) = RESET_BANNER
            .split_once("chromium")
            .unwrap_or((RESET_BANNER, ""));
        let danger = gpui::Hsla::from(color(theme::DANGER));
        let run = |len: usize, family: &'static str| TextRun {
            len,
            font: font(family),
            color: danger,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let text = StyledText::new(RESET_BANNER).with_runs(vec![
            run(before.len(), theme::FONT),
            run("chromium".len(), theme::MONO),
            run(after.len(), theme::FONT),
        ]);
        let buttons: Vec<AnyElement> = if !shows {
            Vec::new()
        } else if self.confirm_reset.is_some() {
            vec![
                Button::new(
                    "reset-yes",
                    Tone::Danger,
                    if self.resetting {
                        "Clearing…"
                    } else {
                        "Yes, clear them"
                    },
                )
                .disabled(self.resetting)
                .leading(self.ringed_on(Control::YesReset))
                .on_click(cx.listener(|this, click: &ClickEvent, _, cx| {
                    if !this.confirm_reset.is_some_and(|shown| shown.takes(click)) {
                        return;
                    }
                    this.clicked(Control::YesReset);
                    this.reset(cx);
                })),
                Button::new("reset-cancel", Tone::Ghost, "Cancel")
                    .disabled(self.resetting)
                    .leading(self.ringed_on(Control::CancelReset))
                    .on_click(cx.listener(|this, _, _, cx| this.ask_reset(false, cx))),
            ]
        } else {
            vec![
                Button::new(
                    "reset-ask",
                    Tone::Danger,
                    "Clear stored passwords and sign-ins…",
                )
                .leading(self.ringed_on(Control::AskReset))
                .on_click(cx.listener(|this, _, _, cx| this.ask_reset(true, cx))),
            ]
        };
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .rounded(px(8.))
                .border_1()
                .border_color(alpha(theme::DANGER, 0.3))
                .bg(alpha(theme::DANGER, 0.1))
                .px(px(16.))
                .py(px(12.))
                .text_color(color(theme::DANGER))
                .child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(8.))
                        .child(icon(assets::SHIELD_OFF, 16., theme::DANGER).mt(px(2.)))
                        .child(div().flex_1().min_w_0().child(text)),
                )
                .when(self.confirm_reset.is_some() && shows, |banner| {
                    banner.child(
                        div()
                            .ml(px(24.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .child(RESET_WARNING),
                    )
                })
                .child(
                    div()
                        .ml(px(24.))
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.))
                        .children(buttons)
                        .children(
                            self.reset_note.clone().map(|note| {
                                div().text_size(px(12.)).line_height(px(16.)).child(note)
                            }),
                        ),
                ),
        )
    }

    fn render_general(&self, cx: &mut Context<Self>) -> Vec<Div> {
        let autostart = switch(
            "autostart",
            self.autostart.open_at_login,
            !self.autostart.available,
            false,
            cx.listener(|this, _: &ClickEvent, _, cx| this.set_autostart(cx)),
        );
        let general = panel(
            "General",
            None,
            None,
            div()
                .child(setting_row(
                    "Start with Windows",
                    Some("Starts One Launch when you sign in to Windows.".into()),
                    self.refusal(Refused::Autostart),
                    true,
                    false,
                    autostart,
                ))
                .child(self.toggle_row(
                    SettingKey::WindowCloseToTray,
                    "Minimize One Launch to System Tray",
                    "Closing One Launch minimizes the app.",
                    (false, true),
                    cx,
                )),
        );
        let (state, hint) = match &self.update {
            UpdateStatus::Unavailable(reason) => (self.update.state_text(), Some(reason.hint())),
            other => (other.state_text(), None),
        };
        let check = Button::new("check-updates", Tone::Secondary, "Check for updates")
            .glyph(assets::RESCAN)
            .disabled(!self.update.can_check())
            .on_click(|_, _, _| {});
        let updates = panel(
            "Updates",
            None,
            Some(check),
            div()
                .child(self.toggle_row(
                    SettingKey::UpdatesAuto,
                    "Keep One Launch updated automatically",
                    "Downloads new versions in the background and installs them the next time One Launch restarts.",
                    (true, false),
                    cx,
                ))
                .child(setting_row(
                    "Version",
                    hint.map(SharedString::from),
                    None,
                    false,
                    true,
                    div()
                        .truncate()
                        .child(format!("{} · {state}", self.info.version)),
                )),
        );
        vec![general, updates]
    }

    fn render_launchers(&self, cx: &mut Context<Self>) -> Vec<Div> {
        let hide_hint = if self.on(SettingKey::WindowCloseToTray) {
            "Sends the window to the tray as the game starts, when no account switch is needed."
        } else {
            "Needs Minimize One Launch to System Tray, in General."
        };
        let status = self.render_launchers_panel(cx);
        vec![status, panel(
            "Play",
            Some("What One Launch does for you when you press Play.".into()),
            None,
            div()
                .child(self.toggle_row(
                    SettingKey::SteamAutoLogin,
                    "Steam automatic sign-in",
                    "Passes a stored Steam password to Steam for you; other programs can see it while Steam runs.",
                    (true, false),
                    cx,
                ))
                .child(self.toggle_row(
                    SettingKey::RiotPressPlay,
                    "Start Riot games for you",
                    "Presses Play in the Riot Client for you. Off, it opens the game's page and you press Play.",
                    (false, false),
                    cx,
                ))
                .child(self.toggle_row(
                    SettingKey::PlayHideToTray,
                    "Hide One Launch after Play",
                    hide_hint,
                    (false, true),
                    cx,
                )),
        )]
    }

    fn render_advanced(&mut self, cx: &mut Context<Self>) -> Vec<Div> {
        let epic = self.render_pick(Pick::EpicLaunch, cx);
        let battlenet = self.render_pick(Pick::BattlenetLaunch, cx);
        let log_level = self.render_pick(Pick::LogLevel, cx);
        let launching = panel(
            "Launching",
            Some("How Play switches and starts games.".into()),
            None,
            div()
                .child(self.toggle_row(
                    SettingKey::DryRun,
                    "Preview mode",
                    "Play shows what a switch would do instead of doing it.",
                    (true, false),
                    cx,
                ))
                .child(setting_row(
                    "Epic launch link",
                    Some("Change only if Epic games stop launching after an Epic update.".into()),
                    self.refusal(Refused::Setting(SettingKey::EpicLaunchUri)),
                    false,
                    false,
                    epic,
                ))
                .child(setting_row(
                    "Battle.net launch",
                    Some("Change only if games stop launching after a Battle.net update.".into()),
                    self.refusal(Refused::Setting(SettingKey::BattlenetLaunch)),
                    false,
                    true,
                    battlenet,
                )),
        );

        let running = matches!(self.test, SelfTest::Running);
        let run_test = Button::new(
            "run-test",
            Tone::Secondary,
            if running { "Testing…" } else { "Run test" },
        )
        .disabled(running || !self.vault_works())
        .leading(self.ringed_on(Control::RunTest))
        .on_click(cx.listener(|this, _, _, cx| {
            this.clicked(Control::RunTest);
            this.run_test(cx);
        }));
        let run_test = self.track(Control::RunTest, run_test);
        let status = match self.vault.map(VaultHealth::state) {
            None => div().child("Checking…"),
            Some(EncryptionState::Working) => {
                shield(assets::SHIELD_CHECK, theme::SUCCESS, "Working")
            }
            Some(state) => shield(assets::SHIELD_OFF, theme::DANGER, state.label()),
        };
        let last_test = match &self.test {
            SelfTest::Idle => div().child("Not run"),
            SelfTest::Running => div().child("Running…"),
            SelfTest::Passed(took) => div()
                .text_color(color(theme::SUCCESS))
                .child(format!("Passed in {took} ms")),
            SelfTest::Failed(message) => div()
                .min_w_0()
                .text_color(color(theme::DANGER))
                .child(format!("Failed: {message}")),
        };
        let encryption = panel(
            "Encryption",
            Some("Windows encrypts passwords and saved sign-ins for your account; nothing is in plain text.".into()),
            Some(run_test),
            div()
                .child(setting_row("Status", None, None, true, false, status))
                .child(setting_row(
                    "Last test",
                    Some("Stores a random value, reads it back, deletes it.".into()),
                    None,
                    false,
                    true,
                    last_test,
                )),
        );

        let folder_button = |this: &Self,
                             folder: Folder,
                             id: &'static str,
                             label: &'static str,
                             cx: &mut Context<Self>| {
            let control = Control::Open(folder);
            let button = Button::new(id, Tone::Secondary, label)
                .glyph(assets::FOLDER_OPEN)
                .leading(this.ringed_on(control))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.clicked(control);
                    this.open_folder(folder, cx);
                }));
            this.track(control, button)
        };
        let actions = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .child(folder_button(self, Folder::Logs, "open-logs", "Logs", cx))
            .child(folder_button(
                self,
                Folder::Data,
                "open-data",
                "Data folder",
                cx,
            ))
            .into_any_element();
        let data_dir = self.info.data_dir.display().to_string();
        let diagnostics = panel(
            "Diagnostics",
            None,
            Some(actions),
            div()
                .children(self.folder_error.clone().map(|error| {
                    div()
                        .mb(px(12.))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color(theme::DANGER))
                        .child(format!("Could not open the folder: {error}"))
                }))
                .child(setting_row(
                    "Log level",
                    Some("Logs stay on this PC, with passwords, tokens, emails and account ids removed.".into()),
                    self.refusal(Refused::Setting(SettingKey::LogLevel)),
                    true,
                    false,
                    log_level,
                ))
                .child(setting_row(
                    "Data folder",
                    None,
                    None,
                    false,
                    false,
                    div()
                        .min_w_0()
                        .font_family(theme::MONO)
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .child(data_dir),
                ))
                .child(setting_row(
                    "Build",
                    None,
                    None,
                    false,
                    true,
                    div().child(self.info.build),
                )),
        );
        vec![launching, encryption, diagnostics]
    }
}

fn shield(glyph: &'static str, hex: u32, label: &'static str) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .text_color(color(hex))
        .child(icon(glyph, 16., hex))
        .child(label)
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        if self.viewport.is_some_and(|seen| seen != viewport) {
            self.menu = None;
        }
        self.viewport = Some(viewport);
        if std::mem::take(&mut self.reveal) {
            let (spot, scroll) = (self.focused.clone(), self.scroll.clone());
            window.on_next_frame(move |window, _| {
                if let Some(bounds) = spot.get() {
                    scroll.reveal(bounds);
                    window.refresh();
                }
            });
        }
        let header = self.render_header(cx);
        let panels = match self.tab.get() {
            SettingsTab::General => self.render_general(cx),
            SettingsTab::Launchers => self.render_launchers(cx),
            SettingsTab::Backups => self.render_backups(cx),
            SettingsTab::Advanced => self.render_advanced(cx),
        };
        let body = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .pb(px(24.))
            .children(self.render_banner(cx))
            .children(panels);
        let menu = self.render_menu(viewport, cx);
        let dialog = self
            .render_restore(cx)
            .map(|dialog| overlay(viewport, 2, dialog));
        div()
            .id("settings")
            .size_full()
            .on_scroll(cx.listener(|this, _, _, cx| {
                if this.menu.take().is_some() {
                    cx.notify();
                }
            }))
            .child(scroll_page("settings-page", &self.scroll, header, body))
            .children(menu)
            .children(dialog)
    }
}
