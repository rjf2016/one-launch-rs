// A stand-in program for the tests of the process port, copied under a name of its own for each test. `--window` gives it a hidden top-level window and it exits when that window is asked to close; `--child=<exe>` starts another program through the port; `--echo=<file>` writes its arguments and its folder there, after the child. It always exits within a minute.

use std::time::Duration;

use ol_platform_api::ProcessRunner;
use ol_platform_win::processes::WindowsProcesses;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, MSG,
    PostQuitMessage, RegisterClassExW, SetTimer, WINDOW_EX_STYLE, WM_DESTROY, WM_TIMER,
    WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
};
use windows::core::w;

const LIFETIME: Duration = Duration::from_secs(60);

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| {
        args.iter()
            .find_map(|arg| arg.strip_prefix(name).map(str::to_owned))
    };
    if let Some(exe) = value("--child=") {
        let _ = WindowsProcesses::new().start(&exe, &[], "");
    }
    // Written last: a test that sees the file knows the child, if any, has been started in full.
    if let Some(file) = value("--echo=") {
        let folder = std::env::current_dir().unwrap_or_default();
        let mut text = format!("{}\n", folder.display());
        for arg in &args {
            text.push_str(arg);
            text.push('\n');
        }
        let _ = std::fs::write(file, text);
    }
    if args.iter().any(|arg| arg == "--window") {
        run_window();
    } else {
        std::thread::sleep(LIFETIME);
    }
}

unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_TIMER => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn run_window() {
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else {
            return;
        };
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(procedure),
            hInstance: module.into(),
            lpszClassName: w!("OneLaunch.TestChild"),
            ..Default::default()
        };
        RegisterClassExW(&class);
        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("OneLaunch.TestChild"),
            w!("One Launch test child"),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            100,
            100,
            None,
            None,
            Some(module.into()),
            None,
        ) else {
            return;
        };
        SetTimer(Some(hwnd), 1, LIFETIME.as_millis() as u32, None);
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            DispatchMessageW(&message);
        }
    }
}
