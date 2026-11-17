// Riot's games and Vanguard. A port never chooses to open, ask the path of, close or end a process with one of these names, nor anything one of them started. Windows names a process by an id it hands out again, so one handed to such a process between a port's last read of the table and its call is the window that is left.
pub const PROTECTED_IMAGES: [&str; 12] = [
    "VALORANT.exe",
    "VALORANT-Win64-Shipping.exe",
    "LeagueClient.exe",
    "LeagueClientUx.exe",
    "LeagueClientUxRender.exe",
    "League of Legends.exe",
    "LoR.exe",
    "2XKO.exe",
    "2XKO-Win64-Shipping.exe",
    "vgc.exe",
    "vgk.exe",
    "vgtray.exe",
];

// An image name as it is compared: in lower case and without its `.exe`.
pub fn image_key(name: &str) -> String {
    let lower = name.to_lowercase();
    match lower.strip_suffix(".exe") {
        Some(stem) => stem.to_owned(),
        None => lower,
    }
}

pub fn is_protected(name: &str) -> bool {
    let key = image_key(name);
    PROTECTED_IMAGES
        .iter()
        .any(|protected| image_key(protected) == key || short_form(name, protected))
}

// A program started by its 8.3 path runs under that name, such as VALORA~1.EXE: it keeps the long name's first two characters without its spaces and has `~` and a digit. Any name of that shape counts, which takes in more than the one.
fn short_form(name: &str, long: &str) -> bool {
    let numbered = name
        .as_bytes()
        .windows(2)
        .any(|pair| pair[0] == b'~' && pair[1].is_ascii_digit());
    let head = |text: &str| -> Vec<u8> {
        text.bytes()
            .filter(|byte| *byte != b' ')
            .take(2)
            .map(|byte| byte.to_ascii_lowercase())
            .collect()
    };
    let start = head(long);
    numbered && start.len() == 2 && head(name) == start
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_protected_name_is_found_in_any_case_with_or_without_its_exe() {
        for name in [
            "VALORANT.exe",
            "valorant",
            "VALORANT-WIN64-SHIPPING.EXE",
            "league of legends.exe",
            "VgTray",
            "vgc.EXE",
            "2xko-win64-shipping",
        ] {
            assert!(is_protected(name), "{name}");
        }
        for name in [
            "RiotClientServices.exe",
            "Riot Client.exe",
            "steam.exe",
            "valorant.exe.exe",
            "",
            "vg",
        ] {
            assert!(!is_protected(name), "{name}");
        }
    }

    #[test]
    fn a_short_name_windows_made_from_a_protected_one_is_protected_and_no_launchers_is() {
        for name in [
            "VALORA~1.EXE",
            "valora~2.exe",
            "LEAGUE~1.EXE",
            "Le ague~4",
            "2XKO-W~1.EXE",
            "VA3F2A~1.EXE",
            "VGTRAY~1.EXE",
            "LOR~1.EXE",
        ] {
            assert!(is_protected(name), "{name}");
        }
        for name in [
            "steam.exe",
            "STEAM~1.EXE",
            "steamwebhelper.exe",
            "STEAMW~1.EXE",
            "EpicGamesLauncher.exe",
            "EPICGA~1.EXE",
            "Battle.net.exe",
            "BATTLE~1.EXE",
            "Battle.net Launcher.exe",
            "BATTLE~2.EXE",
            "RiotClientServices.exe",
            "RIOTCL~1.EXE",
            "Riot Client.exe",
            "RIOTCL~2.EXE",
            "VALORA~X.EXE",
            "V~1.EXE",
            "~1",
        ] {
            assert!(!is_protected(name), "{name}");
        }
    }
}
