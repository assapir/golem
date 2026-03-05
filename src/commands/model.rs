use async_trait::async_trait;
use tokio::io::AsyncBufReadExt;

use super::{Command, CommandResult, SessionInfo, StateChange};
use crate::auth::storage::AuthStorage;
use crate::commands::parse_menu_choice;
use crate::provider::{all_login_providers, build_provider_by_id};
use crate::thinker::ModelInfo;

pub struct ModelCommand;

/// A model entry in the combined list, tracking which provider it belongs to.
struct ModelEntry {
    provider_id: String,
    model: ModelInfo,
}

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

        // Collect models from all authenticated providers.
        // For the active provider, use the engine's thinker.
        // For others, build temporary thinkers.
        let mut all_entries: Vec<ModelEntry> = Vec::new();
        let mut provider_order: Vec<String> = Vec::new();

        // Active provider first
        match engine.models().await {
            Ok(models) => {
                if !models.is_empty() {
                    provider_order.push(info.provider.to_string());
                    for model in models {
                        all_entries.push(ModelEntry {
                            provider_id: info.provider.to_string(),
                            model,
                        });
                    }
                }
            }
            Err(e) => {
                eprintln!("  ✗ failed to fetch {} models: {e}", info.provider);
            }
        }

        // Other authenticated providers
        let other_providers = find_other_authenticated_providers(info.db_path, info.provider);
        for provider_id in &other_providers {
            let setup =
                match build_provider_by_id(provider_id, info.db_path, None, info.debug.clone()) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("  ✗ failed to load {provider_id}: {e}");
                        continue;
                    }
                };
            match setup.thinker.models().await {
                Ok(models) if !models.is_empty() => {
                    provider_order.push(provider_id.clone());
                    for model in models {
                        all_entries.push(ModelEntry {
                            provider_id: provider_id.clone(),
                            model,
                        });
                    }
                }
                Ok(_) => {} // empty — skip silently
                Err(e) => {
                    eprintln!("  ✗ failed to fetch {provider_id} models: {e}");
                }
            }
        }

        if all_entries.is_empty() {
            println!("  no models available");
            return CommandResult::Handled;
        }

        let current_model = info.model;
        let current_provider = info.provider;

        // Find the current model's flat index (1-based) for the default
        let current_idx = all_entries
            .iter()
            .position(|e| e.model.id == current_model && e.provider_id == current_provider)
            .map(|i| i + 1);

        // Display models grouped by provider
        let mut flat_idx = 0;
        for provider_id in &provider_order {
            let display_name = provider_display_name(provider_id);
            let active = if provider_id == current_provider {
                " ← active"
            } else {
                ""
            };
            println!("\n  {display_name}{active}:");

            for entry in &all_entries {
                if entry.provider_id != *provider_id {
                    continue;
                }
                flat_idx += 1;
                let marker =
                    if entry.model.id == current_model && entry.provider_id == current_provider {
                        " ← current"
                    } else {
                        ""
                    };
                println!("  {flat_idx:>3}. {}{marker}", entry.model.display_name);
            }
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

        let stdin = tokio::io::BufReader::new(tokio::io::stdin());
        let mut lines = stdin.lines();
        let input = match lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) | Err(_) => return CommandResult::Handled,
        };
        let input = input.trim().to_string();

        // Empty input = keep current
        if input.is_empty() {
            if current_idx.is_some() {
                return CommandResult::Handled;
            }
            eprintln!("  ✗ no model selected");
            return CommandResult::Handled;
        }

        let choice = match parse_menu_choice(&input, all_entries.len()) {
            Some(n) => n,
            None => {
                eprintln!("  ✗ invalid selection: {input}");
                return CommandResult::Handled;
            }
        };

        let selected = &all_entries[choice - 1];

        if selected.model.id == current_model && selected.provider_id == current_provider {
            println!("  already using {}", selected.model.display_name);
            return CommandResult::Handled;
        }

        if selected.provider_id == current_provider {
            // Same provider — just change the model
            println!("  ✓ model changed to {}", selected.model.display_name);
            CommandResult::StateChanged(StateChange::Model(selected.model.id.clone()))
        } else {
            // Different provider — switch provider and model
            println!(
                "  ✓ switched to {} ({})",
                selected.model.display_name,
                provider_display_name(&selected.provider_id)
            );
            CommandResult::StateChanged(StateChange::Provider(
                selected.provider_id.clone(),
                Some(selected.model.id.clone()),
            ))
        }
    }
}

/// Find providers (other than `exclude`) that have stored credentials or env vars.
fn find_other_authenticated_providers(db_path: &str, exclude: &str) -> Vec<String> {
    let storage = match AuthStorage::open(db_path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let mut result = Vec::new();
    for config in all_login_providers() {
        if config.id() == exclude {
            continue;
        }
        let has_stored = storage.get(config.id()).ok().flatten().is_some();
        let has_env = std::env::var(config.env_var())
            .map(|k| !k.is_empty())
            .unwrap_or(false);
        if has_stored || has_env {
            result.push(config.id().to_string());
        }
    }
    result
}

/// Get a human-readable display name for a provider id.
fn provider_display_name(id: &str) -> &str {
    match id {
        "anthropic" => "Anthropic (Claude)",
        "google" => "Google (Gemini)",
        _ => id,
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

    #[test]
    fn provider_display_name_known() {
        assert_eq!(provider_display_name("anthropic"), "Anthropic (Claude)");
        assert_eq!(provider_display_name("google"), "Google (Gemini)");
    }

    #[test]
    fn provider_display_name_unknown_returns_id() {
        assert_eq!(provider_display_name("unknown"), "unknown");
    }

    #[test]
    fn find_other_authenticated_empty_db() {
        let result = find_other_authenticated_providers(":memory:", "anthropic");
        assert!(result.is_empty());
    }
}
