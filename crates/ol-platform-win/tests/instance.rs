// The lock of a data folder: one holder at a time, per folder, released by a holder that crashes, and a second start that reaches the holder's window.

use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use ol_platform_win::instance::{Claim, InstanceEvent, bring_forward, claim};
use ol_testkit::Scratch;
use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, MSG, PM_REMOVE, PeekMessageW};

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

#[test]
fn one_copy_holds_a_folder_and_a_second_start_reaches_it() {
    let scratch = Scratch::new("instance-one");
    let folder = scratch.join("data");
    let other = scratch.join("other");
    let Claim::First(held) = claim(&folder).unwrap() else {
        panic!("a free folder was held");
    };
    assert!(matches!(claim(&folder).unwrap(), Claim::Second));
    let upper = std::path::PathBuf::from(folder.to_string_lossy().to_uppercase());
    assert!(matches!(claim(&upper).unwrap(), Claim::Second));
    let Claim::First(beside) = claim(&other).unwrap() else {
        panic!("another folder was blocked");
    };
    drop(beside);

    let heard = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let seen = std::rc::Rc::clone(&heard);
    held.listen(move |event| seen.borrow_mut().push(event));
    assert!(bring_forward(&folder, true, Duration::from_secs(1)));
    assert!(bring_forward(&folder, false, Duration::from_secs(1)));
    assert!(pump_until(|| heard.borrow().len() == 2));
    assert_eq!(
        *heard.borrow(),
        [
            InstanceEvent::Show { quiet: true },
            InstanceEvent::Show { quiet: false }
        ]
    );
    assert!(ol_platform_win::instance::ask_to_quit());
    assert!(pump_until(|| heard.borrow().len() == 3));
    assert_eq!(heard.borrow()[2], InstanceEvent::Quit);

    drop(held);
    assert!(!bring_forward(&folder, true, Duration::from_millis(200)));
    assert!(!ol_platform_win::instance::ask_to_quit());
    assert!(matches!(claim(&folder).unwrap(), Claim::First(_)));
}

#[test]
fn a_second_start_before_anyone_listens_is_handed_over_when_someone_does() {
    let scratch = Scratch::new("instance-early");
    let folder = scratch.join("data");
    let Claim::First(held) = claim(&folder).unwrap() else {
        panic!("a free folder was held");
    };
    assert!(bring_forward(&folder, true, Duration::from_secs(1)));
    let mut message = MSG::default();
    while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
        unsafe { DispatchMessageW(&message) };
    }
    let heard = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let seen = std::rc::Rc::clone(&heard);
    held.listen(move |event| seen.borrow_mut().push(event));
    assert_eq!(*heard.borrow(), [InstanceEvent::Show { quiet: true }]);
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test waits for its own child's exit code, which the process port does not give"
)]
fn a_copy_that_crashed_leaves_no_lock() {
    let scratch = Scratch::new("instance-crash");
    let folder = scratch.join("data");
    let echo = scratch.join("echo.txt");
    let mut child = Command::new(env!("CARGO_BIN_EXE_ol-test-child"))
        .arg(format!("--claim={}", folder.display()))
        .arg(format!("--echo={}", echo.display()))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !echo.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(echo.exists(), "the child did not take the lock");
    assert!(matches!(claim(&folder).unwrap(), Claim::Second));
    let status = child.wait().unwrap();
    assert!(!status.success(), "the child was to crash");
    assert!(matches!(claim(&folder).unwrap(), Claim::First(_)));
}
