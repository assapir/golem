use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use super::Tool;

/// A tool the agent can invoke to exit the REPL.
///
/// When the user says something like "bye" or "quit", the LLM can call this
/// tool to signal a graceful shutdown. The caller checks [`ExitTool::triggered`]
/// after each engine run to decide whether to break the loop.
///
/// Share via `Arc<ExitTool>` — the `AtomicBool` is stored inline; no inner
/// `Arc` is needed.
pub struct ExitTool {
    flag: AtomicBool,
}

impl ExitTool {
    pub fn new() -> Self {
        Self {
            flag: AtomicBool::new(false),
        }
    }

    /// Returns `true` if the tool was invoked (the agent wants to exit).
    pub fn triggered(&self) -> bool {
        self.flag.load(Ordering::Acquire)
    }

    /// Reset the flag (useful if the caller decides not to exit after all).
    pub fn reset(&self) {
        self.flag.store(false, Ordering::Release);
    }
}

impl Default for ExitTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ExitTool {
    fn name(&self) -> &str {
        "exit"
    }

    fn description(&self) -> &str {
        "Exit the session. Call this when the user wants to quit, say goodbye, or end the conversation. No arguments required."
    }

    async fn execute(&self, _args: &HashMap<String, String>) -> Result<String> {
        self.flag.store(true, Ordering::Release);
        Ok("Goodbye! Exiting session.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_triggered_initially() {
        let tool = ExitTool::new();
        assert!(!tool.triggered());
    }

    #[tokio::test]
    async fn triggered_after_execute() {
        let tool = ExitTool::new();
        let result = tool.execute(&HashMap::new()).await;
        assert!(result.is_ok());
        assert!(tool.triggered());
    }

    #[tokio::test]
    async fn reset_clears_flag() {
        let tool = ExitTool::new();
        tool.execute(&HashMap::new()).await.unwrap();
        assert!(tool.triggered());
        tool.reset();
        assert!(!tool.triggered());
    }

    #[test]
    fn name_is_exit() {
        let tool = ExitTool::new();
        assert_eq!(Tool::name(&tool), "exit");
    }

    #[test]
    fn description_mentions_quit() {
        let tool = ExitTool::new();
        assert!(Tool::description(&tool).contains("quit"));
    }

    #[test]
    fn default_impl_works() {
        let tool = ExitTool::default();
        assert!(!tool.triggered());
    }
}
