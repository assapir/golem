use std::fs;

use golem::auth::oauth::{OAuthCredentials, build_authorize_url, verify_pkce};
use golem::auth::storage::{AuthStorage, Credential};

/// Helper: create a temp dir with an AuthStorage pointing at it.
fn temp_storage() -> (AuthStorage, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("auth.json");
    let storage = AuthStorage::with_path(path);
    (storage, dir)
}

// ── Storage CRUD ──────────────────────────────────────────────────

#[test]
fn get_returns_none_when_no_file() {
    let (storage, _dir) = temp_storage();
    let result = storage.get("anthropic").unwrap();
    assert!(result.is_none());
}

#[test]
fn set_and_get_api_key() {
    let (storage, _dir) = temp_storage();
    storage
        .set(
            "anthropic",
            Credential::ApiKey {
                key: "sk-test".to_string(),
            },
        )
        .unwrap();

    let cred = storage.get("anthropic").unwrap().unwrap();
    match cred {
        Credential::ApiKey { key } => assert_eq!(key, "sk-test"),
        _ => panic!("expected ApiKey"),
    }
}

#[test]
fn set_and_get_oauth() {
    let (storage, _dir) = temp_storage();
    let oauth = OAuthCredentials {
        access: "access-token".to_string(),
        refresh: "refresh-token".to_string(),
        expires: 9999999999999,
    };
    storage.set("anthropic", Credential::OAuth(oauth)).unwrap();

    let cred = storage.get("anthropic").unwrap().unwrap();
    match cred {
        Credential::OAuth(c) => {
            assert_eq!(c.access, "access-token");
            assert_eq!(c.refresh, "refresh-token");
            assert_eq!(c.expires, 9999999999999);
        }
        _ => panic!("expected OAuth"),
    }
}

#[test]
fn remove_deletes_credential() {
    let (storage, _dir) = temp_storage();
    storage
        .set(
            "anthropic",
            Credential::ApiKey {
                key: "sk-test".to_string(),
            },
        )
        .unwrap();

    storage.remove("anthropic").unwrap();
    assert!(storage.get("anthropic").unwrap().is_none());
}

#[test]
fn remove_nonexistent_is_ok() {
    let (storage, _dir) = temp_storage();
    // Should not error when removing a key that doesn't exist
    storage.remove("nonexistent").unwrap();
}

#[test]
fn multiple_providers_independent() {
    let (storage, _dir) = temp_storage();
    storage
        .set(
            "anthropic",
            Credential::ApiKey {
                key: "sk-ant".to_string(),
            },
        )
        .unwrap();
    storage
        .set(
            "openai",
            Credential::ApiKey {
                key: "sk-oai".to_string(),
            },
        )
        .unwrap();

    let ant = storage.get("anthropic").unwrap().unwrap();
    let oai = storage.get("openai").unwrap().unwrap();

    match (ant, oai) {
        (Credential::ApiKey { key: k1 }, Credential::ApiKey { key: k2 }) => {
            assert_eq!(k1, "sk-ant");
            assert_eq!(k2, "sk-oai");
        }
        _ => panic!("expected ApiKey for both"),
    }
}

#[test]
fn set_overwrites_existing() {
    let (storage, _dir) = temp_storage();
    storage
        .set(
            "anthropic",
            Credential::ApiKey {
                key: "old".to_string(),
            },
        )
        .unwrap();
    storage
        .set(
            "anthropic",
            Credential::ApiKey {
                key: "new".to_string(),
            },
        )
        .unwrap();

    match storage.get("anthropic").unwrap().unwrap() {
        Credential::ApiKey { key } => assert_eq!(key, "new"),
        _ => panic!("expected ApiKey"),
    }
}

#[test]
fn remove_one_preserves_others() {
    let (storage, _dir) = temp_storage();
    storage
        .set(
            "anthropic",
            Credential::ApiKey {
                key: "ant".to_string(),
            },
        )
        .unwrap();
    storage
        .set(
            "openai",
            Credential::ApiKey {
                key: "oai".to_string(),
            },
        )
        .unwrap();

    storage.remove("anthropic").unwrap();

    assert!(storage.get("anthropic").unwrap().is_none());
    assert!(storage.get("openai").unwrap().is_some());
}

// ── File permissions ──────────────────────────────────────────────

#[cfg(unix)]
#[test]
fn auth_file_has_0600_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let (storage, dir) = temp_storage();
    storage
        .set(
            "test",
            Credential::ApiKey {
                key: "secret".to_string(),
            },
        )
        .unwrap();

    let path = dir.path().join("auth.json");
    let perms = fs::metadata(&path).unwrap().permissions();
    assert_eq!(perms.mode() & 0o777, 0o600);
}

// ── JSON format ───────────────────────────────────────────────────

#[test]
fn stored_json_is_readable() {
    let (storage, dir) = temp_storage();
    storage
        .set(
            "anthropic",
            Credential::ApiKey {
                key: "sk-test".to_string(),
            },
        )
        .unwrap();

    let path = dir.path().join("auth.json");
    let content = fs::read_to_string(path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

    assert_eq!(parsed["anthropic"]["type"], "api_key");
    assert_eq!(parsed["anthropic"]["key"], "sk-test");
}

#[test]
fn oauth_stored_json_format() {
    let (storage, dir) = temp_storage();
    let oauth = OAuthCredentials {
        access: "access".to_string(),
        refresh: "refresh".to_string(),
        expires: 12345,
    };
    storage.set("anthropic", Credential::OAuth(oauth)).unwrap();

    let path = dir.path().join("auth.json");
    let content = fs::read_to_string(path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

    assert_eq!(parsed["anthropic"]["type"], "oauth");
    assert_eq!(parsed["anthropic"]["access"], "access");
    assert_eq!(parsed["anthropic"]["refresh"], "refresh");
    assert_eq!(parsed["anthropic"]["expires"], 12345);
}

// ── get_api_key resolution ────────────────────────────────────────

#[tokio::test]
async fn get_api_key_from_api_key_credential() {
    let (storage, _dir) = temp_storage();
    storage
        .set(
            "anthropic",
            Credential::ApiKey {
                key: "sk-my-key".to_string(),
            },
        )
        .unwrap();

    let key = storage
        .get_api_key("anthropic", "GOLEM_TEST_NONEXISTENT_VAR")
        .await
        .unwrap();
    assert_eq!(key, Some("sk-my-key".to_string()));
}

#[tokio::test]
async fn get_api_key_from_oauth_non_expired() {
    let (storage, _dir) = temp_storage();
    let oauth = OAuthCredentials {
        access: "sk-ant-oat01-valid".to_string(),
        refresh: "refresh".to_string(),
        expires: u64::MAX, // far future
    };
    storage.set("anthropic", Credential::OAuth(oauth)).unwrap();

    let key = storage
        .get_api_key("anthropic", "GOLEM_TEST_NONEXISTENT_VAR")
        .await
        .unwrap();
    assert_eq!(key, Some("sk-ant-oat01-valid".to_string()));
}

#[tokio::test]
async fn get_api_key_falls_back_to_env() {
    let (storage, _dir) = temp_storage();
    // No credential stored, set env var
    unsafe { std::env::set_var("GOLEM_TEST_API_KEY", "sk-from-env") };

    let key = storage
        .get_api_key("anthropic", "GOLEM_TEST_API_KEY")
        .await
        .unwrap();
    assert_eq!(key, Some("sk-from-env".to_string()));

    unsafe { std::env::remove_var("GOLEM_TEST_API_KEY") };
}

#[tokio::test]
async fn get_api_key_returns_none_when_nothing() {
    let (storage, _dir) = temp_storage();

    let key = storage
        .get_api_key("anthropic", "GOLEM_TEST_NONEXISTENT_VAR")
        .await
        .unwrap();
    assert_eq!(key, None);
}

#[tokio::test]
async fn get_api_key_ignores_empty_env() {
    let (storage, _dir) = temp_storage();
    unsafe { std::env::set_var("GOLEM_TEST_EMPTY_KEY", "") };

    let key = storage
        .get_api_key("anthropic", "GOLEM_TEST_EMPTY_KEY")
        .await
        .unwrap();
    assert_eq!(key, None);

    unsafe { std::env::remove_var("GOLEM_TEST_EMPTY_KEY") };
}

#[tokio::test]
async fn get_api_key_credential_takes_priority_over_env() {
    let (storage, _dir) = temp_storage();
    storage
        .set(
            "anthropic",
            Credential::ApiKey {
                key: "from-file".to_string(),
            },
        )
        .unwrap();
    unsafe { std::env::set_var("GOLEM_TEST_PRIORITY_KEY", "from-env") };

    let key = storage
        .get_api_key("anthropic", "GOLEM_TEST_PRIORITY_KEY")
        .await
        .unwrap();
    assert_eq!(key, Some("from-file".to_string()));

    unsafe { std::env::remove_var("GOLEM_TEST_PRIORITY_KEY") };
}

// ── OAuth URL ─────────────────────────────────────────────────────

#[test]
fn authorize_url_pkce_is_valid() {
    let (url, verifier) = build_authorize_url();

    let challenge = url
        .split('&')
        .find(|p| p.starts_with("code_challenge="))
        .unwrap()
        .strip_prefix("code_challenge=")
        .unwrap();

    assert!(verify_pkce(&verifier, challenge));
}

#[test]
fn authorize_url_is_unique_per_call() {
    let (url1, v1) = build_authorize_url();
    let (url2, v2) = build_authorize_url();

    assert_ne!(url1, url2);
    assert_ne!(v1, v2);
}
