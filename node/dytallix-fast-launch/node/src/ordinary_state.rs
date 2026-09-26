//! Durable ordinary-v2 state for explicit fresh local combined genesis.
//! This module prepares bytes in the caller's block batch. It never writes a DB.
//! Versioned grants are authoritative. Legacy DMS configurations are not migrated.
use crate::{
    block_lifecycle::Writes,
    ordinary_authority::{self, Grants},
    ordinary_fee_settlement::FeeHistory,
    ordinary_reservations::QueueLimits,
    recovery_fees::RecoveryBook,
    runtime::validator_lifecycle::LifecycleConfig,
    storage::state::Storage,
};
use anyhow::{ensure, Context, Result};
use dytallix_protocol_types::{
    address::{AccountAddress, AddressNetwork, OriginKeyAlgorithm},
    ordinary_fees::{self, FeeProfile},
    recovery::KeyIdentity,
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
    pub origins: BTreeMap<String, KeyIdentity>,
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
    let config = serde_json::to_vec(lifecycle)?;
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
fn origin_algorithm(value: &str) -> Result<OriginKeyAlgorithm> {
    match value {
        "mldsa65" => Ok(OriginKeyAlgorithm::MlDsa65),
        "mldsa87" => Ok(OriginKeyAlgorithm::MlDsa87),
        _ => anyhow::bail!("Unsupported exact ordinary origin algorithm"),
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
            self.origins.len() == book.accounts.len(),
            "Explicit origin records must cover exactly the registered accounts"
        );
        let mut addresses = BTreeSet::new();
        for (id, account) in &book.accounts {
            let d = &account.recovery.domain;
            ensure!(
                d.chain_id == lifecycle.chain_id
                    && !d.chain_id.is_empty()
                    && d.chain_id.len() <= 128,
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
            let key = self
                .origins
                .get(id)
                .context("Ordinary origin record missing")?;
            let address = AccountAddress::from_origin_key(
                network(d.network)?,
                &d.chain_id,
                origin_algorithm(&key.algorithm)?,
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
            book.accounts.values().all(|a| a.sponsor_nonce == 0
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
            native_nonces.len() == book.accounts.len(),
            "Native nonce mirrors must cover exactly the registered ordinary accounts"
        );
        ordinary_authority::validate_nonce_mirrors(book, native_nonces)?;
        ordinary_authority::validate_grants(book, &self.grants)?;
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
            let actor = book
                .accounts
                .get(&hex::encode(receipt.actor()))
                .context("Ordinary receipt actor missing")?;
            ensure!(
                receipt.block_height() <= self.last_height
                    && receipt.nonce_after() <= actor.recovery.spending_nonce,
                "Ordinary retained receipt exceeds committed authority"
            );
            ensure!(
                receipt.block_height() >= self.config.fee_profile.activation_height,
                "Ordinary receipt predates profile activation"
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
        value.validate(lifecycle, book, native_nonces)?;
        ensure!(
            value.encode()? == bytes,
            "Noncanonical ordinary state encoding"
        );
        Ok(value)
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
pub(crate) fn read_native_nonces(
    storage: &Storage,
    book: &RecoveryBook,
) -> Result<BTreeMap<String, u64>> {
    let mut values = BTreeMap::new();
    for account in book.accounts.values() {
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
    let bytes = storage
        .db
        .get(STATE_KEY)?
        .context("Configured ordinary state is missing; no automatic initialization")?;
    OrdinaryState::decode(&bytes, expected, lifecycle, book, native_nonces)
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
