use ol_model::{Ciphertext, LauncherId, LauncherSession};
use rusqlite::{OptionalExtension, Row, params};

use crate::{Store, StoreError};

// Every read of a saved sign-in takes these columns: the encrypted sign-in itself is never one of them.
const SESSION: &str = "
  SELECT account_id, launcher, launcher_uid, fingerprint, captured_at, verified_at, stale_reason
  FROM launcher_sessions";

pub struct NewSession<'a> {
    pub account_id: &'a str,
    pub launcher: LauncherId,
    pub launcher_uid: Option<&'a str>,
    pub blob: &'a Ciphertext,
    pub fingerprint: &'a str,
    pub captured_at: &'a str,
}

fn session(row: &Row<'_>) -> rusqlite::Result<Option<LauncherSession>> {
    let Some(launcher) = LauncherId::parse(&row.get::<_, String>(1)?) else {
        return Ok(None);
    };
    Ok(Some(LauncherSession {
        account_id: row.get(0)?,
        launcher,
        launcher_uid: row.get(2)?,
        fingerprint: row.get(3)?,
        captured_at: row.get(4)?,
        verified_at: row.get(5)?,
        stale_reason: row.get(6)?,
    }))
}

impl Store {
    pub fn session(&self, account_id: &str) -> Result<Option<LauncherSession>, StoreError> {
        let found = self
            .conn
            .query_row(
                &format!("{SESSION} WHERE account_id = ?1"),
                [account_id],
                session,
            )
            .optional()?;
        Ok(found.flatten())
    }

    // Every saved sign-in of one launcher, the newest first.
    pub fn sessions_of(&self, launcher: LauncherId) -> Result<Vec<LauncherSession>, StoreError> {
        let mut statement = self.conn.prepare(&format!(
            "{SESSION} WHERE launcher = ?1 ORDER BY captured_at DESC"
        ))?;
        let rows = statement.query_map([launcher.as_str()], session)?;
        Ok(rows
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect())
    }

    // The saved sign-in of this launcher that holds exactly this, the newest when two do.
    pub fn session_by_fingerprint(
        &self,
        launcher: LauncherId,
        fingerprint: &str,
    ) -> Result<Option<LauncherSession>, StoreError> {
        let found = self
            .conn
            .query_row(
                &format!(
                    "{SESSION} WHERE launcher = ?1 AND fingerprint = ?2 ORDER BY captured_at DESC LIMIT 1"
                ),
                params![launcher.as_str(), fingerprint],
                session,
            )
            .optional()?;
        Ok(found.flatten())
    }

    // One saved sign-in per account. A new one starts out neither confirmed nor refused, and one saved without the launcher's id keeps the id it had.
    pub fn save_session(&self, new: &NewSession<'_>) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO launcher_sessions (account_id, launcher, launcher_uid, blob_enc, fingerprint, captured_at, verified_at, stale_reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL)
             ON CONFLICT (account_id) DO UPDATE
               SET launcher = excluded.launcher, launcher_uid = COALESCE(excluded.launcher_uid, launcher_sessions.launcher_uid),
                   blob_enc = excluded.blob_enc, fingerprint = excluded.fingerprint, captured_at = excluded.captured_at,
                   verified_at = NULL, stale_reason = NULL",
            params![
                new.account_id,
                new.launcher.as_str(),
                new.launcher_uid,
                new.blob.as_bytes(),
                new.fingerprint,
                new.captured_at,
            ],
        )?;
        Ok(())
    }

    pub fn mark_session_verified(&self, account_id: &str, at: &str) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE launcher_sessions SET verified_at = ?1, stale_reason = NULL WHERE account_id = ?2",
            params![at, account_id],
        )?;
        Ok(())
    }

    pub fn mark_session_stale(&self, account_id: &str, reason: &str) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE launcher_sessions SET stale_reason = ?1 WHERE account_id = ?2",
            params![reason, account_id],
        )?;
        Ok(())
    }

    pub fn delete_session(&self, account_id: &str) -> Result<bool, StoreError> {
        Ok(self.conn.execute(
            "DELETE FROM launcher_sessions WHERE account_id = ?1",
            [account_id],
        )? > 0)
    }
}
