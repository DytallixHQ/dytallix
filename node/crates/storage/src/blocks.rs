use super::tx::Transaction;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    pub height: u64,
    pub parent: String,
    pub timestamp: u64,
    pub tx_count: u32,
    pub tx_root: String,
    #[serde(default)]
    pub asset_hashes: Vec<String>,
    #[serde(default)]
    pub execution_version: u32,
    #[serde(default)]
    pub execution_state_digest: String,
    #[serde(default)]
    pub execution_receipts_digest: String,
    #[serde(default)]
    pub execution_input_digest: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub txs: Vec<Transaction>,
    pub hash: String,
}

impl Block {
    pub fn compute_hash(header: &BlockHeader, txs: &Vec<Transaction>) -> String {
        let mut hasher = Sha256::new();
        if header.execution_version != 0 {
            hasher.update(b"dytallix-development-block-v1");
            hasher.update(serde_json::to_vec(&(header, txs)).expect("block fields serialize"));
            return format!("0x{:x}", hasher.finalize());
        }
        hasher.update(header.height.to_be_bytes());
        hasher.update(header.parent.as_bytes());
        hasher.update(header.timestamp.to_be_bytes());
        for tx in txs {
            hasher.update(tx.hash.as_bytes());
        }
        // normalized 0x + lowercase hex
        format!("0x{:x}", hasher.finalize())
    }
    pub fn new(height: u64, parent: String, timestamp: u64, txs: Vec<Transaction>) -> Self {
        let header = BlockHeader {
            height,
            parent: parent.clone(),
            timestamp,
            tx_count: txs.len() as u32,
            tx_root: "".to_string(),
            asset_hashes: Vec::new(),
            execution_version: 0,
            execution_state_digest: String::new(),
            execution_receipts_digest: String::new(),
            execution_input_digest: String::new(),
        };
        let hash = Self::compute_hash(&header, &txs);
        Self { header, txs, hash }
    }
}

// Rolling TPS helper structure (ring buffer of (timestamp, tx_count))
#[derive(Default)]
pub struct TpsWindow {
    pub entries: std::collections::VecDeque<(u64, u32)>,
    pub window_secs: u64,
}
impl TpsWindow {
    pub fn new(window_secs: u64) -> Self {
        Self {
            entries: std::collections::VecDeque::new(),
            window_secs,
        }
    }
    pub fn record_block(&mut self, ts: u64, txs: u32) {
        self.entries.push_back((ts, txs));
        self.evict(ts);
    }
    fn evict(&mut self, now: u64) {
        while let Some((t, _)) = self.entries.front() {
            if now.saturating_sub(*t) > self.window_secs {
                self.entries.pop_front();
            } else {
                break;
            }
        }
    }
    pub fn rolling_tps(&mut self, now: u64) -> f64 {
        self.evict(now);
        let total_txs: u64 = self.entries.iter().map(|(_, c)| *c as u64).sum();
        let span = self.window_secs.max(1);
        (total_txs as f64) / (span as f64)
    }
}
