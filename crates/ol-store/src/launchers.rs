use std::collections::{BTreeMap, BTreeSet};

use ol_model::{Account, Ciphertext, Game, InstalledGame, LaunchResult, LauncherId, RecentLaunch};
use rusqlite::{OptionalExtension, params};

use crate::read::{ACCOUNT, GAME, account, game};
use crate::{Store, StoreError};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GameSync {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    // False when the scan found the library exactly as the table had it.
    pub changed: bool,
}

pub struct NewAccount<'a> {
    pub id: &'a str,
    pub launcher: LauncherId,
    pub display_name: &'a str,
    pub login: &'a str,
    pub password: Option<&'a Ciphertext>,
    pub launcher_uid: Option<&'a str>,
    pub tags: &'a [String],
    pub notes: &'a str,
    pub now: &'a str,
}

struct Known {
    id: String,
    name: String,
    install_path: Option<String>,
    installed: bool,
    needs_update: bool,
    last_played_at: Option<String>,
}

impl Store {
    pub fn game(&self, id: &str) -> Result<Option<Game>, StoreError> {
        let found = self
            .conn
            .query_row(&format!("{GAME} WHERE g.id = ?1"), [id], game)
            .optional()?;
        Ok(found.flatten())
    }

    // Makes the launcher's installed games what a scan just found. A row keeps its id from scan to scan, so its launches and artwork stay with it; a game that is gone is marked as not installed, never deleted.
    pub fn sync_games(
        &self,
        launcher: LauncherId,
        games: &[InstalledGame],
        now: &str,
        mut new_id: impl FnMut() -> String,
    ) -> Result<GameSync, StoreError> {
        let transaction = self.conn.unchecked_transaction()?;
        let mut before: BTreeMap<String, Known> = BTreeMap::new();
        {
            let mut statement = transaction.prepare(
                "SELECT id, launcher_game_id, name, install_path, installed, needs_update, last_played_at
                 FROM games WHERE launcher = ?1",
            )?;
            let rows = statement.query_map([launcher.as_str()], |row| {
                Ok((
                    row.get::<_, String>(1)?,
                    Known {
                        id: row.get(0)?,
                        name: row.get(2)?,
                        install_path: row.get(3)?,
                        installed: row.get::<_, i64>(4)? == 1,
                        needs_update: row.get::<_, i64>(5)? == 1,
                        last_played_at: row.get(6)?,
                    },
                ))
            })?;
            for row in rows {
                let (launcher_game_id, known) = row?;
                before.insert(launcher_game_id, known);
            }
        }
        let mut sync = GameSync::default();
        {
            let mut upsert = transaction.prepare(
                "INSERT INTO games (id, launcher, launcher_game_id, name, install_path, installed, last_seen_at, needs_update, last_played_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?7, ?8)
                 ON CONFLICT (launcher, launcher_game_id) DO UPDATE
                   SET name = excluded.name, install_path = excluded.install_path, installed = 1, last_seen_at = excluded.last_seen_at,
                       needs_update = excluded.needs_update, last_played_at = excluded.last_played_at",
            )?;
            for found in games {
                let existing = before.get(&found.launcher_game_id);
                let id = existing.map_or_else(&mut new_id, |known| known.id.clone());
                upsert.execute(params![
                    id,
                    launcher.as_str(),
                    found.launcher_game_id,
                    found.name,
                    found.install_path,
                    now,
                    i64::from(found.needs_update),
                    found.last_played_at,
                ])?;
                match existing {
                    None => {
                        sync.added += 1;
                        sync.changed = true;
                    }
                    Some(known) => {
                        sync.updated += 1;
                        sync.changed |= !known.installed
                            || known.name != found.name
                            || known.install_path.as_deref() != Some(found.install_path.as_str())
                            || known.needs_update != found.needs_update
                            || known.last_played_at != found.last_played_at;
                    }
                }
            }
            let seen: BTreeSet<&str> = games
                .iter()
                .map(|found| found.launcher_game_id.as_str())
                .collect();
            let mut mark_missing = transaction
                .prepare("UPDATE games SET installed = 0 WHERE id = ?1 AND installed = 1")?;
            for (launcher_game_id, known) in &before {
                if !seen.contains(launcher_game_id.as_str()) {
                    sync.removed += mark_missing.execute([&known.id])?;
                }
            }
        }
        transaction.commit()?;
        sync.changed |= sync.removed > 0;
        Ok(sync)
    }

    pub fn account(&self, id: &str) -> Result<Option<Account>, StoreError> {
        self.one_account(&format!("{ACCOUNT} WHERE id = ?1"), params![id])
    }

    // By login within one launcher, whatever the case of its ASCII letters.
    pub fn account_by_login(
        &self,
        launcher: LauncherId,
        login: &str,
    ) -> Result<Option<Account>, StoreError> {
        self.one_account(
            &format!("{ACCOUNT} WHERE launcher = ?1 AND login = ?2 COLLATE NOCASE"),
            params![launcher.as_str(), login],
        )
    }

    pub fn account_by_launcher_uid(
        &self,
        launcher: LauncherId,
        uid: &str,
    ) -> Result<Option<Account>, StoreError> {
        self.one_account(
            &format!("{ACCOUNT} WHERE launcher = ?1 AND launcher_uid = ?2 COLLATE NOCASE"),
            params![launcher.as_str(), uid],
        )
    }

    fn one_account(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
    ) -> Result<Option<Account>, StoreError> {
        let found = self.conn.query_row(sql, params, account).optional()?;
        Ok(found.flatten())
    }

    pub fn accounts_of(&self, launcher: LauncherId) -> Result<Vec<Account>, StoreError> {
        let mut statement = self.conn.prepare(&format!(
            "{ACCOUNT} WHERE launcher = ?1 ORDER BY display_name COLLATE NOCASE"
        ))?;
        let rows = statement.query_map([launcher.as_str()], account)?;
        Ok(rows
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect())
    }

    pub fn insert_account(&self, new: &NewAccount<'_>) -> Result<(), StoreError> {
        let tags = serde_json::Value::from(new.tags.to_vec()).to_string();
        self.conn.execute(
            "INSERT INTO accounts (id, launcher, display_name, login, secret_enc, launcher_uid, tags, notes, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
            params![
                new.id,
                new.launcher.as_str(),
                new.display_name,
                new.login,
                new.password.map(Ciphertext::as_bytes),
                new.launcher_uid,
                tags,
                new.notes,
                new.now,
            ],
        )?;
        Ok(())
    }

    pub fn set_account_launcher_uid(
        &self,
        id: &str,
        launcher_uid: Option<&str>,
        now: &str,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE accounts SET launcher_uid = ?1, updated_at = ?2 WHERE id = ?3",
            params![launcher_uid, now, id],
        )?;
        Ok(())
    }

    pub fn touch_account(&self, id: &str, at: &str) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE accounts SET last_used_at = ?1 WHERE id = ?2",
            params![at, id],
        )?;
        Ok(())
    }

    pub fn record_launch(
        &self,
        id: &str,
        game_id: &str,
        account_id: &str,
        at: &str,
        result: LaunchResult,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO launch_history (id, game_id, account_id, launched_at, result) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, game_id, account_id, at, result.as_str()],
        )?;
        Ok(())
    }

    // Game and account pairs that were launched and still exist, the newest launch of each pair first.
    pub fn recent_launches(&self, limit: u32) -> Result<Vec<RecentLaunch>, StoreError> {
        let mut statement = self.conn.prepare(
            "SELECT h.game_id, g.name AS game_name, g.launcher, h.account_id, a.display_name AS account_name, MAX(h.launched_at) AS launched_at
             FROM launch_history h
             JOIN games g ON g.id = h.game_id AND g.installed = 1
             JOIN accounts a ON a.id = h.account_id
             WHERE h.result = 'ok'
             GROUP BY h.game_id, h.account_id
             ORDER BY launched_at DESC
             LIMIT ?1",
        )?;
        let rows = statement.query_map([limit], |row| {
            let Some(launcher) = LauncherId::parse(&row.get::<_, String>(2)?) else {
                return Ok(None);
            };
            Ok(Some(RecentLaunch {
                game_id: row.get(0)?,
                game_name: row.get(1)?,
                launcher,
                account_id: row.get(3)?,
                account_name: row.get(4)?,
                launched_at: row.get(5)?,
            }))
        })?;
        Ok(rows
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect())
    }
}
