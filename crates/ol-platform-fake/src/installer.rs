use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use ol_platform_api::InstallerStart;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallerAsked {
    pub installer: PathBuf,
    pub args: Vec<String>,
}

// Starts nothing: each start is written down and answered as the test chose.
pub struct RecordingInstaller {
    starts: bool,
    asked: Mutex<Vec<InstallerAsked>>,
}

impl RecordingInstaller {
    pub fn new() -> Self {
        Self::answering(true)
    }

    // A start that Windows refused.
    pub fn refusing() -> Self {
        Self::answering(false)
    }

    fn answering(starts: bool) -> Self {
        Self {
            starts,
            asked: Mutex::default(),
        }
    }

    pub fn asked(&self) -> Vec<InstallerAsked> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Default for RecordingInstaller {
    fn default() -> Self {
        Self::new()
    }
}

impl InstallerStart for RecordingInstaller {
    fn start(&self, installer: &Path, args: &[&str]) -> bool {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(InstallerAsked {
                installer: installer.to_path_buf(),
                args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            });
        self.starts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_start_is_written_down_and_answered_as_chosen() {
        let started = RecordingInstaller::new();
        assert!(started.start(Path::new("setup.exe"), &["--updated", "/S"]));
        assert_eq!(
            started.asked(),
            [InstallerAsked {
                installer: PathBuf::from("setup.exe"),
                args: vec!["--updated".to_owned(), "/S".to_owned()],
            }]
        );

        let refused = RecordingInstaller::refusing();
        assert!(!refused.start(Path::new("setup.exe"), &[]));
        assert_eq!(refused.asked().len(), 1);
    }
}
