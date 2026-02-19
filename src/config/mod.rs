//! Key-value configuration storage backed by SQLite.
//!
//! Shares a database with [`AuthStorage`](crate::auth::AuthStorage) and
//! [`SqliteMemory`](crate::memory::sqlite::SqliteMemory) — pass the same
//! path to all three.

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::sync::Mutex;

/// Persistent key-value configuration store.
pub struct Config {
    conn: Mutex<Connection>,
}

impl Config {
    /// Open or create the config table in the given database.
    /// Use `":memory:"` for tests.
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path).context("failed to open config database")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS config (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
        )
        .context("failed to create config table")?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Get a config value by key.
    pub fn get(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT value FROM config WHERE key = ?1")?;
        let mut rows = stmt.query([key])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    /// Set a config value (upsert).
    pub fn set(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO config (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )?;
        Ok(())
    }

    /// Remove a config key.
    pub fn remove(&self, key: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM config WHERE key = ?1", [key])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_config() -> Config {
        Config::open(":memory:").unwrap()
    }

    #[test]
    fn get_returns_none_for_missing_key() {
        let config = mem_config();
        assert!(config.get("nonexistent").unwrap().is_none());
    }

    #[test]
    fn set_and_get() {
        let config = mem_config();
        config.set("model", "claude-sonnet-4-20250514").unwrap();
        assert_eq!(
            config.get("model").unwrap().unwrap(),
            "claude-sonnet-4-20250514"
        );
    }

    #[test]
    fn set_overwrites_existing() {
        let config = mem_config();
        config.set("model", "old").unwrap();
        config.set("model", "new").unwrap();
        assert_eq!(config.get("model").unwrap().unwrap(), "new");
    }

    #[test]
    fn remove_deletes_key() {
        let config = mem_config();
        config.set("model", "test").unwrap();
        config.remove("model").unwrap();
        assert!(config.get("model").unwrap().is_none());
    }

    #[test]
    fn remove_nonexistent_is_ok() {
        let config = mem_config();
        config.remove("nonexistent").unwrap();
    }

    #[test]
    fn multiple_keys_independent() {
        let config = mem_config();
        config.set("model", "sonnet").unwrap();
        config.set("theme", "dark").unwrap();

        assert_eq!(config.get("model").unwrap().unwrap(), "sonnet");
        assert_eq!(config.get("theme").unwrap().unwrap(), "dark");
    }

    #[test]
    fn persists_to_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config-test.db");
        let path_str = path.to_str().unwrap();

        {
            let config = Config::open(path_str).unwrap();
            config.set("model", "persisted").unwrap();
        }

        {
            let config = Config::open(path_str).unwrap();
            assert_eq!(config.get("model").unwrap().unwrap(), "persisted");
        }
    }
}
