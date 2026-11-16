// The plan of a Riot switch: everything the real switch would read, and every change it would make, in the Electron app's own words. Nothing here changes anything, and nothing is decrypted: a sign-in is shown as the start of its fingerprint and the end of its account id.

use ol_formats::riot::client_log::last_sign_in;
use ol_formats::riot::products::{image_key, launch_args, parse_product_id};
use ol_formats::riot::session::short_fingerprint;
use ol_formats::winpath::join;
use ol_model::{
    LauncherId, LauncherInstall, PlanAccount, RunningGame, SwitchChange, SwitchPlan,
    SwitchPlanStep, SwitchStepKind, SwitchStrategy,
};

use super::{EXE, RiotAdapter, SESSION_FILE, WINDOW_EXE, id_tail};
use crate::steam::{SwitchPreflight, minutes, seconds, step};
use crate::{AccountRef, AdapterError, GameRef};

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

// What a close does, for the confirmation. A running game never gets this far: the switch is refused instead.
pub const CLOSE_MESSAGE: &str = "The Riot Client is running. Switching closes it.";

const BLOCK: &str = "psl.authorization.riot-client";

// What the plan learned, kept for the real switch so that it derives nothing twice.
pub struct PlanContext {
    pub install: LauncherInstall,
    // The fingerprint of the sign-in the file holds. The file itself is not kept: it is wiped when the plan is made.
    pub fingerprint: Option<String>,
    // The file already holds the target account's sign-in, so the client signs in as it at its next start.
    pub in_place: bool,
    // The client runs, signed in as the target account: the file names it and the log confirms a sign-in.
    pub already_active: bool,
    pub running: bool,
    // A Riot game runs, which refuses the switch.
    pub game: Option<RunningGame>,
    // The plan's warning that the running client will be closed. A preflight leaves it out, because the dialog says it in its own confirm box.
    pub close_warning: Option<String>,
}

fn tail(uid: Option<&str>) -> String {
    id_tail(uid, 4)
}

impl RiotAdapter<'_> {
    pub fn preflight(
        &self,
        account: &AccountRef,
        for_game: Option<GameRef<'_>>,
    ) -> Result<SwitchPreflight, AdapterError> {
        let (mut plan, context) = self.build_switch_plan(account, for_game)?;
        if let Some(game) = context.game {
            return Err(Self::game_running_error(game, "switch"));
        }
        if let Some(close_warning) = &context.close_warning {
            plan.warnings.retain(|warning| warning != close_warning);
        }
        Ok(SwitchPreflight {
            plan,
            launcher_running: context.running,
            running_game: None,
            already_active: context.already_active,
            close_message: CLOSE_MESSAGE.to_owned(),
        })
    }

    pub fn plan_switch(&self, account: &AccountRef) -> Result<SwitchPlan, AdapterError> {
        Ok(self.build_switch_plan(account, None)?.0)
    }

    // `for_game` is the game a launch follows the switch with: the client is then started with its launch arguments.
    pub fn build_switch_plan(
        &self,
        account: &AccountRef,
        for_game: Option<GameRef<'_>>,
    ) -> Result<(SwitchPlan, PlanContext), AdapterError> {
        let timing = self.timing();
        let install = self.require_install()?;
        let session_path = self.session_file_path();
        let log_dir = self.log_dir();
        let names = self.processes.names()?;
        let running = names.contains(&image_key(EXE));
        let game = self.find_running_game(Some(&names))?;
        let current = self.current_session()?;
        let stored = match &account.id {
            Some(id) => self.directory.session(id)?,
            None => None,
        };
        let session = current.session.as_ref();
        let mut warnings: Vec<String> = Vec::new();
        let mut notes: Vec<String> = Vec::new();

        // The file names the account the client signs in as, whether the client runs or not: it is written at every sign-in and sign-out. A running client also needs its log to say that it is signed in.
        let in_place = session.is_some()
            && current
                .owner
                .as_ref()
                .is_some_and(|owner| owner.id == account.id);
        let already_active = in_place
            && running
            && last_sign_in(&self.current_run_events()).is_some_and(|last| last.is_signed_in());
        let usable = stored
            .as_ref()
            .is_some_and(|stored| stored.stale_reason.is_none());
        let strategy = if in_place || usable {
            SwitchStrategy::Remembered
        } else {
            SwitchStrategy::Credentials
        };
        let remembered = strategy == SwitchStrategy::Remembered;
        let login = &account.login;
        let name = &account.display_name;
        let expected_uid = stored
            .as_ref()
            .and_then(|stored| stored.launcher_uid.clone())
            .or_else(|| account.launcher_uid.as_ref().map(|uid| uid.to_lowercase()));
        let fingerprint = session.map(|session| session.fingerprint.as_str());
        let session_uid = session.map(|session| session.account_id.as_str());
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
            "Close the Riot Client",
            EXE.to_owned(),
            !running || already_active,
            if already_active {
                "Skipped: the client is already signed in as this account, so it is left running.".to_owned()
            } else if running {
                format!(
                    "Send {EXE} a window close and wait up to {} s; a process that ignores the close is ended on its own, never with its children: a running game would refuse the switch before this step. Then the same for the window processes it leaves behind ({WINDOW_EXE}).",
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
                .map(|stored| stored.fingerprint.as_str())
                != fingerprint;
        // The file is read once the client has closed: it replaces the token at every start and writes the file when it exits. Saving writes only One Launch's own database.
        let capture_needed = outgoing.is_some()
            && !already_active
            && (running || (session.is_some() && (!in_place || outgoing_changed)));
        let nothing_changes = "Nothing in the client's files changes.";
        steps.push(step(
            STEP.capture,
            SwitchStepKind::Backup,
            if in_place {
                "Save this account's sign-in"
            } else {
                "Save the outgoing sign-in"
            },
            session_path.clone(),
            !capture_needed,
            match outgoing {
                _ if already_active => {
                    "Skipped: the client stays running, so its file is not re-read.".to_owned()
                }
                None if session.is_some() => format!(
                    "Skipped: the client holds a sign-in (Riot account {}) that no One Launch account owns, so it is left alone and not saved. To keep it, save it from that account's page before switching.",
                    tail(session_uid)
                ),
                None => "Skipped: the client holds no saved sign-in.".to_owned(),
                Some(owner) if !capture_needed => format!(
                    "Skipped: {}'s saved sign-in already matches what the client holds.",
                    owner.display_name
                ),
                Some(owner) if running => format!(
                    "After the client has closed, read {SESSION_FILE} and, if it holds {}'s sign-in and it changed, store the file, encrypted, as the account's saved sign-in. {nothing_changes}",
                    owner.display_name
                ),
                Some(owner) if in_place => format!(
                    "{SESSION_FILE} holds {}'s sign-in but it differs from the saved copy ({}), so store the file, encrypted, as the account's saved sign-in. {nothing_changes}",
                    owner.display_name,
                    if outgoing_stored.is_some() {
                        "the client rotated the token"
                    } else {
                        "no copy yet"
                    }
                ),
                Some(owner) => format!(
                    "{SESSION_FILE} holds {}'s sign-in{}; store the file, encrypted, as the account's saved sign-in if it changed. {nothing_changes}",
                    owner.display_name,
                    if outgoing_changed {
                        " and it changed since it was last saved (the client rotates the token at every start)"
                    } else {
                        ""
                    }
                ),
            },
        ));

        steps.push(step(
            STEP.backup,
            SwitchStepKind::Backup,
            &format!("Back up {SESSION_FILE}"),
            join(&[&self.config.backups_dir, "riot", "<timestamp>"]),
            already_active || in_place,
            if already_active || in_place {
                "Skipped: the file will not be modified.".to_owned()
            } else {
                format!(
                    "Copy {session_path}, encrypted, into a new snapshot folder so a failed switch can be rolled back. The newest {} snapshots are kept.",
                    self.config.backups_to_keep
                )
            },
        ));

        let before = || match (&current.file, session) {
            (None, _) => "(absent)".to_owned(),
            (Some(_), Some(session)) => format!(
                "(saved sign-in for Riot account {}, fingerprint {})",
                tail(Some(&session.account_id)),
                short_fingerprint(Some(&session.fingerprint))
            ),
            (Some(_), None) => "null (signed out)".to_owned(),
        };
        let change = |after: String| SwitchChange {
            place: BLOCK.to_owned(),
            before: before(),
            after,
        };
        if in_place {
            steps.push(step(
                STEP.file,
                SwitchStepKind::File,
                "Put the account's sign-in in place",
                session_path.clone(),
                true,
                format!(
                    "No change: {SESSION_FILE} already holds {name}'s sign-in (fingerprint {}).",
                    short_fingerprint(fingerprint)
                ),
            ));
        } else if let (true, Some(stored)) = (remembered, &stored) {
            let mut file = step(
                STEP.file,
                SwitchStepKind::File,
                "Put the account's sign-in in place",
                session_path.clone(),
                false,
                format!(
                    "Decrypt {name}'s saved copy of {SESSION_FILE} from the vault and write it over the file, whole, as the client itself left it (its sign-in, token write count and device cookie together): the swap confirmed by hand on the real client. The new content is written to a temp file and renamed over the original. The copy is never shown or logged."
                ),
            );
            file.changes = vec![change(format!(
                "(saved copy for {name}, fingerprint {}, saved {})",
                short_fingerprint(Some(&stored.fingerprint)),
                stored.captured_at
            ))];
            steps.push(file);
        } else {
            let signed_out = session.is_none();
            let mut file = step(
                STEP.file,
                SwitchStepKind::File,
                "Sign the client out",
                session_path.clone(),
                signed_out,
                if current.file.is_none() {
                    format!(
                        "No change: there is no {SESSION_FILE}, so the client opens on its login page."
                    )
                } else if signed_out {
                    "No change: the client holds no saved sign-in, so it opens on its login page."
                        .to_owned()
                } else {
                    format!(
                        "Set {BLOCK} to null, the state the client itself writes at a sign-out, and leave every other line (the device cookie included) as it is, so the client opens on its login page instead of signing in as {}.",
                        match outgoing {
                            Some(owner) => owner.display_name.clone(),
                            None => format!("Riot account {}", tail(session_uid)),
                        }
                    )
                },
            );
            if !signed_out {
                file.changes = vec![change("null (signed out)".to_owned())];
            }
            steps.push(file);
        }

        let launch_for =
            for_game.and_then(|game| Some((parse_product_id(game.launcher_game_id)?, game)));
        let mut command = vec![format!("\"{}\"", install.exe_path)];
        if let Some((id, _)) = &launch_for {
            command.extend(launch_args(id));
        }
        steps.push(step(
            STEP.start,
            SwitchStepKind::Launch,
            "Start the Riot Client",
            command.join(" "),
            already_active,
            match &launch_for {
                _ if already_active => {
                    "Skipped: the client is already running as this account.".to_owned()
                }
                Some((_, game)) if remembered => format!(
                    "Start the client with {}'s launch arguments, as the game's own shortcut does. It signs in as {login} on its own from the saved sign-in and opens the game's page.",
                    game.name
                ),
                Some((_, game)) => format!(
                    "Start the client with {}'s launch arguments, as the game's own shortcut does. With no saved sign-in it opens on its login page, and the game's page follows the sign-in.",
                    game.name
                ),
                None if remembered => format!(
                    "Start the client with no arguments, as its own shortcut does. It signs in as {login} on its own from the saved sign-in."
                ),
                None => "Start the client with no arguments. With no saved sign-in it opens on its login page.".to_owned(),
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
                    "Read what the client appends to its new run log (no tokens in it) until it reports a sign-in verdict, up to {sign_in_seconds} s: the signed-in state (PendingProductContext), after which {SESSION_FILE} names the Riot account id, which must be {}; or the login page (PendingLoginStrategy), which hands over to you in the next step. Only an invalid_grant refusal marks the saved copy stale.",
                    match expected_uid.as_deref().filter(|uid| !uid.is_empty()) {
                        Some(uid) => tail(Some(uid)),
                        None => "an id no other account holds, which is then recorded on the account".to_owned(),
                    }
                )
            } else {
                "Read what the client appends to its new run log until it reports a sign-in verdict. With no saved sign-in it opens on its login page, so this goes straight to the next step.".to_owned()
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
                    "Runs only if the client refuses the saved sign-in or does not sign in within {sign_in_seconds} s. The client stays on its login page; you sign in there once as {login} with “Stay signed in” ticked{can_copy} and One Launch continues as soon as the log reports the sign-in, up to {manual_minutes} min."
                )
            } else {
                format!(
                    "The client opens on its login page. Sign in there as {login} with “Stay signed in” ticked{can_copy}. One Launch continues as soon as the log reports the sign-in, up to {manual_minutes} min. Signing in as a different Riot account than the one linked to {name} ends the switch and puts the file back."
                )
            },
        ));
        steps.push(step(
            STEP.recapture,
            SwitchStepKind::Backup,
            "Save the new sign-in",
            session_path.clone(),
            remembered,
            if remembered {
                "Runs only after a manual sign-in: the fresh sign-in replaces the stale saved copy."
                    .to_owned()
            } else {
                format!(
                    "Once the sign-in is reported, wait up to {} s for the client to write {SESSION_FILE}, then store the file, encrypted, as {name}'s saved sign-in and record the Riot account id on the account. If the client writes it later, it is saved at the next switch instead. Later switches use the saved sign-in.",
                    seconds(timing.capture)
                )
            },
        ));

        if let Some(game) = &game {
            warnings.push(format!(
                "{} is running. Close it first: One Launch never closes a Riot game, and no switch runs while one does.",
                game.name
            ));
        }
        if current.file.is_none() {
            warnings.push(format!(
                "{session_path} does not exist. One Launch never creates it: start the Riot Client once by hand (it writes the file at its first start), then try again."
            ));
        }
        let mut close_warning = None;
        if already_active {
            notes.push(format!(
                "The Riot Client is running and signed in as {name}. The real switch would change nothing and would leave the client running."
            ));
        } else if running {
            let warning = format!(
                "The Riot Client is running{}. It will be closed.",
                match (outgoing, session) {
                    (Some(owner), _) => format!(" as {}", owner.display_name),
                    (None, Some(_)) => " with a sign-in no One Launch account owns".to_owned(),
                    (None, None) => " on its login page".to_owned(),
                }
            );
            warnings.push(warning.clone());
            close_warning = Some(warning);
        } else if in_place {
            notes.push(format!(
                "The Riot Client already holds {name}'s sign-in, so it is only started."
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
                "{name}'s saved sign-in no longer works: {reason}. You will sign in once by hand, with “Stay signed in” ticked, and One Launch stores the fresh sign-in."
            ));
        }
        if !remembered && stored.is_none() && !in_place {
            warnings.push(format!(
                "No sign-in is saved for {name} yet. The client will open on its login page; sign in once with “Stay signed in” ticked and One Launch saves it for later switches.{}",
                if account.has_secret {
                    ""
                } else {
                    " No password is stored in One Launch, so have it ready."
                }
            ));
        }
        if !in_place && session.is_some() && outgoing.is_none() {
            warnings.push(format!(
                "The Riot Client holds a sign-in that no One Launch account owns (Riot account {}). It will be replaced. The backup taken before this switch keeps a copy and is left out of the usual clean-up, so restoring it from Settings › Backups brings that sign-in back.",
                tail(session_uid)
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
                "The Riot account id on this account ({}) differs from the one recorded with its saved sign-in ({}). The saved sign-in's id is used to confirm the sign-in.",
                tail(Some(&own)),
                tail(Some(&saved))
            ));
        }

        let plan = SwitchPlan {
            launcher: LauncherId::Riot,
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
            executable: current.file.is_some(),
            generated_at: ol_store::iso_timestamp(self.clock.now()),
        };
        Ok((
            plan,
            PlanContext {
                install,
                fingerprint: fingerprint.map(str::to_owned),
                in_place,
                already_active,
                running,
                game,
                close_warning,
            },
        ))
    }
}
