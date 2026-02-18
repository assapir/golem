//! Built-in REPL commands prefixed with `/`.

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

/// Try to handle input as a built-in command. Returns `true` if handled.
pub fn handle_command(input: &str, info: &SessionInfo) -> bool {
    let cmd = input.trim();
    match cmd {
        "/help" | "/h" | "/?" => {
            cmd_help();
            true
        }
        "/whoami" => {
            cmd_whoami(info);
            true
        }
        "/tools" => {
            cmd_tools(info);
            true
        }
        "/tokens" => {
            cmd_tokens(info);
            true
        }
        _ if cmd.starts_with('/') => {
            println!("unknown command: {cmd}");
            println!("type /help for available commands");
            true
        }
        _ => false,
    }
}

fn cmd_help() {
    println!(
        "\
  /help     show this help
  /whoami   show provider, model, and auth status
  /tools    list registered tools
  /tokens   show session token usage
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

    #[test]
    fn help_is_handled() {
        assert!(handle_command("/help", &test_info()));
        assert!(handle_command("/h", &test_info()));
        assert!(handle_command("/?", &test_info()));
    }

    #[test]
    fn whoami_is_handled() {
        assert!(handle_command("/whoami", &test_info()));
    }

    #[test]
    fn tools_is_handled() {
        assert!(handle_command("/tools", &test_info()));
    }

    #[test]
    fn tokens_is_handled() {
        assert!(handle_command("/tokens", &test_info()));
    }

    #[test]
    fn unknown_slash_command_is_handled() {
        assert!(handle_command("/foobar", &test_info()));
    }

    #[test]
    fn non_command_is_not_handled() {
        assert!(!handle_command("hello world", &test_info()));
        assert!(!handle_command("quit", &test_info()));
    }

    #[test]
    fn tokens_with_usage() {
        let info = SessionInfo {
            usage: TokenUsage {
                input_tokens: 1234,
                output_tokens: 567,
            },
            ..test_info()
        };
        assert!(handle_command("/tokens", &info));
    }
}
