//! Built-in REPL commands prefixed with `/`.
//!
//! Commands implement the [`Command`] trait and are registered in a
//! [`CommandRegistry`]. The registry handles dispatch, alias resolution,
//! and dynamic help generation. Plugins can register additional commands
//! at runtime via `registry.register(Arc::new(MyCommand))`.

mod help;
mod login;
mod logout;
mod model;
mod quit;
mod tokens;
mod tools;
mod whoami;

use async_trait::async_trait;
use std::sync::Arc;

use crate::engine::react::ReactEngine;
use crate::thinker::TokenUsage;

/// Session info available to commands during execution.
pub struct SessionInfo<'a> {
    pub provider: &'a str,
    pub model: &'a str,
    pub auth_status: &'a str,
    pub shell_mode: &'a str,
    pub tools: &'a [String],
    pub usage: TokenUsage,
    pub db_path: &'a str,
    /// Engine reference for commands that need provider access (e.g. `/model`).
    pub engine: Option<&'a ReactEngine>,
}

/// A state change the REPL needs to apply after a command runs.
#[derive(Debug, Clone)]
pub enum StateChange {
    /// Auth status changed (new status string).
    Auth(String),
    /// Active model changed (new model ID).
    Model(String),
}

/// What the REPL should do after a command runs.
pub enum CommandResult {
    /// Not a command — pass input to the thinker.
    NotACommand,
    /// Command handled, continue the REPL loop.
    Handled,
    /// Command produced a state change the REPL must apply.
    StateChanged(StateChange),
    /// Exit the REPL.
    Quit,
}

/// A REPL command. Implement this trait to add new commands.
#[async_trait]
pub trait Command: Send + Sync {
    /// Primary name, e.g. `"/whoami"`.
    fn name(&self) -> &str;

    /// Alternative names, e.g. `&["/h", "/?"]`.
    fn aliases(&self) -> &[&str] {
        &[]
    }

    /// One-line description for `/help`.
    fn description(&self) -> &str;

    /// Run the command.
    async fn execute(&self, info: &SessionInfo<'_>) -> CommandResult;
}

/// Holds registered commands. Supports runtime registration for plugins.
pub struct CommandRegistry {
    commands: Vec<Arc<dyn Command>>,
}

impl CommandRegistry {
    /// Create a registry with all built-in commands.
    pub fn new() -> Self {
        let commands: Vec<Arc<dyn Command>> = vec![
            Arc::new(help::HelpCommand),
            Arc::new(whoami::WhoamiCommand),
            Arc::new(tools::ToolsCommand),
            Arc::new(tokens::TokensCommand),
            Arc::new(model::ModelCommand),
            Arc::new(login::LoginCommand),
            Arc::new(logout::LogoutCommand),
            Arc::new(quit::QuitCommand),
        ];
        Self { commands }
    }

    /// Register an additional command (e.g. from a plugin).
    pub fn register(&mut self, command: Arc<dyn Command>) {
        self.commands.push(command);
    }

    /// Dispatch input to a matching command, or return `NotACommand`.
    pub async fn dispatch(&self, input: &str, info: &SessionInfo<'_>) -> CommandResult {
        let cmd = input.trim();

        for command in &self.commands {
            if cmd == command.name() || command.aliases().contains(&cmd) {
                // /help is special — it needs the registry to list all commands
                if command.name() == "/help" {
                    print!("{}", self.help_text());
                    return CommandResult::Handled;
                }
                return command.execute(info).await;
            }
        }

        if cmd.starts_with('/') {
            println!("unknown command: {cmd}");
            println!("type /help for available commands");
            return CommandResult::Handled;
        }

        CommandResult::NotACommand
    }

    /// Generate help text from all registered commands.
    pub fn help_text(&self) -> String {
        let entries: Vec<(String, &str)> = self
            .commands
            .iter()
            .map(|c| (format_label(c.name(), c.aliases()), c.description()))
            .collect();

        let max_width = entries
            .iter()
            .map(|(label, _)| label.len())
            .max()
            .unwrap_or(10);

        let mut out = String::new();
        for (label, desc) in &entries {
            out.push_str(&format!("  {label:<max_width$}  {desc}\n"));
        }
        out
    }

    /// All registered command names (for testing).
    pub fn names(&self) -> Vec<&str> {
        self.commands.iter().map(|c| c.name()).collect()
    }

    /// All registered names and aliases (for duplicate detection).
    pub fn all_triggers(&self) -> Vec<&str> {
        let mut triggers = Vec::new();
        for cmd in &self.commands {
            triggers.push(cmd.name());
            triggers.extend_from_slice(cmd.aliases());
        }
        triggers
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn format_label(name: &str, aliases: &[&str]) -> String {
    if aliases.is_empty() {
        name.to_string()
    } else {
        format!("{} ({})", name, aliases.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn test_info() -> SessionInfo<'static> {
        SessionInfo {
            provider: "anthropic",
            model: "claude-sonnet-4-20250514",
            auth_status: "OAuth ✓",
            shell_mode: "read-only",
            tools: &[],
            usage: TokenUsage::default(),
            db_path: ":memory:",
            engine: None,
        }
    }

    #[test]
    fn all_builtins_registered() {
        let reg = CommandRegistry::new();
        let names = reg.names();
        assert!(names.contains(&"/help"));
        assert!(names.contains(&"/whoami"));
        assert!(names.contains(&"/tools"));
        assert!(names.contains(&"/tokens"));
        assert!(names.contains(&"/model"));
        assert!(names.contains(&"/login"));
        assert!(names.contains(&"/logout"));
        assert!(names.contains(&"/quit"));
    }

    #[test]
    fn no_duplicate_triggers() {
        let reg = CommandRegistry::new();
        let triggers = reg.all_triggers();
        let mut seen = Vec::new();
        for t in &triggers {
            assert!(!seen.contains(t), "duplicate trigger: {t}");
            seen.push(t);
        }
    }

    #[test]
    fn help_text_includes_all_commands() {
        let reg = CommandRegistry::new();
        let text = reg.help_text();
        for name in reg.names() {
            assert!(text.contains(name), "help missing: {name}");
        }
    }

    #[test]
    fn help_text_includes_aliases() {
        let reg = CommandRegistry::new();
        let text = reg.help_text();
        assert!(text.contains("/h"));
        assert!(text.contains("/?"));
    }

    #[tokio::test]
    async fn unknown_slash_command_is_handled() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("/foobar", &test_info()).await,
            CommandResult::Handled
        ));
    }

    #[tokio::test]
    async fn non_command_passes_through() {
        let reg = CommandRegistry::new();
        assert!(matches!(
            reg.dispatch("hello world", &test_info()).await,
            CommandResult::NotACommand
        ));
        assert!(matches!(
            reg.dispatch("list files", &test_info()).await,
            CommandResult::NotACommand
        ));
    }

    #[tokio::test]
    async fn plugin_command_works() {
        struct PingCommand;

        #[async_trait]
        impl Command for PingCommand {
            fn name(&self) -> &str {
                "/ping"
            }
            fn description(&self) -> &str {
                "pong"
            }
            async fn execute(&self, _info: &SessionInfo<'_>) -> CommandResult {
                CommandResult::Handled
            }
        }

        let mut reg = CommandRegistry::new();
        reg.register(Arc::new(PingCommand));
        assert!(reg.names().contains(&"/ping"));
        assert!(matches!(
            reg.dispatch("/ping", &test_info()).await,
            CommandResult::Handled
        ));
        assert!(reg.help_text().contains("/ping"));
    }

    #[test]
    fn format_label_no_aliases() {
        assert_eq!(format_label("/whoami", &[]), "/whoami");
    }

    #[test]
    fn format_label_with_aliases() {
        assert_eq!(format_label("/help", &["/h", "/?"]), "/help (/h, /?)");
    }
}
