#![forbid(unsafe_code)]

mod account;
mod game;
mod launcher;
mod preference;
mod secret;
mod settings;
mod switching;

pub use account::Account;
pub use game::{ArtworkSource, Game, GameArtwork, LastLaunch};
pub use launcher::LauncherId;
pub use preference::LibrarySort;
pub use secret::{Ciphertext, SecretKey};
pub use settings::{SettingKey, Settings};
pub use switching::{
    BackupSnapshot, InstallSource, InstalledGame, JobAccount, JobGame, JobState, JobStep,
    JobStepStatus, LaunchAttention, LaunchJob, LaunchPreflight, LaunchResult, LauncherInstall,
    LibraryScanResult, PlanAccount, RecentLaunch, RecoveryPhase, RememberedAccount, RestoreResult,
    RunningGame, RunningGameSource, SwitchChange, SwitchPlan, SwitchPlanStep, SwitchRecovery,
    SwitchStepKind, SwitchStrategy,
};

const REDACTED: &str = "[REDACTED]";
