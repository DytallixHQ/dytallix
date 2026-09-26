use dytallix_fast_node::{
    execution::execute_transaction,
    gas::GasSchedule,
    mempool::{Mempool, MempoolConfig, RejectionReason},
    state::State,
    storage::{
        state::Storage,
        tx::{Transaction, TxMessage},
    },
    transaction_cost::{effective_gas, normalize_send, required_balances},
};
use std::sync::Arc;
fn fixture() -> (State, Mempool, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut state = State::new(Arc::new(Storage::open(dir.path().join("node.db")).unwrap()));
    for sender in ["alice", "bob"] {
        state.set_balance(sender, "udgt", 1_000_000_000);
        state.set_balance(sender, "udrt", 1_000_000_000);
    }
    (
        state,
        Mempool::with_config(MempoolConfig {
            min_gas_price: 1,
            ..Default::default()
        }),
        dir,
    )
}
fn tx(hash: &str, nonce: u64, price: u64) -> Transaction {
    Transaction::new(hash, "alice", "receiver", 100, 999, nonce, None).with_gas(21_000, price)
}
fn send(to: &str, amount: u128) -> TxMessage {
    TxMessage::Send {
        from: "alice".into(),
        to: to.into(),
        denom: "udgt".into(),
        amount,
    }
}
#[test]
fn data_admission_and_execution_charge_the_same_drt_fee_once() {
    let (mut state, mut pool, _dir) = fixture();
    state.set_balance("alice", "udgt", 0);
    state.set_balance("alice", "udrt", 21_000);
    let input = Transaction {
        messages: Some(vec![TxMessage::Data {
            from: "alice".into(),
            data: "ordinary note".into(),
        }]),
        ..tx("data", 0, 1)
    };
    pool.add_transaction_trusted(&state, input.clone()).unwrap();
    let outcome =
        execute_transaction(&input, &mut state, 1, 0, &GasSchedule::default(), None).unwrap();
    assert!(outcome.success);
    assert_eq!(state.get_balance("alice", "udrt"), 0);
    assert_eq!(state.get_balance("alice", "udgt"), 0);
    assert_eq!(state.nonce_of("alice"), 1);
    let withheld: u128 = bincode::deserialize(
        &state
            .storage
            .db
            .get("execution:v1:withheld_udrt")
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(withheld, 21_000);
}
#[test]
fn dgt_cannot_substitute_for_the_fee_token() {
    let (mut state, mut pool, _dir) = fixture();
    state.set_balance("alice", "udrt", 0);
    assert!(
        matches!(pool.add_transaction_trusted(&state,tx("fee",0,1)),Err(RejectionReason::InsufficientFunds {denom,required:21_000,available:0}) if denom=="udrt")
    );
    assert!(pool.is_empty());
}
#[test]
fn transfer_reservation_is_separate_from_fee_reservation() {
    let (mut state, mut pool, _dir) = fixture();
    state.set_balance("alice", "udgt", 100);
    state.set_balance("alice", "udrt", 21_000);
    pool.add_transaction_trusted(&state, tx("exact", 0, 1))
        .unwrap();
    assert!(pool
        .add_transaction_trusted(&state, tx("second", 1, 1))
        .is_err());
    pool.drop_hashes(&["exact".into()]);
    pool.add_transaction_trusted(&state, tx("replacement", 0, 1))
        .unwrap();
}
#[test]
fn reservation_overflow_rejects_without_mutating_the_queue() {
    let (mut state, mut pool, _dir) = fixture();
    state.set_balance("alice", "udrt", u128::MAX);
    let input = Transaction {
        denom: "udrt".into(),
        amount: u128::MAX - 21_000,
        ..tx("full", 0, 1)
    };
    pool.add_transaction_trusted(&state, input).unwrap();
    let before = pool.total_bytes();
    let second = Transaction {
        denom: "udrt".into(),
        amount: 0,
        ..tx("overflow", 1, 1)
    };
    assert!(matches!(
        pool.add_transaction_trusted(&state, second),
        Err(RejectionReason::InternalError(_))
    ));
    assert_eq!(pool.total_bytes(), before);
    assert!(pool.contains("full"));
    assert!(!pool.contains("overflow"));
}
#[test]
fn required_balances_handle_self_transfer_peaks_and_sender_identity() {
    let first = Transaction {
        messages: Some(vec![send("alice", 100), send("receiver", 100)]),
        ..tx("self", 0, 1)
    };
    assert_eq!(required_balances(&first).unwrap()["udgt"], 100);
    let later = Transaction {
        messages: Some(vec![send("receiver", 100), send("alice", 100)]),
        ..tx("self-later", 0, 1)
    };
    assert_eq!(required_balances(&later).unwrap()["udgt"], 200);
    let mismatch = Transaction {
        from: "bob".into(),
        ..first
    };
    assert!(required_balances(&mismatch).is_err());
}
#[test]
fn canonical_aliases_scale_only_whole_token_inputs() {
    for denom in ["DGT", "dgt", "DgT"] {
        assert_eq!(
            normalize_send(denom, 3).unwrap(),
            ("udgt".into(), 3_000_000)
        );
    }
    for denom in ["UDRT", "udrt", "uDrT"] {
        assert_eq!(normalize_send(denom, 3).unwrap(), ("udrt".into(), 3));
    }
    assert!(normalize_send("DRT", u128::MAX).is_err());
    assert!(normalize_send("unknown", 1).is_err());
    assert!(required_balances(&Transaction {
        denom: "UDGT".into(),
        ..tx("raw", 0, 1)
    })
    .is_err());
}
#[test]
fn legacy_fee_selection_matches_execution_domain() {
    let mut input = tx("legacy", 0, 1);
    input.gas_price = 0;
    input.fee = 123;
    assert_eq!(effective_gas(&input).unwrap(), (123, 1));
    assert_eq!(required_balances(&input).unwrap()["udrt"], 123);
    input.fee = u128::MAX;
    assert!(effective_gas(&input).is_err());
    input.gas_price = u64::MAX;
    input.gas_limit = u64::MAX;
    assert!(effective_gas(&input).is_ok());
}
#[test]
fn higher_fee_successors_cannot_pass_their_sender_nonce() {
    let (mut state, mut pool, _dir) = fixture();
    state.set_balance("alice", "udrt", u128::MAX);
    pool.add_transaction_trusted(&state, tx("next", 1, u64::MAX))
        .unwrap();
    assert!(pool.take_snapshot(10).is_empty());
    pool.add_transaction_trusted(&state, tx("first", 0, 1))
        .unwrap();
    assert_eq!(
        pool.take_snapshot(10)
            .iter()
            .map(|t| t.nonce)
            .collect::<Vec<_>>(),
        [0, 1]
    );
}
#[test]
fn dropping_rejected_nonce_keeps_successor_deferred() {
    let (state, mut pool, _dir) = fixture();
    pool.add_transaction_trusted(&state, tx("first", 0, 1))
        .unwrap();
    pool.add_transaction_trusted(&state, tx("next", 1, 2))
        .unwrap();
    pool.reconcile(&state, &["first".into()]).unwrap();
    assert!(pool.contains("next"));
    assert!(pool.take_snapshot(10).is_empty());
    pool.add_transaction_trusted(&state, tx("replacement", 0, 1))
        .unwrap();
    assert_eq!(
        pool.take_snapshot(10)
            .iter()
            .map(|t| t.hash.as_str())
            .collect::<Vec<_>>(),
        ["replacement", "next"]
    );
}
#[test]
fn charged_failure_advances_ready_nonce_from_committed_state() {
    let (mut state, mut pool, _dir) = fixture();
    pool.add_transaction_trusted(&state, tx("first", 0, 1))
        .unwrap();
    pool.add_transaction_trusted(&state, tx("next", 1, 2))
        .unwrap();
    // A normal state change can make a previously admitted transfer fail.
    state.set_balance("alice", "udgt", 0);
    let out = execute_transaction(
        &tx("first", 0, 1),
        &mut state,
        1,
        0,
        &GasSchedule::default(),
        None,
    )
    .unwrap();
    assert!(!out.success);
    assert_eq!(state.nonce_of("alice"), 1);
    pool.reconcile(&state, &["first".into()]).unwrap();
    assert_eq!(pool.take_snapshot(10)[0].nonce, 1);
}
#[test]
fn eviction_demotes_successors_and_zero_capacity_does_not_admit() {
    let (state, _, _dir) = fixture();
    let mut pool = Mempool::with_config(MempoolConfig {
        max_txs: 2,
        min_gas_price: 1,
        ..Default::default()
    });
    pool.add_transaction_trusted(&state, tx("first", 0, 1))
        .unwrap();
    pool.add_transaction_trusted(&state, tx("next", 1, 10))
        .unwrap();
    let other = Transaction {
        from: "bob".into(),
        ..tx("other", 0, 20)
    };
    pool.add_transaction_trusted(&state, other).unwrap();
    assert!(!pool.contains("first"));
    assert!(pool.contains("next"));
    assert_eq!(
        pool.take_snapshot(10)
            .iter()
            .map(|t| t.hash.as_str())
            .collect::<Vec<_>>(),
        ["other"]
    );
    let mut zero = Mempool::with_config(MempoolConfig {
        max_txs: 0,
        min_gas_price: 1,
        ..Default::default()
    });
    assert!(zero
        .add_transaction_trusted(&state, tx("zero", 0, 1))
        .is_err());
    assert_eq!(zero.total_bytes(), 0);
}
#[test]
fn exhausted_nonce_and_multimessage_sum_fail_without_admission() {
    let (mut state, mut pool, _dir) = fixture();
    state.set_balance("alice", "udgt", u128::MAX);
    let input = Transaction {
        messages: Some(vec![send("receiver", u128::MAX), send("receiver", 1)]),
        ..tx("overflow", 0, 1)
    };
    assert!(pool.add_transaction_trusted(&state, input).is_err());
    assert!(pool
        .add_transaction_trusted(&state, tx("exhausted", u64::MAX, 1))
        .is_err());
    assert!(pool.is_empty());
}
