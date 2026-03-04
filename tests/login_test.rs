use golem::auth::oauth::OAuthCredentials;
use golem::auth::storage::{AuthStorage, Credential};
use golem::commands::{CommandRegistry, CommandResult, SessionInfo};
use golem::config::Config;
use golem::debug::DebugMode;
use golem::provider::{
    Provider, all_login_providers, build_provider, build_provider_by_id, provider_config_by_id,
};
use golem::thinker::TokenUsage;

fn test_info(provider: &str) -> SessionInfo<'_> {
    SessionInfo {
        provider,
        model: "test-model",
        auth_status: "not authenticated",
        shell_mode: "read-only",
        tools: &[],
        usage: TokenUsage::default(),
        db_path: ":memory:",
        engine: None,
        debug: DebugMode::default(),
    }
}

// ── all_login_providers ───────────────────────────────────────────

#[test]
fn login_providers_excludes_human() {
    let ids: Vec<&str> = all_login_providers().iter().map(|p| p.id()).collect();
    assert!(!ids.contains(&"human"));
}

#[test]
fn login_providers_includes_all_configurable_variants() {
    use clap::ValueEnum;

    let login_ids: Vec<&str> = all_login_providers().iter().map(|p| p.id()).collect();

    for variant in Provider::value_variants() {
        if let Some(config) = provider_config_by_id(variant.to_possible_value().unwrap().get_name())
        {
            assert!(
                login_ids.contains(&config.id()),
                "Provider {:?} missing from all_login_providers()",
                variant
            );
        }
    }
}

#[test]
fn login_providers_ids_are_unique() {
    let mut ids: Vec<&str> = all_login_providers().iter().map(|p| p.id()).collect();
    let original_len = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), original_len, "duplicate provider ids found");
}

// ── /login command dispatch ───────────────────────────────────────

#[tokio::test]
async fn login_command_is_registered() {
    let reg = CommandRegistry::new();
    assert!(reg.names().contains(&"/login"));
}

#[tokio::test]
async fn login_command_appears_in_help() {
    let reg = CommandRegistry::new();
    let help = reg.help_text();
    assert!(help.contains("/login"), "help text missing /login");
    assert!(
        help.contains("provider"),
        "help text should mention provider selection"
    );
}

/// /login reads from stdin; with no stdin attached it gets EOF and
/// returns Handled without blocking or panicking.
#[tokio::test]
async fn login_command_handles_eof_gracefully() {
    let reg = CommandRegistry::new();
    let info = test_info("anthropic");

    // dispatch /login — stdin is closed in test environment,
    // so the async readline returns None (EOF) → Handled
    let result = reg.dispatch("/login", &info).await;
    assert!(
        matches!(result, CommandResult::Handled),
        "expected Handled on stdin EOF, got: {result:?}"
    );
}

// ── build_provider ────────────────────────────────────────────────

// ── build_provider basics ─────────────────────────────────────────

#[test]
fn build_provider_anthropic() {
    // Ensure no env var interference from parallel tests
    let had_key = std::env::var("ANTHROPIC_API_KEY").ok();
    unsafe { std::env::remove_var("ANTHROPIC_API_KEY") };

    let setup =
        build_provider(&Provider::Anthropic, ":memory:", None, DebugMode::default()).unwrap();

    // Restore if it was set
    if let Some(key) = had_key {
        unsafe { std::env::set_var("ANTHROPIC_API_KEY", key) };
    }

    assert_eq!(setup.name, "anthropic");
    assert!(!setup.model.is_empty());
    assert_eq!(setup.auth_status, "not authenticated");
}

#[test]
fn build_provider_google() {
    let had_key = std::env::var("GEMINI_API_KEY").ok();
    unsafe { std::env::remove_var("GEMINI_API_KEY") };

    let setup = build_provider(&Provider::Google, ":memory:", None, DebugMode::default()).unwrap();

    if let Some(key) = had_key {
        unsafe { std::env::set_var("GEMINI_API_KEY", key) };
    }

    assert_eq!(setup.name, "google");
    assert!(!setup.model.is_empty());
    assert_eq!(setup.auth_status, "not authenticated");
}

#[test]
fn build_provider_human_ignores_model() {
    let setup = build_provider(&Provider::Human, ":memory:", None, DebugMode::default()).unwrap();
    assert_eq!(setup.name, "human");
    assert_eq!(setup.auth_status, "N/A");
}

// ── build_provider auth status detection ──────────────────────────

#[test]
fn build_provider_detects_oauth_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("auth-oauth.db");
    let db_str = db_path.to_str().unwrap();

    let storage = AuthStorage::open(db_str).unwrap();
    storage
        .set(
            "anthropic",
            Credential::OAuth(OAuthCredentials {
                access: "token".to_string(),
                refresh: "refresh".to_string(),
                expires: u64::MAX,
                client_hint: None,
            }),
        )
        .unwrap();
    drop(storage);

    let setup = build_provider(&Provider::Anthropic, db_str, None, DebugMode::default()).unwrap();
    assert_eq!(setup.auth_status, "OAuth ✓");
}

#[test]
fn build_provider_detects_api_key_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("auth-apikey.db");
    let db_str = db_path.to_str().unwrap();

    let storage = AuthStorage::open(db_str).unwrap();
    storage
        .set(
            "google",
            Credential::ApiKey {
                key: "test-key".to_string(),
            },
        )
        .unwrap();
    drop(storage);

    let setup = build_provider(&Provider::Google, db_str, None, DebugMode::default()).unwrap();
    assert_eq!(setup.auth_status, "API key ✓");
}

/// Env var auth detection. Uses a dedicated env var name that no other
/// test touches, by going through the Google provider + GEMINI_API_KEY.
/// The build_provider_google test clears GEMINI_API_KEY, but we save/restore
/// to minimise races (these still share a process, so env vars are inherently racy
/// under parallel test execution — acceptable tradeoff for an integration test).
#[test]
fn build_provider_detects_env_var() {
    let var = "GEMINI_API_KEY";
    let had = std::env::var(var).ok();

    unsafe { std::env::set_var(var, "test-key-from-env") };
    let setup = build_provider(&Provider::Google, ":memory:", None, DebugMode::default()).unwrap();

    // Restore
    match had {
        Some(v) => unsafe { std::env::set_var(var, v) },
        None => unsafe { std::env::remove_var(var) },
    }

    assert_eq!(setup.auth_status, "API key (env) ✓");
}

// ── build_provider model resolution ───────────────────────────────

#[test]
fn build_provider_uses_cli_model_override() {
    let setup = build_provider(
        &Provider::Anthropic,
        ":memory:",
        Some("custom-model".to_string()),
        DebugMode::default(),
    )
    .unwrap();
    assert_eq!(setup.model, "custom-model");
}

#[test]
fn build_provider_reads_model_from_config_db() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("config-model.db");
    let db_str = db_path.to_str().unwrap();

    let config = Config::open(db_str).unwrap();
    config.set("model", "persisted-model").unwrap();
    drop(config);

    let setup = build_provider(&Provider::Anthropic, db_str, None, DebugMode::default()).unwrap();
    assert_eq!(setup.model, "persisted-model");
}

#[test]
fn build_provider_cli_model_overrides_config_db() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("config-override.db");
    let db_str = db_path.to_str().unwrap();

    let config = Config::open(db_str).unwrap();
    config.set("model", "from-db").unwrap();
    drop(config);

    let setup = build_provider(
        &Provider::Anthropic,
        db_str,
        Some("from-cli".to_string()),
        DebugMode::default(),
    )
    .unwrap();
    assert_eq!(setup.model, "from-cli");
}

// ── logout round-trip per provider ────────────────────────────────

#[test]
fn logout_clears_credentials_for_each_login_provider() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("logout-all.db");
    let db_str = db_path.to_str().unwrap();

    let storage = AuthStorage::open(db_str).unwrap();

    // Store credentials for every login provider
    for provider in all_login_providers() {
        storage
            .set(
                provider.id(),
                Credential::ApiKey {
                    key: format!("key-{}", provider.id()),
                },
            )
            .unwrap();
    }

    // Verify all stored
    for provider in all_login_providers() {
        assert!(
            storage.get(provider.id()).unwrap().is_some(),
            "credential for {} should exist before logout",
            provider.id()
        );
    }

    // Logout each provider via the shared auth::logout
    for provider in all_login_providers() {
        golem::auth::logout(db_str, provider.id()).unwrap();
    }

    // Verify all gone
    for provider in all_login_providers() {
        assert!(
            storage.get(provider.id()).unwrap().is_none(),
            "credential for {} should be gone after logout",
            provider.id()
        );
    }
}

#[test]
fn logout_one_provider_preserves_others() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("logout-preserve.db");
    let db_str = db_path.to_str().unwrap();

    let providers = all_login_providers();
    assert!(
        providers.len() >= 2,
        "need at least 2 providers for this test"
    );

    let storage = AuthStorage::open(db_str).unwrap();
    for p in &providers {
        storage
            .set(
                p.id(),
                Credential::ApiKey {
                    key: format!("key-{}", p.id()),
                },
            )
            .unwrap();
    }

    // Logout only the first provider
    golem::auth::logout(db_str, providers[0].id()).unwrap();

    assert!(
        storage.get(providers[0].id()).unwrap().is_none(),
        "{} should be logged out",
        providers[0].id()
    );

    // All others should remain
    for p in &providers[1..] {
        assert!(
            storage.get(p.id()).unwrap().is_some(),
            "{} should still have credentials",
            p.id()
        );
    }
}

// ── build_provider_by_id ──────────────────────────────────────────

#[test]
fn build_provider_by_id_returns_anthropic() {
    let setup = build_provider_by_id("anthropic", ":memory:", None, DebugMode::default()).unwrap();
    assert_eq!(setup.name, "anthropic");
    assert!(!setup.model.is_empty());
}

#[test]
fn build_provider_by_id_returns_google() {
    let setup = build_provider_by_id("google", ":memory:", None, DebugMode::default()).unwrap();
    assert_eq!(setup.name, "google");
    assert!(!setup.model.is_empty());
}

#[test]
fn build_provider_by_id_unknown_returns_error() {
    let result = build_provider_by_id("unknown", ":memory:", None, DebugMode::default());
    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(err.to_string().contains("unknown"));
}

#[test]
fn build_provider_by_id_human_returns_error() {
    // "human" has no ProviderConfig, so it should fail
    let result = build_provider_by_id("human", ":memory:", None, DebugMode::default());
    assert!(result.is_err());
}

#[test]
fn build_provider_by_id_uses_default_model_not_config_db() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("by-id-model.db");
    let db_str = db_path.to_str().unwrap();

    // Store a model in config DB
    let config = Config::open(db_str).unwrap();
    config.set("model", "persisted-model").unwrap();
    drop(config);

    // build_provider_by_id should ignore the config DB and use the provider's default
    let setup = build_provider_by_id("anthropic", db_str, None, DebugMode::default()).unwrap();
    assert_ne!(
        setup.model, "persisted-model",
        "build_provider_by_id should use provider default, not config DB"
    );
}

#[test]
fn build_provider_by_id_detects_auth_status() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("by-id-auth.db");
    let db_str = db_path.to_str().unwrap();

    let storage = AuthStorage::open(db_str).unwrap();
    storage
        .set(
            "google",
            Credential::OAuth(OAuthCredentials {
                access: "token".to_string(),
                refresh: "refresh".to_string(),
                expires: u64::MAX,
                client_hint: None,
            }),
        )
        .unwrap();
    drop(storage);

    let setup = build_provider_by_id("google", db_str, None, DebugMode::default()).unwrap();
    assert_eq!(setup.auth_status, "OAuth ✓");
}

#[test]
fn build_provider_by_id_with_model_override() {
    let setup = build_provider_by_id(
        "anthropic",
        ":memory:",
        Some("custom-model".to_string()),
        DebugMode::default(),
    )
    .unwrap();
    assert_eq!(setup.model, "custom-model");
    assert_eq!(setup.name, "anthropic");
}
