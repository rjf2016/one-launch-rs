// What the health checks of the four adapters share. A check reads and never writes, and never shows a session. A detail is in the Electron app's words, and so can name an account (Battle.net's "belongs to <display name>"), give the last four characters of a launcher's id, and give a folder that holds the Windows user name: it is redacted like any other text before it is logged.

use ol_formats::winpath::file_name;
use ol_model::{HealthCheck, HealthStatus, InstallSource, LauncherInstall};

// What the status pass already knows, so the health check reads nothing twice.
#[derive(Clone, Default)]
pub struct Known {
    pub install: Option<LauncherInstall>,
    pub running: Option<bool>,
}

#[derive(Default)]
pub(crate) struct Checks(pub(crate) Vec<HealthCheck>);

impl Checks {
    pub(crate) fn add(
        &mut self,
        id: &'static str,
        label: &'static str,
        status: HealthStatus,
        detail: impl Into<String>,
    ) {
        self.0.push(HealthCheck {
            id,
            label,
            status,
            detail: detail.into(),
        });
    }

    pub(crate) fn push(&mut self, check: Option<HealthCheck>) {
        self.0.extend(check);
    }

    // The process check: what the status pass saw, or a listing of its own.
    pub(crate) fn process<E: std::fmt::Display>(
        &mut self,
        exe: &str,
        running: Result<bool, E>,
    ) -> Option<bool> {
        match running {
            Ok(running) => {
                let state = if running {
                    "is running"
                } else {
                    "is not running"
                };
                self.add(
                    "process",
                    "Process",
                    HealthStatus::Ok,
                    format!("{exe} {state}"),
                );
                Some(running)
            }
            Err(error) => {
                self.add(
                    "process",
                    "Process",
                    HealthStatus::Warn,
                    format!("Could not query the process list: {error}"),
                );
                None
            }
        }
    }
}

// Whether a file is named one of these: Windows does not tell case apart in a file name.
pub(crate) fn is_named(exe: &str, names: &[&str]) -> bool {
    let name = file_name(exe).to_lowercase();
    names.iter().any(|known| known.to_lowercase() == name)
}

// A warning for a folder chosen with Locate that the launcher is no longer in. None while that choice is in use or none was made.
pub fn chosen_folder_check(
    chosen: Option<&str>,
    install: Option<&LauncherInstall>,
    exe: &str,
) -> Option<HealthCheck> {
    let chosen = chosen?;
    if install.is_some_and(|install| install.source == InstallSource::User) {
        return None;
    }
    let used = if install.is_some() {
        "the installation found automatically is used"
    } else {
        "nothing is used"
    };
    Some(HealthCheck {
        id: "chosen-folder",
        label: "Chosen folder",
        status: HealthStatus::Warn,
        detail: format!(
            "{exe} is no longer in {chosen}, the folder chosen in Settings, so {used}. Use Locate to choose it again, or Reset to go back to automatic detection."
        ),
    })
}

// Where Epic, Battle.net and Riot say an install came from.
pub(crate) const fn source_label(source: InstallSource) -> &'static str {
    match source {
        InstallSource::User => "the folder chosen in Settings",
        InstallSource::DefaultPath => "default location",
        InstallSource::RegistryHkcu => "HKCU",
        InstallSource::RegistryHklm | InstallSource::LauncherFile => "HKLM",
    }
}
