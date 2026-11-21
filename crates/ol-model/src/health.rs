// What the Launchers tab says of each launcher: where it is, whether it runs, and how its files look to One Launch. Every detail can name a folder, so none of these has Debug output.

use crate::{LauncherId, LauncherInstall};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthStatus {
    Ok,
    Warn,
    Fail,
    // Not installed, which is no problem.
    Absent,
}

impl HealthStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Fail => "fail",
            Self::Absent => "absent",
        }
    }

    // The pill of the launcher's row.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ok => "Healthy",
            Self::Warn => "Warnings",
            Self::Fail => "Problems",
            Self::Absent => "Not installed",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct HealthCheck {
    pub id: &'static str,
    pub label: &'static str,
    pub status: HealthStatus,
    pub detail: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct HealthReport {
    pub launcher: LauncherId,
    pub status: HealthStatus,
    pub checks: Vec<HealthCheck>,
    pub checked_at: String,
}

impl HealthReport {
    // Fail when one check fails, else warn when one warns, else ok.
    pub fn of(launcher: LauncherId, checks: Vec<HealthCheck>, checked_at: String) -> Self {
        let has = |status| checks.iter().any(|check| check.status == status);
        let status = if has(HealthStatus::Fail) {
            HealthStatus::Fail
        } else if has(HealthStatus::Warn) {
            HealthStatus::Warn
        } else {
            HealthStatus::Ok
        };
        Self {
            launcher,
            status,
            checks,
            checked_at,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct LauncherStatus {
    pub launcher: LauncherId,
    pub display_name: &'static str,
    // False in a build that cannot reach this launcher: everything below is then empty.
    pub implemented: bool,
    pub install: Option<LauncherInstall>,
    // The folder stored with Locate, as the adapter reads it.
    pub chosen_path: Option<String>,
    // None while not installed, or when no listing was taken.
    pub running: Option<bool>,
    pub health: Option<HealthReport>,
    // Why the status could not be taken whole: the message of what failed.
    pub attention: Option<String>,
}

impl LauncherStatus {
    pub const fn unreachable(launcher: LauncherId) -> Self {
        Self {
            launcher,
            display_name: launcher.display_name(),
            implemented: false,
            install: None,
            chosen_path: None,
            running: None,
            health: None,
            attention: None,
        }
    }

    // The line under an open row that says where the launcher was found.
    pub fn location_note(&self) -> String {
        let exe = self.launcher.exe_name();
        match (&self.install, &self.chosen_path) {
            (Some(install), _) if install.source == crate::InstallSource::User => {
                "You chose this folder.".to_owned()
            }
            (_, Some(_)) => format!("{exe} is no longer in the folder you chose."),
            (Some(_), None) => "Found automatically.".to_owned(),
            (None, None) => format!("Not found. If it is installed, point One Launch at {exe}."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(status: HealthStatus) -> HealthCheck {
        HealthCheck {
            id: "x",
            label: "X",
            status,
            detail: String::new(),
        }
    }

    #[test]
    fn a_report_is_as_bad_as_its_worst_check() {
        let report = |statuses: &[HealthStatus]| {
            HealthReport::of(
                LauncherId::Steam,
                statuses.iter().copied().map(check).collect(),
                String::new(),
            )
            .status
        };
        assert_eq!(report(&[]), HealthStatus::Ok);
        assert_eq!(
            report(&[HealthStatus::Ok, HealthStatus::Absent]),
            HealthStatus::Ok
        );
        assert_eq!(
            report(&[HealthStatus::Warn, HealthStatus::Ok]),
            HealthStatus::Warn
        );
        assert_eq!(
            report(&[HealthStatus::Warn, HealthStatus::Fail, HealthStatus::Ok]),
            HealthStatus::Fail
        );
    }
}
