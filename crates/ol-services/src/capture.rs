use ol_adapters::battlenet::{BattlenetAdapter, UNIFIED_AUTH_PATH};
use ol_adapters::epic::{EpicAdapter, id_tail};
use ol_adapters::riot::RiotAdapter;
use ol_adapters::{AccountRef, AdapterError};
use ol_formats::battlenet::client_log::SignInOutcome as BattlenetSignIn;
use ol_formats::epic::launcher_log::SignInOutcome as EpicSignIn;
use ol_formats::epic::session::session_blob_from_ini;
use ol_model::{Account, LauncherSession};
use ol_store::Store;

use crate::accounts::{self, AccountError};
use crate::launchers::{Adapter, ServiceError};
use crate::sessions::account_ref;
use crate::switch;

pub struct Captured {
    pub session: LauncherSession,
    pub account: Account,
}

fn blocked(message: String) -> AdapterError {
    AdapterError::SwitchBlocked(message)
}

fn differs(one: &str, other: &str) -> bool {
    one.to_lowercase() != other.to_lowercase()
}

fn owned_by_another(owner: Option<&AccountRef>, account: &AccountRef) -> Option<String> {
    owner
        .filter(|owner| owner.id != account.id)
        .map(|owner| owner.display_name.clone())
}

fn linked(account: &AccountRef) -> Option<&str> {
    account
        .launcher_uid
        .as_deref()
        .filter(|uid| !uid.is_empty())
}

pub fn capture(
    store: &Store,
    adapter: &Adapter<'_>,
    account: &Account,
) -> Result<Captured, ServiceError> {
    let launcher = adapter.launcher();
    let reference = account_ref(account);
    let captured = match adapter {
        Adapter::Steam(_) => return Err(ServiceError::NotImplemented(launcher)),
        Adapter::Epic(epic) => capture_epic(epic, &reference),
        Adapter::Battlenet(battlenet) => capture_battlenet(battlenet, &reference),
        Adapter::Riot(riot) => capture_riot(riot, &reference),
    };
    captured.map_err(ServiceError::adapter(launcher))?;
    let session = store.session(&account.id)?.ok_or_else(|| {
        ServiceError::adapter(launcher)(AdapterError::Failed(
            "The saved sign-in was gone right after it was stored".to_owned(),
        ))
    })?;
    Ok(Captured {
        session,
        account: accounts::require(store, &account.id)?,
    })
}

fn capture_epic(epic: &EpicAdapter<'_>, account: &AccountRef) -> Result<(), AdapterError> {
    epic.require_install()?;
    let running = epic.is_running()?;
    let text = epic.read_ini()?;
    let current = epic.current_session(text.as_deref(), running)?;
    let login = &account.login;
    let blob = text
        .as_deref()
        .and_then(session_blob_from_ini)
        .filter(|_| current.disk.remembered);
    let Some(blob) = blob else {
        return Err(blocked(format!(
            "Epic Games Launcher is not remembering a session right now. Sign in to it as {login}, exit it once so it saves the session, then save again."
        )));
    };
    if running {
        let signed_in = matches!(current.last_sign_in, Some(EpicSignIn::SignedIn { .. }));
        if !signed_in || current.logged_uid.is_none() {
            let why = match &current.last_sign_in {
                Some(EpicSignIn::Failed { reason }) => format!(" ({reason})"),
                _ => String::new(),
            };
            return Err(blocked(format!(
                "Epic Games Launcher is running but has not signed in during this run{why}. Sign in to it as {login}, then save again."
            )));
        }
        if !current.disk_current {
            return Err(blocked(
                "Epic Games Launcher has not saved its current sign-in to disk yet. Exit the launcher (it writes the session on exit), then save again.".to_owned(),
            ));
        }
    }
    if let Some(owner) = owned_by_another(current.owner.as_ref(), account) {
        return Err(blocked(format!(
            "The session Epic Games Launcher currently remembers belongs to {owner}. Sign in to the launcher as {login} first."
        )));
    }
    let uid = if running {
        current.logged_uid.clone()
    } else {
        current
            .owner
            .as_ref()
            .and_then(|owner| owner.launcher_uid.clone())
            .or_else(|| current.logged_uid.clone())
    };
    if let (Some(uid), Some(own)) = (uid.as_deref(), linked(account))
        && differs(uid, own)
    {
        return Err(blocked(format!(
            "The launcher's last sign-in was Epic account {}, but {} is linked to {}. Check the launcher account ID on the account page.",
            id_tail(Some(uid), 6),
            account.display_name,
            id_tail(Some(own), 6)
        )));
    }
    let uid = uid.or_else(|| account.launcher_uid.clone());
    switch::epic::store_session(epic.directory(), account, &blob, uid.as_deref())
}

fn capture_battlenet(
    battlenet: &BattlenetAdapter<'_>,
    account: &AccountRef,
) -> Result<(), AdapterError> {
    battlenet.require_install()?;
    let running = battlenet.is_running()?;
    let current = battlenet.current_session(running)?;
    let login = &account.login;
    let Some(token) = &current.token else {
        return Err(blocked(format!(
            "Battle.net has no remembered session right now (nothing under {UNIFIED_AUTH_PATH}). Sign in to the client as {login} with \"Keep me logged in\" on, then save again."
        )));
    };
    let signed_in = matches!(current.last_sign_in, Some(BattlenetSignIn::SignedIn { .. }));
    if running && (!signed_in || current.logged_uid.is_none()) {
        let why = match &current.last_sign_in {
            Some(BattlenetSignIn::Failed { reason }) => format!(" ({reason})"),
            _ => String::new(),
        };
        return Err(blocked(format!(
            "Battle.net is running but has not signed in during this run{why}. Sign in to it as {login}, then save again."
        )));
    }
    if !running && current.signed_out {
        return Err(blocked(
            "The last Battle.net run ended with a logout, so the remembered session cannot be attributed to an account. Sign in to the client, exit it, then save again.".to_owned(),
        ));
    }
    if let Some(owner) = owned_by_another(current.owner.as_ref(), account) {
        return Err(blocked(format!(
            "The session Battle.net currently remembers belongs to {owner}. Sign in to the client as {login} first."
        )));
    }
    let uid = if running {
        current.logged_uid.clone()
    } else {
        current
            .owner
            .as_ref()
            .and_then(|owner| owner.launcher_uid.clone())
            .or_else(|| current.logged_uid.clone())
    };
    if let (Some(uid), Some(own)) = (uid.as_deref(), linked(account))
        && differs(uid, own)
    {
        return Err(blocked(format!(
            "The client's last sign-in was Battle.net account {}, but {} is linked to {}. Check the account ID on the account page.",
            id_tail(Some(uid), 4),
            account.display_name,
            id_tail(Some(own), 4)
        )));
    }
    let uid = uid.or_else(|| account.launcher_uid.clone());
    switch::battlenet::store_session(battlenet.directory(), account, token, uid.as_deref())
}

const RIOT_CLIENT_RUNS: &str =
    "The Riot Client is running. Right-click its tray icon, choose Exit, then save again.";

// The client rewrites the file while it runs, so the second look catches a client that started during the read.
fn capture_riot(riot: &RiotAdapter<'_>, account: &AccountRef) -> Result<(), AdapterError> {
    riot.require_install()?;
    if riot.client_runs_beside_no_game("save again")? {
        return Err(blocked(RIOT_CLIENT_RUNS.to_owned()));
    }
    let current = riot.current_session()?;
    if riot.client_runs_beside_no_game("save again")? {
        return Err(blocked(RIOT_CLIENT_RUNS.to_owned()));
    }
    let login = &account.login;
    let (Some(session), Some(file)) = (&current.session, &current.file) else {
        return Err(blocked(format!(
            "The Riot Client has no saved sign-in. Sign in to it as {login} with “Stay signed in” ticked, exit the client, then save again."
        )));
    };
    if let Some(owner) = owned_by_another(current.owner.as_ref(), account) {
        return Err(blocked(format!(
            "The sign-in the Riot Client has saved belongs to {owner}. Sign in to the Riot Client as {login} first."
        )));
    }
    let uid = session.account_id.as_str();
    if let Some(own) = linked(account)
        && differs(own, uid)
    {
        return Err(blocked(format!(
            "The Riot Client's saved sign-in is Riot account {}, but {} is linked to {}. Check the Riot account id on the account page.",
            id_tail(Some(uid), 4),
            account.display_name,
            id_tail(Some(own), 4)
        )));
    }
    switch::riot::store_session(riot.directory(), account, session, file)
}

pub fn forget(store: &Store, account_id: &str) -> Result<(), AccountError> {
    let account = accounts::require(store, account_id)?;
    store.delete_session(&account.id)?;
    Ok(())
}
