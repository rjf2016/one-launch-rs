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

    // A vault reset: each sealed value `opens` says cannot be opened goes, an account keeping its row without its password; every other stays. All of it or nothing.
    pub fn clear_unopened(
        &self,
        at: &str,
        opens: impl Fn(Sealed<'_>, &Ciphertext) -> bool,
    ) -> Result<ClearedSecrets, StoreError> {
        let transaction = self.conn.unchecked_transaction()?;
        let rows = |sql: &str| -> Result<Vec<(String, Vec<u8>)>, StoreError> {
            let mut statement = transaction.prepare(sql)?;
            let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
            Ok(rows.collect::<Result<_, _>>()?)
        };
        let passwords = rows("SELECT id, secret_enc FROM accounts WHERE secret_enc IS NOT NULL")?;
        let sessions = rows("SELECT account_id, blob_enc FROM launcher_sessions")?;
        let secrets = rows("SELECT key, value_enc FROM secrets")?;
        let mut cleared = ClearedSecrets::default();
        for (id, sealed) in passwords {
            if opens(Sealed::Password, &Ciphertext::new(sealed)) {
                cleared.kept += 1;
            } else {
                transaction.execute(
                    "UPDATE accounts SET secret_enc = NULL, updated_at = ?1 WHERE id = ?2",
                    params![at, id],
                )?;
                cleared.passwords += 1;
            }
        }
        for (account_id, sealed) in sessions {
            if opens(Sealed::Session, &Ciphertext::new(sealed)) {
                cleared.kept += 1;
            } else {
                transaction.execute(
                    "DELETE FROM launcher_sessions WHERE account_id = ?1",
                    [account_id],
                )?;
                cleared.sessions += 1;
            }
        }
        for (key, sealed) in secrets {
            if opens(Sealed::Secret(&key), &Ciphertext::new(sealed)) {
                cleared.kept += 1;
            } else {
                transaction.execute("DELETE FROM secrets WHERE key = ?1", [&key])?;
                cleared.secrets += 1;
            }
        }
        transaction.commit()?;
        Ok(cleared)
    }
}

// Which sealed value a reset is looking at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sealed<'a> {
    Password,
    Session,
    Secret(&'a str),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClearedSecrets {
    pub passwords: usize,
    pub sessions: usize,
    pub secrets: usize,
    pub kept: usize,
}
