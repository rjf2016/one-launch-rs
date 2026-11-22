// One running copy per data folder: a named mutex that Windows drops with the process however it ends, and a message-only window a second start finds by the same name and asks to show the window.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::marker::PhantomData;
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::{Duration, Instant};

use thiserror::Error;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT,
    WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowExW,
    GWLP_USERDATA, GetWindowLongPtrW, GetWindowThreadProcessId, HWND_MESSAGE, PostMessageW,
    RegisterClassExW, SetWindowLongPtrW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WNDCLASSEXW,
};
use windows::core::{PCWSTR, w};

use crate::wide;

const CLASS: PCWSTR = w!("OneLaunch.Instance");
const SHOW: u32 = WM_APP + 1;
const QUIT: u32 = WM_APP + 2;
const LOOK_EVERY: Duration = Duration::from_millis(50);

// Held for the panic hook, which runs on any thread: the lock a relaunch must not find, and the window a request to quit goes to.
static MUTEX: AtomicIsize = AtomicIsize::new(0);
static WINDOW: AtomicIsize = AtomicIsize::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstanceEvent {
    // A second start on this data folder. `quiet`: it came from a run that must take no focus.
    Show { quiet: bool },
    // Asked from another thread, by the crash box's Relaunch.
    Quit,
}

#[derive(Debug, Error)]
pub enum InstanceError {
    #[error("the lock of the data folder could not be made: {0}")]
    Lock(windows::core::Error),
    #[error("the window a second start reaches could not be made: {0}")]
    Window(windows::core::Error),
}

pub enum Claim {
    First(Instance),
    // Another copy runs on this data folder.
    Second,
}

pub struct Instance {
    window: isize,
    mutex: isize,
    // The first holder of the process is the one the panic hook can reach.
    registered: bool,
    on_one_thread: PhantomData<*const ()>,
}

type Heard = Box<dyn Fn(InstanceEvent)>;

struct State {
    heard: RefCell<Option<Heard>>,
    // What came before anyone listened, handed over when someone does.
    waiting: Cell<Option<InstanceEvent>>,
}

// The name both copies derive from the data folder: the same folder written in another case, with `..` in it or a slash at its end is the same lock.
pub fn name_for(data_dir: &Path) -> String {
    let full = std::path::absolute(data_dir).unwrap_or_else(|_| data_dir.to_path_buf());
    let text = full
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase();
    // FNV-1a over UTF-16, so the name has a fixed length whatever the path.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for unit in text.encode_utf16() {
        hash ^= u64::from(unit);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("OneLaunch.Instance.{hash:016x}")
}

// Takes the lock of the data folder before anything of it is opened. A copy that holds it already makes this a second start.
pub fn claim(data_dir: &Path) -> Result<Claim, InstanceError> {
    let name = name_for(data_dir);
    let lock = wide(&format!("Local\\{name}"));
    unsafe {
        let mutex =
            CreateMutexW(None, false, PCWSTR(lock.as_ptr())).map_err(InstanceError::Lock)?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(mutex);
            return Ok(Claim::Second);
        }
        let window = match message_window(&name) {
            Ok(window) => window,
            Err(error) => {
                let _ = CloseHandle(mutex);
                return Err(InstanceError::Window(error));
            }
        };
        let (mutex, window) = (mutex.0 as isize, window.0 as isize);
        let registered = MUTEX
            .compare_exchange(0, mutex, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok();
        if registered {
            WINDOW.store(window, Ordering::SeqCst);
        }
        Ok(Claim::First(Instance {
            window,
            mutex,
            registered,
            on_one_thread: PhantomData,
        }))
    }
}

unsafe fn message_window(name: &str) -> windows::core::Result<HWND> {
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(None)?.into();
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(instance_procedure),
            hInstance: instance,
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassExW(&class);
        let title = wide(name);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CLASS,
            PCWSTR(title.as_ptr()),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance),
            None,
        )?;
        let state = Rc::new(State {
            heard: RefCell::new(None),
            waiting: Cell::new(None),
        });
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Rc::into_raw(state) as isize);
        Ok(hwnd)
    }
}

impl Instance {
    // From now on `heard` is called, on this thread and from the message loop, for each second start and each request to quit. One that came before is handed over at once.
    pub fn listen(&self, heard: impl Fn(InstanceEvent) + 'static) {
        let Some(state) = self.state() else {
            return;
        };
        *state.heard.borrow_mut() = Some(Box::new(heard));
        if let Some(event) = state.waiting.take()
            && let Some(heard) = state.heard.borrow().as_ref()
        {
            heard(event);
        }
    }

    fn state(&self) -> Option<Rc<State>> {
        let hwnd = HWND(self.window as *mut c_void);
        let state = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const State;
        if state.is_null() {
            return None;
        }
        unsafe {
            Rc::increment_strong_count(state);
            Some(Rc::from_raw(state))
        }
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        let hwnd = HWND(self.window as *mut c_void);
        if self.registered {
            WINDOW.store(0, Ordering::SeqCst);
        }
        unsafe {
            let state = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *const State;
            if !state.is_null() {
                drop(Rc::from_raw(state));
            }
            let _ = DestroyWindow(hwnd);
        }
        // A registered lock that `release` already let go is closed already.
        let ours = MUTEX
            .compare_exchange(self.mutex, 0, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok();
        if ours || !self.registered {
            unsafe {
                let _ = CloseHandle(HANDLE(self.mutex as *mut c_void));
            }
        }
    }
}

// Lets the data folder go: for a relaunch, which must find no copy holding it. Safe to call from any thread and more than once.
pub fn release() {
    let mutex = MUTEX.swap(0, Ordering::SeqCst);
    if mutex != 0 {
        unsafe {
            let _ = CloseHandle(HANDLE(mutex as *mut c_void));
        }
    }
}

// Asks the copy's own thread to quit, from any thread. False when there is no window to ask.
pub fn ask_to_quit() -> bool {
    let window = WINDOW.load(Ordering::SeqCst);
    window != 0
        && unsafe {
            PostMessageW(
                Some(HWND(window as *mut c_void)),
                QUIT,
                WPARAM(0),
                LPARAM(0),
            )
        }
        .is_ok()
}

// A second start: finds the copy that holds the data folder, lets it take the foreground unless this run must take no focus, and asks it to show its window. The copy may still be starting, so it is looked for until `wait` has passed. False when it was not found.
pub fn bring_forward(data_dir: &Path, quiet: bool, wait: Duration) -> bool {
    let title = wide(&name_for(data_dir));
    let started = Instant::now();
    loop {
        let found =
            unsafe { FindWindowExW(Some(HWND_MESSAGE), None, CLASS, PCWSTR(title.as_ptr())) };
        if let Ok(hwnd) = found
            && !hwnd.is_invalid()
        {
            unsafe {
                if !quiet {
                    let mut process = 0_u32;
                    GetWindowThreadProcessId(hwnd, Some(&raw mut process));
                    let _ = AllowSetForegroundWindow(process);
                }
                return PostMessageW(Some(hwnd), SHOW, WPARAM(usize::from(quiet)), LPARAM(0))
                    .is_ok();
            }
        }
        if started.elapsed() >= wait {
            return false;
        }
        std::thread::sleep(LOOK_EVERY);
    }
}

unsafe extern "system" fn instance_procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let event = match message {
        SHOW => InstanceEvent::Show {
            quiet: wparam.0 != 0,
        },
        QUIT => InstanceEvent::Quit,
        _ => return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    };
    let state = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const State;
    if state.is_null() {
        return LRESULT(0);
    }
    // A handler may drop the instance; this keeps the state alive until it returns.
    let state = unsafe {
        Rc::increment_strong_count(state);
        Rc::from_raw(state)
    };
    match state.heard.borrow().as_ref() {
        Some(heard) => heard(event),
        None => state.waiting.set(Some(event)),
    }
    LRESULT(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_written_another_way_is_the_same_lock_and_another_folder_is_not() {
        let name = name_for(Path::new(r"D:\Scratch\Data"));
        assert_eq!(name, name_for(Path::new(r"d:\scratch\data\")));
        assert_eq!(name, name_for(Path::new(r"D:\scratch\other\..\data")));
        assert_eq!(name, name_for(Path::new("D:/scratch/data")));
        assert_ne!(name, name_for(Path::new(r"D:\scratch\data2")));
        assert!(name.starts_with("OneLaunch.Instance."));
        assert_eq!(name.len(), "OneLaunch.Instance.".len() + 16);
    }
}
