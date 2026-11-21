use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use ol_platform_api::{CancelToken, Clipboard, ClipboardError, Clock};
use ol_store::Store;
use ol_vault::{Secret, Vault, VaultError};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::accounts::{self, AccountError};

pub const CLEAR_AFTER: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyField {
    Login,
    Password,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CopyResult {
    pub clear_after: Duration,
    pub history_excluded: bool,
}

#[derive(Error)]
pub enum CopyError {
    #[error(transparent)]
    Account(#[from] AccountError),
    #[error("No password is stored for this account")]
    NoPassword,
    #[error(transparent)]
    Vault(#[from] VaultError),
    #[error("This build of One Launch has no clipboard to copy to.")]
    NoClipboard,
    #[error("Windows would not let One Launch use the clipboard: {0}. Try again.")]
    Clipboard(ClipboardError),
}

impl CopyError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Account(error) => error.code(),
            Self::NoPassword => "NO_PASSWORD",
            Self::Vault(error) => error.code(),
            Self::NoClipboard => "CLIPBOARD_UNAVAILABLE",
            Self::Clipboard(_) => "CLIPBOARD_FAILED",
        }
    }
}

impl fmt::Debug for CopyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CopyError({})", self.code())
    }
}

pub trait Timer: Send + Sync {
    fn after(&self, delay: Duration, cancel: CancelToken, run: Box<dyn FnOnce() + Send>);
}

pub struct SleepingTimer {
    clock: Arc<dyn Clock>,
}

impl SleepingTimer {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self { clock }
    }
}

impl Timer for SleepingTimer {
    fn after(&self, delay: Duration, cancel: CancelToken, run: Box<dyn FnOnce() + Send>) {
        let clock = Arc::clone(&self.clock);
        // Without the thread the copy is still taken off at quit.
        let _ = thread::Builder::new()
            .name("clipboard".to_owned())
            .spawn(move || {
                clock.sleep(delay, &cancel);
                if !cancel.is_cancelled() {
                    run();
                }
            });
    }
}

type Fingerprint = [u8; 32];

fn fingerprint(text: &str) -> Fingerprint {
    Sha256::digest(text.as_bytes()).into()
}

struct Pending {
    fingerprint: Fingerprint,
    cancel: CancelToken,
    copy: u64,
}

struct Inner {
    board: Arc<dyn Clipboard>,
    timer: Arc<dyn Timer>,
    pending: Mutex<Option<Pending>>,
    copies: AtomicU64,
}

#[derive(Clone)]
pub struct ClipboardService {
    inner: Arc<Inner>,
}

impl ClipboardService {
    pub fn new(board: Arc<dyn Clipboard>, timer: Arc<dyn Timer>) -> Self {
        Self {
            inner: Arc::new(Inner {
                board,
                timer,
                pending: Mutex::new(None),
                copies: AtomicU64::new(0),
            }),
        }
    }

    pub fn copy_sensitive(&self, text: &Secret) -> Result<CopyResult, ClipboardError> {
        let inner = &self.inner;
        drop(inner.take_pending());
        let text = text.expose();
        let history_excluded = inner.board.write_private(text).is_ok()
            && inner
                .board
                .read_text()
                .is_ok_and(|back| back.as_deref().map(String::as_str) == Some(text));
        if !history_excluded {
            inner.board.write_text(text)?;
        }
        let copy = inner.copies.fetch_add(1, Ordering::SeqCst);
        let cancel = CancelToken::new();
        *inner.pending() = Some(Pending {
            fingerprint: fingerprint(text),
            cancel: cancel.clone(),
            copy,
        });
        let due = Arc::downgrade(inner);
        inner.timer.after(
            CLEAR_AFTER,
            cancel,
            Box::new(move || {
                if let Some(inner) = due.upgrade() {
                    inner.clear_copy(Some(copy));
                }
            }),
        );
        Ok(CopyResult {
            clear_after: CLEAR_AFTER,
            history_excluded,
        })
    }

    pub fn clear_if_unchanged(&self) -> bool {
        self.inner.clear_copy(None)
    }
}

impl Inner {
    fn pending(&self) -> MutexGuard<'_, Option<Pending>> {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn take_pending(&self) -> Option<Pending> {
        let taken = self.pending().take();
        if let Some(pending) = &taken {
            pending.cancel.cancel();
        }
        taken
    }

    // A timer names its copy, so one that fires late cannot clear a newer copy.
    fn clear_copy(&self, copy: Option<u64>) -> bool {
        let pending = {
            let mut pending = self.pending();
            if copy.is_some() && pending.as_ref().map(|pending| pending.copy) != copy {
                return false;
            }
            pending.take()
        };
        let Some(pending) = pending else {
            return false;
        };
        pending.cancel.cancel();
        self.board
            .clear_if(&|held| fingerprint(held) == pending.fingerprint)
            .unwrap_or(false)
    }
}

pub fn copy_account_field(
    store: &Store,
    vault: &dyn Vault,
    clipboard: Option<&ClipboardService>,
    account_id: &str,
    field: CopyField,
) -> Result<CopyResult, CopyError> {
    let account = accounts::require(store, account_id)?;
    let clipboard = clipboard.ok_or(CopyError::NoClipboard)?;
    let text = match field {
        CopyField::Login => Secret::new(account.login),
        CopyField::Password => {
            let sealed = store
                .account_password(&account.id)
                .map_err(AccountError::from)?
                .ok_or(CopyError::NoPassword)?;
            vault.decrypt(&sealed)?
        }
    };
    clipboard
        .copy_sensitive(&text)
        .map_err(CopyError::Clipboard)
}
