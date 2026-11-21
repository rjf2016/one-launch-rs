use ol_model::{Ciphertext, LibrarySort, SecretKey, SettingKey, Sidebar};
use rusqlite::params;

use crate::{Store, StoreError};

impl Store {
    pub fn set_setting(&self, key: SettingKey, value: &str) -> Result<(), StoreError> {
        self.store_value(key.as_str(), value)
    }

    pub fn set_library_sort(&self, sort: LibrarySort) -> Result<(), StoreError> {
        self.store_value(LibrarySort::KEY, sort.as_str())
    }

    pub fn set_sidebar(&self, sidebar: Sidebar) -> Result<(), StoreError> {
        self.store_value(Sidebar::KEY, sidebar.as_str())
    }

    fn store_value(&self, key: &str, value: &str) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn delete_setting(&self, key: SettingKey) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM settings WHERE key = ?1", [key.as_str()])?;
        Ok(())
    }

    pub fn put_secret(
        &self,
        key: SecretKey,
        value: &Ciphertext,
        updated_at: &str,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO secrets (key, value_enc, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value_enc = excluded.value_enc, updated_at = excluded.updated_at",
            params![key.as_str(), value.as_bytes(), updated_at],
        )?;
        Ok(())
    }
}
