//! Native-token custody accounting for the selected block lifecycle.
//! Unpaid rewards and both reserves remain inside staking-pool custody.
use crate::{
    block_lifecycle::Writes,
    runtime::governance_state::{GovernanceState, STATE_KEY as GOVERNANCE_STATE_KEY},
    runtime::issuance_timing::{self, PoolAmounts, TimingState, TIMING_STATE_KEY},
    runtime::penalty_custody::{PenaltyState, STATE_KEY as PENALTY_STATE_KEY},
    runtime::reward_runtime::{RewardState, REWARD_STATE_KEY},
    runtime::staking::{DelegatorRewardRecord, TOTAL_STAKE_KEY},
    runtime::validator_lifecycle::{LifecycleState, STATE_KEY as VALIDATOR_STATE_KEY},
    settlement::FEE_KEY,
    state::{DGT_MAX_SUPPLY, DGT_MINTED_KEY},
    storage::state::Storage,
};
use anyhow::{ensure, Context, Result};
use rocksdb::IteratorMode;
use serde::Serialize;
use std::collections::BTreeMap;

pub const DRT_GENESIS_KEY: &str = "supply:drt_genesis";
const EMITTED: &str = "emission:circulating_supply";
const POOLS: [&str; 4] = [
    "block_rewards",
    "staking_rewards",
    "ai_module_incentives",
    "bridge_operations",
];
const TIMED_POOLS: [&str; 4] = [
    "validator_rewards",
    "staking_rewards",
    "treasury",
    "issuance_reserve",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TimingSupply {
    pub mode: &'static str,
    pub profile: String,
    pub last_height: u64,
    pub active_epoch: u64,
    pub epoch_blocks: u64,
    pub epoch_budget_udrt: String,
    pub issued_in_epoch: BTreeMap<String, String>,
    pub total_issued: BTreeMap<String, String>,
}
fn timed_amounts(amounts: &PoolAmounts) -> BTreeMap<String, u128> {
    BTreeMap::from([
        ("validator_rewards".into(), amounts.validator_rewards),
        ("staking_rewards".into(), amounts.staking_rewards),
        ("treasury".into(), amounts.treasury),
        ("issuance_reserve".into(), amounts.issuance_reserve),
    ])
}
impl TimingSupply {
    fn from_state(state: &TimingState) -> Self {
        let strings = |p| {
            timed_amounts(p)
                .into_iter()
                .map(|(k, v)| (k, v.to_string()))
                .collect()
        };
        Self {
            mode: "adaptive_epoch_v1",
            profile: state.config.profile.clone(),
            last_height: state.last_height,
            active_epoch: state.active_epoch,
            epoch_blocks: state.config.epoch_blocks,
            epoch_budget_udrt: state.epoch_budget_udrt.to_string(),
            issued_in_epoch: strings(&state.issued_in_epoch),
            total_issued: strings(&state.total_issued),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RewardLiabilitiesResponse {
    pub unpaid: String,
    pub rounding_reserve: String,
    pub inactive_reserve: String,
    pub total_claimed: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeSupply {
    pub drt: DrtSupply,
    pub dgt: DgtSupply,
    // Reward claims remain within DRT pool custody; these are query metadata.
    pub staking_reward_index: u128,
    pub pending_staking_emission: u128,
    pub reward_rate_bps: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DgtSupply {
    pub height: u64,
    pub issued: u128,
    pub liquid: u128,
    pub staked: u128,
    pub pending_bonded: u128,
    pub unbonding: u128,
    pub penalty_reserve: u128,
    pub governance_escrow: Option<u128>,
}
impl DgtSupply {
    pub fn response(&self) -> DgtSupplyResponse {
        DgtSupplyResponse {
            accounting_version: if self.governance_escrow.is_some() {
                2
            } else {
                1
            },
            denomination: "udgt",
            height: self.height,
            issued: self.issued.to_string(),
            liquid: self.liquid.to_string(),
            staked: self.staked.to_string(),
            pending_bonded: self.pending_bonded.to_string(),
            unbonding: self.unbonding.to_string(),
            penalty_reserve: self.penalty_reserve.to_string(),
            governance_escrow: self.governance_escrow.map(|amount| amount.to_string()),
            cap: DGT_MAX_SUPPLY.to_string(),
        }
    }
    /// Floor of the stake share in basis points. Zero issuance has zero stake.
    pub fn stake_basis_points(&self) -> Result<u128> {
        ensure!(
            self.issued <= DGT_MAX_SUPPLY && self.staked <= self.issued,
            "Invalid DGT staking ratio inputs"
        );
        if self.issued == 0 {
            return Ok(0);
        }
        self.staked
            .checked_mul(10_000)
            .map(|n| n / self.issued)
            .context("DGT ratio exceeds u128")
    }
}
#[derive(Debug, Serialize)]
pub struct DgtSupplyResponse {
    pub accounting_version: u32,
    pub denomination: &'static str,
    pub height: u64,
    pub issued: String,
    pub liquid: String,
    pub staked: String,
    pub pending_bonded: String,
    pub unbonding: String,
    #[serde(skip_serializing_if = "decimal_zero")]
    pub penalty_reserve: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub governance_escrow: Option<String>,
    pub cap: String,
}

fn decimal_zero(value: &str) -> bool {
    value == "0"
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrtSupply {
    pub height: u64,
    pub genesis: u128,
    pub emitted: u128,
    pub total: u128,
    pub liquid: u128,
    pub withheld_fees: u128,
    pub pools: BTreeMap<String, u128>,
    pub issuance_timing: Option<TimingSupply>,
    pub staking_reward_liabilities: Option<RewardLiabilitiesResponse>,
}
impl DrtSupply {
    /// Decimal strings preserve exact amounts in JSON clients.
    pub fn response(&self) -> SupplyResponse {
        SupplyResponse {
            accounting_version: if self.issuance_timing.is_some() { 2 } else { 1 },
            denomination: "udrt",
            height: self.height,
            genesis: self.genesis.to_string(),
            emitted: self.emitted.to_string(),
            burned: "0",
            total: self.total.to_string(),
            liquid: self.liquid.to_string(),
            withheld_fees: self.withheld_fees.to_string(),
            pools: self
                .pools
                .iter()
                .map(|(k, v)| (k.clone(), v.to_string()))
                .collect(),
            issuance_timing: self.issuance_timing.clone(),
            staking_reward_liabilities: self.staking_reward_liabilities.clone(),
        }
    }
}
#[derive(Debug, Serialize)]
pub struct SupplyResponse {
    pub accounting_version: u32,
    pub denomination: &'static str,
    pub height: u64,
    pub genesis: String,
    pub emitted: String,
    pub burned: &'static str,
    pub total: String,
    pub liquid: String,
    pub withheld_fees: String,
    pub pools: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issuance_timing: Option<TimingSupply>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub staking_reward_liabilities: Option<RewardLiabilitiesResponse>,
}
fn amount(raw: &[u8]) -> Result<u128> {
    ensure!(raw.len() == 16, "Invalid native amount encoding");
    Ok(bincode::deserialize(raw)?)
}
fn validate_timed_pools(
    issued: &PoolAmounts,
    emitted: u128,
    staking_claimed: u128,
    pools: &BTreeMap<String, u128>,
) -> Result<()> {
    let expected = timed_amounts(issued);
    ensure!(
        pools.len() == TIMED_POOLS.len() && pools.keys().all(|name| expected.contains_key(name)),
        "Timed issuance has unsupported custody pools"
    );
    let issued_total = expected
        .values()
        .try_fold(0u128, |sum, v| sum.checked_add(*v))
        .context("Timed cumulative issuance exceeds u128")?;
    ensure!(
        issued_total == emitted,
        "Timed issuance differs from emission counter"
    );
    for (name, issued_amount) in expected {
        let held = pools[&name];
        if name == "staking_rewards" {
            ensure!(
                held.checked_add(staking_claimed) == Some(issued_amount),
                "Staking custody plus claims differs from cumulative staking issuance"
            );
        } else {
            ensure!(
                held == issued_amount,
                "Unpaid timed pool differs from cumulative issuance: {name}"
            );
        }
    }
    Ok(())
}
pub(crate) fn genesis_amount(storage: &Storage) -> Result<u128> {
    amount(
        &storage
            .db
            .get(DRT_GENESIS_KEY)?
            .context("Missing DRT genesis counter; migration required")?,
    )
}
fn relevant(key: &[u8]) -> bool {
    key.starts_with(b"supply:")
        || key.starts_with(b"staking:")
        || key.starts_with(b"rewards:")
        || key.starts_with(b"lifecycle:")
        || key.starts_with(b"penalty:")
        || key.starts_with(b"issuance:")
        || key.starts_with(b"adaptive:")
        || key.starts_with(b"governance:")
        || key.starts_with(b"gov:")
        || key.starts_with(b"acct:balances:")
        || key.starts_with(b"emission:pool:")
        || key.starts_with(b"emission:event:")
        || [
            DRT_GENESIS_KEY.as_bytes(),
            EMITTED.as_bytes(),
            FEE_KEY.as_bytes(),
            b"genesis:monetary:v1",
            b"emission:last_height",
            b"meta:chain_id",
        ]
        .contains(&key)
}
/// Caller holds the storage execution lock, or owns exclusive startup access.
/// Overlay values replace stored values. This planner never writes storage.
pub(crate) fn validate_native(storage: &Storage, overlay: &Writes) -> Result<NativeSupply> {
    let snapshot = storage.db.snapshot();
    let mut values = Writes::new();
    for item in snapshot.iterator(IteratorMode::Start) {
        let (key, value) = item?;
        if relevant(&key) {
            values.insert(key.to_vec(), value.to_vec());
        }
    }
    for (key, value) in overlay {
        if relevant(key) {
            values.insert(key.clone(), value.clone());
        }
    }
    let marker = values
        .get(b"genesis:monetary:v1".as_slice())
        .context("Missing monetary genesis; migration required")?;
    ensure!(
        marker.len() == 33 && marker[0] == 1,
        "Invalid monetary genesis marker"
    );
    let read = |key: &str, required: bool| -> Result<u128> {
        match values.get(key.as_bytes()) {
            Some(raw) => amount(raw).with_context(|| format!("Invalid supply record: {key}")),
            None => {
                ensure!(!required, "Missing supply record: {key}");
                Ok(0)
            }
        }
    };
    let height = match values.get(b"emission:last_height".as_slice()) {
        Some(raw) => {
            u64::from_be_bytes(raw.as_slice().try_into().context("Invalid supply height")?)
        }
        None => 0,
    };
    let governance = values
        .get(GOVERNANCE_STATE_KEY.as_bytes())
        .map(|raw| GovernanceState::decode(raw))
        .transpose()?;
    let governance_escrow = governance
        .as_ref()
        .map(|state| {
            ensure!(
                state.last_height() == height
                    && values.get(b"meta:chain_id".as_slice()).map(Vec::as_slice)
                        == Some(state.chain_id().as_bytes())
                    && marker[1..] == state.genesis_digest(),
                "Governance supply state height, chain, or genesis differs"
            );
            state.held_total_udgt()
        })
        .transpose()?;
    let genesis = read(DRT_GENESIS_KEY, true)?;
    let emitted = read(EMITTED, height > 0)?;
    ensure!(
        height > 0 || emitted == 0,
        "Emission exists before the first block"
    );
    let total = genesis
        .checked_add(emitted)
        .context("Total DRT supply exceeds u128")?;
    let withheld_fees = read(FEE_KEY, false)?;
    let issued = read(DGT_MINTED_KEY, true)?;
    ensure!(
        issued <= DGT_MAX_SUPPLY,
        "Issued DGT exceeds the supply cap"
    );
    let stored_stake = read(TOTAL_STAKE_KEY, true)?;
    let staking_reward_index = read("staking:reward_index", false)?;
    let pending_staking_emission = read("staking:pending_emission", false)?;
    let reward_residual = read("staking:reward_residual", false)?;
    let reward_state = values
        .get(REWARD_STATE_KEY.as_bytes())
        .map(|raw| RewardState::decode(raw))
        .transpose()?;
    let validators = values
        .get(VALIDATOR_STATE_KEY.as_bytes())
        .map(|raw| LifecycleState::decode(raw))
        .transpose()?;
    ensure!(
        values
            .keys()
            .all(|key| !key.starts_with(b"lifecycle:") || key == VALIDATOR_STATE_KEY.as_bytes()),
        "Unsupported validator lifecycle record"
    );
    let pending_by_owner = if let Some(validators) = &validators {
        let rewards = reward_state
            .as_ref()
            .context("Validator lifecycle requires reward state")?;
        validators.validate_rewards(rewards)?;
        ensure!(
            validators.last_height == height,
            "Validator lifecycle height differs from supply"
        );
        ensure!(
            validators.unbonding_by_owner()? == rewards.unbonding,
            "Validator unbonding entries differ from owner custody"
        );
        validators.pending_bonds_by_owner()?
    } else {
        BTreeMap::new()
    };
    let penalties = values
        .get(PENALTY_STATE_KEY.as_bytes())
        .map(|raw| PenaltyState::decode(raw))
        .transpose()?;
    ensure!(
        values
            .keys()
            .all(|key| !key.starts_with(b"penalty:") || key == PENALTY_STATE_KEY.as_bytes()),
        "Unsupported penalty custody record"
    );
    if let Some(penalties) = &penalties {
        let validators = validators
            .as_ref()
            .context("Penalty custody requires validator lifecycle")?;
        penalties.validate(validators)?;
        ensure!(
            reward_state
                .as_ref()
                .context("Penalty custody requires reward state")?
                .locks
                .is_empty(),
            "Penalty custody does not support vesting locks"
        );
    }
    let penalty_reserve = penalties
        .as_ref()
        .map(PenaltyState::penalty_reserve)
        .transpose()?
        .unwrap_or(0);
    let pending_bonded = pending_by_owner.values().try_fold(0u128, |sum, amount| {
        sum.checked_add(*amount)
            .context("Pending bond custody exceeds u128")
    })?;
    for owner in pending_by_owner.keys() {
        ensure!(
            values.contains_key(format!("acct:balances:{owner}").as_bytes()),
            "Pending bond owner has no funding account record"
        );
    }
    let timing = if values.contains_key(TIMING_STATE_KEY.as_bytes()) {
        ensure!(
            reward_state.is_some(),
            "Timed issuance requires reward-v2 state"
        );
        Some(issuance_timing::verify_overlay(&values)?)
    } else {
        ensure!(
            !values
                .keys()
                .any(|key| key.starts_with(b"issuance:") || key.starts_with(b"adaptive:")),
            "Orphan issuance or adaptive-controller record"
        );
        None
    };
    let allowed_pools = if timing.is_some() {
        &TIMED_POOLS
    } else {
        &POOLS
    };
    if let Some(rewards) = &reward_state {
        ensure!(
            values.get(b"meta:chain_id".as_slice()).map(Vec::as_slice)
                == Some(rewards.config.chain_id.as_bytes())
                && rewards.config.genesis_digest == hex::encode(&marker[1..]),
            "Reward configuration differs from local chain or genesis"
        );
        ensure!(
            rewards.last_height == height,
            "Reward interval differs from emission height"
        );
        ensure!(
            staking_reward_index == 0 && pending_staking_emission == 0 && reward_residual == 0,
            "Legacy reward state requires explicit migration"
        );
    }
    let reward_rate_bps = match values.get(b"staking:reward_rate_bps".as_slice()) {
        Some(raw) => {
            ensure!(raw.len() == 8, "Invalid staking reward-rate encoding");
            bincode::deserialize::<u64>(raw)?
        }
        None => 500,
    };
    let mut dgt_liquid = 0u128;
    let mut staked = 0u128;
    let mut bonded_owners = BTreeMap::new();
    let mut liquid = 0u128;
    let mut pools = BTreeMap::new();
    for (key, raw) in &values {
        ensure!(
            (!key.starts_with(b"governance:") && !key.starts_with(b"gov:"))
                || key == GOVERNANCE_STATE_KEY.as_bytes(),
            "Unsupported governance supply record"
        );
        ensure!(
            !key.starts_with(b"rewards:") || key == REWARD_STATE_KEY.as_bytes(),
            "Unsupported reward record; accounting migration required"
        );
        ensure!(
            !key.starts_with(b"supply:")
                || key == DRT_GENESIS_KEY.as_bytes()
                || key == DGT_MINTED_KEY.as_bytes(),
            "Unsupported native supply record; accounting migration required"
        );
        if key.starts_with(b"acct:balances:") {
            let balances: BTreeMap<String, u128> =
                bincode::deserialize(raw).context("Invalid account balance map")?;
            ensure!(
                bincode::serialize(&balances)? == *raw,
                "Noncanonical account balance map"
            );
            dgt_liquid = dgt_liquid
                .checked_add(balances.get("udgt").copied().unwrap_or(0))
                .context("Liquid DGT supply exceeds u128")?;
            liquid = liquid
                .checked_add(balances.get("udrt").copied().unwrap_or(0))
                .context("Liquid DRT supply exceeds u128")?;
        }
        if let Some(address) = key.strip_prefix(b"staking:delegator:") {
            let address =
                std::str::from_utf8(address).context("Invalid delegator address encoding")?;
            ensure!(
                !address.is_empty()
                    && !address.chars().any(|c| c.is_whitespace() || c.is_control()),
                "Invalid delegator address"
            );
            let record: DelegatorRewardRecord =
                bincode::deserialize(raw).context("Invalid delegator record")?;
            ensure!(
                bincode::serialize(&record)? == *raw,
                "Noncanonical delegator record"
            );
            ensure!(
                values.contains_key(format!("acct:balances:{address}").as_bytes()),
                "Delegator has no funding account record"
            );
            staked = staked
                .checked_add(record.stake_amount)
                .context("DGT stake sum exceeds u128")?;
            if reward_state.is_some() {
                ensure!(
                    record.last_reward_index == 0 && record.accrued_rewards == 0,
                    "Legacy delegator rewards require explicit migration"
                );
                if record.stake_amount > 0 {
                    bonded_owners.insert(address.to_string(), record.stake_amount);
                }
            }
        } else if key.starts_with(b"staking:") {
            ensure!(
                [
                    TOTAL_STAKE_KEY.as_bytes(),
                    b"staking:reward_index",
                    b"staking:pending_emission",
                    b"staking:reward_residual",
                    b"staking:reward_rate_bps"
                ]
                .contains(&key.as_slice()),
                "Unsupported staking record; accounting migration required"
            );
        }
        if let Some(name) = key.strip_prefix(b"emission:pool:") {
            let name = std::str::from_utf8(name)?;
            ensure!(
                allowed_pools.contains(&name),
                "Unknown DRT emission pool for selected issuance mode"
            );
            pools.insert(name.to_string(), amount(raw)?);
        }
    }
    let pool_total = pools
        .values()
        .try_fold(0u128, |sum, v| sum.checked_add(*v))
        .context("DRT pool supply exceeds u128")?;
    let held = liquid
        .checked_add(withheld_fees)
        .and_then(|v| v.checked_add(pool_total))
        .context("DRT custody sum exceeds u128")?;
    ensure!(
        held == total,
        "DRT conservation failed: custody differs from genesis plus emission"
    );
    for name in allowed_pools {
        pools.entry((*name).into()).or_insert(0);
    }
    if let Some(timing) = &timing {
        validate_timed_pools(
            &timing.total_issued,
            emitted,
            reward_state
                .as_ref()
                .context("Missing timed reward state")?
                .total_claimed,
            &pools,
        )?;
    }
    let unbonding = if let Some(rewards) = &reward_state {
        let expected_bonded: BTreeMap<String, u128> = rewards
            .positions
            .keys()
            .map(|owner| Ok((owner.clone(), rewards.owner_bonded(owner)?)))
            .collect::<Result<_>>()?;
        ensure!(
            bonded_owners == expected_bonded,
            "Reward positions differ from funded delegator bonds"
        );
        for owner in rewards
            .positions
            .keys()
            .chain(rewards.unbonding.keys())
            .chain(rewards.unpaid.keys())
            .chain(rewards.locks.keys())
        {
            ensure!(
                values.contains_key(format!("acct:balances:{owner}").as_bytes()),
                "Reward owner has no funding account record"
            );
        }
        if !rewards.locks.is_empty() {
            let timestamp = if height == 0 {
                0
            } else {
                type StoredEmissionEvent =
                    (u64, u64, u128, BTreeMap<String, u128>, Option<u128>, u128);
                let raw = values
                    .get(format!("emission:event:{height}").as_bytes())
                    .context("Missing emission event for vesting timestamp")?;
                let event: StoredEmissionEvent = bincode::deserialize(raw)
                    .context("Invalid emission event for vesting timestamp")?;
                ensure!(
                    event.0 == height && bincode::serialize(&event)? == *raw,
                    "Vesting timestamp event differs from accounting height"
                );
                event.1
            };
            for owner in rewards.locks.keys() {
                let balances: BTreeMap<String, u128> =
                    bincode::deserialize(&values[format!("acct:balances:{owner}").as_bytes()])?;
                rewards
                    .liquid_spendable(
                        owner,
                        balances.get("udgt").copied().unwrap_or(0),
                        rewards
                            .owner_bonded(owner)?
                            .checked_add(pending_by_owner.get(owner).copied().unwrap_or(0))
                            .context("Owner bond custody exceeds u128")?,
                        rewards.unbonding.get(owner).copied().unwrap_or(0),
                        timestamp,
                    )
                    .context("Vesting lock has insufficient owner custody")?;
            }
        }
        let liability = rewards
            .total_unpaid()?
            .checked_add(rewards.rounding_reserve)
            .and_then(|v| v.checked_add(rewards.inactive_reserve))
            .context("Staking reward liability exceeds u128")?;
        ensure!(
            pools["staking_rewards"] == liability,
            "Staking pool differs from unpaid rewards and reserves"
        );
        rewards.total_unbonding()?
    } else {
        0
    };
    let unbonding = if let Some(penalties) = &penalties {
        let validators = validators
            .as_ref()
            .context("Penalty custody requires validator lifecycle")?;
        let net = penalties.net_unbonding_total(validators)?;
        let deductions = penalties.deducted_total()?;
        let releases = penalties.released_total()?;
        ensure!(
            net.checked_add(deductions)
                .and_then(|value| value.checked_add(releases))
                == Some(unbonding),
            "Gross and net unbond custody differ"
        );
        net
    } else {
        unbonding
    };
    ensure!(
        staked == stored_stake,
        "DGT stake total differs from delegator records"
    );
    let dgt_held = dgt_liquid
        .checked_add(pending_bonded)
        .and_then(|v| v.checked_add(staked))
        .and_then(|v| v.checked_add(unbonding))
        .and_then(|v| v.checked_add(penalty_reserve))
        .and_then(|v| v.checked_add(governance_escrow.unwrap_or(0)))
        .context("DGT custody sum exceeds u128")?;
    ensure!(
        dgt_held == issued,
        "DGT conservation failed: custody differs from issued supply"
    );
    Ok(NativeSupply {
        staking_reward_index,
        pending_staking_emission,
        reward_rate_bps,
        drt: DrtSupply {
            height,
            genesis,
            emitted,
            total,
            liquid,
            withheld_fees,
            pools,
            issuance_timing: timing.as_ref().map(TimingSupply::from_state),
            staking_reward_liabilities: reward_state
                .as_ref()
                .map(|r| -> Result<_> {
                    Ok(RewardLiabilitiesResponse {
                        unpaid: r.total_unpaid()?.to_string(),
                        rounding_reserve: r.rounding_reserve.to_string(),
                        inactive_reserve: r.inactive_reserve.to_string(),
                        total_claimed: r.total_claimed.to_string(),
                    })
                })
                .transpose()?,
        },
        dgt: DgtSupply {
            height,
            issued,
            liquid: dgt_liquid,
            staked,
            pending_bonded,
            unbonding,
            penalty_reserve,
            governance_escrow,
        },
    })
}
/// Return one committed accounting view. Invalid or unsupported state returns an error.
pub fn inspect_native(storage: &Storage) -> Result<NativeSupply> {
    let _guard = storage.lock_execution()?;
    crate::block_settlement::verify_recovery(storage)?;
    validate_native(storage, &Writes::new())
}
/// Return a committed timing view only when this store contains timing records.
/// Partial or corrupt timing state returns an error and cannot select legacy defaults.
pub fn inspect_timed_native(storage: &Storage) -> Result<Option<NativeSupply>> {
    let _guard = storage.lock_execution()?;
    let mut timed = false;
    for prefix in [b"issuance:".as_slice(), b"adaptive:".as_slice()] {
        for item in storage.db.prefix_iterator(prefix) {
            let (key, _) = item?;
            if !key.starts_with(prefix) {
                break;
            }
            timed = true;
        }
    }
    for name in ["validator_rewards", "treasury", "issuance_reserve"] {
        timed |= storage.db.get(format!("emission:pool:{name}"))?.is_some();
    }
    if !timed {
        return Ok(None);
    }
    crate::block_settlement::verify_recovery(storage)?;
    let supply = validate_native(storage, &Writes::new())?;
    ensure!(
        supply.drt.issuance_timing.is_some(),
        "Missing issuance timing state"
    );
    Ok(Some(supply))
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewardOwnerSupply {
    pub bonded: u128,
    pub pending_bonded: u128,
    pub unbonding: u128,
    pub unpaid: u128,
    pub liquid: u128,
}
/// Read one committed reward-v2 owner view. None means no reward-v2 records exist.
/// Malformed, unsupported or inconsistent reward state always returns an error.
pub fn inspect_reward_owner(storage: &Storage, owner: &str) -> Result<Option<RewardOwnerSupply>> {
    let _guard = storage.lock_execution()?;
    let mut has_rewards = false;
    for item in storage.db.prefix_iterator(b"rewards:") {
        let (key, _) = item?;
        if !key.starts_with(b"rewards:") {
            break;
        }
        has_rewards = true;
        ensure!(
            key.as_ref() == REWARD_STATE_KEY.as_bytes(),
            "Unsupported reward record"
        );
    }
    if !has_rewards {
        return Ok(None);
    }
    crate::block_settlement::verify_recovery(storage)?;
    validate_native(storage, &Writes::new())?;
    let rewards = RewardState::decode(
        &storage
            .db
            .get(REWARD_STATE_KEY)?
            .context("Missing reward-v2 state")?,
    )?;
    let balances: BTreeMap<String, u128> = storage
        .db
        .get(format!("acct:balances:{owner}"))?
        .map(|raw| bincode::deserialize(&raw))
        .transpose()?
        .unwrap_or_default();
    let pending_bonded = storage
        .db
        .get(VALIDATOR_STATE_KEY)?
        .map(|raw| LifecycleState::decode(&raw)?.pending_bond_by_owner(owner))
        .transpose()?
        .unwrap_or(0);
    let unbonding = if let Some(raw) = storage.db.get(PENALTY_STATE_KEY)? {
        let penalties = PenaltyState::decode(&raw)?;
        let validators = LifecycleState::decode(
            &storage
                .db
                .get(VALIDATOR_STATE_KEY)?
                .context("Penalty custody requires validator lifecycle")?,
        )?;
        penalties
            .net_unbonding_by_owner(&validators)?
            .get(owner)
            .copied()
            .unwrap_or(0)
    } else {
        rewards.unbonding.get(owner).copied().unwrap_or(0)
    };
    Ok(Some(RewardOwnerSupply {
        pending_bonded,
        bonded: rewards.owner_bonded(owner)?,
        unbonding,
        unpaid: rewards.unpaid.get(owner).copied().unwrap_or(0),
        liquid: balances.get("udgt").copied().unwrap_or(0),
    }))
}
pub fn inspect(storage: &Storage) -> Result<DrtSupply> {
    inspect_native(storage).map(|s| s.drt)
}
pub(crate) fn validate(storage: &Storage, overlay: &Writes) -> Result<DrtSupply> {
    validate_native(storage, overlay).map(|s| s.drt)
}

#[cfg(test)]
mod reward_v2_tests {
    use super::*;
    use crate::runtime::governance_deposit_stage::{DepositRules, DepositStage};
    use crate::runtime::governance_escrow::DepositEscrow;
    use crate::runtime::governance_state::ProposalRecord;
    use crate::runtime::reward_runtime::{RewardConfig, ValidatorStatus};

    fn fixture() -> (tempfile::TempDir, Storage, RewardState) {
        let dir = tempfile::tempdir().unwrap();
        let mut storage = Storage::open(dir.path().join("db")).unwrap();
        crate::genesis::initialize(
            &mut storage,
            "supply-test",
            Some(
                br#"{
            "chain_id":"supply-test",
            "accounts":[{"address":"alice","balances":{"udgt":"100"}},
                        {"address":"bob","balances":{"udgt":"20"}}],
            "staking":{"delegations":[{"delegator":"alice","amount_udgt":"40"},
                                          {"delegator":"bob","amount_udgt":"20"}]}
        }"#,
            ),
        )
        .unwrap();
        let marker = storage.db.get("genesis:monetary:v1").unwrap().unwrap();
        let rewards = RewardState::new(
            RewardConfig {
                version: 2,
                activation_height: 1,
                decimals: 6,
                profile: "development".into(),
                chain_id: "supply-test".into(),
                genesis_digest: hex::encode(&marker[1..]),
                max_validators: 4,
                max_positions: 8,
            },
            BTreeMap::from([(
                "validator".into(),
                ValidatorStatus {
                    active: true,
                    jailed: false,
                },
            )]),
            BTreeMap::from([
                ("alice".into(), BTreeMap::from([("validator".into(), 40)])),
                ("bob".into(), BTreeMap::from([("validator".into(), 20)])),
            ]),
            BTreeMap::new(),
        )
        .unwrap();
        storage
            .db
            .put(REWARD_STATE_KEY, rewards.encode().unwrap())
            .unwrap();
        (dir, storage, rewards)
    }
    fn put_amount(writes: &mut Writes, key: &str, value: u128) {
        writes.insert(key.as_bytes().to_vec(), bincode::serialize(&value).unwrap());
    }
    #[test]
    fn governance_supply_state_is_bound_to_chain_and_height() {
        let (_dir, storage, _) = fixture();
        let marker = storage.db.get("genesis:monetary:v1").unwrap().unwrap();
        let mut digest = [0u8; 32];
        digest.copy_from_slice(&marker[1..]);
        let state = GovernanceState::new(1, "supply-test".into(), digest, 0).unwrap();
        let mut writes = Writes::new();
        writes.insert(
            GOVERNANCE_STATE_KEY.as_bytes().to_vec(),
            state.encode().unwrap(),
        );
        let supply = validate_native(&storage, &writes).unwrap();
        assert_eq!(supply.dgt.governance_escrow, Some(0));
        assert_eq!(supply.dgt.response().accounting_version, 2);

        let wrong_chain = GovernanceState::new(1, "other-chain".into(), digest, 0).unwrap();
        writes.insert(
            GOVERNANCE_STATE_KEY.as_bytes().to_vec(),
            wrong_chain.encode().unwrap(),
        );
        assert!(validate_native(&storage, &writes).is_err());

        let wrong_height = GovernanceState::new(1, "supply-test".into(), digest, 1).unwrap();
        writes.insert(
            GOVERNANCE_STATE_KEY.as_bytes().to_vec(),
            wrong_height.encode().unwrap(),
        );
        assert!(validate_native(&storage, &writes).is_err());

        let wrong_genesis = GovernanceState::new(1, "supply-test".into(), [7; 32], 0).unwrap();
        writes.insert(
            GOVERNANCE_STATE_KEY.as_bytes().to_vec(),
            wrong_genesis.encode().unwrap(),
        );
        assert!(validate_native(&storage, &writes).is_err());

        writes.insert(
            GOVERNANCE_STATE_KEY.as_bytes().to_vec(),
            state.encode().unwrap(),
        );
        for key in ["governance:v1:unknown", "gov:legacy"] {
            writes.insert(key.as_bytes().to_vec(), vec![1]);
            assert!(validate_native(&storage, &writes).is_err());
            writes.remove(key.as_bytes());
        }
    }
    #[test]
    fn governance_deposit_remains_in_dgt_custody() {
        let dir = tempfile::tempdir().unwrap();
        let mut storage = Storage::open(dir.path().join("db")).unwrap();
        crate::genesis::initialize(
            &mut storage,
            "supply-test",
            Some(
                br#"{
            "chain_id":"supply-test",
            "accounts":[{"address":"alice","balances":{"udgt":"100"}}],
            "staking":{"delegations":[{"delegator":"alice","amount_udgt":"40"}]}
        }"#,
            ),
        )
        .unwrap();
        let marker = storage.db.get("genesis:monetary:v1").unwrap().unwrap();
        let mut digest = [0u8; 32];
        digest.copy_from_slice(&marker[1..]);
        let stage = DepositStage::new(
            DepositRules {
                deposit_period_blocks: 3,
                minimum_deposit_udgt: 10,
                max_action_bytes: 8,
            },
            1,
            1,
            vec![1],
            dytallix_protocol_types::ordinary_v3::governance_action_digest(1, &[1]).unwrap(),
            1,
        )
        .unwrap();
        let escrow = DepositEscrow::new(1, 4).unwrap();
        let state = GovernanceState::new(1, "supply-test".into(), digest, 0).unwrap();
        let state = state
            .plan_commit(
                1,
                BTreeMap::from([(1, ProposalRecord::new(stage.clone(), escrow.clone(), None))]),
            )
            .unwrap();
        let (escrow, balance) = escrow.plan_deposit([1; 32], 5, 60, 60).unwrap();
        let state = state
            .plan_commit(
                2,
                BTreeMap::from([(1, ProposalRecord::new(stage, escrow, None))]),
            )
            .unwrap();
        let mut writes = Writes::new();
        writes.insert(
            GOVERNANCE_STATE_KEY.as_bytes().to_vec(),
            state.encode().unwrap(),
        );
        writes.insert(
            b"emission:last_height".to_vec(),
            2u64.to_be_bytes().to_vec(),
        );
        put_amount(&mut writes, EMITTED, 0);
        writes.insert(
            b"acct:balances:alice".to_vec(),
            bincode::serialize(&BTreeMap::from([("udgt".to_string(), balance)])).unwrap(),
        );
        let supply = validate_native(&storage, &writes).unwrap();
        assert_eq!(supply.dgt.governance_escrow, Some(5));
        assert_eq!(supply.dgt.liquid, 55);
        writes.insert(
            b"acct:balances:alice".to_vec(),
            bincode::serialize(&BTreeMap::from([("udgt".to_string(), 60u128)])).unwrap(),
        );
        assert!(validate_native(&storage, &writes).is_err());
    }
    fn interval(writes: &mut Writes, rewards: &RewardState, amount: u128) {
        writes.insert(
            REWARD_STATE_KEY.as_bytes().to_vec(),
            rewards.encode().unwrap(),
        );
        writes.insert(
            b"emission:last_height".to_vec(),
            1u64.to_be_bytes().to_vec(),
        );
        put_amount(writes, EMITTED, amount);
        put_amount(writes, "emission:pool:staking_rewards", amount);
    }
    #[test]
    fn reward_v2_reserves_stay_inside_staking_pool() {
        let (_dir, storage, mut rewards) = fixture();
        rewards.stage_interval(1, "parent", 1).unwrap();
        assert_eq!(rewards.rounding_reserve, 1);
        let mut writes = Writes::new();
        interval(&mut writes, &rewards, 1);
        let supply = validate_native(&storage, &writes).unwrap();
        assert_eq!(supply.drt.total, 1);
        assert_eq!(supply.drt.pools["staking_rewards"], 1);
        assert_eq!(supply.drt.liquid, 0);
        // The same issuance cannot fund both the reserve and an unrelated pool.
        put_amount(&mut writes, "emission:pool:block_rewards", 1);
        assert!(validate_native(&storage, &writes).is_err());
        // Moving custody out of the staking pool also leaves its liability unfunded.
        put_amount(&mut writes, "emission:pool:staking_rewards", 0);
        assert!(validate_native(&storage, &writes)
            .unwrap_err()
            .to_string()
            .contains("Staking pool differs"));
    }
    #[test]
    fn reward_v2_unbonding_is_custody_but_not_bonded_stake() {
        let (_dir, storage, mut rewards) = fixture();
        rewards.begin_unbond("alice", "validator", 10).unwrap();
        let mut writes = Writes::new();
        writes.insert(
            REWARD_STATE_KEY.as_bytes().to_vec(),
            rewards.encode().unwrap(),
        );
        put_amount(&mut writes, TOTAL_STAKE_KEY, 50);
        writes.insert(
            b"staking:delegator:alice".to_vec(),
            bincode::serialize(&DelegatorRewardRecord {
                last_reward_index: 0,
                accrued_rewards: 0,
                stake_amount: 30,
            })
            .unwrap(),
        );
        let supply = validate_native(&storage, &writes).unwrap();
        assert_eq!(
            (supply.dgt.liquid, supply.dgt.staked, supply.dgt.unbonding),
            (60, 50, 10)
        );
        assert_eq!(supply.dgt.issued, 120);
        assert_eq!(supply.dgt.response().unbonding, "10");
        put_amount(&mut writes, TOTAL_STAKE_KEY, 60);
        assert!(validate_native(&storage, &writes).is_err());
    }
    #[test]
    fn reward_v2_rejects_legacy_rewards_and_unsupported_records() {
        let (_dir, storage, _) = fixture();
        for key in [
            "staking:reward_index",
            "staking:pending_emission",
            "staking:reward_residual",
            "rewards:other",
        ] {
            let mut writes = Writes::new();
            put_amount(&mut writes, key, 1);
            assert!(
                validate_native(&storage, &writes).is_err(),
                "accepted {key}"
            );
        }
        let mut writes = Writes::new();
        writes.insert(
            b"staking:delegator:alice".to_vec(),
            bincode::serialize(&DelegatorRewardRecord {
                last_reward_index: 0,
                accrued_rewards: 1,
                stake_amount: 40,
            })
            .unwrap(),
        );
        assert!(validate_native(&storage, &writes).is_err());
    }
    #[test]
    fn reward_v2_rejects_unfunded_positions_and_wrong_identity() {
        let (_dir, storage, mut rewards) = fixture();
        rewards
            .positions
            .insert("unfunded".into(), BTreeMap::from([("validator".into(), 1)]));
        let mut writes = Writes::new();
        writes.insert(
            REWARD_STATE_KEY.as_bytes().to_vec(),
            rewards.encode().unwrap(),
        );
        assert!(validate_native(&storage, &writes).is_err());
        rewards.positions.remove("unfunded");
        rewards.config.chain_id = "other-chain".into();
        writes.insert(
            REWARD_STATE_KEY.as_bytes().to_vec(),
            rewards.encode().unwrap(),
        );
        assert!(validate_native(&storage, &writes).is_err());
        rewards.config.chain_id = "supply-test".into();
        rewards.config.genesis_digest = "11".repeat(32);
        writes.insert(
            REWARD_STATE_KEY.as_bytes().to_vec(),
            rewards.encode().unwrap(),
        );
        assert!(validate_native(&storage, &writes).is_err());
    }
    #[test]
    fn reward_v2_owner_read_uses_stored_bonded_and_unpaid_amounts() {
        let (_dir, storage, _) = fixture();
        assert_eq!(
            inspect_reward_owner(&storage, "alice").unwrap().unwrap(),
            RewardOwnerSupply {
                bonded: 40,
                pending_bonded: 0,
                unbonding: 0,
                unpaid: 0,
                liquid: 60,
            }
        );
        storage.db.put(REWARD_STATE_KEY, b"corrupt").unwrap();
        assert!(inspect_reward_owner(&storage, "alice").is_err());
    }
    #[test]
    fn reward_v2_rejects_sum_preserving_transfer_that_drains_locked_owner() {
        let (_dir, storage, mut rewards) = fixture();
        for permits_staking in [false, true] {
            rewards.locks.insert(
                "alice".into(),
                crate::runtime::reward_runtime::VestingLock {
                    total_amount: if permits_staking { 100 } else { 60 },
                    start_time: 0,
                    cliff_duration: 10,
                    vesting_duration: 20,
                    permits_staking,
                },
            );
            let mut writes = Writes::new();
            writes.insert(
                REWARD_STATE_KEY.as_bytes().to_vec(),
                rewards.encode().unwrap(),
            );
            validate_native(&storage, &writes).unwrap();
            writes.insert(
                b"acct:balances:alice".to_vec(),
                bincode::serialize(&BTreeMap::from([("udgt".to_string(), 59u128)])).unwrap(),
            );
            writes.insert(
                b"acct:balances:bob".to_vec(),
                bincode::serialize(&BTreeMap::from([("udgt".to_string(), 1u128)])).unwrap(),
            );
            assert!(validate_native(&storage, &writes)
                .unwrap_err()
                .to_string()
                .contains("Vesting lock has insufficient owner custody"));
        }
    }
}

#[cfg(test)]
mod issuance_timing_supply_tests {
    use super::*;

    #[test]
    fn zero_timed_issuance_has_four_empty_custody_buckets() {
        let issued = PoolAmounts {
            validator_rewards: 0,
            staking_rewards: 0,
            treasury: 0,
            issuance_reserve: 0,
        };
        validate_timed_pools(&issued, 0, 0, &timed_amounts(&issued)).unwrap();
    }
    #[test]
    fn mid_epoch_custody_preserves_claimed_and_unpaid_staking_supply() {
        let issued = PoolAmounts::epoch_split(103).prefix(10, 5).unwrap();
        let mut pools = timed_amounts(&issued);
        let claimed = 5;
        *pools.get_mut("staking_rewards").unwrap() -= claimed;
        validate_timed_pools(&issued, issued.total().unwrap(), claimed, &pools).unwrap();
        assert_eq!(
            pools.values().sum::<u128>() + claimed,
            issued.total().unwrap()
        );
        // Claimed rewards move to liquid custody and cannot also remain in the pool.
        *pools.get_mut("staking_rewards").unwrap() += claimed;
        assert!(validate_timed_pools(&issued, issued.total().unwrap(), claimed, &pools).is_err());
    }
    #[test]
    fn timed_custody_rejects_pool_reassignment_counter_tampering_and_legacy_names() {
        let issued = PoolAmounts::epoch_split(103);
        let total = issued.total().unwrap();
        let original = timed_amounts(&issued);
        let mut reassigned = original.clone();
        *reassigned.get_mut("validator_rewards").unwrap() -= 1;
        *reassigned.get_mut("treasury").unwrap() += 1;
        assert!(validate_timed_pools(&issued, total, 0, &reassigned).is_err());
        assert!(validate_timed_pools(&issued, total + 1, 0, &original).is_err());
        let mut renamed = original;
        let value = renamed.remove("validator_rewards").unwrap();
        renamed.insert("block_rewards".into(), value);
        assert!(validate_timed_pools(&issued, total, 0, &renamed).is_err());
    }
    #[test]
    fn orphan_timing_records_and_adaptive_pool_names_cannot_select_legacy_accounting() {
        let dir = tempfile::tempdir().unwrap();
        let mut storage = Storage::open(dir.path().join("db")).unwrap();
        crate::genesis::initialize(&mut storage, "supply-timing-test", None).unwrap();
        for key in [
            "issuance:unknown",
            "adaptive:v1:head",
            "emission:pool:validator_rewards",
        ] {
            let writes =
                BTreeMap::from([(key.as_bytes().to_vec(), bincode::serialize(&0u128).unwrap())]);
            assert!(
                validate_native(&storage, &writes).is_err(),
                "accepted {key}"
            );
        }
        storage.db.put("adaptive:v1:head", b"corrupt").unwrap();
        assert!(inspect_timed_native(&storage).is_err());
    }
}
