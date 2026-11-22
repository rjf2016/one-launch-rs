use std::ffi::c_void;
use std::marker::PhantomData;
use std::rc::Rc;

use thiserror::Error;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION, NIN_SELECT,
    NINF_KEY, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    GWLP_USERDATA, GetSystemMetrics, GetWindowLongPtrW, HICON, IDI_APPLICATION, IMAGE_ICON,
    LR_SHARED, LoadIconW, LoadImageW, MF_SEPARATOR, MF_STRING, PostMessageW, RegisterClassExW,
    RegisterWindowMessageW, SM_CXSMICON, SM_CYSMICON, SetForegroundWindow, SetWindowLongPtrW,
    TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_APP, WM_CONTEXTMENU, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_NULL, WNDCLASSEXW,
};
use windows::core::{PCWSTR, w};

use crate::wide;

const CLASS: PCWSTR = w!("OneLaunch.Tray");
pub(crate) const CALLBACK: u32 = WM_APP + 1;
const DEFERRED: u32 = WM_APP + 2;
pub(crate) const ICON_ID: u32 = 1;
const EXE_ICON: PCWSTR = PCWSTR(std::ptr::without_provenance(1));
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;

pub enum TrayEvent {
    Activate,
    Menu(MenuAnchor),
}

pub enum MenuEntry<'a> {
    Item(&'a str),
    Separator,
}

pub struct MenuAnchor {
    owner: isize,
    x: i32,
    y: i32,
}

#[derive(Debug, Error)]
pub enum TrayError {
    #[error("the tray's helper window could not be created: {0}")]
    Window(#[from] windows::core::Error),
    #[error("Windows did not accept the tray icon")]
    Refused,
}

impl TrayError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Window(_) => "TRAY_WINDOW",
            Self::Refused => "TRAY_REFUSED",
        }
    }

    pub fn hresult(&self) -> Option<i32> {
        match self {
            Self::Window(error) => Some(error.code().0),
            Self::Refused => None,
        }
    }
}

pub struct Tray {
    hwnd: isize,
    shows_app_icon: bool,
    on_one_thread: PhantomData<*const ()>,
}

struct State {
    on_event: Box<dyn Fn(TrayEvent)>,
    icon: NOTIFYICONDATAW,
    taskbar_created: u32,
}

impl Tray {
    pub fn new(tooltip: &str, on_event: impl Fn(TrayEvent) + 'static) -> Result<Self, TrayError> {
        unsafe {
            let instance: HINSTANCE = GetModuleHandleW(None)?.into();
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(tray_procedure),
                hInstance: instance,
                lpszClassName: CLASS,
                ..Default::default()
            };
            RegisterClassExW(&class);
            // Not a message-only window: those never get the TaskbarCreated broadcast.
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                CLASS,
                PCWSTR::null(),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                None,
            )?;

            let app_icon = app_icon(instance);
            let mut icon = NOTIFYICONDATAW {
                cbSize: size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: ICON_ID,
                uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP,
                uCallbackMessage: CALLBACK,
                hIcon: app_icon
                    .or_else(|| LoadIconW(None, IDI_APPLICATION).ok())
                    .unwrap_or_default(),
                ..Default::default()
            };
            let room = icon.szTip.len() - 1;
            for (slot, unit) in icon.szTip.iter_mut().zip(tooltip.encode_utf16().take(room)) {
                *slot = unit;
            }
            icon.Anonymous.uVersion = NOTIFYICON_VERSION_4;

            let state = Rc::new(State {
                on_event: Box::new(on_event),
                icon,
                taskbar_created: RegisterWindowMessageW(w!("TaskbarCreated")),
            });
            if !state.add() {
                let _ = Shell_NotifyIconW(NIM_DELETE, &state.icon);
                let _ = DestroyWindow(hwnd);
                return Err(TrayError::Refused);
            }
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Rc::into_raw(state) as isize);
            Ok(Self {
                hwnd: hwnd.0 as isize,
                shows_app_icon: app_icon.is_some(),
                on_one_thread: PhantomData,
            })
        }
    }

    pub fn shows_app_icon(&self) -> bool {
        self.shows_app_icon
    }

    pub(crate) fn hwnd(&self) -> HWND {
        HWND(self.hwnd as *mut c_void)
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        let hwnd = self.hwnd();
        unsafe {
            let state = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *const State;
            if !state.is_null() {
                let state = Rc::from_raw(state);
                let _ = Shell_NotifyIconW(NIM_DELETE, &state.icon);
            }
            let _ = DestroyWindow(hwnd);
        }
    }
}

impl State {
    fn add(&self) -> bool {
        unsafe {
            Shell_NotifyIconW(NIM_ADD, &self.icon).as_bool()
                && Shell_NotifyIconW(NIM_SETVERSION, &self.icon).as_bool()
        }
    }
}

impl MenuAnchor {
    // Runs a modal message loop until the menu closes. Call it from the tray handler: a GPUI foreground task that waits here holds up every other foreground task.
    pub fn show(&self, entries: &[MenuEntry<'_>]) -> Option<usize> {
        let owner = HWND(self.owner as *mut c_void);
        unsafe {
            let menu = CreatePopupMenu().ok()?;
            for (index, entry) in entries.iter().enumerate() {
                let _ = match entry {
                    MenuEntry::Item(label) => {
                        let label = wide(label);
                        AppendMenuW(menu, MF_STRING, index + 1, PCWSTR(label.as_ptr()))
                    }
                    MenuEntry::Separator => AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null()),
                };
            }
            // A menu whose owner is not the foreground window stays open when the user clicks elsewhere.
            let _ = SetForegroundWindow(owner);
            let chosen = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
                self.x,
                self.y,
                None,
                owner,
                None,
            );
            let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
            let _ = DestroyMenu(menu);
            usize::try_from(chosen.0).ok()?.checked_sub(1)
        }
    }
}

fn app_icon(instance: HINSTANCE) -> Option<HICON> {
    unsafe {
        let (width, height) = (GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON));
        LoadImageW(
            Some(instance),
            EXE_ICON,
            IMAGE_ICON,
            width,
            height,
            LR_SHARED,
        )
        .ok()
        .map(|icon| HICON(icon.0))
    }
}

fn signed_word(value: usize) -> i32 {
    i32::from(value as u16 as i16)
}

unsafe extern "system" fn tray_procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const State;
    if state.is_null() {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    // A handler may drop the tray; this keeps the state alive until the handler returns.
    let state = unsafe {
        Rc::increment_strong_count(state);
        Rc::from_raw(state)
    };

    if message == CALLBACK {
        let event = lparam.0 as u32 & 0xffff;
        if matches!(
            event,
            WM_LBUTTONUP | WM_LBUTTONDBLCLK | NIN_KEYSELECT | WM_CONTEXTMENU
        ) {
            // The shell's message can land in the middle of a GPUI update; the handler runs from a posted message instead, which only a message loop picks up.
            unsafe {
                let _ = PostMessageW(Some(hwnd), DEFERRED, wparam, LPARAM(event as isize));
            }
        }
        return LRESULT(0);
    }
    if message == DEFERRED {
        (state.on_event)(if lparam.0 as u32 == WM_CONTEXTMENU {
            TrayEvent::Menu(MenuAnchor {
                owner: hwnd.0 as isize,
                x: signed_word(wparam.0),
                y: signed_word(wparam.0 >> 16),
            })
        } else {
            TrayEvent::Activate
        });
        return LRESULT(0);
    }
    if message == state.taskbar_created && message != 0 {
        state.add();
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}
