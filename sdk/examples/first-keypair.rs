//! Generate your first ML-DSA-65 keypair and derive a Dytallix address.
//! Run with: cargo run --example first-keypair

use dytallix_sdk::{verify_mldsa65, DAddr, DytallixKeypair};

fn main() {
    println!("Generating ML-DSA-65 keypair...");
    let keypair = DytallixKeypair::generate();
    println!("Public key:  {} bytes", keypair.public_key().len());
    println!("Private key: {} bytes", keypair.private_key().len());

    let addr = DAddr::from_public_key(keypair.public_key()).unwrap();
    println!("D-Addr:      {addr}");

    let message = b"hello dytallix";
    let signature = keypair.sign(message).unwrap();
    println!("Signature:   {} bytes", signature.len());

    let valid = verify_mldsa65(keypair.public_key(), message, &signature).unwrap();
    println!("Valid:       {valid}");
}
