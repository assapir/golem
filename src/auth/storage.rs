use std::sync::Mutex;

use anyhow::Result;
use rusqlite::Connection;

use super::oauth::OAuthCredentials;

/// Credential types stored per provider.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum Credential {
    #[serde(rename = "oauth")]
    OAuth(OAuthCredentials),
    #[serde(rename = "api_key")]
    ApiKey { key: String },
}

/// A resolved credential ready for use in API calls.
#[derive(Debug, Clone)]
pub struct ResolvedCredential {
    /// The token or API key string.
    pub token: String,
    /// Whether this is an OAuth access token (true) or an API key (false).
    pub is_oauth: bool,
}

/// Manages credential storage in SQLite.
///
/// Shares a database with memory and config — pass the same connection
/// or path used for `SqliteMemory`.
pub struct AuthStorage {
    conn: Mutex<Connection>,
}

impl AuthStorage {
    /// Open or create a credentials table in the given database path.
    /// Use `":memory:"` for tests.
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS credentials (
                provider TEXT PRIMARY KEY,
                data     TEXT NOT NULL
            )",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Get credential for a provider.
    pub fn get(&self, provider: &str) -> Result<Option<Credential>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT data FROM credentials WHERE provider = ?1")?;
        let mut rows = stmt.query([provider])?;
        match rows.next()? {
            Some(row) => {
                let json: String = row.get(0)?;
                let cred: Credential = serde_json::from_str(&json)?;
                Ok(Some(cred))
            }
            None => Ok(None),
        }
    }

    /// Store credential for a provider (upsert).
    pub fn set(&self, provider: &str, credential: Credential) -> Result<()> {
        let json = serde_json::to_string(&credential)?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO credentials (provider, data) VALUES (?1, ?2)
             ON CONFLICT(provider) DO UPDATE SET data = excluded.data",
            [provider, &json],
        )?;
        Ok(())
    }

    /// Remove credential for a provider.
    pub fn remove(&self, provider: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM credentials WHERE provider = ?1", [provider])?;
        Ok(())
    }

    /// Get the API key for a provider, handling OAuth token refresh.
    /// Priority: stored OAuth → stored API key → environment variable.
    pub async fn get_api_key(&self, provider: &str, env_var: &str) -> Result<Option<String>> {
        self.get_credential(provider, env_var)
            .await
            .map(|opt| opt.map(|r| r.token))
    }

    /// Get the resolved credential for a provider, including its type.
    /// Priority: stored OAuth → stored API key → environment variable.
    pub async fn get_credential(
        &self,
        provider: &str,
        env_var: &str,
    ) -> Result<Option<ResolvedCredential>> {
        if let Some(cred) = self.get(provider)? {
            match cred {
                Credential::ApiKey { key } => {
                    return Ok(Some(ResolvedCredential {
                        token: key,
                        is_oauth: false,
                    }));
                }
                Credential::OAuth(mut oauth) => {
                    if oauth.is_expired() {
                        let refreshed = match provider {
                            "google" => super::google_oauth::refresh_token(&oauth.refresh).await?,
                            _ => super::oauth::refresh_token(&oauth.refresh).await?,
                        };
                        oauth = refreshed.clone();
                        self.set(provider, Credential::OAuth(refreshed))?;
                    }
                    return Ok(Some(ResolvedCredential {
                        token: oauth.access,
                        is_oauth: true,
                    }));
                }
            }
        }

        // Fall back to environment variable
        if let Ok(key) = std::env::var(env_var)
            && !key.is_empty()
        {
            return Ok(Some(ResolvedCredential {
                token: key,
                is_oauth: false,
            }));
        }

        Ok(None)
    }
}
