use async_trait::async_trait;

use super::{Command, CommandResult, SessionInfo};
use crate::auth::oauth;
use crate::auth::storage::{AuthStorage, Credential};

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
        match oauth::exchange_code(code, &verifier).await {
            Ok(credentials) => {
                let storage = match AuthStorage::new() {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("  ✗ failed to open auth storage: {e}");
                        return CommandResult::Handled;
                    }
                };
                if let Err(e) = storage.set(provider, Credential::OAuth(credentials)) {
                    eprintln!("  ✗ failed to save credentials: {e}");
                    return CommandResult::Handled;
                }
                println!("  ✓ logged in to {provider}");
                CommandResult::AuthChanged("OAuth ✓".to_string())
            }
            Err(e) => {
                eprintln!("  ✗ login failed: {e}");
                CommandResult::Handled
            }
        }
    }
}
