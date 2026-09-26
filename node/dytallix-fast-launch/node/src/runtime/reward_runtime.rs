//! Bounded reward state for an explicit development lifecycle.
//! Callers fund bonds and commit pool debits, account credits and this state atomically.
//! Development block commits do not establish production consensus finality.
use super::reward_allocation::allocate_reward_budget;
use anyhow::{ensure, Context, Result};
use bincode::Options;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const REWARD_STATE_KEY: &str = "rewards:v2:state";
const MAX_ITEMS: usize = 10_000;
const MAX_STATE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RewardConfig {
    pub version: u32,
    pub activation_height: u64,
    pub decimals: u8,
    pub profile: String,
    pub chain_id: String,
    pub genesis_digest: String,
    pub max_validators: usize,
    pub max_positions: usize,
}
impl RewardConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 2 && self.activation_height == 1 && self.decimals == 6,
            "Unsupported reward version, activation height or token scale"
        );
        ensure!(
            self.profile == "development",
            "Production rewards require qualified finality and activation"
        );
        valid_id(&self.chain_id)?;
        ensure!(
            self.genesis_digest.len() == 64
                && self.genesis_digest.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid reward genesis digest"
        );
        ensure!(
            (1..=MAX_ITEMS).contains(&self.max_validators)
                && (1..=MAX_ITEMS).contains(&self.max_positions),
            "Invalid reward resource limits"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidatorStatus {
    pub active: bool,
    pub jailed: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VestingLock {
    pub total_amount: u128,
    pub start_time: u64,
    pub cliff_duration: u64,
    pub vesting_duration: u64,
    pub permits_staking: bool,
}
impl VestingLock {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.vesting_duration >= self.cliff_duration,
            "Vesting duration precedes cliff"
        );
        self.start_time
            .checked_add(self.vesting_duration)
            .context("Vesting deadline exceeds u64")?;
        Ok(())
    }
    pub fn locked_amount(&self, timestamp: u64) -> Result<u128> {
        self.validate()?;
        if timestamp < self.start_time {
            return Ok(self.total_amount);
        }
        let elapsed = timestamp - self.start_time;
        if elapsed < self.cliff_duration {
            return Ok(self.total_amount);
        }
        if elapsed >= self.vesting_duration {
            return Ok(0);
        }
        let numerator = u128::from(elapsed - self.cliff_duration);
        let denominator = u128::from(self.vesting_duration - self.cliff_duration);
        let vested = (self.total_amount / denominator)
            .checked_mul(numerator)
            .and_then(|a| {
                (self.total_amount % denominator)
                    .checked_mul(numerator)
                    .and_then(|b| a.checked_add(b / denominator))
            })
            .context("Vesting arithmetic exceeds u128")?;
        self.total_amount
            .checked_sub(vested)
            .context("Vesting exceeds allocation")
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RewardState {
    pub config: RewardConfig,
    pub validators: BTreeMap<String, ValidatorStatus>,
    pub positions: BTreeMap<String, BTreeMap<String, u128>>,
    pub unbonding: BTreeMap<String, u128>,
    pub locks: BTreeMap<String, VestingLock>,
    pub unpaid: BTreeMap<String, u128>,
    pub rounding_reserve: u128,
    pub inactive_reserve: u128,
    pub total_budget: u128,
    pub total_claimed: u128,
    pub last_height: u64,
    pub last_interval_digest: Option<[u8; 32]>,
    pub last_interval_input_digest: Option<[u8; 32]>,
}
fn valid_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 256
            && !id.chars().any(|c| c.is_whitespace() || c.is_control()),
        "Invalid reward identifier"
    );
    Ok(())
}
fn sum(mut values: impl Iterator<Item = u128>) -> Result<u128> {
    values.try_fold(0u128, |a, b| {
        a.checked_add(b).context("Reward amount sum exceeds u128")
    })
}
fn digest(value: &impl Serialize) -> Result<[u8; 32]> {
    let mut hash = Sha256::new();
    hash.update(b"dytallix-reward-runtime-v2");
    hash.update(bincode::serialize(value)?);
    Ok(hash.finalize().into())
}
impl RewardState {
    pub fn new(
        config: RewardConfig,
        validators: BTreeMap<String, ValidatorStatus>,
        positions: BTreeMap<String, BTreeMap<String, u128>>,
        locks: BTreeMap<String, VestingLock>,
    ) -> Result<Self> {
        let state = Self {
            config,
            validators,
            positions,
            locks,
            unbonding: BTreeMap::new(),
            unpaid: BTreeMap::new(),
            rounding_reserve: 0,
            inactive_reserve: 0,
            total_budget: 0,
            total_claimed: 0,
            last_height: 0,
            last_interval_digest: None,
            last_interval_input_digest: None,
        };
        state.validate_internal()?;
        Ok(state)
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate_internal()?;
        let bytes = bincode::serialize(self)?;
        ensure!(
            bytes.len() <= MAX_STATE_BYTES,
            "Reward state exceeds encoded limit"
        );
        Ok(bytes)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= MAX_STATE_BYTES,
            "Reward state exceeds encoded limit"
        );
        let state: Self = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_limit(MAX_STATE_BYTES as u64)
            .reject_trailing_bytes()
            .deserialize(bytes)
            .context("Invalid reward state encoding")?;
        state.validate_internal()?;
        ensure!(
            bincode::serialize(&state)? == bytes,
            "Noncanonical reward state"
        );
        Ok(state)
    }
    pub fn validate_internal(&self) -> Result<()> {
        self.config.validate()?;
        ensure!(
            self.validators.len() <= self.config.max_validators,
            "Validator limit exceeded"
        );
        for id in self.validators.keys() {
            valid_id(id)?;
        }
        let mut count = 0usize;
        for (owner, positions) in &self.positions {
            valid_id(owner)?;
            ensure!(!positions.is_empty(), "Empty owner positions");
            for (validator, amount) in positions {
                ensure!(
                    self.validators.contains_key(validator) && *amount > 0,
                    "Invalid bonded position"
                );
                count = count.checked_add(1).context("Position count overflow")?;
            }
            sum(positions.values().copied())?;
        }
        ensure!(
            count <= self.config.max_positions,
            "Position limit exceeded"
        );
        for entries in [&self.unbonding, &self.unpaid] {
            ensure!(
                entries.len() <= self.config.max_positions,
                "Reward owner limit exceeded"
            );
            for (owner, amount) in entries {
                valid_id(owner)?;
                ensure!(*amount > 0, "Zero reward owner entry");
            }
            sum(entries.values().copied())?;
        }
        ensure!(
            self.locks.len() <= self.config.max_positions,
            "Vesting owner limit exceeded"
        );
        for (owner, lock) in &self.locks {
            valid_id(owner)?;
            lock.validate()?;
        }
        // Reserve owner slots at admission, before an interval can create a claim.
        let owners: BTreeSet<_> = self
            .positions
            .keys()
            .chain(self.unbonding.keys())
            .chain(self.unpaid.keys())
            .chain(self.locks.keys())
            .collect();
        ensure!(
            owners.len() <= self.config.max_positions,
            "Reward owner limit exceeded"
        );
        self.total_bonded()?;
        ensure!(
            sum(self.unpaid.values().copied().chain([
                self.total_claimed,
                self.rounding_reserve,
                self.inactive_reserve
            ]))? == self.total_budget,
            "Reward liability conservation failed"
        );
        ensure!(
            (self.last_height == 0) == self.last_interval_digest.is_none()
                && self.last_interval_digest.is_some() == self.last_interval_input_digest.is_some(),
            "Invalid reward interval marker"
        );
        ensure!(
            self.last_height > 0 || self.total_budget == 0,
            "Reward budget precedes activation"
        );
        Ok(())
    }
    pub fn total_bonded(&self) -> Result<u128> {
        sum(self.positions.values().flat_map(|p| p.values().copied()))
    }
    pub fn owner_bonded(&self, owner: &str) -> Result<u128> {
        sum(self
            .positions
            .get(owner)
            .into_iter()
            .flat_map(|p| p.values().copied()))
    }
    pub fn total_unpaid(&self) -> Result<u128> {
        sum(self.unpaid.values().copied())
    }
    pub fn total_unbonding(&self) -> Result<u128> {
        sum(self.unbonding.values().copied())
    }
    pub fn eligible_snapshot(&self) -> Result<BTreeMap<String, u128>> {
        self.validate_internal()?;
        let mut snapshot = BTreeMap::new();
        for (owner, positions) in &self.positions {
            let weight = sum(positions.iter().filter_map(|(validator, amount)| {
                let status = &self.validators[validator];
                (status.active && !status.jailed).then_some(*amount)
            }))?;
            if weight > 0 {
                snapshot.insert(owner.clone(), weight);
            }
        }
        Ok(snapshot)
    }
    /// Call against parent state before applying the interval block's transactions.
    /// A matching last-interval retry returns false without allocating twice.
    pub fn stage_interval(&mut self, height: u64, parent_hash: &str, budget: u128) -> Result<bool> {
        self.validate_internal()?;
        valid_id(parent_hash)?;
        let input_digest = digest(&(
            &self.config.chain_id,
            &self.config.genesis_digest,
            self.config.version,
            height,
            parent_hash,
            budget,
        ))?;
        if height == self.last_height && self.last_height > 0 {
            ensure!(
                self.last_interval_input_digest == Some(input_digest),
                "Conflicting reward interval retry"
            );
            return Ok(false);
        }
        ensure!(
            self.last_height.checked_add(1) == Some(height),
            "Reward height is not next interval"
        );
        let snapshot = self.eligible_snapshot()?;
        let mut next = self.clone();
        next.total_budget = next
            .total_budget
            .checked_add(budget)
            .context("Reward budget exceeds u128")?;
        if snapshot.is_empty() {
            next.inactive_reserve = next
                .inactive_reserve
                .checked_add(budget)
                .context("Inactive reserve exceeds u128")?;
        } else {
            let allocation = allocate_reward_budget(budget, &snapshot)?;
            next.rounding_reserve = next
                .rounding_reserve
                .checked_add(allocation.reserve)
                .context("Rounding reserve exceeds u128")?;
            for (owner, amount) in allocation.entitlements {
                if amount > 0 {
                    let unpaid = next.unpaid.entry(owner).or_default();
                    *unpaid = unpaid
                        .checked_add(amount)
                        .context("Unpaid reward exceeds u128")?;
                }
            }
        }
        next.last_height = height;
        next.last_interval_input_digest = Some(input_digest);
        next.last_interval_digest = Some(digest(&(input_digest, snapshot))?);
        next.validate_internal()?;
        *self = next;
        Ok(true)
    }
    /// Discharge the owner's liability. The caller stages its pool-to-account transfer.
    pub fn claim(&mut self, owner: &str) -> Result<u128> {
        self.validate_internal()?;
        let amount = self.unpaid.get(owner).copied().unwrap_or(0);
        let mut next = self.clone();
        next.total_claimed = next
            .total_claimed
            .checked_add(amount)
            .context("Claim total exceeds u128")?;
        next.unpaid.remove(owner);
        next.validate_internal()?;
        *self = next;
        Ok(amount)
    }
    /// Caller must debit spendable DGT in the same atomic transition.
    pub fn bond(&mut self, owner: &str, validator: &str, amount: u128) -> Result<()> {
        self.validate_internal()?;
        valid_id(owner)?;
        ensure!(
            amount > 0 && self.validators.contains_key(validator),
            "Invalid bond"
        );
        let mut next = self.clone();
        let stake = next
            .positions
            .entry(owner.into())
            .or_default()
            .entry(validator.into())
            .or_default();
        *stake = stake
            .checked_add(amount)
            .context("Bonded position exceeds u128")?;
        next.validate_internal()?;
        *self = next;
        Ok(())
    }
    /// Unbonding custody remains locked. This module grants no release operation.
    pub fn begin_unbond(&mut self, owner: &str, validator: &str, amount: u128) -> Result<()> {
        self.validate_internal()?;
        ensure!(amount > 0, "Unbond amount is zero");
        let mut next = self.clone();
        let positions = next
            .positions
            .get_mut(owner)
            .context("Owner has no bonded position")?;
        let stake = positions
            .get_mut(validator)
            .context("Validator position is absent")?;
        *stake = stake
            .checked_sub(amount)
            .context("Unbond exceeds position")?;
        if *stake == 0 {
            positions.remove(validator);
        }
        if positions.is_empty() {
            next.positions.remove(owner);
        }
        let unbonding = next.unbonding.entry(owner.into()).or_default();
        *unbonding = unbonding
            .checked_add(amount)
            .context("Unbonding amount exceeds u128")?;
        next.validate_internal()?;
        *self = next;
        Ok(())
    }
    pub fn locked_amount(&self, owner: &str, timestamp: u64) -> Result<u128> {
        self.locks
            .get(owner)
            .map_or(Ok(0), |lock| lock.locked_amount(timestamp))
    }
    /// Nonliquid custody backs locked principal only when the lock permits staking.
    pub fn liquid_spendable(
        &self,
        owner: &str,
        liquid: u128,
        bonded: u128,
        unbonding: u128,
        timestamp: u64,
    ) -> Result<u128> {
        let custody = bonded
            .checked_add(unbonding)
            .context("Nonliquid custody exceeds u128")?;
        let locked = self.locked_amount(owner, timestamp)?;
        let liquid_lock = if self
            .locks
            .get(owner)
            .is_some_and(|lock| lock.permits_staking)
        {
            locked.saturating_sub(custody)
        } else {
            locked
        };
        liquid
            .checked_sub(liquid_lock)
            .context("Insufficient custody for vesting lock")
    }
}
