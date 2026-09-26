//! Prometheus metrics exporter for Dytallix node
//!
//! This module provides optional observability functionality that can be enabled
//! via CLI flags or environment variables. When disabled, it has zero performance impact.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;




#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;




    #[tokio::test]
    async fn test_metrics_config_default() {
        // Test default config
        let default_config = MetricsConfig::default();
        assert!(!default_config.enabled);
        assert_eq!(default_config.listen_addr.port(), 9464);
    }

    #[tokio::test]
    async fn test_metrics_disabled_no_ops() {
        let metrics = Metrics::new().expect("Should create no-op metrics");

        // All operations should be no-ops when metrics feature is disabled
        metrics.record_block(1, 5, 100, Duration::from_millis(250));
        metrics.update_mempool_size(25);
        metrics.record_oracle_update(Duration::from_millis(150));
        metrics.update_emission_pool(1500.0);

        // If we get here without panicking, the no-op implementation works
    }

    #[tokio::test]
    async fn test_disabled_metrics_config() {
        let config = MetricsConfig {
            enabled: false,
            listen_addr: "127.0.0.1:9464".parse().unwrap(),
        };

        let (server, _metrics) =
            MetricsServer::new(config).expect("Should create server even when disabled");

        // Starting a disabled server should return immediately
        let result = tokio::time::timeout(Duration::from_millis(100), server.start()).await;
        assert!(result.is_ok(), "Disabled server should start immediately");
    }
}

/// Metrics configuration
#[derive(Debug, Clone)]
pub struct MetricsConfig {
    pub enabled: bool,
    pub listen_addr: SocketAddr,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen_addr: "0.0.0.0:9464".parse().unwrap(),
        }
    }
}



pub struct Metrics;

impl Metrics {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self)
    }

    pub fn record_block(
        &self,
        _height: u64,
        _tx_count: usize,
        _gas_used: u64,
        _processing_time: Duration,
    ) {
    }
    pub fn update_mempool_size(&self, _size: usize) {}
    pub fn update_mempool_bytes(&self, _bytes: usize) {}
    pub fn record_mempool_admission(&self) {}
    pub fn record_mempool_rejection(&self, _reason: &str) {}
    pub fn record_mempool_eviction(&self, _reason: &str) {}
    pub fn update_mempool_min_gas_price(&self, _gas_price: u64) {}
    pub fn record_gossip_duplicate(&self) {}
    pub fn record_transaction(&self, _processing_time: Duration) {}
    pub fn record_oracle_update(&self, _latency: Duration) {}
    pub fn record_oracle_submission(&self, _status: &str) {}
    pub fn update_emission_pool(&self, _pool_size: f64) {}
    pub fn update_emission_apply(&self, _height: u64, _pending_udrt_total: u128, _ts: u64) {}
    pub fn update_current_block_gas(&self, _gas: u64) {}
}

/// Metrics server handle
pub struct MetricsServer {
    config: MetricsConfig,
}

impl MetricsServer {
    pub fn new(config: MetricsConfig) -> anyhow::Result<(Self, Arc<Metrics>)> {
        let metrics = Arc::new(Metrics::new()?);
        let server = Self {
            config,
        };
        Ok((server, metrics))
    }

    /// Start the metrics server if enabled
    pub async fn start(self) -> anyhow::Result<()> {
        if !self.config.enabled {
            // When disabled, this function returns immediately with no overhead
            println!("Metrics collection disabled");
            return Ok(());
        }


        {
            println!("Metrics feature not compiled in");
        }

        Ok(())
    }
}


/// Parse metrics configuration from environment and CLI args
pub fn parse_metrics_config() -> MetricsConfig {

    {
        // Check environment variables even without metrics feature
        let enabled = std::env::var("DY_METRICS")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        if enabled {
            eprintln!("Warning: DY_METRICS=1 but metrics feature not compiled in. Rebuild with --features metrics");
        }

        MetricsConfig::default()
    }
}
