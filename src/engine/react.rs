use anyhow::{Result, bail};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use super::Engine;
use crate::memory::{Memory, MemoryEntry};
use crate::thinker::{Context, Step, Thinker, TokenUsage};
use crate::tools::{Outcome, ToolRegistry, ToolResult};

pub struct ReactConfig {
    pub max_iterations: usize,
    pub tool_timeout: Duration,
}

impl Default for ReactConfig {
    fn default() -> Self {
        Self {
            max_iterations: 20,
            tool_timeout: Duration::from_secs(30),
        }
    }
}

/// The ReAct loop. Wires together a Thinker, ToolRegistry, and Memory.
pub struct ReactEngine {
    thinker: Arc<RwLock<Box<dyn Thinker>>>,
    tools: Arc<ToolRegistry>,
    memory: Box<dyn Memory>,
    config: ReactConfig,
    session_usage: TokenUsage,
}

impl ReactEngine {
    pub fn new(
        thinker: Box<dyn Thinker>,
        tools: Arc<ToolRegistry>,
        memory: Box<dyn Memory>,
        config: ReactConfig,
    ) -> Self {
        Self {
            thinker: Arc::new(RwLock::new(thinker)),
            tools,
            memory,
            config,
            session_usage: TokenUsage::default(),
        }
    }

    /// Swap the thinker at runtime. The next iteration will use the new one.
    pub async fn set_thinker(&self, thinker: Box<dyn Thinker>) {
        *self.thinker.write().await = thinker;
    }

    /// Access memory history (useful for tests and inspection).
    pub async fn history(&self) -> Result<Vec<MemoryEntry>> {
        self.memory.history().await
    }

    /// Cumulative token usage across all tasks in this session.
    pub fn session_usage(&self) -> TokenUsage {
        self.session_usage
    }
}

#[async_trait]
impl Engine for ReactEngine {
    async fn run(&mut self, task: &str) -> Result<String> {
        // Each task starts with a clean slate
        self.memory.clear().await?;

        self.memory
            .store(MemoryEntry::Task {
                content: task.to_string(),
            })
            .await?;

        for iteration in 0..self.config.max_iterations {
            let context = Context {
                task: task.to_string(),
                history: self.memory.history().await?,
                available_tools: self.tools.descriptions().await,
            };

            let step_result = {
                let thinker = self.thinker.read().await;
                thinker.next_step(&context).await?
            };

            if let Some(usage) = step_result.usage {
                self.session_usage.add(usage);
            }

            match step_result.step {
                Step::Act { thought, calls } => {
                    println!("\n[iteration {}] Thought: {}", iteration + 1, thought);
                    println!(
                        "[iteration {}] Executing {} tool call(s)...",
                        iteration + 1,
                        calls.len()
                    );

                    let timeout = self.config.tool_timeout;
                    let tools = Arc::clone(&self.tools);

                    let futures: Vec<_> = calls
                        .into_iter()
                        .map(|call| {
                            let tools = Arc::clone(&tools);
                            async move {
                                match tokio::time::timeout(
                                    timeout,
                                    tools.execute(&call.tool, &call.args),
                                )
                                .await
                                {
                                    Ok(result) => result,
                                    Err(_) => ToolResult {
                                        tool: call.tool,
                                        outcome: Outcome::Error("timed out".to_string()),
                                    },
                                }
                            }
                        })
                        .collect();

                    let results = futures::future::join_all(futures).await;

                    for result in &results {
                        match &result.outcome {
                            Outcome::Success(out) => {
                                println!("  [{}] ✓ {}", result.tool, out);
                            }
                            Outcome::Error(err) => {
                                println!("  [{}] ✗ {}", result.tool, err);
                            }
                        }
                    }

                    self.memory
                        .store(MemoryEntry::Iteration { thought, results })
                        .await?;
                }

                Step::Finish { thought, answer } => {
                    println!("\n[done] Thought: {}", thought);
                    println!("[done] Answer: {}", answer);

                    self.memory
                        .store(MemoryEntry::Answer {
                            thought,
                            content: answer.clone(),
                        })
                        .await?;

                    return Ok(answer);
                }
            }
        }

        bail!("max iterations ({}) reached", self.config.max_iterations)
    }
}
