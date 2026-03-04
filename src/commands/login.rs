use async_trait::async_trait;
use tokio::io::AsyncBufReadExt;

use super::{Command, CommandResult, SessionInfo, StateChange};
use crate::commands::parse_menu_choice;
use crate::provider::all_login_providers;

pub struct LoginCommand;

#[async_trait]
impl Command for LoginCommand {
    fn name(&self) -> &str {
        "/login"
    }

    fn description(&self) -> &str {
        "log in to a provider (choose from list)"
    }

    async fn execute(&self, info: &SessionInfo<'_>) -> CommandResult {
        let providers = all_login_providers();

        println!("  Choose a provider:\n");
        for (i, config) in providers.iter().enumerate() {
            let marker = if config.id() == info.provider {
                " ← current"
            } else {
                ""
            };
            println!("  {}. {}{}", i + 1, config.display_name(), marker);
        }

        print!("\n  Select provider: ");
        if std::io::Write::flush(&mut std::io::stdout()).is_err() {
            return CommandResult::Handled;
        }

        let stdin = tokio::io::BufReader::new(tokio::io::stdin());
        let mut lines = stdin.lines();
        let input = match lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) | Err(_) => {
                return CommandResult::Handled;
            }
        };
        let input = input.trim().to_string();

        if input.is_empty() {
            return CommandResult::Handled;
        }

        let choice = match parse_menu_choice(&input, providers.len()) {
            Some(n) => n,
            None => {
                eprintln!("  ✗ invalid selection: {input}");
                return CommandResult::Handled;
            }
        };

        let config = &providers[choice - 1];
        println!("\nLogging in to {}...\n", config.display_name());

        match config.login(info.db_path).await {
            Ok(()) => {
                println!("  ✓ logged in to {}", config.display_name());
                if config.id() == info.provider {
                    // Same provider — just update auth status
                    CommandResult::StateChanged(StateChange::Auth("OAuth ✓".to_string()))
                } else {
                    // Different provider — switch to it
                    CommandResult::StateChanged(StateChange::Provider(config.id().to_string()))
                }
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
