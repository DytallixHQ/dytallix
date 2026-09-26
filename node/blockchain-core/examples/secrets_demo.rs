//! Example demonstrating the secrets abstraction usage
//!
//! This example shows how to use the secrets system in real code.

use dytallix_node::secrets::config::ProviderConfig;
use dytallix_node::secrets::{SecretConfig, SecretManager};
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("=== Dytallix Secrets Management Demo ===\n");

    // Example 1: Using default configuration from environment
    println!("1. Creating SecretManager from environment configuration...");
    match create_manager_from_env().await {
        Ok(()) => println!("✓ Environment-based configuration works"),
        Err(e) => println!("✗ Error: {}", e),
    }

    // Example 2: Using explicit configuration
    println!("\n2. Creating SecretManager with explicit configuration...");
    match create_manager_explicit().await {
        Ok(()) => println!("✓ Explicit configuration works"),
        Err(e) => println!("✗ Error: {}", e),
    }

    // Example 3: Retrieving secrets with fallback
    println!("\n3. Testing secret retrieval with fallback...");
    test_secret_retrieval().await;

    // Example 4: Health checks
    println!("\n4. Testing provider health checks...");
    test_health_checks().await;

    println!("\n=== Demo Complete ===");
    Ok(())
}

async fn create_manager_from_env() -> Result<(), Box<dyn std::error::Error>> {
    // Set some example environment variables
    env::set_var("DYTALLIX_USE_VAULT", "true");
    env::set_var("DYTALLIX_VAULT_URL", "http://localhost:8200");
    env::set_var("DYTALLIX_VAULT_TOKEN", "stub_token");
    env::set_var("DYTALLIX_ENVIRONMENT", "dev");

    let mut manager = SecretManager::from_env()?;
    manager.initialize().await?;

    println!("  Providers: {:?}", manager.get_provider_info().len());
    Ok(())
}

async fn create_manager_explicit() -> Result<(), Box<dyn std::error::Error>> {
    use ProviderConfig::*;

    let config = SecretConfig {
        provider_order: vec![
            Vault {
                url: "http://localhost:8200".to_string(),
                token: "stub_token".to_string(),
                mount_path: "secret".to_string(),
                environment: "dev".to_string(),
                stub_mode: true,
            },
            Environment {
                prefix: Some("DYTALLIX_".to_string()),
                case_sensitive: false,
            },
        ],
        timeout_seconds: 30,
        enable_caching: false,
        cache_ttl_seconds: 300,
    };

    let mut manager = SecretManager::new(config)?;
    manager.initialize().await?;

    println!("  Providers: {:?}", manager.get_provider_info().len());
    Ok(())
}

async fn test_secret_retrieval() {
    // Set up test environment variables
    env::set_var("DYTALLIX_DATABASE_PASSWORD", "env_secret_password");
    env::set_var("DYTALLIX_API_KEY", "env_api_key_12345");

    let mut manager = match SecretManager::from_env() {
        Ok(m) => m,
        Err(e) => {
            println!("  ✗ Failed to create manager: {}", e);
            return;
        }
    };

    if let Err(e) = manager.initialize().await {
        println!("  ✗ Failed to initialize manager: {}", e);
        return;
    }

    // Test retrieving secrets
    let test_cases = vec![
        ("database/password", "Should be found in Vault stub"),
        ("api/api_key", "Should be found in Vault stub"),
        ("DATABASE_PASSWORD", "Should be found in environment"),
        ("API_KEY", "Should be found in environment"),
        ("NONEXISTENT_SECRET", "Should not be found"),
    ];

    for (secret_name, description) in test_cases {
        match manager.get_secret(secret_name).await {
            Ok(value) => {
                let truncated = if value.len() > 20 {
                    format!("{}...", &value[..20])
                } else {
                    value
                };
                println!("  ✓ {}: {} = {}", secret_name, description, truncated);
            }
            Err(e) => {
                println!("  ✗ {}: {} - {}", secret_name, description, e);
            }
        }
    }

    // Test with default value
    let default_value = manager
        .get_secret_or_default("NONEXISTENT_SECRET", "default_value")
        .await;
    println!(
        "  ✓ get_secret_or_default: NONEXISTENT_SECRET = {}",
        default_value
    );
}

async fn test_health_checks() {
    let mut manager = match SecretManager::from_env() {
        Ok(m) => m,
        Err(e) => {
            println!("  ✗ Failed to create manager: {}", e);
            return;
        }
    };

    if let Err(e) = manager.initialize().await {
        println!("  ✗ Failed to initialize manager: {}", e);
        return;
    }

    let health = manager.health_check().await;
    for (provider, is_healthy) in health {
        let status = if is_healthy {
            "✓ Healthy"
        } else {
            "✗ Unhealthy"
        };
        println!(
            "  {} Provider '{}': {}",
            if is_healthy { "✓" } else { "✗" },
            provider,
            status
        );
    }
}
