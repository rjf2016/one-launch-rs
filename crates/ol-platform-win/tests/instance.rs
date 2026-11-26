// The lock of a data folder: one holder at a time per folder however it is reached, released with a holder that never let it go, and a second start that reaches the holder's window and nothing else does.

use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::rc::Rc;
use std::thread;
use std::time::{Duration, Instant};

use ol_platform_win::instance::{Claim, bring_forward, claim, claim_waiting, name_for};
use ol_testkit::Scratch;
use windows::Win32::Foundation::{CloseHandle, LPARAM, WPARAM};
use windows::Win32::System::Threading::{OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, FindWindowExW, HWND_MESSAGE, MSG, PM_REMOVE, PeekMessageW, PostMessageW,
    WM_CLOSE,
};
use windows::core::{HSTRING, w};

fn pump_until(done: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let mut message = MSG::default();
        while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            unsafe { DispatchMessageW(&message) };
        }
        if done() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

#[expect(
    clippy::disallowed_methods,
    reason = "std makes no junction, and a junction is what this test needs"
)]
fn junction(link: &Path, target: &Path) {
    let made = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .unwrap();
    assert!(made.status.success(), "mklink /J failed");
}

#[test]
fn one_copy_holds_a_folder_however_it_is_reached_and_a_second_start_reaches_it() {
    let scratch = Scratch::new("instance-one");
    let folder = scratch.join("data");
    fs::create_dir_all(&folder).unwrap();
    let link = scratch.join("data-link");
    junction(&link, &folder);
    let other = scratch.join("other");
    let Claim::First(held) = claim(&folder).unwrap() else {
        panic!("a free folder was held");
    };
    assert!(matches!(claim(&folder).unwrap(), Claim::Second));
    let upper = std::path::PathBuf::from(folder.to_string_lossy().to_uppercase());
    assert!(matches!(claim(&upper).unwrap(), Claim::Second));
    assert!(matches!(claim(&link).unwrap(), Claim::Second));
    assert_eq!(name_for(&link).unwrap(), name_for(&folder).unwrap());
    let Claim::First(beside) = claim(&other).unwrap() else {
        panic!("another folder was blocked");
    };
    drop(beside);

    let heard = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&heard);
    held.listen(move |quiet| seen.borrow_mut().push(quiet));
    assert!(bring_forward(&folder, true, Duration::from_secs(1)));
    assert!(bring_forward(&link, false, Duration::from_secs(1)));
    assert!(pump_until(|| heard.borrow().len() == 2));
    assert_eq!(*heard.borrow(), [true, false]);

    let title = HSTRING::from(name_for(&folder).unwrap());
    let window =
        unsafe { FindWindowExW(Some(HWND_MESSAGE), None, w!("OneLaunch.Instance"), &title) }
            .unwrap();
    unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) }.unwrap();
    assert!(pump_until(|| true));
    assert!(bring_forward(&folder, true, Duration::from_secs(1)));
    assert!(pump_until(|| heard.borrow().len() == 3));

    let quits = Rc::new(std::cell::Cell::new(0));
    let counted = Rc::clone(&quits);
    let asker = held.on_quit(move || counted.set(counted.get() + 1));
    unsafe {
        PostMessageW(
            Some(window),
            windows::Win32::UI::WindowsAndMessaging::WM_APP + 2,
            WPARAM(0),
            LPARAM(0),
        )
    }
    .unwrap();
    assert!(pump_until(|| true));
    assert_eq!(quits.get(), 0);
    assert!(asker.ask());
    assert!(pump_until(|| quits.get() == 1));

    drop(held);
    assert!(!bring_forward(&folder, true, Duration::from_millis(200)));
    assert!(matches!(claim(&folder).unwrap(), Claim::First(_)));
}

#[test]
fn names_the_file_system_tells_apart_are_two_locks_and_names_it_joins_are_one() {
    let scratch = Scratch::new("instance-names");
    let sharp = scratch.join("straße");
    let Claim::First(held) = claim(&sharp).unwrap() else {
        panic!("a free folder was held");
    };
    assert!(matches!(
        claim(&scratch.join("STRAßE")).unwrap(),
        Claim::Second
    ));
    let Claim::First(beside) = claim(&scratch.join("strasse")).unwrap() else {
        panic!("another folder was blocked");
    };
    drop(beside);
    fs::create_dir(&sharp).unwrap();
    assert!(scratch.join("STRAßE").is_dir());
    assert!(!scratch.join("strasse").exists());
    assert!(matches!(
        claim(&scratch.join("STRAßE")).unwrap(),
        Claim::Second
    ));
    drop(held);
}

#[test]
fn the_lock_is_one_every_session_of_this_user_sees() {
    let scratch = Scratch::new("instance-global");
    let folder = scratch.join("data");
    let global = HSTRING::from(format!("Global\\{}", name_for(&folder).unwrap()));
    assert!(unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, &global) }.is_err());
    let Claim::First(held) = claim(&folder).unwrap() else {
        panic!("a free folder was held");
    };
    let seen = unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, &global) }
        .expect("the lock is not in the global namespace");
    unsafe { CloseHandle(seen) }.unwrap();
    drop(held);
}

#[test]
fn a_second_start_before_anyone_listens_is_heard_from_the_loop_once_someone_does() {
    let scratch = Scratch::new("instance-early");
    let folder = scratch.join("data");
    let Claim::First(held) = claim(&folder).unwrap() else {
        panic!("a free folder was held");
    };
    assert!(bring_forward(&folder, true, Duration::from_secs(1)));
    assert!(pump_until(|| true));
    let heard = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&heard);
    held.listen(move |quiet| seen.borrow_mut().push(quiet));
    assert!(heard.borrow().is_empty(), "heard inside listen");
    assert!(pump_until(|| !heard.borrow().is_empty()));
    assert_eq!(*heard.borrow(), [true]);
}

#[test]
fn a_relaunched_copy_waits_for_the_lock_of_the_copy_before_it() {
    let scratch = Scratch::new("instance-relaunch");
    let folder = scratch.join("data");
    let Claim::First(held) = claim(&folder).unwrap() else {
        panic!("a free folder was held");
    };
    let waiting = folder.clone();
    let (timing, timed) = std::sync::mpsc::channel();
    let relaunched = thread::spawn(move || {
        let started = Instant::now();
        timing.send(()).unwrap();
        let first = matches!(
            claim_waiting(&waiting, Duration::from_secs(10)).unwrap(),
            Claim::First(_)
        );
        (first, started.elapsed())
    });
    timed.recv().unwrap();
    thread::sleep(Duration::from_millis(300));
    assert!(matches!(claim(&folder).unwrap(), Claim::Second));
    drop(held);
    let (first, waited) = relaunched.join().unwrap();
    assert!(first);
    assert!(waited >= Duration::from_millis(250), "{waited:?}");
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test waits for its own child's exit code, which the process port does not give"
)]
fn a_copy_that_ends_without_letting_go_of_the_lock_leaves_no_lock() {
    let scratch = Scratch::new("instance-ended");
    let folder = scratch.join("data");
    let echo = scratch.join("echo.txt");
    let release = scratch.join("release.txt");
    let child =
        scratch.join(format!("olrs-test-child-{}.exe", uuid::Uuid::new_v4().simple()).as_str());
    fs::copy(env!("CARGO_BIN_EXE_ol-test-child"), &child).unwrap();
    let mut running = Command::new(&child)
        .arg(format!("--claim={}", folder.display()))
        .arg(format!("--echo={}", echo.display()))
        .arg(format!("--release={}", release.display()))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let said = || fs::read_to_string(&echo).unwrap_or_default();
    while !said().contains("claimed") && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(said().contains("claimed true"), "{}", said());
    assert!(matches!(claim(&folder).unwrap(), Claim::Second));
    fs::write(&release, "").unwrap();
    assert_eq!(running.wait().unwrap().code(), Some(3));
    assert!(matches!(claim(&folder).unwrap(), Claim::First(_)));
}
