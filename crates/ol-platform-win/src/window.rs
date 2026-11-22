use std::ffi::c_void;
use std::ptr;

use thiserror::Error;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, GdiFlush, GetDC, MONITOR_DEFAULTTOPRIMARY, MonitorFromPoint, ReleaseDC,
    SelectObject,
};
use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PW_CLIENTONLY, PrintWindow};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumThreadWindows, GetClassNameW, GetClientRect, GetForegroundWindow, GetWindowPlacement,
    GetWindowRect, IsIconic, IsWindowVisible, IsZoomed, PW_RENDERFULLCONTENT, PostMessageW,
    SW_HIDE, SW_RESTORE, SW_SHOW, SW_SHOWMAXIMIZED, SW_SHOWNA, SW_SHOWNOACTIVATE,
    SetForegroundWindow, SetWindowPlacement, ShowWindow, ShowWindowAsync, USER_DEFAULT_SCREEN_DPI,
    WINDOWPLACEMENT, WM_CHAR,
};
use windows::core::BOOL;

// GPUI gives out a window's handle only through raw-window-handle, which is not an approved dependency; this is the class gpui-pre-windows 0.3.12 registers in window.rs.
const GPUI_WINDOW_CLASS: &str = "Zed::Window";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppWindow(isize);

pub struct Capture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("the window has no client area")]
    Empty,
    #[error("Windows could not prepare the capture: {0}")]
    Os(#[from] windows::core::Error),
    #[error("PrintWindow did not render the window")]
    NotRendered,
}

impl AppWindow {
    pub fn on_this_thread() -> Option<Self> {
        let mut found = 0_isize;
        unsafe {
            let _ = EnumThreadWindows(
                GetCurrentThreadId(),
                Some(keep_gpui_window),
                LPARAM(&raw mut found as isize),
            );
        }
        (found != 0).then_some(Self(found))
    }

    pub fn is_visible(self) -> bool {
        unsafe { IsWindowVisible(self.hwnd()).as_bool() }
    }

    pub fn is_foreground(self) -> bool {
        unsafe { GetForegroundWindow() == self.hwnd() }
    }

    pub fn is_minimized(self) -> bool {
        unsafe { IsIconic(self.hwnd()).as_bool() }
    }

    pub fn is_maximized(self) -> bool {
        unsafe { IsZoomed(self.hwnd()).as_bool() }
    }

    pub fn scale(self) -> f32 {
        unsafe { GetDpiForWindow(self.hwnd()) as f32 / USER_DEFAULT_SCREEN_DPI as f32 }
    }

    pub fn client_size(self) -> Option<(i32, i32)> {
        let mut client = RECT::default();
        unsafe { GetClientRect(self.hwnd(), &mut client) }.ok()?;
        Some((client.right - client.left, client.bottom - client.top))
    }

    pub fn hide(self) {
        unsafe {
            let _ = ShowWindowAsync(self.hwnd(), SW_HIDE);
        }
    }

    // Windows delivers the show and activate messages before these calls return, and GPUI answers them by borrowing the app: never call this from inside a GPUI update.
    pub fn show_and_focus(self) {
        let hwnd = self.hwnd();
        unsafe {
            let command = if IsIconic(hwnd).as_bool() {
                SW_RESTORE
            } else {
                SW_SHOW
            };
            let _ = ShowWindow(hwnd, command);
            let _ = SetForegroundWindow(hwnd);
        }
    }

    // A second start or the tray's Open: out of the tray, out of the taskbar if minimised, maximised when a start in the tray left that for the first show. `focus` false shows it without taking the keyboard from the window that has it, and then without the maximise, which Windows only does with the focus.
    pub fn bring_forward(self, maximize: bool, focus: bool) {
        let hwnd = self.hwnd();
        unsafe {
            let minimized = IsIconic(hwnd).as_bool();
            let command = match (focus, maximize, minimized) {
                (true, true, _) => SW_SHOWMAXIMIZED,
                (true, false, true) => SW_RESTORE,
                (true, false, false) => SW_SHOW,
                (false, _, true) => SW_SHOWNOACTIVATE,
                (false, _, false) => SW_SHOWNA,
            };
            let _ = ShowWindow(hwnd, command);
            if focus {
                let _ = SetForegroundWindow(hwnd);
            }
        }
    }

    // Puts a window that was made hidden where it is to show, its client area at `x`, `y`, `width` by `height` in logical pixels of the primary display, as GPUI places a shown one; GPUI keeps that place for a window it shows itself, which this app never asks it to.
    pub fn place_hidden(self, x: f32, y: f32, width: f32, height: f32) {
        let hwnd = self.hwnd();
        unsafe {
            let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
            let (mut dpi_x, mut dpi_y) = (USER_DEFAULT_SCREEN_DPI, USER_DEFAULT_SCREEN_DPI);
            let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
            let scale = dpi_x as f32 / USER_DEFAULT_SCREEN_DPI as f32;
            let mut frame = RECT::default();
            let mut client = RECT::default();
            if GetWindowRect(hwnd, &mut frame).is_err() || GetClientRect(hwnd, &mut client).is_err()
            {
                return;
            }
            // As GPUI's `calculate_window_rect` turns a client area into a window: the frame's extra width and height, half on each side.
            let wide = (frame.right - frame.left) - (client.right - client.left);
            let tall = (frame.bottom - frame.top) - (client.bottom - client.top);
            let left = (x * scale).round() as i32;
            let top = (y * scale).round() as i32;
            let right = left + (width * scale).round() as i32;
            let bottom = top + (height * scale).round() as i32;
            let mut placement = WINDOWPLACEMENT {
                length: size_of::<WINDOWPLACEMENT>() as u32,
                ..Default::default()
            };
            if GetWindowPlacement(hwnd, &mut placement).is_err() {
                return;
            }
            placement.rcNormalPosition = RECT {
                left: left - wide / 2,
                top: top - tall / 2,
                right: right + (wide - wide / 2),
                bottom: bottom + (tall - tall / 2),
            };
            placement.showCmd = SW_HIDE.0 as u32;
            let _ = SetWindowPlacement(hwnd, &placement);
        }
    }

    pub fn post_text(self, text: &str) {
        for unit in text.encode_utf16() {
            unsafe {
                let _ = PostMessageW(
                    Some(self.hwnd()),
                    WM_CHAR,
                    WPARAM(usize::from(unit)),
                    LPARAM(1),
                );
            }
        }
    }

    pub fn capture(self) -> Result<Capture, CaptureError> {
        let hwnd = self.hwnd();
        let mut client = RECT::default();
        unsafe { GetClientRect(hwnd, &mut client) }?;
        let (width, height) = (client.right - client.left, client.bottom - client.top);
        if width <= 0 || height <= 0 {
            return Err(CaptureError::Empty);
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let length = width as usize * height as usize * 4;
        let flags = PRINT_WINDOW_FLAGS(PW_CLIENTONLY.0 | PW_RENDERFULLCONTENT);

        let pixels = unsafe {
            let screen = GetDC(None);
            let memory = CreateCompatibleDC(Some(screen));
            let mut bits: *mut c_void = ptr::null_mut();
            let pixels = CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut bits, None, 0)
                .map_err(CaptureError::Os)
                .and_then(|bitmap| {
                    let previous = SelectObject(memory, bitmap.into());
                    let rendered = PrintWindow(hwnd, memory, flags).as_bool();
                    let _ = GdiFlush();
                    let pixels = (rendered && !bits.is_null())
                        .then(|| std::slice::from_raw_parts(bits.cast::<u8>(), length).to_vec());
                    SelectObject(memory, previous);
                    let _ = DeleteObject(bitmap.into());
                    pixels.ok_or(CaptureError::NotRendered)
                });
            let _ = DeleteDC(memory);
            ReleaseDC(None, screen);
            pixels
        };

        let mut rgba = pixels?;
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
            pixel[3] = 255;
        }
        Ok(Capture {
            width: width as u32,
            height: height as u32,
            rgba,
        })
    }

    pub(crate) fn hwnd(self) -> HWND {
        HWND(self.0 as *mut c_void)
    }
}

unsafe extern "system" fn keep_gpui_window(hwnd: HWND, found: LPARAM) -> BOOL {
    let mut name = [0_u16; 64];
    let length = unsafe { GetClassNameW(hwnd, &mut name) }.max(0) as usize;
    if String::from_utf16_lossy(&name[..length]) != GPUI_WINDOW_CLASS {
        return true.into();
    }
    unsafe { *(found.0 as *mut isize) = hwnd.0 as isize };
    false.into()
}
