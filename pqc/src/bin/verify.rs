use pqcrypto_dilithium::dilithium5;
use pqcrypto_traits::sign::{PublicKey as _, SignedMessage as _};
use std::env;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() != 3 {
        eprintln!("Usage: pqc-verify <public.key> <input.bin> <signature.hex>");
        std::process::exit(1);
    }
    let pk_bytes = fs::read(&args[0])?;
    let msg = fs::read(&args[1])?;
    let sig_hex = fs::read_to_string(&args[2])?;
    let sig = hex::decode(sig_hex.trim())?;
    let pk = dilithium5::PublicKey::from_bytes(&pk_bytes)
        .map_err(|_| "invalid Dilithium5 public key bytes")?;
    let sm = dilithium5::SignedMessage::from_bytes(&sig)
        .map_err(|_| "invalid Dilithium5 signature bytes")?;
    match dilithium5::open(&sm, &pk) {
        Ok(opened) if opened == msg => {
            println!("verified:true");
            Ok(())
        }
        _ => {
            println!("verified:false");
            std::process::exit(2)
        }
    }
}
