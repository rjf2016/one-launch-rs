use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::marker::PhantomData;
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use thiserror::Error;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT,
    WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, ChangeWindowMessageFilterEx, CreateWindowExW, DefWindowProcW,
    DestroyWindow, FindWindowExW, GWLP_USERDATA, GetWindowLongPtrW, GetWindowThreadProcessId,
    HWND_MESSAGE, MSGFLT_ALLOW, PostMessageW, RegisterClassExW, SetWindowLongPtrW, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_APP, WM_CLOSE, WNDCLASSEXW,
};
use windows::core::{PCWSTR, w};

use crate::data_folder::{FolderError, identity};
use crate::wide;

const CLASS: PCWSTR = w!("OneLaunch.Instance");
const SHOW: u32 = WM_APP + 1;
const WAKE: u32 = WM_APP + 2;
// A wake posted from outside finds it clear and does nothing.
static QUIT_ASKED: AtomicBool = AtomicBool::new(false);
const LOOK_EVERY: Duration = Duration::from_millis(50);

#[derive(Debug, Error)]
pub enum InstanceError {
    #[error("the data folder could not be told apart from another: {0}")]
    Folder(#[from] FolderError),
    #[error("the lock of the data folder could not be made ({0})")]
    Lock(windows::core::Error),
    #[error("the window a second start reaches could not be made ({0})")]
    Window(windows::core::Error),
}

pub enum Claim {
    First(Instance),
    Second,
}

pub struct Instance {
    window: isize,
    mutex: isize,
    on_one_thread: PhantomData<*const ()>,
}

type Heard = Box<dyn Fn(bool)>;

struct State {
    heard: RefCell<Option<Heard>>,
    waiting: Cell<Option<bool>>,
    quit: RefCell<Option<Box<dyn Fn()>>>,
}

#[derive(Clone, Copy, Debug)]
pub struct QuitAsker(isize);

impl QuitAsker {
    pub fn ask(self) -> bool {
        QUIT_ASKED.store(true, Ordering::SeqCst);
        unsafe {
            PostMessageW(
                Some(HWND(self.0 as *mut c_void)),
                WAKE,
                WPARAM(0),
                LPARAM(0),
            )
        }
        .is_ok()
    }
}

pub fn name_for(data_dir: &Path) -> Result<String, InstanceError> {
    let text = identity(data_dir)?;
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for unit in text.encode_utf16() {
        hash ^= u64::from(unit);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    Ok(format!("OneLaunch.Instance.{hash:016x}"))
}

pub fn claim(data_dir: &Path) -> Result<Claim, InstanceError> {
    let name = name_for(data_dir)?;
    let lock = wide(&format!("Local\\{name}"));
    unsafe {
        let mutex =
            CreateMutexW(None, false, PCWSTR(lock.as_ptr())).map_err(InstanceError::Lock)?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(mutex);
            return Ok(Claim::Second);
        }
        match message_window(&name) {
            Ok(window) => Ok(Claim::First(Instance {
                window: window.0 as isize,
                mutex: mutex.0 as isize,
                on_one_thread: PhantomData,
            })),
            Err(error) => {
                let _ = CloseHandle(mutex);
                Err(InstanceError::Window(error))
            }
        }
    }
}

pub fn claim_waiting(data_dir: &Path, wait: Duration) -> Result<Claim, InstanceError> {
    let started = Instant::now();
    loop {
        let claim = claim(data_dir)?;
        if matches!(claim, Claim::First(_)) || started.elapsed() >= wait {
            return Ok(claim);
        }
        std::thread::sleep(LOOK_EVERY);
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
        // A copy started elevated still hears an ordinary second start.
        let _ = ChangeWindowMessageFilterEx(hwnd, SHOW, MSGFLT_ALLOW, None);
        let state = Rc::new(State {
            heard: RefCell::new(None),
            waiting: Cell::new(None),
            quit: RefCell::new(None),
        });
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Rc::into_raw(state) as isize);
        Ok(hwnd)
    }
}

impl Instance {
    // One that came before is posted again, so that it too is heard from the loop and not inside an update.
    pub fn listen(&self, heard: impl Fn(bool) + 'static) {
        let Some(state) = self.state() else {
            return;
        };
        *state.heard.borrow_mut() = Some(Box::new(heard));
        if let Some(quiet) = state.waiting.take() {
            unsafe {
                let _ = PostMessageW(
                    Some(self.hwnd()),
                    SHOW,
                    WPARAM(usize::from(quiet)),
                    LPARAM(0),
                );
            }
        }
    }

    pub fn on_quit(&self, quit: impl Fn() + 'static) -> QuitAsker {
        if let Some(state) = self.state() {
            *state.quit.borrow_mut() = Some(Box::new(quit));
        }
        QuitAsker(self.window)
    }

    fn hwnd(&self) -> HWND {
        HWND(self.window as *mut c_void)
    }

    fn state(&self) -> Option<Rc<State>> {
        let state = unsafe { GetWindowLongPtrW(self.hwnd(), GWLP_USERDATA) } as *const State;
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
        let hwnd = self.hwnd();
        unsafe {
            let state = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *const State;
            if !state.is_null() {
                drop(Rc::from_raw(state));
            }
            let _ = DestroyWindow(hwnd);
            let _ = CloseHandle(HANDLE(self.mutex as *mut c_void));
        }
    }
}

// The copy may still be starting.
pub fn bring_forward(data_dir: &Path, quiet: bool, wait: Duration) -> bool {
    let Ok(name) = name_for(data_dir) else {
        return false;
    };
    let title = wide(&name);
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

// A close taken from outside would leave every second start without an answer.
unsafe extern "system" fn instance_procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_CLOSE {
        return LRESULT(0);
    }
    if message != SHOW && message != WAKE {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    let state = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const State;
    if state.is_null() {
        return LRESULT(0);
    }
    // A handler may drop the instance; this keeps the state alive until it returns.
    let state = unsafe {
        Rc::increment_strong_count(state);
        Rc::from_raw(state)
    };
    if message == WAKE {
        if QUIT_ASKED.swap(false, Ordering::SeqCst)
            && let Some(quit) = state.quit.borrow().as_ref()
        {
            quit();
        }
        return LRESULT(0);
    }
    let quiet = wparam.0 != 0;
    match state.heard.borrow().as_ref() {
        Some(heard) => heard(quiet),
        None => state.waiting.set(Some(quiet)),
    }
    LRESULT(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_written_another_way_is_the_same_lock_and_another_folder_is_not() {
        let name = name_for(Path::new(r"D:\Scratch\Data")).unwrap();
        assert_eq!(name, name_for(Path::new(r"d:\scratch\data\")).unwrap());
        assert_eq!(
            name,
            name_for(Path::new(r"D:\scratch\other\..\data")).unwrap()
        );
        assert_eq!(name, name_for(Path::new("D:/scratch/data")).unwrap());
        assert_ne!(name, name_for(Path::new(r"D:\scratch\data2")).unwrap());
        assert!(name.starts_with("OneLaunch.Instance."));
        assert_eq!(name.len(), "OneLaunch.Instance.".len() + 16);
    }
}
