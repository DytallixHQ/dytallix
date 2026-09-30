//! Pure issuance and reward planning for the selected block profile.
use crate::storage::state::Storage;
use anyhow::{ensure, Context, Result};
use serde::de::DeserializeOwned;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) type Writes = BTreeMap<Vec<u8>, Vec<u8>>;
/// Keys a block removes. A block never writes and deletes the same key.
pub(crate) type Deletes = std::collections::BTreeSet<Vec<u8>>;
/// Entries under `prefixes` plus the listed `keys`, read from one snapshot.
/// Unlike a full scan, the cost does not grow with unrelated records such as
/// block history. Keys already covered by a prefix are not read twice.
pub(crate) fn snapshot_selected(
    snapshot: &rocksdb::Snapshot<'_>,
    prefixes: &[&[u8]],
    keys: &[&[u8]],
) -> Result<Vec<(Box<[u8]>, Box<[u8]>)>> {
    let mut entries = Vec::new();
    for prefix in prefixes {
        let mode = rocksdb::IteratorMode::From(prefix, rocksdb::Direction::Forward);
        for item in snapshot.iterator(mode) {
            let (key, value) = item?;
            if !key.starts_with(prefix) {
                break;
            }
            entries.push((key, value));
        }
    }
    for key in keys {
        if prefixes.iter().any(|prefix| key.starts_with(prefix)) {
            continue;
        }
        if let Some(value) = snapshot.get(key)? {
            entries.push((Box::from(*key), value.into_boxed_slice()));
        }
    }
    Ok(entries)
}
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

/// Stage the finalized block's issuance, effective stake, and reward liabilities.
/// The caller joins the prepared journal to the same guarded block write.
pub(crate) fn prepare_adaptive_interval(
    storage: &Storage,
    next: u64,
    timestamp: u64,
    parent_hash: &str,
    observation: Option<&crate::runtime::issuance_timing::EpochObservation>,
) -> Result<(
    Writes,
    u128,
    Option<dytallix_storage::adaptive::PreparedJournalUpdate>,
    Deletes,
)> {
    use crate::runtime::{
        issuance_timing::{plan_block, TimingState, TIMING_STATE_KEY},
        reward_runtime::{RewardState, REWARD_STATE_KEY},
    };
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
    // Voting power of each operator owner in this block's validator set.
    let mut payout_weights: BTreeMap<String, u128> = BTreeMap::new();
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
        let mut released = Vec::new();
        let penalty_raw = storage.db.get(crate::runtime::penalty_custody::STATE_KEY)?;
        let custody = penalty_raw.is_some();
        if let Some(raw) = penalty_raw {
            let locked: BTreeSet<String> = rewards.locks.keys().cloned().collect();
            let mut penalties = crate::runtime::penalty_custody::PenaltyState::decode(&raw)?;
            penalties.sync_lifecycle(&before, &validators)?;
            let committed_parent_time =
                crate::consensus_settlement::committed_parent_time(storage, next)?;
            ensure!(
                committed_parent_time.0 == parent_time,
                "Lifecycle and penalty parent timestamps differ"
            );
            penalties.begin_block(next, committed_parent_time, &validators, &locked)?;
            // Unbonds released in the previous block leave custody here, with
            // their lifecycle entries below (state model step 4).
            released = penalties.prune_released()?;
            validators.remove_released(&released, &mut rewards)?;
            // Records past the evidence horizon: incidents before the history
            // they name.
            penalties.prune_incidents(&validators, next - 1, parent_time)?;
            validators.prune_history(next - 1, parent_time)?;
            penalties.consolidate(validators.history.base_height)?;
            penalties.validate(&validators)?;
            penalties.validate_lock_relief(&locked)?;
            writes.insert(
                crate::runtime::penalty_custody::STATE_KEY
                    .as_bytes()
                    .to_vec(),
                penalties.encode()?,
            );
        }
        if released.is_empty() {
            // Without releases, still free the slots of owners holding nothing.
            validators.remove_released(&[], &mut rewards)?;
        }
        if !custody {
            validators.prune_history(next - 1, parent_time)?;
        }
        payout_weights = validators.payout_weights(&rewards)?;
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
    // The validator share is owed by voting power (fees v1). Without a
    // lifecycle there is no power, so it stays in the reserve.
    rewards.stage_validator_payouts(planned.block_pools.validator_rewards, &payout_weights)?;
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
    let deletes: Deletes = planned.deletes.into_iter().collect();
    ensure!(
        deletes.iter().all(|key| !writes.contains_key(key)),
        "Issuance window writes and removes one record"
    );
    Ok((writes, circulating, planned.journal, deletes))
}
