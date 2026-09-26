//! Integration tests for the secrets management system

use dytallix_node::secrets::{
    config::ProviderConfig,
    providers::{EnvProvider, SecretProvider, VaultProvider},
    SecretConfig, SecretError, SecretManager,
};
use std::env;
use tokio;

#[tokio::test]
async fn test_env_provider_basic() {
    let mut provider = EnvProvider::new(Some("TEST_".to_string()), false);
    provider.initialize().await.unwrap();

    // Set up test environment
    env::set_var("TEST_SECRET_KEY", "test_value");

    // Test retrieval
    let result = provider.get_secret("SECRET_KEY").await.unwrap();
    assert_eq!(result, Some("test_value".to_string()));

    // Test not found
    let result = provider.get_secret("NONEXISTENT").await.unwrap();
    assert_eq!(result, None);

    // Clean up
    env::remove_var("TEST_SECRET_KEY");
}

#[tokio::test]
async fn test_env_provider_case_sensitivity() {
    let mut provider = EnvProvider::new(None, true);
    provider.initialize().await.unwrap();

    env::set_var("CaseSensitive", "case_value");

    // Should find exact match
    let result = provider.get_secret("CaseSensitive").await.unwrap();
    assert_eq!(result, Some("case_value".to_string()));

    // Should not find different case
    let result = provider.get_secret("casesensitive").await.unwrap();
    assert_eq!(result, None);

    env::remove_var("CaseSensitive");
}

#[tokio::test]
async fn test_vault_provider_stub() {
    let mut provider = VaultProvider::new(
        "http://localhost:8200".to_string(),
        "stub_token".to_string(),
        "secret".to_string(),
        "dev".to_string(),
    );

    provider.initialize().await.unwrap();

    // Test built-in stub data
    let result = provider.get_secret("database/host").await.unwrap();
    assert_eq!(result, Some("localhost".to_string()));

    let result = provider.get_secret("api/api_key").await.unwrap();
    assert!(result.is_some());
    assert!(result.unwrap().contains("stub"));

    // Test not found
    let result = provider.get_secret("nonexistent/key").await.unwrap();
    assert_eq!(result, None);
}

#[tokio::test]
async fn test_secret_manager_provider_order() {
    // Set up environment variable
    env::set_var("DYTALLIX_TEST_SECRET", "env_value");

    let config = SecretConfig {
        provider_order: vec![
            ProviderConfig::Vault {
                url: "http://localhost:8200".to_string(),
                token: "stub_token".to_string(),
                mount_path: "secret".to_string(),
                environment: "dev".to_string(),
                stub_mode: true,
            },
            ProviderConfig::Environment {
                prefix: Some("DYTALLIX_".to_string()),
                case_sensitive: false,
            },
        ],
        timeout_seconds: 5,
        enable_caching: false,
        cache_ttl_seconds: 60,
    };

    let mut manager = SecretManager::new(config).unwrap();
    manager.initialize().await.unwrap();

    // Should find vault secret first (vault has priority)
    let result = manager.get_secret("database/host").await.unwrap();
    assert_eq!(result, "localhost");

    // Should find env secret when not in vault
    let result = manager.get_secret("TEST_SECRET").await.unwrap();
    assert_eq!(result, "env_value");

    // Should fail when not in any provider
    let result = manager.get_secret("TOTALLY_NONEXISTENT").await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), SecretError::NotFound { .. }));

    env::remove_var("DYTALLIX_TEST_SECRET");
}

#[tokio::test]
async fn test_secret_manager_with_default() {
    let config = SecretConfig::for_testing();
    let mut manager = SecretManager::new(config).unwrap();
    manager.initialize().await.unwrap();

    // Should return default for nonexistent secret
    let result = manager
        .get_secret_or_default("NONEXISTENT", "default_value")
        .await;
    assert_eq!(result, "default_value");

    // Should return actual value when found
    env::set_var("TEST_EXISTING_SECRET", "actual_value");
    let result = manager
        .get_secret_or_default("EXISTING_SECRET", "default_value")
        .await;
    assert_eq!(result, "actual_value");

    env::remove_var("TEST_EXISTING_SECRET");
}

#[tokio::test]
async fn test_health_checks() {
    let config = SecretConfig::for_testing();
    let mut manager = SecretManager::new(config).unwrap();
    manager.initialize().await.unwrap();

    let health = manager.health_check().await;
    assert!(!health.is_empty());

    // Environment provider should always be healthy
    assert_eq!(health.get("environment"), Some(&true));
}

#[tokio::test]
async fn test_provider_info() {
    let config = SecretConfig::for_testing();
    let mut manager = SecretManager::new(config).unwrap();
    manager.initialize().await.unwrap();

    let info = manager.get_provider_info();
    assert!(!info.is_empty());

    // Should have type information
    let env_info = &info[0];
    assert_eq!(env_info.get("type"), Some(&"environment".to_string()));
}

#[tokio::test]
async fn test_config_validation() {
    // Valid config should pass
    let valid_config = SecretConfig::for_testing();
    assert!(valid_config.validate().is_ok());

    // Empty provider list should fail
    let mut invalid_config = SecretConfig::for_testing();
    invalid_config.provider_order.clear();
    assert!(invalid_config.validate().is_err());

    // Zero timeout should fail
    let mut invalid_config = SecretConfig::for_testing();
    invalid_config.timeout_seconds = 0;
    assert!(invalid_config.validate().is_err());
}

#[tokio::test]
async fn test_config_from_env() {
    // Test with Vault disabled
    env::set_var("DYTALLIX_USE_VAULT", "false");

    let config = SecretConfig::from_env().unwrap();
    assert_eq!(config.provider_order.len(), 1);
    assert!(matches!(
        config.provider_order[0],
        ProviderConfig::Environment { .. }
    ));

    // Test with Vault enabled
    env::set_var("DYTALLIX_USE_VAULT", "true");
    env::set_var("DYTALLIX_VAULT_URL", "http://test:8200");
    env::set_var("DYTALLIX_VAULT_TOKEN", "test_token");

    let config = SecretConfig::from_env().unwrap();
    assert_eq!(config.provider_order.len(), 2);
    assert!(matches!(
        config.provider_order[0],
        ProviderConfig::Vault { .. }
    ));
    assert!(matches!(
        config.provider_order[1],
        ProviderConfig::Environment { .. }
    ));

    // Clean up
    env::remove_var("DYTALLIX_USE_VAULT");
    env::remove_var("DYTALLIX_VAULT_URL");
    env::remove_var("DYTALLIX_VAULT_TOKEN");
}
