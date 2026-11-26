use rusqlite::{OptionalExtension, params};

use crate::{Store, StoreError};

// The artwork row of one game as it is stored: the source as text, and each path relative to the artwork folder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ArtworkRow {
    pub source: String,
    pub grid_path: Option<String>,
    pub hero_path: Option<String>,
    pub logo_path: Option<String>,
    pub fetched_at: Option<String>,
}

impl Store {
    pub fn artwork_row(&self, game_id: &str) -> Result<Option<ArtworkRow>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT source, grid_path, hero_path, logo_path, fetched_at FROM artwork WHERE game_id = ?1",
                [game_id],
                |row| {
                    Ok(ArtworkRow {
                        source: row.get(0)?,
                        grid_path: row.get(1)?,
                        hero_path: row.get(2)?,
                        logo_path: row.get(3)?,
                        fetched_at: row.get(4)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn put_artwork(&self, game_id: &str, row: &ArtworkRow) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO artwork (game_id, source, grid_path, hero_path, logo_path, fetched_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (game_id) DO UPDATE SET source = excluded.source, grid_path = excluded.grid_path, hero_path = excluded.hero_path,
               logo_path = excluded.logo_path, fetched_at = excluded.fetched_at",
            params![
                game_id,
                row.source,
                row.grid_path,
                row.hero_path,
                row.logo_path,
                row.fetched_at
            ],
        )?;
        Ok(())
    }

    // When the last lookup for a game found nothing.
    pub fn artwork_attempt(&self, game_id: &str) -> Result<Option<String>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT attempted_at FROM artwork_attempts WHERE game_id = ?1",
                [game_id],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn record_artwork_attempt(
        &self,
        game_id: &str,
        attempted_at: &str,
        error: &str,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO artwork_attempts (game_id, attempted_at, error) VALUES (?1, ?2, ?3)
             ON CONFLICT (game_id) DO UPDATE SET attempted_at = excluded.attempted_at, error = excluded.error",
            params![game_id, attempted_at, error],
        )?;
        Ok(())
    }

    pub fn clear_artwork_attempt(&self, game_id: &str) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM artwork_attempts WHERE game_id = ?1", [game_id])?;
        Ok(())
    }

    pub fn delete_artwork(&self, game_id: &str) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM artwork WHERE game_id = ?1", [game_id])?;
        Ok(())
    }

    // Every game that has an artwork row or a recorded miss.
    pub fn artwork_game_ids(&self) -> Result<Vec<String>, StoreError> {
        let mut statement = self
            .conn
            .prepare("SELECT game_id FROM artwork UNION SELECT game_id FROM artwork_attempts")?;
        let rows = statement.query_map([], |row| row.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}
