// For the app's --probe run only. These make Windows call a window procedure of this app, at once or from the queue: never call them from inside a GPUI update.

use windows::Win32::Foundation::{LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Shell::{NOTIFYICONIDENTIFIER, Shell_NotifyIconGetRect};
use windows::Win32::UI::WindowsAndMessaging::{
    EndMenu, GUI_INMENUMODE, GUITHREADINFO, GetGUIThreadInfo, GetWindowRect, HTBOTTOM,
    HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTCLIENT, HTCLOSE, HTLEFT, HTMAXBUTTON, HTMINBUTTON,
    HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, PostMessageW, SWP_NOACTIVATE, SWP_NOZORDER,
    SendMessageW, SetWindowPos, WM_CONTEXTMENU, WM_KEYDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP,
};

use crate::tray::{CALLBACK, ICON_ID, Tray};
use crate::window::AppWindow;

const VK_RETURN: usize = 0x0d;
const VK_UP: usize = 0x26;

#[derive(Clone, Copy)]
pub enum MenuKey {
    Up,
    Enter,
}

pub fn tray_is_shown(tray: &Tray) -> bool {
    tray_rect(tray).is_some()
}

pub fn tray_click(tray: &Tray) {
    post_to_tray(tray, WM_LBUTTONUP, 0);
}

pub fn tray_menu(tray: &Tray) {
    let anchor = tray_rect(tray).map_or(0, |icon| {
        packed((icon.left + icon.right) / 2, (icon.top + icon.bottom) / 2).0
    });
    post_to_tray(tray, WM_CONTEXTMENU, anchor as usize);
}

pub fn tray_menu_is_open(tray: &Tray) -> bool {
    let mut thread = GUITHREADINFO {
        cbSize: size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetGUIThreadInfo(GetCurrentThreadId(), &mut thread) }.is_ok()
        && thread.flags.0 & GUI_INMENUMODE.0 != 0
        && thread.hwndMenuOwner == tray.hwnd()
}

pub fn tray_menu_key(tray: &Tray, key: MenuKey) {
    let code = match key {
        MenuKey::Up => VK_UP,
        MenuKey::Enter => VK_RETURN,
    };
    unsafe {
        let _ = PostMessageW(Some(tray.hwnd()), WM_KEYDOWN, WPARAM(code), LPARAM(0));
    }
}

pub fn close_menu() {
    unsafe {
        let _ = EndMenu();
    }
}

fn tray_rect(tray: &Tray) -> Option<RECT> {
    let icon = NOTIFYICONIDENTIFIER {
        cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
        hWnd: tray.hwnd(),
        uID: ICON_ID,
        ..Default::default()
    };
    unsafe { Shell_NotifyIconGetRect(&icon) }.ok()
}

fn post_to_tray(tray: &Tray, event: u32, wparam: usize) {
    unsafe {
        let _ = PostMessageW(
            Some(tray.hwnd()),
            CALLBACK,
            WPARAM(wparam),
            LPARAM(event as isize),
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Area {
    Client,
    Caption,
    Minimize,
    Maximize,
    Close,
    Left,
    Right,
    Top,
    TopLeft,
    TopRight,
    Bottom,
    BottomLeft,
    BottomRight,
    Other(u32),
}

const AREAS: [(Area, u32); 13] = [
    (Area::Client, HTCLIENT),
    (Area::Caption, HTCAPTION),
    (Area::Minimize, HTMINBUTTON),
    (Area::Maximize, HTMAXBUTTON),
    (Area::Close, HTCLOSE),
    (Area::Left, HTLEFT),
    (Area::Right, HTRIGHT),
    (Area::Top, HTTOP),
    (Area::TopLeft, HTTOPLEFT),
    (Area::TopRight, HTTOPRIGHT),
    (Area::Bottom, HTBOTTOM),
    (Area::BottomLeft, HTBOTTOMLEFT),
    (Area::BottomRight, HTBOTTOMRIGHT),
];

impl Area {
    fn code(self) -> u32 {
        match self {
            Self::Other(code) => code,
            known => AREAS
                .iter()
                .find(|(area, _)| *area == known)
                .map_or(0, |(_, code)| *code),
        }
    }

    fn of(code: u32) -> Self {
        AREAS
            .iter()
            .find(|(_, known)| *known == code)
            .map_or(Self::Other(code), |(area, _)| *area)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct OuterRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

// GPUI answers a hit test from the last pointer position it was told, so that is sent first; both points are in client pixels.
pub fn area_at(window: AppWindow, pointer: (i32, i32), at: (i32, i32)) -> Area {
    let position = on_screen(window, at.0, at.1);
    let answer = unsafe {
        SendMessageW(
            window.hwnd(),
            WM_MOUSEMOVE,
            None,
            Some(packed(pointer.0, pointer.1)),
        );
        SendMessageW(window.hwnd(), WM_NCHITTEST, None, Some(position))
    };
    Area::of(answer.0 as u32)
}

pub fn click(window: AppWindow, area: Area, x: i32, y: i32) {
    post_frame_message(window, WM_NCLBUTTONDOWN, area, x, y);
    post_frame_message(window, WM_NCLBUTTONUP, area, x, y);
}

pub fn double_click(window: AppWindow, area: Area, x: i32, y: i32) {
    post_frame_message(window, WM_NCLBUTTONDBLCLK, area, x, y);
    post_frame_message(window, WM_NCLBUTTONUP, area, x, y);
}

pub fn outer_rect(window: AppWindow) -> Option<OuterRect> {
    let mut rect = RECT::default();
    unsafe { GetWindowRect(window.hwnd(), &mut rect) }.ok()?;
    Some(OuterRect {
        x: rect.left,
        y: rect.top,
        width: rect.right - rect.left,
        height: rect.bottom - rect.top,
    })
}

pub fn set_outer_rect(window: AppWindow, rect: OuterRect) {
    unsafe {
        let _ = SetWindowPos(
            window.hwnd(),
            None,
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

fn post_frame_message(window: AppWindow, message: u32, area: Area, x: i32, y: i32) {
    unsafe {
        let _ = PostMessageW(
            Some(window.hwnd()),
            message,
            WPARAM(area.code() as usize),
            on_screen(window, x, y),
        );
    }
}

fn on_screen(window: AppWindow, x: i32, y: i32) -> LPARAM {
    let mut point = POINT { x, y };
    unsafe {
        let _ = ClientToScreen(window.hwnd(), &mut point);
    }
    packed(point.x, point.y)
}

fn packed(x: i32, y: i32) -> LPARAM {
    LPARAM(((y as u16 as isize) << 16) | (x as u16 as isize))
}
