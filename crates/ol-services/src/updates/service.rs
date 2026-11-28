use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ol_formats::latest_yml::Yaml;
use ol_platform_api::update_http::ReleaseTag;
use ol_platform_api::{Clock, InstallerStart};
use ol_store::iso_timestamp;
use semver::Version;
use serde_json::{Value, json};
use thiserror::Error;

use super::check::Checked;
use super::download::Fetched;
use super::failure::UpdateFailure;
use super::manifest::{ManifestFile, ReleaseKey, Verified, sha512_of_file, verify_manifest};
use crate::install::{UpdateStatus, UpdateStep, UpdateUnavailable};
use crate::log::Log;

pub const FIRST_CHECK: Duration = Duration::from_millis(15_000);
pub const CHECK_EVERY: Duration = Duration::from_millis(6 * 60 * 60 * 1000);

const SESSION_END: &str = "Windows is shutting down or signing out";
const CRASH: &str = "an unexpected error happened in this run";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Background,
    Manual,
}

impl Origin {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Background => "background",
            Self::Manual => "manual",
        }
    }
}

// What the caller carries out for the machine, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateEffect {
    // The one timer, armed for this long; `timer_fired` once it ends.
    Arm(Duration),
    Disarm,
    // `check::check`, answered through `checked`.
    Check,
    // `download::download` of this file of this release, its percents through `progress` and its end through `downloaded`.
    Download { tag: ReleaseTag, file: ManifestFile },
    CancelDownload,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
#[error("No verified update has been downloaded yet")]
pub struct UpdateNotReady;

impl UpdateNotReady {
    pub const fn code(self) -> &'static str {
        "NOT_READY"
    }
}

pub struct UpdatesSetup<'a> {
    pub version: Version,
    pub keys: &'a [ReleaseKey<'a>],
    pub auto: bool,
    pub this_copy: Result<(), UpdateUnavailable>,
    pub log: &'a Log,
    pub clock: Arc<dyn Clock>,
}

struct Waiting {
    version: String,
    path: PathBuf,
    sha512: String,
}

enum Run {
    Checking(Origin),
    Downloading {
        origin: Origin,
        verified: Verified,
        cancelled: bool,
    },
}

// The Electron app's `UpdateService` as a machine with no thread, timer or network of its own: each event is a call, and what has to happen outside comes back as effects.
pub struct Updates {
    version: Version,
    keys: Vec<(String, String)>,
    auto: bool,
    available: bool,
    log: Log,
    clock: Arc<dyn Clock>,
    status: UpdateStatus,
    armed: bool,
    run: Option<Run>,
    // A timer fired while this run went on: the next one is armed when it ends.
    rearm: bool,
    verified: Option<Waiting>,
    install_requested: bool,
    blocked: Option<&'static str>,
    stopped: bool,
}

impl Updates {
    pub fn new(setup: UpdatesSetup<'_>) -> Self {
        let log = setup.log.child("updates");
        let status = match setup.this_copy {
            Ok(()) => UpdateStatus::Idle,
            Err(reason) => {
                log.info("updates unavailable", json!({ "reason": reason.as_str() }));
                UpdateStatus::Unavailable(reason)
            }
        };
        Self {
            version: setup.version,
            keys: setup
                .keys
                .iter()
                .map(|key| (key.id.to_owned(), key.spki.to_owned()))
                .collect(),
            auto: setup.auto,
            available: setup.this_copy.is_ok(),
            log,
            clock: setup.clock,
            status,
            armed: false,
            run: None,
            rearm: false,
            verified: None,
            install_requested: false,
            blocked: None,
            stopped: false,
        }
    }

    pub fn status(&self) -> UpdateStatus {
        self.status.clone()
    }

    // What `check::check` is to compare the release with.
    pub const fn running(&self) -> &Version {
        &self.version
    }

    pub fn start(&mut self) -> Vec<UpdateEffect> {
        let mut effects = Vec::new();
        if self.available {
            self.schedule(FIRST_CHECK, &mut effects);
        }
        effects
    }

    // The Check for updates button: the status to show at once, and what to do.
    pub fn check(&mut self) -> (UpdateStatus, Vec<UpdateEffect>) {
        let mut effects = Vec::new();
        if self.available && !self.stopped && self.verified.is_none() && self.run.is_none() {
            self.begin(Origin::Manual, &mut effects);
        }
        (self.status(), effects)
    }

    pub fn set_auto(&mut self, on: bool) -> Vec<UpdateEffect> {
        let mut effects = Vec::new();
        if self.auto == on {
            return effects;
        }
        self.auto = on;
        if !self.available || self.stopped {
            return effects;
        }
        if on {
            self.schedule(FIRST_CHECK, &mut effects);
            return effects;
        }
        self.disarm(&mut effects);
        if let Some(Run::Downloading {
            origin: Origin::Background,
            cancelled,
            ..
        }) = &mut self.run
            && !*cancelled
        {
            *cancelled = true;
            self.log.info(
                "update download cancelled: automatic updates turned off",
                (),
            );
            effects.push(UpdateEffect::CancelDownload);
        }
        effects
    }

    pub fn timer_fired(&mut self) -> Vec<UpdateEffect> {
        let mut effects = Vec::new();
        if !self.armed || self.stopped {
            return effects;
        }
        self.armed = false;
        self.rearm = true;
        if self.run.is_none() {
            self.begin(Origin::Background, &mut effects);
        }
        effects
    }

    pub fn checked(&mut self, answer: Result<Checked, UpdateFailure>) -> Vec<UpdateEffect> {
        let mut effects = Vec::new();
        let Some(Run::Checking(origin)) = self.run else {
            return effects;
        };
        if self.stopped {
            return effects;
        }
        self.run = None;
        match answer {
            Err(failure) => self.fail(UpdateStep::Check, &failure.to_string(), json!({})),
            Ok(Checked::UpToDate) => {
                self.status = UpdateStatus::UpToDate {
                    checked_at: iso_timestamp(self.clock.now()),
                };
                self.log.info("update check: up to date", ());
            }
            // A background check already on its way when automatic updates went off still completes, and downloads nothing (triage S12.2-11).
            Ok(Checked::Newer { .. }) if origin == Origin::Background && !self.auto => {
                self.status = UpdateStatus::Idle;
            }
            Ok(Checked::Newer { tag, manifest, .. }) => {
                let keys: Vec<ReleaseKey<'_>> = self
                    .keys
                    .iter()
                    .map(|(id, spki)| ReleaseKey { id, spki })
                    .collect();
                match verify_manifest(&manifest, &keys) {
                    Err(refusal) => {
                        let version = match manifest.get("version") {
                            Some(Yaml::Text(text)) => Value::from(text.as_str()),
                            _ => Value::Null,
                        };
                        self.fail(
                            UpdateStep::Verify,
                            &refusal.detail,
                            json!({ "version": version, "reason": refusal.reason.as_str() }),
                        );
                    }
                    Ok(verified) => {
                        self.status = UpdateStatus::Downloading {
                            version: verified.version.clone(),
                            percent: 0,
                        };
                        effects.push(UpdateEffect::Download {
                            tag,
                            file: verified.file.clone(),
                        });
                        self.log.info(
                            "update available",
                            json!({ "version": verified.version, "keyId": verified.key_id, "size": verified.file.size }),
                        );
                        self.run = Some(Run::Downloading {
                            origin,
                            verified,
                            cancelled: false,
                        });
                        return effects;
                    }
                }
            }
        }
        self.ended(&mut effects);
        effects
    }

    pub fn progress(&mut self, percent: u8) {
        if self.stopped {
            return;
        }
        if let UpdateStatus::Downloading { percent: shown, .. } = &mut self.status {
            *shown = percent.min(100);
        }
    }

    pub fn downloaded(&mut self, answer: Result<Fetched, UpdateFailure>) -> Vec<UpdateEffect> {
        let mut effects = Vec::new();
        if self.stopped {
            return effects;
        }
        let verified = match self.run.take() {
            Some(Run::Downloading { verified, .. }) => verified,
            other => {
                self.run = other;
                return effects;
            }
        };
        let version = verified.version;
        match answer {
            Err(failure) => self.fail(
                UpdateStep::Download,
                &failure.to_string(),
                json!({ "version": version }),
            ),
            Ok(Fetched::Cancelled) => self.status = UpdateStatus::Idle,
            // The download already refuses other bytes; this is the second lock.
            Ok(Fetched::Ready { sha512, .. }) if sha512 != verified.file.sha512 => self.fail(
                UpdateStep::Verify,
                "the downloaded file does not match the signed release",
                json!({ "version": version, "sha512": first_12(&sha512) }),
            ),
            Ok(Fetched::Ready { path, .. }) => {
                self.disarm(&mut effects);
                self.status = UpdateStatus::Ready {
                    version: version.clone(),
                };
                self.log.info(
                    "update ready",
                    json!({ "version": version, "sha512": first_12(&verified.file.sha512) }),
                );
                self.verified = Some(Waiting {
                    version,
                    path,
                    sha512: verified.file.sha512,
                });
            }
        }
        self.ended(&mut effects);
        effects
    }

    // Restart to update.
    pub fn request_install(&mut self) -> Result<Vec<UpdateEffect>, UpdateNotReady> {
        let waiting = self.verified.as_ref().ok_or(UpdateNotReady)?;
        self.install_requested = true;
        self.log.info(
            "update install requested",
            json!({ "version": waiting.version }),
        );
        Ok(vec![UpdateEffect::Quit])
    }

    pub fn on_session_end(&mut self) {
        self.blocked = Some(SESSION_END);
    }

    pub fn on_crash(&mut self) {
        self.blocked = Some(CRASH);
    }

    pub fn stop(&mut self) -> Vec<UpdateEffect> {
        let mut effects = Vec::new();
        if self.stopped {
            return effects;
        }
        self.stopped = true;
        self.disarm(&mut effects);
        if let Some(Run::Downloading { cancelled, .. }) = &mut self.run
            && !*cancelled
        {
            *cancelled = true;
            effects.push(UpdateEffect::CancelDownload);
        }
        effects
    }

    // From the app's quit, after the database has closed.
    pub fn install_at_quit(&mut self, exit_code: i32, installer: &dyn InstallerStart) {
        let Some(update) = &self.verified else {
            return;
        };
        let version = update.version.as_str();
        let relaunch = self.install_requested;
        if !relaunch && !self.auto {
            self.log.info(
                "update left for a restart: automatic updates are off",
                json!({ "version": version }),
            );
            return;
        }
        if let Some(reason) = self.blocked {
            self.log.info(
                "update not installed at quit",
                json!({ "version": version, "reason": reason }),
            );
            return;
        }
        if exit_code != 0 {
            self.log.info(
                "update not installed at quit",
                json!({ "version": version, "exitCode": exit_code }),
            );
            return;
        }
        match sha512_of_file(&update.path) {
            Err(error) => {
                self.log.warn(
                    "update install refused: the downloaded file cannot be read",
                    json!({ "version": version, "error": error.to_string() }),
                );
                return;
            }
            Ok(measured) if measured != update.sha512 => {
                self.log.warn(
                    "update install refused: the downloaded file changed after it was verified",
                    json!({ "version": version }),
                );
                return;
            }
            Ok(_) => {}
        }
        let args: &[&str] = if relaunch {
            &["--updated", "/S", "--force-run"]
        } else {
            &["--updated", "/S"]
        };
        let started = installer.start(&update.path, args);
        self.log.info(
            "update install",
            json!({ "version": version, "relaunch": relaunch, "started": started }),
        );
    }

    fn begin(&mut self, origin: Origin, effects: &mut Vec<UpdateEffect>) {
        self.status = UpdateStatus::Checking;
        self.run = Some(Run::Checking(origin));
        self.log
            .info("update check", json!({ "origin": origin.as_str() }));
        effects.push(UpdateEffect::Check);
    }

    // As the `finally` of the Electron timer's run: a background run arms the next check when it ends, whatever its outcome.
    fn ended(&mut self, effects: &mut Vec<UpdateEffect>) {
        if std::mem::take(&mut self.rearm) {
            self.schedule(CHECK_EVERY, effects);
        }
    }

    fn schedule(&mut self, delay: Duration, effects: &mut Vec<UpdateEffect>) {
        self.disarm(effects);
        if self.stopped || !self.auto || self.verified.is_some() {
            return;
        }
        self.armed = true;
        effects.push(UpdateEffect::Arm(delay));
    }

    fn disarm(&mut self, effects: &mut Vec<UpdateEffect>) {
        if std::mem::take(&mut self.armed) {
            effects.push(UpdateEffect::Disarm);
        }
    }

    fn fail(&mut self, step: UpdateStep, message: &str, mut meta: Value) {
        self.status = UpdateStatus::Failed {
            step,
            message: message.to_owned(),
        };
        meta["message"] = Value::from(message);
        let line = match step {
            UpdateStep::Check => "update check failed",
            UpdateStep::Download => "update download failed",
            UpdateStep::Verify => "update verify failed",
        };
        self.log.warn(line, meta);
    }
}

fn first_12(sha512: &str) -> &str {
    sha512.get(..12).unwrap_or(sha512)
}
