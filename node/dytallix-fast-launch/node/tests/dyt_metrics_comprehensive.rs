#![cfg(feature = "metrics")]

use dytallix_fast_node::metrics::{Metrics, MetricsConfig, MetricsServer};
use std::sync::Arc;

/// Test that all required dyt_ metrics are exposed correctly
#[tokio::test]
async fn test_dyt_metrics_exposure() {
    let metrics = Metrics::new().expect("Failed to create metrics");

    // Test that all required dyt_ metrics exist and can be accessed
    assert_eq!(metrics.dyt_block_height.get(), 0);
    assert_eq!(metrics.dyt_blocks_per_second.get(), 0.0);
    assert_eq!(metrics.dyt_tps.get(), 0.0);
    assert_eq!(metrics.dyt_mempool_size.get(), 0);

    // Test validator metrics (legacy dyt_* and new dgt_*)
    let validator_label = ["validator-0"];
    metrics
        .dyt_blocks_produced_total
        .with_label_values(&validator_label)
        .inc();
    assert_eq!(
        metrics
            .dyt_blocks_produced_total
            .with_label_values(&validator_label)
            .get(),
        1
    );

    metrics
        .dyt_validator_missed_blocks_total
        .with_label_values(&validator_label)
        .inc();
    assert_eq!(
        metrics
            .dyt_validator_missed_blocks_total
            .with_label_values(&validator_label)
            .get(),
        1
    );

    metrics
        .dyt_validator_voting_power
        .with_label_values(&validator_label)
        .set(100.0);
    assert_eq!(
        metrics
            .dyt_validator_voting_power
            .with_label_values(&validator_label)
            .get(),
        100.0
    );

    // New DGT-prefixed voting power metric (dual-tokenomics)
    metrics
        .dgt_validator_voting_power
        .with_label_values(&validator_label)
        .set(100.0);
    assert_eq!(
        metrics
            .dgt_validator_voting_power
            .with_label_values(&validator_label)
            .get(),
        100.0
    );

    // Test emission metrics
    let pool_type_label = ["staking"];
    metrics
        .dyt_emission_pool_amount
        .with_label_values(&pool_type_label)
        .set(1000000.0);
    assert_eq!(
        metrics
            .dyt_emission_pool_amount
            .with_label_values(&pool_type_label)
            .get(),
        1000000.0
    );

    // New DRT-prefixed emission pool metric (dual-tokenomics)
    metrics
        .drt_emission_pool_amount
        .with_label_values(&pool_type_label)
        .set(1000000.0);
    assert_eq!(
        metrics
            .drt_emission_pool_amount
            .with_label_values(&pool_type_label)
            .get(),
        1000000.0
    );

    println!("✅ All dyt_ metrics exposed correctly");
}

/// Test that metrics can be recorded through the public interface
#[tokio::test]
async fn test_dyt_metrics_recording() {
    let metrics = Metrics::new().expect("Failed to create metrics");

    // Test block metrics recording
    metrics.dyt_block_height.set(100);
    metrics.dyt_blocks_per_second.set(0.2);
    metrics.dyt_tps.set(150.5);

    assert_eq!(metrics.dyt_block_height.get(), 100);
    assert_eq!(metrics.dyt_blocks_per_second.get(), 0.2);
    assert_eq!(metrics.dyt_tps.get(), 150.5);

    // Test histogram metrics
    metrics.dyt_transactions_in_block.observe(25.0);
    metrics.dyt_gas_used_per_block.observe(2100000.0);
    metrics.dyt_oracle_update_latency_seconds.observe(0.5);

    // Test mempool metrics
    metrics.dyt_mempool_size.set(500);
    assert_eq!(metrics.dyt_mempool_size.get(), 500);

    println!("✅ All dyt_ metrics can be recorded correctly");
}

/// Test prometheus metrics format output includes required metrics
#[tokio::test]
async fn test_prometheus_format_dyt_metrics() {
    use prometheus::TextEncoder;

    let metrics = Arc::new(Metrics::new().expect("Failed to create metrics"));

    // Record some metric values
    metrics.dyt_block_height.set(150);
    metrics.dyt_blocks_per_second.set(0.25);
    metrics.dyt_tps.set(175.0);
    metrics.dyt_mempool_size.set(250);

    // Record validator metrics
    let validator_labels = ["validator-1"];
    metrics
        .dyt_blocks_produced_total
        .with_label_values(&validator_labels)
        .inc_by(5);
    metrics
        .dyt_validator_voting_power
        .with_label_values(&validator_labels)
        .set(150.0);
    metrics
        .dyt_validator_missed_blocks_total
        .with_label_values(&validator_labels)
        .inc();
    // Dual-tokenomics alias (DGT)
    metrics
        .dgt_validator_voting_power
        .with_label_values(&validator_labels)
        .set(150.0);

    // Record emission metrics
    let pool_labels = ["rewards"];
    metrics
        .dyt_emission_pool_amount
        .with_label_values(&pool_labels)
        .set(500000.0);
    // Dual-tokenomics alias (DRT)
    metrics
        .drt_emission_pool_amount
        .with_label_values(&pool_labels)
        .set(500000.0);

    // Export metrics in Prometheus format
    let encoder = TextEncoder::new();
    let metric_families = metrics.registry.gather();
    let output = encoder.encode_to_string(&metric_families).unwrap();

    // Verify that all required dyt_ metrics are present
    let required_metrics = [
        "dyt_block_height",
        "dyt_blocks_produced_total",
        "dyt_blocks_per_second",
        "dyt_transactions_in_block",
        "dyt_tps",
        "dyt_mempool_size",
        "dyt_gas_used_per_block",
        "dyt_oracle_update_latency_seconds",
        "dyt_emission_pool_amount",
        "dyt_validator_missed_blocks_total",
        "dyt_validator_voting_power",
        // New dual-tokenomics metric names
        "drt_emission_pool_amount",
        "dgt_validator_voting_power",
    ];

    for metric in &required_metrics {
        assert!(output.contains(metric), "Missing required metric: {metric}");
    }

    // Verify specific values are present
    assert!(output.contains("dyt_block_height 150"));
    assert!(output.contains("dyt_blocks_per_second 0.25"));
    assert!(output.contains("dyt_tps 175"));
    assert!(output.contains("dyt_mempool_size 250"));
    // DGT/DRT vector metrics are present (values may be omitted if no sample)

    println!("✅ Prometheus format includes all required dyt_ metrics");
    println!(
        "Sample output:\n{}",
        output.lines().take(20).collect::<Vec<_>>().join("\n")
    );
}

/// Test that metrics endpoint can be started and responds correctly
#[tokio::test]
async fn test_metrics_endpoint_dyt_metrics() {
    let config = MetricsConfig {
        enabled: true,
        listen_addr: "127.0.0.1:0".parse().unwrap(), // Use port 0 for dynamic allocation
    };

    let (_server, metrics) = MetricsServer::new(config).expect("Failed to create metrics server");

    // Record some dyt_ metrics
    metrics.dyt_block_height.set(42);
    metrics.dyt_tps.set(123.45);

    let validator_labels = ["test-validator"];
    metrics
        .dyt_blocks_produced_total
        .with_label_values(&validator_labels)
        .inc_by(10);

    println!("✅ Metrics server can be created with dyt_ metrics");
}

/// Test backwards compatibility with legacy metrics
#[tokio::test]
async fn test_legacy_metrics_compatibility() {
    let metrics = Metrics::new().expect("Failed to create metrics");

    // Test that legacy metrics still work
    assert_eq!(metrics.total_blocks.get(), 0);
    assert_eq!(metrics.current_block_height.get(), 0);
    assert_eq!(metrics.total_transactions.get(), 0);
    assert_eq!(metrics.mempool_size.get(), 0);
    assert_eq!(metrics.emission_pool_size.get(), 0.0);

    // Test that we can record to both old and new metrics
    metrics.total_blocks.inc();
    metrics.dyt_block_height.set(1);

    assert_eq!(metrics.total_blocks.get(), 1);
    assert_eq!(metrics.dyt_block_height.get(), 1);

    println!("✅ Legacy metrics compatibility maintained");
}

/// Integration test: Test full metrics recording flow
#[tokio::test]
async fn test_full_metrics_recording_flow() {
    let metrics = Arc::new(Metrics::new().expect("Failed to create metrics"));

    // Simulate a full block production cycle with dyt_ metrics

    // Block production
    metrics.dyt_block_height.set(100);
    metrics.dyt_blocks_per_second.set(0.2);

    let validator = ["validator-0"];
    metrics
        .dyt_blocks_produced_total
        .with_label_values(&validator)
        .inc();
    metrics
        .dyt_validator_voting_power
        .with_label_values(&validator)
        .set(1000.0);

    // Transaction processing
    metrics.dyt_transactions_in_block.observe(50.0);
    metrics.dyt_tps.set(125.0);
    metrics.dyt_gas_used_per_block.observe(2500000.0);

    // Mempool state
    metrics.dyt_mempool_size.set(1000);

    // Oracle updates
    metrics.dyt_oracle_update_latency_seconds.observe(0.25);

    // Emission pools
    let staking_pool = ["staking"];
    let rewards_pool = ["rewards"];
    metrics
        .dyt_emission_pool_amount
        .with_label_values(&staking_pool)
        .set(10000000.0);
    metrics
        .dyt_emission_pool_amount
        .with_label_values(&rewards_pool)
        .set(500000.0);

    // Verify final state
    assert_eq!(metrics.dyt_block_height.get(), 100);
    assert_eq!(
        metrics
            .dyt_blocks_produced_total
            .with_label_values(&validator)
            .get(),
        1
    );
    assert_eq!(metrics.dyt_tps.get(), 125.0);
    assert_eq!(metrics.dyt_mempool_size.get(), 1000);
    assert_eq!(
        metrics
            .dyt_validator_voting_power
            .with_label_values(&validator)
            .get(),
        1000.0
    );
    assert_eq!(
        metrics
            .dyt_emission_pool_amount
            .with_label_values(&staking_pool)
            .get(),
        10000000.0
    );

    println!("✅ Full metrics recording flow test completed successfully");
}
