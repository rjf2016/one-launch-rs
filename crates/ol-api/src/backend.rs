use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use async_channel::{Receiver, Sender, WeakSender};
use ol_model::{
    Account, BackupSnapshot, InstalledGame, LaunchJob, LaunchPreflight, LauncherId,
    LibraryScanResult, LibrarySort, RecentLaunch, RememberedAccount, RestoreResult, SettingKey,
    SwitchPlan, SwitchRecovery,
};
use ol_services::accounts::{self, AccountError, NewAccount};
use ol_services::backup::BackupService;
use ol_services::data_folder::{self, OpenError};
use ol_services::journal::Recovery;
use ol_services::launch::{self, FinishedJob, JobBoard, LastPreflight, StartInput};
use ol_services::launchers::{self, ImportResult, Launchers, ServiceError, require_implemented};
use ol_services::library::{self, LibraryGame};
use ol_services::switch;
use ol_store::{Store, StoreError};
use thiserror::Error;

const RECENT_LAUNCHES_MOST: u32 = 50;

// What the Electron app sends across to its page for a failure: a code to act on and a sentence to show.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct ApiError {
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    fn stopped() -> Self {
        Self {
            code: "INTERNAL",
            message: "One Launch is no longer reading its data. Restart One Launch.".to_owned(),
        }
    }
}

impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

impl From<OpenError> for ApiError {
    fn from(error: OpenError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

impl From<ServiceError> for ApiError {
    fn from(error: ServiceError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

impl From<AccountError> for ApiError {
    fn from(error: AccountError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

pub struct Library {
    pub games: Vec<LibraryGame>,
    pub sort: LibrarySort,
}

// A switch to start: the account to sign the launcher in as, and the game to start after it, if any.
pub struct StartLaunch {
    pub account_id: String,
    pub game_id: Option<String>,
    // The user has said that the launcher, and any game running under it, may be closed.
    pub confirm_close: bool,
}

// Hears the launch job each time it changes, on the thread that changed it.
pub type JobObserver = Arc<dyn Fn(&LaunchJob) + Send + Sync>;

type Reply<T> = Sender<Result<T, ApiError>>;

enum Command {
    Library(Reply<Library>),
    Accounts(Reply<Vec<Account>>),
    SetLibrarySort(LibrarySort, Reply<()>),
    BackUpDatabase(Reply<()>),
    SetSetting(SettingKey, String, Reply<()>),
    CreateAccount(Box<NewAccount>, Reply<Account>),
    Rescan(Reply<Vec<LibraryScanResult>>),
    InstalledGames(LauncherId, Reply<Vec<InstalledGame>>),
    RememberedAccounts(LauncherId, Reply<Vec<RememberedAccount>>),
    ImportAccounts(LauncherId, Vec<String>, Reply<ImportResult>),
    SwitchPlan(String, Reply<SwitchPlan>),
    LaunchPreflight(String, Option<String>, Reply<LaunchPreflight>),
    StartLaunch(StartLaunch, Reply<LaunchJob>),
    RecentLaunches(u32, Reply<Vec<RecentLaunch>>),
    Backups(LauncherId, Reply<Vec<BackupSnapshot>>),
    RestoreBackup(LauncherId, String, Reply<RestoreResult>),
    Recovery(Reply<Option<SwitchRecovery>>),
    RestoreRecovery(Reply<RestoreResult>),
    DismissRecovery(Reply<()>),
    Settle(Box<FinishedJob>),
}

type Thread = Arc<Mutex<Option<JoinHandle<()>>>>;

// The database lives on one thread of its own. A handle only sends that thread a request and waits for the answer, so it can be held and cloned anywhere. A switch waits for a launcher on a second thread, which never touches the database.
#[derive(Clone)]
pub struct Backend {
    commands: Sender<Command>,
    thread: Thread,
    job_thread: Thread,
    jobs: JobBoard,
    has_launchers: bool,
}

fn take(thread: &Thread) -> Option<JoinHandle<()>> {
    match thread.lock() {
        Ok(mut thread) => thread.take(),
        Err(poisoned) => poisoned.into_inner().take(),
    }
}

impl Backend {
    // Returns at once: the folder is made and the database opened on the new thread, and a failure there is the answer to every request. Without launchers nothing can be scanned, switched or started.
    pub fn start(data_dir: PathBuf, version: &str) -> Self {
        Self::start_on(data_dir, version, None, None)
    }

    // As `start`, on the machine `launchers` describes. `on_job` hears every change of a launch job.
    pub fn start_with(
        data_dir: PathBuf,
        version: &str,
        launchers: Launchers,
        on_job: Option<JobObserver>,
    ) -> Self {
        Self::start_on(data_dir, version, Some(launchers), on_job)
    }

    fn start_on(
        data_dir: PathBuf,
        version: &str,
        launchers: Option<Launchers>,
        on_job: Option<JobObserver>,
    ) -> Self {
        let (commands, requests) = async_channel::unbounded();
        let version = version.to_owned();
        let jobs = JobBoard::default();
        let job_thread: Thread = Arc::default();
        let has_launchers = launchers.is_some();
        let serving = Serving {
            data_dir,
            launchers,
            jobs: jobs.clone(),
            on_job: on_job.unwrap_or_else(|| Arc::new(|_| {})),
            commands: commands.downgrade(),
            job_thread: Arc::clone(&job_thread),
            last_preflight: LastPreflight::default(),
            recovery: Recovery::default(),
        };
        let thread = thread::Builder::new()
            .name("backend".to_owned())
            .spawn(move || serving.serve(&version, &requests))
            .ok();
        Self {
            commands,
            thread: Arc::new(Mutex::new(thread)),
            job_thread,
            jobs,
            has_launchers,
        }
    }

    // Whether this build can scan, switch and start the launcher: it has an adapter for it and a machine to run it on.
    pub fn can_reach(&self, launcher: LauncherId) -> bool {
        self.has_launchers && Launchers::IMPLEMENTED.contains(&launcher)
    }

    pub async fn library(&self) -> Result<Library, ApiError> {
        self.ask(Command::Library).await
    }

    pub async fn accounts(&self) -> Result<Vec<Account>, ApiError> {
        self.ask(Command::Accounts).await
    }

    pub async fn set_library_sort(&self, sort: LibrarySort) -> Result<(), ApiError> {
        self.ask(|reply| Command::SetLibrarySort(sort, reply)).await
    }

    pub async fn back_up_database(&self) -> Result<(), ApiError> {
        self.ask(Command::BackUpDatabase).await
    }

    // Stores a setting as it is given: the rules for what each setting may hold come with the Settings screen.
    pub async fn set_setting(&self, key: SettingKey, value: String) -> Result<(), ApiError> {
        self.ask(|reply| Command::SetSetting(key, value, reply))
            .await
    }

    // Makes an account. Its password, when it has one, is encrypted before the database is touched.
    pub async fn create_account(&self, account: NewAccount) -> Result<Account, ApiError> {
        self.ask(|reply| Command::CreateAccount(Box::new(account), reply))
            .await
    }

    // Reads every launcher again and brings the Library in line with what is installed.
    pub async fn rescan(&self) -> Result<Vec<LibraryScanResult>, ApiError> {
        self.ask(Command::Rescan).await
    }

    // The games a launcher has installed, read from its own files and not from the Library.
    pub async fn installed_games(
        &self,
        launcher: LauncherId,
    ) -> Result<Vec<InstalledGame>, ApiError> {
        self.ask(|reply| Command::InstalledGames(launcher, reply))
            .await
    }

    // The accounts a launcher itself remembers, each with the One Launch account that goes with it.
    pub async fn remembered_accounts(
        &self,
        launcher: LauncherId,
    ) -> Result<Vec<RememberedAccount>, ApiError> {
        self.ask(|reply| Command::RememberedAccounts(launcher, reply))
            .await
    }

    // Makes accounts for remembered accounts, each named by the launcher's id for it or, where the launcher keeps none, by its login.
    pub async fn import_accounts(
        &self,
        launcher: LauncherId,
        keys: Vec<String>,
    ) -> Result<ImportResult, ApiError> {
        self.ask(|reply| Command::ImportAccounts(launcher, keys, reply))
            .await
    }

    // What switching to an account would touch, without touching it.
    pub async fn switch_plan(&self, account_id: &str) -> Result<SwitchPlan, ApiError> {
        let account_id = account_id.to_owned();
        self.ask(|reply| Command::SwitchPlan(account_id, reply))
            .await
    }

    // What the launch dialog needs before it asks the user to confirm.
    pub async fn launch_preflight(
        &self,
        account_id: &str,
        game_id: Option<&str>,
    ) -> Result<LaunchPreflight, ApiError> {
        let (account_id, game_id) = (account_id.to_owned(), game_id.map(str::to_owned));
        self.ask(|reply| Command::LaunchPreflight(account_id, game_id, reply))
            .await
    }

    // Starts a switch, and a game after it when one is named. Answers with the job as it begins; the work goes on, and the observer and `launch_status` say how it fares.
    pub async fn start_launch(&self, start: StartLaunch) -> Result<LaunchJob, ApiError> {
        self.ask(|reply| Command::StartLaunch(start, reply)).await
    }

    // The running job or one of the last twenty that finished.
    pub fn launch_status(&self, job_id: &str) -> Result<LaunchJob, ApiError> {
        Ok(self.jobs.status(job_id)?)
    }

    // Stops the running job. Before the launcher was started again, what was written is put back; after that the writes stand and the launcher finishes signing in.
    pub fn cancel_launch(&self, job_id: &str) -> Result<LaunchJob, ApiError> {
        Ok(self.jobs.cancel(job_id)?)
    }

    // Game and account pairs that were launched before, newest first. Anything but 1 to 50 asks for ten.
    pub async fn recent_launches(&self, limit: u32) -> Result<Vec<RecentLaunch>, ApiError> {
        let limit = match limit {
            0 => 10,
            limit => limit.min(RECENT_LAUNCHES_MOST),
        };
        self.ask(|reply| Command::RecentLaunches(limit, reply))
            .await
    }

    pub async fn backups(&self, launcher: LauncherId) -> Result<Vec<BackupSnapshot>, ApiError> {
        self.ask(|reply| Command::Backups(launcher, reply)).await
    }

    // Puts a snapshot of a launcher's files back. Refused while a switch runs and while the launcher runs.
    pub async fn restore_backup(
        &self,
        launcher: LauncherId,
        id: &str,
    ) -> Result<RestoreResult, ApiError> {
        let id = id.to_owned();
        self.ask(|reply| Command::RestoreBackup(launcher, id, reply))
            .await
    }

    // The switch the last run did not finish, if it left one that changed something.
    pub async fn recovery(&self) -> Result<Option<SwitchRecovery>, ApiError> {
        self.ask(Command::Recovery).await
    }

    pub async fn restore_recovery(&self) -> Result<RestoreResult, ApiError> {
        self.ask(Command::RestoreRecovery).await
    }

    pub async fn dismiss_recovery(&self) -> Result<(), ApiError> {
        self.ask(Command::DismissRecovery).await
    }

    // Stops a running switch and waits for it, then waits for the request in hand and for the database to close, which folds its journal back into the one file.
    pub fn shut_down(&self) {
        self.jobs.close();
        if let Some(job) = take(&self.job_thread) {
            let _ = job.join();
        }
        self.commands.close();
        if let Some(thread) = take(&self.thread) {
            let _ = thread.join();
        }
    }

    async fn ask<T>(&self, command: impl FnOnce(Reply<T>) -> Command) -> Result<T, ApiError> {
        let (reply, answer) = async_channel::bounded(1);
        self.commands
            .send(command(reply))
            .await
            .map_err(|_| ApiError::stopped())?;
        answer
            .recv()
            .await
            .unwrap_or_else(|_| Err(ApiError::stopped()))
    }
}

struct Serving {
    data_dir: PathBuf,
    launchers: Option<Launchers>,
    jobs: JobBoard,
    on_job: JobObserver,
    commands: WeakSender<Command>,
    job_thread: Thread,
    last_preflight: LastPreflight,
    recovery: Recovery,
}

impl Serving {
    fn serve(mut self, version: &str, requests: &Receiver<Command>) {
        let opened = data_folder::open(&self.data_dir, version)
            .map(|(store, _)| store)
            .map_err(ApiError::from);
        if opened.is_ok() {
            self.recovery = Recovery::load(&self.data_dir);
        }
        while let Ok(command) = requests.recv_blocking() {
            let store = opened.as_ref().map_err(ApiError::clone);
            self.handle(command, store);
        }
    }

    fn handle(&mut self, command: Command, store: Result<&Store, ApiError>) {
        match command {
            Command::Library(reply) => answer(
                &reply,
                store.and_then(|store| read_library(store, &self.data_dir)),
            ),
            Command::Accounts(reply) => {
                answer(&reply, store.and_then(|store| Ok(store.accounts()?)));
            }
            Command::SetLibrarySort(sort, reply) => answer(
                &reply,
                store.and_then(|store| Ok(store.set_library_sort(sort)?)),
            ),
            Command::BackUpDatabase(reply) => answer(
                &reply,
                store.and_then(|store| {
                    data_folder::back_up(store, &self.data_dir)?;
                    Ok(())
                }),
            ),
            Command::SetSetting(key, value, reply) => answer(
                &reply,
                store.and_then(|store| Ok(store.set_setting(key, &value)?)),
            ),
            Command::CreateAccount(account, reply) => {
                answer(
                    &reply,
                    store.and_then(|store| self.create_account(store, &account)),
                );
            }
            Command::Rescan(reply) => answer(&reply, store.and_then(|store| self.rescan(store))),
            Command::InstalledGames(launcher, reply) => answer(
                &reply,
                store.and_then(|store| {
                    let (launchers, config) = self.steam(store, launcher)?;
                    Ok(launchers::installed_games(&launchers.steam(&config))?)
                }),
            ),
            Command::RememberedAccounts(launcher, reply) => answer(
                &reply,
                store.and_then(|store| {
                    let (launchers, config) = self.steam(store, launcher)?;
                    Ok(launchers::remembered_accounts(
                        store,
                        &launchers.steam(&config),
                    )?)
                }),
            ),
            Command::ImportAccounts(launcher, keys, reply) => answer(
                &reply,
                store.and_then(|store| {
                    let (launchers, config) = self.steam(store, launcher)?;
                    Ok(launchers::import_accounts(
                        store,
                        launchers.vault.as_ref(),
                        &launchers.steam(&config),
                        &keys,
                        &self.now(launchers),
                    )?)
                }),
            ),
            Command::SwitchPlan(account_id, reply) => answer(
                &reply,
                store.and_then(|store| {
                    let account = accounts::require(store, &account_id)?;
                    let (launchers, config) = self.steam(store, account.launcher)?;
                    Ok(launchers
                        .steam(&config)
                        .plan_switch(&launch::account_ref(&account))
                        .map_err(ServiceError::adapter(account.launcher))?)
                }),
            ),
            Command::LaunchPreflight(account_id, game_id, reply) => answer(
                &reply,
                store.and_then(|store| {
                    let launchers = self.launchers.as_ref().ok_or_else(no_launchers)?;
                    Ok(launch::preflight(
                        store,
                        launchers,
                        &self.data_dir,
                        &mut self.last_preflight,
                        &account_id,
                        game_id.as_deref(),
                    )?)
                }),
            ),
            Command::StartLaunch(start, reply) => {
                answer(
                    &reply,
                    store.and_then(|store| self.start_launch(store, start)),
                );
            }
            Command::RecentLaunches(limit, reply) => answer(
                &reply,
                store.and_then(|store| Ok(store.recent_launches(limit)?)),
            ),
            Command::Backups(launcher, reply) => answer(
                &reply,
                store.and_then(|store| {
                    let (launchers, config) = self.steam(store, launcher)?;
                    let backups = self.backup_service(launchers, &config);
                    Ok(backups
                        .list(launcher)
                        .map_err(ServiceError::backup(launcher))?)
                }),
            ),
            Command::RestoreBackup(launcher, id, reply) => {
                let restored =
                    store.and_then(|store| self.restore_backup(store, launcher, &id, true));
                if restored.is_ok() {
                    self.recovery
                        .restored_from_backups(&self.data_dir, launcher);
                }
                answer(&reply, restored);
            }
            Command::Recovery(reply) => {
                answer(&reply, store.and_then(|store| self.pending_recovery(store)));
            }
            Command::RestoreRecovery(reply) => {
                let restored = store.and_then(|store| {
                    if self.jobs.is_busy() {
                        return Err(ServiceError::Busy.into());
                    }
                    let entry = self
                        .recovery
                        .pending()
                        .ok_or(ServiceError::NothingToRecover)?;
                    let (launcher, id) =
                        (entry.launcher, entry.backup_id.clone().unwrap_or_default());
                    self.restore_backup(store, launcher, &id, false)
                });
                if restored.is_ok() {
                    self.recovery.settle(&self.data_dir);
                }
                answer(&reply, restored);
            }
            Command::DismissRecovery(reply) => {
                self.recovery.settle(&self.data_dir);
                answer(&reply, Ok(()));
            }
            Command::Settle(finished) => {
                let on_job = Arc::clone(&self.on_job);
                match (store, &self.launchers) {
                    (Ok(store), Some(launchers)) => {
                        launch::settle(store, *finished, &self.now(launchers), &*on_job);
                    }
                    _ => launch::abandon(*finished, &*on_job),
                }
            }
        }
    }

    fn now(&self, launchers: &Launchers) -> String {
        ol_store::iso_timestamp(launchers.platform.clock.now())
    }

    // The launchers of this build with Steam's settings as they are now. Another launcher has no adapter yet.
    fn steam(
        &self,
        store: &Store,
        launcher: LauncherId,
    ) -> Result<(&Launchers, ol_adapters::steam::SteamConfig), ApiError> {
        require_implemented(launcher)?;
        let launchers = self.launchers.as_ref().ok_or_else(no_launchers)?;
        let config = launchers.steam_config(&store.settings()?, &self.data_dir);
        Ok((launchers, config))
    }

    fn backup_service<'a>(
        &self,
        launchers: &'a Launchers,
        config: &ol_adapters::steam::SteamConfig,
    ) -> BackupService<'a> {
        BackupService::new(
            &self.data_dir,
            config.backups_to_keep,
            launchers.platform.files.as_ref(),
            launchers.platform.clock.as_ref(),
        )
    }

    fn create_account(&self, store: &Store, account: &NewAccount) -> Result<Account, ApiError> {
        let launchers = self.launchers.as_ref().ok_or_else(no_launchers)?;
        Ok(accounts::create(
            store,
            launchers.vault.as_ref(),
            account,
            &self.now(launchers),
        )?)
    }

    fn rescan(&self, store: &Store) -> Result<Vec<LibraryScanResult>, ApiError> {
        let Some(launchers) = &self.launchers else {
            return Ok(Vec::new());
        };
        let config = launchers.steam_config(&store.settings()?, &self.data_dir);
        Ok(launchers::scan_library(
            store,
            &launchers.steam(&config),
            &self.now(launchers),
        ))
    }

    fn start_launch(&mut self, store: &Store, start: StartLaunch) -> Result<LaunchJob, ApiError> {
        let launchers = self.launchers.as_ref().ok_or_else(no_launchers)?;
        let prepared = launch::prepare(
            store,
            launchers,
            &self.data_dir,
            &self.jobs,
            &mut self.last_preflight,
            &StartInput {
                account_id: start.account_id,
                game_id: start.game_id,
                confirm_close: start.confirm_close,
            },
        )?;
        let job = prepared.job().clone();
        let on_job = Arc::clone(&self.on_job);
        let commands = self.commands.upgrade();
        let run = move || {
            let finished = launch::run(prepared, &*on_job);
            // Without the database thread nobody can record the launch; the job still ends for whoever watches it.
            let undelivered = match commands {
                Some(commands) => commands
                    .send_blocking(Command::Settle(Box::new(finished)))
                    .err()
                    .map(|unsent| unsent.into_inner()),
                None => Some(Command::Settle(Box::new(finished))),
            };
            if let Some(Command::Settle(finished)) = undelivered {
                launch::abandon(*finished, &*on_job);
            }
        };
        let previous = take(&self.job_thread);
        if let Some(previous) = previous {
            let _ = previous.join();
        }
        let spawned = thread::Builder::new().name("switch".to_owned()).spawn(run);
        match spawned {
            Ok(thread) => {
                if let Ok(mut slot) = self.job_thread.lock() {
                    *slot = Some(thread);
                }
                Ok(job)
            }
            Err(_) => {
                self.jobs.discard(&job.id);
                Err(ApiError {
                    code: "INTERNAL",
                    message: "The switch could not be started.".to_owned(),
                })
            }
        }
    }

    fn restore_backup(
        &self,
        store: &Store,
        launcher: LauncherId,
        id: &str,
        must_be_listed: bool,
    ) -> Result<RestoreResult, ApiError> {
        if self.jobs.is_busy() {
            return Err(ServiceError::Busy.into());
        }
        let (launchers, config) = self.steam(store, launcher)?;
        let backups = self.backup_service(launchers, &config);
        if must_be_listed
            && backups
                .get(launcher, id)
                .map_err(ServiceError::backup(launcher))?
                .is_none()
        {
            return Err(ServiceError::SnapshotGone.into());
        }
        Ok(switch::restore_backup(
            &launchers.steam(&config),
            &launchers.platform,
            &backups,
            id,
        )?)
    }

    fn pending_recovery(&self, store: &Store) -> Result<Option<SwitchRecovery>, ApiError> {
        let Some(entry) = self.recovery.pending() else {
            return Ok(None);
        };
        let Some(backup_id) = entry.backup_id.clone() else {
            return Ok(None);
        };
        let snapshot_available = match self.steam(store, entry.launcher) {
            Ok((launchers, config)) => self
                .backup_service(launchers, &config)
                .get(entry.launcher, &backup_id)
                .is_ok_and(|snapshot| snapshot.is_some()),
            Err(_) => false,
        };
        Ok(Some(SwitchRecovery {
            job_id: entry.job_id.clone(),
            launcher: entry.launcher,
            account_name: store
                .account(&entry.account_id)?
                .map(|account| account.display_name),
            started_at: entry.started_at.clone(),
            backup_id,
            snapshot_available,
            phase: entry.phase(),
        }))
    }
}

fn no_launchers() -> ApiError {
    ApiError {
        code: "NOT_IMPLEMENTED",
        message: "This build cannot reach a launcher yet.".to_owned(),
    }
}

fn read_library(store: &Store, data_dir: &Path) -> Result<Library, ApiError> {
    Ok(Library {
        games: library::list(store, data_dir)?,
        sort: store.library_sort()?,
    })
}

fn answer<T>(reply: &Reply<T>, result: Result<T, ApiError>) {
    // Whoever asked may have gone away meanwhile, and then nobody is left to tell.
    let _ = reply.send_blocking(result);
}
