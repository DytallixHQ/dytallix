/// Integration test to verify active PQC signing and verification roundtrip.
#[test]
fn test_dilithium5_sign_and_verify_roundtrip() {
    use dytallix_fast_node::crypto::{ActivePQC, PQC};

    let (secret_key, public_key) = ActivePQC::keypair();
    let message = b"test message to sign";
    let signature = ActivePQC::sign(&secret_key, message);

    assert!(
        ActivePQC::verify(&public_key, message, &signature),
        "roundtrip verification should succeed"
    );

    println!("✅ Active PQC roundtrip test passed");
    println!("PK len: {}", public_key.len());
    println!("SK len: {}", secret_key.len());
    println!("Signature len: {}", signature.len());
}
