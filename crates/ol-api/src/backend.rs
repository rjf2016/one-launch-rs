use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use async_channel::{Receiver, Sender, WeakSender};
use ol_model::{
    Account, BackupSnapshot, Ciphertext, InstalledGame, LaunchJob, LaunchPreflight, LauncherId,
    LauncherInstall, LauncherSession, LauncherSessionState, LauncherStatus, LibraryScanResult,
    LibrarySort, RecentLaunch, RememberedAccount, RestoreResult, SecretKey, SettingKey, Settings,
    Sidebar, SwitchPlan, SwitchRecovery,
};
use ol_platform_api::{Clipboard, DataProtector, Shell};
use ol_services::accounts::{self, AccountError, AccountPatch, NewAccount};
use ol_services::capture::{self, Captured};
use ol_services::clipboard::{self, ClipboardService, CopyError, CopyField, CopyResult, Timer};
use ol_services::data_folder::{self, Folder, OpenError};
use ol_services::install::{self, AutostartError, AutostartStatus, UpdateStatus};
use ol_services::journal::Recovery;
use ol_services::launch::{self, FinishedJob, JobBoard, LastPreflight, StartInput};
use ol_services::launchers::{
    self, Adapter, Configs, ImportResult, Launchers, ServiceError, require_implemented,
};
use ol_services::library::{self, LibraryGame};
use ol_services::log::paths::in_data_folder;
use ol_services::log::{Log, is_refusal};
use ol_services::sessions::{StoreThread, StoreWork};
use ol_services::settings::{self as setting, SettingError};
use ol_services::switch;
use ol_services::vault_health::{
    self, EncryptionState, VaultAccess, VaultHealth, VaultHealthError, VaultReset,
    VaultResetRefusal, VaultResetSummary,
};
use ol_services::window::{self, QuitQuestion, WindowLog};
use ol_store::{Store, StoreError};
use ol_vault::{KeyError, OsCryptVault, Recovered, Secret, Vault, VaultError};
use serde_json::{Value, json};
use thiserror::Error;

const RECENT_LAUNCHES_MOST: u32 = 50;

// What the Electron app sends across to its page for a failure: a code to act on and a sentence to show.
#[derive(Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct ApiError {
    pub code: &'static str,
    pub message: String,
}

// The message can name an account, as a refused duplicate does, so only the code is printed.
impl std::fmt::Debug for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ApiError({})", self.code)
    }
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

impl From<SettingError> for ApiError {
    fn from(error: SettingError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

impl From<VaultHealthError> for ApiError {
    fn from(error: VaultHealthError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

impl From<VaultResetRefusal> for ApiError {
    fn from(refusal: VaultResetRefusal) -> Self {
        Self {
            code: refusal.code(),
            message: refusal.to_string(),
        }
    }
}

impl From<AutostartError> for ApiError {
    fn from(error: AutostartError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

impl From<CopyError> for ApiError {
    fn from(error: CopyError) -> Self {
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

// What One Launch has saved of an account's sign-in, and what the launcher's own files say now. Never the sign-in itself.
pub struct SavedSignIn {
    pub session: Option<LauncherSession>,
    // None for a launcher that keeps a list of accounts and no session to save, and when its files could not be read.
    pub state: Option<LauncherSessionState>,
    pub state_error: Option<String>,
}

// Hears the launch job each time it changes, on the thread that changed it.
pub type JobObserver = Arc<dyn Fn(&LaunchJob) + Send + Sync>;

// A setting the backend has just stored, or removed (None): the Electron app's onSettingChanged, which turns the tray icon on and off.
pub type SettingChange = (SettingKey, Option<String>);

// What the Diagnostics panel shows of this copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppInfo {
    pub version: String,
    pub data_dir: PathBuf,
    pub log_dir: PathBuf,
    pub db_path: PathBuf,
    // "Development" or "Packaged", as the Build row says.
    pub build: &'static str,
}

type Reply<T> = Sender<Result<T, ApiError>>;

enum Command {
    Library(Reply<Library>),
    Accounts(Reply<Vec<Account>>),
    SetLibrarySort(LibrarySort, Reply<()>),
    Sidebar(Reply<Sidebar>),
    SetSidebar(Sidebar, Reply<()>),
    BackUpDatabase(Reply<()>),
    Settings(Reply<Settings>),
    SetSetting(SettingKey, String, Reply<()>),
    SetWindowBounds(String),
    #[cfg(feature = "fake")]
    SeedSetting(SettingKey, String, Reply<()>),
    WatchSettings(Sender<SettingChange>),
    VaultStatus(Reply<VaultHealth>),
    ResetVault(Reply<VaultReset>),
    TestVault(Reply<Duration>),
    LauncherStatuses(Reply<Vec<LauncherStatus>>),
    LocateLauncher(LauncherId, String, Reply<LauncherInstall>),
    ResetLauncherPath(LauncherId, Reply<()>),
    CreateAccount(Box<NewAccount>, Reply<Account>),
    Account(String, Reply<Account>),
    UpdateAccount(String, Box<AccountPatch>, Reply<Account>),
    DeleteAccount(String, Reply<()>),
    SetAccountPassword(String, Secret, Reply<Account>),
    ClearAccountPassword(String, Reply<Account>),
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
    Session(String, Reply<SavedSignIn>),
    CaptureSession(String, Reply<Captured>),
    ForgetSession(String, Reply<()>),
    UseClipboard(ClipboardService),
    UseShell(Arc<dyn Shell>),
    OpenFolder(Folder, Reply<()>),
    Copy(String, CopyField, Reply<CopyResult>),
    #[cfg(feature = "fake")]
    SeedSession(String, Option<String>, ol_vault::Secret, Reply<()>),
    // Work a switch hands over from its own thread: a question about accounts or saved sign-ins.
    OnStore(StoreWork),
    Settle(Box<FinishedJob>),
    // Answered once every request sent before it is handled.
    Drain(Reply<()>),
}

impl Command {
    const fn channel(&self) -> &'static str {
        match self {
            Self::Library(_) => "library:list",
            Self::Accounts(_) => "accounts:list",
            Self::SetLibrarySort(..) => "library:setSort",
            Self::Sidebar(_) => "sidebar:get",
            Self::SetSidebar(..) => "sidebar:set",
            Self::BackUpDatabase(_) => "database:backUp",
            Self::Settings(_) => "settings:getAll",
            Self::SetSetting(..) => "settings:set",
            Self::SetWindowBounds(_) => "window:bounds",
            #[cfg(feature = "fake")]
            Self::SeedSetting(..) => "settings:seed",
            Self::WatchSettings(_) => "settings:watch",
            Self::VaultStatus(_) => "vault:status",
            Self::ResetVault(_) => "vault:reset",
            Self::TestVault(_) => "vault:test",
            Self::LauncherStatuses(_) => "launchers:status",
            Self::LocateLauncher(..) => "launchers:locate",
            Self::ResetLauncherPath(..) => "launchers:resetPath",
            Self::CreateAccount(..) => "accounts:create",
            Self::Account(..) => "accounts:get",
            Self::UpdateAccount(..) => "accounts:update",
            Self::DeleteAccount(..) => "accounts:delete",
            Self::SetAccountPassword(..) => "accounts:setPassword",
            Self::ClearAccountPassword(..) => "accounts:clearPassword",
            Self::Rescan(_) => "library:rescan",
            Self::InstalledGames(..) => "launchers:games",
            Self::RememberedAccounts(..) => "launchers:rememberedAccounts",
            Self::ImportAccounts(..) => "launchers:importAccounts",
            Self::SwitchPlan(..) => "launchers:switchPlan",
            Self::LaunchPreflight(..) => "launch:preflight",
            Self::StartLaunch(..) => "launch:start",
            Self::RecentLaunches(..) => "launch:recent",
            Self::Backups(..) => "backups:list",
            Self::RestoreBackup(..) => "backups:restore",
            Self::Recovery(_) => "recovery:get",
            Self::RestoreRecovery(_) => "recovery:restore",
            Self::DismissRecovery(_) => "recovery:dismiss",
            Self::Session(..) => "sessions:get",
            Self::CaptureSession(..) => "sessions:capture",
            Self::ForgetSession(..) => "sessions:forget",
            Self::UseClipboard(_) => "clipboard:use",
            Self::UseShell(_) => "shell:use",
            Self::OpenFolder(..) => "app:openPath",
            Self::Copy(..) => "accounts:copy",
            #[cfg(feature = "fake")]
            Self::SeedSession(..) => "sessions:seed",
            Self::OnStore(_) => "store:work",
            Self::Settle(_) => "launch:settle",
            Self::Drain(_) => "backend:drain",
        }
    }
}

struct Said {
    log: Log,
    channel: &'static str,
}

impl Said {
    fn answer<T>(&self, reply: &Reply<T>, result: Result<T, ApiError>) {
        if let Err(error) = &result {
            let fields = json!({ "code": error.code, "message": error.message });
            if is_refusal(error.code) {
                self.log
                    .warn(&format!("ipc {} refused", self.channel), fields);
            } else {
                self.log
                    .error(&format!("ipc {} failed", self.channel), fields);
            }
        }
        answer(reply, result);
    }
}

fn removed_counts(removed: &VaultResetSummary) -> Value {
    json!({ "passwords": removed.passwords, "sessions": removed.sessions, "secrets": removed.secrets })
}

// The database's thread as a switch reaches it.
struct DatabaseThread(Option<Sender<Command>>);

impl StoreThread for DatabaseThread {
    fn run(&self, work: StoreWork) -> bool {
        self.0
            .as_ref()
            .is_some_and(|commands| commands.send_blocking(Command::OnStore(work)).is_ok())
    }
}

type Thread = Arc<Mutex<Option<JoinHandle<()>>>>;

type SharedVault = Arc<dyn Vault + Send + Sync>;

enum VaultSource {
    None,
    Launchers,
    DataFolder(Box<dyn DataProtector + Send>),
    // The data folder's key as a start on this PC found it.
    Opened(
        Result<OsCryptVault, KeyError>,
        Box<dyn DataProtector + Send>,
    ),
}

#[derive(Clone, Default)]
enum VaultSlot {
    #[default]
    Unavailable,
    Open(SharedVault),
    // The data folder's key is there, Windows works, and it does not unwrap the key for this user. It is replaced only by a reset the user asked for.
    KeyLost,
}

impl VaultSlot {
    fn of(opened: Result<OsCryptVault, KeyError>) -> Self {
        match opened {
            Ok(vault) => Self::Open(Arc::new(vault)),
            Err(KeyError::Unwrap) => Self::KeyLost,
            Err(_) => Self::Unavailable,
        }
    }
}

pub(crate) struct NoVault;

impl Vault for NoVault {
    fn encrypt(&self, _: &Secret) -> Result<Ciphertext, VaultError> {
        Err(VaultError::Unavailable)
    }

    fn decrypt(&self, _: &Ciphertext) -> Result<Secret, VaultError> {
        Err(VaultError::Unavailable)
    }

    fn has_key(&self) -> bool {
        false
    }
}

// While the key is lost nothing new is sealed, because no other key may take its place but by the reset; what was sealed cannot be read, as the reset's banner says.
struct LostKey;

impl Vault for LostKey {
    fn encrypt(&self, _: &Secret) -> Result<Ciphertext, VaultError> {
        Err(VaultError::Unavailable)
    }

    fn decrypt(&self, _: &Ciphertext) -> Result<Secret, VaultError> {
        Err(VaultError::Unreadable)
    }

    fn has_key(&self) -> bool {
        false
    }
}

// The one vault the backend and the launchers seal and open with, so that a key a reset or a re-check puts in is the key of every seal from then on, on the switch's thread too.
#[derive(Default)]
struct VaultCell(RwLock<VaultSlot>);

impl VaultCell {
    fn slot(&self) -> VaultSlot {
        match self.0.read() {
            Ok(slot) => slot.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    fn put(&self, slot: VaultSlot) {
        match self.0.write() {
            Ok(mut held) => *held = slot,
            Err(poisoned) => *poisoned.into_inner() = slot,
        }
    }
}

impl Vault for VaultCell {
    fn encrypt(&self, plaintext: &Secret) -> Result<Ciphertext, VaultError> {
        match self.slot() {
            VaultSlot::Open(vault) => vault.encrypt(plaintext),
            VaultSlot::KeyLost => LostKey.encrypt(plaintext),
            VaultSlot::Unavailable => NoVault.encrypt(plaintext),
        }
    }

    fn decrypt(&self, ciphertext: &Ciphertext) -> Result<Secret, VaultError> {
        match self.slot() {
            VaultSlot::Open(vault) => vault.decrypt(ciphertext),
            VaultSlot::KeyLost => LostKey.decrypt(ciphertext),
            VaultSlot::Unavailable => NoVault.decrypt(ciphertext),
        }
    }

    fn has_key(&self) -> bool {
        match self.slot() {
            VaultSlot::Open(vault) => vault.has_key(),
            VaultSlot::KeyLost | VaultSlot::Unavailable => false,
        }
    }
}

// The database lives on one thread of its own. A handle only sends that thread a request and waits for the answer, so it can be held and cloned anywhere. A switch waits for a launcher on a second thread, which has no database: what it needs of the accounts and their saved sign-ins it asks of the first.
#[derive(Clone)]
pub struct Backend {
    commands: Sender<Command>,
    thread: Thread,
    job_thread: Thread,
    jobs: JobBoard,
    has_launchers: bool,
    data_dir: PathBuf,
    version: String,
    log: Log,
}

pub struct BackendStart {
    log: Log,
}

impl BackendStart {
    pub fn start(self, data_dir: PathBuf, version: &str) -> Backend {
        Backend::start_on(data_dir, version, None, VaultSource::None, None, self.log)
    }

    pub fn start_with_vault_of_data_folder(
        self,
        data_dir: PathBuf,
        version: &str,
        protector: Box<dyn DataProtector + Send>,
    ) -> Backend {
        Backend::start_on(
            data_dir,
            version,
            None,
            VaultSource::DataFolder(protector),
            None,
            self.log,
        )
    }

    pub fn start_on_this_pc(
        self,
        data_dir: PathBuf,
        version: &str,
        launchers: Launchers,
        protector: Box<dyn DataProtector + Send>,
        on_job: Option<JobObserver>,
    ) -> Result<Backend, KeyError> {
        let opened = match OsCryptVault::open_or_create(&data_dir, &*protector) {
            Err(error)
                if !matches!(
                    error,
                    KeyError::Unwrap
                        | KeyError::WindowsUnavailable
                        | KeyError::Wrap
                        | KeyError::Random
                ) =>
            {
                self.log.error(
                    "the vault key could not be opened",
                    json!({ "error": error.to_string() }),
                );
                return Err(error);
            }
            opened => opened,
        };
        Ok(Backend::start_on(
            data_dir,
            version,
            Some(launchers),
            VaultSource::Opened(opened, protector),
            on_job,
            self.log,
        ))
    }

    pub fn start_with(
        self,
        data_dir: PathBuf,
        version: &str,
        launchers: Launchers,
        on_job: Option<JobObserver>,
    ) -> Backend {
        Backend::start_on(
            data_dir,
            version,
            Some(launchers),
            VaultSource::Launchers,
            on_job,
            self.log,
        )
    }
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
        Self::logging_to(Log::off()).start(data_dir, version)
    }

    // A key that does not unwrap is never replaced: it may be another Windows user's, and what it sealed would be lost with it.
    pub fn start_with_vault_of_data_folder(
        data_dir: PathBuf,
        version: &str,
        protector: Box<dyn DataProtector + Send>,
    ) -> Self {
        Self::logging_to(Log::off()).start_with_vault_of_data_folder(data_dir, version, protector)
    }

    // The launchers of this PC on the data folder's own vault. Its key is opened, or made for a folder that has none, before the thread starts. A key Windows does not unwrap leaves the vault in its Key lost state, and a Windows that unwraps or wraps nothing just now leaves it unavailable with nothing written, to be tried again at the next look; the accounts are readable either way. Any other fault of the key file stops the start.
    pub fn start_on_this_pc(
        data_dir: PathBuf,
        version: &str,
        launchers: Launchers,
        protector: Box<dyn DataProtector + Send>,
        on_job: Option<JobObserver>,
    ) -> Result<Self, KeyError> {
        Self::logging_to(Log::off())
            .start_on_this_pc(data_dir, version, launchers, protector, on_job)
    }

    // As `start`, on the machine `launchers` describes. `on_job` hears every change of a launch job.
    pub fn start_with(
        data_dir: PathBuf,
        version: &str,
        launchers: Launchers,
        on_job: Option<JobObserver>,
    ) -> Self {
        Self::logging_to(Log::off()).start_with(data_dir, version, launchers, on_job)
    }

    pub const fn logging_to(log: Log) -> BackendStart {
        BackendStart { log }
    }

    fn start_on(
        data_dir: PathBuf,
        version: &str,
        mut launchers: Option<Launchers>,
        vault: VaultSource,
        on_job: Option<JobObserver>,
        log: Log,
    ) -> Self {
        if let Some(launchers) = &mut launchers {
            launchers.log = log.clone();
        }
        let (commands, requests) = async_channel::unbounded();
        let version = version.to_owned();
        let jobs = JobBoard::default();
        let job_thread: Thread = Arc::default();
        let has_launchers = launchers.is_some();
        let folder = data_dir.clone();
        let serving = Serving {
            data_dir,
            launchers,
            vault: Arc::default(),
            protector: None,
            shell: None,
            settings_watch: None,
            jobs: jobs.clone(),
            on_job: on_job.unwrap_or_else(|| Arc::new(|_| {})),
            commands: commands.downgrade(),
            job_thread: Arc::clone(&job_thread),
            last_preflight: LastPreflight::default(),
            recovery: Recovery::default(),
            clipboard: None,
            log: log.clone(),
        };
        let running_version = version.clone();
        let thread = thread::Builder::new()
            .name("backend".to_owned())
            .spawn(move || serving.serve(&running_version, vault, &requests))
            .ok();
        Self {
            commands,
            thread: Arc::new(Mutex::new(thread)),
            job_thread,
            jobs,
            has_launchers,
            data_dir: folder,
            version,
            log,
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

    pub async fn sidebar(&self) -> Result<Sidebar, ApiError> {
        self.ask(Command::Sidebar).await
    }

    pub async fn set_sidebar(&self, sidebar: Sidebar) -> Result<(), ApiError> {
        self.ask(|reply| Command::SetSidebar(sidebar, reply)).await
    }

    pub async fn back_up_database(&self) -> Result<(), ApiError> {
        self.ask(Command::BackUpDatabase).await
    }

    // Every row of the settings table that is one of the Electron app's nineteen keys, as its settings:getAll answers. The typed reads of `Settings` give each key with its default.
    pub async fn settings(&self) -> Result<Settings, ApiError> {
        self.ask(Command::Settings).await
    }

    // The Electron app's settings:set: the key's rule first (refused with BAD_REQUEST and its message), then the row, then the settings observer.
    pub async fn set_setting(&self, key: SettingKey, value: String) -> Result<(), ApiError> {
        self.ask(|reply| Command::SetSetting(key, value, reply))
            .await
    }

    // No answer is waited for, so a place handed over as the window closes is still stored before the database closes.
    pub fn remember_window_bounds(&self, bounds: String) {
        let _ = self
            .commands
            .send_blocking(Command::SetWindowBounds(bounds));
    }

    // Stores a setting as the fake launchers' seed gives it, as the Electron app's seed does: a key only the app sets is passed over, and any other is stored without its rule.
    #[cfg(feature = "fake")]
    pub async fn seed_setting(&self, key: SettingKey, value: String) -> Result<(), ApiError> {
        self.ask(|reply| Command::SeedSetting(key, value, reply))
            .await
    }

    // From now on the answer hears every setting this backend stores or removes, each before the request that changed it is answered. A later call takes the place of an earlier one. The changes come over a channel, so nothing that hears them runs on the database's thread or can hold it up.
    pub fn setting_changes(&self) -> Receiver<SettingChange> {
        let (changes, heard) = async_channel::unbounded();
        let _ = self.commands.send_blocking(Command::WatchSettings(changes));
        heard
    }

    // The Encryption panel's Status row and the banner above the tabs.
    pub async fn vault_status(&self) -> Result<VaultHealth, ApiError> {
        self.ask(Command::VaultStatus).await
    }

    // Clears every stored password, saved sign-in and named secret, and puts a new probe in: only while they cannot be read, and never while a switch runs. A key Windows does not unwrap for this user is tried once more and, failing that, replaced first; nothing else ever replaces one. A key that unwraps on that try clears nothing.
    pub async fn reset_vault(&self) -> Result<VaultReset, ApiError> {
        self.ask(Command::ResetVault).await
    }

    // Run test: a random value stored, read back and deleted. Answers how long it took.
    pub async fn test_vault(&self) -> Result<Duration, ApiError> {
        self.ask(Command::TestVault).await
    }

    // The Launchers tab: each of the four launchers in order. A build that cannot reach a launcher answers it as not implemented.
    pub async fn launcher_statuses(&self) -> Result<Vec<LauncherStatus>, ApiError> {
        self.ask(Command::LauncherStatuses).await
    }

    // Locate: `exe` is the file the user picked in the window's own dialog. The adapter only reads; the launcher's folder is stored when it holds the launcher, and the Library is scanned again before the next request is answered.
    pub async fn locate_launcher(
        &self,
        launcher: LauncherId,
        exe: &str,
    ) -> Result<LauncherInstall, ApiError> {
        let exe = exe.to_owned();
        self.ask(|reply| Command::LocateLauncher(launcher, exe, reply))
            .await
    }

    // Reset: the folder chosen with Locate is forgotten and detection takes over, with a scan of the Library after it.
    pub async fn reset_launcher_path(&self, launcher: LauncherId) -> Result<(), ApiError> {
        self.ask(|reply| Command::ResetLauncherPath(launcher, reply))
            .await
    }

    // The folder the Diagnostics and Backups panels open.
    pub fn folder(&self, folder: Folder) -> PathBuf {
        folder.path(&self.data_dir)
    }

    // Until this is called a folder is not opened, so a run that is handed no shell never reaches Explorer.
    pub fn use_shell(&self, shell: Arc<dyn Shell>) {
        let _ = self.commands.send_blocking(Command::UseShell(shell));
    }

    // Shows one of the data folder's own folders in Explorer, through the shell this run was given; no other path can be named.
    pub async fn open_folder(&self, folder: Folder) -> Result<(), ApiError> {
        self.ask(|reply| Command::OpenFolder(folder, reply)).await
    }

    pub fn app_info(&self) -> AppInfo {
        AppInfo {
            version: self.version.clone(),
            data_dir: self.data_dir.clone(),
            log_dir: Folder::Logs.path(&self.data_dir),
            db_path: ol_store::database_path(&self.data_dir),
            build: install::build_label(),
        }
    }

    pub fn update_status(&self) -> UpdateStatus {
        install::update_status()
    }

    pub fn autostart_status(&self) -> AutostartStatus {
        install::autostart_status()
    }

    pub fn set_autostart(&self, open_at_login: bool) -> Result<AutostartStatus, ApiError> {
        let status = install::set_autostart(open_at_login)?;
        self.log
            .child("ipc")
            .info("autostart updated", json!({ "openAtLogin": open_at_login }));
        Ok(status)
    }

    // Whether a switch or a launch runs now: what a close of the window and a quit ask about.
    pub fn is_busy(&self) -> bool {
        self.jobs.is_busy()
    }

    // The refusal of the Electron app's requireIdle, for a step that comes before a request, such as the file dialog of Locate.
    pub fn require_idle(&self) -> Result<(), ApiError> {
        if self.jobs.is_busy() {
            return Err(ServiceError::Busy.into());
        }
        Ok(())
    }

    pub fn running_job(&self) -> Option<LaunchJob> {
        self.jobs.current()
    }

    // The question to ask before a quit stops the running switch. None when nothing runs.
    pub fn quit_question(&self) -> Option<QuitQuestion> {
        self.jobs.current().as_ref().map(window::quit_question)
    }

    pub fn window_log(&self) -> WindowLog {
        WindowLog::new(&self.log)
    }

    // Makes an account. Its password, when it has one, is encrypted before the database is touched.
    pub async fn create_account(&self, account: NewAccount) -> Result<Account, ApiError> {
        self.ask(|reply| Command::CreateAccount(Box::new(account), reply))
            .await
    }

    pub async fn account(&self, id: &str) -> Result<Account, ApiError> {
        let id = id.to_owned();
        self.ask(|reply| Command::Account(id, reply)).await
    }

    pub async fn update_account(&self, id: &str, patch: AccountPatch) -> Result<Account, ApiError> {
        let id = id.to_owned();
        self.ask(|reply| Command::UpdateAccount(id, Box::new(patch), reply))
            .await
    }

    pub async fn delete_account(&self, id: &str) -> Result<(), ApiError> {
        let id = id.to_owned();
        self.ask(|reply| Command::DeleteAccount(id, reply)).await
    }

    pub async fn set_account_password(
        &self,
        id: &str,
        password: Secret,
    ) -> Result<Account, ApiError> {
        let id = id.to_owned();
        self.ask(|reply| Command::SetAccountPassword(id, password, reply))
            .await
    }

    pub async fn clear_account_password(&self, id: &str) -> Result<Account, ApiError> {
        let id = id.to_owned();
        self.ask(|reply| Command::ClearAccountPassword(id, reply))
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
        let job = self.jobs.cancel(job_id)?;
        if job.state == ol_model::JobState::Running {
            self.log
                .child("launch")
                .info("launch job cancel requested", json!({ "jobId": job_id }));
        }
        Ok(job)
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

    // The sign-in saved for an account and what its launcher remembers now. A launcher whose files cannot be read never hides what is saved.
    pub async fn session(&self, account_id: &str) -> Result<SavedSignIn, ApiError> {
        let account_id = account_id.to_owned();
        self.ask(|reply| Command::Session(account_id, reply)).await
    }

    pub async fn capture_session(&self, account_id: &str) -> Result<Captured, ApiError> {
        let account_id = account_id.to_owned();
        self.ask(|reply| Command::CaptureSession(account_id, reply))
            .await
    }

    pub async fn forget_session(&self, account_id: &str) -> Result<(), ApiError> {
        let account_id = account_id.to_owned();
        self.ask(|reply| Command::ForgetSession(account_id, reply))
            .await
    }

    // Until this is called a copy is refused, so a run that is handed no clipboard never reaches one.
    pub fn use_clipboard(&self, clipboard: Arc<dyn Clipboard>, timer: Arc<dyn Timer>) {
        let service = ClipboardService::new(clipboard, timer);
        let _ = self.commands.send_blocking(Command::UseClipboard(service));
    }

    pub async fn copy_to_clipboard(
        &self,
        account_id: &str,
        field: CopyField,
    ) -> Result<CopyResult, ApiError> {
        let account_id = account_id.to_owned();
        self.ask(|reply| Command::Copy(account_id, field, reply))
            .await
    }

    // Stores a sign-in for an account as it is given, encrypted like any other. Only a build with the fake launchers has it: the app itself saves a sign-in only from the launcher's own file.
    #[cfg(feature = "fake")]
    pub async fn seed_session(
        &self,
        account_id: &str,
        launcher_uid: Option<String>,
        blob: ol_vault::Secret,
    ) -> Result<(), ApiError> {
        let account_id = account_id.to_owned();
        self.ask(|reply| Command::SeedSession(account_id, launcher_uid, blob, reply))
            .await
    }

    // Stops a running switch and waits for it, then waits for the request in hand and for the database to close, which folds its journal back into the one file.
    pub fn shut_down(&self) {
        if let Some(job) = self.jobs.current() {
            self.log
                .warn("stopping the running switch", json!({ "reason": "quit" }));
            self.log.child("launch").info(
                "launch service stopping; aborting the running job",
                json!({ "jobId": job.id }),
            );
        }
        self.jobs.close();
        // A start the database's thread had in hand when the board closed has its thread in the slot once this is answered.
        let (reply, drained) = async_channel::bounded(1);
        if self.commands.send_blocking(Command::Drain(reply)).is_ok() {
            let _ = drained.recv_blocking();
        }
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

// When the database's thread ends, however it ends, what was still asked of it is dropped: whoever waits for an answer hears that none will come, and a switch on its own thread does not wait for good.
struct Unanswered<'a>(&'a Receiver<Command>);

impl Drop for Unanswered<'_> {
    fn drop(&mut self) {
        self.0.close();
        while self.0.try_recv().is_ok() {}
    }
}

// However the database's thread ends, a panic included, a copy still on the clipboard is taken off.
impl Drop for Serving {
    fn drop(&mut self) {
        if let Some(clipboard) = &self.clipboard {
            clipboard.clear_if_unchanged();
        }
    }
}

struct Serving {
    data_dir: PathBuf,
    launchers: Option<Launchers>,
    vault: Arc<VaultCell>,
    // Kept for a reset of a lost key: the one way a key that is there is ever replaced.
    protector: Option<Box<dyn DataProtector + Send>>,
    shell: Option<Arc<dyn Shell>>,
    settings_watch: Option<Sender<SettingChange>>,
    jobs: JobBoard,
    on_job: JobObserver,
    commands: WeakSender<Command>,
    job_thread: Thread,
    last_preflight: LastPreflight,
    recovery: Recovery,
    clipboard: Option<ClipboardService>,
    log: Log,
}

impl Serving {
    fn serve(mut self, version: &str, vault: VaultSource, requests: &Receiver<Command>) {
        let _answered = Unanswered(requests);
        let log = self.log.clone();
        log.info(
            "starting",
            json!({
                "version": version,
                "dataDir": "chosen",
                "build": install::build_label(),
                "launchers": self.launchers.is_some(),
            }),
        );
        let opened = data_folder::open(&self.data_dir, version)
            .map(|(store, migrated)| {
                if !migrated.applied.is_empty() {
                    log.info(
                        "database migrated",
                        json!({ "from": migrated.from, "to": migrated.to, "applied": migrated.applied }),
                    );
                }
                store
            })
            .map_err(ApiError::from);
        match &opened {
            Ok(store) => {
                self.follow_stored_level(store);
                // Every name the database holds goes to the redaction before anything can mention it.
                if let Err(error) = store.accounts() {
                    log.warn(
                        "the accounts could not be read",
                        json!({ "code": error.code(), "error": error.to_string() }),
                    );
                }
                self.recovery = Recovery::load_logged(&self.data_dir, &log);
                self.open_vault(vault);
                self.store_probe(store);
                self.log_vault(store);
            }
            Err(error) => log.error(
                "the data folder could not be opened",
                json!({ "code": error.code, "error": error.message }),
            ),
        }
        while let Ok(command) = requests.recv_blocking() {
            let store = opened.as_ref().map_err(ApiError::clone);
            self.handle(command, store);
        }
        drop(opened);
        log.info("exit", ());
    }

    fn follow_stored_level(&self, store: &Store) {
        let stored = store
            .setting(SettingKey::LogLevel)
            .ok()
            .flatten()
            .and_then(|level| ol_model::LogLevel::parse(&level));
        if let Some(level) = stored {
            self.log.set_level(level);
        }
    }

    fn store_probe(&self, store: &Store) {
        match vault_health::ensure_probe(store, self.vault_access(), &self.stamp()) {
            Ok(true) => self.log.child("vault").info("vault probe stored", ()),
            Ok(false) => {}
            Err(error) => self.log.child("vault").warn(
                "the vault probe could not be stored",
                json!({ "code": error.code(), "error": error.to_string() }),
            ),
        }
    }

    fn log_vault(&self, store: &Store) {
        let Ok(health) = vault_health::status(store, self.vault_access()) else {
            return;
        };
        self.log.info(
            "vault",
            json!({ "available": health.available, "readable": health.readable }),
        );
        if !health.available {
            self.log.warn(
                "secure storage unavailable; secrets cannot be stored on this system",
                (),
            );
        }
        if health.readable == Some(false) {
            self.log.error(
                "stored secrets cannot be decrypted; the vault key was lost or belongs to another Windows user",
                (),
            );
        }
    }

    fn handle(&mut self, command: Command, store: Result<&Store, ApiError>) {
        let ipc = self.log.child("ipc");
        let said = Said {
            log: ipc.clone(),
            channel: command.channel(),
        };
        match command {
            Command::Library(reply) => said.answer(
                &reply,
                store.and_then(|store| read_library(store, &self.data_dir)),
            ),
            Command::Accounts(reply) => {
                said.answer(&reply, store.and_then(|store| Ok(store.accounts()?)));
            }
            Command::SetLibrarySort(sort, reply) => said.answer(
                &reply,
                store.and_then(|store| Ok(store.set_library_sort(sort)?)),
            ),
            Command::Sidebar(reply) => {
                said.answer(&reply, store.and_then(|store| Ok(store.sidebar()?)))
            }
            Command::SetSidebar(sidebar, reply) => said.answer(
                &reply,
                store.and_then(|store| Ok(store.set_sidebar(sidebar)?)),
            ),
            Command::BackUpDatabase(reply) => {
                let copied =
                    store.and_then(|store| Ok(data_folder::back_up(store, &self.data_dir)?));
                match &copied {
                    Ok(dest) => self.log.info(
                        "database copied",
                        json!({ "dest": in_data_folder(dest, &self.data_dir) }),
                    ),
                    Err(error) => self.log.warn(
                        "database copy failed",
                        json!({ "code": error.code, "error": error.message }),
                    ),
                }
                answer(&reply, copied.map(|_| ()));
            }
            Command::Settings(reply) => {
                said.answer(&reply, store.and_then(|store| Ok(store.settings()?)));
            }
            Command::SetSetting(key, value, reply) => {
                let changed = store.and_then(|store| Ok(setting::change(store, key, &value)?));
                if changed.is_ok() {
                    if key == SettingKey::LogLevel
                        && let Some(level) = ol_model::LogLevel::parse(&value)
                    {
                        self.log.set_level(level);
                    }
                    ipc.info(
                        "setting updated",
                        json!({ "key": key.as_str(), "value": value }),
                    );
                    self.changed(key, Some(&value));
                }
                said.answer(&reply, changed.map(|_| ()));
            }
            Command::SetWindowBounds(bounds) => {
                if let Ok(store) = store {
                    let _ = store.set_setting(SettingKey::WindowBounds, &bounds);
                }
            }
            #[cfg(feature = "fake")]
            Command::SeedSetting(key, value, reply) => said.answer(
                &reply,
                store.and_then(|store| {
                    if !key.is_main_only() {
                        store.set_setting(key, &value)?;
                    }
                    Ok(())
                }),
            ),
            Command::WatchSettings(changes) => self.settings_watch = Some(changes),
            Command::VaultStatus(reply) => said.answer(
                &reply,
                store.and_then(|store| {
                    self.recheck_vault(store);
                    Ok(vault_health::status(store, self.vault_access())?)
                }),
            ),
            Command::ResetVault(reply) => {
                let reset = store.and_then(|store| self.reset_vault(store));
                if let Ok(reset) = &reset {
                    match reset.summary() {
                        Some(removed) => ipc.warn("vault reset by user", removed_counts(&removed)),
                        None => self
                            .log
                            .child("vault")
                            .info("vault reset: the key opened after all; nothing removed", ()),
                    }
                }
                said.answer(&reply, reset);
            }
            Command::TestVault(reply) => said.answer(
                &reply,
                store.and_then(|store| {
                    Ok(vault_health::self_test(
                        store,
                        self.vault_access(),
                        &self.stamp(),
                    )?)
                }),
            ),
            Command::LauncherStatuses(reply) => {
                said.answer(
                    &reply,
                    store.and_then(|store| self.launcher_statuses(store)),
                );
            }
            Command::LocateLauncher(launcher, exe, reply) => {
                let located = store
                    .as_ref()
                    .map_err(ApiError::clone)
                    .and_then(|store| self.locate_launcher(store, launcher, &exe));
                let rescan = located.is_ok();
                if rescan {
                    ipc.info(
                        "launcher folder chosen by user",
                        json!({ "launcher": launcher.as_str() }),
                    );
                }
                said.answer(&reply, located);
                if rescan && let Ok(store) = store {
                    self.rescan_after_folder_change(store, launcher);
                }
            }
            Command::ResetLauncherPath(launcher, reply) => {
                let reset = store
                    .as_ref()
                    .map_err(ApiError::clone)
                    .and_then(|store| self.reset_launcher_path(store, launcher));
                let rescan = reset.is_ok();
                if rescan {
                    ipc.info(
                        "launcher folder reset to automatic detection",
                        json!({ "launcher": launcher.as_str() }),
                    );
                }
                said.answer(&reply, reset);
                if rescan && let Ok(store) = store {
                    self.rescan_after_folder_change(store, launcher);
                }
            }
            Command::CreateAccount(account, reply) => {
                let created = store.and_then(|store| self.create_account(store, &account));
                if let Ok(created) = &created {
                    ipc.info(
                        "account created",
                        json!({ "id": created.id, "launcher": created.launcher.as_str(), "hasSecret": created.has_secret }),
                    );
                }
                said.answer(&reply, created);
            }
            Command::Account(id, reply) => {
                said.answer(
                    &reply,
                    store.and_then(|store| Ok(accounts::require(store, &id)?)),
                );
            }
            Command::UpdateAccount(id, patch, reply) => {
                let updated = store.and_then(|store| {
                    self.require_idle()?;
                    let account = accounts::update(store, &id, &patch, &self.stamp())?;
                    self.last_preflight = LastPreflight::default();
                    Ok(account)
                });
                if updated.is_ok() {
                    let fields: Vec<&str> = [
                        ("displayName", patch.display_name.is_some()),
                        ("login", patch.login.is_some()),
                        ("launcherUid", patch.launcher_uid.is_some()),
                        ("tags", patch.tags.is_some()),
                        ("notes", patch.notes.is_some()),
                    ]
                    .into_iter()
                    .filter_map(|(field, given)| given.then_some(field))
                    .collect();
                    ipc.info("account updated", json!({ "id": id, "fields": fields }));
                }
                said.answer(&reply, updated);
            }
            Command::DeleteAccount(id, reply) => {
                let deleted = store.and_then(|store| {
                    self.require_idle()?;
                    accounts::delete(store, &id)?;
                    self.last_preflight = LastPreflight::default();
                    Ok(())
                });
                if deleted.is_ok() {
                    ipc.info("account deleted", json!({ "id": id }));
                }
                said.answer(&reply, deleted);
            }
            Command::SetAccountPassword(id, password, reply) => {
                let set = store.and_then(|store| {
                    self.require_idle()?;
                    let account =
                        accounts::set_password(store, self.vault(), &id, &password, &self.stamp())?;
                    self.last_preflight = LastPreflight::default();
                    Ok(account)
                });
                if set.is_ok() {
                    ipc.info("account password updated", json!({ "id": id }));
                }
                said.answer(&reply, set);
            }
            Command::ClearAccountPassword(id, reply) => {
                let cleared = store.and_then(|store| {
                    self.require_idle()?;
                    let account = accounts::clear_password(store, &id, &self.stamp())?;
                    self.last_preflight = LastPreflight::default();
                    Ok(account)
                });
                if cleared.is_ok() {
                    ipc.info("account password removed", json!({ "id": id }));
                }
                said.answer(&reply, cleared);
            }
            Command::Rescan(reply) => {
                said.answer(&reply, store.and_then(|store| self.rescan(store)))
            }
            Command::InstalledGames(launcher, reply) => said.answer(
                &reply,
                store.and_then(|store| {
                    self.with_adapter(store, launcher, |_, _, adapter| {
                        Ok(launchers::installed_games(adapter)?)
                    })
                }),
            ),
            Command::RememberedAccounts(launcher, reply) => said.answer(
                &reply,
                store.and_then(|store| {
                    self.with_adapter(store, launcher, |_, _, adapter| {
                        Ok(launchers::remembered_accounts(store, adapter)?)
                    })
                }),
            ),
            Command::ImportAccounts(launcher, keys, reply) => {
                let imported = store.and_then(|store| {
                    self.with_adapter(store, launcher, |launchers, _, adapter| {
                        Ok(launchers::import_accounts(
                            store,
                            launchers.vault.as_ref(),
                            adapter,
                            &keys,
                            &self.now(launchers),
                        )?)
                    })
                });
                if let Ok(imported) = &imported {
                    ipc.info(
                        "imported launcher accounts",
                        json!({
                            "launcher": launcher.as_str(),
                            "created": imported.accounts.len(),
                            "skipped": imported.skipped.len(),
                        }),
                    );
                }
                said.answer(&reply, imported);
            }
            Command::SwitchPlan(account_id, reply) => {
                let planned = store.and_then(|store| {
                    let account = accounts::require(store, &account_id)?;
                    self.with_adapter(store, account.launcher, |_, _, adapter| {
                        Ok(adapter
                            .plan_switch(&launch::account_ref(&account))
                            .map_err(ServiceError::adapter(account.launcher))?)
                    })
                });
                if let Ok(plan) = &planned {
                    ipc.info(
                        "switch plan generated",
                        json!({
                            "accountId": account_id,
                            "launcher": plan.launcher.as_str(),
                            "strategy": plan.strategy.as_str(),
                            "steps": plan.steps.len(),
                        }),
                    );
                }
                said.answer(&reply, planned);
            }
            Command::LaunchPreflight(account_id, game_id, reply) => said.answer(
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
                let confirm_close = start.confirm_close;
                let started = store.and_then(|store| self.start_launch(store, start));
                if let Ok(job) = &started {
                    ipc.info(
                        "launch job accepted",
                        json!({
                            "jobId": job.id,
                            "accountId": job.account.id,
                            "gameId": job.game.as_ref().map(|game| &game.id),
                            "confirmClose": confirm_close,
                            "direct": job.direct,
                        }),
                    );
                }
                said.answer(&reply, started);
            }
            Command::RecentLaunches(limit, reply) => said.answer(
                &reply,
                store.and_then(|store| Ok(store.recent_launches(limit)?)),
            ),
            Command::Backups(launcher, reply) => said.answer(
                &reply,
                store.and_then(|store| {
                    self.with_adapter(store, launcher, |launchers, configs, _| {
                        Ok(launchers
                            .backups(&self.data_dir, configs)
                            .list(launcher)
                            .map_err(ServiceError::backup(launcher))?)
                    })
                }),
            ),
            Command::RestoreBackup(launcher, id, reply) => {
                let restored =
                    store.and_then(|store| self.restore_backup(store, launcher, &id, true));
                if let Ok(restored) = &restored {
                    ipc.warn(
                        "backup restored by user",
                        json!({
                            "launcher": launcher.as_str(),
                            "id": id,
                            "files": restored.files.len(),
                            "registry": restored.registry.len(),
                        }),
                    );
                    self.recovery
                        .restored_from_backups(&self.data_dir, launcher);
                }
                said.answer(&reply, restored);
            }
            Command::Recovery(reply) => {
                said.answer(&reply, store.and_then(|store| self.pending_recovery(store)));
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
                    if let Some(entry) = self.recovery.pending() {
                        self.log.child("recovery").warn(
                            "interrupted switch recovered from its snapshot",
                            json!({ "jobId": entry.job_id, "launcher": entry.launcher.as_str(), "backupId": entry.backup_id }),
                        );
                    }
                    self.recovery.settle(&self.data_dir);
                }
                said.answer(&reply, restored);
            }
            Command::DismissRecovery(reply) => {
                if let Some(entry) = self.recovery.pending() {
                    self.log.child("recovery").info(
                        "interrupted switch left as it is",
                        json!({ "jobId": entry.job_id, "launcher": entry.launcher.as_str() }),
                    );
                }
                self.recovery.settle(&self.data_dir);
                said.answer(&reply, Ok(()));
            }
            Command::Session(account_id, reply) => said.answer(
                &reply,
                store.and_then(|store| self.saved_sign_in(store, &account_id)),
            ),
            Command::CaptureSession(account_id, reply) => {
                let captured = store.and_then(|store| {
                    self.require_idle()?;
                    let account = accounts::require(store, &account_id)?;
                    let captured =
                        self.with_adapter(store, account.launcher, |_, _, adapter| {
                            Ok(capture::capture(store, adapter, &account)?)
                        })?;
                    self.last_preflight = LastPreflight::default();
                    Ok(captured)
                });
                if let Ok(captured) = &captured {
                    self.log.child("launchers").info(
                        "session captured",
                        json!({ "launcher": captured.account.launcher.as_str(), "accountId": account_id }),
                    );
                    ipc.info(
                        "session captured by user",
                        json!({ "accountId": account_id, "launcher": captured.account.launcher.as_str() }),
                    );
                }
                said.answer(&reply, captured);
            }
            Command::ForgetSession(account_id, reply) => {
                let forgotten = store.and_then(|store| {
                    self.require_idle()?;
                    let forgotten = capture::forget(store, &account_id)?;
                    self.last_preflight = LastPreflight::default();
                    Ok(forgotten)
                });
                if let Ok(Some(launcher)) = &forgotten {
                    self.log.child("launchers").info(
                        "session forgotten",
                        json!({ "launcher": launcher.as_str(), "accountId": account_id }),
                    );
                }
                said.answer(&reply, forgotten.map(|_| ()));
            }
            Command::UseShell(shell) => self.shell = Some(shell),
            Command::OpenFolder(folder, reply) => said.answer(&reply, self.open_folder(folder)),
            Command::UseClipboard(service) => {
                if let Some(previous) = self.clipboard.replace(service) {
                    previous.clear_if_unchanged();
                }
            }
            Command::Copy(account_id, field, reply) => {
                let copied = store.and_then(|store| {
                    Ok(clipboard::copy_account_field(
                        store,
                        self.vault(),
                        self.clipboard.as_ref(),
                        &account_id,
                        field,
                    )?)
                });
                if let Ok(copied) = &copied {
                    ipc.info(
                        "copied to clipboard",
                        json!({
                            "id": account_id,
                            "field": match field {
                                CopyField::Login => "login",
                                CopyField::Password => "password",
                            },
                            "clearAfterMs": copied.clear_after.as_millis(),
                            "historyExcluded": copied.history_excluded,
                        }),
                    );
                }
                said.answer(&reply, copied);
            }
            #[cfg(feature = "fake")]
            Command::SeedSession(account_id, launcher_uid, blob, reply) => said.answer(
                &reply,
                store.and_then(|store| {
                    let account = accounts::require(store, &account_id)?;
                    let launchers = self.launchers.as_ref().ok_or_else(no_launchers)?;
                    let directory = launchers.directory(store, account.launcher);
                    Ok(ol_services::sessions::seed(
                        &directory,
                        &account,
                        launcher_uid.as_deref(),
                        &blob,
                    )?)
                }),
            ),
            Command::OnStore(work) => {
                if let Ok(store) = store {
                    work(store);
                }
            }
            Command::Drain(reply) => said.answer(&reply, Ok(())),
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

    // The machine's clock when there is one, so a fake machine's rows carry its time; else the system's.
    fn stamp(&self) -> String {
        self.launchers
            .as_ref()
            .map_or_else(ol_store::iso_now, |launchers| self.now(launchers))
    }

    // Whatever the source, the launchers seal with the backend's own vault from here on.
    fn open_vault(&mut self, source: VaultSource) {
        let slot = match source {
            VaultSource::None => VaultSlot::Unavailable,
            VaultSource::Launchers => self
                .launchers
                .as_ref()
                .map_or(VaultSlot::Unavailable, |launchers| {
                    VaultSlot::Open(Arc::clone(&launchers.vault))
                }),
            VaultSource::DataFolder(protector) => {
                let opened = OsCryptVault::open_or_create(&self.data_dir, &*protector);
                self.protector = Some(protector);
                VaultSlot::of(opened)
            }
            VaultSource::Opened(opened, protector) => {
                self.protector = Some(protector);
                VaultSlot::of(opened)
            }
        };
        self.vault.put(slot);
        if let Some(launchers) = &mut self.launchers {
            launchers.vault = Arc::clone(&self.vault) as SharedVault;
        }
    }

    // A key that did not open is tried again, so a fault of Windows that cleared, or a key that unwraps after all, shows as it is now. One that opened is never opened again.
    fn recheck_vault(&self, store: &Store) {
        let Some(protector) = &self.protector else {
            return;
        };
        if matches!(self.vault.slot(), VaultSlot::Open(_)) {
            return;
        }
        let slot = VaultSlot::of(OsCryptVault::open_or_create(&self.data_dir, &**protector));
        let opened = matches!(slot, VaultSlot::Open(_));
        self.vault.put(slot);
        if opened {
            self.store_probe(store);
        }
    }

    fn open_folder(&self, folder: Folder) -> Result<(), ApiError> {
        let refused = |message: String| ApiError {
            code: "OPEN_FAILED",
            message,
        };
        let shell = self
            .shell
            .as_ref()
            .ok_or_else(|| refused("This build of One Launch cannot open folders.".to_owned()))?;
        let path = folder.path(&self.data_dir);
        let path = path.to_str().ok_or_else(|| {
            refused("The folder's path is not text Windows can be handed.".to_owned())
        })?;
        shell
            .open_folder(path)
            .map_err(|error| refused(error.to_string()))
    }

    fn vault(&self) -> &dyn Vault {
        &*self.vault
    }

    fn vault_access(&self) -> VaultAccess<'_> {
        match self.vault.slot() {
            VaultSlot::Open(_) => VaultAccess::Open(&*self.vault),
            VaultSlot::Unavailable => VaultAccess::Unavailable,
            VaultSlot::KeyLost => VaultAccess::KeyLost,
        }
    }

    fn changed(&self, key: SettingKey, value: Option<&str>) {
        if let Some(changes) = &self.settings_watch {
            let _ = changes.try_send((key, value.map(str::to_owned)));
        }
    }

    // Only a vault whose secrets cannot be read is reset. A lost key is tried once more and then replaced first, so a reset that cannot make a new one removes nothing; a clear that fails after it leaves the old secrets beside the new key, which reads as needing a reset again. The probe is put in last, and a reset whose clear went through has succeeded whether or not it could be.
    fn reset_vault(&self, store: &Store) -> Result<VaultReset, ApiError> {
        self.require_idle()?;
        let was_lost = matches!(self.vault.slot(), VaultSlot::KeyLost);
        self.recheck_vault(store);
        match vault_health::status(store, self.vault_access())?.state() {
            EncryptionState::KeyLost => {}
            EncryptionState::Working if was_lost => return Ok(VaultReset::KeyReadable),
            EncryptionState::Working => return Err(VaultResetRefusal::Readable.into()),
            EncryptionState::Unavailable => return Err(VaultResetRefusal::Unavailable.into()),
        }
        if matches!(self.vault.slot(), VaultSlot::KeyLost) {
            let protector = self
                .protector
                .as_ref()
                .ok_or(VaultResetRefusal::Unavailable)?;
            let marked = vault_health::mark_reset_pending(store, &self.stamp())?;
            let unmark = || {
                if marked {
                    let _ = store.delete_secret(SecretKey::VaultProbe);
                }
            };
            match OsCryptVault::replace_lost_key(&self.data_dir, &**protector) {
                Ok(Recovered::Opened(vault)) => {
                    self.vault.put(VaultSlot::Open(Arc::new(vault)));
                    unmark();
                    self.store_probe(store);
                    if vault_health::status(store, self.vault_access())?.state()
                        == EncryptionState::Working
                    {
                        return Ok(VaultReset::KeyReadable);
                    }
                }
                Ok(Recovered::Replaced(vault)) => self.vault.put(VaultSlot::Open(Arc::new(vault))),
                Err(error) => {
                    unmark();
                    if matches!(error, KeyError::WindowsUnavailable) {
                        self.vault.put(VaultSlot::Unavailable);
                    }
                    return Err(ApiError {
                        code: VaultError::Unavailable.code(),
                        message: format!(
                            "No new vault key could be made, so nothing was removed: {error}"
                        ),
                    });
                }
            }
        }
        let removed = vault_health::clear_stored_secrets(store, &*self.vault, &self.stamp())?;
        self.log.child("vault").warn(
            "vault reset: all stored secrets removed",
            removed_counts(&removed),
        );
        self.store_probe(store);
        Ok(VaultReset::Cleared(removed))
    }

    fn launcher_statuses(&self, store: &Store) -> Result<Vec<LauncherStatus>, ApiError> {
        let Some(launchers) = &self.launchers else {
            return Ok(LauncherId::ALL
                .into_iter()
                .map(LauncherStatus::unreachable)
                .collect());
        };
        let configs = launchers.configs(&store.settings()?, &self.data_dir);
        Ok(LauncherId::ALL
            .into_iter()
            .map(|launcher| {
                if Launchers::IMPLEMENTED.contains(&launcher) {
                    launchers::status(store, launchers, &configs, &self.data_dir, launcher)
                } else {
                    LauncherStatus::unreachable(launcher)
                }
            })
            .collect())
    }

    // The folder of a program the user picked becomes the launcher's, once the adapter has found the launcher there.
    fn locate_launcher(
        &self,
        store: &Store,
        launcher: LauncherId,
        exe: &str,
    ) -> Result<LauncherInstall, ApiError> {
        self.require_idle()?;
        let install = self.with_adapter(store, launcher, |_, _, adapter| {
            Ok(launchers::locate(adapter, exe)?)
        })?;
        let key = SettingKey::launcher_path(launcher);
        store.set_setting(key, &install.install_dir)?;
        self.changed(key, Some(&install.install_dir));
        Ok(install)
    }

    fn reset_launcher_path(&self, store: &Store, launcher: LauncherId) -> Result<(), ApiError> {
        self.require_idle()?;
        let key = SettingKey::launcher_path(launcher);
        store.delete_setting(key)?;
        self.changed(key, None);
        Ok(())
    }

    // A switch works from the accounts as they were when it began: what changes one waits until it is done.
    fn require_idle(&self) -> Result<(), ApiError> {
        if self.jobs.is_busy() {
            return Err(ServiceError::Busy.into());
        }
        Ok(())
    }

    // Asks the adapter of one launcher, with the settings as they are now. A launcher without an adapter is refused before anything is read.
    fn with_adapter<T>(
        &self,
        store: &Store,
        launcher: LauncherId,
        ask: impl FnOnce(&Launchers, &Configs, &Adapter<'_>) -> Result<T, ApiError>,
    ) -> Result<T, ApiError> {
        require_implemented(launcher)?;
        let launchers = self.launchers.as_ref().ok_or_else(no_launchers)?;
        let configs = launchers.configs(&store.settings()?, &self.data_dir);
        let directory = launchers.directory(store, launcher);
        let adapter = launchers.adapter(launcher, &configs, &directory)?;
        ask(launchers, &configs, &adapter)
    }

    fn saved_sign_in(&self, store: &Store, account_id: &str) -> Result<SavedSignIn, ApiError> {
        let account = accounts::require(store, account_id)?;
        let launcher = account.launcher;
        let state = self
            .with_adapter(store, launcher, |_, _, adapter| Ok(adapter.session_state()))
            .ok()
            .flatten();
        let (state, state_error) = match state {
            Some(Ok(state)) => (Some(state), None),
            Some(Err(error)) => (
                None,
                Some(ServiceError::adapter(launcher)(error).to_string()),
            ),
            None => (None, None),
        };
        Ok(SavedSignIn {
            session: store.session(&account.id)?,
            state,
            state_error,
        })
    }

    fn create_account(&self, store: &Store, account: &NewAccount) -> Result<Account, ApiError> {
        Ok(accounts::create(
            store,
            self.vault(),
            account,
            &self.stamp(),
        )?)
    }

    fn rescan(&self, store: &Store) -> Result<Vec<LibraryScanResult>, ApiError> {
        let Some(launchers) = &self.launchers else {
            return Ok(Vec::new());
        };
        let configs = launchers.configs(&store.settings()?, &self.data_dir);
        let scanned = launchers::scan_library(store, launchers, &configs, &self.now(launchers));
        let log = self.log.child("scan");
        for result in &scanned {
            let launcher = result.launcher.as_str();
            match &result.error {
                Some(error) => log.warn(
                    "library scan failed for launcher",
                    json!({ "launcher": launcher, "error": error }),
                ),
                None if !result.installed => log.info(
                    "launcher not installed; library scan skipped",
                    json!({ "launcher": launcher, "removed": result.removed }),
                ),
                None => log.info(
                    "library rescanned",
                    json!({
                        "launcher": launcher,
                        "added": result.added,
                        "updated": result.updated,
                        "removed": result.removed,
                        "installed": result.installed,
                    }),
                ),
            }
        }
        Ok(scanned)
    }

    fn rescan_after_folder_change(&self, store: &Store, launcher: LauncherId) {
        if let Err(error) = self.rescan(store) {
            self.log.child("ipc").warn(
                "rescan after a launcher folder change failed",
                json!({ "launcher": launcher.as_str(), "code": error.code, "error": error.message }),
            );
        }
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
        let database: Arc<dyn StoreThread> = Arc::new(DatabaseThread(commands.clone()));
        let (board, job_id, log) = (self.jobs.clone(), job.id.clone(), self.log.clone());
        let run = move || {
            // A panic in a switch must not leave the board busy for good.
            let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                launch::run(prepared, database, &*on_job)
            }));
            let Some(finished) = ran.ok().or_else(|| launch::unwound(&board, &job_id, &log)) else {
                return;
            };
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
                *self
                    .job_thread
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(thread);
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
        self.with_adapter(store, launcher, |launchers, configs, adapter| {
            let backups = launchers.backups(&self.data_dir, configs);
            if must_be_listed
                && backups
                    .get(launcher, id)
                    .map_err(ServiceError::backup(launcher))?
                    .is_none()
            {
                return Err(ServiceError::SnapshotGone.into());
            }
            let restored = switch::restore_backup(adapter, &launchers.platform, &backups, id);
            if let Err(error) = &restored {
                launchers::log_adapter_failure(&launchers.log, error, "restoreBackup");
            }
            Ok(restored?)
        })
    }

    fn pending_recovery(&self, store: &Store) -> Result<Option<SwitchRecovery>, ApiError> {
        let Some(entry) = self.recovery.pending() else {
            return Ok(None);
        };
        let Some(backup_id) = entry.backup_id.clone() else {
            return Ok(None);
        };
        let snapshot_available = self
            .with_adapter(store, entry.launcher, |launchers, configs, _| {
                Ok(launchers
                    .backups(&self.data_dir, configs)
                    .get(entry.launcher, &backup_id)
                    .is_ok_and(|snapshot| snapshot.is_some()))
            })
            .unwrap_or(false);
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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use futures_lite::future::block_on;
    use ol_model::LauncherId;
    use ol_platform_api::{CancelToken, Clock, Platform};
    use ol_platform_fake::{
        FakeClock, FakeProcesses, FakeRegistry, FakeShell, OpLog, ProcessKnobs, ScratchFiles,
    };
    use ol_testkit::Scratch;
    use ol_vault::FakeVault;
    use time::OffsetDateTime;
    use time::macros::datetime;

    use super::*;

    type Trip = Box<dyn FnOnce() + Send>;

    // This user's DPAPI, and another user's, which unwraps only what it wrapped itself.
    struct Mine;

    impl DataProtector for Mine {
        fn protect(&self, plain: &[u8]) -> Result<Vec<u8>, ol_platform_api::ProtectError> {
            Ok(plain.iter().map(|byte| byte ^ 0x5a).collect())
        }

        fn unprotect(
            &self,
            protected: &[u8],
        ) -> Result<ol_platform_api::Zeroizing<Vec<u8>>, ol_platform_api::ProtectError> {
            Ok(ol_platform_api::Zeroizing::new(
                protected.iter().map(|byte| byte ^ 0x5a).collect(),
            ))
        }
    }

    struct Theirs;

    impl DataProtector for Theirs {
        fn protect(&self, plain: &[u8]) -> Result<Vec<u8>, ol_platform_api::ProtectError> {
            Ok([b"theirs".as_slice(), plain].concat())
        }

        fn unprotect(
            &self,
            protected: &[u8],
        ) -> Result<ol_platform_api::Zeroizing<Vec<u8>>, ol_platform_api::ProtectError> {
            protected
                .strip_prefix(b"theirs".as_slice())
                .map(|plain| ol_platform_api::Zeroizing::new(plain.to_vec()))
                .ok_or(ol_platform_api::ProtectError::Unprotect)
        }
    }

    // The key is replaced and then the clear cannot write: the old secrets stay beside the new key, which reads as a lost key again, and a second reset finishes the job.
    #[test]
    fn a_reset_whose_clear_fails_after_the_key_was_replaced_is_finished_by_a_second_one() {
        let scratch = Scratch::new("api-reset");
        let data_dir = scratch.join("data");
        let backend =
            Backend::start_with_vault_of_data_folder(data_dir.clone(), "9.8.7", Box::new(Mine));
        block_on(backend.create_account(NewAccount {
            launcher: LauncherId::Steam,
            display_name: "Main".to_owned(),
            login: "user_main".to_owned(),
            launcher_uid: None,
            password: Some(Secret::new("placeholder".to_owned())),
            tags: Vec::new(),
            notes: String::new(),
        }))
        .unwrap();
        backend.shut_down();
        let key = std::fs::read(ol_vault::local_state_path(&data_dir)).unwrap();

        let (commands, _requests) = async_channel::unbounded();
        let mut serving = Serving {
            data_dir: data_dir.clone(),
            launchers: None,
            vault: Arc::default(),
            protector: None,
            shell: None,
            settings_watch: None,
            jobs: JobBoard::default(),
            on_job: Arc::new(|_| {}),
            commands: commands.downgrade(),
            job_thread: Arc::default(),
            last_preflight: LastPreflight::default(),
            recovery: Recovery::default(),
            clipboard: None,
            log: Log::off(),
        };
        serving.open_vault(VaultSource::DataFolder(Box::new(Theirs)));
        let database = ol_store::database_path(&data_dir);
        let read_only = Store::open_read_only(&database).unwrap().store;
        assert!(serving.reset_vault(&read_only).is_err());
        assert!(
            std::fs::read(ol_vault::local_state_path(&data_dir)).unwrap() != key,
            "the key was not replaced before the clear"
        );
        let status = vault_health::status(&read_only, serving.vault_access()).unwrap();
        assert_eq!(status.state(), EncryptionState::KeyLost);
        drop(read_only);

        let (store, _) = Store::open(&database).unwrap();
        let removed = serving.reset_vault(&store).unwrap().summary().unwrap();
        assert_eq!((removed.passwords, removed.secrets), (1, 1));
        let status = vault_health::status(&store, serving.vault_access()).unwrap();
        assert_eq!(status.state(), EncryptionState::Working);
        assert_eq!(status.readable, Some(true));
    }

    // A clock that runs something once, the next time it is read.
    struct Tripwire {
        clock: FakeClock,
        trip: Mutex<Option<Trip>>,
    }

    impl Clock for Tripwire {
        fn now(&self) -> OffsetDateTime {
            let trip = self.trip.lock().unwrap().take();
            if let Some(trip) = trip {
                trip();
            }
            self.clock.now()
        }

        fn sleep(&self, duration: Duration, cancel: &CancelToken) {
            self.clock.sleep(duration, cancel);
        }
    }

    #[test]
    fn a_quit_that_lands_while_a_start_is_prepared_refuses_that_start() {
        let scratch = Scratch::new("api-quit");
        let log = OpLog::new();
        let clock = Arc::new(Tripwire {
            clock: FakeClock::starting_at(datetime!(2026-11-06 16:46:23 UTC)),
            trip: Mutex::new(None),
        });
        let platform = Platform {
            registry: Arc::new(FakeRegistry::new(log.clone())),
            processes: Arc::new(FakeProcesses::new(log.clone(), ProcessKnobs::default())),
            files: Arc::new(ScratchFiles::new(&[scratch.path()], log.clone())),
            shell: Arc::new(FakeShell::new(log)),
            clock: Arc::clone(&clock) as Arc<dyn Clock>,
        };
        let backend = Backend::start_with(
            scratch.join("data"),
            "9.8.7",
            Launchers::on(platform, Arc::new(FakeVault)),
            None,
        );
        let account = block_on(backend.create_account(NewAccount {
            launcher: LauncherId::Steam,
            display_name: "Main".to_owned(),
            login: "user_main".to_owned(),
            launcher_uid: None,
            password: None,
            tags: Vec::new(),
            notes: String::new(),
        }))
        .unwrap();
        let jobs = backend.jobs.clone();
        *clock.trip.lock().unwrap() = Some(Box::new(move || jobs.close()));

        let started = block_on(backend.start_launch(StartLaunch {
            account_id: account.id,
            game_id: None,
            confirm_close: true,
        }));
        assert_eq!(started.err().map(|refused| refused.code), Some("BUSY"));
        assert!(!backend.jobs.is_busy());
        backend.shut_down();
    }
}
