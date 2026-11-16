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
        .any(|protected| image_key(protected) == key)
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
}
