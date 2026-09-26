/*
Unit tests for gas accounting in the deterministic execution engine.

Tests upfront fee deduction, out-of-gas handling, receipt creation,
and other core gas accounting functionality.
*/

use dytallix_fast_node::execution::{execute_transaction, ExecutionContext};
use dytallix_fast_node::gas::GasSchedule;
use dytallix_fast_node::state::State;
use dytallix_fast_node::storage::receipts::{TxStatus, RECEIPT_FORMAT_VERSION};
use dytallix_fast_node::storage::state::Storage;
use dytallix_fast_node::storage::tx::Transaction;
use std::sync::Arc;

fn create_test_state() -> (State, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(Storage::open(dir.path().join("node.db")).unwrap());
    (State::new(storage), dir)
}

#[test]
fn test_upfront_fee_deduction_success() {
    let (mut state, _directory) = create_test_state();
    let gas_schedule = GasSchedule::default();

    // Setup sufficient balance
    state.set_balance("alice", "udgt", 100_000_000);
    state.set_balance("alice", "udrt", 100_000_000);

    let tx = Transaction::new("test_hash", "alice", "bob", 1_000, 10_000, 0, None)
        .with_gas(25_000, 1_000)
        .with_signature("sig");

    let result = execute_transaction(&tx, &mut state, 100, 0, &gas_schedule, None).unwrap();

    assert!(result.success);
    assert_eq!(result.receipt.status, TxStatus::Success);
    assert_eq!(result.receipt.gas_limit, 25_000);
    assert_eq!(result.receipt.gas_price, 1_000);
    assert_eq!(result.receipt.receipt_version, RECEIPT_FORMAT_VERSION);

    // Verify upfront fee was charged
    let upfront_fee = 25_000u128 * 1_000u128; // gas_limit * gas_price
    assert_eq!(
        state.balance_of("alice", "udrt"),
        100_000_000u128 - upfront_fee
    );
    assert_eq!(
        state.balance_of("alice", "udgt"),
        100_000_000u128 - 1_000u128
    );
}

#[test]
fn test_upfront_fee_deduction_failure_insufficient() {
    let (mut state, _directory) = create_test_state();
    let gas_schedule = GasSchedule::default();

    // Setup insufficient balance
    state.set_balance("alice", "udgt", 1_000);
    state.set_balance("alice", "udrt", 1_000); // Not enough for the current gas fee

    let tx = Transaction::new("test_hash", "alice", "bob", 500, 10_000, 0, None)
        .with_gas(25_000, 1_000)
        .with_signature("sig");

    let result = execute_transaction(&tx, &mut state, 100, 0, &gas_schedule, None).unwrap();

    assert!(!result.success);
    assert_eq!(result.receipt.status, TxStatus::Failed);
    assert!(result
        .receipt
        .error
        .as_ref()
        .unwrap()
        .contains("InsufficientFunds"));
    assert_eq!(result.gas_used, 0);

    // Balance should be unchanged since no fee was deducted
    assert_eq!(state.balance_of("alice", "udgt"), 1_000);
}

#[test]
fn test_oom_full_revert() {
    let (mut state, _directory) = create_test_state();
    let gas_schedule = GasSchedule::default();

    // Setup balance
    state.set_balance("alice", "udgt", 100_000);
    state.set_balance("alice", "udrt", 100_000);
    state.set_balance("bob", "udgt", 50_000);

    // Create transaction with very low gas limit to trigger OOM
    let tx = Transaction::new("test_hash", "alice", "bob", 1_000, 10_000, 0, None)
        .with_gas(100, 1_000)
        .with_signature("sig");

    let initial_alice_balance = state.balance_of("alice", "udgt");
    let initial_bob_balance = state.balance_of("bob", "udgt");

    let result = execute_transaction(&tx, &mut state, 100, 0, &gas_schedule, None).unwrap();

    // Transaction should fail due to out of gas
    assert!(!result.success);
    assert_eq!(result.receipt.status, TxStatus::Failed);
    assert!(result.receipt.error.as_ref().unwrap().contains("OutOfGas"));
    assert!(result.gas_used > 0); // The failed receipt records the runtime's minimum gas charge.

    // State should be reverted except for the gas fee
    let upfront_fee = 100u128 * 1_000u128; // gas_limit * gas_price
    assert_eq!(state.balance_of("alice", "udgt"), initial_alice_balance);
    assert_eq!(state.balance_of("alice", "udrt"), 100_000 - upfront_fee);
    assert_eq!(state.balance_of("bob", "udgt"), initial_bob_balance); // No change to bob
}

#[test]
fn test_receipt_fields() {
    let (mut state, _directory) = create_test_state();
    let gas_schedule = GasSchedule::default();

    // Ensure sufficient balance to cover upfront fee (gas_limit * gas_price) and amount
    state.set_balance("alice", "udgt", 20_000_000);
    state.set_balance("alice", "udrt", 20_000_000);

    let tx = Transaction::new("test_hash_123", "alice", "bob", 2_000, 5_000, 0, None)
        .with_gas(30_000, 500)
        .with_signature("signature_data");

    let result = execute_transaction(&tx, &mut state, 150, 3, &gas_schedule, None).unwrap();

    assert!(result.success);
    // Check all receipt fields are properly set
    let receipt = &result.receipt;
    assert_eq!(receipt.receipt_version, RECEIPT_FORMAT_VERSION);
    assert_eq!(receipt.tx_hash, "test_hash_123");
    assert_eq!(receipt.block_height, Some(150));
    assert_eq!(receipt.index, Some(3));
    assert_eq!(receipt.from, "alice");
    assert_eq!(receipt.to, "bob");
    assert_eq!(receipt.amount, 2_000);
    assert_eq!(receipt.fee, 5_000);
    assert_eq!(receipt.nonce, 0);
    assert_eq!(receipt.gas_limit, 30_000);
    assert_eq!(receipt.gas_price, 500);
    assert_eq!(receipt.gas_refund, 0); // Always 0 as per spec
    assert!(receipt.gas_used > 0);
    assert_eq!(receipt.success, result.success);

    if result.success {
        assert!(receipt.error.is_none());
        assert_eq!(receipt.status, TxStatus::Success);
    } else {
        assert!(receipt.error.is_some());
        assert_eq!(receipt.status, TxStatus::Failed);
    }
}

#[test]
fn test_legacy_transaction_compatibility() {
    let (mut state, _directory) = create_test_state();
    let gas_schedule = GasSchedule::default();

    state.set_balance("alice", "udgt", 100_000);
    state.set_balance("alice", "udrt", 100_000);

    // Legacy transaction with gas_limit=0, gas_price=0
    let tx = Transaction::new("legacy_hash", "alice", "bob", 1_000, 25_000, 0, None)
        .with_signature("sig");

    let result = execute_transaction(&tx, &mut state, 100, 0, &gas_schedule, None).unwrap();

    assert!(result.success);
    assert_eq!(result.receipt.gas_limit, 25_000); // fee used as gas_limit
    assert_eq!(result.receipt.gas_price, 1); // gas_price=1
}

#[test]
fn test_execution_context_fee_calculation() {
    let ctx = ExecutionContext::new(25_000, 1_500);
    let fee = ctx.calculate_upfront_fee().unwrap();
    assert_eq!(fee, 37_500_000); // 25000 * 1500
}

#[test]
fn test_execution_context_fee_overflow() {
    let ctx = ExecutionContext::new(u64::MAX, u64::MAX);
    let fee = ctx
        .calculate_upfront_fee()
        .expect("should not overflow for u64::MAX product");
    let expected = (u128::from(u64::MAX)) * (u128::from(u64::MAX));
    assert_eq!(fee, expected);
}

#[test]
fn test_invalid_nonce_handling() {
    let (mut state, _directory) = create_test_state();
    let gas_schedule = GasSchedule::default();

    state.set_balance("alice", "udgt", 100_000);
    state.set_balance("alice", "udrt", 100_000);
    // Nonce is 0 initially, but transaction has nonce 5

    let tx = Transaction::new("test_hash", "alice", "bob", 1_000, 10_000, 5, None)
        .with_gas(25_000, 1_000)
        .with_signature("sig");

    let result = execute_transaction(&tx, &mut state, 100, 0, &gas_schedule, None).unwrap();

    assert!(!result.success);
    assert_eq!(result.receipt.status, TxStatus::Failed);
    assert!(result
        .receipt
        .error
        .as_ref()
        .unwrap()
        .contains("InvalidNonce"));
    assert_eq!(result.gas_used, 0);
}
