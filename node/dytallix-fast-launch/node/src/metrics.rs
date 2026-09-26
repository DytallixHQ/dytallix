//! Prometheus metrics exporter for Dytallix node
//!
//! This module provides optional observability functionality that can be enabled
//! via CLI flags or environment variables. When disabled, it has zero performance impact.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

#[cfg(feature = "metrics")]
use prometheus::{
    Gauge, Histogram, HistogramOpts, IntCounter, IntGauge, Opts, Registry, TextEncoder,
};

#[cfg(feature = "metrics")]
use axum::{extract::Extension, http::StatusCode, response::Response, routing::get, Router};

#[cfg(feature = "metrics")]
use tokio::net::TcpListener;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[cfg(feature = "metrics")]
    #[tokio::test]
    async fn test_metrics_creation() {
        let metrics = Metrics::new().expect("Failed to create metrics");

        // Test initial values
        assert_eq!(metrics.total_blocks.get(), 0);
        assert_eq!(metrics.total_transactions.get(), 0);
        assert_eq!(metrics.mempool_size.get(), 0);
        assert_eq!(metrics.build_info.get(), 1); // Should be set to 1 on creation
    }

    #[cfg(feature = "metrics")]
    #[tokio::test]
    async fn test_metrics_recording() {
        let metrics = Metrics::new().expect("Failed to create metrics");

        // Test block recording
        metrics.record_block(1, 5, 100, Duration::from_millis(250));
        assert_eq!(metrics.total_blocks.get(), 1);
        assert_eq!(metrics.total_transactions.get(), 5);
        assert_eq!(metrics.current_block_height.get(), 1);
        assert_eq!(metrics.total_gas_used.get(), 100);

        // Test mempool update
        metrics.update_mempool_size(25);
        assert_eq!(metrics.mempool_size.get(), 25);

        // Test oracle update
        metrics.record_oracle_update(Duration::from_millis(150));
        // Just ensure it doesn't panic - histogram values are not directly testable

        // Test emission pool update
        metrics.update_emission_pool(1500.0);
        assert_eq!(metrics.emission_pool_size.get(), 1500.0);
    }

    #[cfg(feature = "metrics")]
    #[tokio::test]
    async fn test_metrics_server_creation() {
        let config = MetricsConfig {
            enabled: true,
            listen_addr: "127.0.0.1:0".parse().unwrap(), // Use port 0 for dynamic allocation
        };

        let (_server, metrics) =
            MetricsServer::new(config).expect("Failed to create metrics server");

        // Test that we can use the metrics
        metrics.record_block(1, 0, 0, Duration::from_millis(100));
        assert_eq!(metrics.total_blocks.get(), 1);
    }

    #[tokio::test]
    async fn test_metrics_config_default() {
        // Test default config
        let default_config = MetricsConfig::default();
        assert!(!default_config.enabled);
        assert_eq!(default_config.listen_addr.port(), 9464);
    }

    #[cfg(not(feature = "metrics"))]
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

/// Main metrics context containing all metric collectors
#[cfg(feature = "metrics")]
pub struct Metrics {
    pub registry: Registry, // made public for test access; consider exposing gather() instead

    // Block metrics - new dyt_ prefixed metrics
    pub dyt_block_height: IntGauge,
    pub dyt_blocks_produced_total: prometheus::IntCounterVec,
    pub dyt_blocks_per_second: Gauge,
    pub dyt_transactions_in_block: Histogram,
    pub dyt_tps: Gauge,
    pub dyt_block_time_seconds: Histogram,
    pub dyt_block_last_time_seconds: Gauge,
    pub dyt_txs_processed_total: IntCounter,
    // Simple aliases requested
    pub block_time: Histogram,
    pub tps: Gauge,
    pub mempool_len: IntGauge,

    // Legacy block metrics
    pub total_blocks: IntCounter,
    pub current_block_height: IntGauge,
    pub block_processing_time: Histogram,

    // Transaction metrics
    pub total_transactions: IntCounter,
    pub transaction_processing_time: Histogram,

    // Mempool metrics - new dyt_ prefixed
    pub dyt_mempool_size: IntGauge,

    // Legacy mempool metrics
    pub mempool_size: IntGauge,
    pub mempool_bytes: IntGauge,
    pub mempool_admitted_total: IntCounter,
    pub mempool_rejected_total: prometheus::IntCounterVec,
    pub mempool_evicted_total: prometheus::IntCounterVec,
    pub mempool_current_min_gas_price: IntGauge,
    pub mempool_gossip_duplicates_total: IntCounter,

    // Gas metrics - new dyt_ prefixed
    pub dyt_gas_used_per_block: Histogram,

    // Legacy gas metrics
    pub total_gas_used: IntCounter,
    pub current_block_gas: IntGauge,

    // Oracle metrics - new dyt_ prefixed
    pub dyt_oracle_update_latency_seconds: Histogram,
    pub dyt_oracle_request_latency_seconds: Histogram,

    // Oracle metrics - enhanced
    pub oracle_submit_total: prometheus::IntCounterVec,
    pub oracle_latency_seconds: Histogram,

    // Legacy oracle metrics
    pub oracle_latency: Histogram,
    pub last_oracle_update: IntGauge,

    // Emission metrics - new dyt_ prefixed
    pub dyt_emission_pool_amount: prometheus::GaugeVec,
    pub dyt_emission_pool_balance: prometheus::GaugeVec,

    // Emission metrics - DRT-prefixed (dual-tokenomics)
    pub drt_emission_pool_amount: prometheus::GaugeVec,
    pub drt_emission_pool_balance: prometheus::GaugeVec,

    // Validator metrics - new dyt_ prefixed
    pub dyt_validator_missed_blocks_total: prometheus::IntCounterVec,
    pub dyt_validator_voting_power: prometheus::GaugeVec,
    // Governance/Staking metrics - DGT-prefixed (dual-tokenomics)
    pub dgt_validator_voting_power: prometheus::GaugeVec,

    // Legacy emission metrics
    pub emission_pool_size: Gauge,

    // System metrics
    pub build_info: IntGauge,

    // Emissions ops metrics (uDRT)
    pub drt_emission_applied_height: IntGauge,
    pub drt_emission_pending_claims: IntGauge,
    pub drt_emission_last_apply_timestamp_seconds: IntGauge,
}

#[cfg(feature = "metrics")]
impl Metrics {
    pub fn new() -> anyhow::Result<Self> {
        let registry = Registry::new();

        // Block metrics - using dyt_ prefix as per spec
        let dyt_block_height =
            IntGauge::with_opts(Opts::new("dyt_block_height", "Current blockchain height"))?;
        registry.register(Box::new(dyt_block_height.clone()))?;

        let dyt_blocks_produced_total = prometheus::IntCounterVec::new(
            Opts::new(
                "dyt_blocks_produced_total",
                "Total number of blocks produced by validator",
            ),
            &["validator"],
        )?;
        registry.register(Box::new(dyt_blocks_produced_total.clone()))?;

        let dyt_blocks_per_second = Gauge::with_opts(Opts::new(
            "dyt_blocks_per_second",
            "Current blocks per second rate",
        ))?;
        registry.register(Box::new(dyt_blocks_per_second.clone()))?;

        let dyt_transactions_in_block = Histogram::with_opts(HistogramOpts::new(
            "dyt_transactions_in_block",
            "Number of transactions per block",
        ))?;
        registry.register(Box::new(dyt_transactions_in_block.clone()))?;

        let dyt_tps = Gauge::with_opts(Opts::new(
            "dyt_tps",
            "Transactions per second - rolling 1m average",
        ))?;
        registry.register(Box::new(dyt_tps.clone()))?;

        let dyt_block_time_seconds = Histogram::with_opts(
            HistogramOpts::new("dyt_block_time_seconds", "Block processing time in seconds")
                .buckets(vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0]),
        )?;
        registry.register(Box::new(dyt_block_time_seconds.clone()))?;

        let dyt_block_last_time_seconds = Gauge::with_opts(Opts::new(
            "dyt_block_last_time_seconds",
            "Unix timestamp of last block",
        ))?;
        registry.register(Box::new(dyt_block_last_time_seconds.clone()))?;

        let dyt_txs_processed_total = IntCounter::with_opts(Opts::new(
            "dyt_txs_processed_total",
            "Total number of transactions processed",
        ))?;
        registry.register(Box::new(dyt_txs_processed_total.clone()))?;

        // Aliases requested by ops
        let block_time = Histogram::with_opts(
            HistogramOpts::new("block_time", "Block processing time (seconds)")
                .buckets(vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0]),
        )?;
        registry.register(Box::new(block_time.clone()))?;

        let tps = Gauge::with_opts(Opts::new("tps", "Transactions per second (rolling)"))?;
        registry.register(Box::new(tps.clone()))?;

        let mempool_len = IntGauge::with_opts(Opts::new(
            "mempool_len",
            "Current number of pending transactions in mempool",
        ))?;
        registry.register(Box::new(mempool_len.clone()))?;

        let block_processing_time = Histogram::with_opts(HistogramOpts::new(
            "dytallix_block_processing_seconds",
            "Time spent processing blocks",
        ))?;
        registry.register(Box::new(block_processing_time.clone()))?;

        // Legacy block metrics
        let total_blocks = IntCounter::with_opts(Opts::new(
            "dytallix_total_blocks",
            "Total number of blocks produced",
        ))?;
        registry.register(Box::new(total_blocks.clone()))?;

        let current_block_height = IntGauge::with_opts(Opts::new(
            "dytallix_current_block_height",
            "Current blockchain height",
        ))?;
        registry.register(Box::new(current_block_height.clone()))?;

        // Transaction metrics
        let total_transactions = IntCounter::with_opts(Opts::new(
            "dytallix_total_transactions",
            "Total number of transactions processed",
        ))?;
        registry.register(Box::new(total_transactions.clone()))?;

        let dyt_mempool_size = IntGauge::with_opts(Opts::new(
            "dyt_mempool_size",
            "Current number of pending transactions in mempool",
        ))?;
        registry.register(Box::new(dyt_mempool_size.clone()))?;

        // Legacy mempool size metric
        let mempool_size = IntGauge::with_opts(Opts::new(
            "dytallix_mempool_size",
            "Current number of pending transactions in mempool",
        ))?;
        registry.register(Box::new(mempool_size.clone()))?;

        let mempool_bytes = IntGauge::with_opts(Opts::new(
            "dytallix_mempool_bytes",
            "Current total bytes of pending transactions in mempool",
        ))?;
        registry.register(Box::new(mempool_bytes.clone()))?;

        let transaction_processing_time = Histogram::with_opts(HistogramOpts::new(
            "dytallix_transaction_processing_seconds",
            "Time spent processing individual transactions",
        ))?;
        registry.register(Box::new(transaction_processing_time.clone()))?;

        // Mempool-specific metrics
        let mempool_admitted_total = IntCounter::with_opts(Opts::new(
            "dytallix_mempool_admitted_total",
            "Total number of transactions admitted to mempool",
        ))?;
        registry.register(Box::new(mempool_admitted_total.clone()))?;

        let mempool_rejected_total = prometheus::IntCounterVec::new(
            Opts::new(
                "dytallix_mempool_rejected_total",
                "Total number of transactions rejected by mempool",
            ),
            &["reason"],
        )?;
        registry.register(Box::new(mempool_rejected_total.clone()))?;

        let mempool_evicted_total = prometheus::IntCounterVec::new(
            Opts::new(
                "dytallix_mempool_evicted_total",
                "Total number of transactions evicted from mempool",
            ),
            &["reason"],
        )?;
        registry.register(Box::new(mempool_evicted_total.clone()))?;

        let mempool_current_min_gas_price = IntGauge::with_opts(Opts::new(
            "dytallix_mempool_current_min_gas_price",
            "Current minimum gas price in the mempool",
        ))?;
        registry.register(Box::new(mempool_current_min_gas_price.clone()))?;

        let mempool_gossip_duplicates_total = IntCounter::with_opts(Opts::new(
            "dytallix_mempool_gossip_duplicates_total",
            "Total number of duplicate transactions suppressed in gossip",
        ))?;
        registry.register(Box::new(mempool_gossip_duplicates_total.clone()))?;

        // Gas metrics - using dyt_ prefix
        let dyt_gas_used_per_block = Histogram::with_opts(HistogramOpts::new(
            "dyt_gas_used_per_block",
            "Gas used per block",
        ))?;
        registry.register(Box::new(dyt_gas_used_per_block.clone()))?;

        let total_gas_used = IntCounter::with_opts(Opts::new(
            "dytallix_total_gas_used",
            "Total gas consumed by all transactions",
        ))?;
        registry.register(Box::new(total_gas_used.clone()))?;

        let current_block_gas = IntGauge::with_opts(Opts::new(
            "dytallix_current_block_gas",
            "Gas used in the current block being processed",
        ))?;
        registry.register(Box::new(current_block_gas.clone()))?;

        // Oracle metrics - using dyt_ prefix
        let dyt_oracle_update_latency_seconds = Histogram::with_opts(HistogramOpts::new(
            "dyt_oracle_update_latency_seconds",
            "Latency of oracle data updates in seconds",
        ))?;
        registry.register(Box::new(dyt_oracle_update_latency_seconds.clone()))?;

        let dyt_oracle_request_latency_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "dyt_oracle_request_latency_seconds",
                "Oracle request latency in seconds",
            )
            .buckets(vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0]),
        )?;
        registry.register(Box::new(dyt_oracle_request_latency_seconds.clone()))?;

        // Oracle metrics - enhanced with specific requirements
        let oracle_submit_total = prometheus::IntCounterVec::new(
            Opts::new("oracle_submit_total", "Total oracle submissions"),
            &["status"],
        )?;
        registry.register(Box::new(oracle_submit_total.clone()))?;

        let oracle_latency_seconds = Histogram::with_opts(HistogramOpts {
            common_opts: Opts::new(
                "oracle_latency_seconds",
                "Oracle ingest to persistence latency in seconds",
            ),
            buckets: vec![0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0],
        })?;
        registry.register(Box::new(oracle_latency_seconds.clone()))?;

        // Legacy oracle metrics
        let oracle_latency = Histogram::with_opts(HistogramOpts::new(
            "dytallix_oracle_latency_seconds",
            "Latency of oracle data updates",
        ))?;
        registry.register(Box::new(oracle_latency.clone()))?;

        let last_oracle_update = IntGauge::with_opts(Opts::new(
            "dytallix_last_oracle_update_timestamp",
            "Timestamp of the last oracle update",
        ))?;
        registry.register(Box::new(last_oracle_update.clone()))?;

        // Emission metrics - using dyt_ prefix
        let dyt_emission_pool_amount = prometheus::GaugeVec::new(
            Opts::new(
                "dyt_emission_pool_amount",
                "Current amount in emission pools by pool type",
            ),
            &["pool_type"],
        )?;
        registry.register(Box::new(dyt_emission_pool_amount.clone()))?;

        let dyt_emission_pool_balance = prometheus::GaugeVec::new(
            Opts::new(
                "dyt_emission_pool_balance",
                "Current balance in emission pools by pool",
            ),
            &["pool"],
        )?;
        registry.register(Box::new(dyt_emission_pool_balance.clone()))?;

        // Emission metrics - DRT-prefixed (mirrors of dyt_* for migration)
        let drt_emission_pool_amount = prometheus::GaugeVec::new(
            Opts::new(
                "drt_emission_pool_amount",
                "Current amount in DRT emission pools by pool type",
            ),
            &["pool_type"],
        )?;
        registry.register(Box::new(drt_emission_pool_amount.clone()))?;

        let drt_emission_pool_balance = prometheus::GaugeVec::new(
            Opts::new(
                "drt_emission_pool_balance",
                "Current balance in DRT emission pools by pool",
            ),
            &["pool"],
        )?;
        registry.register(Box::new(drt_emission_pool_balance.clone()))?;

        // Validator metrics - using dyt_ prefix
        let dyt_validator_missed_blocks_total = prometheus::IntCounterVec::new(
            Opts::new(
                "dyt_validator_missed_blocks_total",
                "Total number of blocks missed by validator",
            ),
            &["validator"],
        )?;
        registry.register(Box::new(dyt_validator_missed_blocks_total.clone()))?;

        let dyt_validator_voting_power = prometheus::GaugeVec::new(
            Opts::new(
                "dyt_validator_voting_power",
                "Current voting power of validators",
            ),
            &["validator"],
        )?;
        registry.register(Box::new(dyt_validator_voting_power.clone()))?;

        // Governance/Staking metrics - DGT-prefixed (migration alias)
        let dgt_validator_voting_power = prometheus::GaugeVec::new(
            Opts::new(
                "dgt_validator_voting_power",
                "Current voting power of validators (DGT governance context)",
            ),
            &["validator"],
        )?;
        registry.register(Box::new(dgt_validator_voting_power.clone()))?;

        // Legacy emission metric
        let emission_pool_size = Gauge::with_opts(Opts::new(
            "dytallix_emission_pool_size",
            "Current size of the emission/reward pool",
        ))?;
        registry.register(Box::new(emission_pool_size.clone()))?;

        // Build info
        let build_info = IntGauge::with_opts(Opts::new(
            "dytallix_build_info",
            "Build information with version and commit labels",
        ))?;
        registry.register(Box::new(build_info.clone()))?;

        // Emissions ops metrics
        let drt_emission_applied_height = IntGauge::with_opts(Opts::new(
            "drt_emission_applied_height",
            "Last block height at which emissions were applied",
        ))?;
        registry.register(Box::new(drt_emission_applied_height.clone()))?;

        let drt_emission_pending_claims = IntGauge::with_opts(Opts::new(
            "drt_emission_pending_claims",
            "Total pending uDRT across emission pools",
        ))?;
        registry.register(Box::new(drt_emission_pending_claims.clone()))?;

        let drt_emission_last_apply_timestamp_seconds = IntGauge::with_opts(Opts::new(
            "drt_emission_last_apply_timestamp_seconds",
            "Unix timestamp of last emissions apply",
        ))?;
        registry.register(Box::new(drt_emission_last_apply_timestamp_seconds.clone()))?;

        // Set build info to 1
        build_info.set(1);

        Ok(Self {
            registry,
            // New dyt_ prefixed metrics
            dyt_block_height,
            dyt_blocks_produced_total,
            dyt_blocks_per_second,
            dyt_transactions_in_block,
            dyt_tps,
            dyt_block_time_seconds,
            dyt_block_last_time_seconds,
            dyt_txs_processed_total,
            block_time,
            tps,
            mempool_len,
            dyt_mempool_size,
            dyt_gas_used_per_block,
            dyt_oracle_update_latency_seconds,
            dyt_oracle_request_latency_seconds,
            dyt_emission_pool_amount,
            dyt_emission_pool_balance,
            drt_emission_pool_amount,
            drt_emission_pool_balance,
            dyt_validator_missed_blocks_total,
            dyt_validator_voting_power,
            dgt_validator_voting_power,
            // Legacy metrics
            total_blocks,
            current_block_height,
            block_processing_time,
            total_transactions,
            mempool_size,
            mempool_bytes,
            transaction_processing_time,
            mempool_admitted_total,
            mempool_rejected_total,
            mempool_evicted_total,
            mempool_current_min_gas_price,
            mempool_gossip_duplicates_total,
            total_gas_used,
            current_block_gas,
            // Oracle metrics
            oracle_submit_total,
            oracle_latency_seconds,
            oracle_latency,
            last_oracle_update,
            emission_pool_size,
            build_info,
            drt_emission_applied_height,
            drt_emission_pending_claims,
            drt_emission_last_apply_timestamp_seconds,
        })
    }

    /// Record that a new block was produced
    pub fn record_block(
        &self,
        height: u64,
        tx_count: usize,
        gas_used: u64,
        processing_time: Duration,
    ) {
        self.total_blocks.inc();
        self.current_block_height.set(height as i64);
        self.total_transactions.inc_by(tx_count as u64);
        self.total_gas_used.inc_by(gas_used);
        self.block_processing_time
            .observe(processing_time.as_secs_f64());
        // Update dyt_* surfaces
        self.dyt_block_height.set(height as i64);
        self.dyt_block_time_seconds
            .observe(processing_time.as_secs_f64());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        self.dyt_block_last_time_seconds.set(now);
        self.dyt_transactions_in_block.observe(tx_count as f64);
        self.dyt_gas_used_per_block.observe(gas_used as f64);
        // Alias histogram
        self.block_time.observe(processing_time.as_secs_f64());
    }

    /// Update mempool size and bytes
    pub fn update_mempool_size(&self, size: usize) {
        self.mempool_size.set(size as i64);
        // dyt_* and alias
        self.dyt_mempool_size.set(size as i64);
        self.mempool_len.set(size as i64);
    }

    pub fn update_mempool_bytes(&self, bytes: usize) {
        self.mempool_bytes.set(bytes as i64);
    }

    /// Record mempool admission
    pub fn record_mempool_admission(&self) {
        self.mempool_admitted_total.inc();
    }

    /// Record mempool rejection with reason
    pub fn record_mempool_rejection(&self, reason: &str) {
        self.mempool_rejected_total
            .with_label_values(&[reason])
            .inc();
    }

    /// Record mempool eviction with reason
    pub fn record_mempool_eviction(&self, reason: &str) {
        self.mempool_evicted_total
            .with_label_values(&[reason])
            .inc();
    }

    /// Update current minimum gas price in mempool
    pub fn update_mempool_min_gas_price(&self, gas_price: u64) {
        self.mempool_current_min_gas_price.set(gas_price as i64);
    }

    /// Record gossip duplicate suppression
    pub fn record_gossip_duplicate(&self) {
        self.mempool_gossip_duplicates_total.inc();
    }

    /// Record transaction processing time
    pub fn record_transaction(&self, processing_time: Duration) {
        self.transaction_processing_time
            .observe(processing_time.as_secs_f64());
    }

    /// Record oracle update
    pub fn record_oracle_update(&self, latency: Duration) {
        self.oracle_latency.observe(latency.as_secs_f64());
        self.oracle_latency_seconds.observe(latency.as_secs_f64());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.last_oracle_update.set(now as i64);
    }

    /// Record oracle submission
    pub fn record_oracle_submission(&self, status: &str) {
        self.oracle_submit_total.with_label_values(&[status]).inc();
    }

    /// Update emission pool size
    pub fn update_emission_pool(&self, pool_size: f64) {
        self.emission_pool_size.set(pool_size);
    }

    /// Update emissions apply progress/telemetry
    pub fn update_emission_apply(&self, height: u64, pending_udrt_total: u128, ts: u64) {
        self.drt_emission_applied_height.set(height as i64);
        self.drt_emission_pending_claims
            .set(pending_udrt_total as i64);
        self.drt_emission_last_apply_timestamp_seconds
            .set(ts as i64);
    }

    /// Update current block gas
    pub fn update_current_block_gas(&self, gas: u64) {
        self.current_block_gas.set(gas as i64);
    }

    pub fn gather(&self) -> Vec<prometheus::proto::MetricFamily> {
        self.registry.gather()
    }
}

#[cfg(not(feature = "metrics"))]
pub struct Metrics;

#[cfg(not(feature = "metrics"))]
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
    #[cfg(feature = "metrics")]
    metrics: Arc<Metrics>,
    config: MetricsConfig,
}

impl MetricsServer {
    pub fn new(config: MetricsConfig) -> anyhow::Result<(Self, Arc<Metrics>)> {
        let metrics = Arc::new(Metrics::new()?);
        let server = Self {
            #[cfg(feature = "metrics")]
            metrics: metrics.clone(),
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

        #[cfg(feature = "metrics")]
        {
            println!("Starting metrics server on {}", self.config.listen_addr);

            let app = Router::new()
                .route("/metrics", get(metrics_handler))
                .layer(Extension(self.metrics));

            let listener = TcpListener::bind(self.config.listen_addr).await?;
            axum::serve(listener, app).await?;
        }

        #[cfg(not(feature = "metrics"))]
        {
            println!("Metrics feature not compiled in");
        }

        Ok(())
    }
}

#[cfg(feature = "metrics")]
async fn metrics_handler(
    Extension(metrics): Extension<Arc<Metrics>>,
) -> Result<Response<String>, StatusCode> {
    let encoder = TextEncoder::new();
    let metric_families = metrics.registry.gather();

    match encoder.encode_to_string(&metric_families) {
        Ok(body) => {
            let response = Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/plain; version=0.0.4; charset=utf-8")
                .body(body)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            Ok(response)
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// Parse metrics configuration from environment and CLI args
pub fn parse_metrics_config() -> MetricsConfig {
    #[cfg(feature = "metrics")]
    {
        use clap::Parser;

        #[derive(Parser)]
        #[command(about = "Dytallix Node Metrics Configuration")]
        struct Args {
            /// Enable metrics collection and export
            #[arg(long)]
            enable_metrics: bool,

            /// Metrics server listen address
            #[arg(long, default_value = "0.0.0.0:9464")]
            metrics_addr: String,
        }

        // Try to parse args, fall back to env vars only if parsing fails
        let args = Args::try_parse().unwrap_or_else(|_| {
            // Fallback to just environment variables
            let enabled = std::env::var("DY_METRICS")
                .map(|v| v == "1" || v.to_lowercase() == "true")
                .unwrap_or(false);
            let addr =
                std::env::var("DY_METRICS_ADDR").unwrap_or_else(|_| "0.0.0.0:9464".to_string());

            Args {
                enable_metrics: enabled,
                metrics_addr: addr,
            }
        });

        MetricsConfig {
            enabled: args.enable_metrics,
            listen_addr: args
                .metrics_addr
                .parse()
                .unwrap_or_else(|_| "0.0.0.0:9464".parse().unwrap()),
        }
    }

    #[cfg(not(feature = "metrics"))]
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
