// Windows paths as the Electron app handles them: Node's win32 path rules, worked on UTF-16 units because the lengths they compare are counted in them.

use crate::js;

const SEP: u16 = b'\\' as u16;
const SLASH: u16 = b'/' as u16;
const DOT: u16 = b'.' as u16;
const COLON: u16 = b':' as u16;

const fn is_sep(unit: u16) -> bool {
    unit == SEP || unit == SLASH
}

const fn is_drive_letter(unit: u16) -> bool {
    matches!(unit, 0x41..=0x5a | 0x61..=0x7a)
}

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn text(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

fn last_sep(units: &[u16]) -> Option<usize> {
    units.iter().rposition(|unit| *unit == SEP)
}

fn normalize_tail(path: &[u16], allow_above_root: bool) -> Vec<u16> {
    let mut res: Vec<u16> = Vec::new();
    let mut last_segment = 0_usize;
    let mut last_slash: isize = -1;
    let mut dots: i32 = 0;
    let mut code = 0_u16;
    for i in 0..=path.len() {
        if i < path.len() {
            code = path[i];
        } else if is_sep(code) {
            break;
        } else {
            code = SLASH;
        }
        let at = isize::try_from(i).expect("a path fits in memory");
        if is_sep(code) {
            if last_slash == at - 1 || dots == 1 {
            } else if dots == 2 {
                let ends_with_dots = res.len() >= 2
                    && last_segment == 2
                    && res[res.len() - 1] == DOT
                    && res[res.len() - 2] == DOT;
                if !ends_with_dots {
                    if res.len() > 2 {
                        match last_sep(&res) {
                            None => {
                                res.clear();
                                last_segment = 0;
                            }
                            Some(cut) => {
                                res.truncate(cut);
                                last_segment = match last_sep(&res) {
                                    Some(before) => res.len() - 1 - before,
                                    None => res.len(),
                                };
                            }
                        }
                        last_slash = at;
                        dots = 0;
                        continue;
                    } else if !res.is_empty() {
                        res.clear();
                        last_segment = 0;
                        last_slash = at;
                        dots = 0;
                        continue;
                    }
                }
                if allow_above_root {
                    if !res.is_empty() {
                        res.push(SEP);
                    }
                    res.extend([DOT, DOT]);
                    last_segment = 2;
                }
            } else {
                let from = usize::try_from(last_slash + 1).expect("not negative");
                if !res.is_empty() {
                    res.push(SEP);
                }
                res.extend_from_slice(&path[from..i]);
                last_segment = i - from;
            }
            last_slash = at;
            dots = 0;
        } else if code == DOT && dots != -1 {
            dots += 1;
        } else {
            dots = -1;
        }
    }
    res
}

struct Root {
    device: Option<Vec<u16>>,
    end: usize,
    absolute: bool,
    // A share named in full with nothing after it: the path is its own normal form plus a separator.
    bare_share: bool,
}

fn root_of(path: &[u16]) -> Root {
    let len = path.len();
    let mut root = Root {
        device: None,
        end: 0,
        absolute: false,
        bare_share: false,
    };
    if len == 0 {
        return root;
    }
    let code = path[0];
    if len == 1 {
        if is_sep(code) {
            root.end = 1;
            root.absolute = true;
        }
        return root;
    }
    if is_sep(code) {
        root.absolute = true;
        if is_sep(path[1]) {
            let mut j = 2;
            let mut last = j;
            while j < len && !is_sep(path[j]) {
                j += 1;
            }
            if j < len && j != last {
                let first = &path[last..j];
                last = j;
                while j < len && is_sep(path[j]) {
                    j += 1;
                }
                if j < len && j != last {
                    last = j;
                    while j < len && !is_sep(path[j]) {
                        j += 1;
                    }
                    if j == len || j != last {
                        let mut device = vec![SEP, SEP];
                        device.extend_from_slice(first);
                        if first == [DOT] || first == [b'?' as u16] {
                            root.device = Some(device);
                            root.end = 4;
                        } else {
                            device.push(SEP);
                            device.extend_from_slice(&path[last..j]);
                            root.device = Some(device);
                            root.end = j;
                            root.bare_share = j == len;
                        }
                    }
                }
            }
        } else {
            root.end = 1;
        }
    } else if is_drive_letter(code) && path[1] == COLON {
        root.device = Some(path[..2].to_vec());
        root.end = 2;
        if len > 2 && is_sep(path[2]) {
            root.absolute = true;
            root.end = 3;
        }
    }
    root
}

fn normalize_units(path: &[u16]) -> Vec<u16> {
    let len = path.len();
    if len == 0 {
        return vec![DOT];
    }
    if len == 1 {
        return if is_sep(path[0]) {
            vec![SEP]
        } else {
            path.to_vec()
        };
    }
    let root = root_of(path);
    if root.bare_share {
        let mut share = root.device.expect("a share has a device");
        share.push(SEP);
        return share;
    }
    let mut tail = if root.end < len {
        normalize_tail(&path[root.end.min(len)..], !root.absolute)
    } else {
        Vec::new()
    };
    if tail.is_empty() && !root.absolute {
        tail.push(DOT);
    }
    if !tail.is_empty() && is_sep(path[len - 1]) {
        tail.push(SEP);
    }
    if !root.absolute && root.device.is_none() && path.contains(&COLON) {
        // What is left must not read as a drive of its own: the path named none.
        let reads_as_drive = tail.len() >= 2 && is_drive_letter(tail[0]) && tail[1] == COLON;
        let colon_ends_a_segment = path
            .iter()
            .enumerate()
            .any(|(index, unit)| *unit == COLON && (index == len - 1 || is_sep(path[index + 1])));
        if reads_as_drive || colon_ends_a_segment {
            let mut kept = vec![DOT, SEP];
            kept.extend(tail);
            return kept;
        }
    }
    let mut out = root.device.unwrap_or_default();
    if root.absolute {
        out.push(SEP);
    }
    out.extend(tail);
    out
}

pub fn normalize(path: &str) -> String {
    text(&normalize_units(&units(path)))
}

pub fn join(parts: &[&str]) -> String {
    let mut joined: Vec<u16> = Vec::new();
    let mut first: Option<Vec<u16>> = None;
    for part in parts.iter().filter(|part| !part.is_empty()) {
        let part = units(part);
        if first.is_none() {
            first = Some(part.clone());
        } else {
            joined.push(SEP);
        }
        joined.extend(part);
    }
    let Some(first) = first else {
        return ".".to_owned();
    };
    // Two separators at the start would read as a share, unless the first part is one.
    let mut needs_replace = true;
    let mut slashes = 0;
    if is_sep(first[0]) {
        slashes += 1;
        if first.len() > 1 && is_sep(first[1]) {
            slashes += 1;
            if first.len() > 2 {
                if is_sep(first[2]) {
                    slashes += 1;
                } else {
                    needs_replace = false;
                }
            }
        }
    }
    if needs_replace {
        while slashes < joined.len() && is_sep(joined[slashes]) {
            slashes += 1;
        }
        if slashes >= 2 {
            let mut replaced = vec![SEP];
            replaced.extend_from_slice(&joined[slashes..]);
            joined = replaced;
        }
    }
    text(&normalize_units(&joined))
}

// `path` taken from inside `base`, or None when the answer would depend on the folder the app itself runs in.
pub fn resolve_from(base: &str, path: &str) -> Option<String> {
    let mut device: Vec<u16> = Vec::new();
    let mut tail: Vec<u16> = Vec::new();
    let mut absolute = false;
    for part in [path, base] {
        let part = units(part);
        if part.is_empty() {
            continue;
        }
        let root = root_of(&part);
        if let Some(found) = &root.device {
            if device.is_empty() {
                device.clone_from(found);
            } else if text(found).to_lowercase() != text(&device).to_lowercase() {
                continue;
            }
        }
        if absolute {
            if !device.is_empty() {
                break;
            }
        } else {
            let mut joined = part[root.end.min(part.len())..].to_vec();
            joined.push(SEP);
            joined.extend(tail);
            tail = joined;
            absolute = root.absolute;
            if absolute && !device.is_empty() {
                break;
            }
        }
    }
    if !absolute || device.is_empty() {
        return None;
    }
    let mut out = device;
    out.push(SEP);
    out.extend(normalize_tail(&tail, false));
    Some(text(&out))
}

// A path read from a launcher's file or registry value: trimmed, doubled backslashes undone, separators made one backslash, dot segments resolved, no separator at the end.
pub fn normalize_launcher_path(raw: &str) -> String {
    let trimmed = js::trim(raw).replace("\\\\", "\\");
    let mut collapsed: Vec<u16> = Vec::with_capacity(trimmed.len());
    for unit in trimmed.encode_utf16() {
        if is_sep(unit) {
            if collapsed.last() != Some(&SEP) {
                collapsed.push(SEP);
            }
        } else {
            collapsed.push(unit);
        }
    }
    let mut normal = normalize_units(&collapsed);
    if normal.len() > 3 && normal.last() == Some(&SEP) {
        normal.pop();
    }
    text(&normal)
}

// Two paths with the same key are the same Windows path.
pub fn path_key(path: &str) -> String {
    normalize_launcher_path(path).to_lowercase()
}

pub fn same_path(a: &str, b: &str) -> bool {
    path_key(a) == path_key(b)
}

pub fn is_absolute(path: &str) -> bool {
    let path = units(path);
    match path.as_slice() {
        [] => false,
        [first, ..] if is_sep(*first) => true,
        [first, second, third, ..] => is_drive_letter(*first) && *second == COLON && is_sep(*third),
        _ => false,
    }
}

// The last name of a path, as a file's image name is read from its full path.
pub fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joined_parts_are_one_normal_path() {
        assert_eq!(
            join(&[r"D:\Games\Steam", "config", "loginusers.vdf"]),
            r"D:\Games\Steam\config\loginusers.vdf"
        );
        assert_eq!(join(&[r"C:\", "steam.exe"]), r"C:\steam.exe");
        assert_eq!(join(&["", ""]), ".");
        assert_eq!(join(&[r"\\server\share", "a"]), r"\\server\share\a");
        assert_eq!(join(&[r"D:\a", r"..\b"]), r"D:\b");
    }

    #[test]
    fn a_folder_resolves_inside_its_base_or_shows_that_it_left() {
        let common = r"D:\Lib\steamapps\common";
        assert_eq!(
            resolve_from(common, "game").as_deref(),
            Some(r"D:\Lib\steamapps\common\game")
        );
        assert_eq!(
            resolve_from(common, r"..\..\x").as_deref(),
            Some(r"D:\Lib\x")
        );
        assert_eq!(
            resolve_from(common, r"E:\other").as_deref(),
            Some(r"E:\other")
        );
        assert_eq!(
            resolve_from(common, r"\rooted").as_deref(),
            Some(r"D:\rooted")
        );
        assert_eq!(
            resolve_from(common, "D:game").as_deref(),
            Some(r"D:\Lib\steamapps\common\game")
        );
        assert_eq!(resolve_from(common, "E:game"), None);
        assert_eq!(resolve_from("relative", "game"), None);
        assert_eq!(resolve_from(common, "").as_deref(), Some(common));
    }

    #[test]
    fn an_absolute_path_starts_at_a_root() {
        assert!(is_absolute(r"C:\x"));
        assert!(is_absolute(r"\\server\share"));
        assert!(is_absolute("/x"));
        assert!(!is_absolute("C:x"));
        assert!(!is_absolute("x"));
        assert!(!is_absolute(""));
    }

    #[test]
    fn a_file_name_is_what_follows_the_last_separator() {
        assert_eq!(file_name(r"D:\Games\Steam\steam.exe"), "steam.exe");
        assert_eq!(file_name("d:/games/steam.exe"), "steam.exe");
        assert_eq!(file_name("steam.exe"), "steam.exe");
    }
}
