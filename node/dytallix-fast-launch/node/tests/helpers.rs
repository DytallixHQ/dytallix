use dytallix_fast_node::storage::tx::Transaction;
use dytallix_fast_node::util::hash::blake3_tx_hash;

#[allow(dead_code)]
pub fn dummy_sig() -> String {
    "0x".to_string() + &"00".repeat(64)
}

#[allow(dead_code)]
pub fn dummy_hash(tag: &str) -> String {
    blake3_tx_hash(tag.as_bytes())
}

#[allow(dead_code)]
pub fn make_tx(from: &str, to: &str, amount: u128, fee: u128, nonce: u64) -> Transaction {
    let hash = blake3_tx_hash(format!("{from}:{to}:{amount}:{fee}:{nonce}").as_bytes());
    Transaction::new(
        hash,
        from.to_string(),
        to.to_string(),
        amount,
        fee,
        nonce,
        Some(dummy_sig()),
    )
}
