mod support;

use dytallix_fast_node::mempool::{basic_validate, Mempool, RejectionReason};
use dytallix_fast_node::state::State;
use dytallix_fast_node::storage::{state::Storage, tx::Transaction};
use std::sync::Arc;

fn assert_rejection_and_control(sender: &str) {
    let directory = tempfile::tempdir().unwrap();
    let storage = Arc::new(Storage::open(directory.path().join("state.db")).unwrap());
    let mut state = State::new(storage);
    state.set_balance(sender, "udgt", 500);
    state.set_balance(sender, "udrt", 21_000_000);
    let tx = support::sign_transaction(
        Transaction::new("logging-test", sender, "receiver", 1_000, 100, 0, None)
            .with_gas(21_000, 1_000),
    );
    let expected = RejectionReason::InsufficientFunds {
        denom: "udgt".to_string(),
        required: 1_000,
        available: 500,
    };

    // Both admission methods share the same funds check.
    for trusted in [false, true] {
        let mut pool = Mempool::new();
        let result = if trusted {
            pool.add_transaction_trusted(&state, tx.clone())
        } else {
            pool.add_transaction(&state, tx.clone())
        };
        assert_eq!(result.unwrap_err().to_string(), expected.to_string());
        assert_eq!(pool.len(), 0);
        assert_eq!(pool.total_bytes(), 0);
        assert!(!pool.contains(&tx.hash));
        assert_eq!(state.balance_of(sender, "udgt"), 500);
        assert_eq!(state.nonce_of(sender), 0);

        // A rejection must not prevent a later funded admission.
        state.set_balance(sender, "udgt", 1_000);
        pool.add_transaction(&state, tx.clone()).unwrap();
        assert_eq!(pool.take_snapshot(1)[0].hash, tx.hash);
        state.set_balance(sender, "udgt", 500);
    }
    assert_eq!(
        basic_validate(&state, &tx).unwrap_err(),
        expected.to_string()
    );
}

#[test]
fn insufficient_funds_short_sender_returns_error() {
    assert_rejection_and_control("sender1");
}

#[test]
fn insufficient_funds_utf8_sender_returns_error() {
    // Byte 12 lies inside the two-byte character.
    assert_rejection_and_control("abcdefghijkérest");
}

#[test]
fn insufficient_funds_empty_sender_returns_error() {
    assert_rejection_and_control("");
}

#[test]
fn insufficient_funds_ascii_boundary_returns_error() {
    assert_rejection_and_control("abcdefghijkl");
    assert_rejection_and_control("abcdefghijklmnopqrstuvwxyz");
}
