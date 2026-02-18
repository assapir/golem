use async_trait::async_trait;

use super::{Command, CommandResult, SessionInfo};
use crate::auth::storage::AuthStorage;

pub struct LogoutCommand;

#[async_trait]
impl Command for LogoutCommand {
    fn name(&self) -> &str {
        "/logout"
    }

    fn description(&self) -> &str {
        "log out from the current provider"
    }

    async fn execute(&self, _info: &SessionInfo<'_>) -> CommandResult {
        let storage = match AuthStorage::new() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("  ✗ failed to open auth storage: {e}");
                return CommandResult::Handled;
            }
        };
        if let Err(e) = storage.remove("anthropic") {
            eprintln!("  ✗ failed to remove credentials: {e}");
            return CommandResult::Handled;
        }
        println!("  ✓ logged out from Anthropic");
        CommandResult::AuthChanged("not authenticated".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::tests::test_info;

    #[tokio::test]
    async fn returns_auth_changed() {
        assert!(matches!(
            LogoutCommand.execute(&test_info()).await,
            CommandResult::AuthChanged(_)
        ));
    }
}
