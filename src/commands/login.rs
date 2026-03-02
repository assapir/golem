use async_trait::async_trait;

use super::{Command, CommandResult, SessionInfo, StateChange};
use crate::provider::provider_config_by_id;

pub struct LoginCommand;

#[async_trait]
impl Command for LoginCommand {
    fn name(&self) -> &str {
        "/login"
    }

    fn description(&self) -> &str {
        "log in to the current provider"
    }

    async fn execute(&self, info: &SessionInfo<'_>) -> CommandResult {
        let provider = info.provider;

        let Some(config) = provider_config_by_id(provider) else {
            eprintln!("  ✗ provider {provider} does not support login");
            return CommandResult::Handled;
        };

        println!("Logging in to {}...\n", config.display_name());

        match config.login(info.db_path).await {
            Ok(()) => {
                println!("  ✓ logged in to {}", config.display_name());
                CommandResult::StateChanged(StateChange::Auth("OAuth ✓".to_string()))
            }
            Err(e) => {
                eprintln!("  ✗ login failed: {e}");
                CommandResult::Handled
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata() {
        assert_eq!(LoginCommand.name(), "/login");
        assert!(LoginCommand.aliases().is_empty());
        assert!(!LoginCommand.description().is_empty());
    }
}
