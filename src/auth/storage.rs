use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use super::oauth::OAuthCredentials;

/// Credential entry in auth.json.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum Credential {
    #[serde(rename = "oauth")]
    OAuth(OAuthCredentials),
    #[serde(rename = "api_key")]
    ApiKey { key: String },
}

/// Manages credential storage in `~/.golem/auth.json`.
pub struct AuthStorage {
    path: PathBuf,
}

impl AuthStorage {
    pub fn new() -> Result<Self> {
        let dir = dirs::home_dir()
            .context("cannot determine home directory")?
            .join(".golem");
        fs::create_dir_all(&dir)?;
        Ok(Self {
            path: dir.join("auth.json"),
        })
    }

    /// Create with a custom path (for testing).
    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    fn load(&self) -> Result<HashMap<String, Credential>> {
        if !self.path.exists() {
            return Ok(HashMap::new());
        }
        let data = fs::read_to_string(&self.path)?;
        let creds: HashMap<String, Credential> = serde_json::from_str(&data)?;
        Ok(creds)
    }

    fn save(&self, creds: &HashMap<String, Credential>) -> Result<()> {
        let data = serde_json::to_string_pretty(creds)?;
        fs::write(&self.path, &data)?;

        // Set file permissions to 0600 (owner read/write only) on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600))?;
        }

        Ok(())
    }

    /// Get credential for a provider.
    pub fn get(&self, provider: &str) -> Result<Option<Credential>> {
        let creds = self.load()?;
        Ok(creds.get(provider).cloned())
    }

    /// Store credential for a provider.
    pub fn set(&self, provider: &str, credential: Credential) -> Result<()> {
        let mut creds = self.load()?;
        creds.insert(provider.to_string(), credential);
        self.save(&creds)
    }

    /// Remove credential for a provider.
    pub fn remove(&self, provider: &str) -> Result<()> {
        let mut creds = self.load()?;
        creds.remove(provider);
        self.save(&creds)
    }

    /// Get the API key for a provider, handling OAuth token refresh.
    /// Priority: auth.json OAuth → auth.json API key → environment variable.
    pub async fn get_api_key(&self, provider: &str, env_var: &str) -> Result<Option<String>> {
        if let Some(cred) = self.get(provider)? {
            match cred {
                Credential::ApiKey { key } => return Ok(Some(key)),
                Credential::OAuth(mut oauth) => {
                    if oauth.is_expired() {
                        // Refresh the token
                        let refreshed =
                            super::oauth::refresh_token(&oauth.refresh).await?;
                        oauth = refreshed.clone();
                        self.set(provider, Credential::OAuth(refreshed))?;
                    }
                    return Ok(Some(oauth.access));
                }
            }
        }

        // Fall back to environment variable
        if let Ok(key) = std::env::var(env_var)
            && !key.is_empty()
        {
            return Ok(Some(key));
        }

        Ok(None)
    }
}
