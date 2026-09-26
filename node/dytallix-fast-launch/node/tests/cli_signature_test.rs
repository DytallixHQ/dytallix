/// Tests that mirror the current CLI signature transport format.
///
/// The public CLI signs the canonical transaction hash with the active PQC
/// backend, transports detached signature bytes, and base64-encodes both the
/// signature and public key in `SignedTx`.
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use dytallix_fast_node::crypto::{canonical_json, sha3_256, ActivePQC, PQC};
use dytallix_fast_node::types::tx::{Msg, SignedTx, Tx};
use sha3::{Digest, Sha3_256};

#[test]
fn test_cli_signature_format() {
    let (secret_key, public_key) = ActivePQC::keypair();
    let msg = b"test message hash";
    let signature = ActivePQC::sign(&secret_key, msg);

    println!("Message length: {}", msg.len());
    println!("Signature length: {}", signature.len());
    println!("Public key length: {}", public_key.len());

    assert!(!signature.is_empty(), "signature should not be empty");
    assert!(
        ActivePQC::verify(&public_key, msg, &signature),
        "active PQC backend should verify its detached signature"
    );

    println!("\n✓ CLI signature format is compatible with node verification");
}

#[test]
fn test_node_verification_with_cli_format() {
    let (secret_key, public_key) = ActivePQC::keypair();

    let json = r#"{"from":"addr1","to":"addr2","amount":100,"nonce":1}"#;
    let mut hasher = Sha3_256::new();
    hasher.update(json.as_bytes());
    let hash = hasher.finalize();

    println!("Transaction JSON: {}", json);
    println!("Hash (hex): {}", hex::encode(hash));
    println!("Hash length: {}", hash.len());

    let signature = ActivePQC::sign(&secret_key, hash.as_slice());
    let cli_signature_hex = hex::encode(&signature);
    let sig_bytes = hex::decode(&cli_signature_hex).expect("Failed to decode hex");

    assert_eq!(
        sig_bytes, signature,
        "hex roundtrip should preserve signature"
    );
    assert!(
        ActivePQC::verify(&public_key, hash.as_slice(), &sig_bytes),
        "node verification should accept CLI-generated detached signatures"
    );

    println!("\n✓ Node can successfully verify CLI-generated signatures");
}

#[test]
fn test_signed_tx_base64_transport() {
    let (secret_key, public_key) = ActivePQC::keypair();

    let tx = Tx {
        chain_id: "dyt-local-1".to_string(),
        nonce: 1,
        msgs: vec![Msg::Send {
            from: "dytallix1sender".to_string(),
            to: "dytallix1recipient".to_string(),
            denom: "DGT".to_string(),
            amount: 100,
        }],
        fee: 1_000,
        memo: String::new(),
    };

    let canonical_bytes = canonical_json(&tx).expect("canonical_json should succeed");
    let hash = sha3_256(&canonical_bytes);
    let signature = ActivePQC::sign(&secret_key, &hash);

    let signed_tx = SignedTx {
        tx,
        signature: B64.encode(&signature),
        public_key: B64.encode(&public_key),
        algorithm: ActivePQC::ALG.to_string(),
        version: 1,
    };

    signed_tx
        .verify()
        .expect("SignedTx verification should succeed");

    println!("✅ Base64 transport is compatible with SignedTx verification");
}
