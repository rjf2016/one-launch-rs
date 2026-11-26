// What finding a launcher, switching its account and starting a game hand back. Whatever names an account or a folder has no Debug output.

use crate::LauncherId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallSource {
    User,
    RegistryHkcu,
    RegistryHklm,
    LauncherFile,
    DefaultPath,
}

impl InstallSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::RegistryHkcu => "registry-hkcu",
            Self::RegistryHklm => "registry-hklm",
            Self::LauncherFile => "launcher-file",
            Self::DefaultPath => "default-path",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct LauncherInstall {
    pub launcher: LauncherId,
    pub install_dir: String,
    pub exe_path: String,
    pub source: InstallSource,
}

// A game as the launcher's own files describe it, before the library gives it an id.
#[derive(Clone, PartialEq, Eq)]
pub struct InstalledGame {
    pub launcher: LauncherId,
    pub launcher_game_id: String,
    pub name: String,
    pub install_path: String,
    pub library_path: Option<String>,
    pub size_on_disk_bytes: Option<i64>,
    pub last_played_at: Option<String>,
    pub needs_update: bool,
}

// An account the launcher itself remembers: names and ids only, never a password or a session.
#[derive(Clone, PartialEq, Eq)]
pub struct RememberedAccount {
    pub launcher: LauncherId,
    pub launcher_uid: String,
    pub login: String,
    pub persona_name: String,
    pub remember_password: bool,
    pub most_recent: bool,
    pub last_login_at: Option<String>,
    pub session_may_be_expired: bool,
    pub linked_account_id: Option<String>,
}

// A saved sign-in as anyone but the vault may see it: whose it is and the SHA-256 of what it holds, never the sign-in itself.
#[derive(Clone, PartialEq, Eq)]
pub struct LauncherSession {
    pub account_id: String,
    pub launcher: LauncherId,
    pub launcher_uid: Option<String>,
    pub fingerprint: String,
    pub captured_at: String,
    pub verified_at: Option<String>,
    // Why the launcher refused it, until it is saved or confirmed again.
    pub stale_reason: Option<String>,
}

// How the launcher's last sign-in ended, by its own log.
#[derive(Clone, PartialEq, Eq)]
pub struct LastSignIn {
    pub ok: bool,
    pub reason: Option<String>,
}

// What a launcher's own files say about the session it would sign in from now.
#[derive(Clone, PartialEq, Eq)]
pub struct LauncherSessionState {
    pub launcher: LauncherId,
    pub remembered: bool,
    pub fingerprint: Option<String>,
    pub launcher_uid: Option<String>,
    pub linked_account_id: Option<String>,
    pub launcher_running: bool,
    pub last_sign_in: Option<LastSignIn>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchStepKind {
    Process,
    Backup,
    File,
    Registry,
    Launch,
    Wait,
}

impl SwitchStepKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Process => "process",
            Self::Backup => "backup",
            Self::File => "file",
            Self::Registry => "registry",
            Self::Launch => "launch",
            Self::Wait => "wait",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct SwitchChange {
    pub place: String,
    pub before: String,
    pub after: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct SwitchPlanStep {
    pub id: &'static str,
    pub kind: SwitchStepKind,
    pub title: String,
    pub target: String,
    pub detail: String,
    pub changes: Vec<SwitchChange>,
    pub skipped: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchStrategy {
    Remembered,
    Credentials,
}

impl SwitchStrategy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Remembered => "remembered",
            Self::Credentials => "credentials",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PlanAccount {
    pub id: Option<String>,
    pub display_name: String,
    pub login: String,
    pub launcher_uid: Option<String>,
}

// Every process, file and registry value a switch would touch, read without changing any of them.
#[derive(Clone, PartialEq, Eq)]
pub struct SwitchPlan {
    pub launcher: LauncherId,
    pub account: PlanAccount,
    pub strategy: SwitchStrategy,
    pub most_recent_login: Option<String>,
    pub auto_login_user: Option<String>,
    pub running: bool,
    pub steps: Vec<SwitchPlanStep>,
    pub warnings: Vec<String>,
    pub notes: Vec<String>,
    pub executable: bool,
    pub generated_at: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunningGameSource {
    Launcher,
    Process,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunningGame {
    pub launcher: LauncherId,
    pub launcher_game_id: String,
    pub name: String,
    pub source: RunningGameSource,
}

// What the launch dialog needs before it asks the user to confirm.
#[derive(Clone, PartialEq, Eq)]
pub struct LaunchPreflight {
    pub plan: SwitchPlan,
    pub dry_run: bool,
    pub launcher_running: bool,
    pub running_game: Option<RunningGame>,
    pub already_active: bool,
    pub needs_confirmation: bool,
    pub close_message: Option<String>,
    pub blocked: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobStepStatus {
    Pending,
    Running,
    Done,
    Skipped,
    Failed,
}

impl JobStepStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Done => "done",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct JobStep {
    pub id: &'static str,
    pub title: String,
    pub status: JobStepStatus,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobState {
    Running,
    Done,
    Failed,
    NeedsSignIn,
    Cancelled,
}

impl JobState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::NeedsSignIn => "needs-sign-in",
            Self::Cancelled => "cancelled",
        }
    }
}

// The switch waits for the user in the launcher's own window.
#[derive(Clone, PartialEq, Eq)]
pub struct LaunchAttention {
    pub title: String,
    pub message: String,
    pub steps: Vec<String>,
    pub reason: Option<String>,
    pub can_copy_password: bool,
    pub until: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct JobAccount {
    pub id: String,
    pub display_name: String,
    pub login: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobGame {
    pub id: String,
    pub name: String,
    pub launcher_game_id: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct LaunchJob {
    pub id: String,
    pub state: JobState,
    pub launcher: LauncherId,
    pub account: JobAccount,
    pub game: Option<JobGame>,
    pub steps: Vec<JobStep>,
    pub attention: Option<LaunchAttention>,
    // The launcher already ran as the account, so the job went straight to the game: nothing written, copied or journalled.
    pub direct: bool,
    pub switched: bool,
    // The launcher was seen starting the game, not only handed it.
    pub game_seen: bool,
    pub hand_off: Option<String>,
    pub backup_id: Option<String>,
    pub restored: Option<bool>,
    pub error: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchResult {
    Ok,
    Failed,
    Cancelled,
}

impl LaunchResult {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

// A game and account pair that was launched before, the newest launch of the pair.
#[derive(Clone, PartialEq, Eq)]
pub struct RecentLaunch {
    pub game_id: String,
    pub game_name: String,
    pub launcher: LauncherId,
    pub account_id: String,
    pub account_name: String,
    pub launched_at: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct BackupSnapshot {
    pub id: String,
    pub launcher: LauncherId,
    pub created_at: String,
    pub reason: String,
    pub files: Vec<String>,
    pub bytes: u64,
    pub pinned: Option<String>,
    pub path: String,
    pub holds_registry: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub struct RestoreResult {
    pub launcher: LauncherId,
    pub id: String,
    pub files: Vec<String>,
    pub registry: Vec<String>,
}

// One launcher's share of a library scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryScanResult {
    pub launcher: LauncherId,
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub installed: bool,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryPhase {
    Writing,
    SigningIn,
    RollbackFailed,
}

impl RecoveryPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Writing => "writing",
            Self::SigningIn => "signing-in",
            Self::RollbackFailed => "rollback-failed",
        }
    }
}

// A switch the last run did not finish, as the journal left it.
#[derive(Clone, PartialEq, Eq)]
pub struct SwitchRecovery {
    pub job_id: String,
    pub launcher: LauncherId,
    pub account_name: Option<String>,
    pub started_at: String,
    pub backup_id: String,
    pub snapshot_available: bool,
    pub phase: RecoveryPhase,
}
