// Riot's switch: refuse while a Riot game runs, close the client, save the sign-in it leaves behind for its owner, copy RiotGamesPrivateSettings.yaml, put the target account's saved copy of the whole file in its place (or the client's own signed-out state, so that the client asks), start the client as its shortcut does and read its log and the file for the sign-in. It reads, waits and decides; every write goes through the two functions for Riot at the top of switch.rs, and neither takes a write that cannot show a `ReadyToWrite`: the client was seen closed, and what the file held was stored for its owner. A saved sign-in is decrypted in one place, `write_session`, and goes nowhere but into the client's own file.

use std::collections::HashMap;
use std::time::Duration;

use ol_adapters::riot::{
    CLOSE_MESSAGE, EXE, LogMark, RiotAdapter, SESSION_FILE, SESSION_FILE_RELATIVE, STEP,
    UNOWNED_SESSION_PIN, id_tail,
};
use ol_adapters::steam::{minutes, seconds};
use ol_adapters::{AccountRef, AdapterError, Directory, GameRef};
use ol_formats::riot::client_log::{FailReason, SignInOutcome, last_sign_in};
use ol_formats::riot::products::{RiotProductId, parse_product_id};
use ol_formats::riot::session::{
    RiotSession, RiotSessionBlob, SessionRead, parse_blob, plan_sign_out, serialize_blob,
    session_of_blob, short_fingerprint,
};
use ol_model::{
    BackupSnapshot, JobStep, JobStepStatus, LaunchAttention, LauncherId, LauncherInstall,
    RestoreResult, SwitchStrategy,
};
use ol_platform_api::{CancelToken, Platform};
use ol_runtime::wait_until;
use ol_vault::Secret;
use serde_json::Map;
use time::OffsetDateTime;

use super::{
    SwitchOptions, SwitchResult, SwitchSink, Thrown, job_steps, restore_riot, write_riot_session,
};
use crate::backup::{BackupError, BackupFile, BackupService, SnapshotRequest};
use crate::launchers::ServiceError;

// The client was seen closed a moment ago. Only `seen_closed` makes one.
struct Closed(());

// What every write of the session file has to show: the client was seen closed, and the sign-in the file held was stored for the account it belongs to, because the client replaces the token at every start and an older copy must never be the one that is kept. Only `save_what_the_file_holds` makes one, here, in this module.
pub(super) struct ReadyToWrite(());

fn seen_closed(riot: &RiotAdapter<'_>) -> Result<Option<Closed>, AdapterError> {
    Ok((!riot.is_running()?).then_some(Closed(())))
}

// Records the Riot account id on the account, then encrypts and stores the file for it. The id comes first and is refused when another account holds it, so no sign-in is ever stored under an id the account was not given.
fn store_session(
    directory: &dyn Directory,
    account: &AccountRef,
    session: &RiotSession,
    file: &RiotSessionBlob,
) -> Result<(), AdapterError> {
    let Some(account_id) = account.id.as_deref() else {
        return Ok(());
    };
    let uid = session.account_id.as_str();
    let linked = account
        .launcher_uid
        .as_deref()
        .filter(|linked| !linked.is_empty());
    if linked.is_none_or(|linked| linked.to_lowercase() != uid) {
        directory.set_launcher_uid(account_id, uid)?;
    }
    let sealed = Secret::new(serialize_blob(file));
    directory.save_session(account_id, Some(uid), &session.fingerprint, &sealed)?;
    Ok(())
}

// Stores the sign-in the file holds for the One Launch account it belongs to, when it changed since it was last stored, and says in one sentence what came of it. A sign-in that cannot be saved costs one sign-in by hand later, never the switch; one that nobody owns is left alone.
fn save_what_the_file_holds(riot: &RiotAdapter<'_>, _closed: &Closed) -> (ReadyToWrite, String) {
    let saved = (|| -> Result<String, AdapterError> {
        let current = riot.current_session()?;
        let (Some(session), Some(file)) = (&current.session, &current.file) else {
            return Ok("The client left no saved sign-in behind (signed out).".to_owned());
        };
        let Some((owner, owner_id)) = current
            .owner
            .as_ref()
            .and_then(|owner| Some((owner, owner.id.as_deref()?)))
        else {
            return Ok(
                "The saved sign-in belongs to no One Launch account; left alone.".to_owned(),
            );
        };
        let name = &owner.display_name;
        let known = riot.directory().session(owner_id)?;
        if known.is_some_and(|known| known.fingerprint == session.fingerprint) {
            return Ok(format!("{name}'s saved sign-in is already current."));
        }
        store_session(riot.directory(), owner, session, file)?;
        Ok(format!(
            "Saved {name}'s sign-in (fingerprint {}).",
            short_fingerprint(Some(&session.fingerprint))
        ))
    })();
    let detail = saved.unwrap_or_else(|error| format!("Not saved: {error}"));
    (ReadyToWrite(()), detail)
}

// The session file as a snapshot takes it: encrypted, because it is a sign-in.
fn snapshot(
    riot: &RiotAdapter<'_>,
    backups: &BackupService<'_>,
    reason: &str,
    protect: Option<&str>,
    pinned: bool,
) -> Result<BackupSnapshot, BackupError> {
    let source = riot.session_file_path();
    backups.snapshot(&SnapshotRequest {
        launcher: LauncherId::Riot,
        reason,
        files: &[BackupFile {
            source: &source,
            relative: SESSION_FILE_RELATIVE,
            sensitive: true,
        }],
        meta: Map::new(),
        protect,
        pinned: pinned.then_some(UNOWNED_SESSION_PIN),
    })
}

// Puts a snapshot back from Settings or after a crash. Refused while the client runs, because it rewrites the file while it runs, and while a Riot game runs. What is there now is snapshotted first, so restoring the wrong one can be undone, and stored for its owner.
pub fn restore_backup(
    riot: &RiotAdapter<'_>,
    platform: &Platform,
    backups: &BackupService<'_>,
    id: &str,
) -> Result<RestoreResult, ServiceError> {
    let adapter = ServiceError::adapter(LauncherId::Riot);
    riot.require_install().map_err(&adapter)?;
    let Some(closed) = seen_closed(riot).map_err(&adapter)? else {
        return Err(adapter(AdapterError::LauncherRunning(format!(
            "Exit the Riot Client from its tray icon before restoring: it rewrites {SESSION_FILE} while it runs, which would undo the restore."
        ))));
    };
    if let Some(game) = riot.find_running_game(None).map_err(&adapter)? {
        return Err(adapter(RiotAdapter::game_running_error(game, "restore")));
    }
    let unowned = riot.holds_unowned_session();
    snapshot(
        riot,
        backups,
        &format!("Before restoring {id}"),
        Some(id),
        unowned,
    )
    .map_err(ServiceError::backup(LauncherId::Riot))?;
    let (ready, _) = save_what_the_file_holds(riot, &closed);
    restore_riot(riot, platform, backups, id, &ready)
        .map_err(|error| error.into_service(LauncherId::Riot))
}

enum SignInWait {
    // The account id is None when the client has not written the file yet.
    SignedIn {
        account_id: Option<String>,
    },
    WrongAccount {
        account_id: String,
        owner: Option<AccountRef>,
    },
    Failed {
        reason: FailReason,
        detail: String,
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
    let said = reason.as_deref().filter(|reason| !reason.is_empty());
    LaunchAttention {
        title: if cause == Cause::New {
            format!("Sign in to the Riot Client as {login}")
        } else {
            format!("The Riot Client is asking you to sign in as {login}")
        },
        message: match cause {
            Cause::Expired => "The client refused its saved sign-in.".to_owned(),
            Cause::New => "No sign-in is saved for this account yet.".to_owned(),
            Cause::Other => format!(
                "The client did not sign in on its own{}.",
                said.map(|reason| format!(" ({reason})"))
                    .unwrap_or_default()
            ),
        },
        steps: vec![
            format!("In the Riot Client window, enter the login {login} and your password."),
            "Tick “Stay signed in” before you sign in, or the client forgets the account when it closes.".to_owned(),
            "Press Sign in. One Launch continues on its own, and later switches are automatic."
                .to_owned(),
        ],
        reason,
        can_copy_password,
        until,
    }
}

struct Switch<'a> {
    riot: &'a RiotAdapter<'a>,
    platform: &'a Platform,
    backups: &'a BackupService<'a>,
    directory: &'a dyn Directory,
    account: &'a AccountRef,
    // The game a launch follows the switch with: the client is started with its launch arguments.
    game: Option<(RiotProductId, GameRef<'a>)>,
    cancel: &'a CancelToken,
    sink: &'a mut dyn SwitchSink,
    install: LauncherInstall,
    session_path: String,
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
    // The fingerprint of the sign-in the switch left in the file, so that what the client writes afterwards is told from the switch's own.
    written_fingerprint: Option<String>,
    log_at: LogMark,
    ready_at: Option<OffsetDateTime>,
    started_with_game: Option<String>,
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

    fn stop_if_aborted(&self) -> Result<(), Thrown> {
        if self.cancel.is_cancelled() {
            Err(Thrown::Aborted)
        } else {
            Ok(())
        }
    }

    // Runs once the client is closed: the file then holds what the last run left. The step shows what came of it when the plan had it; when the plan passed it over the file is looked at all the same, so that nothing the client wrote since is lost to the write that follows.
    fn save_outgoing(&mut self, closed: &Closed) -> ReadyToWrite {
        let planned = self
            .index
            .get(STEP.capture)
            .is_some_and(|at| self.steps[*at].status != JobStepStatus::Skipped);
        if planned {
            self.mark(STEP.capture, JobStepStatus::Running, "");
        }
        let (ready, detail) = save_what_the_file_holds(self.riot, closed);
        if planned {
            self.mark(STEP.capture, JobStepStatus::Done, detail);
        }
        ready
    }

    fn take_snapshot(&mut self) -> Result<(), Thrown> {
        self.mark(STEP.backup, JobStepStatus::Running, "");
        // The client is closed by now, so the file is final: a sign-in nobody owns is about to be replaced, and this snapshot is its only copy.
        let unowned = !self.in_place && self.riot.holds_unowned_session();
        let taken = snapshot(
            self.riot,
            self.backups,
            &format!("Before switching to {}", self.account.display_name),
            None,
            unowned,
        )
        .map_err(|error| Thrown::Failed(error.to_string()))?;
        self.sink.backup(&taken.id);
        self.backup_id = Some(taken.id);
        self.mark(STEP.backup, JobStepStatus::Done, taken.path);
        Ok(())
    }

    // The stored copy is decrypted here and nowhere else, written once, and wiped.
    fn write_session(&mut self, ready: &ReadyToWrite) -> Result<(), Thrown> {
        self.mark(STEP.file, JobStepStatus::Running, "");
        let name = &self.account.display_name;
        // The client's file is never made up: it carries the mark Riot knows this PC by.
        let Some(on_disk) = self.riot.read_session_file()? else {
            return Err(Thrown::Failed(format!(
                "{} is missing. Start the Riot Client once by hand so it writes its settings file, then try again.",
                self.session_path
            )));
        };
        let files = self.platform.files.as_ref();
        if self.strategy == SwitchStrategy::Remembered {
            let none_saved = || Thrown::Failed(format!("No sign-in is saved for {name}."));
            let account_id = self.account.id.as_deref().ok_or_else(none_saved)?;
            let stored = self
                .directory
                .decrypt_session(account_id)?
                .ok_or_else(none_saved)?;
            let failed =
                |error: ol_formats::riot::session::BlobError| Thrown::Failed(error.to_string());
            let blob = parse_blob(stored.expose()).map_err(failed)?;
            let session = session_of_blob(&blob).map_err(failed)?;
            if blob.file == on_disk.file.file {
                self.written_fingerprint = Some(session.fingerprint);
                self.mark(STEP.file, JobStepStatus::Skipped, "Already in place.");
                return Ok(());
            }
            self.modified = true;
            write_riot_session(files, &self.session_path, blob.file.as_bytes(), ready)
                .map_err(|error| Thrown::Failed(error.to_string()))?;
            let detail = format!(
                "{SESSION_FILE} now holds the saved copy (fingerprint {}).",
                short_fingerprint(Some(&session.fingerprint))
            );
            self.written_fingerprint = Some(session.fingerprint);
            self.mark(STEP.file, JobStepStatus::Done, detail);
            return Ok(());
        }
        let write =
            plan_sign_out(&on_disk.file.file).map_err(|error| Thrown::Failed(error.to_string()))?;
        self.written_fingerprint = None;
        if write.noop {
            self.mark(STEP.file, JobStepStatus::Skipped, "Already signed out.");
            return Ok(());
        }
        // What is left of the file still holds the device cookie, so it is wiped like the file itself.
        let signed_out = RiotSessionBlob { file: write.text };
        self.modified = true;
        write_riot_session(files, &self.session_path, signed_out.file.as_bytes(), ready)
            .map_err(|error| Thrown::Failed(error.to_string()))?;
        self.mark(
            STEP.file,
            JobStepStatus::Done,
            format!(
                "{SESSION_FILE} now holds the client's signed-out state; everything else in it is as it was."
            ),
        );
        Ok(())
    }

    // Something other than this switch started the client. Before the write nothing is lost by stopping; after it the snapshot goes back.
    fn started_meanwhile(&mut self, at: &'static str) -> Thrown {
        self.mark(at, JobStepStatus::Running, "");
        Thrown::Failed(format!(
            "The Riot Client was started by something else while One Launch was switching accounts, so the switch {}. Try again.",
            if self.modified {
                "was stopped"
            } else {
                "stopped before changing anything"
            }
        ))
    }

    fn start_launcher(&mut self) -> Result<(), Thrown> {
        self.mark(STEP.start, JobStepStatus::Running, "");
        self.stop_if_aborted()?;
        self.log_at = self.riot.log_mark();
        self.launcher_started = true;
        let launch_for = self.game.as_ref().map(|(id, _)| id);
        self.riot.start_client(&self.install, launch_for)?;
        let detail = match &self.game {
            Some((_, game)) => {
                self.started_with_game = Some(game.launcher_game_id.to_owned());
                format!("Client started with {}'s launch arguments.", game.name)
            }
            None => "Client started.".to_owned(),
        };
        self.mark(STEP.start, JobStepStatus::Done, detail);
        self.mark(STEP.wait, JobStepStatus::Running, "");
        let timing = *self.riot.timing();
        let riot = self.riot;
        let appeared = wait_until(
            self.platform.clock.as_ref(),
            self.cancel,
            timing.startup,
            timing.poll,
            || riot.is_running(),
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

    // The sign-in the file names right now. None while it holds none, or is in the middle of being written.
    fn file_names(&self) -> Option<RiotSession> {
        match self.riot.read_session_file().ok()??.read {
            SessionRead::Session(session) => Some(session),
            _ => None,
        }
    }

    // Reads the client's new log for its verdict, then the file for the account id: the log never names the account, and the client writes the file before it logs that it is signed in. While the user signs in by hand a verdict that is not a sign-in, the login page or a sign-out, settles nothing.
    fn wait_for_sign_in(&mut self, timeout: Duration, manual: bool) -> Result<SignInWait, Thrown> {
        let timing = *self.riot.timing();
        let deadline = self.now() + timeout;
        let mut file_deadline = None;
        loop {
            if self.cancel.is_cancelled() {
                return Ok(SignInWait::Cancelled);
            }
            match last_sign_in(&self.riot.events_since(&self.log_at)) {
                Some(SignInOutcome::SignedIn { .. }) => {
                    if let Some(session) = self.file_names() {
                        let account_id = session.account_id;
                        let owner = self.directory.account_by_launcher_uid(&account_id)?;
                        return Ok(match &self.expected_uid {
                            // The file names an id and nothing else, so an id nobody holds is taken as this account's. One that another account holds is not.
                            None if owner
                                .as_ref()
                                .is_some_and(|owner| owner.id != self.account.id) =>
                            {
                                SignInWait::WrongAccount { account_id, owner }
                            }
                            Some(expected) if *expected != account_id => {
                                SignInWait::WrongAccount { account_id, owner }
                            }
                            _ => SignInWait::SignedIn {
                                account_id: Some(account_id),
                            },
                        });
                    }
                    let now = self.now();
                    if now >= *file_deadline.get_or_insert(now + timing.capture) {
                        return Ok(SignInWait::SignedIn { account_id: None });
                    }
                }
                Some(SignInOutcome::Failed { reason, detail }) if !manual => {
                    return Ok(SignInWait::Failed { reason, detail });
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
            Some(owner) => format!("{} (Riot account {tail})", owner.display_name),
            None => format!("Riot account {tail}"),
        };
        let (name, login) = (&self.account.display_name, &self.account.login);
        Thrown::Failed(match &self.expected_uid {
            None => format!(
                "The Riot Client signed in as {who}, which is another One Launch account, instead of {name}. Sign in to the client as {login}."
            ),
            Some(expected) => format!(
                "The Riot Client signed in as {who} instead of {name} ({}). Sign in to the client as {login}, or fix the Riot account id on the account page.",
                id_tail(Some(expected), 4)
            ),
        })
    }

    // The first sign-in that is confirmed is where the account's Riot id comes from. Refused when another account holds it.
    fn signed_in_detail(&mut self, signed_in_as: Option<&str>) -> Result<String, Thrown> {
        let login = &self.account.login;
        let Some(signed_in_as) = signed_in_as else {
            return Ok(format!(
                "Signed in as {login} (the client has not written {SESSION_FILE} yet, so the account id is unconfirmed)."
            ));
        };
        let (Some(account_id), None) = (self.account.id.as_deref(), &self.expected_uid) else {
            return Ok(format!("Signed in as {login}."));
        };
        self.directory.set_launcher_uid(account_id, signed_in_as)?;
        self.expected_uid = Some(signed_in_as.to_owned());
        Ok(format!(
            "Signed in as {login}. Linked to Riot account {}.",
            id_tail(Some(signed_in_as), 4)
        ))
    }

    fn confirm_sign_in(&mut self) -> Result<SignInWait, Thrown> {
        self.mark(STEP.verify, JobStepStatus::Running, "");
        let timing = *self.riot.timing();
        let remembered = self.strategy == SwitchStrategy::Remembered;
        let timeout = if remembered {
            timing.sign_in
        } else {
            Duration::from_millis(1)
        };
        let waited = self.wait_for_sign_in(timeout, false)?;
        let (status, detail) = match &waited {
            SignInWait::SignedIn { account_id } => (
                JobStepStatus::Done,
                self.signed_in_detail(account_id.as_deref())?,
            ),
            SignInWait::WrongAccount { .. } => (
                JobStepStatus::Failed,
                "Signed in as a different account.".to_owned(),
            ),
            SignInWait::Failed { detail, .. } => (
                JobStepStatus::Failed,
                format!("The client ended on its login page: {detail}."),
            ),
            SignInWait::Timeout if remembered => (
                JobStepStatus::Failed,
                "The client did not report a sign-in in time.".to_owned(),
            ),
            SignInWait::Timeout => (JobStepStatus::Failed, "Waiting for you.".to_owned()),
            SignInWait::Cancelled => (JobStepStatus::Failed, "Cancelled.".to_owned()),
        };
        self.mark(STEP.verify, status, detail);
        Ok(waited)
    }

    // The client sits on its login page: the user takes over, and the wait is for the sign-in to land in the log.
    fn wait_for_manual_sign_in(&mut self, why: &SignInWait) -> Result<SignInWait, Thrown> {
        let timing = *self.riot.timing();
        let until = ol_store::iso_timestamp(self.now() + timing.manual_sign_in);
        let refused = match why {
            SignInWait::Failed { reason, detail } => Some((*reason, detail.clone())),
            _ => None,
        };
        let cause = match (self.strategy, &refused) {
            (SwitchStrategy::Credentials, _) => Cause::New,
            (SwitchStrategy::Remembered, Some((FailReason::Rejected, _))) => Cause::Expired,
            (SwitchStrategy::Remembered, _) => Cause::Other,
        };
        let reason = refused
            .filter(|(reason, _)| *reason != FailReason::NoSession)
            .map(|(_, detail)| detail);
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
                .signed_in_detail(account_id.as_deref())
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

    // After a confirmed sign-in by hand: stores the file once the client has written a sign-in that differs from what the switch left there. The file names the account, so one that names another, because the user changed accounts in the client meanwhile, is not stored.
    fn capture_fresh(&mut self, wait: Duration) -> Result<(), Thrown> {
        let Some(account_id) = self.account.id.clone() else {
            return Ok(());
        };
        self.mark(STEP.recapture, JobStepStatus::Running, "");
        let timing = *self.riot.timing();
        let deadline = self.now() + wait;
        loop {
            let on_disk = self.riot.read_session_file()?;
            let fresh = on_disk.as_ref().and_then(|on_disk| match &on_disk.read {
                SessionRead::Session(session) => Some((session, &on_disk.file)),
                _ => None,
            });
            if let Some((session, file)) = fresh
                && Some(&session.fingerprint) != self.written_fingerprint.as_ref()
            {
                if self
                    .expected_uid
                    .as_ref()
                    .is_some_and(|expected| *expected != session.account_id)
                {
                    let detail = format!(
                        "Not saved: {SESSION_FILE} names Riot account {}, not this account's.",
                        id_tail(Some(&session.account_id), 4)
                    );
                    self.mark(STEP.recapture, JobStepStatus::Done, detail);
                    return Ok(());
                }
                store_session(self.directory, self.account, session, file)?;
                self.directory.mark_session_verified(&account_id)?;
                let detail = format!(
                    "Stored {}'s sign-in (fingerprint {}).",
                    self.account.display_name,
                    short_fingerprint(Some(&session.fingerprint))
                );
                self.mark(STEP.recapture, JobStepStatus::Done, detail);
                return Ok(());
            }
            if self.now() >= deadline || self.cancel.is_cancelled() {
                let stored = self.directory.session(&account_id)?;
                let current = fresh.is_some_and(|(session, _)| {
                    stored.is_some_and(|stored| stored.fingerprint == session.fingerprint)
                });
                if current {
                    self.directory.mark_session_verified(&account_id)?;
                    self.mark(
                        STEP.recapture,
                        JobStepStatus::Done,
                        "The saved sign-in is current.",
                    );
                } else {
                    self.mark(
                        STEP.recapture,
                        JobStepStatus::Done,
                        "The client has not written the new sign-in yet; it is saved at the next switch.",
                    );
                }
                return Ok(());
            }
            self.platform.clock.sleep(timing.log_poll, self.cancel);
        }
    }

    fn attempt(&mut self, running: bool) -> Result<Ending, Thrown> {
        let timing = *self.riot.timing();
        let remembered = self.strategy == SwitchStrategy::Remembered;
        self.stop_if_aborted()?;
        if running && !self.already_active {
            self.mark(STEP.close, JobStepStatus::Running, "");
            self.riot.shutdown(None, self.cancel)?;
            self.mark(STEP.close, JobStepStatus::Done, "Client closed.");
        }
        self.stop_if_aborted()?;
        if !self.already_active {
            // A running client rewrites the file when it exits and at every change of its token, so nothing is read or written against one.
            let Some(closed) = seen_closed(self.riot)? else {
                return Err(self.started_meanwhile(STEP.close));
            };
            let ready = self.save_outgoing(&closed);
            if !self.in_place {
                self.take_snapshot()?;
                self.write_session(&ready)?;
            }
        }

        if self.riot.is_running()? {
            if !self.already_active {
                return Err(self.started_meanwhile(STEP.start));
            }
            let detail = "The client is already running.";
            self.mark(STEP.start, JobStepStatus::Skipped, detail);
            self.mark(STEP.wait, JobStepStatus::Skipped, detail);
        } else {
            self.start_launcher()?;
        }

        if self.already_active {
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
                if let (true, Some(account_id)) = (remembered, self.account.id.as_deref())
                    && self.directory.session(account_id)?.is_some()
                {
                    self.directory.mark_session_verified(account_id)?;
                }
            }
            unsettled => {
                // Only a refused sign-in is stale. A failure of the service or the network says nothing about the saved copy, and marking it would cost a sign-in by hand.
                if let (
                    SignInWait::Failed {
                        reason: FailReason::Rejected,
                        detail,
                    },
                    true,
                    Some(account_id),
                ) = (&unsettled, remembered, self.account.id.as_deref())
                {
                    self.directory.mark_session_stale(account_id, detail)?;
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
        // A client that was started with the game's arguments is sent nothing afterwards, so there is no moment to wait for.
        if self.started_with_game.is_none() {
            self.ready_at = Some(self.now() + timing.ready_grace);
        }
        Ok(Ending::Done)
    }

    // Puts the file back after a failure. A Riot game that runs now is not disturbed: the client is not closed under it and nothing is written, and the snapshot stays for the user to put back once the game is closed. Otherwise the client is closed first, because it rewrites the file when it exits, and what the file holds, a sign-in the client rotated or one made by hand, goes to its owner before the file is overwritten. Nothing stops this once it has begun.
    fn roll_back(&self, backup_id: &str) -> Result<(), String> {
        let text = |error: AdapterError| error.to_string();
        if let Some(game) = self.riot.find_running_game(None).map_err(text)? {
            return Err(RiotAdapter::game_running_error(game, "restore").to_string());
        }
        if self.riot.is_running().map_err(text)? {
            let _ = self.riot.shutdown(None, &CancelToken::new());
        }
        let Some(closed) = seen_closed(self.riot).map_err(text)? else {
            return Err(format!(
                "the Riot Client is still running, and it rewrites {SESSION_FILE} while it runs"
            ));
        };
        let (ready, _) = save_what_the_file_holds(self.riot, &closed);
        restore_riot(self.riot, self.platform, self.backups, backup_id, &ready)
            .map(|_| ())
            .map_err(|error| error.message())
    }
}

// Makes `account` the one the Riot Client signs in. A refusal before anything is touched is an error; from the first step on, whatever happens comes back as a result that says what was put back.
pub fn run_switch(
    riot: &RiotAdapter<'_>,
    platform: &Platform,
    backups: &BackupService<'_>,
    account: &AccountRef,
    options: &SwitchOptions<'_>,
    sink: &mut dyn SwitchSink,
) -> Result<SwitchResult, ServiceError> {
    let adapter = ServiceError::adapter(LauncherId::Riot);
    let started_at = ol_store::iso_timestamp(platform.clock.now());
    let (plan, context) = riot
        .build_switch_plan(account, options.game)
        .map_err(&adapter)?;
    if let Some(game) = context.game {
        return Err(adapter(RiotAdapter::game_running_error(game, "switch")));
    }
    if !plan.executable {
        return Err(adapter(AdapterError::SwitchBlocked(
            plan.warnings
                .first()
                .cloned()
                .unwrap_or_else(|| "This switch cannot run.".to_owned()),
        )));
    }
    if context.running && !context.already_active && !options.confirm_close {
        return Err(adapter(AdapterError::ConfirmationRequired {
            message: CLOSE_MESSAGE.to_owned(),
            running_game: None,
        }));
    }

    let (steps, index) = job_steps(&plan);
    let game = options
        .game
        .and_then(|game| Some((parse_product_id(game.launcher_game_id)?, game)));
    let mut switch = Switch {
        riot,
        platform,
        backups,
        directory: riot.directory(),
        account,
        game,
        cancel: options.cancel,
        sink,
        log_at: riot.log_mark(),
        expected_uid: plan
            .account
            .launcher_uid
            .as_ref()
            .map(|uid| uid.to_lowercase()),
        written_fingerprint: context.fingerprint,
        install: context.install,
        session_path: riot.session_file_path(),
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
        ready_at: None,
        started_with_game: None,
    };

    let attempt = switch.attempt(context.running);
    let login = account.login.clone();
    let manual_minutes = minutes(riot.timing().manual_sign_in);
    let cancelled_after_start = |handed_over: bool| {
        if handed_over {
            format!(
                "Stopped waiting for the sign-in as {login}. The Riot Client was left on its login page; signing in there with “Stay signed in” ticked finishes the switch, and the sign-in is saved at the next one."
            )
        } else {
            format!(
                "Stopped while the Riot Client was starting as {login}. Nothing was rolled back: the client carries on signing in on its own."
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
                "One Launch waited {manual_minutes} min for the sign-in as {login} and gave up. The Riot Client is still on its login page: sign in there with “Stay signed in” ticked, and the sign-in is saved at the next switch."
            )),
        ),
        Err(thrown) => {
            let cancelled = matches!(thrown, Thrown::Aborted);
            let message = match thrown {
                Thrown::Aborted => {
                    format!("Stopped before the Riot Client was started as {login}.")
                }
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
        started_with_game: switch.started_with_game.filter(|_| ok),
    })
}
