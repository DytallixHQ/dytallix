//! Pure issuance and reward planning for the selected block profile.
use crate::runtime::{
    emission::{EmissionEngine, EmissionSchedule},
    staking::{StakingModule, REWARD_SCALE},
};
use crate::storage::state::Storage;
use anyhow::{ensure, Context, Result};
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;

pub(crate) type Writes = BTreeMap<Vec<u8>, Vec<u8>>;
pub(crate) fn read<T: DeserializeOwned + Default>(storage: &Storage, key: &str) -> Result<T> {
    storage
        .db
        .get(key)?
        .map(|v| bincode::deserialize(&v).context("Invalid lifecycle value"))
        .transpose()
        .map(|v| v.unwrap_or_default())
}
pub(crate) fn height(storage: &Storage, key: &str) -> Result<u64> {
    storage
        .db
        .get(key)?
        .map(|v| {
            Ok(u64::from_be_bytes(
                v.as_slice().try_into().context("Invalid stored height")?,
            ))
        })
        .unwrap_or(Ok(0))
}
fn put<T: serde::Serialize>(writes: &mut Writes, key: &str, value: &T) -> Result<()> {
    writes.insert(key.as_bytes().to_vec(), bincode::serialize(value)?);
    Ok(())
}
// Exact floor(n * p / denominator), without an overflowing intermediate product.
fn ratio(n: u128, p: u128, denominator: u128) -> Result<u128> {
    (n / denominator)
        .checked_mul(p)
        .and_then(|a| {
            (n % denominator)
                .checked_mul(p)
                .and_then(|b| a.checked_add(b / denominator))
        })
        .context("Emission ratio exceeds u128")
}
pub(crate) fn validate_config(config: &crate::runtime::emission::EmissionConfig) -> Result<()> {
    let split = &config.emission_breakdown;
    ensure!(
        u16::from(split.block_rewards)
            + u16::from(split.staking_rewards)
            + u16::from(split.ai_module_incentives)
            + u16::from(split.bridge_operations)
            == 100,
        "Emission shares must sum to 100"
    );
    match &config.schedule {
        EmissionSchedule::Percentage {
            annual_inflation_rate,
        } => ensure!(
            *annual_inflation_rate <= 10000,
            "Development inflation rate exceeds 100 percent"
        ),
        EmissionSchedule::Phased { phases } => {
            for (i, p) in phases.iter().enumerate() {
                ensure!(
                    p.end_height.is_none_or(|end| end >= p.start_height),
                    "Invalid emission phase interval"
                );
                if i > 0 {
                    ensure!(
                        phases[i - 1]
                            .end_height
                            .is_some_and(|end| end < p.start_height),
                        "Overlapping or unordered emission phases"
                    );
                }
            }
        }
        EmissionSchedule::Static { .. } => {}
    }
    Ok(())
}

pub(crate) fn validate_initial_supply(emission: &EmissionEngine) -> Result<()> {
    ensure!(
        emission.config.initial_supply == crate::supply::genesis_amount(&emission.storage)?,
        "Emission initial supply differs from funded genesis; correct the configuration or migrate"
    );
    Ok(())
}

pub(crate) fn prepare(
    emission: &EmissionEngine,
    staking: Option<&StakingModule>,
    next: u64,
    timestamp: u64,
) -> Result<(Writes, u128, Option<StakingModule>)> {
    validate_config(&emission.config)?;
    validate_initial_supply(emission)?;
    let storage = &emission.storage;
    ensure!(
        height(storage, "emission:last_height")?.checked_add(1) == Some(next),
        "Emission height differs from block parent; recovery required"
    );
    let split = &emission.config.emission_breakdown;
    let circulating: u128 = read(storage, "emission:circulating_supply")?;
    let total = match &emission.config.schedule {
        EmissionSchedule::Static { per_block } => *per_block,
        EmissionSchedule::Phased { phases } => phases
            .iter()
            .find(|p| next >= p.start_height && p.end_height.is_none_or(|end| next <= end))
            .map_or(0, |p| p.per_block_amount),
        EmissionSchedule::Percentage {
            annual_inflation_rate,
        } => {
            let supply = emission
                .config
                .initial_supply
                .checked_add(circulating)
                .context("Emission supply exceeds u128")?;
            if supply == 0 {
                1_000_000
            } else {
                let annual = ratio(supply, u128::from(*annual_inflation_rate), 10000)?;
                if annual == 0 {
                    0
                } else {
                    (annual / 5_256_000).max(100)
                }
            }
        }
    };
    let circulating = circulating
        .checked_add(total)
        .context("Emission supply exceeds u128")?;
    emission
        .config
        .initial_supply
        .checked_add(circulating)
        .context("Total development supply exceeds u128")?;
    let mut pools = BTreeMap::new();
    let mut allocated = 0u128;
    for (name, percent) in [
        ("block_rewards", split.block_rewards),
        ("staking_rewards", split.staking_rewards),
        ("ai_module_incentives", split.ai_module_incentives),
    ] {
        let amount = ratio(total, u128::from(percent), 100)?;
        allocated = allocated
            .checked_add(amount)
            .context("Emission allocation overflow")?;
        pools.insert(name.to_string(), amount);
    }
    pools.insert(
        "bridge_operations".into(),
        total
            .checked_sub(allocated)
            .context("Emission overallocation")?,
    );
    let mut writes = Writes::new();
    for (name, amount) in &pools {
        let key = format!("emission:pool:{name}");
        let balance = read::<u128>(storage, &key)?
            .checked_add(*amount)
            .context("Emission pool exceeds u128")?;
        put(&mut writes, &key, &balance)?;
    }
    put(&mut writes, "emission:circulating_supply", &circulating)?;
    writes.insert(
        b"emission:last_height".to_vec(),
        next.to_be_bytes().to_vec(),
    );
    let mut next_staking = None;
    if let Some(staking) = staking {
        staking
            .ensure_legacy_fixture_mutation()
            .map_err(anyhow::Error::msg)?;
        let mut plan = staking.clone();
        plan.total_stake = read(storage, "staking:total_stake")?;
        plan.reward_index = read(storage, "staking:reward_index")?;
        plan.pending_staking_emission = read(storage, "staking:pending_emission")?;
        plan.reward_index_residual = read(storage, "staking:reward_residual")?;
        let amount = pools["staking_rewards"]
            .checked_add(plan.pending_staking_emission)
            .context("Pending staking emission exceeds u128")?;
        if plan.total_stake == 0 {
            plan.pending_staking_emission = amount;
        } else {
            let scaled = amount
                .checked_mul(REWARD_SCALE)
                .and_then(|v| v.checked_add(plan.reward_index_residual))
                .context("Scaled staking emission exceeds u128")?;
            plan.reward_index = plan
                .reward_index
                .checked_add(scaled / plan.total_stake)
                .context("Staking index exceeds u128")?;
            plan.reward_index_residual = scaled % plan.total_stake;
            plan.pending_staking_emission = 0;
        }
        put(&mut writes, "staking:reward_index", &plan.reward_index)?;
        put(
            &mut writes,
            "staking:pending_emission",
            &plan.pending_staking_emission,
        )?;
        put(
            &mut writes,
            "staking:reward_residual",
            &plan.reward_index_residual,
        )?;
        next_staking = Some(plan);
    }
    // Bincode struct fields and tuple fields have the same wire layout. Ordered
    // pool entries preserve the existing EmissionEvent decoder and stable bytes.
    let event = (
        next,
        timestamp,
        total,
        pools,
        next_staking.as_ref().map(|s| s.reward_index),
        circulating,
    );
    put(&mut writes, &format!("emission:event:{next}"), &event)?;
    Ok((writes, circulating, next_staking))
}

/// Prepare the v2 interval from parent state for explicit development finalization.
/// Issuance remains the configured development schedule; no adaptive mainnet
/// epoch-to-block budget is inferred here.
pub(crate) fn prepare_reward_interval(
    emission: &EmissionEngine,
    next: u64,
    timestamp: u64,
    parent_hash: &str,
) -> Result<(Writes, u128, Option<StakingModule>)> {
    use crate::runtime::reward_runtime::{RewardState, REWARD_STATE_KEY};
    let raw = emission
        .storage
        .db
        .get(REWARD_STATE_KEY)?
        .context("Reward-v2 genesis is required")?;
    let mut rewards = RewardState::decode(&raw)?;
    let (mut writes, circulating, _) = prepare(emission, None, next, timestamp)?;
    let pool_key = "emission:pool:staking_rewards";
    let next_pool: u128 = bincode::deserialize(
        writes
            .get(pool_key.as_bytes())
            .context("Missing staking pool credit")?,
    )?;
    let old_pool: u128 = read(&emission.storage, pool_key)?;
    let budget = next_pool
        .checked_sub(old_pool)
        .context("Staking interval budget underflow")?;
    ensure!(
        rewards.stage_interval(next, parent_hash, budget)?,
        "Interval already allocated outside this block"
    );
    writes.insert(REWARD_STATE_KEY.as_bytes().to_vec(), rewards.encode()?);
    Ok((writes, circulating, None))
}

/// Stage the finalized block's issuance, effective stake, and reward liabilities.
/// The caller joins the prepared journal to the same guarded block write.
pub(crate) fn prepare_adaptive_interval(
    emission: &EmissionEngine,
    next: u64,
    timestamp: u64,
    parent_hash: &str,
    observation: Option<&crate::runtime::issuance_timing::EpochObservation>,
) -> Result<(
    Writes,
    u128,
    Option<dytallix_storage::adaptive::PreparedJournalUpdate>,
)> {
    use crate::runtime::{
        issuance_timing::{plan_block, TimingState, TIMING_STATE_KEY},
        reward_runtime::{RewardState, REWARD_STATE_KEY},
    };
    let storage = &emission.storage;
    let timing = TimingState::decode(
        &storage
            .db
            .get(TIMING_STATE_KEY)?
            .context("Explicit issuance genesis is required")?,
    )?;
    ensure!(
        height(storage, "emission:last_height")?.checked_add(1) == Some(next),
        "Issuance height differs from block parent"
    );
    let planned = plan_block(storage, &timing, next, parent_hash, observation)?;
    let pools: BTreeMap<String, u128> = [
        ("validator_rewards", planned.block_pools.validator_rewards),
        ("staking_rewards", planned.block_pools.staking_rewards),
        ("treasury", planned.block_pools.treasury),
        ("issuance_reserve", planned.block_pools.issuance_reserve),
    ]
    .into_iter()
    .map(|(name, amount)| (name.to_owned(), amount))
    .collect();
    let total = pools.values().try_fold(0u128, |sum, amount| {
        sum.checked_add(*amount)
            .context("Block issuance exceeds u128")
    })?;
    let circulating = read::<u128>(storage, "emission:circulating_supply")?
        .checked_add(total)
        .context("Cumulative issuance exceeds u128")?;
    crate::supply::genesis_amount(storage)?
        .checked_add(circulating)
        .context("Total DRT supply exceeds u128")?;
    let mut writes = planned.writes;
    for (name, amount) in &pools {
        let key = format!("emission:pool:{name}");
        let balance = read::<u128>(storage, &key)?
            .checked_add(*amount)
            .context("Issuance pool exceeds u128")?;
        put(&mut writes, &key, &balance)?;
    }
    put(&mut writes, "emission:circulating_supply", &circulating)?;
    writes.insert(
        b"emission:last_height".to_vec(),
        next.to_be_bytes().to_vec(),
    );
    let mut rewards = RewardState::decode(
        &storage
            .db
            .get(REWARD_STATE_KEY)?
            .context("Reward-v2 genesis is required")?,
    )?;
    if let Some(raw) = storage
        .db
        .get(crate::runtime::validator_lifecycle::STATE_KEY)?
    {
        let mut validators = crate::runtime::validator_lifecycle::LifecycleState::decode(&raw)?;
        let parent_time = if next == 1 {
            0
        } else {
            type StoredEmissionEvent = (u64, u64, u128, BTreeMap<String, u128>, Option<u128>, u128);
            let raw = storage
                .db
                .get(format!("emission:event:{}", next - 1))?
                .context("Missing parent timestamp for validator activation")?;
            let event: StoredEmissionEvent = bincode::deserialize(&raw)
                .context("Invalid parent timestamp for validator activation")?;
            ensure!(
                event.0 == next - 1 && bincode::serialize(&event)? == raw,
                "Validator activation parent event differs"
            );
            event.1
        };
        let before = validators.clone();
        validators.advance(next, parent_time, &mut rewards)?;
        if let Some(raw) = storage.db.get(crate::runtime::penalty_custody::STATE_KEY)? {
            ensure!(
                rewards.locks.is_empty(),
                "Penalty qualification does not support vesting locks"
            );
            let mut penalties = crate::runtime::penalty_custody::PenaltyState::decode(&raw)?;
            penalties.sync_lifecycle(&before, &validators)?;
            let committed_parent_time =
                crate::consensus_settlement::committed_parent_time(storage, next)?;
            ensure!(
                committed_parent_time.0 == parent_time,
                "Lifecycle and penalty parent timestamps differ"
            );
            penalties.begin_block(next, committed_parent_time, &validators)?;
            writes.insert(
                crate::runtime::penalty_custody::STATE_KEY
                    .as_bytes()
                    .to_vec(),
                penalties.encode()?,
            );
        }
        writes.insert(
            crate::runtime::validator_lifecycle::STATE_KEY
                .as_bytes()
                .to_vec(),
            validators.encode()?,
        );
    }
    ensure!(
        rewards.stage_interval(next, parent_hash, planned.block_pools.staking_rewards)?,
        "Interval already allocated outside this block"
    );
    writes.insert(REWARD_STATE_KEY.as_bytes().to_vec(), rewards.encode()?);
    let event = (next, timestamp, total, pools, None::<u128>, circulating);
    put(&mut writes, &format!("emission:event:{next}"), &event)?;
    if let Some(journal) = &planned.journal {
        for (key, value) in journal.writes() {
            ensure!(
                writes.insert(key.clone(), value.clone()).is_none(),
                "Duplicate planned controller write"
            );
        }
    }
    Ok((writes, circulating, planned.journal))
}
