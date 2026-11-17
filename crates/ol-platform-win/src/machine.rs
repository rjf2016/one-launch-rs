// This PC behind the ports, for the build that reaches the real launchers.

use std::sync::Arc;

use ol_platform_api::Platform;

use crate::clock::SystemClock;
use crate::files::WindowsFiles;
use crate::processes::WindowsProcesses;
use crate::registry::WindowsRegistry;
use crate::shell::WindowsShell;

pub fn platform() -> Platform {
    Platform {
        registry: Arc::new(WindowsRegistry),
        processes: Arc::new(WindowsProcesses::new()),
        files: Arc::new(WindowsFiles),
        shell: Arc::new(WindowsShell),
        clock: Arc::new(SystemClock),
    }
}

// The folders the launchers keep their files in, from the environment as the Electron app reads them. One that is not set is left to the adapter's own fallback.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Folders {
    pub app_data: Option<String>,
    pub local_app_data: Option<String>,
    pub program_data: Option<String>,
}

#[expect(
    clippy::disallowed_methods,
    reason = "the environment is read here, in ol-platform-win"
)]
fn variable(name: &str) -> Option<String> {
    std::env::var_os(name)
        .map(|value| value.to_string_lossy().into_owned())
        .filter(|value| !value.trim().is_empty())
}

pub fn folders() -> Folders {
    Folders {
        app_data: variable("APPDATA"),
        local_app_data: variable("LOCALAPPDATA"),
        program_data: variable("ProgramData"),
    }
}
