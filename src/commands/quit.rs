use async_trait::async_trait;

use super::{Command, CommandResult, SessionInfo};

pub struct QuitCommand;

#[async_trait]
impl Command for QuitCommand {
    fn name(&self) -> &str {
        "/quit"
    }

    fn aliases(&self) -> &[&str] {
        &["quit", "exit", "/exit"]
    }

    fn description(&self) -> &str {
        "exit the REPL"
    }

    async fn execute(&self, _info: &SessionInfo<'_>) -> CommandResult {
        CommandResult::Quit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::tests::test_info;

    #[tokio::test]
    async fn returns_quit() {
        assert!(matches!(
            QuitCommand.execute(&test_info()).await,
            CommandResult::Quit
        ));
    }

    #[test]
    fn has_aliases() {
        let aliases = QuitCommand.aliases();
        assert!(aliases.contains(&"quit"));
        assert!(aliases.contains(&"exit"));
        assert!(aliases.contains(&"/exit"));
    }
}
