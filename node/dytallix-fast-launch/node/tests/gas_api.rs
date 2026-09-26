/*
API tests to verify gas fields are properly exposed via RPC endpoints
and JSON serialization works correctly.
*/

use dytallix_fast_node::execution::execute_transaction;
use dytallix_fast_node::gas::GasSchedule;
use dytallix_fast_node::state::State;
use dytallix_fast_node::storage::receipts::{TxReceipt, TxStatus, RECEIPT_FORMAT_VERSION};
use dytallix_fast_node::storage::state::Storage;
use dytallix_fast_node::storage::tx::Transaction;
// removed unused serde_json::json, Value, and PathBuf imports
use std::sync::Arc;

fn create_test_state() -> (State, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(Storage::open(dir.path().join("node.db")).unwrap());
    (State::new(storage), dir)
}

#[test]
fn test_receipt_json_serialization() {
    let (mut state, _directory) = create_test_state();
    let gas_schedule = GasSchedule::default();

    // Ensure sufficient balance for upfront fee (30_000 * 1_200) and transfer amount
    state.set_balance("alice", "udgt", 50_000_000);
    state.set_balance("alice", "udrt", 50_000_000);

    // Use a valid nonce to avoid early InvalidNonce failure
    let tx = Transaction::new("test_hash_12345", "alice", "bob", 1_500, 7_500, 0, None)
        .with_gas(30_000, 1_200)
        .with_signature("signature_data");

    let result = execute_transaction(&tx, &mut state, 150, 2, &gas_schedule, None).unwrap();
    let receipt = result.receipt;

    // Serialize to JSON
    let json_value = serde_json::to_value(&receipt).expect("Failed to serialize receipt");

    // Verify all required gas fields are present in JSON
    assert!(json_value.get("gas_used").is_some());
    assert!(json_value.get("gas_limit").is_some());
    assert!(json_value.get("gas_price").is_some());
    assert!(json_value.get("gas_refund").is_some());
    assert!(json_value.get("receipt_version").is_some());
    assert!(json_value.get("success").is_some());

    // Verify field values
    assert_eq!(json_value["gas_limit"], 30_000);
    assert_eq!(json_value["gas_price"], 1_200);
    assert_eq!(json_value["gas_refund"], 0);
    assert_eq!(json_value["receipt_version"], RECEIPT_FORMAT_VERSION);
    assert_eq!(json_value["tx_hash"], "test_hash_12345");
    assert_eq!(json_value["block_height"], 150);
    assert_eq!(json_value["index"], 2);

    if result.success {
        assert_eq!(json_value["success"], true);
        assert_eq!(json_value["status"], "Success");
        assert!(json_value["gas_used"].as_u64().unwrap() > 0);
    } else {
        assert_eq!(json_value["success"], false);
        assert_eq!(json_value["status"], "Failed");
        assert!(json_value.get("error").is_some());
    }
}

#[test]
fn test_receipt_deserialization_compatibility() {
    // Test that receipts can be deserialized from JSON with all gas fields
    let json_str = r#"{
        "receipt_version": 1,
        "tx_hash": "test_hash",
        "status": "Success",
        "block_height": 100,
        "index": 0,
        "from": "alice",
        "to": "bob",
        "amount": "1000",
        "fee": "5000",
        "nonce": 42,
        "error": null,
        "gas_used": 15000,
        "gas_limit": 25000,
        "gas_price": 1000,
        "gas_refund": 0,
        "success": true
    }"#;

    let receipt: TxReceipt = serde_json::from_str(json_str).expect("Failed to deserialize receipt");

    assert_eq!(receipt.receipt_version, 1);
    assert_eq!(receipt.tx_hash, "test_hash");
    assert_eq!(receipt.status, TxStatus::Success);
    assert_eq!(receipt.block_height, Some(100));
    assert_eq!(receipt.index, Some(0));
    assert_eq!(receipt.gas_used, 15000);
    assert_eq!(receipt.gas_limit, 25000);
    assert_eq!(receipt.gas_price, 1000);
    assert_eq!(receipt.gas_refund, 0);
    assert!(receipt.success);
}

#[test]
fn test_gas_fields_in_failed_transaction() {
    let (mut state, _directory) = create_test_state();
    let gas_schedule = GasSchedule::default();

    // Ensure sufficient balance to cover upfront fee so we hit OutOfGas path
    state.set_balance("alice", "udgt", 1_000_000);
    state.set_balance("alice", "udrt", 1_000_000);

    // Transaction that will fail due to OOM
    let tx = Transaction::new("oom_hash", "alice", "bob", 1_000, 5_000, 0, None)
        .with_gas(100, 2_000)
        .with_signature("sig");

    let result = execute_transaction(&tx, &mut state, 200, 1, &gas_schedule, None).unwrap();
    let receipt = result.receipt;

    assert!(!result.success);
    assert_eq!(receipt.status, TxStatus::Failed);
    assert!(receipt.error.as_ref().unwrap().contains("OutOfGas"));

    // Verify gas fields are correct even for failed transaction
    assert_eq!(receipt.gas_limit, 100);
    assert_eq!(receipt.gas_price, 2_000);
    assert!(receipt.gas_used > 0); // Some gas was consumed
    assert_eq!(receipt.gas_refund, 0);
    assert!(!receipt.success);

    // Verify JSON serialization includes all fields
    let json_value = serde_json::to_value(&receipt).unwrap();
    assert_eq!(json_value["gas_limit"], 100);
    assert_eq!(json_value["gas_price"], 2_000);
    assert_eq!(json_value["success"], false);
    assert!(json_value["error"].as_str().unwrap().contains("OutOfGas"));
}

#[test]
fn test_legacy_transaction_json_compatibility() {
    let (mut state, _directory) = create_test_state();
    let gas_schedule = GasSchedule::default();

    state.set_balance("alice", "udgt", 100_000);

    // Legacy transaction (gas_limit=0, gas_price=0)
    let tx = Transaction::new(
        "legacy_hash".to_string(),
        "alice".to_string(),
        "bob".to_string(),
        1_000,
        8_000, // fee
        0,
        Some("sig".to_string()),
    );

    let result = execute_transaction(&tx, &mut state, 100, 0, &gas_schedule, None).unwrap();
    let receipt = result.receipt;

    // Should be treated as gas_limit=fee, gas_price=1
    assert_eq!(receipt.gas_limit, 8_000);
    assert_eq!(receipt.gas_price, 1);

    // JSON should include proper gas fields
    let json_value = serde_json::to_value(&receipt).unwrap();
    assert_eq!(json_value["gas_limit"], 8_000);
    assert_eq!(json_value["gas_price"], 1);
    assert_eq!(json_value["gas_refund"], 0);
}

#[test]
fn test_fee_charged_calculation() {
    let (mut state, _directory) = create_test_state();
    let gas_schedule = GasSchedule::default();

    state.set_balance("alice", "udgt", 100_000);

    let tx = Transaction::new("fee_test", "alice", "bob", 1_000, 5_000, 0, None)
        .with_gas(25_000, 1_500)
        .with_signature("sig");

    let result = execute_transaction(&tx, &mut state, 100, 0, &gas_schedule, None).unwrap();
    let receipt = result.receipt;

    // Calculate expected fee
    let expected_fee = receipt.fee_charged_datt();
    let manual_calculation = receipt.gas_limit as u64 * receipt.gas_price as u64;

    assert_eq!(expected_fee, manual_calculation);
    assert_eq!(expected_fee, 25_000 * 1_500); // gas_limit * gas_price

    // Verify it's the same for both success and failure cases
    state.set_balance("charlie", "udgt", 1_000); // Insufficient for gas

    let tx_fail = Transaction::new("fail_test", "charlie", "alice", 500, 1_000, 0, None)
        .with_gas(50_000, 2_000)
        .with_signature("sig");

    let result_fail =
        execute_transaction(&tx_fail, &mut state, 100, 1, &gas_schedule, None).unwrap();
    let receipt_fail = result_fail.receipt;

    assert!(!result_fail.success);

    let fee_fail = receipt_fail.fee_charged_datt();
    assert_eq!(fee_fail, 50_000 * 2_000); // Still full fee calculation
}

#[test]
fn test_receipt_version_consistency() {
    let (mut state, _directory) = create_test_state();
    let gas_schedule = GasSchedule::default();

    state.set_balance("alice", "udgt", 100_000);

    let transactions = vec![
        Transaction::new("tx1", "alice", "bob", 1_000, 5_000, 0, None)
            .with_gas(25_000, 1_000)
            .with_signature("sig"),
        Transaction::new("tx2", "alice", "bob", 500, 3_000, 1, None).with_signature("sig"),
    ];

    for (i, tx) in transactions.iter().enumerate() {
        let result =
            execute_transaction(tx, &mut state, 100, i as u32, &gas_schedule, None).unwrap();
        let receipt = result.receipt;

        // All receipts must have the same version
        assert_eq!(receipt.receipt_version, RECEIPT_FORMAT_VERSION);

        // JSON serialization must preserve version
        let json_value = serde_json::to_value(&receipt).unwrap();
        assert_eq!(json_value["receipt_version"], RECEIPT_FORMAT_VERSION);
    }
}

#[test]
fn test_json_schema_validation() {
    let (mut state, _directory) = create_test_state();
    let gas_schedule = GasSchedule::default();

    state.set_balance("alice", "udgt", 100_000);

    let tx = Transaction::new("schema_test", "alice", "bob", 1_000, 5_000, 0, None)
        .with_gas(25_000, 1_000)
        .with_signature("sig");

    let result = execute_transaction(&tx, &mut state, 100, 0, &gas_schedule, None).unwrap();
    let receipt = result.receipt;

    let json_value = serde_json::to_value(&receipt).unwrap();

    // Verify all required fields exist and have correct types
    let required_fields = vec![
        ("receipt_version", "number"),
        ("tx_hash", "string"),
        ("status", "string"),
        ("from", "string"),
        ("to", "string"),
        ("amount", "string"), // Serialized as string for large numbers
        ("fee", "string"),    // Serialized as string for large numbers
        ("nonce", "number"),
        ("gas_used", "number"),
        ("gas_limit", "number"),
        ("gas_price", "number"),
        ("gas_refund", "number"),
        ("success", "boolean"),
    ];

    for (field_name, expected_type) in required_fields {
        assert!(
            json_value.get(field_name).is_some(),
            "Missing field: {field_name}"
        );

        let field_value = &json_value[field_name];
        match expected_type {
            "number" => assert!(
                field_value.is_number(),
                "Field {field_name} should be number"
            ),
            "string" => assert!(
                field_value.is_string(),
                "Field {field_name} should be string"
            ),
            "boolean" => assert!(
                field_value.is_boolean(),
                "Field {field_name} should be boolean"
            ),
            _ => panic!("Unknown expected type: {expected_type}"),
        }
    }

    // Verify optional fields
    if result.success {
        assert!(json_value["error"].is_null());
    } else {
        assert!(json_value["error"].is_string());
    }

    if receipt.block_height.is_some() {
        assert!(json_value["block_height"].is_number());
    }

    if receipt.index.is_some() {
        assert!(json_value["index"].is_number());
    }
}
