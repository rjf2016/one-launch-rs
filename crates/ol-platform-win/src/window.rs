use std::ffi::c_void;
use std::ptr;
use std::time::Duration;

use thiserror::Error;
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, EnumDisplayMonitors, GdiFlush, GetDC, GetMonitorInfoW, HDC, HMONITOR,
    MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect, ReleaseDC, SelectObject,
};
use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PW_CLIENTONLY, PrintWindow};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumThreadWindows, GetClassNameW, GetClientRect, GetForegroundWindow, GetWindowPlacement,
    GetWindowRect, IsIconic, IsWindowVisible, IsZoomed, MONITORINFOF_PRIMARY, PW_RENDERFULLCONTENT,
    PostMessageW, SW_HIDE, SW_RESTORE, SW_SHOW, SW_SHOWMAXIMIZED, SW_SHOWNA, SW_SHOWNOACTIVATE,
    SetForegroundWindow, SetWindowPlacement, ShowWindow, ShowWindowAsync, USER_DEFAULT_SCREEN_DPI,
    WINDOWPLACEMENT, WM_CHAR,
};
use windows::core::BOOL;

const SCREEN_LIMIT: i32 = 1 << 24;

// A placement counts from its monitor's work area, not the screen: a taskbar on the left or the top shifts it, a second monitor by itself does not.
fn in_workspace(screen: RECT, monitor: RECT, work: RECT) -> RECT {
    let dx = work.left.saturating_sub(monitor.left);
    let dy = work.top.saturating_sub(monitor.top);
    RECT {
        left: screen.left.saturating_sub(dx),
        top: screen.top.saturating_sub(dy),
        right: screen.right.saturating_sub(dx),
        bottom: screen.bottom.saturating_sub(dy),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Display {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    pub primary: bool,
}

pub fn displays() -> Vec<Display> {
    let mut found: Vec<Display> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(keep_display),
            LPARAM(&raw mut found as isize),
        );
    }
    found
}

unsafe extern "system" fn keep_display(
    monitor: HMONITOR,
    _: HDC,
    _: *mut RECT,
    found: LPARAM,
) -> BOOL {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return true.into();
    }
    let (mut dpi_x, mut dpi_y) = (USER_DEFAULT_SCREEN_DPI, USER_DEFAULT_SCREEN_DPI);
    let _ = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
    let scale = f64::from(dpi_x) / f64::from(USER_DEFAULT_SCREEN_DPI);
    let work = info.rcWork;
    let found = unsafe { &mut *(found.0 as *mut Vec<Display>) };
    found.push(Display {
        x: f64::from(work.left) / scale,
        y: f64::from(work.top) / scale,
        width: f64::from(work.right - work.left) / scale,
        height: f64::from(work.bottom - work.top) / scale,
        scale,
        primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
    });
    true.into()
}

// GPUI gives out a window's handle only through raw-window-handle, which is not an approved dependency; this is the class gpui-pre-windows 0.3.12 registers in window.rs.
const GPUI_WINDOW_CLASS: &str = "Zed::Window";

// Read at every ask, as gpui-pre-windows 0.3.12 (window.rs, ClickState) reads it again when the setting changes.
pub fn double_click_time() -> Duration {
    Duration::from_millis(u64::from(unsafe { GetDoubleClickTime() }))
}

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

    // Windows maximises only with the focus.
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

    // As GPUI's `calculate_window_rect` turns a client area into a window.
    pub fn place_hidden(self, x: f64, y: f64, width: f64, height: f64, scale: f64) {
        let hwnd = self.hwnd();
        unsafe {
            let mut frame = RECT::default();
            let mut client = RECT::default();
            if GetWindowRect(hwnd, &mut frame).is_err() || GetClientRect(hwnd, &mut client).is_err()
            {
                return;
            }
            let wide = (frame.right - frame.left) - (client.right - client.left);
            let tall = (frame.bottom - frame.top) - (client.bottom - client.top);
            let device = |logical: f64| {
                (logical * scale)
                    .round()
                    .clamp(f64::from(-SCREEN_LIMIT), f64::from(SCREEN_LIMIT)) as i32
            };
            let (left, top) = (device(x), device(y));
            let right = left.saturating_add(device(width));
            let bottom = top.saturating_add(device(height));
            let mut placement = WINDOWPLACEMENT {
                length: size_of::<WINDOWPLACEMENT>() as u32,
                ..Default::default()
            };
            if GetWindowPlacement(hwnd, &mut placement).is_err() {
                return;
            }
            let screen = RECT {
                left: left.saturating_sub(wide / 2),
                top: top.saturating_sub(tall / 2),
                right: right.saturating_add(wide - wide / 2),
                bottom: bottom.saturating_add(tall - tall / 2),
            };
            let mut monitor = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let found = MonitorFromRect(&screen, MONITOR_DEFAULTTONEAREST);
            placement.rcNormalPosition = if GetMonitorInfoW(found, &mut monitor).as_bool() {
                in_workspace(screen, monitor.rcMonitor, monitor.rcWork)
            } else {
                screen
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn a_place_on_the_screen_is_moved_into_its_monitors_work_area() {
        let window = rect(300, 200, 1100, 800);
        let primary = rect(0, 0, 2560, 1440);
        assert_eq!(
            in_workspace(window, primary, rect(0, 0, 2560, 1392)),
            window
        );
        assert_eq!(
            in_workspace(window, primary, rect(48, 0, 2560, 1440)),
            rect(252, 200, 1052, 800)
        );
        assert_eq!(
            in_workspace(window, primary, rect(0, 48, 2560, 1440)),
            rect(300, 152, 1100, 752)
        );
        let second = rect(2560, 0, 5120, 1440);
        let there = rect(3000, 200, 3800, 800);
        assert_eq!(
            in_workspace(there, second, rect(2560, 0, 5120, 1392)),
            there
        );
        assert_eq!(
            in_workspace(there, second, rect(2608, 0, 5120, 1440)),
            rect(2952, 200, 3752, 800)
        );
        let left = rect(-2560, -300, 0, 1140);
        assert_eq!(
            in_workspace(
                rect(-2000, -100, -1200, 500),
                left,
                rect(-2560, -252, 0, 1140)
            ),
            rect(-2000, -148, -1200, 452)
        );
    }
}
