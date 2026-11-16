// The app on fake launchers: a scenario folder under a scratch folder stands in for the PC's launchers, with a registry and a process table that exist only in this process. Nothing here can reach a real launcher.

use std::fs;
use std::path::Path;

use anyhow::{Context as _, bail};
use ol_api::launchers::{
    BattlenetMachine, BattlenetTiming, EpicMachine, EpicTiming, NewAccount, Secret, SteamTiming,
};
use ol_api::{ApiError, Backend};
use ol_model::SettingKey;
use ol_sim::Seed;

use crate::args::FakeMachine;

const SCENARIO: &str = "scenario.json";
// The launchers a fake machine starts and writes beside.
const PROGRAMS: [&str; 4] = [
    r"steam\steam.exe",
    r"epic\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe",
    r"battlenet\Battle.net.exe",
    r"battlenet\Battle.net Launcher.exe",
];
// What a laid-down scenario holds at its top: its two files, and the folders its launchers and their files live in.
const SCENARIO_ENTRIES: [&str; 11] = [
    SCENARIO,
    "knobs.json",
    "steam",
    "epic",
    "battlenet",
    "riot",
    "localappdata",
    "appdata",
    "programdata",
    "games",
    "nowhere",
];

pub fn start(
    data_dir: &Path,
    machine: &FakeMachine,
    version: &str,
) -> anyhow::Result<(Backend, Seed)> {
    let folder = &machine.folder;
    let scenario = folder.join(SCENARIO);
    if let Some((from, fixtures)) = &machine.lay_down
        && !scenario.exists()
    {
        // A scenario's files carry the names of a launcher's own, so it is written only where nothing is.
        let holds_something =
            fs::read_dir(folder).is_ok_and(|mut entries| entries.next().is_some());
        if holds_something {
            bail!(
                "--fake-launchers names a folder that already holds something; a scenario is laid down only in an empty or a new folder"
            );
        }
        ol_sim::lay_down(from, fixtures, folder)
            .context("--fake-scenario could not be laid down")?;
    }
    if !scenario.is_file() {
        bail!(
            "--fake-launchers names a folder without {SCENARIO}; lay one down with --fake-scenario=<folder> and --fake-fixtures=<folder>"
        );
    }
    // The fake machine may write anywhere under this folder, so it has to be one a scenario made: nothing in it but a scenario's own folders.
    for entry in fs::read_dir(folder).context("--fake-launchers could not be listed")? {
        let name = entry?.file_name().to_string_lossy().to_lowercase();
        if !SCENARIO_ENTRIES.contains(&name.as_str()) {
            bail!(
                "--fake-launchers names a folder that holds more than a scenario; give the fake launchers a folder of their own"
            );
        }
    }
    // The fake machine writes beside these programs. A scenario's is an empty file; one with content is a real launcher's.
    for program in PROGRAMS {
        if fs::metadata(folder.join(program)).is_ok_and(|program| program.len() > 0) {
            bail!("--fake-launchers names a folder with a real {program} in it");
        }
    }
    let fake = ol_sim::materialize_in_real_time(folder, data_dir)
        .context("--fake-launchers does not hold a scenario that reads")?;
    let mut launchers = ol_api::launchers::on_the_stand_in_vault(fake.platform.clone());
    launchers.steam_timing = SteamTiming::with(&fake.steam_timing);
    launchers
        .steam_default_dir
        .clone_from(&fake.steam_default_dir);
    launchers.epic = EpicMachine {
        timing: EpicTiming::with(&fake.epic_timing),
        default_dir: fake.epic_default_dir.clone(),
        local_app_data: fake.local_app_data.clone(),
        program_data: fake.program_data.clone(),
    };
    launchers.battlenet = BattlenetMachine {
        timing: BattlenetTiming::with(&fake.battlenet_timing),
        default_dir: fake.battlenet_default_dir.clone(),
        app_data: fake.app_data.clone(),
        local_app_data: fake.local_app_data.clone(),
        program_data: fake.program_data.clone(),
    };
    let backend = Backend::start_with(data_dir.to_path_buf(), version, launchers, None);
    Ok((backend, fake.seed))
}

// What the Electron app does once for a data folder that holds no account: the scenario's accounts, the sign-ins it saves for them, made-up ones, and its settings.
pub async fn seed(backend: &Backend, seed: Seed) -> Result<(), ApiError> {
    if !backend.accounts().await?.is_empty() {
        return Ok(());
    }
    for account in seed.accounts {
        backend
            .create_account(NewAccount {
                launcher: account.launcher,
                display_name: account.display_name,
                login: account.login,
                launcher_uid: account.launcher_uid,
                password: account.password.map(Secret::new),
                tags: Vec::new(),
                notes: String::new(),
            })
            .await?;
    }
    let accounts = backend.accounts().await?;
    for session in seed.sessions {
        let owner = accounts
            .iter()
            .find(|account| account.launcher == session.launcher && account.login == session.login);
        if let Some(owner) = owner {
            backend
                .seed_session(
                    &owner.id,
                    Some(session.launcher_uid),
                    Secret::new(session.blob),
                )
                .await?;
        }
    }
    for (key, value) in seed.settings {
        if let Some(key) = SettingKey::parse(&key) {
            backend.set_setting(key, value).await?;
        }
    }
    Ok(())
}
