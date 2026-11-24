// Epic's switch: close the launcher, save the session it leaves behind, copy GameUserSettings.ini, put the target account's saved session into it (or take the session out, so that the launcher asks), start the launcher and read its log for the sign-in. It reads, waits and decides; every write goes through the functions at the top of switch.rs. A saved session is decrypted in one place, `write_session`, and goes nowhere but into the launcher's own file.

use std::collections::HashMap;
use std::time::Duration;

use ol_adapters::epic::{
    EXE, EpicAdapter, LogMark, STEP, UNOWNED_SESSION_PIN, close_message, id_tail, settings_text,
};
use ol_adapters::steam::{minutes, plural, seconds};
use ol_adapters::{AccountRef, AdapterError, Directory};
use ol_formats::epic::launcher_log::{SignInOutcome, is_session_rejection, last_sign_in};
use ol_formats::epic::session::{
    EpicSessionBlob, fingerprint_of, parse_blob, plan_session_write, read_session_from_ini,
    serialize_blob, session_blob_from_ini, short_fingerprint,
};
use ol_formats::winpath::join;
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
    SwitchOptions, SwitchResult, SwitchSink, Thrown, job_steps, restore_epic,
    write_game_user_settings,
};
use crate::backup::{BackupError, BackupFile, BackupService, SnapshotRequest};
use crate::launchers::ServiceError;

// Every settings file there is, as a snapshot takes it: encrypted, because it holds the session.
fn snapshot(
    epic: &EpicAdapter<'_>,
    backups: &BackupService<'_>,
    reason: &str,
    protect: Option<&str>,
    pinned: bool,
) -> Result<BackupSnapshot, BackupError> {
    let paths = epic.settings_paths();
    let relatives: Vec<String> = paths
        .iter()
        .map(|path| EpicAdapter::settings_relative(path))
        .collect();
    let files: Vec<BackupFile<'_>> = paths
        .iter()
        .zip(&relatives)
        .map(|(source, relative)| BackupFile {
            source,
            relative,
            sensitive: true,
        })
        .collect();
    backups.snapshot(&SnapshotRequest {
        launcher: LauncherId::Epic,
        reason,
        files: &files,
        meta: Map::new(),
        protect,
        pinned: pinned.then_some(UNOWNED_SESSION_PIN),
    })
}

// Puts a snapshot back from Settings or after a crash. Refused while the launcher runs, because it rewrites GameUserSettings.ini while it runs; what is there now is snapshotted first, so restoring the wrong one can be undone.
pub fn restore_backup(
    epic: &EpicAdapter<'_>,
    platform: &Platform,
    backups: &BackupService<'_>,
    id: &str,
) -> Result<RestoreResult, ServiceError> {
    let adapter = ServiceError::adapter(LauncherId::Epic);
    epic.require_install().map_err(&adapter)?;
    if epic.is_running().map_err(&adapter)? {
        return Err(adapter(AdapterError::LauncherRunning(
            "Close Epic Games Launcher before restoring: it rewrites GameUserSettings.ini while it runs, which would undo the restore."
                .to_owned(),
        )));
    }
    let unowned = epic.holds_unowned_session().map_err(&adapter)?;
    snapshot(
        epic,
        backups,
        &format!("Before restoring {id}"),
        Some(id),
        unowned,
    )
    .map_err(ServiceError::backup(LauncherId::Epic))?;
    restore_epic(epic, platform, backups, id).map_err(|error| error.into_service(LauncherId::Epic))
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

// What the log says after a confirmed sign-in, when it no longer names that sign-in's account.
enum Elsewhere {
    WrongAccount {
        account_id: String,
        owner: Option<AccountRef>,
        detail: String,
    },
    Unclear(String),
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
            format!("Sign in to Epic Games Launcher as {login}")
        } else {
            format!("Epic Games Launcher is asking you to sign in as {login}")
        },
        message: match cause {
            Cause::Expired => "Its remembered session has expired.".to_owned(),
            Cause::New => "No session is stored for this account yet.".to_owned(),
            Cause::Other => format!(
                "The launcher did not sign in on its own{}.",
                said.map(|reason| format!(" ({reason})"))
                    .unwrap_or_default()
            ),
        },
        steps: vec![format!(
            "Sign in as {login} in the Epic Games Launcher window. One Launch saves the sign-in and continues on its own, and later switches are automatic."
        )],
        reason,
        can_copy_password,
        until,
    }
}

// Records the Epic id on the account, then encrypts and stores the session for it. The id comes first and is refused when another account holds it, so no session is ever stored under an id the account was not given.
pub(crate) fn store_session(
    directory: &dyn Directory,
    account: &AccountRef,
    blob: &EpicSessionBlob,
    uid: Option<&str>,
) -> Result<(), AdapterError> {
    let Some(account_id) = account.id.as_deref() else {
        return Ok(());
    };
    let uid = uid.filter(|uid| !uid.is_empty());
    let linked = account
        .launcher_uid
        .as_deref()
        .filter(|linked| !linked.is_empty());
    if let Some(uid) = uid
        && linked.is_none_or(|linked| linked.to_lowercase() != uid.to_lowercase())
    {
        directory.set_launcher_uid(account_id, uid)?;
    }
    let sealed = Secret::new(serialize_blob(blob));
    directory.save_session(account_id, uid, &fingerprint_of(&blob.data), &sealed)?;
    Ok(())
}

struct Switch<'a> {
    epic: &'a EpicAdapter<'a>,
    platform: &'a Platform,
    backups: &'a BackupService<'a>,
    directory: &'a dyn Directory,
    account: &'a AccountRef,
    cancel: &'a CancelToken,
    sink: &'a mut dyn SwitchSink,
    install: LauncherInstall,
    ini_path: String,
    strategy: SwitchStrategy,
    in_place: bool,
    already_active: bool,
    steps: Vec<JobStep>,
    index: HashMap<&'static str, usize>,
    current: Option<&'static str>,
    backup_id: Option<String>,
    modified: bool,
    // Once the launcher has been started again a stop no longer puts anything back: the launcher is reading what was written.
    launcher_started: bool,
    handed_over: bool,
    expected_uid: Option<String>,
    // The fingerprint of what the switch left in [RememberMe], so that a session the launcher writes afterwards is told from the switch's own.
    written_fingerprint: Option<String>,
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

    fn stop_if_aborted(&self) -> Result<(), Thrown> {
        if self.cancel.is_cancelled() {
            Err(Thrown::Aborted)
        } else {
            Ok(())
        }
    }

    fn is_running(&self) -> Result<bool, Thrown> {
        Ok(self.epic.is_running()?)
    }

    fn missing_settings_file(&self) -> Thrown {
        Thrown::Failed(format!(
            "{} is missing. Restore the newest Epic Games backup from Settings, or start the launcher once by hand, then try again.",
            self.ini_path
        ))
    }

    fn store_session(
        &self,
        account: &AccountRef,
        blob: &EpicSessionBlob,
        uid: Option<&str>,
    ) -> Result<(), AdapterError> {
        store_session(self.directory, account, blob, uid)
    }

    // Runs once the launcher is closed: the file is then the last run's, and the launcher's log names the account. When the plan passed the step over, the launcher is looked at all the same if this switch closed it, because a sign-in can land while it closes; the step then shows only a session that was saved. A session that cannot be saved costs one sign-in by hand later, never the switch.
    fn save_outgoing(&mut self, closed: bool) {
        let planned = self
            .index
            .get(STEP.capture)
            .is_some_and(|at| self.steps[*at].status != JobStepStatus::Skipped);
        if !planned && !closed {
            return;
        }
        if planned {
            self.mark(STEP.capture, JobStepStatus::Running, "");
        }
        let saved = (|| -> Result<(bool, String), AdapterError> {
            let signed_out = "The launcher left no remembered session behind (signed out).";
            let Some(text) = self.epic.read_ini()? else {
                return Ok((false, signed_out.to_owned()));
            };
            let fresh = self.epic.current_session(Some(&text), false)?;
            let Some(blob) = session_blob_from_ini(&text) else {
                return Ok((false, signed_out.to_owned()));
            };
            let Some((owner, owner_id)) = fresh
                .owner
                .as_ref()
                .and_then(|owner| Some((owner, owner.id.as_deref()?)))
            else {
                let detail = "The remembered session belongs to no One Launch account; left alone.";
                return Ok((false, detail.to_owned()));
            };
            let name = &owner.display_name;
            if !fresh.disk_current {
                return Ok((
                    false,
                    format!(
                        "The file predates the launcher's last sign-in, so it is not trusted as {name}'s session."
                    ),
                ));
            }
            let known = self.directory.session(owner_id)?;
            if known
                .is_some_and(|known| Some(&known.fingerprint) == fresh.disk.fingerprint.as_ref())
            {
                return Ok((
                    false,
                    format!("{name}'s stored session is already current."),
                ));
            }
            if !planned {
                self.mark(STEP.capture, JobStepStatus::Running, "");
            }
            let uid = owner
                .launcher_uid
                .as_deref()
                .or(fresh.logged_uid.as_deref());
            self.store_session(owner, &blob, uid)?;
            Ok((
                true,
                format!(
                    "Saved {name}'s session (fingerprint {}).",
                    short_fingerprint(fresh.disk.fingerprint.as_deref())
                ),
            ))
        })();
        let (shown, detail) = match saved {
            Ok((stored, detail)) => (planned || stored, detail),
            Err(error) => (
                planned || self.current == Some(STEP.capture),
                format!("Not saved: {error}"),
            ),
        };
        if shown {
            self.mark(STEP.capture, JobStepStatus::Done, detail);
        }
    }

    fn take_snapshot(&mut self) -> Result<(), Thrown> {
        self.mark(STEP.backup, JobStepStatus::Running, "");
        // The launcher is closed by now, so the file is final: a remembered session nobody owns is about to be replaced, and this snapshot is its only copy.
        let unowned = !self.in_place && self.epic.holds_unowned_session()?;
        let taken = snapshot(
            self.epic,
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

    // The stored session is decrypted here and nowhere else, written once, and dropped.
    fn write_session(&mut self) -> Result<(), Thrown> {
        self.mark(STEP.file, JobStepStatus::Running, "");
        let paths = self.epic.settings_paths();
        // The launcher's settings file is never made up: one with only the two sections loses every setting of every account, and the launcher does not sign in from it.
        if paths.is_empty() {
            return Err(self.missing_settings_file());
        }
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
        let files = self.platform.files.as_ref();
        let mut written = 0;
        for (age, path) in paths.iter().enumerate() {
            let before = files
                .read(path)
                .map_err(|error| Thrown::Failed(error.to_string()))?;
            let text = match settings_text(path, &before) {
                Ok(text) => text,
                // Only the newest file was looked at when the plan read it. An older one that is not UTF-8 text is left as it is: a rewrite would break it, and the launcher does not read it.
                Err(_) if age > 0 => continue,
                Err(refused) => return Err(refused.into()),
            };
            let write = plan_session_write(text, blob.as_ref());
            if write.noop {
                continue;
            }
            self.modified = true;
            write_game_user_settings(files, path, &write.text)?;
            written += 1;
        }
        self.written_fingerprint = blob.as_ref().map(|blob| fingerprint_of(&blob.data));
        if written == 0 {
            let detail = if blob.is_some() {
                "Already in place."
            } else {
                "Already signed out."
            };
            self.mark(STEP.file, JobStepStatus::Skipped, detail);
            return Ok(());
        }
        let what = match &self.written_fingerprint {
            Some(fingerprint) => format!(
                "[RememberMe] now holds the snapshot (fingerprint {})",
                short_fingerprint(Some(fingerprint))
            ),
            None => "[RememberMe] cleared".to_owned(),
        };
        self.mark(
            STEP.file,
            JobStepStatus::Done,
            format!("{what} in {}.", plural(written, "file")),
        );
        Ok(())
    }

    // Something other than this switch started the launcher. Before the writes nothing is lost by stopping; after them the snapshot goes back.
    fn started_meanwhile(&mut self, at: &'static str) -> Thrown {
        self.mark(at, JobStepStatus::Running, "");
        Thrown::Failed(format!(
            "Epic Games Launcher was started by something else while One Launch was switching accounts, so the switch {}. Try again.",
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
        self.log_at = self.epic.log_mark();
        self.launcher_started = true;
        let folder = join(&[&self.install.exe_path, ".."]);
        self.platform
            .processes
            .start(&self.install.exe_path, &[], &folder)?;
        self.mark(STEP.start, JobStepStatus::Done, "Launcher started.");
        self.mark(STEP.wait, JobStepStatus::Running, "");
        let timing = *self.epic.timing();
        let epic = self.epic;
        let appeared = wait_until(
            self.platform.clock.as_ref(),
            self.cancel,
            timing.startup,
            timing.poll,
            || epic.is_running(),
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

    // Reads what the launcher has logged since it was started. While the user signs in by hand a failed try, a wrong password say, settles nothing.
    fn wait_for_sign_in(&mut self, timeout: Duration, manual: bool) -> Result<SignInWait, Thrown> {
        let timing = *self.epic.timing();
        let deadline = self.now() + timeout;
        loop {
            if self.cancel.is_cancelled() {
                return Ok(SignInWait::Cancelled);
            }
            match last_sign_in(&self.epic.events_since(&self.log_at)) {
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
        let tail = id_tail(Some(account_id), 6);
        let who = match owner {
            Some(owner) => format!("{} (account {tail})", owner.display_name),
            None => format!("account {tail}"),
        };
        let (name, login) = (&self.account.display_name, &self.account.login);
        Thrown::Failed(match &self.expected_uid {
            None => format!(
                "Epic Games Launcher signed in as {who}, which is another One Launch account, instead of {name}. Sign in to the launcher as {login}."
            ),
            Some(expected) => format!(
                "Epic Games Launcher signed in as {who} instead of {name} ({}). Sign in to the launcher as {login}, or fix the launcher account ID on the account page.",
                id_tail(Some(expected), 6)
            ),
        })
    }

    // The first sign-in that is confirmed is where the account's Epic id comes from. Refused when another account holds it.
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
            " Linked to Epic Games account {}.",
            id_tail(Some(signed_in_as), 4)
        ))
    }

    fn signed_in_detail(&mut self, account_id: &str) -> Result<String, Thrown> {
        let linked = self.link_if_new(account_id)?;
        Ok(format!("Signed in as {}.{linked}", self.account.login))
    }

    fn confirm_sign_in(&mut self) -> Result<SignInWait, Thrown> {
        self.mark(STEP.verify, JobStepStatus::Running, "");
        let timing = *self.epic.timing();
        let remembered = self.strategy == SwitchStrategy::Remembered;
        let timeout = if remembered {
            timing.sign_in
        } else {
            Duration::from_millis(1)
        };
        let waited = self.wait_for_sign_in(timeout, false)?;
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
                    "The launcher rejected the session: {}.",
                    if reason.is_empty() {
                        "no reason given"
                    } else {
                        reason
                    }
                ),
            ),
            SignInWait::Timeout if remembered => (
                JobStepStatus::Failed,
                "The launcher did not report a sign-in in time.".to_owned(),
            ),
            SignInWait::Timeout => (JobStepStatus::Failed, "Waiting for you.".to_owned()),
            SignInWait::Cancelled => (JobStepStatus::Failed, "Cancelled.".to_owned()),
        };
        self.mark(STEP.verify, status, detail);
        Ok(waited)
    }

    // The launcher sits on its sign-in page: the user takes over, and the wait is for the sign-in to land in the log.
    fn wait_for_manual_sign_in(&mut self, why: &SignInWait) -> Result<SignInWait, Thrown> {
        let timing = *self.epic.timing();
        let until = ol_store::iso_timestamp(self.now() + timing.manual_sign_in);
        let reason = match why {
            SignInWait::Failed { reason } => Some(reason.clone()),
            _ => None,
        };
        let expired = reason
            .as_deref()
            .is_some_and(|reason| reason.to_ascii_lowercase().contains("expired"));
        let cause = match self.strategy {
            SwitchStrategy::Credentials => Cause::New,
            SwitchStrategy::Remembered if expired => Cause::Expired,
            SwitchStrategy::Remembered => Cause::Other,
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

    // What the log says now, when it no longer names the account whose sign-in was confirmed: the session in the file may then be another account's. A sign-in as an account that is not this one, by the rule the sign-in was confirmed by, is a wrong account; one that cannot be told apart from this account's only stops the save.
    fn signed_in_elsewhere(&self, signed_in_as: &str) -> Result<Option<Elsewhere>, Thrown> {
        match last_sign_in(&self.epic.events_since(&self.log_at)) {
            Some(SignInOutcome::SignedIn { account_id, .. }) if account_id == signed_in_as => {
                Ok(None)
            }
            Some(SignInOutcome::SignedIn { account_id, .. }) => {
                let detail = format!(
                    "Not saved: the launcher signed in as account {} meanwhile.",
                    id_tail(Some(&account_id), 6)
                );
                let owner = if account_id.is_empty() {
                    None
                } else {
                    self.directory.account_by_launcher_uid(&account_id)?
                };
                let another = owner
                    .as_ref()
                    .is_some_and(|owner| owner.id != self.account.id);
                Ok(Some(
                    if !account_id.is_empty() && (self.expected_uid.is_some() || another) {
                        Elsewhere::WrongAccount {
                            account_id,
                            owner,
                            detail,
                        }
                    } else {
                        Elsewhere::Unclear(detail)
                    },
                ))
            }
            _ => Ok(Some(Elsewhere::Unclear(format!(
                "Not saved: the launcher no longer reports the sign-in as {}.",
                self.account.login
            )))),
        }
    }

    // After a confirmed sign-in: stores the session the launcher has written, when it differs from what was there, for as long as the log names the account that signed in. A sign-in by hand writes it moments later; one from a remembered session may not write it at all. A sign-in as another account meanwhile ends the switch as a wrong account.
    fn capture_fresh(&mut self, signed_in_as: &str, wait: Duration) -> Result<(), Thrown> {
        let Some(account_id) = self.account.id.clone() else {
            return Ok(());
        };
        self.mark(STEP.recapture, JobStepStatus::Running, "");
        let timing = *self.epic.timing();
        let deadline = self.now() + wait;
        loop {
            let text = self.epic.read_ini()?;
            // The log is read after the file, so a sign-in that is logged before its session is written is seen before what it wrote is stored.
            match self.signed_in_elsewhere(signed_in_as)? {
                None => {}
                Some(Elsewhere::Unclear(detail)) => {
                    self.mark(STEP.recapture, JobStepStatus::Done, detail);
                    return Ok(());
                }
                Some(Elsewhere::WrongAccount {
                    account_id,
                    owner,
                    detail,
                }) => {
                    self.mark(STEP.recapture, JobStepStatus::Failed, detail);
                    return Err(self.wrong_account(&account_id, owner.as_ref()));
                }
            }
            let disk = text.as_deref().map(read_session_from_ini);
            let blob = text.as_deref().and_then(session_blob_from_ini);
            let remembered = disk.as_ref().filter(|disk| disk.remembered);
            if let (Some(disk), Some(blob)) = (remembered, &blob)
                && disk.fingerprint != self.written_fingerprint
            {
                self.store_session(self.account, blob, self.expected_uid.as_deref())?;
                self.directory.mark_session_verified(&account_id)?;
                let detail = format!(
                    "Stored {}'s session (fingerprint {}).",
                    self.account.display_name,
                    short_fingerprint(disk.fingerprint.as_deref())
                );
                self.mark(STEP.recapture, JobStepStatus::Done, detail);
                return Ok(());
            }
            if self.now() >= deadline || self.cancel.is_cancelled() {
                let stored = self.directory.session(&account_id)?;
                let current = remembered.is_some_and(|disk| {
                    stored.is_some_and(|stored| {
                        Some(&stored.fingerprint) == disk.fingerprint.as_ref()
                    })
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
                        "The launcher has not written the new session yet; it is saved at the next switch.",
                    );
                }
                return Ok(());
            }
            self.platform.clock.sleep(timing.log_poll, self.cancel);
        }
    }

    fn attempt(&mut self, running: bool) -> Result<Ending, Thrown> {
        let timing = *self.epic.timing();
        let remembered = self.strategy == SwitchStrategy::Remembered;
        self.stop_if_aborted()?;
        if running && !self.already_active {
            self.mark(STEP.close, JobStepStatus::Running, "");
            self.epic.shutdown(None, self.cancel)?;
            self.mark(STEP.close, JobStepStatus::Done, "Launcher closed.");
        }
        self.stop_if_aborted()?;
        // A running launcher rewrites the file when it exits and passes over what is written under it, so nothing is read or written against one.
        if !self.already_active && self.is_running()? {
            return Err(self.started_meanwhile(STEP.close));
        }

        self.save_outgoing(running && !self.already_active);
        if !self.already_active && !self.in_place {
            if self.epic.settings_paths().is_empty() {
                return Err(self.missing_settings_file());
            }
            self.take_snapshot()?;
            self.write_session()?;
        }

        if self.is_running()? {
            if !self.already_active {
                return Err(self.started_meanwhile(STEP.start));
            }
            let detail = "The launcher is already running.";
            self.mark(STEP.start, JobStepStatus::Skipped, detail);
            self.mark(STEP.wait, JobStepStatus::Skipped, detail);
        } else {
            self.start_launcher()?;
        }

        if self.already_active && running && !self.launcher_started {
            self.mark(
                STEP.verify,
                JobStepStatus::Skipped,
                "The launcher is already signed in as this account.",
            );
            return Ok(Ending::Done);
        }
        match self.confirm_sign_in()? {
            SignInWait::Cancelled => return Ok(Ending::Cancelled),
            SignInWait::WrongAccount { account_id, owner } => {
                return Err(self.wrong_account(&account_id, owner.as_ref()));
            }
            SignInWait::SignedIn {
                account_id: signed_in_as,
            } => {
                if let (true, Some(account_id)) = (remembered, self.account.id.clone()) {
                    if self.in_place {
                        self.capture_fresh(&signed_in_as, Duration::ZERO)?;
                    } else {
                        self.directory.mark_session_verified(&account_id)?;
                    }
                } else if !remembered {
                    // Without a saved session the launcher was signed in by hand, and writes the session after the log says so.
                    self.capture_fresh(&signed_in_as, timing.capture)?;
                }
            }
            unsettled => {
                // Only a refused session is stale. A failure of the service or the network says nothing about the saved session, and marking it would cost a sign-in by hand.
                if let (SignInWait::Failed { reason }, true, Some(account_id)) =
                    (&unsettled, remembered, self.account.id.as_deref())
                    && is_session_rejection(reason)
                {
                    self.directory.mark_session_stale(account_id, reason)?;
                }
                match self.wait_for_manual_sign_in(&unsettled)? {
                    SignInWait::Cancelled => return Ok(Ending::Cancelled),
                    SignInWait::WrongAccount { account_id, owner } => {
                        return Err(self.wrong_account(&account_id, owner.as_ref()));
                    }
                    SignInWait::SignedIn { account_id } => {
                        self.capture_fresh(&account_id, timing.capture)?;
                    }
                    _ => return Ok(Ending::NeedsSignIn),
                }
            }
        }
        self.ready_at = Some(self.now() + timing.ready_grace);
        Ok(Ending::Done)
    }

    // The launcher rewrites its settings file when its state changes and when it exits, so it is closed before the file goes back. Nothing stops this once it has begun.
    // A launcher that will not close keeps the snapshot for the next start: it would write its own sign-in over the one put back.
    fn roll_back(&self, backup_id: &str) -> Result<(), String> {
        let text = |error: AdapterError| error.to_string();
        if self.epic.is_running().map_err(text)? {
            let closed = self.epic.shutdown(None, &CancelToken::new());
            if self.epic.is_running().map_err(text)? {
                return Err(closed.err().map_or_else(
                    || "Epic Games Launcher is still running, and it rewrites GameUserSettings.ini while it runs".to_owned(),
                    text,
                ));
            }
        }
        restore_epic(self.epic, self.platform, self.backups, backup_id)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

// Makes `account` the one the Epic Games Launcher signs in. A refusal before anything is touched is an error; from the first step on, whatever happens comes back as a result that says what was put back.
pub fn run_switch(
    epic: &EpicAdapter<'_>,
    platform: &Platform,
    backups: &BackupService<'_>,
    account: &AccountRef,
    options: &SwitchOptions<'_>,
    sink: &mut dyn SwitchSink,
) -> Result<SwitchResult, ServiceError> {
    let adapter = ServiceError::adapter(LauncherId::Epic);
    let started_at = ol_store::iso_timestamp(platform.clock.now());
    let (plan, context) = epic.build_switch_plan(account).map_err(&adapter)?;
    if !plan.executable {
        return Err(adapter(AdapterError::SwitchBlocked(
            plan.warnings
                .first()
                .cloned()
                .unwrap_or_else(|| "This switch cannot run.".to_owned()),
        )));
    }
    if context.running && !context.already_active && !options.confirm_close {
        let running_game = epic.running_game().map_err(&adapter)?;
        return Err(adapter(AdapterError::ConfirmationRequired {
            message: close_message(running_game.as_ref()),
            running_game,
        }));
    }

    let (steps, index) = job_steps(&plan);
    let mut switch = Switch {
        epic,
        platform,
        backups,
        directory: epic.directory(),
        account,
        cancel: options.cancel,
        sink,
        log_at: epic.log_mark(),
        expected_uid: plan
            .account
            .launcher_uid
            .as_ref()
            .map(|uid| uid.to_lowercase()),
        written_fingerprint: context.current.disk.fingerprint.clone(),
        install: context.install,
        ini_path: context.ini_path,
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
    };

    let attempt = switch.attempt(context.running);
    let login = account.login.clone();
    let manual_minutes = minutes(epic.timing().manual_sign_in);
    let cancelled_after_start = |handed_over: bool| {
        if handed_over {
            format!(
                "Stopped waiting for the sign-in as {login}. Epic Games Launcher was left on its sign-in page; signing in there finishes the switch, and the session is saved at the next one."
            )
        } else {
            format!(
                "Stopped while Epic Games Launcher was starting as {login}. Nothing was rolled back: the launcher carries on signing in on its own."
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
                "One Launch waited {manual_minutes} min for the sign-in as {login} and gave up. Epic Games Launcher is still on its sign-in page: sign in there, and the session is saved at the next switch."
            )),
        ),
        Err(thrown) => {
            let cancelled = matches!(thrown, Thrown::Aborted);
            let message = match thrown {
                Thrown::Aborted => {
                    format!("Stopped before Epic Games Launcher was started as {login}.")
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
        started_with_game: None,
    })
}
