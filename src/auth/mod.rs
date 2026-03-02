pub mod google_oauth;
pub mod oauth;
pub mod storage;

pub use storage::AuthStorage;

use anyhow::{Context, Result, bail};
use storage::Credential;

/// Providers that support OAuth login.
const SUPPORTED_PROVIDERS: &[&str] = &["anthropic", "google"];

/// Complete OAuth login: exchange the authorization code and save credentials.
///
/// This is the shared logic used by both the CLI `golem login` subcommand
/// and the `/login` REPL slash command.
///
/// Returns an error if the provider is not supported, the token exchange
/// fails, or credentials cannot be saved.
pub async fn login(
    db_path: &str,
    provider: &str,
    code: &str,
    verifier: &str,
    port: Option<u16>,
) -> Result<()> {
    if !SUPPORTED_PROVIDERS.contains(&provider) {
        bail!("unsupported provider: {provider}");
    }
    let credentials = match provider {
        "google" => {
            let port = port.ok_or_else(|| anyhow::anyhow!("port required for Google login"))?;
            google_oauth::exchange_code(code, verifier, port)
                .await
                .context("Google token exchange failed")?
        }
        _ => oauth::exchange_code(code, verifier)
            .await
            .context("token exchange failed")?,
    };
    let storage = AuthStorage::open(db_path).context("failed to open auth storage")?;
    storage
        .set(provider, Credential::OAuth(credentials))
        .context("failed to save credentials")?;
    Ok(())
}

/// Remove stored credentials for a provider.
///
/// This is the shared logic used by both the CLI `golem logout` subcommand
/// and the `/logout` REPL slash command.
pub fn logout(db_path: &str, provider: &str) -> Result<()> {
    let storage = AuthStorage::open(db_path).context("failed to open auth storage")?;
    storage
        .remove(provider)
        .context("failed to remove credentials")?;
    Ok(())
}
