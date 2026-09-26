use pqcrypto_dilithium::dilithium5;
use pqcrypto_traits::sign::{SecretKey, SignedMessage as _};
use std::env;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        eprintln!("Usage: pqc-sign <private.key> <input.bin>");
        std::process::exit(1);
    }
    let sk_path = &args[0];
    let input_path = &args[1];
    let sk_bytes = fs::read(sk_path)?;
    let msg = fs::read(input_path)?;

    let sk = dilithium5::SecretKey::from_bytes(&sk_bytes)
        .map_err(|_| "invalid Dilithium5 secret key bytes")?;
    let sm = dilithium5::sign(&msg, &sk);
    let hex = hex::encode(sm.as_bytes());
    println!("{hex}");
    Ok(())
}
