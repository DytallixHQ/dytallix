use base64::{engine::general_purpose::STANDARD as B64, Engine};
use fips204::ml_dsa_87;
use fips204::traits::{KeyGen, SerDes, Signer, Verifier};
use rand_core::OsRng;

fn main() {
    println!("Testing FIPS 204 ML-DSA-87 compatibility...\n");

    // Generate keypair
    let (pk, sk) = ml_dsa_87::KG::try_keygen_with_rng(&mut OsRng).expect("keygen failed");

    println!("Key sizes:");
    println!("  Public key: {} bytes", ml_dsa_87::PK_LEN);
    println!("  Secret key: {} bytes", ml_dsa_87::SK_LEN);

    // Sign a message
    let msg = b"test message";
    let sig = sk.try_sign(msg, &[]).expect("signing failed");
    println!("  Signature: {} bytes", sig.len());

    // Verify
    let verified = pk.verify(msg, &sig, &[]);
    println!("\nVerification result: {}", verified);

    // Test with base64 encoding (like in the app)
    let pk_bytes = pk.into_bytes();
    let sk_bytes = sk.into_bytes();
    let pk_b64 = B64.encode(&pk_bytes);
    let sk_b64 = B64.encode(&sk_bytes);
    let sig_b64 = B64.encode(&sig);

    println!("\nBase64 encoded lengths:");
    println!("  PK: {}", pk_b64.len());
    println!("  SK: {}", sk_b64.len());
    println!("  Sig: {}", sig_b64.len());

    // Test round-trip
    println!("\nTesting round-trip with base64...");
    let pk_bytes = B64.decode(&pk_b64).unwrap();
    let sig_bytes = B64.decode(&sig_b64).unwrap();

    let pk_array: [u8; ml_dsa_87::PK_LEN] = pk_bytes.try_into().unwrap();
    let pk_recovered = ml_dsa_87::PublicKey::try_from_bytes(pk_array).unwrap();

    let sig_array: [u8; ml_dsa_87::SIG_LEN] = sig_bytes.try_into().unwrap();
    let verified_after_roundtrip = pk_recovered.verify(msg, &sig_array, &[]);

    println!(
        "  Verification after base64 round-trip: {}",
        verified_after_roundtrip
    );
}
