//! Transaction-local account and inactivity-switch state for selected-node execution.
use crate::runtime::dead_man_switch::DeadManSwitchConfig;
use crate::runtime::governance_store::GovernanceStore;
use crate::runtime::penalty_custody::{EvidenceFact, PenaltyState, STATE_KEY as PENALTY_STATE_KEY};
use crate::runtime::reward_runtime::{RewardState, REWARD_STATE_KEY};
use crate::runtime::staking::{delegator_key, DelegatorRewardRecord, TOTAL_STAKE_KEY};
use crate::runtime::validator_lifecycle::{
    LifecycleState, Operation, STATE_KEY as VALIDATOR_STATE_KEY,
};
use crate::state::AccountState;
use crate::storage::state::Storage;
use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub(crate) struct RuleViolation(pub String);
fn violation(message: &str) -> RuleViolation {
    RuleViolation(message.into())
}

pub(crate) const FEE_KEY: &str = "execution:v1:withheld_udrt";
pub(crate) const EVIDENCE_PREFIX: &str = "evidence:v1:";
/// The record of the `index`th evidence fact in block `height`.
pub(crate) fn evidence_key(height: u64, index: usize) -> Result<Vec<u8>> {
    anyhow::ensure!(index < 64, "Evidence record index outside the batch bound");
    Ok(format!("{EVIDENCE_PREFIX}{height:020}:{index:02}").into_bytes())
}
pub(crate) fn encode_evidence(fact: &EvidenceFact) -> Result<Vec<u8>> {
    use bincode::Options;
    Ok(bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .serialize(fact)?)
}
fn dms_key(owner: &str) -> String {
    format!("dms:config:{owner}")
}
fn read<T: DeserializeOwned>(storage: &Storage, key: &str) -> Result<Option<T>> {
    storage
        .db
        .get(key)?
        .map(|bytes| bincode::deserialize(&bytes).context("Invalid stored execution value"))
        .transpose()
}

#[derive(Clone)]
pub(crate) struct Settlement {
    pub(crate) storage: Arc<Storage>,
    pub(crate) accounts: BTreeMap<String, AccountState>,
    switches: BTreeMap<String, DeadManSwitchConfig>,
    pub(crate) fee_total: Option<u128>,
    /// Staged cumulative burned uDRT (`supply:drt_burned`); None if unchanged.
    pub(crate) burned_total: Option<u128>,
    pub(crate) lifecycle: BTreeMap<Vec<u8>, Vec<u8>>,
    pub(crate) rewards: Option<RewardState>,
    pub(crate) reward_timestamp: Option<u64>,
    pub(crate) validators: Option<LifecycleState>,
    pub(crate) penalties: Option<PenaltyState>,
    pub(crate) governance: Option<GovernanceStore>,
    /// Evidence recorded with no stake or set effect (P01, 27 September 2026).
    pub(crate) evidence_records: BTreeMap<Vec<u8>, Vec<u8>>,
}
impl Settlement {
    pub(crate) fn new(storage: Arc<Storage>) -> Self {
        Self {
            storage,
            accounts: BTreeMap::new(),
            switches: BTreeMap::new(),
            fee_total: None,
            burned_total: None,
            lifecycle: BTreeMap::new(),
            rewards: None,
            reward_timestamp: None,
            validators: None,
            penalties: None,
            governance: None,
            evidence_records: BTreeMap::new(),
        }
    }
    /// Attach one parent-snapshot interval before executing its transactions.
    /// The block adapter stages all writes before it executes transactions.
    pub(crate) fn attach_reward_lifecycle(
        &mut self,
        writes: BTreeMap<Vec<u8>, Vec<u8>>,
        timestamp: u64,
    ) -> Result<()> {
        anyhow::ensure!(self.rewards.is_none(), "Reward interval already attached");
        let raw = writes
            .get(REWARD_STATE_KEY.as_bytes())
            .context("Missing staged reward state")?;
        let rewards = RewardState::decode(raw)?;
        let validators = writes
            .get(VALIDATOR_STATE_KEY.as_bytes())
            .map(|raw| LifecycleState::decode(raw))
            .transpose()?;
        anyhow::ensure!(
            validators.is_some() || self.storage.db.get(VALIDATOR_STATE_KEY)?.is_none(),
            "Stored validator lifecycle requires a staged activation plan"
        );
        if let Some(validators) = &validators {
            validators.validate_rewards(&rewards)?;
        }
        let penalties = writes
            .get(PENALTY_STATE_KEY.as_bytes())
            .map(|raw| PenaltyState::decode(raw))
            .transpose()?;
        anyhow::ensure!(
            penalties.is_some() || self.storage.db.get(PENALTY_STATE_KEY)?.is_none(),
            "Stored penalty custody requires a staged activation plan"
        );
        if let Some(penalties) = &penalties {
            anyhow::ensure!(
                rewards.locks.is_empty(),
                "Penalty custody does not support vesting locks"
            );
            penalties.validate(
                validators
                    .as_ref()
                    .context("Penalty custody requires validator lifecycle")?,
            )?;
        }
        self.penalties = penalties;
        self.validators = validators;
        self.rewards = Some(rewards);
        self.reward_timestamp = Some(timestamp);
        self.lifecycle = writes;
        Ok(())
    }
    fn reward_plan(&self) -> Result<RewardState> {
        self.rewards
            .clone()
            .ok_or_else(|| violation("Reward operations require versioned block settlement").into())
    }
    fn pool(&self, key: &str) -> Result<u128> {
        let raw = self
            .lifecycle
            .get(key.as_bytes())
            .with_context(|| format!("Missing staged pool {key}"))?;
        bincode::deserialize(raw).context("Invalid staged pool")
    }
    /// Burn the block's withheld fees (fees v1, P01 27 September 2026). Every
    /// fee a block charged leaves supply at the end of that block; the
    /// withheld counter is zero at every commit.
    pub(crate) fn burn_withheld_fees(&mut self) -> Result<u128> {
        let withheld = self.ordinary_fee_total()?;
        if withheld == 0 {
            return Ok(0);
        }
        let burned = self
            .burned_total()?
            .checked_add(withheld)
            .context("Burned DRT total exceeds u128")?;
        self.burned_total = Some(burned);
        self.fee_total = Some(0);
        Ok(withheld)
    }
    pub(crate) fn reward_pool(&self) -> Result<u128> {
        let key = b"emission:pool:staking_rewards";
        let raw = self
            .lifecycle
            .get(key.as_slice())
            .context("Missing staged staking pool")?;
        bincode::deserialize(raw).context("Invalid staged staking pool")
    }
    pub(crate) fn pending_bond(&self, owner: &str) -> Result<u128> {
        self.validators
            .as_ref()
            .map_or(Ok(0), |state| state.pending_bond_by_owner(owner))
    }
    /// Apply engine-validated evidence before any transaction in this block.
    pub(crate) fn apply_validator_evidence(
        &mut self,
        fact: &EvidenceFact,
        historical_time: (u64, i32),
        parent_height: u64,
        parent_time: (u64, i32),
    ) -> Result<()> {
        // Until penalty rules are approved, and for kinds that are never
        // penalized, evidence is only recorded.
        if self.penalties.is_none() || fact.kind != "duplicate_vote" {
            return self.record_evidence(fact);
        }
        let before = self
            .validators
            .as_ref()
            .context("Evidence requires validator lifecycle")?;
        let mut next = before.clone();
        let mut penalties = self
            .penalties
            .clone()
            .context("Evidence requires penalty custody profile")?;
        let assessment =
            penalties.assess(fact, historical_time, parent_height, parent_time, before)?;
        if assessment.requires_exit {
            let height = self.reward_plan()?.last_height;
            let activation = height
                .checked_add(2)
                .context("Penalty exit height overflow")?;
            let projected = next
                .schedules
                .range(..=activation)
                .next_back()
                .map(|(_, schedule)| &schedule.view)
                .unwrap_or(&next.effective);
            let owner = projected
                .validators
                .get(&assessment.validator)
                .context("Penalty exit validator is missing from scheduled set")?
                .owner
                .clone();
            next.schedule(
                height,
                &owner,
                0,
                Operation::Exit {
                    validator: assessment.validator,
                },
            )?;
        }
        penalties.sync_lifecycle(before, &next)?;
        self.validators = Some(next);
        self.penalties = Some(penalties);
        Ok(())
    }
    fn record_evidence(&mut self, fact: &EvidenceFact) -> Result<()> {
        fact.validate_shape()?;
        let height = self.reward_plan()?.last_height;
        let key = evidence_key(height, self.evidence_records.len())?;
        self.evidence_records.insert(key, encode_evidence(fact)?);
        Ok(())
    }
    pub(crate) fn finalize_validator_evidence(&mut self) -> Result<()> {
        if let Some(mut penalties) = self.penalties.clone() {
            penalties.finalize_evidence_batch(
                self.validators
                    .as_ref()
                    .context("Evidence requires validator lifecycle")?,
            )?;
            self.penalties = Some(penalties);
        }
        Ok(())
    }
    /// Pay the owner's staking rewards and validator payouts, each from its
    /// own pool.
    pub(crate) fn reward_claim(&mut self, owner: &str) -> Result<u128> {
        let mut next = self.reward_plan()?;
        let (staking, validator) = next.claim(owner).map_err(|e| violation(&e.to_string()))?;
        for (key, amount) in [
            ("emission:pool:staking_rewards", staking),
            ("emission:pool:validator_rewards", validator),
        ] {
            // Paths without validator payouts never stage that pool.
            if amount == 0 && key.ends_with("validator_rewards") {
                continue;
            }
            let pool = self
                .pool(key)?
                .checked_sub(amount)
                .context("Reward liability exceeds pool backing")?;
            self.lifecycle
                .insert(key.as_bytes().to_vec(), bincode::serialize(&pool)?);
        }
        let amount = staking
            .checked_add(validator)
            .ok_or_else(|| violation("Reward claim exceeds u128"))?;
        let balance = self
            .account(owner)?
            .balance_of("udrt")
            .checked_add(amount)
            .ok_or_else(|| violation("Reward recipient balance exceeds u128"))?;
        self.account(owner)?.set_balance("udrt", balance);
        self.rewards = Some(next);
        Ok(amount)
    }
    /// The native record of an existing account, without loading it into the
    /// overlay: a loaded address is written at commit, which would create it.
    /// An account exists when it has a balance record.
    pub(crate) fn peek(&self, address: &str) -> Result<Option<AccountState>> {
        if let Some(account) = self.accounts.get(address) {
            return Ok(Some(account.clone()));
        }
        let Some(balances) = read(&self.storage, &format!("acct:balances:{address}"))? else {
            return Ok(None);
        };
        let nonce = read(&self.storage, &format!("acct:nonce:{address}"))?.unwrap_or(0);
        Ok(Some(AccountState { balances, nonce }))
    }
    pub(crate) fn exists(&self, address: &str) -> Result<bool> {
        Ok(self.peek(address)?.is_some())
    }
    pub(crate) fn account(&mut self, address: &str) -> Result<&mut AccountState> {
        if !self.accounts.contains_key(address) {
            let balances =
                read(&self.storage, &format!("acct:balances:{address}"))?.unwrap_or_default();
            let nonce = read(&self.storage, &format!("acct:nonce:{address}"))?.unwrap_or(0);
            self.accounts
                .insert(address.into(), AccountState { balances, nonce });
        }
        Ok(self
            .accounts
            .get_mut(address)
            .expect("account inserted above"))
    }
    /// Debit sponsored recovery fees without consuming the ordinary spending nonce.
    /// The recovery overlay owns the independent sponsor counter.
    /// Current eligible native liquidity for the explicit ordinary action class.
    pub(crate) fn ordinary_eligible(
        &mut self,
        owner: &str,
        denom: &str,
        staking: bool,
    ) -> Result<u128> {
        let liquid = self.account(owner)?.balance_of(denom);
        if denom != "udgt" {
            return Ok(liquid);
        }
        let Some(rewards) = &self.rewards else {
            return Ok(liquid);
        };
        if staking
            && rewards
                .locks
                .get(owner)
                .is_some_and(|lock| lock.permits_staking)
        {
            return Ok(liquid);
        }
        rewards.liquid_spendable(
            owner,
            liquid,
            rewards
                .owner_bonded(owner)?
                .checked_add(self.pending_bond(owner)?)
                .context("Owner bond custody exceeds u128")?,
            rewards.unbonding.get(owner).copied().unwrap_or(0),
            self.reward_timestamp
                .context("Missing staged reward timestamp")?,
        )
    }
    pub(crate) fn burned_total(&self) -> Result<u128> {
        match self.burned_total {
            Some(total) => Ok(total),
            None => Ok(read::<u128>(&self.storage, crate::supply::DRT_BURNED_KEY)?.unwrap_or(0)),
        }
    }
    /// Remove `amount` uDRT from `address` and from total supply.
    pub(crate) fn burn(&mut self, address: &str, amount: u128) -> Result<()> {
        let total = self
            .burned_total()?
            .checked_add(amount)
            .context("Burned DRT total exceeds u128")?;
        let account = self.account(address)?;
        let balance = account
            .balance_of("udrt")
            .checked_sub(amount)
            .ok_or_else(|| violation("Insufficient balance for burned fee"))?;
        account.set_balance("udrt", balance);
        self.burned_total = Some(total);
        Ok(())
    }
    pub(crate) fn ordinary_fee_total(&self) -> Result<u128> {
        match self.fee_total {
            Some(total) => Ok(total),
            None => Ok(read::<u128>(&self.storage, FEE_KEY)?.unwrap_or(0)),
        }
    }
    pub(crate) fn charge_sponsored(&mut self, address: &str, fee: u128) -> Result<()> {
        let current = match self.fee_total {
            Some(total) => total,
            None => read::<u128>(&self.storage, FEE_KEY)?.unwrap_or(0),
        };
        let total = current
            .checked_add(fee)
            .context("Withheld fee total exceeds u128")?;
        let account = self.account(address)?;
        let balance = account
            .balance_of("udrt")
            .checked_sub(fee)
            .context("Insufficient sponsored fee balance")?;
        account.set_balance("udrt", balance);
        self.fee_total = Some(total);
        Ok(())
    }
    pub(crate) fn writes(&self) -> Result<BTreeMap<Vec<u8>, Vec<u8>>> {
        let mut writes = self.lifecycle.clone();
        for (key, value) in &self.evidence_records {
            anyhow::ensure!(
                writes.insert(key.clone(), value.clone()).is_none(),
                "Evidence record already staged"
            );
        }
        if let Some(validators) = &self.validators {
            validators
                .validate_rewards(self.rewards.as_ref().context("Missing lifecycle rewards")?)?;
            writes.insert(
                VALIDATOR_STATE_KEY.as_bytes().to_vec(),
                validators.encode()?,
            );
        }
        if let Some(penalties) = &self.penalties {
            penalties.validate(
                self.validators
                    .as_ref()
                    .context("Penalty custody requires validator lifecycle")?,
            )?;
            writes.insert(PENALTY_STATE_KEY.as_bytes().to_vec(), penalties.encode()?);
        }
        if let Some(rewards) = &self.rewards {
            writes.insert(REWARD_STATE_KEY.as_bytes().to_vec(), rewards.encode()?);
            writes.insert(
                TOTAL_STAKE_KEY.as_bytes().to_vec(),
                bincode::serialize(&rewards.total_bonded()?)?,
            );
            let owners: std::collections::BTreeSet<_> = rewards
                .positions
                .keys()
                .chain(rewards.unbonding.keys())
                .chain(rewards.unpaid.keys())
                .chain(rewards.validator_payouts.unpaid.keys())
                .collect();
            for owner in owners {
                let key = delegator_key(owner);
                let mut record =
                    read::<DelegatorRewardRecord>(&self.storage, &key)?.unwrap_or_default();
                anyhow::ensure!(
                    record.last_reward_index == 0 && record.accrued_rewards == 0,
                    "Legacy reward claim requires migration"
                );
                record.stake_amount = rewards.owner_bonded(owner)?;
                writes.insert(key.into_bytes(), bincode::serialize(&record)?);
            }
        }
        for (address, account) in &self.accounts {
            writes.insert(
                format!("acct:balances:{address}").into_bytes(),
                bincode::serialize(&account.balances)?,
            );
            writes.insert(
                format!("acct:nonce:{address}").into_bytes(),
                bincode::serialize(&account.nonce)?,
            );
        }
        for (owner, config) in &self.switches {
            writes.insert(dms_key(owner).into_bytes(), bincode::serialize(config)?);
        }
        if let Some(total) = self.fee_total {
            writes.insert(FEE_KEY.as_bytes().to_vec(), bincode::serialize(&total)?);
        }
        if let Some(total) = self.burned_total {
            writes.insert(
                crate::supply::DRT_BURNED_KEY.as_bytes().to_vec(),
                bincode::serialize(&total)?,
            );
        }
        Ok(writes)
    }
}
