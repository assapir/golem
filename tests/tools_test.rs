use std::collections::HashMap;
use std::sync::Arc;

use golem::tools::shell::ShellTool;
use golem::tools::{Outcome, ToolRegistry};

#[tokio::test]
async fn shell_tool_executes_command() {
    let tool = ShellTool;
    let args = HashMap::from([("command".to_string(), "echo hello".to_string())]);

    let result = golem::tools::Tool::execute(&tool, &args).await.unwrap();
    assert_eq!(result.trim(), "hello");
}

#[tokio::test]
async fn shell_tool_returns_error_on_bad_command() {
    let tool = ShellTool;
    let args = HashMap::from([("command".to_string(), "false".to_string())]);

    let result = golem::tools::Tool::execute(&tool, &args).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn shell_tool_requires_command_arg() {
    let tool = ShellTool;
    let args = HashMap::new();

    let result = golem::tools::Tool::execute(&tool, &args).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("command"));
}

#[tokio::test]
async fn registry_executes_known_tool() {
    let registry = ToolRegistry::new();
    registry.register(Arc::new(ShellTool)).await;

    let args = HashMap::from([("command".to_string(), "echo works".to_string())]);
    let result = registry.execute("shell", &args).await;

    assert!(matches!(result.outcome, Outcome::Success(ref s) if s.trim() == "works"));
}

#[tokio::test]
async fn registry_returns_error_for_unknown_tool() {
    let registry = ToolRegistry::new();

    let result = registry.execute("nonexistent", &HashMap::new()).await;
    assert!(matches!(result.outcome, Outcome::Error(ref s) if s.contains("unknown tool")));
}

#[tokio::test]
async fn registry_unregister_removes_tool() {
    let registry = ToolRegistry::new();
    registry.register(Arc::new(ShellTool)).await;

    assert_eq!(registry.descriptions().await.len(), 1);

    registry.unregister("shell").await;

    assert_eq!(registry.descriptions().await.len(), 0);
}
