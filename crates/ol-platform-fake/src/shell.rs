use ol_platform_api::{Shell, ShellError};

use crate::OpLog;

// Opens nothing: a link handed to it is written down.
pub struct FakeShell {
    log: OpLog,
}

impl FakeShell {
    pub fn new(log: OpLog) -> Self {
        Self { log }
    }
}

impl Shell for FakeShell {
    fn open_url(&self, url: &str) -> Result<(), ShellError> {
        self.log.record("shell.open", [("url", url.into())]);
        Ok(())
    }

    fn open_folder(&self, folder: &str) -> Result<(), ShellError> {
        self.log.record("shell.folder", [("path", folder.into())]);
        Ok(())
    }
}
