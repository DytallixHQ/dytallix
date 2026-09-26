//! Synthetic local evidence custody. Engine facts are not cryptographic proofs.
//! Gross lifecycle history stays immutable. This sidecar records net losses and releases.
use super::reward_runtime::RewardState;
use super::validator_lifecycle::{consensus_address, LifecycleState, ValidatorView};
use anyhow::{ensure, Context, Result};
use bincode::Options;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const STATE_KEY: &str = "penalty:v1:state";
pub const PROFILE: &str = "cometbft-penalty-local-qualification";
const MAX_ITEMS: usize = 10_000;
const MAX_BYTES: usize = 16 * 1024 * 1024;
type Positions = BTreeMap<String, BTreeMap<String, u128>>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PenaltyConfig {
    pub version: u32,
    pub profile: String,
    pub chain_id: String,
    pub penalty_numerator: u64,
    pub penalty_denominator: u64,
    pub production_activation: bool,
}
impl PenaltyConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1 && self.profile == PROFILE,
            "Unsupported penalty profile"
        );
        valid_id(&self.chain_id)?;
        ensure!(
            !self.production_activation,
            "Production penalties are not qualified"
        );
        ensure!(
            self.penalty_numerator > 0 && self.penalty_numerator <= self.penalty_denominator,
            "Penalty ratio must be explicit, positive and at most one"
        );
        Ok(())
    }
}

/// The caller obtains these facts only from the configured engine's committed evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceFact {
    pub kind: String,
    pub validator_address: String,
    pub height: u64,
    pub time_seconds: u64,
    pub time_nanos: i32,
    pub power: i64,
    pub total_power: i64,
}
impl EvidenceFact {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.kind == "duplicate_vote", "Unsupported evidence kind");
        ensure!(
            self.validator_address.len() == 40
                && self
                    .validator_address
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "Noncanonical evidence validator address"
        );
        valid_time((self.time_seconds, self.time_nanos))?;
        ensure!(
            self.height > 0 && self.power > 0 && self.total_power >= self.power,
            "Invalid evidence height or voting power"
        );
        Ok(())
    }
    pub fn id(&self, chain_id: &str) -> Result<String> {
        self.validate()?;
        let mut hash = Sha256::new();
        hash.update(b"dytallix-local-penalty-evidence-v1\0");
        hash.update(serde_json::to_vec(&(chain_id, self))?);
        Ok(hex::encode(hash.finalize()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrincipalTranche {
    pub id: String,
    pub owner: String,
    pub validator: String,
    pub gross_amount: u128,
    pub start_height: u64,
    pub last_exposure_height: Option<u64>,
    pub unbond_id: Option<String>,
    pub deducted: u128,
    pub released: u128,
}
impl PrincipalTranche {
    fn remaining(&self) -> Result<u128> {
        self.gross_amount
            .checked_sub(self.deducted)
            .and_then(|v| v.checked_sub(self.released))
            .context("Tranche deductions or releases exceed principal")
    }
    fn exposed_at(&self, height: u64) -> bool {
        self.start_height <= height
            && self
                .last_exposure_height
                .map_or(true, |last| height <= last)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Incident {
    pub fact: EvidenceFact,
    pub admitted_height: u64,
    pub validator: String,
    pub first_fault: bool,
    pub activation_height: u64,
    pub allocations: BTreeMap<String, u128>,
    pub settled_height: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseReceipt {
    pub unbond_id: String,
    pub owner: String,
    pub amount: u128,
    pub height: u64,
    pub parent_height: u64,
    pub parent_time_seconds: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assessment {
    pub validator: String,
    pub first_fault: bool,
    pub requires_exit: bool,
    pub incident_id: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PenaltyState {
    pub config: PenaltyConfig,
    pub tranches: BTreeMap<String, PrincipalTranche>,
    pub incidents: BTreeMap<String, Incident>,
    pub first_faults: BTreeMap<String, String>,
    pub releases: BTreeMap<String, ReleaseReceipt>,
    pub next_tranche_id: u64,
    pub last_height: u64,
    pub parent_time: (u64, i32),
    pub evidence_processed_height: u64,
}

fn valid_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 256
            && !id.chars().any(|c| c.is_whitespace() || c.is_control()),
        "Invalid penalty identifier"
    );
    Ok(())
}
fn valid_time(time: (u64, i32)) -> Result<()> {
    ensure!(
        (0..1_000_000_000).contains(&time.1),
        "Invalid evidence timestamp nanoseconds"
    );
    Ok(())
}
fn add(map: &mut BTreeMap<String, u128>, key: &str, amount: u128) -> Result<()> {
    let value = map.entry(key.into()).or_default();
    *value = value
        .checked_add(amount)
        .context("Penalty custody overflow")?;
    Ok(())
}
fn sum(values: impl IntoIterator<Item = u128>) -> Result<u128> {
    values.into_iter().try_fold(0u128, |a, b| {
        a.checked_add(b).context("Penalty custody overflow")
    })
}
fn floor_ratio(amount: u128, numerator: u64, denominator: u64) -> Result<u128> {
    ensure!(
        denominator > 0 && numerator <= denominator,
        "Invalid penalty fraction"
    );
    let (n, d) = (u128::from(numerator), u128::from(denominator));
    (amount / d)
        .checked_mul(n)
        .and_then(|whole| {
            (amount % d)
                .checked_mul(n)
                .and_then(|fraction| whole.checked_add(fraction / d))
        })
        .context("Penalty arithmetic overflow")
}
fn historical_view(lifecycle: &LifecycleState, height: u64) -> Result<&ValidatorView> {
    lifecycle
        .history
        .range(..=height)
        .next_back()
        .map(|(_, view)| view)
        .context("Missing offence validator history")
}
fn future_view(lifecycle: &LifecycleState, height: u64) -> &ValidatorView {
    lifecycle
        .schedules
        .range(..=height)
        .next_back()
        .map(|(_, s)| &s.view)
        .unwrap_or(&lifecycle.effective)
}

impl PenaltyState {
    pub fn initialize(
        config: PenaltyConfig,
        lifecycle: &LifecycleState,
        rewards: &RewardState,
    ) -> Result<Self> {
        config.validate()?;
        lifecycle.validate_rewards(rewards)?;
        ensure!(
            lifecycle.last_height == 0
                && lifecycle.schedules.is_empty()
                && lifecycle.unbonding.is_empty(),
            "Penalty custody requires fresh genesis"
        );
        ensure!(
            rewards.locks.is_empty(),
            "Locked principal penalty policy is not configured"
        );
        ensure!(
            config.chain_id == lifecycle.config.chain_id,
            "Penalty chain differs from lifecycle"
        );
        let mut state = Self {
            config,
            tranches: BTreeMap::new(),
            incidents: BTreeMap::new(),
            first_faults: BTreeMap::new(),
            releases: BTreeMap::new(),
            next_tranche_id: 0,
            last_height: 0,
            parent_time: (0, 0),
            evidence_processed_height: 0,
        };
        for (owner, positions) in &lifecycle.effective.positions {
            for (validator, amount) in positions {
                state.append_tranche(owner, validator, *amount, 1)?;
            }
        }
        state.validate(lifecycle)?;
        Ok(state)
    }
    fn append_tranche(
        &mut self,
        owner: &str,
        validator: &str,
        amount: u128,
        start: u64,
    ) -> Result<String> {
        let id = format!("tranche-{:020}", self.next_tranche_id);
        self.next_tranche_id = self
            .next_tranche_id
            .checked_add(1)
            .context("Tranche sequence overflow")?;
        ensure!(
            amount > 0 && self.tranches.len() < MAX_ITEMS,
            "Tranche capacity exceeded or zero principal"
        );
        ensure!(
            self.tranches
                .insert(
                    id.clone(),
                    PrincipalTranche {
                        id: id.clone(),
                        owner: owner.into(),
                        validator: validator.into(),
                        gross_amount: amount,
                        start_height: start,
                        last_exposure_height: None,
                        unbond_id: None,
                        deducted: 0,
                        released: 0
                    }
                )
                .is_none(),
            "Duplicate tranche identity"
        );
        Ok(id)
    }
    pub fn ensure_validator_allowed(&self, validator: &str) -> Result<()> {
        ensure!(
            !self.first_faults.contains_key(validator),
            "Faulted validator cannot acquire new exposure"
        );
        Ok(())
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate_internal()?;
        let bytes = bincode::serialize(self)?;
        ensure!(bytes.len() <= MAX_BYTES, "Penalty state exceeds byte bound");
        Ok(bytes)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() <= MAX_BYTES, "Penalty state exceeds byte bound");
        let state: Self = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_limit(MAX_BYTES as u64)
            .reject_trailing_bytes()
            .deserialize(bytes)?;
        state.validate_internal()?;
        ensure!(
            bincode::serialize(&state)? == bytes,
            "Noncanonical penalty encoding"
        );
        Ok(state)
    }

    fn validate_internal(&self) -> Result<()> {
        self.config.validate()?;
        valid_time(self.parent_time)?;
        ensure!(
            self.evidence_processed_height <= self.last_height,
            "Future evidence completion watermark"
        );
        ensure!(
            self.tranches.len() <= MAX_ITEMS
                && self.incidents.len() <= MAX_ITEMS
                && self.releases.len() <= MAX_ITEMS
                && self.first_faults.len() <= 64,
            "Penalty record capacity exceeded"
        );
        ensure!(
            u64::try_from(self.tranches.len())? == self.next_tranche_id,
            "Missing tranche sequence"
        );
        for (id, tranche) in &self.tranches {
            valid_id(&tranche.owner)?;
            valid_id(&tranche.validator)?;
            let sequence: u64 = id
                .strip_prefix("tranche-")
                .context("Invalid tranche ID")?
                .parse()?;
            ensure!(
                id == &tranche.id
                    && *id == format!("tranche-{sequence:020}")
                    && sequence < self.next_tranche_id
                    && tranche.gross_amount > 0
                    && tranche.start_height > 0,
                "Invalid principal tranche"
            );
            ensure!(
                tranche.last_exposure_height.is_some() == tranche.unbond_id.is_some(),
                "Incomplete tranche unbond reference"
            );
            if let Some(last) = tranche.last_exposure_height {
                ensure!(
                    last >= tranche.start_height,
                    "Tranche exits before first exposure"
                );
            }
            if tranche.deducted > 0 || tranche.released > 0 {
                ensure!(
                    tranche
                        .last_exposure_height
                        .is_some_and(|h| h < self.last_height),
                    "Penalty or release charged effective principal"
                );
            }
            tranche.remaining()?;
        }
        let mut deductions = BTreeMap::new();
        let mut reservations = BTreeMap::new();
        let mut first_faults = BTreeMap::new();
        for (id, incident) in &self.incidents {
            ensure!(
                *id == incident.fact.id(&self.config.chain_id)?,
                "Evidence receipt ID differs"
            );
            valid_id(&incident.validator)?;
            ensure!(
                incident.admitted_height > incident.fact.height
                    && incident.admitted_height <= self.last_height
                    && incident.admitted_height.checked_add(2) == Some(incident.activation_height),
                "Invalid evidence receipt height"
            );
            if incident.first_fault {
                ensure!(
                    first_faults
                        .insert(incident.validator.clone(), id.clone())
                        .is_none(),
                    "Repeated first-fault penalty"
                );
                ensure!(
                    incident
                        .settled_height
                        .map_or(true, |h| h == incident.activation_height
                            && h <= self.last_height),
                    "Invalid penalty settlement height"
                );
            } else {
                ensure!(
                    incident.allocations.is_empty()
                        && incident.settled_height == Some(incident.admitted_height),
                    "Later fault attempted another penalty"
                );
            }
            ensure!(
                incident.allocations.len() <= MAX_ITEMS,
                "Penalty allocation bound exceeded"
            );
            for (tranche_id, amount) in &incident.allocations {
                let tranche = self
                    .tranches
                    .get(tranche_id)
                    .context("Penalty allocation lacks tranche")?;
                ensure!(
                    *amount > 0
                        && tranche.validator == incident.validator
                        && tranche.exposed_at(incident.fact.height),
                    "Penalty allocation lacks offence exposure"
                );
                if incident.settled_height.is_some() {
                    add(&mut deductions, tranche_id, *amount)?;
                } else {
                    add(&mut reservations, tranche_id, *amount)?;
                }
            }
        }
        ensure!(
            first_faults == self.first_faults,
            "First-fault index differs from receipts"
        );
        for incident in self.incidents.values() {
            let first = &self.incidents[&self.first_faults[&incident.validator]];
            ensure!(
                first.admitted_height <= incident.admitted_height,
                "Later fault precedes first-fault record"
            );
        }
        let mut released_by_entry = BTreeMap::new();
        for (id, tranche) in &self.tranches {
            ensure!(
                tranche.deducted == deductions.get(id).copied().unwrap_or(0)
                    && reservations.get(id).copied().unwrap_or(0) <= tranche.remaining()?,
                "Tranche penalty accounting differs from receipts"
            );
            if let Some(entry) = &tranche.unbond_id {
                add(&mut released_by_entry, entry, tranche.released)?;
            }
        }
        for (id, receipt) in &self.releases {
            ensure!(
                id == &receipt.unbond_id
                    && receipt.height <= self.last_height
                    && receipt.parent_height.checked_add(1) == Some(receipt.height)
                    && released_by_entry.remove(id).unwrap_or(0) == receipt.amount,
                "Invalid release receipt"
            );
            valid_id(&receipt.owner)?;
            ensure!(
                self.tranches
                    .values()
                    .filter(|t| t.unbond_id.as_ref() == Some(id))
                    .all(|t| t.owner == receipt.owner && t.remaining().ok() == Some(0)),
                "Release owner or remaining custody differs"
            );
        }
        ensure!(
            released_by_entry.values().all(|amount| *amount == 0),
            "Released principal lacks receipt"
        );
        self.validate_capacity(MAX_BYTES as u64)?;
        Ok(())
    }
    fn validate_capacity(&self, limit: u64) -> Result<()> {
        // None -> Some(height) grows each scheduled settlement receipt by eight bytes.
        let pending = u64::try_from(
            self.incidents
                .values()
                .filter(|i| i.first_fault && i.settled_height.is_none())
                .count(),
        )?;
        let projected = bincode::serialized_size(self)?
            .checked_add(
                pending
                    .checked_mul(8)
                    .context("Penalty capacity reservation overflow")?,
            )
            .context("Penalty capacity overflow")?;
        ensure!(
            projected <= limit,
            "Penalty state or reserved activation exceeds byte bound"
        );
        Ok(())
    }

    fn positions_at(&self, height: u64) -> Result<Positions> {
        let mut result: Positions = BTreeMap::new();
        for tranche in self.tranches.values().filter(|t| t.exposed_at(height)) {
            add(
                result.entry(tranche.owner.clone()).or_default(),
                &tranche.validator,
                tranche.gross_amount,
            )?;
        }
        Ok(result)
    }
    pub fn validate(&self, lifecycle: &LifecycleState) -> Result<()> {
        self.validate_internal()?;
        lifecycle.validate()?;
        ensure!(
            self.config.chain_id == lifecycle.config.chain_id
                && self.last_height == lifecycle.last_height,
            "Penalty lifecycle context differs"
        );
        ensure!(
            self.positions_at(self.last_height.max(1))? == lifecycle.effective.positions,
            "Tranches differ from effective gross principal"
        );
        for (height, schedule) in &lifecycle.schedules {
            ensure!(
                self.positions_at(*height)? == schedule.view.positions,
                "Tranches differ from scheduled gross principal"
            );
        }
        let mut unbonded = BTreeMap::new();
        let mut pending = BTreeMap::new();
        for tranche in self.tranches.values() {
            if let Some(id) = &tranche.unbond_id {
                let entry = lifecycle
                    .unbonding
                    .get(id)
                    .or_else(|| {
                        lifecycle
                            .schedules
                            .values()
                            .flat_map(|s| &s.removals)
                            .find(|entry| entry.id == *id)
                    })
                    .context("Tranche references missing gross unbond entry")?;
                ensure!(
                    entry.owner == tranche.owner
                        && entry.validator == tranche.validator
                        && tranche.last_exposure_height == entry.effective_height.checked_sub(1),
                    "Tranche unbond exposure differs"
                );
                add(&mut unbonded, id, tranche.gross_amount)?;
            }
            if tranche.start_height > self.last_height.max(1) {
                add(&mut pending, &tranche.owner, tranche.gross_amount)?;
            }
        }
        for entry in lifecycle
            .unbonding
            .values()
            .chain(lifecycle.schedules.values().flat_map(|s| &s.removals))
        {
            ensure!(
                unbonded.remove(&entry.id) == Some(entry.amount),
                "Tranche gross unbond total differs"
            );
        }
        ensure!(
            unbonded.is_empty() && pending == lifecycle.pending_bonds_by_owner()?,
            "Tranche pending or unbond custody differs"
        );
        for incident in self.incidents.values() {
            let view = historical_view(lifecycle, incident.fact.height)?;
            let identity = view
                .validators
                .get(&incident.validator)
                .context("Evidence validator absent from history")?;
            ensure!(
                consensus_address(&identity.pubkey_base64)? == incident.fact.validator_address,
                "Evidence historical key differs"
            );
        }
        Ok(())
    }

    pub fn sync_lifecycle(
        &mut self,
        before: &LifecycleState,
        after: &LifecycleState,
    ) -> Result<()> {
        before.validate()?;
        after.validate()?;
        ensure!(
            before.config == after.config
                && (after.last_height == before.last_height
                    || before.last_height.checked_add(1) == Some(after.last_height)),
            "Invalid lifecycle sync context"
        );
        ensure!(
            self.last_height == before.last_height,
            "Penalty sync height differs"
        );
        let mut next = self.clone();
        for (height, schedule) in &after.schedules {
            let old_additions = before
                .schedules
                .get(height)
                .map(|s| s.additions.as_slice())
                .unwrap_or(&[]);
            let old_removals = before
                .schedules
                .get(height)
                .map(|s| s.removals.as_slice())
                .unwrap_or(&[]);
            ensure!(
                schedule.additions.starts_with(old_additions)
                    && schedule.removals.starts_with(old_removals),
                "Lifecycle sync rewrote existing principal operations"
            );
            for bond in &schedule.additions[old_additions.len()..] {
                next.ensure_validator_allowed(&bond.validator)?;
                next.append_tranche(&bond.owner, &bond.validator, bond.amount, *height)?;
            }
            for entry in &schedule.removals[old_removals.len()..] {
                let mut candidates: Vec<_> = next
                    .tranches
                    .values()
                    .filter(|t| {
                        t.owner == entry.owner
                            && t.validator == entry.validator
                            && t.unbond_id.is_none()
                            && t.start_height < *height
                    })
                    .map(|t| (t.start_height, t.id.clone()))
                    .collect();
                candidates.sort();
                let mut remaining = entry.amount;
                for (_, id) in candidates {
                    if remaining == 0 {
                        break;
                    }
                    let source = next.tranches[&id].clone();
                    let amount = remaining.min(source.gross_amount);
                    let target_id = if amount < source.gross_amount {
                        ensure!(
                            !next
                                .incidents
                                .values()
                                .any(|incident| incident.allocations.contains_key(&id)),
                            "Cannot split reserved penalty principal"
                        );
                        next.tranches
                            .get_mut(&id)
                            .context("Missing split tranche")?
                            .gross_amount -= amount;
                        next.append_tranche(
                            &source.owner,
                            &source.validator,
                            amount,
                            source.start_height,
                        )?
                    } else {
                        id
                    };
                    let target = next
                        .tranches
                        .get_mut(&target_id)
                        .context("Missing unbond tranche")?;
                    target.last_exposure_height = height.checked_sub(1);
                    target.unbond_id = Some(entry.id.clone());
                    remaining -= amount;
                }
                ensure!(
                    remaining == 0,
                    "Gross removal exceeds eligible tranche principal"
                );
            }
        }
        // Block activation does not create principal. begin_block owns the height transition.
        if before.last_height == after.last_height {
            next.validate(after)?;
        } else {
            let mut projected = next.clone();
            projected.last_height = after.last_height;
            projected.validate(after)?;
        }
        *self = next;
        Ok(())
    }

    pub fn begin_block(
        &mut self,
        height: u64,
        parent_time: (u64, i32),
        lifecycle: &LifecycleState,
    ) -> Result<()> {
        self.validate_internal()?;
        valid_time(parent_time)?;
        ensure!(
            self.last_height.checked_add(1) == Some(height) && lifecycle.last_height == height,
            "Penalty block height is not consecutive"
        );
        ensure!(
            parent_time >= self.parent_time,
            "Penalty parent time moved backward"
        );
        let mut next = self.clone();
        next.last_height = height;
        next.parent_time = parent_time;
        let ids: Vec<_> = next
            .incidents
            .iter()
            .filter(|(_, i)| {
                i.first_fault && i.settled_height.is_none() && i.activation_height <= height
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            let incident = next.incidents[&id].clone();
            ensure!(
                incident.activation_height == height,
                "Missed penalty activation"
            );
            ensure!(
                !lifecycle
                    .effective
                    .validators
                    .contains_key(&incident.validator),
                "Penalty activation requires effective full exit"
            );
            for (tranche_id, amount) in &incident.allocations {
                let tranche = next
                    .tranches
                    .get_mut(tranche_id)
                    .context("Missing penalty tranche")?;
                let unbond_id = tranche
                    .unbond_id
                    .as_ref()
                    .context("Penalty principal has not entered unbonding")?;
                ensure!(
                    lifecycle.unbonding.contains_key(unbond_id)
                        && tranche
                            .last_exposure_height
                            .is_some_and(|last| last < height),
                    "Penalty principal is still effective"
                );
                tranche.deducted = tranche
                    .deducted
                    .checked_add(*amount)
                    .context("Penalty deduction overflow")?;
                tranche.remaining()?;
            }
            next.incidents
                .get_mut(&id)
                .context("Missing activated incident")?
                .settled_height = Some(height);
        }
        next.validate(lifecycle)?;
        *self = next;
        Ok(())
    }

    pub fn assess(
        &mut self,
        fact: &EvidenceFact,
        historical_time: (u64, i32),
        parent_height: u64,
        parent_time: (u64, i32),
        lifecycle: &LifecycleState,
    ) -> Result<Assessment> {
        self.validate(lifecycle)?;
        fact.validate()?;
        valid_time(historical_time)?;
        valid_time(parent_time)?;
        ensure!(
            parent_height.checked_add(1) == Some(self.last_height)
                && self.parent_time == parent_time,
            "Evidence parent context differs"
        );
        ensure!(
            fact.height <= parent_height
                && (fact.time_seconds, fact.time_nanos) == historical_time
                && historical_time <= parent_time,
            "Evidence historical timestamp differs or is in future"
        );
        let expiry_time = (
            fact.time_seconds
                .checked_add(lifecycle.config.evidence_max_age_seconds)
                .context("Evidence time expiry overflow")?,
            fact.time_nanos,
        );
        ensure!(
            !(parent_height - fact.height > lifecycle.config.evidence_max_age_blocks
                && parent_time > expiry_time),
            "Evidence is expired by both age limits"
        );
        let view = historical_view(lifecycle, fact.height)?;
        let mut matched = None;
        for (id, identity) in &view.validators {
            if consensus_address(&identity.pubkey_base64)? == fact.validator_address {
                ensure!(
                    matched.replace(id.clone()).is_none(),
                    "Ambiguous historical evidence key"
                );
            }
        }
        let validator = matched.context("Evidence validator not in offence set")?;
        let power = sum(view
            .positions
            .values()
            .filter_map(|p| p.get(&validator))
            .copied())?;
        let total = sum(view.positions.values().flat_map(|p| p.values()).copied())?;
        ensure!(
            i64::try_from(power)? == fact.power && i64::try_from(total)? == fact.total_power,
            "Evidence historical voting power differs"
        );
        let incident_id = fact.id(&self.config.chain_id)?;
        ensure!(
            self.evidence_processed_height < self.last_height,
            "Evidence batch already finalized"
        );
        // ABCI omits vote rounds. Distinct engine evidence can yield identical facts.
        if self.incidents.contains_key(&incident_id) {
            return Ok(Assessment {
                validator,
                first_fault: false,
                requires_exit: false,
                incident_id,
            });
        }
        let mut next = self.clone();
        let first_fault = !next.first_faults.contains_key(&validator);
        let activation_height = self
            .last_height
            .checked_add(2)
            .context("Penalty activation height overflow")?;
        let mut allocations = BTreeMap::new();
        if first_fault {
            let mut eligible: BTreeMap<String, Vec<(u64, String, u128)>> = BTreeMap::new();
            for tranche in next
                .tranches
                .values()
                .filter(|t| t.validator == validator && t.exposed_at(fact.height))
            {
                let amount = tranche.remaining()?;
                if amount > 0 {
                    eligible.entry(tranche.owner.clone()).or_default().push((
                        tranche.start_height,
                        tranche.id.clone(),
                        amount,
                    ));
                }
            }
            for tranches in eligible.values_mut() {
                tranches.sort();
                let mut amount = floor_ratio(
                    sum(tranches.iter().map(|(_, _, a)| *a))?,
                    next.config.penalty_numerator,
                    next.config.penalty_denominator,
                )?;
                for (_, id, available) in tranches {
                    let take = amount.min(*available);
                    if take > 0 {
                        allocations.insert(id.clone(), take);
                    }
                    amount -= take;
                }
                ensure!(amount == 0, "Penalty exceeds eligible owner principal");
            }
            next.first_faults
                .insert(validator.clone(), incident_id.clone());
        }
        next.incidents.insert(
            incident_id.clone(),
            Incident {
                fact: fact.clone(),
                admitted_height: self.last_height,
                validator: validator.clone(),
                first_fault,
                activation_height,
                allocations,
                settled_height: (!first_fault).then_some(self.last_height),
            },
        );
        next.validate(lifecycle)?;
        let requires_exit = future_view(lifecycle, activation_height)
            .validators
            .contains_key(&validator);
        *self = next;
        Ok(Assessment {
            validator,
            first_fault,
            requires_exit,
            incident_id,
        })
    }

    pub fn finalize_evidence_batch(&mut self, lifecycle: &LifecycleState) -> Result<()> {
        self.validate(lifecycle)?;
        ensure!(self.last_height > 0, "Cannot finalize genesis evidence");
        let height = self
            .last_height
            .checked_add(2)
            .context("Evidence exit height overflow")?;
        for validator in self.first_faults.keys() {
            ensure!(
                !future_view(lifecycle, height)
                    .validators
                    .contains_key(validator),
                "Evidence batch has no scheduled full validator exit"
            );
        }
        let mut next = self.clone();
        next.evidence_processed_height = next.last_height;
        next.validate(lifecycle)?;
        *self = next;
        Ok(())
    }

    pub fn withdraw(
        &mut self,
        owner: &str,
        unbond_id: &str,
        parent_height: u64,
        parent_seconds: u64,
        lifecycle: &LifecycleState,
    ) -> Result<u128> {
        self.validate(lifecycle)?;
        valid_id(owner)?;
        valid_id(unbond_id)?;
        ensure!(
            parent_height.checked_add(1) == Some(self.last_height)
                && parent_seconds == self.parent_time.0,
            "Withdrawal parent context differs"
        );
        ensure!(
            self.evidence_processed_height == self.last_height,
            "Evidence batch is incomplete"
        );
        ensure!(
            !self.releases.contains_key(unbond_id),
            "Unbond entry already released"
        );
        let entry = lifecycle
            .unbonding
            .get(unbond_id)
            .context("Unknown unbond entry")?;
        ensure!(
            entry.owner == owner,
            "Unbond entry belongs to another owner"
        );
        let ids: BTreeSet<_> = self
            .tranches
            .values()
            .filter(|t| t.unbond_id.as_deref() == Some(unbond_id))
            .map(|t| t.id.clone())
            .collect();
        ensure!(!ids.is_empty(), "Unbond entry has no principal tranches");
        ensure!(
            !self
                .incidents
                .values()
                .any(|incident| incident.settled_height.is_none()
                    && incident.allocations.keys().any(|id| ids.contains(id))),
            "Unbond entry has unsettled penalties"
        );
        ensure!(
            entry.maturity_satisfied(&lifecycle.config, parent_height, parent_seconds, true)?,
            "Unbond entry has not passed both evidence margins"
        );
        let mut next = self.clone();
        let mut amount = 0u128;
        for id in ids {
            let tranche = next
                .tranches
                .get_mut(&id)
                .context("Missing release tranche")?;
            let remaining = tranche.remaining()?;
            tranche.released = tranche
                .released
                .checked_add(remaining)
                .context("Release amount overflow")?;
            amount = amount
                .checked_add(remaining)
                .context("Release total overflow")?;
        }
        next.releases.insert(
            unbond_id.into(),
            ReleaseReceipt {
                unbond_id: unbond_id.into(),
                owner: owner.into(),
                amount,
                height: self.last_height,
                parent_height,
                parent_time_seconds: parent_seconds,
            },
        );
        next.validate(lifecycle)?;
        *self = next;
        Ok(amount)
    }

    pub fn deducted_total(&self) -> Result<u128> {
        sum(self.tranches.values().map(|t| t.deducted))
    }
    pub fn released_total(&self) -> Result<u128> {
        sum(self.tranches.values().map(|t| t.released))
    }
    pub fn penalty_reserve(&self) -> Result<u128> {
        self.deducted_total()
    }
    pub fn owner_deducted(&self, owner: &str) -> Result<u128> {
        sum(self
            .tranches
            .values()
            .filter(|t| t.owner == owner)
            .map(|t| t.deducted))
    }
    pub fn owner_released(&self, owner: &str) -> Result<u128> {
        sum(self
            .tranches
            .values()
            .filter(|t| t.owner == owner)
            .map(|t| t.released))
    }
    pub fn net_unbonding_by_owner(
        &self,
        lifecycle: &LifecycleState,
    ) -> Result<BTreeMap<String, u128>> {
        self.validate(lifecycle)?;
        let mut owners = lifecycle.unbonding_by_owner()?;
        for tranche in self
            .tranches
            .values()
            .filter(|t| t.deducted > 0 || t.released > 0)
        {
            let remaining = owners
                .get_mut(&tranche.owner)
                .context("Deduction owner has no gross unbond custody")?;
            *remaining = remaining
                .checked_sub(tranche.deducted)
                .and_then(|v| v.checked_sub(tranche.released))
                .context("Net unbond custody underflow")?;
        }
        owners.retain(|_, amount| *amount > 0);
        Ok(owners)
    }
    pub fn net_unbonding_total(&self, lifecycle: &LifecycleState) -> Result<u128> {
        sum(self.net_unbonding_by_owner(lifecycle)?.into_values())
    }
}

#[cfg(test)]
#[path = "penalty_custody_tests.rs"]
mod tests;
