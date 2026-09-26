use super::blocks::Block;
use super::receipts::TxReceipt;
use super::tx::Transaction;
use rocksdb::{Options, DB};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug)]
pub struct Storage {
    pub db: DB,
    execution_lock: std::sync::Mutex<()>,
}

impl Storage {
    /// Open an existing database without creating files or accepting writes.
    /// WAL records can be read in memory. This does not acquire an exclusive
    /// application lifecycle lease or authorize a subsequent writable open.
    pub fn open_read_only(path: PathBuf) -> anyhow::Result<Self> {
        // RocksDB can create a missing directory before reporting that a
        // read-only database does not exist. Refuse before entering its API.
        anyhow::ensure!(
            std::fs::symlink_metadata(&path)?.file_type().is_dir(),
            "Read-only database path must be an existing directory"
        );
        anyhow::ensure!(
            std::fs::symlink_metadata(path.join("CURRENT"))?.file_type().is_file(),
            "Read-only database CURRENT must be an existing regular file"
        );
        let mut opts = Options::default();
        opts.create_if_missing(false);
        let db = DB::open_for_read_only(&opts, path, false)?;
        Ok(Self {
            db,
            execution_lock: std::sync::Mutex::new(()),
        })
    }

    pub fn open(path: PathBuf) -> anyhow::Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        let db = DB::open(&opts, path)?;
        Ok(Self {
            db,
            execution_lock: std::sync::Mutex::new(()),
        })
    }
    /// Serialize selected-node execution planning and commit on this storage handle.
    /// Other legacy writers do not yet participate in this boundary.
    pub fn lock_execution(&self) -> anyhow::Result<std::sync::MutexGuard<'_, ()>> {
        self.execution_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("Execution storage lock poisoned"))
    }

    pub fn put_block(&self, block: &Block, receipts: &[TxReceipt]) -> anyhow::Result<()> {
        let _guard = self.lock_execution()?;
        // Any consensus record reserves this store for consensus block settlement.
        // A missing config or head must not enable legacy block writes.
        let prefix = b"consensus:";
        if let Some(entry) = self
            .db
            .iterator(rocksdb::IteratorMode::From(
                prefix,
                rocksdb::Direction::Forward,
            ))
            .next()
        {
            let (key, _) = entry?;
            anyhow::ensure!(
                !key.starts_with(prefix),
                "Consensus state requires consensus block settlement"
            );
        }
        anyhow::ensure!(
            self.db.get("execution:block:v1:head")?.is_none(),
            "Use block settlement for this database"
        );
        eprintln!(
            "INFO  [Storage] Committing block #{} with {} transaction(s) (hash: {})",
            block.header.height,
            block.txs.len(),
            &block.hash[..16]
        );
        if !block.txs.is_empty() {
            eprintln!(
                "INFO  [Storage] Block includes tx {} (from: {}, amount: {})",
                &block.txs[0].hash[..16],
                &block.txs[0].from[..12],
                block.txs[0].amount
            );
        }

        // Use serde_json instead of bincode for blocks because TxMessage enum uses #[serde(tag = "type")]
        // which is not supported by bincode's deserialize_any
        let serialized = serde_json::to_vec(block)?;
        eprintln!(
            "INFO  [Storage] Block #{} serialized ({} bytes)",
            block.header.height,
            serialized.len()
        );

        // Try deserializing immediately to verify
        match serde_json::from_slice::<Block>(&serialized) {
            Ok(deserialized) => {
                if deserialized.txs.len() != block.txs.len() {
                    eprintln!(
                        "WARN  [Storage] Transaction count mismatch detected: expected {}, got {}",
                        block.txs.len(),
                        deserialized.txs.len()
                    );
                }
            }
            Err(e) => {
                eprintln!("ERROR [Storage] Block serialization verification failed");
                eprintln!("ERROR [Storage] Serialization error: {:?}", e);
                eprintln!(
                    "ERROR [Storage] Block #{} with {} transactions ({} bytes)",
                    block.header.height,
                    block.txs.len(),
                    serialized.len()
                );
            }
        }

        self.db
            .put(format!("blk_hash:{}", block.hash), serialized)?;
        self.db.put(
            format!("blk_num:{:016x}", block.header.height),
            block.hash.as_bytes(),
        )?;
        self.db
            .put("meta:height", block.header.height.to_be_bytes())?;
        self.db.put("meta:best_hash", block.hash.as_bytes())?;
        for r in receipts {
            self.db
                .put(format!("rcpt:{}", r.tx_hash), serde_json::to_vec(r)?)?;
        }
        Ok(())
    }
    pub fn height(&self) -> u64 {
        self.db
            .get("meta:height")
            .ok()
            .flatten()
            .and_then(|v| {
                if v.len() == 8 {
                    let mut arr = [0u8; 8];
                    arr.copy_from_slice(&v);
                    Some(u64::from_be_bytes(arr))
                } else {
                    None
                }
            })
            .unwrap_or(0)
    }
    pub fn best_hash(&self) -> String {
        self.db
            .get("meta:best_hash")
            .ok()
            .flatten()
            .map(|v| String::from_utf8_lossy(&v).to_string())
            .unwrap_or_else(|| "genesis".to_string())
    }
    pub fn get_block_by_height(&self, h: u64) -> Option<Block> {
        let hash = self.db.get(format!("blk_num:{h:016x}")).ok().flatten()?;
        self.get_block_by_hash(String::from_utf8_lossy(&hash).to_string())
    }
    pub fn get_block_by_hash(&self, hash: String) -> Option<Block> {
        let raw_data = self.db.get(format!("blk_hash:{hash}")).ok().flatten();

        if let Some(ref data) = raw_data {
            eprintln!(
                "DEBUG [Storage] Loading block {} ({} bytes)",
                &hash[..16],
                data.len()
            );
        } else {
            eprintln!("WARN  [Storage] Block not found: {}", &hash[..16]);
            return None;
        }

        let block: Option<Block> = raw_data.and_then(|b| {
            // Try JSON first (new format), fallback to bincode for old blocks
            match serde_json::from_slice::<Block>(&b) {
                Ok(block) => Some(block),
                Err(json_err) => {
                    eprintln!("DEBUG [Storage] Attempting legacy deserialization...");
                    match bincode::deserialize::<Block>(&b) {
                        Ok(block) => Some(block),
                        Err(bincode_err) => {
                            eprintln!("ERROR [Storage] Block deserialization failed");
                            eprintln!("ERROR [Storage] JSON error: {}", json_err);
                            eprintln!("ERROR [Storage] Bincode error: {}", bincode_err);
                            None
                        }
                    }
                }
            }
        });

        if let Some(ref b) = block {
            eprintln!(
                "INFO  [Storage] Loaded block #{} with {} transaction(s)",
                b.header.height,
                b.txs.len()
            );
            if !b.txs.is_empty() {
                eprintln!(
                    "DEBUG [Storage] First tx: {} (from: {}, amount: {})",
                    &b.txs[0].hash[..16],
                    &b.txs[0].from[..12],
                    b.txs[0].amount
                );
            }
        }

        block
    }
    pub fn get_transaction_record(
        &self,
        hash: &str,
    ) -> anyhow::Result<Option<super::transaction_record::TransactionRecord>> {
        self.db
            .get(format!("tx:{hash}"))?
            .map(|bytes| super::transaction_record::TransactionRecord::decode(hash, &bytes))
            .transpose()
    }
    /// Plan immutable transaction bytes. Caller holds the execution lock.
    pub fn planned_transaction_record(
        &self,
        tx: &Transaction,
        envelope: Option<super::transaction_record::SignedEnvelope>,
    ) -> anyhow::Result<Vec<u8>> {
        if let Some(record) = self.get_transaction_record(&tx.hash)? {
            anyhow::ensure!(record.matches(tx)?, "Stored transaction body differs");
            if let Some(envelope) = envelope {
                anyhow::ensure!(
                    record.signed_envelope.as_ref() == Some(&envelope),
                    "Stored signed envelope differs"
                );
            }
            return record.encode();
        }
        super::transaction_record::TransactionRecord::new(tx.clone(), envelope)?.encode()
    }
    pub fn put_tx(&self, tx: &Transaction) -> anyhow::Result<()> {
        let _guard = self.lock_execution()?;
        let bytes = self.planned_transaction_record(tx, None)?;
        let receipt = self
            .db
            .get(format!("rcpt:{}", tx.hash))?
            .map(|raw| serde_json::from_slice::<TxReceipt>(&raw))
            .transpose()?;
        if receipt.is_some_and(|r| r.status != super::receipts::TxStatus::Pending) {
            anyhow::ensure!(
                self.get_transaction_record(&tx.hash)?.is_some(),
                "Settled transaction record is missing; recovery required"
            );
        }
        let mut options = rocksdb::WriteOptions::default();
        options.set_sync(true);
        self.db
            .put_opt(format!("tx:{}", tx.hash), bytes, &options)?;
        Ok(())
    }
    /// Persist admission before the producer can observe the mempool entry.
    pub fn put_pending_transaction(&self, tx: &Transaction) -> anyhow::Result<()> {
        self.put_pending_signed_transaction(tx, None)
    }
    pub fn put_pending_signed_transaction(
        &self,
        tx: &Transaction,
        envelope: Option<super::transaction_record::SignedEnvelope>,
    ) -> anyhow::Result<()> {
        let _guard = self.lock_execution()?;
        let key = format!("rcpt:{}", tx.hash);
        if let Some(raw) = self.db.get(&key)? {
            let receipt: TxReceipt = serde_json::from_slice(&raw)?;
            anyhow::ensure!(
                receipt.status == super::receipts::TxStatus::Pending,
                "Transaction already settled"
            );
        }
        anyhow::ensure!(
            self.db
                .get(format!("execution:v1:receipt:{}", tx.hash))?
                .is_none(),
            "Transaction already settled"
        );
        let bytes = self.planned_transaction_record(tx, envelope)?;
        let mut batch = rocksdb::WriteBatch::default();
        batch.put(format!("tx:{}", tx.hash), bytes);
        batch.put(key, serde_json::to_vec(&TxReceipt::pending(tx))?);
        let mut options = rocksdb::WriteOptions::default();
        options.set_sync(true);
        self.db.write_opt(batch, &options)?;
        Ok(())
    }
    pub fn put_pending_receipt(&self, r: &TxReceipt) -> anyhow::Result<()> {
        let _guard = self.lock_execution()?;
        let key = format!("rcpt:{}", r.tx_hash);
        if let Some(raw) = self.db.get(&key)? {
            let receipt: TxReceipt = serde_json::from_slice(&raw)?;
            if receipt.status != super::receipts::TxStatus::Pending {
                anyhow::ensure!(
                    serde_json::to_vec(&receipt)? == serde_json::to_vec(r)?,
                    "Cannot overwrite a final receipt"
                );
                return Ok(());
            }
        }
        anyhow::ensure!(
            self.db
                .get(format!("execution:v1:receipt:{}", r.tx_hash))?
                .is_none(),
            "Cannot overwrite a settlement receipt"
        );
        self.db.put(key, serde_json::to_vec(r)?)?;
        Ok(())
    }
    pub fn get_receipt(&self, hash: &str) -> Option<TxReceipt> {
        let k = format!("rcpt:{hash}");
        match self.db.get(&k) {
            Ok(Some(raw)) => {
                if raw.is_empty() {
                    return None;
                }
                serde_json::from_slice(&raw)
                    .or_else(|_| bincode::deserialize(&raw))
                    .ok()
            }
            Ok(None) => None,
            Err(_) => None,
        }
    }
    pub fn get_chain_id(&self) -> Option<String> {
        self.db
            .get("meta:chain_id")
            .ok()
            .flatten()
            .map(|v| String::from_utf8_lossy(&v).to_string())
    }
    pub fn set_chain_id(&self, id: &str) -> anyhow::Result<()> {
        self.db.put("meta:chain_id", id.as_bytes())?;
        Ok(())
    }

    // Durable balance + nonce methods
    /// Get multi-denomination balances for an address
    pub fn get_balances_db(&self, addr: &str) -> BTreeMap<String, u128> {
        self.db
            .get(format!("acct:balances:{addr}"))
            .ok()
            .flatten()
            .and_then(|b| bincode::deserialize::<BTreeMap<String, u128>>(&b).ok())
            .unwrap_or_default()
    }

    /// Set multi-denomination balances for an address
    pub fn set_balances_db(
        &self,
        addr: &str,
        balances: &BTreeMap<String, u128>,
    ) -> anyhow::Result<()> {
        self.db.put(
            format!("acct:balances:{addr}"),
            bincode::serialize(balances)?,
        )?;
        Ok(())
    }

    /// Legacy single balance getter (for backward compatibility)
    pub fn get_balance_db(&self, addr: &str) -> u128 {
        // Check if new multi-denom format exists first
        let balances = self.get_balances_db(addr);
        if !balances.is_empty() {
            return balances.get("udgt").copied().unwrap_or(0);
        }

        // Fallback to legacy single balance format
        self.db
            .get(format!("acct:bal:{addr}"))
            .ok()
            .flatten()
            .and_then(|b| bincode::deserialize::<u128>(&b).ok())
            .unwrap_or(0)
    }

    /// Legacy single balance setter (for backward compatibility)
    pub fn set_balance_db(&self, addr: &str, bal: u128) -> anyhow::Result<()> {
        // Migrate to multi-denom format
        let mut balances = self.get_balances_db(addr);
        balances.insert("udgt".to_string(), bal);
        self.set_balances_db(addr, &balances)?;

        // Also keep legacy format for compatibility during migration
        self.db
            .put(format!("acct:bal:{addr}"), bincode::serialize(&bal)?)?;
        Ok(())
    }
    pub fn get_nonce_db(&self, addr: &str) -> u64 {
        self.db
            .get(format!("acct:nonce:{addr}"))
            .ok()
            .flatten()
            .and_then(|b| bincode::deserialize::<u64>(&b).ok())
            .unwrap_or(0)
    }
    pub fn set_nonce_db(&self, addr: &str, nonce: u64) -> anyhow::Result<()> {
        self.db
            .put(format!("acct:nonce:{addr}"), bincode::serialize(&nonce)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod consensus_mode_tests {
    use super::*;

    fn snapshot(storage: &Storage) -> BTreeMap<Vec<u8>, Vec<u8>> {
        storage
            .db
            .iterator(rocksdb::IteratorMode::Start)
            .map(|entry| {
                let (key, value) = entry.unwrap();
                (key.to_vec(), value.to_vec())
            })
            .collect()
    }

    #[test]
    fn consensus_and_orphan_records_reject_legacy_block_and_receipt_writes() {
        for key in [
            "consensus:v1:config",
            "consensus:v1:head",
            "consensus:unknown:orphan",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let storage = Storage::open(dir.path().join("db")).unwrap();
            // Presence is sufficient even when the record cannot be decoded.
            storage.db.put(key, b"").unwrap();
            let tx = Transaction::new("receipt-fixture", "alice", "bob", 1, 1, 0, None);
            let receipt = TxReceipt::pending(&tx);
            let block = Block::new(1, "genesis".into(), 10, Vec::new());
            let before = snapshot(&storage);
            let error = storage.put_block(&block, &[receipt]).unwrap_err();
            assert!(error.to_string().contains("consensus block settlement"));
            assert_eq!(snapshot(&storage), before);
            assert_eq!(storage.height(), 0);
            assert_eq!(storage.best_hash(), "genesis");
        }
    }

    #[test]
    fn unrelated_prefix_does_not_disable_legacy_block_writer() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path().join("db")).unwrap();
        storage.db.put("consensus_other", b"unrelated").unwrap();
        let block = Block::new(1, "genesis".into(), 10, Vec::new());
        storage.put_block(&block, &[]).unwrap();
        assert_eq!(storage.height(), 1);
        assert_eq!(storage.best_hash(), block.hash);
    }
}
