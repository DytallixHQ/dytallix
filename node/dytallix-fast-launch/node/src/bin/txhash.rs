use dytallix_fast_node::crypto::{canonical_json, sha3_256};
use dytallix_fast_node::types::tx::Tx;
use std::env;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: txhash <tx.json> <out.bin>");
        std::process::exit(1);
    }
    let tx_path = &args[1];
    let out_path = &args[2];
    let data = fs::read_to_string(tx_path)?;
    let tx: Tx = serde_json::from_str(&data)?;
    let bytes = canonical_json(&tx)?;
    let h = sha3_256(&bytes);
    fs::write(out_path, h)?;
    Ok(())
}
