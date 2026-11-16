// The real process port on child programs these tests start themselves: copies of ol-test-child, each under a name no other program has, so a close or an end by name reaches them and nothing else. Nothing here prints, keeps or checks the names of the other programs on the PC.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use ol_platform_api::{ProcessArg, ProcessError, ProcessLister, ProcessRunner};
use ol_platform_win::processes::WindowsProcesses;
use ol_testkit::Scratch;

const PATIENCE: Duration = Duration::from_secs(20);

struct Child {
    exe: PathBuf,
    image: String,
}

// Ends every copy a test made when the test ends, passed or not.
struct EndOnDrop(Vec<String>);

impl Drop for EndOnDrop {
    fn drop(&mut self) {
        for image in &self.0 {
            let _ = WindowsProcesses::new().kill(image, true);
        }
        let deadline = Instant::now() + PATIENCE;
        while self
            .0
            .iter()
            .any(|image| WindowsProcesses::new().is_running(image).unwrap_or(false))
            && Instant::now() < deadline
        {
            thread::sleep(Duration::from_millis(50));
        }
    }
}

fn copy(scratch: &Scratch, guard: &mut EndOnDrop) -> Child {
    let image = format!("olrs-test-child-{}.exe", uuid::Uuid::new_v4().simple());
    let exe = scratch.join(&image);
    fs::copy(env!("CARGO_BIN_EXE_ol-test-child"), &exe).unwrap();
    guard.0.push(image.clone());
    Child { exe, image }
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn until(what: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        if what() {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

fn runs(image: &str) -> bool {
    WindowsProcesses::new().is_running(image).unwrap()
}

#[test]
fn a_started_program_is_listed_by_name_and_path_and_a_close_ends_it() {
    let scratch = Scratch::new("win-processes-close");
    let mut guard = EndOnDrop(Vec::new());
    let child = copy(&scratch, &mut guard);
    let processes = WindowsProcesses::new();
    processes
        .start(
            &text(&child.exe),
            &[ProcessArg::Plain("--window")],
            &text(scratch.path()),
        )
        .unwrap();
    assert!(until(|| runs(&child.image)), "the child did not start");
    assert!(runs(&child.image.to_uppercase()));
    assert!(runs(child.image.trim_end_matches(".exe")));

    let names = processes.running_names().unwrap();
    assert!(names.contains(&child.image.to_lowercase()));
    let listed = processes.list_processes().unwrap();
    let mine: Vec<_> = listed
        .iter()
        .filter(|process| process.name.eq_ignore_ascii_case(&child.image))
        .collect();
    assert_eq!(mine.len(), 1);
    let path = mine[0].path.as_deref().unwrap_or_default();
    assert!(
        path.eq_ignore_ascii_case(&text(&child.exe)),
        "the child's path was not found"
    );

    processes.close(&child.image).unwrap();
    assert!(until(|| !runs(&child.image)), "the child did not close");
}

#[test]
fn a_program_without_a_window_hears_no_close_and_an_end_takes_it() {
    let scratch = Scratch::new("win-processes-kill");
    let mut guard = EndOnDrop(Vec::new());
    let child = copy(&scratch, &mut guard);
    let processes = WindowsProcesses::new();
    processes.start(&text(&child.exe), &[], "").unwrap();
    assert!(until(|| runs(&child.image)));
    processes.close(&child.image).unwrap();
    thread::sleep(Duration::from_millis(500));
    assert!(runs(&child.image), "a program without a window was closed");
    processes.kill(&child.image, false).unwrap();
    assert!(until(|| !runs(&child.image)));
    processes.kill(&child.image, true).unwrap();
}

#[test]
fn a_tree_end_takes_what_the_program_started_and_a_plain_end_leaves_it() {
    let scratch = Scratch::new("win-processes-tree");
    let mut guard = EndOnDrop(Vec::new());
    let processes = WindowsProcesses::new();
    for tree in [true, false] {
        let parent = copy(&scratch, &mut guard);
        let child = copy(&scratch, &mut guard);
        let started = format!("--child={}", text(&child.exe));
        let ready = scratch.join(&format!("{}.ready", parent.image));
        let echo = format!("--echo={}", text(&ready));
        processes
            .start(
                &text(&parent.exe),
                &[ProcessArg::Plain(&started), ProcessArg::Plain(&echo)],
                "",
            )
            .unwrap();
        // An end while the parent is still inside its start of the child would take the half-made child with it.
        assert!(until(|| ready.exists() && runs(&child.image)));
        processes.kill(&parent.image, tree).unwrap();
        assert!(until(|| !runs(&parent.image)));
        thread::sleep(Duration::from_millis(300));
        assert_eq!(runs(&child.image), !tree, "tree {tree}");
        processes.kill(&child.image, false).unwrap();
        assert!(until(|| !runs(&child.image)));
    }
}

#[test]
fn every_argument_reaches_the_program_as_it_was_given() {
    let scratch = Scratch::new("win-processes-args");
    let mut guard = EndOnDrop(Vec::new());
    let child = copy(&scratch, &mut guard);
    let echo = scratch.join("echo.txt");
    let echoed = format!("--echo={}", text(&echo));
    let args = [
        ProcessArg::Plain(&echoed),
        ProcessArg::Plain(""),
        ProcessArg::Plain("two words"),
        ProcessArg::Plain(r#"--exec="launch Pro""#),
        ProcessArg::Plain(r"C:\folder with space\"),
        ProcessArg::Plain(r#"a\"b\\"#),
        ProcessArg::Plain("Ünïcode ™"),
        ProcessArg::Secret("placeholder pass\"word\\"),
    ];
    WindowsProcesses::new()
        .start(&text(&child.exe), &args, &text(scratch.path()))
        .unwrap();
    assert!(until(|| fs::read_to_string(&echo).is_ok_and(|text| text
        .ends_with('\n')
        && text.lines().count() == 9)));
    let written = fs::read_to_string(&echo).unwrap();
    let mut lines = written.lines();
    assert!(
        lines
            .next()
            .unwrap()
            .eq_ignore_ascii_case(&text(scratch.path()))
    );
    let given: Vec<&str> = args
        .iter()
        .map(|(ProcessArg::Plain(text) | ProcessArg::Secret(text))| *text)
        .collect();
    assert_eq!(lines.collect::<Vec<_>>(), given);
}

#[test]
fn a_start_that_fails_names_the_program_and_no_argument() {
    let scratch = Scratch::new("win-processes-missing");
    let missing = scratch.join("olrs-not-there.exe");
    let refused = WindowsProcesses::new()
        .start(
            &text(&missing),
            &[ProcessArg::Secret("placeholder-password-1")],
            "",
        )
        .unwrap_err();
    let shown = format!("{refused} {refused:?}");
    assert!(!shown.contains("placeholder"), "{shown}");
    assert!(shown.contains("olrs-not-there.exe"), "{shown}");
    assert!(matches!(refused, ProcessError::Failed(_)));
}

#[test]
fn nothing_that_is_not_an_image_name_is_looked_for() {
    let processes = WindowsProcesses::new();
    for image in ["", "  ", r"C:\Windows\explorer.exe", "a/b.exe", "x:y.exe"] {
        assert!(processes.close(image).is_err(), "{image:?}");
        assert!(processes.kill(image, true).is_err(), "{image:?}");
    }
}
