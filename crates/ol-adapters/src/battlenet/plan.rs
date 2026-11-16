// The plan of a Battle.net switch: everything the real switch would read, and every change it would make, in the Electron app's own words. Nothing here changes anything. A saved sign-in is opened once, for the name of the registry value it goes under, and dropped.

use ol_formats::battlenet::config::plan_saved_account_names;
use ol_formats::battlenet::session::{describe_token, parse_blob, short_fingerprint};
use ol_formats::js;
use ol_formats::winpath::join;
use ol_model::{
    LauncherId, LauncherInstall, PlanAccount, RunningGame, SwitchChange, SwitchPlan,
    SwitchPlanStep, SwitchStepKind, SwitchStrategy,
};

use super::{BattlenetAdapter, CurrentSession, EXE, LAUNCHER_EXE, UNIFIED_AUTH_PATH, id_tail};
use crate::steam::{SwitchPreflight, minutes, plural, seconds, step};
use crate::{AccountRef, AdapterError};

pub struct StepIds {
    pub close: &'static str,
    pub capture: &'static str,
    pub backup: &'static str,
    pub registry: &'static str,
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
    registry: "registry",
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
    pub start_exe: String,
    pub config_path: String,
    // There was a Battle.net.config when the plan was made. One Launch never makes one.
    pub config_exists: bool,
    pub current: CurrentSession,
    // The token in the registry is the target account's already.
    pub in_place: bool,
    pub already_active: bool,
    pub running: bool,
    // The plan found the target login first in SavedAccountNames, which is why its file step is passed over.
    pub login_first: bool,
    // The plan's warning that the running client will be closed. A preflight leaves it out, because the dialog says it in its own confirm box.
    pub close_warning: Option<String>,
}

// What closing the client does, for the confirmation: a game that runs already keeps running when the client exits.
pub fn close_message(game: Option<&RunningGame>) -> String {
    match game {
        Some(game) => format!(
            "Battle.net is running {}. Switching closes Battle.net; the game keeps running.",
            game.name
        ),
        None => "Battle.net is running. Switching closes it.".to_owned(),
    }
}

fn removed(place: &str, before: String) -> SwitchChange {
    SwitchChange {
        place: format!("UnifiedAuth\\{place}"),
        before,
        after: "(removed)".to_owned(),
    }
}

impl BattlenetAdapter<'_> {
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

    // The name of the registry value an account's saved sign-in goes under. The token itself is dropped unread.
    fn stored_value_name(&self, account_id: Option<&str>) -> Option<String> {
        let stored = self.directory.decrypt_session(account_id?).ok()??;
        let blob = parse_blob(stored.expose()).ok()?;
        Some(blob.unified_auth.name.clone())
    }

    pub fn build_switch_plan(
        &self,
        account: &AccountRef,
    ) -> Result<(SwitchPlan, PlanContext), AdapterError> {
        let timing = self.timing();
        let install = self.require_install()?;
        let start_exe = self.start_exe(&install);
        let running = self.is_running()?;
        let config = self.read_config()?;
        let current = self.current_session(running)?;
        let stored = match &account.id {
            Some(id) => self.directory.session(id)?,
            None => None,
        };
        let prefs_uid = current
            .owner
            .as_ref()
            .and_then(|owner| owner.launcher_uid.clone())
            .or_else(|| current.logged_uid.clone());
        let prefs_file = self.read_user_prefs(prefs_uid.as_deref());
        let hides = prefs_file
            .as_ref()
            .is_some_and(|(_, prefs)| prefs.hide_on_close == Some(true));
        let prompts = prefs_file
            .as_ref()
            .is_some_and(|(_, prefs)| prefs.display_exit_prompt == Some(true));
        let mut warnings: Vec<String> = Vec::new();
        let mut notes: Vec<String> = Vec::new();
        let log_dir = self.log_dir();

        let in_place = current
            .owner
            .as_ref()
            .is_some_and(|owner| owner.id == account.id)
            && (running || current.remembered());
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
        let uses_stored = remembered && !in_place;
        let login = &account.login;
        let name = &account.display_name;
        let expected_uid = stored
            .as_ref()
            .and_then(|stored| stored.launcher_uid.clone())
            .or_else(|| account.launcher_uid.clone());
        let fingerprint = short_fingerprint(current.fingerprint.as_deref());
        let can_copy = if account.has_secret {
            " (the password can be copied from One Launch)"
        } else {
            ""
        };
        let shutdown_seconds = seconds(timing.shutdown);
        let sign_in_seconds = seconds(timing.sign_in);
        let manual_minutes = minutes(timing.manual_sign_in);

        let mut steps: Vec<SwitchPlanStep> = Vec::new();
        steps.push(step(
            STEP.close,
            SwitchStepKind::Process,
            "Close Battle.net",
            EXE.to_owned(),
            !running || already_active,
            if already_active {
                "Skipped: the client is already signed in as this account, so it is left running.".to_owned()
            } else if running {
                format!(
                    "Send {EXE} a window close (which exits the client{}{}) and wait up to {shutdown_seconds} s for every {EXE} process to be gone, since the client writes its config files at the end of its shutdown. If it is still running after that, force-close every {EXE} process and nothing it started. Agent.exe is left alone. A game already running keeps running.",
                    if hides {
                        ", except that this account has \"close to system tray\" on"
                    } else {
                        ""
                    },
                    if prompts { " after the exit prompt" } else { "" }
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
                .is_none_or(|stored| Some(&stored.fingerprint) != current.fingerprint.as_ref());
        // The token is read once the client has closed. A client that stays running is not read again.
        let capture_needed = outgoing.is_some()
            && !already_active
            && (running || (current.remembered() && (!in_place || outgoing_changed)));
        let nothing_changes = "Nothing of the client's changes.";
        steps.push(step(
            STEP.capture,
            SwitchStepKind::Backup,
            if in_place {
                "Save this account's session"
            } else {
                "Save the outgoing session"
            },
            UNIFIED_AUTH_PATH.to_owned(),
            !capture_needed,
            match outgoing {
                _ if already_active => {
                    "Skipped: the client stays running, so its token is not re-read.".to_owned()
                }
                None if current.remembered() => format!(
                    "Skipped: the client remembers a session (fingerprint {fingerprint}) that no One Launch account owns, so it is left alone and not saved. To keep it, capture it from that account's page before switching."
                ),
                None => "Skipped: the client is not remembering any session.".to_owned(),
                Some(owner) if !capture_needed => format!(
                    "Skipped: {}'s stored snapshot already matches what the client remembers.",
                    owner.display_name
                ),
                Some(owner) if running => format!(
                    "After the client has closed, read the UnifiedAuth value and, if the last run's log names {} and the value changed, store it, encrypted, as the account's snapshot. {nothing_changes}",
                    owner.display_name
                ),
                Some(owner) if in_place => format!(
                    "The UnifiedAuth value holds {}'s session but differs from the stored snapshot ({}), so store it, encrypted, as the account's snapshot. {nothing_changes}",
                    owner.display_name,
                    if outgoing_stored.is_some() {
                        "the client rotates the token at every sign-in"
                    } else {
                        "no snapshot yet"
                    }
                ),
                Some(owner) => format!(
                    "The UnifiedAuth value holds {}'s session{}; store it, encrypted, as the account's snapshot if it changed. {nothing_changes}",
                    owner.display_name,
                    if outgoing_changed {
                        " and it changed since it was last saved (the client rotates it at every sign-in)"
                    } else {
                        ""
                    }
                ),
            },
        ));

        steps.push(step(
            STEP.backup,
            SwitchStepKind::Backup,
            "Back up Battle.net.config and the UnifiedAuth value",
            join(&[&self.config.backups_dir, "battlenet", "<timestamp>"]),
            already_active || in_place,
            if already_active || in_place {
                "Skipped: nothing will be modified.".to_owned()
            } else {
                format!(
                    "Copy {} into a new snapshot folder and record {} in it, vault-encrypted, so a failed switch can be rolled back. The newest {} snapshots are kept.",
                    if config.text.is_some() {
                        config.path.as_str()
                    } else {
                        "(no Battle.net.config to copy)"
                    },
                    plural(current.tokens.len(), "UnifiedAuth value"),
                    self.config.backups_to_keep
                )
            },
        ));

        let token_title = "Put the account's session token in place";
        if in_place {
            steps.push(step(
                STEP.registry,
                SwitchStepKind::Registry,
                token_title,
                UNIFIED_AUTH_PATH.to_owned(),
                true,
                format!(
                    "No change: UnifiedAuth already holds {name}'s token (fingerprint {fingerprint})."
                ),
            ));
        } else if let (true, Some(stored)) = (uses_stored, &stored) {
            let stored_name = self.stored_value_name(account.id.as_deref());
            let is_stored = |token_name: &str| {
                stored_name
                    .as_deref()
                    .is_some_and(|stored| stored.to_lowercase() == token_name.to_lowercase())
            };
            let existing = current.tokens.iter().find(|token| is_stored(&token.name));
            let mut registry = step(
                STEP.registry,
                SwitchStepKind::Registry,
                token_title,
                UNIFIED_AUTH_PATH.to_owned(),
                false,
                format!(
                    "Decrypt {name}'s session snapshot from the vault and write it as the REG_BINARY value under UnifiedAuth (reg add /t REG_BINARY). Any other value there (the outgoing token, saved in the step above when it belongs to a One Launch account) is removed, the way the client's own logout removes it. The bytes are a DPAPI blob bound to this Windows user and are never shown or logged. Identity, EncryptionKey, Launch Options, and every file under the client's folders stay untouched."
                ),
            );
            registry.changes.push(SwitchChange {
                place: format!(
                    "UnifiedAuth\\{}",
                    stored_name.as_deref().unwrap_or("(name from the snapshot)")
                ),
                before: describe_token(existing),
                after: format!(
                    "(stored snapshot, fingerprint {}, captured {})",
                    short_fingerprint(Some(&stored.fingerprint)),
                    stored.captured_at
                ),
            });
            registry.changes.extend(
                current
                    .tokens
                    .iter()
                    .filter(|token| !is_stored(&token.name))
                    .map(|token| removed(&token.name, describe_token(Some(token)))),
            );
            steps.push(registry);
        } else {
            let none = current.tokens.is_empty();
            let mut registry = step(
                STEP.registry,
                SwitchStepKind::Registry,
                "Sign the client out",
                UNIFIED_AUTH_PATH.to_owned(),
                none,
                if none {
                    "No change: the client has no remembered session, so it will open on its login page.".to_owned()
                } else {
                    format!(
                        "Remove the UnifiedAuth value (reg delete), which is what the client's own logout does, so it opens on its login page instead of signing in as {}. \"Keep me logged in\" (Client.AutoLogin) is not touched, so the next sign-in is remembered.",
                        outgoing
                            .map_or("the previous account", |owner| owner.display_name.as_str())
                    )
                },
            );
            registry.changes = current
                .tokens
                .iter()
                .map(|token| removed(&token.name, describe_token(Some(token))))
                .collect();
            steps.push(registry);
        }

        let path = &config.path;
        let mut file_write = None;
        let mut file_skip = None;
        match (&config.text, &config.error, config.round_trips) {
            (None, _, _) => {
                file_skip = Some(format!(
                    "Skipped: {path} does not exist. One Launch never creates it; the client writes it at its next exit."
                ));
            }
            (Some(_), Some(error), _) => {
                file_skip = Some(format!(
                    "Skipped: {path} could not be parsed ({error}), so it is left alone."
                ));
            }
            (Some(_), None, Some(false)) => {
                file_skip = Some(format!(
                    "Skipped: {path} does not round-trip through One Launch's JSON writer byte for byte, so it is left alone rather than rewritten with a different layout."
                ));
            }
            (Some(text), None, _) => match plan_saved_account_names(text, login) {
                Ok(write) => {
                    if write.noop {
                        file_skip = Some(format!(
                            "No change: {login} is already first in Client.SavedAccountNames."
                        ));
                    }
                    file_write = Some(write);
                }
                Err(error) => file_skip = Some(format!("Skipped: {error}")),
            },
        }
        let same_login =
            |saved: &String| js::trim(saved).to_lowercase() == js::trim(login).to_lowercase();
        let mut file = step(
            STEP.file,
            SwitchStepKind::File,
            "Put the login first in the saved-accounts list",
            path.clone(),
            already_active || file_skip.is_some(),
            match (&file_skip, &file_write) {
                _ if already_active => {
                    "Skipped: the client stays running as this account.".to_owned()
                }
                (Some(skip), _) => skip.clone(),
                (None, write) => format!(
                    "Move {login} to the front of Client.SavedAccountNames{}, rewriting the file with the same layout (4-space indent, CRLF, \"\\/\" escapes) via a temp file and rename; every other key stays byte for byte. The client moves the signed-in login to the front at every sign-in and at start only accepts the UnifiedAuth token of the login that is first (2026-09-13: every switch with another login first was asked for a password; the one with the target first signed in silently).",
                    if write
                        .as_ref()
                        .is_some_and(|write| !write.before.iter().any(same_login))
                    {
                        " (it is not in the list yet, so it is added)"
                    } else {
                        ""
                    }
                ),
            },
        );
        if let (Some(write), None) = (&file_write, &file_skip) {
            file.changes.clone_from(&write.changes);
        }
        steps.push(file);

        let starts_launcher = start_exe
            .to_lowercase()
            .ends_with(&LAUNCHER_EXE.to_lowercase());
        steps.push(step(
            STEP.start,
            SwitchStepKind::Launch,
            "Start Battle.net",
            format!("\"{start_exe}\""),
            already_active,
            if already_active {
                "Skipped: the client is already running as this account.".to_owned()
            } else {
                format!(
                    "{} {}",
                    if starts_launcher {
                        format!(
                            "Start {LAUNCHER_EXE} with no arguments, as the Start Menu shortcut does (it applies any pending update, then starts the client with --from-launcher)."
                        )
                    } else {
                        format!("Start {EXE} with no arguments.")
                    },
                    if remembered {
                        format!(
                            "The client signs in as {login} on its own from the UnifiedAuth token."
                        )
                    } else {
                        "With no token the client opens on its login page. The \"Welcome back\" email there comes from the login page's own web session (a cookie One Launch does not touch), so it may name the previous account; \"Switch account\" on that page lists the saved logins.".to_owned()
                    }
                )
            },
        ));
        steps.push(step(
            STEP.wait,
            SwitchStepKind::Wait,
            "Wait for the client to come up",
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
            .map(|uid| id_tail(Some(uid), 4));
        steps.push(step(
            STEP.verify,
            SwitchStepKind::Wait,
            "Confirm the client signed in",
            log_dir.clone(),
            already_active,
            if already_active {
                "Skipped: the client is already signed in as this account.".to_owned()
            } else if remembered {
                format!(
                    "Read the new run's log (battle.net-<timestamp>.log; no tokens in it) until \"Logged into Battle.net successfully\" names {}, up to {sign_in_seconds} s. A \"Login failed\" line means the token was rejected: the snapshot is marked stale and the next step hands over to you.",
                    match &expected_tail {
                        Some(tail) => format!("the account id {tail}"),
                        None => "an account id, which is then recorded on the account".to_owned(),
                    }
                )
            } else {
                "Skipped on this path: with no token the client waits on its login page, so the switch hands over to you at once (next step).".to_owned()
            },
        ));
        steps.push(step(
            STEP.manual,
            SwitchStepKind::Wait,
            "Wait for you to sign in",
            log_dir,
            remembered,
            if remembered {
                format!(
                    "Runs only if the client rejects the stored token or does not sign in within {sign_in_seconds} s. The client stays on its login page; you sign in there once as {login}{can_copy} with \"Keep me logged in\" on, and One Launch continues with the game as soon as the log reports the sign-in, up to {manual_minutes} min."
                )
            } else {
                format!(
                    "The client opens on its login page, showing the last account it signed in as. Click \"Switch account\" there, pick or type {login}, and sign in{can_copy} with \"Keep me logged in\" on. One Launch continues with the game as soon as the log reports the sign-in, up to {manual_minutes} min. Signing in as a different Battle.net account than the one linked to {name} ends the switch and rolls everything back."
                )
            },
        ));
        steps.push(step(
            STEP.recapture,
            SwitchStepKind::Backup,
            "Save the new session",
            UNIFIED_AUTH_PATH.to_owned(),
            remembered,
            if remembered {
                "Runs only after a manual sign-in: the fresh UnifiedAuth value replaces the stale snapshot.".to_owned()
            } else {
                format!(
                    "Once the sign-in is reported, read the UnifiedAuth value the client wrote at that sign-in (up to {} s), store it, encrypted, as {name}'s session snapshot, and record the account id on the account. Later switches use the remembered path.",
                    seconds(timing.capture)
                )
            },
        ));

        match (&config.text, &config.error, config.round_trips) {
            (None, _, _) => warnings.push(format!(
                "{path} does not exist. One Launch never creates it; the saved-accounts step is skipped and the client will write the file at its next exit."
            )),
            (Some(_), Some(error), _) => warnings.push(format!(
                "{path} could not be parsed ({error}). The saved-accounts step is skipped; the rest of the switch does not depend on it."
            )),
            (Some(_), None, Some(false)) => warnings.push(format!(
                "{path} has a layout One Launch cannot reproduce exactly, so the saved-accounts step is skipped rather than rewriting the file differently."
            )),
            _ => {}
        }
        if let Some(auto_login) = config
            .client
            .as_ref()
            .and_then(|client| client.auto_login.as_deref())
            .filter(|auto_login| auto_login.to_lowercase() != "true")
        {
            warnings.push(format!(
                "\"Keep me logged in\" (Client.AutoLogin) is {auto_login} in Battle.net.config, so the client will not sign in on its own from a token. One Launch never changes that setting: turn it on on the client's login page, and the next sign-in is remembered."
            ));
        }
        if hides {
            warnings.push(format!(
                "The current account's Battle.net preferences close to the system tray instead of exiting ({}: HideOnClose). The close step will wait {shutdown_seconds} s and then force-close the client.",
                prefs_file
                    .as_ref()
                    .map_or("per-account config", |(path, _)| path.as_str())
            ));
        }
        if prompts {
            warnings.push(format!(
                "The current account's Battle.net preferences show an exit prompt on close (DisplayExitPrompt). Confirm it in the client window, or the close step force-closes the client after {shutdown_seconds} s."
            ));
        }
        if uses_stored && file_write.is_none() {
            warnings.push(format!(
                "{login} cannot be put first in Client.SavedAccountNames (see the saved-accounts step), and the client only accepts the token of the login that is first, so it will ask for the password."
            ));
        }
        if current.tokens.len() > 1 {
            warnings.push(format!(
                "UnifiedAuth holds {} values; the client is expected to keep one. The value matching a stored snapshot is treated as the session, the others as leftovers.",
                current.tokens.len()
            ));
        }
        let mut close_warning = None;
        if already_active {
            notes.push(format!(
                "Battle.net is running and signed in as {name}. The real switch would change nothing and would leave the client running."
            ));
        } else if in_place {
            notes.push(format!(
                "Battle.net already remembers {name}'s session, so it is only started."
            ));
        } else if running {
            let warning = format!(
                "Battle.net is running{}. It will be closed.",
                match (outgoing, &current.logged_uid) {
                    (Some(owner), _) => format!(" as {}", owner.display_name),
                    (None, Some(_)) => " as an account not linked to One Launch".to_owned(),
                    (None, None) => " on its login page".to_owned(),
                }
            );
            warnings.push(warning.clone());
            close_warning = Some(warning);
        }
        if let (true, false, Some(uid), None) = (running, in_place, &current.logged_uid, outgoing) {
            warnings.push(format!(
                "The client's current sign-in (Battle.net account {}) is not linked to any One Launch account, so its session will not be saved.",
                id_tail(Some(uid), 4)
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
                "The stored session for {name} was rejected by the client ({reason}). You will sign in once by hand and One Launch will store the fresh session."
            ));
        }
        if !remembered && stored.is_none() && !in_place {
            warnings.push(format!(
                "No session snapshot is stored for {name} yet. The client will open on its login page; sign in once with \"Keep me logged in\" on and One Launch captures the session for later switches.{}",
                if account.has_secret {
                    ""
                } else {
                    " No password is stored in One Launch, so have it ready."
                }
            ));
        }
        if !in_place && current.remembered() && outgoing.is_none() {
            warnings.push(format!(
                "The client currently remembers a session that no One Launch account owns (fingerprint {fingerprint}). It will be removed. The backup taken before this switch keeps a copy and is left out of the usual clean-up, so restoring it from Settings › Backups brings that sign-in back."
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
                "The account id on this account ({}) differs from the one recorded with its session snapshot ({}). The snapshot's id is used to confirm the sign-in.",
                id_tail(Some(&own), 4),
                id_tail(Some(&saved), 4)
            ));
        }

        let plan = SwitchPlan {
            launcher: LauncherId::Battlenet,
            account: PlanAccount {
                id: account.id.clone(),
                display_name: account.display_name.clone(),
                login: account.login.clone(),
                launcher_uid: expected_uid,
            },
            strategy,
            most_recent_login: outgoing.map(|owner| owner.login.clone()),
            auto_login_user: config
                .client
                .as_ref()
                .and_then(|client| client.saved_account_names.first().cloned()),
            running,
            steps,
            warnings,
            notes,
            executable: true,
            generated_at: ol_store::iso_timestamp(self.platform.clock.now()),
        };
        let login_first = file_write.as_ref().is_some_and(|write| write.noop);
        Ok((
            plan,
            PlanContext {
                install,
                start_exe,
                config_path: config.path,
                config_exists: config.text.is_some(),
                current,
                in_place,
                already_active,
                running,
                login_first,
                close_warning,
            },
        ))
    }
}
