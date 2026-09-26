use dytallix_fast_node::storage::oracle::{AiRiskRecord, OracleStore};
use dytallix_fast_node::storage::state::Storage;
use tempfile::tempdir;

#[test]
fn oracle_store_roundtrip() {
    let dir = tempdir().unwrap();
    let store = Storage::open(dir.path().join("node.db")).unwrap();
    let rec = AiRiskRecord {
        tx_hash: "0xabc".into(),
        model_id: "test-model-v1".into(),
        risk_score: 0.55,
        score_str: "0.55".into(),
        confidence: Some(0.85),
        signature: None,
        oracle_pubkey: None,
        ingested_at: 0,
        source: "test".into(),
    };
    store
        .db
        .put("oracle:ai:0xabc", serde_json::to_vec(&rec).unwrap())
        .unwrap();
    let raw = store.db.get("oracle:ai:0xabc").unwrap().unwrap();
    let got: AiRiskRecord = serde_json::from_slice(&raw).unwrap();
    assert_eq!(got.risk_score, 0.55);
    assert_eq!(got.model_id, "test-model-v1");
    assert_eq!(got.confidence, Some(0.85));
}

#[test]
fn oracle_validation_tests() {
    let dir = tempdir().unwrap();
    let storage = Storage::open(dir.path().join("node.db")).unwrap();
    let oracle_store = OracleStore { db: &storage.db };

    // Valid record should succeed
    let valid_rec = AiRiskRecord {
        tx_hash: "0x123abc".into(),
        model_id: "model-v1".into(),
        risk_score: 0.5,
        score_str: "0.5".into(),
        confidence: Some(0.8),
        signature: None,
        oracle_pubkey: None,
        ingested_at: 0,
        source: "test".into(),
    };
    assert!(oracle_store.put_ai_risk(&valid_rec).is_ok());

    // Invalid risk_score should fail
    let invalid_score = AiRiskRecord {
        tx_hash: "0x456def".into(),
        model_id: "model-v1".into(),
        risk_score: 1.5, // Invalid - out of range
        score_str: "1.5".into(),
        confidence: Some(0.8),
        signature: None,
        oracle_pubkey: None,
        ingested_at: 0,
        source: "test".into(),
    };
    assert!(oracle_store.put_ai_risk(&invalid_score).is_err());

    // Invalid confidence should fail
    let invalid_confidence = AiRiskRecord {
        tx_hash: "0x789ghi".into(),
        model_id: "model-v1".into(),
        risk_score: 0.5,
        score_str: "0.5".into(),
        confidence: Some(1.2), // Invalid - out of range
        signature: None,
        oracle_pubkey: None,
        ingested_at: 0,
        source: "test".into(),
    };
    assert!(oracle_store.put_ai_risk(&invalid_confidence).is_err());

    // Empty model_id should fail
    let empty_model = AiRiskRecord {
        tx_hash: "0xaabbcc".into(),
        model_id: "".into(), // Invalid - empty
        risk_score: 0.5,
        score_str: "0.5".into(),
        confidence: Some(0.8),
        signature: None,
        oracle_pubkey: None,
        ingested_at: 0,
        source: "test".into(),
    };
    assert!(oracle_store.put_ai_risk(&empty_model).is_err());

    // Invalid tx_hash should fail
    let invalid_hash = AiRiskRecord {
        tx_hash: "invalid_hash".into(), // Invalid - doesn't start with 0x
        model_id: "model-v1".into(),
        risk_score: 0.5,
        score_str: "0.5".into(),
        confidence: Some(0.8),
        signature: None,
        oracle_pubkey: None,
        ingested_at: 0,
        source: "test".into(),
    };
    assert!(oracle_store.put_ai_risk(&invalid_hash).is_err());
}

#[test]
fn oracle_batch_operations() {
    let dir = tempdir().unwrap();
    let storage = Storage::open(dir.path().join("node.db")).unwrap();
    let oracle_store = OracleStore { db: &storage.db };

    let records = vec![
        AiRiskRecord {
            tx_hash: "0x111".into(),
            model_id: "model-v1".into(),
            risk_score: 0.1,
            score_str: "0.1".into(),
            confidence: Some(0.9),
            signature: None,
            oracle_pubkey: None,
            ingested_at: 0,
            source: "test".into(),
        },
        AiRiskRecord {
            tx_hash: "0x222".into(),
            model_id: "model-v2".into(),
            risk_score: 0.8,
            score_str: "0.8".into(),
            confidence: None,
            signature: None,
            oracle_pubkey: None,
            ingested_at: 0,
            source: "test".into(),
        },
        AiRiskRecord {
            tx_hash: "0x333".into(),
            model_id: "model-v1".into(),
            risk_score: 2.0, // Invalid - should fail
            score_str: "2.0".into(),
            confidence: Some(0.5),
            signature: None,
            oracle_pubkey: None,
            ingested_at: 0,
            source: "test".into(),
        },
    ];

    let failed_hashes = oracle_store.put_ai_risks_batch(&records).unwrap();

    // One record should have failed (the one with invalid risk_score)
    assert_eq!(failed_hashes.len(), 1);
    assert_eq!(failed_hashes[0], "0x333");

    // The valid records should be retrievable
    assert!(oracle_store.get_ai_risk("0x111").is_some());
    assert!(oracle_store.get_ai_risk("0x222").is_some());
    assert!(oracle_store.get_ai_risk("0x333").is_none());
}
