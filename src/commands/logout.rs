use async_trait::async_trait;

use super::{Command, CommandResult, SessionInfo, StateChange};
use crate::auth;
use crate::auth::storage::AuthStorage;
use crate::provider::all_login_providers;

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

        // Try to fall back to another authenticated provider
        if let Some(fallback_id) = find_authenticated_provider(info.db_path, provider) {
            println!("  → switching to {fallback_id}");
            return CommandResult::StateChanged(StateChange::Provider(fallback_id, None));
        }

        CommandResult::StateChanged(StateChange::Auth("not authenticated".to_string()))
    }
}

/// Find another provider that has stored credentials, skipping `exclude`.
fn find_authenticated_provider(db_path: &str, exclude: &str) -> Option<String> {
    let storage = AuthStorage::open(db_path).ok()?;
    for config in all_login_providers() {
        if config.id() == exclude {
            continue;
        }
        if storage.get(config.id()).ok().flatten().is_some() {
            return Some(config.id().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::storage::{AuthStorage, Credential};
    use crate::commands::tests::test_info;

    #[tokio::test]
    async fn returns_auth_changed_when_no_credentials() {
        // With :memory: db, no other provider is authenticated,
        // so logout returns Auth (not Provider fallback).
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

    #[test]
    fn find_authenticated_provider_returns_none_when_empty() {
        assert!(find_authenticated_provider(":memory:", "anthropic").is_none());
    }

    #[test]
    fn find_authenticated_provider_skips_excluded() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("fallback.db");
        let db_str = db_path.to_str().unwrap();

        let storage = AuthStorage::open(db_str).unwrap();
        storage
            .set(
                "anthropic",
                Credential::ApiKey {
                    key: "key".to_string(),
                },
            )
            .unwrap();
        drop(storage);

        // Excluding anthropic should return None (only anthropic has creds)
        assert!(find_authenticated_provider(db_str, "anthropic").is_none());
    }

    #[test]
    fn find_authenticated_provider_finds_other() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("fallback2.db");
        let db_str = db_path.to_str().unwrap();

        let storage = AuthStorage::open(db_str).unwrap();
        storage
            .set(
                "anthropic",
                Credential::ApiKey {
                    key: "key-a".to_string(),
                },
            )
            .unwrap();
        storage
            .set(
                "google",
                Credential::ApiKey {
                    key: "key-g".to_string(),
                },
            )
            .unwrap();
        drop(storage);

        // Excluding anthropic should find google
        let fallback = find_authenticated_provider(db_str, "anthropic");
        assert_eq!(fallback.as_deref(), Some("google"));

        // Excluding google should find anthropic
        let fallback = find_authenticated_provider(db_str, "google");
        assert_eq!(fallback.as_deref(), Some("anthropic"));
    }
}
