//! Secret management configuration
//!
//! This module defines configuration structures for the secret management system.

use crate::secrets::{SecretError, SecretResult};
use serde::{Deserialize, Serialize};
use std::env;

/// Configuration for the secret management system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretConfig {
    /// Ordered list of providers to try (first = highest priority)
    pub provider_order: Vec<ProviderConfig>,

    /// Default timeout for secret operations (in seconds)
    pub timeout_seconds: u64,

    /// Whether to cache secrets in memory (disabled by default for security)
    pub enable_caching: bool,

    /// Cache TTL in seconds (if caching is enabled)
    pub cache_ttl_seconds: u64,
}

/// Configuration for individual secret providers
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ProviderConfig {
    #[serde(rename = "environment")]
    Environment {
        /// Optional prefix for environment variables
        prefix: Option<String>,
        /// Whether to use case-sensitive matching
        case_sensitive: bool,
    },

    #[serde(rename = "vault")]
    Vault {
        /// Vault server URL
        url: String,
        /// Authentication token (or path to token file)
        token: String,
        /// KV mount path
        mount_path: String,
        /// Environment namespace
        environment: String,
        /// Enable stub mode (no real network calls)
        stub_mode: bool,
    },
}

impl Default for SecretConfig {
    fn default() -> Self {
        Self {
            // Default order: try Vault first, fallback to environment
            provider_order: vec![
                ProviderConfig::Vault {
                    url: "http://localhost:8200".to_string(),
                    token: "".to_string(),
                    mount_path: "secret".to_string(),
                    environment: "dev".to_string(),
                    stub_mode: true, // Safe default for development
                },
                ProviderConfig::Environment {
                    prefix: Some("DYTALLIX_".to_string()),
                    case_sensitive: false,
                },
            ],
            timeout_seconds: 30,
            enable_caching: false,  // Disabled by default for security
            cache_ttl_seconds: 300, // 5 minutes
        }
    }
}

impl SecretConfig {
    /// Create configuration from environment variables
    ///
    /// This method reads configuration from well-known environment variables
    /// and creates a sensible default configuration.
    pub fn from_env() -> SecretResult<Self> {
        let mut config = Self::default();

        // Override defaults based on environment variables
        let use_vault = env::var("DYTALLIX_USE_VAULT")
            .unwrap_or_else(|_| "false".to_string())
            .parse::<bool>()
            .unwrap_or(false);

        if use_vault {
            // If vault is explicitly enabled, try to configure it from env
            let vault_url = env::var("VAULT_ADDR")
                .or_else(|_| env::var("DYTALLIX_VAULT_URL"))
                .unwrap_or_else(|_| "http://localhost:8200".to_string());

            let vault_token = env::var("VAULT_TOKEN")
                .or_else(|_| env::var("DYTALLIX_VAULT_TOKEN"))
                .unwrap_or_default();

            let vault_mount =
                env::var("DYTALLIX_VAULT_MOUNT").unwrap_or_else(|_| "secret".to_string());

            let environment =
                env::var("DYTALLIX_ENVIRONMENT").unwrap_or_else(|_| "dev".to_string());

            // Determine if we should use stub mode
            let stub_mode =
                vault_token.is_empty() || vault_token.starts_with("stub") || environment == "dev";

            config.provider_order = vec![
                ProviderConfig::Vault {
                    url: vault_url,
                    token: vault_token,
                    mount_path: vault_mount,
                    environment,
                    stub_mode,
                },
                ProviderConfig::Environment {
                    prefix: Some("DYTALLIX_".to_string()),
                    case_sensitive: false,
                },
            ];
        } else {
            // Vault not enabled, use only environment variables
            config.provider_order = vec![ProviderConfig::Environment {
                prefix: Some("DYTALLIX_".to_string()),
                case_sensitive: false,
            }];
        }

        // Override timeout if specified
        if let Ok(timeout_str) = env::var("DYTALLIX_SECRET_TIMEOUT") {
            if let Ok(timeout) = timeout_str.parse::<u64>() {
                config.timeout_seconds = timeout;
            }
        }

        // Override caching settings if specified
        if let Ok(cache_str) = env::var("DYTALLIX_SECRET_CACHE") {
            if let Ok(enable_cache) = cache_str.parse::<bool>() {
                config.enable_caching = enable_cache;
            }
        }

        if let Ok(ttl_str) = env::var("DYTALLIX_SECRET_CACHE_TTL") {
            if let Ok(ttl) = ttl_str.parse::<u64>() {
                config.cache_ttl_seconds = ttl;
            }
        }

        Ok(config)
    }

    /// Validate the configuration
    pub fn validate(&self) -> SecretResult<()> {
        if self.provider_order.is_empty() {
            return Err(SecretError::ConfigError {
                message: "No secret providers configured".to_string(),
            });
        }

        if self.timeout_seconds == 0 {
            return Err(SecretError::ConfigError {
                message: "Timeout must be greater than 0".to_string(),
            });
        }

        if self.enable_caching && self.cache_ttl_seconds == 0 {
            return Err(SecretError::ConfigError {
                message: "Cache TTL must be greater than 0 when caching is enabled".to_string(),
            });
        }

        // Validate individual provider configurations
        for provider in &self.provider_order {
            match provider {
                ProviderConfig::Vault { url, .. } => {
                    if url.is_empty() {
                        return Err(SecretError::ConfigError {
                            message: "Vault URL cannot be empty".to_string(),
                        });
                    }
                }
                ProviderConfig::Environment { .. } => {
                    // Environment provider is always valid
                }
            }
        }

        Ok(())
    }

    /// Create a minimal configuration for testing
    pub fn for_testing() -> Self {
        Self {
            provider_order: vec![ProviderConfig::Environment {
                prefix: Some("TEST_".to_string()),
                case_sensitive: false,
            }],
            timeout_seconds: 5,
            enable_caching: false,
            cache_ttl_seconds: 60,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_is_valid() {
        let config = SecretConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_testing_config_is_valid() {
        let config = SecretConfig::for_testing();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_empty_provider_order_fails_validation() {
        let mut config = SecretConfig::default();
        config.provider_order.clear();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_zero_timeout_fails_validation() {
        let mut config = SecretConfig::default();
        config.timeout_seconds = 0;
        assert!(config.validate().is_err());
    }
}
