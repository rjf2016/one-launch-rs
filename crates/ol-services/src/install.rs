// What Start with Windows and the updater can do for this copy. Both need a copy the installer put in place (slice 6); until then every copy is a development copy, and both say so.

use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateUnavailable {
    Development,
    CustomDataFolder,
    NotInstalled,
    AllUsersInstall,
}

impl UpdateUnavailable {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::CustomDataFolder => "custom-data-folder",
            Self::NotInstalled => "not-installed",
            Self::AllUsersInstall => "all-users-install",
        }
    }

    // The hint under the Version row.
    pub const fn hint(self) -> &'static str {
        match self {
            Self::Development => "Updates work only in the installed app.",
            Self::CustomDataFolder => {
                "Updates are off while One Launch runs from a custom data folder."
            }
            Self::NotInstalled => {
                "Updates work only in a copy installed with the One Launch installer."
            }
            Self::AllUsersInstall => {
                "Updates need a per-user install. Reinstall and choose \"Only for me\"."
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateStep {
    Check,
    Download,
    Verify,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateStatus {
    Unavailable(UpdateUnavailable),
    Idle,
    Checking,
    UpToDate { checked_at: String },
    Downloading { version: String, percent: u8 },
    Ready { version: String },
    Failed { step: UpdateStep, message: String },
}

impl UpdateStatus {
    // After the version in the Version row, "<version> · <this>".
    pub fn state_text(&self) -> String {
        match self {
            Self::Unavailable(_) => "Off for this copy".to_owned(),
            Self::Idle => "Not checked yet".to_owned(),
            Self::Checking => "Checking…".to_owned(),
            Self::UpToDate { .. } => "Up to date".to_owned(),
            Self::Downloading { version, percent } => format!("Downloading {version} · {percent}%"),
            Self::Ready { version } => format!("{version} is ready"),
            Self::Failed { step, .. } => match step {
                UpdateStep::Check => "Check failed",
                UpdateStep::Download => "Download failed",
                UpdateStep::Verify => "Not verified",
            }
            .to_owned(),
        }
    }

    // Whether "Check for updates" can be pressed.
    pub const fn can_check(&self) -> bool {
        !matches!(
            self,
            Self::Unavailable(_) | Self::Checking | Self::Downloading { .. }
        )
    }
}

// Start with Windows, as Settings shows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AutostartStatus {
    pub available: bool,
    pub open_at_login: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum AutostartError {
    #[error("Start with Windows is not available for this copy of One Launch")]
    Unavailable,
}

impl AutostartError {
    pub const fn code(self) -> &'static str {
        "UNAVAILABLE"
    }
}

// No copy is installed yet: the one rule of the Electron app that every copy of this build meets first is that it was not packaged.
pub const fn this_copy() -> Result<(), UpdateUnavailable> {
    Err(UpdateUnavailable::Development)
}

pub const fn update_status() -> UpdateStatus {
    match this_copy() {
        Err(reason) => UpdateStatus::Unavailable(reason),
        Ok(()) => UpdateStatus::Idle,
    }
}

pub const fn autostart_status() -> AutostartStatus {
    AutostartStatus {
        available: this_copy().is_ok(),
        open_at_login: false,
    }
}

// The login item itself comes with the installer: until then no copy is one Windows may start at sign-in.
pub const fn set_autostart(_open_at_login: bool) -> Result<AutostartStatus, AutostartError> {
    Err(AutostartError::Unavailable)
}

// What the Build row says.
pub const fn build_label() -> &'static str {
    match this_copy() {
        Err(UpdateUnavailable::Development) => "Development",
        _ => "Packaged",
    }
}
