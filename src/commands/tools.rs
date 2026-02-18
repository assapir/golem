use async_trait::async_trait;

use super::{Command, CommandResult, SessionInfo};

pub struct ToolsCommand;

#[async_trait]
impl Command for ToolsCommand {
    fn name(&self) -> &str {
        "/tools"
    }

    fn description(&self) -> &str {
        "list registered tools"
    }

    async fn execute(&self, info: &SessionInfo<'_>) -> CommandResult {
        if info.tools.is_empty() {
            println!("  (no tools registered)");
        } else {
            for tool in info.tools {
                println!("  {tool}");
            }
        }
        CommandResult::Handled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::tests::test_info;

    #[tokio::test]
    async fn returns_handled_empty() {
        assert!(matches!(
            ToolsCommand.execute(&test_info()).await,
            CommandResult::Handled
        ));
    }

    #[tokio::test]
    async fn returns_handled_with_tools() {
        let tool_names = vec!["shell — Execute shell commands".to_string()];
        let info = SessionInfo {
            tools: &tool_names,
            ..test_info()
        };
        assert!(matches!(
            ToolsCommand.execute(&info).await,
            CommandResult::Handled
        ));
    }
}
