// The plan of an Epic switch: everything the real switch would read, and every change it would make, in the Electron app's own words. Nothing here changes anything, and nothing is decrypted.

use ol_formats::epic::session::{fingerprint_of, short_fingerprint};
use ol_formats::js;
use ol_formats::winpath::join;
use ol_model::{
    LauncherId, LauncherInstall, PlanAccount, RunningGame, SwitchChange, SwitchPlan,
    SwitchPlanStep, SwitchStepKind, SwitchStrategy,
};

use super::{CurrentSession, EXE, EpicAdapter, id_tail};
use crate::steam::{SwitchPreflight, minutes, seconds, step};
use crate::{AccountRef, AdapterError};

pub struct StepIds {
    pub close: &'static str,
    pub capture: &'static str,
    pub backup: &'static str,
    pub file: &'static str,
    pub start: &'static str,
    pub wait: &'static str,
    pub verify: &'static str,
    pub manual: &'static str,
    pub recapture: &'static str,
}

// The real switch reports its progress against these.
pub const STEP: StepIds = StepIds {
    close: "close",
    capture: "capture",
    backup: "backup",
    file: "file",
    start: "start",
    wait: "wait",
    verify: "verify",
    manual: "manual",
    recapture: "recapture",
};

// What the plan learned, kept for the real switch so that it derives nothing twice.
pub struct PlanContext {
    pub install: LauncherInstall,
    pub ini_path: String,
    pub current: CurrentSession,
    // The session on disk is the target account's already.
    pub in_place: bool,
    pub already_active: bool,
    pub running: bool,
    // The plan's warning that the running launcher will be closed. A preflight leaves it out, because the dialog says it in its own confirm box.
    pub close_warning: Option<String>,
}

// What closing the launcher does, for the confirmation: a game started from it closes with it.
pub fn close_message(game: Option<&RunningGame>) -> String {
    match game {
        Some(game) => format!(
            "Epic Games Launcher is running {}. Switching closes the launcher and the game; unsaved progress in the game is lost.",
            game.name
        ),
        None => "Epic Games Launcher is running. Switching closes it.".to_owned(),
    }
}

// A session value as a plan shows it: its length and the start of its SHA-256, never the value.
fn shown(value: Option<&str>) -> String {
    match value {
        None => "(absent)".to_owned(),
        Some(value) => format!(
            "({} characters, fingerprint {})",
            js::utf16_len(value),
            short_fingerprint(Some(&fingerprint_of(value)))
        ),
    }
}

fn change(place: &str, before: String, after: &str) -> SwitchChange {
    SwitchChange {
        place: place.to_owned(),
        before,
        after: after.to_owned(),
    }
}

impl EpicAdapter<'_> {
    pub fn preflight(&self, account: &AccountRef) -> Result<SwitchPreflight, AdapterError> {
        let (mut plan, context) = self.build_switch_plan(account)?;
        let running_game = if context.running && !context.already_active {
            self.running_game()?
        } else {
            None
        };
        if let Some(close_warning) = &context.close_warning {
            plan.warnings.retain(|warning| warning != close_warning);
        }
        Ok(SwitchPreflight {
            launcher_running: context.running,
            close_message: close_message(running_game.as_ref()),
            running_game,
            already_active: context.already_active,
            plan,
        })
    }

    pub fn plan_switch(&self, account: &AccountRef) -> Result<SwitchPlan, AdapterError> {
        Ok(self.build_switch_plan(account)?.0)
    }

    pub fn build_switch_plan(
        &self,
        account: &AccountRef,
    ) -> Result<(SwitchPlan, PlanContext), AdapterError> {
        let timing = self.timing();
        let install = self.require_install()?;
        let ini_path = self.settings_path();
        let ini_paths = self.settings_paths();
        let ini_targets = if ini_paths.is_empty() {
            ini_path.clone()
        } else {
            ini_paths.join(" and ")
        };
        let both = ini_paths.len() > 1;
        let log_path = self.log_path();
        let running = self.is_running()?;
        let ini_text = self.read_ini()?;
        let current = self.current_session(ini_text.as_deref(), running)?;
        let stored = match &account.id {
            Some(id) => self.directory.session(id)?,
            None => None,
        };
        let disk = &current.disk;
        let mut warnings: Vec<String> = Vec::new();
        let mut notes: Vec<String> = Vec::new();

        // Running: the launcher's own log says it is signed in as this account. Closed: the file holds this account's session, so the next start signs in as it.
        let in_place = current
            .owner
            .as_ref()
            .is_some_and(|owner| owner.id == account.id)
            && (running || disk.remembered);
        let usable = stored
            .as_ref()
            .is_some_and(|stored| stored.stale_reason.is_none());
        let strategy = if in_place || usable {
            SwitchStrategy::Remembered
        } else {
            SwitchStrategy::Credentials
        };
        let remembered = strategy == SwitchStrategy::Remembered;
        let already_active = in_place && running;
        let login = &account.login;
        let name = &account.display_name;
        let expected_uid = stored
            .as_ref()
            .and_then(|stored| stored.launcher_uid.clone())
            .or_else(|| account.launcher_uid.clone());
        let fingerprint = short_fingerprint(disk.fingerprint.as_deref());
        let can_copy = if account.has_secret {
            " (the password can be copied from One Launch)"
        } else {
            ""
        };
        let sign_in_seconds = seconds(timing.sign_in);
        let manual_minutes = minutes(timing.manual_sign_in);

        let mut steps: Vec<SwitchPlanStep> = Vec::new();
        steps.push(step(
            STEP.close,
            SwitchStepKind::Process,
            "Close Epic Games Launcher",
            EXE.to_owned(),
            !running || already_active,
            if already_active {
                "Skipped: the launcher is already signed in as this account, so it is left running.".to_owned()
            } else if running {
                format!(
                    "Send {EXE} a window close (the launcher exits unless \"minimise to tray\" is on) and wait up to {} s. If it is still running after that, force-close {EXE} and every process it started. Any game running under the launcher closes with it.",
                    seconds(timing.shutdown)
                )
            } else {
                format!("Skipped: {EXE} is not running.")
            },
        ));

        let outgoing = current.owner.as_ref();
        let outgoing_stored = match outgoing.and_then(|owner| owner.id.as_deref()) {
            Some(id) => self.directory.session(id)?,
            None => None,
        };
        let outgoing_changed = outgoing.is_some()
            && outgoing_stored
                .as_ref()
                .is_none_or(|stored| Some(&stored.fingerprint) != disk.fingerprint.as_ref());
        // The file is read once the launcher has closed, because it writes the file when it exits. A launcher that stays running is not read again.
        let capture_needed = outgoing.is_some()
            && !already_active
            && (running || (disk.remembered && (!in_place || outgoing_changed)));
        let nothing_changes = "Nothing in the launcher's files changes.";
        steps.push(step(
            STEP.capture,
            SwitchStepKind::Backup,
            if in_place {
                "Save this account's session"
            } else {
                "Save the outgoing session"
            },
            ini_path.clone(),
            !capture_needed,
            match outgoing {
                _ if already_active => {
                    "Skipped: the launcher stays running, so its file is not re-read.".to_owned()
                }
                None if disk.remembered => format!(
                    "Skipped: the launcher remembers a session (fingerprint {fingerprint}) that no One Launch account owns, so it is left alone and not saved. To keep it, capture it from that account's page before switching."
                ),
                None => "Skipped: the launcher is not remembering any session.".to_owned(),
                Some(owner) if !capture_needed => format!(
                    "Skipped: {}'s stored snapshot already matches what the launcher remembers.",
                    owner.display_name
                ),
                Some(owner) if running => format!(
                    "After the launcher has closed (it writes the file on exit), read [RememberMe] and, if it holds {}'s session and it changed, store it, encrypted, as the account's snapshot. {nothing_changes}",
                    owner.display_name
                ),
                Some(owner) if in_place => format!(
                    "[RememberMe] holds {}'s session but it differs from the stored snapshot ({}), so store it, encrypted, as the account's snapshot. {nothing_changes}",
                    owner.display_name,
                    if outgoing_stored.is_some() {
                        "Epic rotated the token"
                    } else {
                        "no snapshot yet"
                    }
                ),
                Some(owner) => format!(
                    "[RememberMe] holds {}'s session{}; store it, encrypted, as the account's snapshot if it changed. {nothing_changes}",
                    owner.display_name,
                    if outgoing_changed {
                        " and it changed since it was last saved (Epic rotates tokens)"
                    } else {
                        ""
                    }
                ),
            },
        ));

        steps.push(step(
            STEP.backup,
            SwitchStepKind::Backup,
            "Back up GameUserSettings.ini",
            join(&[&self.config.backups_dir, "epic", "<timestamp>"]),
            already_active || in_place,
            if already_active || in_place {
                "Skipped: the file will not be modified.".to_owned()
            } else {
                format!(
                    "Copy {ini_targets} into a new snapshot folder so a failed switch can be rolled back. The newest {} snapshots are kept.",
                    self.config.backups_to_keep
                )
            },
        ));

        let enable_before = || disk.enable.clone().unwrap_or_else(|| "(absent)".to_owned());
        if in_place {
            steps.push(step(
                STEP.file,
                SwitchStepKind::File,
                "Put the account's session in place",
                ini_path.clone(),
                true,
                format!(
                    "No change: [RememberMe] already holds {name}'s session (fingerprint {fingerprint})."
                ),
            ));
        } else if let (true, Some(stored)) = (remembered, &stored) {
            let mut file = step(
                STEP.file,
                SwitchStepKind::File,
                "Put the account's session in place",
                ini_targets.clone(),
                false,
                format!(
                    "Decrypt {name}'s session snapshot from the vault and write it into [RememberMe] (Enable=True and Data) and [Offline] (Data) of {}. The new content is written to a temp file and renamed over the original. Every other line, including the per-account [<id>_*] sections, stays byte for byte. The blob itself is never shown or logged.",
                    if both {
                        "both settings files (the launcher reads one of them; writing both is harmless)"
                    } else {
                        "GameUserSettings.ini"
                    }
                ),
            );
            file.changes = vec![
                change("[RememberMe] Enable", enable_before(), "True"),
                change(
                    "[RememberMe] Data",
                    shown(disk.data.as_deref()),
                    &format!(
                        "(stored snapshot, fingerprint {}, captured {})",
                        short_fingerprint(Some(&stored.fingerprint)),
                        stored.captured_at
                    ),
                ),
                change(
                    "[Offline] Data",
                    shown(disk.offline_data.as_deref()),
                    "(from the same snapshot, or removed when it has none)",
                ),
            ];
            steps.push(file);
        } else {
            let signed_out = disk.data.is_none()
                && disk.offline_data.is_none()
                && disk
                    .enable
                    .as_deref()
                    .is_some_and(|enable| enable.eq_ignore_ascii_case("true"));
            let mut file = step(
                STEP.file,
                SwitchStepKind::File,
                "Sign the launcher out",
                ini_targets.clone(),
                signed_out,
                if signed_out {
                    "No change: the launcher is not remembering any session, so it will open on its sign-in page.".to_owned()
                } else {
                    format!(
                        "Remove Data from [RememberMe] and [Offline] and keep Enable=True, so the launcher opens on its sign-in page instead of signing in as {} and remembers the next sign-in. Everything else in {} is preserved.",
                        outgoing
                            .map_or("the previous account", |owner| owner.display_name.as_str()),
                        if both { "both files" } else { "the file" }
                    )
                },
            );
            if !signed_out {
                file.changes = vec![
                    change("[RememberMe] Enable", enable_before(), "True"),
                    change(
                        "[RememberMe] Data",
                        shown(disk.data.as_deref()),
                        "(removed)",
                    ),
                    change(
                        "[Offline] Data",
                        shown(disk.offline_data.as_deref()),
                        "(removed)",
                    ),
                ];
            }
            steps.push(file);
        }

        steps.push(step(
            STEP.start,
            SwitchStepKind::Launch,
            "Start Epic Games Launcher",
            format!("\"{}\"", install.exe_path),
            already_active,
            if already_active {
                "Skipped: the launcher is already running as this account.".to_owned()
            } else if remembered {
                format!(
                    "Start the launcher normally (no arguments, as the com.epicgames.launcher:// handler does). It signs in as {login} on its own from [RememberMe]."
                )
            } else {
                "Start the launcher normally. With no remembered session it opens on its sign-in page.".to_owned()
            },
        ));
        steps.push(step(
            STEP.wait,
            SwitchStepKind::Wait,
            "Wait for the launcher to come up",
            EXE.to_owned(),
            already_active,
            format!(
                "Poll the process list until {EXE} is running, up to {} s.",
                seconds(timing.startup)
            ),
        ));
        let expected_tail = expected_uid
            .as_deref()
            .filter(|uid| !uid.is_empty())
            .map(|uid| id_tail(Some(uid), 6));
        steps.push(step(
            STEP.verify,
            SwitchStepKind::Wait,
            "Confirm the launcher signed in",
            log_path.clone(),
            already_active,
            if already_active {
                "Skipped: the launcher is already signed in as this account.".to_owned()
            } else if remembered {
                format!(
                    "Read what the launcher appends to its own log (no tokens in it) until FAccountService::OnLoginComplete reports TRUE with {}, up to {sign_in_seconds} s. A FALSE line means the launcher did not sign in and the next step hands over to you; when the reason is a refused token (for example TOKEN_EXPIRED) the snapshot is also marked stale.",
                    match &expected_tail {
                        Some(tail) => format!("the Epic account id {tail}"),
                        None => "an account id, which is then recorded on the account".to_owned(),
                    }
                )
            } else {
                format!(
                    "Read what the launcher appends to its own log until FAccountService::OnLoginComplete reports TRUE{}. The launcher will not sign in on its own, so this goes straight to the next step.",
                    expected_tail
                        .as_ref()
                        .map(|tail| format!(" with the Epic account id {tail}"))
                        .unwrap_or_default()
                )
            },
        ));
        steps.push(step(
            STEP.manual,
            SwitchStepKind::Wait,
            "Wait for you to sign in",
            log_path,
            remembered,
            if remembered {
                format!(
                    "Runs only if the launcher rejects the stored session (expired token) or does not sign in within {sign_in_seconds} s. The launcher stays open on its sign-in page; you sign in there once as {login}{can_copy} and One Launch continues with the game as soon as the log reports the sign-in, up to {manual_minutes} min."
                )
            } else {
                format!(
                    "The launcher opens on its sign-in page. Sign in there as {login}{can_copy}. One Launch continues with the game as soon as the log reports the sign-in, up to {manual_minutes} min. Signing in as a different Epic account than the one linked to {name} ends the switch and rolls the file back."
                )
            },
        ));
        steps.push(step(
            STEP.recapture,
            SwitchStepKind::Backup,
            "Save the new session",
            ini_path.clone(),
            remembered,
            if remembered {
                "Runs only after a manual sign-in: the fresh [RememberMe] replaces the stale snapshot.".to_owned()
            } else {
                format!(
                    "Once the sign-in is reported, wait up to {} s for the launcher to write the new [RememberMe], then store it, encrypted, as {name}'s session snapshot and record the Epic account id on the account. If the launcher writes it later, it is saved at the next switch instead. Later switches use the remembered path.",
                    seconds(timing.capture)
                )
            },
        ));

        match (&ini_text, disk.enable.as_deref()) {
            (None, _) => warnings.push(format!(
                "{ini_path} does not exist. One Launch never creates it: restore the newest Epic Games backup from Settings, or start the launcher once by hand so it writes its settings file."
            )),
            (Some(_), Some(enable)) if !enable.eq_ignore_ascii_case("true") => {
                warnings.push(format!(
                    "[RememberMe] Enable is {enable} in {ini_path}, so the launcher does not sign in on its own at start. The switch sets it back to True."
                ));
            }
            _ => {}
        }
        let mut close_warning = None;
        if already_active {
            notes.push(format!(
                "Epic Games Launcher is running and signed in as {name}. The real switch would change nothing and would leave the launcher running."
            ));
        } else if in_place {
            notes.push(format!(
                "Epic Games Launcher already remembers {name}'s session, so it is only started."
            ));
        } else if running {
            let warning = format!(
                "Epic Games Launcher is running{}. It will be closed, along with any game running under it.",
                match (outgoing, &current.logged_uid) {
                    (Some(owner), _) => format!(" as {}", owner.display_name),
                    (None, Some(_)) => " as an account not linked to One Launch".to_owned(),
                    (None, None) => " on its sign-in page".to_owned(),
                }
            );
            warnings.push(warning.clone());
            close_warning = Some(warning);
        }
        if let (true, false, Some(uid), None) = (running, in_place, &current.logged_uid, outgoing) {
            warnings.push(format!(
                "The launcher's current sign-in (Epic account {}) is not linked to any One Launch account, so it will not be saved.",
                id_tail(Some(uid), 6)
            ));
        }
        if let (Some(reason), false) = (
            stored
                .as_ref()
                .and_then(|stored| stored.stale_reason.as_deref())
                .filter(|reason| !reason.is_empty()),
            in_place,
        ) {
            warnings.push(format!(
                "The stored session for {name} was rejected by the launcher ({reason}). You will sign in once by hand and One Launch will store the fresh session."
            ));
        }
        if !remembered && stored.is_none() && !in_place {
            warnings.push(format!(
                "No session snapshot is stored for {name} yet. The launcher will open on its sign-in page; sign in once and One Launch captures the session for later switches.{}",
                if account.has_secret {
                    ""
                } else {
                    " No password is stored in One Launch, so have it ready."
                }
            ));
        }
        if !in_place && disk.remembered && outgoing.is_none() {
            warnings.push(format!(
                "The launcher currently remembers a session that no One Launch account owns (fingerprint {fingerprint}). It will be replaced. The backup taken before this switch keeps a copy and is left out of the usual clean-up, so restoring it from Settings › Backups brings that sign-in back."
            ));
        }
        let filled = |uid: Option<&String>| uid.filter(|uid| !uid.is_empty()).cloned();
        if let (Some(own), Some(saved)) = (
            filled(account.launcher_uid.as_ref()),
            filled(
                stored
                    .as_ref()
                    .and_then(|stored| stored.launcher_uid.as_ref()),
            ),
        ) && own.to_lowercase() != saved.to_lowercase()
        {
            warnings.push(format!(
                "The Epic account id on this account ({}) differs from the one recorded with its session snapshot ({}). The snapshot's id is used to confirm the sign-in.",
                id_tail(Some(&own), 6),
                id_tail(Some(&saved), 6)
            ));
        }

        let plan = SwitchPlan {
            launcher: LauncherId::Epic,
            account: PlanAccount {
                id: account.id.clone(),
                display_name: account.display_name.clone(),
                login: account.login.clone(),
                launcher_uid: expected_uid,
            },
            strategy,
            most_recent_login: outgoing.map(|owner| owner.login.clone()),
            auto_login_user: None,
            running,
            steps,
            warnings,
            notes,
            executable: ini_text.is_some(),
            generated_at: ol_store::iso_timestamp(self.platform.clock.now()),
        };
        Ok((
            plan,
            PlanContext {
                install,
                ini_path,
                current,
                in_place,
                already_active,
                running,
                close_warning,
            },
        ))
    }
}
