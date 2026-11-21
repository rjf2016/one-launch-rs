// Steam's health check and Locate. Reads only.

use ol_formats::steam::loginusers::{SteamLoginUser, parse_login_users};
use ol_formats::winpath::{join, normalize_launcher_path};
use ol_model::{HealthCheck, HealthStatus, InstallSource, LauncherInstall};

use super::{EXE, HKCU_KEY, HKLM_KEY, SteamAdapter, plural};
use crate::files::{MAX_VDF_BYTES, read_launcher_text};
use crate::health::{Checks, Known, chosen_folder_check, is_named};

const fn source_label(source: InstallSource) -> &'static str {
    match source {
        InstallSource::User => "the folder chosen in Settings",
        InstallSource::RegistryHkcu => r"HKCU\Software\Valve\Steam\SteamPath",
        InstallSource::RegistryHklm => r"HKLM\SOFTWARE\WOW6432Node\Valve\Steam\InstallPath",
        InstallSource::LauncherFile => "a launcher file",
        InstallSource::DefaultPath => "default location",
    }
}

fn status_if(warn: bool) -> HealthStatus {
    if warn {
        HealthStatus::Warn
    } else {
        HealthStatus::Ok
    }
}

impl SteamAdapter<'_> {
    // The install a picked steam.exe belongs to, when it is steam.exe and is there.
    pub fn install_from_exe(&self, exe: &str) -> Option<LauncherInstall> {
        let exe = normalize_launcher_path(exe);
        if !is_named(&exe, &[EXE]) {
            return None;
        }
        self.install_at(Some(&join(&[&exe, ".."])), InstallSource::User)
    }

    // Every check but the snapshots, which are One Launch's own and are added by the caller.
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
                    "Steam is not installed on this PC. Checked HKCU\\{HKCU_KEY}\\SteamPath, HKLM\\{HKLM_KEY}\\InstallPath, and {}.",
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
        checks.process(EXE, known.running.map_or_else(|| self.is_running(), Ok));

        let users = self.login_users_checks(&mut checks, install);
        self.auto_login_check(&mut checks, &users);
        self.library_checks(&mut checks, install);
        checks.0
    }

    fn login_users_checks(
        &self,
        checks: &mut Checks,
        install: &LauncherInstall,
    ) -> Vec<SteamLoginUser> {
        let files = self.platform.files.as_ref();
        let login_users = Self::login_users_path(install);
        if !files.exists(&login_users) {
            checks.add(
                "loginusers",
                "Remembered accounts",
                HealthStatus::Warn,
                format!(
                    "{login_users} not found. Steam has not signed in on this PC yet, so only the credentials switch path is available."
                ),
            );
            return Vec::new();
        }
        let parsed = read_launcher_text(files, &login_users, MAX_VDF_BYTES)
            .map_err(|error| error.to_string())
            .and_then(|text| parse_login_users(&text).map_err(|error| error.to_string()));
        let users = match parsed {
            Ok(users) => users,
            Err(reason) => {
                checks.add(
                    "loginusers",
                    "Remembered accounts",
                    HealthStatus::Fail,
                    format!("Could not parse {login_users}: {reason}"),
                );
                return Vec::new();
            }
        };
        let remembered = users.iter().filter(|user| user.remember_password).count();
        checks.add(
            "loginusers",
            "Remembered accounts",
            HealthStatus::Ok,
            format!(
                "{} in loginusers.vdf, {remembered} with a remembered session",
                plural(users.len(), "account")
            ),
        );
        let mut keys: Vec<&str> = Vec::new();
        for key in users.iter().flat_map(|user| &user.most_recent_keys) {
            if !keys.contains(&key.as_str()) {
                keys.push(key.as_str());
            }
        }
        let most_recent = users.iter().filter(|user| user.most_recent).count();
        if !users.is_empty() && keys.is_empty() {
            checks.add(
                "most-recent-key",
                "Most-recent marker",
                HealthStatus::Warn,
                "Neither AutoLogin nor MostRecent appears in loginusers.vdf. A switch will add AutoLogin.",
            );
        } else if !users.is_empty() {
            checks.add(
                "most-recent-key",
                "Most-recent marker",
                status_if(most_recent > 1),
                format!(
                    "File uses {}; {} marked most recent{}",
                    keys.join(" and "),
                    plural(most_recent, "account"),
                    if most_recent > 1 {
                        " (expected one)"
                    } else {
                        ""
                    }
                ),
            );
        }
        users
    }

    fn auto_login_check(&self, checks: &mut Checks, users: &[SteamLoginUser]) {
        match self.auto_login_user() {
            Ok(None) => checks.add(
                "registry",
                "Registry AutoLoginUser",
                HealthStatus::Warn,
                format!(
                    "HKCU\\{HKCU_KEY}\\AutoLoginUser is absent. Steam will ask which account to use on next start."
                ),
            ),
            Ok(Some(auto_login_user)) => {
                let agrees = users
                    .iter()
                    .find(|user| user.most_recent)
                    .is_none_or(|user| {
                        user.account_name.to_lowercase() == auto_login_user.to_lowercase()
                    });
                checks.add(
                    "registry",
                    "Registry AutoLoginUser",
                    status_if(!agrees),
                    if agrees {
                        "Present and matches the most recent account in loginusers.vdf"
                    } else {
                        "Present but names a different account than loginusers.vdf marks as most recent"
                    },
                );
            }
            Err(error) => checks.add(
                "registry",
                "Registry AutoLoginUser",
                HealthStatus::Warn,
                format!("Could not read the registry: {error}"),
            ),
        }
    }

    fn library_checks(&self, checks: &mut Checks, install: &LauncherInstall) {
        let files = self.platform.files.as_ref();
        let library_folders = Self::library_folders_path(install);
        if !files.exists(&library_folders) {
            checks.add(
                "libraries",
                "Library folders",
                HealthStatus::Warn,
                format!("{library_folders} not found. Only the main Steam folder will be scanned."),
            );
        } else {
            match self.library_folders(install) {
                (_, Some(error)) => {
                    checks.add("libraries", "Library folders", HealthStatus::Fail, error);
                }
                (folders, None) => {
                    let missing: Vec<&str> = folders
                        .iter()
                        .filter(|folder| !files.exists(&join(&[folder, "steamapps"])))
                        .map(String::as_str)
                        .collect();
                    let tail = if missing.is_empty() {
                        String::new()
                    } else {
                        format!("; missing steamapps in: {}", missing.join(", "))
                    };
                    checks.add(
                        "libraries",
                        "Library folders",
                        status_if(!missing.is_empty()),
                        format!("{}{tail}", plural(folders.len(), "library folder")),
                    );
                }
            }
        }

        match self.list_installed_games() {
            Ok(games) => {
                let updates = games.iter().filter(|game| game.needs_update).count();
                let tail = if updates > 0 {
                    format!(" ({updates} waiting for an update)")
                } else {
                    String::new()
                };
                checks.add(
                    "games",
                    "Installed games",
                    HealthStatus::Ok,
                    format!("{}{tail}", plural(games.len(), "installed game")),
                );
            }
            Err(error) => checks.add(
                "games",
                "Installed games",
                HealthStatus::Fail,
                format!("Could not scan the library: {error}"),
            ),
        }
    }
}
