// The Epic Games Launcher's health check and Locate. Reads only.

use std::collections::BTreeSet;

use ol_formats::epic::ini::section_names;
use ol_formats::epic::launcher_log::{LogEvent, SignInOutcome, last_sign_in};
use ol_formats::epic::manifests::parse_launcher_installed;
use ol_formats::winpath::{join, normalize_launcher_path, same_path};
use ol_model::{HealthCheck, HealthStatus, InstallSource, InstalledGame, LauncherInstall};

use super::{EXE, EpicAdapter, LogMark, account_of_section, games_from};
use crate::AdapterError;
use crate::files::{MAX_LAUNCHER_FILE_BYTES, read_launcher_text};
use crate::health::{Checks, Known, chosen_folder_check, is_named, source_label};
use crate::steam::plural;

fn status_if(warn: bool) -> HealthStatus {
    if warn {
        HealthStatus::Warn
    } else {
        HealthStatus::Ok
    }
}

impl EpicAdapter<'_> {
    // The launcher sits four folders down from its install folder (Portal\Binaries\Win64), as the command of its link has it.
    pub fn install_from_exe(&self, exe: &str) -> Option<LauncherInstall> {
        let exe = normalize_launcher_path(exe);
        if !is_named(&exe, &[EXE]) {
            return None;
        }
        self.install_at(join(&[&exe, "..", "..", "..", ".."]), InstallSource::User)
            .filter(|install| same_path(&install.exe_path, &exe))
    }

    pub fn launcher_installed_path(&self) -> String {
        join(&[
            &self.config.program_data,
            "Epic",
            "UnrealEngineLauncher",
            "LauncherInstalled.dat",
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
                    "Epic Games Launcher is not installed on this PC. Checked the Uninstall key, the com.epicgames.launcher URI handler, and {}.",
                    self.config.default_dir
                ),
            );
            checks.push(chosen_gone);
            return checks.0;
        };
        checks.add(
            "install",
            "Installation",
            HealthStatus::Ok,
            format!(
                "{} (from {})",
                install.install_dir,
                source_label(install.source)
            ),
        );
        checks.push(chosen_gone);
        let running = checks.process(EXE, known.running.map_or_else(|| self.is_running(), Ok));

        let games = self.manifest_checks(&mut checks);
        self.launcher_installed_check(&mut checks, games.as_deref());
        let log = self
            .platform
            .files
            .exists(&self.log_path())
            .then(|| self.read_events_since(&LogMark::default()));
        let last = match &log {
            Some(Ok(events)) => last_sign_in(events),
            _ => None,
        };
        self.settings_check(&mut checks, running, last);
        self.log_check(&mut checks, log);
        checks.0
    }

    // The installed games, or None when the scan failed or passed over a manifest it could not read.
    fn manifest_checks(&self, checks: &mut Checks) -> Option<Vec<InstalledGame>> {
        match self.scan_manifests() {
            Ok(Some(scan)) => {
                let (read, unreadable) = (scan.manifests.len(), scan.unreadable);
                let games = games_from(scan.manifests);
                let unreadable_text = if unreadable > 0 {
                    format!(", {unreadable} unreadable")
                } else {
                    String::new()
                };
                checks.add(
                    "manifests",
                    "Manifests",
                    status_if(unreadable > 0),
                    format!(
                        "{} in {}{unreadable_text}; {}",
                        plural(read, ".item manifest"),
                        scan.dir,
                        plural(games.len(), "installed game")
                    ),
                );
                (unreadable == 0).then_some(games)
            }
            Ok(None) => {
                let dir = self.manifests_dir().unwrap_or_default();
                checks.add(
                    "manifests",
                    "Manifests",
                    HealthStatus::Warn,
                    format!(
                        "{dir} not found. No Epic games are installed, or the launcher has never installed one."
                    ),
                );
                Some(Vec::new())
            }
            Err(error) => {
                checks.add(
                    "manifests",
                    "Manifests",
                    HealthStatus::Fail,
                    format!("Could not scan the manifests: {error}"),
                );
                None
            }
        }
    }

    fn launcher_installed_check(&self, checks: &mut Checks, games: Option<&[InstalledGame]>) {
        let files = self.platform.files.as_ref();
        let installed = self.launcher_installed_path();
        if !files.exists(&installed) {
            checks.add(
                "launcher-installed",
                "LauncherInstalled.dat",
                HealthStatus::Warn,
                format!("{installed} not found; the cross-check is skipped."),
            );
            return;
        }
        let parsed = read_launcher_text(files, &installed, MAX_LAUNCHER_FILE_BYTES)
            .map_err(|error| error.to_string())
            .and_then(|text| parse_launcher_installed(&text).map_err(|error| error.to_string()));
        match parsed {
            Ok(entries) => {
                let missing = games
                    .unwrap_or_default()
                    .iter()
                    .filter(|game| {
                        !entries
                            .iter()
                            .any(|entry| entry.app_name == game.launcher_game_id)
                    })
                    .count();
                let entries_text = format!(
                    "{} {}",
                    entries.len(),
                    if entries.len() == 1 {
                        "entry"
                    } else {
                        "entries"
                    }
                );
                let tail = match (missing, games) {
                    (0, Some(_)) => ", agrees with the manifests".to_owned(),
                    (0, None) => {
                        "; not every manifest could be read, so the cross-check is incomplete"
                            .to_owned()
                    }
                    (_, _) => format!("; {missing} installed game(s) not listed there"),
                };
                checks.add(
                    "launcher-installed",
                    "LauncherInstalled.dat",
                    status_if(missing > 0 || games.is_none()),
                    format!("{entries_text}{tail}"),
                );
            }
            Err(reason) => checks.add(
                "launcher-installed",
                "LauncherInstalled.dat",
                HealthStatus::Warn,
                format!("Could not parse {installed}: {reason}"),
            ),
        }
    }

    fn settings_check(
        &self,
        checks: &mut Checks,
        running: Option<bool>,
        last: Option<SignInOutcome>,
    ) {
        let path = self.settings_path();
        let paths = self.settings_paths();
        if paths.is_empty() {
            checks.add(
                "settings",
                "GameUserSettings.ini",
                HealthStatus::Warn,
                format!("{path} not found. The launcher has not run for this Windows user yet."),
            );
            return;
        }
        let read = (|| {
            let settings = self.read_settings()?;
            let running = match running {
                Some(running) => running,
                None => self.is_running()?,
            };
            let written_at = settings.as_ref().and_then(|settings| settings.written_at);
            let (path, text) = settings.map_or_else(
                || (path.clone(), String::new()),
                |settings| (settings.path, settings.text),
            );
            let current = self.session_from(Some(&text), written_at, running, last)?;
            Ok::<_, AdapterError>((path, text, current))
        })();
        let (path, text, current) = match read {
            Ok(read) => read,
            Err(error @ AdapterError::LauncherFile { .. }) => {
                checks.add(
                    "settings",
                    "GameUserSettings.ini",
                    HealthStatus::Fail,
                    error.to_string(),
                );
                return;
            }
            Err(error) => {
                checks.add(
                    "settings",
                    "GameUserSettings.ini",
                    HealthStatus::Fail,
                    format!("Could not read {path}: {error}"),
                );
                return;
            }
        };
        let ids: BTreeSet<String> = section_names(&text)
            .iter()
            .filter_map(|name| account_of_section(name))
            .collect();
        let who = match (&current.disk.remembered, &current.owner) {
            (true, Some(owner)) => format!("remembered session belongs to {}", owner.display_name),
            (true, None) => "remembered session not linked to any One Launch account".to_owned(),
            (false, _) => "no remembered session (signed out)".to_owned(),
        };
        let enable = current.disk.enable.as_deref();
        let enabled = enable.is_none_or(|enable| enable.to_lowercase() == "true");
        let files_text = if paths.len() > 1 {
            let newest = if path.contains("WindowsEditor") {
                "WindowsEditor"
            } else {
                "Windows"
            };
            format!(
                "{} settings files, reading the newest ({newest}); ",
                paths.len()
            )
        } else {
            String::new()
        };
        let signed_in = if ids.len() == 1 { "has" } else { "have" };
        let enable_text = if enabled {
            String::new()
        } else {
            format!(
                "; [RememberMe] Enable={}, so the launcher does not sign in on its own until a switch sets it back",
                enable.unwrap_or_default()
            )
        };
        checks.add(
            "settings",
            "GameUserSettings.ini",
            status_if(!enabled),
            format!(
                "{files_text}{} {signed_in} signed in on this PC; {who}{enable_text}",
                plural(ids.len(), "account")
            ),
        );
    }

    fn log_check(&self, checks: &mut Checks, log: Option<Result<Vec<LogEvent>, AdapterError>>) {
        let path = self.log_path();
        let Some(log) = log else {
            checks.add(
                "log",
                "Launcher log",
                HealthStatus::Warn,
                format!("{path} not found. Sign-ins cannot be confirmed until the launcher has run once."),
            );
            return;
        };
        let events = match log {
            Ok(events) => events,
            Err(error) => {
                checks.add(
                    "log",
                    "Launcher log",
                    HealthStatus::Warn,
                    format!("Could not read {path}: {error}"),
                );
                return;
            }
        };
        let (status, detail) = match last_sign_in(&events) {
            None => (
                HealthStatus::Ok,
                "Readable; no sign-in attempt in the current log".to_owned(),
            ),
            Some(SignInOutcome::SignedIn { .. }) => (
                HealthStatus::Ok,
                "Readable; the last run signed in".to_owned(),
            ),
            Some(SignInOutcome::Failed { reason }) => (
                HealthStatus::Warn,
                format!("Readable; the last sign-in failed ({reason})"),
            ),
        };
        checks.add("log", "Launcher log", status, detail);
    }
}
