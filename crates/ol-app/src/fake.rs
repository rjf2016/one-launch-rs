// The app on fake launchers: a scenario folder under a scratch folder stands in for the PC's launchers, with a registry and a process table that exist only in this process. Nothing here can reach a real launcher.

use std::fs;
use std::path::Path;

use anyhow::{Context as _, bail};
use ol_api::launchers::{NewAccount, Secret, SteamTiming};
use ol_api::{ApiError, Backend};
use ol_model::SettingKey;
use ol_sim::Seed;

use crate::args::FakeMachine;

const SCENARIO: &str = "scenario.json";
const STEAM_PROGRAM: &str = r"steam\steam.exe";

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
    // The fake machine writes beside this program. A scenario's is an empty file; one with content is a real launcher's.
    let real_program =
        fs::metadata(folder.join(STEAM_PROGRAM)).is_ok_and(|program| program.len() > 0);
    if real_program {
        bail!("--fake-launchers names a folder with a real {STEAM_PROGRAM} in it");
    }
    let fake = ol_sim::materialize_in_real_time(folder, data_dir)
        .context("--fake-launchers does not hold a scenario that reads")?;
    let mut launchers = ol_api::launchers::on_the_stand_in_vault(fake.platform.clone());
    launchers.steam_timing = SteamTiming::with(&fake.steam_timing);
    launchers
        .steam_default_dir
        .clone_from(&fake.steam_default_dir);
    let backend = Backend::start_with(data_dir.to_path_buf(), version, launchers, None);
    Ok((backend, fake.seed))
}

// What the Electron app does once for a data folder that holds no account: the scenario's accounts and settings.
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
    for (key, value) in seed.settings {
        if let Some(key) = SettingKey::parse(&key) {
            backend.set_setting(key, value).await?;
        }
    }
    Ok(())
}
