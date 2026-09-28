//! Legacy signed-transaction cost rules (the legacy path until E04 gap 14,
//! step L-b removes it). The legacy mempool and execution tests went with
//! those modules in L-a.
use dytallix_fast_node::{
    storage::tx::{Transaction, TxMessage},
    transaction_cost::{effective_gas, normalize_send, required_balances},
};
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
