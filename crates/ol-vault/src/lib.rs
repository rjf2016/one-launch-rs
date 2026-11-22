#![forbid(unsafe_code)]

mod codec;
mod fake;
mod local_state;
mod os_crypt;
mod secret;

use ol_model::Ciphertext;
use thiserror::Error;

pub use fake::FakeVault;
pub use local_state::local_state_path;
pub use os_crypt::{KeyError, OsCryptVault, Recovered};
pub use secret::Secret;

pub const PROBE_VALUE: &str = "one-launch-vault-probe-v1";

const REDACTED: &str = "[REDACTED]";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum VaultError {
    #[error("Secure storage is not available on this system. Secrets cannot be stored.")]
    Unavailable,
    #[error(
        "One Launch cannot decrypt its stored secrets: the encryption key in the app data folder (chromium\\Local State) was lost or belongs to a different Windows user. In Settings, clear the stored passwords and sign-ins, then enter them again."
    )]
    Unreadable,
}

impl VaultError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Unavailable => "VAULT_UNAVAILABLE",
            Self::Unreadable => "VAULT_UNREADABLE",
        }
    }
}

pub trait Vault {
    fn encrypt(&self, plaintext: &Secret) -> Result<Ciphertext, VaultError>;
    fn decrypt(&self, ciphertext: &Ciphertext) -> Result<Secret, VaultError>;

    // Whether it has a key to seal and open with at all, asked without sealing anything.
    fn has_key(&self) -> bool {
        true
    }
}
