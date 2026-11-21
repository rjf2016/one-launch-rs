#![forbid(unsafe_code)]

// Saving the sign-in a launcher holds now from an account's page, and forgetting a saved one, on fake launchers: a save leaves the row and the sealed sign-in a switch leaves for the same state, reads the launcher and writes nothing to it, and refuses with the Electron app's words; a forget takes the account's row and nothing else.

use std::collections::BTreeMap;
use std::fs;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

use futures_lite::future::block_on;
use ol_api::accounts::AccountPatch;
use ol_api::launchers::{
    BattlenetMachine, BattlenetTiming, EpicMachine, EpicTiming, Launchers, NewAccount, RiotMachine,
    RiotTiming, Secret, SteamTiming,
};
use ol_api::{ApiError, Backend, StartLaunch};
use ol_model::{Account, JobState, LaunchJob, LauncherId, LauncherSession};
use ol_sim::FakeLaunchers;
use ol_store::{Store, database_path};
use ol_testkit::{Scratch, oracle, snapshot};
use ol_vault::{FakeVault, Vault};
use serde_json::Value;
use time::macros::datetime;

const VERSION: &str = "9.8.7";
const SCENARIO: &str = "fixtures/dev-only/fake-launchers/all-four";
const RIOT_SESSION_FILE: &str =
    "localappdata/Riot Games/Riot Client/Data/RiotGamesPrivateSettings.yaml";
const EPIC_SETTINGS: &str =
    "localappdata/EpicGamesLauncher/Saved/Config/Windows/GameUserSettings.ini";
const NEW_UID: &str = "44444444-4444-4444-8444-444444444444";

struct Fakes {
    scratch: Scratch,
    fake: FakeLaunchers,
    backend: Backend,
    jobs: Receiver<LaunchJob>,
}

// What a saved sign-in is in the database: its row and its sealed sign-in.
type Saved = (LauncherSession, Vec<u8>);

impl Fakes {
    fn account(&self, launcher: LauncherId, login: &str) -> Account {
        block_on(self.backend.accounts())
            .unwrap()
            .into_iter()
            .find(|account| account.launcher == launcher && account.login == login)
            .expect("the scenario seeds this account")
    }

    fn capture(
        &self,
        launcher: LauncherId,
        login: &str,
    ) -> Result<ol_api::CapturedSignIn, ApiError> {
        block_on(
            self.backend
                .capture_session(&self.account(launcher, login).id),
        )
    }

    fn switch(&self, launcher: LauncherId, login: &str) -> LaunchJob {
        let start = StartLaunch {
            account_id: self.account(launcher, login).id,
            game_id: None,
            confirm_close: true,
        };
        block_on(self.backend.start_launch(start)).expect("the switch is accepted");
        loop {
            let job = self
                .jobs
                .recv()
                .expect("the job ends before its observer goes");
            if job.state != JobState::Running {
                return job;
            }
        }
    }

    fn launchers_folder(&self) -> BTreeMap<String, Vec<u8>> {
        snapshot(&self.scratch.join("launchers"))
    }

    // What was asked of the fake machine that changes something: a write, a start, a close or an end.
    fn changes_asked(&self) -> Vec<String> {
        self.fake
            .log
            .snapshot()
            .iter()
            .filter_map(|event| event["op"].as_str().map(str::to_owned))
            .filter(|op| {
                [
                    ".set", ".delete", ".write", ".start", ".close", ".kill", "shell.",
                ]
                .iter()
                .any(|change| op.contains(change))
            })
            .collect()
    }

    // The app quits; then every saved sign-in, by the login of its account.
    fn quit_and_read(self) -> BTreeMap<(LauncherId, String), Saved> {
        self.backend.shut_down();
        let opened = Store::open_read_only(&database_path(&self.scratch.join("data"))).unwrap();
        let store = opened.store;
        let mut saved = BTreeMap::new();
        for account in store.accounts().unwrap() {
            if let Some(session) = store.session(&account.id).unwrap() {
                let blob = store.session_blob(&account.id).unwrap().unwrap();
                saved.insert(
                    (account.launcher, account.login.clone()),
                    (session, blob.as_bytes().to_vec()),
                );
            }
        }
        saved
    }
}

fn fakes() -> Fakes {
    fakes_with(&|_| {})
}

fn fakes_with(prepare: &dyn Fn(&std::path::Path)) -> Fakes {
    let scratch = Scratch::new("api-sessions");
    let launchers_dir = scratch.join("launchers");
    ol_sim::lay_down(
        &oracle(SCENARIO),
        &oracle("fixtures/launchers"),
        &launchers_dir,
    )
    .unwrap();
    prepare(&launchers_dir);
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
    )
    .with_local_launch(fake.riot_launch.clone());
    let (changed, jobs) = channel();
    let changed = Mutex::new(changed);
    let backend = Backend::start_with(
        scratch.join("data"),
        VERSION,
        launchers,
        Some(Arc::new(move |job: &LaunchJob| {
            let _ = changed.lock().unwrap().send(job.clone());
        })),
    );
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
            .find(|account| account.launcher == session.launcher && account.login == session.login)
            .unwrap();
        block_on(backend.seed_session(
            &owner.id,
            Some(session.launcher_uid.clone()),
            Secret::new(session.blob.clone()),
        ))
        .unwrap();
    }
    block_on(backend.rescan()).unwrap();
    Fakes {
        scratch,
        fake,
        backend,
        jobs,
    }
}

// The client's own signed-out state: the `riot-client` block replaced by `null`, every other line as it was.
fn signed_out(text: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for line in text.split_inclusive('\n') {
        let indent = line.len() - line.trim_start().len();
        if inside && (line.trim().is_empty() || indent > 8) {
            continue;
        }
        inside = false;
        if line.trim_end() == "        riot-client:" {
            let end = &line[line.trim_end().len()..];
            out.push_str("        riot-client: null");
            out.push_str(end);
            inside = true;
            continue;
        }
        out.push_str(line);
    }
    out
}

fn refusal<T>(answer: Result<T, ApiError>) -> (&'static str, String) {
    let error = answer.err().expect("a refusal");
    (error.code, error.message)
}

// The account each launcher's files hold a sign-in for as the scenario lays them down, and another account of the same launcher a switch goes to.
const HOLDERS: [(LauncherId, &str, &str); 3] = [
    (LauncherId::Epic, "main@example.com", "alt@example.com"),
    (
        LauncherId::Battlenet,
        "saved1@example.com",
        "saved3@example.com",
    ),
    (LauncherId::Riot, "riot_main", "riot_alt"),
];

#[test]
fn a_saved_sign_in_is_the_row_and_the_sealed_sign_in_a_switch_saves_for_the_same_state() {
    for (launcher, holder, other) in HOLDERS {
        let fakes = self::fakes();
        let (before, asked) = (fakes.launchers_folder(), fakes.changes_asked());
        let captured = fakes.capture(launcher, holder).unwrap();
        assert_eq!(captured.session.launcher, launcher);
        assert_eq!(captured.account.id, fakes.account(launcher, holder).id);
        assert!(captured.account.has_session);
        assert!(
            fakes.launchers_folder() == before,
            "a save from the account's page changed a file of {launcher:?}"
        );
        assert_eq!(fakes.changes_asked(), asked);
        let by_save = fakes.quit_and_read();

        let fakes = self::fakes();
        let job = fakes.switch(launcher, other);
        assert_eq!(job.state, JobState::Done, "{launcher:?}: {:?}", job.error);
        let by_switch = fakes.quit_and_read();

        let key = (launcher, holder.to_owned());
        let (saved, sealed) = by_save.get(&key).expect("the save stored a sign-in");
        let (switched, sealed_by_switch) = by_switch
            .get(&key)
            .expect("the switch stored the outgoing sign-in");
        let row = |session: &LauncherSession| {
            (
                session.launcher,
                session.launcher_uid.clone(),
                session.fingerprint.clone(),
                session.verified_at.clone(),
                session.stale_reason.clone(),
            )
        };
        assert_eq!(row(saved), row(switched), "{launcher:?}");
        assert!(
            sealed == sealed_by_switch,
            "{launcher:?}: the sealed sign-in differs from the one a switch seals"
        );
        let opened = FakeVault
            .decrypt(&ol_model::Ciphertext::new(sealed.clone()))
            .unwrap();
        let blob: Value = serde_json::from_str(opened.expose()).unwrap();
        assert_eq!(blob["v"], 1);
        assert!(saved.verified_at.is_none() && saved.stale_reason.is_none());
        assert!(saved.captured_at.ends_with('Z'));
    }
}

#[test]
fn a_save_records_the_id_the_launcher_names_for_an_account_that_had_none() {
    let fakes = fakes_with(&|launchers| {
        let file = launchers.join(RIOT_SESSION_FILE);
        let text = String::from_utf8(fs::read(&file).unwrap()).unwrap();
        let block = ol_sim::session_block_lines(
            &ol_sim::fake_id_token(NEW_UID, "Brightwing", "BW11"),
            &format!("made-up-token-of-riot-new-{}", "n".repeat(48)),
            1,
            1_760_000_000_000,
        );
        fs::write(&file, ol_sim::session_file_with(&text, &block)).unwrap();
    });
    let new = fakes.account(LauncherId::Riot, "riot_new");
    assert_eq!(new.launcher_uid, None);
    let captured = fakes.capture(LauncherId::Riot, "riot_new").unwrap();
    assert_eq!(captured.account.launcher_uid.as_deref(), Some(NEW_UID));
    assert_eq!(captured.session.launcher_uid.as_deref(), Some(NEW_UID));
    assert_eq!(captured.session.account_id, new.id);
    let state = block_on(fakes.backend.session(&new.id)).unwrap();
    assert_eq!(
        state.session.unwrap().fingerprint,
        captured.session.fingerprint
    );
    assert_eq!(
        state.state.unwrap().linked_account_id.as_deref(),
        Some(new.id.as_str())
    );
    fakes.backend.shut_down();
}

#[test]
fn a_save_for_another_account_than_the_one_signed_in_is_refused_and_stores_nothing() {
    let cases = [
        (
            LauncherId::Epic,
            "alt@example.com",
            "Epic Games: The session Epic Games Launcher currently remembers belongs to Main. Sign in to the launcher as alt@example.com first.",
        ),
        (
            LauncherId::Battlenet,
            "saved3@example.com",
            "Battle.net: The session Battle.net currently remembers belongs to Main. Sign in to the client as saved3@example.com first.",
        ),
        (
            LauncherId::Riot,
            "riot_alt",
            "Riot Games: The sign-in the Riot Client has saved belongs to Riot Main. Sign in to the Riot Client as riot_alt first.",
        ),
    ];
    for (launcher, login, message) in cases {
        let fakes = fakes();
        let before = block_on(fakes.backend.session(&fakes.account(launcher, login).id))
            .unwrap()
            .session;
        assert_eq!(
            refusal(fakes.capture(launcher, login)),
            ("SWITCH_BLOCKED", message.to_owned())
        );
        let after = block_on(fakes.backend.session(&fakes.account(launcher, login).id))
            .unwrap()
            .session;
        assert!(
            after == before,
            "{launcher:?}: a refused save changed what was saved"
        );
        fakes.backend.shut_down();
    }
}

#[test]
fn a_save_is_refused_with_the_electron_apps_words_when_there_is_nothing_to_save() {
    let fakes = fakes_with(&|launchers| fs::remove_file(launchers.join(EPIC_SETTINGS)).unwrap());
    assert_eq!(
        refusal(fakes.capture(LauncherId::Epic, "main@example.com")),
        (
            "SWITCH_BLOCKED",
            "Epic Games: Epic Games Launcher is not remembering a session right now. Sign in to it as main@example.com, exit it once so it saves the session, then save again.".to_owned()
        )
    );
    fakes.backend.shut_down();

    let fakes = self::fakes();
    fakes
        .fake
        .registry
        .remove(r"HKCU\Software\Blizzard Entertainment\Battle.net\UnifiedAuth\AA11BB22");
    assert_eq!(
        refusal(fakes.capture(LauncherId::Battlenet, "saved1@example.com")),
        (
            "SWITCH_BLOCKED",
            r#"Battle.net: Battle.net has no remembered session right now (nothing under HKCU\Software\Blizzard Entertainment\Battle.net\UnifiedAuth). Sign in to the client as saved1@example.com with "Keep me logged in" on, then save again."#.to_owned()
        )
    );
    fakes.backend.shut_down();

    let fakes = fakes_with(&|launchers| {
        let file = launchers.join(RIOT_SESSION_FILE);
        let text = String::from_utf8(fs::read(&file).unwrap()).unwrap();
        fs::write(&file, signed_out(&text)).unwrap();
    });
    let signed_out = fakes.capture(LauncherId::Riot, "riot_main");
    assert_eq!(
        refusal(signed_out),
        (
            "SWITCH_BLOCKED",
            "Riot Games: The Riot Client has no saved sign-in. Sign in to it as riot_main with “Stay signed in” ticked, exit the client, then save again.".to_owned()
        )
    );
    fakes.backend.shut_down();
}

#[test]
fn a_riot_sign_in_is_saved_only_with_the_client_closed_and_never_beside_a_riot_game() {
    let fakes = fakes();
    let main = fakes.account(LauncherId::Riot, "riot_main");
    let (before, asked) = (fakes.launchers_folder(), fakes.changes_asked());
    fakes
        .fake
        .processes
        .set_running("RiotClientServices.exe", true);
    assert_eq!(
        refusal(fakes.capture(LauncherId::Riot, "riot_main")),
        (
            "SWITCH_BLOCKED",
            "Riot Games: The Riot Client is running. Right-click its tray icon, choose Exit, then save again.".to_owned()
        )
    );
    fakes
        .fake
        .processes
        .set_running("RiotClientServices.exe", false);
    fakes
        .fake
        .processes
        .add_name_to_table("VALORANT-Win64-Shipping.exe");
    assert_eq!(
        refusal(fakes.capture(LauncherId::Riot, "riot_main")),
        (
            "GAME_RUNNING",
            "Riot Games: VALORANT is running. Close it, then save again: One Launch never closes a Riot game.".to_owned()
        )
    );
    fakes
        .fake
        .processes
        .remove_from_table("VALORANT-Win64-Shipping.exe");
    fakes.fake.processes.fail_listings(true);
    let unlisted = refusal(fakes.capture(LauncherId::Riot, "riot_main"));
    assert_eq!(unlisted.0, "ADAPTER_FAILED");
    fakes.fake.processes.fail_listings(false);
    assert!(
        block_on(fakes.backend.session(&main.id))
            .unwrap()
            .session
            .is_none()
    );
    assert!(fakes.launchers_folder() == before);
    assert_eq!(fakes.changes_asked(), asked);

    // A client that starts while the file is read: the second look refuses the save.
    let processes = Arc::clone(&fakes.fake.processes);
    let looks = Arc::new(Mutex::new(0));
    let counted = Arc::clone(&looks);
    fakes.fake.processes.on_look(move || {
        let mut looks = counted.lock().unwrap();
        *looks += 1;
        if *looks == 2 {
            processes.set_running("RiotClientServices.exe", true);
        }
    });
    let started_meanwhile = refusal(fakes.capture(LauncherId::Riot, "riot_main"));
    assert_eq!(started_meanwhile.0, "SWITCH_BLOCKED");
    assert!(
        block_on(fakes.backend.session(&main.id))
            .unwrap()
            .session
            .is_none()
    );
    fakes.backend.shut_down();
}

#[test]
fn a_save_and_a_forget_are_refused_while_a_switch_runs_and_for_steam_and_an_unknown_account() {
    let fakes = fakes();
    let main = fakes.account(LauncherId::Epic, "main@example.com");
    let alt = fakes.account(LauncherId::Epic, "alt@example.com");
    let asked = Arc::new(Mutex::new(None));
    let (backend, seen) = (fakes.backend.clone(), Arc::clone(&asked));
    let (main_id, alt_id) = (main.id.clone(), alt.id.clone());
    fakes.fake.clock.before_sleep(move || {
        let mut seen = seen.lock().unwrap();
        if seen.is_none() {
            // Kept as answers: a panic here would end the switch's own thread and leave the job running for good.
            let answer =
                |refused: Option<ApiError>| refused.map(|error| (error.code, error.message));
            *seen = Some((
                answer(block_on(backend.capture_session(&main_id)).err()),
                answer(block_on(backend.forget_session(&alt_id)).err()),
            ));
        }
    });
    let job = fakes.switch(LauncherId::Epic, "alt@example.com");
    assert_eq!(job.state, JobState::Done);
    let busy = (
        "BUSY",
        "Another switch or launch is still running".to_owned(),
    );
    assert_eq!(
        asked.lock().unwrap().clone(),
        Some((Some(busy.clone()), Some(busy)))
    );

    let steam = fakes.account(LauncherId::Steam, "user_1");
    assert_eq!(
        refusal(block_on(fakes.backend.capture_session(&steam.id))),
        (
            "NOT_IMPLEMENTED",
            "The Steam adapter is not available yet".to_owned()
        )
    );
    assert_eq!(
        refusal(block_on(fakes.backend.capture_session("no-such-account"))).0,
        "NOT_FOUND"
    );
    assert_eq!(
        refusal(block_on(fakes.backend.forget_session("no-such-account"))).0,
        "NOT_FOUND"
    );
    fakes.backend.shut_down();
}

#[test]
fn a_linked_id_the_launcher_does_not_name_refuses_the_save() {
    let fakes = fakes();
    let main = fakes.account(LauncherId::Riot, "riot_main");
    // Riot Main's id moves to another account first, so the file's sign-in belongs to nobody, and then Riot New is given an id the file does not name.
    let unlink = AccountPatch {
        launcher_uid: Some(None),
        ..AccountPatch::default()
    };
    block_on(fakes.backend.update_account(&main.id, unlink)).unwrap();
    let relink = AccountPatch {
        launcher_uid: Some(Some(NEW_UID.to_owned())),
        ..AccountPatch::default()
    };
    let new = fakes.account(LauncherId::Riot, "riot_new");
    block_on(fakes.backend.update_account(&new.id, relink)).unwrap();
    assert_eq!(
        refusal(fakes.capture(LauncherId::Riot, "riot_new")),
        (
            "SWITCH_BLOCKED",
            "Riot Games: The Riot Client's saved sign-in is Riot account …1111, but Riot New is linked to …4444. Check the Riot account id on the account page.".to_owned()
        )
    );
    fakes.backend.shut_down();
}

#[test]
fn a_forget_takes_the_accounts_saved_sign_in_and_nothing_else() {
    for (launcher, login) in [
        (LauncherId::Epic, "alt@example.com"),
        (LauncherId::Battlenet, "saved3@example.com"),
        (LauncherId::Riot, "riot_alt"),
    ] {
        let fakes = fakes();
        let account = fakes.account(launcher, login);
        assert!(account.has_session);
        let (folder, asked) = (fakes.launchers_folder(), fakes.changes_asked());
        let accounts_before = block_on(fakes.backend.accounts()).unwrap();
        block_on(fakes.backend.forget_session(&account.id)).unwrap();
        assert!(fakes.launchers_folder() == folder);
        assert_eq!(fakes.changes_asked(), asked);
        let mut accounts_after = block_on(fakes.backend.accounts()).unwrap();
        for after in &mut accounts_after {
            if after.id == account.id {
                assert!(!after.has_session);
                after.has_session = true;
            }
        }
        assert!(
            accounts_after == accounts_before,
            "{launcher:?}: a forget changed an account"
        );
        // Forgetting again, and forgetting for an account that has none, is not an error.
        block_on(fakes.backend.forget_session(&account.id)).unwrap();
        let steam = fakes.account(LauncherId::Steam, "user_1");
        block_on(fakes.backend.forget_session(&steam.id)).unwrap();

        let reference = self::fakes();
        let mut expected = reference.quit_and_read();
        assert!(expected.remove(&(launcher, login.to_owned())).is_some());
        let left = fakes.quit_and_read();
        let without_times = |saved: &BTreeMap<(LauncherId, String), Saved>| {
            saved
                .iter()
                .map(|(key, (session, sealed))| {
                    let mut session = session.clone();
                    session.captured_at.clear();
                    session.account_id.clear();
                    (key.clone(), session, sealed.clone())
                })
                .collect::<Vec<_>>()
        };
        assert!(
            without_times(&left) == without_times(&expected),
            "{launcher:?}: a forget changed another saved sign-in"
        );
    }
}
