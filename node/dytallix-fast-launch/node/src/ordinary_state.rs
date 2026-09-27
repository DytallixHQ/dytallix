//! Durable ordinary-v2 state for explicit fresh local combined genesis.
//! This module prepares bytes in the caller's block batch. It never writes a DB.
//! Versioned grants are authoritative. Legacy DMS configurations are not migrated.
use crate::{
    block_lifecycle::Writes,
    ordinary_authority::{self, Grants},
    ordinary_fee_settlement::FeeHistory,
    ordinary_reservations::QueueLimits,
    recovery_fees::{RecoveryAccount, RecoveryBook},
    runtime::validator_lifecycle::LifecycleConfig,
    storage::state::Storage,
};
use anyhow::{ensure, Context, Result};
use dytallix_protocol_types::{
    address::{AccountAddress, AddressNetwork},
    ordinary_fees::{self, FeeProfile},
    recovery::RecoveryConfig,
    sha3_256,
};
use rocksdb::{Direction, IteratorMode};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub const STATE_KEY: &str = "ordinary:v1:state";
const STATE_PREFIX: &[u8] = b"ordinary:";
const LEGACY_DMS_PREFIX: &[u8] = b"dms:config:";
const COMPONENT_HISTORY_BOUND: u32 = 65_536;

/// All values are explicit local inputs. This configuration does not provide a
/// migration route or a production activation flag.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrdinaryConfig {
    pub version: u16,
    pub fee_profile: FeeProfile,
    pub account_template: AccountTemplate,
    pub(crate) initial_grants: Grants,
    pub max_state_bytes: u64,
    pub max_grants: u32,
    pub max_receipts: u32,
    pub max_retained_profiles: u32,
    pub max_transport_bytes: u64,
    pub queue_max_entries: u32,
    pub queue_max_wire_bytes: u64,
    pub queue_max_signature_work: u64,
}
/// Recovery settings given to an account created by receiving, when its first
/// transaction initializes it (B1c). The values are genesis inputs; the chain
/// domain is the recovery book's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountTemplate {
    pub recovery: RecoveryConfig,
}
/// The existing lifecycle role permits pure ML-DSA-65 only. Bind its full
/// validated config (including chain, owners, and limits), fixed role, key size,
/// signature size and verification mode into a domain-separated role digest.
/// JSON follows LifecycleConfig's fixed field order and sorted operator map.
/// Changing any lifecycle field requires an explicit new matching fee profile.
pub(crate) fn validator_profile_digest(lifecycle: &LifecycleConfig) -> Result<[u8; 32]> {
    lifecycle.validate()?;
    let mut bytes = b"DYTALLIX/ORDINARY-VALIDATOR-PROOF-PROFILE\0".to_vec();
    bytes.extend_from_slice(&1u16.to_be_bytes());
    bytes.extend_from_slice(&1u16.to_be_bytes()); // Exact mldsa65 registry code.
    bytes.extend_from_slice(&1952u32.to_be_bytes());
    bytes.extend_from_slice(&3309u32.to_be_bytes());
    bytes.extend_from_slice(b"PURE-ML-DSA/EMPTY-CONTEXT\0");
    // The exact role, less the values governance may change (T6): operators,
    // the minimum self-bond and the active-set bound.
    let mut role = lifecycle.clone();
    role.approved_operators.clear();
    role.min_self_bond = 0;
    role.max_active = 0;
    let config = serde_json::to_vec(&role)?;
    bytes.extend_from_slice(
        &u32::try_from(config.len())
            .context("Lifecycle role length overflow")?
            .to_be_bytes(),
    );
    bytes.extend_from_slice(&config);
    Ok(sha3_256(&bytes))
}
fn network(value: u8) -> Result<AddressNetwork> {
    match value {
        1 => Ok(AddressNetwork::Mainnet),
        2 => Ok(AddressNetwork::Testnet),
        3 => Ok(AddressNetwork::Development),
        _ => anyhow::bail!("Unsupported ordinary origin network"),
    }
}
impl OrdinaryConfig {
    /// Public limits and prices. Origin and grant inventories remain separate.
    pub(crate) fn client_view(
        &self,
    ) -> dytallix_protocol_types::ordinary_client::PublicOrdinaryConfig {
        dytallix_protocol_types::ordinary_client::PublicOrdinaryConfig {
            version: self.version,
            fee_profile: self.fee_profile.clone(),
            max_state_bytes: self.max_state_bytes,
            max_grants: self.max_grants,
            max_receipts: self.max_receipts,
            max_retained_profiles: self.max_retained_profiles,
            max_transport_bytes: self.max_transport_bytes,
            queue_max_entries: self.queue_max_entries,
            queue_max_wire_bytes: self.queue_max_wire_bytes,
            queue_max_signature_work: self.queue_max_signature_work,
        }
    }
    pub(crate) fn validate(&self, lifecycle: &LifecycleConfig, book: &RecoveryBook) -> Result<()> {
        ensure!(
            self.version == 1,
            "Unsupported ordinary configuration version"
        );
        ensure!(
            cfg!(feature = "pqc-fips204"),
            "Combined ordinary execution requires the FIPS 204 backend"
        );
        self.fee_profile.validate()?;
        book.validate()?;
        lifecycle.validate()?;
        ensure!(
            self.max_state_bytes > 0
                && self.max_grants > 0
                && self.max_receipts > 0
                && self.max_receipts <= COMPONENT_HISTORY_BOUND
                && self.max_retained_profiles > 0
                && self.max_retained_profiles <= COMPONENT_HISTORY_BOUND
                && self.max_transport_bytes > 0,
            "Explicit ordinary storage and transport bounds required"
        );
        usize::try_from(self.max_state_bytes).context("Ordinary state bound exceeds platform")?;
        usize::try_from(self.max_transport_bytes)
            .context("Ordinary transport bound exceeds platform")?;
        self.queue_limits()?;
        ensure!(
            self.fee_profile.max_transaction_gas <= i64::MAX as u64
                && self.fee_profile.max_block_transaction_gas <= i64::MAX as u64,
            "Ordinary gas exceeds consensus result range"
        );
        self.fee_profile.validate_validator_profile(
            validator_profile_digest(lifecycle)?,
            &BTreeSet::from(["mldsa65".to_owned()]),
        )?;
        ensure!(
            book.origins.all()?.len() == book.accounts.all()?.len(),
            "Explicit origin records must cover exactly the registered accounts"
        );
        self.validate_template(book)?;
        let mut addresses = BTreeSet::new();
        for (id, account) in book.accounts.all()? {
            self.validate_account(lifecycle, book, id, account)?;
            addresses.insert(account.address.as_str());
        }
        for owner in lifecycle.approved_operators.values() {
            ensure!(
                addresses.contains(owner.as_str()),
                "Lifecycle operator lacks a registered stable ordinary account"
            );
        }
        ensure!(
            self.initial_grants.len() <= self.max_grants as usize,
            "Initial ordinary grant capacity exceeded"
        );
        ordinary_authority::validate_grants(book, &self.initial_grants)?;
        // Explicit initial grants are genesis facts, never migrated activity.
        ensure!(
            self.initial_grants
                .values()
                .all(|g| g.last_active_height == 0),
            "Initial ordinary grant is not a genesis record"
        );
        Ok(())
    }
    /// The account template must be usable by both the recovery and the
    /// ordinary account roles, and ML-DSA-65 only on mainnet.
    fn validate_template(&self, book: &RecoveryBook) -> Result<()> {
        let template = &self.account_template.recovery;
        template.validate()?;
        let network = book.chain()?.network;
        for (algorithm, length) in &template.algorithms {
            let expected = match algorithm.as_str() {
                "mldsa65" => 1952,
                "mldsa87" => 2592,
                _ => anyhow::bail!("Unsupported account template algorithm"),
            };
            ensure!(
                *length == expected
                    && book.profile.signature_costs.contains_key(algorithm)
                    && self.fee_profile.limits.allowed_algorithms.contains(algorithm),
                "Account template algorithm outside the configured account roles"
            );
            ensure!(
                network != 1 || algorithm == "mldsa65",
                "Mainnet account template requires ML-DSA-65 exclusively"
            );
        }
        Ok(())
    }
    /// Account-level configuration rules for one registered account.
    fn validate_account(
        &self,
        lifecycle: &LifecycleConfig,
        book: &RecoveryBook,
        id: &str,
        account: &RecoveryAccount,
    ) -> Result<()> {
        let d = &account.recovery.domain;
        ensure!(
            d.chain_id == lifecycle.chain_id && !d.chain_id.is_empty() && d.chain_id.len() <= 128,
            "Ordinary lifecycle/chain domain mismatch"
        );
        if d.network == 1 {
            ensure!(
                self.fee_profile
                    .limits
                    .allowed_algorithms
                    .iter()
                    .all(|a| a == "mldsa65")
                    && account
                        .recovery
                        .config
                        .algorithms
                        .keys()
                        .all(|a| a == "mldsa65"),
                "Mainnet operational profile requires ML-DSA-65 exclusively"
            );
        }
        let key = book
            .origins
            .find(id)?
            .context("Ordinary origin record missing")?;
        let address = AccountAddress::from_origin_key(
            network(d.network)?,
            &d.chain_id,
            ordinary_authority::origin_algorithm(&key.algorithm)?,
            &key.public_key,
        )?;
        ensure!(
            address.account_id() == &d.account_id && address.encode() == account.address,
            "Ordinary origin differs from stable ID or native address"
        );
        ensure!(
            self.fee_profile
                .limits
                .allowed_algorithms
                .contains(&account.recovery.active_key.algorithm),
            "Current ordinary key is outside selected account role"
        );
        // RecoveryConfig uses one algorithm map for all recovery keys. Every
        // entry can become a replacement active key. Reject an incompatible
        // combined profile before activation, rather than accepting a key
        // change that would make its final ordinary state invalid. This does
        // not select additional ordinary algorithms or change guardian roles.
        ensure!(
            account.recovery.config.algorithms.keys().all(|algorithm| self
                .fee_profile
                .limits
                .allowed_algorithms
                .contains(algorithm)),
            "Configured recovery replacement algorithms exceed the selected ordinary account role"
        );
        Ok(())
    }
    pub(crate) fn queue_limits(&self) -> Result<QueueLimits> {
        ensure!(
            self.queue_max_entries > 0
                && self.queue_max_wire_bytes > 0
                && self.queue_max_signature_work > 0,
            "Explicit ordinary queue capacities required"
        );
        Ok(QueueLimits {
            max_entries: usize::try_from(self.queue_max_entries)
                .context("Queue entry bound overflow")?,
            max_wire_bytes: self.queue_max_wire_bytes,
            max_signature_work: self.queue_max_signature_work,
            max_action_debits_per_entry: usize::from(self.fee_profile.limits.max_actions)
                .checked_mul(2)
                .context("Queue debit bound overflow")?,
        })
    }
}
/// Prior values of the state entries one ordinary candidate may change.
pub(crate) struct StateCheckpoint {
    grant: (String, Option<crate::ordinary_authority::DiscretionaryGrant>),
    grants: usize,
    history: crate::ordinary_fee_settlement::HistoryCheckpoint,
    last_height: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OrdinaryState {
    pub version: u16,
    pub config: OrdinaryConfig,
    pub grants: Grants,
    pub history: FeeHistory,
    pub last_height: u64,
}
impl OrdinaryState {
    pub(crate) fn genesis(
        config: OrdinaryConfig,
        lifecycle: &LifecycleConfig,
        book: &RecoveryBook,
        native_nonces: &BTreeMap<String, u64>,
    ) -> Result<Self> {
        config.validate(lifecycle, book)?;
        ensure!(
            book.last_height == 0
                && book.sponsor_receipts.is_empty()
                && book.operation_success.is_empty()
                && book.expiry_index.is_empty(),
            "Ordinary activation requires fresh recovery genesis"
        );
        ensure!(
            book.accounts.all()?.values().all(|a| a.sponsor_nonce == 0
                && a.recovery.pending_recovery.is_none()
                && a.recovery.pending_policy.is_none()),
            "Ordinary genesis cannot migrate pending recovery work"
        );
        let state = Self {
            version: 1,
            grants: config.initial_grants.clone(),
            config,
            history: FeeHistory::default(),
            last_height: 0,
        };
        state.validate(lifecycle, book, native_nonces)?;
        Ok(state)
    }
    /// Capture what one ordinary candidate may change: the actor's grant and
    /// its receipt and profile entries. The configuration never changes.
    pub(crate) fn checkpoint(&self, actor: &str, receipt: &str, profile: &str) -> StateCheckpoint {
        StateCheckpoint {
            grant: (actor.to_owned(), self.grants.get(actor).cloned()),
            grants: self.grants.len(),
            history: self.history.checkpoint(receipt, profile),
            last_height: self.last_height,
        }
    }
    pub(crate) fn rollback(&mut self, checkpoint: StateCheckpoint) -> Result<()> {
        let (key, value) = checkpoint.grant;
        match value {
            Some(value) => self.grants.insert(key, value),
            None => self.grants.remove(&key),
        };
        self.history
            .rollback(checkpoint.history)
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        ensure!(
            self.grants.len() == checkpoint.grants && self.last_height == checkpoint.last_height,
            "Ordinary rollback does not restore the state"
        );
        Ok(())
    }
    /// Retained receipts cover this many blocks: the fee profile's maximum
    /// transaction lifetime. The profile cannot change without a migration.
    pub(crate) fn receipt_window(&self) -> u64 {
        self.config.fee_profile.limits.max_expiry_lifetime
    }
    /// Apply the receipt window for block `height` before executing it.
    pub(crate) fn prune_receipts_for(&mut self, height: u64) {
        let window = self.receipt_window();
        self.history.prune_expired(height, window);
    }
    pub(crate) fn validate(
        &self,
        lifecycle: &LifecycleConfig,
        book: &RecoveryBook,
        native_nonces: &BTreeMap<String, u64>,
    ) -> Result<()> {
        ensure!(self.version == 1, "Unsupported ordinary state version");
        self.config.validate(lifecycle, book)?;
        ensure!(
            self.last_height == book.last_height,
            "Ordinary/recovery state heights differ"
        );
        ensure!(
            native_nonces.len() == book.accounts.all()?.len(),
            "Native nonce mirrors must cover exactly the registered ordinary accounts"
        );
        ordinary_authority::validate_nonce_mirrors(book, native_nonces)?;
        ordinary_authority::validate_grants(book, &self.grants)?;
        self.validate_history(book)?;
        self.encode()?;
        Ok(())
    }
    /// End-of-block check. The committed state passed `validate` at genesis and
    /// passes it again in every complete check. Within a block only the accounts
    /// in `changed` can differ (every account whose recovery record, nonce or
    /// grant changed is loaded into the block's settlement), so account-level
    /// rules run for those accounts only. `native_nonces` must cover them.
    pub(crate) fn validate_block(
        &self,
        lifecycle: &LifecycleConfig,
        book: &RecoveryBook,
        native_nonces: &BTreeMap<String, u64>,
        changed: &BTreeSet<[u8; 32]>,
    ) -> Result<()> {
        ensure!(self.version == 1, "Unsupported ordinary state version");
        ensure!(
            self.last_height == book.last_height,
            "Ordinary/recovery state heights differ"
        );
        for id in changed {
            let key = hex::encode(id);
            if let Some(account) = book.accounts.find(&key)? {
                self.config.validate_account(lifecycle, book, &key, &account)?;
            }
        }
        ordinary_authority::validate_nonce_mirrors_for(book, native_nonces, changed)?;
        ordinary_authority::validate_grants_for(book, &self.grants, changed)?;
        self.validate_history(book)?;
        self.encode()?;
        Ok(())
    }
    fn validate_history(&self, book: &RecoveryBook) -> Result<()> {
        self.history
            .validate()
            .map_err(|e| anyhow::anyhow!("Ordinary history invalid: {e:?}"))?;
        let current = ordinary_fees::profile_digest(&self.config.fee_profile)?;
        ensure!(
            self.history.profiles().len() <= self.config.max_retained_profiles as usize,
            "Ordinary profile retention capacity exceeded"
        );
        for (digest, profile) in self.history.profiles() {
            ensure!(
                *digest == hex::encode(current) && *profile == self.config.fee_profile,
                "Unapproved ordinary fee profile replacement or migration"
            );
        }
        let mut positions: BTreeSet<(u64, u32)> = book
            .sponsor_receipts
            .values()
            .map(|r| (r.block_height, r.block_index))
            .collect();
        let mut count = 0usize;
        for receipt in self.history.receipts() {
            count = count
                .checked_add(1)
                .context("Ordinary receipt count overflow")?;
            ensure!(
                receipt.block_height() <= self.last_height,
                "Ordinary retained receipt exceeds committed authority"
            );
            // A staged book checks the actors of this block's receipts: an
            // older receipt was checked when its block was staged, and nonces
            // only grow. A complete book (the complete check) checks them all.
            if book.accounts.is_complete() || receipt.block_height() == self.last_height {
                let actor = book
                    .accounts
                    .find(&hex::encode(receipt.actor()))?
                    .context("Ordinary receipt actor missing")?;
                ensure!(
                    receipt.nonce_after() <= actor.recovery.spending_nonce,
                    "Ordinary retained receipt exceeds committed authority"
                );
            }
            ensure!(
                receipt.block_height() >= self.config.fee_profile.activation_height,
                "Ordinary receipt predates profile activation"
            );
            ensure!(
                receipt.block_height().saturating_add(self.receipt_window()) > self.last_height,
                "Ordinary receipt outside the retention window"
            );
            ensure!(
                receipt.profile_digest() == current,
                "Ordinary receipt profile differs from retained committed profile"
            );
            ensure!(
                positions.insert((receipt.block_height(), receipt.block_index())),
                "Ordinary/recovery receipts occupy the same block position"
            );
        }
        ensure!(
            count <= self.config.max_receipts as usize,
            "Ordinary receipt capacity exceeded"
        );
        Ok(())
    }
    /// Structural encoding only. The block caller must first validate the state
    /// against its staged recovery book, native nonce mirrors and lifecycle config.
    pub(crate) fn encode(&self) -> Result<Vec<u8>> {
        ensure!(
            self.version == 1 && self.config.version == 1,
            "Unsupported ordinary state/configuration version"
        );
        ensure!(
            self.grants.len() <= self.config.max_grants as usize,
            "Ordinary grant capacity exceeded"
        );
        self.history
            .validate()
            .map_err(|e| anyhow::anyhow!("Ordinary history invalid: {e:?}"))?;
        let bytes = serde_json::to_vec(self)?;
        ensure!(
            u64::try_from(bytes.len()).context("Ordinary state length overflow")?
                <= self.config.max_state_bytes,
            "Ordinary state byte capacity exceeded"
        );
        Ok(bytes)
    }
    pub(crate) fn decode(
        bytes: &[u8],
        expected: &OrdinaryConfig,
        lifecycle: &LifecycleConfig,
        book: &RecoveryBook,
        native_nonces: &BTreeMap<String, u64>,
    ) -> Result<Self> {
        let value = Self::parse(bytes, expected)?;
        value.validate(lifecycle, book, native_nonces)?;
        value.canonical(bytes)?;
        Ok(value)
    }
    /// Decode state this node committed. Every block validated its changed
    /// accounts before commit, and the complete check validates everything, so
    /// only structure, configuration, height and retained history are checked.
    pub(crate) fn decode_committed(
        bytes: &[u8],
        expected: &OrdinaryConfig,
        book: &RecoveryBook,
    ) -> Result<Self> {
        let value = Self::parse(bytes, expected)?;
        ensure!(value.version == 1, "Unsupported ordinary state version");
        ensure!(
            value.last_height == book.last_height,
            "Ordinary/recovery state heights differ"
        );
        value.validate_history(book)?;
        value.canonical(bytes)?;
        Ok(value)
    }
    fn parse(bytes: &[u8], expected: &OrdinaryConfig) -> Result<Self> {
        ensure!(
            u64::try_from(bytes.len()).context("Ordinary state length overflow")?
                <= expected.max_state_bytes,
            "Ordinary state exceeds configured read bound"
        );
        let value: Self =
            serde_json::from_slice(bytes).context("Invalid ordinary state encoding")?;
        ensure!(
            &value.config == expected,
            "Stored ordinary configuration differs from committed consensus configuration"
        );
        Ok(value)
    }
    fn canonical(&self, bytes: &[u8]) -> Result<()> {
        ensure!(
            self.encode()? == bytes,
            "Noncanonical ordinary state encoding"
        );
        Ok(())
    }
}
fn no_legacy_grants(storage: &Storage) -> Result<()> {
    if let Some(entry) = storage
        .db
        .iterator(IteratorMode::From(LEGACY_DMS_PREFIX, Direction::Forward))
        .next()
    {
        let (key, _) = entry?;
        ensure!(
            !key.starts_with(LEGACY_DMS_PREFIX),
            "Combined ordinary profile rejects legacy DMS state; explicit migration required"
        );
    }
    Ok(())
}
pub(crate) fn assert_absent(storage: &Storage) -> Result<()> {
    no_legacy_grants(storage)?;
    if let Some(entry) = storage
        .db
        .iterator(IteratorMode::From(STATE_PREFIX, Direction::Forward))
        .next()
    {
        let (key, _) = entry?;
        ensure!(
            !key.starts_with(STATE_PREFIX),
            "Fresh ordinary genesis rejects existing ordinary state"
        );
    }
    Ok(())
}
/// A native address that decodes on some network and re-encodes unchanged.
pub(crate) fn canonical_account_address(address: &str) -> bool {
    [AddressNetwork::Mainnet, AddressNetwork::Testnet, AddressNetwork::Development]
        .into_iter()
        .any(|network| {
            AccountAddress::decode(network, address).is_ok_and(|decoded| decoded.encode() == address)
        })
}
/// Complete check of native account records under the combined profile.
/// Balance and nonce records come in pairs under canonical addresses, and an
/// account without a recovery record (created by receiving) has nonce 0.
/// Registered accounts' nonces are checked against their recovery records.
pub(crate) fn validate_native_accounts(storage: &Storage, book: &RecoveryBook) -> Result<()> {
    let scan = |prefix: &[u8]| -> Result<BTreeMap<String, Vec<u8>>> {
        let mut records = BTreeMap::new();
        for entry in storage.db.iterator(IteratorMode::From(prefix, Direction::Forward)) {
            let (key, value) = entry?;
            let Some(address) = key.strip_prefix(prefix) else {
                break;
            };
            let address = std::str::from_utf8(address).context("Native account key is not UTF-8")?;
            records.insert(address.to_owned(), value.to_vec());
        }
        Ok(records)
    };
    let balances = scan(b"acct:balances:")?;
    let nonces = scan(b"acct:nonce:")?;
    ensure!(
        balances.keys().eq(nonces.keys()),
        "Native balance and nonce records differ"
    );
    for (address, raw) in &nonces {
        ensure!(
            canonical_account_address(address),
            "Native account address is not canonical"
        );
        if book.account_by_address(address)?.is_none() {
            let nonce: u64 = bincode::deserialize(raw).context("Invalid native nonce")?;
            ensure!(
                nonce == 0 && bincode::serialize(&nonce)? == *raw,
                "Account without a recovery record has a nonzero nonce"
            );
        }
    }
    Ok(())
}
pub(crate) fn read_native_nonces(
    storage: &Storage,
    book: &RecoveryBook,
) -> Result<BTreeMap<String, u64>> {
    let mut values = BTreeMap::new();
    for account in book.accounts.all()?.values() {
        let key = format!("acct:nonce:{}", account.address);
        let raw = storage
            .db
            .get(&key)?
            .context("Explicit ordinary native nonce mirror missing")?;
        let nonce =
            bincode::deserialize::<u64>(&raw).context("Invalid ordinary native nonce mirror")?;
        ensure!(
            bincode::serialize(&nonce)?.as_slice() == raw.as_slice(),
            "Noncanonical ordinary native nonce mirror"
        );
        values.insert(account.address.clone(), nonce);
    }
    Ok(values)
}
pub(crate) fn load(
    storage: &Storage,
    expected: &OrdinaryConfig,
    lifecycle: &LifecycleConfig,
    book: &RecoveryBook,
    native_nonces: &BTreeMap<String, u64>,
) -> Result<OrdinaryState> {
    OrdinaryState::decode(&stored(storage)?, expected, lifecycle, book, native_nonces)
}
/// `load` without the whole-account checks; see `decode_committed`.
pub(crate) fn load_committed(
    storage: &Storage,
    expected: &OrdinaryConfig,
    book: &RecoveryBook,
) -> Result<OrdinaryState> {
    OrdinaryState::decode_committed(&stored(storage)?, expected, book)
}
fn stored(storage: &Storage) -> Result<Vec<u8>> {
    no_legacy_grants(storage)?;
    for entry in storage
        .db
        .iterator(IteratorMode::From(STATE_PREFIX, Direction::Forward))
    {
        let (key, _) = entry?;
        if !key.starts_with(STATE_PREFIX) {
            break;
        }
        ensure!(
            key.as_ref() == STATE_KEY.as_bytes(),
            "Unexpected ordinary state namespace"
        );
    }
    storage
        .db
        .get(STATE_KEY)?
        .context("Configured ordinary state is missing; no automatic initialization")
}
/// Add the complete ordinary record to the caller's atomic block writes. This
/// function cannot commit and rejects a conflicting preexisting staged write.
pub(crate) fn append_writes(state: &OrdinaryState, writes: &mut Writes) -> Result<()> {
    let bytes = state.encode()?;
    if let Some(prior) = writes.get(STATE_KEY.as_bytes()) {
        ensure!(*prior == bytes, "Conflicting staged ordinary state write");
    }
    writes.insert(STATE_KEY.as_bytes().to_vec(), bytes);
    Ok(())
}
#[cfg(test)]
#[path = "ordinary_state_tests.rs"]
mod tests;
