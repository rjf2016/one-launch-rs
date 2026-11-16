// A launch job: switch the launcher to an account, then start a game. One job at a time. `prepare` and `settle` run where the database lives; `run` has no database, so it can wait for a launcher on a thread of its own, and asks the database's thread for what a switch needs of the accounts and their saved sign-ins.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use ol_adapters::steam::SwitchPreflight;
use ol_adapters::{AccountRef, AdapterError, GameRef};
use ol_model::{
    Account, Ciphertext, Game, JobAccount, JobGame, JobState, JobStep, JobStepStatus,
    LaunchAttention, LaunchJob, LaunchPreflight, LaunchResult, SettingKey,
};
use ol_platform_api::CancelToken;
use ol_store::Store;
use ol_vault::Secret;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::accounts;
use crate::journal::SwitchJournal;
use crate::launchers::{Adapter, Configs, Launchers, ServiceError, require_implemented};
pub use crate::sessions::account_ref;
use crate::sessions::{RemoteDirectory, StoreThread};
use crate::switch::{SwitchOptions, SwitchSink, run_switch};

const FINISHED_JOBS_KEPT: usize = 20;
// A preflight the dialog took this recently answers the start that follows it.
const PREFLIGHT_REUSE: Duration = Duration::from_secs(10);
// A job that goes straight to the game trusts a reused preflight only this young.
const PREFLIGHT_DIRECT: Duration = Duration::from_secs(2);
const LAUNCH_STEP: &str = "launch";

pub struct StartInput {
    pub account_id: String,
    // None switches without starting a game.
    pub game_id: Option<String>,
    // The user has said that the launcher, and any game running under it, may be closed.
    pub confirm_close: bool,
}

#[derive(Default)]
struct Board {
    current: Option<LaunchJob>,
    cancel: Option<CancelToken>,
    finished: VecDeque<LaunchJob>,
    closing: bool,
}

// The running job and the last finished ones, where every thread can read them.
#[derive(Clone, Default)]
pub struct JobBoard(Arc<Mutex<Board>>);

impl JobBoard {
    fn board(&self) -> MutexGuard<'_, Board> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn is_busy(&self) -> bool {
        self.board().current.is_some()
    }

    pub fn status(&self, job_id: &str) -> Result<LaunchJob, ServiceError> {
        let board = self.board();
        board
            .current
            .iter()
            .chain(&board.finished)
            .find(|job| job.id == job_id)
            .cloned()
            .ok_or_else(|| ServiceError::JobNotFound(job_id.to_owned()))
    }

    // Stops the running job: whatever wait it is in ends, and the pipeline decides what is put back. A finished job comes back as it is.
    pub fn cancel(&self, job_id: &str) -> Result<LaunchJob, ServiceError> {
        let job = self.status(job_id)?;
        let board = self.board();
        if board.current.as_ref().is_some_and(|job| job.id == job_id)
            && let Some(cancel) = &board.cancel
        {
            cancel.cancel();
        }
        Ok(job)
    }

    // For quitting: no job starts from now on, and the running one is stopped.
    pub fn close(&self) {
        let mut board = self.board();
        board.closing = true;
        if let Some(cancel) = &board.cancel {
            cancel.cancel();
        }
    }

    fn begin(&self, job: &LaunchJob, cancel: &CancelToken) {
        let mut board = self.board();
        board.current = Some(job.clone());
        board.cancel = Some(cancel.clone());
    }

    // A job that was accepted and could not be run after all: the next one is not refused for it.
    pub fn discard(&self, job_id: &str) {
        let mut board = self.board();
        if board.current.as_ref().is_some_and(|job| job.id == job_id) {
            board.current = None;
            board.cancel = None;
        }
    }

    fn update(&self, job: &LaunchJob) {
        self.board().current = Some(job.clone());
    }

    fn finish(&self, job: &LaunchJob) {
        let mut board = self.board();
        board.current = None;
        board.cancel = None;
        board.finished.push_front(job.clone());
        board.finished.truncate(FINISHED_JOBS_KEPT);
    }
}

// The preflight the dialog took last, kept for the start that follows it.
#[derive(Default)]
pub struct LastPreflight(Option<(String, OffsetDateTime, SwitchPreflight)>);

fn game_for(
    store: &Store,
    account: &Account,
    game_id: Option<&str>,
) -> Result<Option<Game>, ServiceError> {
    let Some(game_id) = game_id else {
        return Ok(None);
    };
    let game = store
        .game(game_id)?
        .ok_or_else(|| ServiceError::GameNotFound(game_id.to_owned()))?;
    if game.launcher != account.launcher {
        let launcher = game.launcher.as_str();
        return Err(ServiceError::Mismatch(format!(
            "{} is a {launcher} game; pick a {launcher} account",
            game.name
        )));
    }
    Ok(Some(game))
}

fn dry_run(store: &Store) -> Result<bool, ServiceError> {
    Ok(store.settings()?.get_bool(SettingKey::DryRun, false))
}

fn take_preflight(
    store: &Store,
    launchers: &Launchers,
    data_dir: &Path,
    account: &Account,
    game: Option<&Game>,
) -> Result<SwitchPreflight, ServiceError> {
    let configs = launchers.configs(&store.settings()?, data_dir);
    let directory = launchers.directory(store, account.launcher);
    let game = game.map(|game| GameRef {
        launcher_game_id: &game.launcher_game_id,
        name: &game.name,
    });
    launchers
        .adapter(account.launcher, &configs, &directory)?
        .preflight(&account_ref(account), game)
        .map_err(ServiceError::adapter(account.launcher))
}

// Everything the launch dialog needs before it asks the user to confirm. Changes nothing.
pub fn preflight(
    store: &Store,
    launchers: &Launchers,
    data_dir: &Path,
    last: &mut LastPreflight,
    account_id: &str,
    game_id: Option<&str>,
) -> Result<LaunchPreflight, ServiceError> {
    let account = accounts::require(store, account_id)?;
    let game = game_for(store, &account, game_id)?;
    let taken = take_preflight(store, launchers, data_dir, &account, game.as_ref())?;
    let needs_confirmation = taken.launcher_running && !taken.already_active;
    let answer = LaunchPreflight {
        plan: taken.plan.clone(),
        dry_run: dry_run(store)?,
        launcher_running: taken.launcher_running,
        running_game: taken.running_game.clone(),
        already_active: taken.already_active,
        needs_confirmation,
        close_message: needs_confirmation.then(|| taken.close_message.clone()),
        blocked: (!taken.plan.executable).then(|| {
            taken
                .plan
                .warnings
                .first()
                .cloned()
                .unwrap_or_else(|| "This switch cannot run.".to_owned())
        }),
    };
    last.0 = Some((account.id, launchers.platform.clock.now(), taken));
    Ok(answer)
}

// A job that was accepted, with everything its run needs and no database among it.
pub struct PreparedJob {
    job: LaunchJob,
    account: AccountRef,
    confirm_close: bool,
    password: Option<Ciphertext>,
    configs: Configs,
    data_dir: PathBuf,
    launchers: Launchers,
    cancel: CancelToken,
    board: JobBoard,
}

impl PreparedJob {
    pub fn job(&self) -> &LaunchJob {
        &self.job
    }
}

// Accepts a switch, with or without a game to start, or refuses it: while another job runs, while preview mode is on, and when the switch would close the launcher without the user's yes.
pub fn prepare(
    store: &Store,
    launchers: &Launchers,
    data_dir: &Path,
    board: &JobBoard,
    last: &mut LastPreflight,
    input: &StartInput,
) -> Result<PreparedJob, ServiceError> {
    if board.is_busy() || board.board().closing {
        return Err(ServiceError::Busy);
    }
    if dry_run(store)? {
        return Err(ServiceError::DryRun);
    }
    let account = accounts::require(store, &input.account_id)?;
    let game = game_for(store, &account, input.game_id.as_deref())?;
    require_implemented(account.launcher)?;
    let clock = launchers.platform.clock.as_ref();

    let mut taken = None;
    if !input.confirm_close {
        let reused = last
            .0
            .take()
            .filter(|(account_id, _, _)| *account_id == account.id)
            .map(|(_, at, preflight)| (clock.now() - at, preflight))
            .filter(|(age, _)| *age <= PREFLIGHT_REUSE);
        let stale_for_direct = |age: time::Duration, preflight: &SwitchPreflight| {
            game.is_some()
                && preflight.already_active
                && preflight.launcher_running
                && age > PREFLIGHT_DIRECT
        };
        let preflight = match reused {
            Some((age, preflight)) if !stale_for_direct(age, &preflight) => preflight,
            _ => take_preflight(store, launchers, data_dir, &account, game.as_ref())?,
        };
        if preflight.launcher_running && !preflight.already_active {
            return Err(ServiceError::adapter(account.launcher)(
                AdapterError::ConfirmationRequired {
                    message: preflight.close_message,
                    running_game: preflight.running_game,
                },
            ));
        }
        taken = Some(preflight);
    }

    // The launcher runs and is signed in as the account, so a switch would touch nothing: the job goes straight to the game. A plain switch still runs, since confirming the sign-in is its whole point.
    let direct = taken
        .as_ref()
        .filter(|_| game.is_some())
        .filter(|preflight| preflight.already_active && preflight.launcher_running);
    let mut steps: Vec<JobStep> = direct
        .map(|preflight| {
            preflight
                .plan
                .steps
                .iter()
                .map(|step| JobStep {
                    id: step.id,
                    title: step.title.clone(),
                    status: JobStepStatus::Skipped,
                    detail: step.detail.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    if let Some(game) = &game {
        steps.push(JobStep {
            id: LAUNCH_STEP,
            title: format!("Launch {}", game.name),
            status: JobStepStatus::Pending,
            detail: String::new(),
        });
    } else {
        steps.clear();
    }
    let job = LaunchJob {
        id: Uuid::new_v4().to_string(),
        state: JobState::Running,
        launcher: account.launcher,
        account: JobAccount {
            id: account.id.clone(),
            display_name: account.display_name.clone(),
            login: account.login.clone(),
        },
        game: game.map(|game| JobGame {
            id: game.id,
            name: game.name,
            launcher_game_id: game.launcher_game_id,
        }),
        steps,
        attention: None,
        direct: direct.is_some(),
        switched: false,
        hand_off: None,
        backup_id: None,
        restored: None,
        error: None,
        started_at: ol_store::iso_timestamp(clock.now()),
        finished_at: None,
    };
    // What can still fail is read before the job takes the board: a job that was begun and never run would refuse every later one.
    let password = if account.has_secret {
        store.account_password(&account.id)?
    } else {
        None
    };
    let configs = launchers.configs(&store.settings()?, data_dir);
    let cancel = CancelToken::new();
    board.begin(&job, &cancel);
    Ok(PreparedJob {
        job,
        account: account_ref(&account),
        confirm_close: input.confirm_close,
        password,
        configs,
        data_dir: data_dir.to_path_buf(),
        launchers: launchers.clone(),
        cancel,
        board: board.clone(),
    })
}

// What a job that has run leaves for the database.
pub struct FinishedJob {
    job: LaunchJob,
    board: JobBoard,
    touch_last_used: bool,
    result: LaunchResult,
}

struct JobSink<'a> {
    job: &'a mut LaunchJob,
    launch_step: Option<JobStep>,
    journal: &'a mut SwitchJournal,
    launchers: &'a Launchers,
    password: Option<&'a Ciphertext>,
    board: &'a JobBoard,
    changed: &'a dyn Fn(&LaunchJob),
}

impl JobSink<'_> {
    fn now(&self) -> String {
        ol_store::iso_timestamp(self.launchers.platform.clock.now())
    }

    fn publish(&self) {
        self.board.update(self.job);
        (self.changed)(self.job);
    }

    fn set_steps(&mut self, steps: &[JobStep]) {
        self.job.steps = steps
            .iter()
            .cloned()
            .chain(self.launch_step.clone())
            .collect();
        self.publish();
    }

    fn set_launch(&mut self, status: JobStepStatus) {
        if let Some(step) = &mut self.launch_step {
            step.status = status;
            if let Some(last) = self
                .job
                .steps
                .last_mut()
                .filter(|last| last.id == LAUNCH_STEP)
            {
                *last = step.clone();
            }
        }
    }
}

impl SwitchSink for JobSink<'_> {
    fn progress(&mut self, steps: &[JobStep]) {
        self.set_steps(steps);
        if let Some(running) = steps
            .iter()
            .find(|step| step.status == JobStepStatus::Running)
        {
            let now = self.now();
            self.journal.set_step(running.id, &now);
        }
    }

    // On disk before the pipeline's first write, so a crash from here on can name the snapshot to put back.
    fn backup(&mut self, backup_id: &str) {
        self.job.backup_id = Some(backup_id.to_owned());
        let now = self.now();
        self.journal.set_backup(backup_id, &now);
    }

    fn attention(&mut self, attention: Option<&LaunchAttention>) {
        self.job.attention = attention.cloned();
        self.publish();
    }

    fn password(&mut self) -> Result<Option<Secret>, String> {
        self.password
            .map(|sealed| self.launchers.vault.decrypt(sealed))
            .transpose()
            .map_err(|error| error.to_string())
    }
}

// Runs an accepted job to its end: the switch, then the game. `database` is where the switch asks about accounts and saved sign-ins. `changed` hears the job each time it changes, except the last time, which `settle` reports once the database has it.
pub fn run(
    prepared: PreparedJob,
    database: Arc<dyn StoreThread>,
    changed: &dyn Fn(&LaunchJob),
) -> FinishedJob {
    let PreparedJob {
        mut job,
        account,
        confirm_close,
        password,
        configs,
        data_dir,
        launchers,
        cancel,
        board,
    } = prepared;
    let launcher = job.launcher;
    let directory = RemoteDirectory {
        thread: database,
        vault: Arc::clone(&launchers.vault),
        clock: Arc::clone(&launchers.platform.clock),
        launcher,
    };
    let clock = launchers.platform.clock.as_ref();
    let now = || ol_store::iso_timestamp(clock.now());
    let mut journal = SwitchJournal::new(&data_dir);
    let launch_step = job
        .steps
        .last()
        .filter(|step| step.id == LAUNCH_STEP)
        .cloned();
    let game = job.game.clone();
    let direct = job.direct;
    let job_id = job.id.clone();
    let mut sink = JobSink {
        job: &mut job,
        launch_step,
        journal: &mut journal,
        launchers: &launchers,
        password: password.as_ref(),
        board: &board,
        changed,
    };
    let mut touch_last_used = false;
    let mut result = LaunchResult::Failed;

    let launch = |sink: &mut JobSink<'_>,
                  adapter: &Adapter<'_>,
                  game: &JobGame,
                  started_by_switch: bool|
     -> Result<(), ServiceError> {
        sink.set_launch(JobStepStatus::Running);
        sink.publish();
        let launched = adapter
            .launch_game(game, started_by_switch, &cancel)
            .map_err(ServiceError::adapter(launcher))?;
        if let Some(step) = &mut sink.launch_step {
            step.detail = format!("Sent {}", launched.command);
        }
        sink.set_launch(JobStepStatus::Done);
        sink.job.hand_off = launched.hand_off;
        Ok(())
    };

    let ran: Result<(), ServiceError> = (|| {
        let adapter = launchers.adapter(launcher, &configs, &directory)?;
        if direct && let Some(game) = &game {
            sink.job.switched = true;
            launch(&mut sink, &adapter, game, false)?;
            touch_last_used = true;
            result = LaunchResult::Ok;
            sink.job.state = JobState::Done;
            return Ok(());
        }
        let account_id = account.id.clone().unwrap_or_default();
        sink.journal.begin(&job_id, launcher, &account_id, &now());
        let backups = launchers.backups(&data_dir, &configs);
        let options = SwitchOptions {
            confirm_close,
            cancel: &cancel,
            game: game.as_ref().map(|game| GameRef {
                launcher_game_id: &game.launcher_game_id,
                name: &game.name,
            }),
        };
        let switched = match run_switch(
            &adapter,
            &launchers.platform,
            &backups,
            &account,
            &options,
            &mut sink,
        ) {
            Ok(switched) => switched,
            Err(refused) => {
                // A refusal comes before anything is touched. Past the snapshot a failure is unexpected, so the journal stays for the next start.
                if sink.job.backup_id.is_none() {
                    sink.journal.clear(Some(&job_id));
                }
                return Err(refused);
            }
        };
        // The switch has settled, so the journal goes before the game starts. A snapshot that could not be put back keeps it.
        if switched.restored == Some(false) {
            sink.journal.set_rollback_failed(&now());
        } else {
            sink.journal.clear(Some(&job_id));
        }
        sink.set_steps(&switched.steps);
        sink.job.attention = None;
        sink.job.backup_id = switched.backup_id;
        sink.job.restored = switched.restored;
        if !switched.ok {
            sink.job.error = Some(
                switched
                    .error
                    .unwrap_or_else(|| "The switch failed".to_owned()),
            );
            sink.set_launch(JobStepStatus::Skipped);
            sink.job.state = if switched.cancelled {
                result = LaunchResult::Cancelled;
                JobState::Cancelled
            } else if switched.needs_sign_in {
                JobState::NeedsSignIn
            } else {
                JobState::Failed
            };
            return Ok(());
        }
        sink.job.switched = true;
        if let Some(game) = &game {
            // A launcher the switch has just started takes a game only from `ready_at` on.
            let ready_in = switched
                .ready_at
                .map(|ready_at| ready_at - clock.now())
                .filter(|wait| wait.is_positive() && !cancel.is_cancelled());
            if let Some(wait) = ready_in {
                sink.set_launch(JobStepStatus::Running);
                sink.publish();
                clock.sleep(wait.unsigned_abs(), &cancel);
            }
            if cancel.is_cancelled() {
                sink.set_launch(JobStepStatus::Skipped);
                sink.job.error = Some(format!("Stopped before launching {}.", game.name));
                sink.job.state = JobState::Cancelled;
                result = LaunchResult::Cancelled;
                touch_last_used = true;
                return Ok(());
            }
            let started_by_switch =
                switched.started_with_game.as_deref() == Some(game.launcher_game_id.as_str());
            launch(&mut sink, &adapter, game, started_by_switch)?;
        }
        touch_last_used = true;
        result = LaunchResult::Ok;
        sink.job.state = JobState::Done;
        Ok(())
    })();

    if let Err(failed) = ran {
        sink.job.attention = None;
        sink.job.error = Some(failed.to_string());
        let was_running = sink
            .launch_step
            .as_ref()
            .is_some_and(|step| step.status == JobStepStatus::Running);
        sink.set_launch(if was_running {
            JobStepStatus::Failed
        } else {
            JobStepStatus::Skipped
        });
        sink.job.state = JobState::Failed;
    }
    job.finished_at = Some(now());
    FinishedJob {
        job,
        board,
        touch_last_used,
        result,
    }
}

// Writes what a finished job leaves in the database, then lets it be seen as finished: the account's last use, and the launch in the history when a game was named.
pub fn settle(store: &Store, finished: FinishedJob, now: &str, changed: &dyn Fn(&LaunchJob)) {
    let FinishedJob {
        job,
        board,
        touch_last_used,
        result,
    } = finished;
    if touch_last_used {
        let _ = store.touch_account(&job.account.id, now);
    }
    if let Some(game) = &job.game {
        let _ = store.record_launch(
            &Uuid::new_v4().to_string(),
            &game.id,
            &job.account.id,
            now,
            result,
        );
    }
    board.finish(&job);
    changed(&job);
}

// The same as `settle` when the database is gone: the job still ends for whoever watches it.
pub fn abandon(finished: FinishedJob, changed: &dyn Fn(&LaunchJob)) {
    finished.board.finish(&finished.job);
    changed(&finished.job);
}
