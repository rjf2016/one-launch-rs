// The real registry port, on a scratch key under HKCU\Software\OneLaunchRsTest that each test makes and deletes, whatever happens. No launcher's key is read or written.

use ol_platform_api::{Hive, RegistryReader, RegistryWriter, contract};
use ol_platform_win::registry::WindowsRegistry;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, KEY_WOW64_64KEY, REG_MULTI_SZ, REG_OPTION_NON_VOLATILE,
    REG_QWORD, REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteKeyExW, RegDeleteTreeW,
    RegSetValueExW,
};
use windows::core::PCWSTR;

const PARENT: &str = r"Software\OneLaunchRsTest";

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

struct ScratchKey(String);

impl ScratchKey {
    fn new() -> Self {
        Self(format!(r"{PARENT}\{}", uuid::Uuid::new_v4().simple()))
    }

    // Planted around the port, for kinds of value the port cannot write.
    fn plant(
        &self,
        name: &str,
        kind: windows::Win32::System::Registry::REG_VALUE_TYPE,
        bytes: &[u8],
    ) {
        self.plant_named(&wide(name), kind, bytes);
    }

    // `name` ends with its NUL.
    fn plant_named(
        &self,
        name: &[u16],
        kind: windows::Win32::System::Registry::REG_VALUE_TYPE,
        bytes: &[u8],
    ) {
        let key = wide(&self.0);
        let mut made = HKEY::default();
        unsafe {
            assert!(
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    PCWSTR(key.as_ptr()),
                    None,
                    PCWSTR::null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_SET_VALUE | KEY_WOW64_64KEY,
                    None,
                    &raw mut made,
                    None,
                )
                .is_ok()
            );
            let set = RegSetValueExW(made, PCWSTR(name.as_ptr()), None, kind, Some(bytes));
            let _ = RegCloseKey(made);
            assert!(set.is_ok());
        }
    }
}

impl Drop for ScratchKey {
    fn drop(&mut self) {
        let key = wide(&self.0);
        let parent = wide(PARENT);
        unsafe {
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()));
            let _ = RegDeleteKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(key.as_ptr()),
                KEY_WOW64_64KEY.0,
                None,
            );
            // Fails while another test still has a key under it, which then removes it.
            let _ = RegDeleteKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(parent.as_ptr()),
                KEY_WOW64_64KEY.0,
                None,
            );
        }
    }
}

#[test]
fn the_registry_port_contract_holds() {
    let scratch = ScratchKey::new();
    contract::registry(&WindowsRegistry, &scratch.0);
}

#[test]
fn a_kind_the_ports_do_not_know_is_passed_over() {
    let scratch = ScratchKey::new();
    let multi: Vec<u8> = "a\0b\0\0"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    scratch.plant("Multi", REG_MULTI_SZ, &multi);
    scratch.plant("Wide", REG_QWORD, &7_u64.to_le_bytes());
    WindowsRegistry
        .set_string(Hive::Hkcu, &scratch.0, "Known", "x")
        .unwrap();
    let listed = WindowsRegistry.list_values(Hive::Hkcu, &scratch.0).unwrap();
    assert!(listed.len() == 1 && listed[0].name == "Known");
    assert_eq!(
        WindowsRegistry
            .get_string(Hive::Hkcu, &scratch.0, "Multi")
            .unwrap(),
        None
    );
    assert_eq!(
        WindowsRegistry
            .get_dword(Hive::Hkcu, &scratch.0, "Wide")
            .unwrap(),
        None
    );
}

#[test]
fn an_error_never_repeats_what_was_to_be_written() {
    let scratch = ScratchKey::new();
    let refused = WindowsRegistry
        .set_binary(Hive::Hkcu, &scratch.0, "Token", "0a0bplaceholder")
        .unwrap_err();
    assert!(!format!("{refused} {refused:?}").contains("placeholder"));
    let refused = WindowsRegistry
        .set_string(Hive::Hkcu, &scratch.0, "Name", "place\0holder")
        .unwrap_err();
    assert!(!format!("{refused} {refused:?}").contains("place"));
    assert!(
        WindowsRegistry
            .list_values(Hive::Hkcu, &scratch.0)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_value_is_never_listed_or_written_under_a_name_other_than_its_own() {
    let scratch = ScratchKey::new();
    let half_pair = [u16::from(b'A'), 0xD800, 0];
    scratch.plant_named(&half_pair, REG_SZ, &[0, 0]);
    scratch.plant("A", REG_SZ, &[u8::from(b'x'), 0, 0, 0]);
    let listed = WindowsRegistry.list_values(Hive::Hkcu, &scratch.0).unwrap();
    let names: Vec<&str> = listed.iter().map(|value| value.name.as_str()).collect();
    assert_eq!(names, ["A"]);

    for refused in [
        WindowsRegistry.set_string(Hive::Hkcu, &scratch.0, "A\0B", "y"),
        WindowsRegistry.set_dword(Hive::Hkcu, &scratch.0, "A\0B", 1),
        WindowsRegistry.delete_value(Hive::Hkcu, &scratch.0, "A\0B"),
        WindowsRegistry.set_string(Hive::Hkcu, &format!("{}\0x", scratch.0), "B", "y"),
    ] {
        assert!(refused.is_err());
    }
    assert_eq!(
        WindowsRegistry
            .get_string(Hive::Hkcu, &scratch.0, "A")
            .unwrap()
            .as_deref(),
        Some("x")
    );
}

#[test]
fn nothing_is_written_under_hklm() {
    let scratch = ScratchKey::new();
    for refused in [
        WindowsRegistry.set_string(Hive::Hklm, &scratch.0, "Name", "x"),
        WindowsRegistry.set_dword(Hive::Hklm, &scratch.0, "Count", 1),
        WindowsRegistry.set_binary(Hive::Hklm, &scratch.0, "Token", "00"),
        WindowsRegistry.delete_value(Hive::Hklm, &scratch.0, "Name"),
    ] {
        assert_eq!(
            refused.unwrap_err().to_string(),
            "One Launch writes nothing under HKLM"
        );
    }
}
