use crate::thinker::ToolDescription;

const INTRO: &str = "You are Golem, an AI agent that solves tasks using a ReAct loop.";

const TOOL_FORMAT: &str = r#"{
  "thought": "brief reasoning about what to do next",
  "action": {
    "calls": [
      {
        "tool": "tool_name",
        "args": { "arg_name": "arg_value" }
      }
    ]
  }
}"#;

const ANSWER_FORMAT: &str = r#"{
  "thought": "brief reasoning about why you're done",
  "answer": "your final answer to the task"
}"#;

const RULES: &[&str] = &[
    "Output JSON only. No markdown fences, no extra text, no extra keys.",
    "Thought should be brief (1-2 sentences).",
    "If the task can be answered without tools, respond with the answer format directly.",
    "Use only the tools listed above. Never invent tool names.",
    "Match each tool's expected args exactly as described.",
    "You can run multiple tools in parallel by adding items to the calls array.",
    "If a tool returns an error, analyze it and try a different approach.",
    "When you have enough information, respond with the answer format.",
];

pub fn build_react_system_prompt(tools: &[ToolDescription]) -> String {
    let mut prompt = String::with_capacity(1024);

    prompt.push_str(INTRO);
    prompt.push('\n');

    // Tool list
    if !tools.is_empty() {
        prompt.push_str("\nAvailable tools:\n");
        for tool in tools {
            prompt.push_str(&format!("- {}: {}\n", tool.name, tool.description));
        }
    }

    // Response formats
    prompt.push_str("\nYou MUST respond with valid JSON in one of two formats.\n");

    prompt.push_str("\nTo use tools:\n");
    prompt.push_str(TOOL_FORMAT);
    prompt.push('\n');

    prompt.push_str("\nTo give the final answer:\n");
    prompt.push_str(ANSWER_FORMAT);
    prompt.push('\n');

    // Rules
    prompt.push_str("\nRules:\n");
    for rule in RULES {
        prompt.push_str(&format!("- {}\n", rule));
    }

    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tools() -> Vec<ToolDescription> {
        vec![
            ToolDescription {
                name: "shell".to_string(),
                description: "Execute a shell command. Args: {\"command\": \"<cmd>\"}".to_string(),
            },
            ToolDescription {
                name: "read".to_string(),
                description: "Read a file. Args: {\"path\": \"<filepath>\"}".to_string(),
            },
        ]
    }

    #[test]
    fn includes_tool_list() {
        let prompt = build_react_system_prompt(&sample_tools());
        assert!(prompt.contains("- shell: Execute a shell command"));
        assert!(prompt.contains("- read: Read a file"));
    }

    #[test]
    fn no_tool_section_when_empty() {
        let prompt = build_react_system_prompt(&[]);
        assert!(!prompt.contains("Available tools:"));
    }

    #[test]
    fn mentions_react() {
        let prompt = build_react_system_prompt(&[]);
        assert!(prompt.contains("ReAct"));
    }

    #[test]
    fn json_examples_are_pretty_printed() {
        let prompt = build_react_system_prompt(&[]);
        // Multi-line JSON, not crammed into one line
        assert!(prompt.contains("\"thought\": \"brief reasoning"));
        assert!(prompt.contains("    \"calls\":"));
        assert!(prompt.contains("      {"));
    }

    #[test]
    fn has_both_response_formats() {
        let prompt = build_react_system_prompt(&[]);
        assert!(prompt.contains("\"action\""));
        assert!(prompt.contains("\"answer\""));
    }

    #[test]
    fn no_markdown_fences() {
        let prompt = build_react_system_prompt(&sample_tools());
        assert!(!prompt.contains("```"));
    }

    #[test]
    fn includes_all_rules() {
        let prompt = build_react_system_prompt(&[]);
        for rule in RULES {
            assert!(prompt.contains(rule), "missing rule: {}", rule);
        }
    }

    #[test]
    fn includes_direct_answer_guidance() {
        let prompt = build_react_system_prompt(&[]);
        assert!(prompt.contains("without tools"));
    }

    #[test]
    fn includes_args_matching_rule() {
        let prompt = build_react_system_prompt(&[]);
        assert!(prompt.contains("expected args exactly"));
    }
}
