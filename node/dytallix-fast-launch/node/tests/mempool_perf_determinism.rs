use std::collections::HashSet;
use std::time::{Duration, Instant};

use dytallix_fast_node::mempool::{Mempool, MempoolConfig, PendingTx};
use dytallix_fast_node::state::State;
use dytallix_fast_node::storage::state::Storage;
use dytallix_fast_node::storage::tx::Transaction;
use std::sync::Arc;
use tempfile::TempDir;

// Add: environment-controlled performance factor to make timing tests less flaky on slow machines/CI
fn perf_factor() -> u64 {
    if let Ok(v) = std::env::var("DYTALLIX_PERF_TEST_FACTOR") {
        if let Ok(f) = v.parse::<u64>() {
            return f.max(1);
        }
    }
    // Heuristic: relax timings on CI where machines may be slower
    if std::env::var("CI").is_ok() {
        3
    } else {
        1
    }
}

// Added: crypto and base64 imports for valid PQC signatures
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
    // Generate a fresh keypair per tx (sufficient for tests). In production we would
    // use the sender's actual key. This ensures signature verification passes.
    let (sk, pk) = ActivePQC::keypair();
    let tx = Transaction::base(hash, from, to, amount, fee, nonce)
        .with_gas(gas_limit, gas_price)
        .with_pqc(B64.encode(&pk), "dytallix-testnet", "");

    // Sign canonical JSON of the tx
    let canonical_tx = tx.canonical_fields();
    let tx_bytes = canonical_json(&canonical_tx).expect("serialize canonical tx");
    let tx_hash = sha3_256(&tx_bytes);
    let signature = ActivePQC::sign(&sk, &tx_hash);

    tx.with_signature(B64.encode(&signature))
}

// New helper: reuse a provided keypair to avoid heavy per-tx key generation for perf tests
#[allow(clippy::too_many_arguments)]
fn create_test_transaction_with_key(
    hash: &str,
    from: &str,
    to: &str,
    amount: u128,
    fee: u128,
    nonce: u64,
    gas_limit: u64,
    gas_price: u64,
    sk: &[u8],
    pk: &[u8],
) -> Transaction {
    let tx = Transaction::base(hash, from, to, amount, fee, nonce)
        .with_gas(gas_limit, gas_price)
        .with_pqc(B64.encode(pk), "dytallix-testnet", "");

    let canonical_tx = tx.canonical_fields();
    let tx_bytes = canonical_json(&canonical_tx).expect("serialize canonical tx");
    let tx_hash = sha3_256(&tx_bytes);
    let signature = ActivePQC::sign(sk, &tx_hash);
    tx.with_signature(B64.encode(&signature))
}

fn create_mock_state_with_many_accounts(num_accounts: usize) -> (State, TempDir) {
    let tmp = TempDir::new().unwrap();
    let storage = Arc::new(Storage::open(tmp.path().join("node.db")).unwrap());
    let mut state = State::new(storage);
    for i in 0..num_accounts {
        let mut acc = state.get_account(&format!("sender{i}"));
        // Increase balance to comfortably cover gas costs for tests
        acc.set_balance("udgt", 1_000_000_000);
        acc.set_balance("udrt", 1_000_000_000);
        state.accounts.insert(format!("sender{i}"), acc);
    }
    (state, tmp)
}

#[tokio::test]
async fn test_deterministic_ordering_across_instances() {
    // Create two identical mempool instances
    let config = MempoolConfig::default();
    let mut mempool1 = Mempool::with_config(config.clone());
    let mut mempool2 = Mempool::with_config(config);
    let (state, _tmp) = create_mock_state_with_many_accounts(100);

    // Reuse a single keypair for all txs to reduce overhead
    let (sk, pk) = ActivePQC::keypair();

    // Create a set of transactions with various gas prices and nonces
    let mut transactions = vec![];
    for i in 0..50 {
        let tx = create_test_transaction_with_key(
            &format!("hash{i}"),
            &format!("sender{}", i % 10), // Cycle through 10 senders
            "receiver",
            1000,
            100,
            i / 10, // Nonce based on transaction index
            21000,
            1000 + (i % 5) * 500, // Gas prices: 1000, 1500, 2000, 2500, 3000
            &sk,
            &pk,
        );
        transactions.push(tx);
    }

    // Add transactions to both mempools in different orders
    let tx_set1 = transactions.clone();
    let mut tx_set2 = transactions.clone();

    // Reverse the order for the second mempool
    tx_set2.reverse();

    // Add to first mempool
    for tx in tx_set1 {
        mempool1
            .add_transaction(&state, tx)
            .expect("admit forward order");
    }

    // Add to second mempool
    for tx in tx_set2 {
        mempool2
            .add_transaction(&state, tx)
            .expect("admit reverse order");
    }

    // Take snapshots from both mempools
    let snapshot1 = mempool1.take_snapshot(100);
    let snapshot2 = mempool2.take_snapshot(100);

    // Verify both snapshots have the same length
    assert_eq!(snapshot1.len(), 50);
    assert_eq!(snapshot1.len(), snapshot2.len());

    // Verify deterministic ordering - both snapshots should be identical
    for (tx1, tx2) in snapshot1.iter().zip(snapshot2.iter()) {
        assert_eq!(
            tx1.hash, tx2.hash,
            "Transaction order differs between instances"
        );
        assert_eq!(tx1.gas_price, tx2.gas_price);
        assert_eq!(tx1.nonce, tx2.nonce);
    }

    println!(
        "✅ Deterministic ordering verified across {} transactions",
        snapshot1.len()
    );
}

#[tokio::test]
async fn test_performance_threshold_admission() {
    let total_start = Instant::now();
    let mut mempool = Mempool::new();
    let (state, _tmp) = create_mock_state_with_many_accounts(1000);
    let (sk, pk) = ActivePQC::keypair();
    let transactions: Vec<_> = (0..1000)
        .map(|i| {
            create_test_transaction_with_key(
                &format!("hash{i}"),
                &format!("sender{i}"),
                "receiver",
                1000,
                100,
                0,
                21000,
                1000 + (i % 100),
                &sk,
                &pk,
            )
        })
        .collect();
    let preparation = total_start.elapsed();

    // Measure public admission, including signature verification. Signing belongs to the client.
    let start = Instant::now();
    for tx in transactions {
        mempool
            .add_transaction(&state, tx)
            .expect("admit every signed transaction");
    }
    let admission_duration = start.elapsed();
    assert_eq!(mempool.len(), 1000);
    assert_eq!(mempool.total_count(), 1000);
    let snapshot_start = Instant::now();
    let snapshot = mempool.take_snapshot(500);
    let snapshot_duration = snapshot_start.elapsed();
    assert_eq!(snapshot.len(), 500);
    assert_eq!(
        snapshot
            .iter()
            .map(|tx| &tx.hash)
            .collect::<HashSet<_>>()
            .len(),
        500
    );

    let factor = perf_factor();
    let admission_limit = Duration::from_millis(1000 * factor);
    let snapshot_limit = Duration::from_millis(10 * factor);
    println!(
        "Admission: preparation {:?}, signed admission {:?}, snapshot {:?}, total {:?}, factor {}",
        preparation,
        admission_duration,
        snapshot_duration,
        total_start.elapsed(),
        factor
    );
    assert!(
        admission_duration < admission_limit,
        "Admission took too long: {:?} (limit {:?}, factor {})",
        admission_duration,
        admission_limit,
        factor
    );
    assert!(
        snapshot_duration < snapshot_limit,
        "Snapshot took too long: {:?} (limit {:?}, factor {})",
        snapshot_duration,
        snapshot_limit,
        factor
    );
}

#[tokio::test]
async fn test_deterministic_eviction_order() {
    // Create mempool with small capacity
    let config = MempoolConfig {
        max_txs: 10,
        max_bytes: 1000000, // High byte limit to focus on count limit
        ..Default::default()
    };
    let mut mempool = Mempool::with_config(config);
    let (state, _tmp) = create_mock_state_with_many_accounts(20);

    // Reuse one keypair for perf
    let (sk, pk) = ActivePQC::keypair();

    // Add 15 transactions (will exceed capacity)
    let mut added_transactions = vec![];
    for i in 0..15 {
        let tx = create_test_transaction_with_key(
            &format!("hash{i}"),
            &format!("sender{i}"),
            "receiver",
            1000,
            100,
            0,
            21000,
            1000 + (i % 5) as u64 * 100, // Gas prices vary cyclically
            &sk,
            &pk,
        );

        if mempool.add_transaction(&state, tx.clone()).is_ok() {
            added_transactions.push(tx);
        }
    }

    // Should have exactly 10 transactions (capacity limit)
    assert_eq!(mempool.len(), 10);

    // Verify that the lowest priority transactions were evicted
    let snapshot = mempool.take_snapshot(10);
    assert_eq!(snapshot.len(), 10);

    // All remaining transactions should have gas_price >= some threshold
    // since lowest priority ones should have been evicted
    let min_gas_price = snapshot.iter().map(|tx| tx.gas_price).min().unwrap();
    println!("Minimum gas price in pool after evictions: {min_gas_price}");

    // Verify deterministic eviction by checking that specific low-priority
    // transactions were evicted
    let remaining_hashes: HashSet<String> = snapshot.iter().map(|tx| tx.hash.clone()).collect();

    // Count how many high-priority vs low-priority transactions remain
    let mut high_priority_count = 0;
    let mut low_priority_count = 0;

    for tx in &added_transactions {
        if remaining_hashes.contains(&tx.hash) {
            if tx.gas_price >= 1200 {
                high_priority_count += 1;
            } else {
                low_priority_count += 1;
            }
        }
    }

    // Should favor high-priority transactions
    assert!(
        high_priority_count >= low_priority_count,
        "Eviction should favor high-priority transactions"
    );

    println!(
        "✅ Deterministic eviction verified: {high_priority_count} high-priority, {low_priority_count} low-priority retained"
    );
}

#[tokio::test]
async fn test_priority_key_determinism() {
    // Test that priority keys are deterministic and consistent
    let tx1 = create_test_transaction("hash_a", "sender1", "receiver", 1000, 100, 5, 21000, 2000);
    let tx2 = create_test_transaction("hash_b", "sender2", "receiver", 1000, 100, 3, 21000, 2000);
    let tx3 = create_test_transaction("hash_c", "sender3", "receiver", 1000, 100, 5, 21000, 1500);

    let pending1 = PendingTx::new(tx1);
    let pending2 = PendingTx::new(tx2);
    let pending3 = PendingTx::new(tx3);

    let key1 = pending1.priority_key();
    let key2 = pending2.priority_key();
    let key3 = pending3.priority_key();

    // Same gas price, different nonces: lower nonce should have higher priority
    assert!(key2 < key1, "Lower nonce should have higher priority");

    // Different gas prices: higher gas price should have higher priority
    assert!(key1 < key3, "Higher gas price should have higher priority");
    assert!(key2 < key3, "Higher gas price should have higher priority");

    // Test hash-based tiebreaking for identical gas_price and nonce
    let tx4 = create_test_transaction("hash_d", "sender4", "receiver", 1000, 100, 5, 21000, 2000);
    let pending4 = PendingTx::new(tx4);
    let key4 = pending4.priority_key();

    // key1 and key4 have same gas_price and nonce, should be ordered by hash
    // Ensure total order exists between keys (deterministic tie-break)
    assert!((key1 < key4) ^ (key4 < key1), "Tiebreaking must be strict");

    println!("✅ Priority key determinism verified");
}

#[tokio::test]
async fn test_concurrent_access_simulation() {
    // Sequential cancellation and admission workload; this does not simulate block settlement.
    let preparation_start = Instant::now();
    let mut mempool = Mempool::new();
    let (state, _tmp) = create_mock_state_with_many_accounts(150);
    let (sk, pk) = ActivePQC::keypair();
    let bulk: Vec<_> = (0..100)
        .map(|i| {
            create_test_transaction_with_key(
                &format!("hash{i}"),
                &format!("sender{i}"),
                "receiver",
                1000,
                100,
                0,
                21000,
                1000 + (i % 20) * 50,
                &sk,
                &pk,
            )
        })
        .collect();
    let rounds: Vec<Vec<_>> = (0..10)
        .map(|round| {
            (0..5)
                .map(|i| {
                    let sender = 100 + round * 5 + i;
                    create_test_transaction_with_key(
                        &format!("new_hash_{round}_{i}"),
                        &format!("sender{sender}"),
                        "receiver",
                        1000,
                        100,
                        0,
                        21000,
                        1000 + (round * 5 + i) * 25,
                        &sk,
                        &pk,
                    )
                })
                .collect()
        })
        .collect();
    let preparation = preparation_start.elapsed();

    let bulk_start = Instant::now();
    for tx in bulk {
        mempool
            .add_transaction(&state, tx)
            .expect("bulk signed admission");
    }
    let bulk_duration = bulk_start.elapsed();
    assert_eq!(mempool.len(), 100);
    assert_eq!(mempool.total_count(), 100);

    let interleaved_start = Instant::now();
    for additions in rounds {
        let snapshot = mempool.take_snapshot(20);
        assert_eq!(snapshot.len(), 20);
        let to_remove: Vec<_> = snapshot.iter().take(5).map(|tx| tx.hash.clone()).collect();
        mempool.drop_hashes(&to_remove);
        assert_eq!(mempool.len(), 95);
        assert_eq!(mempool.total_count(), 95);
        assert!(to_remove.iter().all(|hash| !mempool.contains(hash)));
        for tx in additions {
            mempool
                .add_transaction(&state, tx)
                .expect("interleaved signed admission");
        }
        assert_eq!(mempool.len(), 100);
        assert_eq!(mempool.total_count(), 100);
    }
    let interleaved_duration = interleaved_start.elapsed();
    let final_snapshot = mempool.take_snapshot(1000);
    assert_eq!(final_snapshot.len(), 100);
    assert_eq!(
        final_snapshot
            .iter()
            .map(|tx| &tx.hash)
            .collect::<HashSet<_>>()
            .len(),
        100
    );
    // Each sender has nonce zero, so fee, nonce and hash define the full order here.
    for pair in final_snapshot.windows(2) {
        let key = |tx: &Transaction| (u64::MAX - tx.gas_price, tx.nonce, tx.hash.clone());
        assert!(key(&pair[0]) < key(&pair[1]), "strict priority ordering");
    }
    let factor = perf_factor();
    let bulk_limit = Duration::from_millis(500 * factor);
    let interleaved_limit = Duration::from_millis(100 * factor);
    println!(
        "Interleaved workload: preparation {:?}, bulk {:?}, interleaved {:?}, factor {}",
        preparation, bulk_duration, interleaved_duration, factor
    );
    assert!(
        bulk_duration < bulk_limit,
        "Bulk addition took too long: {:?} (limit {:?}, factor {})",
        bulk_duration,
        bulk_limit,
        factor
    );
    assert!(
        interleaved_duration < interleaved_limit,
        "Interleaved operations took too long: {:?} (limit {:?}, factor {})",
        interleaved_duration,
        interleaved_limit,
        factor
    );
}
