use anyhow::{bail, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::auth::AuthStorage;
use crate::memory::MemoryEntry;
use crate::tools::Outcome;

use super::{Context, Step, Thinker, ToolCall};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
const DEFAULT_MODEL: &str = "claude-sonnet-4-20250514";
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

    fn build_system_prompt(context: &Context) -> String {
        let mut tools_desc = String::new();
        for tool in &context.available_tools {
            tools_desc.push_str(&format!("- {}: {}\n", tool.name, tool.description));
        }

        format!(
            r#"You are Golem, an AI agent that solves tasks using a ReAct loop.

You have access to these tools:
{tools_desc}
## How to respond

You MUST respond with valid JSON in one of two formats:

### To use tools:
```json
{{
  "thought": "your reasoning about what to do next",
  "action": {{
    "calls": [
      {{
        "tool": "tool_name",
        "args": {{"arg_name": "arg_value"}}
      }}
    ]
  }}
}}
```

### To give the final answer:
```json
{{
  "thought": "your reasoning about why you're done",
  "answer": "your final answer to the task"
}}
```

## Rules
- Always respond with ONLY valid JSON, no markdown fences, no extra text.
- Think step by step. Use tools to gather information before answering.
- You can make multiple tool calls in parallel by adding more items to the "calls" array.
- If a tool returns an error, analyze it and try a different approach.
- When you have enough information, use the "answer" format to respond."#
        )
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

    fn parse_response(text: &str) -> Result<Step> {
        // Try to extract JSON from the response (may be wrapped in markdown fences)
        let json_str = extract_json(text);

        let response: serde_json::Value = serde_json::from_str(json_str)
            .map_err(|e| anyhow::anyhow!("failed to parse LLM response as JSON: {}\nraw: {}", e, text))?;

        let thought = response
            .get("thought")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Check if this is a finish step
        if let Some(answer) = response.get("answer") {
            let answer = answer.as_str().unwrap_or("").to_string();
            return Ok(Step::Finish { thought, answer });
        }

        // Otherwise parse tool calls
        if let Some(action) = response.get("action")
            && let Some(calls) = action.get("calls").and_then(|c| c.as_array())
        {
            let tool_calls: Vec<ToolCall> = calls
                .iter()
                .filter_map(|call| {
                    let tool = call.get("tool")?.as_str()?.to_string();
                    let args_val = call.get("args")?;
                    let args: HashMap<String, String> = if let Some(obj) = args_val.as_object() {
                        obj.iter()
                            .map(|(k, v)| {
                                let val = match v {
                                    serde_json::Value::String(s) => s.clone(),
                                    other => other.to_string(),
                                };
                                (k.clone(), val)
                            })
                            .collect()
                    } else {
                        HashMap::new()
                    };
                    Some(ToolCall { tool, args })
                })
                .collect();

            if tool_calls.is_empty() {
                bail!("LLM returned action with no valid tool calls: {}", text);
            }

            return Ok(Step::Act {
                thought,
                calls: tool_calls,
            });
        }

        bail!(
            "LLM response is neither an answer nor a tool call: {}",
            text
        )
    }
}

#[async_trait]
impl Thinker for AnthropicThinker {
    async fn next_step(&self, context: &Context) -> Result<Step> {
        let api_key = self
            .auth
            .get_api_key("anthropic", "ANTHROPIC_API_KEY")
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no Anthropic credentials found. Run `golem login` or set ANTHROPIC_API_KEY."
                )
            })?;

        let system = Self::build_system_prompt(context);
        let messages = Self::build_messages(context);

        let body = ApiRequest {
            model: &self.model,
            max_tokens: MAX_TOKENS,
            system: &system,
            messages: &messages,
        };

        let is_oauth = api_key.contains("sk-ant-oat");

        let client = reqwest::Client::new();
        let mut req = client
            .post(API_URL)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json");

        if is_oauth {
            // OAuth tokens use Bearer auth + required beta/identity headers
            req = req
                .header("authorization", format!("Bearer {}", api_key))
                .header("anthropic-beta", OAUTH_BETA)
                .header(
                    "user-agent",
                    format!("claude-cli/{} (external, cli)", CLAUDE_CODE_VERSION),
                )
                .header("x-app", "cli");
        } else {
            // API keys use x-api-key header
            req = req.header("x-api-key", &api_key);
        }

        let resp = req.json(&body).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            bail!("Anthropic API error ({}): {}", status, text);
        }

        let api_resp: ApiResponse = resp.json().await?;

        // Extract text from content blocks
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

        // Log token usage
        if let Some(usage) = api_resp.usage {
            eprintln!(
                "  [tokens] input: {}, output: {}",
                usage.input_tokens, usage.output_tokens
            );
        }

        Self::parse_response(&text)
    }
}

/// Extract JSON from text that may be wrapped in markdown code fences.
fn extract_json(text: &str) -> &str {
    let trimmed = text.trim();

    // Try to strip ```json ... ``` fences
    if let Some(after) = trimmed.strip_prefix("```json")
        && let Some(json) = after.strip_suffix("```")
    {
        return json.trim();
    }
    if let Some(after) = trimmed.strip_prefix("```")
        && let Some(json) = after.strip_suffix("```")
    {
        return json.trim();
    }

    trimmed
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

    #[test]
    fn parse_finish_response() {
        let json = r#"{"thought": "I have the answer", "answer": "42"}"#;
        let step = AnthropicThinker::parse_response(json).unwrap();
        match step {
            Step::Finish { thought, answer } => {
                assert_eq!(thought, "I have the answer");
                assert_eq!(answer, "42");
            }
            _ => panic!("expected Finish"),
        }
    }

    #[test]
    fn parse_action_response() {
        let json = r#"{
            "thought": "I need to list files",
            "action": {
                "calls": [
                    {"tool": "shell", "args": {"command": "ls -la"}}
                ]
            }
        }"#;
        let step = AnthropicThinker::parse_response(json).unwrap();
        match step {
            Step::Act { thought, calls } => {
                assert_eq!(thought, "I need to list files");
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].tool, "shell");
                assert_eq!(calls[0].args.get("command").unwrap(), "ls -la");
            }
            _ => panic!("expected Act"),
        }
    }

    #[test]
    fn parse_parallel_calls() {
        let json = r#"{
            "thought": "check both",
            "action": {
                "calls": [
                    {"tool": "shell", "args": {"command": "uname"}},
                    {"tool": "shell", "args": {"command": "whoami"}}
                ]
            }
        }"#;
        let step = AnthropicThinker::parse_response(json).unwrap();
        match step {
            Step::Act { calls, .. } => assert_eq!(calls.len(), 2),
            _ => panic!("expected Act"),
        }
    }

    #[test]
    fn parse_fenced_json() {
        let text = "```json\n{\"thought\": \"done\", \"answer\": \"hello\"}\n```";
        let step = AnthropicThinker::parse_response(text).unwrap();
        match step {
            Step::Finish { answer, .. } => assert_eq!(answer, "hello"),
            _ => panic!("expected Finish"),
        }
    }

    #[test]
    fn parse_invalid_json_fails() {
        let result = AnthropicThinker::parse_response("not json at all");
        assert!(result.is_err());
    }

    #[test]
    fn parse_no_action_no_answer_fails() {
        let json = r#"{"thought": "hmm"}"#;
        let result = AnthropicThinker::parse_response(json);
        assert!(result.is_err());
    }

    #[test]
    fn parse_empty_calls_array_fails() {
        let json = r#"{"thought": "hmm", "action": {"calls": []}}"#;
        let result = AnthropicThinker::parse_response(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no valid tool calls"));
    }

    #[test]
    fn parse_missing_thought_defaults_to_empty() {
        let json = r#"{"answer": "42"}"#;
        let step = AnthropicThinker::parse_response(json).unwrap();
        match step {
            Step::Finish { thought, answer } => {
                assert_eq!(thought, "");
                assert_eq!(answer, "42");
            }
            _ => panic!("expected Finish"),
        }
    }

    #[test]
    fn parse_non_string_arg_values_serialized() {
        let json = r#"{
            "thought": "test",
            "action": {
                "calls": [
                    {"tool": "shell", "args": {"count": 42, "verbose": true}}
                ]
            }
        }"#;
        let step = AnthropicThinker::parse_response(json).unwrap();
        match step {
            Step::Act { calls, .. } => {
                assert_eq!(calls[0].args.get("count").unwrap(), "42");
                assert_eq!(calls[0].args.get("verbose").unwrap(), "true");
            }
            _ => panic!("expected Act"),
        }
    }

    #[test]
    fn parse_answer_takes_priority_over_action() {
        // If both "answer" and "action" are present, answer wins
        let json = r#"{
            "thought": "done",
            "answer": "the answer",
            "action": {"calls": [{"tool": "shell", "args": {"command": "ls"}}]}
        }"#;
        let step = AnthropicThinker::parse_response(json).unwrap();
        assert!(matches!(step, Step::Finish { .. }));
    }

    #[test]
    fn extract_json_plain() {
        assert_eq!(extract_json(r#"{"a": 1}"#), r#"{"a": 1}"#);
    }

    #[test]
    fn extract_json_with_json_fence() {
        let input = "```json\n{\"a\": 1}\n```";
        assert_eq!(extract_json(input), r#"{"a": 1}"#);
    }

    #[test]
    fn extract_json_with_plain_fence() {
        let input = "```\n{\"a\": 1}\n```";
        assert_eq!(extract_json(input), r#"{"a": 1}"#);
    }

    #[test]
    fn extract_json_trims_whitespace() {
        assert_eq!(extract_json("  \n {\"a\": 1}  \n "), r#"{"a": 1}"#);
    }

    #[test]
    fn extract_json_no_closing_fence_returns_as_is() {
        // Malformed fence — just return trimmed
        let input = "```json\n{\"a\": 1}";
        assert_eq!(extract_json(input), input.trim());
    }

    #[test]
    fn build_system_prompt_lists_tools() {
        let context = Context {
            task: "test".to_string(),
            history: vec![],
            available_tools: vec![
                crate::thinker::ToolDescription {
                    name: "shell".to_string(),
                    description: "run commands".to_string(),
                },
                crate::thinker::ToolDescription {
                    name: "read".to_string(),
                    description: "read files".to_string(),
                },
            ],
        };

        let prompt = AnthropicThinker::build_system_prompt(&context);
        assert!(prompt.contains("- shell: run commands"));
        assert!(prompt.contains("- read: read files"));
        assert!(prompt.contains("Golem"));
        assert!(prompt.contains("ReAct"));
    }

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
