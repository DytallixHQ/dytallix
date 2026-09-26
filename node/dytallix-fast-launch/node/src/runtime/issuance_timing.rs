//! Explicit development epoch budgets. This adapter supplies no production finality.
use anyhow::{ensure, Context, Result};
use bincode::Options;
use dytallix_adaptive_emission::{Config, Gains, Observation};
use dytallix_storage::{
    adaptive::{prepare_transition, verify_view, PreparedJournalUpdate},
    state::Storage,
};
use rocksdb::IteratorMode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const TIMING_STATE_KEY: &str = "issuance:v1:state";
const OBSERVATION_PREFIX: &[u8] = b"issuance:v1:observation:";
const MAX_STATE_BYTES: usize = 16_384;
const MAX_RECORDED_EPOCHS: u64 = 1_000_000;
mod decimal_u64 {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        let s = String::deserialize(d)?;
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(serde::de::Error::custom(
                "Amount must be a decimal digit string",
            ));
        }
        s.parse().map_err(serde::de::Error::custom)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GainsInput {
    pub proportional: u64,
    pub integral: u64,
    pub derivative: u64,
}
impl GainsInput {
    fn gains(&self) -> Gains {
        Gains {
            proportional: self.proportional,
            integral: self.integral,
            derivative: self.derivative,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerInputs {
    pub target_ppm: u64,
    pub shock_threshold_ppm: u64,
    pub volatility_threshold_ppm: u64,
    pub window_samples: u32,
    pub integral_min: i64,
    pub integral_max: i64,
    pub soft: GainsInput,
    pub hard: GainsInput,
    pub base_udrt: u64,
    pub min_udrt: u64,
    pub max_udrt: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimingGenesis {
    pub version: u32,
    pub profile: String,
    pub decimals: u8,
    pub epoch_blocks: u64,
    #[serde(with = "decimal_u64")]
    pub initial_epoch_budget_udrt: u64,
    pub controller: ControllerInputs,
    pub max_recorded_epochs: u64,
}
impl TimingGenesis {
    pub fn controller_config(&self) -> Result<Config> {
        let c = &self.controller;
        let config = Config {
            target_ppm: c.target_ppm,
            shock_threshold_ppm: c.shock_threshold_ppm,
            volatility_threshold_ppm: c.volatility_threshold_ppm,
            window_samples: usize::try_from(c.window_samples)?,
            integral_min: c.integral_min,
            integral_max: c.integral_max,
            soft: c.soft.gains(),
            hard: c.hard.gains(),
            base_udrt: c.base_udrt,
            min_udrt: c.min_udrt,
            max_udrt: c.max_udrt,
        };
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1 && self.decimals == 6 && self.profile == "development",
            "Unsupported issuance version, scale or production activation"
        );
        ensure!(
            self.epoch_blocks > 0 && (1..=MAX_RECORDED_EPOCHS).contains(&self.max_recorded_epochs),
            "Invalid issuance timing or journal limit"
        );
        let c = self.controller_config()?;
        ensure!(
            (c.min_udrt..=c.max_udrt).contains(&self.initial_epoch_budget_udrt),
            "Initial epoch budget exceeds controller bounds"
        );
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PoolAmounts {
    pub validator_rewards: u128,
    pub staking_rewards: u128,
    pub treasury: u128,
    pub issuance_reserve: u128,
}
impl PoolAmounts {
    pub fn entries(&self) -> [(&'static str, u128); 4] {
        [
            ("validator_rewards", self.validator_rewards),
            ("staking_rewards", self.staking_rewards),
            ("treasury", self.treasury),
            ("issuance_reserve", self.issuance_reserve),
        ]
    }
    pub fn total(&self) -> Result<u128> {
        self.entries().iter().try_fold(0u128, |sum, (_, v)| {
            sum.checked_add(*v)
                .context("Issuance pool sum exceeds u128")
        })
    }
    pub fn epoch_split(budget: u64) -> Self {
        let e = u128::from(budget);
        let v = e * 4 / 10;
        let s = e * 3 / 10;
        let t = e * 3 / 10;
        Self {
            validator_rewards: v,
            staking_rewards: s,
            treasury: t,
            issuance_reserve: e - v - s - t,
        }
    }
    fn map(&self, f: impl Fn(u128) -> Result<u128>) -> Result<Self> {
        Ok(Self {
            validator_rewards: f(self.validator_rewards)?,
            staking_rewards: f(self.staking_rewards)?,
            treasury: f(self.treasury)?,
            issuance_reserve: f(self.issuance_reserve)?,
        })
    }
    pub fn prefix(&self, epoch_blocks: u64, blocks: u64) -> Result<Self> {
        ensure!(
            epoch_blocks > 0 && blocks <= epoch_blocks,
            "Invalid epoch prefix"
        );
        let n = u128::from(epoch_blocks);
        let count = u128::from(blocks);
        self.map(|p| {
            (p / n)
                .checked_mul(count)
                .and_then(|q| q.checked_add(count.min(p % n)))
                .context("Epoch prefix exceeds u128")
        })
    }
    pub fn block(&self, epoch_blocks: u64, offset: u64) -> Result<Self> {
        ensure!(
            epoch_blocks > 0 && offset < epoch_blocks,
            "Invalid epoch block offset"
        );
        let n = u128::from(epoch_blocks);
        let k = u128::from(offset);
        self.map(|p| {
            (p / n)
                .checked_add(u128::from(k < p % n))
                .context("Block issuance exceeds u128")
        })
    }
    pub fn checked_add(&self, other: &Self) -> Result<Self> {
        Ok(Self {
            validator_rewards: self
                .validator_rewards
                .checked_add(other.validator_rewards)
                .context("Validator issuance exceeds u128")?,
            staking_rewards: self
                .staking_rewards
                .checked_add(other.staking_rewards)
                .context("Staking issuance exceeds u128")?,
            treasury: self
                .treasury
                .checked_add(other.treasury)
                .context("Treasury issuance exceeds u128")?,
            issuance_reserve: self
                .issuance_reserve
                .checked_add(other.issuance_reserve)
                .context("Issuance reserve exceeds u128")?,
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpochObservation {
    pub epoch: u64,
    pub utilization_ppm: u64,
    pub volatility_ppm: u64,
    pub first_height: u64,
    pub last_height: u64,
    pub parent_hash: String,
}
pub fn observation_key(epoch: u64) -> Vec<u8> {
    let mut key = OBSERVATION_PREFIX.to_vec();
    key.extend_from_slice(&epoch.to_be_bytes());
    key
}
pub fn decode_observation(bytes: &[u8]) -> Result<EpochObservation> {
    let obs: EpochObservation = decode_canonical(bytes)?;
    valid_id(&obs.parent_hash)?;
    Ok(obs)
}
fn valid_id(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 256
            && !value.chars().any(|c| c.is_whitespace() || c.is_control()),
        "Invalid issuance identifier"
    );
    Ok(())
}
fn decode_canonical<T: serde::de::DeserializeOwned + Serialize>(bytes: &[u8]) -> Result<T> {
    ensure!(
        bytes.len() <= MAX_STATE_BYTES,
        "Issuance record exceeds encoded limit"
    );
    let value: T = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(MAX_STATE_BYTES as u64)
        .reject_trailing_bytes()
        .deserialize(bytes)?;
    ensure!(
        bincode::serialize(&value)? == bytes,
        "Noncanonical issuance record"
    );
    Ok(value)
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimingState {
    pub config: TimingGenesis,
    pub chain_id: String,
    pub genesis_digest: String,
    pub binding: [u8; 32],
    pub last_height: u64,
    pub active_epoch: u64,
    pub epoch_budget_udrt: u64,
    pub issued_in_epoch: PoolAmounts,
    pub total_issued: PoolAmounts,
}
impl TimingState {
    pub fn new(config: TimingGenesis, chain_id: String, genesis_digest: String) -> Result<Self> {
        let mut state = Self {
            epoch_budget_udrt: config.initial_epoch_budget_udrt,
            config,
            chain_id,
            genesis_digest,
            binding: [0; 32],
            last_height: 0,
            active_epoch: 0,
            issued_in_epoch: PoolAmounts::default(),
            total_issued: PoolAmounts::default(),
        };
        state.binding = state.compute_binding()?;
        state.validate_internal()?;
        Ok(state)
    }
    fn compute_binding(&self) -> Result<[u8; 32]> {
        valid_id(&self.chain_id)?;
        ensure!(
            self.genesis_digest.len() == 64
                && self
                    .genesis_digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "Invalid canonical genesis digest"
        );
        let mut h = Sha256::new();
        h.update(b"dytallix-issuance-timing-binding-v1");
        h.update(bincode::serialize(&(
            &self.chain_id,
            &self.genesis_digest,
            &self.config,
        ))?);
        Ok(h.finalize().into())
    }
    pub fn validate_internal(&self) -> Result<()> {
        self.config.validate()?;
        ensure!(
            self.binding == self.compute_binding()?,
            "Issuance binding differs from configuration"
        );
        let epoch = if self.last_height == 0 {
            0
        } else {
            (self.last_height - 1) / self.config.epoch_blocks
        };
        ensure!(
            self.active_epoch == epoch && epoch <= self.config.max_recorded_epochs,
            "Issuance epoch differs from height or audit limit"
        );
        let count = if self.last_height == 0 {
            0
        } else {
            (self.last_height - 1) % self.config.epoch_blocks + 1
        };
        let c = self.config.controller_config()?;
        ensure!(
            (c.min_udrt..=c.max_udrt).contains(&self.epoch_budget_udrt),
            "Epoch budget exceeds controller bounds"
        );
        ensure!(
            self.issued_in_epoch
                == PoolAmounts::epoch_split(self.epoch_budget_udrt)
                    .prefix(self.config.epoch_blocks, count)?,
            "Issued epoch prefix differs"
        );
        self.total_issued.total()?;
        for ((_, total), (_, prefix)) in self
            .total_issued
            .entries()
            .iter()
            .zip(self.issued_in_epoch.entries())
        {
            ensure!(
                *total >= prefix,
                "Cumulative issuance is below current epoch"
            );
        }
        if epoch == 0 {
            ensure!(
                self.epoch_budget_udrt == self.config.initial_epoch_budget_udrt
                    && self.total_issued == self.issued_in_epoch,
                "Initial command or issuance differs"
            );
        }
        Ok(())
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate_internal()?;
        let bytes = bincode::serialize(self)?;
        ensure!(bytes.len() <= MAX_STATE_BYTES, "Issuance state too large");
        Ok(bytes)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let state: Self = decode_canonical(bytes)?;
        state.validate_internal()?;
        Ok(state)
    }
}
#[derive(Debug)]
pub struct PlannedIssuance {
    pub next_state: TimingState,
    pub block_pools: PoolAmounts,
    pub journal: Option<PreparedJournalUpdate>,
    pub writes: BTreeMap<Vec<u8>, Vec<u8>>,
}
fn observation_range(obs: &EpochObservation, epoch_blocks: u64) -> Result<()> {
    valid_id(&obs.parent_hash)?;
    let first = obs
        .epoch
        .checked_mul(epoch_blocks)
        .and_then(|h| h.checked_add(1))
        .context("Observation first height exceeds u64")?;
    let last = obs
        .epoch
        .checked_add(1)
        .and_then(|e| e.checked_mul(epoch_blocks))
        .context("Observation last height exceeds u64")?;
    ensure!(
        obs.first_height == first && obs.last_height == last,
        "Observation range differs from completed epoch"
    );
    Ok(())
}
/// Read-only planner. Caller holds this storage's execution lock through the combined commit.
pub fn plan_block(
    storage: &Storage,
    state: &TimingState,
    height: u64,
    parent_hash: &str,
    obs: Option<&EpochObservation>,
) -> Result<PlannedIssuance> {
    state.validate_internal()?;
    valid_id(parent_hash)?;
    ensure!(
        state.last_height.checked_add(1) == Some(height),
        "Issuance height is not next block"
    );
    let mut values = BTreeMap::new();
    let view_limit = state
        .config
        .max_recorded_epochs
        .checked_mul(2)
        .and_then(|v| v.checked_add(5))
        .context("Issuance view limit exceeds u64")?;
    let snapshot = storage.db.snapshot();
    for entry in snapshot.iterator(IteratorMode::Start) {
        let (key, value) = entry?;
        if selected(&key) {
            ensure!(
                u64::try_from(values.len())? < view_limit,
                "Issuance view exceeds record limit"
            );
            let byte_limit = if key.as_ref() == b"adaptive:v1:head" {
                dytallix_adaptive_emission::MAX_ENCODED_LEN + 72
            } else {
                MAX_STATE_BYTES
            };
            ensure!(
                value.len() <= byte_limit,
                "Issuance view record exceeds encoded limit"
            );
            values.insert(key.to_vec(), value.to_vec());
        }
    }
    ensure!(
        verify_overlay(&values)? == *state,
        "Issuance planner state differs from durable state"
    );
    let n = state.config.epoch_blocks;
    let epoch = (height - 1) / n;
    let offset = (height - 1) % n;
    ensure!(
        epoch <= state.config.max_recorded_epochs,
        "Issuance journal audit limit reached"
    );
    let mut next = state.clone();
    let mut writes = BTreeMap::new();
    let mut journal = None;
    if epoch > state.active_epoch {
        ensure!(
            offset == 0 && state.active_epoch.checked_add(1) == Some(epoch),
            "Invalid issuance epoch transition"
        );
        let obs = obs.context("Missing completed epoch observation")?;
        observation_range(obs, n)?;
        ensure!(
            obs.epoch == state.active_epoch
                && obs.last_height == state.last_height
                && obs.parent_hash == parent_hash,
            "Observation epoch or parent differs"
        );
        let prepared = prepare_transition(
            storage,
            state.binding,
            state.config.controller_config()?,
            Observation {
                epoch: obs.epoch,
                utilization_ppm: obs.utilization_ppm,
                volatility_ppm: obs.volatility_ppm,
            },
        )?;
        let command = prepared
            .command()
            .context("Prepared controller command is missing")?;
        ensure!(
            command.for_epoch == epoch,
            "Controller command is for another epoch"
        );
        next.active_epoch = epoch;
        next.epoch_budget_udrt = command.emission_udrt;
        next.issued_in_epoch = PoolAmounts::default();
        let key = observation_key(obs.epoch);
        ensure!(
            storage.db.get(&key)?.is_none(),
            "Observation record already exists"
        );
        writes.insert(key, bincode::serialize(obs)?);
        journal = Some(prepared);
    } else {
        ensure!(obs.is_none(), "Unexpected observation within epoch");
    }
    let block_pools = PoolAmounts::epoch_split(next.epoch_budget_udrt).block(n, offset)?;
    next.issued_in_epoch = next.issued_in_epoch.checked_add(&block_pools)?;
    next.total_issued = next.total_issued.checked_add(&block_pools)?;
    next.last_height = height;
    next.validate_internal()?;
    writes.insert(TIMING_STATE_KEY.as_bytes().to_vec(), next.encode()?);
    Ok(PlannedIssuance {
        next_state: next,
        block_pools,
        journal,
        writes,
    })
}
fn selected(key: &[u8]) -> bool {
    key.starts_with(b"issuance:")
        || key.starts_with(b"adaptive:")
        || [
            b"genesis:monetary:v1".as_slice(),
            b"meta:chain_id",
            b"emission:last_height",
        ]
        .contains(&key)
}
/// Validate an immutable combined view. This checks internal history, not observation authenticity.
pub fn verify_overlay(values: &BTreeMap<Vec<u8>, Vec<u8>>) -> Result<TimingState> {
    let state = TimingState::decode(
        values
            .get(TIMING_STATE_KEY.as_bytes())
            .context("Missing issuance timing state")?,
    )?;
    let marker = values
        .get(b"genesis:monetary:v1".as_slice())
        .context("Missing monetary genesis binding")?;
    ensure!(
        marker.len() == 33 && marker[0] == 1 && state.genesis_digest == hex::encode(&marker[1..]),
        "Issuance genesis binding differs"
    );
    ensure!(
        values.get(b"meta:chain_id".as_slice()).map(Vec::as_slice)
            == Some(state.chain_id.as_bytes()),
        "Issuance chain binding differs"
    );
    let height = match values.get(b"emission:last_height".as_slice()) {
        Some(raw) => u64::from_be_bytes(
            raw.as_slice()
                .try_into()
                .context("Invalid emission height")?,
        ),
        None => 0,
    };
    ensure!(
        state.last_height == height,
        "Issuance and emission heights differ"
    );
    let journal = verify_view(
        values,
        state.binding,
        &state.config.controller_config()?,
        state.config.max_recorded_epochs,
    )?;
    ensure!(
        u64::try_from(journal.commands.len())? == state.active_epoch
            && journal.snapshot.last_epoch == state.active_epoch.checked_sub(1),
        "Controller journal differs from active issuance epoch"
    );
    let mut observation_count = 0u64;
    for key in values.keys().filter(|k| k.starts_with(b"issuance:")) {
        if key.as_slice() == TIMING_STATE_KEY.as_bytes() {
            continue;
        }
        ensure!(
            key.starts_with(OBSERVATION_PREFIX) && key.len() == OBSERVATION_PREFIX.len() + 8,
            "Unsupported issuance record"
        );
        observation_count = observation_count
            .checked_add(1)
            .context("Observation count overflow")?;
    }
    ensure!(
        observation_count == state.active_epoch,
        "Observation record count differs from journal"
    );
    let mut total = PoolAmounts::default();
    if state.active_epoch > 0 {
        total = PoolAmounts::epoch_split(state.config.initial_epoch_budget_udrt);
    }
    let mut expected_budget = state.config.initial_epoch_budget_udrt;
    for record in &journal.commands {
        let obs = decode_observation(
            values
                .get(&observation_key(record.observation.epoch))
                .context("Missing issuance observation")?,
        )?;
        observation_range(&obs, state.config.epoch_blocks)?;
        ensure!(
            obs.epoch == record.observation.epoch
                && obs.utilization_ppm == record.observation.utilization_ppm
                && obs.volatility_ppm == record.observation.volatility_ppm,
            "Observation differs from controller journal"
        );
        if record.for_epoch < state.active_epoch {
            total = total.checked_add(&PoolAmounts::epoch_split(record.emission_udrt))?;
        } else {
            expected_budget = record.emission_udrt;
        }
    }
    ensure!(
        state.epoch_budget_udrt == expected_budget,
        "Active command budget differs"
    );
    total = total.checked_add(&state.issued_in_epoch)?;
    ensure!(
        state.total_issued == total,
        "Cumulative issuance differs from command prefix"
    );
    Ok(state)
}
