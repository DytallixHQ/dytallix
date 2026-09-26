//! Threshold-based alerting subsystem for Dytallix node
//!
//! This module implements an MVP alerting system to detect and surface reliability
//! issues such as TPS drops, oracle timeouts, and validator offline conditions.
//! The system is designed to be lightweight, non-blocking, and configurable.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::time::interval;

#[cfg(feature = "alerts")]
use reqwest::Client;

#[cfg(feature = "metrics")]
use prometheus::{IntCounterVec, IntGaugeVec, Opts, Registry};

use crate::storage::blocks::TpsWindow;

/// Trait for gathering metrics data needed by the alerting system
pub trait MetricsGatherer: Send + Sync {
    /// Get the current transactions per second over a sliding window
    fn get_current_tps(&self) -> f64;

    /// Get the 95th percentile oracle response latency in milliseconds
    /// Falls back to max latency if p95 is not available
    fn get_oracle_latency_p95_ms(&self) -> Option<f64>;

    /// Get a map of validator ID to last seen timestamp in seconds since UNIX epoch
    fn get_validator_heartbeats(&self) -> HashMap<String, u64>;
}

/// Concrete implementation of MetricsGatherer for the Dytallix node
pub struct NodeMetricsGatherer {
    tps_window: Arc<Mutex<TpsWindow>>,
    // TODO: Add oracle latency tracking when oracle module is available
    // TODO: Add validator heartbeat tracking when consensus module is available
}

impl NodeMetricsGatherer {
    pub fn new(tps_window: Arc<Mutex<TpsWindow>>) -> Self {
        Self { tps_window }
    }
}

impl MetricsGatherer for NodeMetricsGatherer {
    fn get_current_tps(&self) -> f64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.tps_window.lock().unwrap().rolling_tps(now)
    }

    fn get_oracle_latency_p95_ms(&self) -> Option<f64> {
        // TODO: Implement when oracle latency tracking is available
        // For now, return None to indicate no oracle data available
        None
    }

    fn get_validator_heartbeats(&self) -> HashMap<String, u64> {
        // TODO: Implement when validator/consensus tracking is available
        // For now, return empty map (no validators to track)
        HashMap::new()
    }
}

/// Alert types that can be triggered
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum AlertKind {
    TPSDrop,
    OracleTimeout,
    ValidatorOffline,
}

/// Alert payload sent when an alert fires or recovers
#[derive(Debug, Clone, Serialize)]
pub struct AlertPayload {
    pub kind: AlertKind,
    pub timestamp: DateTime<Utc>,
    pub details: serde_json::Value,
}

/// Internal state for each alert rule
#[derive(Debug, Clone, Default)]
struct RuleState {
    consecutive_failures: u32,
    firing: bool,
}

/// Configuration for TPS drop alert rule
#[derive(Debug, Clone, Deserialize)]
pub struct TpsDropConfig {
    pub enabled: bool,
    #[serde(default = "default_tps_threshold")]
    pub threshold: f64,
    #[serde(default = "default_consecutive")]
    pub consecutive: u32,
}

fn default_tps_threshold() -> f64 {
    1500.0
}
fn default_consecutive() -> u32 {
    3
}

/// Configuration for oracle timeout alert rule
#[derive(Debug, Clone, Deserialize)]
pub struct OracleTimeoutConfig {
    pub enabled: bool,
    #[serde(default = "default_oracle_threshold_ms")]
    pub threshold_ms: f64,
    #[serde(default = "default_oracle_consecutive")]
    pub consecutive: u32,
}

fn default_oracle_threshold_ms() -> f64 {
    800.0
}
fn default_oracle_consecutive() -> u32 {
    2
}

/// Configuration for validator offline alert rule
#[derive(Debug, Clone, Deserialize)]
pub struct ValidatorOfflineConfig {
    pub enabled: bool,
    #[serde(default = "default_offline_secs")]
    pub offline_secs: u64,
    #[serde(default = "default_validator_consecutive")]
    pub consecutive: u32,
}

fn default_offline_secs() -> u64 {
    30
}
fn default_validator_consecutive() -> u32 {
    1
}

fn default_enabled() -> bool {
    true
}
fn default_evaluation_interval_secs() -> u64 {
    5
}
fn default_log_on_fire() -> bool {
    true
}

/// Rule-specific configurations
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RulesConfig {
    #[serde(default)]
    pub tps_drop: TpsDropConfig,
    #[serde(default)]
    pub oracle_timeout: OracleTimeoutConfig,
    #[serde(default)]
    pub validator_offline: ValidatorOfflineConfig,
}

impl Default for TpsDropConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold: default_tps_threshold(),
            consecutive: default_consecutive(),
        }
    }
}
impl Default for OracleTimeoutConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold_ms: default_oracle_threshold_ms(),
            consecutive: default_oracle_consecutive(),
        }
    }
}
impl Default for ValidatorOfflineConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            offline_secs: default_offline_secs(),
            consecutive: default_validator_consecutive(),
        }
    }
}
impl Default for AlertsConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            evaluation_interval_secs: default_evaluation_interval_secs(),
            webhook_url: None,
            log_on_fire: default_log_on_fire(),
            rules: RulesConfig::default(),
        }
    }
}

/// Main alerts configuration
#[derive(Debug, Clone, Deserialize)]
pub struct AlertsConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_evaluation_interval_secs")]
    pub evaluation_interval_secs: u64,
    pub webhook_url: Option<String>,
    #[serde(default = "default_log_on_fire")]
    pub log_on_fire: bool,
    #[serde(default)]
    pub rules: RulesConfig,
}

/// Prometheus metrics for the alerting system
#[cfg(feature = "metrics")]
pub struct AlertMetrics {
    alert_rule_firing: IntGaugeVec,
    alert_events_total: IntCounterVec,
}

#[cfg(feature = "metrics")]
impl AlertMetrics {
    pub fn new(registry: &Registry) -> Result<Self> {
        let alert_rule_firing = IntGaugeVec::new(
            Opts::new(
                "dytallix_alert_rule_firing",
                "Whether an alert rule is currently firing (1) or not (0)",
            ),
            &["rule"],
        )?;
        registry.register(Box::new(alert_rule_firing.clone()))?;

        let alert_events_total = IntCounterVec::new(
            Opts::new(
                "dytallix_alert_events_total",
                "Total number of alert events (firing or recovery)",
            ),
            &["rule", "event_type"],
        )?;
        registry.register(Box::new(alert_events_total.clone()))?;

        Ok(Self {
            alert_rule_firing,
            alert_events_total,
        })
    }

    pub fn set_rule_firing(&self, rule: &str, firing: bool) {
        self.alert_rule_firing
            .with_label_values(&[rule])
            .set(if firing { 1 } else { 0 });
    }

    pub fn inc_alert_event(&self, rule: &str, event_type: &str) {
        self.alert_events_total
            .with_label_values(&[rule, event_type])
            .inc();
    }
}

#[cfg(not(feature = "metrics"))]
pub struct AlertMetrics;

#[cfg(not(feature = "metrics"))]
impl AlertMetrics {
    pub fn new(_registry: &()) -> Result<Self> {
        Ok(Self)
    }

    pub fn set_rule_firing(&self, _rule: &str, _firing: bool) {}
    pub fn inc_alert_event(&self, _rule: &str, _event_type: &str) {}
}

/// Main alerting engine
pub struct AlertsEngine {
    config: AlertsConfig,
    state: HashMap<AlertKind, RuleState>,
    #[cfg(feature = "alerts")]
    client: Option<Client>,
    #[allow(dead_code)]
    metrics: AlertMetrics,
}

impl AlertsEngine {
    /// Create a new alerts engine with the given configuration
    #[cfg(feature = "metrics")]
    pub fn new(config: AlertsConfig, registry: &Registry) -> Result<Self> {
        let metrics = AlertMetrics::new(registry)?;

        #[cfg(feature = "alerts")]
        let client = if config.webhook_url.is_some() {
            Some(Client::new())
        } else {
            None
        };

        Ok(Self {
            config,
            state: HashMap::new(),
            #[cfg(feature = "alerts")]
            client,
            metrics,
        })
    }

    #[cfg(not(feature = "metrics"))]
    pub fn new(config: AlertsConfig) -> Result<Self> {
        let metrics = AlertMetrics::new(&())?;

        #[cfg(feature = "alerts")]
        let client = if config.webhook_url.is_some() {
            Some(Client::new())
        } else {
            None
        };

        Ok(Self {
            config,
            state: HashMap::new(),
            #[cfg(feature = "alerts")]
            client,
            metrics,
        })
    }

    /// Start the alerting engine evaluation loop
    pub async fn start<G>(&mut self, gatherer: Arc<G>) -> Result<()>
    where
        G: MetricsGatherer + 'static,
    {
        if !self.config.enabled {
            tracing::info!("Alerting system disabled, evaluation loop will not start");
            return Ok(());
        }

        tracing::info!(
            "Starting alerting engine with {}s evaluation interval",
            self.config.evaluation_interval_secs
        );

        let mut interval_timer =
            interval(Duration::from_secs(self.config.evaluation_interval_secs));

        loop {
            interval_timer.tick().await;
            self.evaluate_rules(&*gatherer).await?;
        }
    }

    /// Evaluate all enabled rules and handle state transitions
    async fn evaluate_rules<G>(&mut self, gatherer: &G) -> Result<()>
    where
        G: MetricsGatherer,
    {
        // Evaluate TPS drop rule
        if self.config.rules.tps_drop.enabled {
            let current_tps = gatherer.get_current_tps();
            let threshold_violated = current_tps < self.config.rules.tps_drop.threshold;
            self.handle_rule_evaluation(
                AlertKind::TPSDrop,
                threshold_violated,
                self.config.rules.tps_drop.consecutive,
                serde_json::json!({
                    "current_tps": current_tps,
                    "threshold": self.config.rules.tps_drop.threshold
                }),
            )
            .await?;
        }

        // Evaluate oracle timeout rule
        if self.config.rules.oracle_timeout.enabled {
            if let Some(latency_ms) = gatherer.get_oracle_latency_p95_ms() {
                let threshold_violated = latency_ms > self.config.rules.oracle_timeout.threshold_ms;
                self.handle_rule_evaluation(
                    AlertKind::OracleTimeout,
                    threshold_violated,
                    self.config.rules.oracle_timeout.consecutive,
                    serde_json::json!({
                        "current_latency_ms": latency_ms,
                        "threshold_ms": self.config.rules.oracle_timeout.threshold_ms
                    }),
                )
                .await?;
            }
        }

        // Evaluate validator offline rule
        if self.config.rules.validator_offline.enabled {
            let validator_heartbeats = gatherer.get_validator_heartbeats();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            // Check if any validator is offline
            let mut offline_validators = Vec::new();
            for (validator_id, last_seen) in validator_heartbeats {
                if now.saturating_sub(last_seen) > self.config.rules.validator_offline.offline_secs
                {
                    offline_validators.push(validator_id);
                }
            }

            let threshold_violated = !offline_validators.is_empty();
            self.handle_rule_evaluation(
                AlertKind::ValidatorOffline,
                threshold_violated,
                self.config.rules.validator_offline.consecutive,
                serde_json::json!({
                    "offline_validators": offline_validators,
                    "offline_threshold_secs": self.config.rules.validator_offline.offline_secs
                }),
            )
            .await?;
        }

        Ok(())
    }

    /// Handle rule evaluation and state transitions
    async fn handle_rule_evaluation(
        &mut self,
        alert_kind: AlertKind,
        threshold_violated: bool,
        consecutive_required: u32,
        details: serde_json::Value,
    ) -> Result<()> {
        let state = self.state.entry(alert_kind.clone()).or_default();

        if threshold_violated {
            state.consecutive_failures += 1;

            // Check if we should transition to firing state
            if !state.firing && state.consecutive_failures >= consecutive_required {
                state.firing = true;
                self.emit_alert_event(alert_kind.clone(), details, "firing")
                    .await?;
            }
        } else {
            // Threshold not violated - reset consecutive failures
            if state.firing {
                // Transition from firing to normal - emit recovery event
                state.firing = false;
                state.consecutive_failures = 0;
                self.emit_alert_event(alert_kind.clone(), details, "recovery")
                    .await?;
            } else {
                // Just reset the failure count
                state.consecutive_failures = 0;
            }
        }

        Ok(())
    }

    /// Emit an alert event (firing or recovery)
    async fn emit_alert_event(
        &self,
        alert_kind: AlertKind,
        details: serde_json::Value,
        event_type: &str,
    ) -> Result<()> {
        let payload = AlertPayload {
            kind: alert_kind.clone(),
            timestamp: Utc::now(),
            details,
        };

        // Update metrics
        let rule_name = format!("{alert_kind:?}").to_lowercase();
        self.metrics
            .set_rule_firing(&rule_name, event_type == "firing");
        self.metrics.inc_alert_event(&rule_name, event_type);

        // Log the alert if configured
        if self.config.log_on_fire {
            tracing::warn!(
                "Alert {} for rule {}: {}",
                event_type,
                rule_name,
                serde_json::to_string(&payload)?
            );
        }

        // Send webhook if configured
        #[cfg(feature = "alerts")]
        if let (Some(webhook_url), Some(client)) = (&self.config.webhook_url, &self.client) {
            if let Err(e) = self.send_webhook(client, webhook_url, &payload).await {
                tracing::error!("Failed to send webhook for alert {}: {}", rule_name, e);
            }
        }

        Ok(())
    }

    /// Send webhook notification
    #[cfg(feature = "alerts")]
    async fn send_webhook(
        &self,
        client: &Client,
        webhook_url: &str,
        payload: &AlertPayload,
    ) -> Result<()> {
        let response = client
            .post(webhook_url)
            .json(payload)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .context("Failed to send webhook request")?;

        if !response.status().is_success() {
            anyhow::bail!("Webhook returned non-success status: {}", response.status());
        }

        Ok(())
    }
}

/// Load alerts configuration from a YAML file
pub fn load_alerts_config(path: &Path) -> Result<AlertsConfig> {
    if !path.exists() {
        tracing::warn!(
            "Alerts configuration file {} does not exist, using defaults",
            path.display()
        );
        return Ok(AlertsConfig::default());
    }

    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read alerts config from {}", path.display()))?;

    let config: AlertsConfig = serde_yaml::from_str(&contents)
        .with_context(|| format!("Failed to parse alerts config from {}", path.display()))?;

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;
    use tempfile::NamedTempFile;

    /// Mock metrics gatherer for testing
    struct MockMetricsGatherer {
        tps: AtomicU64,
        oracle_latency_ms: Mutex<Option<f64>>,
        validator_heartbeats: Mutex<HashMap<String, u64>>,
    }

    impl MockMetricsGatherer {
        fn new() -> Self {
            Self {
                tps: AtomicU64::new(2000),
                oracle_latency_ms: Mutex::new(Some(500.0)),
                validator_heartbeats: Mutex::new(HashMap::new()),
            }
        }

        fn set_tps(&self, tps: u64) {
            self.tps.store(tps, Ordering::Relaxed);
        }

        #[allow(dead_code)]
        fn set_oracle_latency_ms(&self, latency: Option<f64>) {
            *self.oracle_latency_ms.lock().unwrap() = latency;
        }

        fn set_validator_heartbeat(&self, validator_id: String, timestamp: u64) {
            self.validator_heartbeats
                .lock()
                .unwrap()
                .insert(validator_id, timestamp);
        }
    }

    impl MetricsGatherer for MockMetricsGatherer {
        fn get_current_tps(&self) -> f64 {
            self.tps.load(Ordering::Relaxed) as f64
        }

        fn get_oracle_latency_p95_ms(&self) -> Option<f64> {
            *self.oracle_latency_ms.lock().unwrap()
        }

        fn get_validator_heartbeats(&self) -> HashMap<String, u64> {
            self.validator_heartbeats.lock().unwrap().clone()
        }
    }

    #[test]
    fn test_load_alerts_config_default() {
        // Test loading default config when file doesn't exist
        let config = load_alerts_config(Path::new("/nonexistent/path")).unwrap();
        assert!(config.enabled);
        assert_eq!(config.evaluation_interval_secs, 5);
        assert!(config.log_on_fire);
    }

    #[test]
    fn test_load_alerts_config_from_file() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(
            temp_file,
            r#"
enabled: false
evaluation_interval_secs: 10
webhook_url: "https://test.example.com/webhook"
log_on_fire: false
rules:
  tps_drop:
    enabled: true
    threshold: 1000
    consecutive: 2
  oracle_timeout:
    enabled: false
    threshold_ms: 1000
    consecutive: 3
  validator_offline:
    enabled: true
    offline_secs: 60
    consecutive: 1
"#
        )
        .unwrap();

        let config = load_alerts_config(temp_file.path()).unwrap();
        assert!(!config.enabled);
        assert_eq!(config.evaluation_interval_secs, 10);
        assert_eq!(
            config.webhook_url,
            Some("https://test.example.com/webhook".to_string())
        );
        assert!(!config.log_on_fire);

        assert!(config.rules.tps_drop.enabled);
        assert_eq!(config.rules.tps_drop.threshold, 1000.0);
        assert_eq!(config.rules.tps_drop.consecutive, 2);

        assert!(!config.rules.oracle_timeout.enabled);
        assert_eq!(config.rules.oracle_timeout.threshold_ms, 1000.0);
        assert_eq!(config.rules.oracle_timeout.consecutive, 3);

        assert!(config.rules.validator_offline.enabled);
        assert_eq!(config.rules.validator_offline.offline_secs, 60);
        assert_eq!(config.rules.validator_offline.consecutive, 1);
    }

    #[tokio::test]
    async fn test_tps_drop_alert() {
        let config = AlertsConfig {
            enabled: true,
            evaluation_interval_secs: 1,
            webhook_url: None,
            log_on_fire: true,
            rules: RulesConfig {
                tps_drop: TpsDropConfig {
                    enabled: true,
                    threshold: 1500.0,
                    consecutive: 2,
                },
                oracle_timeout: OracleTimeoutConfig {
                    enabled: false,
                    ..Default::default()
                },
                validator_offline: ValidatorOfflineConfig {
                    enabled: false,
                    ..Default::default()
                },
            },
        };

        #[cfg(feature = "metrics")]
        let registry = Registry::new();
        #[cfg(feature = "metrics")]
        let mut engine = AlertsEngine::new(config, &registry).unwrap();
        #[cfg(not(feature = "metrics"))]
        let mut engine = AlertsEngine::new(config).unwrap();

        let gatherer = Arc::new(MockMetricsGatherer::new());

        // Set TPS below threshold
        gatherer.set_tps(1000);

        // First evaluation - should not fire yet (consecutive = 2)
        engine.evaluate_rules(&*gatherer).await.unwrap();
        assert!(!engine.state.get(&AlertKind::TPSDrop).unwrap().firing);
        assert_eq!(
            engine
                .state
                .get(&AlertKind::TPSDrop)
                .unwrap()
                .consecutive_failures,
            1
        );

        // Second evaluation - should fire now
        engine.evaluate_rules(&*gatherer).await.unwrap();
        assert!(engine.state.get(&AlertKind::TPSDrop).unwrap().firing);
        assert_eq!(
            engine
                .state
                .get(&AlertKind::TPSDrop)
                .unwrap()
                .consecutive_failures,
            2
        );

        // Set TPS above threshold - should recover
        gatherer.set_tps(2000);
        engine.evaluate_rules(&*gatherer).await.unwrap();
        assert!(!engine.state.get(&AlertKind::TPSDrop).unwrap().firing);
        assert_eq!(
            engine
                .state
                .get(&AlertKind::TPSDrop)
                .unwrap()
                .consecutive_failures,
            0
        );
    }

    #[tokio::test]
    async fn test_validator_offline_alert() {
        let config = AlertsConfig {
            enabled: true,
            evaluation_interval_secs: 1,
            webhook_url: None,
            log_on_fire: true,
            rules: RulesConfig {
                tps_drop: TpsDropConfig {
                    enabled: false,
                    ..Default::default()
                },
                oracle_timeout: OracleTimeoutConfig {
                    enabled: false,
                    ..Default::default()
                },
                validator_offline: ValidatorOfflineConfig {
                    enabled: true,
                    offline_secs: 30,
                    consecutive: 1,
                },
            },
        };

        #[cfg(feature = "metrics")]
        let registry = Registry::new();
        #[cfg(feature = "metrics")]
        let mut engine = AlertsEngine::new(config, &registry).unwrap();
        #[cfg(not(feature = "metrics"))]
        let mut engine = AlertsEngine::new(config).unwrap();

        let gatherer = Arc::new(MockMetricsGatherer::new());

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Set validator heartbeat to 40 seconds ago (offline)
        gatherer.set_validator_heartbeat("validator1".to_string(), now - 40);

        // Should fire immediately (consecutive = 1)
        engine.evaluate_rules(&*gatherer).await.unwrap();
        assert!(
            engine
                .state
                .get(&AlertKind::ValidatorOffline)
                .unwrap()
                .firing
        );

        // Update heartbeat to recent (online)
        gatherer.set_validator_heartbeat("validator1".to_string(), now - 10);
        engine.evaluate_rules(&*gatherer).await.unwrap();
        assert!(
            !engine
                .state
                .get(&AlertKind::ValidatorOffline)
                .unwrap()
                .firing
        );
    }

    #[tokio::test]
    async fn test_alerts_disabled_has_no_performance_penalty() {
        let config = AlertsConfig {
            enabled: false,
            ..Default::default()
        };

        #[cfg(feature = "metrics")]
        let registry = Registry::new();
        #[cfg(feature = "metrics")]
        let mut engine = AlertsEngine::new(config, &registry).unwrap();
        #[cfg(not(feature = "metrics"))]
        let mut engine = AlertsEngine::new(config).unwrap();

        let gatherer = Arc::new(MockMetricsGatherer::new());

        // Measure time for evaluation when disabled
        let start = std::time::Instant::now();

        // This should return immediately since alerts are disabled
        let result = tokio::time::timeout(Duration::from_millis(50), engine.start(gatherer)).await;

        let elapsed = start.elapsed();

        // Should complete very quickly when disabled
        assert!(elapsed < Duration::from_millis(10));
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_node_metrics_gatherer_integration() {
        let tps_window = Arc::new(Mutex::new(super::super::storage::blocks::TpsWindow::new(
            60,
        )));
        let gatherer = super::NodeMetricsGatherer::new(tps_window.clone());

        // Add some test data
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        {
            let mut window = tps_window.lock().unwrap();
            window.record_block(now - 10, 100);
            window.record_block(now - 5, 150);
            window.record_block(now, 200);
        }

        let tps = gatherer.get_current_tps();
        assert!(tps > 0.0);

        // Oracle latency should return None (not implemented yet)
        assert_eq!(gatherer.get_oracle_latency_p95_ms(), None);

        // Validator heartbeats should return empty (not implemented yet)
        assert!(gatherer.get_validator_heartbeats().is_empty());
    }

    #[tokio::test]
    async fn test_end_to_end_alert_workflow() {
        // Create minimal config for testing
        let config = AlertsConfig {
            enabled: true,
            evaluation_interval_secs: 1,
            webhook_url: None,
            log_on_fire: false, // Disable logging for tests
            rules: RulesConfig {
                tps_drop: TpsDropConfig {
                    enabled: true,
                    threshold: 500.0,
                    consecutive: 2,
                },
                oracle_timeout: OracleTimeoutConfig {
                    enabled: false,
                    ..Default::default()
                },
                validator_offline: ValidatorOfflineConfig {
                    enabled: false,
                    ..Default::default()
                },
            },
        };

        #[cfg(feature = "metrics")]
        let registry = Registry::new();
        #[cfg(feature = "metrics")]
        let mut engine = AlertsEngine::new(config, &registry).unwrap();
        #[cfg(not(feature = "metrics"))]
        let mut engine = AlertsEngine::new(config).unwrap();

        let gatherer = Arc::new(MockMetricsGatherer::new());

        // Set TPS below threshold
        gatherer.set_tps(400); // Below 500 threshold

        // First evaluation - should not fire yet (consecutive = 2)
        engine.evaluate_rules(&*gatherer).await.unwrap();
        assert!(!engine.state.get(&AlertKind::TPSDrop).unwrap().firing);
        assert_eq!(
            engine
                .state
                .get(&AlertKind::TPSDrop)
                .unwrap()
                .consecutive_failures,
            1
        );

        // Second evaluation - should fire now
        engine.evaluate_rules(&*gatherer).await.unwrap();
        assert!(engine.state.get(&AlertKind::TPSDrop).unwrap().firing);
        assert_eq!(
            engine
                .state
                .get(&AlertKind::TPSDrop)
                .unwrap()
                .consecutive_failures,
            2
        );

        // Recover - set TPS above threshold
        gatherer.set_tps(600); // Above 500 threshold
        engine.evaluate_rules(&*gatherer).await.unwrap();
        assert!(!engine.state.get(&AlertKind::TPSDrop).unwrap().firing);
        assert_eq!(
            engine
                .state
                .get(&AlertKind::TPSDrop)
                .unwrap()
                .consecutive_failures,
            0
        );
    }
}
