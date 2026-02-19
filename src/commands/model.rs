use async_trait::async_trait;

use super::{Command, CommandResult, SessionInfo};

pub struct ModelCommand;

#[async_trait]
impl Command for ModelCommand {
    fn name(&self) -> &str {
        "/model"
    }

    fn description(&self) -> &str {
        "list and switch the active model"
    }

    async fn execute(&self, info: &SessionInfo<'_>) -> CommandResult {
        let engine = match info.engine {
            Some(e) => e,
            None => {
                eprintln!("  ✗ model selection not available");
                return CommandResult::Handled;
            }
        };

        let models = match engine.models().await {
            Ok(m) => m,
            Err(e) => {
                eprintln!("  ✗ failed to fetch models: {e}");
                return CommandResult::Handled;
            }
        };

        if models.is_empty() {
            println!("  no models available for {}", info.provider);
            return CommandResult::Handled;
        }

        let current = info.model;

        // Find the current model's index (1-based) for the default
        let current_idx = models.iter().position(|m| m.id == current).map(|i| i + 1);

        println!("  Available models for {}:\n", info.provider);
        for (i, model) in models.iter().enumerate() {
            let marker = if model.id == current {
                " ← current"
            } else {
                ""
            };
            println!("  {}. {}{}", i + 1, model.display_name, marker);
        }

        // Prompt with default
        let default_label = match current_idx {
            Some(idx) => format!(" [{idx}]"),
            None => String::new(),
        };
        print!("\n  Select model{default_label}: ");
        if std::io::Write::flush(&mut std::io::stdout()).is_err() {
            return CommandResult::Handled;
        }

        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_err() {
            eprintln!("  ✗ failed to read input");
            return CommandResult::Handled;
        }
        let input = input.trim();

        // Empty input = keep current
        if input.is_empty() {
            if current_idx.is_some() {
                return CommandResult::Handled;
            }
            eprintln!("  ✗ no model selected");
            return CommandResult::Handled;
        }

        let choice: usize = match input.parse() {
            Ok(n) if n >= 1 && n <= models.len() => n,
            _ => {
                eprintln!("  ✗ invalid selection: {input}");
                return CommandResult::Handled;
            }
        };

        let selected = &models[choice - 1];

        if selected.id == current {
            println!("  already using {}", selected.display_name);
            return CommandResult::Handled;
        }

        println!("  ✓ model changed to {}", selected.display_name);
        CommandResult::ModelChanged(selected.id.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata() {
        assert_eq!(ModelCommand.name(), "/model");
        assert!(ModelCommand.aliases().is_empty());
        assert!(!ModelCommand.description().is_empty());
    }

    #[tokio::test]
    async fn returns_handled_without_engine() {
        let info = super::super::tests::test_info();
        // engine is None in test_info
        let result = ModelCommand.execute(&info).await;
        assert!(matches!(result, CommandResult::Handled));
    }
}
