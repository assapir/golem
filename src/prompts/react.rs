use crate::thinker::ToolDescription;

const INTRO: &str = "You are Golem, an AI agent that solves tasks using a ReAct loop.";
const RESPONSE_HEADER: &str = "You MUST respond with valid JSON in one of two formats:";
const TOOL_FORMAT: &str = "To use tools:\n{\"thought\": \"your reasoning about what to do next\", \"action\": {\"calls\": [{\"tool\": \"tool_name\", \"args\": {\"arg_name\": \"arg_value\"}}]}}";
const ANSWER_FORMAT: &str = "To give the final answer:\n{\"thought\": \"your reasoning about why you're done\", \"answer\": \"your final answer to the task\"}";
const RULES_HEADER: &str = "Rules:";
const RULES: &[&str] = &[
    "Output JSON only. No markdown, no extra text, no extra keys.",
    "Thought should be brief (1–2 sentences).",
    "Use only the tools listed above. Never invent tools.",
    "Use the calls array to run tools in parallel.",
    "If a tool returns an error, analyze it and try a different approach.",
    "When you have enough information, respond with the answer format.",
];

pub fn build_react_system_prompt(tools: &[ToolDescription]) -> String {
    let mut tools_desc = String::new();
    for tool in tools {
        tools_desc.push_str(&format!("- {}: {}\n", tool.name, tool.description));
    }

    let rules = RULES
        .iter()
        .map(|rule| format!("- {}", rule))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "{intro}\n\nYou have access to these tools:\n{tools_desc}\n{response_header}\n\n{tool_format}\n\n{answer_format}\n\n{rules_header}\n{rules}\n",
        intro = INTRO,
        tools_desc = tools_desc,
        response_header = RESPONSE_HEADER,
        tool_format = TOOL_FORMAT,
        answer_format = ANSWER_FORMAT,
        rules_header = RULES_HEADER,
        rules = rules
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_includes_tool_list() {
        let tools = vec![
            ToolDescription {
                name: "shell".to_string(),
                description: "run commands".to_string(),
            },
            ToolDescription {
                name: "read".to_string(),
                description: "read files".to_string(),
            },
        ];

        let prompt = build_react_system_prompt(&tools);
        assert!(prompt.contains("- shell: run commands"));
        assert!(prompt.contains("- read: read files"));
    }

    #[test]
    fn prompt_mentions_react() {
        let prompt = build_react_system_prompt(&[]);
        assert!(prompt.contains("ReAct"));
    }

    #[test]
    fn prompt_has_json_formats() {
        let prompt = build_react_system_prompt(&[]);
        assert!(prompt.contains("\"thought\""));
        assert!(prompt.contains("\"action\""));
        assert!(prompt.contains("\"answer\""));
    }

    #[test]
    fn prompt_has_no_markdown_fences() {
        let prompt = build_react_system_prompt(&[]);
        assert!(!prompt.contains("```"));
    }

    #[test]
    fn prompt_includes_rules() {
        let prompt = build_react_system_prompt(&[]);
        for rule in RULES {
            assert!(prompt.contains(rule));
        }
    }
}
