pub mod anthropic;
pub mod human;
pub mod mock;

use anyhow::{Result, bail};
use async_trait::async_trait;
use std::collections::HashMap;

use crate::memory::MemoryEntry;

/// Maximum number of retry attempts when the LLM returns unparseable JSON.
pub const MAX_PARSE_RETRIES: usize = 1;

/// Correction prompt sent to the LLM after a parse failure.
pub const PARSE_RETRY_PROMPT: &str = "Your previous response was not valid JSON. You MUST respond with a JSON object only — no prose, no markdown, no explanation outside the JSON. Respond now with the correct JSON format.";

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

/// Parse an LLM text response into a `Step`. Handles JSON wrapped in
/// markdown fences or preceded/followed by prose text.
pub fn parse_response(text: &str) -> Result<Step> {
    let json_str = extract_json(text);

    let response: serde_json::Value = serde_json::from_str(json_str).map_err(|e| {
        anyhow::anyhow!("failed to parse LLM response as JSON: {}\nraw: {}", e, text)
    })?;

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

/// Extract JSON from text that may be wrapped in markdown code fences or
/// preceded/followed by prose text.
pub fn extract_json(text: &str) -> &str {
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

    // If the trimmed text doesn't start with '{', try to find a JSON object
    // by locating the first '{' and last '}' (handles prose before/after JSON)
    if !trimmed.starts_with('{')
        && let Some(start) = trimmed.find('{')
        && let Some(end) = trimmed.rfind('}')
        && end > start
    {
        return &trimmed[start..=end];
    }

    trimmed
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

    // --- parse_response tests ---

    #[test]
    fn parse_finish_response() {
        let json = r#"{"thought": "I have the answer", "answer": "42"}"#;
        let step = parse_response(json).unwrap();
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
        let step = parse_response(json).unwrap();
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
        let step = parse_response(json).unwrap();
        match step {
            Step::Act { calls, .. } => assert_eq!(calls.len(), 2),
            _ => panic!("expected Act"),
        }
    }

    #[test]
    fn parse_fenced_json() {
        let text = "```json\n{\"thought\": \"done\", \"answer\": \"hello\"}\n```";
        let step = parse_response(text).unwrap();
        match step {
            Step::Finish { answer, .. } => assert_eq!(answer, "hello"),
            _ => panic!("expected Finish"),
        }
    }

    #[test]
    fn parse_invalid_json_fails() {
        let result = parse_response("not json at all");
        assert!(result.is_err());
    }

    #[test]
    fn parse_no_action_no_answer_fails() {
        let json = r#"{"thought": "hmm"}"#;
        let result = parse_response(json);
        assert!(result.is_err());
    }

    #[test]
    fn parse_empty_calls_array_fails() {
        let json = r#"{"thought": "hmm", "action": {"calls": []}}"#;
        let result = parse_response(json);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("no valid tool calls")
        );
    }

    #[test]
    fn parse_missing_thought_defaults_to_empty() {
        let json = r#"{"answer": "42"}"#;
        let step = parse_response(json).unwrap();
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
        let step = parse_response(json).unwrap();
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
        let json = r#"{
            "thought": "done",
            "answer": "the answer",
            "action": {"calls": [{"tool": "shell", "args": {"command": "ls"}}]}
        }"#;
        let step = parse_response(json).unwrap();
        assert!(matches!(step, Step::Finish { .. }));
    }

    #[test]
    fn parse_prose_before_json_succeeds() {
        let input = r#"I need to understand the context.

{
  "thought": "Let me check the system",
  "action": {
    "calls": [
      {"tool": "shell", "args": {"command": "ps aux"}}
    ]
  }
}"#;
        let step = parse_response(input).unwrap();
        match step {
            Step::Act { thought, calls } => {
                assert_eq!(thought, "Let me check the system");
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].tool, "shell");
            }
            _ => panic!("expected Act"),
        }
    }

    // --- extract_json tests ---

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
    fn extract_json_no_closing_fence_still_extracts() {
        let input = "```json\n{\"a\": 1}";
        assert_eq!(extract_json(input), "{\"a\": 1}");
    }

    #[test]
    fn extract_json_prose_before_json() {
        let input = r#"Let me think about this carefully.

{"thought": "I know the answer", "answer": "42"}"#;
        assert_eq!(
            extract_json(input),
            r#"{"thought": "I know the answer", "answer": "42"}"#
        );
    }

    #[test]
    fn extract_json_prose_before_and_after() {
        let input = r#"Here's my response:
{"thought": "done", "answer": "hello"}
Hope that helps!"#;
        assert_eq!(
            extract_json(input),
            r#"{"thought": "done", "answer": "hello"}"#
        );
    }

    #[test]
    fn extract_json_prose_with_nested_braces() {
        let input = r#"I'll check that.

{
  "thought": "checking files",
  "action": {
    "calls": [
      {"tool": "shell", "args": {"command": "ls"}}
    ]
  }
}"#;
        let extracted = extract_json(input);
        let parsed: serde_json::Value = serde_json::from_str(extracted).unwrap();
        assert!(parsed.get("action").is_some());
    }
}
