use ol_model::{Ciphertext, SecretKey, SettingKey};
use rusqlite::params;

use crate::{Store, StoreError};

impl Store {
    pub fn set_setting(&self, key: SettingKey, value: &str) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key.as_str(), value],
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
