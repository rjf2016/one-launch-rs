// What sits over the views: the launch dialog with the job it started and the banner that stands for that job while the dialog is closed, and the question asked at start about a switch the last run did not finish.

mod banner;
mod dialog;
mod recovery;

use std::mem::{Discriminant, discriminant};
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    Context, Div, EventEmitter, Keystroke, Pixels, ScrollHandle, SharedString, Size, Stateful,
    Task, Window, anchored, deferred, div, point, px,
};
use ol_api::clipboard::CopyField;
use ol_api::launch::in_pick_order;
use ol_api::{ApiError, Backend, StartLaunch};
use ol_model::{
    Account, Game, JobState, LaunchJob, LaunchPreflight, LauncherId, SettingKey, SwitchRecovery,
};

use crate::account::copy_note;
use crate::shell::{Go, Route};
use crate::widgets::Appeared;

pub(crate) use dialog::{generated_at, plan_view};

const TITLE_BAR: f32 = 36.;
// Its button, the room above and below it, and the line under it.
const BANNER: f32 = 45.;
// As often as the Electron app asks how a running job fares.
const POLL: Duration = Duration::from_millis(150);
const DONE_BANNER_STAYS: Duration = Duration::from_secs(8);

pub struct LaunchHost {
    backend: Backend,
    request: Option<Request>,
    active: Option<Active>,
    recovery: Option<Recovery>,
    serial: u64,
    following: Option<Task<()>>,
    dismissal: Option<(String, Task<()>)>,
    blurred: bool,
    body: ScrollHandle,
}

// Sent when the job that was followed is no longer running: what the Library shows has changed.
pub struct JobSettled;

impl EventEmitter<JobSettled> for LaunchHost {}

// A job the app followed has ended: what decides whether the window goes to the tray.
#[derive(Clone)]
pub struct PlayEnded {
    pub job: LaunchJob,
}

impl EventEmitter<PlayEnded> for LaunchHost {}

impl EventEmitter<Go> for LaunchHost {}

// What a click asks of the Play flow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    // The account list, also over a finished job of the same game.
    List,
    // The account list, or the finished job of the same game.
    ListOrJob,
    // A one-click Play as the account given: the dialog shows only when something needs the user.
    AtOnce,
}

// A running job is always the one shown; a finished one only when it was for the same game, or a plain switch to the same account, and the click asked to see it.
pub(crate) fn shows_job(ask: Ask, running: bool, same: bool) -> bool {
    running || (ask == Ask::ListOrJob && same)
}

// The job the app follows, with the game it was started for.
struct Active {
    job: LaunchJob,
    game: Option<Game>,
}

struct Request {
    key: u64,
    game: Option<Game>,
    launcher: LauncherId,
    wanted: Option<String>,
    // A one-click Play: nothing is drawn until something needs the user.
    silent: bool,
    asked: bool,
    accounts: Accounts,
    selected: Option<String>,
    stage: Stage,
    starting: bool,
    stopping: Option<String>,
    note: Option<SharedString>,
    copied: Option<SharedString>,
    steps_open: bool,
    plan_open: bool,
    // Try again goes back to what was asked, not to the running job the dialog shows instead.
    asked_for: Option<(Option<Game>, LauncherId)>,
    shown: (bool, Discriminant<Stage>),
    appeared: Appeared,
}

enum Accounts {
    Loading,
    Failed(SharedString),
    Ready(Vec<Account>),
}

enum Stage {
    Pick,
    Checking,
    Confirm(Box<LaunchPreflight>),
    Preview(Box<LaunchPreflight>),
    Blocked(Box<LaunchPreflight>),
    // The dialog shows the job the app follows, running or over.
    Job,
    Error(SharedString),
    // This build has nothing to reach the game's launcher with.
    Unreachable,
}

struct Recovery {
    found: SwitchRecovery,
    stage: RecoveryStage,
}

enum RecoveryStage {
    Ask,
    Restoring,
    Restored,
    Failed(SharedString),
}

impl Request {
    fn candidates(&self) -> &[Account] {
        match &self.accounts {
            Accounts::Ready(candidates) => candidates,
            Accounts::Loading | Accounts::Failed(_) => &[],
        }
    }

    fn account(&self) -> Option<&Account> {
        let selected = self.selected.as_deref()?;
        self.candidates()
            .iter()
            .find(|account| account.id == selected)
    }

    fn empty(&self) -> bool {
        matches!(self.stage, Stage::Pick)
            && matches!(&self.accounts, Accounts::Ready(candidates) if candidates.is_empty())
    }
}

impl LaunchHost {
    pub fn new(backend: Backend, cx: &mut Context<Self>) -> Self {
        cx.spawn({
            let backend = backend.clone();
            async move |this, cx| {
                // A journal that cannot be read is no reason to hold up the window.
                let Ok(Some(found)) = backend.recovery().await else {
                    return;
                };
                this.update(cx, |this, cx| {
                    // Without a way to reach the launcher nothing could be put back, so nothing is asked and the journal stays.
                    if this.backend.can_reach(found.launcher) {
                        this.recovery = Some(Recovery {
                            found,
                            stage: RecoveryStage::Ask,
                        });
                        cx.notify();
                    }
                })
                .ok();
            }
        })
        .detach();
        Self {
            backend,
            request: None,
            active: None,
            recovery: None,
            serial: 0,
            following: None,
            dismissal: None,
            blurred: false,
            body: ScrollHandle::new(),
        }
    }

    pub fn job_running(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.job.state == JobState::Running)
    }

    // The game a one-click Play is being checked for, while nothing of it shows.
    pub fn silent_game(&self) -> Option<&str> {
        self.request
            .as_ref()
            .filter(|request| request.silent)
            .and_then(|request| request.game.as_ref())
            .map(|game| game.id.as_str())
    }

    // The room the banner takes above the views.
    pub fn banner_height(&self) -> f32 {
        if self.active.is_some() && self.request.is_none() {
            BANNER
        } else {
            0.
        }
    }

    pub fn recovery_up(&self) -> bool {
        self.recovery.is_some()
    }

    // A dialog is up: the keyboard is its own.
    pub fn is_modal(&self) -> bool {
        self.recovery.is_some() || self.request.as_ref().is_some_and(|request| !request.silent)
    }

    fn busy(&self) -> bool {
        self.request.as_ref().is_some_and(|request| {
            request.starting
                || match request.stage {
                    Stage::Checking => true,
                    Stage::Job => self.job_running(),
                    _ => false,
                }
        })
    }

    fn request_mut(&mut self, key: u64) -> Option<&mut Request> {
        self.request.as_mut().filter(|request| request.key == key)
    }

    pub fn open(&mut self, game: Game, account: Option<String>, ask: Ask, cx: &mut Context<Self>) {
        let launcher = game.launcher;
        self.open_for(Some(game), launcher, account, ask, cx);
    }

    pub fn open_switch(&mut self, launcher: LauncherId, account: String, cx: &mut Context<Self>) {
        self.open_for(None, launcher, Some(account), Ask::ListOrJob, cx);
    }

    fn open_for(
        &mut self,
        game: Option<Game>,
        launcher: LauncherId,
        account: Option<String>,
        ask: Ask,
        cx: &mut Context<Self>,
    ) {
        // The start under way reports to the request that asked for it.
        if self
            .request
            .as_ref()
            .is_some_and(|request| request.starting)
        {
            return;
        }
        self.serial += 1;
        let instant = ask == Ask::AtOnce;
        let mut request = Request {
            key: self.serial,
            game,
            launcher,
            wanted: account,
            silent: instant,
            asked: false,
            accounts: Accounts::Loading,
            selected: None,
            stage: Stage::Pick,
            starting: false,
            stopping: None,
            note: None,
            copied: None,
            steps_open: false,
            plan_open: false,
            asked_for: None,
            shown: (instant, discriminant(&Stage::Pick)),
            appeared: Appeared::now(),
        };
        let follows = match &self.active {
            Some(active) => {
                let running = active.job.state == JobState::Running;
                let same = match (&active.game, &request.game) {
                    (Some(played), Some(asked)) => played.id == asked.id,
                    (None, None) => request.wanted.as_deref() == Some(&*active.job.account.id),
                    _ => false,
                };
                // A running job is shown under its own game's name.
                if running {
                    let asked = (
                        std::mem::replace(&mut request.game, active.game.clone()),
                        std::mem::replace(&mut request.launcher, active.job.launcher),
                    );
                    request.asked_for = Some(asked);
                }
                shows_job(ask, running, same)
            }
            None => false,
        };
        if follows {
            request.stage = Stage::Job;
            request.silent = false;
        } else {
            self.active = None;
        }
        if self.backend.can_reach(request.launcher) {
            let (key, launcher, backend) = (request.key, request.launcher, self.backend.clone());
            cx.spawn(async move |this, cx| {
                let accounts = backend.accounts().await.map(|accounts| {
                    in_pick_order(
                        accounts
                            .into_iter()
                            .filter(|account| account.launcher == launcher)
                            .collect(),
                    )
                });
                this.update(cx, |this, cx| this.accounts_loaded(key, accounts, cx))
                    .ok();
            })
            .detach();
        } else if !follows {
            request.stage = Stage::Unreachable;
            request.silent = false;
        }
        self.request = Some(request);
        self.changed(cx);
    }

    fn accounts_loaded(
        &mut self,
        key: u64,
        accounts: Result<Vec<Account>, ApiError>,
        cx: &mut Context<Self>,
    ) {
        let Some(request) = self.request_mut(key) else {
            return;
        };
        let instant = request.silent && !request.asked && matches!(request.stage, Stage::Pick);
        request.asked = true;
        match accounts {
            Ok(candidates) => {
                let wanted = request
                    .wanted
                    .as_deref()
                    .and_then(|wanted| candidates.iter().find(|account| account.id == wanted));
                let known = wanted.is_some();
                request.selected = wanted
                    .or(candidates.first())
                    .map(|account| account.id.clone());
                request.accounts = Accounts::Ready(candidates);
                if instant && known {
                    return self.check(cx);
                }
                request.silent = false;
            }
            Err(error) => {
                request.accounts = Accounts::Failed(error.message.into());
                request.silent = false;
            }
        }
        self.changed(cx);
    }

    fn select(&mut self, account_id: String, cx: &mut Context<Self>) {
        if let Some(request) = &mut self.request
            && matches!(request.stage, Stage::Pick)
        {
            request.selected = Some(account_id);
            cx.notify();
        }
    }

    fn check(&mut self, cx: &mut Context<Self>) {
        let Some(request) = &mut self.request else {
            return;
        };
        if request.starting || !matches!(request.stage, Stage::Pick | Stage::Preview(_)) {
            return;
        }
        let Some(account_id) = request.account().map(|account| account.id.clone()) else {
            return;
        };
        request.stage = Stage::Checking;
        let game_id = request.game.as_ref().map(|game| game.id.clone());
        let (key, backend) = (request.key, self.backend.clone());
        cx.spawn(async move |this, cx| {
            let preflight = backend
                .launch_preflight(&account_id, game_id.as_deref())
                .await;
            this.update(cx, |this, cx| this.checked(key, preflight, cx))
                .ok();
        })
        .detach();
        self.changed(cx);
    }

    fn checked(
        &mut self,
        key: u64,
        preflight: Result<LaunchPreflight, ApiError>,
        cx: &mut Context<Self>,
    ) {
        let Some(request) = self.request_mut(key) else {
            return;
        };
        if !matches!(request.stage, Stage::Checking) {
            return;
        }
        request.stage = match preflight {
            Err(error) => Stage::Error(error.message.into()),
            Ok(preflight) if preflight.blocked.is_some() => Stage::Blocked(Box::new(preflight)),
            Ok(preflight) if preflight.dry_run => Stage::Preview(Box::new(preflight)),
            Ok(preflight)
                if preflight.needs_confirmation || !preflight.plan.warnings.is_empty() =>
            {
                Stage::Confirm(Box::new(preflight))
            }
            Ok(_) => return self.start(false, cx),
        };
        request.plan_open = false;
        self.changed(cx);
    }

    fn start(&mut self, confirm_close: bool, cx: &mut Context<Self>) {
        let Some(request) = &mut self.request else {
            return;
        };
        let Some(account_id) = request.selected.clone() else {
            return;
        };
        if request.starting {
            return;
        }
        request.starting = true;
        let (key, game, backend) = (request.key, request.game.clone(), self.backend.clone());
        cx.spawn(async move |this, cx| {
            let game_id = game.as_ref().map(|game| game.id.clone());
            let start = StartLaunch {
                account_id: account_id.clone(),
                game_id: game_id.clone(),
                confirm_close,
            };
            let outcome = match backend.start_launch(start).await {
                Ok(job) => Ok(job),
                // The launcher came up between the check and the start: ask, rather than stop at a refusal.
                Err(refused) if refused.code == "CONFIRM_REQUIRED" => Err(
                    match backend
                        .launch_preflight(&account_id, game_id.as_deref())
                        .await
                    {
                        Ok(preflight) => Stage::Confirm(Box::new(preflight)),
                        Err(error) => Stage::Error(error.message.into()),
                    },
                ),
                Err(error) => Err(Stage::Error(error.message.into())),
            };
            this.update(cx, |this, cx| this.started(key, game, outcome, cx))
                .ok();
        })
        .detach();
        self.changed(cx);
    }

    fn started(
        &mut self,
        key: u64,
        game: Option<Game>,
        outcome: Result<LaunchJob, Stage>,
        cx: &mut Context<Self>,
    ) {
        match outcome {
            Ok(job) => {
                let (job_id, direct) = (job.id.clone(), job.direct);
                self.active = Some(Active { job, game });
                self.follow(job_id, cx);
                let mut nothing_to_watch = false;
                if let Some(request) = self.request_mut(key) {
                    request.starting = false;
                    request.stage = Stage::Job;
                    nothing_to_watch = request.silent && direct;
                }
                // The banner says the rest.
                if nothing_to_watch {
                    self.request = None;
                }
            }
            Err(stage) => {
                if let Some(request) = self.request_mut(key) {
                    request.starting = false;
                    request.plan_open = false;
                    request.stage = stage;
                }
            }
        }
        self.changed(cx);
    }

    fn follow(&mut self, job_id: String, cx: &mut Context<Self>) {
        self.following = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let running = this.update(cx, |this, cx| this.poll(&job_id, cx));
                if !running.unwrap_or(false) {
                    break;
                }
            }
        }));
    }

    fn poll(&mut self, job_id: &str, cx: &mut Context<Self>) -> bool {
        let Some(active) = self
            .active
            .as_mut()
            .filter(|active| active.job.id == job_id)
        else {
            return false;
        };
        let job = self.backend.launch_status(job_id).unwrap_or_else(|error| {
            let mut job = active.job.clone();
            job.state = JobState::Failed;
            job.error = Some(error.message);
            job
        });
        if job == active.job {
            return true;
        }
        let running = job.state == JobState::Running;
        let handing_over = job.attention.is_some() && running;
        let ended = (!running).then(|| PlayEnded { job: job.clone() });
        active.job = job;
        if !handing_over && let Some(request) = &mut self.request {
            request.copied = None;
        }
        if let Some(ended) = ended {
            cx.emit(JobSettled);
            cx.emit(ended);
        }
        self.changed(cx);
        running
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        let (Some(request), Some(active)) = (&mut self.request, &self.active) else {
            return;
        };
        request.stopping = Some(active.job.id.clone());
        if let Err(error) = self.backend.cancel_launch(&active.job.id) {
            request.stopping = None;
            request.note = Some(error.message.into());
        }
        cx.notify();
    }

    fn copy(&mut self, field: CopyField, cx: &mut Context<Self>) {
        let (Some(request), Some(active)) = (&self.request, &self.active) else {
            return;
        };
        let (key, account_id, backend) = (
            request.key,
            active.job.account.id.clone(),
            self.backend.clone(),
        );
        cx.spawn(async move |this, cx| {
            let copied = backend.copy_to_clipboard(&account_id, field).await;
            this.update(cx, |this, cx| {
                if let Some(request) = this.request_mut(key) {
                    request.copied = Some(match copied {
                        Ok(copied) => copy_note(field, &copied).into(),
                        Err(error) => error.message.into(),
                    });
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    // Preview mode goes off and the switch that was shown runs.
    fn switch_now(&mut self, cx: &mut Context<Self>) {
        let Some(request) = &mut self.request else {
            return;
        };
        if request.starting || !matches!(request.stage, Stage::Preview(_)) {
            return;
        }
        // Until the setting is written the dialog stays on this account: no Back, no Escape.
        request.starting = true;
        let (key, backend) = (request.key, self.backend.clone());
        cx.spawn(async move |this, cx| {
            let saved = backend
                .set_setting(SettingKey::DryRun, "false".to_owned())
                .await;
            this.update(cx, |this, cx| {
                let Some(request) = this.request_mut(key) else {
                    return;
                };
                request.starting = false;
                if let Err(error) = saved {
                    request.stage = Stage::Error(error.message.into());
                    return this.changed(cx);
                }
                if !matches!(request.stage, Stage::Preview(_)) {
                    return this.changed(cx);
                }
                this.check(cx);
            })
            .ok();
        })
        .detach();
        self.changed(cx);
    }

    fn back_to_pick(&mut self, cx: &mut Context<Self>) {
        if let Some(request) = &mut self.request {
            request.stage = Stage::Pick;
            request.note = None;
            self.changed(cx);
        }
    }

    fn try_again(&mut self, cx: &mut Context<Self>) {
        self.active = None;
        let asked = self.request.as_mut().and_then(|request| {
            let (game, launcher) = request.asked_for.take()?;
            Some((game, launcher, request.wanted.clone()))
        });
        match asked {
            Some((game, launcher, wanted)) => {
                self.request = None;
                self.open_for(game, launcher, wanted, Ask::List, cx);
            }
            None => self.back_to_pick(cx),
        }
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.request = None;
        self.changed(cx);
    }

    // Escape, the X and a click beside the dialog: not while something is underway.
    fn dismiss(&mut self, cx: &mut Context<Self>) {
        if !self.busy() {
            self.close(cx);
        }
    }

    // The window went to the tray as the job ended: the dialog that shows the job goes as its X takes it, and the banner stands for the job while it lasts.
    pub fn close_finished_job(&mut self, job_id: &str, cx: &mut Context<Self>) {
        let shows_it = self
            .request
            .as_ref()
            .is_some_and(|request| matches!(request.stage, Stage::Job))
            && self
                .active
                .as_ref()
                .is_some_and(|active| active.job.id == job_id);
        if shows_it {
            self.dismiss(cx);
        }
    }

    fn finish(&mut self, cx: &mut Context<Self>) {
        self.active = None;
        self.close(cx);
    }

    fn show_job(&mut self, cx: &mut Context<Self>) {
        if let Some(active) = &self.active {
            let (game, launcher, account) = (
                active.game.clone(),
                active.job.launcher,
                active.job.account.id.clone(),
            );
            self.open_for(game, launcher, Some(account), Ask::ListOrJob, cx);
        }
    }

    fn go_to_accounts(&mut self, cx: &mut Context<Self>) {
        let Some(request) = self.request.take_if(|request| request.empty()) else {
            return;
        };
        let route = if can_import(request.launcher) {
            Route::Import(request.launcher)
        } else {
            Route::NewAccount(request.launcher)
        };
        self.changed(cx);
        cx.emit(Go {
            route,
            guarded: true,
            return_to: request.game,
        });
    }

    fn forget_job(&mut self, cx: &mut Context<Self>) {
        self.active = None;
        self.changed(cx);
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        let direct = self.active.as_ref().is_some_and(|active| active.job.direct);
        if let Some(request) = &mut self.request
            && request.silent
        {
            request.silent = match request.stage {
                Stage::Pick | Stage::Checking => true,
                Stage::Job => direct,
                _ => false,
            };
        }
        if let Some(request) = &mut self.request {
            let shown = (request.silent, discriminant(&request.stage));
            if request.shown != shown {
                request.shown = shown;
                request.appeared = Appeared::now();
            }
        }
        self.arm_dismissal(cx);
        cx.notify();
    }

    // A launch that went well needs no attention: its banner goes by itself, counted while no dialog is open. Any other end stays until it is dismissed.
    fn arm_dismissal(&mut self, cx: &mut Context<Self>) {
        let launched = self
            .active
            .as_ref()
            .filter(|active| active.job.state == JobState::Done && self.request.is_none())
            .map(|active| active.job.id.clone());
        let Some(job_id) = launched else {
            self.dismissal = None;
            return;
        };
        if self
            .dismissal
            .as_ref()
            .is_some_and(|(armed, _)| *armed == job_id)
        {
            return;
        }
        let timer = cx.spawn({
            let job_id = job_id.clone();
            async move |this, cx| {
                cx.background_executor().timer(DONE_BANNER_STAYS).await;
                this.update(cx, |this, cx| {
                    if this
                        .active
                        .as_ref()
                        .is_some_and(|active| active.job.id == job_id)
                    {
                        this.active = None;
                        cx.notify();
                    }
                })
                .ok();
            }
        });
        self.dismissal = Some((job_id, timer));
    }

    fn keep(&mut self, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            // Asked once: the dialog goes whether or not the journal could be cleared.
            let _ = backend.dismiss_recovery().await;
            this.update(cx, |this, cx| this.recovery_done(cx)).ok();
        })
        .detach();
    }

    fn restore(&mut self, cx: &mut Context<Self>) {
        let Some(recovery) = &mut self.recovery else {
            return;
        };
        recovery.stage = RecoveryStage::Restoring;
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let restored = backend.restore_recovery().await;
            this.update(cx, |this, cx| {
                if let Some(recovery) = &mut this.recovery {
                    recovery.stage = match restored {
                        Ok(_) => RecoveryStage::Restored,
                        Err(error) => RecoveryStage::Failed(error.message.into()),
                    };
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn recovery_done(&mut self, cx: &mut Context<Self>) {
        self.recovery = None;
        cx.notify();
    }

    // True when the key is a dialog's: nothing behind the dialog hears it. The Shell hands each key here, after its own question.
    pub(crate) fn on_keystroke(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let held = keystroke.modifiers;
        // The window's own keys, Alt+F4 among them, reach Windows only when nothing stops them.
        if !self.is_modal() || held.alt || held.platform {
            return false;
        }
        let plain = !held.control && !held.alt && !held.shift && !held.platform && !held.function;
        match keystroke.key.as_str() {
            "enter" if plain => self.press_leading(cx),
            "escape" if plain && self.recovery.is_none() => self.dismiss(cx),
            _ => {}
        }
        true
    }

    // Enter presses the button that carries the ring.
    fn press_leading(&mut self, cx: &mut Context<Self>) {
        if let Some(recovery) = &self.recovery {
            match recovery.stage {
                RecoveryStage::Ask if recovery::leads_with_restore(&recovery.found) => {
                    self.restore(cx);
                }
                RecoveryStage::Ask => self.keep(cx),
                RecoveryStage::Restored => self.recovery_done(cx),
                RecoveryStage::Restoring | RecoveryStage::Failed(_) => {}
            }
            return;
        }
        let Some(request) = &self.request else {
            return;
        };
        match request.stage {
            Stage::Pick if request.empty() => self.go_to_accounts(cx),
            Stage::Pick => self.check(cx),
            Stage::Blocked(_) => self.back_to_pick(cx),
            Stage::Job if !self.job_running() => self.finish(cx),
            Stage::Unreachable => self.close(cx),
            _ => {}
        }
    }
}

// Epic and the Riot Client keep no account list, as in the Electron app.
pub(crate) fn can_import(launcher: LauncherId) -> bool {
    matches!(launcher, LauncherId::Steam | LauncherId::Battlenet)
}

pub(crate) fn overlay(
    viewport: Size<Pixels>,
    priority: usize,
    backdrop: Stateful<Div>,
) -> impl IntoElement + use<> {
    deferred(
        anchored()
            .position(point(px(0.), px(TITLE_BAR)))
            .snap_to_window()
            .child(
                backdrop
                    .w(viewport.width)
                    .h(viewport.height - px(TITLE_BAR)),
            ),
    )
    .with_priority(priority)
}

impl Render for LaunchHost {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let modal = self.is_modal();
        if modal && !self.blurred {
            // Whatever had the keyboard behind the dialog gives it up.
            cx.defer_in(window, |_, window, cx| window.blur(cx));
        }
        self.blurred = modal;
        let viewport = window.viewport_size();
        div()
            .flex_none()
            .children(self.render_banner(cx))
            .children(
                self.render_dialog(cx)
                    .map(|dialog| overlay(viewport, 2, dialog)),
            )
            .children(
                self.render_recovery(cx)
                    .map(|dialog| overlay(viewport, 4, dialog)),
            )
    }
}
