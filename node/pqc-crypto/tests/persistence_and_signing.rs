use dytallix_pqc::{
    bridge::{BridgePQCManager, CrossChainPayload},
    PQCManager, SignatureAlgorithm,
};
use std::collections::HashMap;

fn payload(amount: u64) -> CrossChainPayload {
    CrossChainPayload::GenericBridgePayload {
        asset_id: "test".into(),
        amount,
        source_chain: "ethereum".into(),
        dest_chain: "cosmos".into(),
        source_address: "source".into(),
        dest_address: "destination".into(),
        metadata: HashMap::new(),
    }
}

#[test]
fn incomplete_existing_file_is_not_replaced() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("keys.json");
    let original = br#"{"signature_keypair":{"public_key":[],"algorithm":"Dilithium3"}}"#;
    std::fs::write(&path, original).unwrap();
    assert!(PQCManager::load_or_generate(&path).is_err());
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[test]
fn public_key_serialization_still_omits_secret_material() {
    let manager = PQCManager::new().unwrap();
    let pair = manager
        .generate_keypair(&SignatureAlgorithm::Dilithium5)
        .unwrap();
    let json = serde_json::to_value(pair).unwrap();
    assert!(json.get("secret_key").is_none());
}

#[cfg(unix)]
#[test]
fn saved_private_file_has_restricted_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("keys.json");
    PQCManager::new().unwrap().save_to_file(&path).unwrap();
    assert_eq!(
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn registered_public_key_alone_cannot_select_an_unrelated_signer() {
    let mut verifier = BridgePQCManager::new().unwrap();
    let separate = PQCManager::new().unwrap();
    let key = separate
        .generate_keypair(&SignatureAlgorithm::Dilithium5)
        .unwrap();
    verifier.add_validator(
        "validator".into(),
        key.public_key.clone(),
        key.algorithm.clone(),
    );
    assert!(verifier
        .sign_bridge_payload(&payload(42), "ethereum", "validator")
        .is_err());
}

#[test]
fn failed_payload_check_does_not_consume_valid_signature() {
    let mut manager = BridgePQCManager::new().unwrap();
    let key = manager
        .generate_validator_keypair(&SignatureAlgorithm::Dilithium5)
        .unwrap();
    manager.add_validator(
        "validator".into(),
        key.public_key.clone(),
        key.algorithm.clone(),
    );
    let signature = manager
        .sign_bridge_payload(&payload(42), "ethereum", "validator")
        .unwrap();
    assert!(!manager
        .verify_bridge_signature(&signature, &payload(43))
        .unwrap());
    assert!(manager
        .verify_bridge_signature(&signature, &payload(42))
        .unwrap());
    assert!(!manager
        .verify_bridge_signature(&signature, &payload(42))
        .unwrap());
}

#[test]
fn concurrent_verification_consumes_valid_nonce_once() {
    let mut manager = BridgePQCManager::new().unwrap();
    let key = manager
        .generate_validator_keypair(&SignatureAlgorithm::Dilithium5)
        .unwrap();
    manager.add_validator(
        "validator".into(),
        key.public_key.clone(),
        key.algorithm.clone(),
    );
    let message = payload(42);
    let signature = manager
        .sign_bridge_payload(&message, "ethereum", "validator")
        .unwrap();
    let barrier = std::sync::Barrier::new(4);
    let accepted = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    manager
                        .verify_bridge_signature(&signature, &message)
                        .unwrap()
                })
            })
            .collect();
        handles
            .into_iter()
            .filter_map(|handle| handle.join().unwrap().then_some(()))
            .count()
    });
    assert_eq!(accepted, 1);
}

#[test]
fn incomplete_multisignature_batch_can_be_completed() {
    let mut manager = BridgePQCManager::new().unwrap();
    manager.set_min_signatures(2);
    let message = payload(42);
    let mut signatures = Vec::new();
    for id in ["first", "second"] {
        let key = manager
            .generate_validator_keypair(&SignatureAlgorithm::Dilithium5)
            .unwrap();
        manager.add_validator(id.into(), key.public_key.clone(), key.algorithm.clone());
        signatures.push(
            manager
                .sign_bridge_payload(&message, "ethereum", id)
                .unwrap(),
        );
    }
    let partial = manager
        .verify_multi_signature(&signatures[..1], &message)
        .unwrap();
    assert!(!partial.consensus_reached);
    assert_eq!(partial.valid_signatures, 1);
    let complete = manager
        .verify_multi_signature(&signatures, &message)
        .unwrap();
    assert!(complete.consensus_reached);
    assert_eq!(complete.valid_signatures, 2);
    assert!(
        !manager
            .verify_multi_signature(&signatures, &message)
            .unwrap()
            .consensus_reached
    );
}

#[test]
fn multisignature_error_does_not_commit_earlier_entries() {
    let mut manager = BridgePQCManager::new().unwrap();
    let key = manager
        .generate_validator_keypair(&SignatureAlgorithm::Dilithium5)
        .unwrap();
    manager.add_validator(
        "first".into(),
        key.public_key.clone(),
        key.algorithm.clone(),
    );
    let message = payload(42);
    let signature = manager
        .sign_bridge_payload(&message, "ethereum", "first")
        .unwrap();
    let mut unknown = signature.clone();
    unknown.validator_id = "unknown".into();
    assert!(manager
        .verify_multi_signature(&[signature.clone(), unknown], &message)
        .is_err());
    assert!(manager
        .verify_bridge_signature(&signature, &message)
        .unwrap());
}
