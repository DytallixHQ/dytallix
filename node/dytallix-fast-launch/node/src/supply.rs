//! Native-token custody accounting for the selected block lifecycle.
//! Unpaid rewards and both reserves remain inside staking-pool custody.
use crate::{
    block_lifecycle::{Deletes, Writes},
    runtime::governance_store::{self, Header as GovernanceHeader},
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
use serde::Serialize;
use std::collections::BTreeMap;

pub const DRT_GENESIS_KEY: &str = "supply:drt_genesis";
/// Cumulative uDRT burned (account creation fees). Absent means zero.
pub const DRT_BURNED_KEY: &str = "supply:drt_burned";
/// Running totals of liquid uDGT and uDRT over every `acct:balances:` record
/// (E04 gap 5). Consensus blocks update them from the balance records they
/// write, so per-block checks read no other account; the complete check
/// compares them with a scan. Written at consensus genesis.
pub(crate) const ACCOUNT_TOTALS_KEY: &str = "supply:account_totals";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize, Serialize)]
pub(crate) struct AccountTotals {
    pub udgt: u128,
    pub udrt: u128,
}
impl AccountTotals {
    /// The liquid amounts one canonical balance map holds.
    fn of_balances(raw: &[u8]) -> Result<Self> {
        let balances: BTreeMap<String, u128> =
            bincode::deserialize(raw).context("Invalid account balance map")?;
        ensure!(
            bincode::serialize(&balances)? == raw,
            "Noncanonical account balance map"
        );
        Ok(Self {
            udgt: balances.get("udgt").copied().unwrap_or(0),
            udrt: balances.get("udrt").copied().unwrap_or(0),
        })
    }
    fn add(self, other: Self) -> Result<Self> {
        Ok(Self {
            udgt: self
                .udgt
                .checked_add(other.udgt)
                .context("Liquid DGT supply exceeds u128")?,
            udrt: self
                .udrt
                .checked_add(other.udrt)
                .context("Liquid DRT supply exceeds u128")?,
        })
    }
    fn sub(self, other: Self) -> Result<Self> {
        Ok(Self {
            udgt: self
                .udgt
                .checked_sub(other.udgt)
                .context("Account totals below a removed balance")?,
            udrt: self
                .udrt
                .checked_sub(other.udrt)
                .context("Account totals below a removed balance")?,
        })
    }
    pub(crate) fn encode(&self) -> Result<Vec<u8>> {
        Ok(bincode::serialize(self)?)
    }
    pub(crate) fn decode(raw: &[u8]) -> Result<Self> {
        let totals: Self = bincode::deserialize(raw).context("Invalid account totals")?;
        ensure!(totals.encode()? == raw, "Noncanonical account totals");
        Ok(totals)
    }
    /// Totals over the given balance maps: the genesis value.
    pub(crate) fn sum<'a>(maps: impl IntoIterator<Item = &'a BTreeMap<String, u128>>) -> Result<Self> {
        maps.into_iter().try_fold(Self::default(), |totals, balances| {
            totals.add(Self {
                udgt: balances.get("udgt").copied().unwrap_or(0),
                udrt: balances.get("udrt").copied().unwrap_or(0),
            })
        })
    }
}

/// The account totals after a block's staged writes and deletes: the
/// committed totals, less each changed record's committed amounts, plus its
/// new amounts. Reads only the records the block changes.
pub(crate) fn account_totals_after(
    storage: &Storage,
    writes: &Writes,
    deletes: &Deletes,
) -> Result<AccountTotals> {
    let mut totals = AccountTotals::decode(
        &storage
            .db
            .get(ACCOUNT_TOTALS_KEY)?
            .context("Account totals missing; consensus genesis writes them")?,
    )?;
    let changed = writes
        .keys()
        .chain(deletes.iter())
        .filter(|key| key.starts_with(b"acct:balances:"));
    for key in changed {
        if let Some(old) = storage.db.get(key)? {
            totals = totals.sub(AccountTotals::of_balances(&old)?)?;
        }
    }
    for (key, value) in writes {
        if key.starts_with(b"acct:balances:") {
            ensure!(!deletes.contains(key), "A block cannot write and delete a balance");
            totals = totals.add(AccountTotals::of_balances(value)?)?;
        }
    }
    Ok(totals)
}

/// Which way the supply check finds liquid account balances.
#[derive(Clone, Copy)]
enum Accounts<'a> {
    /// Read every balance record; any stored totals must match the sum.
    Scan,
    /// Take the running totals after the staged changes; read single
    /// records only.
    Totals(&'a Deletes),
}
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
    pub burned: u128,
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
            burned: self.burned.to_string(),
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
    pub burned: String,
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
    validator_claimed: u128,
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
        } else if name == "validator_rewards" {
            // Paid by voting power and claimed by operator owners (fees v1).
            ensure!(
                held.checked_add(validator_claimed) == Some(issued_amount),
                "Validator custody plus claims differs from cumulative validator issuance"
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
/// Emission events are not a prefix here: only the current block's event is
/// read, by point lookup, instead of every event since genesis.
/// Governance contributes only its header's held deposit total (T6), and
/// issuance only its timing state; the controller journal is checked at
/// startup (P01, 27 September 2026).
const RELEVANT_PREFIXES: [&[u8]; 8] = [
    b"supply:",
    b"staking:",
    b"rewards:",
    b"lifecycle:",
    b"penalty:",
    b"gov:",
    b"acct:balances:",
    b"emission:pool:",
];
fn relevant_keys() -> [&'static [u8]; 10] {
    [
        ACCOUNT_TOTALS_KEY.as_bytes(),
        governance_store::HEADER_KEY.as_bytes(),
        TIMING_STATE_KEY.as_bytes(),
        DRT_GENESIS_KEY.as_bytes(),
        DRT_BURNED_KEY.as_bytes(),
        EMITTED.as_bytes(),
        FEE_KEY.as_bytes(),
        b"genesis:monetary:v1",
        b"emission:last_height",
        b"meta:chain_id",
    ]
}
fn relevant(key: &[u8]) -> bool {
    RELEVANT_PREFIXES.iter().any(|prefix| key.starts_with(prefix)) || relevant_keys().contains(&key)
}
/// Caller holds the storage execution lock, or owns exclusive startup access.
/// Overlay values replace stored values. This planner never writes storage.
/// The complete supply check: reads every account (startup and the
/// development path).
pub(crate) fn validate_native(storage: &Storage, overlay: &Writes) -> Result<NativeSupply> {
    validate_native_with(storage, overlay, Accounts::Scan)
}
/// The per-block supply check over staged `overlay` and `deletes`: liquid
/// balances come from the running account totals, so its cost does not grow
/// with the number of accounts. The overlay must carry the updated totals
/// when the block changes a balance.
pub(crate) fn validate_native_block(
    storage: &Storage,
    overlay: &Writes,
    deletes: &Deletes,
) -> Result<NativeSupply> {
    validate_native_with(storage, overlay, Accounts::Totals(deletes))
}
#[cfg(test)]
thread_local! { pub(crate) static ACCOUNT_SCANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
fn validate_native_with(
    storage: &Storage,
    overlay: &Writes,
    accounts: Accounts<'_>,
) -> Result<NativeSupply> {
    let snapshot = storage.db.snapshot();
    let mut values = Writes::new();
    let prefixes: Vec<&[u8]> = match accounts {
        Accounts::Scan => {
            #[cfg(test)]
            ACCOUNT_SCANS.with(|n| n.set(n.get() + 1));
            RELEVANT_PREFIXES.to_vec()
        }
        Accounts::Totals(_) => RELEVANT_PREFIXES
            .iter()
            .copied()
            .filter(|prefix| *prefix != b"acct:balances:".as_slice())
            .collect(),
    };
    for (key, value) in
        crate::block_lifecycle::snapshot_selected(&snapshot, &prefixes, &relevant_keys())?
    {
        if relevant(&key) {
            values.insert(key.to_vec(), value.to_vec());
        }
    }
    for (key, value) in overlay {
        if relevant(key) {
            values.insert(key.clone(), value.clone());
        }
    }
    let height = match values.get(b"emission:last_height".as_slice()) {
        Some(raw) => {
            u64::from_be_bytes(raw.as_slice().try_into().context("Invalid supply height")?)
        }
        None => 0,
    };
    let event_key = format!("emission:event:{height}").into_bytes();
    if let Some(event) = match overlay.get(&event_key) {
        Some(value) => Some(value.clone()),
        None => snapshot.get(&event_key)?,
    } {
        values.insert(event_key, event);
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
    // One account's balance record: from the staged or loaded values, else,
    // when balances were not all loaded, from committed storage.
    let balance_record = |owner: &str| -> Result<Option<Vec<u8>>> {
        let key = format!("acct:balances:{owner}").into_bytes();
        if let Some(raw) = values.get(&key) {
            return Ok(Some(raw.clone()));
        }
        match accounts {
            Accounts::Scan => Ok(None),
            Accounts::Totals(deletes) if deletes.contains(&key) => Ok(None),
            Accounts::Totals(_) => Ok(snapshot.get(&key)?),
        }
    };
    let has_account = |owner: &str| -> Result<bool> { Ok(balance_record(owner)?.is_some()) };
    let governance = values
        .get(governance_store::HEADER_KEY.as_bytes())
        .map(|raw| GovernanceHeader::decode(raw))
        .transpose()?;
    let governance_escrow = governance
        .as_ref()
        .map(|header| {
            // The genesis digest is bound to the configured candidate by the
            // history check; it is not the monetary genesis marker.
            ensure!(
                header.height == height
                    && values.get(b"meta:chain_id".as_slice()).map(Vec::as_slice)
                        == Some(header.chain_id.as_bytes()),
                "Governance supply state height or chain differs"
            );
            Ok(header.held_udgt)
        })
        .transpose()?;
    let genesis = read(DRT_GENESIS_KEY, true)?;
    let emitted = read(EMITTED, height > 0)?;
    ensure!(
        height > 0 || emitted == 0,
        "Emission exists before the first block"
    );
    let burned = read(DRT_BURNED_KEY, false)?;
    let total = genesis
        .checked_add(emitted)
        .context("Total DRT supply exceeds u128")?
        .checked_sub(burned)
        .context("Burned DRT exceeds issued supply")?;
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
        penalties.validate_lock_relief(
            &reward_state
                .as_ref()
                .context("Penalty custody requires reward state")?
                .locks
                .keys()
                .cloned()
                .collect(),
        )?;
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
            has_account(owner)?,
            "Pending bond owner has no funding account record"
        );
    }
    let timing = if values.contains_key(TIMING_STATE_KEY.as_bytes()) {
        ensure!(
            reward_state.is_some(),
            "Timed issuance requires reward-v2 state"
        );
        Some(issuance_timing::verify_state_bindings(&values)?)
    } else {
        let mut orphan = overlay
            .keys()
            .any(|key| key.starts_with(b"issuance:") || key.starts_with(b"adaptive:"));
        for prefix in [b"issuance:".as_slice(), b"adaptive:".as_slice()] {
            if let Some(item) = storage.db.prefix_iterator(prefix).next() {
                orphan |= item?.0.starts_with(prefix);
            }
        }
        ensure!(!orphan, "Orphan issuance or adaptive-controller record");
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
    let mut scanned = AccountTotals::default();
    let mut staked = 0u128;
    let mut bonded_owners = BTreeMap::new();
    let mut pools = BTreeMap::new();
    for (key, raw) in &values {
        ensure!(
            (!key.starts_with(b"governance:") && !key.starts_with(b"gov:"))
                || key.starts_with(governance_store::PREFIX.as_bytes()),
            "Unsupported governance supply record"
        );
        ensure!(
            !key.starts_with(b"rewards:") || key == REWARD_STATE_KEY.as_bytes(),
            "Unsupported reward record; accounting migration required"
        );
        ensure!(
            !key.starts_with(b"supply:")
                || key == DRT_GENESIS_KEY.as_bytes()
                || key == DRT_BURNED_KEY.as_bytes()
                || key == DGT_MINTED_KEY.as_bytes()
                || key == ACCOUNT_TOTALS_KEY.as_bytes(),
            "Unsupported native supply record; accounting migration required"
        );
        if key.starts_with(b"acct:balances:") {
            let balances = AccountTotals::of_balances(raw)?;
            if matches!(accounts, Accounts::Scan) {
                scanned = scanned.add(balances)?;
            }
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
                has_account(address)?,
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
    let stored_totals = values
        .get(ACCOUNT_TOTALS_KEY.as_bytes())
        .map(|raw| AccountTotals::decode(raw))
        .transpose()?;
    let accounts_total = match accounts {
        // Stored totals, when present, must equal the scan.
        Accounts::Scan => {
            ensure!(
                stored_totals.is_none_or(|stored| stored == scanned),
                "Account totals differ from the account records"
            );
            scanned
        }
        // The totals the block leaves must follow from its balance changes.
        Accounts::Totals(deletes) => {
            let expected = account_totals_after(storage, overlay, deletes)?;
            ensure!(
                stored_totals == Some(expected),
                "Account totals differ from the block's balance changes"
            );
            expected
        }
    };
    let (dgt_liquid, liquid) = (accounts_total.udgt, accounts_total.udrt);
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
        "DRT conservation failed: custody differs from genesis plus emission minus burns"
    );
    for name in allowed_pools {
        pools.entry((*name).into()).or_insert(0);
    }
    if let Some(timing) = &timing {
        let rewards = reward_state
            .as_ref()
            .context("Missing timed reward state")?;
        validate_timed_pools(
            &timing.total_issued,
            emitted,
            rewards.total_claimed,
            rewards.validator_payouts.total_claimed,
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
        for owner in rewards.owners() {
            ensure!(
                has_account(owner)?,
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
                let (offset, relief) = match &penalties {
                    Some(penalties) => penalties.lock_terms(owner)?,
                    None => (0, 0),
                };
                let balances: BTreeMap<String, u128> = bincode::deserialize(
                    &balance_record(owner)?.context("Vesting lock owner has no account record")?,
                )?;
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
                        offset,
                        relief,
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
        // The validator pool backs exactly the payouts owed and its reserve,
        // when payouts are tracked from the pool's first credit.
        let payouts = &rewards.validator_payouts;
        if timing.is_some() {
            ensure!(
                pools["validator_rewards"]
                    == payouts
                        .total_unpaid()?
                        .checked_add(payouts.reserve)
                        .context("Validator payout liability exceeds u128")?,
                "Validator pool differs from unpaid payouts and reserve"
            );
        }
        rewards.total_unbonding()?
    } else {
        0
    };
    let unbonding = if let Some(penalties) = &penalties {
        let validators = validators
            .as_ref()
            .context("Penalty custody requires validator lifecycle")?;
        // Gross unbond custody still counts the tranches it holds; removed
        // tranches left it with their unbond entries (state model step 4).
        let net = penalties.net_unbonding_total(validators)?;
        let deductions = penalties.live_deducted()?;
        let releases = penalties.live_released()?;
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
            burned,
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
/// The committed consensus history verifies before any inspection. The
/// development block adapter these views also served was removed (E04 gap 14).
fn verify_committed(storage: &Storage) -> Result<()> {
    ensure!(
        storage
            .db
            .get(crate::consensus_settlement::MODE_KEY)?
            .is_some(),
        "Supply inspection requires consensus state"
    );
    crate::consensus_settlement::verify_recovery(storage)
}
/// Return one committed accounting view. Invalid or unsupported state returns an error.
pub fn inspect_native(storage: &Storage) -> Result<NativeSupply> {
    let _guard = storage.lock_execution()?;
    verify_committed(storage)?;
    validate_native(storage, &Writes::new())
}

#[cfg(test)]
mod reward_v2_tests {
    use super::*;
    use crate::runtime::governance_store::HEADER_KEY as GOVERNANCE_HEADER_KEY;
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
    fn burned_drt_reduces_total_supply_and_keeps_conservation() {
        let dir = tempfile::tempdir().unwrap();
        let mut storage = Storage::open(dir.path().join("db")).unwrap();
        crate::genesis::initialize(
            &mut storage,
            "burn-test",
            Some(
                br#"{"chain_id":"burn-test",
                    "accounts":[{"address":"alice","balances":{"udrt":"1000"}}]}"#,
            ),
        )
        .unwrap();
        let storage = std::sync::Arc::new(storage);
        let mut settlement = crate::settlement::Settlement::new(storage.clone());
        settlement.burn("alice", 100).unwrap();
        let writes = settlement.writes().unwrap();
        let supply = validate_native(&storage, &writes).unwrap();
        assert_eq!(
            (supply.drt.genesis, supply.drt.burned, supply.drt.total, supply.drt.liquid),
            (1000, 100, 900, 900)
        );
        assert_eq!(supply.drt.response().burned, "100");
        // A burn counter without the matching debit breaks conservation.
        let mut counter_only = Writes::new();
        put_amount(&mut counter_only, DRT_BURNED_KEY, 100);
        assert!(validate_native(&storage, &counter_only).is_err());
        // More than was ever issued cannot be burned.
        put_amount(&mut counter_only, DRT_BURNED_KEY, 1001);
        assert!(validate_native(&storage, &counter_only).is_err());
        // A burn cannot overdraw the payer.
        assert!(settlement.burn("alice", 901).is_err());
    }
    fn governance_header(chain: &str, digest: [u8; 32], height: u64, held: u128) -> Vec<u8> {
        let mut header = GovernanceHeader::genesis(chain.into(), digest).unwrap();
        header.height = height;
        header.held_udgt = held;
        header.encode().unwrap()
    }
    #[test]
    fn governance_supply_state_is_bound_to_chain_and_height() {
        let (_dir, storage, _) = fixture();
        let marker = storage.db.get("genesis:monetary:v1").unwrap().unwrap();
        let mut digest = [0u8; 32];
        digest.copy_from_slice(&marker[1..]);
        let key = GOVERNANCE_HEADER_KEY.as_bytes().to_vec();
        let mut writes = Writes::new();
        writes.insert(key.clone(), governance_header("supply-test", digest, 0, 0));
        let supply = validate_native(&storage, &writes).unwrap();
        assert_eq!(supply.dgt.governance_escrow, Some(0));
        assert_eq!(supply.dgt.response().accounting_version, 2);
        for wrong in [
            governance_header("other-chain", digest, 0, 0),
            governance_header("supply-test", digest, 1, 0),
        ] {
            writes.insert(key.clone(), wrong);
            assert!(validate_native(&storage, &writes).is_err());
        }
        writes.insert(key, governance_header("supply-test", digest, 0, 0));
        writes.insert(b"gov:legacy".to_vec(), vec![1]);
        assert!(validate_native(&storage, &writes).is_err());
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
        let mut writes = Writes::new();
        writes.insert(
            GOVERNANCE_HEADER_KEY.as_bytes().to_vec(),
            governance_header("supply-test", digest, 2, 5),
        );
        writes.insert(
            b"emission:last_height".to_vec(),
            2u64.to_be_bytes().to_vec(),
        );
        put_amount(&mut writes, EMITTED, 0);
        writes.insert(
            b"acct:balances:alice".to_vec(),
            bincode::serialize(&BTreeMap::from([("udgt".to_string(), 55u128)])).unwrap(),
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
        validate_timed_pools(&issued, 0, 0, 0, &timed_amounts(&issued)).unwrap();
    }
    #[test]
    fn mid_epoch_custody_preserves_claimed_and_unpaid_staking_supply() {
        let issued = PoolAmounts::epoch_split(103).prefix(10, 5).unwrap();
        let mut pools = timed_amounts(&issued);
        let claimed = 5;
        *pools.get_mut("staking_rewards").unwrap() -= claimed;
        validate_timed_pools(&issued, issued.total().unwrap(), claimed, 0, &pools).unwrap();
        assert_eq!(
            pools.values().sum::<u128>() + claimed,
            issued.total().unwrap()
        );
        // Claimed rewards move to liquid custody and cannot also remain in the pool.
        *pools.get_mut("staking_rewards").unwrap() += claimed;
        assert!(validate_timed_pools(&issued, issued.total().unwrap(), claimed, 0, &pools).is_err());
        // Validator payouts claimed from their pool follow the same rule.
        *pools.get_mut("staking_rewards").unwrap() -= claimed;
        *pools.get_mut("validator_rewards").unwrap() -= 3;
        validate_timed_pools(&issued, issued.total().unwrap(), claimed, 3, &pools).unwrap();
        assert!(validate_timed_pools(&issued, issued.total().unwrap(), claimed, 0, &pools).is_err());
    }
    #[test]
    fn timed_custody_rejects_pool_reassignment_counter_tampering_and_legacy_names() {
        let issued = PoolAmounts::epoch_split(103);
        let total = issued.total().unwrap();
        let original = timed_amounts(&issued);
        let mut reassigned = original.clone();
        *reassigned.get_mut("validator_rewards").unwrap() -= 1;
        *reassigned.get_mut("treasury").unwrap() += 1;
        assert!(validate_timed_pools(&issued, total, 0, 0, &reassigned).is_err());
        assert!(validate_timed_pools(&issued, total + 1, 0, 0, &original).is_err());
        let mut renamed = original;
        let value = renamed.remove("validator_rewards").unwrap();
        renamed.insert("block_rewards".into(), value);
        assert!(validate_timed_pools(&issued, total, 0, 0, &renamed).is_err());
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
    }
}

#[cfg(test)]
mod account_totals_tests {
    use super::*;

    /// The totals after a block: an update, a new record and a removal.
    #[test]
    fn account_totals_after_counts_updates_new_records_and_removals() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path().join("db")).unwrap();
        let map = |udgt: u128, udrt: u128| {
            bincode::serialize(&BTreeMap::from([
                ("udgt".to_string(), udgt),
                ("udrt".to_string(), udrt),
            ]))
            .unwrap()
        };
        storage.db.put(b"acct:balances:a", map(10, 100)).unwrap();
        storage.db.put(b"acct:balances:b", map(5, 50)).unwrap();
        let totals = AccountTotals { udgt: 15, udrt: 150 };
        storage.db.put(ACCOUNT_TOTALS_KEY, totals.encode().unwrap()).unwrap();
        let mut writes = Writes::new();
        writes.insert(b"acct:balances:a".to_vec(), map(8, 90));
        writes.insert(b"acct:balances:c".to_vec(), map(1, 1));
        let deletes = Deletes::from([b"acct:balances:b".to_vec()]);
        assert_eq!(
            account_totals_after(&storage, &writes, &deletes).unwrap(),
            AccountTotals { udgt: 9, udrt: 91 }
        );
        assert_eq!(
            account_totals_after(&storage, &Writes::new(), &Deletes::new()).unwrap(),
            totals
        );
        let both = Deletes::from([b"acct:balances:a".to_vec()]);
        assert!(account_totals_after(&storage, &writes, &both).is_err());
    }

    /// The complete check audits stored totals against every record, and a
    /// block check takes liquid supply from the totals its writes leave.
    #[test]
    fn stored_totals_are_audited_and_carry_block_changes() {
        let dir = tempfile::tempdir().unwrap();
        let mut storage = Storage::open(dir.path().join("db")).unwrap();
        crate::genesis::initialize(
            &mut storage,
            "totals-test",
            Some(
                br#"{"chain_id":"totals-test",
                    "accounts":[{"address":"alice","balances":{"udrt":"1000"}}]}"#,
            ),
        )
        .unwrap();
        let put = |totals: AccountTotals| {
            storage.db.put(ACCOUNT_TOTALS_KEY, totals.encode().unwrap()).unwrap()
        };
        let right = AccountTotals { udgt: 0, udrt: 1000 };
        put(right);
        validate_native(&storage, &Writes::new()).unwrap();
        put(AccountTotals { udgt: 0, udrt: 999 });
        let error = validate_native(&storage, &Writes::new()).unwrap_err().to_string();
        assert!(error.contains("Account totals differ"), "{error}");
        put(right);
        let storage = std::sync::Arc::new(storage);
        let mut settlement = crate::settlement::Settlement::new(storage.clone());
        settlement.burn("alice", 100).unwrap();
        let mut writes = settlement.writes().unwrap();
        assert!(validate_native_block(&storage, &writes, &Deletes::new()).is_err());
        let after = account_totals_after(&storage, &writes, &Deletes::new()).unwrap();
        assert_eq!(after, AccountTotals { udgt: 0, udrt: 900 });
        writes.insert(ACCOUNT_TOTALS_KEY.as_bytes().to_vec(), after.encode().unwrap());
        let supply = validate_native_block(&storage, &writes, &Deletes::new()).unwrap();
        assert_eq!((supply.drt.liquid, supply.drt.total), (900, 900));
    }
}
