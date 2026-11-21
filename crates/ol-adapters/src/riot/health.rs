// The Riot Client's health check and Locate. Reads files and the registry, and asks the process list for image names alone: no Riot game or Vanguard is looked at any other way.

use ol_formats::riot::client_log::{
    FailReason, LogEvent, SignInOutcome, SignedInHow, is_client_log_file, last_sign_in,
    parse_log_events, products_needing_update,
};
use ol_formats::riot::session::SessionRead;
use ol_formats::winpath::{join, normalize_launcher_path};
use ol_model::{HealthCheck, HealthStatus, InstallSource, LauncherInstall};

use super::{EXE, INSTALLS_FILE, ProductRecord, RiotAdapter, SESSION_FILE, id_tail};
use crate::health::{Checks, Known, chosen_folder_check, is_named, source_label};
use crate::steam::plural;

const LOG_HEAD_BYTES: u64 = 512;

fn describe_sign_in(last: Option<&SignInOutcome>) -> String {
    match last {
        None => "reached no sign-in verdict".to_owned(),
        Some(SignInOutcome::Failed { detail, .. }) => {
            format!("ended on the login page ({detail})")
        }
        Some(SignInOutcome::SignedIn { how }) => match how {
            SignedInHow::Restored => "signed in from the saved sign-in",
            SignedInHow::Manual => "signed in by hand",
            SignedInHow::Unknown => "signed in",
        }
        .to_owned(),
    }
}

impl RiotAdapter<'_> {
    pub fn install_from_exe(&self, exe: &str) -> Option<LauncherInstall> {
        let exe = normalize_launcher_path(exe);
        if !is_named(&exe, &[EXE]) {
            return None;
        }
        self.install_at(join(&[&exe, ".."]), InstallSource::User)
    }

    fn is_installed(&self, record: &ProductRecord) -> bool {
        record.ok
            && record
                .install_path
                .as_deref()
                .is_some_and(|path| self.files.exists(path))
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
                    "Riot Games is not installed on this PC. Checked the Uninstall key, {}, the riotclient:// URI handler, and {}.",
                    self.installs_path(),
                    self.config.default_dir
                ),
            );
            checks.push(chosen_gone);
            return checks.0;
        };
        let source = match install.source {
            InstallSource::LauncherFile => INSTALLS_FILE,
            other => source_label(other),
        };
        checks.add(
            "install",
            "Installation",
            HealthStatus::Ok,
            format!("{} (from {source})", install.install_dir),
        );
        checks.push(chosen_gone);
        checks.process(EXE, known.running.map_or_else(|| self.is_running(), Ok));

        let installs = self.installs();
        checks.add(
            "installs",
            INSTALLS_FILE,
            if installs.is_some() {
                HealthStatus::Ok
            } else {
                HealthStatus::Warn
            },
            match &installs {
                Some(installs) => format!(
                    "rc_default = {}; {} associated with a client",
                    installs.rc_default.as_deref().unwrap_or("(absent)"),
                    plural(installs.associated_clients.len(), "game folder")
                ),
                None => format!(
                    "{} not found or unreadable; the client has not registered an install on this PC.",
                    self.installs_path()
                ),
            },
        );

        self.products_check(&mut checks);
        self.data_check(&mut checks);
        self.log_check(&mut checks);
        checks.0
    }

    fn products_check(&self, checks: &mut Checks) {
        let dir = self.metadata_dir();
        if !self.files.exists(&dir) {
            checks.add(
                "products",
                "Installed products",
                HealthStatus::Warn,
                format!("{dir} not found. No Riot game has been installed on this PC."),
            );
            return;
        }
        let records = self.products();
        let out_of_date = products_needing_update(&self.current_run_events());
        let installed = records
            .iter()
            .filter(|record| self.is_installed(record))
            .count();
        let state = |record: &ProductRecord| {
            if self.is_installed(record) {
                if out_of_date.contains(&record.id.key()) {
                    "installed, needs an update"
                } else {
                    "installed"
                }
            } else if record.ok {
                "install folder missing"
            } else {
                "not installed"
            }
        };
        let listed = if records.is_empty() {
            String::new()
        } else {
            let each: Vec<String> = records
                .iter()
                .map(|record| format!("{} ({})", record.id.id(), state(record)))
                .collect();
            format!(": {}", each.join(", "))
        };
        checks.add(
            "products",
            "Installed products",
            if installed == 0 {
                HealthStatus::Warn
            } else {
                HealthStatus::Ok
            },
            format!(
                "{} in Metadata{listed}",
                plural(records.len(), "product folder")
            ),
        );
    }

    // One line on the session file: never a value, only whose sign-in it holds.
    fn data_check(&self, checks: &mut Checks) {
        let dir = self.data_dir();
        if !self.files.exists(&dir) {
            checks.add(
                "data",
                "Client data",
                HealthStatus::Warn,
                format!("{dir} not found. The client has not run for this Windows user yet."),
            );
            return;
        }
        let (status, detail) = match self.read_session_file() {
            Err(error) => (
                HealthStatus::Warn,
                format!("{SESSION_FILE} could not be read: {error}"),
            ),
            Ok(None) => (
                HealthStatus::Ok,
                format!("{SESSION_FILE} absent (the client has not saved a sign-in yet)"),
            ),
            Ok(Some(file)) => match &file.read {
                SessionRead::SignedOut => (
                    HealthStatus::Ok,
                    format!("{SESSION_FILE} present, signed out (no saved sign-in)"),
                ),
                SessionRead::Unexpected(reason) => (
                    HealthStatus::Warn,
                    format!("{SESSION_FILE} is not in the shape One Launch knows ({reason})"),
                ),
                SessionRead::Session(session) => {
                    let holds = format!(
                        "{SESSION_FILE} holds a saved sign-in for Riot account {}",
                        id_tail(Some(&session.account_id), 4)
                    );
                    if session.dpop_bound == Some(true) {
                        (
                            HealthStatus::Warn,
                            format!(
                                "{holds}, bound to a device key (is_dpop_bound), which a saved copy may not carry"
                            ),
                        )
                    } else {
                        (HealthStatus::Ok, holds)
                    }
                }
            },
        };
        checks.add("data", "Client data", status, format!("{dir}; {detail}"));
    }

    fn log_check(&self, checks: &mut Checks) {
        let dir = self.log_dir();
        if !self.files.exists(&dir) {
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
        let runs = self.files.list(&dir).map_or(0, |names| {
            names.iter().filter(|name| is_client_log_file(name)).count()
        });
        let last = last_sign_in(&self.current_run_events());
        let build = self.current_run_build();
        let rejected = matches!(
            last,
            Some(SignInOutcome::Failed {
                reason: FailReason::Rejected,
                ..
            })
        );
        let build = build
            .map(|build| format!(" (build {build})"))
            .unwrap_or_default();
        checks.add(
            "log",
            "Client log",
            if rejected {
                HealthStatus::Warn
            } else {
                HealthStatus::Ok
            },
            format!(
                "{} in {dir}; the newest run{build} {}",
                plural(runs, "run log"),
                describe_sign_in(last.as_ref())
            ),
        );
    }

    // The build the newest run's log names, from the head of the file alone.
    fn current_run_build(&self) -> Option<String> {
        let file = self.newest_client_log()?;
        let head = self.files.read_range(&file, 0, LOG_HEAD_BYTES).ok()?;
        parse_log_events(&String::from_utf8_lossy(&head))
            .into_iter()
            .find_map(|event| match event {
                LogEvent::Started { build, .. } => Some(build),
                _ => None,
            })
            .flatten()
    }
}
