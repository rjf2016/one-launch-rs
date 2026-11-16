#![forbid(unsafe_code)]

// Runs a replay script of oracle/l2 against the backend on fake launchers and says what the app did, in the shape of an expectation: the job and platform events in order, and what was left on disk and in the fake registry.

pub mod check;
mod state;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use futures_lite::future::block_on;
use ol_api::launchers::{
    BattlenetMachine, BattlenetTiming, EpicMachine, EpicTiming, Launchers, NewAccount, RiotMachine,
    RiotTiming, Secret, SteamTiming,
};
use ol_api::{ApiError, Backend, StartLaunch};
use ol_model::{JobState, LaunchJob, LauncherId, SettingKey};
use ol_platform_api::Clock;
use ol_sim::{FakeLaunchers, ScenarioError};
use ol_vault::FakeVault;
use serde_json::{Map, Value, json};
use thiserror::Error;
use time::OffsetDateTime;

pub use state::normalise;

#[derive(Debug, Error)]
pub enum ReplayError {
    #[error(transparent)]
    Scenario(#[from] ScenarioError),
    #[error("the replay folders could not be read or written: {0}")]
    Io(#[from] std::io::Error),
    #[error("the database could not be read: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("the script: {0}")]
    Script(String),
    #[error("the start-up work failed: {0}")]
    Startup(ApiError),
    #[error("job {job} ended as {ended} without ever being {wanted}")]
    NeverReached {
        job: String,
        ended: &'static str,
        wanted: String,
    },
}

pub struct Setup<'a> {
    // The folder that holds the scenario folders, each with its scenario.json.
    pub scenarios: &'a Path,
    // The folder "@fixtures/<path>" names a file in.
    pub fixtures: &'a Path,
    // An empty folder for this run: the launchers and the data folder are made in it.
    pub scratch: &'a Path,
    pub version: &'a str,
    // The instant the run's clock starts at.
    pub clock: OffsetDateTime,
}

fn script_error(message: impl Into<String>) -> ReplayError {
    ReplayError::Script(message.into())
}

// A wait of the script for a job to be in some state.
#[derive(Clone)]
struct Until {
    job: String,
    state: Option<String>,
    step: Option<(String, String)>,
}

impl Until {
    fn holds(&self, job: &LaunchJob) -> bool {
        job.id == self.job
            && self
                .state
                .as_deref()
                .is_none_or(|state| job.state.as_str() == state)
            && self.step.as_ref().is_none_or(|(id, status)| {
                job.steps
                    .iter()
                    .any(|step| step.id == id && step.status.as_str() == status)
            })
    }
}

#[derive(Default)]
struct Watch {
    jobs: BTreeMap<String, LaunchJob>,
    until: Option<Until>,
    reached: bool,
    // The job thread stands still at its next wait until the script has acted.
    held: bool,
    // The app this gate belongs to has crashed: its job thread never moves again.
    abandoned: bool,
}

#[derive(Clone, Default)]
struct Gate(Arc<(Mutex<Watch>, Condvar)>);

impl Gate {
    fn watch(&self) -> MutexGuard<'_, Watch> {
        self.0
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn wait<'a>(&self, guard: MutexGuard<'a, Watch>) -> MutexGuard<'a, Watch> {
        self.0
            .1
            .wait(guard)
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    // A change of the job: a job that has ended can be what the script waits for.
    fn changed(&self, job: &LaunchJob) {
        let mut watch = self.watch();
        watch.jobs.insert(job.id.clone(), job.clone());
        if job.state != JobState::Running
            && watch
                .until
                .as_ref()
                .is_some_and(|until| until.job == job.id)
        {
            watch.reached = true;
            self.0.1.notify_all();
        }
    }

    // The job is about to wait: when it is in the state the script waits for, it stands still here until the script lets it go.
    fn before_sleep(&self) {
        let mut watch = self.watch();
        while watch.abandoned {
            watch = self.wait(watch);
        }
        let reached = match &watch.until {
            Some(until) => watch.jobs.values().any(|job| until.holds(job)),
            None => false,
        };
        if !reached {
            return;
        }
        watch.reached = true;
        watch.held = true;
        self.0.1.notify_all();
        while watch.held {
            watch = self.wait(watch);
        }
    }

    // Whatever thread waits at this gate next stays there, and so does one that stands there now.
    fn hold_for_good(&self) {
        let mut watch = self.watch();
        watch.until = None;
        watch.abandoned = true;
        watch.held = true;
    }

    fn release(&self) {
        let mut watch = self.watch();
        watch.until = None;
        watch.reached = false;
        watch.held = false;
        self.0.1.notify_all();
    }

    fn wait_for(&self, until: Until) -> Result<(), ReplayError> {
        let mut watch = self.watch();
        watch.until = Some(until.clone());
        watch.reached = watch
            .jobs
            .get(&until.job)
            .is_some_and(|job| job.state != JobState::Running);
        watch.held = false;
        self.0.1.notify_all();
        while !watch.reached {
            watch = self.wait(watch);
        }
        let job = watch.jobs.get(&until.job).cloned();
        match job {
            Some(job) if until.holds(&job) => Ok(()),
            Some(job) => Err(ReplayError::NeverReached {
                job: until.job.clone(),
                ended: job.state.as_str(),
                wanted: format!("{:?} {:?}", until.state, until.step),
            }),
            None => Err(script_error("a job that was never started is waited for")),
        }
    }
}

struct Running {
    backend: Backend,
    fake: FakeLaunchers,
}

pub struct Replay<'a> {
    setup: &'a Setup<'a>,
    launchers_dir: PathBuf,
    data_dir: PathBuf,
    running: Option<Running>,
    now: OffsetDateTime,
    gate: Gate,
    events: Vec<Value>,
    // The id of each job in the order they were started.
    jobs: Vec<String>,
    registry: Vec<Value>,
}

fn job_event(job: &LaunchJob) -> Value {
    let steps: Vec<Value> = job
        .steps
        .iter()
        .map(|step| json!([step.id, step.status.as_str(), step.title, step.detail]))
        .collect();
    let attention = job.attention.as_ref().map(|attention| {
        json!({
            "kind": "sign-in",
            "reason": attention.reason,
            "canCopyPassword": attention.can_copy_password,
            "until": attention.until,
            "title": attention.title,
            "message": attention.message,
            "steps": attention.steps,
        })
    });
    json!({
        "k": "job",
        "job": job.id,
        "state": job.state.as_str(),
        "steps": steps,
        "attention": attention,
        "switched": job.switched,
        "handOff": job.hand_off,
        "backupId": job.backup_id,
        "restored": job.restored,
        "error": job.error,
    })
}

impl<'a> Replay<'a> {
    // Lays the script's scenario down in the scratch folder and starts the app on it.
    pub fn begin(setup: &'a Setup<'a>, script: &Value) -> Result<Self, ReplayError> {
        let fake = script["fake"]
            .as_str()
            .ok_or_else(|| script_error("no scenario is named"))?;
        let launchers_dir = setup.scratch.join("launchers");
        let data_dir = setup.scratch.join("data");
        ol_sim::lay_down(&setup.scenarios.join(fake), setup.fixtures, &launchers_dir)?;
        if let Some(knobs) = script.get("knobs") {
            ol_sim::write_knobs(&launchers_dir, knobs)?;
        }
        let mut replay = Self {
            setup,
            launchers_dir,
            data_dir,
            running: None,
            now: setup.clock,
            gate: Gate::default(),
            events: Vec::new(),
            jobs: Vec::new(),
            registry: Vec::new(),
        };
        replay.start()?;
        Ok(replay)
    }

    // A start is over once the app has done its start-up work: the seed of a fresh folder, the library scan and the copy of the database.
    fn start(&mut self) -> Result<(), ReplayError> {
        let fake = ol_sim::materialize(&self.launchers_dir, &self.data_dir, self.now)?;
        let mut launchers = Launchers::on(fake.platform.clone(), Arc::new(FakeVault));
        launchers.steam_timing = SteamTiming::with(&fake.steam_timing);
        launchers
            .steam_default_dir
            .clone_from(&fake.steam_default_dir);
        launchers.epic = EpicMachine {
            timing: EpicTiming::with(&fake.epic_timing),
            default_dir: fake.epic_default_dir.clone(),
            local_app_data: fake.local_app_data.clone(),
            program_data: fake.program_data.clone(),
        };
        launchers.battlenet = BattlenetMachine {
            timing: BattlenetTiming::with(&fake.battlenet_timing),
            default_dir: fake.battlenet_default_dir.clone(),
            app_data: fake.app_data.clone(),
            local_app_data: fake.local_app_data.clone(),
            program_data: fake.program_data.clone(),
        };
        launchers.riot = RiotMachine::at(
            RiotTiming::with(&fake.riot_timing),
            fake.riot_default_dir.clone(),
            fake.local_app_data.clone(),
            fake.program_data.clone(),
        )
        .with_local_launch(fake.riot_launch.clone());

        let (log, gate) = (fake.log.clone(), self.gate.clone());
        let observer = move |job: &LaunchJob| {
            log.push(job_event(job));
            gate.changed(job);
        };
        let gate = self.gate.clone();
        fake.clock.before_sleep(move || gate.before_sleep());
        let backend = Backend::start_with(
            self.data_dir.clone(),
            self.setup.version,
            launchers,
            Some(Arc::new(observer)),
        );

        let started = (|| {
            if block_on(backend.accounts())?.is_empty() {
                for account in &fake.seed.accounts {
                    block_on(backend.create_account(NewAccount {
                        launcher: account.launcher,
                        display_name: account.display_name.clone(),
                        login: account.login.clone(),
                        launcher_uid: account.launcher_uid.clone(),
                        password: account.password.clone().map(Secret::new),
                        tags: Vec::new(),
                        notes: String::new(),
                    }))?;
                }
                let accounts = block_on(backend.accounts())?;
                for session in &fake.seed.sessions {
                    let owner = accounts.iter().find(|account| {
                        account.launcher == session.launcher && account.login == session.login
                    });
                    if let Some(owner) = owner {
                        block_on(backend.seed_session(
                            &owner.id,
                            Some(session.launcher_uid.clone()),
                            Secret::new(session.blob.clone()),
                        ))?;
                    }
                }
                for (key, value) in &fake.seed.settings {
                    if let Some(key) = SettingKey::parse(key) {
                        block_on(backend.set_setting(key, value.clone()))?;
                    }
                }
            }
            block_on(backend.rescan())?;
            block_on(backend.back_up_database())
        })();
        self.running = Some(Running { backend, fake });
        started.map_err(ReplayError::Startup)
    }

    fn backend(&self) -> Result<&Backend, ReplayError> {
        self.running
            .as_ref()
            .map(|running| &running.backend)
            .ok_or_else(|| script_error("the app is not running"))
    }

    // The process ends with none of its quit path run. A thread cannot be ended as a process can, so the switch is left standing at the wait it is in, for good: it writes nothing more, the journal and the files stay as they are, and the fake machine's memory goes with it.
    fn crash(&mut self) {
        if let Some(running) = self.running.take() {
            self.events.extend(running.fake.log.snapshot());
            self.registry = state::registry(&running.fake.registry);
            self.now = running.fake.clock.now();
            self.gate.hold_for_good();
            self.gate = Gate::default();
            std::mem::forget(running);
        }
    }

    // The app quits: a running switch is stopped and waited for, and the database is closed.
    fn shut_down(&mut self) {
        self.gate.release();
        if let Some(running) = self.running.take() {
            running.backend.shut_down();
            self.events.extend(running.fake.log.snapshot());
            self.registry = state::registry(&running.fake.registry);
            self.now = running.fake.clock.now();
        }
    }

    fn account_id(&self, alias: &str) -> Result<String, ReplayError> {
        let unknown = || script_error(format!("no account {alias}"));
        let mut parts = alias
            .strip_prefix("@acct:")
            .ok_or_else(unknown)?
            .splitn(2, ':');
        let launcher = parts
            .next()
            .and_then(LauncherId::parse)
            .ok_or_else(unknown)?;
        let login = parts.next().ok_or_else(unknown)?;
        block_on(self.backend()?.accounts())
            .map_err(ReplayError::Startup)?
            .into_iter()
            .find(|account| account.launcher == launcher && account.login == login)
            .map(|account| account.id)
            .ok_or_else(unknown)
    }

    fn game_id(&self, alias: &str) -> Result<String, ReplayError> {
        let unknown = || script_error(format!("no game {alias}"));
        let mut parts = alias
            .strip_prefix("@game:")
            .ok_or_else(unknown)?
            .splitn(2, ':');
        let launcher = parts
            .next()
            .and_then(LauncherId::parse)
            .ok_or_else(unknown)?;
        let launcher_game_id = parts.next().ok_or_else(unknown)?;
        block_on(self.backend()?.library())
            .map_err(ReplayError::Startup)?
            .games
            .into_iter()
            .map(|entry| entry.game)
            .find(|game| game.launcher == launcher && game.launcher_game_id == launcher_game_id)
            .map(|game| game.id)
            .ok_or_else(unknown)
    }

    fn job_id(&self, alias: &str) -> Result<String, ReplayError> {
        alias
            .strip_prefix("@job:")
            .and_then(|number| number.parse::<usize>().ok())
            .and_then(|number| self.jobs.get(number.checked_sub(1)?))
            .cloned()
            .ok_or_else(|| script_error(format!("no job {alias}")))
    }

    fn backup_id(&self, alias: &str) -> Result<String, ReplayError> {
        let unknown = || script_error(format!("no snapshot {alias}"));
        let mut parts = alias
            .strip_prefix("@backup:")
            .ok_or_else(unknown)?
            .splitn(2, ':');
        let launcher = parts
            .next()
            .and_then(LauncherId::parse)
            .ok_or_else(unknown)?;
        let number: usize = parts
            .next()
            .and_then(|n| n.parse().ok())
            .ok_or_else(unknown)?;
        state::snapshot_ids(&self.data_dir, launcher)
            .into_iter()
            .nth(number.checked_sub(1).ok_or_else(unknown)?)
            .ok_or_else(unknown)
    }

    // One call of the app. What it answers is not recorded, and a refusal is an answer like any other.
    fn invoke(&mut self, channel: &str, request: &Value) -> Result<(), ReplayError> {
        let text = |name: &str| request[name].as_str();
        let launcher = || {
            text("launcher")
                .and_then(LauncherId::parse)
                .ok_or_else(|| script_error(format!("{channel} names no launcher")))
        };
        let account = |replay: &Self| {
            replay.account_id(text("accountId").ok_or_else(|| script_error("no accountId"))?)
        };
        let game = |replay: &Self| {
            text("gameId")
                .map(|alias| replay.game_id(alias))
                .transpose()
        };
        let backend = self.backend()?.clone();
        match channel {
            "accounts:list" => drop(block_on(backend.accounts())),
            // Reads the replay has nothing to check, and what this build does not have yet: the copy to the clipboard, and the status of the launchers and of the artwork, which are reads too.
            "settings:get" | "settings:getAll" | "accounts:copy" | "launchers:status"
            | "artwork:status" => {}
            "settings:set" => {
                let key = text("key")
                    .and_then(SettingKey::parse)
                    .ok_or_else(|| script_error("settings:set names no setting"))?;
                let value = text("value").unwrap_or_default().to_owned();
                drop(block_on(backend.set_setting(key, value)));
            }
            "library:list" => drop(block_on(backend.library())),
            "library:rescan" => drop(block_on(backend.rescan())),
            "launchers:games" => drop(block_on(backend.installed_games(launcher()?))),
            "launchers:rememberedAccounts" => {
                drop(block_on(backend.remembered_accounts(launcher()?)));
            }
            "launchers:importAccounts" => {
                let keys = request["launcherUids"]
                    .as_array()
                    .map(|keys| {
                        keys.iter()
                            .filter_map(|key| key.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();
                drop(block_on(backend.import_accounts(launcher()?, keys)));
            }
            "launchers:switchPlan" => drop(block_on(backend.switch_plan(&account(self)?))),
            "sessions:get" => drop(block_on(backend.session(&account(self)?))),
            "launch:preflight" => {
                let (account, game) = (account(self)?, game(self)?);
                drop(block_on(
                    backend.launch_preflight(&account, game.as_deref()),
                ));
            }
            "launch:start" => {
                let start = StartLaunch {
                    account_id: account(self)?,
                    game_id: game(self)?,
                    confirm_close: request["confirmClose"] == true,
                };
                if let Ok(job) = block_on(backend.start_launch(start)) {
                    self.jobs.push(job.id);
                }
            }
            "launch:status" | "launch:cancel" => {
                let job = self.job_id(text("jobId").ok_or_else(|| script_error("no jobId"))?)?;
                if channel == "launch:cancel" {
                    drop(backend.cancel_launch(&job));
                } else {
                    drop(backend.launch_status(&job));
                }
            }
            "launch:recent" => {
                let limit = request["limit"]
                    .as_u64()
                    .and_then(|limit| u32::try_from(limit).ok())
                    .unwrap_or(0);
                drop(block_on(backend.recent_launches(limit)));
            }
            "backups:list" => drop(block_on(backend.backups(launcher()?))),
            "backups:restore" => {
                let id = self.backup_id(text("id").ok_or_else(|| script_error("no id"))?)?;
                drop(block_on(backend.restore_backup(launcher()?, &id)));
            }
            "recovery:get" => drop(block_on(backend.recovery())),
            "recovery:restore" => drop(block_on(backend.restore_recovery())),
            "recovery:dismiss" => drop(block_on(backend.dismiss_recovery())),
            other => return Err(script_error(format!("no such call here: {other}"))),
        }
        Ok(())
    }

    fn step(&mut self, step: &Value) -> Result<(), ReplayError> {
        match step["op"].as_str() {
            Some("invoke") => {
                let channel = step["channel"]
                    .as_str()
                    .ok_or_else(|| script_error("an invoke without a channel"))?;
                let request = step.get("req").unwrap_or(&Value::Null);
                self.invoke(channel, request)
            }
            Some("awaitJob") => {
                let job = self.job_id(step["job"].as_str().unwrap_or_default())?;
                let until = &step["until"];
                let text = |name: &str| until[name].as_str().map(str::to_owned);
                self.gate.wait_for(Until {
                    job,
                    state: text("state"),
                    step: text("step").zip(text("status")),
                })
            }
            Some("shutdown") => {
                self.shut_down();
                Ok(())
            }
            Some("crash") => {
                self.crash();
                Ok(())
            }
            Some("restart") => {
                self.shut_down();
                if let Some(knobs) = step.get("knobs") {
                    ol_sim::write_knobs(&self.launchers_dir, knobs)?;
                }
                self.start()
            }
            Some("snapshot") => {
                self.shut_down();
                Ok(())
            }
            Some(other) => Err(script_error(format!(
                "the step {other} cannot be replayed here"
            ))),
            None => Err(script_error("a step without an op")),
        }
    }

    // Runs every step and answers with what the app did: `events` and `final`, with ids as aliases, the two folders as {launchers} and {data}, and times as the normalisation rules of the script say.
    pub fn run(setup: &'a Setup<'a>, script: &Value, rules: &Value) -> Result<Value, ReplayError> {
        let mut replay = Self::begin(setup, script)?;
        let steps = script["steps"]
            .as_array()
            .ok_or_else(|| script_error("no steps"))?;
        let ran = steps.iter().try_for_each(|step| replay.step(step));
        replay.shut_down();
        ran?;
        replay.report(rules)
    }

    fn report(&self, rules: &Value) -> Result<Value, ReplayError> {
        let mut events: Vec<Value> = Vec::new();
        for event in &self.events {
            // A state reported twice in a row is one event.
            if event["k"] == "job" && events.iter().rev().find(|e| e["k"] == "job") == Some(event) {
                continue;
            }
            events.push(event.clone());
        }
        let events: Vec<Value> = events
            .into_iter()
            .enumerate()
            .map(|(seq, event)| {
                let mut numbered = Map::new();
                numbered.insert("seq".to_owned(), seq.into());
                numbered.extend(event.as_object().cloned().unwrap_or_default());
                Value::Object(numbered)
            })
            .collect();
        let read = state::read(
            &self.data_dir,
            &self.launchers_dir,
            &FakeVault,
            &self.registry,
        )?;
        let aliases = state::Aliases {
            launchers_dir: &self.launchers_dir,
            data_dir: &self.data_dir,
            jobs: &self.jobs,
            ids: read.ids,
        };
        let mut report = json!({ "events": events, "final": read.state });
        normalise(&mut report, &aliases, rules);
        Ok(report)
    }
}
