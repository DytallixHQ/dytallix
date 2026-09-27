//! Local validator lifecycle qualification. Principal release remains disabled.
//! The caller verifies account signatures, funds additions and commits this state atomically.
use super::reward_runtime::{RewardState, ValidatorStatus};
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use bincode::Options;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const STATE_KEY: &str = "lifecycle:v1:state";
pub const PROFILE: &str = "cometbft-lifecycle-local-qualification";
pub const MAX_TOTAL_POWER: u128 = 1_152_921_504_606_846_975;
const MAX_ITEMS: usize = 10_000;
/// Retained validator-history entries: one per activation inside the evidence
/// horizon. Entries are small (a marker or a power change), and the state's
/// byte bound still applies. Scheduling beyond it is a paid capacity failure.
pub const MAX_HISTORY_CHANGES: usize = 100_000;
const MAX_BYTES: usize = 16 * 1024 * 1024;
type Positions = BTreeMap<String, BTreeMap<String, u128>>;

#[serde_with::serde_as]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleConfig {
    pub version: u32,
    pub profile: String,
    pub chain_id: String,
    pub approved_operators: BTreeMap<String, String>,
    #[serde_as(as = "serde_with::DisplayFromStr")]
    pub min_self_bond: u128,
    pub max_active: usize,
    pub evidence_max_age_blocks: u64,
    pub evidence_max_age_seconds: u64,
    pub processing_margin_blocks: u64,
    pub processing_margin_seconds: u64,
}
impl LifecycleConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            cfg!(feature = "pqc-fips204"),
            "Lifecycle requires FIPS 204 verification"
        );
        ensure!(
            self.version == 1 && self.profile == PROFILE,
            "Unsupported lifecycle profile"
        );
        valid_id(&self.chain_id)?;
        ensure!(
            (1..=64).contains(&self.max_active),
            "Lifecycle active-set bound invalid"
        );
        ensure!(
            !self.approved_operators.is_empty() && self.approved_operators.len() <= 64,
            "Operator registry bound invalid"
        );
        ensure!(
            self.min_self_bond > 0 && self.min_self_bond <= MAX_TOTAL_POWER,
            "Invalid minimum self-bond"
        );
        ensure!(
            self.evidence_max_age_blocks > 0
                && self.evidence_max_age_seconds > 0
                && self.processing_margin_blocks > 0
                && self.processing_margin_seconds > 0,
            "Evidence limits and margins must be explicit and positive"
        );
        self.evidence_max_age_blocks
            .checked_add(self.processing_margin_blocks)
            .context("Evidence block bound overflow")?;
        self.evidence_max_age_seconds
            .checked_add(self.processing_margin_seconds)
            .context("Evidence time bound overflow")?;
        for (id, owner) in &self.approved_operators {
            valid_id(id)?;
            valid_id(owner)?;
        }
        ensure!(
            serde_json::to_vec(self)?.len() <= 65_536,
            "Lifecycle configuration exceeds bound"
        );
        Ok(())
    }
    /// The part of the configuration an unbond's maturity depends on. These
    /// limits are not governed, so an unbond's copy keeps matching after a
    /// governed change to validator limits or operators (T6).
    pub fn same_evidence_limits(&self, other: &Self) -> bool {
        self.version == other.version
            && self.profile == other.profile
            && self.chain_id == other.chain_id
            && self.evidence_max_age_blocks == other.evidence_max_age_blocks
            && self.evidence_max_age_seconds == other.evidence_max_age_seconds
            && self.processing_margin_blocks == other.processing_margin_blocks
            && self.processing_margin_seconds == other.processing_margin_seconds
    }
    /// True when no evidence or withdrawal can still need a record that
    /// ended at (`end_height`, `end_time`): both evidence limits plus the
    /// processing margins have passed at the parent, as for unbond maturity
    /// (state model step 4, P01 27 September 2026).
    pub fn past_horizon(
        &self,
        end_height: u64,
        end_time: u64,
        parent_height: u64,
        parent_time: u64,
    ) -> bool {
        let blocks = end_height
            .saturating_add(self.evidence_max_age_blocks)
            .saturating_add(self.processing_margin_blocks);
        let seconds = end_time
            .saturating_add(self.evidence_max_age_seconds)
            .saturating_add(self.processing_margin_seconds);
        parent_height > blocks && parent_time > seconds
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidatorIdentity {
    pub owner: String,
    pub pubkey_base64: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidatorView {
    pub validators: BTreeMap<String, ValidatorIdentity>,
    pub positions: Positions,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidatorUpdate {
    pub pubkey_type: String,
    pub pubkey_base64: String,
    pub power: i64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingBond {
    pub owner: String,
    pub validator: String,
    pub amount: u128,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnbondEntry {
    pub id: String,
    pub owner: String,
    pub validator: String,
    pub amount: u128,
    pub request_height: u64,
    pub effective_height: u64,
    pub last_exposure_height: Option<u64>,
    pub last_exposure_time_seconds: Option<u64>,
    pub evidence_config: LifecycleConfig,
}
impl UnbondEntry {
    /// This is an age predicate only. It does not authorize principal release.
    pub fn maturity_satisfied(
        &self,
        config: &LifecycleConfig,
        height: u64,
        time_seconds: u64,
        evidence_and_penalties_complete: bool,
    ) -> Result<bool> {
        config.validate()?;
        ensure!(
            self.evidence_config.same_evidence_limits(config),
            "Unbond evidence configuration changed"
        );
        let (Some(last_height), Some(last_time)) =
            (self.last_exposure_height, self.last_exposure_time_seconds)
        else {
            return Ok(false);
        };
        ensure!(
            self.amount > 0
                && self.effective_height > self.request_height
                && last_height.checked_add(1) == Some(self.effective_height),
            "Invalid unbond exposure metadata"
        );
        let height_limit = last_height
            .checked_add(config.evidence_max_age_blocks)
            .and_then(|v| v.checked_add(config.processing_margin_blocks))
            .context("Unbond block maturity overflow")?;
        let time_limit = last_time
            .checked_add(config.evidence_max_age_seconds)
            .and_then(|v| v.checked_add(config.processing_margin_seconds))
            .context("Unbond time maturity overflow")?;
        Ok(height > height_limit && time_seconds > time_limit && evidence_and_penalties_complete)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduledChange {
    pub request_height: u64,
    pub view: ValidatorView,
    pub additions: Vec<PendingBond>,
    pub removals: Vec<UnbondEntry>,
}
/// A past validator set: identities and voting power. Owner exposure is kept
/// by penalty tranches, not here (state model step 4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoricalSet {
    pub validators: BTreeMap<String, ValidatorIdentity>,
    pub powers: BTreeMap<String, u128>,
}
/// The change from one historical set to the next, at its activation height.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetChange {
    /// Parent time of the activation block: the last time the previous set
    /// was in effect.
    pub parent_time_seconds: u64,
    /// Validators added or given a new identity (Some) or removed (None).
    pub identities: BTreeMap<String, Option<ValidatorIdentity>>,
    /// New power of each remaining validator whose power changed.
    pub powers: BTreeMap<String, u128>,
}
/// Validator sets from `base_height` on: the set then, and each later change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidatorHistory {
    pub base_height: u64,
    pub base: HistoricalSet,
    pub changes: BTreeMap<u64, SetChange>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleState {
    pub config: LifecycleConfig,
    pub effective: ValidatorView,
    pub schedules: BTreeMap<u64, ScheduledChange>,
    pub unbonding: BTreeMap<String, UnbondEntry>,
    pub history: ValidatorHistory,
    /// Validator updates of the last two heights; older ones are verified
    /// from committed blocks.
    pub update_history: BTreeMap<u64, Vec<ValidatorUpdate>>,
    pub last_height: u64,
    pub max_positions: usize,
    pub next_unbond_id: u64,
    pub reserved_owners: BTreeSet<String>,
    /// Every consensus address any validator has used. A key is never
    /// registered again, even after its history is pruned.
    pub used_addresses: BTreeSet<String>,
}
#[derive(Clone, Debug)]
pub enum Operation {
    Bond {
        validator: String,
        amount: u128,
    },
    Unbond {
        validator: String,
        amount: u128,
    },
    Register {
        validator: String,
        pubkey_base64: String,
        proof_base64: String,
        expires_at_height: u64,
        amount: u128,
    },
    Rotate {
        validator: String,
        pubkey_base64: String,
        proof_base64: String,
        expires_at_height: u64,
    },
    Exit {
        validator: String,
    },
}
fn valid_id(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 256
            && !value.chars().any(|c| c.is_whitespace() || c.is_control()),
        "Invalid lifecycle identifier"
    );
    Ok(())
}
fn checked_sum(values: impl IntoIterator<Item = u128>) -> Result<u128> {
    values.into_iter().try_fold(0u128, |a, b| {
        a.checked_add(b).context("Lifecycle principal overflow")
    })
}
pub fn consensus_address(pubkey_base64: &str) -> Result<String> {
    let bytes = B64
        .decode(pubkey_base64)
        .context("Invalid consensus key base64")?;
    ensure!(
        bytes.len() == 1952 && B64.encode(&bytes) == pubkey_base64,
        "Invalid ML-DSA-65 consensus key"
    );
    Ok(hex::encode(&Sha256::digest(bytes)[..20]))
}
pub fn proof_sign_bytes(
    chain: &str,
    operation: &str,
    validator: &str,
    owner: &str,
    key: &str,
    nonce: u64,
    expiry: u64,
    amount: u128,
) -> Result<Vec<u8>> {
    valid_id(chain)?;
    valid_id(validator)?;
    valid_id(owner)?;
    consensus_address(key)?;
    ensure!(
        operation == "register" || (operation == "rotate" && amount == 0),
        "Invalid key-proof operation"
    );
    let mut bytes = b"dytallix-validator-key-proof-v1\0".to_vec();
    bytes.extend(serde_json::to_vec(&(
        chain,
        operation,
        validator,
        owner,
        key,
        nonce,
        expiry,
        amount.to_string(),
    ))?);
    Ok(bytes)
}
fn verify_proof(
    config: &LifecycleConfig,
    height: u64,
    operation: &str,
    id: &str,
    owner: &str,
    key: &str,
    nonce: u64,
    expiry: u64,
    amount: u128,
    proof: &str,
) -> Result<()> {
    ensure!(
        cfg!(feature = "pqc-fips204"),
        "Lifecycle requires FIPS 204 verification"
    );
    ensure!(height <= expiry, "Consensus key proof expired");
    let bytes = proof_sign_bytes(
        &config.chain_id,
        operation,
        id,
        owner,
        key,
        nonce,
        expiry,
        amount,
    )?;
    let signature = B64.decode(proof).context("Invalid key proof base64")?;
    ensure!(
        signature.len() == 3309 && B64.encode(&signature) == proof,
        "Invalid ML-DSA-65 proof encoding"
    );
    dytallix_runtime_crypto::verify(
        &B64.decode(key)?,
        &bytes,
        &signature,
        dytallix_runtime_crypto::PQCAlgorithm::MlDsa65,
    )
    .context("Consensus key possession proof failed")
}
/// Authenticated validator-role proof. Private fields prevent bypassing verification.
#[derive(Clone, Debug)]
pub(crate) struct VerifiedKeyProof {
    signing_bytes: Vec<u8>,
    proof: String,
    height: u64,
}
impl VerifiedKeyProof {
    pub(crate) fn verify(
        config: &LifecycleConfig,
        height: u64,
        operation: &str,
        id: &str,
        owner: &str,
        key: &str,
        nonce: u64,
        expiry: u64,
        amount: u128,
        proof: &str,
    ) -> Result<Self> {
        verify_proof(
            config, height, operation, id, owner, key, nonce, expiry, amount, proof,
        )?;
        Ok(Self {
            signing_bytes: proof_sign_bytes(
                &config.chain_id,
                operation,
                id,
                owner,
                key,
                nonce,
                expiry,
                amount,
            )?,
            proof: proof.into(),
            height,
        })
    }
    fn matches(
        &self,
        config: &LifecycleConfig,
        height: u64,
        operation: &str,
        id: &str,
        owner: &str,
        key: &str,
        nonce: u64,
        expiry: u64,
        amount: u128,
        proof: &str,
    ) -> Result<()> {
        ensure!(
            self.height == height
                && self.proof == proof
                && self.signing_bytes
                    == proof_sign_bytes(
                        &config.chain_id,
                        operation,
                        id,
                        owner,
                        key,
                        nonce,
                        expiry,
                        amount
                    )?,
            "Verified validator proof binding differs"
        );
        Ok(())
    }
}
/// A capacity predicate on a valid proposed transition. Initial state validation
/// remains the caller's responsibility before classifying this error as paid.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub(crate) struct TransitionCapacity(pub &'static str);

fn set_updates<'a>(
    validators: impl Iterator<Item = (&'a String, &'a ValidatorIdentity)>,
    power: impl Fn(&str) -> Result<u128>,
) -> Result<Vec<ValidatorUpdate>> {
    let mut updates = Vec::new();
    for (id, identity) in validators {
        let power = power(id)?;
        ensure!(
            power > 0 && power <= MAX_TOTAL_POWER,
            "Invalid validator power"
        );
        updates.push((
            consensus_address(&identity.pubkey_base64)?,
            ValidatorUpdate {
                pubkey_type: "ml_dsa_65".into(),
                pubkey_base64: identity.pubkey_base64.clone(),
                power: i64::try_from(power)?,
            },
        ));
    }
    updates.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(updates.into_iter().map(|(_, u)| u).collect())
}
impl HistoricalSet {
    pub fn of(view: &ValidatorView) -> Result<Self> {
        let mut powers = BTreeMap::new();
        for id in view.validators.keys() {
            powers.insert(
                id.clone(),
                checked_sum(view.positions.values().filter_map(|p| p.get(id)).copied())?,
            );
        }
        Ok(Self {
            validators: view.validators.clone(),
            powers,
        })
    }
    fn validate(&self, config: &LifecycleConfig) -> Result<()> {
        ensure!(
            !self.validators.is_empty()
                && self.validators.len() <= config.max_active
                && self.powers.keys().eq(self.validators.keys()),
            "Invalid historical validator set"
        );
        let mut addresses = BTreeSet::new();
        for (id, identity) in &self.validators {
            ensure!(
                config.approved_operators.get(id) == Some(&identity.owner),
                "Validator operator is not approved"
            );
            ensure!(
                addresses.insert(consensus_address(&identity.pubkey_base64)?),
                "Duplicate consensus address"
            );
        }
        ensure!(
            self.powers.values().all(|p| *p > 0)
                && checked_sum(self.powers.values().copied())? <= MAX_TOTAL_POWER,
            "Invalid historical validator power"
        );
        Ok(())
    }
    pub fn validator_set(&self) -> Result<Vec<ValidatorUpdate>> {
        set_updates(self.validators.iter(), |id| {
            self.powers
                .get(id)
                .copied()
                .context("Historical validator has no power")
        })
    }
    fn change_to(&self, next: &Self, parent_time_seconds: u64) -> SetChange {
        let mut identities = BTreeMap::new();
        for (id, identity) in &next.validators {
            if self.validators.get(id) != Some(identity) {
                identities.insert(id.clone(), Some(identity.clone()));
            }
        }
        for id in self.validators.keys() {
            if !next.validators.contains_key(id) {
                identities.insert(id.clone(), None);
            }
        }
        let powers = next
            .powers
            .iter()
            .filter(|(id, power)| self.powers.get(*id) != Some(power))
            .map(|(id, power)| (id.clone(), *power))
            .collect();
        SetChange {
            parent_time_seconds,
            identities,
            powers,
        }
    }
    fn apply(&mut self, change: &SetChange) {
        for (id, identity) in &change.identities {
            match identity {
                Some(identity) => {
                    self.validators.insert(id.clone(), identity.clone());
                }
                None => {
                    self.validators.remove(id);
                    self.powers.remove(id);
                }
            }
        }
        for (id, power) in &change.powers {
            self.powers.insert(id.clone(), *power);
        }
    }
}
impl ValidatorHistory {
    pub(crate) fn new(base: HistoricalSet) -> Self {
        Self {
            base_height: 1,
            base,
            changes: BTreeMap::new(),
        }
    }
    /// The set in effect at `height`.
    pub fn at(&self, height: u64) -> Result<HistoricalSet> {
        ensure!(
            height >= self.base_height,
            "Validator history before the retention horizon"
        );
        let mut set = self.base.clone();
        for change in self.changes.range(..=height).map(|(_, c)| c) {
            set.apply(change);
        }
        Ok(set)
    }
    fn latest(&self) -> HistoricalSet {
        let mut set = self.base.clone();
        for change in self.changes.values() {
            set.apply(change);
        }
        set
    }
    /// Record the set that takes effect at `height`, if it differs.
    fn record(&mut self, height: u64, parent_time_seconds: u64, next: &HistoricalSet) -> Result<()> {
        ensure!(
            height
                > self
                    .changes
                    .keys()
                    .next_back()
                    .copied()
                    .unwrap_or(self.base_height),
            "Validator history height regressed"
        );
        // Every activation is recorded, even one that changes no power: its
        // time lets the evidence horizon advance (state model step 4).
        let change = self.latest().change_to(next, parent_time_seconds);
        self.changes.insert(height, change);
        Ok(())
    }
    fn validate(&self, config: &LifecycleConfig, last_height: u64) -> Result<()> {
        ensure!(
            self.base_height > 0
                && self.base_height <= last_height.max(1)
                && self.changes.len() <= MAX_HISTORY_CHANGES,
            "Lifecycle history bound invalid"
        );
        let mut set = self.base.clone();
        set.validate(config)?;
        let mut previous = (self.base_height, 0u64);
        for (height, change) in &self.changes {
            ensure!(
                *height > previous.0
                    && *height <= last_height.max(1)
                    && change.parent_time_seconds >= previous.1,
                "Invalid validator history change"
            );
            set.apply(change);
            set.validate(config)?;
            previous = (*height, change.parent_time_seconds);
        }
        Ok(())
    }
    /// Parent time of the latest retained change, or zero.
    fn last_change_time(&self) -> u64 {
        self.changes
            .values()
            .next_back()
            .map_or(0, |c| c.parent_time_seconds)
    }
    /// Every identity the retained history holds.
    pub fn identities(&self) -> impl Iterator<Item = &ValidatorIdentity> {
        self.base.validators.values().chain(
            self.changes
                .values()
                .flat_map(|c| c.identities.values().flatten()),
        )
    }
}
impl ValidatorView {
    fn validate(&self, config: &LifecycleConfig, max_positions: usize) -> Result<()> {
        ensure!(
            !self.validators.is_empty() && self.validators.len() <= config.max_active,
            "Empty or excessive active validator set"
        );
        let mut addresses = BTreeSet::new();
        for (id, identity) in &self.validators {
            ensure!(
                config.approved_operators.get(id) == Some(&identity.owner),
                "Validator operator is not approved"
            );
            ensure!(
                addresses.insert(consensus_address(&identity.pubkey_base64)?),
                "Duplicate consensus address"
            );
            ensure!(
                self.positions
                    .get(&identity.owner)
                    .and_then(|p| p.get(id))
                    .copied()
                    .unwrap_or(0)
                    >= config.min_self_bond,
                "Validator self-bond below minimum"
            );
        }
        let mut count = 0usize;
        for (owner, positions) in &self.positions {
            valid_id(owner)?;
            ensure!(!positions.is_empty(), "Empty lifecycle owner positions");
            for (id, amount) in positions {
                ensure!(
                    *amount > 0 && self.validators.contains_key(id),
                    "Invalid lifecycle position"
                );
                count = count.checked_add(1).context("Position count overflow")?;
            }
        }
        ensure!(
            count <= max_positions,
            "Lifecycle position capacity exceeded"
        );
        let power = checked_sum(self.positions.values().flat_map(|v| v.values()).copied())?;
        ensure!(power <= MAX_TOTAL_POWER, "Consensus total power exceeded");
        Ok(())
    }
    pub fn validator_set(&self) -> Result<Vec<ValidatorUpdate>> {
        set_updates(self.validators.iter(), |id| {
            checked_sum(self.positions.values().filter_map(|p| p.get(id)).copied())
        })
    }
}
fn difference(before: &ValidatorView, after: &ValidatorView) -> Result<Vec<ValidatorUpdate>> {
    difference_of(before.validator_set()?, after.validator_set()?)
}
fn difference_of(
    before: Vec<ValidatorUpdate>,
    after: Vec<ValidatorUpdate>,
) -> Result<Vec<ValidatorUpdate>> {
    let mut keys = BTreeMap::<String, (String, i64, i64)>::new();
    for u in before {
        keys.insert(
            consensus_address(&u.pubkey_base64)?,
            (u.pubkey_base64, u.power, 0),
        );
    }
    for u in after {
        let entry =
            keys.entry(consensus_address(&u.pubkey_base64)?)
                .or_insert((u.pubkey_base64, 0, 0));
        entry.2 = u.power;
    }
    Ok(keys
        .into_values()
        .filter(|(_, a, b)| a != b)
        .map(|(key, _, power)| ValidatorUpdate {
            pubkey_type: "ml_dsa_65".into(),
            pubkey_base64: key,
            power,
        })
        .collect())
}
impl LifecycleState {
    pub fn new(
        config: LifecycleConfig,
        identities: BTreeMap<String, ValidatorIdentity>,
        rewards: &RewardState,
    ) -> Result<Self> {
        config.validate()?;
        rewards.validate_internal()?;
        ensure!(
            rewards.last_height == 0 && rewards.unbonding.is_empty(),
            "Lifecycle requires fresh reward genesis"
        );
        ensure!(
            config.chain_id == rewards.config.chain_id
                && config.max_active <= rewards.config.max_validators,
            "Lifecycle and reward configuration differ"
        );
        let effective = ValidatorView {
            validators: identities,
            positions: rewards.positions.clone(),
        };
        let used_addresses = effective
            .validators
            .values()
            .map(|identity| consensus_address(&identity.pubkey_base64))
            .collect::<Result<_>>()?;
        let state = Self {
            config,
            history: ValidatorHistory::new(HistoricalSet::of(&effective)?),
            effective,
            schedules: BTreeMap::new(),
            unbonding: BTreeMap::new(),
            used_addresses,
            update_history: BTreeMap::new(),
            last_height: 0,
            max_positions: rewards.config.max_positions,
            next_unbond_id: 0,
            reserved_owners: rewards
                .positions
                .keys()
                .chain(rewards.unpaid.keys())
                .chain(rewards.locks.keys())
                .cloned()
                .collect(),
        };
        state.validate()?;
        state.validate_rewards(rewards)?;
        Ok(state)
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let bytes = bincode::serialize(self)?;
        ensure!(bytes.len() <= MAX_BYTES, "Lifecycle state exceeds bound");
        Ok(bytes)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() <= MAX_BYTES, "Lifecycle state exceeds bound");
        let state: Self = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_limit(MAX_BYTES as u64)
            .reject_trailing_bytes()
            .deserialize(bytes)?;
        state.validate()?;
        ensure!(
            bincode::serialize(&state)? == bytes,
            "Noncanonical lifecycle encoding"
        );
        Ok(state)
    }
    /// The full view at the current height or a scheduled one.
    fn view(&self, height: u64) -> Result<&ValidatorView> {
        ensure!(
            height >= self.last_height.max(1)
                && height
                    <= self
                        .last_height
                        .checked_add(2)
                        .context("Lifecycle height overflow")?
                        .max(1),
            "Lifecycle set height out of range"
        );
        Ok(self
            .schedules
            .range(..=height)
            .next_back()
            .map(|(_, s)| &s.view)
            .unwrap_or(&self.effective))
    }
    /// The set at any height from the retained history to two blocks ahead.
    pub fn historical_set(&self, height: u64) -> Result<HistoricalSet> {
        ensure!(height > 0, "Lifecycle set height out of range");
        if height >= self.last_height.max(1) {
            return HistoricalSet::of(self.view(height)?);
        }
        self.history.at(height)
    }
    pub fn validator_set(&self, height: u64) -> Result<Vec<ValidatorUpdate>> {
        self.historical_set(height)?.validator_set()
    }
    pub fn historical_validator_set(&self, height: u64) -> Result<Vec<ValidatorUpdate>> {
        self.validator_set(height)
    }
    pub fn validator_updates(&self, height: u64) -> Result<Vec<ValidatorUpdate>> {
        ensure!(
            height > 0 && height <= self.last_height,
            "Validator update height out of range"
        );
        if height < self.last_height {
            return self.historical_validator_updates(height);
        }
        difference(
            self.view(height.checked_add(1).context("Update height overflow")?)?,
            self.view(height.checked_add(2).context("Update height overflow")?)?,
        )
    }
    /// Updates are retained for the last two heights; older blocks' updates
    /// are verified from committed block records.
    pub fn historical_validator_updates(&self, height: u64) -> Result<Vec<ValidatorUpdate>> {
        ensure!(
            height > 0
                && height <= self.last_height
                && height.saturating_add(1) >= self.last_height,
            "Historical update height out of range"
        );
        Ok(self
            .update_history
            .get(&height)
            .cloned()
            .unwrap_or_default())
    }
    pub fn pending_bonds_by_owner(&self) -> Result<BTreeMap<String, u128>> {
        let mut result = BTreeMap::new();
        for entry in self.schedules.values().flat_map(|s| &s.additions) {
            let amount = result.entry(entry.owner.clone()).or_insert(0u128);
            *amount = amount
                .checked_add(entry.amount)
                .context("Pending bond sum overflow")?;
        }
        Ok(result)
    }
    pub fn pending_bond_by_owner(&self, owner: &str) -> Result<u128> {
        Ok(self
            .pending_bonds_by_owner()?
            .get(owner)
            .copied()
            .unwrap_or(0))
    }
    pub fn pending_bond_total(&self) -> Result<u128> {
        checked_sum(self.pending_bonds_by_owner()?.into_values())
    }
    pub fn unbonding_by_owner(&self) -> Result<BTreeMap<String, u128>> {
        let mut result = BTreeMap::new();
        for entry in self.unbonding.values() {
            let amount = result.entry(entry.owner.clone()).or_insert(0u128);
            *amount = amount
                .checked_add(entry.amount)
                .context("Unbond sum overflow")?;
        }
        Ok(result)
    }
    pub fn unbonding_total(&self) -> Result<u128> {
        checked_sum(self.unbonding_by_owner()?.into_values())
    }
    pub fn validate_rewards(&self, rewards: &RewardState) -> Result<()> {
        rewards.validate_internal()?;
        for owner in rewards
            .positions
            .keys()
            .chain(rewards.unbonding.keys())
            .chain(rewards.unpaid.keys())
            .chain(rewards.locks.keys())
        {
            ensure!(
                self.reserved_owners.contains(owner),
                "Reward owner slot was not reserved"
            );
        }
        ensure!(
            rewards.config.chain_id == self.config.chain_id
                && rewards.config.max_positions == self.max_positions,
            "Lifecycle reward config mismatch"
        );
        ensure!(
            rewards.positions == self.effective.positions,
            "Lifecycle reward principal mismatch"
        );
        let statuses: BTreeMap<_, _> = self
            .effective
            .validators
            .keys()
            .map(|id| {
                (
                    id.clone(),
                    ValidatorStatus {
                        active: true,
                        jailed: false,
                    },
                )
            })
            .collect();
        ensure!(
            rewards.validators == statuses,
            "Lifecycle reward validator mismatch"
        );
        ensure!(
            rewards.unbonding == self.unbonding_by_owner()?,
            "Lifecycle unbond custody mismatch"
        );
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        self.config.validate()?;
        ensure!(
            (1..=MAX_ITEMS).contains(&self.max_positions),
            "Lifecycle position bound invalid"
        );
        self.effective.validate(&self.config, self.max_positions)?;
        ensure!(
            self.reserved_owners.len() <= self.max_positions,
            "Lifecycle owner capacity exceeded"
        );
        for owner in &self.reserved_owners {
            valid_id(owner)?;
        }
        for owner in self
            .effective
            .positions
            .keys()
            .chain(self.unbonding.values().map(|e| &e.owner))
            .chain(
                self.schedules
                    .values()
                    .flat_map(|s| s.view.positions.keys()),
            )
        {
            ensure!(
                self.reserved_owners.contains(owner),
                "Lifecycle owner slot was not reserved"
            );
        }
        ensure!(
            self.update_history.len() <= 2
                && self.schedules.len() <= 2
                && self.unbonding.len() <= MAX_ITEMS,
            "Lifecycle history bound invalid"
        );
        self.history.validate(&self.config, self.last_height)?;
        let mut previous = &self.effective;
        let mut ids = BTreeSet::new();
        for (height, schedule) in &self.schedules {
            ensure!(
                *height > self.last_height
                    && *height
                        <= self
                            .last_height
                            .checked_add(2)
                            .context("Lifecycle height overflow")?
                    && schedule.request_height.checked_add(2) == Some(*height),
                "Invalid activation schedule"
            );
            schedule.view.validate(&self.config, self.max_positions)?;
            ensure!(
                schedule.additions.len() <= MAX_ITEMS && schedule.removals.len() <= MAX_ITEMS,
                "Schedule entry bound exceeded"
            );
            let mut derived = previous.positions.clone();
            for add in &schedule.additions {
                ensure!(add.amount > 0, "Zero pending bond");
                add_position(&mut derived, &add.owner, &add.validator, add.amount)?;
            }
            for remove in &schedule.removals {
                ensure!(
                    remove.effective_height == *height
                        && remove.request_height == schedule.request_height
                        && remove.last_exposure_height.is_none()
                        && remove.last_exposure_time_seconds.is_none()
                        && remove.evidence_config.same_evidence_limits(&self.config)
                        && ids.insert(remove.id.clone()),
                    "Invalid pending unbond"
                );
                subtract_position(
                    &mut derived,
                    &remove.owner,
                    &remove.validator,
                    remove.amount,
                )?;
            }
            ensure!(
                derived == schedule.view.positions,
                "Scheduled principal conservation failed"
            );
            previous = &schedule.view;
        }
        ensure!(
            self.history.latest() == HistoricalSet::of(&self.effective)?,
            "Effective validator history mismatch"
        );
        for identity in self
            .history
            .identities()
            .chain(self.effective.validators.values())
            .chain(
                self.schedules
                    .values()
                    .flat_map(|s| s.view.validators.values()),
            )
        {
            ensure!(
                self.used_addresses
                    .contains(&consensus_address(&identity.pubkey_base64)?),
                "Validator key missing from used addresses"
            );
        }
        for (id, entry) in &self.unbonding {
            ensure!(
                *id == entry.id
                    && ids.insert(id.clone())
                    && entry.amount > 0
                    && entry.request_height.checked_add(2) == Some(entry.effective_height)
                    && entry.effective_height <= self.last_height
                    && entry.last_exposure_height == entry.effective_height.checked_sub(1)
                    && entry.last_exposure_time_seconds.is_some()
                    && entry.evidence_config.same_evidence_limits(&self.config),
                "Invalid finalized unbond history"
            );
            // Exposure at the last height is checked against penalty tranches
            // (`PenaltyState::validate`); history keeps no owner positions.
        }
        // Released entries are removed (state model step 4); each retained ID
        // is unique and below the sequence counter.
        ensure!(
            ids.len() <= MAX_ITEMS && u64::try_from(ids.len())? <= self.next_unbond_id,
            "Unbond sequence or record count mismatch"
        );
        let mut sequence_numbers = BTreeSet::new();
        for id in &ids {
            let (request, sequence) = id.split_once('-').context("Invalid unbond ID")?;
            let request: u64 = request.parse().context("Invalid unbond request ID")?;
            let sequence: u64 = sequence.parse().context("Invalid unbond sequence ID")?;
            ensure!(
                format!("{request:020}-{sequence:020}") == *id
                    && request > 0
                    && request <= self.last_height
                    && sequence < self.next_unbond_id
                    && sequence_numbers.insert(sequence),
                "Invalid unbond sequence"
            );
        }
        for (height, updates) in &self.update_history {
            ensure!(
                *height > 0
                    && *height <= self.last_height
                    && height.saturating_add(1) >= self.last_height,
                "Future or unretained validator update history"
            );
            let expected = difference_of(
                self.validator_set(height.checked_add(1).context("Historical height overflow")?)?,
                self.validator_set(height.checked_add(2).context("Historical height overflow")?)?,
            )?;
            ensure!(
                *updates == expected && !updates.is_empty(),
                "Validator update history mismatch"
            );
        }
        // Every scheduled set must have the exact update returned two blocks
        // before activation. Older activations were checked when scheduled.
        for height in self.schedules.keys().copied() {
            let request = height
                .checked_sub(2)
                .context("Invalid history activation height")?;
            ensure!(
                request > 0 && request <= self.last_height,
                "Invalid historical activation request"
            );
            let updates = difference_of(
                self.validator_set(height - 1)?,
                self.validator_set(height)?,
            )?;
            ensure!(
                self.update_history
                    .get(&request)
                    .cloned()
                    .unwrap_or_default()
                    == updates,
                "Missing validator update history"
            );
        }
        self.pending_bond_total()?;
        self.unbonding_total()?;
        Ok(())
    }
    pub fn advance(
        &mut self,
        height: u64,
        parent_time_seconds: u64,
        rewards: &mut RewardState,
    ) -> Result<()> {
        self.validate()?;
        self.validate_rewards(rewards)?;
        ensure!(
            self.last_height.checked_add(1) == Some(height),
            "Lifecycle height is not consecutive"
        );
        let mut next = self.clone();
        let mut next_rewards = rewards.clone();
        if next.apply_activation(height, parent_time_seconds)? {
            next_rewards.positions = next.effective.positions.clone();
            next_rewards.validators = next
                .effective
                .validators
                .keys()
                .map(|id| {
                    (
                        id.clone(),
                        ValidatorStatus {
                            active: true,
                            jailed: false,
                        },
                    )
                })
                .collect();
            next_rewards.unbonding = next.unbonding_by_owner()?;
        }
        next.last_height = height;
        next.retain_recent_updates();
        next.validate()?;
        next.validate_rewards(&next_rewards)?;
        *self = next;
        *rewards = next_rewards;
        Ok(())
    }
    /// Keep the validator updates of the last two heights; older blocks'
    /// updates are verified from committed block records.
    fn retain_recent_updates(&mut self) {
        let last = self.last_height;
        self.update_history
            .retain(|h, _| h.saturating_add(1) >= last);
    }
    fn apply_activation(&mut self, height: u64, parent_time_seconds: u64) -> Result<bool> {
        let Some(schedule) = self.schedules.remove(&height) else {
            return Ok(false);
        };
        for mut entry in schedule.removals {
            entry.last_exposure_height = height.checked_sub(1);
            entry.last_exposure_time_seconds = Some(parent_time_seconds);
            ensure!(
                self.unbonding.insert(entry.id.clone(), entry).is_none(),
                "Duplicate unbond entry"
            );
        }
        self.effective = schedule.view;
        let set = HistoricalSet::of(&self.effective)?;
        self.history.record(height, parent_time_seconds, &set)?;
        Ok(true)
    }
    /// Check both queued activations before accepting their principal changes.
    /// Projection uses a fixed-width timestamp only to measure discarded state.
    fn validate_activation_capacity(&self, max_state_bytes: u64) -> Result<()> {
        ensure!(
            bincode::serialized_size(self)? <= max_state_bytes,
            TransitionCapacity("Lifecycle state exceeds bound")
        );
        let mut projected = self.clone();
        let heights: Vec<_> = projected.schedules.keys().copied().collect();
        for height in heights {
            // Projection only measures size; any nondecreasing time will do.
            let time = projected.history.last_change_time();
            projected.apply_activation(height, time)?;
            projected.last_height = height;
            projected.retain_recent_updates();
            projected.validate()?;
            ensure!(
                bincode::serialized_size(&projected)? <= max_state_bytes,
                TransitionCapacity("Scheduled activation exceeds lifecycle state bound")
            );
        }
        Ok(())
    }
    /// The paid adapter must reject an invalid current or queued baseline before
    /// it classifies capacity errors from a new proposed action.
    pub(crate) fn validate_scheduled_capacity(&self) -> Result<()> {
        self.validate()?;
        self.validate_activation_capacity(MAX_BYTES as u64)
    }
    pub fn schedule(
        &mut self,
        height: u64,
        actor: &str,
        nonce: u64,
        operation: Operation,
    ) -> Result<()> {
        self.schedule_with_capacity(height, actor, nonce, operation, MAX_BYTES as u64)
    }
    fn schedule_with_capacity(
        &mut self,
        height: u64,
        actor: &str,
        nonce: u64,
        operation: Operation,
        max_state_bytes: u64,
    ) -> Result<()> {
        self.schedule_impl(height, actor, nonce, operation, max_state_bytes, None)
    }
    pub(crate) fn schedule_verified(
        &mut self,
        height: u64,
        actor: &str,
        nonce: u64,
        operation: Operation,
        proof: Option<&VerifiedKeyProof>,
    ) -> Result<()> {
        // A register/rotate request must carry the private verification token.
        ensure!(
            !matches!(
                &operation,
                Operation::Register { .. } | Operation::Rotate { .. }
            ) || proof.is_some(),
            "Missing verified validator proof"
        );
        self.schedule_impl(height, actor, nonce, operation, MAX_BYTES as u64, proof)
    }
    fn schedule_impl(
        &mut self,
        height: u64,
        actor: &str,
        nonce: u64,
        operation: Operation,
        max_state_bytes: u64,
        verified: Option<&VerifiedKeyProof>,
    ) -> Result<()> {
        self.validate()?;
        valid_id(actor)?;
        ensure!(
            height > 0 && height == self.last_height,
            "Lifecycle request outside current block"
        );
        let effective_height = height
            .checked_add(2)
            .context("Activation height overflow")?;
        let mut next = self.clone();
        let mut schedule =
            next.schedules
                .get(&effective_height)
                .cloned()
                .unwrap_or(ScheduledChange {
                    request_height: height,
                    view: next.view(effective_height)?.clone(),
                    additions: Vec::new(),
                    removals: Vec::new(),
                });
        match operation {
            Operation::Bond { validator, amount } => {
                ensure!(
                    schedule.view.validators.contains_key(&validator),
                    "Bond validator is not active or scheduled"
                );
                add_position(&mut schedule.view.positions, actor, &validator, amount)?;
                schedule.additions.push(PendingBond {
                    owner: actor.into(),
                    validator,
                    amount,
                });
            }
            Operation::Unbond { validator, amount } => {
                let identity = schedule
                    .view
                    .validators
                    .get(&validator)
                    .context("Unknown unbond validator")?
                    .clone();
                let added = checked_sum(
                    schedule
                        .additions
                        .iter()
                        .filter(|a| a.owner == actor && a.validator == validator)
                        .map(|a| a.amount),
                )?;
                let position = schedule
                    .view
                    .positions
                    .get(actor)
                    .and_then(|p| p.get(&validator))
                    .copied()
                    .unwrap_or(0);
                ensure!(
                    amount > 0
                        && amount
                            <= position
                                .checked_sub(added)
                                .context("Invalid pending stake")?,
                    "Cannot unbond absent or same-block pending stake"
                );
                if actor == identity.owner && position - amount < next.config.min_self_bond {
                    next.remove_validator(&mut schedule, height, effective_height, &validator)?;
                } else {
                    subtract_position(&mut schedule.view.positions, actor, &validator, amount)?;
                    next.append_removal(
                        &mut schedule,
                        height,
                        effective_height,
                        actor,
                        &validator,
                        amount,
                    )?;
                }
            }
            Operation::Register {
                validator,
                pubkey_base64,
                proof_base64,
                expires_at_height,
                amount,
            } => {
                ensure!(
                    next.config
                        .approved_operators
                        .get(&validator)
                        .map(String::as_str)
                        == Some(actor),
                    "Registration requires approved operator"
                );
                ensure!(
                    !schedule.view.validators.contains_key(&validator),
                    "Validator already registered"
                );
                next.check_new_key(&pubkey_base64)?;
                if let Some(token) = verified {
                    token.matches(
                        &next.config,
                        height,
                        "register",
                        &validator,
                        actor,
                        &pubkey_base64,
                        nonce,
                        expires_at_height,
                        amount,
                        &proof_base64,
                    )?;
                } else {
                    verify_proof(
                        &next.config,
                        height,
                        "register",
                        &validator,
                        actor,
                        &pubkey_base64,
                        nonce,
                        expires_at_height,
                        amount,
                        &proof_base64,
                    )?;
                }
                schedule.view.validators.insert(
                    validator.clone(),
                    ValidatorIdentity {
                        owner: actor.into(),
                        pubkey_base64,
                    },
                );
                add_position(&mut schedule.view.positions, actor, &validator, amount)?;
                schedule.additions.push(PendingBond {
                    owner: actor.into(),
                    validator,
                    amount,
                });
            }
            Operation::Rotate {
                validator,
                pubkey_base64,
                proof_base64,
                expires_at_height,
            } => {
                ensure!(
                    schedule
                        .view
                        .validators
                        .get(&validator)
                        .map(|v| v.owner.as_str())
                        == Some(actor),
                    "Rotation requires validator operator"
                );
                next.check_new_key(&pubkey_base64)?;
                if let Some(token) = verified {
                    token.matches(
                        &next.config,
                        height,
                        "rotate",
                        &validator,
                        actor,
                        &pubkey_base64,
                        nonce,
                        expires_at_height,
                        0,
                        &proof_base64,
                    )?;
                } else {
                    verify_proof(
                        &next.config,
                        height,
                        "rotate",
                        &validator,
                        actor,
                        &pubkey_base64,
                        nonce,
                        expires_at_height,
                        0,
                        &proof_base64,
                    )?;
                }
                schedule
                    .view
                    .validators
                    .get_mut(&validator)
                    .context("Unknown rotation validator")?
                    .pubkey_base64 = pubkey_base64;
            }
            Operation::Exit { validator } => {
                ensure!(
                    schedule
                        .view
                        .validators
                        .get(&validator)
                        .map(|v| v.owner.as_str())
                        == Some(actor),
                    "Exit requires validator operator"
                );
                next.remove_validator(&mut schedule, height, effective_height, &validator)?;
            }
        }
        schedule.view.validate(&next.config, next.max_positions)?;
        next.reserved_owners
            .extend(schedule.view.positions.keys().cloned());
        next.schedules.insert(effective_height, schedule);
        let updates = next.validator_updates(height)?;
        if updates.is_empty() {
            next.update_history.remove(&height);
        } else {
            next.update_history.insert(height, updates);
        }
        next.validate()?;
        next.validate_activation_capacity(max_state_bytes)?;
        *self = next;
        Ok(())
    }
    /// A consensus key is never registered twice, even after its history is
    /// pruned (P01, 27 September 2026). Records the address as used.
    fn check_new_key(&mut self, key: &str) -> Result<()> {
        let address = consensus_address(key)?;
        ensure!(
            self.used_addresses.insert(address),
            "Consensus key or address already used"
        );
        Ok(())
    }
    fn append_removal(
        &mut self,
        schedule: &mut ScheduledChange,
        request: u64,
        effective: u64,
        owner: &str,
        validator: &str,
        amount: u128,
    ) -> Result<()> {
        ensure!(amount > 0, "Zero unbond principal");
        let id = format!("{request:020}-{:020}", self.next_unbond_id);
        self.next_unbond_id = self
            .next_unbond_id
            .checked_add(1)
            .context("Unbond ID overflow")?;
        schedule.removals.push(UnbondEntry {
            id,
            owner: owner.into(),
            validator: validator.into(),
            amount,
            request_height: request,
            effective_height: effective,
            last_exposure_height: None,
            last_exposure_time_seconds: None,
            evidence_config: self.config.clone(),
        });
        Ok(())
    }
    fn remove_validator(
        &mut self,
        schedule: &mut ScheduledChange,
        request: u64,
        effective: u64,
        validator: &str,
    ) -> Result<()> {
        ensure!(
            !schedule.additions.iter().any(|a| a.validator == validator),
            "Cannot exit with same-block pending additions"
        );
        ensure!(
            schedule.view.validators.remove(validator).is_some(),
            "Unknown exit validator"
        );
        let positions: Vec<_> = schedule
            .view
            .positions
            .iter()
            .filter_map(|(owner, p)| p.get(validator).map(|a| (owner.clone(), *a)))
            .collect();
        for (owner, amount) in positions {
            subtract_position(&mut schedule.view.positions, &owner, validator, amount)?;
            self.append_removal(schedule, request, effective, &owner, validator, amount)?;
        }
        Ok(())
    }
    /// Drop validator history that no evidence or withdrawal can still need
    /// (state model step 4): fold the base set forward while the set it holds
    /// ended past the horizon at this block's parent. The current set stays.
    pub fn prune_history(&mut self, parent_height: u64, parent_time: u64) -> Result<()> {
        let mut next = self.clone();
        while let Some((&start, change)) = next.history.changes.iter().next() {
            // The base set was last in effect at `start - 1`, at the change's
            // recorded parent time.
            if !next
                .config
                .past_horizon(start - 1, change.parent_time_seconds, parent_height, parent_time)
            {
                break;
            }
            let change = next
                .history
                .changes
                .remove(&start)
                .context("Validator history change vanished")?;
            next.history.base.apply(&change);
            next.history.base_height = start;
        }
        next.validate()?;
        *self = next;
        Ok(())
    }
    /// Remove released unbond entries (state model step 4). The reward state's
    /// gross unbonding drops by each entry's amount, as the penalty custody
    /// removes the entry's tranches in the same block.
    pub fn remove_released(&mut self, ids: &[String], rewards: &mut RewardState) -> Result<()> {
        let mut next = self.clone();
        let mut next_rewards = rewards.clone();
        for id in ids {
            let entry = next
                .unbonding
                .remove(id)
                .context("Released unbond entry missing")?;
            let gross = next_rewards
                .unbonding
                .get_mut(&entry.owner)
                .context("Released unbond owner has no gross custody")?;
            *gross = gross
                .checked_sub(entry.amount)
                .context("Released unbond exceeds gross custody")?;
            if *gross == 0 {
                next_rewards.unbonding.remove(&entry.owner);
            }
        }
        next.release_empty_owners(&next_rewards);
        next.validate()?;
        next.validate_rewards(&next_rewards)?;
        *self = next;
        *rewards = next_rewards;
        Ok(())
    }
    /// Free the slot of every owner holding nothing: no effective, scheduled
    /// or pending principal, no unbonding entry and no reward position,
    /// unbonding, unpaid amount or lock. `max_positions` then limits
    /// concurrent stakers, not lifetime stakers (state model step 4).
    fn release_empty_owners(&mut self, rewards: &RewardState) {
        let held: BTreeSet<&String> = self
            .effective
            .positions
            .keys()
            .chain(self.schedules.values().flat_map(|s| s.view.positions.keys()))
            .chain(
                self.schedules
                    .values()
                    .flat_map(|s| s.additions.iter().map(|a| &a.owner)),
            )
            .chain(
                self.schedules
                    .values()
                    .flat_map(|s| s.removals.iter().map(|r| &r.owner)),
            )
            .chain(self.unbonding.values().map(|e| &e.owner))
            .chain(rewards.positions.keys())
            .chain(rewards.unbonding.keys())
            .chain(rewards.unpaid.keys())
            .chain(rewards.locks.keys())
            .collect();
        let freed: Vec<String> = self
            .reserved_owners
            .iter()
            .filter(|owner| !held.contains(owner))
            .cloned()
            .collect();
        for owner in freed {
            self.reserved_owners.remove(&owner);
        }
    }
    /// Evidence processing and penalty settlement are not qualified in this profile.
    pub fn authorize_withdrawal(&self, _owner: &str, _unbond_id: &str) -> Result<()> {
        anyhow::bail!("Unbond withdrawal requires qualified evidence and penalty accounting")
    }
}
fn add_position(
    positions: &mut Positions,
    owner: &str,
    validator: &str,
    amount: u128,
) -> Result<()> {
    valid_id(owner)?;
    valid_id(validator)?;
    ensure!(amount > 0, "Zero bond principal");
    let entry = positions
        .entry(owner.into())
        .or_default()
        .entry(validator.into())
        .or_default();
    *entry = entry
        .checked_add(amount)
        .context("Bond principal overflow")?;
    Ok(())
}
fn subtract_position(
    positions: &mut Positions,
    owner: &str,
    validator: &str,
    amount: u128,
) -> Result<()> {
    ensure!(amount > 0, "Zero unbond principal");
    let map = positions
        .get_mut(owner)
        .context("No bonded principal for owner")?;
    let entry = map
        .get_mut(validator)
        .context("No bonded principal for validator")?;
    *entry = entry
        .checked_sub(amount)
        .context("Unbond exceeds principal")?;
    if *entry == 0 {
        map.remove(validator);
    }
    if map.is_empty() {
        positions.remove(owner);
    }
    Ok(())
}
#[cfg(test)]
#[path = "validator_lifecycle_tests.rs"]
mod tests;
