use dytallix_node::consensus::{
    AIRequestMetadata, AIRequestPayload, AIServiceType, RequestPriority,
};
use serde_json::json;

#[test]
fn test_ai_request_payload_roundtrip() {
    let mut payload = AIRequestPayload::new(
        AIServiceType::FraudDetection,
        json!({"transaction_id": "tx_12345", "amount": 1000}),
    );
    payload.priority = RequestPriority::High;
    payload.timeout_ms = 30_000;
    payload.requester_id = "validator_node_01".into();
    payload.correlation_id = Some("batch_001".into());
    payload.metadata = Some(AIRequestMetadata {
        client_version: Some("1.0.0".into()),
        request_source: Some("blockchain_consensus".into()),
        context: Some(json!({"block_height": 1234567})),
        ..AIRequestMetadata::new()
    });
    assert!(!payload.id.is_empty());
    assert!(!payload.nonce.is_empty());
    let restored = AIRequestPayload::from_json(&payload.to_json().unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(&restored).unwrap(),
        serde_json::to_value(&payload).unwrap()
    );
    assert_eq!(restored.timeout_ms, 30_000);
    assert_eq!(restored.requester_id, "validator_node_01");
    assert_eq!(
        restored.metadata.unwrap().request_source.as_deref(),
        Some("blockchain_consensus")
    );
}

#[test]
fn test_ai_request_payload_constructor_defaults() {
    for service in [
        AIServiceType::FraudDetection,
        AIServiceType::RiskScoring,
        AIServiceType::ContractAnalysis,
        AIServiceType::TransactionValidation,
        AIServiceType::AML,
    ] {
        let data = json!({"request": "fixture"});
        let payload = AIRequestPayload::new(service.clone(), data.clone());
        assert_eq!(payload.service_type, service);
        assert_eq!(payload.request_data, data);
        assert_eq!(payload.priority, RequestPriority::Normal);
        assert_eq!(payload.timeout_ms, 30_000);
        assert_eq!(payload.requester_id, "unknown");
        assert!(payload.callback_url.is_none());
        assert!(payload.metadata.is_none());
    }
}
