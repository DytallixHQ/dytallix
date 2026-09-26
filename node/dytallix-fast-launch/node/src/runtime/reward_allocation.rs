//! Exact allocation over a caller-supplied eligible stake snapshot.
//!
//! This module selects no interval duration, snapshot source, reward ownership,
//! zero-stake disposition, payout authorization, or legacy migration.
use serde::{de::MapAccess, de::Visitor, Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;

const RECORD_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RewardAllocationError {
    NoEligibleStake,
    TotalStakeOverflow,
    ArithmeticOverflow,
    InvalidIntervalId,
    UnsupportedVersion,
    InvalidRecord,
    Encoding,
    DuplicateInterval,
}

impl fmt::Display for RewardAllocationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for RewardAllocationError {}

/// Entitlements and reserved units classify one budget; they do not add supply.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RewardAllocation {
    #[serde(deserialize_with = "unique_map")]
    pub entitlements: BTreeMap<String, u128>,
    pub reserve: u128,
}

/// Return floor(budget * weight / total) without a u128 product intermediate.
/// The caller establishes 0 <= weight <= total and total > 0.
fn proportional_floor(
    budget: u128,
    weight: u128,
    total: u128,
) -> Result<u128, RewardAllocationError> {
    let mut quotient = 0u128;
    let mut remainder = 0u128;
    // After each bit, quotient * total + remainder equals prefix * weight.
    // Reduce before addition so neither 2*remainder nor remainder+weight wraps.
    for bit in (0..128).rev() {
        let mut carry = 0u128;
        if remainder >= total - remainder {
            remainder -= total - remainder;
            carry = 1;
        } else {
            remainder += remainder;
        }
        if (budget >> bit) & 1 == 1 {
            if remainder >= total - weight {
                remainder -= total - weight;
                carry += 1;
            } else {
                remainder += weight;
            }
        }
        quotient = quotient
            .checked_mul(2)
            .and_then(|q| q.checked_add(carry))
            .ok_or(RewardAllocationError::ArithmeticOverflow)?;
    }
    Ok(quotient)
}

/// Allocate a supplied budget with per-recipient floor rounding.
///
/// The caller must supply the complete eligible snapshot with unique account IDs.
/// Zero-weight entries receive zero. An all-zero snapshot returns an error without
/// assigning or reserving the budget. No remainder is redistributed automatically.
pub fn allocate_reward_budget(
    budget: u128,
    snapshot: &BTreeMap<String, u128>,
) -> Result<RewardAllocation, RewardAllocationError> {
    let total = snapshot.values().try_fold(0u128, |sum, weight| {
        sum.checked_add(*weight)
            .ok_or(RewardAllocationError::TotalStakeOverflow)
    })?;
    if total == 0 {
        return Err(RewardAllocationError::NoEligibleStake);
    }
    let mut entitlements = BTreeMap::new();
    let mut allocated = 0u128;
    for (account, weight) in snapshot {
        let amount = proportional_floor(budget, *weight, total)?;
        allocated = allocated
            .checked_add(amount)
            .ok_or(RewardAllocationError::ArithmeticOverflow)?;
        entitlements.insert(account.clone(), amount);
    }
    let reserve = budget
        .checked_sub(allocated)
        .ok_or(RewardAllocationError::ArithmeticOverflow)?;
    Ok(RewardAllocation {
        entitlements,
        reserve,
    })
}

/// Immutable allocation evidence for a caller-defined interval.
///
/// Decode recomputes allocations and the digest. The digest detects corruption;
/// it is not an authorization signature or proof that the snapshot is eligible.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IntervalRewardAllocation {
    version: u32,
    interval_id: String,
    budget: u128,
    snapshot: BTreeMap<String, u128>,
    allocation: RewardAllocation,
    digest: [u8; 32],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredIntervalRewardAllocation {
    version: u32,
    interval_id: String,
    budget: u128,
    #[serde(deserialize_with = "unique_map")]
    snapshot: BTreeMap<String, u128>,
    allocation: RewardAllocation,
    digest: [u8; 32],
}

// Do not silently overwrite repeated IDs while decoding a persisted snapshot.
fn unique_map<'de, D, T>(deserializer: D) -> Result<BTreeMap<String, T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct UniqueMap<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> Visitor<'de> for UniqueMap<T> {
        type Value = BTreeMap<String, T>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a map with unique account or interval IDs")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
            let mut values = BTreeMap::new();
            while let Some((key, value)) = access.next_entry::<String, T>()? {
                if values.insert(key, value).is_some() {
                    return Err(serde::de::Error::custom("Duplicate record key"));
                }
            }
            Ok(values)
        }
    }
    deserializer.deserialize_map(UniqueMap(std::marker::PhantomData))
}

impl<'de> Deserialize<'de> for IntervalRewardAllocation {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let stored = StoredIntervalRewardAllocation::deserialize(deserializer)?;
        let record = Self {
            version: stored.version,
            interval_id: stored.interval_id,
            budget: stored.budget,
            snapshot: stored.snapshot,
            allocation: stored.allocation,
            digest: stored.digest,
        };
        record.verify().map_err(serde::de::Error::custom)?;
        Ok(record)
    }
}

impl IntervalRewardAllocation {
    pub fn new(
        interval_id: String,
        budget: u128,
        snapshot: BTreeMap<String, u128>,
    ) -> Result<Self, RewardAllocationError> {
        if interval_id.is_empty() {
            return Err(RewardAllocationError::InvalidIntervalId);
        }
        let allocation = allocate_reward_budget(budget, &snapshot)?;
        let mut record = Self {
            version: RECORD_VERSION,
            interval_id,
            budget,
            snapshot,
            allocation,
            digest: [0; 32],
        };
        record.digest = record.compute_digest()?;
        Ok(record)
    }

    fn compute_digest(&self) -> Result<[u8; 32], RewardAllocationError> {
        let encoded = bincode::serialize(&(
            self.version,
            &self.interval_id,
            self.budget,
            &self.snapshot,
            &self.allocation,
        ))
        .map_err(|_| RewardAllocationError::Encoding)?;
        let mut hasher = Sha256::new();
        hasher.update(b"dytallix-interval-reward-allocation-v1");
        hasher.update(encoded);
        Ok(hasher.finalize().into())
    }

    pub fn verify(&self) -> Result<(), RewardAllocationError> {
        if self.version != RECORD_VERSION {
            return Err(RewardAllocationError::UnsupportedVersion);
        }
        if self.interval_id.is_empty() {
            return Err(RewardAllocationError::InvalidIntervalId);
        }
        if allocate_reward_budget(self.budget, &self.snapshot)? != self.allocation
            || self.compute_digest()? != self.digest
        {
            return Err(RewardAllocationError::InvalidRecord);
        }
        Ok(())
    }

    pub fn interval_id(&self) -> &str {
        &self.interval_id
    }

    pub fn budget(&self) -> u128 {
        self.budget
    }

    pub fn snapshot(&self) -> &BTreeMap<String, u128> {
        &self.snapshot
    }

    pub fn allocation(&self) -> &RewardAllocation {
        &self.allocation
    }

    pub fn digest(&self) -> &[u8; 32] {
        &self.digest
    }
}

/// Local claim-liability ledger for explicitly supplied interval records.
///
/// `claim` records a liability discharge. It does not transfer tokens, authorize
/// a claimant, debit pool custody, or make an external payout atomic. Production
/// callers must stage this state with their approved payout transaction.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct RewardLedger {
    records: BTreeMap<String, IntervalRewardAllocation>,
    unpaid: BTreeMap<String, u128>,
    paid: BTreeMap<String, u128>,
    total_budget: u128,
    total_paid: u128,
    reserve: u128,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredRewardLedger {
    #[serde(deserialize_with = "unique_map")]
    records: BTreeMap<String, IntervalRewardAllocation>,
    #[serde(deserialize_with = "unique_map")]
    unpaid: BTreeMap<String, u128>,
    #[serde(deserialize_with = "unique_map")]
    paid: BTreeMap<String, u128>,
    total_budget: u128,
    total_paid: u128,
    reserve: u128,
}

impl<'de> Deserialize<'de> for RewardLedger {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let stored = StoredRewardLedger::deserialize(deserializer)?;
        let ledger = Self {
            records: stored.records,
            unpaid: stored.unpaid,
            paid: stored.paid,
            total_budget: stored.total_budget,
            total_paid: stored.total_paid,
            reserve: stored.reserve,
        };
        ledger.verify().map_err(serde::de::Error::custom)?;
        Ok(ledger)
    }
}

impl RewardLedger {
    /// Accept exactly one allocation per caller-supplied interval ID.
    /// Any error leaves this ledger unchanged.
    pub fn accept(
        &mut self,
        record: IntervalRewardAllocation,
    ) -> Result<(), RewardAllocationError> {
        record.verify()?;
        if self.records.contains_key(record.interval_id()) {
            return Err(RewardAllocationError::DuplicateInterval);
        }
        let mut planned = self.clone();
        planned.total_budget = planned
            .total_budget
            .checked_add(record.budget())
            .ok_or(RewardAllocationError::ArithmeticOverflow)?;
        planned.reserve = planned
            .reserve
            .checked_add(record.allocation().reserve)
            .ok_or(RewardAllocationError::ArithmeticOverflow)?;
        for (account, amount) in &record.allocation().entitlements {
            let unpaid = planned.unpaid.entry(account.clone()).or_default();
            *unpaid = unpaid
                .checked_add(*amount)
                .ok_or(RewardAllocationError::ArithmeticOverflow)?;
        }
        planned.records.insert(record.interval_id().into(), record);
        planned.verify()?;
        *self = planned;
        Ok(())
    }

    pub fn accrued(&self, account: &str) -> u128 {
        self.unpaid.get(account).copied().unwrap_or(0)
    }

    /// Record the complete unpaid entitlement as discharged; no tokens move.
    /// Any error leaves this ledger unchanged. A second claim returns zero.
    pub fn claim(&mut self, account: &str) -> Result<u128, RewardAllocationError> {
        let amount = self.accrued(account);
        if amount == 0 {
            return Ok(0);
        }
        let mut planned = self.clone();
        planned.total_paid = planned
            .total_paid
            .checked_add(amount)
            .ok_or(RewardAllocationError::ArithmeticOverflow)?;
        let paid = planned.paid.entry(account.into()).or_default();
        *paid = paid
            .checked_add(amount)
            .ok_or(RewardAllocationError::ArithmeticOverflow)?;
        planned.unpaid.insert(account.into(), 0);
        planned.verify()?;
        *self = planned;
        Ok(amount)
    }

    pub fn total_budget(&self) -> u128 {
        self.total_budget
    }

    pub fn total_paid(&self) -> u128 {
        self.total_paid
    }

    pub fn reserve(&self) -> u128 {
        self.reserve
    }

    pub fn total_unpaid(&self) -> u128 {
        // The private state is constructed and decoded through verification.
        self.total_budget - self.total_paid - self.reserve
    }

    pub fn interval_count(&self) -> usize {
        self.records.len()
    }

    pub fn verify(&self) -> Result<(), RewardAllocationError> {
        let mut budget = 0u128;
        let mut reserve = 0u128;
        let mut entitlements = BTreeMap::<String, u128>::new();
        for (id, record) in &self.records {
            record.verify()?;
            if id != record.interval_id() {
                return Err(RewardAllocationError::InvalidRecord);
            }
            budget = budget
                .checked_add(record.budget())
                .ok_or(RewardAllocationError::ArithmeticOverflow)?;
            reserve = reserve
                .checked_add(record.allocation().reserve)
                .ok_or(RewardAllocationError::ArithmeticOverflow)?;
            for (account, amount) in &record.allocation().entitlements {
                let total = entitlements.entry(account.clone()).or_default();
                *total = total
                    .checked_add(*amount)
                    .ok_or(RewardAllocationError::ArithmeticOverflow)?;
            }
        }
        let mut paid_total = 0u128;
        for (account, paid) in &self.paid {
            if *paid == 0 {
                return Err(RewardAllocationError::InvalidRecord);
            }
            let unpaid = entitlements
                .get_mut(account)
                .ok_or(RewardAllocationError::InvalidRecord)?;
            *unpaid = unpaid
                .checked_sub(*paid)
                .ok_or(RewardAllocationError::InvalidRecord)?;
            paid_total = paid_total
                .checked_add(*paid)
                .ok_or(RewardAllocationError::ArithmeticOverflow)?;
        }
        let unpaid_total = entitlements.values().try_fold(0u128, |sum, value| {
            sum.checked_add(*value)
                .ok_or(RewardAllocationError::ArithmeticOverflow)
        })?;
        let accounted = unpaid_total
            .checked_add(paid_total)
            .and_then(|v| v.checked_add(reserve))
            .ok_or(RewardAllocationError::ArithmeticOverflow)?;
        if entitlements != self.unpaid
            || budget != self.total_budget
            || reserve != self.reserve
            || paid_total != self.total_paid
            || accounted != budget
        {
            return Err(RewardAllocationError::InvalidRecord);
        }
        Ok(())
    }
}
