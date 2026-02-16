pub mod sqlite;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::tools::ToolResult;

/// A single entry in the agent's memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MemoryEntry {
    /// The initial task given to the agent.
    Task {
        content: String,
    },
    /// A thought + action + observations from one ReAct iteration.
    Iteration {
        thought: String,
        results: Vec<ToolResult>,
    },
    /// The final answer.
    Answer {
        thought: String,
        content: String,
    },
}

/// What the agent remembers. Could be in-memory, SQLite, etc.
#[async_trait]
pub trait Memory: Send + Sync {
    async fn store(&self, entry: MemoryEntry) -> Result<()>;
    async fn history(&self) -> Result<Vec<MemoryEntry>>;
    async fn recall(&self, query: &str) -> Result<Vec<MemoryEntry>>;
    async fn clear(&self) -> Result<()>;
}
