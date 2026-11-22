// What the Encryption panel says of the vault, its self-test and its reset. A probe of known text is stored once, so a later start can tell a key that works from a key that was lost.

use std::time::{Duration, Instant};

use ol_model::SecretKey;
use ol_store::{ClearedSecrets, Sealed, Store, StoreError};
use ol_vault::{PROBE_VALUE, Secret, Vault, VaultError};
use thiserror::Error;
use uuid::Uuid;

// The vault as the app holds it.
#[derive(Clone, Copy)]
pub enum VaultAccess<'a> {
    // No key could be had: nothing can be stored or read.
    Unavailable,
    Open(&'a dyn Vault),
    // chromium\Local State holds a key Windows does not unwrap for this user: what it sealed cannot be read until the stored secrets are cleared.
    KeyLost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaultBackend {
    Dpapi,
    None,
}

impl VaultBackend {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dpapi => "dpapi",
            Self::None => "none",
        }
    }
}

// The Electron app's vault:status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VaultHealth {
    pub available: bool,
    pub backend: VaultBackend,
    // None while nothing was ever stored to test against, and while the vault is unavailable.
    pub readable: Option<bool>,
}

// The three things the Status row says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncryptionState {
    Working,
    KeyLost,
    Unavailable,
}

impl EncryptionState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Working => "Working",
            Self::KeyLost => "Key lost",
            Self::Unavailable => "Unavailable · passwords and sign-ins cannot be stored",
        }
    }
}

impl VaultHealth {
    pub const fn state(self) -> EncryptionState {
        match (self.available, self.readable) {
            (false, _) => EncryptionState::Unavailable,
            (true, Some(false)) => EncryptionState::KeyLost,
            (true, _) => EncryptionState::Working,
        }
    }

    // The banner above the tabs, with its button to clear the stored secrets.
    pub const fn shows_reset(self) -> bool {
        matches!(self.state(), EncryptionState::KeyLost)
    }
}

pub const RESET_BANNER: &str = "Your stored passwords and saved sign-ins can no longer be read. This happens when the app's data folder was copied from another PC or Windows user, or when its chromium folder was deleted. Clearing them makes One Launch usable again; you then enter passwords and save sign-ins afresh.";

pub fn status(store: &Store, vault: VaultAccess<'_>) -> Result<VaultHealth, StoreError> {
    let vault = match vault {
        VaultAccess::Unavailable => {
            return Ok(VaultHealth {
                available: false,
                backend: VaultBackend::None,
                readable: None,
            });
        }
        VaultAccess::KeyLost => {
            return Ok(VaultHealth {
                available: true,
                backend: VaultBackend::Dpapi,
                readable: Some(false),
            });
        }
        VaultAccess::Open(vault) => vault,
    };
    let readable = store.secret(SecretKey::VaultProbe)?.map(|probe| {
        vault
            .decrypt(&probe)
            .is_ok_and(|text| text.expose() == PROBE_VALUE)
    });
    Ok(VaultHealth {
        available: true,
        backend: VaultBackend::Dpapi,
        readable,
    })
}

// Stores the probe when there is none. One that is there is never written over: a probe that no longer opens is how a lost key shows.
pub fn ensure_probe(
    store: &Store,
    vault: VaultAccess<'_>,
    now: &str,
) -> Result<bool, VaultHealthError> {
    let VaultAccess::Open(vault) = vault else {
        return Ok(false);
    };
    if store.secret(SecretKey::VaultProbe)?.is_some() {
        return Ok(false);
    }
    let sealed = vault.encrypt(&Secret::new(PROBE_VALUE.to_owned()))?;
    store.put_secret(SecretKey::VaultProbe, &sealed, now)?;
    Ok(true)
}

// What a reset removed, as the note under the banner counts it, and how many sealed values it kept because they still open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VaultResetSummary {
    pub passwords: usize,
    pub sessions: usize,
    pub secrets: usize,
    pub kept: usize,
}

impl From<ClearedSecrets> for VaultResetSummary {
    fn from(cleared: ClearedSecrets) -> Self {
        Self {
            passwords: cleared.passwords,
            sessions: cleared.sessions,
            secrets: cleared.secrets,
            kept: cleared.kept,
        }
    }
}

impl VaultResetSummary {
    pub fn note(self) -> String {
        let count = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
        let removed = format!(
            "Removed {}, {}, {}.",
            count(self.passwords, "password"),
            count(self.sessions, "saved sign-in"),
            count(self.secrets, "other secret")
        );
        match self.kept {
            0 => removed,
            kept => format!("{removed} Kept {kept} that can still be read."),
        }
    }
}

// What a reset did: cleared what could not be read, or found the key unwraps after all and cleared nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaultReset {
    Cleared(VaultResetSummary),
    KeyReadable,
}

impl VaultReset {
    pub fn note(self) -> String {
        match self {
            Self::Cleared(removed) => removed.note(),
            Self::KeyReadable => KEY_READABLE.to_owned(),
        }
    }

    pub const fn summary(self) -> Option<VaultResetSummary> {
        match self {
            Self::Cleared(removed) => Some(removed),
            Self::KeyReadable => None,
        }
    }
}

pub const KEY_READABLE: &str = "Windows unwrapped the vault key this time, so nothing was removed: your stored passwords and saved sign-ins can be read again.";

// Beside the confirmation: Windows gives no sure sign that the key belongs to another user or PC, so the user is told what a reset loses if it does not.
pub const RESET_WARNING: &str = "This deletes every stored password and saved sign-in that cannot be read, for good. If Windows could read them again later, for example once your PC reaches your work network again, they are gone all the same.";

// Why a reset is not carried out: it exists only for secrets that cannot be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum VaultResetRefusal {
    #[error("Your stored passwords and saved sign-ins can be read, so nothing was removed.")]
    Readable,
    #[error(
        "Windows cannot encrypt or decrypt anything just now, so nothing was removed. Try again later."
    )]
    Unavailable,
}

impl VaultResetRefusal {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Readable => "BAD_REQUEST",
            Self::Unavailable => "VAULT_UNAVAILABLE",
        }
    }
}

// Each password, saved sign-in and named secret `vault` cannot open goes, and the probe unless it opens to its own text; what opens stays, so a value sealed with a key made since the old one was lost survives. The accounts, their other fields and the copies of launcher files under backups stay. The caller puts a new probe in once the vault it holds can seal one.
pub fn clear_stored_secrets(
    store: &Store,
    vault: &dyn Vault,
    now: &str,
) -> Result<VaultResetSummary, StoreError> {
    let opens = |sealed: Sealed<'_>, value: &ol_model::Ciphertext| match sealed {
        Sealed::Secret(key) if key == SecretKey::VaultProbe.as_str() => vault
            .decrypt(value)
            .is_ok_and(|text| text.expose() == PROBE_VALUE),
        _ => vault.decrypt(value).is_ok(),
    };
    Ok(store.clear_unopened(now, opens)?.into())
}

#[derive(Debug, Error)]
pub enum VaultHealthError {
    #[error(transparent)]
    Vault(#[from] VaultError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("Decrypted value did not match what was stored")]
    Mismatch,
}

impl VaultHealthError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Vault(error) => error.code(),
            Self::Store(error) => error.code(),
            Self::Mismatch => "INTERNAL",
        }
    }
}

// Stores a random value under diagnostics.roundtrip, reads it back and deletes it, as the Electron app's Run test does. Answers how long that took.
pub fn self_test(
    store: &Store,
    vault: VaultAccess<'_>,
    now: &str,
) -> Result<Duration, VaultHealthError> {
    let started = Instant::now();
    let vault = match vault {
        VaultAccess::Open(vault) => vault,
        VaultAccess::Unavailable => return Err(VaultError::Unavailable.into()),
        VaultAccess::KeyLost => return Err(VaultError::Unreadable.into()),
    };
    let probe = Secret::new(Uuid::new_v4().to_string());
    let sealed = vault.encrypt(&probe)?;
    store.put_secret(SecretKey::DiagnosticsRoundtrip, &sealed, now)?;
    let read = store
        .secret(SecretKey::DiagnosticsRoundtrip)
        .map_err(VaultHealthError::from)
        .and_then(|stored| match stored {
            Some(stored) => Ok(Some(vault.decrypt(&stored)?)),
            None => Ok(None),
        });
    let deleted = store.delete_secret(SecretKey::DiagnosticsRoundtrip);
    let read = read?;
    deleted?;
    if read.is_none_or(|read| read.expose() != probe.expose()) {
        return Err(VaultHealthError::Mismatch);
    }
    Ok(started.elapsed())
}
