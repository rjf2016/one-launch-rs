#![forbid(unsafe_code)]

// A machine that exists only in memory and under scratch folders: a registry, a process table, a shell and a clock that go nowhere near the real ones, and a file port that refuses every path outside the folders it was given.

mod clock;
mod files;
mod oplog;
mod processes;
mod registry;
mod shell;

pub use clock::FakeClock;
pub use files::ScratchFiles;
pub use oplog::OpLog;
pub use processes::{FakeProcesses, ProcessKnobs};
pub use registry::{FakeRegistry, RegistryValue};
pub use shell::FakeShell;
