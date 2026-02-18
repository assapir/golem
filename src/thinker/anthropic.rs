use anyhow::{Result, bail};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::auth::AuthStorage;
use crate::consts::DEFAULT_MODEL;
use crate::memory::MemoryEntry;
use crate::prompts::build_react_system_prompt;
use crate::tools::Outcome;

use super::{
    Context, MAX_PARSE_RETRIES, PARSE_RETRY_PROMPT, StepResult, Thinker, TokenUsage, parse_response,
};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
const MAX_TOKENS: u32 = 8192;
const OAUTH_BETA: &str = "claude-code-20250219,oauth-2025-04-20";
const CLAUDE_CODE_VERSION: &str = "2.1.2";

/// An LLM thinker that calls the Anthropic Messages API.
pub struct AnthropicThinker {
    model: String,
    auth: AuthStorage,
}

impl AnthropicThinker {
    pub fn new(model: Option<String>, auth: AuthStorage) -> Self {
        Self {
            model: model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            auth,
        }
    }

    fn build_messages(context: &Context) -> Vec<Message> {
        let mut messages: Vec<Message> = Vec::new();

        // The task is the first user message
        messages.push(Message {
            role: "user".to_string(),
            content: format!("Task: {}", context.task),
        });

        // Convert history into assistant/user message pairs
        for entry in &context.history {
            match entry {
                MemoryEntry::Task { .. } => {
                    // Already handled as the first message
                }
                MemoryEntry::Iteration { thought, results } => {
                    // Reconstruct what the assistant said
                    let calls: Vec<serde_json::Value> = results
                        .iter()
                        .map(|r| {
                            serde_json::json!({
                                "tool": r.tool,
                                "args": {}
                            })
                        })
                        .collect();

                    let assistant_msg = serde_json::json!({
                        "thought": thought,
                        "action": {
                            "calls": calls
                        }
                    });

                    messages.push(Message {
                        role: "assistant".to_string(),
                        content: assistant_msg.to_string(),
                    });

                    // Tool results as user message
                    let mut observation = String::from("Tool results:\n");
                    for result in results {
                        match &result.outcome {
                            Outcome::Success(out) => {
                                observation.push_str(&format!("[{}] ✓ {}\n", result.tool, out));
                            }
                            Outcome::Error(err) => {
                                observation.push_str(&format!("[{}] ✗ {}\n", result.tool, err));
                            }
                        }
                    }

                    messages.push(Message {
                        role: "user".to_string(),
                        content: observation,
                    });
                }
                MemoryEntry::Answer { .. } => {
                    // Shouldn't appear in mid-loop context, but ignore gracefully
                }
            }
        }

        messages
    }
}

/// Raw API response: extracted text + optional token usage.
struct RawResponse {
    text: String,
    usage: Option<TokenUsage>,
}

impl AnthropicThinker {
    /// Send messages to the Anthropic API and return the raw text + usage.
    async fn call_api(
        &self,
        api_key: &str,
        system: &str,
        messages: &[Message],
    ) -> Result<RawResponse> {
        let body = ApiRequest {
            model: &self.model,
            max_tokens: MAX_TOKENS,
            system,
            messages,
        };

        let is_oauth = api_key.contains("sk-ant-oat");

        let client = reqwest::Client::new();
        let mut req = client
            .post(API_URL)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json");

        if is_oauth {
            req = req
                .header("authorization", format!("Bearer {}", api_key))
                .header("anthropic-beta", OAUTH_BETA)
                .header(
                    "user-agent",
                    format!("claude-cli/{} (external, cli)", CLAUDE_CODE_VERSION),
                )
                .header("x-app", "cli");
        } else {
            req = req.header("x-api-key", api_key);
        }

        let resp = req.json(&body).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            bail!("Anthropic API error ({}): {}", status, text);
        }

        let api_resp: ApiResponse = resp.json().await?;

        let text: String = api_resp
            .content
            .iter()
            .filter_map(|block| {
                if block.content_type == "text" {
                    block.text.as_deref()
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("");

        if text.is_empty() {
            bail!("Anthropic API returned empty response");
        }

        let usage = api_resp.usage.map(|u| TokenUsage {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
        });

        Ok(RawResponse { text, usage })
    }
}

#[async_trait]
impl Thinker for AnthropicThinker {
    async fn next_step(&self, context: &Context) -> Result<StepResult> {
        let api_key = self
            .auth
            .get_api_key("anthropic", "ANTHROPIC_API_KEY")
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no Anthropic credentials found. Run `golem login` or set ANTHROPIC_API_KEY."
                )
            })?;

        let system = build_react_system_prompt(&context.available_tools);
        let mut messages = Self::build_messages(context);
        let mut total_usage = TokenUsage::default();

        // Try parsing, with up to MAX_PARSE_RETRIES correction rounds
        for attempt in 0..=MAX_PARSE_RETRIES {
            let raw = self.call_api(&api_key, &system, &messages).await?;

            if let Some(usage) = raw.usage {
                total_usage.add(usage);
            }

            match parse_response(&raw.text) {
                Ok(step) => {
                    let combined = if total_usage.total() > 0 {
                        Some(total_usage)
                    } else {
                        None
                    };
                    return Ok(StepResult {
                        step,
                        usage: combined,
                    });
                }
                Err(parse_err) => {
                    if attempt < MAX_PARSE_RETRIES {
                        eprintln!(
                            "warning: LLM returned invalid JSON (attempt {}), retrying with correction",
                            attempt + 1
                        );
                        // Append the malformed response + correction as context
                        messages.push(Message {
                            role: "assistant".to_string(),
                            content: raw.text,
                        });
                        messages.push(Message {
                            role: "user".to_string(),
                            content: PARSE_RETRY_PROMPT.to_string(),
                        });
                    } else {
                        return Err(parse_err);
                    }
                }
            }
        }

        // Unreachable: the loop always returns or errors
        bail!("unexpected: parse retry loop exited without result")
    }
}

// --- API types ---

#[derive(Serialize)]
struct ApiRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    messages: &'a [Message],
}

#[derive(Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ApiResponse {
    content: Vec<ContentBlock>,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
}

#[derive(Deserialize)]
struct Usage {
    input_tokens: u64,
    output_tokens: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thinker::Context;

    #[test]
    fn build_messages_task_only() {
        let context = Context {
            task: "do something".to_string(),
            history: vec![],
            available_tools: vec![],
        };

        let messages = AnthropicThinker::build_messages(&context);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "Task: do something");
    }

    #[test]
    fn build_messages_with_iteration_history() {
        use crate::tools::{Outcome, ToolResult};

        let context = Context {
            task: "check kernel".to_string(),
            history: vec![
                MemoryEntry::Task {
                    content: "check kernel".to_string(),
                },
                MemoryEntry::Iteration {
                    thought: "let me check".to_string(),
                    results: vec![ToolResult {
                        tool: "shell".to_string(),
                        outcome: Outcome::Success("6.18.8".to_string()),
                    }],
                },
            ],
            available_tools: vec![],
        };

        let messages = AnthropicThinker::build_messages(&context);
        // Task message + assistant thought + user observation = 3
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].role, "assistant");
        assert!(messages[1].content.contains("let me check"));
        assert_eq!(messages[2].role, "user");
        assert!(messages[2].content.contains("6.18.8"));
        assert!(messages[2].content.contains("✓"));
    }

    #[test]
    fn build_messages_with_error_result() {
        use crate::tools::{Outcome, ToolResult};

        let context = Context {
            task: "test".to_string(),
            history: vec![
                MemoryEntry::Task {
                    content: "test".to_string(),
                },
                MemoryEntry::Iteration {
                    thought: "try something".to_string(),
                    results: vec![ToolResult {
                        tool: "shell".to_string(),
                        outcome: Outcome::Error("command not found".to_string()),
                    }],
                },
            ],
            available_tools: vec![],
        };

        let messages = AnthropicThinker::build_messages(&context);
        assert_eq!(messages.len(), 3);
        assert!(messages[2].content.contains("✗"));
        assert!(messages[2].content.contains("command not found"));
    }

    #[test]
    fn build_messages_ignores_answer_entries() {
        let context = Context {
            task: "test".to_string(),
            history: vec![
                MemoryEntry::Task {
                    content: "test".to_string(),
                },
                MemoryEntry::Answer {
                    thought: "done".to_string(),
                    content: "42".to_string(),
                },
            ],
            available_tools: vec![],
        };

        let messages = AnthropicThinker::build_messages(&context);
        // Only the task message, Answer is ignored
        assert_eq!(messages.len(), 1);
    }
}
