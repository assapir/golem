//! Provider wiring: trait-based provider config, thinker construction, login/logout flows.

use std::io::{self, Write};

use anyhow::Result;
use async_trait::async_trait;
use clap::ValueEnum;

use crate::auth::storage::{AuthStorage, Credential};
use crate::config::Config;
use crate::consts::default_db_path;
use crate::debug::DebugMode;
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
    fn build_thinker(
        &self,
        model: Option<String>,
        auth: AuthStorage,
        debug: DebugMode,
    ) -> Box<dyn Thinker>;

    /// Run the interactive login flow, storing credentials in `db_path`.
    async fn login(&self, db_path: &str) -> Result<()>;
}

// --- Provider implementations ---

mod anthropic_provider {
    use super::*;
    use crate::auth::oauth;
    use crate::thinker::anthropic::AnthropicThinker;
    use tokio::io::AsyncBufReadExt;

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

        fn build_thinker(
            &self,
            model: Option<String>,
            auth: AuthStorage,
            debug: DebugMode,
        ) -> Box<dyn Thinker> {
            Box::new(AnthropicThinker::new(model, auth, debug))
        }

        async fn login(&self, db_path: &str) -> Result<()> {
            let (url, verifier) = oauth::build_authorize_url();
            let _ = open::that(&url);

            println!("Open this URL to authenticate:\n");
            println!("  {url}\n");

            print!("Paste the authorization code: ");
            io::stdout().flush()?;

            let stdin = tokio::io::BufReader::new(tokio::io::stdin());
            let mut lines = stdin.lines();
            let code = match lines.next_line().await? {
                Some(line) => line,
                None => anyhow::bail!("no authorization code provided"),
            };
            let code = code.trim().to_string();

            if code.is_empty() {
                anyhow::bail!("no authorization code provided");
            }

            println!("\nExchanging code for tokens...");
            crate::auth::login(db_path, self.id(), &code, &verifier, None).await?;
            Ok(())
        }
    }
}

mod google_provider {
    use super::*;
    use crate::auth::google_oauth;
    use crate::auth::storage::Credential;
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

        fn build_thinker(
            &self,
            model: Option<String>,
            auth: AuthStorage,
            debug: DebugMode,
        ) -> Box<dyn Thinker> {
            Box::new(GeminiThinker::new(model, auth, debug))
        }

        async fn login(&self, db_path: &str) -> Result<()> {
            if google_oauth::is_headless() {
                self.login_device_code(db_path).await
            } else {
                self.login_loopback(db_path).await
            }
        }
    }

    impl Google {
        /// Loopback redirect flow — opens browser, Google redirects to localhost.
        async fn login_loopback(&self, db_path: &str) -> Result<()> {
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

        /// Device code flow — works over SSH / headless.
        async fn login_device_code(&self, db_path: &str) -> Result<()> {
            println!("Headless environment detected — using device code flow.\n");

            let auth = google_oauth::device_code_authorize().await?;

            println!("Go to: {}\n", auth.verification_url);
            println!("Enter code: {}\n", auth.user_code);
            println!("Waiting for approval...");

            let creds = google_oauth::poll_device_token(&auth).await?;

            let storage = AuthStorage::open(db_path)?;
            storage.set(self.id(), Credential::OAuth(creds))?;

            Ok(())
        }
    }
}

/// Look up a `ProviderConfig` by its string identifier (e.g. `"anthropic"`, `"google"`).
/// Derived from `Provider::value_variants()` — no separate hardcoded match needed.
pub fn provider_config_by_id(id: &str) -> Option<Box<dyn ProviderConfig>> {
    Provider::value_variants()
        .iter()
        .filter_map(|p| p.config())
        .find(|c| c.id() == id)
}

/// Return all providers that support login, for the `/login` menu.
/// Derived from `Provider::value_variants()` — new providers added to
/// the enum automatically appear here if they implement `ProviderConfig`.
pub fn all_login_providers() -> Vec<Box<dyn ProviderConfig>> {
    Provider::value_variants()
        .iter()
        .filter_map(|p| p.config())
        .collect()
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
    debug: DebugMode,
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
    let thinker = config.build_thinker(model, auth, debug);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_login_providers_is_non_empty() {
        let providers = all_login_providers();
        assert!(!providers.is_empty());
    }

    #[test]
    fn all_login_providers_excludes_human() {
        let providers = all_login_providers();
        for p in &providers {
            assert_ne!(
                p.id(),
                "human",
                "human provider should not appear in login list"
            );
        }
    }

    #[test]
    fn all_login_providers_includes_anthropic_and_google() {
        let providers = all_login_providers();
        let ids: Vec<&str> = providers.iter().map(|p| p.id()).collect();
        assert!(ids.contains(&"anthropic"), "missing anthropic");
        assert!(ids.contains(&"google"), "missing google");
    }

    #[test]
    fn all_login_providers_have_display_names() {
        for p in all_login_providers() {
            assert!(
                !p.display_name().is_empty(),
                "provider {} has empty display name",
                p.id()
            );
        }
    }

    #[test]
    fn all_login_providers_have_env_vars() {
        for p in all_login_providers() {
            assert!(
                !p.env_var().is_empty(),
                "provider {} has empty env var",
                p.id()
            );
        }
    }

    #[test]
    fn all_login_providers_have_unique_ids() {
        let providers = all_login_providers();
        let mut ids: Vec<&str> = providers.iter().map(|p| p.id()).collect();
        let len_before = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), len_before, "duplicate provider ids");
    }

    #[test]
    fn all_login_providers_matches_provider_enum() {
        // Every variant with a config() should appear in all_login_providers()
        let login_ids: Vec<&str> = all_login_providers().iter().map(|p| p.id()).collect();
        for variant in Provider::value_variants() {
            if let Some(config) = variant.config() {
                assert!(
                    login_ids.contains(&config.id()),
                    "Provider variant {:?} has a config but is missing from all_login_providers()",
                    variant
                );
            }
        }
    }

    #[test]
    fn provider_config_by_id_round_trips() {
        for p in all_login_providers() {
            let looked_up = provider_config_by_id(p.id());
            assert!(
                looked_up.is_some(),
                "provider_config_by_id({}) returned None",
                p.id()
            );
            assert_eq!(looked_up.unwrap().id(), p.id());
        }
    }

    #[test]
    fn provider_config_by_id_returns_none_for_unknown() {
        assert!(provider_config_by_id("unknown").is_none());
        assert!(provider_config_by_id("human").is_none());
    }
}
