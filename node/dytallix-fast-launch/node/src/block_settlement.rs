//! Atomic blocks for explicit development use. This is not a consensus protocol.
use crate::{
    block_lifecycle::{self, Writes},
    execution::stage_transaction,
    gas::GasSchedule,
    runtime::{emission::EmissionEngine, fee_burn::FeeBurnEngine, staking::StakingModule},
    settlement::{self, Settlement},
    state::State,
    storage::{blocks::Block, receipts::TxReceipt, state::Storage, tx::Transaction},
};
use anyhow::{bail, ensure, Context, Result};
use rocksdb::{IteratorMode, WriteBatch, WriteOptions};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const HEAD: &str = "execution:block:v1:head";
const POLICY: &str = "execution:block:v1:policy";
#[derive(Serialize, Deserialize)]
struct Head {
    version: u32,
    height: u64,
    hash: String,
    state_digest: String,
    policy: String,
    input_digest: String,
}
#[derive(Debug)]
pub struct BlockOutcome {
    pub block: Option<Block>,
    pub receipts: Vec<TxReceipt>,
    pub gas_used: u64,
    /// Local staging durations. These values are not consensus inputs.
    pub transaction_processing_times: Vec<std::time::Duration>,
}

pub fn check_profile(profile: Option<&str>, governance: bool, dev_endpoints: bool) -> Result<()> {
    ensure!(profile == Some("development"), "Set DYT_BLOCK_PROFILE=development for local qualification; production finality and activation remain unqualified");
    ensure!(
        !governance,
        "Governance activation requires staged block hooks"
    );
    ensure!(
        !dev_endpoints,
        "Direct funding endpoints cannot run with block settlement; use funded genesis"
    );
    bail!("Legacy node timer is retired; use an explicit finalization adapter")
}
fn hash<T: Serialize>(domain: &[u8], value: &T) -> Result<String> {
    let mut h = Sha256::new();
    h.update(domain);
    h.update(serde_json::to_vec(value)?);
    Ok(format!("0x{:x}", h.finalize()))
}
fn selected(key: &[u8]) -> bool {
    [
        b"acct:".as_slice(),
        b"dms:config:",
        b"emission:",
        b"staking:",
        b"rewards:",
        b"issuance:",
        b"adaptive:",
        b"supply:",
        b"genesis:",
    ]
    .iter()
    .any(|p| key.starts_with(p))
        || key == b"execution:v1:withheld_udrt"
        || key == b"meta:chain_id"
        || key == POLICY.as_bytes()
}
fn state_digest(storage: &Storage, writes: &Writes) -> Result<String> {
    let mut values = BTreeMap::new();
    for item in storage.db.iterator(IteratorMode::Start) {
        let (k, v) = item?;
        if selected(&k) {
            values.insert(k.to_vec(), v.to_vec());
        }
    }
    for (k, v) in writes {
        if selected(k) {
            values.insert(k.clone(), v.clone());
        }
    }
    // Byte arrays are encoded as sequence pairs, never JSON object keys.
    hash(
        b"dytallix-development-state-v1",
        &values.iter().collect::<Vec<_>>(),
    )
}
fn read_head(storage: &Storage) -> Result<Option<Head>> {
    storage
        .db
        .get(HEAD)?
        .map(|v| serde_json::from_slice(&v).context("Invalid block settlement head"))
        .transpose()
}
fn bytes(storage: &Storage, key: &str) -> Result<Vec<u8>> {
    storage
        .db
        .get(key)?
        .with_context(|| format!("Missing block record: {key}"))
}
fn text(storage: &Storage, key: &str) -> Result<String> {
    String::from_utf8(bytes(storage, key)?).context("Invalid block text")
}
fn load_block(storage: &Storage, height: u64) -> Result<Block> {
    let h = text(storage, &format!("blk_num:{height:016x}"))?;
    let block: Block = serde_json::from_slice(&bytes(storage, &format!("blk_hash:{h}"))?)?;
    ensure!(
        block.header.height == height
            && block.hash == h
            && block.header.execution_version == 1
            && block.header.tx_count as usize == block.txs.len()
            && block.hash == Block::compute_hash(&block.header, &block.txs),
        "Invalid committed block"
    );
    ensure!(
        block.header.tx_root == hash(b"dytallix-development-transactions-v1", &block.txs)?,
        "Invalid transaction commitment"
    );
    Ok(block)
}
/// Check the complete local journal at startup. Legacy partial state needs migration.
pub fn verify_recovery(storage: &Storage) -> Result<()> {
    if storage
        .db
        .get(crate::consensus_settlement::MODE_KEY)?
        .is_some()
    {
        return crate::consensus_settlement::verify_recovery(storage);
    }
    crate::genesis::reject_consensus_state(storage)?;
    crate::supply::validate(storage, &Writes::new())?;
    let reward_v2 = storage
        .db
        .get(crate::runtime::reward_runtime::REWARD_STATE_KEY)?
        .is_some();
    let timing = storage
        .db
        .get(crate::runtime::issuance_timing::TIMING_STATE_KEY)?
        .map(|raw| crate::runtime::issuance_timing::TimingState::decode(&raw))
        .transpose()?;
    let head = read_head(storage)?;
    let height = block_lifecycle::height(storage, "meta:height")?;
    let emission_height = block_lifecycle::height(storage, "emission:last_height")?;
    let Some(head) = head else {
        ensure!(
            height == 0 && emission_height == 0,
            "Unmarked block or emission history requires migration"
        );
        for item in storage.db.iterator(IteratorMode::Start) {
            let (k, _) = item?;
            ensure!(
                !k.starts_with(b"execution:v1:")
                    && !k.starts_with(b"execution:block:")
                    && !k.starts_with(b"blk_")
                    && !k.starts_with(b"emission:"),
                "Unmarked execution history requires migration"
            );
        }
        if let Some(best) = storage.db.get("meta:best_hash")? {
            ensure!(best == b"genesis", "Unmarked block head requires migration");
        }
        return Ok(());
    };
    ensure!(
        head.version == 1
            && head.height > 0
            && height == head.height
            && emission_height == height
            && text(storage, "meta:best_hash")? == head.hash
            && text(storage, POLICY)? == head.policy,
        "Block head metadata differs; recovery required"
    );
    let mut parent = "genesis".to_string();
    let mut members = BTreeSet::new();
    let mut blocks = BTreeSet::new();
    for h in 1..=height {
        let block = load_block(storage, h)?;
        ensure!(block.header.parent == parent, "Block parent differs");
        if let Some(timing) = &timing {
            if h > 1 && (h - 1) % timing.config.epoch_blocks == 0 {
                let epoch = (h - 1) / timing.config.epoch_blocks - 1;
                let raw = storage
                    .db
                    .get(crate::runtime::issuance_timing::observation_key(epoch))?
                    .context("Missing committed epoch observation")?;
                let observation = crate::runtime::issuance_timing::decode_observation(&raw)?;
                ensure!(
                    observation.parent_hash == block.header.parent,
                    "Epoch observation differs from committed parent"
                );
            }
        }
        let mut receipts = Vec::new();
        for (i, tx) in block.txs.iter().enumerate() {
            ensure!(
                members.insert(tx.hash.clone()),
                "Duplicate committed transaction"
            );
            let record = storage
                .get_transaction_record(&tx.hash)?
                .context("Missing committed transaction record; migration required")?;
            if reward_v2 {
                crate::signed_transaction::verify_reward_mode_record(
                    &record,
                    storage.get_chain_id().as_deref(),
                )?;
            } else {
                crate::signed_transaction::verify_record(
                    &record,
                    storage.get_chain_id().as_deref(),
                )?;
            }
            ensure!(
                record.matches(tx)?,
                "Transaction record differs from committed block"
            );
            let receipt = settlement::existing(storage, tx, h, u32::try_from(i)?)?
                .context("Missing transaction settlement")?;
            ensure!(
                serde_json::to_vec(&receipt)? == bytes(storage, &format!("rcpt:{}", tx.hash))?,
                "Receipt differs from block settlement"
            );
            receipts.push(receipt);
        }
        ensure!(
            block.header.execution_receipts_digest
                == hash(b"dytallix-development-receipts-v1", &receipts)?,
            "Receipt commitment differs"
        );
        if h == height {
            ensure!(
                block.header.execution_input_digest == head.input_digest,
                "Block request commitment differs"
            );
            ensure!(
                block.header.execution_state_digest == head.state_digest,
                "Block state commitment differs"
            );
        }
        blocks.insert(block.hash.clone());
        parent = block.hash;
    }
    ensure!(
        parent == head.hash && state_digest(storage, &Writes::new())? == head.state_digest,
        "Committed state differs; recovery required"
    );
    for item in storage.db.iterator(IteratorMode::Start) {
        let (k, _) = item?;
        if let Some(suffix) = k.strip_prefix(b"execution:v1:receipt:") {
            ensure!(
                members.contains(std::str::from_utf8(suffix)?),
                "Orphan transaction settlement requires migration"
            );
        }
        if let Some(suffix) = k.strip_prefix(b"blk_hash:") {
            ensure!(
                blocks.contains(std::str::from_utf8(suffix)?),
                "Orphan block requires migration"
            );
        }
        if let Some(suffix) = k.strip_prefix(b"blk_num:") {
            let h = u64::from_str_radix(std::str::from_utf8(suffix)?, 16)?;
            ensure!(
                h > 0 && h <= height && suffix == format!("{h:016x}").as_bytes(),
                "Orphan block index requires migration"
            );
        }
    }
    Ok(())
}
fn policy(emission: &EmissionEngine, staking: bool, gas: &GasSchedule) -> Result<String> {
    if let Some(raw) = emission
        .storage
        .db
        .get(crate::runtime::reward_runtime::REWARD_STATE_KEY)?
    {
        let rewards = crate::runtime::reward_runtime::RewardState::decode(&raw)?;
        ensure!(staking, "Reward-v2 genesis requires staking enabled");
        if let Some(raw) = emission
            .storage
            .db
            .get(crate::runtime::issuance_timing::TIMING_STATE_KEY)?
        {
            let timing = crate::runtime::issuance_timing::TimingState::decode(&raw)?;
            return hash(
                b"dytallix-development-issuance-policy-v1",
                &(
                    staking,
                    gas,
                    &rewards.config,
                    &timing.config,
                    &timing.binding,
                ),
            );
        }
        return hash(
            b"dytallix-development-reward-policy-v2",
            &(&emission.config, staking, gas, &rewards.config),
        );
    }
    hash(
        b"dytallix-development-policy-v1",
        &(&emission.config, staking, gas),
    )
}

pub fn verify_policy(emission: &EmissionEngine, staking_enabled: bool) -> Result<()> {
    if emission
        .storage
        .db
        .get(crate::runtime::issuance_timing::TIMING_STATE_KEY)?
        .is_none()
    {
        block_lifecycle::validate_initial_supply(emission)?;
        block_lifecycle::validate_config(&emission.config)?;
    }
    verify_recovery(&emission.storage)?;
    let wanted = policy(emission, staking_enabled, &GasSchedule::default())?;
    if let Some(stored) = emission.storage.db.get(POLICY)? {
        ensure!(
            stored == wanted.as_bytes(),
            "Block policy changed; explicit migration required"
        );
    }
    Ok(())
}

#[derive(Serialize)]
pub struct BlockRequest<'a> {
    pub height: u64,
    pub transactions: &'a [Transaction],
    pub assets: &'a [String],
    pub timestamp: u64,
    pub empty_blocks: bool,
}

/// Caller holds emission, staking, state, then diagnostic locks. No queue locks.
#[allow(clippy::too_many_arguments)]
pub fn commit_block(
    state: &mut State,
    emission: &mut EmissionEngine,
    staking: &mut StakingModule,
    burn: &mut FeeBurnEngine,
    staking_enabled: bool,
    request: &BlockRequest<'_>,
) -> Result<BlockOutcome> {
    commit_with(
        state,
        emission,
        staking,
        burn,
        staking_enabled,
        request,
        |storage, batch| {
            let mut options = WriteOptions::default();
            options.set_sync(true);
            storage.db.write_opt(batch, &options)?;
            Ok(())
        },
    )
}

/// Explicit development finalization fixture. A timer is not consensus finality.
/// Production reward activation remains rejected by the versioned genesis policy.
#[allow(clippy::too_many_arguments)]
pub fn commit_reward_development_block(
    state: &mut State,
    emission: &mut EmissionEngine,
    staking: &mut StakingModule,
    burn: &mut FeeBurnEngine,
    staking_enabled: bool,
    request: &BlockRequest<'_>,
) -> Result<BlockOutcome> {
    ensure!(
        state
            .storage
            .db
            .get(crate::runtime::reward_runtime::REWARD_STATE_KEY)?
            .is_some(),
        "Explicit reward-v2 genesis required"
    );
    commit_reward_with(
        state,
        emission,
        staking,
        burn,
        staking_enabled,
        request,
        true,
        |storage, batch| {
            let mut options = WriteOptions::default();
            options.set_sync(true);
            storage.db.write_opt(batch, &options)?;
            Ok(())
        },
    )
}
#[allow(clippy::too_many_arguments)]
fn commit_with(
    state: &mut State,
    emission: &mut EmissionEngine,
    staking: &mut StakingModule,
    burn: &mut FeeBurnEngine,
    staking_enabled: bool,
    request: &BlockRequest<'_>,
    write: impl FnOnce(&Storage, WriteBatch) -> Result<()>,
) -> Result<BlockOutcome> {
    let _ = (
        state,
        emission,
        staking,
        burn,
        staking_enabled,
        request,
        write,
    );
    bail!("Legacy timer settlement is retired")
}
/// Archived diagnostic entrypoint. The supported timer remains retired.
#[cfg(feature = "legacy-economic-fixtures")]
#[allow(clippy::too_many_arguments)]
pub fn commit_legacy_fixture_block(
    state: &mut State,
    emission: &mut EmissionEngine,
    staking: &mut StakingModule,
    burn: &mut FeeBurnEngine,
    staking_enabled: bool,
    request: &BlockRequest<'_>,
) -> Result<BlockOutcome> {
    commit_legacy_fixture_with(
        state,
        emission,
        staking,
        burn,
        staking_enabled,
        request,
        |storage, batch| {
            let mut options = WriteOptions::default();
            options.set_sync(true);
            storage.db.write_opt(batch, &options)?;
            Ok(())
        },
    )
}
#[cfg(feature = "legacy-economic-fixtures")]
#[allow(clippy::too_many_arguments)]
pub(crate) fn commit_legacy_fixture_with(
    state: &mut State,
    emission: &mut EmissionEngine,
    staking: &mut StakingModule,
    burn: &mut FeeBurnEngine,
    staking_enabled: bool,
    request: &BlockRequest<'_>,
    write: impl FnOnce(&Storage, WriteBatch) -> Result<()>,
) -> Result<BlockOutcome> {
    staking
        .ensure_legacy_fixture_mutation()
        .map_err(anyhow::Error::msg)?;
    commit_reward_with(
        state,
        emission,
        staking,
        burn,
        staking_enabled,
        request,
        false,
        write,
    )
}

#[allow(clippy::too_many_arguments)]
fn commit_reward_with(
    state: &mut State,
    emission: &mut EmissionEngine,
    staking: &mut StakingModule,
    burn: &mut FeeBurnEngine,
    staking_enabled: bool,
    request: &BlockRequest<'_>,
    development_finalization: bool,
    write: impl FnOnce(&Storage, WriteBatch) -> Result<()>,
) -> Result<BlockOutcome> {
    commit_issuance_with(
        state,
        emission,
        staking,
        burn,
        staking_enabled,
        request,
        development_finalization,
        false,
        None,
        write,
    )
}

/// Explicit local finalization adapter with caller-supplied completed-epoch observations.
/// Production consensus and observation authentication are not supplied by this adapter.
#[allow(clippy::too_many_arguments)]
pub fn commit_issuance_development_block(
    state: &mut State,
    emission: &mut EmissionEngine,
    staking: &mut StakingModule,
    burn: &mut FeeBurnEngine,
    staking_enabled: bool,
    request: &BlockRequest<'_>,
    observation: Option<&crate::runtime::issuance_timing::EpochObservation>,
) -> Result<BlockOutcome> {
    commit_issuance_with(
        state,
        emission,
        staking,
        burn,
        staking_enabled,
        request,
        true,
        true,
        observation,
        |storage, batch| {
            let mut options = WriteOptions::default();
            options.set_sync(true);
            storage.db.write_opt(batch, &options)?;
            Ok(())
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn commit_issuance_with(
    state: &mut State,
    emission: &mut EmissionEngine,
    staking: &mut StakingModule,
    burn: &mut FeeBurnEngine,
    staking_enabled: bool,
    request: &BlockRequest<'_>,
    development_finalization: bool,
    issuance_finalization: bool,
    observation: Option<&crate::runtime::issuance_timing::EpochObservation>,
    write: impl FnOnce(&Storage, WriteBatch) -> Result<()>,
) -> Result<BlockOutcome> {
    let BlockRequest {
        height: requested_height,
        transactions,
        assets,
        timestamp,
        empty_blocks,
    } = request;
    let (timestamp, empty_blocks) = (*timestamp, *empty_blocks);
    let storage = state.storage.clone();
    ensure!(
        std::sync::Arc::ptr_eq(&storage, &emission.storage)
            && std::sync::Arc::ptr_eq(&storage, &staking.storage),
        "Block modules use different storage handles"
    );
    let guard = storage.lock_execution()?;
    crate::genesis::reject_consensus_state(&storage)?;
    let reward_v2 = storage
        .db
        .get(crate::runtime::reward_runtime::REWARD_STATE_KEY)?
        .is_some();
    if !reward_v2 {
        staking
            .ensure_legacy_fixture_mutation()
            .map_err(anyhow::Error::msg)?;
    }
    ensure!(!reward_v2 || development_finalization, "Timer commits cannot finalize reward-v2; use the explicit development finalization adapter");
    let timed = storage
        .db
        .get(crate::runtime::issuance_timing::TIMING_STATE_KEY)?
        .is_some();
    ensure!(
        timed == issuance_finalization,
        "Issuance timing requires its explicit development adapter and genesis"
    );
    ensure!(
        !timed || reward_v2,
        "Issuance timing requires reward-v2 genesis"
    );
    ensure!(
        timed || observation.is_none(),
        "Observation supplied without issuance timing"
    );
    // Validate current state before planning any mutation.
    verify_recovery(&storage)?;
    if !timed {
        block_lifecycle::validate_config(&emission.config)?;
    }
    let gas = GasSchedule::default();
    let policy = policy(emission, staking_enabled, &gas)?;
    if let Some(stored) = storage.db.get(POLICY)? {
        ensure!(
            stored == policy.as_bytes(),
            "Block policy changed; explicit migration required"
        );
    }
    let input_digest = if timed {
        hash(
            b"dytallix-development-issuance-block-input-v1",
            &(request, &policy, observation),
        )?
    } else {
        hash(b"dytallix-development-block-input-v1", &(request, &policy))?
    };
    let head = read_head(&storage)?;
    if let Some(head) = head.as_ref().filter(|h| h.height == *requested_height) {
        ensure!(
            head.input_digest == input_digest,
            "Block retry input differs; recovery required"
        );
        let block = load_block(&storage, head.height)?;
        let receipts: Vec<_> = block
            .txs
            .iter()
            .enumerate()
            .map(|(index, tx)| {
                settlement::existing(&storage, tx, head.height, u32::try_from(index)?)
                    .and_then(|r| r.context("Missing block receipt"))
            })
            .collect::<Result<_>>()?;
        let gas_used = receipts.iter().try_fold(0u64, |sum, r| {
            sum.checked_add(r.gas_used).context("Block gas exceeds u64")
        })?;
        // A previous caller can have lost the commit acknowledgement. Hydrate
        // caches from verified durable state without repeating execution.
        let circulating = block_lifecycle::read(&storage, "emission:circulating_supply")?;
        let mut hydrated = staking.clone();
        hydrated.total_stake = block_lifecycle::read(&storage, "staking:total_stake")?;
        hydrated.reward_index = block_lifecycle::read(&storage, "staking:reward_index")?;
        hydrated.pending_staking_emission =
            block_lifecycle::read(&storage, "staking:pending_emission")?;
        hydrated.reward_index_residual =
            block_lifecycle::read(&storage, "staking:reward_residual")?;
        state.accounts.clear();
        emission.circulating_supply = circulating;
        *staking = hydrated;
        return Ok(BlockOutcome {
            block: Some(block),
            receipts,
            gas_used,
            transaction_processing_times: Vec::new(),
        });
    }
    let height = block_lifecycle::height(&storage, "meta:height")?
        .checked_add(1)
        .context("Block height exhausted")?;
    ensure!(
        *requested_height == height,
        "Block height is not the next committed height"
    );
    let parent = head.map_or_else(|| "genesis".into(), |h| h.hash);
    if reward_v2 && height > 1 {
        let prior = load_block(&storage, height - 1)?;
        ensure!(
            timestamp >= prior.header.timestamp,
            "Reward block timestamp regressed"
        );
    }
    let mut staged = Settlement::new(storage.clone());
    let mut journal = None;
    let prepared = if timed {
        let (writes, circulating, prepared_journal) = block_lifecycle::prepare_adaptive_interval(
            emission,
            height,
            timestamp,
            &parent,
            observation,
        )?;
        journal = prepared_journal;
        staged.attach_reward_lifecycle(writes, timestamp)?;
        Some(circulating)
    } else if reward_v2 {
        let (writes, circulating, _) =
            block_lifecycle::prepare_reward_interval(emission, height, timestamp, &parent)?;
        staged.attach_reward_lifecycle(writes, timestamp)?;
        Some(circulating)
    } else {
        None
    };
    let mut txs = Vec::new();
    let mut receipts = Vec::new();
    let mut rejected = Vec::new();
    let mut transaction_processing_times = Vec::new();
    let mut gas_used = 0u64;
    let mut hashes = BTreeSet::new();
    let mut next_burn = burn.clone();
    for tx in *transactions {
        ensure!(
            hashes.insert(&tx.hash),
            "Duplicate transaction in block input"
        );
        if storage
            .db
            .get(format!("execution:v1:receipt:{}", tx.hash))?
            .is_some()
        {
            bail!("Transaction already committed; reload the block head before retry");
        }
        let index = u32::try_from(txs.len()).context("Block transaction count exceeds u32")?;
        if let Some(raw) = storage.db.get(format!("rcpt:{}", tx.hash))? {
            let prior: TxReceipt = serde_json::from_slice(&raw)?;
            ensure!(
                prior.status == crate::storage::receipts::TxStatus::Pending,
                "Transaction already has a terminal receipt"
            );
        }
        let record = crate::storage::transaction_record::TransactionRecord::decode(
            &tx.hash,
            &storage.planned_transaction_record(tx, None)?,
        )?;
        if reward_v2 {
            crate::signed_transaction::verify_reward_mode_record(
                &record,
                storage.get_chain_id().as_deref(),
            )?;
        } else {
            crate::signed_transaction::verify_record(&record, storage.get_chain_id().as_deref())?;
        }
        let checkpoint = staged.clone();
        let started = std::time::Instant::now();
        let planned = stage_transaction(tx, &mut staged, height, index, &gas)?;
        transaction_processing_times.push(started.elapsed());
        if !planned.accepted {
            staged = checkpoint;
            let mut receipt = planned.result.receipt;
            receipt.block_height = None;
            receipt.index = None;
            rejected.push(receipt);
            continue;
        }
        let receipt = planned.result.receipt;
        gas_used = gas_used
            .checked_add(receipt.gas_used)
            .context("Block gas exceeds u64")?;
        if receipt.success {
            // Legacy diagnostics are not part of issuance or state commitment.
            let mut candidate = next_burn.clone();
            if candidate
                .process_fee_burn(
                    tx.hash.clone(),
                    height,
                    u128::from(receipt.gas_limit) * u128::from(receipt.gas_price),
                    state,
                )
                .is_ok()
            {
                next_burn = candidate;
            }
        }
        txs.push(tx.clone());
        receipts.push(receipt);
    }
    if txs.is_empty() && assets.is_empty() && !empty_blocks {
        if !rejected.is_empty() {
            let mut batch = WriteBatch::default();
            for receipt in rejected {
                batch.put(
                    format!("rcpt:{}", receipt.tx_hash),
                    serde_json::to_vec(&receipt)?,
                );
            }
            write(&storage, batch)?;
        }
        return Ok(BlockOutcome {
            block: None,
            receipts,
            gas_used,
            transaction_processing_times,
        });
    }
    let (mut writes, circulating, next_staking) = if let Some(circulating) = prepared {
        (Writes::new(), circulating, None)
    } else {
        block_lifecycle::prepare(
            emission,
            staking_enabled.then_some(&*staking),
            height,
            timestamp,
        )?
    };
    writes.extend(staged.writes()?);
    crate::supply::validate(&storage, &writes)?;
    writes.insert(POLICY.as_bytes().to_vec(), policy.as_bytes().to_vec());
    let mut block = Block::new(height, parent, timestamp, txs);
    block.header.tx_count = u32::try_from(block.txs.len())?;
    block.header.execution_version = 1;
    block.header.execution_input_digest = input_digest.clone();
    block.header.execution_receipts_digest = hash(b"dytallix-development-receipts-v1", &receipts)?;
    block.header.asset_hashes = assets.to_vec();
    block.header.tx_root = hash(b"dytallix-development-transactions-v1", &block.txs)?;
    block.header.execution_state_digest = state_digest(&storage, &writes)?;
    block.hash = Block::compute_hash(&block.header, &block.txs);
    let head = Head {
        version: 1,
        height,
        hash: block.hash.clone(),
        state_digest: block.header.execution_state_digest.clone(),
        policy: policy.clone(),
        input_digest,
    };
    let mut batch = WriteBatch::default();
    let adaptive_writes: Vec<_> = writes
        .iter()
        .filter(|(k, _)| k.starts_with(b"adaptive:"))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if let Some(plan) = &journal {
        let expected: BTreeMap<_, _> = plan.writes().iter().cloned().collect();
        ensure!(
            adaptive_writes.iter().cloned().collect::<BTreeMap<_, _>>() == expected,
            "Controller journal overlay differs from prepared update"
        );
        plan.append_checked(&storage, &guard, &mut batch)?;
    } else {
        ensure!(
            adaptive_writes.is_empty(),
            "Unexpected controller journal writes"
        );
    }
    for (k, v) in writes {
        if !k.starts_with(b"adaptive:") {
            batch.put(k, v);
        }
    }
    for receipt in rejected {
        batch.put(
            format!("rcpt:{}", receipt.tx_hash),
            serde_json::to_vec(&receipt)?,
        );
    }
    for (tx, receipt) in block.txs.iter().zip(&receipts) {
        Settlement::append_receipt(&storage, &mut batch, tx, receipt)?;
    }
    batch.put(
        format!("blk_hash:{}", block.hash),
        serde_json::to_vec(&block)?,
    );
    batch.put(format!("blk_num:{height:016x}"), block.hash.as_bytes());
    batch.put("meta:height", height.to_be_bytes());
    batch.put("meta:best_hash", block.hash.as_bytes());
    batch.put(HEAD, serde_json::to_vec(&head)?);
    batch.put(POLICY, policy.as_bytes());
    write(&storage, batch)?;
    staged.publish(state);
    emission.circulating_supply = circulating;
    if let Some(next) = next_staking {
        *staking = next;
    } else if reward_v2 {
        *staking = StakingModule::new(storage.clone());
    }
    *burn = next_burn;
    Ok(BlockOutcome {
        block: Some(block),
        receipts,
        gas_used,
        transaction_processing_times,
    })
}
/// Remove only the prefix included in the committed block. Retain later arrivals.
pub fn acknowledge_assets(pending: &mut Vec<String>, committed: &[String]) -> Result<()> {
    ensure!(
        pending.starts_with(committed),
        "Pending asset prefix changed after block commit"
    );
    pending.drain(..committed.len());
    Ok(())
}

#[cfg(test)]
#[path = "block_settlement_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "issuance_settlement_tests.rs"]
mod issuance_tests;

#[cfg(test)]
mod consensus_mode_tests {
    use super::*;
    #[test]
    fn orphan_consensus_record_rejects_development_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path().join("db")).unwrap();
        storage
            .db
            .put("consensus:unknown:orphan", b"fixture")
            .unwrap();
        let error = verify_recovery(&storage).unwrap_err().to_string();
        assert!(error.contains("Consensus state requires"));
        assert_eq!(storage.db.iterator(IteratorMode::Start).count(), 1);
    }
}
