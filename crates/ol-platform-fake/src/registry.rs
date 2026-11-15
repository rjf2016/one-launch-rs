use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use ol_platform_api::{Hive, RegistryError, RegistryReader, RegistryWriter};

use crate::OpLog;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryValue {
    Text(String),
    Dword(u32),
    // The bytes as hex digits.
    Binary(String),
}

struct Entry {
    path: String,
    value: RegistryValue,
}

#[derive(Default)]
struct State {
    values: BTreeMap<String, Entry>,
    fail_next_write: Option<String>,
}

// A registry in memory. A value is named by its whole path, such as HKCU\Software\Valve\Steam\SteamPath, whatever its case; every write through the port is recorded.
pub struct FakeRegistry {
    state: Mutex<State>,
    log: OpLog,
}

impl FakeRegistry {
    pub fn new(log: OpLog) -> Self {
        Self {
            state: Mutex::new(State::default()),
            log,
        }
    }

    // Puts a value there as a launcher or a test would, without recording a write.
    pub fn seed(&self, path: &str, value: RegistryValue) {
        let value = match value {
            RegistryValue::Binary(hex) => RegistryValue::Binary(hex.to_uppercase()),
            other => other,
        };
        self.state().values.insert(
            path.to_lowercase(),
            Entry {
                path: path.to_owned(),
                value,
            },
        );
    }

    pub fn remove(&self, path: &str) {
        self.state().values.remove(&path.to_lowercase());
    }

    pub fn value(&self, path: &str) -> Option<RegistryValue> {
        self.state()
            .values
            .get(&path.to_lowercase())
            .map(|entry| entry.value.clone())
    }

    // The next write through the port fails with this message and changes nothing.
    pub fn fail_next_write(&self, message: &str) {
        self.state().fail_next_write = Some(message.to_owned());
    }

    // Every value, with its path as first written.
    pub fn dump(&self) -> Vec<(String, RegistryValue)> {
        self.state()
            .values
            .values()
            .map(|entry| (entry.path.clone(), entry.value.clone()))
            .collect()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write(&self, path: &str, value: Option<RegistryValue>) -> Result<(), RegistryError> {
        let mut state = self.state();
        if let Some(message) = state.fail_next_write.take() {
            return Err(RegistryError(message));
        }
        match &value {
            Some(RegistryValue::Text(text)) => self.log.record(
                "reg.set",
                [("path", path.into()), ("value", text.as_str().into())],
            ),
            Some(RegistryValue::Dword(number)) => self.log.record(
                "reg.set",
                [("path", path.into()), ("value", (*number).into())],
            ),
            Some(RegistryValue::Binary(_)) => self.log.record(
                "reg.set",
                [("path", path.into()), ("value", "binary".into())],
            ),
            None => self.log.record("reg.delete", [("path", path.into())]),
        }
        match value {
            Some(value) => {
                state.values.insert(
                    path.to_lowercase(),
                    Entry {
                        path: path.to_owned(),
                        value,
                    },
                );
            }
            None => {
                state.values.remove(&path.to_lowercase());
            }
        }
        Ok(())
    }
}

fn path(hive: Hive, key: &str, name: &str) -> String {
    format!("{}\\{key}\\{name}", hive.as_str())
}

impl RegistryReader for FakeRegistry {
    fn get_string(
        &self,
        hive: Hive,
        key: &str,
        name: &str,
    ) -> Result<Option<String>, RegistryError> {
        Ok(match self.value(&path(hive, key, name)) {
            Some(RegistryValue::Text(text)) => Some(text),
            _ => None,
        })
    }

    fn get_dword(&self, hive: Hive, key: &str, name: &str) -> Result<Option<u32>, RegistryError> {
        Ok(match self.value(&path(hive, key, name)) {
            Some(RegistryValue::Dword(number)) => Some(number),
            _ => None,
        })
    }
}

impl RegistryWriter for FakeRegistry {
    fn set_string(
        &self,
        hive: Hive,
        key: &str,
        name: &str,
        value: &str,
    ) -> Result<(), RegistryError> {
        self.write(
            &path(hive, key, name),
            Some(RegistryValue::Text(value.to_owned())),
        )
    }

    fn set_dword(
        &self,
        hive: Hive,
        key: &str,
        name: &str,
        value: u32,
    ) -> Result<(), RegistryError> {
        self.write(&path(hive, key, name), Some(RegistryValue::Dword(value)))
    }

    fn delete_value(&self, hive: Hive, key: &str, name: &str) -> Result<(), RegistryError> {
        self.write(&path(hive, key, name), None)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const KEY: &str = r"Software\Example\Launcher";

    #[test]
    fn a_value_reads_back_whatever_the_case_of_its_path_and_only_as_its_own_type() {
        let registry = FakeRegistry::new(OpLog::new());
        registry.seed(
            r"HKCU\Software\Example\Launcher\Name",
            RegistryValue::Text("a".to_owned()),
        );
        registry.seed(
            r"HKCU\Software\Example\Launcher\Count",
            RegistryValue::Dword(7),
        );
        assert_eq!(
            registry
                .get_string(Hive::Hkcu, &KEY.to_uppercase(), "name")
                .unwrap()
                .as_deref(),
            Some("a")
        );
        assert_eq!(
            registry.get_dword(Hive::Hkcu, KEY, "Count").unwrap(),
            Some(7)
        );
        assert_eq!(registry.get_dword(Hive::Hkcu, KEY, "Name").unwrap(), None);
        assert_eq!(registry.get_string(Hive::Hkcu, KEY, "Count").unwrap(), None);
        assert_eq!(registry.get_string(Hive::Hklm, KEY, "Name").unwrap(), None);
    }

    #[test]
    fn only_a_write_through_the_port_is_recorded() {
        let log = OpLog::new();
        let registry = FakeRegistry::new(log.clone());
        registry.seed(
            r"HKCU\Software\Example\Launcher\Name",
            RegistryValue::Text("a".to_owned()),
        );
        assert!(log.snapshot().is_empty());
        registry.set_string(Hive::Hkcu, KEY, "Name", "b").unwrap();
        registry.set_dword(Hive::Hkcu, KEY, "Count", 1).unwrap();
        registry.delete_value(Hive::Hkcu, KEY, "Name").unwrap();
        assert_eq!(
            log.snapshot(),
            [
                json!({ "k": "plat", "op": "reg.set", "path": r"HKCU\Software\Example\Launcher\Name", "value": "b" }),
                json!({ "k": "plat", "op": "reg.set", "path": r"HKCU\Software\Example\Launcher\Count", "value": 1 }),
                json!({ "k": "plat", "op": "reg.delete", "path": r"HKCU\Software\Example\Launcher\Name" }),
            ]
        );
        assert_eq!(
            registry.dump(),
            [(
                r"HKCU\Software\Example\Launcher\Count".to_owned(),
                RegistryValue::Dword(1)
            )]
        );
    }

    #[test]
    fn a_write_told_to_fail_changes_nothing_and_the_next_one_goes_through() {
        let log = OpLog::new();
        let registry = FakeRegistry::new(log.clone());
        registry.fail_next_write("the registry said no");
        let refused = registry.set_dword(Hive::Hkcu, KEY, "Count", 1).unwrap_err();
        assert_eq!(refused.to_string(), "the registry said no");
        assert!(registry.dump().is_empty());
        assert!(log.snapshot().is_empty());
        registry.set_dword(Hive::Hkcu, KEY, "Count", 1).unwrap();
        assert_eq!(
            registry.get_dword(Hive::Hkcu, KEY, "Count").unwrap(),
            Some(1)
        );
    }
}
