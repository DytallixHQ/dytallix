mod support;
use dytallix_fast_node::mempool::{Mempool, MempoolConfig, RejectionReason};
use dytallix_fast_node::state::State;
use dytallix_fast_node::storage::{state::Storage, tx::Transaction};
use std::sync::Arc;

const COST: u128 = 21_000_000;
fn setup() -> (State, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut state = State::new(Arc::new(
        Storage::open(dir.path().join("state.db")).unwrap(),
    ));
    state.set_balance("sender", "udgt", COST * 3);
    state.set_balance("sender", "udrt", COST * 3);
    (state, dir)
}
fn tx(hash: &str, nonce: u64) -> Transaction {
    support::sign_transaction(
        Transaction::new(hash, "sender", "receiver", 1_000, 100, nonce, None)
            .with_gas(21_000, 1_000),
    )
}

#[test]
fn closing_gap_promotes_all_and_preserves_reservations() {
    let (state, _dir) = setup();
    let mut pool = Mempool::new();
    let mut total = 0;
    for (hash, nonce) in [("two", 2), ("one", 1), ("zero", 0)] {
        let transaction = tx(hash, nonce);
        total += dytallix_fast_node::mempool::PendingTx::new(transaction.clone()).serialized_size;
        pool.add_transaction(&state, transaction).unwrap();
    }
    assert_eq!(pool.len(), 3);
    assert_eq!(pool.total_bytes(), total);
    assert_eq!(
        pool.take_snapshot(10)
            .iter()
            .map(|t| t.nonce)
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert!(matches!(
        pool.add_transaction(&state, tx("three", 3)),
        Err(RejectionReason::InsufficientFunds { .. })
    ));
    pool.drop_hashes(&["zero".into(), "one".into(), "two".into()]);
    assert_eq!(pool.total_bytes(), 0);
    assert!(pool.is_empty());
    // Released reservations permit the same account balance to fund admission.
    pool.add_transaction(&state, tx("again", 0)).unwrap();
}

#[test]
fn deferred_nonce_conflict_preserves_original() {
    let (state, _dir) = setup();
    let mut pool = Mempool::new();
    pool.add_transaction(&state, tx("first", 1)).unwrap();
    let bytes = pool.total_bytes();
    assert!(matches!(
        pool.add_transaction(&state, tx("conflict", 1)),
        Err(RejectionReason::PolicyViolation(_))
    ));
    assert_eq!(pool.total_bytes(), bytes);
    assert!(pool.contains("first"));
    assert!(!pool.contains("conflict"));
    pool.add_transaction(&state, tx("zero", 0)).unwrap();
    assert_eq!(
        pool.take_snapshot(10)
            .iter()
            .map(|t| t.hash.as_str())
            .collect::<Vec<_>>(),
        vec!["zero", "first"]
    );
}

#[test]
fn removing_deferred_transaction_releases_index_and_reserve() {
    let (state, _dir) = setup();
    let mut pool = Mempool::new();
    pool.add_transaction(&state, tx("old", 1)).unwrap();
    pool.drop_hashes(&["old".into()]);
    assert!(!pool.contains("old"));
    assert_eq!(pool.total_bytes(), 0);
    pool.add_transaction(&state, tx("replacement", 1)).unwrap();
    pool.add_transaction(&state, tx("zero", 0)).unwrap();
    assert_eq!(pool.len(), 2);
}

#[test]
fn promotion_at_capacity_keeps_admitted_transactions() {
    let (state, _dir) = setup();
    let mut pool = Mempool::with_config(MempoolConfig {
        max_txs: 3,
        ..Default::default()
    });
    for (hash, nonce) in [("two", 2), ("one", 1), ("zero", 0)] {
        pool.add_transaction_trusted(&state, tx(hash, nonce))
            .unwrap();
    }
    assert_eq!(pool.len(), 3);
    assert!(pool.is_full());
    assert!(pool.contains("two") && pool.contains("one") && pool.contains("zero"));
}
