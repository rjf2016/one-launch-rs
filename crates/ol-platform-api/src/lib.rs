#![forbid(unsafe_code)]

pub mod atomic;
pub mod contract;
mod ports;
pub mod protected;

pub use ports::{
    CancelToken, Clock, FileReader, FileWriter, Files, Hive, Platform, ProcessArg, ProcessError,
    ProcessInfo, ProcessLister, ProcessRunner, Processes, Registry, RegistryData, RegistryError,
    RegistryNamedValue, RegistryReader, RegistryWriter, RiotLaunchError, RiotLaunchRequest,
    RiotLocalLaunch, Shell, ShellError,
};

use thiserror::Error;
pub use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum ProtectError {
    #[error("Windows could not protect the data for this user")]
    Protect,
    #[error(
        "Windows could not unprotect the data: it was protected by another user or on another PC"
    )]
    Unprotect,
}

// Windows DPAPI for the signed-in user: what is protected here opens only for that user on that PC.
pub trait DataProtector {
    fn protect(&self, plain: &[u8]) -> Result<Vec<u8>, ProtectError>;
    fn unprotect(&self, protected: &[u8]) -> Result<Zeroizing<Vec<u8>>, ProtectError>;
}
