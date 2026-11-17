use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;
use std::sync::Arc;

use ol_formats::battlenet::session as battlenet_session;
use ol_formats::epic::session::{parse_blob, serialize_blob, session_blob_from_ini};
use ol_formats::riot::session::{self as riot_session, RiotSessionBlob};
use ol_formats::winpath::join;
use ol_model::LauncherId;
use ol_platform_api::{Platform, ProcessInfo};
use ol_platform_fake::{
    FakeClock, FakeProcesses, FakeRegistry, FakeRiotLaunch, FakeShell, OpLog, ProcessKnobs,
    RegistryValue,
};
use ol_runtime::TimingOverrides;
use serde_json::{Map, Value};
use thiserror::Error;
use time::OffsetDateTime;

use crate::battlenet::{BattlenetFolders, BattlenetKnobs, BattlenetScript, BattlenetSim};
use crate::epic::{EpicKnobs, EpicSim};
use crate::files::ContainedFiles;
use crate::riot::{RiotFolders, RiotKnobs, RiotSim, fake_session_file};
use crate::steam::{SteamKnobs, SteamSim};

const SCENARIO_FILE: &str = "scenario.json";
const KNOBS_FILE: &str = "knobs.json";
// The sessions the scripted Epic launcher made up, by fingerprint, with the account each signs in as. None of their data.
const ISSUED_SESSIONS_FILE: &str = "issued-sessions.json";
const FIXTURE_PREFIX: &str = "@fixtures/";
const UNIFIED_AUTH: &str = r"Software\Blizzard Entertainment\Battle.net\UnifiedAuth";
const BINARY_PREFIX: &str = "binary:";

// A person watching cannot quit a fake game, and one that runs refuses the next Play: it quits by itself after this long unless the knobs say otherwise.
const WATCHED_PLAY_MS: u64 = 20_000;
// Waits short enough that a fake switch is over in a few seconds of its own clock.
const SIM_TIMING: [(&str, u64); 17] = [
    ("pollMs", 100),
    ("logPollMs", 100),
    ("processPollMs", 100),
    ("shutdownMs", 3_000),
    ("killGraceMs", 1_000),
    ("restartWatchMs", 500),
    ("cleanExitWatchMs", 200),
    ("restartSettleMs", 2_000),
    ("captureMs", 3_000),
    ("startupMs", 5_000),
    ("signInMs", 8_000),
    ("guardMs", 8_000),
    ("updateMs", 20_000),
    ("manualSignInMs", 30_000),
    ("readyGraceMs", 300),
    ("launchConfirmMs", 2_000),
    ("playConfirmMs", 2_000),
];

#[derive(Debug, Error)]
pub enum ScenarioError {
    #[error("the scenario folder could not be read or written: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0} is not JSON: {1}")]
    Json(&'static str, serde_json::Error),
    #[error("scenario.json: {0}")]
    Shape(String),
}

// An account a scenario puts into a fresh data folder. No Debug: it can hold a password, a made-up one.
#[derive(Clone)]
pub struct SeedAccount {
    pub launcher: LauncherId,
    pub display_name: String,
    pub login: String,
    pub launcher_uid: Option<String>,
    pub password: Option<String>,
}

// A sign-in a scenario saves for one of its accounts, a made-up one. No Debug: `blob` is what would be encrypted and stored.
#[derive(Clone)]
pub struct SeedSession {
    pub launcher: LauncherId,
    pub login: String,
    pub launcher_uid: String,
    pub blob: String,
}

#[derive(Clone, Default)]
pub struct Seed {
    pub settings: Vec<(String, String)>,
    pub accounts: Vec<SeedAccount>,
    pub sessions: Vec<SeedSession>,
}

// A scenario folder turned into the machine the adapters run on.
pub struct FakeLaunchers {
    pub platform: Platform,
    pub registry: Arc<FakeRegistry>,
    pub processes: Arc<FakeProcesses>,
    pub clock: Arc<FakeClock>,
    pub log: OpLog,
    pub steam: Option<Arc<SteamSim>>,
    pub steam_timing: TimingOverrides,
    // Where Steam is looked for last: a folder of the scenario that holds nothing.
    pub steam_default_dir: String,
    pub epic: Option<Arc<EpicSim>>,
    pub epic_timing: TimingOverrides,
    pub epic_default_dir: String,
    pub battlenet: Option<Arc<BattlenetSim>>,
    pub battlenet_timing: TimingOverrides,
    pub battlenet_default_dir: String,
    pub riot: Option<Arc<RiotSim>>,
    pub riot_timing: TimingOverrides,
    pub riot_default_dir: String,
    // Where a launch request for the Riot Client goes on this machine: to the scripted client, as a press of its Play button.
    pub riot_launch: Arc<FakeRiotLaunch>,
    // The scenario's stand-ins for %APPDATA%, %LOCALAPPDATA% and %ProgramData%.
    pub app_data: String,
    pub local_app_data: String,
    pub program_data: String,
    pub seed: Seed,
}

fn shape(message: impl Into<String>) -> ScenarioError {
    ScenarioError::Shape(message.into())
}

fn text_path(path: &Path) -> Result<String, ScenarioError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| shape("the folder's path is not Unicode"))
}

fn read_json(file: &Path, name: &'static str) -> Result<Value, ScenarioError> {
    serde_json::from_slice(&fs::read(file)?).map_err(|error| ScenarioError::Json(name, error))
}

// Copies a scenario into `target`: scenario.json as it is, and every entry of its `files`. "@fixtures/<path>" is a file under `fixtures`, {"inline": text} is written with {root} and {root:vdf} filled in, and "" is an empty file.
pub fn lay_down(scenario_dir: &Path, fixtures: &Path, target: &Path) -> Result<(), ScenarioError> {
    let source = scenario_dir.join(SCENARIO_FILE);
    let scenario = read_json(&source, SCENARIO_FILE)?;
    fs::create_dir_all(target)?;
    let root = text_path(target)?;
    let root_vdf = root.replace('\\', "\\\\");
    let no_files = Map::new();
    let files = scenario
        .get("files")
        .and_then(Value::as_object)
        .unwrap_or(&no_files);
    for (relative, spec) in files {
        let climbs = relative
            .split(['/', '\\'])
            .any(|part| part == ".." || part.contains(':'));
        if climbs || relative.starts_with(['/', '\\']) {
            return Err(shape(format!("a file leaves the folder: {relative}")));
        }
        let file = target.join(relative);
        if let Some(folder) = file.parent() {
            fs::create_dir_all(folder)?;
        }
        match (spec.as_str(), spec.get("inline").and_then(Value::as_str)) {
            (Some(fixture), _) if fixture.starts_with(FIXTURE_PREFIX) => {
                fs::copy(fixtures.join(&fixture[FIXTURE_PREFIX.len()..]), &file)?;
            }
            (Some(content), _) => fs::write(&file, content)?,
            (None, Some(inline)) => fs::write(
                &file,
                inline
                    .replace("{root:vdf}", &root_vdf)
                    .replace("{root}", &root),
            )?,
            (None, None) => return Err(shape(format!("unknown file spec for {relative}"))),
        }
    }
    fs::copy(source, target.join(SCENARIO_FILE))?;
    Ok(())
}

// What the next start on this folder reads over the scenario's own knobs, process table and timing.
pub fn write_knobs(target: &Path, knobs: &Value) -> Result<(), ScenarioError> {
    fs::write(target.join(KNOBS_FILE), knobs.to_string())?;
    Ok(())
}

fn merged<'a>(base: &'a Value, over: &'a Value, path: &[&str]) -> Map<String, Value> {
    let at = |root: &'a Value| path.iter().try_fold(root, |value, key| value.get(*key));
    let mut fields = Map::new();
    for source in [at(base), at(over)].into_iter().flatten() {
        if let Some(object) = source.as_object() {
            for (key, value) in object {
                fields.insert(key.clone(), value.clone());
            }
        }
    }
    fields
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn process_knobs(fields: &Map<String, Value>) -> ProcessKnobs {
    let table = fields
        .get("table")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|row| ProcessInfo {
                    name: row["name"].as_str().unwrap_or_default().to_owned(),
                    pid: row["pid"]
                        .as_u64()
                        .and_then(|pid| u32::try_from(pid).ok())
                        .unwrap_or(0),
                    path: row["path"].as_str().map(str::to_owned),
                })
                .collect()
        })
        .unwrap_or_default();
    let reappears = fields
        .get("reappearsAfterPolls")
        .and_then(Value::as_object)
        .map(|rules| {
            rules
                .iter()
                .filter_map(|(image, after)| {
                    Some((image.clone(), u32::try_from(after.as_u64()?).ok()?))
                })
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    ProcessKnobs {
        running: strings(fields.get("running")),
        table,
        shutdown_polls: fields
            .get("shutdownPolls")
            .and_then(Value::as_u64)
            .map(|polls| u32::try_from(polls).unwrap_or(u32::MAX)),
        start_fails: strings(fields.get("startFails")),
        elevated: strings(fields.get("elevated")),
        reappears_after_polls: reappears,
    }
}

fn steam_knobs(fields: &Map<String, Value>) -> SteamKnobs {
    let number = |name: &str| fields.get(name).and_then(Value::as_u64);
    SteamKnobs {
        expired_sessions: strings(fields.get("expiredSessions")),
        wrong_password: fields.get("wrongPassword").and_then(Value::as_bool) == Some(true),
        login_verdict: fields
            .get("loginVerdict")
            .and_then(Value::as_str)
            .map(str::to_owned),
        self_update_ms: fields.get("selfUpdateMs").and_then(Value::as_i64),
        react_ms: number("reactMs").unwrap_or(0),
        sign_in_when_asked_ms: number("signInWhenAskedMs"),
        sign_in_as: number("signInAs").and_then(|id| u32::try_from(id).ok()),
        running_as: fields
            .get("runningAs")
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
}

fn epic_knobs(fields: &Map<String, Value>) -> EpicKnobs {
    let text = |name: &str| fields.get(name).and_then(Value::as_str).map(str::to_owned);
    EpicKnobs {
        expired: strings(fields.get("expired")),
        rotate_on_start: fields.get("rotateOnStart").and_then(Value::as_bool) == Some(true),
        sign_in_error: text("signInError"),
        react_ms: fields.get("reactMs").and_then(Value::as_u64).unwrap_or(0),
        sign_in_when_asked_ms: fields.get("signInWhenAskedMs").and_then(Value::as_u64),
        sign_in_as: text("signInAs"),
        running_as: text("runningAs"),
    }
}

fn battlenet_knobs(fields: &Map<String, Value>) -> BattlenetKnobs {
    let text = |name: &str| fields.get(name).and_then(Value::as_str).map(str::to_owned);
    BattlenetKnobs {
        rejected: strings(fields.get("rejected")),
        rotate_on_sign_in: fields.get("rotateOnSignIn").and_then(Value::as_bool) == Some(true),
        react_ms: fields.get("reactMs").and_then(Value::as_u64).unwrap_or(0),
        sign_in_when_asked_ms: fields.get("signInWhenAskedMs").and_then(Value::as_u64),
        sign_in_as: text("signInAs"),
        running_as: text("runningAs"),
        updates_pending: strings(fields.get("updatesPending")),
        games_running: strings(fields.get("gamesRunning")),
        game_quits_after_ms: fields.get("gameQuitsAfterMs").and_then(Value::as_u64),
    }
}

fn riot_knobs(fields: &Map<String, Value>) -> RiotKnobs {
    let text = |name: &str| fields.get(name).and_then(Value::as_str).map(str::to_owned);
    let flag = |name: &str| fields.get(name).and_then(Value::as_bool);
    let default = RiotKnobs::default();
    RiotKnobs {
        rejected: strings(fields.get("rejected")),
        rotate_on_start: flag("rotateOnStart").unwrap_or(default.rotate_on_start),
        auth_error: text("authError"),
        react_ms: fields.get("reactMs").and_then(Value::as_u64).unwrap_or(0),
        sign_in_when_asked_ms: fields.get("signInWhenAskedMs").and_then(Value::as_u64),
        sign_in_as: text("signInAs"),
        stay_signed_in: flag("staySignedIn").unwrap_or(default.stay_signed_in),
        running_as: text("runningAs"),
        updates_pending: strings(fields.get("updatesPending")),
        direct_launch: flag("directLaunch") == Some(true),
        press_play_fails: flag("pressPlayFails") == Some(true),
        game_quits_after_ms: fields.get("gameQuitsAfterMs").and_then(Value::as_u64),
    }
}

// The sign-ins a scenario saves for its accounts, each with what its launcher's servers know it by: Epic's session data, Battle.net's token. Riot's is a whole session file, built around the account id and the refresh token the scenario gives; the scripted client reads the account off the file itself.
fn seeded_sessions(scenario: &Value) -> Vec<(String, SeedSession)> {
    let sessions = scenario["seed"]["sessions"].as_array();
    sessions
        .into_iter()
        .flatten()
        .filter_map(|session| {
            let text = |name: &str| session[name].as_str().map(str::to_owned);
            let launcher = LauncherId::parse(session["launcher"].as_str()?)?;
            let given = session["blob"].to_string();
            let (known_by, blob) = match launcher {
                LauncherId::Epic => {
                    let blob = parse_blob(&given).ok()?;
                    (blob.data.clone(), serialize_blob(&blob))
                }
                LauncherId::Battlenet => {
                    let blob = battlenet_session::parse_blob(&given).ok()?;
                    let token = blob.unified_auth.hex.clone();
                    (token, battlenet_session::serialize_blob(&blob))
                }
                LauncherId::Riot => {
                    let refresh_token = session["refreshToken"].as_str()?;
                    let file = fake_session_file(&text("launcherUid")?, refresh_token);
                    let blob = riot_session::serialize_blob(&RiotSessionBlob { file });
                    (refresh_token.to_owned(), blob)
                }
                LauncherId::Steam => return None,
            };
            let seeded = SeedSession {
                launcher,
                login: text("login")?,
                launcher_uid: text("launcherUid")?,
                blob,
            };
            Some((known_by, seeded))
        })
        .collect()
}

// Which account each sign-in of one launcher signs in as.
fn servers(scenario: &Value, launcher: LauncherId) -> HashMap<String, String> {
    seeded_sessions(scenario)
        .into_iter()
        .filter(|(_, session)| session.launcher == launcher)
        .map(|(known_by, session)| (known_by, session.launcher_uid))
        .collect()
}

// What the scripted Battle.net client is told: the token in the scenario's registry signs in as the account the scenario names, each saved token as its own account, and each account with the login the scenario gives it.
fn battlenet_script(scenario: &Value, knobs: BattlenetKnobs) -> BattlenetScript {
    let on_disk = scenario["onDisk"]["battlenet"].as_str().map(str::to_owned);
    let mut servers = HashMap::new();
    if let (Some(uid), Some(values)) = (&on_disk, scenario["registry"].as_object()) {
        for (path, value) in values {
            let token = value
                .as_str()
                .and_then(|text| text.strip_prefix(BINARY_PREFIX));
            if let (true, Some(token)) = (path.contains(UNIFIED_AUTH), token) {
                servers.insert(token.to_lowercase(), uid.clone());
            }
        }
    }
    servers.extend(self::servers(scenario, LauncherId::Battlenet));
    let accounts = scenario["seed"]["accounts"].as_array();
    let logins = accounts
        .into_iter()
        .flatten()
        .filter(|account| account["launcher"] == "battlenet")
        .filter_map(|account| {
            let text = |name: &str| account[name].as_str().map(str::to_owned);
            Some((text("launcherUid")?, text("login")?))
        })
        .collect();
    BattlenetScript {
        servers,
        logins,
        on_disk,
        knobs,
    }
}

fn timing(scenario: &Value, overrides: &Value, launcher: &str) -> TimingOverrides {
    let mut timing = TimingOverrides::default();
    for (name, milliseconds) in SIM_TIMING {
        timing.set(name, milliseconds);
    }
    for (name, value) in merged(scenario, overrides, &["timing", launcher]) {
        if let Some(milliseconds) = value.as_u64() {
            timing.set(&name, milliseconds);
        }
    }
    timing
}

fn seed(scenario: &Value) -> Seed {
    let seed = &scenario["seed"];
    let settings = seed["settings"]
        .as_object()
        .map(|settings| {
            settings
                .iter()
                .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    let accounts = seed["accounts"]
        .as_array()
        .map(|accounts| {
            accounts
                .iter()
                .filter_map(|account| {
                    let text = |name: &str| account[name].as_str().map(str::to_owned);
                    Some(SeedAccount {
                        launcher: LauncherId::parse(account["launcher"].as_str()?)?,
                        display_name: text("displayName")?,
                        login: text("login")?,
                        launcher_uid: text("launcherUid"),
                        password: text("password"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Seed {
        settings,
        accounts,
        sessions: seeded_sessions(scenario)
            .into_iter()
            .map(|(_, session)| session)
            .collect(),
    }
}

// Builds the fake machine for a folder `lay_down` made: an in-memory registry and process table with the launcher simulators, a file port that sees only `dir` and the snapshots under `data_dir`, and a clock that starts at `now`.
pub fn materialize(
    dir: &Path,
    data_dir: &Path,
    now: OffsetDateTime,
) -> Result<FakeLaunchers, ScenarioError> {
    materialize_on(dir, data_dir, FakeClock::starting_at(now), None)
}

// As `materialize`, for a person to watch: the clock is the system's, so a switch takes the seconds its waits add up to, and a game the scripted clients start quits by itself after a while.
pub fn materialize_in_real_time(
    dir: &Path,
    data_dir: &Path,
) -> Result<FakeLaunchers, ScenarioError> {
    materialize_on(
        dir,
        data_dir,
        FakeClock::following_the_system(),
        Some(WATCHED_PLAY_MS),
    )
}

fn materialize_on(
    dir: &Path,
    data_dir: &Path,
    clock: FakeClock,
    play_ms: Option<u64>,
) -> Result<FakeLaunchers, ScenarioError> {
    let scenario = read_json(&dir.join(SCENARIO_FILE), SCENARIO_FILE)?;
    if scenario["version"] != 1 {
        return Err(shape("version must be 1"));
    }
    let installed: Vec<LauncherId> = scenario["installed"]
        .as_array()
        .and_then(|ids| {
            ids.iter()
                .map(|id| LauncherId::parse(id.as_str()?))
                .collect()
        })
        .ok_or_else(|| shape("\"installed\" must be launcher ids"))?;
    let knobs_file = dir.join(KNOBS_FILE);
    let overrides = if knobs_file.exists() {
        read_json(&knobs_file, KNOBS_FILE)?
    } else {
        Value::Null
    };

    let root = text_path(dir)?;
    let steam_dir = join(&[&root, "steam"]);
    let app_data = join(&[&root, "appdata"]);
    let local_app_data = join(&[&root, "localappdata"]);
    let program_data = join(&[&root, "programdata"]);
    let games_dir = join(&[&root, "games"]);
    let log = OpLog::new();
    let registry = Arc::new(FakeRegistry::new(log.clone()));
    if installed.contains(&LauncherId::Steam) {
        SteamSim::seed_registry(&registry, &steam_dir);
    }
    if installed.contains(&LauncherId::Epic) {
        EpicSim::seed_registry(&registry, &join(&[&root, "epic"]), &program_data);
    }
    if installed.contains(&LauncherId::Battlenet) {
        BattlenetSim::seed_registry(&registry, &join(&[&root, "battlenet"]), &games_dir);
    }
    let riot_dir = join(&[&root, "riot"]);
    let riot_folders = RiotFolders {
        local_app_data: &local_app_data,
        program_data: &program_data,
        install_dir: &riot_dir,
        games: &games_dir,
    };
    if installed.contains(&LauncherId::Riot) {
        RiotSim::seed_registry(&registry, &riot_folders);
    }
    if let Some(values) = scenario["registry"].as_object() {
        for (path, value) in values {
            let value = match (value.as_str(), value.as_u64()) {
                (Some(text), _) => match text.strip_prefix(BINARY_PREFIX) {
                    Some(hex) => RegistryValue::Binary(hex.to_owned()),
                    None => RegistryValue::Text(text.to_owned()),
                },
                (None, Some(number)) => RegistryValue::Dword(
                    u32::try_from(number).map_err(|_| shape(format!("{path} is no DWORD")))?,
                ),
                (None, None) => return Err(shape(format!("{path} is neither text nor a number"))),
            };
            registry.seed(path, value);
        }
    }

    let processes = Arc::new(FakeProcesses::new(
        log.clone(),
        process_knobs(&merged(&scenario, &overrides, &["processes"])),
    ));
    let clock = Arc::new(clock);
    let steam = installed.contains(&LauncherId::Steam).then(|| {
        SteamSim::attach(
            &steam_dir,
            Arc::clone(&registry),
            &processes,
            Arc::clone(&clock),
            steam_knobs(&merged(&scenario, &overrides, &["knobs", "steam"])),
        )
    });

    let epic = installed.contains(&LauncherId::Epic).then(|| {
        // The session the settings file holds signs in as the account the scenario names; each seeded one as its own.
        let mut servers = HashMap::new();
        let on_disk = fs::read(EpicSim::settings_file(&local_app_data))
            .ok()
            .and_then(|bytes| session_blob_from_ini(&String::from_utf8_lossy(&bytes)));
        if let (Some(uid), Some(blob)) = (scenario["onDisk"]["epic"].as_str(), on_disk) {
            servers.insert(blob.data.clone(), uid.to_owned());
        }
        servers.extend(self::servers(&scenario, LauncherId::Epic));
        EpicSim::attach(
            &local_app_data,
            dir.join("epic").join(ISSUED_SESSIONS_FILE),
            &processes,
            Arc::clone(&clock),
            servers,
            epic_knobs(&merged(&scenario, &overrides, &["knobs", "epic"])),
        )
    });

    let battlenet = installed.contains(&LauncherId::Battlenet).then(|| {
        let mut knobs = battlenet_knobs(&merged(&scenario, &overrides, &["knobs", "battlenet"]));
        knobs.game_quits_after_ms = knobs.game_quits_after_ms.or(play_ms);
        BattlenetSim::attach(
            &BattlenetFolders {
                app_data: &app_data,
                local_app_data: &local_app_data,
                program_data: &program_data,
                games: &games_dir,
            },
            Arc::clone(&registry),
            &processes,
            Arc::clone(&clock),
            battlenet_script(&scenario, knobs),
        )
    });

    let riot_launch = Arc::new(FakeRiotLaunch::new(log.clone()));
    let riot = installed.contains(&LauncherId::Riot).then(|| {
        let mut knobs = riot_knobs(&merged(&scenario, &overrides, &["knobs", "riot"]));
        knobs.game_quits_after_ms = knobs.game_quits_after_ms.or(play_ms);
        let sim = RiotSim::attach(&riot_folders, &processes, Arc::clone(&clock), knobs);
        let plays = Arc::clone(&sim);
        riot_launch.on_launch(move |id| plays.press_play(id));
        sim
    });

    // Of the data folder only the snapshots: a folder a setting names beside them, such as a real launcher's, stays out of reach.
    let snapshots = data_dir.join("backups");
    let roots: [&Path; 2] = [dir, &snapshots];
    let platform = Platform {
        registry: Arc::clone(&registry) as _,
        processes: Arc::clone(&processes) as _,
        files: Arc::new(ContainedFiles::new(&roots, log.clone())),
        shell: Arc::new(FakeShell::new(log.clone())),
        clock: Arc::clone(&clock) as _,
    };
    Ok(FakeLaunchers {
        platform,
        registry,
        processes,
        clock,
        log,
        steam,
        steam_timing: timing(&scenario, &overrides, "steam"),
        steam_default_dir: join(&[&root, "nowhere"]),
        epic,
        epic_timing: timing(&scenario, &overrides, "epic"),
        epic_default_dir: join(&[&root, "nowhere"]),
        battlenet,
        battlenet_timing: timing(&scenario, &overrides, "battlenet"),
        battlenet_default_dir: join(&[&root, "nowhere"]),
        riot,
        riot_timing: timing(&scenario, &overrides, "riot"),
        riot_default_dir: join(&[&root, "nowhere"]),
        riot_launch,
        app_data,
        local_app_data,
        program_data,
        seed: seed(&scenario),
    })
}

#[cfg(test)]
mod tests {
    use ol_testkit::{Scratch, oracle};
    use time::macros::datetime;

    use super::*;

    #[test]
    fn the_file_port_reaches_the_scenario_and_the_snapshots_and_nothing_else_in_the_data_folder() {
        let scratch = Scratch::new("sim-roots");
        let (dir, data) = (scratch.join("launchers"), scratch.join("data"));
        lay_down(
            &oracle("fixtures/dev-only/fake-launchers/all-four"),
            &oracle("fixtures/launchers"),
            &dir,
        )
        .unwrap();
        let snapshot = data.join("backups").join("steam").join("manifest.json");
        let beside = data.join("Steam").join("steam.exe");
        for file in [&snapshot, &beside] {
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, b"").unwrap();
        }
        let fake = materialize(&dir, &data, datetime!(2026-11-06 16:46:23 UTC)).unwrap();
        let files = &fake.platform.files;
        let text = |path: &Path| path.to_string_lossy().into_owned();

        assert!(files.exists(&text(&dir.join("steam").join("steam.exe"))));
        assert!(files.exists(&text(&snapshot)));
        assert!(!files.exists(&text(&beside)));
        let login_users = text(&data.join("Steam").join("config").join("loginusers.vdf"));
        assert!(files.write_atomic(&[(&login_users, b"x")]).is_err());
        assert!(!Path::new(&login_users).exists());
    }

    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "std makes no junction, and a junction is what a test of a junction needs"
    )]
    fn a_junction_in_the_scenario_folder_does_not_lead_the_file_port_out_of_it() {
        let scratch = Scratch::new("sim-junction");
        let (dir, data) = (scratch.join("launchers"), scratch.join("data"));
        lay_down(
            &oracle("fixtures/dev-only/fake-launchers/all-four"),
            &oracle("fixtures/launchers"),
            &dir,
        )
        .unwrap();
        let beside = data.join("Steam").join("config");
        fs::create_dir_all(&beside).unwrap();
        fs::write(beside.join("loginusers.vdf"), b"old").unwrap();
        let link = dir.join("linked");
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&beside)
            .output()
            .unwrap();
        assert!(made.status.success(), "mklink /J failed");
        let fake = materialize(&dir, &data, datetime!(2026-11-06 16:46:23 UTC));
        let files = fake.as_ref().map(|fake| &fake.platform.files);
        let through = link.join("loginusers.vdf").to_string_lossy().into_owned();
        let read = files.map(|files| files.read(&through).is_err());
        let written = files.map(|files| files.write_atomic(&[(&through, b"new")]).is_err());
        let kept = fs::read(beside.join("loginusers.vdf")).unwrap();
        fs::remove_dir(&link).unwrap();

        assert!(read.unwrap());
        assert!(written.unwrap());
        assert_eq!(kept, b"old");
    }

    #[test]
    fn a_session_the_epic_launcher_made_up_signs_in_as_its_own_account_after_a_restart() {
        let scratch = Scratch::new("sim-issued");
        let (dir, data) = (scratch.join("launchers"), scratch.join("data"));
        lay_down(
            &oracle("fixtures/dev-only/fake-launchers/all-four"),
            &oracle("fixtures/launchers"),
            &dir,
        )
        .unwrap();
        write_knobs(
            &dir,
            &serde_json::json!({ "knobs": { "epic": { "reactMs": 0 } } }),
        )
        .unwrap();
        let scenario = read_json(&dir.join(SCENARIO_FILE), SCENARIO_FILE).unwrap();
        let on_disk = scenario["onDisk"]["epic"].as_str().unwrap();
        let accounts = scenario["seed"]["accounts"].as_array().unwrap();
        let other = accounts
            .iter()
            .filter(|account| account["launcher"] == "epic")
            .filter_map(|account| account["launcherUid"].as_str())
            .find(|uid| *uid != on_disk)
            .unwrap();
        let at = datetime!(2026-11-06 16:46:23 UTC);
        let fake = materialize(&dir, &data, at).unwrap();
        fake.epic.as_ref().unwrap().sign_in_by_hand(Some(other));
        drop(fake);

        let fake = materialize(&dir, &data, at).unwrap();
        let exe = dir.join(r"epic\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe");
        let exe = exe.to_string_lossy().into_owned();
        fake.platform.processes.start(&exe, &[], "").unwrap();
        let log = dir.join(r"localappdata\EpicGamesLauncher\Saved\Logs\EpicGamesLauncher.log");
        let log = fs::read_to_string(log).unwrap();
        assert!(log.contains(&format!("OnLoginComplete - 0 TRUE {other} ")));
        assert!(!log.contains(&format!("OnLoginComplete - 0 TRUE {on_disk} ")));
    }

    const BATTLENET_EXE: &str = "Battle.net.exe";

    fn battlenet_scenario(name: &str, knobs: &Value) -> (Scratch, std::path::PathBuf) {
        let scratch = Scratch::new(name);
        let dir = scratch.join("launchers");
        lay_down(
            &oracle("fixtures/dev-only/fake-launchers/all-four"),
            &oracle("fixtures/launchers"),
            &dir,
        )
        .unwrap();
        write_knobs(&dir, knobs).unwrap();
        (scratch, dir)
    }

    fn newest_battlenet_log(dir: &Path) -> String {
        let logs = dir.join(r"localappdata\Battle.net\Logs");
        let names = fs::read_dir(&logs).unwrap().flatten();
        let newest = names.map(|entry| entry.file_name()).max().unwrap();
        fs::read_to_string(logs.join(newest)).unwrap()
    }

    fn start_battlenet(fake: &FakeLaunchers, dir: &Path) {
        let exe = dir.join("battlenet").join(BATTLENET_EXE);
        let exe = exe.to_string_lossy().into_owned();
        fake.platform.processes.start(&exe, &[], "").unwrap();
    }

    #[test]
    fn the_newest_battlenet_log_is_that_of_the_client_already_running() {
        let knobs = serde_json::json!({ "knobs": { "battlenet": { "runningAs": "0f0f0f0f" } } });
        let (scratch, dir) = battlenet_scenario("sim-bnet-running", &knobs);
        let at = datetime!(2026-11-06 16:46:23 UTC);
        let _fake = materialize(&dir, &scratch.join("data"), at).unwrap();
        assert!(newest_battlenet_log(&dir).contains("|bnet=1:0:0f0f0f0f|"));
    }

    #[test]
    fn a_battlenet_client_closed_before_it_came_up_does_not_come_up_later() {
        let knobs = serde_json::json!({ "knobs": { "battlenet": { "reactMs": 400 } } });
        let (scratch, dir) = battlenet_scenario("sim-bnet-closed", &knobs);
        let at = datetime!(2026-11-06 16:46:23 UTC);
        let fake = materialize(&dir, &scratch.join("data"), at).unwrap();
        let config = fs::read(dir.join(r"appdata\Battle.net\Battle.net.config")).unwrap();
        start_battlenet(&fake, &dir);
        fake.platform.processes.close(BATTLENET_EXE).unwrap();
        while fake.platform.processes.is_running(BATTLENET_EXE).unwrap() {}
        fake.platform.clock.sleep(
            std::time::Duration::from_secs(5),
            &ol_platform_api::CancelToken::new(),
        );
        assert!(!fake.platform.processes.is_running(BATTLENET_EXE).unwrap());
        assert!(fs::read(dir.join(r"appdata\Battle.net\Battle.net.config")).unwrap() == config);
    }

    const GAME_ENDED: &str = "opType=Execute oldStatus=On newStatus=Off agentUid=prometheus";

    fn games_knobs(quits_after_ms: Option<u64>) -> Value {
        let mut knobs = serde_json::json!({ "knobs": {
            "battlenet": { "runningAs": "0a1b2c3d" },
            "riot": { "runningAs": "11111111-1111-4111-8111-111111111111" },
        } });
        if let Some(after) = quits_after_ms {
            for launcher in ["battlenet", "riot"] {
                knobs["knobs"][launcher]["gameQuitsAfterMs"] = after.into();
            }
        }
        knobs
    }

    // Overwatch through the running Battle.net client's launch command, and VALORANT through Play on the running Riot Client.
    fn play_both(fake: &FakeLaunchers, dir: &Path) {
        let exe = dir.join("battlenet").join(BATTLENET_EXE);
        let exe = exe.to_string_lossy().into_owned();
        let command = ol_platform_api::ProcessArg::Plain("--exec=launch Pro");
        fake.platform.processes.start(&exe, &[command], "").unwrap();
        fake.riot
            .as_ref()
            .unwrap()
            .press_play("valorant.live")
            .unwrap();
    }

    // Whether Overwatch and VALORANT are listed, as one look at the processes sees them.
    fn games_listed(fake: &FakeLaunchers) -> (bool, bool) {
        let names = fake.platform.processes.running_names().unwrap();
        let listed = |image: &str| names.iter().any(|name| name == image);
        (listed("overwatch.exe"), listed("valorant.exe"))
    }

    #[test]
    fn a_game_runs_until_its_time_and_is_gone_at_the_first_look_after_it_and_the_client_logs_its_end()
     {
        let (scratch, dir) = battlenet_scenario("sim-quits", &games_knobs(Some(1_000)));
        let at = datetime!(2026-11-06 16:46:23 UTC);
        let fake = materialize(&dir, &scratch.join("data"), at).unwrap();
        play_both(&fake, &dir);
        let clock = &fake.platform.clock;
        let cancel = ol_platform_api::CancelToken::new();
        clock.sleep(std::time::Duration::from_millis(999), &cancel);
        assert_eq!(games_listed(&fake), (true, true));
        assert!(fake.platform.processes.is_running("VALORANT.exe").unwrap());
        let log = newest_battlenet_log(&dir);
        assert!(log.contains("InstallState (prometheus): gameRunning=1"));
        assert!(!log.contains("gameRunning=0"));

        clock.sleep(std::time::Duration::from_millis(1), &cancel);
        assert_eq!(games_listed(&fake), (false, false));
        assert!(!fake.platform.processes.is_running("VALORANT.exe").unwrap());
        let log = newest_battlenet_log(&dir);
        assert!(log.contains(GAME_ENDED));
        assert!(log.contains("InstallState (prometheus): gameRunning=0 isGameProcessRunning=0"));
    }

    // In the window nothing sleeps on the fake clock between two presses of Play: the game has to go at a look all the same.
    #[test]
    fn a_game_a_watched_client_started_goes_at_a_look_with_nothing_sleeping_on_the_fake_clock() {
        let (scratch, dir) = battlenet_scenario("sim-quits-watched", &games_knobs(Some(50)));
        let fake = materialize_in_real_time(&dir, &scratch.join("data")).unwrap();
        play_both(&fake, &dir);
        let given_up = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while games_listed(&fake) != (false, false) && std::time::Instant::now() < given_up {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(games_listed(&fake), (false, false));
        assert!(!fake.platform.processes.is_running("VALORANT.exe").unwrap());
        assert!(newest_battlenet_log(&dir).contains(GAME_ENDED));
    }

    #[test]
    fn a_game_a_test_quits_is_gone_from_the_client_and_the_processes_and_starts_again_at_the_next_play()
     {
        let (scratch, dir) = battlenet_scenario("sim-quit-by-test", &games_knobs(None));
        let at = datetime!(2026-11-06 16:46:23 UTC);
        let fake = materialize(&dir, &scratch.join("data"), at).unwrap();
        play_both(&fake, &dir);
        assert_eq!(games_listed(&fake), (true, true));

        fake.battlenet.as_ref().unwrap().quit_game("prometheus");
        fake.riot.as_ref().unwrap().quit_game("valorant");
        assert_eq!(games_listed(&fake), (false, false));
        assert!(!fake.platform.processes.is_running("VALORANT.exe").unwrap());
        assert!(newest_battlenet_log(&dir).contains(GAME_ENDED));

        play_both(&fake, &dir);
        assert_eq!(games_listed(&fake), (true, true));
        let started = newest_battlenet_log(&dir)
            .matches("Game is running: prometheus")
            .count();
        assert_eq!(started, 2);
    }

    #[test]
    fn a_battlenet_client_started_after_a_restart_logs_after_the_runs_before_it() {
        let knobs = serde_json::json!({ "knobs": { "battlenet": { "reactMs": 0 } } });
        let (scratch, dir) = battlenet_scenario("sim-bnet-restart", &knobs);
        let at = datetime!(2026-11-06 16:46:23 UTC);
        let fake = materialize(&dir, &scratch.join("data"), at).unwrap();
        for _ in 0..3 {
            start_battlenet(&fake, &dir);
        }
        drop(fake);

        let knobs = serde_json::json!({ "knobs": { "battlenet": { "reactMs": 0, "updatesPending": ["prometheus"] } } });
        write_knobs(&dir, &knobs).unwrap();
        let fake = materialize(&dir, &scratch.join("data"), at).unwrap();
        start_battlenet(&fake, &dir);
        assert!(newest_battlenet_log(&dir).contains("(prometheus): currentOperation=Update"));
    }
}
