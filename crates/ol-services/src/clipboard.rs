use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use ol_platform_api::{CancelToken, Clipboard, ClipboardError, ClipboardSequence, Clock};
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

// A clear that finds the clipboard held open is tried again this often, and is still pending for the clear at quit after that.
const CLEAR_TRIES: u32 = 5;
const CLEAR_RETRY: Duration = Duration::from_secs(2);

fn fingerprint(text: &str) -> Fingerprint {
    Sha256::digest(text.as_bytes()).into()
}

struct Pending {
    written: ClipboardSequence,
    fingerprint: Fingerprint,
    length: usize,
    cancel: CancelToken,
    copy: u64,
}

#[derive(Default)]
struct State {
    pending: Option<Pending>,
    copies: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cleared {
    Yes,
    No,
    Busy,
}

struct Inner {
    board: Arc<dyn Clipboard>,
    timer: Arc<dyn Timer>,
    // Held through every write and clear, so a clear acts only for the copy that is current while it reads the clipboard.
    state: Mutex<State>,
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
                state: Mutex::new(State::default()),
            }),
        }
    }

    // The marks that keep the text out of the history are left off only when Windows will not take the marks themselves. Any other failure is an error the user can try again, and the earlier copy stays due to be cleared.
    pub fn copy_sensitive(&self, text: &Secret) -> Result<CopyResult, ClipboardError> {
        let inner = &self.inner;
        let mut state = inner.state();
        let text = text.expose();
        let (written, history_excluded) = match inner.board.write_private(text) {
            Ok(written) => (written, true),
            Err(ClipboardError::Unmarked(_)) => (inner.board.write_text(text)?, false),
            Err(failed) => return Err(failed),
        };
        if let Some(earlier) = state.pending.take() {
            earlier.cancel.cancel();
        }
        let copy = state.copies;
        state.copies += 1;
        let cancel = CancelToken::new();
        state.pending = Some(Pending {
            written,
            fingerprint: fingerprint(text),
            length: text.encode_utf16().count(),
            cancel: cancel.clone(),
            copy,
        });
        drop(state);
        arm(inner, copy, cancel, CLEAR_AFTER, CLEAR_TRIES);
        Ok(CopyResult {
            clear_after: CLEAR_AFTER,
            history_excluded,
        })
    }

    // At quit: the last chance, so a clipboard held open is tried as often as the timer would.
    pub fn clear_if_unchanged(&self) -> bool {
        for _ in 0..CLEAR_TRIES {
            match self.inner.clear_copy(None) {
                Cleared::Yes => return true,
                Cleared::No => return false,
                Cleared::Busy => {}
            }
        }
        false
    }
}

fn arm(inner: &Arc<Inner>, copy: u64, cancel: CancelToken, delay: Duration, tries: u32) {
    let due = Arc::downgrade(inner);
    let again = cancel.clone();
    inner.timer.after(
        delay,
        cancel,
        Box::new(move || {
            let Some(inner) = due.upgrade() else {
                return;
            };
            if inner.clear_copy(Some(copy)) == Cleared::Busy && tries > 1 {
                arm(&inner, copy, again, CLEAR_RETRY, tries - 1);
            }
        }),
    );
}

impl Inner {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    // A timer names its copy, so one that fires late cannot clear a newer copy. A clear that could not read the clipboard leaves the copy pending.
    fn clear_copy(&self, copy: Option<u64>) -> Cleared {
        let mut state = self.state();
        let Some(pending) = state.pending.as_ref() else {
            return Cleared::No;
        };
        if copy.is_some_and(|copy| copy != pending.copy) {
            return Cleared::No;
        }
        let (written, copied, length) = (pending.written, pending.fingerprint, pending.length);
        let Ok(cleared) = self
            .board
            .clear_if(written, length, &|held| fingerprint(held) == copied)
        else {
            return Cleared::Busy;
        };
        if let Some(pending) = state.pending.take() {
            pending.cancel.cancel();
        }
        if cleared { Cleared::Yes } else { Cleared::No }
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
