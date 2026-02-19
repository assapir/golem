use anyhow::Result;
use async_trait::async_trait;
use rusqlite::Connection;
use std::sync::Mutex;

use super::{Memory, MemoryEntry, SessionEntry};

/// SQLite-backed persistent memory.
pub struct SqliteMemory {
    conn: Mutex<Connection>,
}

impl SqliteMemory {
    pub fn new(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memory (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL DEFAULT (datetime('now')),
                entry TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS session_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL DEFAULT (datetime('now')),
                task TEXT NOT NULL,
                answer TEXT NOT NULL
            );",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn in_memory() -> Result<Self> {
        Self::new(":memory:")
    }
}

#[async_trait]
impl Memory for SqliteMemory {
    async fn store(&self, entry: MemoryEntry) -> Result<()> {
        let json = serde_json::to_string(&entry)?;
        let conn = self.conn.lock().unwrap();
        conn.execute("INSERT INTO memory (entry) VALUES (?1)", [&json])?;
        Ok(())
    }

    async fn history(&self) -> Result<Vec<MemoryEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT entry FROM memory ORDER BY id ASC")?;
        let jsons = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let entries = jsons
            .iter()
            .map(|json| serde_json::from_str(json))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(entries)
    }

    async fn recall(&self, query: &str) -> Result<Vec<MemoryEntry>> {
        // Simple substring search for now. Could be upgraded to FTS5 or vector search.
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT entry FROM memory WHERE entry LIKE ?1 ORDER BY id ASC")?;
        let pattern = format!("%{query}%");
        let jsons = stmt
            .query_map([&pattern], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let entries = jsons
            .iter()
            .map(|json| serde_json::from_str(json))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(entries)
    }

    async fn clear(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM memory", [])?;
        Ok(())
    }

    // --- Session memory ---

    async fn store_session(&self, entry: SessionEntry) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO session_history (task, answer) VALUES (?1, ?2)",
            [&entry.task, &entry.answer],
        )?;
        Ok(())
    }

    async fn session_history(&self, limit: usize) -> Result<Vec<SessionEntry>> {
        let conn = self.conn.lock().unwrap();
        // Get the last `limit` entries, but return them in chronological order
        let mut stmt = conn.prepare(
            "SELECT task, answer FROM (
                SELECT task, answer, id FROM session_history ORDER BY id DESC LIMIT ?1
            ) ORDER BY id ASC",
        )?;
        let entries = stmt
            .query_map([limit as i64], |row| {
                Ok(SessionEntry {
                    task: row.get(0)?,
                    answer: row.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(entries)
    }

    async fn clear_session(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM session_history", [])?;
        Ok(())
    }
}
