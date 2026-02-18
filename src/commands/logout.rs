use async_trait::async_trait;

use super::{Command, CommandResult, SessionInfo};
use crate::auth::storage::AuthStorage;
use crate::consts::default_db_path;

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
        let db_path = default_db_path();
        let storage = match AuthStorage::open(&db_path.to_string_lossy()) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("  ✗ failed to open auth storage: {e}");
                return CommandResult::Handled;
            }
        };
        if let Err(e) = storage.remove(provider) {
            eprintln!("  ✗ failed to remove credentials: {e}");
            return CommandResult::Handled;
        }
        println!("  ✓ logged out from {provider}");
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
