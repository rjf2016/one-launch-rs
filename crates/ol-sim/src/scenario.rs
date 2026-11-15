use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use ol_formats::winpath::join;
use ol_model::LauncherId;
use ol_platform_api::{Platform, ProcessInfo};
use ol_platform_fake::{
    FakeClock, FakeProcesses, FakeRegistry, FakeShell, OpLog, ProcessKnobs, RegistryValue,
    ScratchFiles,
};
use ol_runtime::TimingOverrides;
use serde_json::{Map, Value};
use thiserror::Error;
use time::OffsetDateTime;

use crate::steam::{SteamKnobs, SteamSim};

const SCENARIO_FILE: &str = "scenario.json";
const KNOBS_FILE: &str = "knobs.json";
const FIXTURE_PREFIX: &str = "@fixtures/";

// Waits short enough that a fake switch is over in a few seconds of its own clock.
const SIM_TIMING: [(&str, u64); 10] = [
    ("pollMs", 100),
    ("logPollMs", 100),
    ("shutdownMs", 3_000),
    ("killGraceMs", 1_000),
    ("startupMs", 5_000),
    ("signInMs", 8_000),
    ("guardMs", 8_000),
    ("updateMs", 20_000),
    ("manualSignInMs", 30_000),
    ("readyGraceMs", 300),
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

#[derive(Clone, Default)]
pub struct Seed {
    pub settings: Vec<(String, String)>,
    pub accounts: Vec<SeedAccount>,
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
    Seed { settings, accounts }
}

// Builds the fake machine for a folder `lay_down` made: an in-memory registry and process table with the Steam simulator, a file port that sees only `dir` and `data_dir`, and a clock that starts at `now`.
pub fn materialize(
    dir: &Path,
    data_dir: &Path,
    now: OffsetDateTime,
) -> Result<FakeLaunchers, ScenarioError> {
    materialize_on(dir, data_dir, FakeClock::starting_at(now))
}

// As `materialize`, for a person to watch: the clock is the system's, so a switch takes the seconds its waits add up to.
pub fn materialize_in_real_time(
    dir: &Path,
    data_dir: &Path,
) -> Result<FakeLaunchers, ScenarioError> {
    materialize_on(dir, data_dir, FakeClock::following_the_system())
}

fn materialize_on(
    dir: &Path,
    data_dir: &Path,
    clock: FakeClock,
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
    let log = OpLog::new();
    let registry = Arc::new(FakeRegistry::new(log.clone()));
    if installed.contains(&LauncherId::Steam) {
        SteamSim::seed_registry(&registry, &steam_dir);
    }
    if let Some(values) = scenario["registry"].as_object() {
        for (path, value) in values {
            let value = match (value.as_str(), value.as_u64()) {
                (Some(text), _) => match text.strip_prefix("binary:") {
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

    let mut steam_timing = TimingOverrides::default();
    for (name, milliseconds) in SIM_TIMING {
        steam_timing.set(name, milliseconds);
    }
    for (name, value) in merged(&scenario, &overrides, &["timing", "steam"]) {
        if let Some(milliseconds) = value.as_u64() {
            steam_timing.set(&name, milliseconds);
        }
    }

    let roots: [&Path; 2] = [dir, data_dir];
    let platform = Platform {
        registry: Arc::clone(&registry) as _,
        processes: Arc::clone(&processes) as _,
        files: Arc::new(ScratchFiles::new(&roots, log.clone())),
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
        steam_timing,
        steam_default_dir: join(&[&root, "nowhere"]),
        seed: seed(&scenario),
    })
}
