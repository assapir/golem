use anyhow::{Result, bail};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngExt;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::oauth::OAuthCredentials;

/// Desktop app client (loopback redirect flow).
const CLIENT_ID: &str = "701529528334-otljpqp2bjvhm7lp2eqktu5ja8uo05g6.apps.googleusercontent.com";
const CLIENT_SECRET: &str = "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";

/// TV / Limited Input client (device code flow — works over SSH).
const DEVICE_CLIENT_ID: &str = "701529528334-7buapusrvqo9ogqio29gd8i3ka96j3qg.apps.googleusercontent.com";
const DEVICE_CLIENT_SECRET: &str = "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";

const AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const DEVICE_CODE_URL: &str = "https://oauth2.googleapis.com/device/code";
/// OAuth scopes for both flows. The Gemini API's generateContent has no scope
/// requirements (confirmed via Google's discovery doc), so any valid OAuth
/// token works. We request only the minimum needed for authentication.
const SCOPES: &str = "openid email";

/// 5-minute buffer (in ms) subtracted from token expiry.
const EXPIRY_BUFFER_MS: u64 = 5 * 60 * 1000;

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

/// Generate a random state parameter for CSRF protection.
fn generate_state() -> String {
    let mut rng = rand::rng();
    let bytes: [u8; 16] = rng.random();
    URL_SAFE_NO_PAD.encode(bytes)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before epoch")
        .as_millis() as u64
}

/// Calculate expiry timestamp with a safety buffer, using saturating math
/// to avoid underflow if `expires_in` is unexpectedly small.
fn expiry_with_buffer(expires_in: u64) -> u64 {
    now_ms()
        .saturating_add(expires_in.saturating_mul(1000))
        .saturating_sub(EXPIRY_BUFFER_MS)
}

/// Authorization result from the local loopback server.
pub struct AuthResult {
    pub url: String,
    pub verifier: String,
    pub state: String,
    pub port: u16,
}

/// Build the Google authorization URL and bind a local loopback server.
/// Returns the URL, PKCE verifier, state, and the port the server is on.
///
/// The caller should open the URL in a browser, then call
/// `await_callback()` to wait for the redirect.
pub async fn prepare_authorize() -> Result<(AuthResult, TcpListener)> {
    // Bind to a random available port on loopback
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}");

    let pkce = generate_pkce();
    let state = generate_state();

    let params = [
        ("client_id", CLIENT_ID),
        ("response_type", "code"),
        ("redirect_uri", redirect_uri.as_str()),
        ("scope", SCOPES),
        ("code_challenge", pkce.challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("access_type", "offline"),
        ("prompt", "consent"),
        ("state", state.as_str()),
    ];

    let query = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, urlencoded(v)))
        .collect::<Vec<_>>()
        .join("&");

    let url = format!("{}?{}", AUTHORIZE_URL, query);

    Ok((
        AuthResult {
            url,
            verifier: pkce.verifier,
            state,
            port,
        },
        listener,
    ))
}

/// How long to wait for the OAuth callback before giving up.
const CALLBACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Wait for Google's redirect to the loopback server and extract the auth code.
/// Validates the `state` parameter to prevent CSRF.
/// Times out after 2 minutes if no callback arrives.
pub async fn await_callback(listener: TcpListener, expected_state: &str) -> Result<String> {
    let (mut stream, _) = tokio::time::timeout(CALLBACK_TIMEOUT, listener.accept())
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "timed out waiting for OAuth callback ({}s). Try again.",
                CALLBACK_TIMEOUT.as_secs()
            )
        })??;

    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await?;
    let request = String::from_utf8_lossy(&buf[..n]);

    // Extract the path from "GET /path?query HTTP/1.1"
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("");

    // Parse and URL-decode query parameters
    let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
    let params: std::collections::HashMap<String, String> = query
        .split('&')
        .filter_map(|p| {
            let (k, v) = p.split_once('=')?;
            Some((urldecode(k), urldecode(v)))
        })
        .collect();

    // Check for errors from Google
    if let Some(error) = params.get("error") {
        let body = format!("Authentication failed: {error}");
        let response = format!(
            "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
        bail!("Google OAuth error: {error}");
    }

    // Validate state
    let state = params.get("state").map(|s| s.as_str()).unwrap_or("");
    if state != expected_state {
        let body = "State mismatch — possible CSRF attack.";
        let response = format!(
            "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
        bail!("OAuth state mismatch: expected {expected_state}, got {state}");
    }

    let code = params
        .get("code")
        .ok_or_else(|| anyhow::anyhow!("no authorization code in callback"))?
        .to_string();

    // Send a nice response to the browser
    let body = "✓ Authentication successful! You can close this tab and return to Golem.";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;

    Ok(code)
}

/// Exchange an authorization code for tokens.
pub async fn exchange_code(code: &str, verifier: &str, port: u16) -> Result<OAuthCredentials> {
    let redirect_uri = format!("http://127.0.0.1:{port}");
    let params = [
        ("grant_type", "authorization_code"),
        ("client_id", CLIENT_ID),
        ("client_secret", CLIENT_SECRET),
        ("code", code),
        ("redirect_uri", redirect_uri.as_str()),
        ("code_verifier", verifier),
    ];

    let client = reqwest::Client::new();
    let resp = client.post(TOKEN_URL).form(&params).send().await?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        bail!("Google token exchange failed: {}", text);
    }

    let data: TokenResponse = resp.json().await?;

    let refresh = data.refresh_token.ok_or_else(|| {
        anyhow::anyhow!(
            "Google did not return a refresh token. \
             Try revoking access at https://myaccount.google.com/permissions and logging in again."
        )
    })?;

    Ok(OAuthCredentials {
        access: data.access_token,
        refresh,
        expires: expiry_with_buffer(data.expires_in),
        client_hint: None,
    })
}

/// Refresh an expired access token.
///
/// `client_hint` selects which OAuth client to use:
/// - `Some("device")` → TV / Limited Input client (device code flow)
/// - anything else → Desktop client (loopback flow)
pub async fn refresh_token(refresh: &str, client_hint: Option<&str>) -> Result<OAuthCredentials> {
    let (cid, csecret) = match client_hint {
        Some("device") => (DEVICE_CLIENT_ID, DEVICE_CLIENT_SECRET),
        _ => (CLIENT_ID, CLIENT_SECRET),
    };

    let params = [
        ("grant_type", "refresh_token"),
        ("client_id", cid),
        ("client_secret", csecret),
        ("refresh_token", refresh),
    ];

    let client = reqwest::Client::new();
    let resp = client.post(TOKEN_URL).form(&params).send().await?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        bail!("Google token refresh failed: {}", text);
    }

    let data: TokenResponse = resp.json().await?;

    // Google refresh responses may omit refresh_token (reuse the old one)
    Ok(OAuthCredentials {
        access: data.access_token,
        refresh: data.refresh_token.unwrap_or_else(|| refresh.to_string()),
        expires: expiry_with_buffer(data.expires_in),
        client_hint: client_hint.map(String::from),
    })
}

#[derive(serde::Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
}

// ---------------------------------------------------------------------------
// Device code flow (for SSH / headless environments)
// ---------------------------------------------------------------------------

/// Response from Google's device code endpoint.
#[derive(serde::Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_url: String,
    expires_in: u64,
    interval: u64,
}

/// Response while polling — may be a pending status or final tokens.
#[derive(serde::Deserialize)]
struct DevicePollResponse {
    /// Present on error (e.g. "authorization_pending", "slow_down", "access_denied").
    error: Option<String>,
    /// Present on success.
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}

/// Initiate the device code flow. Returns the user code and verification URL
/// for display, plus the device code for polling.
pub async fn device_code_authorize() -> Result<DeviceAuth> {
    let params = [
        ("client_id", DEVICE_CLIENT_ID),
        ("scope", SCOPES),
    ];

    let client = reqwest::Client::new();
    let resp = client.post(DEVICE_CODE_URL).form(&params).send().await?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        bail!("Google device code request failed: {text}");
    }

    let data: DeviceCodeResponse = resp.json().await?;

    Ok(DeviceAuth {
        device_code: data.device_code,
        user_code: data.user_code,
        verification_url: data.verification_url,
        expires_in: data.expires_in,
        interval: data.interval,
    })
}

/// Everything needed to complete the device code flow.
pub struct DeviceAuth {
    pub device_code: String,
    pub user_code: String,
    pub verification_url: String,
    pub expires_in: u64,
    pub interval: u64,
}

/// Poll Google's token endpoint until the user approves (or the code expires).
pub async fn poll_device_token(auth: &DeviceAuth) -> Result<OAuthCredentials> {
    let deadline = std::time::Instant::now()
        + std::time::Duration::from_secs(auth.expires_in);
    let mut interval = std::time::Duration::from_secs(auth.interval.max(5));

    let client = reqwest::Client::new();

    loop {
        tokio::time::sleep(interval).await;

        if std::time::Instant::now() > deadline {
            bail!("device code expired — please try again");
        }

        let params = [
            ("client_id", DEVICE_CLIENT_ID),
            ("client_secret", DEVICE_CLIENT_SECRET),
            ("device_code", auth.device_code.as_str()),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ];

        let resp = client.post(TOKEN_URL).form(&params).send().await?;
        let data: DevicePollResponse = resp.json().await?;

        match data.error.as_deref() {
            Some("authorization_pending") => continue,
            Some("slow_down") => {
                // Back off by 5 seconds as required by Google
                interval += std::time::Duration::from_secs(5);
                continue;
            }
            Some(err) => bail!("Google device auth failed: {err}"),
            None => {
                // Success — tokens present
                let access = data.access_token
                    .ok_or_else(|| anyhow::anyhow!("missing access_token in device response"))?;
                let refresh = data.refresh_token
                    .ok_or_else(|| anyhow::anyhow!(
                        "Google did not return a refresh token. \
                         Try revoking access at https://myaccount.google.com/permissions \
                         and logging in again."
                    ))?;
                let expires_in = data.expires_in.unwrap_or(3600);

                return Ok(OAuthCredentials {
                    access,
                    refresh,
                    expires: expiry_with_buffer(expires_in),
                    client_hint: Some("device".into()),
                });
            }
        }
    }
}

/// Detect whether we're in a headless / SSH environment where loopback
/// redirect won't work.
pub fn is_headless() -> bool {
    // SSH session — browser redirect to 127.0.0.1 on remote won't work
    if std::env::var("SSH_CONNECTION").is_ok() || std::env::var("SSH_TTY").is_ok() {
        return true;
    }
    // No display server on Linux
    #[cfg(target_os = "linux")]
    if std::env::var("DISPLAY").is_err() && std::env::var("WAYLAND_DISPLAY").is_err() {
        return true;
    }
    false
}

/// Decode a percent-encoded string (e.g. `hello%20world` → `hello world`).
fn urldecode(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    let mut bytes = s.bytes();
    while let Some(b) = bytes.next() {
        if b == b'%' {
            let hi = bytes.next().and_then(hex_digit);
            let lo = bytes.next().and_then(hex_digit);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push(h << 4 | l);
            }
        } else if b == b'+' {
            out.push(b' ');
        } else {
            out.push(b);
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
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

/// Verify that a PKCE verifier and challenge are correctly related.
pub fn verify_pkce(verifier: &str, challenge: &str) -> bool {
    let hash = Sha256::digest(verifier.as_bytes());
    let expected = URL_SAFE_NO_PAD.encode(hash);
    expected == challenge
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_verifier_is_43_chars() {
        let pkce = generate_pkce();
        assert_eq!(pkce.verifier.len(), 43);
    }

    #[test]
    fn pkce_challenge_is_43_chars() {
        let pkce = generate_pkce();
        assert_eq!(pkce.challenge.len(), 43);
    }

    #[test]
    fn pkce_verifier_and_challenge_differ() {
        let pkce = generate_pkce();
        assert_ne!(pkce.verifier, pkce.challenge);
    }

    #[test]
    fn pkce_challenge_matches_verifier() {
        let pkce = generate_pkce();
        assert!(verify_pkce(&pkce.verifier, &pkce.challenge));
    }

    #[test]
    fn pkce_wrong_verifier_fails() {
        let pkce = generate_pkce();
        assert!(!verify_pkce(
            "wrong-verifier-value-xxxxxxxxxxxxxxxxxx",
            &pkce.challenge
        ));
    }

    #[test]
    fn pkce_is_random_each_time() {
        let a = generate_pkce();
        let b = generate_pkce();
        assert_ne!(a.verifier, b.verifier);
        assert_ne!(a.challenge, b.challenge);
    }

    #[test]
    fn state_is_random_each_time() {
        let a = generate_state();
        let b = generate_state();
        assert_ne!(a, b);
    }

    #[test]
    fn state_is_non_empty() {
        let state = generate_state();
        assert!(!state.is_empty());
    }

    #[test]
    fn expiry_with_buffer_normal() {
        // 1 hour = 3600s, buffer = 5min = 300s
        let expires = expiry_with_buffer(3600);
        // Should be roughly now + 55 minutes
        let expected_min = now_ms() + (3300 * 1000);
        let expected_max = now_ms() + (3600 * 1000);
        assert!(expires >= expected_min && expires <= expected_max);
    }

    #[test]
    fn expiry_with_buffer_small_value_no_underflow() {
        // If expires_in is 0, should not panic or wrap
        let expires = expiry_with_buffer(0);
        // Should be close to now (minus buffer, saturated)
        assert!(expires <= now_ms());
    }

    #[test]
    fn urldecode_basic() {
        assert_eq!(urldecode("hello%20world"), "hello world");
        assert_eq!(urldecode("a%3Db%26c"), "a=b&c");
    }

    #[test]
    fn urldecode_plus_as_space() {
        assert_eq!(urldecode("hello+world"), "hello world");
    }

    #[test]
    fn urldecode_passthrough() {
        assert_eq!(urldecode("hello"), "hello");
    }

    #[test]
    fn urlencoded_preserves_alphanumeric() {
        assert_eq!(urlencoded("hello"), "hello");
    }

    #[test]
    fn urlencoded_encodes_special_chars() {
        assert_eq!(urlencoded("a=b&c"), "a%3Db%26c");
        assert_eq!(urlencoded("foo:bar"), "foo%3Abar");
    }

    #[test]
    fn urlencoded_preserves_unreserved() {
        assert_eq!(urlencoded("a-b_c.d~e"), "a-b_c.d~e");
    }

    #[test]
    fn urlencoded_encodes_slashes() {
        assert_eq!(
            urlencoded("https://www.googleapis.com/auth/cloud-platform"),
            "https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fcloud-platform"
        );
    }

    #[tokio::test]
    async fn prepare_authorize_returns_valid_url() {
        let (auth, _listener) = prepare_authorize().await.unwrap();
        assert!(
            auth.url
                .starts_with("https://accounts.google.com/o/oauth2/v2/auth?")
        );
        assert!(auth.url.contains("client_id="));
        assert!(auth.url.contains("response_type=code"));
        assert!(auth.url.contains("redirect_uri=http%3A%2F%2F127.0.0.1"));
        assert!(auth.url.contains("code_challenge="));
        assert!(auth.url.contains("code_challenge_method=S256"));
        assert!(auth.url.contains("access_type=offline"));
        assert!(auth.url.contains("state="));
    }

    #[tokio::test]
    async fn prepare_authorize_binds_to_port() {
        let (auth, _listener) = prepare_authorize().await.unwrap();
        assert!(auth.port > 0);
        assert!(auth.url.contains(&format!("127.0.0.1%3A{}", auth.port)));
    }
}
