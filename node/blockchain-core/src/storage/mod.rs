use crate::types::Amount as Tokens;
use crate::types::{
    AccountState, Address, Amount, Block, BlockNumber, Timestamp, Transaction,
    Transaction as TxEnum, TxReceipt,
};
use rocksdb::{IteratorMode, Options, WriteBatch, WriteOptions, DB};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Transaction receipt persisted for lookup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionReceipt {
    pub hash: String,
    pub status: String, // success | failed
    pub block_number: BlockNumber,
    pub index: u32,
    pub fee: Amount,
    pub from: Address,
    pub to: Option<Address>,
    pub amount: Option<Amount>,
    pub nonce: u64,
    pub error: Option<String>,
}

/// Smart Contract State Storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractState {
    pub code: Vec<u8>,
    pub storage: HashMap<Vec<u8>, Vec<u8>>, // removed stray backslash
    pub balance: Amount,
    pub metadata: ContractMetadata,
}

/// Smart Contract Metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractMetadata {
    pub deployer: Address,
    pub deployment_block: BlockNumber,
    pub last_modified: Timestamp,
    pub call_count: u64,
}

impl ContractState {
    pub fn _new(
        code: Vec<u8>,
        deployer: Address,
        deployment_block: BlockNumber,
        timestamp: Timestamp,
    ) -> Self {
        Self {
            code,
            storage: HashMap::new(),
            balance: 0,
            metadata: ContractMetadata {
                deployer,
                deployment_block,
                last_modified: timestamp,
                call_count: 0,
            },
        }
    }
    pub fn _set_storage(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.storage.insert(key, value);
    }
    pub fn _get_storage(&self, key: &[u8]) -> Option<&Vec<u8>> {
        self.storage.get(key)
    }
    pub fn _increment_calls(&mut self) {
        self.metadata.call_count += 1;
    }
    pub fn _update_timestamp(&mut self, timestamp: Timestamp) {
        self.metadata.last_modified = timestamp;
    }
}

/// Persistent storage manager (RocksDB)
#[derive(Debug)]
pub struct StorageManager {
    db: Arc<DB>,
    // lightweight in-memory cache for hot account states (optional)
    _account_cache: Arc<RwLock<HashMap<Address, AccountState>>>, // underscore
}

const META_CHAIN_ID: &str = "meta:chain_id";
const META_HEIGHT: &str = "meta:height";
const META_BEST_HASH: &str = "meta:best_hash";
const META_GENESIS_IMPORT: &str = "meta:genesis_import_v1";

#[derive(Deserialize)]
struct GenesisAllocationDocument {
    #[serde(alias = "allocations")]
    dgt_allocations: Vec<crate::genesis::DGTAllocation>,
}

pub type _KVSnapshotResult = Result<Vec<(Vec<u8>, Vec<u8>)>, Box<dyn std::error::Error>>;

impl StorageManager {
    /// Open or create storage at data_dir. If empty, will initialize genesis (balances)
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // Fallback to env path if provided
        let data_dir = std::env::var("DYT_DATA_DIR").unwrap_or_else(|_| "./data".to_string());
        let chain_id = std::env::var("DYT_CHAIN_ID").unwrap_or_else(|_| "dyt-local-1".to_string());
        let explicit_genesis = std::env::var("DYT_GENESIS_FILE").ok();
        let genesis_path = explicit_genesis.as_deref().unwrap_or("genesisBlock.json");
        let selected_path = if explicit_genesis.is_some() || Path::new(genesis_path).exists() {
            Some(Path::new(genesis_path))
        } else {
            None
        };
        Self::open(Path::new(&data_dir), &chain_id, selected_path).await
    }

    /// Open explicitly selected storage without reading process-wide configuration.
    pub async fn open(
        data_dir: &Path,
        chain_id: &str,
        genesis_path: Option<&Path>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        std::fs::create_dir_all(data_dir)?;
        let mut opts = Options::default();
        opts.create_if_missing(true);
        let db_path = data_dir.join("node.db");
        let db = DB::open(&opts, db_path)?;
        let mgr = Self {
            db: Arc::new(db),
            _account_cache: Arc::new(RwLock::new(HashMap::new())),
        };
        mgr.init_genesis(chain_id, genesis_path)?;
        Ok(mgr)
    }

    /// Import allocations once. The file digest identifies the exact supplied document.
    fn init_genesis(
        &self,
        chain_id: &str,
        genesis_path: Option<&Path>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if chain_id.trim().is_empty() {
            return Err("Chain ID must be nonempty".into());
        }
        let text = genesis_path.map(std::fs::read).transpose()?;
        let mut hasher = Sha3_256::new();
        hasher.update(b"dytallix-core-allocation-import-v1");
        match &text {
            Some(bytes) => {
                hasher.update([1]);
                hasher.update(bytes);
            }
            None => {
                hasher.update([0]);
            }
        }
        let digest = hasher.finalize().to_vec();
        if let Some(stored_digest) = self.db.get(META_GENESIS_IMPORT)? {
            if stored_digest != digest {
                return Err("Genesis document differs from the initialized database".into());
            }
            if self.db.get(META_CHAIN_ID)?.as_deref() != Some(chain_id.as_bytes()) {
                return Err("Chain ID mismatch or missing initialized chain ID".into());
            }
            if self.db.get(META_HEIGHT)?.is_none() || self.db.get(META_BEST_HASH)?.is_none() {
                return Err("Initialized database is missing genesis metadata".into());
            }
            return Ok(());
        }
        if let Some(entry) = self.db.iterator(IteratorMode::Start).next() {
            entry?;
            return Err(
                "Existing database has no genesis import marker; explicit migration is required"
                    .into(),
            );
        }
        let allocations = match text {
            Some(bytes) => {
                serde_json::from_slice::<GenesisAllocationDocument>(&bytes)?.dgt_allocations
            }
            None => Vec::new(),
        };
        crate::genesis::validate_allocations(&allocations)?;
        let mut batch = WriteBatch::default();
        for allocation in allocations {
            // Preserve this importer's address family, but never silently skip an entry.
            if !allocation.address.starts_with("dyt1") || allocation.address.len() <= 4 {
                return Err("Allocation importer requires dyt1 addresses".into());
            }
            let account = AccountState {
                balance: allocation.amount,
                ..Default::default()
            };
            batch.put(
                Self::account_key(&allocation.address),
                bincode::serialize(&account)?,
            );
        }
        batch.put(META_CHAIN_ID, chain_id.as_bytes());
        batch.put(META_HEIGHT, 0u64.to_be_bytes());
        batch.put(META_BEST_HASH, vec![b'0'; 64]);
        batch.put(META_GENESIS_IMPORT, digest);
        let mut options = WriteOptions::default();
        options.set_sync(true);
        self.db.write_opt(batch, &options)?;
        Ok(())
    }

    fn _set_height(&self, h: u64) -> Result<(), Box<dyn std::error::Error>> {
        self.db.put(META_HEIGHT, h.to_be_bytes())?;
        Ok(())
    }
    pub fn get_height(&self) -> Result<u64, Box<dyn std::error::Error>> {
        Ok(self
            .db
            .get(META_HEIGHT)?
            .map(|b| {
                let mut arr = [0u8; 8];
                arr.copy_from_slice(&b);
                u64::from_be_bytes(arr)
            })
            .unwrap_or(0))
    }
    fn _set_best_hash(&self, hash: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.db.put(META_BEST_HASH, hash.as_bytes())?;
        Ok(())
    }
    pub fn _get_best_hash(&self) -> Result<String, Box<dyn std::error::Error>> {
        Ok(self
            .db
            .get(META_BEST_HASH)?
            .map(|b| String::from_utf8_lossy(&b).to_string())
            .unwrap_or_else(|| "0".repeat(64)))
    }

    fn account_key(address: &str) -> String {
        format!("acct:{address}")
    }
    fn block_hash_key(hash: &str) -> String {
        format!("blk_hash:{hash}")
    }
    fn block_num_key(num: u64) -> String {
        format!("blk_num:{num:016x}")
    }
    fn tx_key(hash: &str) -> String {
        format!("tx:{hash}")
    }
    fn rcpt_key(hash: &str) -> String {
        format!("rcpt:{hash}")
    }
    fn _contract_key(address: &str) -> String {
        format!("contract:{address}")
    }
    fn receipt_key(hash: &str) -> String {
        format!("receipt:{hash}")
    }

    pub fn store_account_state(
        &self,
        address: &str,
        state: &AccountState,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let enc = bincode::serialize(state)?;
        self.db.put(Self::account_key(address), enc)?;
        Ok(())
    }
    pub fn get_account_state(
        &self,
        address: &str,
    ) -> Result<AccountState, Box<dyn std::error::Error>> {
        if let Some(v) = self.db.get(Self::account_key(address))? {
            Ok(bincode::deserialize(&v)?)
        } else {
            Ok(AccountState::default())
        }
    }

    /// External API: Get address balance
    pub async fn get_address_balance(
        &self,
        address: &str,
    ) -> Result<Tokens, Box<dyn std::error::Error>> {
        Ok(self.get_account_state(address)?.balance)
    }
    /// External API: Set (overwrite) address balance (used only in tests / genesis)
    pub async fn _set_address_balance(
        &self,
        address: &str,
        balance: Tokens,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut st = self.get_account_state(address)?;
        st.balance = balance;
        self.store_account_state(address, &st)?;
        Ok(())
    }
    pub async fn get_address_nonce(
        &self,
        address: &str,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        Ok(self.get_account_state(address)?.nonce)
    }

    /// Apply a transfer inclusion (validates nonce & balances) and mutate state
    pub fn _apply_transfer(&self, tx: &crate::types::TransferTransaction) -> Result<(), String> {
        let mut sender = self
            .get_account_state(&tx.from)
            .map_err(|e| e.to_string())?;
        let mut recipient = self.get_account_state(&tx.to).map_err(|e| e.to_string())?;
        if sender.nonce != tx.nonce {
            return Err("nonce_mismatch".into());
        }
        let total = tx.amount.checked_add(tx.fee).ok_or("overflow")?;
        if sender.balance < total {
            return Err("insufficient_balance".into());
        }
        sender.balance -= total;
        sender.nonce += 1;
        recipient.balance = recipient.balance.saturating_add(tx.amount);
        self.store_account_state(&tx.from, &sender)
            .map_err(|e| e.to_string())?;
        self.store_account_state(&tx.to, &recipient)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Store block + its transactions + receipts, update metadata
    pub fn _store_block(&self, block: &Block) -> Result<(), Box<dyn std::error::Error>> {
        let hash = block.hash();
        let height = block.header.number;
        // Store each transaction & receipt placeholder (success assumed if already applied)
        for (idx, tx) in block.transactions.iter().enumerate() {
            let tx_hash = tx.hash();
            if self.db.get(Self::tx_key(&tx_hash))?.is_none() {
                self.db
                    .put(Self::tx_key(&tx_hash), bincode::serialize(tx)?)?;
            }
            if self.db.get(Self::receipt_key(&tx_hash))?.is_none() {
                let (from, to, amount, fee, nonce) = match tx {
                    TxEnum::Transfer(t) => (
                        t.from.clone(),
                        Some(t.to.clone()),
                        Some(t.amount),
                        t.fee,
                        t.nonce,
                    ),
                    _ => (tx.from().clone(), None, None, tx.fee(), tx.nonce()),
                };
                let receipt = TransactionReceipt {
                    hash: tx_hash.clone(),
                    status: "success".into(),
                    block_number: height,
                    index: idx as u32,
                    fee,
                    from,
                    to,
                    amount,
                    nonce,
                    error: None,
                };
                self.db
                    .put(Self::receipt_key(&tx_hash), bincode::serialize(&receipt)?)?;
            }
        }
        self.db
            .put(Self::block_hash_key(&hash), bincode::serialize(block)?)?;
        self.db.put(Self::block_num_key(height), hash.as_bytes())?;
        self._set_height(height)?;
        self._set_best_hash(&hash)?;
        Ok(())
    }

    pub async fn get_block_by_height(
        &self,
        height: u64,
    ) -> Result<Option<Block>, Box<dyn std::error::Error>> {
        if let Some(hbytes) = self.db.get(Self::block_num_key(height))? {
            let hash = String::from_utf8(hbytes)?;
            self.get_block_by_hash(&hash).await
        } else {
            Ok(None)
        }
    }
    pub async fn get_block_by_hash(
        &self,
        hash: &str,
    ) -> Result<Option<Block>, Box<dyn std::error::Error>> {
        if let Some(raw) = self.db.get(Self::block_hash_key(hash))? {
            let blk: Block = bincode::deserialize(&raw)?;
            Ok(Some(blk))
        } else {
            Ok(None)
        }
    }

    pub async fn list_blocks_desc(
        &self,
        limit: usize,
        from: Option<u64>,
    ) -> Result<Vec<Block>, Box<dyn std::error::Error>> {
        let current = from.unwrap_or(self.get_height()?);
        let mut out = Vec::new();
        let mut h = current;
        loop {
            if out.len() >= limit {
                break;
            }
            if let Some(b) = self.get_block_by_height(h).await? {
                out.push(b);
            }
            if h == 0 {
                break;
            }
            h -= 1; // safe because we break at 0
        }
        Ok(out)
    }

    pub async fn get_transaction_by_hash(
        &self,
        hash: &str,
    ) -> Result<Option<Transaction>, Box<dyn std::error::Error>> {
        if let Some(raw) = self.db.get(Self::tx_key(hash))? {
            Ok(Some(bincode::deserialize(&raw)?))
        } else {
            Ok(None)
        }
    }
    pub async fn store_transaction(
        &self,
        tx: &Transaction,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let key = Self::tx_key(&tx.hash());
        if self.db.get(&key)?.is_none() {
            self.db.put(key, bincode::serialize(tx)?)?;
        }
        Ok(())
    }

    /// Store a transaction receipt under receipt:{hash}
    pub async fn _store_receipt(
        &self,
        receipt: &TxReceipt,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let key = Self::receipt_key(&receipt.tx_hash);
        self.db.put(key, bincode::serialize(receipt)?)?;
        Ok(())
    }

    /// Fetch a transaction receipt by tx hash (tries new prefix, falls back to legacy rcpt:)
    pub async fn get_receipt(
        &self,
        hash: &str,
    ) -> Result<Option<TxReceipt>, Box<dyn std::error::Error>> {
        if let Some(raw) = self.db.get(Self::receipt_key(hash))? {
            return Ok(Some(bincode::deserialize(&raw)?));
        }
        if let Some(raw) = self.db.get(Self::rcpt_key(hash))? {
            return Ok(Some(bincode::deserialize(&raw)?));
        }
        Ok(None)
    }

    /// Store a full contract state (code + storage + metadata)
    pub async fn _store_contract(
        &self,
        address: &str,
        state: &ContractState,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let enc = bincode::serialize(state)?;
        self.db.put(Self::_contract_key(address), enc)?;
        Ok(())
    }

    /// Check whether a contract exists
    pub async fn _contract_exists(
        &self,
        address: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(self.db.get(Self::_contract_key(address))?.is_some())
    }

    /// Retrieve a contract state
    pub async fn _get_contract(
        &self,
        address: &str,
    ) -> Result<Option<ContractState>, Box<dyn std::error::Error>> {
        if let Some(raw) = self.db.get(Self::_contract_key(address))? {
            Ok(Some(bincode::deserialize(&raw)?))
        } else {
            Ok(None)
        }
    }

    /// Generic put helper (used by runtime persistence)
    pub async fn _put(&self, key: &[u8], value: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        self.db.put(key, value)?;
        Ok(())
    }
    /// Generic get helper (used by runtime persistence)
    pub async fn _get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
        Ok(self.db.get(key)?)
    }

    /// Legacy compatibility (was in-memory). Now uses RocksDB key prefixes.
    pub async fn _clear(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Not efficient; for tests only.
        self.db.flush()?; // leave data (full deletion would require destroying DB)
        Ok(())
    }

    /// Destroy RocksDB at path for tests/dev only (closes and deletes underlying directory)
    pub fn _destroy_for_tests(path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        if Path::new(path).exists() {
            drop(opts); // ensure no open handles (caller must drop StorageManager before calling)
            rocksdb::DB::destroy(&Options::default(), Path::new(path).join("node.db"))?;
        }
        Ok(())
    }

    /// Snapshot all key/value pairs relevant for state root commitment.
    /// Currently includes all RocksDB entries except meta:* keys.
    pub fn _snapshot_kv(&self) -> _KVSnapshotResult {
        use rocksdb::IteratorMode;
        let mut out = Vec::new();
        let iter = self.db.iterator(IteratorMode::Start);
        for (k, v) in iter.flatten() {
            // Exclude meta & ephemeral prefixes from state commitment
            if k.starts_with(b"meta:") {
                continue;
            }
            if k.starts_with(b"rcpt:") {
                continue;
            } // legacy receipts (non-consensus)
            if k.starts_with(b"receipt:") {
                continue;
            } // volatile per-tx receipts
            if k.starts_with(b"tx:") {
                continue;
            } // mempool / tx objects
            out.push((k.to_vec(), v.to_vec()));
        }
        // Canonical sort lexicographically by key bytes
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }
    fn _receipt_height_index_key(height: u64, index: u32) -> String {
        format!("rcpi:{height:016x}:{index}")
    }
    fn _receipt_tx_lookup_key(tx_hash: &str) -> String {
        format!("rcpx:{tx_hash}")
    }

    /// Store receipts in new indexed form (height/index and tx_hash -> (height,index))
    pub fn _store_receipts_indexed(
        &self,
        height: u64,
        receipts: &[TxReceipt],
    ) -> Result<(), Box<dyn std::error::Error>> {
        for (i, r) in receipts.iter().enumerate() {
            let key_hi = Self::_receipt_height_index_key(height, i as u32);
            if self.db.get(&key_hi)?.is_none() {
                self.db.put(key_hi.as_bytes(), bincode::serialize(r)?)?;
            }
            let key_tx = Self::_receipt_tx_lookup_key(&r.tx_hash);
            if self.db.get(&key_tx)?.is_none() {
                self.db
                    .put(key_tx.as_bytes(), bincode::serialize(&(height, i as u32))?)?;
            }
            // Also keep backward compatibility single-key receipt
            let key = Self::receipt_key(&r.tx_hash);
            if self.db.get(&key)?.is_none() {
                self.db.put(key, bincode::serialize(r)?)?;
            }
        }
        Ok(())
    }

    /// Lookup receipt via index map then load
    pub fn _get_receipt_via_index(
        &self,
        tx_hash: &str,
    ) -> Result<Option<TxReceipt>, Box<dyn std::error::Error>> {
        if let Some(raw_idx) = self.db.get(Self::_receipt_tx_lookup_key(tx_hash))? {
            let (h, idx): (u64, u32) = bincode::deserialize(&raw_idx)?;
            let key_hi = Self::_receipt_height_index_key(h, idx);
            if let Some(raw_r) = self.db.get(key_hi)? {
                return Ok(Some(bincode::deserialize(&raw_r)?));
            }
        }
        // fallback to legacy direct storage (synchronous)
        if let Some(raw) = self.db.get(Self::receipt_key(tx_hash))? {
            return Ok(Some(bincode::deserialize(&raw)?));
        }
        if let Some(raw) = self.db.get(Self::rcpt_key(tx_hash))? {
            return Ok(Some(bincode::deserialize(&raw)?));
        }
        Ok(None)
    }
}

// --- Helper: build a simple block from transactions (used by background producer) ---
// Refactored: caller must supply precomputed state_root (from injected snapshot) and timestamp.
pub fn _build_block_with_state(
    parent_hash: String,
    number: u64,
    txs: Vec<Transaction>,
    validator: Address,
    state_root: String,
    timestamp: u64,
) -> Block {
    let transactions_root = crate::types::BlockHeader::calculate_transactions_root(&txs);
    let signature = crate::types::PQCBlockSignature {
        signature: dytallix_pqc::Signature {
            data: vec![],
            algorithm: dytallix_pqc::SignatureAlgorithm::Dilithium5,
        },
        public_key: vec![],
    };
    let header = crate::types::BlockHeader {
        number,
        parent_hash,
        transactions_root,
        state_root,
        timestamp,
        validator,
        signature,
        nonce: 0,
    };
    Block {
        header,
        transactions: txs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{TxReceipt, TxStatus};

    #[tokio::test]
    async fn test_store_get_receipt_roundtrip() {
        let directory = crate::test_support::TestDirectory::new();
        let mgr = StorageManager::open(directory.path(), "receipt-test", None)
            .await
            .unwrap();
        let rcpt = TxReceipt {
            tx_hash: "0xdeadbeef".into(),
            block_number: 1,
            status: TxStatus::Success,
            gas_used: 1,
            fee_paid: 1,
            timestamp: 123,
            index: 0,
            error: None,
            contract_address: None,
            logs: vec![],
            return_data: None,
        };
        mgr._store_receipt(&rcpt).await.unwrap();
        let fetched = mgr.get_receipt(&rcpt.tx_hash).await.unwrap().unwrap();
        assert_eq!(fetched, rcpt);
    }

    #[tokio::test]
    async fn explicit_directories_keep_same_address_balances_separate() {
        let first_dir = crate::test_support::TestDirectory::new();
        let second_dir = crate::test_support::TestDirectory::new();
        let (first, second) = tokio::join!(
            StorageManager::open(first_dir.path(), "isolation-test", None),
            StorageManager::open(second_dir.path(), "isolation-test", None),
        );
        let first = first.unwrap();
        let second = second.unwrap();
        first
            .store_account_state(
                "same-address",
                &AccountState {
                    balance: 41,
                    ..Default::default()
                },
            )
            .unwrap();
        second
            .store_account_state(
                "same-address",
                &AccountState {
                    balance: 73,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(first.get_account_state("same-address").unwrap().balance, 41);
        assert_eq!(
            second.get_account_state("same-address").unwrap().balance,
            73
        );
    }

    #[tokio::test]
    async fn explicit_storage_reopens_data_and_rejects_other_chain() {
        let directory = crate::test_support::TestDirectory::new();
        {
            let store = StorageManager::open(directory.path(), "selected-chain", None)
                .await
                .unwrap();
            store
                .store_account_state(
                    "persisted",
                    &AccountState {
                        balance: 123,
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        let error = StorageManager::open(directory.path(), "other-chain", None)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("Chain ID mismatch"));
        let store = StorageManager::open(directory.path(), "selected-chain", None)
            .await
            .unwrap();
        assert_eq!(store.get_account_state("persisted").unwrap().balance, 123);
    }
}

#[cfg(test)]
mod genesis_import_tests {
    use super::*;
    use crate::test_support::TestDirectory;

    fn write_fixture(directory: &TestDirectory, text: &str) -> std::path::PathBuf {
        let path = directory.path().join("genesis.json");
        std::fs::write(&path, text).unwrap();
        path
    }

    #[tokio::test]
    async fn imports_full_width_strings_and_does_not_reapply_at_height_zero() {
        let directory = TestDirectory::new();
        let path = write_fixture(
            &directory,
            &format!(
                r#"{{"dgt_allocations":[{{"address":"dyt1fixture","amount":"{}"}}]}}"#,
                u128::MAX
            ),
        );
        {
            let store = StorageManager::open(directory.path(), "fixture", Some(&path))
                .await
                .unwrap();
            assert_eq!(
                store.get_address_balance("dyt1fixture").await.unwrap(),
                u128::MAX
            );
            store._set_address_balance("dyt1fixture", 7).await.unwrap();
            assert_eq!(store.get_height().unwrap(), 0);
        }
        {
            let store = StorageManager::open(directory.path(), "fixture", Some(&path))
                .await
                .unwrap();
            assert_eq!(store.get_address_balance("dyt1fixture").await.unwrap(), 7);
        }
        std::fs::write(&path, r#"{"dgt_allocations":[]}"#).unwrap();
        assert!(
            StorageManager::open(directory.path(), "fixture", Some(&path))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn numeric_legacy_values_and_document_alias_are_exact() {
        for amount in [42u128, u128::from(u64::MAX) + 1, u128::MAX] {
            let directory = TestDirectory::new();
            let path = write_fixture(
                &directory,
                &format!(r#"{{"allocations":[{{"address":"dyt1fixture","amount":{amount}}}]}}"#),
            );
            let store = StorageManager::open(directory.path(), "fixture", Some(&path))
                .await
                .unwrap();
            assert_eq!(
                store.get_address_balance("dyt1fixture").await.unwrap(),
                amount
            );
        }
    }

    #[tokio::test]
    async fn invalid_imports_leave_no_accounts_or_metadata() {
        for text in [
            "{",
            "{}",
            r#"{"dgt_allocations":"wrong"}"#,
            r#"{"dgt_allocations":[{"address":"dyt1good","amount":42},{"address":"bad","amount":3}]}"#,
            r#"{"dgt_allocations":[{"address":"dyt1same","amount":42},{"address":"dyt1same","amount":3}]}"#,
            r#"{"dgt_allocations":[{"address":"dyt1good","amount":"340282366920938463463374607431768211455"},{"address":"dyt1other","amount":1}]}"#,
            r#"{"dgt_allocations":[{"address":"dyt1good","amount":1.5}]}"#,
        ] {
            let directory = TestDirectory::new();
            let path = write_fixture(&directory, text);
            assert!(
                StorageManager::open(directory.path(), "fixture", Some(&path))
                    .await
                    .is_err()
            );
            let db = DB::open_default(directory.path().join("node.db")).unwrap();
            assert!(db.iterator(IteratorMode::Start).next().is_none());
        }
    }

    #[tokio::test]
    async fn missing_explicit_file_and_legacy_database_require_action() {
        let directory = TestDirectory::new();
        assert!(StorageManager::open(
            directory.path(),
            "fixture",
            Some(&directory.path().join("missing"))
        )
        .await
        .is_err());
        {
            let db = DB::open_default(directory.path().join("node.db")).unwrap();
            db.put(META_HEIGHT, 0u64.to_be_bytes()).unwrap();
        }
        assert!(StorageManager::open(directory.path(), "fixture", None)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn empty_development_import_stays_bound_to_its_chain() {
        let directory = TestDirectory::new();
        {
            let _store = StorageManager::open(directory.path(), "fixture", None)
                .await
                .unwrap();
        }
        assert!(StorageManager::open(directory.path(), "other-chain", None)
            .await
            .is_err());
        assert!(StorageManager::open(directory.path(), "fixture", None)
            .await
            .is_ok());
    }
}
