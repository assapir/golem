use anyhow::{Result, bail};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::auth::AuthStorage;
use crate::consts::DEFAULT_MODEL;
use crate::memory::MemoryEntry;
use crate::prompts::build_react_system_prompt;
use crate::tools::Outcome;

use super::{
    Context, MAX_PARSE_RETRIES, ModelInfo, PARSE_RETRY_PROMPT, StepResult, Thinker, TokenUsage,
    parse_response,
};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const MODELS_API_URL: &str = "https://api.anthropic.com/v1/models";
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

        // Prepend session history as prior task/answer pairs
        for entry in &context.session_history {
            messages.push(Message {
                role: "user".to_string(),
                content: format!("Task: {}", entry.task),
            });
            messages.push(Message {
                role: "assistant".to_string(),
                content: format!(
                    "{}",
                    serde_json::json!({
                        "thought": "completed",
                        "answer": entry.answer
                    })
                ),
            });
        }

        // The current task
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

/// Whether an API key is an OAuth token (vs a plain API key).
fn is_oauth_token(api_key: &str) -> bool {
    api_key.starts_with("sk-ant-oat")
}

/// Apply Anthropic auth headers to a request builder.
fn apply_auth(builder: reqwest::RequestBuilder, api_key: &str) -> reqwest::RequestBuilder {
    if is_oauth_token(api_key) {
        builder
            .header("authorization", format!("Bearer {api_key}"))
            .header("anthropic-beta", OAUTH_BETA)
            .header(
                "user-agent",
                format!("claude-cli/{CLAUDE_CODE_VERSION} (external, cli)"),
            )
            .header("x-app", "cli")
    } else {
        builder.header("x-api-key", api_key)
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

        let client = reqwest::Client::new();
        let req = client
            .post(API_URL)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json");

        let req = apply_auth(req, api_key);

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

impl AnthropicThinker {
    /// Fetch the list of models from the Anthropic API.
    async fn fetch_models(&self, api_key: &str) -> Result<Vec<ModelInfo>> {
        let client = reqwest::Client::new();
        let req = client
            .get(MODELS_API_URL)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json");

        let req = apply_auth(req, api_key);

        let resp = req.send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            bail!("Anthropic models API error ({status}): {text}");
        }

        let list: ModelsListResponse = resp.json().await?;

        Ok(parse_models_response(list))
    }
}

#[async_trait]
impl Thinker for AnthropicThinker {
    async fn models(&self) -> Result<Vec<ModelInfo>> {
        let api_key = self
            .auth
            .get_api_key("anthropic", "ANTHROPIC_API_KEY")
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no Anthropic credentials found. Run `golem login` or set ANTHROPIC_API_KEY."
                )
            })?;

        self.fetch_models(&api_key).await
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn set_model(&mut self, model: String) {
        self.model = model;
    }

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

// --- Models API types ---

#[derive(Deserialize)]
struct ModelsListResponse {
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
    display_name: String,
    created_at: Option<String>,
    #[serde(rename = "type")]
    model_type: String,
}

/// Filter to real models, map to `ModelInfo`, and sort by ID.
fn parse_models_response(list: ModelsListResponse) -> Vec<ModelInfo> {
    let mut models: Vec<ModelInfo> = list
        .data
        .into_iter()
        .filter(|m| m.model_type == "model")
        .map(|m| ModelInfo {
            id: m.id,
            display_name: m.display_name,
            created_at: m.created_at,
        })
        .collect();

    models.sort_by(|a, b| a.id.cmp(&b.id));
    models
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
            session_history: vec![],
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
            session_history: vec![],
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
            session_history: vec![],
            available_tools: vec![],
        };

        let messages = AnthropicThinker::build_messages(&context);
        assert_eq!(messages.len(), 3);
        assert!(messages[2].content.contains("✗"));
        assert!(messages[2].content.contains("command not found"));
    }

    #[test]
    fn build_messages_includes_session_history() {
        use crate::memory::SessionEntry;

        let context = Context {
            task: "delete the biggest file".to_string(),
            history: vec![],
            session_history: vec![SessionEntry {
                task: "list files in /tmp".to_string(),
                answer: "a.txt (10KB), b.txt (50KB), c.txt (1KB)".to_string(),
            }],
            available_tools: vec![],
        };

        let messages = AnthropicThinker::build_messages(&context);
        // session: user task + assistant answer, then current: user task = 3
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "user");
        assert!(messages[0].content.contains("list files in /tmp"));
        assert_eq!(messages[1].role, "assistant");
        assert!(messages[1].content.contains("a.txt (10KB)"));
        assert_eq!(messages[2].role, "user");
        assert!(messages[2].content.contains("delete the biggest file"));
    }

    #[test]
    fn build_messages_session_history_before_current_task() {
        use crate::memory::SessionEntry;

        let context = Context {
            task: "current task".to_string(),
            history: vec![],
            session_history: vec![
                SessionEntry {
                    task: "first".to_string(),
                    answer: "answer 1".to_string(),
                },
                SessionEntry {
                    task: "second".to_string(),
                    answer: "answer 2".to_string(),
                },
            ],
            available_tools: vec![],
        };

        let messages = AnthropicThinker::build_messages(&context);
        // 2 session entries × 2 messages + 1 current task = 5
        assert_eq!(messages.len(), 5);
        assert!(messages[0].content.contains("first"));
        assert!(messages[1].content.contains("answer 1"));
        assert!(messages[2].content.contains("second"));
        assert!(messages[3].content.contains("answer 2"));
        assert!(messages[4].content.contains("current task"));
    }

    // --- OAuth detection ---

    #[test]
    fn oauth_token_detected() {
        assert!(is_oauth_token("sk-ant-oat01-something"));
    }

    #[test]
    fn api_key_not_detected_as_oauth() {
        assert!(!is_oauth_token("sk-ant-api03-something"));
        assert!(!is_oauth_token("some-key-containing-sk-ant-oat"));
    }

    // --- Models API parsing ---

    fn sample_models_response() -> ModelsListResponse {
        serde_json::from_str(
            r#"{
                "data": [
                    {
                        "id": "claude-sonnet-4-20250514",
                        "display_name": "Claude Sonnet 4",
                        "created_at": "2025-05-14T00:00:00Z",
                        "type": "model"
                    },
                    {
                        "id": "claude-haiku-3-20240307",
                        "display_name": "Claude Haiku 3",
                        "created_at": "2024-03-07T00:00:00Z",
                        "type": "model"
                    },
                    {
                        "id": "claude-opus-4-20250514",
                        "display_name": "Claude Opus 4",
                        "created_at": "2025-05-14T00:00:00Z",
                        "type": "model"
                    },
                    {
                        "id": "some-deprecated-thing",
                        "display_name": "Deprecated",
                        "created_at": null,
                        "type": "deprecated_model"
                    }
                ],
                "has_more": false,
                "first_id": "claude-haiku-3-20240307",
                "last_id": "some-deprecated-thing"
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn parse_models_filters_non_model_types() {
        let models = parse_models_response(sample_models_response());
        assert_eq!(models.len(), 3);
        assert!(models.iter().all(|m| m.id != "some-deprecated-thing"));
    }

    #[test]
    fn parse_models_sorted_by_id() {
        let models = parse_models_response(sample_models_response());
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "claude-haiku-3-20240307",
                "claude-opus-4-20250514",
                "claude-sonnet-4-20250514",
            ]
        );
    }

    #[test]
    fn parse_models_maps_fields_correctly() {
        let models = parse_models_response(sample_models_response());
        let sonnet = models
            .iter()
            .find(|m| m.id == "claude-sonnet-4-20250514")
            .unwrap();
        assert_eq!(sonnet.display_name, "Claude Sonnet 4");
        assert_eq!(sonnet.created_at.as_deref(), Some("2025-05-14T00:00:00Z"));
    }

    #[test]
    fn parse_models_handles_null_created_at() {
        let list: ModelsListResponse = serde_json::from_str(
            r#"{
                "data": [
                    {
                        "id": "test-model",
                        "display_name": "Test",
                        "created_at": null,
                        "type": "model"
                    }
                ],
                "has_more": false,
                "first_id": "test-model",
                "last_id": "test-model"
            }"#,
        )
        .unwrap();
        let models = parse_models_response(list);
        assert_eq!(models.len(), 1);
        assert!(models[0].created_at.is_none());
    }

    #[test]
    fn parse_models_empty_response() {
        let list: ModelsListResponse = serde_json::from_str(
            r#"{"data": [], "has_more": false, "first_id": null, "last_id": null}"#,
        )
        .unwrap();
        let models = parse_models_response(list);
        assert!(models.is_empty());
    }

    // --- Message building ---

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
            session_history: vec![],
            available_tools: vec![],
        };

        let messages = AnthropicThinker::build_messages(&context);
        // Only the task message, Answer is ignored
        assert_eq!(messages.len(), 1);
    }
}
