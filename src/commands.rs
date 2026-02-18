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
    /// Exit the REPL.
    Quit,
}

/// A built-in command definition.
struct Command {
    name: &'static str,
    aliases: &'static [&'static str],
    description: &'static str,
    run: fn(&SessionInfo) -> CommandResult,
}

/// Async commands need separate handling since fn pointers can't be async.
struct AsyncCommand {
    name: &'static str,
    aliases: &'static [&'static str],
    description: &'static str,
}

// --- Sync commands ---

const COMMANDS: &[Command] = &[
    Command {
        name: "/help",
        aliases: &["/h", "/?"],
        description: "show this help",
        run: cmd_help,
    },
    Command {
        name: "/whoami",
        aliases: &[],
        description: "show provider, model, and auth status",
        run: cmd_whoami,
    },
    Command {
        name: "/tools",
        aliases: &[],
        description: "list registered tools",
        run: cmd_tools,
    },
    Command {
        name: "/tokens",
        aliases: &[],
        description: "show session token usage",
        run: cmd_tokens,
    },
    Command {
        name: "/logout",
        aliases: &[],
        description: "log out from the current provider",
        run: cmd_logout,
    },
    Command {
        name: "/quit",
        aliases: &["quit", "exit", "/exit"],
        description: "exit the REPL",
        run: cmd_quit,
    },
];

const ASYNC_COMMANDS: &[AsyncCommand] = &[AsyncCommand {
    name: "/login",
    aliases: &[],
    description: "log in to the current provider",
}];

/// Try to handle input as a built-in command.
pub async fn handle_command(input: &str, info: &SessionInfo<'_>) -> CommandResult {
    let cmd = input.trim();

    // Check sync commands
    for command in COMMANDS {
        if cmd == command.name || command.aliases.contains(&cmd) {
            return (command.run)(info);
        }
    }

    // Check async commands
    for async_cmd in ASYNC_COMMANDS {
        if cmd == async_cmd.name || async_cmd.aliases.contains(&cmd) {
            return match async_cmd.name {
                "/login" => cmd_login().await,
                _ => CommandResult::Handled,
            };
        }
    }

    // Unknown slash command
    if cmd.starts_with('/') {
        println!("unknown command: {cmd}");
        println!("type /help for available commands");
        return CommandResult::Handled;
    }

    CommandResult::NotACommand
}

// --- Command implementations ---

fn cmd_help(_info: &SessionInfo) -> CommandResult {
    // Find the longest command name for alignment
    let max_width = COMMANDS
        .iter()
        .map(|c| format_command_name(c.name, c.aliases).len())
        .chain(
            ASYNC_COMMANDS
                .iter()
                .map(|c| format_command_name(c.name, c.aliases).len()),
        )
        .max()
        .unwrap_or(10);

    for command in COMMANDS {
        let name = format_command_name(command.name, command.aliases);
        println!("  {name:<max_width$}  {}", command.description);
    }
    for command in ASYNC_COMMANDS {
        let name = format_command_name(command.name, command.aliases);
        println!("  {name:<max_width$}  {}", command.description);
    }
    CommandResult::Handled
}

fn format_command_name(name: &str, aliases: &[&str]) -> String {
    if aliases.is_empty() {
        name.to_string()
    } else {
        format!("{} ({})", name, aliases.join(", "))
    }
}

fn cmd_whoami(info: &SessionInfo) -> CommandResult {
    println!("  provider  {} ({})", info.provider, info.model);
    println!("  auth      {}", info.auth_status);
    println!("  shell     {}", info.shell_mode);
    CommandResult::Handled
}

fn cmd_tools(info: &SessionInfo) -> CommandResult {
    if info.tools.is_empty() {
        println!("  (no tools registered)");
    } else {
        for tool in info.tools {
            println!("  {tool}");
        }
    }
    CommandResult::Handled
}

fn cmd_tokens(info: &SessionInfo) -> CommandResult {
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
    CommandResult::Handled
}

fn cmd_quit(_info: &SessionInfo) -> CommandResult {
    CommandResult::Quit
}

fn cmd_logout(_info: &SessionInfo) -> CommandResult {
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

async fn cmd_login() -> CommandResult {
    println!("Logging in to Anthropic...\n");

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
            handle_command("list files", &test_info()).await,
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
        assert!(matches!(
            handle_command("/logout", &test_info()).await,
            CommandResult::AuthChanged(_)
        ));
    }

    #[tokio::test]
    async fn quit_returns_quit() {
        assert!(matches!(
            handle_command("/quit", &test_info()).await,
            CommandResult::Quit
        ));
    }

    #[tokio::test]
    async fn quit_aliases_work() {
        assert!(matches!(
            handle_command("quit", &test_info()).await,
            CommandResult::Quit
        ));
        assert!(matches!(
            handle_command("exit", &test_info()).await,
            CommandResult::Quit
        ));
        assert!(matches!(
            handle_command("/exit", &test_info()).await,
            CommandResult::Quit
        ));
    }

    #[test]
    fn all_commands_registered() {
        let all_names: Vec<&str> = COMMANDS
            .iter()
            .map(|c| c.name)
            .chain(ASYNC_COMMANDS.iter().map(|c| c.name))
            .collect();
        assert!(all_names.contains(&"/help"));
        assert!(all_names.contains(&"/whoami"));
        assert!(all_names.contains(&"/tools"));
        assert!(all_names.contains(&"/tokens"));
        assert!(all_names.contains(&"/login"));
        assert!(all_names.contains(&"/logout"));
        assert!(all_names.contains(&"/quit"));
    }

    #[test]
    fn no_duplicate_names_or_aliases() {
        let mut seen: Vec<&str> = Vec::new();
        for cmd in COMMANDS {
            assert!(!seen.contains(&cmd.name), "duplicate: {}", cmd.name);
            seen.push(cmd.name);
            for alias in cmd.aliases {
                assert!(!seen.contains(alias), "duplicate alias: {alias}");
                seen.push(alias);
            }
        }
        for cmd in ASYNC_COMMANDS {
            assert!(!seen.contains(&cmd.name), "duplicate: {}", cmd.name);
            seen.push(cmd.name);
            for alias in cmd.aliases {
                assert!(!seen.contains(alias), "duplicate alias: {alias}");
                seen.push(alias);
            }
        }
    }

    #[test]
    fn format_command_name_no_aliases() {
        assert_eq!(format_command_name("/whoami", &[]), "/whoami");
    }

    #[test]
    fn format_command_name_with_aliases() {
        assert_eq!(
            format_command_name("/help", &["/h", "/?"]),
            "/help (/h, /?)"
        );
    }
}
