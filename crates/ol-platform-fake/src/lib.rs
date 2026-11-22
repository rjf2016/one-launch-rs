#![forbid(unsafe_code)]

// A machine that exists only in memory and under scratch folders: a registry, a process table, a shell and a clock that go nowhere near the real ones, and a file port that refuses every path outside the folders it was given.

mod clipboard;
mod clock;
mod files;
pub mod http;
mod oplog;
mod processes;
mod registry;
mod riot;
mod shell;

pub use clipboard::{Kept, RecordingClipboard};
pub use clock::FakeClock;
pub use files::ScratchFiles;
pub use http::RecordingHttp;
pub use oplog::OpLog;
pub use processes::{CLOSE_ARGUMENT, FakeProcesses, ProcessKnobs};
pub use registry::{FakeRegistry, RegistryValue};
pub use riot::FakeRiotLaunch;
pub use shell::FakeShell;
