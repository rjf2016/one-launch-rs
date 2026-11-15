// The plan of a Steam switch: everything the real switch would read, and every change it would make, in the Electron app's own words. Nothing here changes anything.

use ol_formats::js;
use ol_formats::steam::loginusers::{
    SteamLoginUser, find_login_user, parse_login_users, plan_auto_login_rewrite, steam_id3,
};
use ol_formats::winpath::join;
use ol_model::{
    LauncherId, LauncherInstall, PlanAccount, RunningGame, SwitchChange, SwitchPlan,
    SwitchPlanStep, SwitchStepKind, SwitchStrategy,
};

use super::{
    AccountRef, EXE, HKCU_KEY, STALE_SESSION_DAYS, SteamAdapter, minutes, plural, seconds,
    session_age_days,
};
use crate::AdapterError;
use crate::files::{MAX_VDF_BYTES, read_launcher_text};

pub struct StepIds {
    pub close: &'static str,
    pub backup: &'static str,
    pub file: &'static str,
    pub registry_user: &'static str,
    pub registry_remember: &'static str,
    pub start: &'static str,
    pub wait: &'static str,
    pub verify: &'static str,
    pub login: &'static str,
    pub manual: &'static str,
}

// The real switch reports its progress against these.
pub const STEP: StepIds = StepIds {
    close: "close",
    backup: "backup",
    file: "file",
    registry_user: "registry-user",
    registry_remember: "registry-remember",
    start: "start",
    wait: "wait",
    verify: "verify",
    login: "login",
    manual: "manual",
};

// What the plan learned, kept for the real switch so that it derives nothing twice.
pub struct PlanContext {
    pub install: LauncherInstall,
    pub login_users_path: String,
    pub target: Option<SteamLoginUser>,
    pub already_active: bool,
    pub stale: bool,
    pub auto_login_user: Option<String>,
    pub remember_dword: Option<u32>,
    // The plan's warning that the running Steam will be closed. A preflight leaves it out, because the dialog says it in its own confirm box.
    pub close_warning: Option<String>,
}

// What a real switch would run into right now.
pub struct SwitchPreflight {
    pub plan: SwitchPlan,
    pub launcher_running: bool,
    pub running_game: Option<RunningGame>,
    pub already_active: bool,
    pub close_message: String,
}

fn step(
    id: &'static str,
    kind: SwitchStepKind,
    title: &str,
    target: String,
    skipped: bool,
    detail: String,
) -> SwitchPlanStep {
    SwitchPlanStep {
        id,
        kind,
        title: title.to_owned(),
        target,
        detail,
        changes: Vec::new(),
        skipped,
    }
}

// What closing Steam does, for the confirmation: Steam takes any game running under it down with it.
pub fn close_message(game: Option<&RunningGame>) -> String {
    match game {
        Some(game) => format!(
            "Steam is running {}. Switching closes Steam and the game; unsaved progress in the game is lost.",
            game.name
        ),
        None => "Steam is running. Switching closes it.".to_owned(),
    }
}

impl SteamAdapter<'_> {
    pub fn preflight(&self, account: &AccountRef) -> Result<SwitchPreflight, AdapterError> {
        let (mut plan, context) = self.build_switch_plan(account)?;
        let running_game = if plan.running && !context.already_active {
            self.running_game()?
        } else {
            None
        };
        if let Some(close_warning) = &context.close_warning {
            plan.warnings.retain(|warning| warning != close_warning);
        }
        Ok(SwitchPreflight {
            launcher_running: plan.running,
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
        let files = self.platform.files.as_ref();
        let install = self.require_install()?;
        let login_users_path = Self::login_users_path(&install);
        let reg_key = format!("HKCU\\{HKCU_KEY}");
        let running = self.is_running()?;
        let auto_login_user = self.auto_login_user()?;
        let remember_dword = self.remember_password()?;
        let now = self.platform.clock.now();

        let mut warnings: Vec<String> = Vec::new();
        let mut notes: Vec<String> = Vec::new();
        let mut text: Option<String> = None;
        let mut users: Vec<SteamLoginUser> = Vec::new();
        if files.exists(&login_users_path) {
            let read = read_launcher_text(files, &login_users_path, MAX_VDF_BYTES)?;
            match parse_login_users(&read) {
                Ok(parsed) => {
                    users = parsed;
                    text = Some(read);
                }
                Err(error) => warnings.push(format!(
                    "loginusers.vdf could not be parsed ({error}). Steam is treated as remembering no accounts."
                )),
            }
        } else {
            warnings.push(
                "loginusers.vdf does not exist yet. Steam has not remembered any account on this PC."
                    .to_owned(),
            );
        }

        let target =
            find_login_user(&users, account.launcher_uid.as_deref(), &account.login).cloned();
        let most_recent_user = users.iter().find(|user| user.most_recent);
        let strategy = if target.as_ref().is_some_and(|user| user.remember_password) {
            SwitchStrategy::Remembered
        } else {
            SwitchStrategy::Credentials
        };
        let with_password = self.config.auto_login && account.has_secret;

        let rewrite = match (strategy, &target, &text) {
            (SwitchStrategy::Remembered, Some(target), Some(text)) => Some(
                plan_auto_login_rewrite(text, &target.steam_id64)
                    .map_err(|error| AdapterError::Failed(error.to_string()))?,
            ),
            _ => None,
        };
        let same_user = match (&target, &auto_login_user) {
            (Some(target), Some(current)) => {
                current.to_lowercase() == target.account_name.to_lowercase()
            }
            _ => false,
        };
        let expected_id3 = target.as_ref().and_then(|user| steam_id3(&user.steam_id64));
        let points_at_target = rewrite.as_ref().is_some_and(|rewrite| rewrite.noop) && same_user;
        // The files name the target, but the client that runs signed in as someone else, or as nobody yet.
        let signed_in_elsewhere = match expected_id3 {
            Some(id3) if points_at_target && running => !self.signed_in_as(&install, id3)?,
            _ => false,
        };
        // Steam is already set up for this account: nothing to change, so no reason to restart it.
        let already_active = points_at_target && !signed_in_elsewhere;
        let age_days = target
            .as_ref()
            .and_then(|user| session_age_days(user.timestamp, now));
        let stale = strategy == SwitchStrategy::Remembered
            && age_days.is_none_or(|days| days >= STALE_SESSION_DAYS);
        let active_user_key = format!("{reg_key}\\ActiveProcess\\ActiveUser");
        let login_log = Self::login_log_path(&install);
        let exe_path = &install.exe_path;
        let login = &account.login;

        let mut steps: Vec<SwitchPlanStep> = Vec::new();
        steps.push(step(
            STEP.close,
            SwitchStepKind::Process,
            "Close Steam",
            EXE.to_owned(),
            !running || already_active,
            if already_active {
                "Skipped: Steam is already signed in as this account, so it is left running.".to_owned()
            } else if running {
                format!(
                    "Run \"{exe_path}\" -shutdown and wait up to {} s for steam.exe to exit. If it is still running after that, force-close steam.exe and every process it started. Any game running under Steam closes with it.",
                    seconds(timing.shutdown)
                )
            } else {
                "Skipped: steam.exe is not running.".to_owned()
            },
        ));
        steps.push(step(
            STEP.backup,
            SwitchStepKind::Backup,
            "Back up Steam files",
            join(&[&self.config.backups_dir, "steam", "<timestamp>"]),
            already_active,
            if already_active {
                "Skipped: nothing will be modified.".to_owned()
            } else {
                format!(
                    "Copy {login_users_path} into a new snapshot folder and record the current {reg_key}\\AutoLoginUser and RememberPassword values in its manifest.json, so a failed switch can be rolled back. The newest {} snapshots are kept.",
                    self.config.backups_to_keep
                )
            },
        ));

        if let (Some(rewrite), Some(target)) = (&rewrite, &target) {
            let name = &target.account_name;
            let others = users.len() - 1;
            let keys = if target.most_recent_keys.is_empty() {
                "AutoLogin (added, because the block has neither AutoLogin nor MostRecent)"
                    .to_owned()
            } else {
                target
                    .most_recent_keys
                    .iter()
                    .map(|key| key.as_str())
                    .collect::<Vec<_>>()
                    .join(" and ")
            };
            let mut file = step(
                STEP.file,
                SwitchStepKind::File,
                "Mark the account as most recent",
                login_users_path.clone(),
                rewrite.noop,
                if rewrite.noop {
                    format!(
                        "No change: {name} is already the most recent account in loginusers.vdf."
                    )
                } else {
                    format!(
                        "Set {keys} to 1 for {name} ({}) and to 0 for the other {}. The new content is written to a temp file and renamed over the original. Nothing else in the file changes.",
                        target.steam_id64,
                        plural(others, "remembered account")
                    )
                },
            );
            file.changes = rewrite
                .changes
                .iter()
                .map(|change| SwitchChange {
                    place: format!("line {}", change.line),
                    before: js::trim(&change.before).to_owned(),
                    after: js::trim(&change.after).to_owned(),
                })
                .collect();
            steps.push(file);

            let mut registry_user = step(
                STEP.registry_user,
                SwitchStepKind::Registry,
                "Point Steam at the account",
                format!("{reg_key}\\AutoLoginUser"),
                same_user,
                if same_user {
                    format!("No change: AutoLoginUser is already {name}.")
                } else {
                    "REG_SZ. Steam reads this at startup to decide which remembered account to sign in as."
                        .to_owned()
                },
            );
            if !same_user {
                registry_user.changes.push(SwitchChange {
                    place: "AutoLoginUser".to_owned(),
                    before: auto_login_user
                        .clone()
                        .unwrap_or_else(|| "(absent)".to_owned()),
                    after: name.clone(),
                });
            }
            steps.push(registry_user);

            let remembered = remember_dword == Some(1);
            let mut registry_remember = step(
                STEP.registry_remember,
                SwitchStepKind::Registry,
                "Keep the session remembered",
                format!("{reg_key}\\RememberPassword"),
                remembered || already_active,
                if remembered {
                    "No change: RememberPassword is already 1.".to_owned()
                } else if already_active {
                    "Skipped: no switch is needed, so the value is left as it is.".to_owned()
                } else {
                    "REG_DWORD set to 1. On this Steam build the value is normally absent and the per-account RememberPassword in loginusers.vdf does the work; writing it is harmless and matches the documented procedure.".to_owned()
                },
            );
            if !(remembered || already_active) {
                registry_remember.changes.push(SwitchChange {
                    place: "RememberPassword".to_owned(),
                    before: remember_dword
                        .map_or_else(|| "(absent)".to_owned(), |value| value.to_string()),
                    after: "1".to_owned(),
                });
            }
            steps.push(registry_remember);

            steps.push(step(
                STEP.start,
                SwitchStepKind::Launch,
                "Start Steam",
                format!("\"{exe_path}\""),
                already_active && running,
                if already_active && running {
                    "Skipped: Steam is already running as this account.".to_owned()
                } else {
                    format!(
                        "Start Steam normally. It signs in as {name} without prompting because the session is remembered."
                    )
                },
            ));
        } else {
            let why = match &target {
                Some(target) => format!(
                    "Steam remembers {} but \"Remember me\" is off for it",
                    target.account_name
                ),
                None => format!("Steam has never signed in as {login} on this PC"),
            };
            steps.push(step(
                STEP.login,
                SwitchStepKind::Launch,
                if with_password {
                    "Start Steam with credentials"
                } else {
                    "Start Steam on its sign-in form"
                },
                if with_password {
                    format!("\"{exe_path}\" -login {login} <password>")
                } else {
                    format!("\"{exe_path}\" -login {login}")
                },
                false,
                if with_password {
                    format!(
                        "{why}, so Steam is started with -login. The password is decrypted from the vault at that moment and passed only as a process argument; it is never written anywhere by One Launch, but it is visible on the command line to other processes on this PC while Steam runs. Afterwards Steam adds the account to loginusers.vdf and later switches use the remembered path."
                    )
                } else {
                    format!(
                        "{why}, so Steam is started with -login and only the account name, which makes it open its sign-in form instead of signing in as the previous account. You enter the password there; nothing secret goes on the command line. Afterwards Steam adds the account to loginusers.vdf and later switches use the remembered path."
                    )
                },
            ));
            let wait = minutes(timing.manual_sign_in);
            if with_password {
                warnings.push(format!(
                    "If this PC has never signed in as {login}, Steam Guard will ask for an emailed code once. One Launch then waits up to {wait} min while you sign in in the Steam window, and continues on its own."
                ));
            } else {
                warnings.push(format!(
                    "Steam will open its sign-in form for {login}. One Launch waits up to {wait} min while you sign in there with \"Remember me\" ticked{}; later switches are automatic.",
                    if account.has_secret {
                        " and offers to copy the login and password"
                    } else {
                        " and offers to copy the login"
                    }
                ));
            }
        }

        let skipped_running = already_active && running;
        steps.push(step(
            STEP.wait,
            SwitchStepKind::Wait,
            "Wait for Steam to come up",
            EXE.to_owned(),
            skipped_running,
            format!(
                "Poll the process list until steam.exe is running, up to {} s.",
                seconds(timing.startup)
            ),
        ));

        let verify_detail = if skipped_running {
            "Skipped: Steam is already signed in as this account.".to_owned()
        } else {
            let poll = match expected_id3 {
                Some(id3) => format!(
                    "Poll until Steam reports the signed-in account id {id3} (the SteamID3 of {}), up to {} s.",
                    target.as_ref().map_or(login, |user| &user.account_name),
                    seconds(if strategy == SwitchStrategy::Credentials {
                        timing.guard
                    } else {
                        timing.sign_in
                    })
                ),
                None => format!(
                    "Poll until Steam reports a signed-in account, up to {} s.",
                    seconds(timing.guard)
                ),
            };
            format!(
                "{poll} Meanwhile {login_log} is read (Steam's own sign-in state log, no credentials in it) to learn at once whether Steam is instead asking for a password or a Steam Guard code, and {} (its updater log) to see whether Steam is updating itself first; then the wait starts over when the update ends, up to {} min in all. A game launch (steam://rungameid/<appid>) happens only after the sign-in is confirmed.",
                Self::bootstrap_log_path(&install),
                minutes(timing.update)
            )
        };
        steps.push(step(
            STEP.verify,
            SwitchStepKind::Wait,
            "Confirm Steam signed in",
            active_user_key.clone(),
            skipped_running,
            verify_detail,
        ));

        if strategy == SwitchStrategy::Remembered {
            steps.push(step(
                STEP.login,
                SwitchStepKind::Launch,
                "Fallback: sign in with the stored password",
                format!("\"{exe_path}\" -login {login} <password>"),
                true,
                if with_password {
                    "Runs only if Steam asks for credentials instead of signing in (an expired session): close Steam, start it again with -login using the password from the vault, and confirm the sign-in again."
                } else if account.has_secret {
                    "Not used: Steam automatic sign-in is off in Settings. If Steam asks for credentials, One Launch waits for you to sign in by hand (next step) and offers to copy the login and password."
                } else {
                    "Not available: no password is stored for this account. If Steam asks for credentials, One Launch waits for you to sign in by hand (next step)."
                }
                .to_owned(),
            ));
        }

        steps.push(step(
            STEP.manual,
            SwitchStepKind::Wait,
            "Wait for you to sign in",
            active_user_key,
            true,
            format!(
                "Runs only if Steam still asks for something One Launch cannot supply: a Steam Guard code for a PC this account has not used before, an expired session with no stored password, or a session Steam simply refuses. The Steam window stays open on its sign-in form; you sign in there once with \"Remember me\" ticked{} and One Launch continues with the game as soon as Steam reports the sign-in, up to {} min. A wrong stored password ends the switch instead.",
                if account.has_secret {
                    " (the password can be copied from One Launch)"
                } else {
                    ""
                },
                minutes(timing.manual_sign_in)
            ),
        ));

        let mut close_warning = None;
        if let (true, Some(target)) = (already_active, &target) {
            notes.push(format!(
                "Steam is already set to sign in as {}. The real switch would change nothing{}.",
                target.account_name,
                if running {
                    " and would leave Steam running"
                } else {
                    ""
                }
            ));
        } else if let (true, Some(target)) = (signed_in_elsewhere, &target) {
            let active = self.active_user()?.map(i128::from);
            let other = users.iter().find(|user| {
                steam_id3(&user.steam_id64) == active && user.steam_id64 != target.steam_id64
            });
            let name = &target.account_name;
            let warning = format!(
                "Steam's files already name {name}, but the Steam that is running is {}. It will be closed, along with any game running under it, and started again as {name}.",
                match other {
                    Some(other) => format!("signed in as {}", other.account_name),
                    None => format!("not signed in as {name}"),
                }
            );
            warnings.push(warning.clone());
            close_warning = Some(warning);
        } else if running {
            let warning =
                "Steam is running. It will be closed, along with any game running under it."
                    .to_owned();
            warnings.push(warning.clone());
            close_warning = Some(warning);
        }
        if let (true, Some(target)) = (stale, &target) {
            let age = match age_days {
                Some(days) => format!("{days} days"),
                None => "an unknown time".to_owned(),
            };
            warnings.push(format!(
                "{} last signed in here {age} ago. Steam sessions expire, so Steam may ask for credentials instead of signing in. {}",
                target.account_name,
                if with_password {
                    "One Launch will then sign in with the stored password."
                } else {
                    "One Launch will then wait while you sign in in the Steam window, and continues on its own."
                }
            ));
        }
        if let (Some(target), Some(uid)) = (
            &target,
            account
                .launcher_uid
                .as_deref()
                .filter(|uid| !uid.is_empty()),
        ) && target.steam_id64 != uid
        {
            warnings.push(format!(
                "The SteamID64 stored on this account ({uid}) does not match Steam's entry for {} ({}). Matched by login instead; check the ID on the account page.",
                target.account_name, target.steam_id64
            ));
        }
        if let (Some(current), Some(most_recent)) = (
            auto_login_user.as_deref().filter(|user| !user.is_empty()),
            most_recent_user,
        ) && current.to_lowercase() != most_recent.account_name.to_lowercase()
        {
            warnings.push(format!(
                "Registry AutoLoginUser ({current}) and the most recent account in loginusers.vdf ({}) disagree. The switch sets both, which resolves it.",
                most_recent.account_name
            ));
        }

        let plan = SwitchPlan {
            launcher: LauncherId::Steam,
            account: PlanAccount {
                id: account.id.clone(),
                display_name: account.display_name.clone(),
                login: account.login.clone(),
                launcher_uid: target
                    .as_ref()
                    .map(|user| user.steam_id64.clone())
                    .or_else(|| account.launcher_uid.clone()),
            },
            strategy,
            most_recent_login: most_recent_user.map(|user| user.account_name.clone()),
            auto_login_user: auto_login_user.clone(),
            running,
            steps,
            warnings,
            notes,
            executable: true,
            generated_at: ol_store::iso_timestamp(now),
        };
        Ok((
            plan,
            PlanContext {
                install,
                login_users_path,
                target,
                already_active,
                stale,
                auto_login_user,
                remember_dword,
                close_warning,
            },
        ))
    }
}
