/// Integration tests for PQC signature validation between CLI and node.
///
/// These tests exercise the current public flow:
/// - canonical JSON transaction encoding
/// - SHA3-256 transaction hashing
/// - detached signature generation with the active PQC backend
/// - base64 transport in SignedTx
/// - node-side verification through SignedTx::verify
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use dytallix_fast_node::crypto::{canonical_json, sha3_256, ActivePQC, PQC};
use dytallix_fast_node::types::tx::{Msg, SignedTx, Tx};

#[test]
fn test_cli_signature_verification() {
    let (secret_key, public_key) = ActivePQC::keypair();

    let tx = Tx {
        chain_id: "dyt-local-1".to_string(),
        nonce: 0,
        msgs: vec![Msg::Send {
            from: "dytallix1test000000000000000000000000000".to_string(),
            to: "dytallix1recipient00000000000000000000".to_string(),
            denom: "DGT".to_string(),
            amount: 1_000_000,
        }],
        fee: 1_000,
        memo: String::new(),
    };

    let canonical_bytes = canonical_json(&tx).expect("canonical_json should succeed");
    let hash = sha3_256(&canonical_bytes);
    let signature_bytes = ActivePQC::sign(&secret_key, &hash);

    println!(
        "Transaction canonical JSON: {}",
        String::from_utf8_lossy(&canonical_bytes)
    );
    println!("Transaction hash: {}", hex::encode(&hash));
    println!("Signature length: {} bytes", signature_bytes.len());
    println!("Public key length: {} bytes", public_key.len());

    let signed_tx = SignedTx {
        tx: tx.clone(),
        signature: B64.encode(&signature_bytes),
        public_key: B64.encode(&public_key),
        algorithm: ActivePQC::ALG.to_string(),
        version: 1,
    };

    println!("SignedTx created:");
    println!("  signature (base64): {} chars", signed_tx.signature.len());
    println!(
        "  public_key (base64): {} chars",
        signed_tx.public_key.len()
    );

    signed_tx
        .verify()
        .expect("node verification should succeed");
    assert!(
        ActivePQC::verify(&public_key, &hash, &signature_bytes),
        "Detached signature should verify against the transaction hash"
    );
}

#[test]
fn test_cli_exact_signature_format() {
    let (secret_key, public_key) = ActivePQC::keypair();

    let tx = Tx {
        chain_id: "test".to_string(),
        nonce: 0,
        msgs: vec![],
        fee: 0,
        memo: String::new(),
    };

    let canonical_bytes = canonical_json(&tx).expect("canonical_json should succeed");
    let hash = sha3_256(&canonical_bytes);
    let signature_bytes = ActivePQC::sign(&secret_key, &hash);

    println!("Detached signature length: {} bytes", signature_bytes.len());
    assert!(
        !signature_bytes.is_empty(),
        "Detached signature should contain signature bytes"
    );
    assert!(
        ActivePQC::verify(&public_key, &hash, &signature_bytes),
        "Detached signature should verify against the original hash"
    );

    let signed_tx = SignedTx {
        tx,
        signature: B64.encode(&signature_bytes),
        public_key: B64.encode(&public_key),
        algorithm: ActivePQC::ALG.to_string(),
        version: 1,
    };

    signed_tx
        .verify()
        .expect("SignedTx verification should succeed");
}

#[test]
fn test_real_world_transaction_flow() {
    println!("=== Simulating Real-World E2E Transaction Flow ===\n");

    let (secret_key, public_key) = ActivePQC::keypair();
    let address = format!("dytallix1{}", hex::encode(&public_key[..20]));
    println!("1. User address: {}", address);
    println!("   Public key length: {} bytes", public_key.len());

    let tx = Tx {
        chain_id: "dyt-local-1".to_string(),
        nonce: 0,
        msgs: vec![Msg::Send {
            from: address.clone(),
            to: "dytallix1test000000000000000000000000000".to_string(),
            denom: "DGT".to_string(),
            amount: 1_000_000,
        }],
        fee: 1_000,
        memo: "E2E test".to_string(),
    };
    println!("2. Transaction created");

    let canonical_bytes = canonical_json(&tx).expect("canonical_json should succeed");
    let hash = sha3_256(&canonical_bytes);
    let signature_bytes = ActivePQC::sign(&secret_key, &hash);
    println!("3. Transaction signed");
    println!(
        "   Canonical JSON: {}",
        String::from_utf8_lossy(&canonical_bytes)
    );
    println!("   Hash: {}", hex::encode(&hash));
    println!("   Signature length: {} bytes", signature_bytes.len());

    let signed_tx = SignedTx {
        tx: tx.clone(),
        signature: B64.encode(&signature_bytes),
        public_key: B64.encode(&public_key),
        algorithm: ActivePQC::ALG.to_string(),
        version: 1,
    };
    println!("4. SignedTx created for RPC submission");
    println!("   Signature (base64): {} chars", signed_tx.signature.len());
    println!(
        "   Public key (base64): {} chars",
        signed_tx.public_key.len()
    );

    println!("5. Node validating signature...");
    signed_tx
        .verify()
        .expect("signature verification should succeed");
    println!("   ✅ Signature verification PASSED!");
    println!("\n=== E2E Flow Complete: Transaction would be accepted ===");
}
