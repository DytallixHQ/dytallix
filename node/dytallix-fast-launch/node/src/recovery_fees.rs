//! Explicit local sponsored-recovery overlay. No migration or ordinary envelope fallback.
//! Native Settlement balances remain authoritative. The caller commits the book,
//! native fee writes, receipts and block head in the same synchronous database batch.
use crate::settlement::Settlement;
use anyhow::{bail, ensure, Context, Result};
use dytallix_protocol_types::{
    address::{AccountAddress, AddressNetwork},
    recovery::{ActionKind, KeyIdentity, RecoveryPolicy, RecoveryState},
    recovery_sponsor::{self as wire, FeeProfile, SponsoredRecovery},
    sha3_256,
};
use dytallix_runtime_crypto::recovery_sponsor::{verify_signed, SponsorVerificationError};
use crate::block_lifecycle::{Deletes, Writes};
use crate::storage::state::Storage;
use rocksdb::{Direction, IteratorMode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Every stored recovery key is under this prefix: a header plus one entry per
/// account, sponsor receipt, success-index entry and expiry-index entry.
pub(crate) const PREFIX: &str = "recovery:v2:";
pub(crate) const HEADER_KEY: &str = "recovery:v2:header";
const ACCOUNT_PREFIX: &str = "recovery:v2:account:";
const RECEIPT_PREFIX: &str = "recovery:v2:receipt:";
const OPERATION_PREFIX: &str = "recovery:v2:operation:";
const EXPIRY_PREFIX: &str = "recovery:v2:expiry:";
const ORIGIN_PREFIX: &str = "recovery:v2:origin:";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredHeader {
    version: u16,
    last_height: u64,
    profile: FeeProfile,
}

/// True when two account records differ at most in their recovery height.
fn same_apart_from_height(a: &RecoveryAccount, b: &RecoveryAccount) -> bool {
    let mut aligned = a.clone();
    aligned.recovery.last_height = b.recovery.last_height;
    aligned == *b
}

fn canonical<T: Serialize + DeserializeOwned>(bytes: &[u8], what: &str) -> Result<T> {
    let value: T =
        serde_json::from_slice(bytes).with_context(|| format!("Invalid recovery {what}"))?;
    ensure!(
        serde_json::to_vec(&value)? == bytes,
        "Recovery {what} encoding is not canonical"
    );
    Ok(value)
}
// Implementation bounds for the isolated local profile. No pruning is authorized.
// The book is stored per entry, so no whole-book byte bound applies.
pub(crate) const MAX_ACCOUNTS: usize = 4096;
const MAX_RECEIPTS: usize = 65536;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryAccount {
    pub address: String,
    pub recovery: RecoveryState,
    pub sponsor_nonce: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SponsorReceipt {
    pub operation_id: [u8; 32],
    pub sponsor_authorization_id: [u8; 32],
    pub envelope_hash: [u8; 32],
    pub sponsor_account_id: [u8; 32],
    pub target_account_id: [u8; 32],
    pub block_height: u64,
    pub block_index: u32,
    pub success: bool,
    pub profile_version: u64,
    pub profile_digest: [u8; 32],
    pub gas_limit: u64,
    pub gas_used: u64,
    pub reserved_cap: u128,
    pub settled_fee: u128,
    pub released_reserve: u128,
    pub sponsor_counter_before: u64,
    pub sponsor_counter_after: u64,
    pub target_state_digest: [u8; 32],
    pub record_hash: [u8; 32],
}
impl SponsorReceipt {
    fn hash(&self) -> Result<[u8; 32]> {
        let mut copy = self.clone();
        copy.record_hash = [0; 32];
        let mut bytes = b"DYTALLIX/RECOVERY-RECEIPT\0".to_vec();
        bytes.extend(serde_json::to_vec(&copy)?);
        Ok(sha3_256(&bytes))
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryBook {
    pub version: u16,
    pub last_height: u64,
    pub profile: FeeProfile,
    pub accounts: BTreeMap<String, RecoveryAccount>,
    pub sponsor_receipts: BTreeMap<String, SponsorReceipt>,
    pub operation_success: BTreeMap<String, String>,
    pub expiry_index: BTreeMap<u64, BTreeSet<String>>,
    /// Each account's immutable origin public key, from which its ID was
    /// derived. Present under the combined ordinary profile; the ordinary
    /// configuration checks that it covers every account and hashes to its ID.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub origins: BTreeMap<String, KeyIdentity>,
}
impl RecoveryBook {
    pub(crate) fn new(profile: FeeProfile, accounts: Vec<RecoveryAccount>) -> Result<Self> {
        let mut book = Self {
            version: 1,
            last_height: 0,
            profile,
            accounts: BTreeMap::new(),
            sponsor_receipts: BTreeMap::new(),
            operation_success: BTreeMap::new(),
            expiry_index: BTreeMap::new(),
            origins: BTreeMap::new(),
        };
        for account in accounts {
            ensure!(
                account.sponsor_nonce == 0 && account.recovery.last_height == 0,
                "Fresh recovery genesis requires zero height and sponsor counters"
            );
            ensure!(
                account.recovery.pending_recovery.is_none()
                    && account.recovery.pending_policy.is_none(),
                "Fresh recovery genesis cannot contain pending work"
            );
            let id = hex::encode(account.recovery.domain.account_id);
            ensure!(
                book.accounts.insert(id, account).is_none(),
                "Duplicate recovery account ID"
            );
        }
        book.validate()?;
        Ok(book)
    }
    /// Look up an account by native address without scanning the book. An
    /// address encodes its account ID, so decode it, look up the ID and require
    /// the stored address to match exactly (as a linear search would).
    pub(crate) fn account_by_address(&self, address: &str) -> Option<&RecoveryAccount> {
        let id = *[
            AddressNetwork::Mainnet,
            AddressNetwork::Testnet,
            AddressNetwork::Development,
        ]
        .into_iter()
        .find_map(|network| AccountAddress::decode(network, address).ok())?
        .account_id();
        self.accounts
            .get(&hex::encode(id))
            .filter(|account| account.address == address)
    }
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "Unsupported recovery book version");
        self.profile.validate()?;
        ensure!(
            !self.accounts.is_empty() && self.accounts.len() <= MAX_ACCOUNTS,
            "Recovery account count outside bounds"
        );
        ensure!(
            self.sponsor_receipts.len() <= MAX_RECEIPTS,
            "Recovery receipt capacity exceeded"
        );
        let profile_digest = wire::profile_digest(&self.profile)?;
        let mut addresses = BTreeSet::new();
        let mut domain = None;
        for (id, account) in &self.accounts {
            account.recovery.validate()?;
            ensure!(
                *id == hex::encode(account.recovery.domain.account_id),
                "Recovery account key mismatch"
            );
            ensure!(
                !account.address.is_empty()
                    && account.address.len() <= 256
                    && addresses.insert(&account.address),
                "Invalid or duplicate native account mapping"
            );
            ensure!(
                account.recovery.last_height == self.last_height,
                "Recovery account height differs from book"
            );
            let d = &account.recovery.domain;
            let shared = (d.network, d.chain_id.clone(), d.genesis_digest);
            if let Some(expected) = &domain {
                ensure!(*expected == shared, "Mixed recovery chain domains");
            }
            domain = Some(shared);
            for (algorithm, length) in &account.recovery.config.algorithms {
                let expected = match algorithm.as_str() {
                    "mldsa65" => 1952,
                    "mldsa87" => 2592,
                    _ => bail!("Unsupported configured recovery algorithm"),
                };
                ensure!(
                    *length == expected && self.profile.signature_costs.contains_key(algorithm),
                    "Recovery key profile differs from fee profile"
                );
            }
        }
        ensure!(
            self.origins.keys().all(|id| self.accounts.contains_key(id)),
            "Recovery origin without account"
        );
        let expected = self.expected_expiries()?;
        ensure!(
            expected == self.expiry_index,
            "Recovery due-expiry index differs from obligations"
        );
        self.check_capacity(&expected)?;
        let mut positions = BTreeSet::new();
        let mut counters = BTreeSet::new();
        for (id, receipt) in &self.sponsor_receipts {
            let sponsor_id = hex::encode(receipt.sponsor_account_id);
            let sponsor = self
                .accounts
                .get(&sponsor_id)
                .context("Missing receipt sponsor")?;
            self.check_receipt(
                id,
                receipt,
                &profile_digest,
                sponsor.sponsor_nonce,
                self.operation_success.get(&hex::encode(receipt.operation_id)),
            )?;
            ensure!(
                positions.insert((receipt.block_height, receipt.block_index)),
                "Invalid receipt block position"
            );
            ensure!(
                counters.insert((sponsor_id, receipt.sponsor_counter_before)),
                "Repeated or future sponsor counter"
            );
        }
        for (operation, id) in &self.operation_success {
            let receipt = self
                .sponsor_receipts
                .get(id)
                .context("Success index references absent receipt")?;
            ensure!(
                receipt.success && *operation == hex::encode(receipt.operation_id),
                "Success index mismatch"
            );
        }
        // All sponsorship history is retained. Gaps cannot be silently accepted.
        let mut counts: BTreeMap<&str, u64> = BTreeMap::new();
        for (sponsor, _) in &counters {
            *counts.entry(sponsor.as_str()).or_default() += 1;
        }
        for (id, account) in &self.accounts {
            ensure!(
                counts.get(id.as_str()).copied().unwrap_or(0) == account.sponsor_nonce,
                "Sponsor history does not cover its counter"
            );
        }
        Ok(())
    }
    /// Checks on one receipt that do not compare it with other receipts.
    /// `sponsor_nonce` and `success_entry` are the sponsor's counter and the
    /// operation's success-index entry in the book that holds the receipt.
    fn check_receipt(
        &self,
        id: &str,
        receipt: &SponsorReceipt,
        profile_digest: &[u8; 32],
        sponsor_nonce: u64,
        success_entry: Option<&String>,
    ) -> Result<()> {
        ensure!(
            id == hex::encode(receipt.sponsor_authorization_id)
                && receipt.record_hash == receipt.hash()?,
            "Recovery receipt digest mismatch"
        );
        ensure!(
            receipt.block_height > 0 && receipt.block_height <= self.last_height,
            "Invalid receipt block position"
        );
        ensure!(
            receipt.profile_version == self.profile.version
                && receipt.profile_digest == *profile_digest,
            "Receipt requires an unavailable fee profile"
        );
        ensure!(
            receipt.gas_limit > 0
                && receipt.gas_limit <= self.profile.max_transaction_gas
                && receipt.gas_used <= receipt.gas_limit
                && receipt.gas_used >= self.profile.minimum_gas,
            "Invalid receipt gas counters"
        );
        ensure!(
            receipt.success || receipt.gas_used == receipt.gas_limit,
            "Invalid charged failure"
        );
        ensure!(
            receipt.settled_fee
                == u128::from(receipt.gas_used) * u128::from(self.profile.gas_price)
                && receipt.reserved_cap.checked_sub(receipt.settled_fee)
                    == Some(receipt.released_reserve)
                && receipt.reserved_cap <= self.profile.max_fee_cap
                && receipt.reserved_cap
                    >= u128::from(receipt.gas_limit) * u128::from(self.profile.gas_price),
            "Invalid receipt fee conservation"
        );
        ensure!(
            receipt.sponsor_counter_before.checked_add(1) == Some(receipt.sponsor_counter_after),
            "Invalid receipt sponsor counter"
        );
        ensure!(
            receipt.sponsor_counter_after <= sponsor_nonce,
            "Repeated or future sponsor counter"
        );
        ensure!(
            self.accounts
                .contains_key(&hex::encode(receipt.target_account_id))
                && receipt.target_account_id != receipt.sponsor_account_id,
            "Invalid receipt target"
        );
        ensure!(
            !receipt.success || success_entry.map(String::as_str) == Some(id),
            "Successful receipt absent from operation index"
        );
        ensure!(
            receipt.success || success_entry.map(String::as_str) != Some(id),
            "Failure entered successful operation index"
        );
        Ok(())
    }
    /// Expiry heights one account currently owes.
    fn account_expiries(account: &RecoveryAccount) -> impl Iterator<Item = u64> {
        [
            account.recovery.pending_recovery.as_ref().map(|p| p.expiry_height),
            account.recovery.pending_policy.as_ref().map(|p| p.expiry_height),
        ]
        .into_iter()
        .flatten()
    }
    fn add_expiries(
        &self,
        index: &mut BTreeMap<u64, BTreeSet<String>>,
        id: &str,
        account: &RecoveryAccount,
    ) -> Result<()> {
        for height in Self::account_expiries(account) {
            ensure!(
                height > self.last_height,
                "Unprocessed mandatory recovery expiry"
            );
            ensure!(
                index.entry(height).or_default().insert(id.to_owned()),
                "Duplicate account expiry obligation"
            );
        }
        Ok(())
    }
    fn expected_expiries(&self) -> Result<BTreeMap<u64, BTreeSet<String>>> {
        let mut index: BTreeMap<u64, BTreeSet<String>> = BTreeMap::new();
        for (id, account) in &self.accounts {
            self.add_expiries(&mut index, id, account)?;
        }
        Ok(index)
    }
    /// The expiry index after one account changes. Given a consistent index,
    /// this equals `expected_expiries` of the changed book.
    fn expiry_index_with(
        &self,
        id: &str,
        account: &RecoveryAccount,
    ) -> Result<BTreeMap<u64, BTreeSet<String>>> {
        let mut index = self.expiry_index.clone();
        index.retain(|_, ids| {
            ids.remove(id);
            !ids.is_empty()
        });
        self.add_expiries(&mut index, id, account)?;
        Ok(index)
    }
    fn capacity_available(&self, index: &BTreeMap<u64, BTreeSet<String>>) -> Result<bool> {
        let pending: BTreeSet<_> = index.values().flat_map(|ids| ids.iter()).collect();
        if pending.len() as u64 > self.profile.max_pending_accounts {
            return Ok(false);
        }
        for ids in index.values() {
            if ids.len() as u64 > self.profile.max_due_expiry_events_per_height {
                return Ok(false);
            }
            let gas = (ids.len() as u64)
                .checked_mul(self.profile.expiry_event_gas_cost)
                .context("Expiry cost overflow")?;
            if gas > self.profile.mandatory_expiry_gas_budget {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn check_capacity(&self, index: &BTreeMap<u64, BTreeSet<String>>) -> Result<()> {
        ensure!(
            self.capacity_available(index)?,
            "Recovery expiry capacity exhausted"
        );
        Ok(())
    }
    /// The retired single-value encoding, kept for tests of whole-book validation.
    #[cfg(test)]
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
        let book: Self = serde_json::from_slice(bytes).context("Invalid recovery book")?;
        book.validate()?;
        ensure!(
            serde_json::to_vec(&book)? == bytes,
            "Recovery book encoding is not canonical"
        );
        Ok(book)
    }
    #[cfg(test)]
    pub(crate) fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        Ok(serde_json::to_vec(self)?)
    }
    /// The stored form: one entry per account, sponsor receipt, success-index
    /// entry and expiry-index entry, plus a header. Blocks write changed entries.
    pub(crate) fn entries(&self) -> Result<BTreeMap<Vec<u8>, Vec<u8>>> {
        self.validate()?;
        let mut entries = BTreeMap::new();
        let header = StoredHeader {
            version: self.version,
            last_height: self.last_height,
            profile: self.profile.clone(),
        };
        entries.insert(HEADER_KEY.as_bytes().to_vec(), serde_json::to_vec(&header)?);
        for (id, account) in &self.accounts {
            entries.insert(
                format!("{ACCOUNT_PREFIX}{id}").into_bytes(),
                serde_json::to_vec(account)?,
            );
        }
        for (id, receipt) in &self.sponsor_receipts {
            entries.insert(
                format!("{RECEIPT_PREFIX}{id}").into_bytes(),
                serde_json::to_vec(receipt)?,
            );
        }
        for (operation, id) in &self.operation_success {
            entries.insert(
                format!("{OPERATION_PREFIX}{operation}").into_bytes(),
                serde_json::to_vec(id)?,
            );
        }
        for (height, ids) in &self.expiry_index {
            for id in ids {
                entries.insert(
                    format!("{EXPIRY_PREFIX}{height:020}:{id}").into_bytes(),
                    Vec::new(),
                );
            }
        }
        for (id, key) in &self.origins {
            entries.insert(
                format!("{ORIGIN_PREFIX}{id}").into_bytes(),
                serde_json::to_vec(key)?,
            );
        }
        Ok(entries)
    }
    /// Writes and deletions that turn `committed`'s stored entries into this
    /// book's. An account is written only when it changed in more than its
    /// recovery height; `load` advances stored accounts to the header height.
    /// `committed` was validated when loaded; only this book is validated here,
    /// and only changed entries are serialized.
    pub(crate) fn changes_from(&self, committed: &Self) -> Result<(Writes, Deletes)> {
        self.validate()?;
        let mut writes = Writes::new();
        let mut deletes = Deletes::new();
        let header = |book: &Self| {
            serde_json::to_vec(&StoredHeader {
                version: book.version,
                last_height: book.last_height,
                profile: book.profile.clone(),
            })
        };
        let next_header = header(self)?;
        if header(committed)? != next_header {
            writes.insert(HEADER_KEY.as_bytes().to_vec(), next_header);
        }
        fn diff<V: Serialize + PartialEq>(
            prefix: &str,
            next: &BTreeMap<String, V>,
            previous: &BTreeMap<String, V>,
            writes: &mut Writes,
            deletes: &mut Deletes,
        ) -> Result<()> {
            for (key, value) in next {
                if previous.get(key) != Some(value) {
                    writes.insert(format!("{prefix}{key}").into_bytes(), serde_json::to_vec(value)?);
                }
            }
            for key in previous.keys().filter(|key| !next.contains_key(*key)) {
                deletes.insert(format!("{prefix}{key}").into_bytes());
            }
            Ok(())
        }
        diff(RECEIPT_PREFIX, &self.sponsor_receipts, &committed.sponsor_receipts, &mut writes, &mut deletes)?;
        diff(OPERATION_PREFIX, &self.operation_success, &committed.operation_success, &mut writes, &mut deletes)?;
        diff(ORIGIN_PREFIX, &self.origins, &committed.origins, &mut writes, &mut deletes)?;
        let expiry_keys = |book: &Self| -> BTreeSet<Vec<u8>> {
            book.expiry_index
                .iter()
                .flat_map(|(height, ids)| {
                    ids.iter().map(move |id| format!("{EXPIRY_PREFIX}{height:020}:{id}").into_bytes())
                })
                .collect()
        };
        let (next_expiry, previous_expiry) = (expiry_keys(self), expiry_keys(committed));
        for key in next_expiry.difference(&previous_expiry) {
            writes.insert(key.clone(), Vec::new());
        }
        deletes.extend(previous_expiry.difference(&next_expiry).cloned());
        for (id, account) in &self.accounts {
            let unchanged = committed
                .accounts
                .get(id)
                .is_some_and(|prior| same_apart_from_height(prior, account));
            if !unchanged {
                writes.insert(
                    format!("{ACCOUNT_PREFIX}{id}").into_bytes(),
                    serde_json::to_vec(account)?,
                );
            }
        }
        for id in committed.accounts.keys() {
            if !self.accounts.contains_key(id) {
                deletes.insert(format!("{ACCOUNT_PREFIX}{id}").into_bytes());
            }
        }
        #[cfg(test)]
        assert_eq!(
            (&writes, &deletes),
            (&self.changes_from_reference(committed)?.0, &self.changes_from_reference(committed)?.1),
            "Incremental recovery diff differs from the full-entry reference"
        );
        Ok((writes, deletes))
    }
    /// The previous whole-entry diff, kept as the test oracle for `changes_from`.
    #[cfg(test)]
    fn changes_from_reference(&self, committed: &Self) -> Result<(Writes, Deletes)> {
        let is_account = |key: &Vec<u8>| key.starts_with(ACCOUNT_PREFIX.as_bytes());
        let next = self.entries()?;
        let previous = committed.entries()?;
        let mut writes: Writes = next
            .iter()
            .filter(|(key, value)| !is_account(key) && previous.get(*key) != Some(*value))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        let mut deletes: Deletes = previous
            .keys()
            .filter(|key| !is_account(key) && !next.contains_key(*key))
            .cloned()
            .collect();
        for (id, account) in &self.accounts {
            if !committed.accounts.get(id).is_some_and(|prior| same_apart_from_height(prior, account)) {
                writes.insert(format!("{ACCOUNT_PREFIX}{id}").into_bytes(), serde_json::to_vec(account)?);
            }
        }
        for id in committed.accounts.keys().filter(|id| !self.accounts.contains_key(*id)) {
            deletes.insert(format!("{ACCOUNT_PREFIX}{id}").into_bytes());
        }
        Ok((writes, deletes))
    }
    /// Read every entry under PREFIX. Unknown keys, noncanonical values and
    /// entries without a header are rejected.
    pub(crate) fn load(storage: &Storage) -> Result<Option<Self>> {
        let prefix = PREFIX.as_bytes();
        let mut header = None;
        let mut accounts = BTreeMap::new();
        let mut sponsor_receipts = BTreeMap::new();
        let mut operation_success = BTreeMap::new();
        let mut expiry_index: BTreeMap<u64, BTreeSet<String>> = BTreeMap::new();
        let mut origins = BTreeMap::new();
        let mut stored = BTreeMap::new();
        for item in storage
            .db
            .iterator(IteratorMode::From(prefix, Direction::Forward))
        {
            let (key, value) = item?;
            if !key.starts_with(prefix) {
                break;
            }
            let text = std::str::from_utf8(&key).context("Recovery key is not UTF-8")?;
            if text == HEADER_KEY {
                header = Some(canonical::<StoredHeader>(&value, "header")?);
            } else if let Some(id) = text.strip_prefix(ACCOUNT_PREFIX) {
                accounts.insert(id.to_owned(), canonical(&value, "account")?);
            } else if let Some(id) = text.strip_prefix(RECEIPT_PREFIX) {
                sponsor_receipts.insert(id.to_owned(), canonical(&value, "receipt")?);
            } else if let Some(operation) = text.strip_prefix(OPERATION_PREFIX) {
                operation_success.insert(operation.to_owned(), canonical(&value, "success index")?);
            } else if let Some(id) = text.strip_prefix(ORIGIN_PREFIX) {
                origins.insert(id.to_owned(), canonical(&value, "origin")?);
            } else if let Some(rest) = text.strip_prefix(EXPIRY_PREFIX) {
                let (height, id) = rest.split_once(':').context("Invalid recovery expiry key")?;
                let parsed: u64 = height.parse().context("Invalid recovery expiry height")?;
                ensure!(
                    format!("{parsed:020}") == height && value.is_empty(),
                    "Recovery expiry entry is not canonical"
                );
                expiry_index.entry(parsed).or_default().insert(id.to_owned());
            } else {
                bail!("Unknown recovery state key");
            }
            stored.insert(key.to_vec(), value.to_vec());
        }
        let Some(header) = header else {
            ensure!(stored.is_empty(), "Recovery entries without a header");
            return Ok(None);
        };
        // A stored account keeps the height of its last material change. Advancing
        // it may change only that height: an expiry always rewrites the account.
        let mut advanced = BTreeMap::new();
        for (id, account) in accounts {
            let account: RecoveryAccount = account;
            ensure!(
                account.recovery.last_height <= header.last_height,
                "Recovery account is ahead of the book height"
            );
            let current = RecoveryAccount {
                recovery: account.recovery.advance_height(header.last_height)?,
                ..account.clone()
            };
            ensure!(
                same_apart_from_height(&account, &current),
                "Recovery account has an unapplied expiry"
            );
            advanced.insert(id, current);
        }
        let book = Self {
            version: header.version,
            last_height: header.last_height,
            profile: header.profile,
            accounts: advanced,
            sponsor_receipts,
            operation_success,
            expiry_index,
            origins,
        };
        let non_account = |entries: BTreeMap<Vec<u8>, Vec<u8>>| -> BTreeMap<Vec<u8>, Vec<u8>> {
            entries
                .into_iter()
                .filter(|(key, _)| !key.starts_with(ACCOUNT_PREFIX.as_bytes()))
                .collect()
        };
        ensure!(
            non_account(book.entries()?) == non_account(stored),
            "Recovery entries are not canonical"
        );
        Ok(Some(book))
    }
    pub(crate) fn begin_block(&self, height: u64) -> Result<RecoveryBlock> {
        self.validate()?;
        ensure!(
            height
                == self
                    .last_height
                    .checked_add(1)
                    .context("Recovery height overflow")?,
            "Recovery blocks must be consecutive"
        );
        {
            let mut book = self.clone();
            let due = self.expiry_index.get(&height).map_or(0, BTreeSet::len) as u64;
            let expiry_gas_used = due
                .checked_mul(self.profile.expiry_event_gas_cost)
                .context("Expiry budget overflow")?;
            ensure!(
                expiry_gas_used <= self.profile.mandatory_expiry_gas_budget,
                "Mandatory expiry exceeds reserved budget"
            );
            for account in book.accounts.values_mut() {
                account.recovery = account.recovery.advance_height(height)?;
            }
            book.last_height = height;
            book.expiry_index = book.expected_expiries()?;
            book.validate()?;
            Ok(RecoveryBlock {
                book,
                gas_used: 0,
                bytes_used: 0,
                signatures_used: 0,
                expiry_gas_used,
            })
        }
    }
}
/// Prior values of the book entries one proposal candidate may change.
pub(crate) struct BookCheckpoint {
    accounts: Vec<(String, Option<RecoveryAccount>)>,
    origins: Vec<(String, Option<KeyIdentity>)>,
    receipts: Vec<(String, Option<SponsorReceipt>)>,
    operations: Vec<(String, Option<String>)>,
    expiry_index: BTreeMap<u64, BTreeSet<String>>,
    sizes: (usize, usize, usize, usize),
    last_height: u64,
}
impl RecoveryBook {
    /// Capture the named entries (an account's origin with the account) and
    /// the (small, capacity-bounded) expiry index. `rollback` restores them and
    /// fails if any entry outside them was added or removed.
    pub(crate) fn checkpoint(
        &self,
        accounts: &[String],
        receipts: &[String],
        operations: &[String],
    ) -> BookCheckpoint {
        BookCheckpoint {
            accounts: accounts.iter().map(|k| (k.clone(), self.accounts.get(k).cloned())).collect(),
            origins: accounts.iter().map(|k| (k.clone(), self.origins.get(k).cloned())).collect(),
            receipts: receipts
                .iter()
                .map(|k| (k.clone(), self.sponsor_receipts.get(k).cloned()))
                .collect(),
            operations: operations
                .iter()
                .map(|k| (k.clone(), self.operation_success.get(k).cloned()))
                .collect(),
            expiry_index: self.expiry_index.clone(),
            sizes: (
                self.accounts.len(),
                self.sponsor_receipts.len(),
                self.operation_success.len(),
                self.origins.len(),
            ),
            last_height: self.last_height,
        }
    }
    pub(crate) fn rollback(&mut self, checkpoint: BookCheckpoint) -> Result<()> {
        fn restore<V>(map: &mut BTreeMap<String, V>, entries: Vec<(String, Option<V>)>) {
            for (key, value) in entries {
                match value {
                    Some(value) => map.insert(key, value),
                    None => map.remove(&key),
                };
            }
        }
        restore(&mut self.accounts, checkpoint.accounts);
        restore(&mut self.origins, checkpoint.origins);
        restore(&mut self.sponsor_receipts, checkpoint.receipts);
        restore(&mut self.operation_success, checkpoint.operations);
        self.expiry_index = checkpoint.expiry_index;
        ensure!(
            (
                self.accounts.len(),
                self.sponsor_receipts.len(),
                self.operation_success.len(),
                self.origins.len(),
            ) == checkpoint.sizes
                && self.last_height == checkpoint.last_height,
            "Recovery rollback does not restore the book"
        );
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub(crate) struct RecoveryBlock {
    pub book: RecoveryBook,
    pub gas_used: u64,
    pub bytes_used: u64,
    pub signatures_used: u64,
    pub expiry_gas_used: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RecoveryResult {
    pub success: bool,
    pub charged: bool,
    pub gas_used: u64,
    pub fee: u128,
    pub error: Option<String>,
    pub receipt: Option<SponsorReceipt>,
}
impl RecoveryResult {
    fn rejected(gas: u64, reason: impl Into<String>) -> Self {
        Self {
            success: false,
            charged: false,
            gas_used: gas,
            fee: 0,
            error: Some(reason.into()),
            receipt: None,
        }
    }
}
/// Reconcile one charged recovery against its staged native custody delta.
/// Recovery may charge the sponsor, but it may not change a native nonce or
/// any other native account balance at this execution boundary.
fn reconcile_sponsor_charge(
    before: &mut Settlement,
    after: &mut Settlement,
    sponsor: &RecoveryAccount,
    receipt: &SponsorReceipt,
    gas_price: u64,
) -> Result<()> {
    ensure!(
        receipt.sponsor_account_id == sponsor.recovery.domain.account_id
            && receipt.sponsor_counter_before == sponsor.sponsor_nonce
            && Some(receipt.sponsor_counter_after) == sponsor.sponsor_nonce.checked_add(1)
            && receipt.settled_fee
                == u128::from(receipt.gas_used)
                    .checked_mul(u128::from(gas_price))
                    .context("Recovery receipt fee overflow")?
            && receipt.reserved_cap.checked_sub(receipt.settled_fee)
                == Some(receipt.released_reserve),
        "Recovery receipt counter or fee differs from charge"
    );
    let prior_fee = before.ordinary_fee_total()?;
    ensure!(
        prior_fee.checked_add(receipt.settled_fee) == Some(after.ordinary_fee_total()?),
        "Recovery receipt differs from withheld fee delta"
    );
    let initial = before.account(&sponsor.address)?.clone();
    let final_account = after.account(&sponsor.address)?.clone();
    let mut expected = initial;
    expected.set_balance(
        "udrt",
        expected
            .balance_of("udrt")
            .checked_sub(receipt.settled_fee)
            .context("Recovery charge exceeds sponsor balance")?,
    );
    ensure!(
        final_account.nonce == expected.nonce && final_account.balances == expected.balances,
        "Recovery receipt differs from sponsor account delta"
    );
    ensure!(
        before.accounts.keys().eq(after.accounts.keys()),
        "Recovery charge changed native account set"
    );
    for (address, initial) in &before.accounts {
        if address == &sponsor.address {
            continue;
        }
        let final_account = &after.accounts[address];
        ensure!(
            final_account.nonce == initial.nonce && final_account.balances == initial.balances,
            "Recovery charge changed unrelated native account"
        );
    }
    Ok(())
}
fn add(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b).context("Recovery meter overflow")
}
fn cost(bytes: u64, price: u64) -> Result<u64> {
    bytes
        .checked_mul(price)
        .context("Recovery meter multiplication overflow")
}
fn action_index(action: &ActionKind) -> Result<usize> {
    Ok(match action {
        ActionKind::Enroll { .. } => 0,
        ActionKind::Rotate { .. } => 1,
        ActionKind::Start { .. } => 2,
        ActionKind::Finalize { .. } => 3,
        ActionKind::Cancel { .. } => 4,
        ActionKind::Resume { .. } => 5,
        ActionKind::StagePolicy { .. } => 6,
        ActionKind::ActivatePolicy { .. } => 7,
        ActionKind::CancelPolicy { .. } => 8,
        ActionKind::Spend { .. } => bail!("Internal Spend is not a complete external operation"),
    })
}
fn key_size(key: &KeyIdentity) -> u64 {
    3 + key.public_key.len() as u64
}
fn policy_size(policy: &RecoveryPolicy) -> Result<u64> {
    policy.guardians.iter().try_fold(3, |size, g| {
        add(
            size,
            add(key_size(&g.key), 2 + g.control_group.len() as u64)?,
        )
    })
}
/// Logical bytes, independent of JSON integer width, database layout and compression.
/// Domain, config, current key/counters/status, policies and pending terminal records.
pub(crate) fn logical_account_bytes(account: &RecoveryAccount) -> Result<u64> {
    let state = &account.recovery;
    let mut size =
        2 + account.address.len() as u64 + 8 + 1 + 2 + state.domain.chain_id.len() as u64 + 64;
    size = add(size, 6 * 8 + 1 + key_size(&state.active_key) + 6 * 8 + 1)?;
    for (name, _) in &state.config.algorithms {
        size = add(size, 2 + name.len() as u64 + 8)?;
    }
    size = add(size, 1)?;
    if let Some(policy) = &state.policy {
        size = add(size, policy_size(policy)?)?;
    }
    size = add(size, 2)?;
    if let Some(p) = &state.pending_recovery {
        size = add(size, 32 + key_size(&p.replacement) + 5 * 8)?;
    }
    if let Some(p) = &state.pending_policy {
        size = add(size, 32 + policy_size(&p.policy)? + 5 * 8)?;
    }
    Ok(size)
}
impl RecoveryBlock {
    fn consume(
        &mut self,
        gas: u64,
        bytes: u64,
        signatures: u64,
        shared: &mut Option<&mut crate::ordinary_meter::SharedBlockMeter>,
    ) -> Result<()> {
        let g = add(self.gas_used, gas)?;
        let b = add(self.bytes_used, bytes)?;
        let s = add(self.signatures_used, signatures)?;
        ensure!(
            g <= self.book.profile.max_block_gas
                && b <= self.book.profile.max_block_recovery_bytes
                && s <= self.book.profile.max_block_recovery_signatures,
            "Recovery block capacity exceeded"
        );
        // Check the combined ceiling before permitting the corresponding work.
        // Failure leaves this subtotal unchanged and requires block rejection.
        if let Some(meter) = shared.as_deref_mut() {
            meter.charge_recovery(gas, bytes, signatures)?;
        }
        self.gas_used = g;
        self.bytes_used = b;
        self.signatures_used = s;
        Ok(())
    }
    pub(crate) fn execute(
        &mut self,
        height: u64,
        index: u32,
        raw: &[u8],
        settlement: &mut Settlement,
    ) -> Result<RecoveryResult> {
        self.execute_inner(height, index, raw, settlement, None)
    }
    /// Combined ordinary/recovery profile. Each increment checks both the
    /// recovery subtotal and shared ceiling before signature or transition work.
    /// Mandatory expiry is charged separately once when the block starts.
    pub(crate) fn execute_with_shared(
        &mut self,
        height: u64,
        index: u32,
        raw: &[u8],
        settlement: &mut Settlement,
        shared: &mut crate::ordinary_meter::SharedBlockMeter,
    ) -> Result<RecoveryResult> {
        self.execute_inner(height, index, raw, settlement, Some(shared))
    }
    fn execute_inner(
        &mut self,
        height: u64,
        index: u32,
        raw: &[u8],
        settlement: &mut Settlement,
        mut shared: Option<&mut crate::ordinary_meter::SharedBlockMeter>,
    ) -> Result<RecoveryResult> {
        ensure!(
            height == self.book.last_height,
            "Recovery execution height mismatch"
        );
        let p = self.book.profile.clone();
        let byte_gas = cost(raw.len() as u64, p.wire_byte_cost)?;
        self.consume(byte_gas, raw.len() as u64, 0, &mut shared)?;
        let signed = match wire::decode(raw) {
            Ok(v) => v,
            Err(e) => return Ok(RecoveryResult::rejected(byte_gas, e.to_string())),
        };
        self.execute_decoded(
            height,
            index,
            raw,
            signed,
            byte_gas,
            settlement,
            &mut shared,
        )
    }
    fn execute_decoded(
        &mut self,
        height: u64,
        index: u32,
        _raw: &[u8],
        signed: SponsoredRecovery,
        byte_gas: u64,
        settlement: &mut Settlement,
        shared: &mut Option<&mut crate::ordinary_meter::SharedBlockMeter>,
    ) -> Result<RecoveryResult> {
        let p = self.book.profile.clone();
        if height < p.activation_height {
            return Ok(RecoveryResult::rejected(
                byte_gas,
                "Recovery fee profile not active",
            ));
        }
        let a = &signed.sponsor;
        let mut gas = byte_gas;
        let action_cost = p.action_costs[action_index(&signed.recovery.operation.action.kind)?];
        self.consume(action_cost, 0, 0, shared)?;
        gas = add(gas, action_cost)?;
        // Reserve all declared signature work before executing the verifier. This is
        // conservative when an early signature fails, and independent of CPU timing.
        let keys = signed
            .recovery
            .signatures
            .iter()
            .map(|s| &s.key)
            .chain(std::iter::once(&a.sponsor_key));
        let mut signature_gas = 0;
        for key in keys {
            let Some(price) = p.signature_costs.get(&key.algorithm) else {
                return Ok(RecoveryResult::rejected(
                    gas,
                    "Requested algorithm absent from fee profile",
                ));
            };
            signature_gas = add(signature_gas, *price)?;
        }
        self.consume(
            signature_gas,
            0,
            signed.recovery.signatures.len() as u64 + 1,
            shared,
        )?;
        gas = add(gas, signature_gas)?;
        macro_rules! reject {
            ($condition:expr,$reason:expr) => {
                if !$condition {
                    return Ok(RecoveryResult::rejected(gas, $reason));
                }
            };
        }
        reject!(
            a.fee_profile_version == p.version
                && a.fee_profile_digest == wire::profile_digest(&p)?
                && a.denomination == p.denomination,
            "Fee profile binding mismatch"
        );
        reject!(
            a.gas_limit > 0
                && a.gas_limit >= p.minimum_gas
                && a.gas_limit <= p.max_transaction_gas
                && gas <= a.gas_limit,
            "Validation exceeds gas limit"
        );
        let maximum = u128::from(a.gas_limit) * u128::from(p.gas_price);
        reject!(
            a.maximum_charge >= maximum && a.maximum_charge <= p.max_fee_cap,
            "Fee cap outside profile bounds"
        );
        reject!(
            height < a.expiry_height && height < signed.recovery.operation.action.submission_expiry,
            "Expired sponsorship or operation"
        );
        let facts = match verify_signed(&signed) {
            Ok(facts) => facts,
            Err(SponsorVerificationError::BackendUnavailable) => {
                bail!("Required recovery verifier backend unavailable")
            }
            Err(e) => return Ok(RecoveryResult::rejected(gas, e.to_string())),
        };
        let target_id = hex::encode(a.domain.account_id);
        let sponsor_id = hex::encode(a.sponsor_account_id);
        reject!(
            target_id != sponsor_id,
            "Recovery sponsor must be independent"
        );
        let Some(target) = self.book.accounts.get(&target_id).cloned() else {
            return Ok(RecoveryResult::rejected(gas, "Unknown recovery target"));
        };
        let Some(sponsor) = self.book.accounts.get(&sponsor_id).cloned() else {
            return Ok(RecoveryResult::rejected(gas, "Unknown recovery sponsor"));
        };
        let auth_id = wire::authorization_id(a)?;
        let auth_key = hex::encode(auth_id);
        let op_key = hex::encode(a.operation_id);
        reject!(
            !self.book.sponsor_receipts.contains_key(&auth_key)
                && !self.book.operation_success.contains_key(&op_key),
            "Consumed sponsorship or successful operation"
        );
        reject!(
            self.book.sponsor_receipts.len() < MAX_RECEIPTS,
            "Receipt retention capacity exhausted"
        );
        reject!(target.recovery.domain == a.domain, "Target domain mismatch");
        reject!(
            sponsor.recovery.outgoing_allowed()
                && sponsor.recovery.active_key == a.sponsor_key
                && sponsor.recovery.active_generation == a.sponsor_generation
                && sponsor.sponsor_nonce == a.sponsor_nonce
                && sponsor.sponsor_nonce < u64::MAX,
            "Sponsor current authority mismatch or protection active"
        );
        let read_bytes = add(
            add(
                logical_account_bytes(&target)?,
                logical_account_bytes(&sponsor)?,
            )?,
            32,
        )?; // native liquid and custody counters
        let read_gas = cost(read_bytes, p.read_byte_cost)?;
        self.consume(read_gas, 0, 0, shared)?;
        gas = add(gas, read_gas)?;
        reject!(
            gas <= a.gas_limit,
            "Validation state reads exceed gas limit"
        );
        let next = match target.recovery.transition(height, &facts.action, &facts) {
            Ok(v) => v,
            Err(e) => return Ok(RecoveryResult::rejected(gas, e.to_string())),
        };
        let mut next_target = target.clone();
        next_target.recovery = next.clone();
        let expiry_index = self.book.expiry_index_with(&target_id, &next_target)?;
        reject!(
            self.book.capacity_available(&expiry_index)?,
            "Future expiry capacity unavailable"
        );
        let mut native = settlement.clone();
        reject!(
            native.account(&sponsor.address)?.balance_of("udrt") >= a.maximum_charge,
            "Insufficient eligible sponsor liquidity"
        );
        // Fee acceptance boundary. All authority, timing and capacity checks passed.
        // Only deterministic final write metering can produce the charged failure.
        let mut next_sponsor = sponsor.clone();
        next_sponsor.sponsor_nonce += 1;
        let write_bytes = add(
            add(
                logical_account_bytes(&next_target)?,
                logical_account_bytes(&next_sponsor)?,
            )?,
            32,
        )?;
        let write_gas = cost(write_bytes, p.write_byte_cost)?;
        let requested = add(gas, write_gas)?;
        let success = requested <= a.gas_limit;
        let final_gas = if success {
            requested.max(p.minimum_gas)
        } else {
            a.gas_limit
        };
        self.consume(
            final_gas
                .checked_sub(gas)
                .context("Gas acceptance invariant")?,
            0,
            0,
            shared,
        )?;
        let fee = u128::from(final_gas) * u128::from(p.gas_price);
        let mut receipt = SponsorReceipt {
            operation_id: a.operation_id,
            sponsor_authorization_id: auth_id,
            envelope_hash: wire::envelope_hash(&signed)?,
            sponsor_account_id: a.sponsor_account_id,
            target_account_id: a.domain.account_id,
            block_height: height,
            block_index: index,
            success,
            profile_version: p.version,
            profile_digest: wire::profile_digest(&p)?,
            gas_limit: a.gas_limit,
            gas_used: final_gas,
            reserved_cap: a.maximum_charge,
            settled_fee: fee,
            released_reserve: a
                .maximum_charge
                .checked_sub(fee)
                .context("Fee exceeds reservation")?,
            sponsor_counter_before: sponsor.sponsor_nonce,
            sponsor_counter_after: next_sponsor.sponsor_nonce,
            target_state_digest: sha3_256(&serde_json::to_vec(if success {
                &next
            } else {
                &target.recovery
            })?),
            record_hash: [0; 32],
        };
        receipt.record_hash = receipt.hash()?;
        // Check the entries this operation changes before mutating anything.
        // The whole book is validated at block start and again when the
        // block's recovery changes are staged.
        if success {
            next.validate()?;
            ensure!(
                next.last_height == self.book.last_height,
                "Recovery account height differs from book"
            );
        }
        self.book.check_receipt(
            &auth_key,
            &receipt,
            &wire::profile_digest(&p)?,
            next_sponsor.sponsor_nonce,
            success.then_some(&auth_key),
        )?;
        let mut baseline = settlement.clone();
        native.charge_sponsored(&sponsor.address, fee)?;
        reconcile_sponsor_charge(&mut baseline, &mut native, &sponsor, &receipt, p.gas_price)?;
        // Publish in place; nothing below can fail.
        if success {
            self.book.accounts.insert(target_id, next_target);
            self.book.expiry_index = expiry_index;
            self.book.operation_success.insert(op_key, auth_key.clone());
        }
        self.book.accounts.insert(sponsor_id, next_sponsor);
        self.book.sponsor_receipts.insert(auth_key, receipt.clone());
        *settlement = native;
        #[cfg(test)]
        {
            assert_eq!(self.book.expiry_index, self.book.expected_expiries()?);
            self.book.validate()?;
        }
        Ok(RecoveryResult {
            success,
            charged: true,
            gas_used: final_gas,
            fee,
            error: if success {
                None
            } else {
                Some("Accepted recovery out of gas".into())
            },
            receipt: Some(receipt),
        })
    }
}
/// Queue-only reservations. No chain state changes or anticipated credits.
#[derive(Clone, Debug, Default)]
pub(crate) struct RecoveryReservations {
    ordinary: BTreeMap<String, u128>,
    sponsors: BTreeMap<String, (String, u64, u128)>,
    nonces: BTreeMap<(String, u64), String>,
}
impl RecoveryReservations {
    fn reserved(&self, address: &str) -> Result<u128> {
        self.sponsors
            .values()
            .filter(|(a, _, _)| a == address)
            .try_fold(
                self.ordinary.get(address).copied().unwrap_or(0),
                |total, (_, _, cap)| {
                    total
                        .checked_add(*cap)
                        .context("Queue reservation overflow")
                },
            )
    }
    pub(crate) fn reserve_ordinary(
        &mut self,
        address: &str,
        amount: u128,
        liquid: u128,
    ) -> Result<()> {
        let total = self
            .reserved(address)?
            .checked_add(amount)
            .context("Queue reservation overflow")?;
        ensure!(total <= liquid, "Queue exceeds current liquid balance");
        let old = self.ordinary.get(address).copied().unwrap_or(0);
        self.ordinary.insert(
            address.into(),
            old.checked_add(amount)
                .context("Ordinary reservation overflow")?,
        );
        Ok(())
    }
    pub(crate) fn reserve_sponsor(
        &mut self,
        id: [u8; 32],
        address: &str,
        nonce: u64,
        cap: u128,
        liquid: u128,
    ) -> Result<bool> {
        let id = hex::encode(id);
        let value = (address.to_owned(), nonce, cap);
        if let Some(old) = self.sponsors.get(&id) {
            ensure!(*old == value, "Queue authorization identity mismatch");
            return Ok(false);
        }
        ensure!(
            !self.nonces.contains_key(&(address.into(), nonce)),
            "Sponsor nonce already reserved"
        );
        ensure!(
            self.reserved(address)?
                .checked_add(cap)
                .context("Queue reservation overflow")?
                <= liquid,
            "Queue exceeds current liquid balance"
        );
        self.nonces.insert((address.into(), nonce), id.clone());
        self.sponsors.insert(id, value);
        Ok(true)
    }
}
#[cfg(test)]
#[path = "recovery_fees_tests.rs"]
mod tests;

