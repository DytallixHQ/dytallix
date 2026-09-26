# Dytallix Secrets Management System

[Docs hub](../docs/README.md) | [Repository map](../docs/repository-map.md) | [Build and run](../docs/build-and-run.md)

A pluggable secrets loading abstraction that provides secure, configurable access to secrets from multiple sources with easy migration path to production secret managers.

## Overview

The secrets management system eliminates hard-coded secrets and provides a unified interface for loading sensitive configuration from various backends:

- **Environment Variables** - Always available fallback
- **HashiCorp Vault** - Stub implementation (MVP), extensible to real Vault
- **Future providers** - AWS Secrets Manager, Azure Key Vault, etc.

## Key Features

- 🔐 **No hard-coded secrets** - All sensitive data loaded from secure sources
- 🔄 **Pluggable providers** - Easy to switch between secret backends
- 📊 **Provider priorities** - Try Vault first, fallback to environment
- ⚡ **Async/await ready** - Non-blocking secret retrieval
- 🛡️ **Error handling** - Graceful degradation when providers are unavailable
- 🎯 **Production ready** - Clear migration path from stub to real implementations
- 🧪 **Testing support** - Mock providers and test configurations

## Quick Start

### Basic Usage

```rust
use dytallix_node::secrets::SecretManager;

// Create and initialize manager from environment
let mut manager = SecretManager::from_env()?;
manager.initialize().await?;

// Get a secret (tries all providers in order)
let db_password = manager.get_secret("database/password").await?;

// Get secret with fallback
let api_key = manager.get_secret_or_default("api/api_key", "development_key").await;
```

### Configuration Loading

```rust
use dytallix_node::config::{ConfigLoader, NodeConfig};

// Load complete node configuration using secrets
let config_loader = ConfigLoader::new().await?;
let node_config = config_loader.load_node_config().await?;

// Secrets are automatically loaded from vault/environment
println!("Database URL: {}", node_config.database_url);
println!("API Key: {}", node_config.api_key);
```

## Configuration

### Environment Variables

The system can be configured via environment variables:

```bash
# Enable Vault integration
export DYTALLIX_USE_VAULT=true
export DYTALLIX_VAULT_URL=http://localhost:8200
export DYTALLIX_VAULT_TOKEN=your_vault_token
export DYTALLIX_ENVIRONMENT=dev

# Optional settings
export DYTALLIX_SECRET_TIMEOUT=30
export DYTALLIX_SECRET_CACHE=false
export DYTALLIX_SECRET_CACHE_TTL=300
```

### Programmatic Configuration

```rust
use dytallix_node::secrets::{SecretConfig, ProviderConfig};

let config = SecretConfig {
    provider_order: vec![
        ProviderConfig::Vault {
            url: "http://vault.example.com:8200".to_string(),
            token: "hvs.token123".to_string(),
            mount_path: "secret".to_string(),
            environment: "prod".to_string(),
            stub_mode: false,
        },
        ProviderConfig::Environment {
            prefix: Some("DYTALLIX_".to_string()),
            case_sensitive: false,
        },
    ],
    timeout_seconds: 30,
    enable_caching: false,
    cache_ttl_seconds: 300,
};

let mut manager = SecretManager::new(config)?;
```

## Secret Naming Conventions

### Vault Secrets (Hierarchical)

Vault secrets use hierarchical paths following the pattern established in `devops/secrets-management/vault-setup.sh`:

```
secret/dytallix/{environment}/database/host
secret/dytallix/{environment}/database/port
secret/dytallix/{environment}/database/username
secret/dytallix/{environment}/database/password
secret/dytallix/{environment}/api/api_key
secret/dytallix/{environment}/api/jwt_secret
secret/dytallix/{environment}/config/log_level
```

### Environment Variables (Flat)

Environment variables use the `DYTALLIX_` prefix with uppercase names:

```bash
DYTALLIX_DATABASE_HOST=localhost
DYTALLIX_DATABASE_PORT=5432
DYTALLIX_DATABASE_USERNAME=dytallix
DYTALLIX_DATABASE_PASSWORD=secret123
DYTALLIX_API_KEY=api_key_12345
DYTALLIX_JWT_SECRET=jwt_secret_67890
DYTALLIX_LOG_LEVEL=info
```

## Provider Details

### Environment Provider

- **Always available** - No external dependencies
- **Supports prefixes** - Default: `DYTALLIX_`
- **Case handling** - Configurable case sensitivity
- **Use case** - Development, testing, simple deployments

```rust
let provider = EnvProvider::with_dytallix_prefix();
```

### Vault Provider (Stub)

- **Development ready** - No real network calls in stub mode
- **Production extensible** - Easy migration to real Vault client
- **KV v2 compatible** - Follows Vault KV v2 API patterns
- **Environment aware** - Automatically namespaces by environment

```rust
let provider = VaultProvider::from_env()?;
```

## Stub vs Production Mode

### Development (Stub Mode)

The Vault provider in stub mode simulates a real Vault deployment without network calls:

- ✅ No Vault server required
- ✅ Consistent with real Vault data structure
- ✅ Perfect for development and testing
- ✅ Safe default for dev environments

### Production Migration

To migrate to real Vault in production:

1. **Replace the stub implementation** with real Vault client library
2. **Update initialization** to use actual HTTP client
3. **Add authentication** (token, AWS IAM, K8s service account, etc.)
4. **Configure TLS** and other production settings

The interface remains exactly the same - only the implementation changes.

## Error Handling

The system provides comprehensive error handling:

```rust
use dytallix_node::secrets::SecretError;

match manager.get_secret("api/key").await {
    Ok(value) => println!("Got secret: {}", value),
    Err(SecretError::NotFound { name }) => {
        eprintln!("Secret '{}' not found in any provider", name);
    }
    Err(SecretError::ProviderError { provider, message }) => {
        eprintln!("Provider '{}' failed: {}", provider, message);
    }
    Err(SecretError::NetworkError { message }) => {
        eprintln!("Network error: {}", message);
    }
    // ... handle other error types
}
```

## Health Monitoring

Check the health of all configured providers:

```rust
let health = manager.health_check().await;
for (provider, is_healthy) in health {
    if is_healthy {
        println!("✓ Provider '{}' is healthy", provider);
    } else {
        println!("✗ Provider '{}' is unhealthy", provider);
    }
}
```

## CLI Integration

The secrets system integrates with the existing CLI:

```bash
# Get a secret value
dcli secrets get database/password

# List available secrets
dcli secrets list --pattern "database/*"

# Check provider health
dcli secrets health --verbose

# Test configuration
dcli secrets test --comprehensive
```

## Testing

### Unit Tests

```rust
#[tokio::test]
async fn test_secret_retrieval() {
    env::set_var("TEST_SECRET", "test_value");

    let config = SecretConfig::for_testing();
    let mut manager = SecretManager::new(config).unwrap();
    manager.initialize().await.unwrap();

    let value = manager.get_secret("SECRET").await.unwrap();
    assert_eq!(value, "test_value");
}
```

### Integration Tests

See `tests/secrets_integration_test.rs` for comprehensive integration tests covering:

- Provider initialization
- Secret retrieval with fallback
- Error handling
- Health checks
- Configuration validation

## Migration Guide

### From Hard-coded Secrets

**Before:**
```rust
let api_key = "hardcoded_api_key_123";
let db_url = "postgresql://user:password@localhost/db";
```

**After:**
```rust
let mut manager = SecretManager::from_env()?;
manager.initialize().await?;

let api_key = manager.get_secret("api/api_key").await?;
let db_password = manager.get_secret("database/password").await?;
let db_url = format!("postgresql://user:{}@localhost/db", db_password);
```

### From Environment Variables Only

**Before:**
```rust
let api_key = env::var("DYTALLIX_API_KEY")?;
let db_password = env::var("DYTALLIX_DB_PASSWORD")?;
```

**After:**
```rust
let mut manager = SecretManager::from_env()?;
manager.initialize().await?;

// Now supports both Vault and environment with priority
let api_key = manager.get_secret("api/api_key").await?;
let db_password = manager.get_secret("database/password").await?;
```

## Production Deployment

### Environment Setup

```bash
# Production Vault configuration
export DYTALLIX_USE_VAULT=true
export DYTALLIX_VAULT_URL=https://vault.mycompany.com
export DYTALLIX_VAULT_TOKEN=${VAULT_TOKEN}  # From CI/CD or service account
export DYTALLIX_ENVIRONMENT=prod

# Security settings
export DYTALLIX_SECRET_TIMEOUT=10
export DYTALLIX_SECRET_CACHE=false  # Disable caching for security
```

### Vault Setup

Follow the existing Vault setup in `devops/secrets-management/vault-setup.sh` to create the proper secret structure.

### Monitoring

- Monitor provider health via the health check API
- Set up alerts for secret retrieval failures
- Log secret access for audit purposes (without values)

## Security Considerations

- 🔒 **No plaintext storage** - Secrets never stored in plaintext in code
- 🚫 **No logging of values** - Secret values are never logged
- ⏱️ **No caching by default** - Secrets aren't cached unless explicitly enabled
- 🔄 **Secure defaults** - All configurations default to secure settings
- 📝 **Audit support** - Provider access is logged for security monitoring

## Future Roadmap

- **AWS Secrets Manager** provider
- **Azure Key Vault** provider
- **Google Secret Manager** provider
- **Kubernetes Secrets** provider
- **Secret rotation** support
- **Hot reloading** of configuration
- **Metrics and monitoring** integration

## Examples

See the `examples/` directory for complete examples:

- `secrets_demo.rs` - Basic usage demonstration
- Integration with existing CLI commands
- Configuration loading patterns
- Error handling strategies

## Contributing

When adding new secret providers:

1. Implement the `SecretProvider` trait
2. Add configuration support in `ProviderConfig`
3. Update the `SecretManager::create_provider()` method
4. Add comprehensive tests
5. Update documentation with usage examples

---

For more details, see the API documentation and integration tests.
