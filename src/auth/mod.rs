pub mod oauth;
pub mod storage;

pub use storage::AuthStorage;

use anyhow::Result;
use storage::Credential;

/// Complete OAuth login: exchange the authorization code and save credentials.
///
/// This is the shared logic used by both the CLI `golem login` subcommand
/// and the `/login` REPL slash command.
pub async fn login(db_path: &str, provider: &str, code: &str, verifier: &str) -> Result<()> {
    let credentials = oauth::exchange_code(code, verifier).await?;
    let storage = AuthStorage::open(db_path)?;
    storage.set(provider, Credential::OAuth(credentials))?;
    Ok(())
}

/// Remove stored credentials for a provider.
///
/// This is the shared logic used by both the CLI `golem logout` subcommand
/// and the `/logout` REPL slash command.
pub fn logout(db_path: &str, provider: &str) -> Result<()> {
    let storage = AuthStorage::open(db_path)?;
    storage.remove(provider)?;
    Ok(())
}
