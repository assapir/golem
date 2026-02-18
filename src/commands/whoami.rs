use async_trait::async_trait;

use super::{Command, CommandResult, SessionInfo};

pub struct WhoamiCommand;

#[async_trait]
impl Command for WhoamiCommand {
    fn name(&self) -> &str {
        "/whoami"
    }

    fn description(&self) -> &str {
        "show provider, model, and auth status"
    }

    async fn execute(&self, info: &SessionInfo<'_>) -> CommandResult {
        println!("  provider  {} ({})", info.provider, info.model);
        println!("  auth      {}", info.auth_status);
        println!("  shell     {}", info.shell_mode);
        CommandResult::Handled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::tests::test_info;

    #[tokio::test]
    async fn returns_handled() {
        assert!(matches!(
            WhoamiCommand.execute(&test_info()).await,
            CommandResult::Handled
        ));
    }

    #[test]
    fn metadata() {
        assert_eq!(WhoamiCommand.name(), "/whoami");
        assert!(WhoamiCommand.aliases().is_empty());
    }
}
