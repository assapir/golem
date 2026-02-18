use anyhow::{bail, Result};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngExt;
use sha2::{Digest, Sha256};

const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
const TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";
const REDIRECT_URI: &str = "https://console.anthropic.com/oauth/code/callback";
const SCOPES: &str = "org:create_api_key user:profile user:inference";

/// OAuth credentials stored after login.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OAuthCredentials {
    pub access: String,
    pub refresh: String,
    /// Expiration timestamp in milliseconds since epoch.
    pub expires: u64,
}

impl OAuthCredentials {
    pub fn is_expired(&self) -> bool {
        now_ms() >= self.expires
    }
}

/// PKCE verifier and challenge pair.
struct Pkce {
    verifier: String,
    challenge: String,
}

/// Generate a PKCE code verifier and S256 challenge.
fn generate_pkce() -> Pkce {
    let mut rng = rand::rng();
    let bytes: [u8; 32] = rng.random();
    let verifier = URL_SAFE_NO_PAD.encode(bytes);

    let hash = Sha256::digest(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(hash);

    Pkce {
        verifier,
        challenge,
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before epoch")
        .as_millis() as u64
}

/// Build the authorization URL for the user to visit.
/// Returns (url, pkce_verifier) — caller must keep the verifier for token exchange.
pub fn build_authorize_url() -> (String, String) {
    let pkce = generate_pkce();

    let params = [
        ("code", "true"),
        ("client_id", CLIENT_ID),
        ("response_type", "code"),
        ("redirect_uri", REDIRECT_URI),
        ("scope", SCOPES),
        ("code_challenge", &pkce.challenge),
        ("code_challenge_method", "S256"),
        ("state", &pkce.verifier),
    ];

    let query = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, urlencoded(v)))
        .collect::<Vec<_>>()
        .join("&");

    let url = format!("{}?{}", AUTHORIZE_URL, query);
    (url, pkce.verifier)
}

/// Exchange an authorization code for tokens.
/// `auth_code_raw` is the string pasted by the user, in the format `code#state`.
pub async fn exchange_code(auth_code_raw: &str, verifier: &str) -> Result<OAuthCredentials> {
    let (code, state) = auth_code_raw
        .split_once('#')
        .unwrap_or((auth_code_raw, ""));

    let body = serde_json::json!({
        "grant_type": "authorization_code",
        "client_id": CLIENT_ID,
        "code": code,
        "state": state,
        "redirect_uri": REDIRECT_URI,
        "code_verifier": verifier,
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(TOKEN_URL)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        bail!("token exchange failed: {}", text);
    }

    let data: TokenResponse = resp.json().await?;

    // 5 minute buffer before expiry
    let expires = now_ms() + (data.expires_in * 1000) - (5 * 60 * 1000);

    Ok(OAuthCredentials {
        access: data.access_token,
        refresh: data.refresh_token,
        expires,
    })
}

/// Refresh an expired access token.
pub async fn refresh_token(refresh: &str) -> Result<OAuthCredentials> {
    let body = serde_json::json!({
        "grant_type": "refresh_token",
        "client_id": CLIENT_ID,
        "refresh_token": refresh,
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(TOKEN_URL)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        bail!("token refresh failed: {}", text);
    }

    let data: TokenResponse = resp.json().await?;

    let expires = now_ms() + (data.expires_in * 1000) - (5 * 60 * 1000);

    Ok(OAuthCredentials {
        access: data.access_token,
        refresh: data.refresh_token,
        expires,
    })
}

#[derive(serde::Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
}

/// Minimal URL encoding for query parameters.
fn urlencoded(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}
