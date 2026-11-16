// Steam's switch: close Steam, copy what will change, point loginusers.vdf and the registry at the account, start Steam and wait for its sign-in. It reads, waits and decides; every write goes through the functions at the top of switch.rs.

use std::collections::HashMap;
use std::time::Duration;

use ol_adapters::AdapterError;
use ol_adapters::steam::{
    AccountRef, HKCU_KEY, STEP, SteamAdapter, close_message, minutes, plural, seconds,
};
use ol_formats::steam::login_log::{SignInCause, SignInOutcome};
use ol_formats::steam::loginusers::{
    SteamLoginUser, UNWRITABLE_LAYOUT, marks_most_recent, parse_login_users,
    plan_auto_login_rewrite, steam_id3,
};
use ol_model::{
    JobStep, JobStepStatus, LaunchAttention, LauncherId, LauncherInstall, RestoreResult,
    SwitchStrategy,
};
use ol_platform_api::{CancelToken, Hive, Platform, ProcessArg};
use ol_runtime::wait_until;
use time::OffsetDateTime;

use super::{
    AUTO_LOGIN_USER, REMEMBER_PASSWORD, SNAPSHOT_RELATIVE, SwitchOptions, SwitchResult, SwitchSink,
    Thrown, job_steps, restore_steam, set_auto_login_user, set_remember_password, snapshot_meta,
    write_login_users,
};
use crate::backup::{BackupFile, BackupService, SnapshotRequest};
use crate::launchers::ServiceError;

// Puts a snapshot back from Settings or after a crash. Refused while Steam runs, because Steam rewrites loginusers.vdf when it exits; what is there now is snapshotted first, so restoring the wrong one can be undone.
pub fn restore_backup(
    steam: &SteamAdapter<'_>,
    platform: &Platform,
    backups: &BackupService<'_>,
    id: &str,
) -> Result<RestoreResult, ServiceError> {
    let adapter = ServiceError::adapter(LauncherId::Steam);
    let install = steam.require_install().map_err(&adapter)?;
    if steam.is_running().map_err(&adapter)? {
        return Err(adapter(AdapterError::LauncherRunning(
            "Close Steam before restoring: Steam rewrites loginusers.vdf when it exits, which would undo the restore."
                .to_owned(),
        )));
    }
    let registry = &platform.registry;
    let auto_login_user = registry
        .get_string(Hive::Hkcu, HKCU_KEY, AUTO_LOGIN_USER)
        .map_err(|error| adapter(error.into()))?;
    let remember = registry
        .get_dword(Hive::Hkcu, HKCU_KEY, REMEMBER_PASSWORD)
        .map_err(|error| adapter(error.into()))?;
    backups
        .snapshot(&SnapshotRequest {
            launcher: LauncherId::Steam,
            reason: &format!("Before restoring {id}"),
            files: &[BackupFile {
                source: &SteamAdapter::login_users_path(&install),
                relative: SNAPSHOT_RELATIVE,
                sensitive: false,
            }],
            meta: snapshot_meta(auto_login_user.as_deref(), remember),
            protect: Some(id),
            pinned: None,
        })
        .map_err(ServiceError::backup(LauncherId::Steam))?;
    restore_steam(steam, platform, backups, id)
        .map_err(|error| error.into_service(LauncherId::Steam))
}

enum SignInWait {
    SignedIn,
    Credentials { reason: String, cause: SignInCause },
    // `updating`: Steam was still updating itself when the wait gave up, so it must not be closed.
    Timeout { updating: bool },
    Cancelled,
}

// How the switch ended, short of a step failing.
enum Ending {
    Done,
    Cancelled,
    NeedsSignIn,
}

// A second -login cannot fix these: the user has to act in the Steam window.
const fn needs_user(cause: SignInCause) -> bool {
    matches!(
        cause,
        SignInCause::GuardEmail | SignInCause::GuardMobile | SignInCause::RateLimit
    )
}

fn attention_for(
    why: &SignInWait,
    login: &str,
    can_copy_password: bool,
    until: String,
) -> LaunchAttention {
    let (reason, cause) = match why {
        SignInWait::Credentials { reason, cause } => (
            Some(reason.clone()).filter(|reason| !reason.is_empty()),
            *cause,
        ),
        _ => (None, SignInCause::Other),
    };
    let remember = "Keep “Remember me” ticked.";
    let finish = "One Launch continues on its own as soon as Steam reports the sign-in, and later switches are automatic.";
    let attention = |title: String, message: String, steps: Vec<String>| LaunchAttention {
        title,
        message,
        steps,
        reason: reason.clone(),
        can_copy_password,
        until: until.clone(),
    };
    if matches!(why, SignInWait::Timeout { updating: true }) {
        return attention(
            "Steam is still updating itself".to_owned(),
            "Steam has been updating itself for longer than One Launch waits on its own, so it was left to finish.".to_owned(),
            vec![format!(
                "If Steam shows its sign-in form afterwards, sign in as {login} with “Remember me” ticked. {finish}"
            )],
        );
    }
    match cause {
        SignInCause::GuardEmail => attention(
            "Steam needs a one-time sign-in on this PC".to_owned(),
            format!(
                "Steam accepted the sign-in for {login}, but this PC is not yet authorized for the account, so it wants the code it just emailed you."
            ),
            vec![
                format!("In the Steam window, enter the account name {login} and the password."),
                remember.to_owned(),
                format!("Enter the code Steam emailed you. {finish} Later switches need none of this."),
            ],
        ),
        SignInCause::GuardMobile => attention(
            "Steam Guard needs your approval".to_owned(),
            format!("Steam wants the sign-in for {login} approved."),
            vec![
                "Approve the sign-in in the Steam Mobile app, or enter its code in the Steam window."
                    .to_owned(),
                format!("{remember} {finish}"),
            ],
        ),
        SignInCause::RateLimit => attention(
            "Steam is rate-limiting sign-ins".to_owned(),
            format!("Steam refused the sign-in for {login} because of too many recent attempts."),
            vec![
                "Wait a few minutes.".to_owned(),
                format!("Sign in as {login} in the Steam window with “Remember me” ticked. {finish}"),
            ],
        ),
        _ => attention(
            format!("Steam is asking you to sign in as {login}"),
            match (&cause, &reason) {
                (SignInCause::Expired, _) => "Its remembered session has expired.".to_owned(),
                (_, Some(reason)) => {
                    format!("Steam did not complete the sign-in on its own ({reason}).")
                }
                (_, None) => "Steam is waiting on its sign-in form.".to_owned(),
            },
            vec![
                format!("In the Steam window, enter the account name {login} and your password."),
                remember.to_owned(),
                format!("Sign in. If Steam Guard asks for a code, enter it. {finish}"),
            ],
        ),
    }
}

struct Switch<'a> {
    steam: &'a SteamAdapter<'a>,
    platform: &'a Platform,
    backups: &'a BackupService<'a>,
    account: &'a AccountRef,
    cancel: &'a CancelToken,
    sink: &'a mut dyn SwitchSink,
    install: LauncherInstall,
    login_users_path: String,
    target: Option<SteamLoginUser>,
    login: String,
    with_password: bool,
    expected_id3: Option<i128>,
    steps: Vec<JobStep>,
    index: HashMap<&'static str, usize>,
    current: Option<&'static str>,
    backup_id: Option<String>,
    modified: bool,
    // Once Steam has been started again a stop no longer puts anything back: Steam is reading what was written.
    launcher_started: bool,
    handed_over: bool,
    log_mark: u64,
    bootstrap_mark: u64,
    plan_auto_login_user: Option<String>,
    plan_remember: Option<u32>,
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
        Ok(self.steam.is_running()?)
    }

    fn active_user(&self) -> Result<Option<u32>, Thrown> {
        Ok(self.steam.active_user()?)
    }

    fn take_snapshot(&mut self) -> Result<(), Thrown> {
        self.mark(STEP.backup, JobStepStatus::Running, "");
        let snapshot = self
            .backups
            .snapshot(&SnapshotRequest {
                launcher: LauncherId::Steam,
                reason: &format!("Before switching to {}", self.account.display_name),
                files: &[BackupFile {
                    source: &self.login_users_path,
                    relative: SNAPSHOT_RELATIVE,
                    sensitive: false,
                }],
                meta: snapshot_meta(self.plan_auto_login_user.as_deref(), self.plan_remember),
                protect: None,
                pinned: None,
            })
            .map_err(|error| Thrown::Failed(error.to_string()))?;
        self.backup_id = Some(snapshot.id);
        self.mark(STEP.backup, JobStepStatus::Done, snapshot.path);
        Ok(())
    }

    // The journal hears of the snapshot only at the first write, so a switch that writes nothing leaves nothing to recover.
    fn before_write(&mut self) {
        if !self.modified
            && let Some(id) = &self.backup_id
        {
            self.sink.backup(id);
        }
        self.modified = true;
    }

    fn wait_for_process(&mut self) -> Result<(), Thrown> {
        self.mark(STEP.wait, JobStepStatus::Running, "");
        let timing = *self.steam.timing();
        let steam = self.steam;
        let appeared = wait_until(
            self.platform.clock.as_ref(),
            self.cancel,
            timing.startup,
            timing.poll,
            || steam.is_running(),
        )?;
        if !appeared {
            return Err(Thrown::Failed(format!(
                "steam.exe did not appear within {} s",
                seconds(timing.startup)
            )));
        }
        self.mark(STEP.wait, JobStepStatus::Done, "steam.exe is running.");
        Ok(())
    }

    fn start_steam(&mut self, args: &[ProcessArg<'_>]) -> Result<(), Thrown> {
        self.stop_if_aborted()?;
        self.log_mark = self.steam.login_log_mark(&self.install);
        self.bootstrap_mark = self.steam.bootstrap_log_mark(&self.install);
        self.launcher_started = true;
        self.platform
            .processes
            .start(&self.install.exe_path, args, &self.install.install_dir)?;
        Ok(())
    }

    // Something other than this switch started Steam. Before the writes nothing is lost by stopping; after them the snapshot goes back.
    fn started_meanwhile(&mut self, at: &'static str) -> Thrown {
        self.mark(at, JobStepStatus::Running, "");
        Thrown::Failed(format!(
            "Steam was started by something else while One Launch was switching accounts, so the switch {}. Try again.",
            if self.modified {
                "was stopped"
            } else {
                "stopped before changing anything"
            }
        ))
    }

    // Looks at the account id Steam reports and, unless the user is signing in by hand, at its sign-in log, so a Steam Guard prompt or a refused password is known at once and not after a timeout. ActiveUser outlives an exit, so a value equal to the one from before this start proves nothing by itself: then the log has to report the sign-in.
    fn wait_for_sign_in(
        &mut self,
        previous: u32,
        timeout: Duration,
        manual: bool,
    ) -> Result<SignInWait, Thrown> {
        let timing = *self.steam.timing();
        let ceiling = self.now() + timeout.max(timing.update);
        let mut deadline = self.now() + timeout;
        let mut updating = false;
        loop {
            if self.cancel.is_cancelled() {
                return Ok(SignInWait::Cancelled);
            }
            let active = self.active_user()?;
            let changed = active != Some(previous);
            let outcome = if manual && changed {
                None
            } else {
                self.steam.outcome_since(&self.install, self.log_mark)
            };
            let names_the_account = active.is_some_and(|id| {
                id != 0
                    && match self.expected_id3 {
                        Some(expected) => i128::from(id) == expected,
                        None => id != previous,
                    }
            });
            if names_the_account && (changed || outcome == Some(SignInOutcome::Success)) {
                return Ok(SignInWait::SignedIn);
            }
            if !manual {
                if let Some(SignInOutcome::Credentials { reason, cause }) = outcome {
                    return Ok(SignInWait::Credentials { reason, cause });
                }
                // A client update comes before any sign-in, so the wait starts over once it ends; a steam.exe that is gone stops stretching it.
                let was_updating = updating;
                updating = self
                    .steam
                    .updating_since(&self.install, self.bootstrap_mark);
                if updating && self.is_running()? {
                    deadline = ceiling.min(self.now() + timeout);
                }
                if updating && !was_updating {
                    self.mark(
                        STEP.verify,
                        JobStepStatus::Running,
                        "Steam is updating itself first; the sign-in follows when it has finished.",
                    );
                } else if !updating && was_updating {
                    self.mark(
                        STEP.verify,
                        JobStepStatus::Running,
                        "Steam finished updating itself.",
                    );
                }
            }
            if self.now() >= deadline {
                return Ok(SignInWait::Timeout { updating });
            }
            self.platform.clock.sleep(timing.log_poll, self.cancel);
        }
    }

    fn confirm_sign_in(&mut self, previous: u32, timeout: Duration) -> Result<SignInWait, Thrown> {
        self.mark(STEP.verify, JobStepStatus::Running, "");
        let waited = self.wait_for_sign_in(previous, timeout, false)?;
        let (status, detail) = match &waited {
            SignInWait::SignedIn => (
                JobStepStatus::Done,
                format!("Steam signed in as {}.", self.login),
            ),
            SignInWait::Credentials { reason, .. } => (
                JobStepStatus::Failed,
                format!(
                    "Steam is asking for credentials: {}.",
                    if reason.is_empty() {
                        "no reason given"
                    } else {
                        reason
                    }
                ),
            ),
            SignInWait::Timeout { updating: true } => (
                JobStepStatus::Failed,
                "Steam is still updating itself.".to_owned(),
            ),
            SignInWait::Timeout { updating: false } => (
                JobStepStatus::Failed,
                "Steam did not report a sign-in in time.".to_owned(),
            ),
            SignInWait::Cancelled => (JobStepStatus::Failed, "Cancelled.".to_owned()),
        };
        self.mark(STEP.verify, status, detail);
        Ok(waited)
    }

    // The password is fetched here and nowhere else, and handed to Steam's command line and nowhere else. Without it, -login with only the name makes Steam open its sign-in form.
    fn start_sign_in(&mut self) -> Result<u32, Thrown> {
        let password = if self.with_password {
            self.sink.password().map_err(Thrown::Failed)?
        } else {
            None
        };
        let password = password.filter(|password| !password.expose().is_empty());
        self.mark(STEP.login, JobStepStatus::Running, "");
        if self.is_running()? {
            self.steam.shutdown(None, self.cancel)?;
        }
        let previous = self.active_user()?.unwrap_or(0);
        let login = self.account.login.clone();
        match &password {
            Some(password) => self.start_steam(&[
                ProcessArg::Plain("-login"),
                ProcessArg::Plain(&login),
                ProcessArg::Secret(password.expose()),
            ])?,
            None => self.start_steam(&[ProcessArg::Plain("-login"), ProcessArg::Plain(&login)])?,
        }
        self.mark(
            STEP.login,
            JobStepStatus::Done,
            if password.is_some() {
                "Steam started with -login."
            } else {
                "Steam started on its sign-in form."
            },
        );
        self.wait_for_process()?;
        Ok(previous)
    }

    fn password_rejected(&self) -> Thrown {
        Thrown::Failed(format!(
            "Steam rejected the stored password for {}. Update it on the account page and try again.",
            self.login
        ))
    }

    // Steam sits on its sign-in form: the user takes over, and the wait is for the sign-in to land.
    fn wait_for_manual_sign_in(
        &mut self,
        previous: u32,
        why: &SignInWait,
    ) -> Result<SignInWait, Thrown> {
        let timing = *self.steam.timing();
        let until = ol_store::iso_timestamp(self.now() + timing.manual_sign_in);
        let attention = attention_for(why, &self.login, self.account.has_secret, until);
        self.mark(STEP.manual, JobStepStatus::Running, attention.title.clone());
        self.handed_over = true;
        self.sink.attention(Some(&attention));
        let waited = self.wait_for_sign_in(previous, timing.manual_sign_in, true);
        if let Ok(waited) = &waited {
            let (status, detail) = match waited {
                SignInWait::SignedIn => (
                    JobStepStatus::Done,
                    format!("Steam signed in as {}.", self.login),
                ),
                SignInWait::Cancelled => (JobStepStatus::Failed, "Stopped waiting.".to_owned()),
                _ => (
                    JobStepStatus::Failed,
                    format!("No sign-in after {} min.", minutes(timing.manual_sign_in)),
                ),
            };
            self.mark(STEP.manual, status, detail);
        }
        self.sink.attention(None);
        waited
    }

    // The sign-in is confirmed by now, so this wait is short and a stop simply ends it.
    fn until_library_ready(&mut self) -> Result<(), Thrown> {
        let timing = *self.steam.timing();
        let (steam, install, mark) = (self.steam, &self.install, self.log_mark);
        let waited: Result<bool, Thrown> = wait_until(
            self.platform.clock.as_ref(),
            self.cancel,
            timing.ready_grace,
            timing.log_poll,
            || Ok(steam.outcome_since(install, mark) == Some(SignInOutcome::Success)),
        );
        match waited {
            Ok(_) | Err(Thrown::Aborted) => Ok(()),
            Err(failed) => Err(failed),
        }
    }

    // After -login for an account Steam had never seen, ActiveUser only says that someone signed in. The entry Steam then writes for the account name says who.
    fn verify_identity(&self) -> Result<(), Thrown> {
        if self.expected_id3.is_some() {
            return Ok(());
        }
        let signed_in = self.active_user()?.map(i128::from);
        let Some(users) = self
            .platform
            .files
            .read(&self.login_users_path)
            .ok()
            .and_then(|bytes| parse_login_users(&String::from_utf8_lossy(&bytes)).ok())
        else {
            return Ok(());
        };
        let login = self.account.login.trim().to_lowercase();
        let entry = users
            .iter()
            .find(|user| user.account_name.to_lowercase() == login);
        let Some(signed_in) = signed_in else {
            return Ok(());
        };
        // Without an entry for the account yet, only an entry of another account for the id that signed in says who it was.
        let wrong = match entry {
            Some(entry) => steam_id3(&entry.steam_id64) != Some(signed_in),
            None => users
                .iter()
                .any(|user| steam_id3(&user.steam_id64) == Some(signed_in)),
        };
        if wrong {
            let other = users
                .iter()
                .find(|user| steam_id3(&user.steam_id64) == Some(signed_in));
            return Err(Thrown::Failed(format!(
                "Steam signed in as {} instead of {}.",
                other.map_or("a different account", |user| user.account_name.as_str()),
                self.login
            )));
        }
        Ok(())
    }

    fn remembered(
        &mut self,
        target: &SteamLoginUser,
        already_active: bool,
    ) -> Result<Ending, Thrown> {
        let timing = *self.steam.timing();
        if !already_active {
            self.take_snapshot()?;

            // Read afresh: Steam has just exited and may have rewritten the file.
            let bytes = self
                .platform
                .files
                .read(&self.login_users_path)
                .map_err(|error| Thrown::Failed(error.to_string()))?;
            let rewrite =
                plan_auto_login_rewrite(&String::from_utf8_lossy(&bytes), &target.steam_id64)
                    .map_err(|error| Thrown::Failed(error.to_string()))?;
            self.mark(STEP.file, JobStepStatus::Running, "");
            if !marks_most_recent(&rewrite.text, &target.steam_id64) {
                return Err(Thrown::Failed(UNWRITABLE_LAYOUT.to_owned()));
            }
            if rewrite.noop {
                self.mark(
                    STEP.file,
                    JobStepStatus::Skipped,
                    "Already marked as most recent.",
                );
            } else {
                self.before_write();
                write_login_users(
                    self.platform.files.as_ref(),
                    &self.login_users_path,
                    &rewrite.text,
                )?;
                self.mark(
                    STEP.file,
                    JobStepStatus::Done,
                    format!("{} changed.", plural(rewrite.changes.len(), "line")),
                );
            }

            let name = &target.account_name;
            self.mark(STEP.registry_user, JobStepStatus::Running, "");
            let current =
                self.platform
                    .registry
                    .get_string(Hive::Hkcu, HKCU_KEY, AUTO_LOGIN_USER)?;
            if current.is_some_and(|current| current.to_lowercase() == name.to_lowercase()) {
                self.mark(
                    STEP.registry_user,
                    JobStepStatus::Skipped,
                    format!("AutoLoginUser is already {name}."),
                );
            } else {
                self.before_write();
                set_auto_login_user(self.platform.registry.as_ref(), name)?;
                self.mark(
                    STEP.registry_user,
                    JobStepStatus::Done,
                    format!("AutoLoginUser = {name}"),
                );
            }

            self.mark(STEP.registry_remember, JobStepStatus::Running, "");
            let remember =
                self.platform
                    .registry
                    .get_dword(Hive::Hkcu, HKCU_KEY, REMEMBER_PASSWORD)?;
            if remember == Some(1) {
                self.mark(
                    STEP.registry_remember,
                    JobStepStatus::Skipped,
                    "RememberPassword is already 1.",
                );
            } else {
                self.before_write();
                set_remember_password(self.platform.registry.as_ref())?;
                self.mark(
                    STEP.registry_remember,
                    JobStepStatus::Done,
                    "RememberPassword = 1",
                );
            }
        }

        let mut previous = self.active_user()?.unwrap_or(0);
        let mut started = false;
        if self.is_running()? {
            // Only a switch that changes nothing may meet a running Steam here; any other was closed above.
            if !already_active {
                return Err(self.started_meanwhile(STEP.start));
            }
            self.mark(
                STEP.start,
                JobStepStatus::Skipped,
                "Steam is already running.",
            );
            self.mark(
                STEP.wait,
                JobStepStatus::Skipped,
                "Steam is already running.",
            );
        } else {
            self.mark(STEP.start, JobStepStatus::Running, "");
            self.start_steam(&[])?;
            started = true;
            self.mark(STEP.start, JobStepStatus::Done, "Steam started.");
            self.wait_for_process()?;
        }

        let signed_in_already = match self.expected_id3 {
            Some(expected) if !started => self.steam.signed_in_as(&self.install, expected)?,
            _ => false,
        };
        if signed_in_already {
            self.mark(
                STEP.verify,
                JobStepStatus::Skipped,
                "Steam is already signed in as this account.",
            );
            return Ok(Ending::Done);
        }
        let mut waited = self.confirm_sign_in(previous, timing.sign_in)?;
        if matches!(waited, SignInWait::Cancelled) {
            return Ok(Ending::Cancelled);
        }
        // A second try with the stored password mends an expired or dropped session, but not a Steam Guard prompt or a rate limit. It closes Steam, so it never runs while Steam updates itself, or against a Steam this switch did not start.
        let retry_with_password = self.with_password
            && started
            && !matches!(&waited, SignInWait::Credentials { cause, .. } if needs_user(*cause))
            && !matches!(waited, SignInWait::Timeout { updating: true });
        if !matches!(waited, SignInWait::SignedIn) && retry_with_password {
            previous = self.start_sign_in()?;
            waited = self.confirm_sign_in(previous, timing.guard)?;
            match waited {
                SignInWait::Cancelled => return Ok(Ending::Cancelled),
                SignInWait::Credentials {
                    cause: SignInCause::Password,
                    ..
                } => return Err(self.password_rejected()),
                _ => {}
            }
        }
        if matches!(waited, SignInWait::SignedIn) {
            return Ok(Ending::Done);
        }
        Ok(match self.wait_for_manual_sign_in(previous, &waited)? {
            SignInWait::SignedIn => Ending::Done,
            SignInWait::Cancelled => Ending::Cancelled,
            _ => Ending::NeedsSignIn,
        })
    }

    fn credentials(&mut self) -> Result<Ending, Thrown> {
        let timing = *self.steam.timing();
        self.take_snapshot()?;
        let previous = self.start_sign_in()?;
        let waited = if self.with_password {
            match self.confirm_sign_in(previous, timing.guard)? {
                SignInWait::Cancelled => return Ok(Ending::Cancelled),
                SignInWait::Credentials {
                    cause: SignInCause::Password,
                    ..
                } => return Err(self.password_rejected()),
                other => other,
            }
        } else {
            self.mark(STEP.verify, JobStepStatus::Skipped, "Handed over to you.");
            SignInWait::Credentials {
                reason: String::new(),
                cause: SignInCause::None,
            }
        };
        if !matches!(waited, SignInWait::SignedIn) {
            match self.wait_for_manual_sign_in(previous, &waited)? {
                SignInWait::SignedIn => {}
                SignInWait::Cancelled => return Ok(Ending::Cancelled),
                _ => return Ok(Ending::NeedsSignIn),
            }
        }
        self.verify_identity()?;
        Ok(Ending::Done)
    }

    fn attempt(
        &mut self,
        running: bool,
        already_active: bool,
        strategy: SwitchStrategy,
    ) -> Result<Ending, Thrown> {
        self.stop_if_aborted()?;
        if running && !already_active {
            self.mark(STEP.close, JobStepStatus::Running, "");
            self.steam.shutdown(None, self.cancel)?;
            self.mark(STEP.close, JobStepStatus::Done, "Steam closed.");
        }
        self.stop_if_aborted()?;
        if !already_active && self.is_running()? {
            return Err(self.started_meanwhile(STEP.close));
        }
        let ending = match (strategy, self.target.clone()) {
            (SwitchStrategy::Remembered, Some(target)) => {
                self.remembered(&target, already_active)?
            }
            _ => self.credentials()?,
        };
        if matches!(ending, Ending::Done) && self.launcher_started {
            self.until_library_ready()?;
        }
        Ok(ending)
    }

    // Steam rewrites loginusers.vdf when it exits, so it is closed before the file goes back, and a Steam that will not close keeps the snapshot for the next start. Nothing stops this once it has begun.
    fn roll_back(&self, backup_id: &str) -> Result<(), String> {
        let text = |error: AdapterError| error.to_string();
        if self.steam.is_running().map_err(text)? {
            let closed = self.steam.shutdown(None, &CancelToken::new());
            if self.steam.is_running().map_err(text)? {
                return Err(closed.err().map_or_else(
                    || {
                        "Steam is still running, and it rewrites loginusers.vdf when it exits"
                            .to_owned()
                    },
                    text,
                ));
            }
        }
        restore_steam(self.steam, self.platform, self.backups, backup_id)
            .map(|_| ())
            .map_err(|error| error.message())
    }
}

// Makes `account` the one Steam signs in. A refusal before anything is touched is an error; from the first step on, whatever happens comes back as a result that says what was put back.
pub fn run_switch(
    steam: &SteamAdapter<'_>,
    platform: &Platform,
    backups: &BackupService<'_>,
    account: &AccountRef,
    options: &SwitchOptions<'_>,
    sink: &mut dyn SwitchSink,
) -> Result<SwitchResult, ServiceError> {
    let adapter = ServiceError::adapter(LauncherId::Steam);
    let started_at = ol_store::iso_timestamp(platform.clock.now());
    let (plan, context) = steam.build_switch_plan(account).map_err(&adapter)?;
    if !plan.executable {
        return Err(adapter(AdapterError::SwitchBlocked(
            plan.warnings
                .first()
                .cloned()
                .unwrap_or_else(|| "This switch cannot run.".to_owned()),
        )));
    }
    if plan.running && !context.already_active && !options.confirm_close {
        let running_game = steam.running_game().map_err(&adapter)?;
        return Err(adapter(AdapterError::ConfirmationRequired {
            message: close_message(running_game.as_ref()),
            running_game,
        }));
    }

    let (steps, index) = job_steps(&plan);
    let login = context
        .target
        .as_ref()
        .map_or_else(|| account.login.clone(), |user| user.account_name.clone());
    let mut switch = Switch {
        steam,
        platform,
        backups,
        account,
        cancel: options.cancel,
        sink,
        log_mark: steam.login_log_mark(&context.install),
        bootstrap_mark: steam.bootstrap_log_mark(&context.install),
        expected_id3: context
            .target
            .as_ref()
            .and_then(|user| steam_id3(&user.steam_id64)),
        with_password: steam.auto_login() && account.has_secret,
        login,
        install: context.install,
        login_users_path: context.login_users_path,
        target: context.target,
        steps,
        index,
        current: None,
        backup_id: None,
        modified: false,
        launcher_started: false,
        handed_over: false,
        plan_auto_login_user: context.auto_login_user,
        plan_remember: context.remember_dword,
    };

    let attempt = switch.attempt(plan.running, context.already_active, plan.strategy);
    let login = switch.login.clone();
    let manual_minutes = minutes(steam.timing().manual_sign_in);
    let cancelled_after_start = |handed_over: bool| {
        if handed_over {
            format!(
                "Stopped waiting for the sign-in as {login}. Steam was left on its sign-in form; signing in there with \"Remember me\" ticked finishes the switch, and later switches are automatic."
            )
        } else {
            format!(
                "Stopped while Steam was starting as {login}. Nothing was rolled back: Steam carries on signing in on its own."
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
                "One Launch waited {manual_minutes} min for the sign-in as {login} and gave up. Steam is still on its sign-in form: sign in there with \"Remember me\" ticked, and later switches are automatic."
            )),
        ),
        Err(thrown) => {
            let cancelled = matches!(thrown, Thrown::Aborted);
            let message = match thrown {
                Thrown::Aborted => format!("Stopped before Steam was started as {login}."),
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
        ready_at: None,
        started_with_game: None,
    })
}
