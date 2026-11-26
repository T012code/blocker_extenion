//! SQLite storage for downloads and settings.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::Result;
use crate::model::{Download, DownloadId};

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Self::init(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS downloads (
                 id   INTEGER PRIMARY KEY AUTOINCREMENT,
                 data TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS settings (
                 key   TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );",
        )?;
        Ok(Self { conn })
    }

    /// Reserves an id for a new download.
    pub fn next_id(&self) -> Result<DownloadId> {
        self.conn
            .execute("INSERT INTO downloads (data) VALUES ('{}')", [])?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn save(&self, d: &Download) -> Result<()> {
        let json = serde_json::to_string(d)?;
        self.conn.execute(
            "INSERT INTO downloads (id, data) VALUES (?1, ?2)
             ON CONFLICT(id) DO UPDATE SET data = excluded.data",
            params![d.id, json],
        )?;
        Ok(())
    }

    pub fn delete(&self, id: DownloadId) -> Result<()> {
        self.conn
            .execute("DELETE FROM downloads WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn load_all(&self) -> Result<Vec<Download>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, data FROM downloads ORDER BY id")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (id, data) = row?;
            match serde_json::from_str::<Download>(&data) {
                Ok(mut d) => {
                    d.id = id;
                    out.push(d);
                }
                // Reserved but never saved (crash right after next_id): drop it.
                Err(_) if data == "{}" => self.delete(id)?,
                Err(e) => tracing::warn!("skipping unreadable download {id}: {e}"),
            }
        }
        Ok(out)
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}
