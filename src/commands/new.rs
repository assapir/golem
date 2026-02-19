use async_trait::async_trait;

use super::{Command, CommandResult, SessionInfo};

pub struct NewCommand;

#[async_trait]
impl Command for NewCommand {
    fn name(&self) -> &str {
        "/new"
    }

    fn description(&self) -> &str {
        "start a new session (clear conversation history)"
    }

    async fn execute(&self, info: &SessionInfo<'_>) -> CommandResult {
        let engine = match info.engine {
            Some(e) => e,
            None => {
                eprintln!("  ✗ session reset not available");
                return CommandResult::Handled;
            }
        };

        if let Err(e) = engine.clear_session().await {
            eprintln!("  ✗ failed to clear session: {e}");
            return CommandResult::Handled;
        }

        println!("  ✓ session history cleared");
        CommandResult::Handled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata() {
        assert_eq!(NewCommand.name(), "/new");
        assert!(NewCommand.aliases().is_empty());
        assert!(!NewCommand.description().is_empty());
    }

    #[tokio::test]
    async fn returns_handled_without_engine() {
        let info = super::super::tests::test_info();
        let result = NewCommand.execute(&info).await;
        assert!(matches!(result, CommandResult::Handled));
    }
}
