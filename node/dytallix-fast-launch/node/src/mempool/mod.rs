use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::crypto::{canonical_json, sha3_256, PQCAlgorithm, PQCVerifyError};
use crate::gas::{validate_gas_limit, GasSchedule, TxKind};
use crate::state::State;
use crate::storage::tx::{Transaction, TxMessage};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use dytallix_signature_policy::{PolicyError, PolicyManager};

#[cfg(test)]
mod gas_tests;

#[cfg(test)]
mod pqc_tests;

/// Configuration constants - can be overridden by environment variables
pub const DEFAULT_MAX_TX_BYTES: usize = 1024 * 1024; // 1MB
pub const DEFAULT_MIN_GAS_PRICE: u64 = 1000; // 1000 wei
pub const DEFAULT_MEMPOOL_MAX_TXS: usize = 10000;
pub const DEFAULT_MEMPOOL_MAX_BYTES: usize = 100 * 1024 * 1024; // 100MB

/// Error code constants for external API responses
pub const TX_INVALID_SIG: &str = "TX_INVALID_SIG";

/// Rejection reasons for transactions
#[derive(Debug, Clone, PartialEq)]
pub enum RejectionReason {
    InvalidSignature,
    NonceGap {
        expected: u64,
        got: u64,
    },
    InsufficientFunds {
        denom: String,
        required: u128,
        available: u128,
    },
    UnderpricedGas {
        min: u64,
        got: u64,
    },
    OversizedTx {
        max: usize,
        got: usize,
    },
    Duplicate(String),
    PolicyViolation(String),
    InternalError(String),
}

impl RejectionReason {
    /// Convert to metric label
    pub fn to_metric_label(&self) -> &'static str {
        match self {
            RejectionReason::InvalidSignature => "invalid_signature",
            RejectionReason::NonceGap { .. } => "nonce_gap",
            RejectionReason::InsufficientFunds { .. } => "insufficient_funds",
            RejectionReason::UnderpricedGas { .. } => "underpriced_gas",
            RejectionReason::OversizedTx { .. } => "oversized_tx",
            RejectionReason::Duplicate(_) => "duplicate",
            RejectionReason::PolicyViolation(_) => "policy_violation",
            RejectionReason::InternalError(_) => "internal_error",
        }
    }
}

impl std::fmt::Display for RejectionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RejectionReason::Duplicate(hash) => write!(f, "duplicate tx {hash}"),
            RejectionReason::InvalidSignature => write!(f, "invalid signature"),
            RejectionReason::NonceGap { expected, got } => {
                write!(f, "nonce gap: expected {expected}, got {got}")
            }
            RejectionReason::InsufficientFunds {
                denom,
                required,
                available,
            } => {
                write!(
                    f,
                    "insufficient funds for {}: required {}, available {}",
                    denom, required, available
                )
            }
            RejectionReason::UnderpricedGas { min, got } => {
                write!(f, "underpriced gas: min {min}, got {got}")
            }
            RejectionReason::OversizedTx { max, got } => {
                write!(f, "oversized transaction: max {max}, got {got}")
            }
            RejectionReason::PolicyViolation(msg) => write!(f, "policy violation: {msg}"),
            RejectionReason::InternalError(msg) => write!(f, "internal error: {msg}"),
        }
    }
}

/// Configuration for mempool
#[derive(Debug, Clone)]
pub struct MempoolConfig {
    pub max_tx_bytes: usize,
    pub min_gas_price: u64,
    pub max_txs: usize,
    pub max_bytes: usize,
}

impl Default for MempoolConfig {
    fn default() -> Self {
        Self {
            max_tx_bytes: std::env::var("DYT_MAX_TX_BYTES")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(DEFAULT_MAX_TX_BYTES),
            min_gas_price: std::env::var("DYT_MIN_GAS_PRICE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(DEFAULT_MIN_GAS_PRICE),
            max_txs: std::env::var("DYT_MEMPOOL_MAX_TXS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(DEFAULT_MEMPOOL_MAX_TXS),
            max_bytes: std::env::var("DYT_MEMPOOL_MAX_BYTES")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(DEFAULT_MEMPOOL_MAX_BYTES),
        }
    }
}

/// Transaction with priority ordering
#[derive(Debug, Clone)]
pub struct PendingTx {
    pub tx: Transaction,
    pub received_at: u64,
    pub serialized_size: usize,
}

/// Priority key for ordering transactions
/// Primary: gas_price desc, Secondary: nonce asc, Tertiary: tx_hash asc
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxPriorityKey {
    // made public so priority_key method returning it is valid
    gas_price_neg: u64, // u64::MAX minus price, for descending order
    nonce: u64,
    hash: String,
}

impl Ord for TxPriorityKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.gas_price_neg
            .cmp(&other.gas_price_neg)
            .then_with(|| self.nonce.cmp(&other.nonce))
            .then_with(|| self.hash.cmp(&other.hash))
    }
}

impl PartialOrd for TxPriorityKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PendingTx {
    pub fn new(tx: Transaction) -> Self {
        let serialized_size = estimate_tx_size(&tx);
        let received_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            tx,
            received_at,
            serialized_size,
        }
    }

    pub fn priority_key(&self) -> TxPriorityKey {
        TxPriorityKey {
            gas_price_neg: u64::MAX - self.tx.gas_price,
            nonce: self.tx.nonce,
            hash: self.tx.hash.clone(),
        }
    }
}

/// Production-grade mempool with admission rules, ordering, and bounded capacity
#[derive(Clone)]
pub struct Mempool {
    config: MempoolConfig,
    confirmed_nonces: HashMap<String, u64>,
    /// Policy manager for signature algorithm enforcement
    policy_manager: PolicyManager,
    /// Priority-ordered eligible transactions (BTreeSet for deterministic ordering)
    ordered_txs: BTreeSet<TxPriorityKey>,
    /// Hash to eligible transaction mapping for O(1) lookup
    tx_lookup: HashMap<String, PendingTx>,
    /// Deferred (future-nonce) transactions: global priority index for eviction
    deferred_index: BTreeSet<TxPriorityKey>,
    /// Hash to deferred transaction mapping for O(1) lookup
    deferred_lookup: HashMap<String, PendingTx>,
    /// Per-sender deferred map to promote next-ready nonces quickly
    deferred_by_sender: HashMap<String, BTreeMap<u64, String>>, // sender -> nonce -> hash
    /// Hash set for O(1) duplicate detection across eligible+deferred
    tx_hashes: HashSet<String>,
    /// Total size in bytes of all transactions (eligible + deferred)
    total_bytes: usize,
    /// Per-sender count of eligible (ready) transactions, used to compute expected nonce fast
    eligible_by_sender: HashMap<String, usize>,
    /// Per-sender checked balance requirements across ready and deferred transactions.
    reserved_by_sender: HashMap<String, HashMap<String, u128>>,
}

impl Mempool {
    pub fn new() -> Self {
        Self::with_config(MempoolConfig::default())
    }

    pub fn with_config(config: MempoolConfig) -> Self {
        // Use a policy that allows all PQC algorithms for testing/development
        use dytallix_signature_policy::SignaturePolicy;
        let policy_manager = PolicyManager::new(SignaturePolicy::allow_all_pqc());

        Self {
            config,
            confirmed_nonces: HashMap::new(),
            ordered_txs: BTreeSet::new(),
            tx_lookup: HashMap::new(),
            deferred_index: BTreeSet::new(),
            deferred_lookup: HashMap::new(),
            deferred_by_sender: HashMap::new(),
            tx_hashes: HashSet::new(),
            total_bytes: 0,
            policy_manager,
            eligible_by_sender: HashMap::new(),
            reserved_by_sender: HashMap::new(),
        }
    }

    /// Number of retained transactions, including deferred nonces.
    pub fn total_count(&self) -> usize {
        self.tx_lookup.len() + self.deferred_lookup.len()
    }
    fn reserved_amounts_for_tx(tx: &Transaction) -> Result<HashMap<String, u128>, RejectionReason> {
        crate::transaction_cost::required_balances(tx)
            .map(|m| m.into_iter().collect())
            .map_err(RejectionReason::InternalError)
    }
    fn validate_tx_funds_only(
        &self,
        state: &State,
        tx: &Transaction,
    ) -> Result<(), RejectionReason> {
        let account = state.snapshot_account(&tx.from);
        // Iterate sorted requirements so a multi-token rejection is deterministic.
        let required = crate::transaction_cost::required_balances(tx)
            .map_err(RejectionReason::InternalError)?;
        for (denom, amount) in required {
            let reserved = self
                .reserved_by_sender
                .get(&tx.from)
                .and_then(|m| m.get(&denom))
                .copied()
                .unwrap_or(0);
            let total = reserved.checked_add(amount).ok_or_else(|| {
                RejectionReason::InternalError("Reserved balance exceeds u128".into())
            })?;
            let available = account.balance_of(&denom);
            if available < total {
                return Err(RejectionReason::InsufficientFunds {
                    denom,
                    required: total,
                    available,
                });
            }
        }
        if tx.gas_limit > 0 || tx.gas_price > 0 {
            validate_gas(tx).map_err(RejectionReason::InternalError)?;
        }
        Ok(())
    }
    /// Rebuild indexes from retained transactions and observed committed nonces.
    /// Removal never implies a nonce increment. Checked reserves are recomputed.
    fn rebuild(&mut self) -> Result<(), RejectionReason> {
        let mut entries: BTreeMap<(String, u64), PendingTx> = BTreeMap::new();
        for entry in self.tx_lookup.values().chain(self.deferred_lookup.values()) {
            if entries
                .insert((entry.tx.from.clone(), entry.tx.nonce), entry.clone())
                .is_some()
            {
                return Err(RejectionReason::InternalError(
                    "Duplicate sender nonce in queue".into(),
                ));
            }
        }
        let senders: HashSet<_> = entries.keys().map(|(sender, _)| sender.clone()).collect();
        self.confirmed_nonces
            .retain(|sender, _| senders.contains(sender));
        self.tx_lookup.clear();
        self.deferred_lookup.clear();
        self.ordered_txs.clear();
        self.deferred_index.clear();
        self.deferred_by_sender.clear();
        self.tx_hashes.clear();
        self.reserved_by_sender.clear();
        self.eligible_by_sender.clear();
        self.total_bytes = 0;
        let mut next = self.confirmed_nonces.clone();
        for ((sender, nonce), pending) in entries {
            let delta = Self::reserved_amounts_for_tx(&pending.tx)?;
            let reserved = self.reserved_by_sender.entry(sender.clone()).or_default();
            for (denom, amount) in delta {
                let slot = reserved.entry(denom).or_insert(0);
                *slot = slot.checked_add(amount).ok_or_else(|| {
                    RejectionReason::InternalError("Reserved balance exceeds u128".into())
                })?;
            }
            self.total_bytes = self
                .total_bytes
                .checked_add(pending.serialized_size)
                .ok_or_else(|| {
                    RejectionReason::InternalError("Queue byte count overflow".into())
                })?;
            self.tx_hashes.insert(pending.tx.hash.clone());
            let expected = next.entry(sender.clone()).or_insert(0);
            if nonce == *expected && nonce < u64::MAX {
                *expected += 1;
                *self.eligible_by_sender.entry(sender).or_insert(0) += 1;
                self.ordered_txs.insert(pending.priority_key());
                self.tx_lookup.insert(pending.tx.hash.clone(), pending);
            } else {
                self.deferred_by_sender
                    .entry(sender)
                    .or_default()
                    .insert(nonce, pending.tx.hash.clone());
                self.deferred_index.insert(pending.priority_key());
                self.deferred_lookup
                    .insert(pending.tx.hash.clone(), pending);
            }
        }
        Ok(())
    }
    fn observe_nonces(&mut self, state: &State) {
        for entry in self.tx_lookup.values().chain(self.deferred_lookup.values()) {
            self.confirmed_nonces
                .insert(entry.tx.from.clone(), state.snapshot_nonce(&entry.tx.from));
        }
    }
    pub fn reconcile(&mut self, state: &State, removed: &[String]) -> Result<(), RejectionReason> {
        crate::genesis::reject_qualified_state(&state.storage)
            .map_err(|error| RejectionReason::PolicyViolation(error.to_string()))?;
        let mut candidate = self.clone();
        for hash in removed {
            candidate.tx_lookup.remove(hash);
            candidate.deferred_lookup.remove(hash);
        }
        candidate.observe_nonces(state);
        candidate.rebuild()?;
        *self = candidate;
        Ok(())
    }
    pub fn add_transaction(
        &mut self,
        state: &State,
        tx: Transaction,
    ) -> Result<(), RejectionReason> {
        self.add_transaction_internal(state, tx, false)
    }
    pub fn add_transaction_trusted(
        &mut self,
        state: &State,
        tx: Transaction,
    ) -> Result<(), RejectionReason> {
        self.add_transaction_internal(state, tx, true)
    }
    fn add_transaction_internal(
        &mut self,
        state: &State,
        tx: Transaction,
        skip_signature: bool,
    ) -> Result<(), RejectionReason> {
        crate::genesis::reject_qualified_state(&state.storage)
            .map_err(|error| RejectionReason::PolicyViolation(error.to_string()))?;
        if !skip_signature && !verify_envelope(&tx) {
            return Err(RejectionReason::InvalidSignature);
        }
        self.validate_signature_policy(&tx)
            .map_err(|e| RejectionReason::PolicyViolation(e.to_string()))?;
        if let Some(reason) = unsupported_reserved_payload_reason(&tx) {
            return Err(RejectionReason::PolicyViolation(reason));
        }
        if self.tx_hashes.contains(&tx.hash) {
            return Err(RejectionReason::Duplicate(tx.hash));
        }
        let size = estimate_tx_size(&tx);
        if size > self.config.max_tx_bytes || size > self.config.max_bytes {
            return Err(RejectionReason::OversizedTx {
                max: self.config.max_tx_bytes.min(self.config.max_bytes),
                got: size,
            });
        }
        if tx.gas_price < self.config.min_gas_price {
            return Err(RejectionReason::UnderpricedGas {
                min: self.config.min_gas_price,
                got: tx.gas_price,
            });
        }
        self.validate_tx_funds_only(state, &tx)?;
        let expected = state.snapshot_nonce(&tx.from);
        if tx.nonce < expected {
            return Err(RejectionReason::NonceGap {
                expected,
                got: tx.nonce,
            });
        }
        if tx.nonce == u64::MAX {
            return Err(RejectionReason::PolicyViolation(
                "Account nonce exhausted".into(),
            ));
        }
        // Ready entries form a contiguous interval from the cached committed nonce.
        // Use that interval even when State has changed: duplicate checks precede
        // reconciliation, as they do on the candidate-rebuild path.
        let ready_count = self.eligible_by_sender.get(&tx.from).copied().unwrap_or(0);
        let cached_nonce = self
            .confirmed_nonces
            .get(&tx.from)
            .copied()
            .unwrap_or(expected);
        let ready_end = cached_nonce
            .checked_add(u64::try_from(ready_count).map_err(|_| {
                RejectionReason::InternalError("Ready transaction count exceeds u64".into())
            })?)
            .ok_or_else(|| RejectionReason::InternalError("Ready nonce range overflow".into()))?;
        if tx.nonce >= cached_nonce && tx.nonce < ready_end {
            return Err(RejectionReason::NonceGap {
                expected: ready_end,
                got: tx.nonce,
            });
        }
        if self
            .deferred_by_sender
            .get(&tx.from)
            .is_some_and(|entries| entries.contains_key(&tx.nonce))
        {
            return Err(RejectionReason::PolicyViolation(
                "A pending transaction already uses this sender and nonce".into(),
            ));
        }
        if self.config.max_txs == 0 {
            return Err(RejectionReason::PolicyViolation(
                "Queue capacity is zero".into(),
            ));
        }
        let bytes_after = self.total_bytes.checked_add(size);
        let has_capacity = self.total_count() < self.config.max_txs
            && bytes_after.is_some_and(|bytes| bytes <= self.config.max_bytes);
        let needs_promotion = tx.nonce == ready_end
            && self
                .deferred_by_sender
                .get(&tx.from)
                .is_some_and(|entries| entries.contains_key(&(tx.nonce + 1)));
        // State has no nonce revision counter. Check every retained sender before
        // relying on cached readiness, including senders other than this transaction.
        let nonces_unchanged = has_capacity
            && !needs_promotion
            && self
                .confirmed_nonces
                .iter()
                .all(|(sender, nonce)| state.snapshot_nonce(sender) == *nonce);
        if nonces_unchanged {
            self.insert_without_rebuild(tx, expected, ready_end, ready_count, size)?;
            return Ok(());
        }
        let mut candidate = self.clone();
        candidate.observe_nonces(state);
        candidate.confirmed_nonces.insert(tx.from.clone(), expected);
        candidate.rebuild()?;
        while candidate.total_count() >= candidate.config.max_txs
            || candidate
                .total_bytes
                .checked_add(size)
                .is_none_or(|v| v > candidate.config.max_bytes)
        {
            let lowest = candidate
                .tx_lookup
                .values()
                .chain(candidate.deferred_lookup.values())
                .max_by_key(|p| p.priority_key())
                .map(|p| p.tx.hash.clone())
                .ok_or_else(|| {
                    RejectionReason::InternalError("Cannot free queue capacity".into())
                })?;
            candidate.tx_lookup.remove(&lowest);
            candidate.deferred_lookup.remove(&lowest);
            candidate.rebuild()?;
        }
        candidate.confirmed_nonces.insert(tx.from.clone(), expected);
        let pending = PendingTx::new(tx);
        candidate
            .deferred_lookup
            .insert(pending.tx.hash.clone(), pending);
        candidate.rebuild()?;
        *self = candidate;
        Ok(())
    }
    /// Insert when readiness is current, capacity is available, and no promotion
    /// is needed. Complete all checked work before changing any live index.
    fn insert_without_rebuild(
        &mut self,
        tx: Transaction,
        confirmed_nonce: u64,
        ready_end: u64,
        ready_count: usize,
        size: usize,
    ) -> Result<(), RejectionReason> {
        let total_bytes = self
            .total_bytes
            .checked_add(size)
            .ok_or_else(|| RejectionReason::InternalError("Queue byte count overflow".into()))?;
        let mut reserved = self
            .reserved_by_sender
            .get(&tx.from)
            .cloned()
            .unwrap_or_default();
        for (denom, amount) in Self::reserved_amounts_for_tx(&tx)? {
            let slot = reserved.entry(denom).or_insert(0);
            *slot = slot.checked_add(amount).ok_or_else(|| {
                RejectionReason::InternalError("Reserved balance exceeds u128".into())
            })?;
        }
        let is_ready = tx.nonce == ready_end;
        let count_after = if is_ready {
            ready_count.checked_add(1).ok_or_else(|| {
                RejectionReason::InternalError("Ready transaction count overflow".into())
            })?
        } else {
            ready_count
        };
        let pending = PendingTx::new(tx);
        let sender = pending.tx.from.clone();
        let hash = pending.tx.hash.clone();
        let priority = pending.priority_key();

        // No Result-returning work follows. Promotion, eviction and reconciliation
        // keep their candidate-and-rebuild implementation.
        self.confirmed_nonces
            .insert(sender.clone(), confirmed_nonce);
        self.reserved_by_sender.insert(sender.clone(), reserved);
        self.total_bytes = total_bytes;
        self.tx_hashes.insert(hash.clone());
        if is_ready {
            self.eligible_by_sender.insert(sender, count_after);
            self.ordered_txs.insert(priority);
            self.tx_lookup.insert(hash, pending);
        } else {
            self.deferred_by_sender
                .entry(sender)
                .or_default()
                .insert(pending.tx.nonce, hash.clone());
            self.deferred_index.insert(priority);
            self.deferred_lookup.insert(hash, pending);
        }
        Ok(())
    }
    /// Choose the next nonce per sender; apply fee priority only among ready heads.
    pub fn take_snapshot(&self, n: usize) -> Vec<Transaction> {
        let by_sender: BTreeMap<(&str, u64), &PendingTx> = self
            .tx_lookup
            .values()
            .map(|p| ((p.tx.from.as_str(), p.tx.nonce), p))
            .collect();
        let mut ready: BTreeSet<TxPriorityKey> = BTreeSet::new();
        for (sender, nonce) in &self.confirmed_nonces {
            if let Some(p) = by_sender.get(&(sender.as_str(), *nonce)) {
                ready.insert(p.priority_key());
            }
        }
        let mut result = Vec::new();
        while result.len() < n {
            let Some(key) = ready.pop_first() else {
                break;
            };
            let Some(p) = self.tx_lookup.get(&key.hash) else {
                break;
            };
            result.push(p.tx.clone());
            if let Some(next) =
                p.tx.nonce
                    .checked_add(1)
                    .and_then(|nonce| by_sender.get(&(p.tx.from.as_str(), nonce)))
            {
                ready.insert(next.priority_key());
            }
        }
        result
    }
    /// Remove queue entries without claiming that execution consumed their nonces.
    /// After block commit, call reconcile with the updated State instead.
    pub fn drop_hashes(&mut self, hashes: &[String]) {
        let mut candidate = self.clone();
        for hash in hashes {
            candidate.tx_lookup.remove(hash);
            candidate.deferred_lookup.remove(hash);
        }
        candidate
            .rebuild()
            .expect("removing validated transactions preserves reservation bounds");
        *self = candidate;
    }

    /// Check if transaction exists in mempool
    pub fn contains(&self, hash: &str) -> bool {
        self.tx_hashes.contains(hash)
    }

    /// Get current mempool statistics (eligible only)
    pub fn len(&self) -> usize {
        self.tx_lookup.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    pub fn is_full(&self) -> bool {
        self.total_count() >= self.config.max_txs || self.total_bytes >= self.config.max_bytes
    }

    /// Get current minimum gas price in the pool (eligible only)
    pub fn current_min_gas_price(&self) -> u64 {
        // Find the lowest gas price in the pool (last eligible transaction)
        self.ordered_txs
            .iter()
            .last()
            .and_then(|key| self.tx_lookup.get(&key.hash))
            .map(|tx| tx.tx.gas_price)
            .unwrap_or(self.config.min_gas_price)
    }

    /// Get pool configuration
    pub fn config(&self) -> &MempoolConfig {
        &self.config
    }

    /// Push a transaction into the mempool (RPC method)
    pub fn push(&mut self, tx: Transaction) -> Result<(), RejectionReason> {
        self.add_transaction(&State::default(), tx)
    }
}

/// Legacy error type for backward compatibility
#[derive(Debug)]
pub enum MempoolError {
    Duplicate,
    Full,
    Rejection(RejectionReason),
}

impl From<RejectionReason> for MempoolError {
    fn from(reason: RejectionReason) -> Self {
        match reason {
            RejectionReason::Duplicate(_) => MempoolError::Duplicate,
            reason => MempoolError::Rejection(reason),
        }
    }
}

impl std::fmt::Display for MempoolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MempoolError::Duplicate => write!(f, "duplicate"),
            MempoolError::Full => write!(f, "full"),
            MempoolError::Rejection(reason) => write!(f, "{reason}"),
        }
    }
}

impl std::error::Error for MempoolError {}

/// Verify transaction envelope (signature validation)
pub(crate) fn verify_envelope(tx: &Transaction) -> bool {
    match (&tx.signature, &tx.public_key) {
        (Some(signature), Some(public_key)) => {
            // Perform real PQC signature verification
            match verify_pqc_signature(tx, signature, public_key) {
                Ok(()) => true,
                Err(_) => false,
            }
        }
        _ => false, // No signature or public key provided
    }
}

/// Verify PQC signature for a transaction
fn verify_pqc_signature(tx: &Transaction, signature: &str, public_key: &str) -> Result<(), String> {
    // 1. Decode base64 signature and public key
    let sig_bytes = B64
        .decode(signature)
        .map_err(|e| format!("invalid signature encoding: {e}"))?;
    let pk_bytes = B64
        .decode(public_key)
        .map_err(|e| format!("invalid public key encoding: {e}"))?;

    tracing::info!(
        "PQC verification: pk_len={}, sig_len={}, from={:?}",
        pk_bytes.len(),
        sig_bytes.len(),
        tx.from
    );

    // 2. Create canonical transaction for signing
    let canonical_tx = tx.canonical_fields();

    // 3. Serialize to canonical JSON
    let tx_bytes = canonical_json(&canonical_tx)
        .map_err(|e| format!("failed to serialize transaction: {e}"))?;

    tracing::debug!("Canonical JSON: {}", String::from_utf8_lossy(&tx_bytes));

    // 4. Hash with SHA3-256
    let tx_hash = sha3_256(&tx_bytes);

    tracing::info!("Transaction hash: {}", hex::encode(&tx_hash));

    // 5. Verify signature using new multi-algorithm verification
    // For mempool transactions, we use the default algorithm (Dilithium5)
    // as the Transaction struct doesn't include algorithm field
    match crate::crypto::pqc_verify::verify(
        &pk_bytes,
        &tx_hash,
        &sig_bytes,
        PQCAlgorithm::default(),
    ) {
        Ok(()) => {
            tracing::info!("✅ Transaction signature verification successful");
            Ok(())
        }
        Err(PQCVerifyError::UnsupportedAlgorithm(alg)) => {
            tracing::error!("Unsupported PQC algorithm: {}", alg);
            Err(format!("unsupported algorithm: {alg}"))
        }
        Err(PQCVerifyError::InvalidPublicKey { algorithm, details }) => {
            tracing::error!("Invalid public key for {}: {}", algorithm, details);
            Err(format!("invalid public key: {details}"))
        }
        Err(PQCVerifyError::InvalidSignature { algorithm, details }) => {
            tracing::error!("Invalid signature for {}: {}", algorithm, details);
            Err(format!("invalid signature: {details}"))
        }
        Err(PQCVerifyError::VerificationFailed { algorithm }) => {
            tracing::warn!(
                "❌ Signature verification failed for algorithm: {}",
                algorithm
            );
            Err("signature verification failed".to_string())
        }
        Err(PQCVerifyError::FeatureNotCompiled { feature }) => {
            tracing::error!("PQC feature not compiled: {}", feature);
            Err(format!("feature not available: {feature}"))
        }
    }
}

/// Enhanced validation including gas validation (legacy function for backward compatibility)
pub fn basic_validate(state: &State, tx: &Transaction) -> Result<(), String> {
    crate::genesis::reject_qualified_state(&state.storage).map_err(|error| error.to_string())?;
    let mempool = Mempool::new();
    match mempool.validate_tx_funds_only(state, tx) {
        Ok(()) => Ok(()),
        Err(reason) => Err(reason.to_string()),
    }
}

/// Gas validation function with enhanced error reporting
fn validate_gas(tx: &Transaction) -> Result<(), String> {
    let schedule = GasSchedule::default();

    // For now, assume all transactions are transfers
    // This will be extended when we have better transaction type detection
    let tx_kind = TxKind::Transfer;

    // Estimate transaction size (approximation for now)
    let tx_size_bytes = estimate_tx_size(tx);
    let additional_signatures = 0; // Single signature for now

    // Validate gas limit against intrinsic requirements
    validate_gas_limit(
        &tx_kind,
        tx_size_bytes,
        additional_signatures,
        tx.gas_limit,
        &schedule,
    )
    .map_err(|e| format!("GasValidationError: {e}"))?;

    // Check gas price is reasonable (non-zero)
    if tx.gas_price == 0 {
        return Err("GasValidationError: gas price cannot be zero".to_string());
    }

    Ok(())
}

/// Estimate transaction size for gas calculation and size limits
fn estimate_tx_size(tx: &Transaction) -> usize {
    // Cap the signature contribution to avoid pathological sizes from different
    // signature schemes (e.g., PQC). This keeps mempool accounting predictable.
    const MAX_SIG_ACCOUNTING: usize = 256;

    let sig_len = tx
        .signature
        .as_ref()
        .map_or(0, |s| s.len().min(MAX_SIG_ACCOUNTING));

    // Rough estimate based on serialized fields
    // This should be more precise in production
    tx.hash.len() +
    tx.from.len() +
    tx.to.len() +
    16 + // amount (u128)
    16 + // fee (u128)
    8 +  // nonce (u64)
    sig_len +
    8 +  // gas_limit (u64)
    8 // gas_price (u64)
}

impl Mempool {
    /// Validate transaction signature algorithm against policy
    fn validate_signature_policy(&self, tx: &Transaction) -> Result<(), PolicyError> {
        if let Some(alg) = tx.signature_algorithm() {
            if self.policy_manager.policy().should_enforce_at_mempool() {
                // All transactions currently use Dilithium5
                // The algorithm is already in dytallix_pqc::SignatureAlgorithm format
                self.policy_manager.validate_transaction_algorithm(&alg)?;
            }
        }
        Ok(())
    }
}

fn unsupported_reserved_payload_reason(tx: &Transaction) -> Option<String> {
    let messages = tx.messages.as_ref()?;
    for message in messages {
        if let TxMessage::Data { data, .. } = message {
            if data.starts_with("stake:") {
                return Some(
                    "staking write payloads are not supported on the generic transaction submit path; use read-only staking routes until staking is explicitly enabled end-to-end".to_string(),
                );
            }
            if data.starts_with("governance:") {
                return Some(
                    "governance write payloads are not supported on the generic transaction submit path; use public governance read routes until governance is explicitly enabled end-to-end".to_string(),
                );
            }
        }
    }
    None
}

#[cfg(test)]
mod policy_tests {
    use super::{unsupported_reserved_payload_reason, Mempool, RejectionReason};
    use crate::state::State;
    use crate::storage::tx::{Transaction, TxMessage};

    fn funded_state() -> State {
        let mut state = State::new_for_test();
        state.credit(
            "dytallix1sender0000000000000000000000000000",
            "udgt",
            10_000_000,
        );
        state
    }

    fn base_tx(messages: Vec<TxMessage>) -> Transaction {
        Transaction::base(
            "0xpolicytest",
            "dytallix1sender0000000000000000000000000000",
            "dytallix1receiver00000000000000000000000000",
            0,
            5_000,
            0,
        )
        .with_gas(5_000, 1_000)
        .with_messages(messages)
    }

    #[test]
    fn reserved_staking_payloads_are_rejected() {
        let state = funded_state();
        let tx = base_tx(vec![TxMessage::Data {
            from: "dytallix1sender0000000000000000000000000000".to_string(),
            data: "stake:delegate:dytallix1validator:1000".to_string(),
        }]);
        let mut mempool = Mempool::new();

        let result = mempool.add_transaction_trusted(&state, tx);
        assert!(matches!(result, Err(RejectionReason::PolicyViolation(_))));
    }

    #[test]
    fn reserved_governance_payloads_are_detected() {
        let tx = base_tx(vec![TxMessage::Data {
            from: "dytallix1sender0000000000000000000000000000".to_string(),
            data: "governance:vote:7:yes".to_string(),
        }]);

        let reason = unsupported_reserved_payload_reason(&tx).unwrap();
        assert!(reason.contains("governance write payloads"));
    }
}

#[cfg(test)]
mod incremental_tests {
    use super::{Mempool, MempoolConfig, PendingTx};
    use crate::state::State;
    use crate::storage::tx::Transaction;
    use std::collections::HashMap;

    fn assert_pending_equal(
        actual: &HashMap<String, PendingTx>,
        expected: &HashMap<String, PendingTx>,
    ) {
        assert_eq!(actual.len(), expected.len());
        for (hash, pending) in actual {
            let other = expected.get(hash).expect("same retained transaction");
            assert_eq!(pending.received_at, other.received_at);
            assert_eq!(pending.serialized_size, other.serialized_size);
            assert_eq!(
                serde_json::to_value(&pending.tx).unwrap(),
                serde_json::to_value(&other.tx).unwrap()
            );
        }
    }

    fn assert_pool_equal(actual: &Mempool, expected: &Mempool) {
        assert_eq!(actual.confirmed_nonces, expected.confirmed_nonces);
        assert_eq!(actual.ordered_txs, expected.ordered_txs);
        assert_eq!(actual.deferred_index, expected.deferred_index);
        assert_eq!(actual.deferred_by_sender, expected.deferred_by_sender);
        assert_eq!(actual.tx_hashes, expected.tx_hashes);
        assert_eq!(actual.total_bytes, expected.total_bytes);
        assert_eq!(actual.eligible_by_sender, expected.eligible_by_sender);
        assert_eq!(actual.reserved_by_sender, expected.reserved_by_sender);
        assert_pending_equal(&actual.tx_lookup, &expected.tx_lookup);
        assert_pending_equal(&actual.deferred_lookup, &expected.deferred_lookup);
    }

    fn assert_matches_rebuild(pool: &Mempool) {
        let mut rebuilt = pool.clone();
        rebuilt.rebuild().unwrap();
        assert_pool_equal(pool, &rebuilt);
    }

    fn state() -> State {
        let mut state = State::new_for_test();
        for sender in ["a", "b", "c"] {
            state.set_balance(sender, "udgt", 1_000_000_000);
            state.set_balance(sender, "udrt", 1_000_000_000);
        }
        state
    }

    fn tx(hash: &str, sender: &str, nonce: u64) -> Transaction {
        Transaction::base(hash, sender, "receiver", 1_000, 100, nonce).with_gas(21_000, 1_000)
    }

    #[test]
    fn incremental_indexes_match_rebuild_across_queue_transitions() {
        let mut state = state();
        let mut pool = Mempool::with_config(MempoolConfig {
            max_txs: 6,
            ..Default::default()
        });
        // Ready insertion, deferred insertion, gap closure, and capacity eviction.
        for (hash, sender, nonce) in [
            ("a2", "a", 2),
            ("b0", "b", 0),
            ("a0", "a", 0),
            ("b2", "b", 2),
            ("a1", "a", 1),
            ("b1", "b", 1),
            ("c0", "c", 0),
        ] {
            pool.add_transaction_trusted(&state, tx(hash, sender, nonce))
                .unwrap();
            assert_matches_rebuild(&pool);
        }
        pool.drop_hashes(&["b0".into()]);
        assert_matches_rebuild(&pool);
        pool.add_transaction_trusted(&state, tx("b0-again", "b", 0))
            .unwrap();
        assert_matches_rebuild(&pool);

        // A nonce change for another sender must prevent use of cached readiness.
        state.accounts.get_mut("a").unwrap().nonce = 1;
        pool.drop_hashes(&["b0-again".into()]);
        pool.add_transaction_trusted(&state, tx("c1", "c", 1))
            .unwrap();
        assert_eq!(pool.confirmed_nonces.get("a"), Some(&1));
        assert_matches_rebuild(&pool);
    }

    #[test]
    fn incremental_nonce_intervals_handle_nonzero_and_exhaustion_boundaries() {
        for initial_nonce in [7, u64::MAX - 2] {
            let mut state = state();
            state.accounts.get_mut("a").unwrap().nonce = initial_nonce;
            let mut pool = Mempool::new();
            for nonce in [initial_nonce, initial_nonce + 1] {
                pool.add_transaction_trusted(&state, tx(&format!("nonce-{nonce}"), "a", nonce))
                    .unwrap();
                assert_matches_rebuild(&pool);
            }
            assert_eq!(pool.len(), 2);
            assert_eq!(pool.total_count(), 2);
            assert_eq!(
                pool.take_snapshot(10)
                    .iter()
                    .map(|tx| tx.nonce)
                    .collect::<Vec<_>>(),
                vec![initial_nonce, initial_nonce + 1]
            );
            let before = pool.clone();
            for nonce in [initial_nonce, initial_nonce + 1] {
                assert_eq!(
                    pool.add_transaction_trusted(&state, tx("duplicate-nonce", "a", nonce)),
                    Err(super::RejectionReason::NonceGap {
                        expected: initial_nonce + 2,
                        got: nonce,
                    })
                );
                assert_pool_equal(&pool, &before);
            }
            assert_eq!(
                pool.add_transaction_trusted(&state, tx("exhausted", "a", u64::MAX)),
                Err(super::RejectionReason::PolicyViolation(
                    "Account nonce exhausted".into()
                ))
            );
            assert_pool_equal(&pool, &before);
        }
    }

    #[test]
    fn rejected_insertions_preserve_all_incremental_indexes() {
        let state = state();
        let mut pool = Mempool::new();
        pool.add_transaction_trusted(&state, tx("a0", "a", 0))
            .unwrap();
        pool.add_transaction_trusted(&state, tx("a2", "a", 2))
            .unwrap();
        let before = pool.clone();
        let mut unaffordable = tx("unaffordable", "a", 1);
        unaffordable.amount = 1_000_000_001;
        for transaction in [
            tx("a0", "a", 1),
            tx("ready-duplicate", "a", 0),
            tx("deferred-duplicate", "a", 2),
            tx("exhausted", "a", u64::MAX),
            unaffordable,
        ] {
            assert!(pool.add_transaction_trusted(&state, transaction).is_err());
            assert_pool_equal(&pool, &before);
        }
    }
}

impl Default for Mempool {
    fn default() -> Self {
        Self::new()
    }
}
