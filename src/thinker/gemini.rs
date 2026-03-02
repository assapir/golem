use anyhow::{Result, bail};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::auth::AuthStorage;
use crate::auth::storage::ResolvedCredential;
use crate::debug::DebugMode;
use crate::memory::MemoryEntry;

/// Default Gemini model when none is specified.
const DEFAULT_MODEL: &str = "gemini-3-flash-preview";
use crate::prompts::build_react_system_prompt_with_session;
use crate::tools::Outcome;

use super::{
    Context, MAX_PARSE_RETRIES, ModelInfo, PARSE_RETRY_PROMPT, StepResult, Thinker, TokenUsage,
    parse_response,
};

const API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";

/// An LLM thinker that calls the Google Gemini API.
pub struct GeminiThinker {
    model: String,
    auth: AuthStorage,
    debug: DebugMode,
}

impl GeminiThinker {
    pub fn new(model: Option<String>, auth: AuthStorage, debug: DebugMode) -> Self {
        Self {
            model: model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            auth,
            debug,
        }
    }

    fn build_contents(context: &Context) -> Vec<Content> {
        let mut contents: Vec<Content> = Vec::new();

        // Prepend session history as prior task/answer pairs
        for (i, entry) in context.session_history.iter().enumerate() {
            contents.push(Content {
                role: "user".to_string(),
                parts: vec![Part {
                    text: format!(
                        "[Prior task {}/{}] {}",
                        i + 1,
                        context.session_history.len(),
                        entry.task
                    ),
                }],
            });
            contents.push(Content {
                role: "model".to_string(),
                parts: vec![Part {
                    text: serde_json::json!({
                        "thought": "completed",
                        "answer": entry.answer
                    })
                    .to_string(),
                }],
            });
        }

        // The current task
        contents.push(Content {
            role: "user".to_string(),
            parts: vec![Part {
                text: format!("Task: {}", context.task),
            }],
        });

        // Convert history into model/user content pairs
        for entry in &context.history {
            match entry {
                MemoryEntry::Task { .. } => {
                    // Already handled as the first message
                }
                MemoryEntry::Iteration { thought, results } => {
                    let calls: Vec<serde_json::Value> = results
                        .iter()
                        .map(|r| {
                            serde_json::json!({
                                "tool": r.tool,
                                "args": {}
                            })
                        })
                        .collect();

                    let model_msg = serde_json::json!({
                        "thought": thought,
                        "action": {
                            "calls": calls
                        }
                    });

                    contents.push(Content {
                        role: "model".to_string(),
                        parts: vec![Part {
                            text: model_msg.to_string(),
                        }],
                    });

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

                    contents.push(Content {
                        role: "user".to_string(),
                        parts: vec![Part { text: observation }],
                    });
                }
                MemoryEntry::Answer { .. } => {
                    // Shouldn't appear in mid-loop context
                }
            }
        }

        contents
    }
}

/// Raw API response: extracted text + optional token usage.
struct RawResponse {
    text: String,
    usage: Option<TokenUsage>,
}

/// Apply Gemini auth to a request: Bearer token for OAuth, query param for API keys.
fn apply_auth(
    builder: reqwest::RequestBuilder,
    credential: &ResolvedCredential,
) -> reqwest::RequestBuilder {
    if credential.is_oauth {
        builder.header("Authorization", format!("Bearer {}", credential.token))
    } else {
        builder.query(&[("key", &credential.token)])
    }
}

impl GeminiThinker {
    /// Send contents to the Gemini API and return the raw text + usage.
    async fn call_api(
        &self,
        credential: &ResolvedCredential,
        system: &str,
        contents: &[Content],
    ) -> Result<RawResponse> {
        let url = format!("{}/models/{}:generateContent", API_BASE, self.model);

        let body = ApiRequest {
            system_instruction: Some(SystemInstruction {
                parts: vec![Part {
                    text: system.to_string(),
                }],
            }),
            contents,
            generation_config: Some(GenerationConfig {
                response_mime_type: "application/json".to_string(),
            }),
        };

        self.debug.log(|| format!("→ POST {url}"));
        self.debug.log(|| format!("→ model: {}", self.model));
        self.debug.log(|| {
            let preview: String = system.chars().take(200).collect();
            format!("→ system: {preview}...")
        });
        self.debug.log(|| {
            let total_chars: usize = contents
                .iter()
                .flat_map(|c| &c.parts)
                .map(|p| p.text.len())
                .sum();
            format!(
                "→ contents: {} messages, {} chars",
                contents.len(),
                total_chars
            )
        });

        let client = reqwest::Client::new();
        let req = client.post(&url).header("Content-Type", "application/json");
        let resp = apply_auth(req, credential).json(&body).send().await?;
        let status = resp.status();

        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            self.debug.log(|| format!("← status: {status}"));
            self.debug.log(|| format!("← error: {text}"));
            bail!("Gemini API error ({}): {}", status, text);
        }

        self.debug.log(|| format!("← status: {status}"));

        let api_resp: ApiResponse = resp.json().await?;

        let text = api_resp
            .candidates
            .into_iter()
            .flat_map(|c| c.content.parts)
            .map(|p| p.text)
            .collect::<Vec<_>>()
            .join("");

        if text.is_empty() {
            bail!("Gemini API returned empty response");
        }

        let usage = api_resp.usage_metadata.map(|u| TokenUsage {
            input_tokens: u.prompt_token_count,
            output_tokens: u.candidates_token_count,
        });

        if let Some(u) = &usage {
            self.debug
                .log(|| format!("← tokens: {} in / {} out", u.input_tokens, u.output_tokens));
        }
        self.debug.log(|| format!("← raw: {text}"));

        Ok(RawResponse { text, usage })
    }

    /// Fetch the list of models from the Gemini API.
    async fn fetch_models(&self, credential: &ResolvedCredential) -> Result<Vec<ModelInfo>> {
        let url = format!("{}/models", API_BASE);

        let client = reqwest::Client::new();
        let req = client.get(&url);
        let resp = apply_auth(req, credential).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            bail!("Gemini models API error ({status}): {text}");
        }

        let list: ModelsListResponse = resp.json().await?;

        Ok(parse_models_response(list))
    }
}

#[async_trait]
impl Thinker for GeminiThinker {
    async fn models(&self) -> Result<Vec<ModelInfo>> {
        let credential = self
            .auth
            .get_credential("google", "GEMINI_API_KEY")
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no Google credentials found. Run `golem login google` or set GEMINI_API_KEY."
                )
            })?;

        self.fetch_models(&credential).await
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn set_model(&mut self, model: String) {
        self.model = model;
    }

    async fn next_step(&self, context: &Context) -> Result<StepResult> {
        let credential = self
            .auth
            .get_credential("google", "GEMINI_API_KEY")
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no Google credentials found. Run `golem login google` or set GEMINI_API_KEY."
                )
            })?;

        let system = build_react_system_prompt_with_session(
            &context.available_tools,
            !context.session_history.is_empty(),
        );
        let mut contents = Self::build_contents(context);
        let mut total_usage = TokenUsage::default();

        for attempt in 0..=MAX_PARSE_RETRIES {
            let raw = self.call_api(&credential, &system, &contents).await?;

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
                        contents.push(Content {
                            role: "model".to_string(),
                            parts: vec![Part { text: raw.text }],
                        });
                        contents.push(Content {
                            role: "user".to_string(),
                            parts: vec![Part {
                                text: PARSE_RETRY_PROMPT.to_string(),
                            }],
                        });
                    } else {
                        return Err(parse_err);
                    }
                }
            }
        }

        bail!("unexpected: parse retry loop exited without result")
    }
}

// --- API types ---

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<SystemInstruction>,
    contents: &'a [Content],
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GenerationConfig>,
}

#[derive(Serialize)]
struct SystemInstruction {
    parts: Vec<Part>,
}

#[derive(Serialize, Deserialize, Clone)]
struct Content {
    role: String,
    parts: Vec<Part>,
}

#[derive(Serialize, Deserialize, Clone)]
struct Part {
    text: String,
}

#[derive(Serialize)]
struct GenerationConfig {
    #[serde(rename = "responseMimeType")]
    response_mime_type: String,
}

#[derive(Deserialize)]
struct ApiResponse {
    #[serde(default)]
    candidates: Vec<Candidate>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<UsageMetadata>,
}

#[derive(Deserialize)]
struct Candidate {
    content: CandidateContent,
}

#[derive(Deserialize)]
struct CandidateContent {
    parts: Vec<ResponsePart>,
}

#[derive(Deserialize)]
struct ResponsePart {
    text: String,
}

#[derive(Deserialize)]
struct UsageMetadata {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: u64,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: u64,
}

// --- Models API types ---

#[derive(Deserialize)]
struct ModelsListResponse {
    models: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    /// Full name like "models/gemini-2.0-flash"
    name: String,
    #[serde(rename = "displayName")]
    display_name: String,
    /// Supported generation methods
    #[serde(rename = "supportedGenerationMethods", default)]
    supported_generation_methods: Vec<String>,
}

/// Filter to models that support generateContent, strip "models/" prefix, sort.
fn parse_models_response(list: ModelsListResponse) -> Vec<ModelInfo> {
    let mut models: Vec<ModelInfo> = list
        .models
        .into_iter()
        .filter(|m| {
            m.supported_generation_methods
                .iter()
                .any(|method| method == "generateContent")
        })
        .map(|m| {
            let id = m
                .name
                .strip_prefix("models/")
                .unwrap_or(&m.name)
                .to_string();
            ModelInfo {
                id,
                display_name: m.display_name,
                created_at: None,
            }
        })
        .collect();

    models.sort_by(|a, b| a.id.cmp(&b.id));
    models
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::SessionEntry;
    use crate::thinker::Context;
    use crate::tools::{Outcome, ToolResult};

    #[test]
    fn build_contents_task_only() {
        let context = Context {
            task: "do something".to_string(),
            history: vec![],
            session_history: vec![],
            available_tools: vec![],
        };

        let contents = GeminiThinker::build_contents(&context);
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0].role, "user");
        assert_eq!(contents[0].parts[0].text, "Task: do something");
    }

    #[test]
    fn build_contents_with_iteration_history() {
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

        let contents = GeminiThinker::build_contents(&context);
        assert_eq!(contents.len(), 3);
        assert_eq!(contents[0].role, "user");
        assert_eq!(contents[1].role, "model");
        assert!(contents[1].parts[0].text.contains("let me check"));
        assert_eq!(contents[2].role, "user");
        assert!(contents[2].parts[0].text.contains("6.18.8"));
        assert!(contents[2].parts[0].text.contains("✓"));
    }

    #[test]
    fn build_contents_with_error_result() {
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

        let contents = GeminiThinker::build_contents(&context);
        assert_eq!(contents.len(), 3);
        assert!(contents[2].parts[0].text.contains("✗"));
        assert!(contents[2].parts[0].text.contains("command not found"));
    }

    #[test]
    fn build_contents_includes_session_history() {
        let context = Context {
            task: "delete the biggest file".to_string(),
            history: vec![],
            session_history: vec![SessionEntry {
                task: "list files in /tmp".to_string(),
                answer: "a.txt (10KB), b.txt (50KB), c.txt (1KB)".to_string(),
            }],
            available_tools: vec![],
        };

        let contents = GeminiThinker::build_contents(&context);
        assert_eq!(contents.len(), 3);
        assert_eq!(contents[0].role, "user");
        assert!(contents[0].parts[0].text.contains("list files in /tmp"));
        assert_eq!(contents[1].role, "model");
        assert!(contents[1].parts[0].text.contains("a.txt (10KB)"));
        assert_eq!(contents[2].role, "user");
        assert!(
            contents[2].parts[0]
                .text
                .contains("delete the biggest file")
        );
    }

    #[test]
    fn build_contents_session_history_before_current_task() {
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

        let contents = GeminiThinker::build_contents(&context);
        assert_eq!(contents.len(), 5);
        assert!(contents[0].parts[0].text.contains("first"));
        assert!(contents[1].parts[0].text.contains("answer 1"));
        assert!(contents[2].parts[0].text.contains("second"));
        assert!(contents[3].parts[0].text.contains("answer 2"));
        assert!(contents[4].parts[0].text.contains("current task"));
    }

    #[test]
    fn build_contents_ignores_answer_entries() {
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

        let contents = GeminiThinker::build_contents(&context);
        assert_eq!(contents.len(), 1);
    }

    #[test]
    fn build_contents_uses_model_role() {
        let context = Context {
            task: "test".to_string(),
            history: vec![
                MemoryEntry::Task {
                    content: "test".to_string(),
                },
                MemoryEntry::Iteration {
                    thought: "thinking".to_string(),
                    results: vec![ToolResult {
                        tool: "shell".to_string(),
                        outcome: Outcome::Success("ok".to_string()),
                    }],
                },
            ],
            session_history: vec![],
            available_tools: vec![],
        };

        let contents = GeminiThinker::build_contents(&context);
        // Gemini uses "model" not "assistant"
        assert_eq!(contents[1].role, "model");
    }

    // --- Models API parsing ---

    fn sample_models_response() -> ModelsListResponse {
        serde_json::from_str(
            r#"{
                "models": [
                    {
                        "name": "models/gemini-3-flash-preview",
                        "displayName": "Gemini 3 Flash Preview",
                        "supportedGenerationMethods": ["generateContent", "countTokens"]
                    },
                    {
                        "name": "models/gemini-3.1-pro-preview",
                        "displayName": "Gemini 3.1 Pro Preview",
                        "supportedGenerationMethods": ["generateContent", "countTokens"]
                    },
                    {
                        "name": "models/embedding-001",
                        "displayName": "Embedding 001",
                        "supportedGenerationMethods": ["embedContent"]
                    },
                    {
                        "name": "models/gemini-2.5-flash",
                        "displayName": "Gemini 2.5 Flash",
                        "supportedGenerationMethods": ["generateContent", "countTokens"]
                    }
                ]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn parse_models_filters_non_generate_models() {
        let models = parse_models_response(sample_models_response());
        assert!(models.iter().all(|m| m.id != "embedding-001"));
    }

    #[test]
    fn parse_models_strips_prefix() {
        let models = parse_models_response(sample_models_response());
        assert!(models.iter().any(|m| m.id == "gemini-3-flash-preview"));
        assert!(models.iter().all(|m| !m.id.starts_with("models/")));
    }

    #[test]
    fn parse_models_sorted_by_id() {
        let models = parse_models_response(sample_models_response());
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "gemini-2.5-flash",
                "gemini-3-flash-preview",
                "gemini-3.1-pro-preview",
            ]
        );
    }

    #[test]
    fn parse_models_maps_display_name() {
        let models = parse_models_response(sample_models_response());
        let flash = models
            .iter()
            .find(|m| m.id == "gemini-3-flash-preview")
            .unwrap();
        assert_eq!(flash.display_name, "Gemini 3 Flash Preview");
    }

    #[test]
    fn parse_models_empty_response() {
        let list: ModelsListResponse = serde_json::from_str(r#"{"models": []}"#).unwrap();
        let models = parse_models_response(list);
        assert!(models.is_empty());
    }

    #[test]
    fn parse_models_no_created_at() {
        let models = parse_models_response(sample_models_response());
        assert!(models.iter().all(|m| m.created_at.is_none()));
    }
}
