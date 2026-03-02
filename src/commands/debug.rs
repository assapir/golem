use async_trait::async_trait;

use super::{Command, CommandResult, SessionInfo};

pub struct DebugCommand;

#[async_trait]
impl Command for DebugCommand {
    fn name(&self) -> &str {
        "/debug"
    }

    fn description(&self) -> &str {
        "toggle debug mode (show raw LLM request/response data)"
    }

    async fn execute(&self, info: &SessionInfo<'_>) -> CommandResult {
        let new_state = info.debug.toggle();
        if new_state {
            println!("  debug mode: on");
        } else {
            println!("  debug mode: off");
        }
        CommandResult::Handled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata() {
        assert_eq!(DebugCommand.name(), "/debug");
        assert!(DebugCommand.aliases().is_empty());
        assert!(!DebugCommand.description().is_empty());
    }
}
