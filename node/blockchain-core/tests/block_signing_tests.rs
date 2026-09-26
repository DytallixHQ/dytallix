use dytallix_node::crypto::PQCManager;
use dytallix_node::types::BlockHeader;

#[test]
fn validator_address_derivation_stable() {
    let mgr = PQCManager::new().expect("pqc");
    let addr1 = mgr.derive_validator_address();
    let addr2 = mgr.derive_validator_address();
    assert_eq!(
        addr1, addr2,
        "address must be stable across derivations in same process"
    );
    assert!(addr1.starts_with("dyt1"));
}

#[test]
fn block_sign_verify_and_tamper() {
    let mgr = PQCManager::new().expect("pqc");
    let header = BlockHeader {
        number: 1,
        parent_hash: "0".repeat(64),
        transactions_root: "a".repeat(64),
        state_root: "b".repeat(64),
        timestamp: 12345,
        validator: mgr.derive_validator_address(),
        signature: dytallix_node::types::PQCBlockSignature {
            signature: dytallix_pqc::Signature {
                data: vec![],
                algorithm: dytallix_pqc::SignatureAlgorithm::Dilithium3,
            },
            public_key: vec![],
        },
        nonce: 0,
    };
    let mut signed = header.clone();
    let sig = mgr.sign_block_header(&signed).unwrap();
    signed.signature = sig.clone();
    assert!(mgr.verify_block_signature(&signed).unwrap());
    // Tamper
    let mut tampered = signed.clone();
    tampered.timestamp += 1;
    assert!(!mgr.verify_block_signature(&tampered).unwrap());
}
