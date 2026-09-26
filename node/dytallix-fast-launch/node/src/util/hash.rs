use blake3;

pub fn blake3_tx_hash(data: &[u8]) -> String {
    let h = blake3::hash(data);
    format!("0x{}", hex::encode(h.as_bytes()))
}
