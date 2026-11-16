// The registry as a 64-bit program sees it, which is what the Electron app's reg.exe read: a key the 32-bit view holds is named with WOW6432Node by the adapter that wants it.

use std::fmt::Write as _;

use ol_platform_api::{
    Hive, RegistryData, RegistryError, RegistryNamedValue, RegistryReader, RegistryWriter,
};
use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, WIN32_ERROR,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_ENUMERATE_SUB_KEYS, KEY_QUERY_VALUE,
    KEY_SET_VALUE, KEY_WOW64_64KEY, REG_BINARY, REG_DWORD, REG_EXPAND_SZ, REG_OPTION_NON_VOLATILE,
    REG_SAM_FLAGS, REG_SZ, REG_VALUE_TYPE, RegCloseKey, RegCreateKeyExW, RegDeleteValueW,
    RegEnumKeyExW, RegEnumValueW, RegOpenKeyExW, RegQueryInfoKeyW, RegQueryValueExW,
    RegSetValueExW,
};
use windows::core::{PCWSTR, PWSTR};
use zeroize::{Zeroize, Zeroizing};

use crate::wide;

// Deeper than the registry lets a key go.
const DEEPEST: usize = 512;
const LONGEST_NAME: usize = 16_384;
const LONGEST_KEY_NAME: usize = 256;
const ATTEMPTS: usize = 4;

#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsRegistry;

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

const fn root(hive: Hive) -> HKEY {
    match hive {
        Hive::Hkcu => HKEY_CURRENT_USER,
        Hive::Hklm => HKEY_LOCAL_MACHINE,
    }
}

fn failed(
    what: &str,
    hive: Hive,
    key: &str,
    name: Option<&str>,
    code: WIN32_ERROR,
) -> RegistryError {
    let mut path = format!("{}\\{key}", hive.as_str());
    if let Some(name) = name {
        let _ = write!(path, "\\{name}");
    }
    RegistryError(format!(
        "Windows could not {what} {path} (error {})",
        code.0
    ))
}

fn open_below(parent: HKEY, key: &str, access: REG_SAM_FLAGS) -> Result<Option<Key>, WIN32_ERROR> {
    let name = wide(key);
    let mut opened = HKEY::default();
    let code = unsafe {
        RegOpenKeyExW(
            parent,
            PCWSTR(name.as_ptr()),
            Some(0),
            access | KEY_WOW64_64KEY,
            &raw mut opened,
        )
    };
    match code {
        code if code.is_ok() => Ok(Some(Key(opened))),
        ERROR_FILE_NOT_FOUND => Ok(None),
        code => Err(code),
    }
}

// Windows reads a name only up to a NUL, so one that holds a NUL would name another key or value.
fn whole(names: &[&str]) -> Result<(), RegistryError> {
    if names.iter().any(|name| name.contains('\0')) {
        return Err(RegistryError(
            "One Launch was asked for a registry key or value by a name that holds a NUL"
                .to_owned(),
        ));
    }
    Ok(())
}

fn open(hive: Hive, key: &str, access: REG_SAM_FLAGS) -> Result<Option<Key>, RegistryError> {
    whole(&[key])?;
    open_below(root(hive), key, access).map_err(|code| failed("open", hive, key, None, code))
}

fn text(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .take_while(|unit| *unit != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

fn hex(bytes: &[u8]) -> String {
    let mut digits = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(digits, "{byte:02X}");
    }
    digits
}

// The kinds the ports know. Any other kind of value is passed over, as the fake registry cannot hold one.
fn data(kind: REG_VALUE_TYPE, bytes: &[u8]) -> Option<RegistryData> {
    match kind {
        REG_SZ | REG_EXPAND_SZ => Some(RegistryData::Text(text(bytes))),
        REG_DWORD => Some(RegistryData::Dword(u32::from_le_bytes(
            bytes.get(..4)?.try_into().ok()?,
        ))),
        REG_BINARY => Some(RegistryData::Binary(hex(bytes))),
        _ => None,
    }
}

// A value's bytes can be a launcher's sign-in: every buffer that held them is wiped when it goes.
fn query(
    key: &Key,
    name: &str,
) -> Result<Option<(REG_VALUE_TYPE, Zeroizing<Vec<u8>>)>, WIN32_ERROR> {
    let name = wide(name);
    let mut bytes = Zeroizing::new(vec![0_u8; 512]);
    for _ in 0..ATTEMPTS {
        let mut kind = REG_VALUE_TYPE::default();
        let mut length = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
        let code = unsafe {
            RegQueryValueExW(
                key.0,
                PCWSTR(name.as_ptr()),
                None,
                Some(&raw mut kind),
                Some(bytes.as_mut_ptr()),
                Some(&raw mut length),
            )
        };
        match code {
            code if code.is_ok() => {
                bytes.truncate(length as usize);
                return Ok(Some((kind, bytes)));
            }
            ERROR_MORE_DATA => bytes = Zeroizing::new(vec![0_u8; length as usize]),
            ERROR_FILE_NOT_FOUND => return Ok(None),
            code => return Err(code),
        }
    }
    Err(ERROR_MORE_DATA)
}

// Only the kind asked for is made into data: the digits of a binary value read for a text would be dropped unwiped.
fn read(
    hive: Hive,
    key: &str,
    name: &str,
    wanted: &[REG_VALUE_TYPE],
) -> Result<Option<RegistryData>, RegistryError> {
    whole(&[name])?;
    let Some(opened) = open(hive, key, KEY_QUERY_VALUE)? else {
        return Ok(None);
    };
    let found = query(&opened, name).map_err(|code| failed("read", hive, key, Some(name), code))?;
    Ok(found
        .filter(|(kind, _)| wanted.contains(kind))
        .and_then(|(kind, bytes)| data(kind, &bytes)))
}

// The longest value name and the largest value under a key, each with room for its NUL.
fn room(key: &Key) -> Result<(usize, usize), WIN32_ERROR> {
    let (mut name, mut value) = (0_u32, 0_u32);
    let code = unsafe {
        RegQueryInfoKeyW(
            key.0,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&raw mut name),
            Some(&raw mut value),
            None,
            None,
        )
    };
    if code.is_err() {
        return Err(code);
    }
    Ok(((name as usize + 1).min(LONGEST_NAME), value as usize + 2))
}

fn wipe(found: &mut [RegistryNamedValue]) {
    for value in found {
        if let RegistryData::Binary(digits) = &mut value.data {
            digits.zeroize();
        }
    }
}

// The values under a key; a binary one only when `binary` is asked for.
fn values(key: &Key, binary: bool) -> Result<Vec<RegistryNamedValue>, WIN32_ERROR> {
    let mut found = Vec::new();
    let listed = list(key, binary, &mut found);
    if let Err(code) = listed {
        wipe(&mut found);
        return Err(code);
    }
    found.sort_by_cached_key(|value| value.name.to_lowercase());
    Ok(found)
}

fn list(key: &Key, binary: bool, found: &mut Vec<RegistryNamedValue>) -> Result<(), WIN32_ERROR> {
    let (mut longest_name, mut largest) = room(key)?;
    let mut index = 0;
    let mut tries = 0;
    loop {
        let mut name = vec![0_u16; longest_name];
        let mut bytes = Zeroizing::new(vec![0_u8; largest]);
        let mut name_length = u32::try_from(name.len()).unwrap_or(u32::MAX);
        let mut length = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
        let mut kind = 0_u32;
        let code = unsafe {
            RegEnumValueW(
                key.0,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &raw mut name_length,
                None,
                Some(&raw mut kind),
                Some(bytes.as_mut_ptr()),
                Some(&raw mut length),
            )
        };
        match code {
            ERROR_NO_MORE_ITEMS => break,
            // A value was made or grew while the key was read.
            ERROR_MORE_DATA if tries < ATTEMPTS => {
                tries += 1;
                (longest_name, largest) = room(key)?;
                largest = largest.max(length as usize);
                continue;
            }
            code if code.is_err() => return Err(code),
            _ => {}
        }
        tries = 0;
        index += 1;
        // A name that does not read back as itself would have a later set or delete act on another value.
        let Ok(name) = String::from_utf16(&name[..(name_length as usize).min(name.len())]) else {
            continue;
        };
        if name.contains('\0') {
            continue;
        }
        bytes.truncate(length as usize);
        let kind = REG_VALUE_TYPE(kind);
        if kind == REG_BINARY && !binary {
            continue;
        }
        if let Some(data) = data(kind, &bytes) {
            found.push(RegistryNamedValue { name, data });
        }
    }
    Ok(())
}

fn subkeys(key: &Key) -> Result<Vec<String>, WIN32_ERROR> {
    let mut found = Vec::new();
    let mut index = 0;
    loop {
        let mut name = vec![0_u16; LONGEST_KEY_NAME];
        let mut length = u32::try_from(name.len()).unwrap_or(u32::MAX);
        let code = unsafe {
            RegEnumKeyExW(
                key.0,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &raw mut length,
                None,
                None,
                None,
                None,
            )
        };
        match code {
            ERROR_NO_MORE_ITEMS => break,
            code if code.is_err() => return Err(code),
            _ => {}
        }
        index += 1;
        if let Ok(name) = String::from_utf16(&name[..(length as usize).min(name.len())])
            && !name.contains('\0')
        {
            found.push(name);
        }
    }
    Ok(found)
}

// One key at a time, each read for its own values and closed before the next: nothing but the matches is kept. A key that cannot be read is passed over.
fn search(
    key: &Key,
    names: Vec<String>,
    below: &str,
    wanted: &str,
    depth: usize,
    found: &mut Vec<String>,
) {
    if depth > DEEPEST {
        return;
    }
    for name in names {
        let Ok(Some(subkey)) = open_below(key.0, &name, KEY_QUERY_VALUE | KEY_ENUMERATE_SUB_KEYS)
        else {
            continue;
        };
        let path = if below.is_empty() {
            name
        } else {
            format!("{below}\\{name}")
        };
        let holds = values(&subkey, false).is_ok_and(|values| {
            values.iter().any(|value| match &value.data {
                RegistryData::Text(text) => text.to_lowercase().contains(wanted),
                _ => false,
            })
        });
        if holds {
            found.push(path.clone());
        }
        if let Ok(names) = subkeys(&subkey) {
            search(&subkey, names, &path, wanted, depth + 1, found);
        }
    }
}

const fn nibble(digit: u8) -> u8 {
    match digit {
        b'0'..=b'9' => digit - b'0',
        b'a'..=b'f' => digit - b'a' + 10,
        _ => digit.wrapping_sub(b'A').wrapping_add(10),
    }
}

// In one buffer of the right size from the start, with no way out half-filled: a buffer that grew or was dropped there would leave a copy of the token behind, unwiped. The caller wipes the one it gets.
fn hex_bytes(hex: &str) -> Option<Vec<u8>> {
    let digits = hex.as_bytes();
    if !digits.len().is_multiple_of(2) || !digits.iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    let mut bytes = Vec::with_capacity(digits.len() / 2);
    for [high, low] in digits.as_chunks::<2>().0 {
        bytes.push((nibble(*high) << 4) | nibble(*low));
    }
    Some(bytes)
}

impl RegistryReader for WindowsRegistry {
    fn get_string(
        &self,
        hive: Hive,
        key: &str,
        name: &str,
    ) -> Result<Option<String>, RegistryError> {
        Ok(match read(hive, key, name, &[REG_SZ, REG_EXPAND_SZ])? {
            Some(RegistryData::Text(text)) => Some(text),
            _ => None,
        })
    }

    fn get_dword(&self, hive: Hive, key: &str, name: &str) -> Result<Option<u32>, RegistryError> {
        Ok(match read(hive, key, name, &[REG_DWORD])? {
            Some(RegistryData::Dword(number)) => Some(number),
            _ => None,
        })
    }

    fn list_values(&self, hive: Hive, key: &str) -> Result<Vec<RegistryNamedValue>, RegistryError> {
        let Some(opened) = open(hive, key, KEY_QUERY_VALUE)? else {
            return Ok(Vec::new());
        };
        values(&opened, true).map_err(|code| failed("list", hive, key, None, code))
    }

    fn keys_holding(
        &self,
        hive: Hive,
        key: &str,
        text: &str,
    ) -> Result<Vec<String>, RegistryError> {
        let Some(opened) = open(hive, key, KEY_QUERY_VALUE | KEY_ENUMERATE_SUB_KEYS)? else {
            return Ok(Vec::new());
        };
        let names = subkeys(&opened).map_err(|code| failed("list", hive, key, None, code))?;
        let mut found = Vec::new();
        search(&opened, names, "", &text.to_lowercase(), 0, &mut found);
        found.sort_by_cached_key(|path| format!("{}\\", path.to_lowercase()));
        Ok(found)
    }
}

impl WindowsRegistry {
    fn write(
        hive: Hive,
        key: &str,
        name: &str,
        kind: REG_VALUE_TYPE,
        bytes: &[u8],
    ) -> Result<(), RegistryError> {
        if hive != Hive::Hkcu {
            return Err(RegistryError(format!(
                "One Launch writes nothing under {}",
                hive.as_str()
            )));
        }
        whole(&[key, name])?;
        let wide_key = wide(key);
        let mut created = HKEY::default();
        let code = unsafe {
            RegCreateKeyExW(
                root(hive),
                PCWSTR(wide_key.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE | KEY_WOW64_64KEY,
                None,
                &raw mut created,
                None,
            )
        };
        if code.is_err() {
            return Err(failed("open", hive, key, None, code));
        }
        let created = Key(created);
        let wide_name = wide(name);
        let code = unsafe {
            RegSetValueExW(
                created.0,
                PCWSTR(wide_name.as_ptr()),
                None,
                kind,
                Some(bytes),
            )
        };
        if code.is_err() {
            return Err(failed("write", hive, key, Some(name), code));
        }
        Ok(())
    }
}

impl RegistryWriter for WindowsRegistry {
    fn set_string(
        &self,
        hive: Hive,
        key: &str,
        name: &str,
        value: &str,
    ) -> Result<(), RegistryError> {
        if value.contains('\0') {
            return Err(RegistryError(format!(
                "{name} cannot hold a text with a NUL in it"
            )));
        }
        let bytes: Vec<u8> = value
            .encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        Self::write(hive, key, name, REG_SZ, &bytes)
    }

    fn set_dword(
        &self,
        hive: Hive,
        key: &str,
        name: &str,
        value: u32,
    ) -> Result<(), RegistryError> {
        Self::write(hive, key, name, REG_DWORD, &value.to_le_bytes())
    }

    fn set_binary(
        &self,
        hive: Hive,
        key: &str,
        name: &str,
        hex: &str,
    ) -> Result<(), RegistryError> {
        let bytes = Zeroizing::new(hex_bytes(hex).ok_or_else(|| {
            RegistryError(format!("{name} was not given its bytes as hex digits"))
        })?);
        Self::write(hive, key, name, REG_BINARY, &bytes)
    }

    fn delete_value(&self, hive: Hive, key: &str, name: &str) -> Result<(), RegistryError> {
        if hive != Hive::Hkcu {
            return Err(RegistryError(format!(
                "One Launch writes nothing under {}",
                hive.as_str()
            )));
        }
        whole(&[name])?;
        let Some(opened) = open(hive, key, KEY_SET_VALUE)? else {
            return Ok(());
        };
        let wide_name = wide(name);
        match unsafe { RegDeleteValueW(opened.0, PCWSTR(wide_name.as_ptr())) } {
            code if code.is_ok() => Ok(()),
            ERROR_FILE_NOT_FOUND => Ok(()),
            code => Err(failed("delete", hive, key, Some(name), code)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_digits_become_bytes_and_anything_else_is_refused() {
        assert_eq!(hex_bytes("0a0BfF").unwrap(), [0x0a, 0x0b, 0xff]);
        assert_eq!(hex_bytes("").unwrap(), Vec::<u8>::new());
        for refused in ["0", "0g", "+1", "é0", " 0a", "0x0a"] {
            assert!(hex_bytes(refused).is_none(), "{refused}");
        }
        assert_eq!(hex(&[0x0a, 0x0b, 0xff]), "0A0BFF");
    }

    #[test]
    fn a_tokens_bytes_and_digits_are_made_in_one_buffer_of_their_own_size() {
        let digits = "0a0bff0c0d0e0f101112";
        let bytes = hex_bytes(digits).unwrap();
        assert_eq!(bytes.capacity(), digits.len() / 2);
        assert_eq!(hex(&bytes).capacity(), digits.len());
    }

    #[test]
    fn a_text_ends_at_its_first_nul_and_an_unknown_kind_is_passed_over() {
        let bytes: Vec<u8> = "ab\0cd\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert!(data(REG_SZ, &bytes) == Some(RegistryData::Text("ab".to_owned())));
        assert!(data(REG_EXPAND_SZ, b"%\0") == Some(RegistryData::Text("%".to_owned())));
        assert!(data(REG_DWORD, &[1, 0, 0, 0]) == Some(RegistryData::Dword(1)));
        assert!(data(REG_DWORD, &[1, 0]).is_none());
        assert!(data(REG_VALUE_TYPE(7), &bytes).is_none());
        assert!(data(REG_VALUE_TYPE(11), &[0; 8]).is_none());
    }
}
