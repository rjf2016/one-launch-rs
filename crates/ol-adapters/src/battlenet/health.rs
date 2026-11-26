// Battle.net's health check and Locate. Reads only; no token is ever named, only how long it is and the start of its fingerprint.

use ol_formats::battlenet::client_log::{LogEvent, SignInOutcome, last_sign_in};
use ol_formats::battlenet::products::parse_aggregate;
use ol_formats::battlenet::session::{TokenValue, fingerprint_of};
use ol_formats::epic::session::short_fingerprint;
use ol_formats::winpath::{join, normalize_launcher_path};
use ol_model::{HealthCheck, HealthStatus, InstallSource, LauncherInstall};

use super::{BattlenetAdapter, EXE, LAUNCHER_EXE, UNIFIED_AUTH_PATH, id_tail};
use crate::files::{MAX_LAUNCHER_FILE_BYTES, read_launcher_text};
use crate::health::{Checks, Known, chosen_folder_check, is_named, source_label};
use crate::steam::plural;
use crate::{AccountRef, AdapterError};

fn status_if(warn: bool) -> HealthStatus {
    if warn {
        HealthStatus::Warn
    } else {
        HealthStatus::Ok
    }
}

fn describe_token(token: &TokenValue) -> String {
    let fingerprint = fingerprint_of(&token.hex);
    format!(
        "{} bytes, fingerprint {}",
        token.hex.len() / 2,
        short_fingerprint(Some(&fingerprint))
    )
}

impl BattlenetAdapter<'_> {
    // Either program will do: the shortcuts point at Battle.net Launcher.exe, which sits beside the client.
    pub fn install_from_exe(&self, exe: &str) -> Option<LauncherInstall> {
        let exe = normalize_launcher_path(exe);
        if !is_named(&exe, &[EXE, LAUNCHER_EXE]) || !self.platform.files.exists(&exe) {
            return None;
        }
        self.install_at(join(&[&exe, ".."]), InstallSource::User)
    }

    pub fn aggregate_path(&self) -> String {
        join(&[
            &self.config.program_data,
            "Battle.net",
            "Agent",
            "aggregate.json",
        ])
    }

    // Every check but the saved sign-ins and the snapshots, which are One Launch's own and are added by the caller.
    pub fn health_checks(&self, known: &Known) -> Vec<HealthCheck> {
        let mut checks = Checks::default();
        let chosen = self.chosen_install_dir();
        let chosen_gone = chosen_folder_check(chosen.as_deref(), known.install.as_ref(), EXE);
        let Some(install) = &known.install else {
            checks.add(
                "install",
                "Installation",
                HealthStatus::Absent,
                format!(
                    "Battle.net is not installed on this PC. Checked the Uninstall key, the battlenet:// URI handler, and {}.",
                    self.config.default_dir
                ),
            );
            checks.push(chosen_gone);
            return checks.0;
        };
        let starts = if self.start_exe(install) == install.exe_path {
            EXE
        } else {
            LAUNCHER_EXE
        };
        checks.add(
            "install",
            "Installation",
            HealthStatus::Ok,
            format!(
                "{} (from {}); starts {starts}",
                install.install_dir,
                source_label(install.source)
            ),
        );
        checks.push(chosen_gone);
        let running = checks
            .process(EXE, known.running.map_or_else(|| self.is_running(), Ok))
            .unwrap_or(false);

        self.product_checks(&mut checks);
        self.config_check(&mut checks);
        let log = self.read_current_run();
        let events = log.as_ref().ok().and_then(Option::as_deref);
        self.token_checks(&mut checks, running, events.unwrap_or_default());
        self.log_check(&mut checks, &log);
        checks.0
    }

    fn product_checks(&self, checks: &mut Checks) {
        let files = self.platform.files.as_ref();
        let games = match self.uninstall_entries() {
            Ok(entries) => {
                let mut games: Vec<(String, String)> = entries
                    .iter()
                    .filter(|entry| {
                        entry
                            .install_path
                            .as_ref()
                            .is_some_and(|path| files.exists(path))
                    })
                    .map(|entry| (entry.codename.clone(), entry.name.clone()))
                    .collect();
                games.sort_by_cached_key(|(_, name)| name.to_lowercase());
                let missing = entries.len().saturating_sub(games.len());
                let detail = if entries.is_empty() {
                    "No Blizzard products in the Uninstall key. No Battle.net games are installed, or the Agent has never installed one.".to_owned()
                } else {
                    let names: Vec<String> = entries
                        .iter()
                        .map(|entry| format!("{} ({})", entry.name, entry.codename))
                        .collect();
                    let tail = if missing > 0 {
                        format!("; {missing} without an install folder")
                    } else {
                        String::new()
                    };
                    format!(
                        "{} in the Uninstall key: {}{tail}",
                        plural(entries.len(), "product"),
                        names.join(", ")
                    )
                };
                checks.add(
                    "uninstall",
                    "Installed products",
                    status_if(entries.is_empty()),
                    detail,
                );
                games
            }
            Err(error) => {
                checks.add(
                    "uninstall",
                    "Installed products",
                    HealthStatus::Fail,
                    format!("Could not read the Uninstall key: {error}"),
                );
                Vec::new()
            }
        };

        let db_path = self.product_db_path();
        if files.exists(&db_path) {
            let read = self.read_product_db();
            let db = read.as_deref().unwrap_or_default();
            let codes: Vec<String> = games
                .iter()
                .map(|(codename, _)| match Self::codes_for(codename, db) {
                    Some(codes) => {
                        format!("{codename} → {} / {}", codes.program_code, codes.tact_code)
                    }
                    None => format!("{codename} → unknown"),
                })
                .collect();
            let unknown: Vec<&str> = games
                .iter()
                .filter(|(codename, _)| Self::codes_for(codename, db).is_none())
                .map(|(_, name)| name.as_str())
                .collect();
            let unverified: Vec<&str> = games
                .iter()
                .filter(|(codename, _)| {
                    Self::codes_for(codename, db).is_some_and(|codes| !codes.verified)
                })
                .map(|(_, name)| name.as_str())
                .collect();
            let codes = if codes.is_empty() {
                "none needed".to_owned()
            } else {
                codes.join(", ")
            };
            let unknown_text = if unknown.is_empty() {
                String::new()
            } else {
                format!("; no code known for {}", unknown.join(", "))
            };
            let unverified_text = if unverified.is_empty() {
                String::new()
            } else {
                format!(
                    " ({}: table value, not verified on this PC)",
                    unverified.join(", ")
                )
            };
            let detail = match &read {
                Ok(db) => format!(
                    "{}; launch codes: {codes}{unknown_text}{unverified_text}",
                    plural(db.len(), "install record")
                ),
                Err(error) => format!(
                    "Could not read {db_path}: {error}. Launch codes come from the built-in table only: {codes}{unknown_text}{unverified_text}"
                ),
            };
            checks.add(
                "product-db",
                "product.db",
                status_if(read.is_err() || !unknown.is_empty()),
                detail,
            );
        } else {
            checks.add(
                "product-db",
                "product.db",
                HealthStatus::Warn,
                format!("{db_path} not found; launch codes come from the built-in table only."),
            );
        }

        let aggregate = self.aggregate_path();
        if files.exists(&aggregate) {
            let parsed = read_launcher_text(files, &aggregate, MAX_LAUNCHER_FILE_BYTES)
                .map_err(|error| error.to_string())
                .and_then(|text| parse_aggregate(&text).map_err(|error| error.to_string()));
            match parsed {
                Ok(list) => {
                    let entries: Vec<String> = list
                        .iter()
                        .map(|entry| {
                            format!(
                                "{} → {}",
                                entry.name,
                                entry.launch_uri.as_deref().unwrap_or(&entry.tact_code)
                            )
                        })
                        .collect();
                    let entries = if entries.is_empty() {
                        "none".to_owned()
                    } else {
                        entries.join(", ")
                    };
                    checks.add(
                        "aggregate",
                        "aggregate.json",
                        HealthStatus::Ok,
                        format!(
                            "{} {}: {entries}",
                            list.len(),
                            if list.len() == 1 { "entry" } else { "entries" }
                        ),
                    );
                }
                Err(reason) => checks.add(
                    "aggregate",
                    "aggregate.json",
                    HealthStatus::Warn,
                    format!("Could not parse {aggregate}: {reason}"),
                ),
            }
        }
    }

    fn config_check(&self, checks: &mut Checks) {
        let config = match self.read_config() {
            Ok(config) => config,
            Err(error) => {
                checks.add(
                    "config",
                    "Battle.net.config",
                    HealthStatus::Fail,
                    error.to_string(),
                );
                return;
            }
        };
        let Some(_) = config.text else {
            checks.add(
                "config",
                "Battle.net.config",
                HealthStatus::Warn,
                format!(
                    "{} not found. The client has not run for this Windows user yet.",
                    config.path
                ),
            );
            return;
        };
        let client = match (&config.error, &config.client) {
            (None, Some(client)) => client,
            (error, _) => {
                checks.add(
                    "config",
                    "Battle.net.config",
                    HealthStatus::Fail,
                    format!(
                        "Could not parse {}: {}",
                        config.path,
                        error.as_deref().unwrap_or("no Client section")
                    ),
                );
                return;
            }
        };
        let auto_login = client.auto_login.as_deref();
        let auto_login_on = auto_login.is_none_or(|auto_login| auto_login.to_lowercase() == "true");
        let round_trips = config.round_trips == Some(true);
        let auto_login_text = if auto_login_on {
            ""
        } else {
            " (the client will not sign in on its own; turn \"Keep me logged in\" on in the client)"
        };
        let layout = if round_trips {
            "layout reproducible"
        } else {
            "layout not reproducible, so One Launch will not rewrite it"
        };
        checks.add(
            "config",
            "Battle.net.config",
            status_if(!(auto_login_on && round_trips)),
            format!(
                "build {}; {}; AutoLogin={}{auto_login_text}; {layout}",
                client.version.as_deref().unwrap_or("?"),
                plural(client.saved_account_names.len(), "saved account name"),
                auto_login.unwrap_or("(absent)")
            ),
        );
    }

    fn token_checks(&self, checks: &mut Checks, running: bool, events: &[LogEvent]) {
        let current = match self.session_from(running, events) {
            Ok(current) => current,
            Err(error) => {
                checks.add(
                    "token",
                    "Session token",
                    HealthStatus::Warn,
                    format!("Could not read {UNIFIED_AUTH_PATH}: {error}"),
                );
                return;
            }
        };
        let owner_uid = current
            .owner
            .as_ref()
            .and_then(|owner| owner.launcher_uid.clone())
            .or_else(|| current.logged_uid.clone());
        let prefs = self.read_user_prefs(owner_uid.as_deref());
        let logged = |owner: &AccountRef| format!("; the client log names {}", owner.display_name);
        let who = match (current.remembered(), &current.token_owner, &current.owner) {
            (false, _, _) => "no remembered session (signed out)".to_owned(),
            (true, Some(stored), Some(owner)) if owner != stored => {
                format!("belongs to {}{}", stored.display_name, logged(owner))
            }
            (true, Some(stored), _) => format!("belongs to {}", stored.display_name),
            (true, None, Some(owner)) => format!("matches no saved sign-in{}", logged(owner)),
            (true, None, None) if current.signed_out => {
                "left over from a run that ended with a logout".to_owned()
            }
            (true, None, None) => "not linked to any One Launch account".to_owned(),
        };
        let token = current
            .token
            .as_ref()
            .map(|token| format!(" ({})", describe_token(token)))
            .unwrap_or_default();
        checks.add(
            "token",
            "Session token",
            status_if(current.tokens.len() > 1),
            format!(
                "{} under {UNIFIED_AUTH_PATH}; {who}{token}",
                plural(current.tokens.len(), "value")
            ),
        );
        if let Some((path, prefs)) = prefs {
            let closes = prefs.hide_on_close == Some(true);
            let shown = |value: Option<bool>| {
                value.map_or_else(|| "(absent)".to_owned(), |value| value.to_string())
            };
            let tail = if closes {
                " (a window close hides the client; the switch will have to force-close it)"
            } else {
                ""
            };
            checks.add(
                "prefs",
                "Close behaviour",
                status_if(closes || prefs.display_exit_prompt == Some(true)),
                format!(
                    "{path}: HideOnClose={}, DisplayExitPrompt={}{tail}",
                    shown(prefs.hide_on_close),
                    shown(prefs.display_exit_prompt)
                ),
            );
        }
    }

    fn log_check(&self, checks: &mut Checks, log: &Result<Option<Vec<LogEvent>>, AdapterError>) {
        let dir = self.log_dir();
        if !self.platform.files.exists(&dir) {
            checks.add(
                "log",
                "Client log",
                HealthStatus::Warn,
                format!(
                    "{dir} not found. Sign-ins cannot be confirmed until the client has run once."
                ),
            );
            return;
        }
        let events = match log {
            Ok(Some(events)) => events,
            Ok(None) => {
                checks.add(
                    "log",
                    "Client log",
                    HealthStatus::Warn,
                    format!(
                        "No client log in {dir}. Sign-ins cannot be confirmed until the client has run once."
                    ),
                );
                return;
            }
            Err(error) => {
                checks.add(
                    "log",
                    "Client log",
                    HealthStatus::Warn,
                    format!("Could not read the client log in {dir}: {error}"),
                );
                return;
            }
        };
        let (status, detail) = match last_sign_in(events).as_ref() {
            None => (
                HealthStatus::Ok,
                "Readable; no sign-in in the current log".to_owned(),
            ),
            Some(SignInOutcome::SignedIn { account_id, .. }) => (
                HealthStatus::Ok,
                format!(
                    "Readable; the last run signed in as account {}",
                    id_tail(Some(account_id), 4)
                ),
            ),
            Some(SignInOutcome::Failed { reason }) => (
                HealthStatus::Warn,
                format!("Readable; the last sign-in failed ({reason})"),
            ),
        };
        checks.add("log", "Client log", status, detail);
    }
}
