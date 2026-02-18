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
