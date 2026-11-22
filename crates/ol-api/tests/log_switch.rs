#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_lite::future::block_on;
use ol_api::accounts::AccountPatch;
use ol_api::clipboard::{CopyField, Timer};
use ol_api::launchers::{
    BattlenetMachine, BattlenetTiming, EpicMachine, EpicTiming, Launchers, NewAccount, RiotMachine,
    RiotTiming, Secret, SteamTiming,
};
use ol_api::log::{LogLevel, LogOptions, Logger, log_panics};
use ol_api::settings::{CloseAction, QuitAnswer};
use ol_api::{Backend, StartLaunch};
use ol_model::{JobState, LaunchJob, LauncherId, SettingKey};
use ol_platform_api::CancelToken;
use ol_platform_fake::{FakeClock, RecordingClipboard};
use ol_sim::FakeLaunchers;
use ol_testkit::{Scratch, oracle};
use ol_vault::{FakeVault, Vault};
use serde_json::{Value, json};
use time::macros::datetime;

const VERSION: &str = "9.8.7";
const SCENARIO: &str = "fixtures/dev-only/fake-launchers/all-four";
const WAIT: Duration = Duration::from_secs(10);
const TYPED: [&str; 3] = ["Persona One again", "Gordon Renamed", "placeholder-pw-1"];
// The redaction leaves these alone on purpose, as the Electron app's does: they name nobody.
const NOT_A_NAME: [&str; 4] = ["main", "alt", "true", "false"];

struct OnTheClock(Arc<FakeClock>);

impl Timer for OnTheClock {
    fn after(&self, delay: Duration, cancel: CancelToken, run: Box<dyn FnOnce() + Send>) {
        self.0.schedule(delay, move || {
            if !cancel.is_cancelled() {
                run();
            }
        });
    }
}

struct Run {
    fake: FakeLaunchers,
    backend: Backend,
    jobs: Receiver<LaunchJob>,
}

impl Run {
    fn account(&self, launcher: LauncherId, login: &str) -> ol_model::Account {
        block_on(self.backend.accounts())
            .unwrap()
            .into_iter()
            .find(|account| account.launcher == launcher && account.login == login)
            .expect("the scenario seeds this account")
    }

    fn game(&self, launcher: LauncherId) -> ol_model::Game {
        let library = block_on(self.backend.library()).unwrap();
        library
            .games
            .into_iter()
            .map(|entry| entry.game)
            .find(|game| game.launcher == launcher)
            .expect("the scan found the launcher's game")
    }

    fn play(&self, launcher: LauncherId, login: &str, confirm_close: bool) -> Option<LaunchJob> {
        let start = StartLaunch {
            account_id: self.account(launcher, login).id,
            game_id: Some(self.game(launcher).id),
            confirm_close,
        };
        block_on(self.backend.start_launch(start)).ok()?;
        Some(self.ended())
    }

    fn ended(&self) -> LaunchJob {
        loop {
            let job = self.jobs.recv().expect("the job ends");
            if job.state != JobState::Running {
                return job;
            }
        }
    }
}

fn start(scratch: &Scratch, logger: &Logger) -> Run {
    let launchers_dir = scratch.join("launchers");
    let fake = ol_sim::materialize(
        &launchers_dir,
        &data_dir(scratch),
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
    )
    .with_local_launch(fake.riot_launch.clone());
    let (changed, jobs) = channel();
    let changed = Mutex::new(changed);
    let backend = Backend::logging_to(logger.log()).start_with(
        data_dir(scratch),
        VERSION,
        launchers,
        Some(Arc::new(move |job: &LaunchJob| {
            let _ = changed.lock().unwrap().send(job.clone());
        })),
    );
    if block_on(backend.accounts()).unwrap().is_empty() {
        for account in &fake.seed.accounts {
            block_on(backend.create_account(NewAccount {
                launcher: account.launcher,
                display_name: account.display_name.clone(),
                login: account.login.clone(),
                launcher_uid: account.launcher_uid.clone(),
                password: account.password.clone().map(Secret::new),
                tags: Vec::new(),
                notes: String::new(),
            }))
            .unwrap();
        }
        let accounts = block_on(backend.accounts()).unwrap();
        for session in &fake.seed.sessions {
            let owner = accounts
                .iter()
                .find(|account| {
                    account.launcher == session.launcher && account.login == session.login
                })
                .unwrap();
            block_on(backend.seed_session(
                &owner.id,
                Some(session.launcher_uid.clone()),
                Secret::new(session.blob.clone()),
            ))
            .unwrap();
        }
    }
    block_on(backend.rescan()).unwrap();
    Run {
        fake,
        backend,
        jobs,
    }
}

fn strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => out.push(text.clone()),
        Value::Array(items) => items.iter().for_each(|item| strings(item, out)),
        Value::Object(fields) => fields.values().for_each(|field| strings(field, out)),
        _ => {}
    }
}

const BASE64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let value = chunk.iter().enumerate().fold(0_u32, |value, (at, byte)| {
            value | (u32::from(*byte) << (16 - 8 * at))
        });
        for at in 0..4 {
            if at <= chunk.len() {
                out.push(BASE64[((value >> (18 - 6 * at)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn base64_decoded(text: &str) -> Vec<u8> {
    let mut bits = 0_u32;
    let mut count = 0;
    let mut out = Vec::new();
    for byte in text.bytes() {
        let Some(value) = BASE64
            .iter()
            .position(|known| *known == byte)
            .or(match byte {
                b'-' => Some(62),
                b'_' => Some(63),
                _ => None,
            })
        else {
            continue;
        };
        bits = (bits << 6) | value as u32;
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    out
}

fn quoted(line: &str) -> Vec<String> {
    line.split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_under(&path, out);
        } else {
            out.push(path);
        }
    }
}

// What must never be written, read off the fixtures, the scenario, what the fake launchers made up during the run and what the stand-in vault sealed.
fn secrets(scratch: &Scratch, fake: &FakeLaunchers) -> Vec<String> {
    let scenario: Value =
        serde_json::from_str(&fs::read_to_string(oracle(SCENARIO).join("scenario.json")).unwrap())
            .unwrap();
    let mut found: Vec<String> = TYPED.iter().map(|typed| (*typed).to_owned()).collect();
    found.extend(FOLDER_WORDS.map(str::to_owned));
    found.push(scratch.path().to_string_lossy().into_owned());
    found.push("no row for".to_owned());
    for account in scenario["seed"]["accounts"].as_array().unwrap() {
        for key in ["displayName", "login", "launcherUid", "password"] {
            strings(&account[key], &mut found);
        }
    }
    for session in scenario["seed"]["sessions"].as_array().unwrap() {
        for key in ["login", "launcherUid", "refreshToken", "blob"] {
            strings(&session[key], &mut found);
        }
    }
    for (path, value) in scenario["registry"].as_object().unwrap() {
        found.extend(path.rsplit('\\').next().map(str::to_owned));
        strings(value, &mut found);
    }
    strings(&scenario["onDisk"], &mut found);

    let mut sealed = Vec::new();
    for session in &fake.seed.sessions {
        sealed.push(session.blob.clone());
    }
    for account in &fake.seed.accounts {
        sealed.extend(account.password.clone());
    }
    sealed.push(TYPED[2].to_owned());
    for plain in &sealed {
        let ciphertext = FakeVault.encrypt(&Secret::new(plain.clone())).unwrap();
        found.push(base64(ciphertext.as_bytes()));
        found.push(base64(plain.as_bytes()));
    }
    for n in 0..400 {
        found.push(base64(
            format!("fresh-blob-{n}-{}", "x".repeat(40)).as_bytes(),
        ));
        found.push(format!("01000000d08c9ddf{n:04x}{}", "ab".repeat(24)));
    }
    found.extend(["fresh-blob", "placeholder-refresh", "blob-for-"].map(str::to_owned));

    let fixture =
        |relative: &str| fs::read_to_string(oracle("fixtures/launchers").join(relative)).unwrap();
    for line in fixture("steam/loginusers.vdf").lines() {
        let line = line.trim();
        if line.starts_with("\"AccountName\"") || line.starts_with("\"PersonaName\"") {
            found.extend(quoted(line).into_iter().skip(1));
        } else if line.starts_with("\"7656119") {
            found.extend(quoted(line));
        }
    }
    let config = fixture("battlenet/Battle.net.config");
    let saved = config
        .lines()
        .find_map(|line| line.trim().strip_prefix("\"SavedAccountNames\": \""))
        .unwrap();
    found.extend(
        saved
            .trim_end_matches(['"', ','])
            .split(',')
            .map(str::to_owned),
    );
    let launchers = scratch.join("launchers");
    let ini = [
        fixture("epic/GameUserSettings.ini"),
        fs::read_to_string(
            launchers
                .join("localappdata/EpicGamesLauncher/Saved/Config/Windows/GameUserSettings.ini"),
        )
        .unwrap_or_default(),
    ];
    for line in ini.iter().flat_map(|text| text.lines()) {
        if let Some(data) = line.strip_prefix("Data=") {
            found.push(data.to_owned());
            let decoded = String::from_utf8_lossy(&base64_decoded(data)).into_owned();
            found.extend(
                decoded
                    .split(|c: char| !c.is_ascii_graphic())
                    .filter(|part| part.len() >= 8)
                    .map(str::to_owned),
            );
        }
    }
    let riot = [
        fixture("riot/RiotGamesPrivateSettings.yaml"),
        fs::read_to_string(
            launchers
                .join("localappdata/Riot Games/Riot Client/Data/RiotGamesPrivateSettings.yaml"),
        )
        .unwrap_or_default(),
    ];
    for text in &riot {
        for line in text.lines() {
            for value in quoted(line) {
                if value.len() >= 16 && !value.contains("placeholder.example") {
                    found.push(value);
                }
            }
        }
        if let Some(id_token) = text
            .lines()
            .find_map(|line| line.trim().strip_prefix("id_token: \""))
        {
            let payload = base64_decoded(id_token.trim_end_matches('"').split('.').nth(1).unwrap());
            let claims: Value = serde_json::from_slice(&payload).unwrap();
            strings(&claims["sub"], &mut found);
            let (game_name, tag_line) = (
                claims["acct"]["game_name"].as_str().unwrap(),
                claims["acct"]["tag_line"].as_str().unwrap(),
            );
            found.push(game_name.to_owned());
            found.push(format!("{game_name}#{tag_line}"));
        }
    }
    for (_, value) in fake.registry.dump() {
        if let ol_platform_fake::RegistryValue::Binary(hex) = value {
            found.push(hex);
        }
    }
    let mut kept = Vec::new();
    files_under(&data_dir(scratch).join("backups"), &mut kept);
    for file in kept {
        let text = String::from_utf8_lossy(&fs::read(&file).unwrap()).into_owned();
        if file.extension().is_some_and(|extension| extension == "enc") {
            found.push(text);
        } else if file.file_name().is_some_and(|name| name == "manifest.json") {
            let manifest: Value = serde_json::from_str(&text).unwrap();
            strings(&manifest["meta"], &mut found);
        }
    }

    found.retain(|value| value.len() >= 3 && !NOT_A_NAME.contains(&value.to_lowercase().as_str()));
    found.sort();
    found.dedup();
    found
}

fn read_logs(dir: &Path) -> String {
    let mut text = String::new();
    let mut names: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    names.sort();
    for path in names {
        text.push_str(&fs::read_to_string(path).unwrap());
    }
    text
}

fn written(logger: &Logger) -> String {
    assert!(logger.flush(WAIT));
    read_logs(logger.dir())
}

fn restart(scratch: &Scratch, logger: &Logger, run: Run, knobs: &Value) -> Run {
    run.backend.shut_down();
    drop(run);
    ol_sim::write_knobs(&scratch.join("launchers"), knobs).unwrap();
    start(scratch, logger)
}

// Another program holding a file, sharing only what `share` allows, keeps it from being removed.
#[cfg(windows)]
fn hold(path: &Path, share: u32) -> fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .share_mode(share)
        .open(path)
        .unwrap()
}

// Made-up words with a space and a folder of the kind a company's OneDrive makes: neither may reach the log.
const FOLDER_WORDS: [&str; 2] = ["Wren Halloway", "Brightwater Holdings"];

fn data_dir(scratch: &Scratch) -> PathBuf {
    scratch
        .join(FOLDER_WORDS[0])
        .join(format!("OneDrive - {}", FOLDER_WORDS[1]))
        .join("OneLaunch")
}

fn launch_dir(scratch: &Scratch) -> PathBuf {
    scratch.join("launchers")
}

#[test]
fn a_whole_run_on_each_launcher_leaves_no_secret_in_the_log() {
    let scratch = Scratch::new("api-log-switch");
    ol_sim::lay_down(
        &oracle(SCENARIO),
        &oracle("fixtures/launchers"),
        &launch_dir(&scratch),
    )
    .unwrap();
    let logger = Arc::new(Logger::for_data_folder(
        &data_dir(&scratch),
        LogOptions {
            level: LogLevel::Debug,
            max_bytes: 16 * 1024,
            keep: 100,
            ..LogOptions::default()
        },
    ));
    let data = data_dir(&scratch);
    let run = start(&scratch, &logger);
    let backend = run.backend.clone();
    block_on(backend.set_setting(SettingKey::LogLevel, "debug".to_owned())).unwrap();
    assert!(block_on(backend.set_setting(SettingKey::LogLevel, "loud".to_owned())).is_err());
    let elsewhere = "2026-11-07T03-23-28.574Z";
    let copied = data.join("backups").join("steam").join(elsewhere);
    fs::create_dir_all(copied.join("config")).unwrap();
    let fixture = oracle("fixtures/data-folder/backups/steam").join(elsewhere);
    for file in ["manifest.json", r"config\loginusers.vdf"] {
        fs::copy(fixture.join(file), copied.join(file)).unwrap();
    }
    assert!(block_on(backend.restore_backup(LauncherId::Steam, elsewhere)).is_err());
    fs::remove_dir_all(&copied).unwrap();

    let remembered = block_on(backend.remembered_accounts(LauncherId::Steam)).unwrap();
    let keys: Vec<String> = remembered
        .iter()
        .map(|entry| {
            if entry.launcher_uid.is_empty() {
                entry.login.clone()
            } else {
                entry.launcher_uid.clone()
            }
        })
        .collect();
    let imported = block_on(backend.import_accounts(LauncherId::Steam, keys)).unwrap();
    assert!(!imported.accounts.is_empty());
    let duplicate = block_on(backend.create_account(NewAccount {
        launcher: LauncherId::Steam,
        display_name: TYPED[0].to_owned(),
        login: "user_1".to_owned(),
        launcher_uid: None,
        password: None,
        tags: Vec::new(),
        notes: String::new(),
    }));
    assert_eq!(
        duplicate.err().map(|refused| refused.code),
        Some("DUPLICATE")
    );

    let recording = Arc::new(RecordingClipboard::new(run.fake.log.clone()));
    backend.use_clipboard(
        Arc::clone(&recording) as _,
        Arc::new(OnTheClock(Arc::clone(&run.fake.clock))),
    );
    let user_3 = run.account(LauncherId::Steam, "user_3");
    block_on(backend.copy_to_clipboard(&user_3.id, CopyField::Password)).unwrap();
    block_on(backend.copy_to_clipboard(&user_3.id, CopyField::Login)).unwrap();
    let user_1 = run.account(LauncherId::Steam, "user_1");
    block_on(backend.set_account_password(&user_1.id, Secret::new(TYPED[2].to_owned()))).unwrap();
    block_on(backend.clear_account_password(&user_1.id)).unwrap();

    let plays = [
        (LauncherId::Steam, ["user_3", "user_1"]),
        (LauncherId::Epic, ["alt@example.com", "main@example.com"]),
        (
            LauncherId::Battlenet,
            ["saved3@example.com", "saved1@example.com"],
        ),
        (LauncherId::Riot, ["riot_alt", "riot_main"]),
    ];
    for (launcher, logins) in plays {
        for (at, login) in logins.into_iter().enumerate() {
            let plan = block_on(backend.switch_plan(&run.account(launcher, login).id));
            assert!(plan.is_ok(), "{} plans a switch", launcher.as_str());
            let job = run
                .play(launcher, login, true)
                .expect("the switch is accepted");
            if at == 0 {
                assert_eq!(job.state, JobState::Done, "{}", launcher.as_str());
            }
        }
    }

    let stopped = Arc::new(AtomicBool::new(false));
    let (canceller, flag) = (backend.clone(), Arc::clone(&stopped));
    run.fake.clock.before_sleep(move || {
        if let Some(job) = canceller.running_job()
            && !flag.swap(true, Ordering::SeqCst)
        {
            let _ = canceller.cancel_launch(&job.id);
        }
    });
    let cancelled = run.play(LauncherId::Steam, "user_3", true).unwrap();
    assert_eq!(cancelled.state, JobState::Cancelled);
    run.fake.clock.before_sleep(|| {});

    let window = backend.window_log();
    window.close_requested(CloseAction::Hide);
    window.hidden_to_tray();
    window.shown_from_tray();
    window.hidden_after_instant_launch(&cancelled.id);
    window.tray_added();
    window.tray_removed();
    window.tray_unavailable("TRAY_WINDOW", Some(-1));
    window.fatal_during_startup();
    window.started_in_tray();
    window.shown_for_second_start();
    log_panics(Arc::clone(&logger));
    let named = run.fake.seed.accounts[0].clone();
    let thrown = std::thread::spawn(move || {
        panic!(
            "no row for {} ({}) under {}",
            named.display_name,
            named.login,
            named.launcher.as_str()
        )
    })
    .join();
    assert!(thrown.is_err());
    drop(std::panic::take_hook());

    let alt = run.account(LauncherId::Epic, "main@example.com");
    run.fake
        .processes
        .set_running("EpicGamesLauncher.exe", false);
    block_on(backend.capture_session(&alt.id)).unwrap();
    block_on(backend.forget_session(&alt.id)).unwrap();
    block_on(backend.forget_session(&alt.id)).unwrap();
    assert_eq!(
        block_on(backend.reset_vault())
            .err()
            .map(|refused| refused.code),
        Some("BAD_REQUEST")
    );
    let snapshots = block_on(backend.backups(LauncherId::Epic)).unwrap();
    block_on(backend.restore_backup(LauncherId::Epic, &snapshots[0].id)).unwrap();

    fs::create_dir_all(data.join("backups").join("steam").join("not-a-snapshot")).unwrap();
    block_on(backend.backups(LauncherId::Steam)).unwrap();
    let mut steam_snapshots = block_on(backend.backups(LauncherId::Steam)).unwrap();
    let oldest = steam_snapshots.pop().unwrap();
    let held = data
        .join("backups")
        .join("steam")
        .join(&oldest.id)
        .join("held.txt");
    let open_elsewhere = hold(&held, 0);
    block_on(backend.set_setting(SettingKey::BackupsKeep, "1".to_owned())).unwrap();
    fs::create_dir_all(data.join("switch.json")).unwrap();
    let job = run.play(LauncherId::Steam, "user_3", true).unwrap();
    assert!(job.backup_id.is_some(), "{:?}", job.state);
    for left in ["switch.json", "switch.json.unreadable"] {
        let _ = fs::remove_dir_all(data.join(left));
    }
    drop(open_elsewhere);

    let steam_exe = launch_dir(&scratch).join("steam").join("steam.exe");
    block_on(backend.locate_launcher(LauncherId::Steam, steam_exe.to_str().unwrap())).unwrap();
    block_on(backend.reset_launcher_path(LauncherId::Steam)).unwrap();
    block_on(backend.back_up_database()).unwrap();
    let riot_new = run.account(LauncherId::Riot, "riot_new");
    block_on(backend.update_account(
        &riot_new.id,
        AccountPatch {
            display_name: Some(TYPED[1].to_owned()),
            login: None,
            launcher_uid: None,
            tags: None,
            notes: None,
        },
    ))
    .unwrap();
    let _ = block_on(backend.session(&riot_new.id));
    block_on(backend.delete_account(&riot_new.id)).unwrap();
    let folders = launch_dir(&scratch)
        .join("steam")
        .join("steamapps")
        .join("libraryfolders.vdf");
    let library = fs::read(&folders).unwrap();
    fs::write(&folders, "not a vdf {").unwrap();
    let scanned = block_on(backend.rescan()).unwrap();
    assert!(scanned.iter().any(|result| result.error.is_some()));
    fs::write(&folders, library).unwrap();

    let rollback = json!({
        "processes": { "startFails": ["steam.exe"] },
        "timing": { "steam": { "startupMs": 1500 } }
    });
    let run = restart(&scratch, &logger, run, &rollback);
    let job = run.play(LauncherId::Steam, "user_3", true).unwrap();
    assert_eq!((job.state, job.restored), (JobState::Failed, Some(true)));

    let stuck = json!({
        "processes": { "elevated": ["steam.exe"], "shutdownPolls": 100_000 },
        "knobs": { "steam": { "expiredSessions": ["user_3"] } },
        "timing": { "steam": { "shutdownMs": 1000, "killGraceMs": 500 } }
    });
    let run = restart(&scratch, &logger, run, &stuck);
    block_on(
        run.backend
            .set_setting(SettingKey::SteamAutoLogin, "true".to_owned()),
    )
    .unwrap();
    let start_steam = StartLaunch {
        account_id: run.account(LauncherId::Steam, "user_3").id,
        game_id: None,
        confirm_close: false,
    };
    block_on(run.backend.start_launch(start_steam)).unwrap();
    let job = run.ended();
    assert_eq!((job.state, job.restored), (JobState::Failed, Some(false)));

    let run = restart(&scratch, &logger, run, &json!({}));
    assert!(block_on(run.backend.recovery()).unwrap().is_some());
    let journal_held = hold(&data.join("switch.json"), 0x1);
    block_on(run.backend.restore_recovery()).unwrap();
    drop(journal_held);

    run.backend.shut_down();
    fs::copy(
        oracle("fixtures/journal/switch-before-writes.json"),
        data.join("switch.json"),
    )
    .unwrap();
    let run = start(&scratch, &logger);
    run.backend.shut_down();
    fs::copy(
        oracle("fixtures/journal/switch.json.unreadable"),
        data.join("switch.json"),
    )
    .unwrap();
    let run = start(&scratch, &logger);

    let (ready, readied) = channel();
    let ready = Mutex::new(Some(ready));
    let released = Arc::new(AtomicBool::new(false));
    let (watcher, release) = (run.backend.clone(), Arc::clone(&released));
    run.fake.clock.before_sleep(move || {
        if watcher.running_job().is_none() {
            return;
        }
        if let Some(ready) = ready.lock().unwrap().take() {
            let _ = ready.send(());
            while !release.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    });
    let start_steam = StartLaunch {
        account_id: run.account(LauncherId::Steam, "user_1").id,
        game_id: None,
        confirm_close: true,
    };
    block_on(run.backend.start_launch(start_steam)).unwrap();
    readied.recv_timeout(WAIT).unwrap();
    let question = run.backend.quit_question().expect("a switch runs");
    let window = run.backend.window_log();
    window.quit_asked(&question.job_id);
    window.quit_answered(QuitAnswer::Keep);
    window.quit_answered(QuitAnswer::Quit);
    window.stopping_for_quit();
    assert_eq!(
        block_on(run.backend.reset_vault())
            .err()
            .map(|refused| refused.code),
        Some("BUSY")
    );
    let quitting = run.backend.clone();
    let quit = std::thread::spawn(move || quitting.shut_down());
    let until = Instant::now() + WAIT;
    while !written(&logger).contains("stopping the running switch") {
        assert!(Instant::now() < until, "the quit was not logged");
        std::thread::sleep(Duration::from_millis(5));
    }
    released.store(true, Ordering::SeqCst);
    quit.join().unwrap();
    let fake = run.fake;
    logger.close(WAIT);

    let log = read_logs(logger.dir());
    assert!(
        logger.dir().join("onelaunch.1.log").exists(),
        "the log was rotated"
    );
    let lower = log.to_lowercase();
    for expected in [
        "[main] starting",
        "[main:steam] switch started",
        "[main:epic] epic switch started",
        "[main:battlenet] battlenet switch started",
        "[main:riot] riot switch started",
        "[main:launch] launch job finished",
        "[main:launch] launch step finished",
        "[main:launch] launch job cancel requested",
        "[main:steam] switch stopped",
        "[main:steam] switch failed",
        "[main:steam] switch rolled back from backup",
        "[main:steam] rollback failed",
        "[main:backup] restored launcher files from backup",
        "[main:steam] backup restored",
        "[main:epic] epic backup restored",
        "[main:ipc] backup restored by user",
        "[main:recovery] the last run ended during a switch; offering its snapshot",
        "[main:recovery] interrupted switch recovered from its snapshot",
        "[main:recovery] the last run ended during a switch before anything was written; nothing to recover",
        "[main:journal] the switch journal could not be read; moving it aside",
        "[main:journal] could not write the switch journal",
        "[main:journal] could not remove the switch journal",
        "[main:backup] unreadable backup manifest",
        "[main:backup] could not remove an old backup",
        "[main:backup] pruned backups",
        "[main:backup] backup snapshot written",
        "[main:ipc] session captured by user",
        r#"[main:launchers] session captured {"launcher":"epic","accountId":"#,
        r#"[main:launchers] session forgotten {"launcher":"epic","accountId":"#,
        "[main:ipc] ipc vault:reset failed",
        "[main:ipc] ipc vault:reset refused",
        r#"DEBUG [main] window close requested {"action":"hide"}"#,
        "INFO  [main] window hidden to the tray\n",
        "INFO  [main] window shown from the tray",
        r#"INFO  [main] window hidden to the tray after an instant launch {"jobId":"#,
        "INFO  [main] tray icon added",
        "INFO  [main] tray icon removed",
        r#"WARN  [main] tray icon unavailable; closing the window quits {"error":{"code":"TRAY_WINDOW","hresult":"0xffffffff"}}"#,
        "ERROR [main] fatal during startup",
        r#"ERROR [main] uncaught exception {"location":"crates\\ol-api\\tests\\log_switch.rs:"#,
        r#""thread":"unnamed"}"#,
        r#"[main] database copied {"dest":"backups\\db\\onelaunch-"#,
        r#"[main] starting {"version":"9.8.7","dataDir":"chosen","#,
        r#"INFO  [main] quit prompt shown during a switch {"jobId":"#,
        r#"INFO  [main] quit prompt answered {"answer":"keep"}"#,
        r#"INFO  [main] quit prompt answered {"answer":"quit"}"#,
        r#"WARN  [main] stopping the running switch {"reason":"quit confirmed during a switch"}"#,
        "[main:ipc] copied to clipboard",
        "[main:ipc] account password updated",
        "[main:ipc] account password removed",
        "[main:ipc] account deleted",
        "[main:ipc] account updated",
        "[main:ipc] launcher folder chosen by user",
        "[main:ipc] launcher folder reset to automatic detection",
        "[main] database copied",
        "[main:scan] library rescanned",
        "[main:scan] library scan failed for launcher",
        "[main:ipc] imported launcher accounts",
        "[main:launchers] adapter call refused",
        "ipc accounts:create refused",
        "ipc settings:set failed",
        "[main] stopping the running switch",
        "[main:launch] launch service stopping; aborting the running job",
        "[main] exit",
        "[ACCOUNT]",
    ] {
        assert!(log.contains(expected), "the log has {expected:?}");
    }
    assert_eq!(log.matches("session forgotten").count(), 1);
    assert!(
        log.lines()
            .any(|line| line.contains("ipc backups:restore") && line.contains("[PATH]")),
        "an error that quotes a path outside the data folder has it masked"
    );
    let secrets = secrets(&scratch, &fake);
    assert!(
        secrets.len() >= 800,
        "the run gives {} values to look for",
        secrets.len()
    );
    let leaked: Vec<String> = secrets
        .into_iter()
        .filter(|secret| lower.contains(&secret.to_lowercase()))
        .collect();
    assert!(
        leaked.is_empty(),
        "the log holds {} value(s) it must not; the first is {} characters long",
        leaked.len(),
        leaked[0].len()
    );
    let mut after_users = lower.split("users");
    after_users.next();
    for rest in after_users {
        let name = rest.trim_start_matches(['\\', '/']);
        assert!(
            rest == name || name.starts_with("[user]"),
            "a profile path in the log names its user"
        );
    }
}

#[test]
fn a_launcher_that_is_not_installed_is_said_at_debug_and_nothing_else() {
    let scratch = Scratch::new("api-log-absent");
    let launchers = launch_dir(&scratch);
    ol_sim::lay_down(&oracle(SCENARIO), &oracle("fixtures/launchers"), &launchers).unwrap();
    let file = launchers.join("scenario.json");
    let mut scenario: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    scenario["installed"] = json!(["steam", "epic", "battlenet"]);
    fs::write(&file, serde_json::to_string_pretty(&scenario).unwrap()).unwrap();
    let _ = fs::remove_file(launchers.join("riot").join("RiotClientServices.exe"));
    let id = "2026-11-07T03-23-28.604Z";
    let snapshot = data_dir(&scratch).join("backups").join("riot").join(id);
    fs::create_dir_all(&snapshot).unwrap();
    fs::copy(
        oracle("fixtures/data-folder/backups/riot")
            .join(id)
            .join("manifest.json"),
        snapshot.join("manifest.json"),
    )
    .unwrap();
    let logger = Logger::for_data_folder(
        &data_dir(&scratch),
        LogOptions {
            level: LogLevel::Debug,
            ..LogOptions::default()
        },
    );
    let run = start(&scratch, &logger);
    let refused = block_on(run.backend.restore_backup(LauncherId::Riot, id));
    assert_eq!(refused.err().map(|error| error.code), Some("NOT_INSTALLED"));
    run.backend.shut_down();
    let log = written(&logger);
    assert!(log.contains(
        r#"[main:scan] launcher not installed; library scan skipped {"launcher":"riot""#
    ));
    assert!(log.contains(r#"DEBUG [main:launchers] launcher not installed {"launcher":"riot","operation":"restoreBackup"}"#));
}
