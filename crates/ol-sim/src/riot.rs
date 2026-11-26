use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ol_formats::riot::products::{
    RiotProductId, game_images_of, parse_product_id, product_display_name,
};
use ol_formats::riot::session::{RiotSession, SessionRead, read_session, splice_session_block};
use ol_platform_api::{Clock, RiotLaunchError};
use ol_platform_fake::{CLOSE_ARGUMENT, FakeClock, FakeProcesses, FakeRegistry, RegistryValue};
use time::OffsetDateTime;

// Where a Riot Client keeps what the app reads. The simulator has its own copy of these names: it stands in for the client, not for the app's idea of the client.
const EXE: &str = "RiotClientServices.exe";
const WINDOW_EXE: &str = "Riot Client.exe";
const UNINSTALL: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall";
const URI_COMMAND: &str = r"HKLM\SOFTWARE\Classes\riotclient\shell\open\command\";
const SESSION_FILE: &str = "RiotGamesPrivateSettings.yaml";
const BUILD: &str = "140.0.7.5045";
// The product the simulated install holds.
const PRODUCTS: [&str; 1] = ["valorant.live"];
const SESSION_ID: &str = "22222222-2222-4222-8222-222222222222";
const LOCKFILE: &str = "C:/Users/Placeholder/AppData/Local/Riot Games/Riot Client/Config/lockfile_";
const SESSION_WRITTEN: &str = "SDK: private-settings: Updated private settings for user localuser with key psl.authorization.riot-client";
const DEVICE_WRITTEN: &str = "SDK: private-settings: Updated private settings for user localuser with key rso-authenticator.tdid";
const REFUSED: &str = "SDK: rso-auth: error 'authorization_creation_failure_request_error': in plugin library function HandleAuthorizationCreate: Failed to create RSO authorization (401) invalid_grant:";

// The client's launch pipeline, from the product's context to the point where the game can start.
const LAUNCH_PIPELINE: [&str; 22] = [
    "SelfUpdate",
    "LifecycleSync",
    "LeagueRegion",
    "Login",
    "HomeBaseCountry",
    "Eula",
    "ClientConfig",
    "VanguardHealthCheckStatus",
    "PatchlineData",
    "RiotIdRequired",
    "AccountAlias",
    "LeagueRegionElection",
    "PlayerAffinitySync",
    "AppRepairAndUpdate",
    "PatchStatus",
    "AgeRestriction",
    "Permissions",
    "LaunchRestriction",
    "PatchlineLaunchDisabled",
    "VngAccountRequired",
    "RestartRequired",
    "WaitForLaunch",
];

#[derive(Clone)]
pub struct RiotKnobs {
    // Saved sign-ins the client refuses at its start: a refresh token's fingerprint, or the account id it signs in as.
    pub rejected: Vec<String>,
    // The client swaps the refresh token for a new one and writes the file again at every start from a saved sign-in, as the real one does.
    pub rotate_on_start: bool,
    // The sign-in fails with this error without a verdict on the saved sign-in: the service or the network.
    pub auth_error: Option<String>,
    pub react_ms: u64,
    // Once on its login page, the simulated user signs in by hand after this long.
    pub sign_in_when_asked_ms: Option<u64>,
    // The account id a sign-in by hand lands on.
    pub sign_in_as: Option<String>,
    // The simulated user ticks "Stay signed in". Without it the client writes `riot-client: null` when it exits and keeps nothing.
    pub stay_signed_in: bool,
    // An account id the client is signed in as when the run begins.
    pub running_as: Option<String>,
    // Product ids the client reports as out of date: a launch request for one only opens its page, with Update on it.
    pub updates_pending: Vec<String>,
    // A start with the launch arguments starts the game once signed in, as an older build did. Off, the client only opens the game's page.
    pub direct_launch: bool,
    // The client refuses the launch request its Play button sends.
    pub press_play_fails: bool,
    // The client takes the launch request its Play button sends and starts nothing, as one that first updates the game or something it needs may.
    pub press_play_ignored: bool,
    // Each launch request finds the client checking what the product needs for this long, as the recorded run shows it: the DependencyUpdate patch state and the refusal "Product is not up to date", and only then UpToDate and the game's page.
    pub dependency_update_ms: Option<u64>,
    // The simulated user quits a game this long after its start. Without it the game runs until a test ends it.
    pub game_quits_after_ms: Option<u64>,
}

impl Default for RiotKnobs {
    fn default() -> Self {
        Self {
            rejected: Vec::new(),
            rotate_on_start: true,
            auth_error: None,
            react_ms: 0,
            sign_in_when_asked_ms: None,
            sign_in_as: None,
            stay_signed_in: true,
            running_as: None,
            updates_pending: Vec::new(),
            direct_launch: false,
            press_play_fails: false,
            press_play_ignored: false,
            dependency_update_ms: None,
            game_quits_after_ms: None,
        }
    }
}

// The scenario's stand-ins for %LOCALAPPDATA% and %ProgramData%, the folder the client is installed in and the folder its games are installed under.
pub struct RiotFolders<'a> {
    pub local_app_data: &'a str,
    pub program_data: &'a str,
    pub install_dir: &'a str,
    pub games: &'a str,
}

struct State {
    runs: u32,
    tokens: u32,
    last_stamp: Option<OffsetDateTime>,
    log_file: Option<PathBuf>,
    // The time a log line carries, in milliseconds since the run's start.
    line_clock: u64,
    // The current run signed in by hand without "Stay signed in", so the client forgets it when it exits.
    forget_at_exit: bool,
    // The client runs, as the simulator sees it: an end of the process is not heard, as it sends nothing.
    core_running: bool,
    signed_in: bool,
    // How often the client was closed: what was scheduled for a run that has been closed since does nothing.
    closes: u32,
    // The games the user will quit, by image, each with the time they quit it.
    quits: Vec<(String, OffsetDateTime)>,
    // Until then the client checks what the product needs and its page offers no Play.
    checking_until: Option<OffsetDateTime>,
}

pub struct RiotSim {
    knobs: RiotKnobs,
    session_path: PathBuf,
    log_dir: PathBuf,
    processes: Arc<FakeProcesses>,
    clock: Arc<FakeClock>,
    state: Mutex<State>,
}

fn forward(path: &str) -> String {
    path.replace('\\', "/")
}

// A signed-out file as the client writes one, with a made-up device cookie, for a session file built from nothing.
fn template() -> String {
    let lines = [
        "psl:",
        "    authorization:",
        "        riot-client: null",
        "riot-login:",
        "    persist: null",
        "rso-authenticator:",
        "    tdid:",
        "        domain: \"placeholder.example\"",
        "        expiryTime: 1790000000",
        "        hostOnly: false",
        "        httpOnly: true",
        "        name: \"tdid\"",
        "        path: \"/\"",
        "        persistent: true",
        "        secureOnly: true",
    ];
    format!(
        "{}\r\n        value: \"eyJ{}\"\r\n",
        lines.join("\r\n"),
        "placeholder".repeat(19)
    )
}

fn quoted(text: &str) -> String {
    serde_json::Value::from(text).to_string()
}

// An id token whose payload names `uid`, in the shape a reader expects: three parts in the URL alphabet of base64, an account id and a Riot ID. Nobody signed it.
pub fn fake_id_token(uid: &str, game_name: &str, tag_line: &str) -> String {
    let payload = format!(
        r#"{{"sub":{},"acct":{{"game_name":{},"tag_line":{},"state":"ENABLED"}},"iss":"https://auth.placeholder.example","iat":1760000000,"exp":1760003600}}"#,
        quoted(uid),
        quoted(game_name),
        quoted(tag_line)
    );
    format!(
        "{}.{}.placeholdersignature",
        URL_SAFE_NO_PAD.encode(r#"{"alg":"none","typ":"JWT"}"#),
        URL_SAFE_NO_PAD.encode(payload)
    )
}

// The lines of a `riot-client` block for a sign-in with the given tokens.
pub fn session_block_lines(
    id_token: &str,
    refresh_token: &str,
    write_count: u32,
    at: i128,
) -> Vec<String> {
    let mut lines = vec![
        "claims: {}".to_owned(),
        format!("id_token: \"{id_token}\""),
        "is_dpop_bound: false".to_owned(),
        format!("last_token_creation_time: {at}"),
        format!("original_token_creation_time: {at}"),
        format!("refresh_token: \"{refresh_token}\""),
        format!("refresh_token_write_count: {write_count}"),
        format!("refresh_tokens_session_id: \"{SESSION_ID}\""),
        "scopes:".to_owned(),
    ];
    let scopes = ["openid", "link", "ban", "lol_region", "lol", "account"];
    lines.extend(scopes.map(|scope| format!("- \"{scope}\"")));
    lines
}

fn with_block(text: &str, lines: Option<&[String]>) -> String {
    let lines: Option<Vec<&str>> = lines.map(|lines| lines.iter().map(String::as_str).collect());
    splice_session_block(text, lines.as_deref())
        .expect("the simulated client's file has its session block")
}

// A session file with the given sign-in in the place of the one it holds. Every other line stays, the device cookie among them.
pub fn session_file_with(text: &str, block: &[String]) -> String {
    with_block(text, Some(block))
}

// A whole session file that holds a sign-in as `uid`: what One Launch stores for an account, and what a scenario saves for one.
pub fn fake_session_file(uid: &str, refresh_token: &str) -> String {
    let id_token = fake_id_token(uid, "Placeholder", "0000");
    let lines = session_block_lines(&id_token, refresh_token, 1, 1_760_000_000_000);
    with_block(&template(), Some(&lines))
}

// The product a command line asks the client for, as the game's shortcut passes it.
fn launch_of(args: &[String]) -> Option<RiotProductId> {
    let (mut product, mut patchline) = (None, "live".to_owned());
    for arg in args {
        let lower = arg.to_ascii_lowercase();
        if let Some(value) = lower.strip_prefix("--launch-product=") {
            product = Some(value.to_owned());
        } else if let Some(value) = lower.strip_prefix("--launch-patchline=") {
            patchline = value.to_owned();
        }
    }
    parse_product_id(&format!("{}.{patchline}", product?))
}

impl RiotSim {
    // The registry values a Riot install with VALORANT leaves behind.
    pub fn seed_registry(registry: &FakeRegistry, folders: &RiotFolders<'_>) {
        let text = |value: String| RegistryValue::Text(value);
        registry.seed(
            &format!("{UNINSTALL}\\Riot Game Riot_Client.\\InstallLocation"),
            text(format!("{}/", forward(folders.install_dir))),
        );
        registry.seed(
            URI_COMMAND,
            text(format!(
                "\"{}\\{EXE}\" --app-command=\"%1\"",
                folders.install_dir
            )),
        );
        for id in PRODUCTS {
            let Some(parsed) = parse_product_id(id) else {
                continue;
            };
            let value = |name: &str, value: String| {
                registry.seed(&format!("{UNINSTALL}\\Riot Game {id}\\{name}"), text(value));
            };
            value(
                "DisplayName",
                product_display_name(parsed.product()).to_owned(),
            );
            value(
                "InstallLocation",
                forward(&Self::install_folder(folders.games, &parsed).to_string_lossy()),
            );
            value(
                "DisplayIcon",
                forward(&format!(
                    "{}\\Riot Games\\Metadata\\{id}\\{id}.ico",
                    folders.program_data
                )),
            );
        }
    }

    // Where a product lives under the games folder: `<name>\<patchline>`, as the real installer lays it out.
    fn install_folder(games: &str, id: &RiotProductId) -> PathBuf {
        [games, product_display_name(id.product()), id.patchline()]
            .iter()
            .collect()
    }

    // What the real installer leaves under %ProgramData%\Riot Games and the games folder for each product: the metadata folder with its settings file and its mark, the install folder, and RiotClientInstalls.json. A file that is there is left alone.
    fn ensure_products(folders: &RiotFolders<'_>) {
        let exe = forward(&format!("{}\\{EXE}", folders.install_dir));
        let riot_games: PathBuf = [folders.program_data, "Riot Games"].iter().collect();
        let mut associated = Vec::new();
        for id in PRODUCTS {
            let Some(parsed) = parse_product_id(id) else {
                continue;
            };
            let metadata = riot_games.join("Metadata").join(id);
            let install = Self::install_folder(folders.games, &parsed);
            let install_text = forward(&install.to_string_lossy());
            for folder in [&metadata, &install] {
                fs::create_dir_all(folder)
                    .expect("the simulated client could not make its folders");
            }
            let settings = metadata.join(format!("{id}.product_settings.yaml"));
            if !settings.exists() {
                let lines = [
                    "auto_patching_enabled_by_player: false".to_owned(),
                    "patching_policy: \"manual\"".to_owned(),
                    format!("product_install_full_path: \"{install_text}\""),
                    format!("product_install_root: \"{}\"", forward(folders.games)),
                    "should_repair: false".to_owned(),
                    String::new(),
                ];
                Self::write(&settings, lines.join("\n").as_bytes());
            }
            let installed = metadata.join(format!("{id}.ok"));
            if !installed.exists() {
                Self::write(&installed, b"");
            }
            associated.push(format!(
                "    {}: {}",
                quoted(&format!("{install_text}/")),
                quoted(&exe)
            ));
        }
        let installs = riot_games.join("RiotClientInstalls.json");
        if !installs.exists() {
            let text = format!(
                "{{\n  \"associated_client\": {{\n{}\n  }},\n  \"rc_default\": {exe},\n  \"rc_live\": {exe}\n}}",
                associated.join(",\n"),
                exe = quoted(&exe)
            );
            Self::write(&installs, text.as_bytes());
        }
    }

    pub fn attach(
        folders: &RiotFolders<'_>,
        processes: &Arc<FakeProcesses>,
        clock: Arc<FakeClock>,
        knobs: RiotKnobs,
    ) -> Arc<Self> {
        Self::ensure_products(folders);
        let data: PathBuf = [folders.local_app_data, "Riot Games", "Riot Client"]
            .iter()
            .collect();
        let sim = Arc::new(Self {
            knobs,
            session_path: data.join("Data").join(SESSION_FILE),
            log_dir: data.join("Logs").join("Riot Client Logs"),
            processes: Arc::clone(processes),
            clock,
            state: Mutex::new(State {
                runs: 0,
                tokens: 0,
                last_stamp: None,
                log_file: None,
                line_clock: 0,
                forget_at_exit: false,
                core_running: false,
                signed_in: false,
                closes: 0,
                quits: Vec::new(),
                checking_until: None,
            }),
        });
        if let Some(uid) = &sim.knobs.running_as {
            let holds = sim.current_session();
            if holds.is_none_or(|session| session.account_id != *uid) {
                sim.write_session_for(uid, 1);
            }
            processes.set_running(EXE, true);
            processes.set_running(WINDOW_EXE, true);
            {
                let mut state = sim.state();
                state.core_running = true;
                state.signed_in = true;
            }
            let mut lines = sim.start_lines(None);
            lines.extend(sim.restored_lines());
            sim.write_log(&lines);
        }
        let hears = Arc::clone(&sim);
        processes.on_start(move |exe, args| hears.on_start(exe, args));
        let plays = Arc::clone(&sim);
        processes.on_look(move || plays.games_quit_when_due());
        sim
    }

    fn games_quit_when_due(&self) {
        let now = self.clock.now();
        let due: Vec<String> = {
            let mut state = self.state();
            let (due, later) = std::mem::take(&mut state.quits)
                .into_iter()
                .partition(|(_, at)| *at <= now);
            state.quits = later;
            due.into_iter().map(|(image, _)| image).collect()
        };
        for image in due {
            self.quit_image(&image);
        }
    }

    // Stands in for the user quitting a product's game, such as "valorant": its process goes from the running images and the table alike.
    pub fn quit_game(&self, product: &str) {
        for image in game_images_of(product) {
            self.quit_image(image);
        }
    }

    fn quit_image(&self, image: &str) {
        self.state()
            .quits
            .retain(|(quitting, _)| !quitting.eq_ignore_ascii_case(image));
        self.processes.set_running(image, false);
        self.processes.remove_from_table(image);
    }

    fn write(file: &Path, bytes: &[u8]) {
        let written = file
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| fs::write(file, bytes));
        written.expect("the simulated client could not write its files");
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn now_ms(&self) -> i128 {
        self.clock.now().unix_timestamp_nanos() / 1_000_000
    }

    fn on_start(self: &Arc<Self>, exe: &str, args: &[String]) {
        let image = exe.rsplit(['\\', '/']).next().unwrap_or(exe);
        if !image.eq_ignore_ascii_case(EXE) {
            return;
        }
        if args.first().map(String::as_str) == Some(CLOSE_ARGUMENT) {
            self.core_closed();
            return;
        }
        let launch = launch_of(args);
        if self.knobs.react_ms > 0 {
            let (sim, run) = (Arc::clone(self), self.state().closes);
            self.clock
                .schedule(Duration::from_millis(self.knobs.react_ms), move || {
                    if sim.state().closes == run {
                        sim.run(launch.as_ref());
                    }
                });
        } else {
            self.run(launch.as_ref());
        }
    }

    // The client is one of a kind: a launch request while it runs is handed to it. A plain start is always a new run.
    fn run(self: &Arc<Self>, launch: Option<&RiotProductId>) {
        let core_running = self.state().core_running;
        match launch {
            Some(launch) if core_running => self.forwarded_launch(launch),
            _ => self.react(launch),
        }
    }

    fn fresh_token(&self) -> String {
        let mut state = self.state();
        state.tokens += 1;
        format!("placeholder-refresh-{}-{}", state.tokens, "x".repeat(48))
    }

    fn read_file(&self) -> Option<String> {
        fs::read(&self.session_path)
            .ok()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    }

    // The session the file holds now. None when there is no file, when it is signed out, and when it is in a shape the reader does not know.
    fn current_session(&self) -> Option<RiotSession> {
        match read_session(&self.read_file()?) {
            SessionRead::Session(session) => Some(session),
            _ => None,
        }
    }

    // Writes a sign-in as `uid` with a fresh refresh token into the file, or into a new file when there is none, and keeps everything else.
    fn write_session_for(&self, uid: &str, write_count: u32) {
        let token = self.fresh_token();
        let text = self.read_file().unwrap_or_else(template);
        let id_token = fake_id_token(uid, "Placeholder", "0000");
        let lines = session_block_lines(&id_token, &token, write_count, self.now_ms());
        Self::write(
            &self.session_path,
            with_block(&text, Some(&lines)).as_bytes(),
        );
    }

    // The client's own signed-out state: `riot-client: null`, and nothing else touched.
    fn write_signed_out(&self) {
        let text = self.read_file().unwrap_or_else(template);
        Self::write(&self.session_path, with_block(&text, None).as_bytes());
    }

    // Starts a new log, as the client does at every start, named by the time the run started and a process id.
    fn write_log(&self, lines: &[String]) {
        let file = {
            let mut state = self.state();
            state.runs += 1;
            let now = self.clock.now();
            let stamp = match state.last_stamp {
                Some(last) if last >= now => last + Duration::from_millis(1),
                _ => now,
            };
            state.last_stamp = Some(stamp);
            let name = format!(
                "{:04}-{:02}-{:02}T{:02}-{:02}-{:02}.{:03}_{}_Riot Client.log",
                stamp.year(),
                u8::from(stamp.month()),
                stamp.day(),
                stamp.hour(),
                stamp.minute(),
                stamp.second(),
                stamp.millisecond(),
                10_000 + state.runs
            );
            let file = self.log_dir.join(name);
            state.log_file = Some(file.clone());
            file
        };
        Self::write(&file, format!("{}\r\n", lines.join("\r\n")).as_bytes());
    }

    fn append_log(&self, lines: &[String]) {
        let file = self.state().log_file.clone();
        let file = file.expect("the simulated client has a log to add to: it has started");
        let written = fs::OpenOptions::new()
            .append(true)
            .open(file)
            .and_then(|mut log| log.write_all(format!("{}\r\n", lines.join("\r\n")).as_bytes()));
        written.expect("the simulated client could not write its log");
    }

    fn line(&self, level: &str, text: &str) -> String {
        let mut state = self.state();
        state.line_clock += 50;
        let at = state.line_clock;
        format!("{:06}.{:03}| {level:>6}| {text}", at / 1000, at % 1000)
    }

    fn psl(&self, name: &str) -> String {
        self.line(
            "ALWAYS",
            &format!("riot-client-lifecycle: PSLProvider: PSL state changed to {name}"),
        )
    }

    fn start_lines(&self, launch: Option<&RiotProductId>) -> Vec<String> {
        let run = {
            let mut state = self.state();
            state.line_clock = 0;
            state.runs + 1
        };
        let now = self.clock.now();
        let mut lines = vec![
            format!(
                "000000.000| ALWAYS| Logging started at {:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}",
                now.year(),
                u8::from(now.month()),
                now.day(),
                now.hour(),
                now.minute(),
                now.second(),
                now.millisecond()
            ),
            format!("000000.000| ALWAYS| Application Version: {BUILD}"),
            self.line("OKAY", "Sentry Enabled"),
            self.line("ALWAYS", "Command Line Arguments (Riot Client App):"),
        ];
        if let Some(launch) = launch {
            lines.push(format!("    --launch-product={}", launch.product()));
            lines.push(format!("    --launch-patchline={}", launch.patchline()));
        }
        lines.extend([
            format!("    --session-id=00000000-0000-4000-8000-{run:012}"),
            self.line("OKAY", "ProcessSingleton: Acquired process singleton"),
            self.psl("PendingInitialization"),
            self.psl("PendingClientId"),
            self.psl("PendingPersistenceInitialization"),
        ]);
        lines
    }

    fn patch_line(&self, id: &RiotProductId) -> String {
        let pending = self
            .knobs
            .updates_pending
            .iter()
            .any(|held| *held == id.key());
        self.patch_status(id, if pending { "OutOfDate" } else { "UpToDate" })
    }

    fn patch_status(&self, id: &RiotProductId, status: &str) -> String {
        self.line(
            "OKAY",
            &format!(
                "patch-proxy: HandleProductPatchlineStatusUpdate: productId: {}, patchlineId: {}, status: {status}, progress: 0",
                id.product(),
                id.patchline(),
            ),
        )
    }

    fn restored_lines(&self) -> Vec<String> {
        let mut lines = vec![
            self.psl("RestoreAuthorization"),
            self.psl("PendingEulaData"),
            self.line("OKAY", SESSION_WRITTEN),
            self.psl("PendingPersistentFlagsData"),
            self.psl("PendingProductContext"),
            self.line("OKAY", DEVICE_WRITTEN),
        ];
        lines.extend(
            PRODUCTS
                .iter()
                .filter_map(|id| parse_product_id(id))
                .map(|id| self.patch_line(&id)),
        );
        lines
    }

    // What the client logs for a launch request: the product's context, the patch state and the pipeline as far as WaitForLaunch; then either the launch itself, or the line with which the real client showed the page in place of launching. A product that needs an update only gets its page; one the simulated install does not hold only gets its context.
    fn launch_lines(self: &Arc<Self>, launch: &RiotProductId, boot: bool) -> Vec<String> {
        let direct = boot && self.knobs.direct_launch;
        // At a start the context is set to launch by itself, and the client's own switch takes that back at once.
        let mut lines = if boot && !direct {
            vec![
                self.context_line(launch, true),
                self.context_line(launch, false),
            ]
        } else {
            vec![self.context_line(launch, boot)]
        };
        let key = launch.key();
        let installed = PRODUCTS.contains(&key.as_str());
        let pending = self.knobs.updates_pending.contains(&key);
        let Some(after) = self
            .knobs
            .dependency_update_ms
            .filter(|_| installed && !pending)
        else {
            lines.push(self.patch_line(launch));
            if !installed {
                return lines;
            }
            if pending {
                lines.push(self.line(
                    "OKAY",
                    &format!(
                        "riot-client-lifecycle: Patchline '{key}' is out of date, and needs patching before launch"
                    ),
                ));
                lines.push(self.declined_line(boot, "Product is not up to date"));
                return lines;
            }
            lines.extend(self.page_lines(launch, boot, 0));
            return lines;
        };
        lines.push(self.patch_status(launch, "DependencyUpdate"));
        let check = LAUNCH_PIPELINE
            .iter()
            .position(|step| *step == "PatchStatus")
            .expect("the pipeline has a patch check");
        lines.extend(self.transitions(&LAUNCH_PIPELINE[..=check]));
        lines.push(self.declined_line(boot, "Product is not up to date"));
        let wait = Duration::from_millis(after);
        let now = self.clock.now();
        self.state().checking_until = Some(now + wait);
        let run = self.state().closes;
        let (sim, launch) = (Arc::clone(self), launch.clone());
        self.clock.schedule(wait, move || {
            if sim.state().closes != run {
                return;
            }
            sim.state().checking_until = None;
            let mut lines = vec![sim.patch_line(&launch)];
            lines.extend(sim.page_lines(&launch, boot, check));
            sim.append_log(&lines);
        });
        lines
    }

    fn context_line(&self, launch: &RiotProductId, auto_launch: bool) -> String {
        self.line(
            "ALWAYS",
            &format!(
                "riot-client-lifecycle: Setting product context to {}, willAutoLaunch={auto_launch}",
                launch.key()
            ),
        )
    }

    fn declined_line(&self, boot: bool, reason: &str) -> String {
        self.line(
            "OKAY",
            &if boot {
                format!(
                    "riot-client-lifecycle: Attempting to launch default product with reason ({reason})"
                )
            } else {
                format!(
                    "riot-client-lifecycle: Client is already running. Explicitly request focus instead of launching ({reason})"
                )
            },
        )
    }

    fn transitions(&self, steps: &[&str]) -> Vec<String> {
        steps
            .windows(2)
            .map(|step| {
                self.line(
                    "ALWAYS",
                    &format!(
                        "riot-client-lifecycle: Context #1: Middleware transition ({}) -> ({})",
                        step[0], step[1]
                    ),
                )
            })
            .collect()
    }

    // The pipeline from the step at `from` to WaitForLaunch, then the line with which the real client showed the page in place of launching, or the launch itself for a client that still launches at its start.
    fn page_lines(&self, launch: &RiotProductId, boot: bool, from: usize) -> Vec<String> {
        let mut lines = self.transitions(&LAUNCH_PIPELINE[from..]);
        if !(boot && self.knobs.direct_launch) {
            lines.push(self.declined_line(boot, "Auto-launch is disabled"));
            return lines;
        }
        lines.push(self.context_line(launch, false));
        lines.extend(self.game_start_lines(launch));
        lines
    }

    // The launch itself, the client's own or the user's Play: the game's process, listed by its name alone, and the lines of its start.
    fn game_start_lines(&self, launch: &RiotProductId) -> Vec<String> {
        if let Some(image) = game_images_of(launch.product()).first() {
            self.processes.set_running(image, true);
            self.processes.add_name_to_table(image);
            if let Some(after) = self.knobs.game_quits_after_ms {
                let at = self.clock.now() + Duration::from_millis(after);
                let mut state = self.state();
                state.quits.retain(|(quitting, _)| quitting != image);
                state.quits.push(((*image).to_owned(), at));
            }
        }
        vec![
            self.line(
                "OKAY",
                &format!(
                    "product-launcher: Launch starting: productId='{}' patchlineId='{}'",
                    launch.product(),
                    launch.patchline()
                ),
            ),
            self.line("OKAY", "product-launcher: Stopping all patching jobs"),
            self.line(
                "ALWAYS",
                "riot-client-lifecycle: Context #1: Middleware transition (WaitForLaunch) -> (WaitForSessionExit)",
            ),
        ]
    }

    // Stands in for a press of Play on the game's page of the running, signed-in client, which is what the launch request its Play button sends comes to.
    pub fn press_play(&self, id: &str) -> Result<(), RiotLaunchError> {
        if self.knobs.press_play_fails {
            return Err(RiotLaunchError::Status(500));
        }
        let ready = {
            let state = self.state();
            state.core_running && state.signed_in
        };
        let Some(launch) = parse_product_id(id).filter(|_| ready) else {
            return Err(RiotLaunchError::NoLockfile);
        };
        let now = self.clock.now();
        let checking = self.state().checking_until.is_some_and(|until| now < until);
        if self.knobs.press_play_ignored || checking {
            return Ok(());
        }
        // The page of a product that waits for an update offers Update, not Play: nothing starts.
        if self.knobs.updates_pending.contains(&launch.key()) {
            let lines = [self.patch_line(&launch)];
            self.append_log(&lines);
            return Ok(());
        }
        let lines = self.game_start_lines(&launch);
        self.append_log(&lines);
        Ok(())
    }

    // A launch request handed to the running client: it sets the context and shows the page, never the game. On the login page the request waits for the user.
    fn forwarded_launch(self: &Arc<Self>, launch: &RiotProductId) {
        if !self.state().signed_in {
            return;
        }
        let lines = self.launch_lines(launch, false);
        self.append_log(&lines);
    }

    fn rejects(&self, session: &RiotSession) -> bool {
        let rejected = &self.knobs.rejected;
        rejected.iter().any(|refused| {
            *refused == session.account_id || refused.to_lowercase() == session.fingerprint
        })
    }

    fn react(self: &Arc<Self>, launch: Option<&RiotProductId>) {
        self.processes.set_running(WINDOW_EXE, true);
        {
            let mut state = self.state();
            state.core_running = true;
            state.forget_at_exit = false;
        }
        let session = self.current_session();
        let mut lines = self.start_lines(launch);
        let mut signed_in = false;
        match (&session, &self.knobs.auth_error) {
            (Some(_), Some(error)) => lines.extend([
                self.line(
                    "ERROR",
                    &format!(
                        "SDK: rso-auth: error '{error}': in plugin library function HandleAuthorizationCreate: request failed"
                    ),
                ),
                self.psl("RestoreAuthorization"),
                self.psl("PendingLoginStrategy"),
            ]),
            (Some(session), None) if self.rejects(session) => {
                lines.extend([
                    self.line("ERROR", REFUSED),
                    self.line("OKAY", SESSION_WRITTEN),
                    self.psl("RestoreAuthorization"),
                    self.psl("PendingLoginStrategy"),
                ]);
                self.write_signed_out();
            }
            (Some(session), None) => {
                if self.knobs.rotate_on_start {
                    self.write_session_for(
                        &session.account_id,
                        session.write_count.unwrap_or(0) + 1,
                    );
                }
                lines.extend(self.restored_lines());
                signed_in = true;
            }
            (None, _) => lines.extend([
                self.line("OKAY", SESSION_WRITTEN),
                self.psl("PendingLoginStrategy"),
            ]),
        }
        self.state().signed_in = signed_in;
        if let (true, Some(launch)) = (signed_in, launch) {
            lines.extend(self.launch_lines(launch, true));
        }
        self.write_log(&lines);
        if !signed_in && let Some(after) = self.knobs.sign_in_when_asked_ms {
            let (sim, run) = (Arc::clone(self), self.state().closes);
            self.clock.schedule(Duration::from_millis(after), move || {
                if sim.state().closes == run {
                    sim.sign_in_by_hand(None);
                }
            });
        }
    }

    // Stands in for the user signing in on the client's login page. Without "Stay signed in" the client forgets the sign-in when it exits.
    pub fn sign_in_by_hand(&self, account: Option<&str>) {
        let uid = account
            .or(self.knobs.sign_in_as.as_deref())
            .expect("a sign-in by hand needs an account id: pass one or set signInAs");
        self.write_session_for(uid, 1);
        {
            let mut state = self.state();
            state.forget_at_exit = !self.knobs.stay_signed_in;
            state.signed_in = true;
        }
        let lines = [
            self.line("OKAY", "SDK: rso-auth: Using direct token exchange"),
            self.psl("LoginTokenExchange"),
            self.line("OKAY", SESSION_WRITTEN),
            self.psl("PendingEulaData"),
            self.psl("PendingPersistentFlagsData"),
            self.psl("PendingProductContext"),
            self.line("OKAY", DEVICE_WRITTEN),
        ];
        self.append_log(&lines);
    }

    // The client's clean exit: its last lines, and the file forgotten when "Stay signed in" was not ticked.
    fn core_closed(&self) {
        let forget = {
            let mut state = self.state();
            state.closes += 1;
            state.core_running = false;
            state.signed_in = false;
            if state.log_file.is_none() {
                return;
            }
            std::mem::take(&mut state.forget_at_exit)
        };
        if forget {
            self.write_signed_out();
        }
        let lines = [
            self.line(
                "OKAY",
                "riot-client-lifecycle: Attempting to quit. SwitchToBackgroundMode: False",
            ),
            self.line(
                "ALWAYS",
                &format!("ProcessSingleton: Removal of LockFile complete: \"{LOCKFILE}\""),
            ),
        ];
        self.append_log(&lines);
    }
}

#[cfg(test)]
mod tests {
    use ol_testkit::oracle;
    use ol_testkit::vectors::sha256_hex;
    use serde_json::Value;

    use super::*;

    fn client(
        scratch: &ol_testkit::Scratch,
        knobs: RiotKnobs,
    ) -> (Arc<RiotSim>, Arc<FakeProcesses>, Arc<FakeClock>) {
        let folder = |name: &str| scratch.join(name).to_string_lossy().into_owned();
        let (local, program, install, games) = (
            folder("local"),
            folder("program"),
            folder("install"),
            folder("games"),
        );
        let folders = RiotFolders {
            local_app_data: &local,
            program_data: &program,
            install_dir: &install,
            games: &games,
        };
        let processes = Arc::new(FakeProcesses::new(
            ol_platform_fake::OpLog::new(),
            ol_platform_fake::ProcessKnobs::default(),
        ));
        let clock = Arc::new(FakeClock::starting_at(OffsetDateTime::UNIX_EPOCH));
        let sim = RiotSim::attach(&folders, &processes, Arc::clone(&clock), knobs);
        (sim, processes, clock)
    }

    #[test]
    fn a_start_or_a_sign_in_scheduled_for_a_run_that_was_closed_since_does_nothing() {
        use ol_platform_api::{CancelToken, ProcessLister, ProcessRunner};
        let scratch = ol_testkit::Scratch::new("sim-riot-closed");
        let knobs = RiotKnobs {
            react_ms: 400,
            ..RiotKnobs::default()
        };
        let (_sim, processes, clock) = client(&scratch, knobs);
        processes.start(EXE, &[], "").unwrap();
        processes.close(EXE).unwrap();
        clock.sleep(Duration::from_secs(1), &CancelToken::new());
        assert!(!processes.is_running(WINDOW_EXE).unwrap());
        assert!(
            !scratch
                .join("local")
                .join("Riot Games")
                .join("Riot Client")
                .join("Logs")
                .exists()
        );
    }

    #[test]
    fn play_on_the_page_of_a_product_that_waits_for_an_update_starts_no_game() {
        use ol_platform_api::ProcessLister;
        let scratch = ol_testkit::Scratch::new("sim-riot-update");
        let knobs = RiotKnobs {
            running_as: Some("11111111-1111-4111-8111-111111111111".to_owned()),
            updates_pending: vec!["valorant.live".to_owned()],
            ..RiotKnobs::default()
        };
        let (sim, processes, _clock) = client(&scratch, knobs);
        sim.press_play("valorant.live").unwrap();
        let image = game_images_of("valorant")[0];
        assert!(!processes.is_running(image).unwrap());
    }

    fn recorded(relative: &str) -> Value {
        serde_json::from_str(&fs::read_to_string(oracle(relative)).unwrap()).unwrap()
    }

    // The recorded run wrote the file the scenario saves for Riot Alt over the client's own, and its digest is in the recording: the file made here is that file, byte for byte.
    #[test]
    fn the_file_a_scenario_saves_for_an_account_is_the_one_the_recorded_run_wrote() {
        let scenario = recorded("fixtures/dev-only/fake-launchers/all-four/scenario.json");
        let sessions = scenario["seed"]["sessions"].as_array().unwrap();
        let saved = sessions
            .iter()
            .find(|session| session["launcher"] == "riot")
            .expect("the scenario saves a Riot sign-in");
        let file = fake_session_file(
            saved["launcherUid"].as_str().unwrap(),
            saved["refreshToken"].as_str().unwrap(),
        );
        let run = recorded("l2/expected/play-riot.json");
        let written = run["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| {
                let path = event["path"].as_str().unwrap_or_default();
                event["op"] == "file.write" && path.ends_with(SESSION_FILE)
            })
            .expect("the run wrote the session file");
        assert_eq!(
            sha256_hex(file.as_bytes()),
            written["sha256"].as_str().unwrap()
        );
        let SessionRead::Session(session) = read_session(&file) else {
            panic!("the file holds a session");
        };
        assert_eq!(session.account_id, saved["launcherUid"].as_str().unwrap());
        assert_eq!(session.write_count, Some(1));
    }
}
