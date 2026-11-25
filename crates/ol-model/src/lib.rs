#![forbid(unsafe_code)]

mod account;
mod game;
mod health;
mod launcher;
mod preference;
pub mod redact;
mod secret;
mod settings;
mod switching;

pub use account::Account;
pub use game::{ArtworkSource, Game, GameArtwork, LastLaunch};
pub use health::{HealthCheck, HealthReport, HealthStatus, LauncherStatus};
pub use launcher::LauncherId;
pub use preference::{LibrarySort, Sidebar};
pub use secret::{Ciphertext, SecretKey};
pub use settings::{
    BACKUPS_KEEP_CHOICES, BATTLENET_LAUNCH_CHOICES, DEFAULT_BACKUPS_TO_KEEP,
    EPIC_LAUNCH_URI_CHOICES, LogLevel, MAX_BACKUPS_TO_KEEP, MAX_SETTING_LENGTH, SettingKey,
    SettingRefusal, Settings, check_setting, is_version,
};
pub use switching::{
    BackupSnapshot, InstallSource, InstalledGame, JobAccount, JobGame, JobState, JobStep,
    JobStepStatus, LastSignIn, LaunchAttention, LaunchJob, LaunchPreflight, LaunchResult,
    LauncherInstall, LauncherSession, LauncherSessionState, LibraryScanResult, PlanAccount,
    RecentLaunch, RecoveryPhase, RememberedAccount, RestoreResult, RunningGame, RunningGameSource,
    SwitchChange, SwitchPlan, SwitchPlanStep, SwitchRecovery, SwitchStepKind, SwitchStrategy,
};

const REDACTED: &str = "[REDACTED]";
