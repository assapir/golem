//! Provider wiring: trait-based provider config, thinker construction, login/logout flows.

use std::io::{self, Write};

use anyhow::Result;
use async_trait::async_trait;
use clap::ValueEnum;

use crate::auth::storage::{AuthStorage, Credential};
use crate::config::Config;
use crate::consts::default_db_path;
use crate::thinker::Thinker;

// --- Provider trait ---

/// Defines everything a provider needs: identity, auth, thinker construction, and login flow.
#[async_trait]
pub trait ProviderConfig: Send + Sync {
    /// Internal identifier used for storage and auth routing (e.g. `"anthropic"`).
    fn id(&self) -> &'static str;

    /// Human-readable label for prompts (e.g. `"Anthropic (Claude Pro/Max)"`).
    fn display_name(&self) -> &'static str;

    /// Environment variable name for API key fallback (e.g. `"ANTHROPIC_API_KEY"`).
    fn env_var(&self) -> &'static str;

    /// Build the thinker for this provider.
    fn build_thinker(&self, model: Option<String>, auth: AuthStorage) -> Box<dyn Thinker>;

    /// Run the interactive login flow, storing credentials in `db_path`.
    async fn login(&self, db_path: &str) -> Result<()>;
}

// --- Provider implementations ---

mod anthropic_provider {
    use super::*;
    use crate::auth::oauth;
    use crate::thinker::anthropic::AnthropicThinker;

    pub struct Anthropic;

    #[async_trait]
    impl ProviderConfig for Anthropic {
        fn id(&self) -> &'static str {
            "anthropic"
        }

        fn display_name(&self) -> &'static str {
            "Anthropic (Claude Pro/Max)"
        }

        fn env_var(&self) -> &'static str {
            "ANTHROPIC_API_KEY"
        }

        fn build_thinker(&self, model: Option<String>, auth: AuthStorage) -> Box<dyn Thinker> {
            Box::new(AnthropicThinker::new(model, auth))
        }

        async fn login(&self, db_path: &str) -> Result<()> {
            let (url, verifier) = oauth::build_authorize_url();
            let _ = open::that(&url);

            println!("Open this URL to authenticate:\n");
            println!("  {url}\n");

            print!("Paste the authorization code: ");
            io::stdout().flush()?;
            let mut code = String::new();
            io::stdin().read_line(&mut code)?;
            let code = code.trim();

            if code.is_empty() {
                anyhow::bail!("no authorization code provided");
            }

            println!("\nExchanging code for tokens...");
            crate::auth::login(db_path, self.id(), code, &verifier, None).await?;
            Ok(())
        }
    }
}

mod google_provider {
    use super::*;
    use crate::auth::google_oauth;
    use crate::thinker::gemini::GeminiThinker;

    pub struct Google;

    #[async_trait]
    impl ProviderConfig for Google {
        fn id(&self) -> &'static str {
            "google"
        }

        fn display_name(&self) -> &'static str {
            "Google (Gemini)"
        }

        fn env_var(&self) -> &'static str {
            "GEMINI_API_KEY"
        }

        fn build_thinker(&self, model: Option<String>, auth: AuthStorage) -> Box<dyn Thinker> {
            Box::new(GeminiThinker::new(model, auth))
        }

        async fn login(&self, db_path: &str) -> Result<()> {
            let (auth_result, listener) = google_oauth::prepare_authorize().await?;

            let _ = open::that(&auth_result.url);
            println!("Open this URL to authenticate:\n");
            println!("  {}\n", auth_result.url);
            println!(
                "Waiting for callback on http://127.0.0.1:{}...",
                auth_result.port
            );

            let code = google_oauth::await_callback(listener, &auth_result.state).await?;

            println!("\nExchanging code for tokens...");
            crate::auth::login(
                db_path,
                self.id(),
                &code,
                &auth_result.verifier,
                Some(auth_result.port),
            )
            .await?;
            Ok(())
        }
    }
}

/// Look up a `ProviderConfig` by its string identifier (e.g. `"anthropic"`, `"google"`).
/// Used by REPL commands that only have the provider name as a string.
pub fn provider_config_by_id(id: &str) -> Option<Box<dyn ProviderConfig>> {
    match id {
        "anthropic" => Some(Box::new(anthropic_provider::Anthropic)),
        "google" => Some(Box::new(google_provider::Google)),
        _ => None,
    }
}

// --- CLI enums ---

/// Runtime provider selection for the main CLI.
#[derive(Debug, Clone, ValueEnum)]
pub enum Provider {
    Human,
    Anthropic,
    Google,
}

impl Provider {
    /// Get the `ProviderConfig` for this provider, if it has one.
    /// Human provider has no config (no auth, no model defaults).
    fn config(&self) -> Option<Box<dyn ProviderConfig>> {
        match self {
            Self::Human => None,
            Self::Anthropic => Some(Box::new(anthropic_provider::Anthropic)),
            Self::Google => Some(Box::new(google_provider::Google)),
        }
    }
}

/// Provider selection for login/logout subcommands.
#[derive(Debug, Clone, ValueEnum)]
pub enum LoginProvider {
    Anthropic,
    Google,
}

impl LoginProvider {
    /// Get the `ProviderConfig` for this login provider.
    fn config(&self) -> Box<dyn ProviderConfig> {
        match self {
            Self::Anthropic => Box::new(anthropic_provider::Anthropic),
            Self::Google => Box::new(google_provider::Google),
        }
    }
}

// --- Provider wiring ---

/// Everything needed to run a provider, resolved at startup.
pub struct ProviderSetup {
    pub thinker: Box<dyn Thinker>,
    pub name: &'static str,
    pub model: String,
    pub auth_status: String,
}

/// Check auth status for a provider: stored credential → env var → not authenticated.
fn check_auth_status(auth: &AuthStorage, config: &dyn ProviderConfig) -> String {
    match auth.get(config.id()) {
        Ok(Some(Credential::OAuth(_))) => "OAuth ✓".to_string(),
        Ok(Some(Credential::ApiKey { .. })) => "API key ✓".to_string(),
        _ => {
            if std::env::var(config.env_var())
                .map(|k| !k.is_empty())
                .unwrap_or(false)
            {
                "API key (env) ✓".to_string()
            } else {
                "not authenticated".to_string()
            }
        }
    }
}

/// Resolve model override: --model flag > config DB > None (let thinker use its default).
fn resolve_model(cli_model: Option<String>, db_path: &str) -> Option<String> {
    cli_model.or_else(|| {
        Config::open(db_path)
            .ok()
            .and_then(|c| c.get("model").ok().flatten())
    })
}

/// Build the thinker, auth status, and model for the selected provider.
pub fn build_provider(
    provider: &Provider,
    db_path: &str,
    cli_model: Option<String>,
) -> Result<ProviderSetup> {
    let Some(config) = provider.config() else {
        // Human provider — no auth, no model
        if cli_model.is_some() {
            eprintln!("warning: --model is ignored for human provider");
        }
        return Ok(ProviderSetup {
            thinker: Box::new(crate::thinker::human::HumanThinker),
            name: "human",
            model: "—".to_string(),
            auth_status: "N/A".to_string(),
        });
    };

    let auth = AuthStorage::open(db_path)?;
    let auth_status = check_auth_status(&auth, config.as_ref());
    let model = resolve_model(cli_model, db_path);
    let thinker = config.build_thinker(model, auth);
    let display = thinker.model().to_string();

    Ok(ProviderSetup {
        thinker,
        name: config.id(),
        model: display,
        auth_status,
    })
}

// --- Login / Logout ---

pub async fn handle_login(provider: &LoginProvider) -> Result<()> {
    let db_path = default_db_path();
    let db_str = db_path.to_string_lossy();

    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let config = provider.config();
    println!("Logging in to {}...\n", config.display_name());
    config.login(&db_str).await?;
    println!("✓ Logged in to {} successfully!", config.display_name());
    Ok(())
}

pub fn handle_logout(provider: &LoginProvider) -> Result<()> {
    let db_path = default_db_path();
    let db_str = db_path.to_string_lossy();

    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let config = provider.config();
    crate::auth::logout(&db_str, config.id())?;
    println!("✓ Logged out from {}.", config.display_name());
    Ok(())
}
