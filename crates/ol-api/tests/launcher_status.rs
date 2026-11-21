#![forbid(unsafe_code)]

// The Launchers tab on fake launchers: each launcher's status and health, Locate with a right and a wrong program, and Reset. Nothing here writes a launcher's file or touches a process: the fake machine's log says so.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use futures_lite::future::block_on;
use ol_api::launchers::{
    BattlenetMachine, BattlenetTiming, EpicMachine, EpicTiming, Launchers, RiotMachine, RiotTiming,
    SteamTiming,
};
use ol_api::{ApiError, Backend};
use ol_model::{HealthStatus, InstallSource, LauncherId, LauncherStatus, SettingKey};
use ol_sim::FakeLaunchers;
use ol_testkit::{Scratch, oracle};
use ol_vault::FakeVault;
use serde_json::{Value, json};
use time::macros::datetime;

const VERSION: &str = "9.8.7";

type Heard = Arc<Mutex<Vec<(SettingKey, Option<String>)>>>;

struct Fakes {
    scratch: Scratch,
    fake: FakeLaunchers,
    backend: Backend,
}

impl Fakes {
    fn launchers_dir(&self) -> PathBuf {
        self.scratch.join("launchers")
    }

    fn statuses(&self) -> Vec<LauncherStatus> {
        block_on(self.backend.launcher_statuses()).unwrap()
    }

    fn status(&self, launcher: LauncherId) -> LauncherStatus {
        self.statuses()
            .into_iter()
            .find(|status| status.launcher == launcher)
            .unwrap()
    }

    fn setting(&self, key: SettingKey) -> Option<String> {
        block_on(self.backend.settings())
            .unwrap()
            .get(key)
            .map(str::to_owned)
    }
}

// The scenario all-four laid down, with `knobs` over it and `change` made to its files before the fake machine reads them.
fn fakes(knobs: Option<&Value>, change: impl FnOnce(&PathBuf)) -> Fakes {
    let scratch = Scratch::new("api-status");
    let launchers_dir = scratch.join("launchers");
    ol_sim::lay_down(
        &oracle("fixtures/dev-only/fake-launchers/all-four"),
        &oracle("fixtures/launchers"),
        &launchers_dir,
    )
    .unwrap();
    if let Some(knobs) = knobs {
        ol_sim::write_knobs(&launchers_dir, knobs).unwrap();
    }
    change(&launchers_dir);
    let fake = ol_sim::materialize(
        &launchers_dir,
        &scratch.join("data"),
        datetime!(2026-11-06 16:46:23 UTC),
    )
    .unwrap();
    let mut launchers = Launchers::on(fake.platform.clone(), Arc::new(FakeVault));
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
    launchers.riot = RiotMachine::at(
        RiotTiming::with(&fake.riot_timing),
        fake.riot_default_dir.clone(),
        fake.local_app_data.clone(),
        fake.program_data.clone(),
    );
    let backend = Backend::start_with(scratch.join("data"), VERSION, launchers, None);
    Fakes {
        scratch,
        fake,
        backend,
    }
}

fn refusal<T>(answer: Result<T, ApiError>) -> (&'static str, String) {
    let error = answer.err().expect("a refusal");
    (error.code, error.message)
}

fn ids(status: &LauncherStatus) -> Vec<&'static str> {
    status
        .health
        .as_ref()
        .map(|health| health.checks.iter().map(|check| check.id).collect())
        .unwrap_or_default()
}

fn check<'s>(status: &'s LauncherStatus, id: &str) -> (HealthStatus, &'s str) {
    let found = status
        .health
        .as_ref()
        .and_then(|health| health.checks.iter().find(|check| check.id == id))
        .unwrap_or_else(|| panic!("{:?} has no {id} check", status.launcher));
    (found.status, found.detail.as_str())
}

// Only reads: the fake machine's log holds no write, no start, no close and no end.
fn assert_only_read(fakes: &Fakes) {
    const ACTS: [&str; 8] = [
        "reg.set",
        "reg.delete",
        "file.write",
        "proc.start",
        "proc.close",
        "proc.kill",
        "shell.open",
        "riot.launch",
    ];
    for event in fakes.fake.log.snapshot() {
        let op = event["op"].as_str().unwrap_or_default();
        assert!(!ACTS.contains(&op), "the Launchers tab did {op}");
    }
}

#[test]
fn every_installed_launcher_is_found_with_its_checks_in_the_electron_apps_order() {
    let fakes = fakes(None, |launchers| {
        let listed = launchers.join("programdata/Epic/UnrealEngineLauncher/LauncherInstalled.dat");
        let text = fs::read_to_string(&listed).unwrap();
        let mut list: Value = serde_json::from_str(&text).unwrap();
        let entries = list["InstallationList"].as_array_mut().unwrap();
        let mut other = entries[0].clone();
        other["AppName"] = "Other".into();
        entries.push(other);
        fs::write(&listed, list.to_string()).unwrap();
    });
    let statuses = fakes.statuses();
    assert_eq!(
        statuses
            .iter()
            .map(|status| status.launcher)
            .collect::<Vec<_>>(),
        LauncherId::ALL
    );
    for status in &statuses {
        assert!(status.implemented);
        assert_eq!(status.attention, None, "{:?}", status.launcher);
        assert_eq!(status.chosen_path, None);
        assert_eq!(status.running, Some(false), "{:?}", status.launcher);
        let install = status.install.as_ref().expect("installed");
        assert_ne!(install.source, InstallSource::User);
        assert_eq!(status.location_note(), "Found automatically.");
        assert_ne!(status.health.as_ref().unwrap().status, HealthStatus::Absent);
        assert_eq!(
            check(status, "process").1,
            format!("{} is not running", status.launcher.exe_name())
        );
    }
    let [steam, epic, battlenet, riot] = statuses.as_slice() else {
        panic!("four launchers");
    };
    assert_eq!(
        ids(steam),
        [
            "install",
            "process",
            "loginusers",
            "most-recent-key",
            "registry",
            "libraries",
            "games",
            "backups"
        ]
    );
    assert_eq!(
        ids(epic),
        [
            "install",
            "process",
            "manifests",
            "launcher-installed",
            "settings",
            "log",
            "sessions",
            "backups"
        ]
    );
    assert_eq!(
        ids(battlenet),
        [
            "install",
            "process",
            "uninstall",
            "product-db",
            "aggregate",
            "config",
            "token",
            "prefs",
            "log",
            "sessions",
            "backups"
        ]
    );
    assert_eq!(
        ids(riot),
        [
            "install", "process", "installs", "products", "data", "log", "sessions", "backups"
        ]
    );
    assert_eq!(
        check(steam, "games"),
        (
            HealthStatus::Ok,
            "1 installed game (1 waiting for an update)"
        )
    );
    assert_eq!(
        check(steam, "backups"),
        (HealthStatus::Ok, "No snapshots yet")
    );
    assert_eq!(
        check(epic, "sessions"),
        (HealthStatus::Ok, "0 snapshots stored")
    );
    assert_eq!(
        check(epic, "launcher-installed"),
        (HealthStatus::Ok, "2 entries, agrees with the manifests")
    );
    let (_, detail) = check(battlenet, "token");
    assert!(
        detail.starts_with(
            "1 value under HKCU\\Software\\Blizzard Entertainment\\Battle.net\\UnifiedAuth; "
        ),
        "{detail}"
    );
    assert!(!detail.contains("(("), "{detail}");
    assert!(!detail.contains("ababababab"), "the token is in the detail");
    let (_, detail) = check(riot, "data");
    assert!(
        detail.contains("RiotGamesPrivateSettings.yaml holds a saved sign-in for Riot account …"),
        "{detail}"
    );
    assert_only_read(&fakes);
    fakes.backend.shut_down();
}

#[test]
fn a_running_launcher_a_missing_one_and_an_unhealthy_one_say_so() {
    let knobs = json!({ "processes": { "running": ["steam.exe", "RiotClientServices.exe"] } });
    let fakes = fakes(Some(&knobs), |launchers| {
        fs::remove_file(
            launchers.join("epic/Launcher/Portal/Binaries/Win64/EpicGamesLauncher.exe"),
        )
        .unwrap();
        fs::write(
            launchers.join("appdata/Battle.net/Battle.net.config"),
            "not json",
        )
        .unwrap();
    });
    let steam = fakes.status(LauncherId::Steam);
    assert_eq!(steam.running, Some(true));
    assert_eq!(check(&steam, "process").1, "steam.exe is running");
    let riot = fakes.status(LauncherId::Riot);
    assert_eq!(riot.running, Some(true));

    let epic = fakes.status(LauncherId::Epic);
    assert!(epic.install.is_none());
    assert_eq!(epic.running, None);
    assert_eq!(epic.attention, None);
    let health = epic.health.as_ref().unwrap();
    assert_eq!(health.status, HealthStatus::Absent);
    assert_eq!(ids(&epic), ["install"]);
    assert!(check(&epic, "install")
        .1
        .starts_with("Epic Games Launcher is not installed on this PC. Checked the Uninstall key, the com.epicgames.launcher URI handler, and "));
    assert_eq!(
        epic.location_note(),
        "Not found. If it is installed, point One Launch at EpicGamesLauncher.exe."
    );

    let battlenet = fakes.status(LauncherId::Battlenet);
    let (status, detail) = check(&battlenet, "config");
    assert_eq!(status, HealthStatus::Fail, "{detail}");
    assert!(detail.starts_with("Could not parse "), "{detail}");
    assert_eq!(
        battlenet.health.as_ref().unwrap().status,
        HealthStatus::Fail
    );
    assert_only_read(&fakes);
    fakes.backend.shut_down();
}

#[test]
fn locate_takes_the_launchers_own_program_and_reset_goes_back_to_detection() {
    let fakes = fakes(None, |launchers| {
        fs::create_dir_all(launchers.join("moved/Steam")).unwrap();
        fs::write(launchers.join("moved/Steam/steam.exe"), "").unwrap();
        fs::write(launchers.join("steam/other.exe"), "").unwrap();
    });
    let heard: Heard = Arc::default();
    let into = Arc::clone(&heard);
    fakes.backend.observe_settings(Arc::new(move |key, value| {
        into.lock().unwrap().push((key, value.map(str::to_owned)));
    }));
    let launchers = fakes.launchers_dir();
    let path = |relative: &str| {
        launchers
            .join(relative.replace('/', "\\"))
            .to_str()
            .unwrap()
            .to_owned()
    };
    let wrong = (
        "NOT_A_LAUNCHER",
        "That file is not Steam. Choose steam.exe in the folder Steam is installed in.".to_owned(),
    );
    for picked in [
        path("steam/other.exe"),
        path("steam/config/loginusers.vdf"),
        path("battlenet/Battle.net.exe"),
        path("nowhere/steam.exe"),
    ] {
        assert_eq!(
            refusal(block_on(
                fakes.backend.locate_launcher(LauncherId::Steam, &picked)
            )),
            wrong,
            "{picked}"
        );
    }
    assert_eq!(
        refusal(block_on(fakes.backend.locate_launcher(
            LauncherId::Riot,
            &path("steam/steam.exe")
        ))),
        (
            "NOT_A_LAUNCHER",
            "That file is not Riot Games. Choose RiotClientServices.exe in the folder Riot Games is installed in.".to_owned()
        )
    );
    assert_eq!(fakes.setting(SettingKey::SteamPath), None);
    assert!(heard.lock().unwrap().is_empty());

    let install = block_on(
        fakes
            .backend
            .locate_launcher(LauncherId::Steam, &path("moved/Steam/STEAM.EXE")),
    )
    .unwrap();
    let folder = path("moved/Steam");
    assert_eq!(install.install_dir, folder);
    assert_eq!(install.source, InstallSource::User);
    assert_eq!(fakes.setting(SettingKey::SteamPath), Some(folder.clone()));
    let steam = fakes.status(LauncherId::Steam);
    assert_eq!(steam.chosen_path, Some(folder.clone()));
    assert_eq!(steam.location_note(), "You chose this folder.");
    assert!(!ids(&steam).contains(&"chosen-folder"));

    let battlenet = block_on(fakes.backend.locate_launcher(
        LauncherId::Battlenet,
        &path("battlenet/Battle.net Launcher.exe"),
    ))
    .unwrap();
    assert_eq!(battlenet.install_dir, path("battlenet"));
    let epic = block_on(fakes.backend.locate_launcher(
        LauncherId::Epic,
        &path("epic/Launcher/Portal/Binaries/Win64/EpicGamesLauncher.exe"),
    ))
    .unwrap();
    assert_eq!(epic.install_dir, path("epic/Launcher"));

    fs::remove_file(launchers.join("moved/Steam/steam.exe")).unwrap();
    let steam = fakes.status(LauncherId::Steam);
    assert_eq!(
        steam.location_note(),
        "steam.exe is no longer in the folder you chose."
    );
    let (status, detail) = check(&steam, "chosen-folder");
    assert_eq!(status, HealthStatus::Warn);
    assert_eq!(
        detail,
        format!(
            "steam.exe is no longer in {folder}, the folder chosen in Settings, so the installation found automatically is used. Use Locate to choose it again, or Reset to go back to automatic detection."
        )
    );

    block_on(fakes.backend.reset_launcher_path(LauncherId::Steam)).unwrap();
    assert_eq!(fakes.setting(SettingKey::SteamPath), None);
    let steam = fakes.status(LauncherId::Steam);
    assert_eq!(steam.chosen_path, None);
    assert_eq!(steam.location_note(), "Found automatically.");
    assert_eq!(
        *heard.lock().unwrap(),
        [
            (SettingKey::SteamPath, Some(folder)),
            (SettingKey::BattlenetPath, Some(path("battlenet"))),
            (SettingKey::EpicPath, Some(path("epic/Launcher"))),
            (SettingKey::SteamPath, None),
        ]
    );
    assert_only_read(&fakes);
    fakes.backend.shut_down();
}

#[test]
fn without_launchers_every_row_is_not_implemented_and_locate_is_refused() {
    let scratch = Scratch::new("api-status");
    let backend = Backend::start(scratch.join("data"), VERSION);
    let statuses = block_on(backend.launcher_statuses()).unwrap();
    assert_eq!(statuses.len(), 4);
    for status in statuses {
        assert!(!status.implemented);
        assert!(status.health.is_none());
        assert_eq!(status.display_name, status.launcher.display_name());
    }
    assert_eq!(
        refusal(block_on(
            backend.locate_launcher(LauncherId::Steam, r"D:\x\steam.exe")
        ))
        .0,
        "NOT_IMPLEMENTED"
    );
    backend.shut_down();
}
