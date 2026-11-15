use std::fmt;

use ol_model::{LauncherId, RunningGame};
use ol_platform_api::{ProcessError, RegistryError, ShellError};
use ol_runtime::Aborted;
use thiserror::Error;

// Why a launcher adapter could not do what it was asked. The codes are the Electron app's, which the interface acts on.
#[derive(Clone, PartialEq, Eq, Error)]
pub enum AdapterError {
    #[error("{} is not installed on this PC", .0.as_str())]
    NotInstalled(LauncherId),
    #[error("{message}: {path}")]
    LauncherFile { path: String, message: String },
    // The switch would close the launcher, and maybe a game, and the caller has not said yes.
    #[error("{message}")]
    ConfirmationRequired {
        message: String,
        running_game: Option<RunningGame>,
    },
    #[error("{0}")]
    LauncherRunning(String),
    #[error("{0}")]
    SwitchBlocked(String),
    #[error(transparent)]
    Registry(#[from] RegistryError),
    #[error(transparent)]
    Process(#[from] ProcessError),
    #[error("The switch was stopped")]
    Aborted,
    #[error("{0}")]
    Failed(String),
}

impl AdapterError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NotInstalled(_) => "NOT_INSTALLED",
            Self::LauncherFile { .. } => "LAUNCHER_FILE",
            Self::ConfirmationRequired { .. } => "CONFIRM_REQUIRED",
            Self::LauncherRunning(_) => "LAUNCHER_RUNNING",
            Self::SwitchBlocked(_) => "SWITCH_BLOCKED",
            Self::Registry(_) => RegistryError::CODE,
            Self::Process(ProcessError::AccessDenied(_)) => "PROCESS_ACCESS_DENIED",
            Self::Aborted => "ABORTED",
            Self::Process(ProcessError::Failed(_)) | Self::Failed(_) => "ADAPTER_FAILED",
        }
    }

    // A refusal the app expects and explains before anything is touched, as opposed to something going wrong.
    pub fn is_refusal(&self) -> bool {
        matches!(
            self.code(),
            "CONFIRM_REQUIRED"
                | "LAUNCHER_RUNNING"
                | "NOT_INSTALLED"
                | "PROCESS_ACCESS_DENIED"
                | "SWITCH_BLOCKED"
        )
    }

    pub(crate) fn io(error: &std::io::Error) -> Self {
        Self::Failed(error.to_string())
    }
}

// The message can hold a path or an account's name, so only the code is printed.
impl fmt::Debug for AdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AdapterError({})", self.code())
    }
}

impl From<Aborted> for AdapterError {
    fn from(_: Aborted) -> Self {
        Self::Aborted
    }
}

impl From<ShellError> for AdapterError {
    fn from(error: ShellError) -> Self {
        Self::Failed(error.to_string())
    }
}
