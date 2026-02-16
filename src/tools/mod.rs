pub mod shell;

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::thinker::ToolDescription;

/// Outcome of a single tool execution. Errors are information, not failures.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Outcome {
    Success(String),
    Error(String),
}

/// Result of executing a tool call.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolResult {
    pub tool: String,
    pub outcome: Outcome,
}

/// Something the agent can do.
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    async fn execute(&self, args: &HashMap<String, String>) -> Result<String>;
}

/// Holds all registered tools. RwLock allows runtime registration + parallel reads.
pub struct ToolRegistry {
    tools: RwLock<HashMap<String, Arc<dyn Tool>>>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
        }
    }

    pub async fn register(&self, tool: Arc<dyn Tool>) {
        let name = tool.name().to_string();
        self.tools.write().await.insert(name, tool);
    }

    pub async fn unregister(&self, name: &str) {
        self.tools.write().await.remove(name);
    }

    pub async fn execute(&self, tool_name: &str, args: &HashMap<String, String>) -> ToolResult {
        let tools = self.tools.read().await;
        match tools.get(tool_name) {
            Some(tool) => match tool.execute(args).await {
                Ok(output) => ToolResult {
                    tool: tool_name.to_string(),
                    outcome: Outcome::Success(output),
                },
                Err(e) => ToolResult {
                    tool: tool_name.to_string(),
                    outcome: Outcome::Error(e.to_string()),
                },
            },
            None => ToolResult {
                tool: tool_name.to_string(),
                outcome: Outcome::Error(format!("unknown tool: {}", tool_name)),
            },
        }
    }

    pub async fn descriptions(&self) -> Vec<ToolDescription> {
        self.tools
            .read()
            .await
            .values()
            .map(|t| ToolDescription {
                name: t.name().to_string(),
                description: t.description().to_string(),
            })
            .collect()
    }
}
