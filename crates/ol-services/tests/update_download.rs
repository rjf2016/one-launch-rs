#![forbid(unsafe_code)]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ol_platform_api::http::HttpError;
use ol_platform_api::update_http::{ReleaseTag, UpdateAsk};
use ol_platform_api::{CancelToken, Clock};
use ol_platform_fake::FakeClock;
use ol_platform_fake::update_http::{
    FakeAnswer, Part, ScriptedUpdates, UpdateAsked, UpdateCall, file, in_parts, redirect, status,
};
use ol_services::updates::download::{Fetched, download};
use ol_services::updates::failure::UpdateFailure;
use ol_services::updates::manifest::ManifestFile;
use ol_testkit::Scratch;
use sha2::{Digest, Sha512};
use time::OffsetDateTime;

const NAME: &str = "OneLaunch-1.0.0-beta.4-setup.exe";
const TAG: &str = "v1.0.0-beta.4";
const INSTALLER: &str = "https://github.com/rjf2016/one-launch/releases/download/v1.0.0-beta.4/OneLaunch-1.0.0-beta.4-setup.exe";
const ASSET: &str = "https://release-assets.githubusercontent.com/github-production-release-asset/1/2?sp=r&se=2026-11-27T10%3A00%3A00Z&rscd=attachment%3B+filename%3DOneLaunch-1.0.0-beta.4-setup.exe&sig=placeholdersignature%2Bslash%2Fequals%3D";
const REDIRECTED: &str = "the update server redirected to an address One Launch does not ask";
const MISMATCH: &str = "the downloaded file does not match the release";
const UNREACHABLE: &str = "the update server could not be reached";

fn installer_bytes(size: usize) -> Vec<u8> {
    (0..size).map(|at| b'a' + (at % 26) as u8).collect()
}

fn sha512(bytes: &[u8]) -> String {
    STANDARD.encode(Sha512::digest(bytes))
}

fn release_file(bytes: &[u8]) -> ManifestFile {
    ManifestFile {
        url: NAME.to_owned(),
        sha512: sha512(bytes),
        size: bytes.len() as u64,
    }
}

fn tag() -> ReleaseTag {
    ReleaseTag::new(TAG).unwrap()
}

fn clock() -> Arc<FakeClock> {
    Arc::new(FakeClock::starting_at(OffsetDateTime::UNIX_EPOCH))
}

struct Folder {
    scratch: Scratch,
    pending: PathBuf,
}

impl Folder {
    fn new(name: &str) -> Self {
        let scratch = Scratch::new(name);
        let pending = scratch.join("one-launch-updater").join("pending");
        Self { scratch, pending }
    }

    fn made(name: &str) -> Self {
        let folder = Self::new(name);
        fs::create_dir_all(&folder.pending).unwrap();
        folder
    }

    fn installer(&self) -> PathBuf {
        self.pending.join(NAME)
    }

    fn temp(&self) -> PathBuf {
        self.pending.join(format!("temp-{NAME}"))
    }

    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.pending)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

struct Run {
    result: Result<Fetched, UpdateFailure>,
    reported: Vec<u8>,
}

fn run_with(
    http: &ScriptedUpdates,
    clock: &dyn Clock,
    folder: &Folder,
    release: &ManifestFile,
    cancel: &CancelToken,
) -> Run {
    let mut reported = Vec::new();
    let result = download(
        http,
        clock,
        &folder.pending,
        &tag(),
        release,
        &mut |percent| reported.push(percent),
        cancel,
    );
    Run { result, reported }
}

fn run(http: &ScriptedUpdates, folder: &Folder, release: &ManifestFile) -> Run {
    run_with(http, &*clock(), folder, release, &CancelToken::new())
}

fn ready(folder: &Folder, bytes: &[u8]) -> Result<Fetched, UpdateFailure> {
    Ok(Fetched::Ready {
        path: folder.installer(),
        sha512: sha512(bytes),
    })
}

fn failed(sentence: &str, run: &Run) {
    match &run.result {
        Err(failure) => assert_eq!(failure.to_string(), sentence),
        other => panic!("expected {sentence:?}, got {other:?}"),
    }
}

fn once(answer: FakeAnswer) -> ScriptedUpdates {
    let answer = Mutex::new(Some(answer));
    ScriptedUpdates::new(move |request| {
        assert_eq!(request.url().to_string(), INSTALLER);
        Ok(answer
            .lock()
            .unwrap()
            .take()
            .expect("the installer is asked once"))
    })
}

fn untouched() -> ScriptedUpdates {
    ScriptedUpdates::new(|request| panic!("nothing is asked, but {} was", request.url()))
}

fn parts_of(bytes: &[u8], sizes: &[usize]) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut at = 0;
    for size in sizes {
        parts.push(Part::Bytes(bytes[at..at + size].to_vec()));
        at += size;
    }
    assert_eq!(at, bytes.len());
    parts
}

#[test]
fn a_good_file_in_one_part_becomes_the_installer_in_pending() {
    let folder = Folder::new("update-download-one-part");
    let bytes = installer_bytes(50);
    let http = once(file(bytes.clone()));
    let run = run(&http, &folder, &release_file(&bytes));
    assert_eq!(run.result, ready(&folder, &bytes));
    assert_eq!(fs::read(folder.installer()).unwrap(), bytes);
    assert_eq!(folder.names(), vec![NAME.to_owned()]);
    assert!(run.reported.is_empty());
    assert_eq!(
        http.asked(),
        vec![UpdateAsked {
            url: INSTALLER.to_owned(),
            ask: UpdateAsk::Installer,
            headers: vec![
                ("accept", "*/*"),
                ("cache-control", "no-cache"),
                ("user-agent", "electron-builder"),
            ],
            call: UpdateCall::Download,
        }]
    );
}

#[test]
fn a_good_file_in_several_parts_declared_or_not_is_kept_whole() {
    for declared in [true, false] {
        let folder = Folder::new("update-download-parts");
        let bytes = installer_bytes(200_000);
        let size = declared.then_some(bytes.len() as u64);
        let http = once(in_parts(
            size,
            parts_of(&bytes, &[1, 70_000, 65_536, 64_463]),
        ));
        let run = run(&http, &folder, &release_file(&bytes));
        assert_eq!(run.result, ready(&folder, &bytes));
        assert_eq!(fs::read(folder.installer()).unwrap(), bytes);
        assert_eq!(folder.names(), vec![NAME.to_owned()]);
    }
}

#[test]
fn the_request_asks_for_no_more_than_the_signed_size_within_the_wait() {
    let folder = Folder::new("update-download-limit");
    let bytes = installer_bytes(321);
    let seen = Arc::new(Mutex::new(None));
    let http = ScriptedUpdates::new({
        let seen = Arc::clone(&seen);
        let bytes = bytes.clone();
        move |request| {
            *seen.lock().unwrap() = Some((request.limit, request.timeout));
            Ok(file(bytes.clone()))
        }
    });
    run(&http, &folder, &release_file(&bytes)).result.unwrap();
    assert_eq!(*seen.lock().unwrap(), Some((321, Duration::from_secs(30))));
}

#[test]
fn a_present_installer_with_the_right_hash_is_used_without_asking() {
    let folder = Folder::made("update-download-reuse");
    let bytes = installer_bytes(50);
    fs::write(folder.installer(), &bytes).unwrap();
    let http = untouched();
    let run = run(&http, &folder, &release_file(&bytes));
    assert_eq!(run.result, ready(&folder, &bytes));
    assert!(http.asked().is_empty());
    assert!(run.reported.is_empty());
}

#[test]
fn a_present_installer_with_the_wrong_hash_is_replaced() {
    let folder = Folder::made("update-download-wrong-hash");
    let bytes = installer_bytes(50);
    let mut stale = bytes.clone();
    stale[49] = b'!';
    fs::write(folder.installer(), &stale).unwrap();
    let http = once(file(bytes.clone()));
    let run = run(&http, &folder, &release_file(&bytes));
    assert_eq!(run.result, ready(&folder, &bytes));
    assert_eq!(fs::read(folder.installer()).unwrap(), bytes);
    assert_eq!(http.asked().len(), 1);
}

#[test]
fn a_folder_with_the_installer_name_is_not_reused_and_cannot_be_replaced() {
    let folder = Folder::made("update-download-folder-named");
    fs::create_dir(folder.installer()).unwrap();
    fs::write(folder.installer().join("inside.txt"), b"kept").unwrap();
    let bytes = installer_bytes(50);
    let http = once(file(bytes.clone()));
    let run = run(&http, &folder, &release_file(&bytes));
    failed("the update could not be saved to disk", &run);
    assert_eq!(http.asked().len(), 1);
    assert_eq!(folder.names(), vec![NAME.to_owned()]);
    assert_eq!(
        fs::read(folder.installer().join("inside.txt")).unwrap(),
        b"kept"
    );
}

#[test]
fn a_temp_name_taken_by_a_folder_fails_to_save_before_anything_is_asked() {
    let folder = Folder::made("update-download-temp-taken");
    fs::create_dir(folder.temp()).unwrap();
    let bytes = installer_bytes(50);
    let http = untouched();
    let run = run(&http, &folder, &release_file(&bytes));
    failed("the update could not be saved to disk", &run);
    assert!(folder.temp().is_dir());
    assert_eq!(folder.names(), vec![format!("temp-{NAME}")]);
}

#[test]
fn the_leftovers_in_pending_are_removed_and_folders_links_and_the_outside_are_left() {
    let folder = Folder::made("update-download-leftovers");
    for leftover in [
        "OneLaunch-1.0.0-beta.2-setup.exe",
        "update-info.json",
        "current.blockmap",
        "temp-OneLaunch-1.0.0-beta.2-setup.exe",
        "temp-OneLaunch-1.0.0-beta.4-setup.exe",
    ] {
        fs::write(folder.pending.join(leftover), b"old").unwrap();
    }
    fs::create_dir(folder.pending.join("kept")).unwrap();
    fs::write(folder.pending.join("kept").join("inside.exe"), b"kept").unwrap();
    let outside = folder.scratch.join("outside.exe");
    fs::write(&outside, b"outside").unwrap();
    let beside = folder
        .scratch
        .join("one-launch-updater")
        .join("installer.exe");
    fs::write(&beside, b"beside").unwrap();
    let link = folder.pending.join("link.exe");
    let linked = std::os::windows::fs::symlink_file(&outside, &link).is_ok();
    if !linked {
        eprintln!(
            "this machine does not let a test make a symbolic link; the link case is skipped"
        );
    }

    let bytes = installer_bytes(50);
    let http = once(file(bytes.clone()));
    let run = run(&http, &folder, &release_file(&bytes));
    assert_eq!(run.result, ready(&folder, &bytes));
    let mut expected = vec![NAME.to_owned(), "kept".to_owned()];
    if linked {
        expected.push("link.exe".to_owned());
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
    expected.sort();
    assert_eq!(folder.names(), expected);
    assert_eq!(
        fs::read(folder.pending.join("kept").join("inside.exe")).unwrap(),
        b"kept"
    );
    assert_eq!(fs::read(&outside).unwrap(), b"outside");
    assert_eq!(fs::read(&beside).unwrap(), b"beside");
}

fn sorted_names(folder: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_pending_folder_that_is_a_link_is_refused_before_anything_is_asked_or_touched() {
    let folder = Folder::new("update-download-pending-link");
    let updater = folder.scratch.join("one-launch-updater");
    let elsewhere = updater.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    let bytes = installer_bytes(50);
    fs::write(elsewhere.join("notes.txt"), b"not the updater's").unwrap();
    fs::write(elsewhere.join(NAME), &bytes).unwrap();
    if std::os::windows::fs::symlink_dir(&elsewhere, &folder.pending).is_err() {
        eprintln!(
            "this machine does not let a test make a symbolic link; the linked pending folder is skipped"
        );
        return;
    }

    let http = untouched();
    let run = run(&http, &folder, &release_file(&bytes));
    failed("the update could not be saved to disk", &run);
    assert!(http.asked().is_empty());
    assert_eq!(
        sorted_names(&elsewhere),
        vec![NAME.to_owned(), "notes.txt".to_owned()]
    );
    assert_eq!(
        fs::read(elsewhere.join("notes.txt")).unwrap(),
        b"not the updater's"
    );
    assert_eq!(fs::read(elsewhere.join(NAME)).unwrap(), bytes);
    assert!(
        fs::symlink_metadata(&folder.pending)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn a_pending_folder_that_is_a_file_is_refused_before_anything_is_asked() {
    let folder = Folder::new("update-download-pending-file");
    let updater = folder.scratch.join("one-launch-updater");
    fs::create_dir_all(&updater).unwrap();
    fs::write(&folder.pending, b"a file").unwrap();

    let http = untouched();
    let run = run(&http, &folder, &release_file(&installer_bytes(50)));
    failed("the update could not be saved to disk", &run);
    assert!(http.asked().is_empty());
    assert_eq!(fs::read(&folder.pending).unwrap(), b"a file");
    assert_eq!(sorted_names(&updater), vec!["pending".to_owned()]);
}

#[test]
fn the_leftovers_are_removed_before_asking_even_when_the_download_then_fails() {
    let folder = Folder::made("update-download-leftovers-failed");
    fs::write(folder.pending.join("update-info.json"), b"old").unwrap();
    let http = once(status(503));
    let bytes = installer_bytes(50);
    let run = run(&http, &folder, &release_file(&bytes));
    failed("the update server answered HTTP 503", &run);
    assert!(folder.names().is_empty());
}

#[test]
fn the_pending_folder_is_made_when_missing() {
    let folder = Folder::new("update-download-made");
    assert!(!folder.pending.exists());
    let bytes = installer_bytes(50);
    let http = once(file(bytes.clone()));
    assert_eq!(
        run(&http, &folder, &release_file(&bytes)).result,
        ready(&folder, &bytes)
    );
}

#[test]
fn a_file_name_the_port_refuses_fails_without_asking_or_touching_the_disk() {
    let folder = Folder::new("update-download-bad-name");
    let bytes = installer_bytes(50);
    for name in [
        "../OneLaunch-setup.exe",
        "OneLaunch setup.exe",
        "OneLaunch.msi",
        "",
    ] {
        let release = ManifestFile {
            url: name.to_owned(),
            ..release_file(&bytes)
        };
        let http = untouched();
        let run = run(&http, &folder, &release);
        failed("the release file is not a plain .exe name", &run);
        assert!(!folder.pending.exists());
    }
}

fn watched_temp(folder: &Folder, seen: &Arc<AtomicBool>) -> Part {
    let temp = folder.temp();
    let seen = Arc::clone(seen);
    Part::run(move || seen.store(temp.is_file(), Ordering::SeqCst))
}

#[test]
fn the_temp_file_is_gone_after_every_way_a_download_fails() {
    let bytes = installer_bytes(50);
    let wrong = {
        let mut wrong = bytes.clone();
        wrong[0] = b'!';
        wrong
    };
    let longer = installer_bytes(51);
    type Case = (&'static str, Box<dyn Fn(Part) -> FakeAnswer>, String);
    let cases: Vec<Case> = vec![
        (
            "dropped",
            Box::new({
                let bytes = bytes.clone();
                move |watch| {
                    in_parts(
                        Some(50),
                        vec![Part::Bytes(bytes[..20].to_vec()), watch, Part::Drop],
                    )
                }
            }),
            UNREACHABLE.to_owned(),
        ),
        (
            "stalled",
            Box::new({
                let bytes = bytes.clone();
                move |watch| {
                    in_parts(
                        Some(50),
                        vec![Part::Bytes(bytes[..20].to_vec()), watch, Part::Stall],
                    )
                }
            }),
            UNREACHABLE.to_owned(),
        ),
        (
            "short and declared",
            Box::new({
                let bytes = bytes.clone();
                move |watch| in_parts(Some(50), vec![Part::Bytes(bytes[..49].to_vec()), watch])
            }),
            UNREACHABLE.to_owned(),
        ),
        (
            "short and undeclared",
            Box::new({
                let bytes = bytes.clone();
                move |watch| in_parts(None, vec![Part::Bytes(bytes[..49].to_vec()), watch])
            }),
            MISMATCH.to_owned(),
        ),
        (
            "long and undeclared",
            Box::new({
                let longer = longer.clone();
                move |watch| {
                    in_parts(
                        None,
                        vec![
                            Part::Bytes(longer[..25].to_vec()),
                            watch,
                            Part::Bytes(longer[25..].to_vec()),
                        ],
                    )
                }
            }),
            MISMATCH.to_owned(),
        ),
        (
            "another hash",
            Box::new({
                let wrong = wrong.clone();
                move |watch| in_parts(Some(50), vec![Part::Bytes(wrong.clone()), watch])
            }),
            MISMATCH.to_owned(),
        ),
    ];
    for (name, answer, sentence) in cases {
        let folder = Folder::made("update-download-temp-gone");
        let seen = Arc::new(AtomicBool::new(false));
        let http = once(answer(watched_temp(&folder, &seen)));
        let run = run(&http, &folder, &release_file(&bytes));
        failed(&sentence, &run);
        assert!(
            seen.load(Ordering::SeqCst),
            "{name}: the temp file was there"
        );
        assert!(folder.names().is_empty(), "{name}: {:?}", folder.names());
    }
}

#[test]
fn a_body_with_the_signed_hash_but_not_the_signed_size_is_refused() {
    let bytes = installer_bytes(50);
    for declared in [Some(50), None] {
        let folder = Folder::made("update-download-length");
        let release = ManifestFile {
            size: 51,
            ..release_file(&bytes)
        };
        let http = once(in_parts(declared, vec![Part::Bytes(bytes.clone())]));
        let run = run(&http, &folder, &release);
        failed(MISMATCH, &run);
        assert!(folder.names().is_empty());
    }
}

#[test]
fn a_long_body_that_says_so_is_refused_unread_and_leaves_nothing() {
    let folder = Folder::made("update-download-long-declared");
    let bytes = installer_bytes(50);
    let http = once(file(installer_bytes(51)));
    let run = run(&http, &folder, &release_file(&bytes));
    failed(MISMATCH, &run);
    assert!(folder.names().is_empty());
}

#[test]
fn a_bad_status_leaves_nothing_and_says_its_code() {
    for code in [404, 500, 403] {
        let folder = Folder::made("update-download-status");
        let http = once(status(code));
        let run = run(&http, &folder, &release_file(&installer_bytes(50)));
        failed(&format!("the update server answered HTTP {code}"), &run);
        assert!(folder.names().is_empty());
    }
}

#[test]
fn a_network_failure_before_the_body_reads_as_its_sentence_and_leaves_nothing() {
    for (error, sentence) in [
        (HttpError::Failed("the connection failed"), UNREACHABLE),
        (HttpError::Timeout, UNREACHABLE),
        (
            HttpError::Failed("TLS failed"),
            "the secure connection to the update server failed",
        ),
    ] {
        let folder = Folder::made("update-download-network");
        let http = ScriptedUpdates::new(move |_| Err(error));
        let run = run(&http, &folder, &release_file(&installer_bytes(50)));
        failed(sentence, &run);
        assert!(folder.names().is_empty());
    }
}

#[test]
fn a_cancel_between_parts_ends_the_download_and_removes_its_temp_file() {
    let folder = Folder::made("update-download-cancel");
    let bytes = installer_bytes(50);
    let cancel = CancelToken::new();
    let seen = Arc::new(AtomicBool::new(false));
    let after = Arc::new(AtomicBool::new(false));
    let answer = in_parts(
        Some(50),
        vec![
            Part::Bytes(bytes[..20].to_vec()),
            watched_temp(&folder, &seen),
            Part::run({
                let cancel = cancel.clone();
                move || cancel.cancel()
            }),
            Part::Bytes(bytes[20..].to_vec()),
            Part::run({
                let after = Arc::clone(&after);
                move || after.store(true, Ordering::SeqCst)
            }),
        ],
    );
    let http = once(answer);
    let run = run_with(&http, &*clock(), &folder, &release_file(&bytes), &cancel);
    assert_eq!(run.result, Ok(Fetched::Cancelled));
    assert!(seen.load(Ordering::SeqCst));
    assert!(!after.load(Ordering::SeqCst));
    assert!(folder.names().is_empty());
}

#[test]
fn a_cancel_before_the_start_asks_nothing_and_touches_nothing() {
    let folder = Folder::made("update-download-cancel-early");
    fs::write(folder.pending.join("update-info.json"), b"old").unwrap();
    let cancel = CancelToken::new();
    cancel.cancel();
    let http = untouched();
    let run = run_with(
        &http,
        &*clock(),
        &folder,
        &release_file(&installer_bytes(50)),
        &cancel,
    );
    assert_eq!(run.result, Ok(Fetched::Cancelled));
    assert_eq!(folder.names(), vec!["update-info.json".to_owned()]);
}

#[test]
fn the_rename_replaces_an_installer_that_appeared_meanwhile() {
    let folder = Folder::made("update-download-rename");
    let bytes = installer_bytes(50);
    let installer = folder.installer();
    let answer = in_parts(
        Some(50),
        vec![
            Part::Bytes(bytes[..10].to_vec()),
            Part::run(move || fs::write(&installer, b"an older installer").unwrap()),
            Part::Bytes(bytes[10..].to_vec()),
        ],
    );
    let http = once(answer);
    let run = run(&http, &folder, &release_file(&bytes));
    assert_eq!(run.result, ready(&folder, &bytes));
    assert_eq!(fs::read(folder.installer()).unwrap(), bytes);
    assert_eq!(folder.names(), vec![NAME.to_owned()]);
}

fn hops(routes: Vec<(String, FakeAnswer)>) -> ScriptedUpdates {
    let routes = Mutex::new(routes);
    ScriptedUpdates::new(move |request| {
        let url = request.url().to_string();
        let mut routes = routes.lock().unwrap();
        let at = routes
            .iter()
            .position(|(at, _)| *at == url)
            .unwrap_or_else(|| panic!("nothing answers {url}"));
        Ok(routes.remove(at).1)
    })
}

#[test]
fn the_installer_is_followed_to_the_asset_host_with_its_query_unchanged() {
    let folder = Folder::made("update-download-redirect");
    let bytes = installer_bytes(50);
    let http = hops(vec![
        (INSTALLER.to_owned(), redirect(302, ASSET)),
        (ASSET.to_owned(), file(bytes.clone())),
    ]);
    let run = run(&http, &folder, &release_file(&bytes));
    assert_eq!(run.result, ready(&folder, &bytes));
    let asked = http.asked();
    assert_eq!(http.urls(), vec![INSTALLER.to_owned(), ASSET.to_owned()]);
    assert!(asked.iter().all(|asked| asked.call == UpdateCall::Download
        && asked.ask == UpdateAsk::Installer
        && asked.headers[0] == ("accept", "*/*")));
}

fn asset_hop(step: usize) -> String {
    format!("https://release-assets.githubusercontent.com/hop{step}")
}

#[test]
fn five_redirects_are_followed_and_a_sixth_or_a_refused_one_is_not() {
    for (redirects, works) in [(5, true), (6, false)] {
        let folder = Folder::made("update-download-redirects");
        let bytes = installer_bytes(50);
        let mut routes = vec![(INSTALLER.to_owned(), redirect(302, &asset_hop(1)))];
        for step in 1..redirects {
            routes.push((asset_hop(step), redirect(307, &asset_hop(step + 1))));
        }
        routes.push((asset_hop(redirects), file(bytes.clone())));
        let http = hops(routes);
        let run = run(&http, &folder, &release_file(&bytes));
        if works {
            assert_eq!(run.result, ready(&folder, &bytes));
            assert_eq!(http.urls().len(), 6);
        } else {
            failed(REDIRECTED, &run);
            assert_eq!(http.urls().len(), 6);
            assert!(folder.names().is_empty());
        }
    }
    for location in [
        Some("https://example.com/OneLaunch-1.0.0-beta.4-setup.exe"),
        Some("http://release-assets.githubusercontent.com/x"),
        None,
    ] {
        let folder = Folder::made("update-download-refused");
        let answer = match location {
            Some(location) => redirect(302, location),
            None => status(302),
        };
        let http = once(answer);
        let run = run(&http, &folder, &release_file(&installer_bytes(50)));
        failed(REDIRECTED, &run);
        assert_eq!(http.urls(), vec![INSTALLER.to_owned()]);
        assert!(folder.names().is_empty());
    }
}

fn wait(clock: &Arc<FakeClock>, millis: u64) -> Part {
    let clock = Arc::clone(clock);
    Part::pause(move || clock.sleep(Duration::from_millis(millis), &CancelToken::new()))
}

#[test]
fn progress_follows_the_recorded_pacing() {
    let folder = Folder::made("update-download-pacing");
    let bytes = installer_bytes(50);
    let clock = clock();
    let answer = in_parts(
        Some(50),
        vec![
            Part::Bytes(bytes[..16].to_vec()),
            wait(&clock, 1200),
            Part::Bytes(bytes[16..32].to_vec()),
            Part::Bytes(bytes[32..].to_vec()),
        ],
    );
    let http = once(answer);
    let run = run_with(
        &http,
        &*clock,
        &folder,
        &release_file(&bytes),
        &CancelToken::new(),
    );
    assert_eq!(run.result, ready(&folder, &bytes));
    assert_eq!(run.reported, vec![64]);
}

#[test]
fn progress_is_told_at_most_once_a_second_from_the_last_time_it_was_told() {
    let folder = Folder::made("update-download-every-second");
    let bytes = installer_bytes(100);
    let clock = clock();
    let mut parts = vec![Part::Bytes(bytes[..10].to_vec())];
    for step in 1..10 {
        parts.push(wait(&clock, 600));
        parts.push(Part::Bytes(bytes[step * 10..step * 10 + 10].to_vec()));
    }
    let http = once(in_parts(Some(100), parts));
    let run = run_with(
        &http,
        &*clock,
        &folder,
        &release_file(&bytes),
        &CancelToken::new(),
    );
    assert_eq!(run.result, ready(&folder, &bytes));
    assert_eq!(run.reported, vec![30, 50, 70, 90]);
}

#[test]
fn progress_is_rounded_down() {
    let folder = Folder::made("update-download-floor");
    let bytes = installer_bytes(7);
    let clock = clock();
    let answer = in_parts(
        Some(7),
        vec![
            Part::Bytes(bytes[..1].to_vec()),
            wait(&clock, 1000),
            Part::Bytes(bytes[1..2].to_vec()),
            wait(&clock, 1000),
            Part::Bytes(bytes[2..6].to_vec()),
            wait(&clock, 1000),
            Part::Bytes(bytes[6..].to_vec()),
        ],
    );
    let http = once(answer);
    let run = run_with(
        &http,
        &*clock,
        &folder,
        &release_file(&bytes),
        &CancelToken::new(),
    );
    assert_eq!(run.result, ready(&folder, &bytes));
    assert_eq!(run.reported, vec![28, 85]);
}

// The caller shows 0 when it starts the download; this side never repeats it, so many parts inside one second report nothing at all.
#[test]
fn many_small_parts_inside_one_second_report_nothing() {
    let folder = Folder::made("update-download-small-parts");
    let bytes = installer_bytes(50);
    let clock = clock();
    let mut parts = Vec::new();
    for byte in &bytes {
        parts.push(wait(&clock, 15));
        parts.push(Part::Bytes(vec![*byte]));
    }
    let http = once(in_parts(Some(50), parts));
    let run = run_with(
        &http,
        &*clock,
        &folder,
        &release_file(&bytes),
        &CancelToken::new(),
    );
    assert_eq!(run.result, ready(&folder, &bytes));
    assert!(run.reported.is_empty());
}

#[test]
fn a_last_part_after_a_long_wait_is_not_reported_because_it_completes_the_file() {
    let folder = Folder::made("update-download-last-part");
    let bytes = installer_bytes(50);
    let clock = clock();
    let answer = in_parts(
        Some(50),
        vec![
            Part::Bytes(bytes[..49].to_vec()),
            wait(&clock, 5000),
            Part::Bytes(bytes[49..].to_vec()),
        ],
    );
    let http = once(answer);
    let run = run_with(
        &http,
        &*clock,
        &folder,
        &release_file(&bytes),
        &CancelToken::new(),
    );
    assert_eq!(run.result, ready(&folder, &bytes));
    assert!(run.reported.is_empty());
}
