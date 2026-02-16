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

/// What the agent remembers. Could be in-memory, SQLite, etc.
#[async_trait]
pub trait Memory: Send + Sync {
    async fn store(&self, entry: MemoryEntry) -> Result<()>;
    async fn history(&self) -> Result<Vec<MemoryEntry>>;
    async fn recall(&self, query: &str) -> Result<Vec<MemoryEntry>>;
    async fn clear(&self) -> Result<()>;
}
