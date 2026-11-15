use ol_model::{
    Account, ArtworkSource, Ciphertext, Game, GameArtwork, LastLaunch, LauncherId, SecretKey,
    SettingKey, Settings,
};
use rusqlite::{OptionalExtension, Row};

use crate::{Store, StoreError};

const ACCOUNTS: &str = "
  SELECT id, launcher, display_name, login,
         CASE WHEN secret_enc IS NULL THEN 0 ELSE 1 END AS has_secret,
         EXISTS (SELECT 1 FROM launcher_sessions s WHERE s.account_id = accounts.id) AS has_session,
         launcher_uid, tags, notes, last_used_at, created_at, updated_at
  FROM accounts
  ORDER BY launcher, display_name COLLATE NOCASE";

// The account that goes with MAX(launched_at) is the one SQLite takes from the same row: the Electron app reads it this way, ties included.
const INSTALLED_GAMES: &str = "
  SELECT g.id, g.launcher, g.launcher_game_id, g.name, g.install_path, g.installed, g.last_seen_at,
         g.needs_update, g.last_played_at,
         a.source AS art_source, a.grid_path, a.hero_path, a.logo_path, a.fetched_at,
         att.error AS artwork_error,
         ll.account_id AS ll_account_id, acc.display_name AS ll_account_name, ll.launched_at AS ll_launched_at
  FROM games g
  LEFT JOIN artwork a ON a.game_id = g.id
  LEFT JOIN artwork_attempts att ON att.game_id = g.id
  LEFT JOIN (
    SELECT game_id, account_id, MAX(launched_at) AS launched_at
    FROM launch_history WHERE result = 'ok' GROUP BY game_id
  ) ll ON ll.game_id = g.id
  LEFT JOIN accounts acc ON acc.id = ll.account_id
  WHERE g.installed = 1
  ORDER BY g.name COLLATE NOCASE";

const ROW_COUNTS: &str = "
  SELECT (SELECT COUNT(*) FROM accounts),
         (SELECT COUNT(*) FROM games),
         (SELECT COUNT(*) FROM artwork),
         (SELECT COUNT(*) FROM artwork_attempts),
         (SELECT COUNT(*) FROM launch_history),
         (SELECT COUNT(*) FROM launcher_sessions),
         (SELECT COUNT(*) FROM settings),
         (SELECT COUNT(*) FROM secrets),
         (SELECT COUNT(*) FROM games WHERE installed = 1)";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RowCounts {
    pub accounts: u64,
    pub games: u64,
    pub artwork: u64,
    pub artwork_attempts: u64,
    pub launch_history: u64,
    pub launcher_sessions: u64,
    pub settings: u64,
    pub secrets: u64,
    pub games_installed: u64,
}

#[derive(Debug, Default)]
pub struct Ciphertexts {
    pub passwords: Vec<Ciphertext>,
    pub sessions: Vec<Ciphertext>,
    pub secrets: Vec<(String, Ciphertext)>,
}

impl Store {
    pub fn accounts(&self) -> Result<Vec<Account>, StoreError> {
        let mut statement = self.conn.prepare(ACCOUNTS)?;
        let rows = statement.query_map([], account)?;
        Ok(rows
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect())
    }

    pub fn installed_games(&self) -> Result<Vec<Game>, StoreError> {
        let mut statement = self.conn.prepare(INSTALLED_GAMES)?;
        let rows = statement.query_map([], game)?;
        Ok(rows
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect())
    }

    pub fn settings(&self) -> Result<Settings, StoreError> {
        let mut statement = self.conn.prepare("SELECT key, value FROM settings")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut settings = Settings::default();
        for row in rows {
            let (key, value) = row?;
            if let Some(key) = SettingKey::parse(&key) {
                settings.insert(key, value);
            }
        }
        Ok(settings)
    }

    pub fn setting(&self, key: SettingKey) -> Result<Option<String>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                [key.as_str()],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn secret(&self, key: SecretKey) -> Result<Option<Ciphertext>, StoreError> {
        self.ciphertext("SELECT value_enc FROM secrets WHERE key = ?1", key.as_str())
    }

    pub fn account_password(&self, account_id: &str) -> Result<Option<Ciphertext>, StoreError> {
        self.ciphertext("SELECT secret_enc FROM accounts WHERE id = ?1", account_id)
    }

    pub fn session_blob(&self, account_id: &str) -> Result<Option<Ciphertext>, StoreError> {
        self.ciphertext(
            "SELECT blob_enc FROM launcher_sessions WHERE account_id = ?1",
            account_id,
        )
    }

    pub fn ciphertexts(&self) -> Result<Ciphertexts, StoreError> {
        let column = |sql: &str| -> Result<Vec<Ciphertext>, StoreError> {
            let mut statement = self.conn.prepare(sql)?;
            let rows = statement.query_map([], |row| row.get(0).map(Ciphertext::new))?;
            Ok(rows.collect::<Result<_, _>>()?)
        };
        let mut statement = self.conn.prepare("SELECT key, value_enc FROM secrets")?;
        let secrets = statement
            .query_map([], |row| Ok((row.get(0)?, Ciphertext::new(row.get(1)?))))?
            .collect::<Result<_, _>>()?;
        Ok(Ciphertexts {
            passwords: column("SELECT secret_enc FROM accounts WHERE secret_enc IS NOT NULL")?,
            sessions: column("SELECT blob_enc FROM launcher_sessions")?,
            secrets,
        })
    }

    pub fn row_counts(&self) -> Result<RowCounts, StoreError> {
        Ok(self.conn.query_row(ROW_COUNTS, [], |row| {
            let count = |index: usize| -> rusqlite::Result<u64> {
                Ok(u64::try_from(row.get::<_, i64>(index)?).unwrap_or_default())
            };
            Ok(RowCounts {
                accounts: count(0)?,
                games: count(1)?,
                artwork: count(2)?,
                artwork_attempts: count(3)?,
                launch_history: count(4)?,
                launcher_sessions: count(5)?,
                settings: count(6)?,
                secrets: count(7)?,
                games_installed: count(8)?,
            })
        })?)
    }

    fn ciphertext(&self, sql: &str, key: &str) -> Result<Option<Ciphertext>, StoreError> {
        let found: Option<Option<Vec<u8>>> = self
            .conn
            .query_row(sql, [key], |row| row.get(0))
            .optional()?;
        Ok(found.flatten().map(Ciphertext::new))
    }
}

fn account(row: &Row<'_>) -> rusqlite::Result<Option<Account>> {
    let Some(launcher) = LauncherId::parse(&row.get::<_, String>(1)?) else {
        return Ok(None);
    };
    Ok(Some(Account {
        id: row.get(0)?,
        launcher,
        display_name: row.get(2)?,
        login: row.get(3)?,
        has_secret: row.get::<_, i64>(4)? == 1,
        has_session: row.get::<_, i64>(5)? == 1,
        launcher_uid: row.get(6)?,
        tags: tags(&row.get::<_, String>(7)?),
        notes: row.get(8)?,
        last_used_at: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    }))
}

fn tags(stored: &str) -> Vec<String> {
    match serde_json::from_str(stored) {
        Ok(serde_json::Value::Array(items)) => items
            .into_iter()
            .filter_map(|item| match item {
                serde_json::Value::String(tag) => Some(tag),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn game(row: &Row<'_>) -> rusqlite::Result<Option<Game>> {
    let Some(launcher) = LauncherId::parse(&row.get::<_, String>(1)?) else {
        return Ok(None);
    };
    let filled = |index: usize| -> rusqlite::Result<Option<String>> {
        Ok(row
            .get::<_, Option<String>>(index)?
            .filter(|text| !text.is_empty()))
    };
    let source = row
        .get::<_, Option<String>>(9)?
        .and_then(|source| ArtworkSource::parse(&source));
    let artwork = match source {
        Some(source) => Some(GameArtwork {
            source,
            grid: filled(10)?,
            hero: filled(11)?,
            logo: filled(12)?,
            fetched_at: row.get(13)?,
        }),
        None => None,
    };
    let last_launch = match (filled(15)?, filled(17)?) {
        (Some(account_id), Some(launched_at)) => Some(LastLaunch {
            account_id,
            account_name: row.get(16)?,
            launched_at,
        }),
        _ => None,
    };
    Ok(Some(Game {
        id: row.get(0)?,
        launcher,
        launcher_game_id: row.get(2)?,
        name: row.get(3)?,
        install_path: row.get(4)?,
        installed: row.get::<_, i64>(5)? == 1,
        last_seen_at: row.get(6)?,
        needs_update: row.get::<_, i64>(7)? == 1,
        last_played_at: row.get(8)?,
        artwork,
        artwork_error: row.get(14)?,
        last_launch,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_that_are_not_a_list_of_text_read_as_none() {
        assert_eq!(tags(r#"["main","alt"]"#), ["main", "alt"]);
        assert_eq!(tags(r#"["main",3,null,"alt"]"#), ["main", "alt"]);
        for stored in ["", "{", r#"{"a":1}"#, r#""main""#, "null"] {
            assert_eq!(tags(stored), Vec::<String>::new(), "{stored}");
        }
    }
}
