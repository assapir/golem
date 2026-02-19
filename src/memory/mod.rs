pub mod sqlite;

use std::fmt;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::tools::{Outcome, ToolResult};

/// A single entry in the agent's memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MemoryEntry {
    /// The initial task given to the agent.
    Task { content: String },
    /// A thought + action + observations from one ReAct iteration.
    Iteration {
        thought: String,
        results: Vec<ToolResult>,
    },
    /// The final answer.
    Answer { thought: String, content: String },
}

impl fmt::Display for MemoryEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemoryEntry::Task { content } => {
                write!(f, "Task: {}", content)
            }
            MemoryEntry::Iteration { thought, results } => {
                write!(f, "Thought: {}", thought)?;
                for r in results {
                    match &r.outcome {
                        Outcome::Success(out) => {
                            let truncated = truncate(out, 200);
                            write!(f, "\n  [{}] ✓ {}", r.tool, truncated)?;
                        }
                        Outcome::Error(err) => {
                            write!(f, "\n  [{}] ✗ {}", r.tool, err)?;
                        }
                    }
                }
                Ok(())
            }
            MemoryEntry::Answer { thought, content } => {
                write!(f, "Answer ({}): {}", thought, content)
            }
        }
    }
}

fn truncate(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// A completed task summary carried across tasks in a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEntry {
    /// The task the user gave.
    pub task: String,
    /// The final answer the agent produced.
    pub answer: String,
}

/// What the agent remembers. Could be in-memory, SQLite, etc.
#[async_trait]
pub trait Memory: Send + Sync {
    // --- Per-task memory (cleared each run) ---

    async fn store(&self, entry: MemoryEntry) -> Result<()>;
    async fn history(&self) -> Result<Vec<MemoryEntry>>;
    async fn recall(&self, query: &str) -> Result<Vec<MemoryEntry>>;
    async fn clear(&self) -> Result<()>;

    // --- Session memory (persists across tasks) ---

    /// Store a completed task summary.
    async fn store_session(&self, entry: SessionEntry) -> Result<()>;
    /// Retrieve the last `limit` session entries (oldest first).
    async fn session_history(&self, limit: usize) -> Result<Vec<SessionEntry>>;
    /// Clear all session history (e.g. `/new` command).
    async fn clear_session(&self) -> Result<()>;
}
