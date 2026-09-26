//! Unit tests for Oracle runtime functions

// Wrap whole file to avoid dangling feature-specific imports when oracle feature disabled
#![cfg(feature = "oracle")]

use crate::runtime::oracle::{apply_oracle_risk, current_timestamp, get_oracle_risk, verify_sig};
use crate::storage::oracle::OracleStore;
use base64::Engine;
use rocksdb::DB;
use tempfile::TempDir; // switched from NamedTempFile to TempDir

#[tokio::test]
async fn test_apply_and_get_oracle_risk() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("oracle_db");
    let db = DB::open_default(db_path).unwrap();
    let store = OracleStore { db: &db };

    let tx_hash = "0x1234567890abcdef1234567890abcdef12345678";
    let score_str = "0.75";
    let model_id = "risk-v1";
    let timestamp = current_timestamp();
    let source = "oracle-1";

    // Test apply
    let result = apply_oracle_risk(&store, tx_hash, score_str, model_id, timestamp, source);
    assert!(result.is_ok(), "Failed to apply oracle risk: {result:?}");

    // Test get
    let retrieved = get_oracle_risk(&store, tx_hash);
    assert!(retrieved.is_some(), "Failed to retrieve oracle risk");

    let record = retrieved.unwrap();
    assert_eq!(record.tx_hash, tx_hash);
    assert_eq!(record.score_str, score_str);
    assert_eq!(record.model_id, model_id);
    assert_eq!(record.ingested_at, timestamp);
    assert_eq!(record.source, source);
    assert_eq!(record.risk_score, 0.75);
}

#[tokio::test]
async fn test_apply_oracle_risk_validation() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("oracle_db");
    let db = DB::open_default(db_path).unwrap();
    let store = OracleStore { db: &db };
    let timestamp = current_timestamp();

    // Test invalid transaction hash
    let result = apply_oracle_risk(&store, "invalid", "0.5", "model", timestamp, "source");
    assert!(result.is_err(), "Should fail with invalid tx hash");

    // Test empty score string
    let result = apply_oracle_risk(&store, "0x123", "", "model", timestamp, "source");
    assert!(result.is_err(), "Should fail with empty score string");

    // Test empty model ID
    let result = apply_oracle_risk(&store, "0x123", "0.5", "", timestamp, "source");
    assert!(result.is_err(), "Should fail with empty model ID");

    // Test empty source
    let result = apply_oracle_risk(&store, "0x123", "0.5", "model", timestamp, "");
    assert!(result.is_err(), "Should fail with empty source");

    // Test invalid score range
    let result = apply_oracle_risk(&store, "0x123", "1.5", "model", timestamp, "source");
    assert!(result.is_err(), "Should fail with score > 1.0");

    let result = apply_oracle_risk(&store, "0x123", "-0.1", "model", timestamp, "source");
    assert!(result.is_err(), "Should fail with score < 0.0");

    // Test invalid score format
    let result = apply_oracle_risk(
        &store,
        "0x123",
        "not_a_number",
        "model",
        timestamp,
        "source",
    );
    assert!(result.is_err(), "Should fail with invalid score format");
}

#[test]
fn test_verify_sig_with_invalid_inputs() {
    // Test with invalid base64
    assert!(!verify_sig("payload", "invalid_base64", "invalid_base64"));

    // Test with empty inputs
    assert!(!verify_sig("", "", ""));

    // Test with valid base64 but invalid key/signature format
    let invalid_b64 = base64::engine::general_purpose::STANDARD.encode(b"too_short");
    assert!(!verify_sig("payload", &invalid_b64, &invalid_b64));
}

#[tokio::test]
async fn test_deterministic_score_preservation() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("oracle_db");
    let db = DB::open_default(db_path).unwrap();
    let store = OracleStore { db: &db };

    let tx_hash = "0xabcdef1234567890abcdef1234567890abcdef12";
    let original_score = "0.123456789123456789"; // High precision string
    let model_id = "risk-v1";
    let timestamp = current_timestamp();
    let source = "oracle-1";

    apply_oracle_risk(&store, tx_hash, original_score, model_id, timestamp, source).unwrap();

    let record = get_oracle_risk(&store, tx_hash).unwrap();
    // Verify original string is preserved exactly
    assert_eq!(record.score_str, original_score);
}

#[tokio::test]
async fn test_get_oracle_risk_not_found() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("oracle_db");
    let db = DB::open_default(db_path).unwrap();
    let store = OracleStore { db: &db };

    let result = get_oracle_risk(&store, "0xnonexistent1234567890abcdef123456");
    assert!(
        result.is_none(),
        "Should return None for non-existent record"
    );
}

#[test]
fn test_current_timestamp() {
    let timestamp1 = current_timestamp();
    std::thread::sleep(std::time::Duration::from_millis(10));
    let timestamp2 = current_timestamp();

    assert!(
        timestamp2 >= timestamp1,
        "Timestamp should be monotonically increasing"
    );
    assert!(
        timestamp2 - timestamp1 < 2,
        "Timestamps should be close together"
    );
}
