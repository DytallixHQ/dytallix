//! Validated node configuration loaded through the secrets manager.
//! Runtime consumers must apply the returned configuration explicitly.

use crate::policy::SignaturePolicy;
use crate::secrets::{SecretError, SecretManager, SecretResult};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use tracing::{debug, info};

/// Node configuration loaded from secrets and environment
#[derive(Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    // Network configuration
    pub bind_address: String,
    pub port: u16,
    pub p2p_port: u16,

    // Database configuration
    pub database_url: String,
    pub database_pool_size: u32,

    // API configuration
    pub api_key: String,
    pub jwt_secret: String,
    pub rate_limit: u32,

    // Logging configuration
    pub log_level: String,
    pub debug_mode: bool,

    // Security configuration
    pub require_tls: bool,
    pub min_tls_version: String,
    pub audit_logging: bool,

    // PQC configuration
    pub pqc_keys_path: String,
    pub pqc_algorithm: String,

    // Signature Policy configuration
    pub signature_policy: SignaturePolicy,
}

// Credential fields must never enter Debug output, including database credentials.
impl std::fmt::Debug for NodeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeConfig")
            .field("port", &self.port)
            .field("p2p_port", &self.p2p_port)
            .field("require_tls", &self.require_tls)
            .field("audit_logging", &self.audit_logging)
            .field("signature_policy", &self.signature_policy)
            .field("database_url", &"[REDACTED]")
            .field("api_key", &"[REDACTED]")
            .field("jwt_secret", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

fn config_error(field: &str) -> SecretError {
    SecretError::ConfigError {
        message: format!("Invalid or unavailable configuration field: {field}"),
    }
}

async fn optional(manager: &SecretManager, name: &str) -> SecretResult<Option<String>> {
    match manager.get_secret(name).await {
        Ok(value) => Ok(Some(value)),
        Err(SecretError::NotFound { .. }) => Ok(None),
        Err(_) => Err(config_error(name)),
    }
}

async fn setting<T: FromStr>(manager: &SecretManager, name: &str, default: T) -> SecretResult<T> {
    match optional(manager, name).await? {
        Some(value) => value.parse().map_err(|_| config_error(name)),
        None => Ok(default),
    }
}

fn is_placeholder(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    normalized.is_empty() || normalized.contains("placeholder") || normalized.starts_with("stub_")
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0".to_string(),
            port: 8080,
            p2p_port: 30303,
            database_url: "sqlite://dytallix.db".to_string(),
            database_pool_size: 10,
            api_key: "placeholder_api_key".to_string(),
            jwt_secret: "placeholder_jwt_secret".to_string(),
            rate_limit: 1000,
            log_level: "info".to_string(),
            debug_mode: false,
            require_tls: true,
            min_tls_version: "1.2".to_string(),
            audit_logging: true,
            pqc_keys_path: "./pqc_keys.json".to_string(),
            pqc_algorithm: "Dilithium3".to_string(),
            signature_policy: SignaturePolicy::default(),
        }
    }
}

impl NodeConfig {
    /// Load and validate settings. Defaults apply only to absent optional settings.
    pub async fn load_with_secrets(secret_manager: &SecretManager) -> SecretResult<Self> {
        let mut config = Self::default();
        config.bind_address = setting(secret_manager, "BIND_ADDRESS", config.bind_address).await?;
        config.port = setting(secret_manager, "PORT", config.port).await?;
        config.p2p_port = setting(secret_manager, "P2P_PORT", config.p2p_port).await?;

        // SQLite remains the default only when the password is absent.
        if let Some(password) = optional(secret_manager, "database/password").await? {
            if is_placeholder(&password) {
                return Err(config_error("database/password"));
            }
            let host = setting(secret_manager, "database/host", "localhost".to_string()).await?;
            let port = setting(secret_manager, "database/port", 5432u16).await?;
            let name = setting(secret_manager, "database/database", "dytallix".to_string()).await?;
            let username =
                setting(secret_manager, "database/username", "dytallix".to_string()).await?;
            if port == 0
                || host.trim().is_empty()
                || name.trim().is_empty()
                || username.trim().is_empty()
            {
                return Err(config_error("database"));
            }
            let mut url = url::Url::parse("postgresql://localhost").expect("static database URL");
            url.set_host(Some(&host))
                .map_err(|_| config_error("database/host"))?;
            url.set_port(Some(port))
                .map_err(|_| config_error("database/port"))?;
            url.set_username(&username)
                .map_err(|_| config_error("database/username"))?;
            url.set_password(Some(&password))
                .map_err(|_| config_error("database/password"))?;
            url.path_segments_mut()
                .map_err(|_| config_error("database/database"))?
                .clear()
                .push(&name);
            config.database_url = url.into();
        }

        config.api_key = optional(secret_manager, "api/api_key")
            .await?
            .ok_or_else(|| config_error("api/api_key"))?;
        config.jwt_secret = optional(secret_manager, "api/jwt_secret")
            .await?
            .ok_or_else(|| config_error("api/jwt_secret"))?;
        config.rate_limit = setting(secret_manager, "api/rate_limit", config.rate_limit).await?;
        config.log_level = setting(secret_manager, "config/log_level", config.log_level).await?;
        config.debug_mode = setting(secret_manager, "config/debug_mode", config.debug_mode).await?;
        config.require_tls = setting(secret_manager, "REQUIRE_TLS", config.require_tls).await?;
        config.min_tls_version =
            setting(secret_manager, "MIN_TLS_VERSION", config.min_tls_version).await?;
        config.audit_logging =
            setting(secret_manager, "AUDIT_LOGGING", config.audit_logging).await?;
        config.pqc_keys_path =
            setting(secret_manager, "PQC_KEYS_PATH", config.pqc_keys_path).await?;
        config.pqc_algorithm = setting(
            secret_manager,
            "PREFERRED_SIGNATURE_ALGORITHM",
            config.pqc_algorithm,
        )
        .await?;
        config.signature_policy.reject_legacy =
            setting(secret_manager, "SIGNATURE_POLICY_REJECT_LEGACY", true).await?;
        config.signature_policy.enforce_at_mempool =
            setting(secret_manager, "SIGNATURE_POLICY_ENFORCE_MEMPOOL", true).await?;
        config.signature_policy.enforce_at_consensus =
            setting(secret_manager, "SIGNATURE_POLICY_ENFORCE_CONSENSUS", true).await?;
        if let Some(names) = optional(secret_manager, "SIGNATURE_POLICY_ALLOWED_ALGORITHMS").await?
        {
            config.signature_policy.allowed_algorithms = names
                .split(',')
                .map(|name| {
                    SignaturePolicy::parse_algorithm_name(name.trim())
                        .map_err(|_| config_error("SIGNATURE_POLICY_ALLOWED_ALGORITHMS"))
                })
                .collect::<SecretResult<_>>()?;
        }
        config.validate()?;
        info!("Node configuration loaded and validated");
        debug!("Configuration: {:?}", config);
        Ok(config)
    }

    /// Return an unvalidated legacy template. Use load_with_secrets for validated loading.
    pub fn load_from_env() -> Self {
        use std::env;

        let mut config = Self::default();

        if let Ok(bind_address) = env::var("DYTALLIX_BIND_ADDRESS") {
            config.bind_address = bind_address;
        }

        if let Ok(port_str) = env::var("DYTALLIX_PORT") {
            if let Ok(port) = port_str.parse::<u16>() {
                config.port = port;
            }
        }

        // ... more environment variable loading ...
        // This approach requires manual handling of each variable
        // and doesn't support hierarchical secrets or multiple providers

        config
    }

    /// Check usable credentials and internally consistent settings.
    /// This does not prove secret entropy, provider assurance, or node activation.
    pub fn validate(&self) -> SecretResult<()> {
        if self.port == 0 || self.p2p_port == 0 {
            return Err(config_error("ports"));
        }
        if self.bind_address.trim().is_empty() {
            return Err(config_error("BIND_ADDRESS"));
        }
        if is_placeholder(&self.api_key) {
            return Err(config_error("api/api_key"));
        }
        if is_placeholder(&self.jwt_secret) || self.jwt_secret.trim().len() < 32 {
            return Err(config_error("api/jwt_secret"));
        }
        if self.rate_limit == 0 {
            return Err(config_error("api/rate_limit"));
        }
        if self.database_url.trim().is_empty() || self.database_pool_size == 0 {
            return Err(config_error("database"));
        }
        if self.log_level.parse::<tracing::Level>().is_err() {
            return Err(config_error("config/log_level"));
        }
        if self.pqc_keys_path.trim().is_empty() {
            return Err(config_error("PQC_KEYS_PATH"));
        }
        if !matches!(self.min_tls_version.as_str(), "1.2" | "1.3") {
            return Err(config_error("MIN_TLS_VERSION"));
        }
        self.signature_policy
            .validate_algorithm_name(&self.pqc_algorithm)
            .map_err(|_| config_error("PREFERRED_SIGNATURE_ALGORITHM"))?;
        Ok(())
    }

    /// Check production configuration requirements only. This is not a release gate.
    pub fn is_production_ready(&self) -> bool {
        self.validate().is_ok()
            && self.require_tls
            && self.audit_logging
            && !self.debug_mode
            && self.signature_policy.reject_legacy
            && self.signature_policy.enforce_at_mempool
            && self.signature_policy.enforce_at_consensus
    }
}

/// Configuration loader utility
pub struct ConfigLoader {
    secret_manager: SecretManager,
}

impl ConfigLoader {
    /// Create a new configuration loader
    pub async fn new() -> SecretResult<Self> {
        let mut secret_manager = SecretManager::from_env()?;
        secret_manager.initialize().await?;

        Ok(Self { secret_manager })
    }

    /// Load node configuration
    pub async fn load_node_config(&self) -> SecretResult<NodeConfig> {
        NodeConfig::load_with_secrets(&self.secret_manager).await
    }

    /// Get the underlying secret manager
    pub fn secret_manager(&self) -> &SecretManager {
        &self.secret_manager
    }

    /// Perform health check on secret providers
    pub async fn health_check(&self) -> std::collections::HashMap<String, bool> {
        self.secret_manager.health_check().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::manager::test_support::{manager, FixtureProvider};

    fn valid() -> NodeConfig {
        NodeConfig {
            api_key: "fixture-api-value".into(),
            jwt_secret: "fixture-jwt-value-with-at-least-32-bytes".into(),
            ..Default::default()
        }
    }
    fn values() -> FixtureProvider {
        FixtureProvider::values(&[
            ("api/api_key", "fixture-api-value"),
            ("api/jwt_secret", "fixture-jwt-value-with-at-least-32-bytes"),
        ])
    }
    #[tokio::test]
    async fn test_node_config_loading() {
        let mut provider = values();
        provider
            .values
            .insert("BIND_ADDRESS".into(), "127.0.0.1".into());
        provider.values.insert("PORT".into(), "9090".into());
        let config = NodeConfig::load_with_secrets(&manager(vec![provider]))
            .await
            .unwrap();
        assert_eq!(config.bind_address, "127.0.0.1");
        assert_eq!(config.port, 9090);
        assert_eq!(config.api_key, "fixture-api-value");
        assert_eq!(config.database_url, "sqlite://dytallix.db");
        assert_eq!(config.signature_policy.allowed_algorithms.len(), 1);
    }
    #[test]
    fn test_config_validation() {
        assert!(valid().validate().is_ok());
        assert!(NodeConfig::default().validate().is_err());
        for config in [
            NodeConfig { port: 0, ..valid() },
            NodeConfig {
                p2p_port: 0,
                ..valid()
            },
            NodeConfig {
                jwt_secret: "short".into(),
                ..valid()
            },
            NodeConfig {
                api_key: " ".into(),
                ..valid()
            },
            NodeConfig {
                api_key: "stub_api_key_replace_in_prod".into(),
                ..valid()
            },
        ] {
            assert!(config.validate().is_err());
        }
    }
    #[test]
    fn test_production_readiness() {
        assert!(valid().is_production_ready());
        assert!(!NodeConfig::default().is_production_ready());
        let mut configs = vec![
            NodeConfig { port: 0, ..valid() },
            NodeConfig {
                require_tls: false,
                ..valid()
            },
            NodeConfig {
                audit_logging: false,
                ..valid()
            },
            NodeConfig {
                debug_mode: true,
                ..valid()
            },
        ];
        let mut policy = valid();
        policy.signature_policy.enforce_at_consensus = false;
        configs.push(policy);
        let mut policy = valid();
        policy.signature_policy.enforce_at_mempool = false;
        configs.push(policy);
        let mut policy = valid();
        policy.signature_policy.reject_legacy = false;
        configs.push(policy);
        assert!(configs.iter().all(|c| !c.is_production_ready()));
    }
    #[tokio::test]
    async fn missing_empty_or_placeholder_credentials_fail_loading() {
        for field in ["api/api_key", "api/jwt_secret"] {
            for replacement in [
                None,
                Some(""),
                Some("  "),
                Some("placeholder_api_key"),
                Some("STUB_credential-value-at-least-32-characters"),
            ] {
                let mut provider = values();
                match replacement {
                    Some(value) => {
                        provider.values.insert(field.into(), value.into());
                    }
                    None => {
                        provider.values.remove(field);
                    }
                }
                assert!(NodeConfig::load_with_secrets(&manager(vec![provider]))
                    .await
                    .is_err());
            }
        }
    }
    #[tokio::test]
    async fn explicit_invalid_settings_fail_instead_of_using_defaults() {
        for (field, value) in [
            ("PORT", "not-a-port"),
            ("P2P_PORT", "65536"),
            ("api/rate_limit", "-1"),
            ("REQUIRE_TLS", ""),
            ("AUDIT_LOGGING", "yes"),
            ("config/debug_mode", "maybe"),
            ("config/log_level", ""),
            ("config/log_level", "invalid-level"),
            ("SIGNATURE_POLICY_ENFORCE_CONSENSUS", "TRUE"),
            ("MIN_TLS_VERSION", "1.0"),
            ("PQC_KEYS_PATH", ""),
        ] {
            let mut provider = values();
            provider.values.insert(field.into(), value.into());
            assert!(
                NodeConfig::load_with_secrets(&manager(vec![provider]))
                    .await
                    .is_err(),
                "{field}"
            );
        }
    }
    #[tokio::test]
    async fn allowlist_replacement_is_complete_and_preference_must_match() {
        let mut provider = values();
        provider.values.insert(
            "SIGNATURE_POLICY_ALLOWED_ALGORITHMS".into(),
            "DILITHIUM5, falcon".into(),
        );
        provider
            .values
            .insert("PREFERRED_SIGNATURE_ALGORITHM".into(), "dilithium5".into());
        let config = NodeConfig::load_with_secrets(&manager(vec![provider]))
            .await
            .unwrap();
        assert_eq!(config.signature_policy.allowed_algorithms.len(), 2);
        assert!(config
            .signature_policy
            .validate_algorithm_name("dilithium3")
            .is_err());
        for names in [
            "",
            "dilithium3,",
            "dilithium3,unknown",
            "rsa",
            "falcon",
            "dilithium3,,falcon",
        ] {
            let mut provider = values();
            provider
                .values
                .insert("SIGNATURE_POLICY_ALLOWED_ALGORITHMS".into(), names.into());
            assert!(
                NodeConfig::load_with_secrets(&manager(vec![provider]))
                    .await
                    .is_err(),
                "{names}"
            );
        }
    }
    #[tokio::test]
    async fn provider_failure_cannot_become_an_optional_default() {
        let mut provider = values();
        provider.fail_on = Some("PORT".into());
        let error = NodeConfig::load_with_secrets(&manager(vec![provider]))
            .await
            .unwrap_err();
        assert!(matches!(error, SecretError::ConfigError { .. }));
        assert!(!error.to_string().contains("private-provider-detail"));
    }
    #[test]
    fn debug_redacts_credentials_in_all_formats() {
        let mut config = valid();
        config.database_url = "postgresql://user:fixture-db-password@localhost/db".into();
        for output in [format!("{config:?}"), format!("{config:#?}")] {
            for secret in [&config.api_key, &config.jwt_secret, &config.database_url] {
                assert!(!output.contains(secret));
            }
            assert!(output.contains("[REDACTED]"));
        }
    }
    #[tokio::test]
    async fn database_components_preserve_reserved_characters() {
        let mut provider = values();
        for (key, value) in [
            ("database/password", "p@ss:/?#"),
            ("database/username", "user@name"),
            ("database/database", "db/name"),
        ] {
            provider.values.insert(key.into(), value.into());
        }
        let config = NodeConfig::load_with_secrets(&manager(vec![provider]))
            .await
            .unwrap();
        let parsed = url::Url::parse(&config.database_url).unwrap();
        assert_eq!(parsed.host_str(), Some("localhost"));
        assert_eq!(parsed.username(), "user%40name");
        assert_eq!(parsed.password(), Some("p%40ss%3A%2F%3F%23"));
        assert_eq!(parsed.path(), "/db%2Fname");
        assert!(parsed.query().is_none() && parsed.fragment().is_none());
    }
    #[derive(Clone, Default)]
    struct LogBuffer(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for LogBuffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    #[tokio::test]
    async fn loader_logs_and_errors_do_not_contain_credential_values() {
        // Tracing callsite interest is process-global. Isolate capture from parallel tests.
        const CHILD: &str = "DYT_CONFIG_LOG_CAPTURE_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "config::tests::loader_logs_and_errors_do_not_contain_credential_values",
                ])
                .env(CHILD, "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
            return;
        }
        let buffer = LogBuffer::default();
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_max_level(tracing::Level::DEBUG)
            .with_writer(move || writer.clone())
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        let mut provider = values();
        provider
            .values
            .insert("database/password".into(), "fixture-db-password".into());
        let config = NodeConfig::load_with_secrets(&manager(vec![provider]))
            .await
            .unwrap();
        let mut provider = values();
        provider
            .values
            .insert("PORT".into(), "sensitive-invalid-port-value".into());
        let error = NodeConfig::load_with_secrets(&manager(vec![provider]))
            .await
            .unwrap_err();
        let logs = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        assert!(logs.contains("loaded and validated"));
        for secret in [
            &config.api_key,
            &config.jwt_secret,
            "fixture-db-password",
            "sensitive-invalid-port-value",
        ] {
            assert!(!logs.contains(secret));
            assert!(!error.to_string().contains(secret));
        }
    }
}
