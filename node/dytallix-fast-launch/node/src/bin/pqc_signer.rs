use base64::{engine::general_purpose::STANDARD as B64, Engine};
use fips204::ml_dsa_65;
use fips204::traits::{KeyGen, SerDes, Signer};
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: pqc_signer <command> [args...]");
        eprintln!("Commands:");
        eprintln!("  keygen              -> prints sk_b64 pk_b64");
        eprintln!("  sign <sk_b64> <msg> [ctx] -> prints sig_b64 (ctx defaults to empty)");
        std::process::exit(1);
    }

    let command = &args[1];

    match command.as_str() {
        "keygen" => {
            let (pk, sk) = ml_dsa_65::KG::try_keygen().unwrap();
            println!(
                "{} {}",
                B64.encode(sk.into_bytes()),
                B64.encode(pk.into_bytes())
            );
        }
        "sign" => {
            if args.len() < 4 {
                eprintln!("Usage: pqc_signer sign <sk_b64> <msg> [ctx]");
                std::process::exit(1);
            }
            let sk_b64 = &args[2];
            let msg = &args[3];
            let ctx = args.get(4).map(|s| s.as_bytes()).unwrap_or(b"");

            let sk_bytes_vec = B64.decode(sk_b64).expect("Invalid base64 SK");
            if sk_bytes_vec.len() != ml_dsa_65::SK_LEN {
                eprintln!(
                    "Invalid SK length: expected {}, got {}",
                    ml_dsa_65::SK_LEN,
                    sk_bytes_vec.len()
                );
                std::process::exit(1);
            }
            let mut sk_arr = [0u8; ml_dsa_65::SK_LEN];
            sk_arr.copy_from_slice(&sk_bytes_vec);

            let sk = ml_dsa_65::PrivateKey::try_from_bytes(sk_arr).expect("Invalid SK bytes");

            // ML-DSA supports an optional context string for domain separation.
            // For transaction-style signatures, the node verification path defaults to an empty context.
            // For oracle signatures, use ctx="dytallix-oracle" to match the oracle verifier.
            let sig = sk.try_sign(msg.as_bytes(), ctx).expect("Signing failed");
            println!("{}", B64.encode(sig));
        }
        _ => {
            eprintln!("Unknown command: {}", command);
            std::process::exit(1);
        }
    }
}
