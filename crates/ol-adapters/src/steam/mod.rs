// The Steam adapter: everything it reads, and the two things it does to a process (ask Steam to shut down, hand a game link to the shell). It writes no launcher file and no registry value; the switch pipeline does that with what this module plans.

mod plan;

use std::collections::HashSet;
use std::time::Duration;

use ol_formats::js;
use ol_formats::steam::bootstrap_log::update_in_progress;
use ol_formats::steam::login_log::{SignInOutcome, latest_outcome, parse_login_states};
use ol_formats::steam::loginusers::{SteamLoginUser, parse_login_users};
use ol_formats::steam::manifests::{
    is_app_manifest_file, is_fully_installed, needs_update, parse_app_manifest,
    parse_library_folders,
};
use ol_formats::winpath::{join, normalize_launcher_path, path_key, resolve_from};
use ol_model::{
    InstallSource, InstalledGame, LauncherId, LauncherInstall, RememberedAccount, RunningGame,
    RunningGameSource,
};
use ol_platform_api::{CancelToken, Hive, Platform, ProcessArg};
use ol_runtime::{TimingOverrides, wait_until};
use time::OffsetDateTime;

use crate::AdapterError;
use crate::files::{MAX_TAIL_BYTES, MAX_VDF_BYTES, log_mark, read_launcher_text, read_log_since};
use crate::running::game_with_running_process;

pub use crate::directory::AccountRef;
pub(crate) use plan::step;
pub use plan::{PlanContext, STEP, SwitchPreflight, close_message};

pub const HKCU_KEY: &str = r"Software\Valve\Steam";
pub const HKLM_KEY: &str = r"SOFTWARE\WOW6432Node\Valve\Steam";
pub const EXE: &str = "steam.exe";
pub const DEFAULT_DIR: &str = r"C:\Program Files (x86)\Steam";
// A remembered session older than this may have expired. Only the times in loginusers.vdf are looked at; Steam's token store is never read.
pub const STALE_SESSION_DAYS: f64 = 60.0;

// Apps Steam installs beside games that are no games: the redistributables and the Linux runtimes.
const TOOL_APP_IDS: [&str; 4] = ["228980", "1070560", "1391110", "1628350"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timing {
    pub poll: Duration,
    // The waits that read a log or the registry look more often than those that read the process list.
    pub log_poll: Duration,
    pub shutdown: Duration,
    pub kill_grace: Duration,
    pub startup: Duration,
    pub sign_in: Duration,
    pub guard: Duration,
    // The whole automatic wait while Steam updates itself.
    pub update: Duration,
    pub manual_sign_in: Duration,
    // ActiveUser can name the account before Steam has loaded its library.
    pub ready_grace: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            poll: Duration::from_millis(500),
            log_poll: Duration::from_millis(250),
            shutdown: Duration::from_secs(30),
            kill_grace: Duration::from_secs(5),
            startup: Duration::from_secs(60),
            sign_in: Duration::from_secs(60),
            guard: Duration::from_secs(120),
            update: Duration::from_secs(900),
            manual_sign_in: Duration::from_secs(600),
            ready_grace: Duration::from_secs(5),
        }
    }
}

impl Timing {
    pub fn with(overrides: &TimingOverrides) -> Self {
        let default = Self::default();
        let named = |name: &str, default: Duration| overrides.get(name).unwrap_or(default);
        Self {
            poll: named("pollMs", default.poll),
            log_poll: overrides.finer_poll("logPollMs", default.log_poll),
            shutdown: named("shutdownMs", default.shutdown),
            kill_grace: named("killGraceMs", default.kill_grace),
            startup: named("startupMs", default.startup),
            sign_in: named("signInMs", default.sign_in),
            guard: named("guardMs", default.guard),
            update: named("updateMs", default.update),
            manual_sign_in: named("manualSignInMs", default.manual_sign_in),
            ready_grace: named("readyGraceMs", default.ready_grace),
        }
    }
}

// A length of time as the Electron app writes it into a sentence: 30, 1.5, 0.5.
pub fn in_units(duration: Duration, unit: Duration) -> String {
    format!("{}", duration.as_millis() as f64 / unit.as_millis() as f64)
}

pub fn seconds(duration: Duration) -> String {
    in_units(duration, Duration::from_secs(1))
}

pub fn minutes(duration: Duration) -> String {
    in_units(duration, Duration::from_secs(60))
}

pub fn plural(count: usize, word: &str) -> String {
    format!("{count} {word}{}", if count == 1 { "" } else { "s" })
}

// What the settings and the data folder say, read once for the call in hand.
#[derive(Clone)]
pub struct SteamConfig {
    // The folder the user chose with Locate, tried before the registry.
    pub chosen_dir: Option<String>,
    pub default_dir: String,
    // Put the stored password on Steam's command line when its remembered session has expired.
    pub auto_login: bool,
    pub backups_to_keep: i64,
    // Where a snapshot of Steam's files would go, for the plan's wording.
    pub backups_dir: String,
    pub timing: Timing,
}

pub struct SteamAdapter<'a> {
    pub(crate) platform: &'a Platform,
    pub(crate) config: &'a SteamConfig,
}

fn iso(unix_seconds: i64) -> Option<String> {
    let at = OffsetDateTime::from_unix_timestamp(unix_seconds).ok()?;
    (0..=9999)
        .contains(&at.year())
        .then(|| ol_store::iso_timestamp(at))
}

// Whole days since a sign-in, or None when its time was never recorded.
pub(crate) fn session_age_days(timestamp: Option<i64>, now: OffsetDateTime) -> Option<f64> {
    let timestamp = timestamp.filter(|at| *at != 0)?;
    let now_seconds = (now.unix_timestamp_nanos() / 1_000_000) as f64 / 1000.0;
    Some(((now_seconds - timestamp as f64) / 86_400.0).floor())
}

impl<'a> SteamAdapter<'a> {
    pub const fn new(platform: &'a Platform, config: &'a SteamConfig) -> Self {
        Self { platform, config }
    }

    pub fn timing(&self) -> &Timing {
        &self.config.timing
    }

    pub fn auto_login(&self) -> bool {
        self.config.auto_login
    }

    pub fn login_users_path(install: &LauncherInstall) -> String {
        join(&[&install.install_dir, "config", "loginusers.vdf"])
    }

    pub fn library_folders_path(install: &LauncherInstall) -> String {
        join(&[&install.install_dir, "steamapps", "libraryfolders.vdf"])
    }

    pub fn login_log_path(install: &LauncherInstall) -> String {
        join(&[&install.install_dir, "logs", "steamui_login.txt"])
    }

    pub fn bootstrap_log_path(install: &LauncherInstall) -> String {
        join(&[&install.install_dir, "logs", "bootstrap_log.txt"])
    }

    pub fn chosen_install_dir(&self) -> Option<String> {
        self.config
            .chosen_dir
            .as_deref()
            .filter(|raw| !js::trim(raw).is_empty())
            .map(normalize_launcher_path)
    }

    fn install_at(&self, raw: Option<&str>, source: InstallSource) -> Option<LauncherInstall> {
        let raw = raw.filter(|raw| !js::trim(raw).is_empty())?;
        let install_dir = normalize_launcher_path(raw);
        let exe_path = join(&[&install_dir, EXE]);
        self.platform
            .files
            .exists(&exe_path)
            .then_some(LauncherInstall {
                launcher: LauncherId::Steam,
                install_dir,
                exe_path,
                source,
            })
    }

    // The first of these that holds steam.exe: the folder chosen in Settings, the two registry values, the usual place.
    pub fn detect_install(&self) -> Result<Option<LauncherInstall>, AdapterError> {
        let registry = &self.platform.registry;
        if let Some(chosen) =
            self.install_at(self.chosen_install_dir().as_deref(), InstallSource::User)
        {
            return Ok(Some(chosen));
        }
        let path = registry.get_string(Hive::Hkcu, HKCU_KEY, "SteamPath")?;
        if let Some(mut found) = self.install_at(path.as_deref(), InstallSource::RegistryHkcu) {
            if let Some(exe) = registry
                .get_string(Hive::Hkcu, HKCU_KEY, "SteamExe")?
                .filter(|exe| !exe.is_empty())
            {
                let exe = normalize_launcher_path(&exe);
                if self.platform.files.exists(&exe) {
                    found.exe_path = exe;
                }
            }
            return Ok(Some(found));
        }
        let path = registry.get_string(Hive::Hklm, HKLM_KEY, "InstallPath")?;
        if let Some(found) = self.install_at(path.as_deref(), InstallSource::RegistryHklm) {
            return Ok(Some(found));
        }
        Ok(self.install_at(Some(&self.config.default_dir), InstallSource::DefaultPath))
    }

    pub fn require_install(&self) -> Result<LauncherInstall, AdapterError> {
        self.detect_install()?
            .ok_or(AdapterError::NotInstalled(LauncherId::Steam))
    }

    // The install folder first, then every other folder of libraryfolders.vdf, each once. A file that cannot be read leaves the install folder alone, with the reason.
    pub fn library_folders(&self, install: &LauncherInstall) -> (Vec<String>, Option<String>) {
        let file = Self::library_folders_path(install);
        let mut folders = vec![install.install_dir.clone()];
        if !self.platform.files.exists(&file) {
            return (folders, None);
        }
        let parsed = read_launcher_text(self.platform.files.as_ref(), &file, MAX_VDF_BYTES)
            .map_err(|error| error.to_string())
            .and_then(|text| parse_library_folders(&text).map_err(|error| error.to_string()));
        match parsed {
            Ok(paths) => {
                let mut seen = HashSet::from([path_key(&install.install_dir)]);
                for path in paths {
                    if seen.insert(path_key(&path)) {
                        folders.push(path);
                    }
                }
                (folders, None)
            }
            Err(reason) => (folders, Some(format!("Could not parse {file}: {reason}"))),
        }
    }

    // A game's folder under steamapps\common, and that folder itself when the manifest names one outside it.
    fn install_path_for(apps_dir: &str, install_dir: &str) -> String {
        let common = join(&[apps_dir, "common"]);
        let inside = format!("{}\\", common.to_lowercase());
        match resolve_from(&common, install_dir) {
            Some(candidate) if candidate.to_lowercase().starts_with(&inside) => candidate,
            _ => common,
        }
    }

    pub fn list_installed_games(&self) -> Result<Vec<InstalledGame>, AdapterError> {
        Ok(self.scan_installed_games()?.0)
    }

    // The games, and why the list can be short: a library list that could not be read leaves out every library but the main one.
    pub fn scan_installed_games(
        &self,
    ) -> Result<(Vec<InstalledGame>, Option<String>), AdapterError> {
        let install = self.require_install()?;
        let files = self.platform.files.as_ref();
        let (folders, unread) = self.library_folders(&install);
        let mut games = Vec::new();
        let mut seen = HashSet::new();
        for library in folders {
            let apps_dir = join(&[&library, "steamapps"]);
            if !files.exists(&apps_dir) {
                continue;
            }
            let names = files
                .list(&apps_dir)
                .map_err(|error| AdapterError::io(&error))?;
            for name in names.iter().filter(|name| is_app_manifest_file(name)) {
                let file = join(&[&apps_dir, name]);
                let Ok(manifest) = read_launcher_text(files, &file, MAX_VDF_BYTES)
                    .map_err(|error| error.to_string())
                    .and_then(|text| parse_app_manifest(&text).map_err(|error| error.to_string()))
                else {
                    continue;
                };
                if !is_fully_installed(manifest.state_flags)
                    || TOOL_APP_IDS.contains(&manifest.app_id.as_str())
                    || !seen.insert(manifest.app_id.clone())
                {
                    continue;
                }
                games.push(InstalledGame {
                    launcher: LauncherId::Steam,
                    install_path: Self::install_path_for(&apps_dir, &manifest.install_dir),
                    library_path: Some(library.clone()),
                    size_on_disk_bytes: manifest.size_on_disk,
                    last_played_at: manifest.last_played.and_then(iso),
                    needs_update: needs_update(manifest.state_flags),
                    launcher_game_id: manifest.app_id,
                    name: manifest.name,
                });
            }
        }
        games.sort_by_cached_key(|game| game.name.to_lowercase());
        Ok((games, unread))
    }

    pub fn read_login_users(
        &self,
        install: &LauncherInstall,
    ) -> Result<Vec<SteamLoginUser>, AdapterError> {
        let file = Self::login_users_path(install);
        let failed = |message: String| AdapterError::LauncherFile {
            path: file.clone(),
            message,
        };
        if !self.platform.files.exists(&file) {
            return Err(failed("loginusers.vdf not found".to_owned()));
        }
        read_launcher_text(self.platform.files.as_ref(), &file, MAX_VDF_BYTES)
            .map_err(|error| error.to_string())
            .and_then(|text| parse_login_users(&text).map_err(|error| error.to_string()))
            .map_err(|reason| failed(format!("Could not parse loginusers.vdf ({reason})")))
    }

    // The accounts Steam remembers: the one it would sign in first, then the latest sign-in first.
    pub fn list_remembered_accounts(&self) -> Result<Vec<RememberedAccount>, AdapterError> {
        let install = self.require_install()?;
        let now = self.platform.clock.now();
        let mut accounts: Vec<RememberedAccount> = self
            .read_login_users(&install)?
            .into_iter()
            .map(|user| RememberedAccount {
                launcher: LauncherId::Steam,
                last_login_at: user.timestamp.filter(|at| *at != 0).and_then(iso),
                session_may_be_expired: session_age_days(user.timestamp, now)
                    .is_none_or(|days| days >= STALE_SESSION_DAYS),
                launcher_uid: user.steam_id64,
                login: user.account_name,
                persona_name: user.persona_name,
                remember_password: user.remember_password,
                most_recent: user.most_recent,
                linked_account_id: None,
            })
            .collect();
        accounts.sort_by(|a, b| {
            b.most_recent.cmp(&a.most_recent).then_with(|| {
                let at =
                    |account: &RememberedAccount| account.last_login_at.clone().unwrap_or_default();
                at(b).cmp(&at(a))
            })
        });
        Ok(accounts)
    }

    pub fn is_running(&self) -> Result<bool, AdapterError> {
        Ok(self.platform.processes.is_running(EXE)?)
    }

    pub(crate) fn auto_login_user(&self) -> Result<Option<String>, AdapterError> {
        Ok(self
            .platform
            .registry
            .get_string(Hive::Hkcu, HKCU_KEY, "AutoLoginUser")?)
    }

    pub(crate) fn remember_password(&self) -> Result<Option<u32>, AdapterError> {
        Ok(self
            .platform
            .registry
            .get_dword(Hive::Hkcu, HKCU_KEY, "RememberPassword")?)
    }

    // The account id Steam last signed in, which it does not clear when it exits.
    pub fn active_user(&self) -> Result<Option<u32>, AdapterError> {
        Ok(self.platform.registry.get_dword(
            Hive::Hkcu,
            &format!("{HKCU_KEY}\\ActiveProcess"),
            "ActiveUser",
        )?)
    }

    pub fn login_log_mark(&self, install: &LauncherInstall) -> u64 {
        log_mark(self.platform.files.as_ref(), &Self::login_log_path(install))
    }

    pub fn bootstrap_log_mark(&self, install: &LauncherInstall) -> u64 {
        log_mark(
            self.platform.files.as_ref(),
            &Self::bootstrap_log_path(install),
        )
    }

    // What Steam's sign-in log has settled on since `mark`, if anything.
    pub fn outcome_since(&self, install: &LauncherInstall, mark: u64) -> Option<SignInOutcome> {
        outcome_since(
            self.platform.files.as_ref(),
            &Self::login_log_path(install),
            mark,
        )
    }

    pub fn updating_since(&self, install: &LauncherInstall, mark: u64) -> bool {
        updating_since(
            self.platform.files.as_ref(),
            &Self::bootstrap_log_path(install),
            mark,
        )
    }

    // A running Steam is signed in as this id. A client sitting on its sign-in form still shows the last account's id, so the log's last word can say no.
    pub fn signed_in_as(&self, install: &LauncherInstall, id3: i128) -> Result<bool, AdapterError> {
        if self.active_user()?.map(i128::from) != Some(id3) {
            return Ok(false);
        }
        Ok(!matches!(
            self.outcome_since(install, 0),
            Some(SignInOutcome::Credentials { .. })
        ))
    }

    // Steam's own flag first (RunningAppID, 0 when idle), then any process that runs out of a game's folder.
    pub fn running_game(&self) -> Result<Option<RunningGame>, AdapterError> {
        if self.detect_install()?.is_none() {
            return Ok(None);
        }
        let registry = &self.platform.registry;
        if let Some(app_id) = registry
            .get_dword(Hive::Hkcu, HKCU_KEY, "RunningAppID")?
            .filter(|id| *id > 0)
        {
            let name = registry
                .get_string(Hive::Hkcu, &format!("{HKCU_KEY}\\Apps\\{app_id}"), "Name")?
                .unwrap_or_else(|| format!("Steam app {app_id}"));
            return Ok(Some(RunningGame {
                launcher: LauncherId::Steam,
                launcher_game_id: app_id.to_string(),
                name,
                source: RunningGameSource::Launcher,
            }));
        }
        let Ok(processes) = self.platform.processes.list_processes() else {
            return Ok(None);
        };
        if processes.is_empty() {
            return Ok(None);
        }
        let games = self.list_installed_games()?;
        let files = self.platform.files.as_ref();
        Ok(
            game_with_running_process(files, &processes, &games).map(|game| RunningGame {
                launcher: LauncherId::Steam,
                launcher_game_id: game.launcher_game_id.clone(),
                name: game.name.clone(),
                source: RunningGameSource::Process,
            }),
        )
    }

    // Asks Steam to close with `steam.exe -shutdown`, waits, then ends it and what it started. Fails when Steam still runs after that.
    pub fn shutdown(
        &self,
        timeout: Option<Duration>,
        cancel: &CancelToken,
    ) -> Result<(), AdapterError> {
        let install = self.require_install()?;
        if !self.is_running()? {
            return Ok(());
        }
        let timing = self.timing();
        let clock = self.platform.clock.as_ref();
        let processes = &self.platform.processes;
        processes.start(
            &install.exe_path,
            &[ProcessArg::Plain("-shutdown")],
            &install.install_dir,
        )?;
        let gone = || Ok::<_, AdapterError>(!self.is_running()?);
        if wait_until(
            clock,
            cancel,
            timeout.unwrap_or(timing.shutdown),
            timing.poll,
            gone,
        )? {
            return Ok(());
        }
        processes.kill(EXE, true)?;
        if !wait_until(clock, cancel, timing.kill_grace, timing.poll, gone)? {
            return Err(AdapterError::Failed(
                "steam.exe is still running after -shutdown and a forced close".to_owned(),
            ));
        }
        Ok(())
    }

    // Hands steam://rungameid/<appid> to the shell, as Steam's own shortcuts do. Returns what was sent.
    pub fn launch_game(&self, launcher_game_id: &str) -> Result<String, AdapterError> {
        if !js::is_ascii_digits(launcher_game_id) {
            return Err(AdapterError::Failed(format!(
                "Not a Steam app id: {launcher_game_id}"
            )));
        }
        let url = format!("steam://rungameid/{launcher_game_id}");
        self.platform.shell.open_url(&url)?;
        Ok(url)
    }
}

pub fn outcome_since(
    files: &dyn ol_platform_api::FileReader,
    path: &str,
    mark: u64,
) -> Option<SignInOutcome> {
    latest_outcome(&parse_login_states(&read_log_since(
        files,
        path,
        mark,
        MAX_TAIL_BYTES,
    )))
}

pub fn updating_since(files: &dyn ol_platform_api::FileReader, path: &str, mark: u64) -> bool {
    update_in_progress(&read_log_since(files, path, mark, MAX_TAIL_BYTES))
}
