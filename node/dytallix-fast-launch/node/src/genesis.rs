//! One-time genesis monetary initialization for the selected node.
//! Explicit adaptive issuance is available only for fresh development state.
//! This does not activate distributed consensus or production issuance.
use crate::runtime::issuance_timing::{TimingGenesis, TimingState, TIMING_STATE_KEY};
use crate::runtime::reward_runtime::{
    RewardConfig, RewardState, ValidatorStatus, VestingLock, REWARD_STATE_KEY,
};
use crate::runtime::staking::{delegator_key, DelegatorRewardRecord, TOTAL_STAKE_KEY};
use crate::state::{DGT_MAX_SUPPLY, DGT_MINTED_KEY};
use crate::storage::state::Storage;
use anyhow::{bail, ensure, Context, Result};
use dytallix_protocol_types::units::{DGT_BASE_DENOM, DRT_BASE_DENOM};
use rocksdb::{IteratorMode, WriteBatch, WriteOptions};
use serde::de::{Error, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const GENESIS_KEY: &[u8] = b"genesis:monetary:v1";
const DRT_INITIAL_KEY: &[u8] = b"supply:drt_genesis";

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(try_from = "String")]
struct Amount(u128);
impl TryFrom<String> for Amount {
    type Error = &'static str;
    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("Genesis amount must be a decimal digit string");
        }
        value
            .parse()
            .map(Self)
            .map_err(|_| "Genesis amount exceeds u128")
    }
}

fn balances<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<BTreeMap<String, Amount>, D::Error> {
    struct BalanceVisitor;
    impl<'de> Visitor<'de> for BalanceVisitor {
        type Value = BTreeMap<String, Amount>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("unique native denomination balances")
        }
        fn visit_map<M: MapAccess<'de>>(
            self,
            mut map: M,
        ) -> std::result::Result<Self::Value, M::Error> {
            let mut result = BTreeMap::new();
            while let Some((denom, amount)) = map.next_entry::<String, Amount>()? {
                if result.insert(denom, amount).is_some() {
                    return Err(M::Error::custom("Duplicate genesis denomination"));
                }
            }
            Ok(result)
        }
    }
    deserializer.deserialize_map(BalanceVisitor)
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum VestingInput {
    Unlocked {},
    LinearAfterCliff {
        total_amount: Amount,
        start_time: u64,
        cliff_duration: u64,
        vesting_duration: u64,
        allow_staking: bool,
    },
}

fn explicit_vesting<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<VestingInput>, D::Error> {
    // A supplied value must be an explicit supported object. In particular, null
    // must not become the compatibility default for an absent field.
    VestingInput::deserialize(deserializer).map(Some)
}

#[derive(Deserialize)]
struct Account {
    address: String,
    #[serde(deserialize_with = "balances")]
    balances: BTreeMap<String, Amount>,
    // Missing input retains development-fixture compatibility only.
    #[serde(default, deserialize_with = "explicit_vesting")]
    vesting: Option<VestingInput>,
}
#[derive(Deserialize)]
struct Delegation {
    delegator: String,
    amount_udgt: Amount,
}
#[derive(Default, Deserialize)]
struct InitialStake {
    #[serde(default)]
    delegations: Vec<Delegation>,
    user_delegation: Option<Delegation>,
}
#[derive(Deserialize)]
struct Document {
    chain_id: String,
    accounts: Vec<Account>,
    staking: Option<InitialStake>,
    #[serde(default, deserialize_with = "explicit_reward_v2")]
    reward_v2: Option<RewardGenesisInput>,
    #[serde(default, deserialize_with = "explicit_adaptive_issuance")]
    adaptive_issuance: Option<TimingGenesis>,
}

fn explicit_adaptive_issuance<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<TimingGenesis>, D::Error> {
    TimingGenesis::deserialize(deserializer).map(Some)
}

fn explicit_reward_v2<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<RewardGenesisInput>, D::Error> {
    RewardGenesisInput::deserialize(deserializer).map(Some)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RewardGenesisInput {
    version: u32,
    activation_height: u64,
    decimals: u8,
    profile: String,
    max_validators: usize,
    max_positions: usize,
    validators: Vec<RewardValidatorInput>,
    positions: Vec<RewardPositionInput>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RewardValidatorInput {
    address: String,
    active: bool,
    jailed: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RewardPositionInput {
    owner: String,
    validator: String,
    amount_udgt: Amount,
}

/// Prepared monetary state. Construction validates every account before any write.
struct AllocationPlan {
    balances: BTreeMap<String, BTreeMap<String, u128>>,
    delegations: BTreeMap<String, u128>,
    dgt_total: u128,
    drt_total: u128,
    stake_total: u128,
    locks: BTreeMap<String, VestingLock>,
    reward_v2: Option<RewardGenesisInput>,
    adaptive_issuance: Option<TimingGenesis>,
}
impl AllocationPlan {
    fn build(document: Document, chain_id: &str) -> Result<Self> {
        ensure!(
            document.chain_id == chain_id,
            "Genesis chain ID differs from configured chain ID"
        );
        ensure!(
            document.adaptive_issuance.is_none() || document.reward_v2.is_some(),
            "Adaptive issuance requires explicit reward v2 activation"
        );
        let mut plan = Self {
            balances: BTreeMap::new(),
            delegations: BTreeMap::new(),
            dgt_total: 0,
            drt_total: 0,
            stake_total: 0,
            locks: BTreeMap::new(),
            reward_v2: document.reward_v2,
            adaptive_issuance: document.adaptive_issuance,
        };
        for account in document.accounts {
            let allocation = account
                .balances
                .get(DGT_BASE_DENOM)
                .map(|amount| amount.0)
                .unwrap_or(0);
            match account.vesting {
                None => ensure!(
                    plan.reward_v2.is_none(),
                    "Reward v2 requires explicit vesting for every account"
                ),
                Some(VestingInput::Unlocked {}) => {}
                Some(VestingInput::LinearAfterCliff {
                    total_amount,
                    start_time,
                    cliff_duration,
                    vesting_duration,
                    allow_staking,
                }) => {
                    ensure!(
                        plan.reward_v2.is_some(),
                        "Locked vesting requires reward v2 state and lock enforcement"
                    );
                    ensure!(
                        total_amount.0 > 0 && total_amount.0 == allocation,
                        "Vesting amount must equal the account's DGT allocation"
                    );
                    ensure!(
                        cliff_duration < vesting_duration,
                        "Linear vesting end must follow the cliff"
                    );
                    start_time
                        .checked_add(vesting_duration)
                        .context("Vesting end exceeds u64")?;
                    plan.locks.insert(
                        account.address.clone(),
                        VestingLock {
                            total_amount: total_amount.0,
                            start_time,
                            cliff_duration,
                            vesting_duration,
                            permits_staking: allow_staking,
                        },
                    );
                }
            }
            ensure!(
                !account.address.is_empty()
                    && !account.address.chars().any(char::is_whitespace)
                    && !account.address.chars().any(char::is_control),
                "Invalid genesis account address"
            );
            ensure!(
                !plan.balances.contains_key(&account.address),
                "Duplicate genesis account"
            );
            let mut balances = BTreeMap::new();
            for (denom, amount) in account.balances {
                let total = match denom.as_str() {
                    DGT_BASE_DENOM => &mut plan.dgt_total,
                    DRT_BASE_DENOM => &mut plan.drt_total,
                    _ => bail!("Unsupported genesis denomination"),
                };
                *total = total
                    .checked_add(amount.0)
                    .context("Genesis denomination total exceeds u128")?;
                if amount.0 > 0 {
                    balances.insert(denom, amount.0);
                }
            }
            plan.balances.insert(account.address, balances);
        }
        ensure!(
            plan.dgt_total <= DGT_MAX_SUPPLY,
            "Genesis DGT allocation exceeds the existing supply cap"
        );
        let staking = document.staking.unwrap_or_default();
        for delegation in staking
            .delegations
            .into_iter()
            .chain(staking.user_delegation)
        {
            ensure!(
                !plan.delegations.contains_key(&delegation.delegator),
                "Duplicate genesis delegator"
            );
            let balances = plan
                .balances
                .get_mut(&delegation.delegator)
                .context("Genesis stake has no funding account")?;
            let balance = balances.get("udgt").copied().unwrap_or(0);
            let remaining = balance
                .checked_sub(delegation.amount_udgt.0)
                .context("Genesis stake exceeds its account's DGT allocation")?;
            if remaining == 0 {
                balances.remove("udgt");
            } else {
                balances.insert("udgt".into(), remaining);
            }
            plan.stake_total = plan
                .stake_total
                .checked_add(delegation.amount_udgt.0)
                .context("Genesis stake total exceeds u128")?;
            plan.delegations
                .insert(delegation.delegator, delegation.amount_udgt.0);
        }
        let liquid = plan
            .balances
            .values()
            .try_fold(0u128, |sum, balances| {
                sum.checked_add(balances.get("udgt").copied().unwrap_or(0))
            })
            .context("Genesis liquid total exceeds u128")?;
        ensure!(
            liquid.checked_add(plan.stake_total) == Some(plan.dgt_total),
            "Genesis DGT conservation check failed"
        );
        Ok(plan)
    }

    fn reward_state(&self, chain_id: &str, marker: &[u8]) -> Result<Option<RewardState>> {
        let Some(input) = &self.reward_v2 else {
            return Ok(None);
        };
        ensure!(
            input.version == 2 && input.decimals == 6 && input.activation_height == 1,
            "Reward v2 requires version 2, six decimals and activation height 1"
        );
        ensure!(
            input.profile == "development",
            "Production reward activation remains disabled"
        );
        let mut validators = BTreeMap::new();
        for validator in &input.validators {
            ensure!(
                validators
                    .insert(
                        validator.address.clone(),
                        ValidatorStatus {
                            active: validator.active,
                            jailed: validator.jailed,
                        }
                    )
                    .is_none(),
                "Duplicate reward validator"
            );
        }
        let mut positions: BTreeMap<String, BTreeMap<String, u128>> = BTreeMap::new();
        let mut owner_totals: BTreeMap<String, u128> = BTreeMap::new();
        for position in &input.positions {
            ensure!(
                position.amount_udgt.0 > 0,
                "Reward bonded position must be positive"
            );
            ensure!(
                self.balances.contains_key(&position.owner),
                "Reward position owner has no genesis allocation"
            );
            ensure!(
                validators.contains_key(&position.validator),
                "Reward position validator is missing"
            );
            if let Some(lock) = self.locks.get(&position.owner) {
                ensure!(
                    lock.permits_staking,
                    "Vesting policy does not permit bonded stake"
                );
            }
            ensure!(
                positions
                    .entry(position.owner.clone())
                    .or_default()
                    .insert(position.validator.clone(), position.amount_udgt.0)
                    .is_none(),
                "Duplicate reward owner-validator position"
            );
            let total = owner_totals.entry(position.owner.clone()).or_default();
            *total = total
                .checked_add(position.amount_udgt.0)
                .context("Reward owner stake exceeds u128")?;
        }
        ensure!(
            owner_totals == self.delegations,
            "Reward positions differ from funded genesis delegations"
        );
        let state = RewardState::new(
            RewardConfig {
                version: input.version,
                activation_height: input.activation_height,
                decimals: input.decimals,
                profile: input.profile.clone(),
                chain_id: chain_id.into(),
                genesis_digest: hex::encode(&marker[1..]),
                max_validators: input.max_validators,
                max_positions: input.max_positions,
            },
            validators,
            positions,
            self.locks.clone(),
        )?;
        Ok(Some(state))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Initialization {
    Created,
    Existing,
}

/// Call before creating cached state or staking objects. Exclusive storage access is required.
/// `None` preserves the empty development-store path. A supplied file must be valid.
pub fn initialize(
    storage: &mut Storage,
    chain_id: &str,
    bytes: Option<&[u8]>,
) -> Result<Initialization> {
    initialize_mode(storage, chain_id, bytes, None)
}

/// Initialize a fresh consensus store with its canonical configuration in the
/// monetary genesis batch. Existing development stores cannot change mode.
pub(crate) fn initialize_consensus(
    storage: &mut Storage,
    chain_id: &str,
    bytes: &[u8],
    config_bytes: &[u8],
) -> Result<Initialization> {
    ensure!(
        !config_bytes.is_empty() && config_bytes.len() <= 65_536,
        "Consensus configuration must contain 1 through 65536 bytes"
    );
    initialize_mode(storage, chain_id, Some(bytes), Some(config_bytes))
}

pub(crate) fn initialize_consensus_with_root(
    storage: &mut Storage,
    chain_id: &str,
    bytes: &[u8],
    config_bytes: &[u8],
    root: Option<&crate::root_genesis::PreparedRootGenesis>,
) -> Result<Initialization> {
    ensure!(
        !config_bytes.is_empty() && config_bytes.len() <= 65_536,
        "Consensus configuration size differs"
    );
    initialize_mode_with_root(
        storage,
        chain_id,
        Some(bytes),
        Some(config_bytes),
        root,
        |storage, batch| {
            let mut options = WriteOptions::default();
            options.set_sync(true);
            storage.db.write_opt(batch, &options)?;
            Ok(())
        },
    )
}

fn initialize_mode(
    storage: &mut Storage,
    chain_id: &str,
    bytes: Option<&[u8]>,
    consensus_config: Option<&[u8]>,
) -> Result<Initialization> {
    initialize_mode_with_root(
        storage,
        chain_id,
        bytes,
        consensus_config,
        None,
        |storage, batch| {
            let mut options = WriteOptions::default();
            options.set_sync(true);
            storage.db.write_opt(batch, &options)?;
            Ok(())
        },
    )
}

pub(crate) fn initialize_mode_with_root(
    storage: &mut Storage,
    chain_id: &str,
    bytes: Option<&[u8]>,
    consensus_config: Option<&[u8]>,
    root: Option<&crate::root_genesis::PreparedRootGenesis>,
    write: impl FnOnce(&Storage, WriteBatch) -> Result<()>,
) -> Result<Initialization> {
    ensure!(
        root.is_none() || consensus_config.is_some(),
        "Root genesis requires consensus configuration"
    );
    let execution_guard = storage.lock_execution()?;
    let mode_key = crate::consensus_settlement::MODE_KEY;
    if consensus_config.is_none() {
        reject_qualified_state(storage)?;
    }
    ensure!(!chain_id.trim().is_empty(), "Chain ID must be nonempty");
    let document = match bytes {
        Some(bytes) => {
            serde_json::from_slice(bytes).context("Invalid genesis monetary document")?
        }
        None => Document {
            chain_id: chain_id.into(),
            accounts: Vec::new(),
            staking: None,
            reward_v2: None,
            adaptive_issuance: None,
        },
    };
    let plan = AllocationPlan::build(document, chain_id)?;
    let mut hash = Sha256::new();
    hash.update(b"dytallix-selected-node-genesis-monetary-v1");
    hash.update((chain_id.len() as u64).to_be_bytes());
    hash.update(chain_id.as_bytes());
    match bytes {
        Some(bytes) => {
            hash.update([1]);
            hash.update(bytes);
        }
        None => hash.update([0]),
    }
    let mut marker = vec![1u8];
    marker.extend_from_slice(&hash.finalize());
    let reward_state = plan.reward_state(chain_id, &marker)?;
    let timing_state = plan
        .adaptive_issuance
        .as_ref()
        .map(|config| TimingState::new(config.clone(), chain_id.into(), hex::encode(&marker[1..])))
        .transpose()?;
    if let Some(stored) = storage.db.get(GENESIS_KEY)? {
        crate::root_genesis::check_receipt(
            storage.db.get(crate::root_genesis::STATE_KEY)?.as_deref(),
            root,
        )?;
        ensure!(
            storage.db.get(mode_key)?.as_deref() == consensus_config,
            "Consensus mode or configuration differs; automatic mode activation is prohibited"
        );
        ensure!(
            stored == marker,
            "Genesis source differs from initialized storage"
        );
        ensure!(
            storage.db.get("meta:chain_id")?.as_deref() == Some(chain_id.as_bytes()),
            "Stored chain ID differs or is missing"
        );
        ensure!(
            crate::supply::genesis_amount(storage)? == plan.drt_total,
            "Stored DRT genesis counter differs from source allocation"
        );
        match (&reward_state, storage.db.get(REWARD_STATE_KEY)?) {
            (Some(expected), Some(encoded)) => {
                let stored = RewardState::decode(&encoded)?;
                ensure!(
                    stored.config == expected.config,
                    "Stored reward configuration differs from genesis"
                );
                reject_legacy_reward_liabilities(storage)?;
            }
            (Some(_), None) => {
                bail!("Reward v2 state is missing; automatic legacy activation is prohibited")
            }
            (None, Some(_)) => bail!("Reward v2 state has no matching genesis activation"),
            (None, None) => {}
        }
        verify_existing_timing(storage, timing_state.as_ref())?;
        return Ok(Initialization::Existing);
    }
    if let Some(entry) = storage.db.iterator(IteratorMode::Start).next() {
        entry?;
        bail!("Existing storage has no monetary genesis marker; explicit migration is required");
    }
    let mut batch = WriteBatch::default();
    if let Some(config) = consensus_config {
        batch.put(mode_key, config);
    }
    if let Some(state) = &timing_state {
        let prepared = dytallix_storage::adaptive::prepare_initialize(
            storage,
            state.binding,
            state.config.controller_config()?,
        )?;
        prepared.append_checked(storage, &execution_guard, &mut batch)?;
        batch.put(TIMING_STATE_KEY, state.encode()?);
    }
    for (address, balances) in plan.balances {
        batch.put(
            format!("acct:balances:{address}"),
            bincode::serialize(&balances)?,
        );
    }
    for (address, stake_amount) in plan.delegations {
        let record = DelegatorRewardRecord {
            stake_amount,
            ..Default::default()
        };
        batch.put(delegator_key(&address), bincode::serialize(&record)?);
    }
    batch.put(DGT_MINTED_KEY, bincode::serialize(&plan.dgt_total)?);
    batch.put(DRT_INITIAL_KEY, bincode::serialize(&plan.drt_total)?);
    batch.put(TOTAL_STAKE_KEY, bincode::serialize(&plan.stake_total)?);
    batch.put("meta:chain_id", chain_id.as_bytes());
    if let Some(state) = reward_state {
        batch.put(REWARD_STATE_KEY, state.encode()?);
    }
    batch.put(GENESIS_KEY, marker);
    if let Some(root) = root {
        batch.put(crate::root_genesis::STATE_KEY, root.bytes());
    }
    if consensus_config.is_some() {
        crate::consensus_settlement::append_genesis_anchor(
            storage,
            &mut batch,
            bytes.context("Consensus genesis requires an explicit source")?,
        )?;
    }
    write(storage, batch)?;
    Ok(Initialization::Created)
}

/// Presence of any consensus record excludes every development mutator.
/// Unknown records require recovery; a missing mode key never enables mutation.
pub(crate) fn reject_consensus_state(storage: &Storage) -> Result<()> {
    for prefix in [b"consensus:".as_slice(), b"root:authorization:"] {
        if let Some(entry) = storage
            .db
            .iterator(IteratorMode::From(prefix, rocksdb::Direction::Forward))
            .next()
        {
            let (key, _) = entry?;
            ensure!(
                !key.starts_with(prefix),
                "Consensus state requires the consensus adapter; development mutation is disabled"
            );
        }
    }
    Ok(())
}

/// Legacy external routes cannot admit or mutate a qualified account store.
/// Prefix presence is sufficient: corrupt or orphan records never enable fallback.
/// This check does not parse state or authorize a qualified transaction.
pub(crate) fn reject_qualified_state(storage: &Storage) -> Result<()> {
    for prefix in [
        b"consensus:".as_slice(),
        b"recovery:",
        b"ordinary:",
        b"root:authorization:",
    ] {
        if let Some(entry) = storage
            .db
            .iterator(IteratorMode::From(prefix, rocksdb::Direction::Forward))
            .next()
        {
            let (key, _) = entry?;
            ensure!(!key.starts_with(prefix),
                "Qualified account state requires consensus admission; legacy and direct mutation routes are disabled");
        }
    }
    Ok(())
}

fn verify_existing_timing(storage: &Storage, expected: Option<&TimingState>) -> Result<()> {
    let mut values = BTreeMap::new();
    let record_limit = match expected {
        Some(state) => state
            .config
            .max_recorded_epochs
            .checked_mul(2)
            .and_then(|limit| limit.checked_add(2))
            .context("Issuance history limit exceeds u64")?,
        None => 0,
    };
    let mut count = 0u64;
    for prefix in [b"adaptive:".as_slice(), b"issuance:".as_slice()] {
        for entry in storage
            .db
            .iterator(IteratorMode::From(prefix, rocksdb::Direction::Forward))
        {
            let (key, value) = entry?;
            if !key.starts_with(prefix) {
                break;
            }
            ensure!(
                expected.is_some(),
                "Adaptive or issuance state has no matching genesis activation"
            );
            count = count
                .checked_add(1)
                .context("Issuance record count exceeds u64")?;
            ensure!(
                count <= record_limit,
                "Issuance history exceeds its configured bound"
            );
            if key.starts_with(b"adaptive:") {
                ensure!(
                    key.as_ref() == b"adaptive:v1:head" || key.starts_with(b"adaptive:v1:event:"),
                    "Unknown adaptive journal record"
                );
            }
            values.insert(key.to_vec(), value.to_vec());
        }
    }
    let Some(expected) = expected else {
        return Ok(());
    };
    for key in [GENESIS_KEY, b"meta:chain_id", b"emission:last_height"] {
        if let Some(value) = storage.db.get(key)? {
            values.insert(key.to_vec(), value);
        }
    }
    let stored = crate::runtime::issuance_timing::verify_overlay(&values)?;
    ensure!(
        stored.config == expected.config
            && stored.chain_id == expected.chain_id
            && stored.genesis_digest == expected.genesis_digest
            && stored.binding == expected.binding,
        "Stored adaptive issuance configuration differs from genesis"
    );
    Ok(())
}

fn reject_legacy_reward_liabilities(storage: &Storage) -> Result<()> {
    for key in [
        "staking:reward_index",
        "staking:pending_emission",
        "staking:reward_residual",
    ] {
        if let Some(bytes) = storage.db.get(key)? {
            let amount: u128 =
                bincode::deserialize(&bytes).context("Invalid legacy reward scalar")?;
            ensure!(
                amount == 0,
                "Legacy reward liabilities require an explicit migration"
            );
        }
    }
    let prefix = b"staking:delegator:";
    for entry in storage
        .db
        .iterator(IteratorMode::From(prefix, rocksdb::Direction::Forward))
    {
        let (key, bytes) = entry?;
        if !key.starts_with(prefix) {
            break;
        }
        let record: DelegatorRewardRecord =
            bincode::deserialize(&bytes).context("Invalid legacy delegator reward record")?;
        ensure!(
            record.accrued_rewards == 0 && record.last_reward_index == 0,
            "Legacy delegator reward liabilities require an explicit migration"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{runtime::staking::StakingModule, state::State};
    use dytallix_protocol_types::units::{DGT_SCALE, DGT_TOTAL_BASE_UNITS};
    use std::sync::Arc;
    #[test]
    fn consensus_genesis_mode_is_atomic_and_exact_on_reopen() {
        let (_dir, mut storage) = empty();
        let config = b"canonical-consensus-fixture";
        assert_eq!(
            initialize_consensus(&mut storage, "test", SOURCE, config).unwrap(),
            Initialization::Created
        );
        assert_eq!(
            storage
                .db
                .get(crate::consensus_settlement::MODE_KEY)
                .unwrap()
                .as_deref(),
            Some(config.as_slice())
        );
        assert!(storage.db.get(GENESIS_KEY).unwrap().is_some());
        assert_eq!(crate::supply::genesis_amount(&storage).unwrap(), 9);
        assert_eq!(
            storage
                .db
                .get("consensus:v1:genesis_source")
                .unwrap()
                .as_deref(),
            Some(SOURCE)
        );
        let root = storage
            .db
            .get("consensus:v1:genesis_app_hash")
            .unwrap()
            .unwrap();
        assert!(!root.is_empty() && root.is_ascii());
        let before = snapshot(&storage);
        assert_eq!(
            initialize_consensus(&mut storage, "test", SOURCE, config).unwrap(),
            Initialization::Existing
        );
        assert!(initialize_consensus(&mut storage, "test", SOURCE, b"changed").is_err());
        assert!(initialize(&mut storage, "test", Some(SOURCE)).is_err());
        assert_eq!(snapshot(&storage), before);
    }
    #[test]
    fn consensus_genesis_rejects_invalid_input_without_partial_state() {
        for (source, config) in [
            (b"invalid".as_slice(), b"config".as_slice()),
            (SOURCE, b"".as_slice()),
        ] {
            let (_dir, mut storage) = empty();
            assert!(initialize_consensus(&mut storage, "test", source, config).is_err());
            assert!(snapshot(&storage).is_empty());
        }
        let (_dir, mut storage) = empty();
        assert!(initialize_consensus(&mut storage, "test", SOURCE, &vec![1; 65_537]).is_err());
        assert!(snapshot(&storage).is_empty());
    }
    #[test]
    fn consensus_genesis_cannot_activate_existing_development_or_orphan_store() {
        let (_dir, mut storage) = empty();
        initialize(&mut storage, "test", Some(SOURCE)).unwrap();
        let before = snapshot(&storage);
        assert!(initialize_consensus(&mut storage, "test", SOURCE, b"config").is_err());
        assert_eq!(snapshot(&storage), before);
        for key in [
            crate::consensus_settlement::MODE_KEY,
            "consensus:unknown:orphan",
        ] {
            let (_dir, mut storage) = empty();
            storage.db.put(key, b"orphan").unwrap();
            let before = snapshot(&storage);
            assert!(initialize(&mut storage, "test", Some(SOURCE)).is_err());
            assert!(initialize_consensus(&mut storage, "test", SOURCE, b"config").is_err());
            assert_eq!(snapshot(&storage), before);
        }
    }
    const SOURCE: &[u8] = br#"{"chain_id":"test","_comment":"fixture","accounts":[{"address":"alice","balances":{"udgt":"100","udrt":"9"}},{"address":"bob","balances":{"udgt":"50"}}],"staking":{"delegations":[{"delegator":"alice","amount_udgt":"40"}],"user_delegation":{"delegator":"bob","amount_udgt":"10"}}}"#;
    type StoredEntry = (Box<[u8]>, Box<[u8]>);
    fn snapshot(storage: &Storage) -> Vec<StoredEntry> {
        storage
            .db
            .iterator(IteratorMode::Start)
            .collect::<std::result::Result<_, _>>()
            .unwrap()
    }
    fn empty() -> (tempfile::TempDir, Storage) {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path().join("db")).unwrap();
        (dir, storage)
    }
    fn reject(bytes: &[u8]) {
        let (_dir, mut storage) = empty();
        assert!(initialize(&mut storage, "test", Some(bytes)).is_err());
        assert!(snapshot(&storage).is_empty(), "invalid genesis wrote state");
    }

    fn reward_source() -> serde_json::Value {
        let mut source: serde_json::Value = serde_json::from_slice(SOURCE).unwrap();
        source["accounts"][0]["vesting"] = serde_json::json!({
            "kind": "linear_after_cliff", "total_amount": "100", "start_time": 0,
            "cliff_duration": 10, "vesting_duration": 30, "allow_staking": true,
        });
        source["accounts"][1]["vesting"] = serde_json::json!({"kind": "unlocked"});
        source["reward_v2"] = serde_json::json!({
            "version": 2, "activation_height": 1, "decimals": 6,
            "profile": "development", "max_validators": 64, "max_positions": 128,
            "validators": [{"address": "fixture-validator", "active": true, "jailed": false}],
            "positions": [
                {"owner": "alice", "validator": "fixture-validator", "amount_udgt": "40"},
                {"owner": "bob", "validator": "fixture-validator", "amount_udgt": "10"},
            ],
        });
        source
    }

    fn issuance_source() -> serde_json::Value {
        let mut source = reward_source();
        source["adaptive_issuance"] = serde_json::json!({
            "version": 1,
            "profile": "development",
            "decimals": 6,
            "epoch_blocks": 3,
            "initial_epoch_budget_udrt": "1000",
            "max_recorded_epochs": 16,
            "controller": {
                "target_ppm": 500000,
                "shock_threshold_ppm": 200000,
                "volatility_threshold_ppm": 300000,
                "window_samples": 2,
                "integral_min": -2000000,
                "integral_max": 2000000,
                "soft": {"proportional": 1000, "integral": 0, "derivative": 0},
                "hard": {"proportional": 2000, "integral": 0, "derivative": 0},
                "base_udrt": 1000,
                "min_udrt": 500,
                "max_udrt": 2000,
            },
        });
        source
    }

    #[test]
    fn adaptive_genesis_initializes_one_atomic_bound_state_and_reopens_exactly() {
        let source = serde_json::to_vec(&issuance_source()).unwrap();
        let (dir, mut storage) = empty();
        assert_eq!(
            initialize(&mut storage, "test", Some(&source)).unwrap(),
            Initialization::Created
        );
        let timing =
            TimingState::decode(&storage.db.get(TIMING_STATE_KEY).unwrap().unwrap()).unwrap();
        let reward =
            RewardState::decode(&storage.db.get(REWARD_STATE_KEY).unwrap().unwrap()).unwrap();
        let marker = storage.db.get(GENESIS_KEY).unwrap().unwrap();
        assert_eq!(timing.genesis_digest, hex::encode(&marker[1..]));
        assert_eq!(timing.genesis_digest, reward.config.genesis_digest);
        assert_eq!(timing.chain_id, "test");
        assert_eq!(timing.last_height, 0);
        assert_eq!(timing.epoch_budget_udrt, 1000);
        assert!(storage.db.get("adaptive:v1:head").unwrap().is_some());
        assert_eq!(crate::supply::genesis_amount(&storage).unwrap(), 9);
        let before = snapshot(&storage);
        drop(storage);
        let mut storage = Storage::open(dir.path().join("db")).unwrap();
        assert_eq!(
            initialize(&mut storage, "test", Some(&source)).unwrap(),
            Initialization::Existing
        );
        assert_eq!(snapshot(&storage), before);
    }

    #[test]
    fn adaptive_genesis_requires_every_timing_and_controller_input() {
        let base = issuance_source();
        let fields: Vec<_> = base["adaptive_issuance"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        for field in fields {
            let mut source = base.clone();
            source["adaptive_issuance"]
                .as_object_mut()
                .unwrap()
                .remove(&field);
            reject(&serde_json::to_vec(&source).unwrap());
        }
        let fields: Vec<_> = base["adaptive_issuance"]["controller"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        for field in fields {
            let mut source = base.clone();
            source["adaptive_issuance"]["controller"]
                .as_object_mut()
                .unwrap()
                .remove(&field);
            reject(&serde_json::to_vec(&source).unwrap());
        }
        let mut source = base.clone();
        source.as_object_mut().unwrap().remove("reward_v2");
        reject(&serde_json::to_vec(&source).unwrap());
        let mut source = base;
        source["adaptive_issuance"] = serde_json::Value::Null;
        reject(&serde_json::to_vec(&source).unwrap());
    }

    #[test]
    fn adaptive_genesis_rejects_malformed_limits_budgets_and_production_profile() {
        for (field, value) in [
            ("version", serde_json::json!(2)),
            ("profile", serde_json::json!("mainnet")),
            ("decimals", serde_json::json!(18)),
            ("epoch_blocks", serde_json::json!(0)),
            ("initial_epoch_budget_udrt", serde_json::json!("499")),
            ("initial_epoch_budget_udrt", serde_json::json!("2001")),
            ("initial_epoch_budget_udrt", serde_json::json!(1000)),
            ("initial_epoch_budget_udrt", serde_json::json!("1.5")),
            (
                "initial_epoch_budget_udrt",
                serde_json::json!("18446744073709551616"),
            ),
            ("max_recorded_epochs", serde_json::json!(0)),
            ("max_recorded_epochs", serde_json::json!(1000001)),
            ("unrecognized", serde_json::json!(true)),
        ] {
            let mut source = issuance_source();
            source["adaptive_issuance"][field] = value;
            reject(&serde_json::to_vec(&source).unwrap());
        }
    }

    #[test]
    fn adaptive_genesis_rejects_legacy_activation_and_orphaned_namespaces() {
        let legacy_source = serde_json::to_vec(&reward_source()).unwrap();
        let source = serde_json::to_vec(&issuance_source()).unwrap();
        let (_dir, mut storage) = empty();
        initialize(&mut storage, "test", Some(&legacy_source)).unwrap();
        let before = snapshot(&storage);
        assert!(initialize(&mut storage, "test", Some(&source)).is_err());
        assert_eq!(snapshot(&storage), before);

        for key in [
            "adaptive:v1:head",
            "adaptive:v1:event:orphan",
            "issuance:v1:state",
            "issuance:unknown:orphan",
        ] {
            let (_dir, mut storage) = empty();
            initialize(&mut storage, "test", Some(SOURCE)).unwrap();
            storage.db.put(key, b"orphan").unwrap();
            let before = snapshot(&storage);
            assert!(initialize(&mut storage, "test", Some(SOURCE)).is_err());
            assert_eq!(snapshot(&storage), before);
        }
    }

    #[test]
    fn adaptive_genesis_reopen_rejects_missing_or_corrupt_timing_and_journal() {
        let source = serde_json::to_vec(&issuance_source()).unwrap();
        for key in [TIMING_STATE_KEY, "adaptive:v1:head"] {
            let (_dir, mut storage) = empty();
            initialize(&mut storage, "test", Some(&source)).unwrap();
            storage.db.delete(key).unwrap();
            let before = snapshot(&storage);
            assert!(initialize(&mut storage, "test", Some(&source)).is_err());
            assert_eq!(snapshot(&storage), before);
        }
        for key in [
            TIMING_STATE_KEY,
            "adaptive:v1:head",
            "adaptive:v1:event:orphan",
            "issuance:unknown:orphan",
        ] {
            let (_dir, mut storage) = empty();
            initialize(&mut storage, "test", Some(&source)).unwrap();
            storage.db.put(key, b"corrupt").unwrap();
            let before = snapshot(&storage);
            assert!(initialize(&mut storage, "test", Some(&source)).is_err());
            assert_eq!(snapshot(&storage), before);
        }
    }

    #[test]
    fn reward_v2_genesis_binds_funded_positions_locks_and_identity_atomically() {
        let source = serde_json::to_vec(&reward_source()).unwrap();
        let (dir, mut storage) = empty();
        assert_eq!(
            initialize(&mut storage, "test", Some(&source)).unwrap(),
            Initialization::Created
        );
        let reward =
            RewardState::decode(&storage.db.get(REWARD_STATE_KEY).unwrap().unwrap()).unwrap();
        assert_eq!(reward.config.chain_id, "test");
        let marker = storage.db.get(GENESIS_KEY).unwrap().unwrap();
        assert_eq!(reward.config.genesis_digest, hex::encode(&marker[1..]));
        assert_eq!(reward.config.version, 2);
        assert_eq!(reward.config.activation_height, 1);
        assert_eq!(reward.config.decimals, 6);
        assert_eq!(reward.positions["alice"]["fixture-validator"], 40);
        assert_eq!(reward.positions["bob"]["fixture-validator"], 10);
        assert_eq!(reward.locks["alice"].total_amount, 100);
        assert!(reward.locks["alice"].permits_staking);
        assert!(!reward.locks.contains_key("bob"));
        let state = State::new(Arc::new(storage));
        assert_eq!(state.get_balance("alice", "udgt"), 60);
        assert_eq!(state.get_balance("bob", "udgt"), 40);
        assert_eq!(state.dgt_total_minted(), 150);
        drop(state);
        let mut storage = Storage::open(dir.path().join("db")).unwrap();
        let before = snapshot(&storage);
        assert_eq!(
            initialize(&mut storage, "test", Some(&source)).unwrap(),
            Initialization::Existing
        );
        assert_eq!(snapshot(&storage), before);
    }

    #[test]
    fn reward_v2_rejects_unapproved_activation_and_invalid_limits_without_writes() {
        for (field, value) in [
            ("version", serde_json::json!(1)),
            ("activation_height", serde_json::json!(0)),
            ("activation_height", serde_json::json!(2)),
            ("decimals", serde_json::json!(18)),
            ("profile", serde_json::json!("mainnet")),
            ("max_validators", serde_json::json!(0)),
            ("max_positions", serde_json::json!(1)),
        ] {
            let mut source = reward_source();
            source["reward_v2"][field] = value;
            reject(&serde_json::to_vec(&source).unwrap());
        }
        let mut source = reward_source();
        source["reward_v2"] = serde_json::Value::Null;
        reject(&serde_json::to_vec(&source).unwrap());
    }

    #[test]
    fn reward_v2_positions_must_match_funded_owner_stake_without_duplicates() {
        let base = reward_source();
        let mut cases = Vec::new();
        for (field, value) in [
            ("owner", "unknown"),
            ("validator", "unknown"),
            ("amount_udgt", "41"),
            ("amount_udgt", "0"),
        ] {
            let mut source = base.clone();
            source["reward_v2"]["positions"][0][field] = serde_json::json!(value);
            cases.push(source);
        }
        let mut source = base.clone();
        let position = source["reward_v2"]["positions"][0].clone();
        source["reward_v2"]["positions"]
            .as_array_mut()
            .unwrap()
            .push(position);
        cases.push(source);
        let mut source = base.clone();
        let validator = source["reward_v2"]["validators"][0].clone();
        source["reward_v2"]["validators"]
            .as_array_mut()
            .unwrap()
            .push(validator);
        cases.push(source);
        let mut source = base;
        source["reward_v2"]["positions"]
            .as_array_mut()
            .unwrap()
            .pop();
        cases.push(source);
        for source in cases {
            reject(&serde_json::to_vec(&source).unwrap());
        }
    }

    #[test]
    fn reward_v2_requires_explicit_checked_vesting_and_permission_to_bond() {
        for (field, value) in [
            ("total_amount", serde_json::json!("99")),
            ("cliff_duration", serde_json::json!(30)),
            ("start_time", serde_json::json!(u64::MAX)),
            ("allow_staking", serde_json::json!(false)),
        ] {
            let mut source = reward_source();
            source["accounts"][0]["vesting"][field] = value;
            reject(&serde_json::to_vec(&source).unwrap());
        }
        let mut source = reward_source();
        source["accounts"][1]
            .as_object_mut()
            .unwrap()
            .remove("vesting");
        reject(&serde_json::to_vec(&source).unwrap());
        // An explicit lock that forbids staking can be imported if no principal bonds.
        let mut source = reward_source();
        source["accounts"][0]["vesting"]["allow_staking"] = serde_json::json!(false);
        source["staking"]["delegations"] = serde_json::json!([]);
        source["reward_v2"]["positions"]
            .as_array_mut()
            .unwrap()
            .remove(0);
        let source = serde_json::to_vec(&source).unwrap();
        let (_dir, mut storage) = empty();
        initialize(&mut storage, "test", Some(&source)).unwrap();
    }

    #[test]
    fn reward_v2_rejects_existing_legacy_activation_and_missing_or_changed_state() {
        let source = serde_json::to_vec(&reward_source()).unwrap();
        let (_dir, mut storage) = empty();
        initialize(&mut storage, "test", Some(SOURCE)).unwrap();
        let before = snapshot(&storage);
        assert!(initialize(&mut storage, "test", Some(&source)).is_err());
        assert_eq!(snapshot(&storage), before);

        let (_dir, mut storage) = empty();
        initialize(&mut storage, "test", Some(&source)).unwrap();
        let original = storage.db.get(REWARD_STATE_KEY).unwrap().unwrap();
        storage.db.delete(REWARD_STATE_KEY).unwrap();
        let before = snapshot(&storage);
        assert!(initialize(&mut storage, "test", Some(&source)).is_err());
        assert_eq!(snapshot(&storage), before);
        let mut reward = RewardState::decode(&original).unwrap();
        reward.config.genesis_digest = "00".repeat(32);
        storage
            .db
            .put(REWARD_STATE_KEY, reward.encode().unwrap())
            .unwrap();
        let before = snapshot(&storage);
        assert!(initialize(&mut storage, "test", Some(&source)).is_err());
        assert_eq!(snapshot(&storage), before);
    }

    #[test]
    fn reward_v2_reopen_rejects_legacy_reward_liabilities_without_mutation() {
        let source = serde_json::to_vec(&reward_source()).unwrap();
        for key in [
            "staking:reward_index",
            "staking:pending_emission",
            "staking:reward_residual",
        ] {
            let (_dir, mut storage) = empty();
            initialize(&mut storage, "test", Some(&source)).unwrap();
            storage
                .db
                .put(key, bincode::serialize(&1u128).unwrap())
                .unwrap();
            let before = snapshot(&storage);
            assert!(initialize(&mut storage, "test", Some(&source)).is_err());
            assert_eq!(snapshot(&storage), before);
        }
        for index in [false, true] {
            let (_dir, mut storage) = empty();
            initialize(&mut storage, "test", Some(&source)).unwrap();
            let mut record: DelegatorRewardRecord =
                bincode::deserialize(&storage.db.get(delegator_key("alice")).unwrap().unwrap())
                    .unwrap();
            if index {
                record.last_reward_index = 1;
            } else {
                record.accrued_rewards = 1;
            }
            storage
                .db
                .put(delegator_key("alice"), bincode::serialize(&record).unwrap())
                .unwrap();
            let before = snapshot(&storage);
            assert!(initialize(&mut storage, "test", Some(&source)).is_err());
            assert_eq!(snapshot(&storage), before);
        }
    }

    #[test]
    fn six_decimal_native_fixture_import_and_reopen_preserve_allocations() {
        // Synthetic recipients are unlocked in this test only. This fixture does
        // not supply production beneficiaries, custody records or vesting terms.
        let allocations = [
            ("fixture-ecosystem", "300000000", 300_000_000_000_000u128),
            ("fixture-team", "200000000", 200_000_000_000_000),
            ("fixture-public", "150000000", 150_000_000_000_000),
            ("fixture-private", "150000000", 150_000_000_000_000),
            ("fixture-reserve", "200000000", 200_000_000_000_000),
        ];
        let accounts: Vec<_> = allocations
            .iter()
            .map(|(address, human, expected)| {
                let units = DGT_SCALE.to_base_units(human).unwrap();
                assert_eq!(units, *expected);
                serde_json::json!({
                    "address": address,
                    "balances": {"udgt": units.to_string()},
                    "vesting": {"kind": "unlocked"},
                })
            })
            .collect();
        let source = serde_json::to_vec(&serde_json::json!({
            "chain_id": "test",
            "_comment": "Synthetic beneficiaries. Explicitly unlocked fixture allocations only.",
            "accounts": accounts,
        }))
        .unwrap();
        assert_eq!(
            allocations.iter().map(|(_, _, units)| units).sum::<u128>(),
            1_000_000_000_000_000
        );
        assert_eq!(DGT_MAX_SUPPLY, DGT_TOTAL_BASE_UNITS);
        let (dir, mut storage) = empty();
        assert_eq!(
            initialize(&mut storage, "test", Some(&source)).unwrap(),
            Initialization::Created
        );
        let state = State::new(Arc::new(storage));
        for (address, _, expected) in allocations {
            assert_eq!(state.get_balance(address, "udgt"), expected);
        }
        assert_eq!(state.dgt_total_minted(), DGT_TOTAL_BASE_UNITS);
        drop(state);

        let mut storage = Storage::open(dir.path().join("db")).unwrap();
        initialize(&mut storage, "test", Some(&source)).unwrap();
        let state = State::new(Arc::new(storage));
        for (address, _, expected) in allocations {
            assert_eq!(state.get_balance(address, "udgt"), expected);
        }
        assert_eq!(state.dgt_total_minted(), DGT_TOTAL_BASE_UNITS);
    }

    #[test]
    fn native_genesis_rejects_legacy_totals_and_whole_token_aliases() {
        for amount in ["1000000000000000000", "1000000000000000000000000000"] {
            reject(format!(r#"{{"chain_id":"test","accounts":[{{"address":"fixture","balances":{{"udgt":"{amount}"}}}}]}}"#).as_bytes());
        }
        for denomination in ["DGT", "DRT", "dgt", "drt"] {
            reject(format!(r#"{{"chain_id":"test","accounts":[{{"address":"fixture","balances":{{"{denomination}":"1"}}}}]}}"#).as_bytes());
        }
    }

    #[test]
    fn native_genesis_rejects_unsupported_vesting_without_writes() {
        for vesting in [
            serde_json::json!({"kind": "linear", "start_time": 1, "end_time": 2}),
            serde_json::json!({"kind": "locked"}),
            serde_json::json!({"kind": "unknown"}),
            serde_json::json!({"kind": "unlocked", "cliff_duration": 1}),
            serde_json::json!({"kind": "unlocked", "schedule": {}}),
            serde_json::json!({}),
            serde_json::Value::Null,
            serde_json::json!(true),
        ] {
            let source = serde_json::to_vec(&serde_json::json!({
                "chain_id": "test",
                "accounts": [
                    {"address": "valid-first", "balances": {"udgt": "1"}, "vesting": {"kind": "unlocked"}},
                    {"address": "invalid-second", "balances": {"udgt": "1"}, "vesting": vesting},
                ],
            })).unwrap();
            reject(&source);
        }
    }
    #[test]
    fn funded_stake_and_supply_are_loaded_by_runtime() {
        let (_dir, mut storage) = empty();
        assert_eq!(
            initialize(&mut storage, "test", Some(SOURCE)).unwrap(),
            Initialization::Created
        );
        let storage = Arc::new(storage);
        let mut state = State::new(storage.clone());
        let staking = StakingModule::new(storage.clone());
        assert_eq!(state.balance_of("alice", "udgt"), 60);
        assert_eq!(state.balance_of("bob", "udgt"), 40);
        assert_eq!(state.balance_of("alice", "udrt"), 9);
        assert_eq!(staking.total_stake, 50);
        assert_eq!(staking.get_total_stake("alice"), 40);
        assert_eq!(staking.get_total_stake("bob"), 10);
        assert_eq!(state.dgt_total_minted(), 150);
        assert_eq!(
            bincode::deserialize::<u128>(&storage.db.get(DRT_INITIAL_KEY).unwrap().unwrap())
                .unwrap(),
            9
        );
        assert_eq!(staking.load_delegator_record("alice").accrued_rewards, 0);
    }
    #[test]
    fn read_only_balance_queries_load_persisted_genesis_without_warming_cache() {
        let (dir, mut storage) = empty();
        initialize(&mut storage, "test", Some(SOURCE)).unwrap();
        let storage = Arc::new(storage);
        let mut state = State::new(storage);
        assert_eq!(state.get_balance("alice", "udgt"), 60);
        assert_eq!(state.get_balance("alice", "udrt"), 9);
        assert_eq!(state.get_balance("unknown", "udgt"), 0);
        state.set_balance("alice", "udgt", 55);
        assert_eq!(state.get_balance("alice", "udgt"), 55);
        drop(state);
        let mut storage = Storage::open(dir.path().join("db")).unwrap();
        initialize(&mut storage, "test", Some(SOURCE)).unwrap();
        let state = State::new(Arc::new(storage));
        assert_eq!(state.get_balance("alice", "udgt"), 55);
        assert_eq!(state.get_balance("alice", "udrt"), 9);
    }

    #[test]
    fn reopen_preserves_post_genesis_state_byte_for_byte() {
        let (dir, mut storage) = empty();
        initialize(&mut storage, "test", Some(SOURCE)).unwrap();
        // Represent later committed execution without exercising unrelated mutation APIs.
        storage
            .set_balances_db("alice", &BTreeMap::from([("udgt".into(), 55u128)]))
            .unwrap();
        storage
            .db
            .put(DGT_MINTED_KEY, bincode::serialize(&155u128).unwrap())
            .unwrap();
        storage
            .db
            .put(TOTAL_STAKE_KEY, bincode::serialize(&45u128).unwrap())
            .unwrap();
        storage
            .db
            .put(
                delegator_key("alice"),
                bincode::serialize(&DelegatorRewardRecord {
                    stake_amount: 35,
                    last_reward_index: 7,
                    accrued_rewards: 8,
                })
                .unwrap(),
            )
            .unwrap();
        storage
            .db
            .put("staking:reward_index", bincode::serialize(&7u128).unwrap())
            .unwrap();
        let before = snapshot(&storage);
        drop(storage);
        let mut reopened = Storage::open(dir.path().join("db")).unwrap();
        assert_eq!(
            initialize(&mut reopened, "test", Some(SOURCE)).unwrap(),
            Initialization::Existing
        );
        assert_eq!(snapshot(&reopened), before);
    }
    #[test]
    fn malformed_late_accounts_and_amounts_leave_no_state() {
        for amount in [
            "\"-1\"",
            "\"+1\"",
            "\"1.5\"",
            "\"1e2\"",
            "\"\"",
            "100",
            "null",
            "\"340282366920938463463374607431768211456\"",
        ] {
            reject(format!(r#"{{"chain_id":"test","accounts":[{{"address":"alice","balances":{{"udgt":"1"}}}},{{"address":"bob","balances":{{"udgt":{amount}}}}}]}}"#).as_bytes());
        }
        reject(br#"{"chain_id":"test","accounts":[{"address":"alice","balances":{"udgt":"1"}},{"address":"bob","balances":{"other":"1"}}]}"#);
        reject(br#"{"chain_id":"test","accounts":[{"address":"bad address","balances":{}}]}"#);
    }
    #[test]
    fn duplicate_representations_are_rejected_before_write() {
        for source in [
            r#"{"chain_id":"test","accounts":[{"address":"a","balances":{}},{"address":"a","balances":{}}]}"#,
            r#"{"chain_id":"test","accounts":[{"address":"a","balances":{"udgt":"1","udgt":"2"}}]}"#,
            r#"{"chain_id":"test","accounts":[],"accounts":[]}"#,
            r#"{"chain_id":"test","accounts":[{"address":"a","balances":{"udgt":"10"}}],"staking":{"delegations":[{"delegator":"a","amount_udgt":"1"},{"delegator":"a","amount_udgt":"1"}]}}"#,
            r#"{"chain_id":"test","accounts":[{"address":"a","balances":{"udgt":"10"}}],"staking":{"delegations":[{"delegator":"a","amount_udgt":"1"}],"user_delegation":{"delegator":"a","amount_udgt":"1"}}}"#,
        ] {
            reject(source.as_bytes());
        }
    }
    #[test]
    fn cap_and_aggregate_overflow_are_rejected() {
        reject(format!(r#"{{"chain_id":"test","accounts":[{{"address":"a","balances":{{"udgt":"{}"}}}}]}}"#,DGT_MAX_SUPPLY+1).as_bytes());
        reject(format!(r#"{{"chain_id":"test","accounts":[{{"address":"a","balances":{{"udrt":"{}"}}}},{{"address":"b","balances":{{"udrt":"1"}}}}]}}"#,u128::MAX).as_bytes());
    }
    #[test]
    fn missing_or_insufficient_stake_funding_is_rejected() {
        reject(br#"{"chain_id":"test","accounts":[],"staking":{"user_delegation":{"delegator":"a","amount_udgt":"1"}}}"#);
        reject(br#"{"chain_id":"test","accounts":[{"address":"a","balances":{"udgt":"1"}}],"staking":{"user_delegation":{"delegator":"a","amount_udgt":"2"}}}"#);
    }
    #[test]
    fn changed_source_chain_and_legacy_store_preserve_state() {
        let (_dir, mut storage) = empty();
        initialize(&mut storage, "test", Some(SOURCE)).unwrap();
        let before = snapshot(&storage);
        let mut whitespace = SOURCE.to_vec();
        whitespace.push(b' ');
        assert!(initialize(&mut storage, "test", Some(&whitespace)).is_err());
        assert!(initialize(&mut storage, "other", Some(SOURCE)).is_err());
        assert!(initialize(&mut storage, "test", None).is_err());
        assert_eq!(snapshot(&storage), before);
        storage.db.delete(GENESIS_KEY).unwrap();
        let legacy = snapshot(&storage);
        assert!(initialize(&mut storage, "test", Some(SOURCE)).is_err());
        assert_eq!(snapshot(&storage), legacy);
    }
    #[test]
    fn zero_and_exact_cap_allocations_preserve_existing_mint_limit() {
        let (_dir, mut storage) = empty();
        let source = format!(
            r#"{{"chain_id":"test","accounts":[{{"address":"a","balances":{{"udgt":"{DGT_MAX_SUPPLY}","udrt":"0"}}}},{{"address":"b","balances":{{"udgt":"0"}}}}],"staking":{{"user_delegation":{{"delegator":"a","amount_udgt":"{DGT_MAX_SUPPLY}"}}}}}}"#,
        );
        initialize(&mut storage, "test", Some(source.as_bytes())).unwrap();
        let storage = Arc::new(storage);
        let mut state = State::new(storage.clone());
        assert_eq!(state.balance_of("a", "udgt"), 0);
        assert_eq!(state.dgt_total_minted(), DGT_MAX_SUPPLY);
        assert_eq!(StakingModule::new(storage).total_stake, DGT_MAX_SUPPLY);
        assert!(state.mint_dgt("a", 1).is_err());
        assert_eq!(state.balance_of("a", "udgt"), 0);
    }
    #[test]
    fn empty_development_store_is_initialized_once() {
        let (_dir, mut storage) = empty();
        assert_eq!(
            initialize(&mut storage, "test", None).unwrap(),
            Initialization::Created
        );
        let before = snapshot(&storage);
        assert_eq!(
            initialize(&mut storage, "test", None).unwrap(),
            Initialization::Existing
        );
        assert_eq!(snapshot(&storage), before);
    }
    #[test]
    fn checked_in_development_genesis_files_are_accepted() {
        for source in [
            include_bytes!("../../../deploy/genesis.json").as_slice(),
            include_bytes!("../../../deploy/genesis.dyt-local-1.json").as_slice(),
        ] {
            let (_dir, mut storage) = empty();
            initialize(&mut storage, "dyt-local-1", Some(source)).unwrap();
        }
    }
    #[test]
    fn restart_requires_the_original_drt_genesis_counter() {
        let (_dir, mut storage) = empty();
        initialize(&mut storage, "test", Some(SOURCE)).unwrap();
        storage
            .db
            .put(DRT_INITIAL_KEY, bincode::serialize(&10u128).unwrap())
            .unwrap();
        let before = snapshot(&storage);
        assert!(initialize(&mut storage, "test", Some(SOURCE))
            .unwrap_err()
            .to_string()
            .contains("DRT genesis counter"));
        assert_eq!(snapshot(&storage), before);
        storage.db.delete(DRT_INITIAL_KEY).unwrap();
        assert!(initialize(&mut storage, "test", Some(SOURCE)).is_err());
    }
}
