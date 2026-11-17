use ol_platform_api::{Shell, ShellError};
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{PCWSTR, w};

use crate::wide;

// The links the adapters hand over: Steam's steam://rungameid/, the Epic Games Launcher's com.epicgames.launcher://apps/ and Battle.net's battlenet://game/. Anything else, a path or a file: link among them, could start a program, and is refused.
pub const SCHEMES: [&str; 3] = ["steam", "com.epicgames.launcher", "battlenet"];
const LONGEST: usize = 2048;

pub fn allowed(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    SCHEMES.contains(&scheme)
        && !rest.is_empty()
        && url.len() <= LONGEST
        && rest
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._~/?=&%:".contains(&byte))
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsShell;

impl Shell for WindowsShell {
    fn open_url(&self, url: &str) -> Result<(), ShellError> {
        if !allowed(url) {
            return Err(ShellError(
                "One Launch opens only the links of Steam, the Epic Games Launcher and Battle.net"
                    .to_owned(),
            ));
        }
        let scheme = url.split_once("://").map_or("", |(scheme, _)| scheme);
        let link = wide(url);
        // A handler may use COM, which ShellExecute expects the calling thread to have set up.
        let com =
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        let opened = unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                PCWSTR(link.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        if com.is_ok() {
            unsafe { CoUninitialize() };
        }
        let code = opened.0 as isize;
        if code <= 32 {
            return Err(ShellError(format!(
                "Windows could not open the {scheme}:// link (error {code})"
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_launchers_links_are_handed_to_windows() {
        for url in [
            "steam://rungameid/730",
            "com.epicgames.launcher://apps/ns%3Aitem%3AFortnite?action=launch&silent=true",
            "com.epicgames.launcher://apps/Fortnite?action=launch&silent=true",
            "battlenet://game/pro",
        ] {
            assert!(allowed(url), "{url}");
        }
        for url in [
            "",
            "steam://",
            "STEAM://rungameid/730",
            "file:///C:/Windows/System32/calc.exe",
            "file://C:/Windows/System32/calc.exe",
            r"C:\Windows\System32\calc.exe",
            "calc.exe",
            "https://example.com/",
            "ms-settings://display",
            "steam:rungameid/730",
            "steam://rungameid/730 -arg",
            "steam://rungameid/730\"",
            "steam://run\"game",
            "steam://rungameid/730\n",
            "steam://rungameid/730\\..\\x",
            "battlenet://game/pro'",
            " steam://rungameid/730",
            "riotclient://launch",
        ] {
            assert!(!allowed(url), "{url}");
        }
        assert!(!allowed(&format!("steam://{}", "1".repeat(LONGEST))));
    }

    #[test]
    fn a_refused_link_is_never_handed_over() {
        let refused = WindowsShell.open_url("file:///C:/nowhere.exe").unwrap_err();
        assert!(refused.0.starts_with("One Launch opens only"));
    }
}
