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

    pub fn delete_secret(&self, key: SecretKey) -> Result<bool, StoreError> {
        Ok(self
            .conn
            .execute("DELETE FROM secrets WHERE key = ?1", [key.as_str()])?
            > 0)
    }

    // The Electron app's vault reset: every account keeps its row without its password, and every saved sign-in and every named secret is deleted, the probe among them. All of it or nothing.
    pub fn clear_secrets(&self, at: &str) -> Result<ClearedSecrets, StoreError> {
        let transaction = self.conn.unchecked_transaction()?;
        let passwords = transaction.execute(
            "UPDATE accounts SET secret_enc = NULL, updated_at = ?1 WHERE secret_enc IS NOT NULL",
            [at],
        )?;
        let sessions = transaction.execute("DELETE FROM launcher_sessions", [])?;
        let secrets = transaction.execute("DELETE FROM secrets", [])?;
        transaction.commit()?;
        Ok(ClearedSecrets {
            passwords,
            sessions,
            secrets,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClearedSecrets {
    pub passwords: usize,
    pub sessions: usize,
    pub secrets: usize,
}
