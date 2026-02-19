use async_trait::async_trait;

use super::{Command, CommandResult, SessionInfo, StateChange};
use crate::auth;
use crate::auth::oauth;

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
        println!("Logging in to {provider}...\n");

        let (url, verifier) = oauth::build_authorize_url();
        let _ = open::that(&url);

        println!("Open this URL to authenticate:\n");
        println!("  {url}\n");

        print!("Paste the authorization code: ");
        if std::io::Write::flush(&mut std::io::stdout()).is_err() {
            return CommandResult::Handled;
        }

        let mut code = String::new();
        if std::io::stdin().read_line(&mut code).is_err() {
            eprintln!("  ✗ failed to read input");
            return CommandResult::Handled;
        }
        let code = code.trim();

        if code.is_empty() {
            eprintln!("  ✗ no authorization code provided");
            return CommandResult::Handled;
        }

        println!("\nExchanging code for tokens...");
        match auth::login(info.db_path, provider, code, &verifier).await {
            Ok(()) => {
                println!("  ✓ logged in to {provider}");
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
