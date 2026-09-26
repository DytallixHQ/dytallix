// removed unused imports

use dytallix_fast_node::mempool::{Mempool, MempoolConfig};
use dytallix_fast_node::p2p::TransactionGossip;
use dytallix_fast_node::state::State;
use dytallix_fast_node::storage::state::Storage;
use dytallix_fast_node::storage::tx::Transaction;
use std::sync::Arc;
use tempfile::TempDir;
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
        .with_pqc(B64.encode(&pk), "dytallix-testnet", "integration test");

    // Sign canonical fields and attach a valid signature
    let canonical_tx = tx.canonical_fields();
    let tx_bytes = canonical_json(&canonical_tx).expect("canonical serialize");
    let tx_hash = sha3_256(&tx_bytes);
    let signature = ActivePQC::sign(&sk, &tx_hash);
    tx.signature = Some(B64.encode(&signature));

    tx
}

fn create_mock_state() -> State {
    let tmp = TempDir::new().unwrap();
    let storage = Arc::new(Storage::open(tmp.path().join("node.db")).unwrap());
    let mut state = State::new(storage);
    // Increased balances to cover gas costs in tests
    state.set_balance("sender1", "udgt", 1_000_000_000);
    state.set_balance("sender1", "udrt", 1_000_000_000);
    state.set_balance("sender2", "udgt", 1_000_000_000);
    state.set_balance("sender2", "udrt", 1_000_000_000);
    state
}

#[allow(dead_code)]
fn create_mock_state_with_many_accounts(num_accounts: usize) -> State {
    let tmp = TempDir::new().unwrap();
    let storage = Arc::new(Storage::open(tmp.path().join("node.db")).unwrap());
    let mut state = State::new(storage);
    for i in 0..num_accounts {
        // Increased balances to cover gas costs in stress tests
        state.set_balance(&format!("sender{i}"), "udgt", 1_000_000_000);
        state.set_balance(&format!("sender{i}"), "udrt", 1_000_000_000);
    }
    state
}

#[tokio::test]
async fn test_admit_then_include() {
    let mut mempool = Mempool::new();
    let state = create_mock_state();

    // Create and admit a transaction
    let tx = create_test_transaction("hash1", "sender1", "receiver", 1000, 100, 0, 21000, 1000);

    assert!(mempool.add_transaction(&state, tx.clone()).is_ok());
    assert_eq!(mempool.len(), 1);
    assert!(mempool.contains(&tx.hash));

    // Simulate block inclusion
    let snapshot = mempool.take_snapshot(10);
    assert_eq!(snapshot.len(), 1);
    assert_eq!(snapshot[0].hash, tx.hash);

    // Remove after inclusion
    let h = tx.hash.clone();
    mempool.drop_hashes(&[h.clone()]);
    assert_eq!(mempool.len(), 0);
    assert!(!mempool.contains(&h));
}

#[tokio::test]
async fn test_gossip_dedup() {
    let gossip = TransactionGossip::new();
    let tx_hash = "test_transaction_hash";

    // First time receiving from peer1 should allow gossip
    assert!(gossip.should_gossip(tx_hash, Some("peer1")));

    // Second time receiving from peer2 should suppress gossip (duplicate)
    assert!(!gossip.should_gossip(tx_hash, Some("peer2")));

    // Verify statistics
    let stats = gossip.get_stats();
    assert_eq!(stats.seen_cache_size, 1);
}

#[tokio::test]
async fn test_pool_limits() {
    // Test transaction count limit
    let config = MempoolConfig {
        max_txs: 3,
        max_bytes: 1000000, // High byte limit
        ..Default::default()
    };
    let mut mempool = Mempool::with_config(config);
    let state = create_mock_state();

    // Add transactions up to limit
    for i in 0..3 {
        let tx = create_test_transaction(
            &format!("hash{i}"),
            "sender1",
            "receiver",
            1000,
            100,
            i,
            21000,
            1000 + i, // Different gas prices for deterministic ordering
        );
        assert!(mempool.add_transaction(&state, tx).is_ok());
    }

    assert_eq!(mempool.len(), 3);

    // Adding one more should evict the lowest priority
    let tx = create_test_transaction(
        "hash_new", "sender2", "receiver", 1000, 100, 0, 21000, 2000, // High priority
    );
    assert!(mempool.add_transaction(&state, tx).is_ok());

    // Three retained entries; two successors are deferred after predecessor eviction.
    assert_eq!(mempool.total_count(), 3);
    assert_eq!(mempool.len(), 1);

    // The lowest priority transaction (hash0 with gas_price 1000) should be evicted
    assert!(!mempool.contains("hash0"));
    assert!(mempool.contains("hash_new"));
}

#[tokio::test]
async fn test_byte_limits() {
    // Test byte limit
    let config = MempoolConfig {
        max_txs: 1000,  // High transaction limit
        max_bytes: 500, // Very low byte limit
        ..Default::default()
    };
    let mut mempool = Mempool::with_config(config);
    let state = create_mock_state();

    // Add a few transactions until byte limit is reached
    let mut added_count = 0;
    for i in 0..10 {
        let tx = create_test_transaction(
            &format!("hash{i}"),
            "sender1",
            "receiver",
            1000,
            100,
            i,
            21000,
            1000 + i,
        );

        if mempool.add_transaction(&state, tx).is_ok() {
            added_count += 1;
        }

        // Once we hit byte limit, additions might trigger evictions
        if mempool.total_bytes() >= 500 {
            break;
        }
    }

    // Should have added some transactions
    assert!(added_count > 0);

    // Total bytes should be within or close to limit
    assert!(mempool.total_bytes() <= 500);
}

#[tokio::test]
async fn test_integrated_gossip_flow() {
    let mut mempool = Mempool::new();
    let gossip = TransactionGossip::new();
    let state = create_mock_state();

    let tx = create_test_transaction("hash1", "sender1", "receiver", 1000, 100, 0, 21000, 1000);

    // Check if we should gossip (new transaction)
    assert!(gossip.should_gossip(&tx.hash, None));

    // Add to mempool
    assert!(mempool.add_transaction(&state, tx.clone()).is_ok());

    // Queue for gossip to peers
    let peers = vec!["peer1".to_string(), "peer2".to_string()];
    gossip.queue_for_gossip(&tx.hash, &peers);

    // Check that it's marked as broadcast
    assert!(gossip.was_broadcast(&tx.hash));

    // Get pending gossip for peer1
    let pending = gossip.get_pending_for_peer("peer1", 5);
    assert!(!pending.is_empty());
    assert_eq!(pending[0], tx.hash);

    // Simulate receiving the same transaction from another peer
    assert!(!gossip.should_gossip(&tx.hash, Some("peer3")));

    // Verify statistics
    let stats = gossip.get_stats();
    assert_eq!(stats.seen_cache_size, 1);
    assert_eq!(stats.active_peers, 2);
    assert_eq!(stats.total_broadcast, 1);
}

#[tokio::test]
async fn test_concurrent_operations() {
    let mut mempool = Mempool::new();
    let state = create_mock_state();

    // Simulate sequential additions (concurrency omitted in unit test)

    for i in 0..10 {
        let tx = create_test_transaction(
            &format!("hash{i}"),
            "sender1",
            "receiver",
            1000,
            100,
            i,
            21000,
            1000 + i,
        );

        // Add transaction
        assert!(mempool.add_transaction(&state, tx).is_ok());
    }

    assert_eq!(mempool.len(), 10);

    // Take snapshot
    let snapshot = mempool.take_snapshot(5);
    assert_eq!(snapshot.len(), 5);

    // One sender must execute in nonce order, even when successor fees are higher.
    assert_eq!(
        snapshot.iter().map(|tx| tx.nonce).collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4]
    );

    // Remove some transactions
    let to_remove: Vec<String> = snapshot.iter().take(3).map(|tx| tx.hash.clone()).collect();
    mempool.drop_hashes(&to_remove);

    assert_eq!(mempool.total_count(), 7);
    assert_eq!(mempool.len(), 0); // Removal alone does not consume committed nonces.
}

#[tokio::test]
async fn test_mempool_state_consistency() {
    let mut mempool = Mempool::new();
    let state = create_mock_state();

    // Add transactions
    for i in 0..5 {
        let tx = create_test_transaction(
            &format!("hash{i}"),
            "sender1",
            "receiver",
            1000,
            100,
            i,
            21000,
            1000 + i,
        );
        assert!(mempool.add_transaction(&state, tx).is_ok());
    }

    // Verify internal consistency
    assert_eq!(mempool.len(), 5);

    // Take snapshot and verify all transactions are present
    let snapshot = mempool.take_snapshot(10);
    assert_eq!(snapshot.len(), 5);

    // Verify each transaction in snapshot exists in mempool
    for tx in &snapshot {
        assert!(mempool.contains(&tx.hash));
    }

    // Remove all transactions
    let all_hashes: Vec<String> = snapshot.iter().map(|tx| tx.hash.clone()).collect();
    mempool.drop_hashes(&all_hashes);

    // Mempool should be empty
    assert_eq!(mempool.len(), 0);
    assert_eq!(mempool.total_bytes(), 0);

    // Verify no transactions remain
    for hash in &all_hashes {
        assert!(!mempool.contains(hash));
    }
}
