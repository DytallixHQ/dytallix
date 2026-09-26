/// Integration tests for the current CLI-style signature workflow.
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use dytallix_fast_node::crypto::{canonical_json, sha3_256, ActivePQC, PQC};
use dytallix_fast_node::types::tx::{Msg, SignedTx, Tx};

#[test]
fn test_cli_binary_signature_verification() {
    let (secret_key, public_key) = ActivePQC::keypair();

    let tx = Tx {
        chain_id: "dytallix-testnet".to_string(),
        nonce: 1,
        msgs: vec![Msg::Send {
            from: "dyt1sender".to_string(),
            to: "dyt1recipient".to_string(),
            denom: "DGT".to_string(),
            amount: 100,
        }],
        fee: 1_000,
        memo: String::new(),
    };

    let canonical = canonical_json(&tx).expect("canonical_json should succeed");
    let hash = sha3_256(&canonical);
    let signature = ActivePQC::sign(&secret_key, &hash);

    println!("\n=== CLI Integration Test ===");
    println!("Canonical JSON: {}", String::from_utf8_lossy(&canonical));
    println!("Hash (hex): {}", hex::encode(hash));
    println!("Signature bytes: {}", signature.len());

    assert!(
        ActivePQC::verify(&public_key, &hash, &signature),
        "active backend should verify CLI-style detached signature"
    );

    println!("\n✅ SUCCESS: CLI-style signature verified in Rust node");
}

#[test]
fn test_base64_roundtrip() {
    let (secret_key, public_key) = ActivePQC::keypair();
    let msg = b"test message";
    let signature = ActivePQC::sign(&secret_key, msg);

    println!("\n=== Base64 Encoding Test ===");
    println!("Original signature bytes: {}", signature.len());

    let sig_b64 = B64.encode(&signature);
    let pk_b64 = B64.encode(&public_key);
    let decoded_sig = B64.decode(&sig_b64).expect("Failed to decode base64");
    let decoded_pk = B64.decode(&pk_b64).expect("Failed to decode base64");

    assert_eq!(decoded_sig, signature, "signature base64 roundtrip failed");
    assert_eq!(decoded_pk, public_key, "public key base64 roundtrip failed");
    assert!(
        ActivePQC::verify(&decoded_pk, msg, &decoded_sig),
        "decoded signature should still verify"
    );

    println!("✅ Base64 encoding/decoding works correctly");
}

#[test]
fn test_signature_format_details() {
    let (secret_key, public_key) = ActivePQC::keypair();
    let hash: [u8; 32] = [0x42; 32];
    let signature = ActivePQC::sign(&secret_key, &hash);

    println!("\n=== Signature Format Details ===");
    println!("Input hash: {} bytes", hash.len());
    println!("Hash (hex): {}", hex::encode(hash));
    println!("Public key length: {}", public_key.len());
    println!("Signature length: {}", signature.len());

    let signed_tx = SignedTx {
        tx: Tx {
            chain_id: "dyt-local-1".to_string(),
            nonce: 1,
            msgs: vec![Msg::Send {
                from: "dyt1sender".to_string(),
                to: "dyt1recipient".to_string(),
                denom: "DGT".to_string(),
                amount: 42,
            }],
            fee: 1_000,
            memo: "format-details".to_string(),
        },
        public_key: B64.encode(&public_key),
        signature: B64.encode(&signature),
        algorithm: ActivePQC::ALG.to_string(),
        version: 1,
    };

    assert!(
        ActivePQC::verify(&public_key, &hash, &signature),
        "detached signature should verify directly"
    );
    assert_eq!(signed_tx.algorithm, ActivePQC::ALG);

    println!("\n✅ Signature format verified");
}
