// What sits over the views: the launch dialog with the job it started and the banner that stands for that job while the dialog is closed, and the question asked at start about a switch the last run did not finish.

mod banner;
mod dialog;
mod recovery;

use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    Context, EventEmitter, Keystroke, Pixels, ScrollHandle, SharedString, Size, Stateful,
    Subscription, Task, Window, anchored, deferred, div, point, px,
};
use ol_api::launch::in_pick_order;
use ol_api::{ApiError, Backend, StartLaunch};
use ol_model::{Account, Game, JobState, LaunchJob, LaunchPreflight, SettingKey, SwitchRecovery};

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
    _keys: Subscription,
}

// Sent when the job that was followed is no longer running: what the Library shows has changed.
pub struct JobSettled;

impl EventEmitter<JobSettled> for LaunchHost {}

// The job the app follows, with the game it was started for.
struct Active {
    job: LaunchJob,
    game: Game,
}

struct Request {
    key: u64,
    game: Game,
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
    steps_open: bool,
    plan_open: bool,
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
        let this = cx.weak_entity();
        let keys = cx.intercept_keystrokes(move |event, _, cx| {
            let taken = this
                .update(cx, |this, cx| this.on_keystroke(&event.keystroke, cx))
                .unwrap_or(false);
            if taken {
                cx.stop_propagation();
            }
        });
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
            _keys: keys,
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
            .map(|request| request.game.id.as_str())
    }

    // The room the banner takes above the views.
    pub fn banner_height(&self) -> f32 {
        if self.active.is_some() && self.request.is_none() {
            BANNER
        } else {
            0.
        }
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

    // Opens the Play flow for a game. `instant` plays at once as `account`, and shows the dialog only when something needs the user.
    pub fn open(
        &mut self,
        game: Game,
        account: Option<String>,
        instant: bool,
        cx: &mut Context<Self>,
    ) {
        self.serial += 1;
        let mut request = Request {
            key: self.serial,
            game,
            wanted: account,
            silent: instant,
            asked: false,
            accounts: Accounts::Loading,
            selected: None,
            stage: Stage::Pick,
            starting: false,
            stopping: None,
            note: None,
            steps_open: false,
            plan_open: false,
        };
        // A running job is always the one shown, under its own game's name; a finished one only when it was for this game and the user asked to see it.
        let follows = match &self.active {
            Some(active) if active.job.state == JobState::Running => {
                request.game = active.game.clone();
                true
            }
            Some(active) => !instant && active.game.id == request.game.id,
            None => false,
        };
        if follows {
            request.stage = Stage::Job;
            request.silent = false;
        } else {
            self.active = None;
        }
        if self.backend.can_reach(request.game.launcher) {
            let (key, launcher, backend) =
                (request.key, request.game.launcher, self.backend.clone());
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
        let Some(account_id) = request.selected.clone() else {
            return;
        };
        request.stage = Stage::Checking;
        let (key, game_id, backend) = (request.key, request.game.id.clone(), self.backend.clone());
        cx.spawn(async move |this, cx| {
            let preflight = backend.launch_preflight(&account_id, Some(&game_id)).await;
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
            let start = StartLaunch {
                account_id: account_id.clone(),
                game_id: Some(game.id.clone()),
                confirm_close,
            };
            let outcome = match backend.start_launch(start).await {
                Ok(job) => Ok(job),
                // The launcher came up between the check and the start: ask, rather than stop at a refusal.
                Err(refused) if refused.code == "CONFIRM_REQUIRED" => Err(
                    match backend.launch_preflight(&account_id, Some(&game.id)).await {
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
        game: Game,
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
        active.job = job;
        if !running {
            cx.emit(JobSettled);
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

    // Preview mode goes off and the switch that was shown runs.
    fn switch_now(&mut self, cx: &mut Context<Self>) {
        let Some(request) = &self.request else {
            return;
        };
        let (key, backend) = (request.key, self.backend.clone());
        cx.spawn(async move |this, cx| {
            let saved = backend
                .set_setting(SettingKey::DryRun, "false".to_owned())
                .await;
            this.update(cx, |this, cx| {
                let Some(request) = this.request_mut(key) else {
                    return;
                };
                if let Err(error) = saved {
                    request.stage = Stage::Error(error.message.into());
                    return this.changed(cx);
                }
                this.check(cx);
            })
            .ok();
        })
        .detach();
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
        self.back_to_pick(cx);
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

    fn finish(&mut self, cx: &mut Context<Self>) {
        self.active = None;
        self.close(cx);
    }

    fn show_job(&mut self, cx: &mut Context<Self>) {
        if let Some(active) = &self.active {
            let (game, account) = (active.game.clone(), active.job.account.id.clone());
            self.open(game, Some(account), false, cx);
        }
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

    // True when the key is a dialog's: nothing behind the dialog hears it.
    fn on_keystroke(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if !self.is_modal() {
            return false;
        }
        let held = keystroke.modifiers;
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
            Stage::Pick if request.empty() => self.close(cx),
            Stage::Pick => self.check(cx),
            Stage::Blocked(_) => self.back_to_pick(cx),
            Stage::Job if !self.job_running() => self.finish(cx),
            Stage::Unreachable => self.close(cx),
            _ => {}
        }
    }
}

fn overlay(
    viewport: Size<Pixels>,
    priority: usize,
    backdrop: Stateful<gpui::Div>,
) -> impl IntoElement {
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
                    .map(|dialog| overlay(viewport, 3, dialog)),
            )
    }
}
