/*
Integration tests for gas execution to verify deterministic behavior
across block production and replay scenarios.
*/

use dytallix_fast_node::execution::execute_transaction;
use dytallix_fast_node::gas::GasSchedule;
use dytallix_fast_node::state::State;
use dytallix_fast_node::storage::state::Storage;
use dytallix_fast_node::storage::tx::Transaction;
use std::sync::Arc;

fn create_test_state() -> (State, tempfile::TempDir) {
    // Use a unique temporary directory for each state to ensure isolation
    let dir = tempfile::tempdir().expect("tempdir");
    let storage = Arc::new(Storage::open(dir.path().join("state.db")).unwrap());
    (State::new(storage), dir)
}

#[test]
fn test_block_replay_determinism() {
    let gas_schedule = GasSchedule::default();

    // Create two identical states
    let (mut state1, _state1_directory) = create_test_state();
    let (mut state2, _state2_directory) = create_test_state();

    // Setup identical initial conditions
    state1.set_balance("alice", "udgt", 100_000);
    state1.set_balance("alice", "udrt", 100_000_000);
    state1.set_balance("bob", "udgt", 50_000);
    state1.set_balance("bob", "udrt", 100_000_000);
    state2.set_balance("alice", "udgt", 100_000);
    state2.set_balance("alice", "udrt", 100_000_000);
    state2.set_balance("bob", "udgt", 50_000);
    state2.set_balance("bob", "udrt", 100_000_000);

    // Create identical transactions
    let txs = vec![
        Transaction::new("tx1", "alice", "bob", 1_000, 5_000, 0, None)
            .with_gas(25_000, 1_000)
            .with_signature("sig1"),
        Transaction::new("tx2", "bob", "alice", 500, 3_000, 0, None)
            .with_gas(25_000, 800)
            .with_signature("sig2"),
    ];

    // Execute on both states
    let mut results1 = Vec::new();
    let mut results2 = Vec::new();

    for (i, tx) in txs.iter().enumerate() {
        let result1 =
            execute_transaction(tx, &mut state1, 100, i as u32, &gas_schedule, None).unwrap();
        let result2 =
            execute_transaction(tx, &mut state2, 100, i as u32, &gas_schedule, None).unwrap();

        results1.push(result1);
        results2.push(result2);
    }

    // Verify identical results
    assert_eq!(results1.len(), results2.len());

    for (r1, r2) in results1.iter().zip(results2.iter()) {
        assert!(r1.success, "{:?}", r1.receipt.error);
        assert!(r2.success, "{:?}", r2.receipt.error);
        assert_eq!(r1.success, r2.success);
        assert_eq!(r1.gas_used, r2.gas_used);
        assert_eq!(r1.receipt.status, r2.receipt.status);
        assert_eq!(r1.receipt.gas_used, r2.receipt.gas_used);
        assert_eq!(r1.receipt.gas_limit, r2.receipt.gas_limit);
        assert_eq!(r1.receipt.gas_price, r2.receipt.gas_price);
        assert_eq!(r1.receipt.error, r2.receipt.error);
    }

    // Verify identical final state
    assert_eq!(
        state1.balance_of("alice", "udgt"),
        state2.balance_of("alice", "udgt")
    );
    assert_eq!(
        state1.balance_of("bob", "udgt"),
        state2.balance_of("bob", "udgt")
    );
    assert_eq!(state1.nonce_of("alice"), state2.nonce_of("alice"));
    assert_eq!(state1.nonce_of("bob"), state2.nonce_of("bob"));
}

#[test]
fn test_multiple_txs_mixed_success_failure() {
    let (mut state, _state_directory) = create_test_state();
    let gas_schedule = GasSchedule::default();

    // Setup initial balances
    state.set_balance("alice", "udgt", 100_000);
    state.set_balance("alice", "udrt", 100_000_000);
    state.set_balance("bob", "udgt", 50_000);
    state.set_balance("bob", "udrt", 100_000_000);
    state.set_balance("charlie", "udgt", 1_000); // Low balance

    let transactions = vec![
        // Success case (low gas price to fit balance)
        Transaction::new("tx_success", "alice", "bob", 1_000, 5_000, 0, None)
            .with_gas(25_000, 1)
            .with_signature("sig"),
        // Insufficient funds case (still insufficient for upfront fee)
        Transaction::new(
            "tx_insufficient",
            "charlie",
            "alice",
            10_000,
            5_000,
            0,
            None,
        )
        .with_gas(25_000, 1)
        .with_signature("sig"),
        // Out of gas case (very low gas limit)
        Transaction::new("tx_oom", "alice", "bob", 1_000, 5_000, 1, None)
            .with_gas(50, 1)
            .with_signature("sig"),
        // Another success case (bob pays fee)
        Transaction::new("tx_success2", "bob", "alice", 500, 3_000, 0, None)
            .with_gas(30_000, 1)
            .with_signature("sig"),
    ];

    let initial_alice_balance = state.balance_of("alice", "udgt");
    let initial_bob_balance = state.balance_of("bob", "udgt");
    let initial_charlie_balance = state.balance_of("charlie", "udgt");

    let mut results = Vec::new();

    for (i, tx) in transactions.iter().enumerate() {
        let result =
            execute_transaction(tx, &mut state, 100, i as u32, &gas_schedule, None).unwrap();
        results.push(result);
    }

    // Verify expected outcomes
    assert!(results[0].success); // alice -> bob success
    assert!(!results[1].success); // charlie insufficient funds
    assert!(!results[2].success); // alice OOM
    assert!(results[3].success); // bob -> alice success

    // Verify gas usage and receipts
    assert!(results[0].gas_used > 0);
    assert_eq!(results[1].gas_used, 0); // No gas used on insufficient funds
    assert!(results[2].gas_used > 0); // Some gas used before OOM
    assert!(results[3].gas_used > 0);

    // Verify state changes
    let final_alice_balance = state.balance_of("alice", "udgt");
    let final_bob_balance = state.balance_of("bob", "udgt");
    let final_charlie_balance = state.balance_of("charlie", "udgt");

    // Charlie should be unchanged (transaction failed early)
    assert_eq!(final_charlie_balance, initial_charlie_balance);

    // Transfers change udgt. Upfront gas fees change udrt.
    let expected_alice = initial_alice_balance - 1_000 + 500;
    assert_eq!(final_alice_balance, expected_alice);

    let expected_bob = initial_bob_balance + 1_000 - 500;
    assert_eq!(final_bob_balance, expected_bob);
    assert_eq!(state.balance_of("alice", "udrt"), 100_000_000 - 25_000 - 50);
    assert_eq!(state.balance_of("bob", "udrt"), 100_000_000 - 30_000);
    assert_eq!(state.balance_of("charlie", "udrt"), 0);
    assert_eq!(state.nonce_of("alice"), 2);
    assert_eq!(state.nonce_of("bob"), 1);
    assert_eq!(state.nonce_of("charlie"), 0);
    assert!(results[1]
        .receipt
        .error
        .as_ref()
        .unwrap()
        .contains("InsufficientFunds"));
    assert_eq!(results[2].receipt.error.as_deref(), Some("OutOfGas"));
}

#[test]
fn test_deterministic_gas_consumption() {
    let gas_schedule = GasSchedule::default();

    // Test that identical transactions consume identical gas
    let test_cases = vec![
        ("alice", "bob", 1_000u128, 25_000u64, 1_000u64),
        ("bob", "charlie", 2_000u128, 30_000u64, 800u64),
        ("charlie", "alice", 500u128, 25_000u64, 1_200u64),
    ];

    for (from, to, amount, gas_limit, gas_price) in test_cases {
        let (mut state1, _state1_directory) = create_test_state();
        let (mut state2, _state2_directory) = create_test_state();

        // Setup identical states
        state1.set_balance(from, "udgt", 100_000);
        state1.set_balance(from, "udrt", 100_000_000);
        state2.set_balance(from, "udgt", 100_000);
        state2.set_balance(from, "udrt", 100_000_000);

        let tx = Transaction::new(
            "test_hash",
            from.to_string(),
            to.to_string(),
            amount,
            5_000,
            0,
            None,
        )
        .with_gas(gas_limit, gas_price)
        .with_signature("sig");

        // Execute multiple times
        let result1 = execute_transaction(&tx, &mut state1, 100, 0, &gas_schedule, None).unwrap();
        let result2 = execute_transaction(&tx, &mut state2, 100, 0, &gas_schedule, None).unwrap();

        // Results must be identical
        assert!(result1.success, "{:?}", result1.receipt.error);
        assert!(result2.success, "{:?}", result2.receipt.error);
        assert_eq!(state1.balance_of(to, "udgt"), amount);
        assert_eq!(state2.balance_of(to, "udgt"), amount);
        assert_eq!(
            state1.balance_of(from, "udrt"),
            100_000_000 - u128::from(gas_limit) * u128::from(gas_price)
        );
        assert_eq!(
            state1.balance_of(from, "udrt"),
            state2.balance_of(from, "udrt")
        );
        assert_eq!(result1.success, result2.success);
        assert_eq!(result1.gas_used, result2.gas_used);
        assert_eq!(result1.receipt.gas_used, result2.receipt.gas_used);
        assert_eq!(result1.receipt.status, result2.receipt.status);
    }
}

#[test]
fn test_execution_order_determinism() {
    let gas_schedule = GasSchedule::default();

    // Create transactions that could affect each other
    let transactions = vec![
        Transaction::new("tx1", "alice", "bob", 1_000, 5_000, 0, None)
            .with_gas(25_000, 1_000)
            .with_signature("sig"),
        Transaction::new("tx2", "bob", "charlie", 500, 3_000, 0, None)
            .with_gas(25_000, 800)
            .with_signature("sig"),
        Transaction::new("tx3", "alice", "charlie", 2_000, 4_000, 1, None)
            .with_gas(30_000, 1_200)
            .with_signature("sig"),
    ];

    // Execute in order on state1
    let (mut state1, _state1_directory) = create_test_state();
    state1.set_balance("alice", "udgt", 100_000);
    state1.set_balance("alice", "udrt", 100_000_000);
    state1.set_balance("bob", "udgt", 50_000);
    state1.set_balance("bob", "udrt", 100_000_000);
    state1.set_balance("charlie", "udgt", 25_000);

    let mut results1 = Vec::new();
    for (i, tx) in transactions.iter().enumerate() {
        let result =
            execute_transaction(tx, &mut state1, 100, i as u32, &gas_schedule, None).unwrap();
        results1.push(result);
    }

    // Execute in same order on state2 (should be identical)
    let (mut state2, _state2_directory) = create_test_state();
    state2.set_balance("alice", "udgt", 100_000);
    state2.set_balance("alice", "udrt", 100_000_000);
    state2.set_balance("bob", "udgt", 50_000);
    state2.set_balance("bob", "udrt", 100_000_000);
    state2.set_balance("charlie", "udgt", 25_000);

    let mut results2 = Vec::new();
    for (i, tx) in transactions.iter().enumerate() {
        let result =
            execute_transaction(tx, &mut state2, 100, i as u32, &gas_schedule, None).unwrap();
        results2.push(result);
    }

    // Results must be identical
    for (r1, r2) in results1.iter().zip(results2.iter()) {
        assert!(r1.success, "{:?}", r1.receipt.error);
        assert!(r2.success, "{:?}", r2.receipt.error);
        assert_eq!(r1.success, r2.success);
        assert_eq!(r1.gas_used, r2.gas_used);
        assert_eq!(r1.receipt.status, r2.receipt.status);
    }

    // Final states must be identical
    assert_eq!(
        state1.balance_of("alice", "udgt"),
        state2.balance_of("alice", "udgt")
    );
    assert_eq!(
        state1.balance_of("bob", "udgt"),
        state2.balance_of("bob", "udgt")
    );
    assert_eq!(
        state1.balance_of("charlie", "udgt"),
        state2.balance_of("charlie", "udgt")
    );
}

#[test]
fn test_state_isolation_between_transactions() {
    let (mut state, _state_directory) = create_test_state();
    let gas_schedule = GasSchedule::default();

    state.set_balance("alice", "udgt", 50_000);
    state.set_balance("alice", "udrt", 100_000_000);

    // First transaction that will fail due to OOM
    let tx_fail = Transaction::new("tx_fail", "alice", "bob", 1_000, 5_000, 0, None)
        .with_gas(50, 1)
        .with_signature("sig");

    let initial_balance = state.balance_of("alice", "udgt");
    let result_fail =
        execute_transaction(&tx_fail, &mut state, 100, 0, &gas_schedule, None).unwrap();
    assert!(!result_fail.success);

    // Bob should not have received any money due to revert
    assert_eq!(state.balance_of("bob", "udgt"), 0);

    // Failed execution consumes the nonce and fee, but transfers no udgt.
    assert_eq!(result_fail.receipt.error.as_deref(), Some("OutOfGas"));
    assert_eq!(state.balance_of("alice", "udgt"), initial_balance);
    assert_eq!(state.balance_of("alice", "udrt"), 100_000_000 - 50);
    assert_eq!(state.nonce_of("alice"), 1);

    // Second transaction that will succeed
    let tx_success = Transaction::new("tx_success", "alice", "bob", 1_000, 5_000, 1, None)
        .with_gas(25_000, 1)
        .with_signature("sig");

    let result_success =
        execute_transaction(&tx_success, &mut state, 100, 1, &gas_schedule, None).unwrap();
    assert!(result_success.success);

    // Now bob should have received the money
    assert_eq!(state.balance_of("bob", "udgt"), 1_000);
    assert_eq!(state.balance_of("alice", "udgt"), initial_balance - 1_000);
    assert_eq!(state.balance_of("alice", "udrt"), 100_000_000 - 50 - 25_000);
    assert_eq!(state.nonce_of("alice"), 2);
}
