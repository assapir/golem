//! Built-in REPL commands prefixed with `/`.

use crate::auth::oauth;
use crate::auth::storage::{AuthStorage, Credential};
use crate::consts::format_number;
use crate::thinker::TokenUsage;

/// Session info available to built-in commands.
pub struct SessionInfo<'a> {
    pub provider: &'a str,
    pub model: &'a str,
    pub auth_status: &'a str,
    pub shell_mode: &'a str,
    pub tools: &'a [String],
    pub usage: TokenUsage,
}

/// Result of command handling.
pub enum CommandResult {
    /// Not a command — pass input to the thinker.
    NotACommand,
    /// Command handled, continue the REPL loop.
    Handled,
    /// Auth changed — caller should update auth status display.
    AuthChanged(String),
}

/// Try to handle input as a built-in command.
pub async fn handle_command(input: &str, info: &SessionInfo<'_>) -> CommandResult {
    let cmd = input.trim();
    match cmd {
        "/help" | "/h" | "/?" => {
            cmd_help();
            CommandResult::Handled
        }
        "/whoami" => {
            cmd_whoami(info);
            CommandResult::Handled
        }
        "/tools" => {
            cmd_tools(info);
            CommandResult::Handled
        }
        "/tokens" => {
            cmd_tokens(info);
            CommandResult::Handled
        }
        "/login" => cmd_login().await,
        "/logout" => cmd_logout(),
        _ if cmd.starts_with('/') => {
            println!("unknown command: {cmd}");
            println!("type /help for available commands");
            CommandResult::Handled
        }
        _ => CommandResult::NotACommand,
    }
}

fn cmd_help() {
    println!(
        "\
  /help     show this help
  /whoami   show provider, model, and auth status
  /tools    list registered tools
  /tokens   show session token usage
  /login    log in to the current provider
  /logout   log out from the current provider
  quit      exit the REPL"
    );
}

fn cmd_whoami(info: &SessionInfo) {
    println!("  provider  {} ({})", info.provider, info.model);
    println!("  auth      {}", info.auth_status);
    println!("  shell     {}", info.shell_mode);
}

fn cmd_tools(info: &SessionInfo) {
    if info.tools.is_empty() {
        println!("  (no tools registered)");
    } else {
        for tool in info.tools {
            println!("  {tool}");
        }
    }
}

fn cmd_tokens(info: &SessionInfo) {
    if info.usage.total() == 0 {
        println!("  no tokens used this session");
    } else {
        println!(
            "  {} input + {} output = {} total",
            format_number(info.usage.input_tokens),
            format_number(info.usage.output_tokens),
            format_number(info.usage.total()),
        );
    }
}

async fn cmd_login() -> CommandResult {
    println!("Logging in to Anthropic...\n");

    let (url, verifier) = oauth::build_authorize_url();
    let _ = open::that(&url);

    println!("Open this URL to authenticate:\n");
    println!("  {url}\n");

    // Read auth code from stdin (blocking is fine here — user is interacting)
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
            if let Err(e) = storage.set("anthropic", Credential::OAuth(credentials)) {
                eprintln!("  ✗ failed to save credentials: {e}");
                return CommandResult::Handled;
            }
            println!("  ✓ logged in to Anthropic");
            CommandResult::AuthChanged("OAuth ✓".to_string())
        }
        Err(e) => {
            eprintln!("  ✗ login failed: {e}");
            CommandResult::Handled
        }
    }
}

fn cmd_logout() -> CommandResult {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_info() -> SessionInfo<'static> {
        SessionInfo {
            provider: "anthropic",
            model: "claude-sonnet-4-20250514",
            auth_status: "OAuth ✓",
            shell_mode: "read-only",
            tools: &[],
            usage: TokenUsage::default(),
        }
    }

    #[tokio::test]
    async fn help_is_handled() {
        assert!(matches!(
            handle_command("/help", &test_info()).await,
            CommandResult::Handled
        ));
        assert!(matches!(
            handle_command("/h", &test_info()).await,
            CommandResult::Handled
        ));
        assert!(matches!(
            handle_command("/?", &test_info()).await,
            CommandResult::Handled
        ));
    }

    #[tokio::test]
    async fn whoami_is_handled() {
        assert!(matches!(
            handle_command("/whoami", &test_info()).await,
            CommandResult::Handled
        ));
    }

    #[tokio::test]
    async fn tools_is_handled() {
        assert!(matches!(
            handle_command("/tools", &test_info()).await,
            CommandResult::Handled
        ));
    }

    #[tokio::test]
    async fn tokens_is_handled() {
        assert!(matches!(
            handle_command("/tokens", &test_info()).await,
            CommandResult::Handled
        ));
    }

    #[tokio::test]
    async fn unknown_slash_command_is_handled() {
        assert!(matches!(
            handle_command("/foobar", &test_info()).await,
            CommandResult::Handled
        ));
    }

    #[tokio::test]
    async fn non_command_is_not_handled() {
        assert!(matches!(
            handle_command("hello world", &test_info()).await,
            CommandResult::NotACommand
        ));
        assert!(matches!(
            handle_command("quit", &test_info()).await,
            CommandResult::NotACommand
        ));
    }

    #[tokio::test]
    async fn tokens_with_usage() {
        let info = SessionInfo {
            usage: TokenUsage {
                input_tokens: 1234,
                output_tokens: 567,
            },
            ..test_info()
        };
        assert!(matches!(
            handle_command("/tokens", &info).await,
            CommandResult::Handled
        ));
    }

    #[tokio::test]
    async fn logout_returns_auth_changed() {
        // Logout always succeeds (even if no credentials)
        let info = test_info();
        assert!(matches!(
            handle_command("/logout", &info).await,
            CommandResult::AuthChanged(_)
        ));
    }
}
