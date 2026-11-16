// All the Riot adapter may do with a process: ask which image names run, start the client, ask the client's windows to close, and end the client alone. There is no way through it to a process's path or handle, and none to close or end anything but the client's two images: a Riot game and Vanguard are never touched, and a game the client started is not taken down with it.

use ol_formats::riot::products::image_key;
use ol_platform_api::{ProcessArg, Processes};

use crate::AdapterError;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ClientImage {
    // The client itself, one process.
    Core,
    // Its window processes, which the client leaves behind when it goes.
    Window,
}

impl ClientImage {
    pub const fn exe(self) -> &'static str {
        match self {
            Self::Core => "RiotClientServices.exe",
            Self::Window => "Riot Client.exe",
        }
    }
}

pub(super) struct ClientProcesses<'a>(&'a dyn Processes);

impl<'a> ClientProcesses<'a> {
    pub(super) const fn new(processes: &'a dyn Processes) -> Self {
        Self(processes)
    }

    pub(super) fn is_running(&self, image: ClientImage) -> Result<bool, AdapterError> {
        Ok(self.0.is_running(image.exe())?)
    }

    // The image names that run, in lower case and without `.exe`. A listing that fails is an error, never an empty list.
    pub(super) fn names(&self) -> Result<Vec<String>, AdapterError> {
        let names = self.0.running_names()?;
        Ok(names.iter().map(|name| image_key(name)).collect())
    }

    pub(super) fn start(&self, exe: &str, args: &[String], cwd: &str) -> Result<(), AdapterError> {
        let args: Vec<ProcessArg<'_>> = args.iter().map(|arg| ProcessArg::Plain(arg)).collect();
        Ok(self.0.start(exe, &args, cwd)?)
    }

    pub(super) fn close(&self, image: ClientImage) -> Result<(), AdapterError> {
        Ok(self.0.close(image.exe())?)
    }

    // Ends the image's own processes and nothing they started.
    pub(super) fn end(&self, image: ClientImage) -> Result<(), AdapterError> {
        Ok(self.0.kill(image.exe(), false)?)
    }
}
