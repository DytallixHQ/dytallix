//! Pluggable secrets loading abstraction for Dytallix
//!
//! This module provides a secure, pluggable system for loading secrets from various sources:
//! - Environment variables (fallback)
//! - HashiCorp Vault (stub implementation for MVP, extensible to real vault)
//! - Future: AWS Secrets Manager, Azure Key Vault, etc.
//!
//! Design goals:
//! - No hard-coded secrets in source code
//! - Easy migration to production secret managers
//! - Configurable provider priority and fallback
//! - Type-safe secret retrieval with clear error handling

pub mod cli_integration;
pub mod config;
pub mod manager;
pub mod providers;

pub use config::SecretConfig;
pub use manager::SecretManager;
pub use providers::{EnvProvider, SecretProvider, VaultProvider};

use thiserror::Error;

/// Result type for secret operations
pub type SecretResult<T> = Result<T, SecretError>;

/// Errors that can occur during secret operations
#[derive(Error, Debug)]
pub enum SecretError {
    #[error("Secret '{name}' not found in any configured provider")]
    NotFound { name: String },

    #[error("Provider '{provider}' failed: {message}")]
    ProviderError { provider: String, message: String },

    #[error("Configuration error: {message}")]
    ConfigError { message: String },

    #[error("Network error connecting to secret provider: {message}")]
    NetworkError { message: String },

    #[error("Authentication failed for provider '{provider}': {message}")]
    AuthError { provider: String, message: String },
}
