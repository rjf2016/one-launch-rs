// What an adapter asks about accounts and their saved sign-ins, answered from the database. A saved sign-in is a bearer credential and is kept like a password: encrypted by the vault before it is stored, decrypted only for the switch pipeline at the moment it goes into the launcher's own file, and never printed.

use std::sync::Arc;
use std::sync::mpsc::sync_channel;

use ol_adapters::{AccountRef, AdapterError, Directory};
use ol_formats::battlenet::session as battlenet;
use ol_formats::epic::session::{fingerprint_of, parse_blob};
use ol_formats::riot::session as riot;
use ol_model::{Account, LauncherId, LauncherSession};
use ol_platform_api::Clock;
use ol_store::{NewSession, Store};
use ol_vault::{Secret, Vault};

use crate::accounts;
use crate::launchers::ServiceError;

pub fn account_ref(account: &Account) -> AccountRef {
    AccountRef {
        id: Some(account.id.clone()),
        display_name: account.display_name.clone(),
        login: account.login.clone(),
        launcher_uid: account.launcher_uid.clone(),
        has_secret: account.has_secret,
    }
}

fn failed(error: impl std::fmt::Display) -> AdapterError {
    AdapterError::Failed(error.to_string())
}

// One launcher's accounts and saved sign-ins, on the thread the database lives on.
pub struct StoreDirectory<'a> {
    pub store: &'a Store,
    pub vault: &'a dyn Vault,
    pub clock: &'a dyn Clock,
    pub launcher: LauncherId,
}

impl StoreDirectory<'_> {
    fn now(&self) -> String {
        ol_store::iso_timestamp(self.clock.now())
    }
}

impl Directory for StoreDirectory<'_> {
    fn account_by_launcher_uid(&self, uid: &str) -> Result<Option<AccountRef>, AdapterError> {
        let found = self
            .store
            .account_by_launcher_uid(self.launcher, uid)
            .map_err(failed)?;
        Ok(found.as_ref().map(account_ref))
    }

    fn account_by_id(&self, id: &str) -> Result<Option<AccountRef>, AdapterError> {
        let found = self.store.account(id).map_err(failed)?;
        Ok(found
            .filter(|account| account.launcher == self.launcher)
            .as_ref()
            .map(account_ref))
    }

    fn set_launcher_uid(&self, account_id: &str, uid: &str) -> Result<(), AdapterError> {
        accounts::set_launcher_uid(self.store, account_id, Some(uid), &self.now())
            .map(|_| ())
            .map_err(failed)
    }

    fn session(&self, account_id: &str) -> Result<Option<LauncherSession>, AdapterError> {
        self.store.session(account_id).map_err(failed)
    }

    fn session_by_fingerprint(
        &self,
        fingerprint: &str,
    ) -> Result<Option<LauncherSession>, AdapterError> {
        self.store
            .session_by_fingerprint(self.launcher, fingerprint)
            .map_err(failed)
    }

    fn save_session(
        &self,
        account_id: &str,
        launcher_uid: Option<&str>,
        fingerprint: &str,
        blob: &Secret,
    ) -> Result<LauncherSession, AdapterError> {
        let sealed = self.vault.encrypt(blob).map_err(failed)?;
        self.store
            .save_session(&NewSession {
                account_id,
                launcher: self.launcher,
                launcher_uid,
                blob: &sealed,
                fingerprint,
                captured_at: &self.now(),
            })
            .map_err(failed)?;
        self.session(account_id)?
            .ok_or_else(|| failed("The saved sign-in was gone right after it was stored"))
    }

    fn decrypt_session(&self, account_id: &str) -> Result<Option<Secret>, AdapterError> {
        self.store
            .session_blob(account_id)
            .map_err(failed)?
            .map(|sealed| self.vault.decrypt(&sealed).map_err(failed))
            .transpose()
    }

    fn mark_session_verified(&self, account_id: &str) -> Result<(), AdapterError> {
        self.store
            .mark_session_verified(account_id, &self.now())
            .map_err(failed)
    }

    fn mark_session_stale(&self, account_id: &str, reason: &str) -> Result<(), AdapterError> {
        self.store
            .mark_session_stale(account_id, reason)
            .map_err(failed)
    }
}

// Stores a sign-in for an account as it is given, once its shape has been checked. The fake launchers' seed is its only caller: the app itself saves a sign-in only from the launcher's own file.
pub fn seed(
    directory: &StoreDirectory<'_>,
    account: &Account,
    launcher_uid: Option<&str>,
    blob: &Secret,
) -> Result<(), ServiceError> {
    let adapter = ServiceError::adapter(account.launcher);
    let fingerprint = match account.launcher {
        LauncherId::Epic => parse_blob(blob.expose())
            .map(|blob| fingerprint_of(&blob.data))
            .map_err(|error| adapter(failed(error)))?,
        LauncherId::Battlenet => battlenet::parse_blob(blob.expose())
            .map(|blob| battlenet::fingerprint_of(&blob.unified_auth.hex))
            .map_err(|error| adapter(failed(error)))?,
        LauncherId::Riot => riot::parse_blob(blob.expose())
            .and_then(|blob| riot::session_of_blob(&blob))
            .map(|session| session.fingerprint)
            .map_err(|error| adapter(failed(error)))?,
        LauncherId::Steam => return Err(ServiceError::NotImplemented(LauncherId::Steam)),
    };
    directory
        .save_session(&account.id, launcher_uid, &fingerprint, blob)
        .map(|_| ())
        .map_err(adapter)
}

pub type StoreWork = Box<dyn FnOnce(&Store) + Send>;

// The thread the database lives on, as a switch reaches it: the switch waits for a launcher on a thread of its own and has no database there.
pub trait StoreThread: Send + Sync {
    // Hands `work` to the database's thread. False when that thread is gone.
    fn run(&self, work: StoreWork) -> bool;
}

// The same directory for a switch on its own thread: each question goes to the database's thread and waits there for its answer.
pub struct RemoteDirectory {
    pub thread: Arc<dyn StoreThread>,
    pub vault: Arc<dyn Vault + Send + Sync>,
    pub clock: Arc<dyn Clock>,
    pub launcher: LauncherId,
}

impl RemoteDirectory {
    fn ask<T: Send + 'static>(
        &self,
        question: impl FnOnce(&StoreDirectory<'_>) -> Result<T, AdapterError> + Send + 'static,
    ) -> Result<T, AdapterError> {
        let gone = || failed("One Launch is no longer reading its data. Restart One Launch.");
        let (answer, heard) = sync_channel(1);
        let (vault, clock, launcher) = (
            Arc::clone(&self.vault),
            Arc::clone(&self.clock),
            self.launcher,
        );
        let queued = self.thread.run(Box::new(move |store| {
            let directory = StoreDirectory {
                store,
                vault: vault.as_ref(),
                clock: clock.as_ref(),
                launcher,
            };
            let _ = answer.send(question(&directory));
        }));
        if !queued {
            return Err(gone());
        }
        heard.recv().unwrap_or_else(|_| Err(gone()))
    }
}

impl Directory for RemoteDirectory {
    fn account_by_launcher_uid(&self, uid: &str) -> Result<Option<AccountRef>, AdapterError> {
        let uid = uid.to_owned();
        self.ask(move |directory| directory.account_by_launcher_uid(&uid))
    }

    fn account_by_id(&self, id: &str) -> Result<Option<AccountRef>, AdapterError> {
        let id = id.to_owned();
        self.ask(move |directory| directory.account_by_id(&id))
    }

    fn set_launcher_uid(&self, account_id: &str, uid: &str) -> Result<(), AdapterError> {
        let (account_id, uid) = (account_id.to_owned(), uid.to_owned());
        self.ask(move |directory| directory.set_launcher_uid(&account_id, &uid))
    }

    fn session(&self, account_id: &str) -> Result<Option<LauncherSession>, AdapterError> {
        let account_id = account_id.to_owned();
        self.ask(move |directory| directory.session(&account_id))
    }

    fn session_by_fingerprint(
        &self,
        fingerprint: &str,
    ) -> Result<Option<LauncherSession>, AdapterError> {
        let fingerprint = fingerprint.to_owned();
        self.ask(move |directory| directory.session_by_fingerprint(&fingerprint))
    }

    fn save_session(
        &self,
        account_id: &str,
        launcher_uid: Option<&str>,
        fingerprint: &str,
        blob: &Secret,
    ) -> Result<LauncherSession, AdapterError> {
        let (account_id, launcher_uid, fingerprint) = (
            account_id.to_owned(),
            launcher_uid.map(str::to_owned),
            fingerprint.to_owned(),
        );
        let blob = Secret::new(blob.expose().to_owned());
        self.ask(move |directory| {
            directory.save_session(&account_id, launcher_uid.as_deref(), &fingerprint, &blob)
        })
    }

    fn decrypt_session(&self, account_id: &str) -> Result<Option<Secret>, AdapterError> {
        let account_id = account_id.to_owned();
        self.ask(move |directory| directory.decrypt_session(&account_id))
    }

    fn mark_session_verified(&self, account_id: &str) -> Result<(), AdapterError> {
        let account_id = account_id.to_owned();
        self.ask(move |directory| directory.mark_session_verified(&account_id))
    }

    fn mark_session_stale(&self, account_id: &str, reason: &str) -> Result<(), AdapterError> {
        let (account_id, reason) = (account_id.to_owned(), reason.to_owned());
        self.ask(move |directory| directory.mark_session_stale(&account_id, &reason))
    }
}
