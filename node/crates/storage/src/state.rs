use rocksdb::{Options, DB};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug)]
pub struct Storage {
    pub db: DB,
    execution_lock: std::sync::Mutex<()>,
    /// RocksDB sequence number at the last complete consensus history
    /// verification on this handle. Any later write invalidates it.
    verified_sequence: std::sync::Mutex<Option<u64>>,
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
            verified_sequence: std::sync::Mutex::new(None),
        })
    }

    pub fn open(path: PathBuf) -> anyhow::Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        let db = DB::open(&opts, path)?;
        Ok(Self {
            db,
            execution_lock: std::sync::Mutex::new(()),
            verified_sequence: std::sync::Mutex::new(None),
        })
    }
    /// Serialize selected-node execution planning and commit on this storage handle.
    /// Other legacy writers do not yet participate in this boundary.
    pub fn lock_execution(&self) -> anyhow::Result<std::sync::MutexGuard<'_, ()>> {
        self.execution_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("Execution storage lock poisoned"))
    }

    /// True when no write has occurred since `mark_verified` recorded the
    /// current sequence number.
    pub fn verified_at_current_sequence(&self) -> anyhow::Result<bool> {
        let mark = *self
            .verified_sequence
            .lock()
            .map_err(|_| anyhow::anyhow!("Verification mark lock poisoned"))?;
        Ok(mark == Some(self.db.latest_sequence_number()))
    }

    /// Record that committed state at `sequence` passed verification. The mark
    /// is kept only if no write occurred after `sequence`.
    pub fn mark_verified(&self, sequence: u64) -> anyhow::Result<()> {
        let mut mark = self
            .verified_sequence
            .lock()
            .map_err(|_| anyhow::anyhow!("Verification mark lock poisoned"))?;
        *mark = (self.db.latest_sequence_number() == sequence).then_some(sequence);
        Ok(())
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
