// Battle.net's switch: close the client, save the token it leaves behind, copy Battle.net.config and the token, put the target account's saved token under UnifiedAuth (or take the token out, so that the client asks), put the login first in the saved-accounts list, start the client and read its log for the sign-in. It reads, waits and decides; every write goes through the functions at the top of switch.rs. A saved token is decrypted in two places, `write_registry` and the opening of a snapshot's copy, and goes nowhere but under the client's own key.

use std::collections::HashMap;
use std::time::Duration;

use ol_adapters::battlenet::{
    BattlenetAdapter, CONFIG_FILE, EXE, LogMark, STEP, UNOWNED_SESSION_PIN, close_message, id_tail,
};
use ol_adapters::steam::{minutes, plural, seconds};
use ol_adapters::{AccountRef, AdapterError, Directory};
use ol_formats::battlenet::client_log::{SignInOutcome, last_sign_in};
use ol_formats::battlenet::config::{ConfigError, plan_saved_account_names};
use ol_formats::battlenet::session::{
    BattlenetSessionBlob, TokenValue, fingerprint_of, is_token_hex, parse_blob, serialize_blob,
    short_fingerprint,
};
use ol_model::{
    BackupSnapshot, JobStep, JobStepStatus, LaunchAttention, LauncherId, LauncherInstall,
    RestoreResult, SwitchStrategy,
};
use ol_platform_api::{CancelToken, Platform};
use ol_runtime::wait_until;
use ol_vault::Secret;
use serde_json::{Map, Value, json};
use time::OffsetDateTime;

use super::{
    RestoreError, SwitchOptions, SwitchResult, SwitchSink, Thrown, delete_unified_auth, job_steps,
    restore_battlenet, set_unified_auth, write_battlenet_config,
};
use crate::backup::{BackupError, BackupFile, BackupService, SnapshotRequest};
use crate::launchers::ServiceError;

// Where a snapshot's manifest keeps the UnifiedAuth values: a JSON list of names, each with its bytes sealed by the vault.
pub(super) const UNIFIED_AUTH_META: &str = "unifiedAuth";

// The values under UnifiedAuth as a manifest keeps them: names in the clear, bytes sealed, so that nothing under backups\ signs anyone in.
fn seal_tokens(backups: &BackupService<'_>, tokens: &[TokenValue]) -> Result<Value, BackupError> {
    let sealed = tokens
        .iter()
        .map(|token| {
            let enc = backups.seal(&Secret::new(token.hex.clone()))?;
            Ok(json!({ "name": token.name, "enc": enc }))
        })
        .collect::<Result<Vec<Value>, BackupError>>()?;
    Ok(Value::Array(sealed).to_string().into())
}

// The values a manifest recorded, opened. Anything that is not a list of sealed tokens refuses the snapshot.
pub(super) fn open_tokens(
    backups: &BackupService<'_>,
    recorded: &Value,
) -> Result<Vec<TokenValue>, BackupError> {
    let list: Value = recorded
        .as_str()
        .and_then(|text| serde_json::from_str(text).ok())
        .ok_or(BackupError::MetaShape)?;
    list.as_array()
        .ok_or(BackupError::MetaShape)?
        .iter()
        .map(|item| {
            let (Some(name), Some(enc)) = (item["name"].as_str(), item["enc"].as_str()) else {
                return Err(BackupError::MetaShape);
            };
            let token = TokenValue {
                name: name.to_owned(),
                hex: backups.open(enc)?.expose().to_lowercase(),
            };
            if is_token_hex(&token.hex) {
                Ok(token)
            } else {
                Err(BackupError::MetaShape)
            }
        })
        .collect()
}

// Battle.net.config when there is one, and the values under UnifiedAuth, as a snapshot takes them.
fn snapshot(
    battlenet: &BattlenetAdapter<'_>,
    backups: &BackupService<'_>,
    reason: &str,
    protect: Option<&str>,
    with_config: bool,
) -> Result<BackupSnapshot, RestoreError> {
    let tokens = battlenet.read_tokens()?;
    // The client is closed by now, so the token is final: one that nobody owns may be about to be removed, and this snapshot is its only copy.
    let unowned = battlenet.holds_unowned_session()?;
    let config_path = battlenet.config_path();
    let files = [BackupFile {
        source: &config_path,
        relative: CONFIG_FILE,
        sensitive: false,
    }];
    let mut meta = Map::new();
    meta.insert(UNIFIED_AUTH_META.to_owned(), seal_tokens(backups, &tokens)?);
    Ok(backups.snapshot(&SnapshotRequest {
        launcher: LauncherId::Battlenet,
        reason,
        files: if with_config { &files } else { &[] },
        meta,
        protect,
        pinned: unowned.then_some(UNOWNED_SESSION_PIN),
    })?)
}

// Puts a snapshot back from Settings or after a crash. Refused while the client runs, because it writes its token again at a sign-in and Battle.net.config when it exits; what is there now is snapshotted first, so restoring the wrong one can be undone.
pub fn restore_backup(
    battlenet: &BattlenetAdapter<'_>,
    platform: &Platform,
    backups: &BackupService<'_>,
    id: &str,
) -> Result<RestoreResult, ServiceError> {
    let adapter = ServiceError::adapter(LauncherId::Battlenet);
    battlenet.require_install().map_err(&adapter)?;
    if battlenet.is_running().map_err(&adapter)? {
        return Err(adapter(AdapterError::LauncherRunning(
            "Close Battle.net before restoring: it rewrites its token at sign-in and Battle.net.config at exit, which would undo the restore."
                .to_owned(),
        )));
    }
    let has_config = platform.files.exists(&battlenet.config_path());
    let reason = format!("Before restoring {id}");
    snapshot(battlenet, backups, &reason, Some(id), has_config)
        .and_then(|_| restore_battlenet(battlenet, platform, backups, id))
        .map_err(|error| error.into_service(LauncherId::Battlenet))
}

enum SignInWait {
    SignedIn {
        account_id: String,
    },
    WrongAccount {
        account_id: String,
        owner: Option<AccountRef>,
    },
    Failed {
        reason: String,
    },
    Timeout,
    Cancelled,
}

// How the switch ended, short of a step failing.
enum Ending {
    Done,
    Cancelled,
    NeedsSignIn,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cause {
    Expired,
    New,
    Other,
}

fn attention_for(
    login: &str,
    cause: Cause,
    reason: Option<String>,
    can_copy_password: bool,
    until: String,
) -> LaunchAttention {
    let why = match cause {
        Cause::Expired => "The client rejected its stored session.",
        Cause::New => "No session is stored for this account yet.",
        Cause::Other => "The client did not sign in on its own.",
    };
    LaunchAttention {
        title: if cause == Cause::New {
            format!("Sign in to Battle.net as {login}")
        } else {
            format!("Battle.net is asking you to sign in as {login}")
        },
        message: format!("{why} Its login page opens on the account it last signed in as."),
        steps: vec![
            "In the Battle.net window, click “Switch account”.".to_owned(),
            format!("Pick or type {login} and enter the password."),
            "Leave “Keep me logged in” on and sign in. One Launch continues on its own, and later switches are automatic.".to_owned(),
        ],
        reason,
        can_copy_password,
        until,
    }
}

// An account id as the client's log writes one: up to sixteen hex digits.
fn account_id(uid: Option<&str>) -> Option<String> {
    uid.filter(|uid| (1..=16).contains(&uid.len()) && uid.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_lowercase)
}

struct Switch<'a> {
    battlenet: &'a BattlenetAdapter<'a>,
    platform: &'a Platform,
    backups: &'a BackupService<'a>,
    directory: &'a dyn Directory,
    account: &'a AccountRef,
    cancel: &'a CancelToken,
    sink: &'a mut dyn SwitchSink,
    install: LauncherInstall,
    start_exe: String,
    config_path: String,
    config_exists: bool,
    // The plan found the target login first in the saved-accounts list already.
    login_first: bool,
    strategy: SwitchStrategy,
    in_place: bool,
    already_active: bool,
    steps: Vec<JobStep>,
    index: HashMap<&'static str, usize>,
    current: Option<&'static str>,
    backup_id: Option<String>,
    modified: bool,
    // Once the client has been started again a stop no longer puts anything back: the client is reading what was written.
    launcher_started: bool,
    handed_over: bool,
    expected_uid: Option<String>,
    // The fingerprint of what the switch left under UnifiedAuth, so that a token the client writes afterwards is told from the switch's own.
    written_fingerprint: Option<String>,
    // The target login is first in the saved-accounts list, so a token the client refuses is a bad token.
    list_ready: bool,
    log_at: LogMark,
    ready_at: Option<OffsetDateTime>,
}

impl Switch<'_> {
    fn now(&self) -> OffsetDateTime {
        self.platform.clock.now()
    }

    fn mark(&mut self, id: &'static str, status: JobStepStatus, detail: impl Into<String>) {
        let Some(at) = self.index.get(id).copied() else {
            return;
        };
        self.steps[at].status = status;
        self.steps[at].detail = detail.into();
        self.current = (status == JobStepStatus::Running).then_some(id);
        self.sink.progress(&self.steps);
    }

    fn planned(&self, id: &'static str) -> bool {
        self.index
            .get(id)
            .is_some_and(|at| self.steps[*at].status != JobStepStatus::Skipped)
    }

    fn stop_if_aborted(&self) -> Result<(), Thrown> {
        if self.cancel.is_cancelled() {
            Err(Thrown::Aborted)
        } else {
            Ok(())
        }
    }

    fn is_running(&self) -> Result<bool, Thrown> {
        Ok(self.battlenet.is_running()?)
    }

    // Records the account id on the account, then encrypts and stores the token for it. The id comes first and is refused when another account holds it, so no token is ever stored under an id the account was not given.
    fn store_session(
        &self,
        account: &AccountRef,
        token: &TokenValue,
        uid: Option<&str>,
    ) -> Result<(), AdapterError> {
        let Some(owner_id) = account.id.as_deref() else {
            return Ok(());
        };
        let uid = account_id(uid);
        let linked = account
            .launcher_uid
            .as_deref()
            .filter(|linked| !linked.is_empty());
        if let Some(uid) = &uid
            && linked.is_none_or(|linked| linked.to_lowercase() != *uid)
        {
            self.directory.set_launcher_uid(owner_id, uid)?;
        }
        let blob = BattlenetSessionBlob {
            unified_auth: token.clone(),
        };
        let sealed = Secret::new(serialize_blob(&blob));
        self.directory.save_session(
            owner_id,
            uid.as_deref(),
            &fingerprint_of(&token.hex),
            &sealed,
        )?;
        Ok(())
    }

    // Runs once the client is closed: the token is then the last run's, and the client's log names the account. A token that cannot be saved costs one sign-in by hand later, never the switch.
    fn save_outgoing(&mut self) {
        if !self.planned(STEP.capture) {
            return;
        }
        self.mark(STEP.capture, JobStepStatus::Running, "");
        let saved = (|| -> Result<String, AdapterError> {
            let fresh = self.battlenet.current_session(false)?;
            let Some(token) = &fresh.token else {
                return Ok("The client left no remembered session behind (signed out).".to_owned());
            };
            let Some((owner, owner_id)) = fresh
                .owner
                .as_ref()
                .and_then(|owner| Some((owner, owner.id.as_deref()?)))
            else {
                return Ok(
                    "The remembered session belongs to no One Launch account; left alone."
                        .to_owned(),
                );
            };
            let name = &owner.display_name;
            let known = self.directory.session(owner_id)?;
            if known.is_some_and(|known| Some(&known.fingerprint) == fresh.fingerprint.as_ref()) {
                return Ok(format!("{name}'s stored session is already current."));
            }
            let uid = owner
                .launcher_uid
                .as_deref()
                .or(fresh.logged_uid.as_deref());
            self.store_session(owner, token, uid)?;
            Ok(format!(
                "Saved {name}'s session (fingerprint {}).",
                short_fingerprint(fresh.fingerprint.as_deref())
            ))
        })();
        let detail = saved.unwrap_or_else(|error| format!("Not saved: {error}"));
        self.mark(STEP.capture, JobStepStatus::Done, detail);
    }

    fn take_snapshot(&mut self) -> Result<(), Thrown> {
        self.mark(STEP.backup, JobStepStatus::Running, "");
        let reason = format!("Before switching to {}", self.account.display_name);
        let taken = snapshot(
            self.battlenet,
            self.backups,
            &reason,
            None,
            self.config_exists,
        )
        .map_err(|error| Thrown::Failed(error.message()))?;
        self.sink.backup(&taken.id);
        self.backup_id = Some(taken.id);
        self.mark(STEP.backup, JobStepStatus::Done, taken.path);
        Ok(())
    }

    // The stored token is decrypted here, written once, and dropped. Whatever else is under UnifiedAuth goes, as it does at the client's own logout.
    fn write_registry(&mut self) -> Result<(), Thrown> {
        self.mark(STEP.registry, JobStepStatus::Running, "");
        let mut blob = None;
        if let (SwitchStrategy::Remembered, Some(account_id)) =
            (self.strategy, self.account.id.as_deref())
        {
            let stored = self.directory.decrypt_session(account_id)?.ok_or_else(|| {
                Thrown::Failed(format!(
                    "No session snapshot is stored for {}.",
                    self.account.display_name
                ))
            })?;
            blob = Some(
                parse_blob(stored.expose()).map_err(|error| Thrown::Failed(error.to_string()))?,
            );
        }
        let wanted = blob.as_ref().map(|blob| &blob.unified_auth);
        let is_wanted = |name: &str| {
            wanted.is_some_and(|wanted| wanted.name.to_lowercase() == name.to_lowercase())
        };
        let existing = self.battlenet.read_tokens()?;
        let registry = self.platform.registry.as_ref();
        let mut changes = 0;
        if let Some(wanted) = wanted {
            let in_place = existing
                .iter()
                .any(|token| is_wanted(&token.name) && token.hex == wanted.hex);
            if !in_place {
                self.modified = true;
                set_unified_auth(registry, wanted)?;
                changes += 1;
            }
        }
        for token in existing.iter().filter(|token| !is_wanted(&token.name)) {
            self.modified = true;
            delete_unified_auth(registry, &token.name)?;
            changes += 1;
        }
        self.written_fingerprint = wanted.map(|wanted| fingerprint_of(&wanted.hex));
        let (status, detail) = match (&self.written_fingerprint, changes) {
            (Some(_), 0) => (JobStepStatus::Skipped, "Already in place.".to_owned()),
            (None, 0) => (JobStepStatus::Skipped, "Already signed out.".to_owned()),
            (Some(fingerprint), _) => (
                JobStepStatus::Done,
                format!(
                    "UnifiedAuth now holds the snapshot (fingerprint {}).",
                    short_fingerprint(Some(fingerprint))
                ),
            ),
            (None, _) => (JobStepStatus::Done, "UnifiedAuth cleared.".to_owned()),
        };
        self.mark(STEP.registry, status, detail);
        Ok(())
    }

    // The file is read again here: the client wrote it when it exited, after the plan was made.
    fn write_file(&mut self) -> Result<(), Thrown> {
        if !self.planned(STEP.file) {
            self.list_ready = self.login_first;
            return Ok(());
        }
        self.mark(STEP.file, JobStepStatus::Running, "");
        let account = self.account;
        let login = &account.login;
        let read = if self.config_exists {
            self.battlenet.read_config_text()?
        } else {
            None
        };
        let Some((text, exact)) = read else {
            let detail = "Battle.net.config is missing; One Launch never creates it.";
            self.mark(STEP.file, JobStepStatus::Skipped, detail);
            return Ok(());
        };
        // A byte that is not UTF-8 would be written back as another, so such a file is left as it is.
        let planned = if exact {
            plan_saved_account_names(&text, login)
        } else {
            Err(ConfigError::NoRoundTrip)
        };
        let write = match planned {
            Ok(write) => write,
            Err(error) => {
                let detail = format!("Left alone: {error}");
                self.mark(STEP.file, JobStepStatus::Skipped, detail);
                return Ok(());
            }
        };
        self.list_ready = true;
        if write.noop {
            let detail = format!("{login} is already first in Client.SavedAccountNames.");
            self.mark(STEP.file, JobStepStatus::Skipped, detail);
            return Ok(());
        }
        self.modified = true;
        write_battlenet_config(self.platform.files.as_ref(), &self.config_path, &write.text)?;
        let detail = format!(
            "{login} is now first of {}.",
            plural(write.after.len(), "saved account")
        );
        self.mark(STEP.file, JobStepStatus::Done, detail);
        Ok(())
    }

    // Something other than this switch started the client. Before the writes nothing is lost by stopping; after them the snapshot goes back.
    fn started_meanwhile(&mut self, at: &'static str) -> Thrown {
        self.mark(at, JobStepStatus::Running, "");
        Thrown::Failed(format!(
            "Battle.net was started by something else while One Launch was switching accounts, so the switch {}. Try again.",
            if self.modified {
                "was stopped"
            } else {
                "stopped before changing anything"
            }
        ))
    }

    fn start_client(&mut self) -> Result<(), Thrown> {
        self.mark(STEP.start, JobStepStatus::Running, "");
        self.stop_if_aborted()?;
        self.log_at = self.battlenet.log_mark();
        self.launcher_started = true;
        self.platform
            .processes
            .start(&self.start_exe, &[], &self.install.install_dir)?;
        self.mark(STEP.start, JobStepStatus::Done, "Client started.");
        self.mark(STEP.wait, JobStepStatus::Running, "");
        let timing = *self.battlenet.timing();
        let battlenet = self.battlenet;
        let appeared = wait_until(
            self.platform.clock.as_ref(),
            self.cancel,
            timing.startup,
            timing.poll,
            || battlenet.is_running(),
        )?;
        if !appeared {
            return Err(Thrown::Failed(format!(
                "{EXE} did not appear within {} s",
                seconds(timing.startup)
            )));
        }
        self.mark(STEP.wait, JobStepStatus::Done, format!("{EXE} is running."));
        Ok(())
    }

    // Reads what the client has logged since it was started. While the user signs in by hand a failed try, a wrong password say, settles nothing.
    fn wait_for_sign_in(&mut self, timeout: Duration, manual: bool) -> Result<SignInWait, Thrown> {
        let timing = *self.battlenet.timing();
        let deadline = self.now() + timeout;
        loop {
            if self.cancel.is_cancelled() {
                return Ok(SignInWait::Cancelled);
            }
            match last_sign_in(&self.battlenet.events_since(&self.log_at)) {
                Some(SignInOutcome::SignedIn { account_id, .. }) => {
                    let owner = if account_id.is_empty() {
                        None
                    } else {
                        self.directory.account_by_launcher_uid(&account_id)?
                    };
                    match &self.expected_uid {
                        // The log holds an id and no name, so an id nobody holds is taken as this account's. One that another account holds is not.
                        None => {
                            let another = owner
                                .as_ref()
                                .is_some_and(|owner| owner.id != self.account.id);
                            return Ok(if another {
                                SignInWait::WrongAccount { account_id, owner }
                            } else {
                                SignInWait::SignedIn { account_id }
                            });
                        }
                        Some(expected) if *expected == account_id => {
                            return Ok(SignInWait::SignedIn { account_id });
                        }
                        Some(_) if !account_id.is_empty() => {
                            return Ok(SignInWait::WrongAccount { account_id, owner });
                        }
                        Some(_) => {}
                    }
                }
                Some(SignInOutcome::Failed { reason }) if !manual => {
                    return Ok(SignInWait::Failed { reason });
                }
                _ => {}
            }
            if self.now() >= deadline {
                return Ok(SignInWait::Timeout);
            }
            self.platform.clock.sleep(timing.log_poll, self.cancel);
        }
    }

    fn wrong_account(&self, account_id: &str, owner: Option<&AccountRef>) -> Thrown {
        let tail = id_tail(Some(account_id), 4);
        let who = match owner {
            Some(owner) => format!("{} (account {tail})", owner.display_name),
            None => format!("account {tail}"),
        };
        let (name, login) = (&self.account.display_name, &self.account.login);
        Thrown::Failed(match &self.expected_uid {
            None => format!(
                "Battle.net signed in as {who}, which is another One Launch account, instead of {name}. Sign in to the client as {login}."
            ),
            Some(expected) => format!(
                "Battle.net signed in as {who} instead of {name} ({}). Sign in to the client as {login}, or fix the account ID on the account page.",
                id_tail(Some(expected), 4)
            ),
        })
    }

    // The first sign-in that is confirmed is where the account's Battle.net id comes from. Refused when another account holds it.
    fn link_if_new(&mut self, signed_in_as: &str) -> Result<String, Thrown> {
        let Some(account_id) = self.account.id.as_deref() else {
            return Ok(String::new());
        };
        if signed_in_as.is_empty() || self.expected_uid.is_some() {
            return Ok(String::new());
        }
        self.directory.set_launcher_uid(account_id, signed_in_as)?;
        self.expected_uid = Some(signed_in_as.to_owned());
        Ok(format!(
            " Linked to Battle.net account {}.",
            id_tail(Some(signed_in_as), 4)
        ))
    }

    fn signed_in_detail(&mut self, account_id: &str) -> Result<String, Thrown> {
        let linked = self.link_if_new(account_id)?;
        Ok(format!("Signed in as {}.{linked}", self.account.login))
    }

    // With a token the client signs in by itself and the log is watched for it. Without one it waits on its login page: one look, in case it is signed in already, and the user takes over.
    fn confirm_sign_in(&mut self) -> Result<SignInWait, Thrown> {
        let timing = *self.battlenet.timing();
        if self.strategy == SwitchStrategy::Credentials {
            let waited = self.wait_for_sign_in(Duration::from_millis(1), false)?;
            let (status, detail) = match &waited {
                SignInWait::SignedIn { account_id } => {
                    (JobStepStatus::Done, self.signed_in_detail(account_id)?)
                }
                SignInWait::WrongAccount { .. } => (
                    JobStepStatus::Failed,
                    "Signed in as a different account.".to_owned(),
                ),
                _ => (
                    JobStepStatus::Skipped,
                    "Handed over to you: the client is on its login page.".to_owned(),
                ),
            };
            self.mark(STEP.verify, status, detail);
            return Ok(match waited {
                SignInWait::Failed { .. } => SignInWait::Timeout,
                other => other,
            });
        }
        self.mark(STEP.verify, JobStepStatus::Running, "");
        let waited = self.wait_for_sign_in(timing.sign_in, false)?;
        let (status, detail) = match &waited {
            SignInWait::SignedIn { account_id } => {
                (JobStepStatus::Done, self.signed_in_detail(account_id)?)
            }
            SignInWait::WrongAccount { .. } => (
                JobStepStatus::Failed,
                "Signed in as a different account.".to_owned(),
            ),
            SignInWait::Failed { reason } => (
                JobStepStatus::Failed,
                format!(
                    "The client rejected the session: {}.",
                    if reason.is_empty() {
                        "no reason given"
                    } else {
                        reason
                    }
                ),
            ),
            SignInWait::Timeout => (
                JobStepStatus::Failed,
                "The client did not report a sign-in in time.".to_owned(),
            ),
            SignInWait::Cancelled => (JobStepStatus::Failed, "Cancelled.".to_owned()),
        };
        self.mark(STEP.verify, status, detail);
        Ok(waited)
    }

    // The client sits on its login page: the user takes over, and the wait is for the sign-in to land in the log.
    fn wait_for_manual_sign_in(&mut self, why: &SignInWait) -> Result<SignInWait, Thrown> {
        let timing = *self.battlenet.timing();
        let until = ol_store::iso_timestamp(self.now() + timing.manual_sign_in);
        let reason = match why {
            SignInWait::Failed { reason } => Some(reason.clone()),
            _ => None,
        };
        let cause = match (self.strategy, &reason) {
            (SwitchStrategy::Credentials, _) => Cause::New,
            (SwitchStrategy::Remembered, Some(_)) => Cause::Expired,
            (SwitchStrategy::Remembered, None) => Cause::Other,
        };
        let attention = attention_for(
            &self.account.login,
            cause,
            reason,
            self.account.has_secret,
            until,
        );
        self.mark(STEP.manual, JobStepStatus::Running, attention.title.clone());
        self.handed_over = true;
        self.sink.attention(Some(&attention));
        let waited = self.wait_for_sign_in(timing.manual_sign_in, true);
        let settled = match &waited {
            Ok(SignInWait::SignedIn { account_id }) => self
                .signed_in_detail(account_id)
                .map(|detail| Some((JobStepStatus::Done, detail))),
            Ok(SignInWait::WrongAccount { .. }) => Ok(Some((
                JobStepStatus::Failed,
                "Signed in as a different account.".to_owned(),
            ))),
            Ok(SignInWait::Cancelled) => {
                Ok(Some((JobStepStatus::Failed, "Stopped waiting.".to_owned())))
            }
            Ok(_) => Ok(Some((
                JobStepStatus::Failed,
                format!("No sign-in after {} min.", minutes(timing.manual_sign_in)),
            ))),
            Err(_) => Ok(None),
        };
        if let Ok(Some((status, detail))) = &settled {
            self.mark(STEP.manual, *status, detail.clone());
        }
        self.sink.attention(None);
        settled?;
        waited
    }

    // After a confirmed sign-in: stores the token the client wrote at that sign-in, when it is not the one the switch put there. The client writes a new one at every sign-in by hand, and may at any other.
    fn capture_fresh(&mut self, wait: Duration) -> Result<(), Thrown> {
        let Some(account_id) = self.account.id.clone() else {
            return Ok(());
        };
        self.mark(STEP.recapture, JobStepStatus::Running, "");
        let timing = *self.battlenet.timing();
        let deadline = self.now() + wait;
        loop {
            let tokens = self.battlenet.read_tokens()?;
            let written = self.written_fingerprint.as_deref();
            let fresh = tokens
                .iter()
                .find(|token| Some(fingerprint_of(&token.hex).as_str()) != written);
            if let Some(fresh) = fresh {
                self.store_session(self.account, fresh, self.expected_uid.as_deref())?;
                self.directory.mark_session_verified(&account_id)?;
                let detail = format!(
                    "Stored {}'s session (fingerprint {}).",
                    self.account.display_name,
                    short_fingerprint(Some(&fingerprint_of(&fresh.hex)))
                );
                self.mark(STEP.recapture, JobStepStatus::Done, detail);
                return Ok(());
            }
            if self.now() >= deadline || self.cancel.is_cancelled() {
                let stored = self.directory.session(&account_id)?;
                let current = tokens.first().is_some_and(|kept| {
                    stored.is_some_and(|stored| stored.fingerprint == fingerprint_of(&kept.hex))
                });
                if current {
                    self.directory.mark_session_verified(&account_id)?;
                    self.mark(
                        STEP.recapture,
                        JobStepStatus::Done,
                        "The stored session is current.",
                    );
                } else {
                    self.mark(
                        STEP.recapture,
                        JobStepStatus::Done,
                        "The client has not written a new token yet; it is saved at the next switch.",
                    );
                }
                return Ok(());
            }
            self.platform.clock.sleep(timing.log_poll, self.cancel);
        }
    }

    fn attempt(&mut self, running: bool) -> Result<Ending, Thrown> {
        let timing = *self.battlenet.timing();
        let remembered = self.strategy == SwitchStrategy::Remembered;
        self.stop_if_aborted()?;
        if running && !self.already_active {
            self.mark(STEP.close, JobStepStatus::Running, "");
            self.battlenet.shutdown(None, self.cancel)?;
            self.mark(STEP.close, JobStepStatus::Done, "Client closed.");
        }
        self.stop_if_aborted()?;
        // A running client writes its token again at the next sign-in and its config when it exits, so nothing is read or written against one.
        if !self.already_active && self.is_running()? {
            return Err(self.started_meanwhile(STEP.close));
        }

        self.save_outgoing();
        if !self.already_active && !self.in_place {
            self.take_snapshot()?;
            self.write_registry()?;
            self.write_file()?;
        }

        if self.is_running()? {
            if !self.already_active {
                return Err(self.started_meanwhile(STEP.start));
            }
            let detail = "The client is already running.";
            self.mark(STEP.start, JobStepStatus::Skipped, detail);
            self.mark(STEP.wait, JobStepStatus::Skipped, detail);
        } else {
            self.start_client()?;
        }

        if self.already_active && running {
            self.mark(
                STEP.verify,
                JobStepStatus::Skipped,
                "The client is already signed in as this account.",
            );
            return Ok(Ending::Done);
        }
        match self.confirm_sign_in()? {
            SignInWait::Cancelled => return Ok(Ending::Cancelled),
            SignInWait::WrongAccount { account_id, owner } => {
                return Err(self.wrong_account(&account_id, owner.as_ref()));
            }
            SignInWait::SignedIn { .. } => {
                if remembered {
                    self.capture_fresh(Duration::ZERO)?;
                }
            }
            unsettled => {
                // A refused token is stale only when the login was first in the list: otherwise the client refuses any token.
                if let (SignInWait::Failed { reason }, true, true, Some(account_id)) = (
                    &unsettled,
                    remembered,
                    self.list_ready,
                    self.account.id.as_deref(),
                ) {
                    let reason = if reason.is_empty() {
                        "rejected by the client"
                    } else {
                        reason
                    };
                    self.directory.mark_session_stale(account_id, reason)?;
                }
                match self.wait_for_manual_sign_in(&unsettled)? {
                    SignInWait::Cancelled => return Ok(Ending::Cancelled),
                    SignInWait::WrongAccount { account_id, owner } => {
                        return Err(self.wrong_account(&account_id, owner.as_ref()));
                    }
                    SignInWait::SignedIn { .. } => self.capture_fresh(timing.capture)?,
                    _ => return Ok(Ending::NeedsSignIn),
                }
            }
        }
        self.ready_at = Some(self.now() + timing.ready_grace);
        Ok(Ending::Done)
    }

    // The client writes its token at a sign-in and its config when it exits, so it is closed before the snapshot goes back. Nothing stops this once it has begun.
    // A client that will not close keeps the snapshot for the next start: it would write its own token and list over the ones put back.
    fn roll_back(&self, backup_id: &str) -> Result<(), String> {
        let text = |error: AdapterError| error.to_string();
        if self.battlenet.is_running().map_err(text)? {
            let closed = self.battlenet.shutdown(None, &CancelToken::new());
            if self.battlenet.is_running().map_err(text)? {
                return Err(closed.err().map_or_else(
                    || "Battle.net is still running, and it rewrites Battle.net.config while it runs".to_owned(),
                    text,
                ));
            }
        }
        restore_battlenet(self.battlenet, self.platform, self.backups, backup_id)
            .map(|_| ())
            .map_err(|error| error.message())
    }
}

// Makes `account` the one Battle.net signs in. A refusal before anything is touched is an error; from the first step on, whatever happens comes back as a result that says what was put back.
pub fn run_switch(
    battlenet: &BattlenetAdapter<'_>,
    platform: &Platform,
    backups: &BackupService<'_>,
    account: &AccountRef,
    options: &SwitchOptions<'_>,
    sink: &mut dyn SwitchSink,
) -> Result<SwitchResult, ServiceError> {
    let adapter = ServiceError::adapter(LauncherId::Battlenet);
    let started_at = ol_store::iso_timestamp(platform.clock.now());
    let (plan, context) = battlenet.build_switch_plan(account).map_err(&adapter)?;
    if context.running && !context.already_active && !options.confirm_close {
        let running_game = battlenet.running_game().map_err(&adapter)?;
        return Err(adapter(AdapterError::ConfirmationRequired {
            message: close_message(running_game.as_ref()),
            running_game,
        }));
    }

    let (steps, index) = job_steps(&plan);
    let mut switch = Switch {
        battlenet,
        platform,
        backups,
        directory: battlenet.directory(),
        account,
        cancel: options.cancel,
        sink,
        log_at: battlenet.log_mark(),
        expected_uid: plan
            .account
            .launcher_uid
            .as_ref()
            .map(|uid| uid.to_lowercase()),
        written_fingerprint: context.current.fingerprint.clone(),
        install: context.install,
        start_exe: context.start_exe,
        config_path: context.config_path,
        config_exists: context.config_exists,
        login_first: context.login_first,
        strategy: plan.strategy,
        in_place: context.in_place,
        already_active: context.already_active,
        steps,
        index,
        current: None,
        backup_id: None,
        modified: false,
        launcher_started: false,
        handed_over: false,
        list_ready: false,
        ready_at: None,
    };

    let attempt = switch.attempt(context.running);
    let login = account.login.clone();
    let manual_minutes = minutes(battlenet.timing().manual_sign_in);
    let cancelled_after_start = |handed_over: bool| {
        if handed_over {
            format!(
                "Stopped waiting for the sign-in as {login}. Battle.net was left on its login page; signing in there finishes the switch, and the session is saved at the next one."
            )
        } else {
            format!(
                "Stopped while Battle.net was starting as {login}. Nothing was rolled back: the client carries on signing in on its own."
            )
        }
    };

    let (ok, needs_sign_in, cancelled, restored, error) = match attempt {
        Ok(Ending::Done) => (true, false, false, None, None),
        Ok(Ending::Cancelled) => (
            false,
            false,
            true,
            None,
            Some(cancelled_after_start(switch.handed_over)),
        ),
        Ok(Ending::NeedsSignIn) => (
            false,
            true,
            false,
            None,
            Some(format!(
                "One Launch waited {manual_minutes} min for the sign-in as {login} and gave up. Battle.net is still on its login page: sign in there, and the session is saved at the next switch."
            )),
        ),
        Err(thrown) => {
            let cancelled = matches!(thrown, Thrown::Aborted);
            let message = match thrown {
                Thrown::Aborted => format!("Stopped before Battle.net was started as {login}."),
                Thrown::Failed(message) => message,
            };
            if let Some(current) = switch.current {
                let detail = if cancelled {
                    "Stopped.".to_owned()
                } else {
                    message.clone()
                };
                switch.mark(current, JobStepStatus::Failed, detail);
            }
            if cancelled && switch.launcher_started {
                (
                    false,
                    false,
                    true,
                    None,
                    Some(cancelled_after_start(switch.handed_over)),
                )
            } else if !switch.modified {
                let error = if cancelled {
                    format!("{message} Nothing had been changed.")
                } else {
                    message
                };
                (false, false, cancelled, None, Some(error))
            } else {
                match switch.backup_id.clone() {
                    None => (
                        false,
                        false,
                        cancelled,
                        Some(false),
                        Some(format!("{message} (no backup was available to restore)")),
                    ),
                    Some(backup_id) => match switch.roll_back(&backup_id) {
                        Ok(()) => (false, false, cancelled, Some(true), Some(message)),
                        Err(reason) => (
                            false,
                            false,
                            cancelled,
                            Some(false),
                            Some(format!(
                                "{message}. Restoring backup {backup_id} also failed: {reason}"
                            )),
                        ),
                    },
                }
            }
        }
    };

    Ok(SwitchResult {
        account: plan.account,
        strategy: plan.strategy,
        ok,
        needs_sign_in,
        cancelled,
        backup_id: switch.backup_id,
        restored,
        error,
        steps: switch.steps,
        started_at,
        finished_at: ol_store::iso_timestamp(platform.clock.now()),
        ready_at: switch.ready_at.filter(|_| ok),
        started_with_game: None,
    })
}
