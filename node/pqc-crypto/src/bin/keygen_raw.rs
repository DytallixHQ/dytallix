use pqcrypto_dilithium::dilithium5;
use pqcrypto_traits::sign::{PublicKey as _, SecretKey as _};
use std::env;
use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = env::args().nth(1).unwrap_or_else(|| ".".to_string());
    let out = PathBuf::from(out_dir);
    let (pk, sk) = dilithium5::keypair();
    let pk_path = out.join("pk.bin");
    let sk_path = out.join("sk.bin");
    fs::write(&pk_path, pk.as_bytes())?;
    fs::write(&sk_path, sk.as_bytes())?;
    println!("public_key: {}", pk_path.display());
    println!("secret_key: {}", sk_path.display());
    Ok(())
}
