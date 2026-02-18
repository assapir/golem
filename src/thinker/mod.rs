pub mod anthropic;
pub mod human;
pub mod mock;

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;

use crate::memory::MemoryEntry;

/// A single tool invocation request.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub tool: String,
    pub args: HashMap<String, String>,
}

/// What the thinker produces each iteration.
#[derive(Debug, Clone)]
pub enum Step {
    /// Execute tool calls. One item = single call. Multiple = parallel.
    Act {
        thought: String,
        calls: Vec<ToolCall>,
    },
    /// Task is complete.
    Finish { thought: String, answer: String },
}

/// Token usage from a single LLM call.
#[derive(Debug, Clone, Copy, Default)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl TokenUsage {
    /// Accumulate another usage into this one.
    pub fn add(&mut self, other: TokenUsage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
    }

    /// Total tokens (input + output).
    pub fn total(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

/// The result of a single thinker step: the step itself + optional token usage.
pub struct StepResult {
    pub step: Step,
    pub usage: Option<TokenUsage>,
}

/// The conversation context fed to the thinker each iteration.
pub struct Context {
    pub task: String,
    pub history: Vec<MemoryEntry>,
    pub available_tools: Vec<ToolDescription>,
}

/// Describes a tool so the thinker knows what's available.
#[derive(Debug, Clone)]
pub struct ToolDescription {
    pub name: String,
    pub description: String,
}

/// The borrowed brain. Could be a human, an LLM, or a test script.
#[async_trait]
pub trait Thinker: Send + Sync {
    async fn next_step(&self, context: &Context) -> Result<StepResult>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_usage_default_is_zero() {
        let usage = TokenUsage::default();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert_eq!(usage.total(), 0);
    }

    #[test]
    fn token_usage_total() {
        let usage = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
        };
        assert_eq!(usage.total(), 150);
    }

    #[test]
    fn token_usage_add_accumulates() {
        let mut usage = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
        };
        usage.add(TokenUsage {
            input_tokens: 200,
            output_tokens: 75,
        });
        assert_eq!(usage.input_tokens, 300);
        assert_eq!(usage.output_tokens, 125);
        assert_eq!(usage.total(), 425);
    }

    #[test]
    fn token_usage_add_zero_is_noop() {
        let mut usage = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
        };
        usage.add(TokenUsage::default());
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
    }
}
