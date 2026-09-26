#![cfg(feature = "metrics")]

use std::sync::Arc;
use tempfile::TempDir;

use dytallix_fast_node::mempool::Mempool;
use dytallix_fast_node::metrics::Metrics;
use dytallix_fast_node::p2p::TransactionGossip;
use dytallix_fast_node::state::State;
use dytallix_fast_node::storage::tx::Transaction;
// Added: PQC signing utilities and base64 encoder
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use dytallix_fast_node::crypto::{canonical_json, sha3_256, ActivePQC, PQC};

#[allow(clippy::too_many_arguments)]
fn create_test_transaction(
    hash: &str,
    from: &str,
    to: &str,
    amount: u128,
    fee: u128,
    nonce: u64,
    gas_limit: u64,
    gas_price: u64,
) -> Transaction {
    // Generate a fresh keypair for the test tx
    let (sk, pk) = ActivePQC::keypair();

    // Build base tx and attach PQC public key + metadata
    let mut tx = Transaction::new(hash, from, to, amount, fee, nonce, None)
        .with_gas(gas_limit, gas_price)
        .with_pqc(B64.encode(&pk), "dytallix-testnet", "metrics test");

    // Sign canonical fields and attach a valid signature
    let canonical_tx = tx.canonical_fields();
    let tx_bytes = canonical_json(&canonical_tx).expect("canonical serialize");
    let tx_hash = sha3_256(&tx_bytes);
    let signature = ActivePQC::sign(&sk, &tx_hash);
    tx.signature = Some(B64.encode(&signature));

    tx
}

fn create_mock_state() -> (State, TempDir) {
    let tmp = TempDir::new().unwrap();
    let storage = std::sync::Arc::new(
        dytallix_fast_node::storage::state::Storage::open(tmp.path().join("node.db")).unwrap(),
    );
    let mut state = State::new(storage);
    {
        let mut acc = state.get_account("sender1");
        // Increased balance to cover gas costs in tests
        acc.set_balance("udgt", 1_000_000_000);
        acc.set_balance("udrt", 1_000_000_000);
        state.accounts.insert("sender1".to_string(), acc);
    }
    {
        let mut acc = state.get_account("sender2");
        // Increased balance to cover gas costs in tests
        acc.set_balance("udgt", 1_000_000_000);
        acc.set_balance("udrt", 1_000_000_000);
        state.accounts.insert("sender2".to_string(), acc);
    }
    (state, tmp)
}

#[tokio::test]
async fn test_mempool_admission_metrics() {
    let metrics = Arc::new(Metrics::new().expect("Failed to create metrics"));
    let mut mempool = Mempool::new();
    let (state, _directory) = create_mock_state();

    // Test successful admission
    let tx1 = create_test_transaction("hash1", "sender1", "receiver", 1000, 100, 0, 21000, 1000);

    let result = mempool.add_transaction(&state, tx1);
    assert!(result.is_ok());

    // Record metrics for admission
    metrics.record_mempool_admission();
    metrics.update_mempool_size(mempool.len());
    metrics.update_mempool_bytes(mempool.total_bytes());
    metrics.update_mempool_min_gas_price(mempool.current_min_gas_price());

    // Verify metrics were recorded
    assert_eq!(metrics.mempool_admitted_total.get(), 1);
    assert_eq!(metrics.mempool_size.get(), 1);
    assert!(metrics.mempool_bytes.get() > 0);
    assert_eq!(metrics.mempool_current_min_gas_price.get(), 1000);

    // Test rejection
    let tx2 = create_test_transaction(
        "hash1", "sender2", "receiver", 1000, 100, 0, 21000, 1000, // Duplicate hash
    );

    let result = mempool.add_transaction(&state, tx2);
    assert!(result.is_err());

    if let Err(reason) = result {
        metrics.record_mempool_rejection(reason.to_metric_label());

        // Verify rejection metric
        assert_eq!(
            metrics
                .mempool_rejected_total
                .with_label_values(&["duplicate"])
                .get(),
            1
        );
    }
}

#[tokio::test]
async fn test_mempool_rejection_reasons_metrics() {
    let metrics = Arc::new(Metrics::new().expect("Failed to create metrics"));
    let mut mempool = Mempool::new();
    let (state, _directory) = create_mock_state();

    // Submit a valid transaction first to advance expected nonce for sender1 to 1
    let valid_tx = create_test_transaction(
        "hash_valid",
        "sender1",
        "receiver",
        1000,
        100,
        0,
        21000,
        1000,
    );
    assert!(mempool.add_transaction(&state, valid_tx).is_ok());

    // Test different rejection reasons
    let test_cases = vec![
        (
            // Now nonce 0 is lower than expected (1), so this should error with nonce_gap
            create_test_transaction("hash1", "sender1", "receiver", 1000, 100, 0, 21000, 1000),
            "nonce_gap",
        ),
        (
            // Bump amount well beyond mock balance to trigger insufficient funds
            create_test_transaction(
                "hash2",
                "sender1",
                "receiver",
                2_000_000_000_000u128,
                100,
                0,
                21000,
                1000,
            ),
            "insufficient_funds",
        ),
        (
            create_test_transaction("hash3", "sender1", "receiver", 1000, 100, 0, 21000, 500), // Underpriced
            "underpriced_gas",
        ),
    ];

    for (tx, expected_reason) in test_cases {
        let result = mempool.add_transaction(&state, tx);
        assert!(result.is_err());

        if let Err(reason) = result {
            metrics.record_mempool_rejection(reason.to_metric_label());

            // Verify the specific rejection metric
            assert_eq!(
                metrics
                    .mempool_rejected_total
                    .with_label_values(&[expected_reason])
                    .get(),
                1
            );
        }
    }
}

#[tokio::test]
async fn test_mempool_eviction_metrics() {
    let metrics = Arc::new(Metrics::new().expect("Failed to create metrics"));

    // Create mempool with very small capacity
    let config = dytallix_fast_node::mempool::MempoolConfig {
        max_txs: 2,
        ..Default::default()
    };
    let mut mempool = Mempool::with_config(config);
    let (state, _directory) = create_mock_state();

    // Add transactions up to capacity
    let tx1 = create_test_transaction("hash1", "sender1", "receiver", 1000, 100, 0, 21000, 1000);
    let tx2 = create_test_transaction("hash2", "sender2", "receiver", 1000, 100, 0, 21000, 2000);

    assert!(mempool.add_transaction(&state, tx1).is_ok());
    assert!(mempool.add_transaction(&state, tx2).is_ok());

    // Add one more to trigger eviction
    let tx3 = create_test_transaction(
        "hash3", "sender1", "receiver", 1000, 100, 1, 21000, 3000, // Highest priority
    );

    assert!(mempool.add_transaction(&state, tx3).is_ok());

    // Record eviction metric
    metrics.record_mempool_eviction("capacity");

    // Verify eviction metric
    assert_eq!(
        metrics
            .mempool_evicted_total
            .with_label_values(&["capacity"])
            .get(),
        1
    );

    // Update size metrics
    metrics.update_mempool_size(mempool.len());
    assert_eq!(metrics.mempool_size.get(), 1); // One ready entry; its peer is deferred.
    assert_eq!(mempool.total_count(), 2); // Retained capacity is unchanged.
}

#[tokio::test]
async fn test_gossip_duplicate_metrics() {
    let metrics = Arc::new(Metrics::new().expect("Failed to create metrics"));
    let gossip = TransactionGossip::new();

    let tx_hash = "test_hash";

    // First time should allow gossip
    assert!(gossip.should_gossip(tx_hash, Some("peer1")));

    // Second time should suppress (duplicate)
    assert!(!gossip.should_gossip(tx_hash, Some("peer2")));

    // Record duplicate suppression
    metrics.record_gossip_duplicate();

    // Verify duplicate metric
    assert_eq!(metrics.mempool_gossip_duplicates_total.get(), 1);

    // Test multiple duplicates
    for i in 3..=5 {
        assert!(!gossip.should_gossip(tx_hash, Some(&format!("peer{i}"))));
        metrics.record_gossip_duplicate();
    }

    assert_eq!(metrics.mempool_gossip_duplicates_total.get(), 4);
}

#[tokio::test]
async fn test_mempool_watermark_metrics() {
    let metrics = Arc::new(Metrics::new().expect("Failed to create metrics"));
    let mut mempool = Mempool::new();
    let (state, _directory) = create_mock_state();

    // Track metrics as mempool fills up
    let mut max_size = 0;
    let mut max_bytes = 0;
    let mut min_gas_price = u64::MAX;

    for i in 0..10 {
        let tx = create_test_transaction(
            &format!("hash{i}"),
            "sender1",
            "receiver",
            1000,
            100,
            i,
            21000,
            1000 + i * 100, // Increasing gas prices
        );

        if mempool.add_transaction(&state, tx).is_ok() {
            max_size = max_size.max(mempool.len());
            max_bytes = max_bytes.max(mempool.total_bytes());
            min_gas_price = min_gas_price.min(mempool.current_min_gas_price());

            // Update metrics
            metrics.update_mempool_size(mempool.len());
            metrics.update_mempool_bytes(mempool.total_bytes());
            metrics.update_mempool_min_gas_price(mempool.current_min_gas_price());
        }
    }

    // Verify watermark metrics were updated
    assert_eq!(metrics.mempool_size.get(), max_size as i64);
    assert_eq!(metrics.mempool_bytes.get(), max_bytes as i64);

    // Min gas price should be the lowest in the pool
    assert_eq!(
        metrics.mempool_current_min_gas_price.get(),
        min_gas_price as i64
    );

    println!("Max size: {max_size}, Max bytes: {max_bytes}, Min gas price: {min_gas_price}");
}

#[tokio::test]
async fn test_comprehensive_metrics_flow() {
    let metrics = Arc::new(Metrics::new().expect("Failed to create metrics"));
    let mut mempool = Mempool::new();
    let gossip = TransactionGossip::new();
    let (state, _directory) = create_mock_state();

    // Simulate a complete flow with metrics
    let mut successful_admissions = 0;
    let mut rejections_by_reason = std::collections::HashMap::new();
    let mut duplicates_suppressed = 0;

    // Phase 1: Add valid transactions
    for i in 0..5 {
        let tx = create_test_transaction(
            &format!("hash{i}"),
            "sender1",
            "receiver",
            1000,
            100,
            i,
            21000,
            1000 + i * 200,
        );

        // Check gossip
        if gossip.should_gossip(&tx.hash, None) {
            gossip.mark_broadcast(&tx.hash);
        } else {
            duplicates_suppressed += 1;
            metrics.record_gossip_duplicate();
        }

        // Add to mempool
        match mempool.add_transaction(&state, tx) {
            Ok(()) => {
                successful_admissions += 1;
                metrics.record_mempool_admission();
            }
            Err(reason) => {
                let label = reason.to_metric_label();
                *rejections_by_reason.entry(label).or_insert(0) += 1;
                metrics.record_mempool_rejection(label);
            }
        }

        // Update size metrics
        metrics.update_mempool_size(mempool.len());
        metrics.update_mempool_bytes(mempool.total_bytes());
        metrics.update_mempool_min_gas_price(mempool.current_min_gas_price());
    }

    // Phase 2: Add some invalid transactions
    let invalid_txs = vec![
        create_test_transaction(
            "hash_stale",
            "sender1",
            "receiver",
            1000,
            100,
            0,
            21000,
            1000,
        ), // Nonce below the pending sequence
        create_test_transaction("hash1", "sender2", "receiver", 1000, 100, 0, 21000, 1000), // Duplicate hash
        create_test_transaction(
            "hash_poor",
            "sender2",
            "receiver",
            2_000_000_000_000u128,
            100,
            0,
            21000,
            1000,
        ), // Insufficient funds with high amount
    ];

    for tx in invalid_txs {
        match mempool.add_transaction(&state, tx) {
            Ok(()) => {
                successful_admissions += 1;
                metrics.record_mempool_admission();
            }
            Err(reason) => {
                let label = reason.to_metric_label();
                *rejections_by_reason.entry(label).or_insert(0) += 1;
                metrics.record_mempool_rejection(label);
            }
        }
    }

    // Require the intended admission and rejection paths.
    assert_eq!(successful_admissions, 5);
    assert_eq!(rejections_by_reason.len(), 3);
    for reason in ["nonce_gap", "duplicate", "insufficient_funds"] {
        assert_eq!(rejections_by_reason.get(reason), Some(&1));
    }

    // Verify final metrics
    assert_eq!(metrics.mempool_admitted_total.get(), successful_admissions);
    assert_eq!(metrics.mempool_size.get(), mempool.len() as i64);
    assert_eq!(
        metrics.mempool_gossip_duplicates_total.get(),
        duplicates_suppressed
    );

    // Verify rejection metrics
    for (reason, count) in &rejections_by_reason {
        assert_eq!(
            metrics
                .mempool_rejected_total
                .with_label_values(&[*reason])
                .get(),
            *count
        );
    }

    println!(
        "✅ Comprehensive metrics test completed: {} admissions, {} rejections by {:?}, {} gossip duplicates",
        successful_admissions,
        rejections_by_reason.values().sum::<u64>(),
        rejections_by_reason,
        duplicates_suppressed
    );
}

#[tokio::test]
async fn test_metrics_prometheus_format() {
    use prometheus::TextEncoder;

    let metrics = Arc::new(Metrics::new().expect("Failed to create metrics"));

    // Record some metrics
    metrics.record_mempool_admission();
    metrics.record_mempool_rejection("duplicate");
    metrics.update_mempool_size(5);
    metrics.update_mempool_bytes(1024);
    metrics.update_mempool_min_gas_price(1500);
    metrics.record_gossip_duplicate();

    // Export metrics in Prometheus format
    let encoder = TextEncoder::new();
    let metric_families = metrics.registry.gather();
    let output = encoder.encode_to_string(&metric_families).unwrap();

    // Verify that our mempool metrics are present
    assert!(output.contains("dytallix_mempool_admitted_total"));
    assert!(output.contains("dytallix_mempool_rejected_total"));
    assert!(output.contains("dytallix_mempool_size"));
    assert!(output.contains("dytallix_mempool_bytes"));
    assert!(output.contains("dytallix_mempool_current_min_gas_price"));
    assert!(output.contains("dytallix_mempool_gossip_duplicates_total"));

    // Verify specific values are present
    assert!(output.contains("dytallix_mempool_admitted_total 1"));
    assert!(output.contains("dytallix_mempool_size 5"));
    assert!(output.contains("dytallix_mempool_bytes 1024"));
    assert!(output.contains("dytallix_mempool_current_min_gas_price 1500"));
    assert!(output.contains("dytallix_mempool_gossip_duplicates_total 1"));
    assert!(output.contains(r#"dytallix_mempool_rejected_total{reason="duplicate"} 1"#));

    println!("✅ Prometheus metrics format verified");
    println!(
        "Sample output:\n{}",
        output.lines().take(20).collect::<Vec<_>>().join("\n")
    );
}
