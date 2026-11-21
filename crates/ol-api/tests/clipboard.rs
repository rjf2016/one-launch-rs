#![forbid(unsafe_code)]

// Copy login and Copy password through the backend, on a copy of the fixture data folder and a recording clipboard: what reaches the clipboard and what the answer says, the clear 30 s later and at quit, and the refusals with their codes.

use std::sync::Arc;
use std::time::Duration;

use futures_lite::future::block_on;
use ol_api::clipboard::{CopyField, Timer};
use ol_api::launchers::Launchers;
use ol_api::{ApiError, Backend};
use ol_model::Account;
use ol_platform_api::{CancelToken, Clock, Platform};
use ol_platform_fake::{
    FakeClock, FakeProcesses, FakeRegistry, FakeShell, Kept, OpLog, ProcessKnobs,
    RecordingClipboard, ScratchFiles,
};
use ol_store::{Store, database_path};
use ol_testkit::{Scratch, oracle};
use ol_vault::{FakeVault, Vault};
use time::macros::datetime;

const VERSION: &str = "9.8.7";

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
    _scratch: Scratch,
    clock: Arc<FakeClock>,
    clipboard: Arc<RecordingClipboard>,
    log: OpLog,
    backend: Backend,
}

impl Run {
    fn wait(&self, seconds: u64) {
        self.clock
            .sleep(Duration::from_secs(seconds), &CancelToken::new());
    }

    fn account(&self, has_secret: bool) -> Account {
        block_on(self.backend.accounts())
            .unwrap()
            .into_iter()
            .find(|account| account.has_secret == has_secret)
            .unwrap()
    }

    // The password as the fixture stores it, opened here and not by the code under test.
    fn stored_password(&self, account: &Account) -> String {
        let reference =
            Scratch::with_copy_of("api-clipboard-reference", &oracle("fixtures/data-folder"));
        let opened = Store::open_read_only(&database_path(reference.path())).unwrap();
        let sealed = opened.store.account_password(&account.id).unwrap().unwrap();
        FakeVault.decrypt(&sealed).unwrap().expose().to_owned()
    }

    fn copy(
        &self,
        account: &Account,
        field: CopyField,
    ) -> Result<ol_api::clipboard::CopyResult, ApiError> {
        block_on(self.backend.copy_to_clipboard(&account.id, field))
    }

    fn ops(&self) -> Vec<String> {
        self.log
            .snapshot()
            .iter()
            .map(|event| event["op"].as_str().unwrap().to_owned())
            .collect()
    }
}

fn run_with(start: impl FnOnce(&Scratch, Platform) -> Backend, clipboard: bool) -> Run {
    let scratch = Scratch::new("api-clipboard");
    ol_testkit::copy_tree(&oracle("fixtures/data-folder"), &scratch.join("data"));
    let clock = Arc::new(FakeClock::starting_at(datetime!(2026-11-21 10:00:00 UTC)));
    let log = OpLog::new();
    let platform = Platform {
        registry: Arc::new(FakeRegistry::new(log.clone())),
        processes: Arc::new(FakeProcesses::new(log.clone(), ProcessKnobs::default())),
        files: Arc::new(ScratchFiles::new(&[scratch.path()], log.clone())),
        shell: Arc::new(FakeShell::new(log.clone())),
        clock: Arc::clone(&clock) as Arc<dyn Clock>,
    };
    let backend = start(&scratch, platform);
    let recording = Arc::new(RecordingClipboard::new(log.clone()));
    if clipboard {
        backend.use_clipboard(
            Arc::clone(&recording) as _,
            Arc::new(OnTheClock(Arc::clone(&clock))),
        );
    }
    Run {
        _scratch: scratch,
        clock,
        clipboard: recording,
        log,
        backend,
    }
}

fn run() -> Run {
    run_with(
        |scratch, platform| {
            Backend::start_with(
                scratch.join("data"),
                VERSION,
                Launchers::on(platform, Arc::new(FakeVault)),
                None,
            )
        },
        true,
    )
}

fn refusal<T>(answer: Result<T, ApiError>) -> (&'static str, String) {
    let error = answer.err().expect("a refusal");
    (error.code, error.message)
}

#[test]
fn a_copied_password_is_the_stored_one_kept_out_of_the_history_and_gone_30_s_later() {
    let run = run();
    let account = run.account(true);
    let login = run.copy(&account, CopyField::Login).unwrap();
    assert!(login.history_excluded);
    assert_eq!(login.clear_after, Duration::from_secs(30));
    assert!(run.clipboard.holds(&account.login));

    let copied = run.copy(&account, CopyField::Password).unwrap();
    assert!(copied.history_excluded);
    assert_eq!(copied.clear_after.as_millis(), 30_000);
    let password = run.stored_password(&account);
    assert!(
        run.clipboard.holds(&password),
        "the clipboard does not hold the stored password"
    );
    assert_eq!(run.clipboard.kept(), Some(Kept::Private));
    run.wait(29);
    assert!(run.clipboard.holds(&password));
    run.wait(1);
    assert!(run.clipboard.is_empty());
    assert_eq!(run.ops(), ["clip.write", "clip.write", "clip.clear"]);
    run.backend.shut_down();
}

#[test]
fn a_quit_takes_a_copy_still_there_off_the_clipboard_and_leaves_the_users_own() {
    let run = run();
    let account = run.account(true);
    run.copy(&account, CopyField::Password).unwrap();
    run.wait(5);
    run.backend.shut_down();
    assert!(run.clipboard.is_empty());
    assert_eq!(run.ops(), ["clip.write", "clip.clear"]);

    let run = self::run();
    let account = run.account(true);
    run.copy(&account, CopyField::Password).unwrap();
    run.clipboard.user_copies("the user's own text");
    run.backend.shut_down();
    assert!(run.clipboard.holds("the user's own text"));
    assert_eq!(run.ops(), ["clip.write"]);
}

#[test]
fn a_copy_is_refused_with_its_code_and_nothing_reaches_the_clipboard() {
    let run = run();
    let without = run.account(false);
    assert_eq!(
        refusal(run.copy(&without, CopyField::Password)),
        (
            "NO_PASSWORD",
            "No password is stored for this account".to_owned()
        )
    );
    assert_eq!(
        refusal(block_on(
            run.backend
                .copy_to_clipboard("no-such-account", CopyField::Login)
        ))
        .0,
        "NOT_FOUND"
    );
    assert!(run.clipboard.is_empty());
    run.backend.shut_down();

    // A backend that was handed no clipboard, as a capture or a probe run is not, copies nothing.
    let run = run_with(
        |scratch, platform| {
            Backend::start_with(
                scratch.join("data"),
                VERSION,
                Launchers::on(platform, Arc::new(FakeVault)),
                None,
            )
        },
        false,
    );
    let account = run.account(true);
    assert_eq!(
        refusal(run.copy(&account, CopyField::Password)).0,
        "CLIPBOARD_UNAVAILABLE"
    );
    run.backend.shut_down();

    // Without a vault a login is still copied, and a password is refused.
    let run = run_with(
        |scratch, _| Backend::start(scratch.join("data"), VERSION),
        true,
    );
    let account = run.account(true);
    assert_eq!(
        refusal(run.copy(&account, CopyField::Password)),
        (
            "VAULT_UNAVAILABLE",
            "Secure storage is not available on this system. Secrets cannot be stored.".to_owned()
        )
    );
    assert!(run.clipboard.is_empty());
    run.copy(&account, CopyField::Login).unwrap();
    assert!(run.clipboard.holds(&account.login));
    run.backend.shut_down();
}
