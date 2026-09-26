//! Secret manager implementation
//!
//! The SecretManager coordinates between multiple secret providers and implements
//! caching, timeout handling, and provider failover logic.

use crate::secrets::{
    config::{ProviderConfig, SecretConfig},
    providers::{EnvProvider, SecretProvider, VaultProvider},
    SecretError, SecretResult,
};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

/// Cached secret entry
#[derive(Debug, Clone)]
struct CachedSecret {
    value: String,
    expires_at: Instant,
}

/// Main secret manager that coordinates providers
pub struct SecretManager {
    providers: Vec<Box<dyn SecretProvider>>,
    config: SecretConfig,
    cache: Arc<RwLock<HashMap<String, CachedSecret>>>,
    initialized: bool,
    unavailable_provider: bool,
}

impl SecretManager {
    /// Create a new SecretManager with the given configuration
    pub fn new(config: SecretConfig) -> SecretResult<Self> {
        config.validate()?;

        Ok(Self {
            providers: Vec::new(),
            config,
            cache: Arc::new(RwLock::new(HashMap::new())),
            initialized: false,
            unavailable_provider: false,
        })
    }

    /// Create SecretManager from environment variables
    pub fn from_env() -> SecretResult<Self> {
        let config = SecretConfig::from_env()?;
        Self::new(config)
    }

    /// Create SecretManager with default configuration
    pub fn with_defaults() -> SecretResult<Self> {
        Self::new(SecretConfig::default())
    }

    /// Initialize the secret manager and all providers
    pub async fn initialize(&mut self) -> SecretResult<()> {
        if self.initialized {
            debug!("SecretManager already initialized");
            return Ok(());
        }

        info!(
            "Initializing SecretManager with {} provider(s)",
            self.config.provider_order.len()
        );

        // Create and initialize providers based on configuration
        for provider_config in &self.config.provider_order {
            let mut provider = self.create_provider(provider_config)?;

            match provider.initialize().await {
                Ok(()) => {
                    info!(
                        "Successfully initialized provider: {}",
                        provider.provider_name()
                    );
                    self.providers.push(provider);
                }
                Err(_) => {
                    self.unavailable_provider = true;
                    warn!(
                        "Failed to initialize provider {}. Skipping.",
                        provider.provider_name()
                    );
                    // Continue with other providers instead of failing completely
                }
            }
        }

        if self.providers.is_empty() {
            return Err(SecretError::ConfigError {
                message: "No secret providers could be initialized".to_string(),
            });
        }

        self.initialized = true;
        info!(
            "SecretManager initialized with {} active provider(s)",
            self.providers.len()
        );
        Ok(())
    }

    /// Get a secret by name, trying providers in order
    pub async fn get_secret(&self, name: &str) -> SecretResult<String> {
        if !self.initialized {
            return Err(SecretError::ConfigError {
                message: "SecretManager not initialized".to_string(),
            });
        }

        // Check cache first if enabled
        if self.config.enable_caching {
            if let Some(cached) = self.get_from_cache(name) {
                debug!("Retrieved secret '{}' from cache", name);
                return Ok(cached);
            }
        }

        debug!("Attempting to retrieve secret: {}", name);

        // Preserve provider failures when no fallback returns a value.
        let mut provider_failed = self.unavailable_provider;
        for provider in &self.providers {
            debug!("Trying provider: {}", provider.provider_name());

            let result = timeout(
                Duration::from_secs(self.config.timeout_seconds),
                provider.get_secret(name),
            )
            .await;

            match result {
                Ok(Ok(Some(value))) => {
                    info!(
                        "Successfully retrieved secret '{}' from provider: {}",
                        name,
                        provider.provider_name()
                    );

                    // Cache the result if caching is enabled
                    if self.config.enable_caching {
                        self.cache_secret(name, &value);
                    }

                    return Ok(value);
                }
                Ok(Ok(None)) => {
                    debug!(
                        "Secret '{}' not found in provider: {}",
                        name,
                        provider.provider_name()
                    );
                    // Continue to next provider
                }
                Ok(Err(_)) => {
                    provider_failed = true;
                    warn!(
                        "Provider {} failed for secret '{}'. Trying next provider.",
                        provider.provider_name(),
                        name
                    );
                }
                Err(_) => {
                    provider_failed = true;
                    warn!(
                        "Provider {} timed out for secret '{}'. Trying next provider.",
                        provider.provider_name(),
                        name
                    );
                }
            }
        }

        if provider_failed {
            return Err(SecretError::ProviderError {
                provider: "secret-manager".into(),
                message: format!("No provider returned '{name}'; at least one provider failed"),
            });
        }
        error!("Secret '{}' not found in any provider", name);
        Err(SecretError::NotFound {
            name: name.to_string(),
        })
    }

    /// Compatibility fallback on any lookup error. Do not use for required settings.
    pub async fn get_secret_or_default(&self, name: &str, default: &str) -> String {
        match self.get_secret(name).await {
            Ok(value) => value,
            Err(_) => {
                debug!("Using default value for secret: {}", name);
                default.to_string()
            }
        }
    }

    /// Check health of all providers
    pub async fn health_check(&self) -> HashMap<String, bool> {
        let mut results = HashMap::new();

        for provider in &self.providers {
            let health = timeout(
                Duration::from_secs(5), // Shorter timeout for health checks
                provider.health_check(),
            )
            .await;

            let is_healthy = match health {
                Ok(Ok(healthy)) => healthy,
                _ => false,
            };

            results.insert(provider.provider_name().to_string(), is_healthy);
        }

        results
    }

    /// Get information about all providers
    pub fn get_provider_info(&self) -> Vec<HashMap<String, String>> {
        self.providers.iter().map(|p| p.get_info()).collect()
    }

    /// Clear the secret cache
    pub fn clear_cache(&self) {
        if let Ok(mut cache) = self.cache.write() {
            cache.clear();
            debug!("Secret cache cleared");
        }
    }

    /// Get cache statistics
    pub fn get_cache_stats(&self) -> HashMap<String, usize> {
        let mut stats = HashMap::new();

        if let Ok(cache) = self.cache.read() {
            let now = Instant::now();
            let total = cache.len();
            let expired = cache
                .values()
                .filter(|entry| entry.expires_at <= now)
                .count();

            stats.insert("total_entries".to_string(), total);
            stats.insert("expired_entries".to_string(), expired);
            stats.insert("valid_entries".to_string(), total - expired);
        }

        stats
    }

    // Private helper methods

    fn create_provider(&self, config: &ProviderConfig) -> SecretResult<Box<dyn SecretProvider>> {
        match config {
            ProviderConfig::Environment {
                prefix,
                case_sensitive,
            } => Ok(Box::new(EnvProvider::new(prefix.clone(), *case_sensitive))),
            ProviderConfig::Vault {
                url,
                token,
                mount_path,
                environment,
                stub_mode: _,
            } => {
                // For now, we always use stub mode regardless of the setting
                // In production, this would check the stub_mode flag and create
                // either a real Vault client or the stub implementation
                Ok(Box::new(VaultProvider::new(
                    url.clone(),
                    token.clone(),
                    mount_path.clone(),
                    environment.clone(),
                )))
            }
        }
    }

    fn get_from_cache(&self, name: &str) -> Option<String> {
        if let Ok(cache) = self.cache.read() {
            if let Some(entry) = cache.get(name) {
                if entry.expires_at > Instant::now() {
                    return Some(entry.value.clone());
                }
            }
        }
        None
    }

    fn cache_secret(&self, name: &str, value: &str) {
        if let Ok(mut cache) = self.cache.write() {
            let expires_at = Instant::now() + Duration::from_secs(self.config.cache_ttl_seconds);
            cache.insert(
                name.to_string(),
                CachedSecret {
                    value: value.to_string(),
                    expires_at,
                },
            );
            debug!("Cached secret: {}", name);
        }
    }
}

// Convenience functions for common patterns

/// Create a globally shared secret manager instance
pub async fn create_global_secret_manager() -> SecretResult<SecretManager> {
    let mut manager = SecretManager::from_env()?;
    manager.initialize().await?;
    Ok(manager)
}

/// Get a secret using environment-based configuration
pub async fn get_secret_simple(name: &str) -> SecretResult<String> {
    let mut manager = SecretManager::from_env()?;
    manager.initialize().await?;
    manager.get_secret(name).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::config::SecretConfig;
    use std::env;

    #[tokio::test]
    async fn test_secret_manager_creation() {
        let config = SecretConfig::for_testing();
        let manager = SecretManager::new(config);
        assert!(manager.is_ok());
    }

    #[tokio::test]
    async fn test_secret_manager_initialization() {
        let config = SecretConfig::for_testing();
        let mut manager = SecretManager::new(config).unwrap();
        let result = manager.initialize().await;
        assert!(result.is_ok());
        assert!(manager.initialized);
    }

    #[tokio::test]
    async fn test_environment_secret_retrieval() {
        // Set up test environment variable
        env::set_var("TEST_EXAMPLE_SECRET", "test_value");

        let config = SecretConfig::for_testing();
        let mut manager = SecretManager::new(config).unwrap();
        manager.initialize().await.unwrap();

        let result = manager.get_secret("EXAMPLE_SECRET").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test_value");

        // Clean up
        env::remove_var("TEST_EXAMPLE_SECRET");
    }

    #[tokio::test]
    async fn test_secret_not_found() {
        let config = SecretConfig::for_testing();
        let mut manager = SecretManager::new(config).unwrap();
        manager.initialize().await.unwrap();

        let result = manager.get_secret("NONEXISTENT_SECRET").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SecretError::NotFound { .. }));
    }

    #[tokio::test]
    async fn test_secret_with_default() {
        let config = SecretConfig::for_testing();
        let mut manager = SecretManager::new(config).unwrap();
        manager.initialize().await.unwrap();

        let result = manager
            .get_secret_or_default("NONEXISTENT_SECRET", "default_value")
            .await;
        assert_eq!(result, "default_value");
    }

    #[tokio::test]
    async fn test_health_check() {
        let config = SecretConfig::for_testing();
        let mut manager = SecretManager::new(config).unwrap();
        manager.initialize().await.unwrap();

        let health = manager.health_check().await;
        assert!(!health.is_empty());
        // Environment provider should always be healthy
        assert_eq!(health.get("environment"), Some(&true));
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    #[derive(Default)]
    pub(crate) struct FixtureProvider {
        pub values: HashMap<String, String>,
        pub fail_on: Option<String>,
        pub pending: bool,
    }
    impl FixtureProvider {
        pub fn values(values: &[(&str, &str)]) -> Self {
            Self {
                values: values
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
                ..Default::default()
            }
        }
    }
    #[async_trait::async_trait]
    impl SecretProvider for FixtureProvider {
        fn provider_name(&self) -> &'static str {
            "fixture"
        }
        async fn initialize(&mut self) -> SecretResult<()> {
            Ok(())
        }
        async fn get_secret(&self, name: &str) -> SecretResult<Option<String>> {
            if self.pending {
                std::future::pending::<()>().await;
            }
            if self.fail_on.as_deref() == Some(name) {
                return Err(SecretError::ProviderError {
                    provider: "fixture".into(),
                    message: "private-provider-detail".into(),
                });
            }
            Ok(self.values.get(name).cloned())
        }
        async fn health_check(&self) -> SecretResult<bool> {
            Ok(true)
        }
        fn get_info(&self) -> HashMap<String, String> {
            HashMap::new()
        }
    }
    pub(crate) fn manager(providers: Vec<FixtureProvider>) -> SecretManager {
        SecretManager {
            providers: providers
                .into_iter()
                .map(|p| Box::new(p) as Box<dyn SecretProvider>)
                .collect(),
            config: SecretConfig {
                timeout_seconds: 1,
                ..SecretConfig::for_testing()
            },
            cache: Arc::new(RwLock::new(HashMap::new())),
            initialized: true,
            unavailable_provider: false,
        }
    }
    #[tokio::test]
    async fn failure_and_timeout_are_not_missing_but_fallback_still_works() {
        let failed = || FixtureProvider {
            fail_on: Some("key".into()),
            ..Default::default()
        };
        assert!(matches!(
            manager(vec![failed()]).get_secret("key").await,
            Err(SecretError::ProviderError { .. })
        ));
        assert!(matches!(
            manager(vec![FixtureProvider::default()])
                .get_secret("key")
                .await,
            Err(SecretError::NotFound { .. })
        ));
        assert_eq!(
            manager(vec![failed(), FixtureProvider::values(&[("key", "value")])])
                .get_secret("key")
                .await
                .unwrap(),
            "value"
        );
        let pending = FixtureProvider {
            pending: true,
            ..Default::default()
        };
        assert!(matches!(
            manager(vec![pending]).get_secret("key").await,
            Err(SecretError::ProviderError { .. })
        ));
    }
    #[tokio::test]
    async fn unavailable_provider_prevents_missing_result() {
        let mut source = manager(vec![FixtureProvider::default()]);
        source.unavailable_provider = true;
        assert!(matches!(
            source.get_secret("missing").await,
            Err(SecretError::ProviderError { .. })
        ));
    }
}
