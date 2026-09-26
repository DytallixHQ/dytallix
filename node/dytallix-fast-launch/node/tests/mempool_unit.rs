mod support;

use dytallix_fast_node::mempool::{Mempool, MempoolConfig, PendingTx, RejectionReason};
use dytallix_fast_node::state::State;
use dytallix_fast_node::storage::state::Storage;
use dytallix_fast_node::storage::tx::Transaction;
use std::sync::Arc;
use tempfile::TempDir;

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
    support::sign_transaction(
        Transaction::new(hash, from, to, amount, fee, nonce, None).with_gas(gas_limit, gas_price),
    )
}

fn create_mock_state() -> (State, TempDir) {
    let tmp = TempDir::new().unwrap();
    let storage = Arc::new(Storage::open(tmp.path().join("node.db")).unwrap());
    let mut state = State::new(storage);
    state.set_balance("sender1", "udgt", 1_000_000_000);
    state.set_balance("sender1", "udrt", 1_000_000_000);
    state.set_balance("sender2", "udgt", 1_000_000_000);
    state.set_balance("sender2", "udrt", 1_000_000_000);
    (state, tmp)
}

#[test]
fn test_reject_invalid_signature() {
    let mut mempool = Mempool::new();
    let (state, _directory) = create_mock_state();

    let mut tx = create_test_transaction("hash1", "sender1", "receiver", 1000, 100, 0, 21000, 1000);
    tx.signature = None; // Invalid signature

    let result = mempool.add_transaction(&state, tx);
    assert!(matches!(result, Err(RejectionReason::InvalidSignature)));
}

#[test]
fn test_reject_stale_nonce() {
    let mut mempool = Mempool::new();
    let (state, _directory) = create_mock_state();

    let first = create_test_transaction("first", "sender1", "receiver", 1000, 100, 0, 21000, 1000);
    mempool.add_transaction(&state, first).unwrap();
    // Future nonces are deferred. A nonce below the pending sequence is rejected.
    let tx = create_test_transaction("hash1", "sender1", "receiver", 1000, 100, 0, 21000, 1000);

    let result = mempool.add_transaction(&state, tx);
    assert!(matches!(
        result,
        Err(RejectionReason::NonceGap {
            expected: 1,
            got: 0
        })
    ));
}

#[test]
fn test_reject_insufficient_funds() {
    let mut mempool = Mempool::new();
    let (state, _directory) = create_mock_state();

    let tx = create_test_transaction(
        "hash1",
        "sender1",
        "receiver",
        2_000_000_000,
        100,
        0,
        21000,
        1000, // Amount > balance
    );

    let result = mempool.add_transaction(&state, tx);
    assert!(matches!(
        result,
        Err(RejectionReason::InsufficientFunds {
            denom,
            required: 2_000_000_000,
            available: 1_000_000_000
        }) if denom == "udgt"
    ));
}

#[test]
fn test_reject_underpriced_gas() {
    let mut mempool = Mempool::new();
    let (state, _directory) = create_mock_state();

    let tx = create_test_transaction(
        "hash1", "sender1", "receiver", 1000, 100, 0, 21000, 500, // Gas price below minimum
    );

    let result = mempool.add_transaction(&state, tx);
    assert!(matches!(
        result,
        Err(RejectionReason::UnderpricedGas {
            min: 1000,
            got: 500
        })
    ));
}

#[test]
fn test_reject_oversized_tx() {
    let config = MempoolConfig {
        max_tx_bytes: 100, // Very small limit
        ..Default::default()
    };
    let mut mempool = Mempool::with_config(config);
    let (state, _directory) = create_mock_state();

    let tx = create_test_transaction("hash1", "sender1", "receiver", 1000, 100, 0, 21000, 1000);

    let result = mempool.add_transaction(&state, tx);
    assert!(matches!(
        result,
        Err(RejectionReason::OversizedTx { max: 100, got: _ })
    ));
}

#[test]
fn test_dedup_same_hash() {
    let mut mempool = Mempool::new();
    let (state, _directory) = create_mock_state();

    let tx1 = create_test_transaction("hash1", "sender1", "receiver", 1000, 100, 0, 21000, 1000);
    let tx2 = create_test_transaction(
        "hash1", "sender2", "receiver", 2000, 200, 0, 21000, 1000, // Same hash
    );

    // First transaction should succeed
    assert!(mempool.add_transaction(&state, tx1).is_ok());

    // Second transaction with same hash should be rejected as duplicate
    let result = mempool.add_transaction(&state, tx2);
    assert!(matches!(result, Err(RejectionReason::Duplicate(_))));
}

#[test]
fn test_ordering_by_price_then_nonce() {
    let mut mempool = Mempool::new();
    let (state, _directory) = create_mock_state();

    // Add transactions with different gas prices and nonces
    let tx1 = create_test_transaction(
        "hash1", "sender1", "receiver", 1000, 100, 0, 21000, 2000, // High gas price
    );
    let tx2 = create_test_transaction(
        "hash2", "sender2", "receiver", 1000, 100, 0, 21000, 1000, // Low gas price
    );
    let tx3 = create_test_transaction(
        "hash3", "sender1", "receiver", 1000, 100, 1, 21000,
        2000, // High gas price, higher nonce
    );

    assert!(mempool.add_transaction(&state, tx2).is_ok());
    assert!(mempool.add_transaction(&state, tx1).is_ok());
    assert!(mempool.add_transaction(&state, tx3).is_ok());

    // Get snapshot and verify ordering
    let snapshot = mempool.take_snapshot(3);
    assert_eq!(snapshot.len(), 3);

    // Should be ordered by gas price desc, then nonce asc
    assert_eq!(snapshot[0].hash, "hash1"); // gas_price 2000, nonce 0
    assert_eq!(snapshot[1].hash, "hash3"); // gas_price 2000, nonce 1
    assert_eq!(snapshot[2].hash, "hash2"); // gas_price 1000, nonce 0
}

#[test]
fn test_eviction_policy_evicts_lowest_priority() {
    let config = MempoolConfig {
        max_txs: 2, // Very small limit
        ..Default::default()
    };
    let mut mempool = Mempool::with_config(config);
    let (state, _directory) = create_mock_state();

    // Add transactions with different priorities
    let tx1 = create_test_transaction(
        "hash1", "sender1", "receiver", 1000, 100, 0, 21000, 1000, // Low priority
    );
    let tx2 = create_test_transaction(
        "hash2", "sender2", "receiver", 1000, 100, 0, 21000, 2000, // High priority
    );
    let tx3 = create_test_transaction(
        "hash3", "sender1", "receiver", 1000, 100, 1, 21000, 3000, // Highest priority
    );

    // Add first two transactions
    assert!(mempool.add_transaction(&state, tx1).is_ok());
    assert!(mempool.add_transaction(&state, tx2).is_ok());
    assert_eq!(mempool.len(), 2);

    // Adding third transaction should evict the lowest priority (tx1)
    assert!(mempool.add_transaction(&state, tx3).is_ok());
    assert_eq!(mempool.total_count(), 2);
    assert_eq!(mempool.len(), 1); // The evicted predecessor leaves its successor deferred.

    // Check that tx1 was evicted (lowest priority)
    assert!(!mempool.contains("hash1"));
    assert!(mempool.contains("hash2"));
    assert!(mempool.contains("hash3"));
}

#[test]
fn test_priority_key_ordering() {
    let tx1 = PendingTx::new(create_test_transaction(
        "hash1", "sender", "receiver", 1000, 100, 0, 21000, 2000,
    ));
    let tx2 = PendingTx::new(create_test_transaction(
        "hash2", "sender", "receiver", 1000, 100, 0, 21000, 1000,
    ));
    let tx3 = PendingTx::new(create_test_transaction(
        "hash3", "sender", "receiver", 1000, 100, 1, 21000, 2000,
    ));

    let key1 = tx1.priority_key();
    let key2 = tx2.priority_key();
    let key3 = tx3.priority_key();

    // Higher gas price should come first (lower in ordering)
    assert!(key1 < key2);

    // Same gas price, lower nonce should come first
    assert!(key1 < key3);

    // Lower gas price should come last
    assert!(key3 < key2);
}

#[test]
fn test_mempool_statistics() {
    let mut mempool = Mempool::new();
    let (state, _directory) = create_mock_state();

    assert_eq!(mempool.len(), 0);
    assert_eq!(mempool.total_bytes(), 0);
    assert!(!mempool.is_full());

    let tx = create_test_transaction("hash1", "sender1", "receiver", 1000, 100, 0, 21000, 1000);

    assert!(mempool.add_transaction(&state, tx).is_ok());

    assert_eq!(mempool.len(), 1);
    assert!(mempool.total_bytes() > 0);
    assert_eq!(mempool.current_min_gas_price(), 1000);
}

#[test]
fn test_drop_hashes() {
    let mut mempool = Mempool::new();
    let (state, _directory) = create_mock_state();

    let tx1 = create_test_transaction("hash1", "sender1", "receiver", 1000, 100, 0, 21000, 1000);
    let tx2 = create_test_transaction("hash2", "sender2", "receiver", 1000, 100, 0, 21000, 2000);

    assert!(mempool.add_transaction(&state, tx1).is_ok());
    assert!(mempool.add_transaction(&state, tx2).is_ok());
    assert_eq!(mempool.len(), 2);

    // Remove one transaction
    mempool.drop_hashes(&["hash1".to_string()]);

    assert_eq!(mempool.len(), 1);
    assert!(!mempool.contains("hash1"));
    assert!(mempool.contains("hash2"));
}

#[test]
fn test_future_nonce_is_held_out_of_snapshot() {
    let (state, _directory) = create_mock_state();
    let mut pool = Mempool::new();
    let tx = create_test_transaction("future", "sender1", "receiver", 1000, 100, 5, 21000, 1000);
    pool.add_transaction(&state, tx).unwrap();
    assert!(pool.contains("future"));
    assert!(pool.take_snapshot(10).is_empty());
    assert!(pool.total_bytes() > 0);
}

#[test]
fn test_reject_changed_signed_transaction() {
    let (state, _directory) = create_mock_state();
    let mut pool = Mempool::new();
    let mut tx =
        create_test_transaction("changed", "sender1", "receiver", 1000, 100, 0, 21000, 1000);
    tx.amount += 1;
    assert!(matches!(
        pool.add_transaction(&state, tx),
        Err(RejectionReason::InvalidSignature)
    ));
    assert!(pool.is_empty());
    assert_eq!(pool.total_bytes(), 0);
}
