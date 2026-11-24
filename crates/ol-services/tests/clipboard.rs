#![forbid(unsafe_code)]

// A copy to the clipboard, on a recording clipboard and a clock the test moves: the private write and what the answer says of it, the plain write when Windows refuses the private one, the clear 30 s later only while the clipboard still holds the copy, a newer copy putting the clear off, and the clear at quit. On a copy of the fixture data folder: what is copied for a login and a password, and each refusal.

use std::sync::Arc;
use std::time::Duration;

use ol_model::{Account, Ciphertext};
use ol_platform_api::{CancelToken, ClipboardError, Clock};
use ol_platform_fake::{FakeClock, Kept, OpLog, RecordingClipboard};
use ol_services::clipboard::{CLEAR_AFTER, ClipboardService, CopyField, Timer, copy_account_field};
use ol_store::{Store, database_path};
use ol_testkit::{Scratch, oracle};
use ol_vault::{FakeVault, Secret, Vault, VaultError};
use time::macros::datetime;

const PASSWORD: &str = "placeholder-password-of-the-test";

// Runs what is due on the fake clock, when the test sleeps past it.
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

struct Board {
    clock: Arc<FakeClock>,
    log: OpLog,
    clipboard: Arc<RecordingClipboard>,
    service: ClipboardService,
}

impl Board {
    fn wait(&self, seconds: u64) {
        self.clock
            .sleep(Duration::from_secs(seconds), &CancelToken::new());
    }

    fn ops(&self) -> Vec<String> {
        self.log
            .snapshot()
            .iter()
            .map(|event| event["op"].as_str().unwrap().to_owned())
            .collect()
    }
}

fn board_with(clipboard: impl FnOnce(OpLog) -> RecordingClipboard) -> Board {
    let clock = Arc::new(FakeClock::starting_at(datetime!(2026-11-21 10:00:00 UTC)));
    let log = OpLog::new();
    let clipboard = Arc::new(clipboard(log.clone()));
    let service = ClipboardService::new(
        Arc::clone(&clipboard) as _,
        Arc::new(OnTheClock(Arc::clone(&clock))),
    );
    Board {
        clock,
        log,
        clipboard,
        service,
    }
}

fn board() -> Board {
    board_with(RecordingClipboard::new)
}

fn secret(text: &str) -> Secret {
    Secret::new(text.to_owned())
}

#[test]
fn a_copy_is_kept_out_of_the_history_and_taken_off_after_30_s_when_it_is_still_there() {
    let board = board();
    let copied = board.service.copy_sensitive(&secret(PASSWORD)).unwrap();
    assert_eq!(copied.clear_after, Duration::from_secs(30));
    assert_eq!(CLEAR_AFTER, Duration::from_secs(30));
    assert!(copied.history_excluded);
    assert_eq!(board.clipboard.kept(), Some(Kept::Private));
    assert!(board.clipboard.holds(PASSWORD));

    board.wait(29);
    assert!(
        board.clipboard.holds(PASSWORD),
        "the copy is taken off before its time"
    );
    board.wait(1);
    assert!(
        board.clipboard.is_empty(),
        "the copy is still there at 30 s"
    );
    assert_eq!(board.ops(), ["clip.write", "clip.clear"]);
    assert!(!board.service.clear_if_unchanged());
}

#[test]
fn what_the_user_copied_meanwhile_is_left_alone() {
    let board = board();
    board.service.copy_sensitive(&secret(PASSWORD)).unwrap();
    board.wait(10);
    board.clipboard.user_copies("something of the user's own");
    board.wait(60);
    assert!(board.clipboard.holds("something of the user's own"));
    assert_eq!(board.clipboard.kept(), Some(Kept::ByTheUser));
    assert_eq!(board.ops(), ["clip.write"]);
    assert!(!board.service.clear_if_unchanged());
    assert!(board.clipboard.holds("something of the user's own"));
}

#[test]
fn the_same_text_copied_again_by_the_user_still_counts_as_the_copy() {
    let board = board();
    board.service.copy_sensitive(&secret(PASSWORD)).unwrap();
    board.clipboard.user_copies(PASSWORD);
    board.wait(30);
    assert!(board.clipboard.is_empty());
}

#[test]
fn without_the_private_formats_the_copy_is_plain_and_the_answer_says_so() {
    let board = board_with(RecordingClipboard::refusing_private_writes);
    let copied = board.service.copy_sensitive(&secret(PASSWORD)).unwrap();
    assert!(!copied.history_excluded);
    assert_eq!(board.clipboard.kept(), Some(Kept::Plain));
    board.wait(30);
    assert!(board.clipboard.is_empty());
    assert_eq!(board.ops(), ["clip.write", "clip.clear"]);
}

#[test]
fn a_newer_copy_puts_the_clear_off_until_its_own_time() {
    let board = board();
    board.service.copy_sensitive(&secret("the-login")).unwrap();
    board.wait(20);
    board.service.copy_sensitive(&secret(PASSWORD)).unwrap();
    board.wait(10);
    assert!(
        board.clipboard.holds(PASSWORD),
        "the first copy's clear took the second copy off"
    );
    board.wait(19);
    assert!(board.clipboard.holds(PASSWORD));
    board.wait(1);
    assert!(board.clipboard.is_empty());
    assert_eq!(board.ops(), ["clip.write", "clip.write", "clip.clear"]);
}

#[test]
fn at_quit_the_copy_is_taken_off_once_and_its_timer_then_does_nothing() {
    let board = board();
    board.service.copy_sensitive(&secret(PASSWORD)).unwrap();
    board.wait(5);
    assert!(board.service.clear_if_unchanged());
    assert!(board.clipboard.is_empty());
    board.clipboard.user_copies("copied after the quit began");
    board.wait(60);
    assert!(board.clipboard.holds("copied after the quit began"));
    assert!(!board.service.clear_if_unchanged());
    assert_eq!(board.ops(), ["clip.write", "clip.clear"]);
}

#[test]
fn a_clipboard_another_program_holds_refuses_the_copy_and_names_no_text() {
    let board = board_with(RecordingClipboard::held_by_another_program);
    let refused = board.service.copy_sensitive(&secret(PASSWORD)).unwrap_err();
    assert!(!format!("{refused} {refused:?}").contains(PASSWORD));
    assert!(board.log.snapshot().is_empty());
    board.wait(60);
    assert!(!board.service.clear_if_unchanged());
}

#[test]
fn a_copy_that_fails_leaves_the_earlier_copy_due_at_30_s_and_at_quit() {
    let board = board();
    board.service.copy_sensitive(&secret(PASSWORD)).unwrap();
    board.wait(10);
    board.clipboard.busy_on_calls(&[2]);
    let refused = board
        .service
        .copy_sensitive(&secret("the-login"))
        .unwrap_err();
    assert_eq!(refused, ClipboardError::Busy);
    assert!(board.clipboard.holds(PASSWORD));
    board.wait(20);
    assert!(
        board.clipboard.is_empty(),
        "the earlier copy was not cleared at its time"
    );

    let board = self::board();
    board.service.copy_sensitive(&secret(PASSWORD)).unwrap();
    board.clipboard.busy_on_calls(&[2]);
    assert!(board.service.copy_sensitive(&secret("the-login")).is_err());
    assert!(board.service.clear_if_unchanged());
    assert!(board.clipboard.is_empty());
}

#[test]
fn a_private_copy_is_never_written_again_without_its_marks_because_the_clipboard_was_busy_after_it()
{
    let board = board();
    board.clipboard.busy_on_calls(&[2, 3]);
    let copied = board.service.copy_sensitive(&secret(PASSWORD)).unwrap();
    assert!(copied.history_excluded);
    assert_eq!(board.clipboard.kept(), Some(Kept::Private));
    assert_eq!(board.ops(), ["clip.write"]);

    let board = self::board();
    board.clipboard.busy_on_calls(&[1]);
    assert_eq!(
        board.service.copy_sensitive(&secret(PASSWORD)).unwrap_err(),
        ClipboardError::Busy
    );
    assert!(board.clipboard.is_empty());
    assert!(board.log.snapshot().is_empty());
}

#[test]
fn a_clear_that_finds_the_clipboard_busy_is_tried_again_and_still_made_at_quit() {
    let board = board();
    board.service.copy_sensitive(&secret(PASSWORD)).unwrap();
    board.clipboard.busy_on_calls(&[2]);
    board.wait(30);
    assert!(board.clipboard.holds(PASSWORD));
    board.wait(2);
    assert!(board.clipboard.is_empty(), "the clear was not tried again");

    let board = self::board();
    board.service.copy_sensitive(&secret(PASSWORD)).unwrap();
    board.clipboard.busy_on_calls(&[2, 3, 4, 5, 6]);
    board.wait(120);
    assert!(board.clipboard.holds(PASSWORD));
    assert!(board.service.clear_if_unchanged());
    assert!(board.clipboard.is_empty());
}

#[test]
fn a_clear_never_reads_a_text_of_another_length() {
    let board = board();
    board.service.copy_sensitive(&secret(PASSWORD)).unwrap();
    board
        .clipboard
        .user_copies("a text of the user's own, of another length");
    board.wait(30);
    assert_eq!(board.clipboard.reads(), 0);
    assert!(!board.service.clear_if_unchanged());
    assert_eq!(board.clipboard.reads(), 0);

    let board = self::board();
    board.service.copy_sensitive(&secret(PASSWORD)).unwrap();
    let same_length = "x".repeat(PASSWORD.len());
    board.clipboard.user_copies(&same_length);
    board.wait(30);
    assert_eq!(board.clipboard.reads(), 1);
    assert!(board.clipboard.holds(&same_length));
}

#[test]
fn a_write_refused_for_anything_but_the_marks_is_an_error_and_never_a_plain_copy() {
    let board = board_with(RecordingClipboard::refusing_the_private_text);
    let refused = board.service.copy_sensitive(&secret(PASSWORD)).unwrap_err();
    assert!(matches!(refused, ClipboardError::Refused(_)));
    assert!(board.clipboard.is_empty());
    assert!(board.log.snapshot().is_empty());
}

#[test]
fn a_text_with_a_nul_is_refused_before_the_clipboard_is_touched_and_the_earlier_copy_stays_due() {
    let board = board();
    board.service.copy_sensitive(&secret(PASSWORD)).unwrap();
    let cut_short = "made-up\u{0}-rest";
    let refused = board
        .service
        .copy_sensitive(&secret(cut_short))
        .unwrap_err();
    assert!(matches!(refused, ClipboardError::Refused(_)));
    assert!(!format!("{refused} {refused:?}").contains("made-up"));
    assert!(board.clipboard.holds(PASSWORD));
    assert_eq!(board.ops(), ["clip.write"]);
    board.wait(30);
    assert!(board.clipboard.is_empty());
}

#[test]
fn a_password_of_any_letters_is_cleared_after_the_clipboard_changed_and_holds_it_again() {
    let letters = "h\u{e4}rse-b\u{f6}ttery-\u{1f600}-made-up";
    let board = board();
    board.service.copy_sensitive(&secret(letters)).unwrap();
    board.clipboard.user_copies(letters);
    board.wait(30);
    assert!(
        board.clipboard.is_empty(),
        "a copy of letters outside ASCII was left on the clipboard"
    );
    assert_eq!(board.clipboard.reads(), 1);
}

// Runs what is due even after it was put off, as a timer whose thread was already past its look at the token would.
struct Late(Arc<FakeClock>);

impl Timer for Late {
    fn after(&self, delay: Duration, _: CancelToken, run: Box<dyn FnOnce() + Send>) {
        self.0.schedule(delay, run);
    }
}

#[test]
fn a_timer_that_fires_late_leaves_a_newer_copy_alone() {
    let clock = Arc::new(FakeClock::starting_at(datetime!(2026-11-21 10:00:00 UTC)));
    let clipboard = Arc::new(RecordingClipboard::new(OpLog::new()));
    let service = ClipboardService::new(
        Arc::clone(&clipboard) as _,
        Arc::new(Late(Arc::clone(&clock))),
    );
    service.copy_sensitive(&secret("the-login")).unwrap();
    clock.sleep(Duration::from_secs(20), &CancelToken::new());
    service.copy_sensitive(&secret(PASSWORD)).unwrap();
    clock.sleep(Duration::from_secs(10), &CancelToken::new());
    assert!(
        clipboard.holds(PASSWORD),
        "the first copy's timer took the second copy off"
    );
    clock.sleep(Duration::from_secs(20), &CancelToken::new());
    assert!(clipboard.is_empty());
}

struct Unavailable;

impl Vault for Unavailable {
    fn encrypt(&self, _: &Secret) -> Result<Ciphertext, VaultError> {
        Err(VaultError::Unavailable)
    }

    fn decrypt(&self, _: &Ciphertext) -> Result<Secret, VaultError> {
        Err(VaultError::Unavailable)
    }
}

struct Fixture {
    _scratch: Scratch,
    store: Store,
}

impl Fixture {
    fn accounts(&self) -> Vec<Account> {
        self.store.accounts().unwrap()
    }

    fn with_password(&self) -> Account {
        let mut accounts = self.accounts().into_iter();
        accounts.find(|account| account.has_secret).unwrap()
    }

    fn without_password(&self) -> Account {
        let mut accounts = self.accounts().into_iter();
        accounts.find(|account| !account.has_secret).unwrap()
    }
}

fn fixture() -> Fixture {
    let scratch = Scratch::with_copy_of("clipboard", &oracle("fixtures/data-folder"));
    let (store, _) = Store::open(&database_path(scratch.path())).unwrap();
    Fixture {
        _scratch: scratch,
        store,
    }
}

fn code(refused: &ol_services::clipboard::CopyError) -> &'static str {
    refused.code()
}

#[test]
fn a_login_is_copied_as_it_is_stored_and_a_password_as_it_decrypts() {
    let fixture = fixture();
    let board = board();
    let account = fixture.with_password();
    let copied = copy_account_field(
        &fixture.store,
        &FakeVault,
        Some(&board.service),
        &account.id,
        CopyField::Login,
    )
    .unwrap();
    assert!(copied.history_excluded);
    assert!(board.clipboard.holds(&account.login));

    let stored = fixture
        .store
        .account_password(&account.id)
        .unwrap()
        .unwrap();
    let password = FakeVault.decrypt(&stored).unwrap();
    copy_account_field(
        &fixture.store,
        &FakeVault,
        Some(&board.service),
        &account.id,
        CopyField::Password,
    )
    .unwrap();
    assert!(
        board.clipboard.holds(password.expose()),
        "the clipboard does not hold the decrypted password"
    );
    assert_eq!(board.clipboard.kept(), Some(Kept::Private));
    board.wait(30);
    assert!(board.clipboard.is_empty());
}

#[test]
fn each_refusal_of_a_copy_comes_with_its_code_and_reaches_no_clipboard() {
    let fixture = fixture();
    let board = board();
    let copy = |vault: &dyn Vault, id: &str, field| {
        copy_account_field(&fixture.store, vault, Some(&board.service), id, field).unwrap_err()
    };

    let without = fixture.without_password();
    let refused = copy(&FakeVault, &without.id, CopyField::Password);
    assert_eq!(code(&refused), "NO_PASSWORD");
    assert_eq!(
        refused.to_string(),
        "No password is stored for this account"
    );

    let refused = copy(&FakeVault, "no-such-account", CopyField::Login);
    assert_eq!(code(&refused), "NOT_FOUND");

    let with = fixture.with_password();
    let refused = copy(&Unavailable, &with.id, CopyField::Password);
    assert_eq!(code(&refused), "VAULT_UNAVAILABLE");
    assert_eq!(
        refused.to_string(),
        "Secure storage is not available on this system. Secrets cannot be stored."
    );

    fixture
        .store
        .set_account_password(
            &with.id,
            Some(&Ciphertext::new(vec![0xa5, 0x5a])),
            "2026-11-21T10:00:00.000Z",
        )
        .unwrap();
    let refused = copy(&FakeVault, &with.id, CopyField::Password);
    assert_eq!(code(&refused), "VAULT_UNREADABLE");

    let refused = copy_account_field(
        &fixture.store,
        &FakeVault,
        None,
        &without.id,
        CopyField::Login,
    )
    .unwrap_err();
    assert_eq!(code(&refused), "CLIPBOARD_UNAVAILABLE");

    assert!(board.log.snapshot().is_empty());
    assert!(board.clipboard.is_empty());
}

#[test]
fn a_refused_copy_names_no_password_in_its_message_or_its_debug_output() {
    let fixture = fixture();
    let account = fixture.with_password();
    let stored = fixture
        .store
        .account_password(&account.id)
        .unwrap()
        .unwrap();
    let password = FakeVault.decrypt(&stored).unwrap();
    let board = board_with(RecordingClipboard::held_by_another_program);
    let refused = copy_account_field(
        &fixture.store,
        &FakeVault,
        Some(&board.service),
        &account.id,
        CopyField::Password,
    )
    .unwrap_err();
    assert_eq!(code(&refused), "CLIPBOARD_FAILED");
    let shown = format!("{refused} {refused:?}");
    assert!(
        !shown.contains(password.expose()),
        "a refused copy shows the password"
    );
}
