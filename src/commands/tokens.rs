use async_trait::async_trait;

use super::{Command, CommandResult, SessionInfo};
use crate::consts::format_number;

pub struct TokensCommand;

#[async_trait]
impl Command for TokensCommand {
    fn name(&self) -> &str {
        "/tokens"
    }

    fn description(&self) -> &str {
        "show session token usage"
    }

    async fn execute(&self, info: &SessionInfo<'_>) -> CommandResult {
        if info.usage.total() == 0 {
            println!("  no tokens used this session");
        } else {
            println!(
                "  {} input + {} output = {} total",
                format_number(info.usage.input_tokens),
                format_number(info.usage.output_tokens),
                format_number(info.usage.total()),
            );
        }
        CommandResult::Handled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::tests::test_info;
    use crate::thinker::TokenUsage;

    #[tokio::test]
    async fn returns_handled_zero() {
        assert!(matches!(
            TokensCommand.execute(&test_info()).await,
            CommandResult::Handled
        ));
    }

    #[tokio::test]
    async fn returns_handled_with_usage() {
        let info = SessionInfo {
            usage: TokenUsage {
                input_tokens: 1234,
                output_tokens: 567,
            },
            ..test_info()
        };
        assert!(matches!(
            TokensCommand.execute(&info).await,
            CommandResult::Handled
        ));
    }
}
