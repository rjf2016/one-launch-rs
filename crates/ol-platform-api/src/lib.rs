#![forbid(unsafe_code)]

use thiserror::Error;
use zeroize::Zeroizing;

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
