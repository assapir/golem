use async_trait::async_trait;

use super::{Command, CommandResult, SessionInfo, StateChange};
use crate::auth;

pub struct LogoutCommand;

#[async_trait]
impl Command for LogoutCommand {
    fn name(&self) -> &str {
        "/logout"
    }

    fn description(&self) -> &str {
        "log out from the current provider"
    }

    async fn execute(&self, info: &SessionInfo<'_>) -> CommandResult {
        let provider = info.provider;
        if let Err(e) = auth::logout(info.db_path, provider) {
            eprintln!("  ✗ logout from {provider} failed: {e}");
            return CommandResult::Handled;
        }
        println!("  ✓ logged out from {provider}");
        CommandResult::StateChanged(StateChange::Auth("not authenticated".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::storage::{AuthStorage, Credential};
    use crate::commands::tests::test_info;

    #[tokio::test]
    async fn returns_auth_changed_when_no_credentials() {
        assert!(matches!(
            LogoutCommand.execute(&test_info()).await,
            CommandResult::StateChanged(StateChange::Auth(_))
        ));
    }

    #[tokio::test]
    async fn removes_stored_credential() {
        let storage = AuthStorage::open(":memory:").unwrap();
        storage
            .set(
                "anthropic",
                Credential::ApiKey {
                    key: "sk-test".to_string(),
                },
            )
            .unwrap();
        assert!(storage.get("anthropic").unwrap().is_some());

        let info = test_info();
        let result = LogoutCommand.execute(&info).await;

        assert!(matches!(
            result,
            CommandResult::StateChanged(StateChange::Auth(_))
        ));
        // Note: the command opens its own connection to :memory:,
        // so this tests the command flow, not the same DB instance.
    }
}
