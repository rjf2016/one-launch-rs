// What the app asks of the machine to find, switch and start a launcher. The adapters reach a launcher's files, the registry and the process list through these and no other way, so a build with fakes behind them touches no real launcher.

use std::fmt;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use thiserror::Error;
use time::OffsetDateTime;
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Hive {
    Hkcu,
    Hklm,
}

impl Hive {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hkcu => "HKCU",
            Self::Hklm => "HKLM",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct RegistryError(pub String);

impl RegistryError {
    pub const CODE: &'static str = "REGISTRY";
}

// What a registry value holds. No Debug: a binary value can be a launcher's sign-in.
#[derive(Clone, PartialEq, Eq)]
pub enum RegistryData {
    Text(String),
    Dword(u32),
    // The bytes as hex digits.
    Binary(String),
}

#[derive(Clone, PartialEq, Eq)]
pub struct RegistryNamedValue {
    pub name: String,
    pub data: RegistryData,
}

pub trait RegistryReader: Send + Sync {
    fn get_string(
        &self,
        hive: Hive,
        key: &str,
        name: &str,
    ) -> Result<Option<String>, RegistryError>;
    fn get_dword(&self, hive: Hive, key: &str, name: &str) -> Result<Option<u32>, RegistryError>;
    // Every value directly under a key. A key that is not there has none.
    fn list_values(&self, hive: Hive, key: &str) -> Result<Vec<RegistryNamedValue>, RegistryError>;
    // The keys below `key`, at any depth, with a text value that holds `text` in any case, each by its path below `key`. The whole tree is never listed: under Uninstall that takes seconds.
    fn keys_holding(&self, hive: Hive, key: &str, text: &str)
    -> Result<Vec<String>, RegistryError>;
}

// Only the switch pipeline is handed one of these.
pub trait RegistryWriter: Send + Sync {
    fn set_string(
        &self,
        hive: Hive,
        key: &str,
        name: &str,
        value: &str,
    ) -> Result<(), RegistryError>;
    fn set_dword(&self, hive: Hive, key: &str, name: &str, value: u32)
    -> Result<(), RegistryError>;
    // `hex` is the value's bytes as hex digits. An error never repeats them.
    fn set_binary(&self, hive: Hive, key: &str, name: &str, hex: &str)
    -> Result<(), RegistryError>;
    // A value that is not there counts as removed.
    fn delete_value(&self, hive: Hive, key: &str, name: &str) -> Result<(), RegistryError>;
}

pub trait Registry: RegistryReader + RegistryWriter {}

impl<T: RegistryReader + RegistryWriter> Registry for T {}

// No Debug: a path can run through the user's profile folder.
#[derive(Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub name: String,
    pub pid: u32,
    pub path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ProcessError {
    #[error(
        "Windows would not let One Launch close {0}. It is most likely running as administrator: close it yourself, or start One Launch as administrator too, then try again."
    )]
    AccessDenied(String),
    #[error("{0}")]
    Failed(String),
}

impl ProcessError {
    pub const fn code(&self) -> Option<&'static str> {
        match self {
            Self::AccessDenied(_) => Some("PROCESS_ACCESS_DENIED"),
            Self::Failed(_) => None,
        }
    }
}

// One argument of a command line. A secret one is the account's password, on its way to the launcher that asked for it: it is never printed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProcessArg<'a> {
    Plain(&'a str),
    Secret(&'a str),
}

impl fmt::Debug for ProcessArg<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plain(text) => write!(f, "{text:?}"),
            Self::Secret(_) => f.write_str("[REDACTED]"),
        }
    }
}

pub trait ProcessLister: Send + Sync {
    // By image name, such as steam.exe.
    fn is_running(&self, image: &str) -> Result<bool, ProcessError>;
    fn list_processes(&self) -> Result<Vec<ProcessInfo>, ProcessError>;
    // The image names that run now, from a listing that opens no process: no handle, no path and no command line is asked for. It is all that is ever asked about a Riot game.
    fn running_names(&self) -> Result<Vec<String>, ProcessError>;
}

pub trait ProcessRunner: Send + Sync {
    fn start(&self, exe: &str, args: &[ProcessArg<'_>], cwd: &str) -> Result<(), ProcessError>;
    // Asks every window of this image to close, as a click on its close button does. A program without a window hears nothing.
    fn close(&self, image: &str) -> Result<(), ProcessError>;
    // Ends every process with this image name and, with `tree`, the processes they started.
    fn kill(&self, image: &str, tree: bool) -> Result<(), ProcessError>;
}

pub trait Processes: ProcessLister + ProcessRunner {}

impl<T: ProcessLister + ProcessRunner> Processes for T {}

pub trait FileReader: Send + Sync {
    fn exists(&self, path: &str) -> bool;
    fn size(&self, path: &str) -> io::Result<u64>;
    // When the file was last written.
    fn modified(&self, path: &str) -> io::Result<OffsetDateTime>;
    fn read(&self, path: &str) -> io::Result<Vec<u8>>;
    // At most `length` bytes from `from` on.
    fn read_range(&self, path: &str, from: u64, length: u64) -> io::Result<Vec<u8>>;
    // The names in a folder.
    fn list(&self, folder: &str) -> io::Result<Vec<String>>;
    // Where a folder really is, with links and substituted drives followed.
    fn resolved(&self, folder: &str) -> Option<String>;
}

pub trait FileWriter: Send + Sync {
    // Each file goes to a temporary file beside its target, and only when all are written is each moved over its target: a failure leaves every target as it was.
    fn write_atomic(&self, files: &[(&str, &[u8])]) -> io::Result<()>;
}

pub trait Files: FileReader + FileWriter {}

impl<T: FileReader + FileWriter> Files for T {}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct ShellError(pub String);

pub trait Shell: Send + Sync {
    // Hands a link such as steam://rungameid/1 to whatever program Windows opens it with.
    fn open_url(&self, url: &str) -> Result<(), ShellError>;
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ClipboardError {
    // Another program held the clipboard open: a later try may succeed.
    #[error("another program holds the clipboard open")]
    Busy,
    // Windows would not take the marks that keep a copy out of the clipboard history: a plain copy may still be made.
    #[error("{0}")]
    Unmarked(String),
    #[error("{0}")]
    Refused(String),
}

pub trait Clipboard: Send + Sync {
    // With the formats that keep the text out of Windows' clipboard history and the cloud clipboard.
    fn write_private(&self, text: &str) -> Result<(), ClipboardError>;
    fn write_text(&self, text: &str) -> Result<(), ClipboardError>;
    fn read_text(&self) -> Result<Option<Zeroizing<String>>, ClipboardError>;
    // Reads and empties in one hold of the clipboard, so that nothing copied in between is emptied. A text whose length in UTF-16 units is not `length` is not read at all.
    fn clear_if(&self, length: usize, holds: &dyn Fn(&str) -> bool)
    -> Result<bool, ClipboardError>;
}

// The launch request the Riot Client's own Play button sends, to be sent to the client's local server on this PC once the client shows the game's page. Nothing else is ever asked of that server, and nothing at all of Riot's.
pub struct RiotLaunchRequest<'a> {
    // Both as the game's metadata folder names them: letters, digits and underscores. Whoever sends the request checks that before it reads the lockfile.
    pub product: &'a str,
    pub patchline: &'a str,
    // The client's Config\lockfile, which names the server's port and the password of this run. It is read at the moment of the request, the password is used for that one request, and neither is kept.
    pub lockfile_path: &'a str,
    pub cancel: &'a CancelToken,
}

// Why the launch request did not take. Each is a fixed sentence with at most a status or a word of the network's: none can carry the lockfile's password or anything the client answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum RiotLaunchError {
    #[error("the Riot Client has no lockfile to answer on")]
    NoLockfile,
    #[error("the lockfile is not in the shape One Launch knows")]
    LockfileShape,
    #[error("the launch request did not reach the Riot Client ({0})")]
    Unreachable(&'static str),
    #[error("the Riot Client answered the launch request with HTTP {0}")]
    Status(u16),
}

// Handed to the Riot adapter's launch and to nothing else.
pub trait RiotLocalLaunch: Send + Sync {
    fn launch(&self, request: &RiotLaunchRequest<'_>) -> Result<(), RiotLaunchError>;
}

// Stops a switch: whoever holds a copy can ask, and every wait of the switch looks at it.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

pub trait Clock: Send + Sync {
    fn now(&self) -> OffsetDateTime;
    // Returns after `duration`, or at once when the token is cancelled.
    fn sleep(&self, duration: Duration, cancel: &CancelToken);
}

// Everything a launcher adapter and the switch pipeline may touch on the machine.
#[derive(Clone)]
pub struct Platform {
    pub registry: Arc<dyn Registry>,
    pub processes: Arc<dyn Processes>,
    pub files: Arc<dyn Files>,
    pub shell: Arc<dyn Shell>,
    pub clock: Arc<dyn Clock>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_argument_is_never_printed() {
        let args = [
            ProcessArg::Plain("-login"),
            ProcessArg::Secret("placeholder-password-1"),
        ];
        let shown = format!("{args:?} {:#?}", args[1]);
        assert!(!shown.contains("placeholder"));
        assert!(shown.contains("-login"));
    }

    #[test]
    fn a_cancelled_token_is_cancelled_for_every_copy() {
        let token = CancelToken::new();
        let copy = token.clone();
        assert!(!copy.is_cancelled());
        token.cancel();
        assert!(copy.is_cancelled());
    }
}
