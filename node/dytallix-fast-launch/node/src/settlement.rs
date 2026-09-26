//! Transaction-local account and inactivity-switch state for selected-node execution.
use crate::governance_signed_admission::PreadmissionAssessment;
use crate::governance_v3_fee_settlement::{
    ActionDisposition, FeePlan as GovernanceFeePlan, Outcome as GovernanceFeeOutcome,
};
use crate::ordinary_fee_settlement::FinancialState;
use crate::ordinary_reservations::{Asset, Denomination};
use crate::recovery_fees::RecoveryBook;
use crate::runtime::dead_man_switch::DeadManSwitchConfig;
use crate::runtime::governance_ordered_admission::OrderedAdmissionPlan;
use crate::runtime::governance_state::{GovernanceState, STATE_KEY as GOVERNANCE_STATE_KEY};
use crate::runtime::penalty_custody::{EvidenceFact, PenaltyState, STATE_KEY as PENALTY_STATE_KEY};
use crate::runtime::reward_runtime::{RewardState, REWARD_STATE_KEY};
use crate::runtime::staking::{delegator_key, DelegatorRewardRecord, TOTAL_STAKE_KEY};
use crate::runtime::validator_lifecycle::{
    LifecycleState, Operation, STATE_KEY as VALIDATOR_STATE_KEY,
};
use crate::state::{AccountState, State};
use crate::storage::{receipts::TxReceipt, state::Storage, tx::Transaction};
use anyhow::{bail, Context, Result};
use rocksdb::{WriteBatch, WriteOptions};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub(crate) struct RuleViolation(pub String);
fn violation(message: &str) -> RuleViolation {
    RuleViolation(message.into())
}

pub(crate) const FEE_KEY: &str = "execution:v1:withheld_udrt";
fn record_key(hash: &str) -> String {
    format!("execution:v1:receipt:{hash}")
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
#[derive(Serialize, Deserialize)]
struct Record {
    version: u8,
    digest: [u8; 32],
    receipt: TxReceipt,
}
fn digest(tx: &Transaction) -> Result<[u8; 32]> {
    let mut hash = Sha256::new();
    hash.update(b"dytallix-execution-input-v1");
    hash.update(serde_json::to_vec(tx)?);
    Ok(hash.finalize().into())
}
pub(crate) fn existing(
    storage: &Storage,
    tx: &Transaction,
    height: u64,
    index: u32,
) -> Result<Option<TxReceipt>> {
    let Some(bytes) = storage.db.get(record_key(&tx.hash))? else {
        return Ok(None);
    };
    let record: Record = serde_json::from_slice(&bytes).context("Invalid settlement record")?;
    if record.version != 1
        || record.digest != digest(tx)?
        || record.receipt.block_height != Some(height)
        || record.receipt.index != Some(index)
    {
        bail!("Settlement input or position differs; explicit block recovery is required");
    }
    Ok(Some(record.receipt))
}

#[derive(Clone)]
pub(crate) struct Settlement {
    pub(crate) storage: Arc<Storage>,
    pub(crate) accounts: BTreeMap<String, AccountState>,
    switches: BTreeMap<String, DeadManSwitchConfig>,
    pub(crate) fee_total: Option<u128>,
    pub(crate) lifecycle: BTreeMap<Vec<u8>, Vec<u8>>,
    pub(crate) rewards: Option<RewardState>,
    pub(crate) reward_timestamp: Option<u64>,
    pub(crate) validators: Option<LifecycleState>,
    pub(crate) penalties: Option<PenaltyState>,
    pub(crate) governance: Option<GovernanceState>,
}
impl Settlement {
    pub(crate) fn new(storage: Arc<Storage>) -> Self {
        Self {
            storage,
            accounts: BTreeMap::new(),
            switches: BTreeMap::new(),
            fee_total: None,
            lifecycle: BTreeMap::new(),
            rewards: None,
            reward_timestamp: None,
            validators: None,
            penalties: None,
            governance: None,
        }
    }
    /// Stage every governance account change with its state record. The
    /// consensus caller must still bind the stored parent to the committed
    /// head and enable the exact governance profile before these writes commit.
    pub(crate) fn apply_ordered_governance(
        &mut self,
        parent: &GovernanceState,
        plan: &OrderedAdmissionPlan,
        book: &RecoveryBook,
    ) -> Result<()> {
        anyhow::ensure!(self.governance.is_none(), "Governance already staged");
        parent.validate()?;
        book.validate()?;
        anyhow::ensure!(
            parent.encode()? == plan.parent_state_bytes
                && self.storage.db.get(GOVERNANCE_STATE_KEY)?
                    == Some(plan.parent_state_bytes.clone())
                && plan.next_state.encode()? == plan.state_bytes
                && parent.finalized_height().checked_add(1)
                    == Some(plan.next_state.finalized_height())
                && parent.plan_commit(
                    plan.next_state.finalized_height(),
                    plan.next_state.proposals().clone(),
                )? == plan.next_state
                && book.last_height == plan.next_state.finalized_height()
                && book.accounts.values().all(|account| {
                    account.recovery.domain.chain_id == parent.chain_id()
                        && account.recovery.domain.genesis_digest == parent.genesis_digest()
                }),
            "Governance plan differs from stored parent or staged recovery"
        );
        let before_accounts = plan.account_writes.iter().try_fold(0u128, |sum, write| {
            sum.checked_add(write.balance_before_udgt)
                .context("Governance before-account custody overflow")
        })?;
        let after_accounts = plan.account_writes.iter().try_fold(0u128, |sum, write| {
            sum.checked_add(write.balance_after_udgt)
                .context("Governance after-account custody overflow")
        })?;
        anyhow::ensure!(
            before_accounts.checked_add(parent.held_total_udgt()?)
                == after_accounts.checked_add(plan.next_state.held_total_udgt()?),
            "Governance native account and escrow custody differ"
        );
        let mut staged = self.clone();
        let mut seen = BTreeSet::new();
        for write in &plan.account_writes {
            anyhow::ensure!(
                seen.insert(write.owner),
                "Repeated governance account write"
            );
            let account = book
                .accounts
                .get(&hex::encode(write.owner))
                .context("Governance account owner is not registered")?;
            let address = &account.address;
            let before_balance = staged.account(address)?.balance_of("udgt");
            let before_spendable = staged.ordinary_eligible(address, "udgt", false)?;
            anyhow::ensure!(
                before_balance == write.balance_before_udgt
                    && before_spendable == write.spendable_before_udgt,
                "Governance account write differs from staged balance"
            );
            staged
                .account(address)?
                .set_balance("udgt", write.balance_after_udgt);
            anyhow::ensure!(
                staged.ordinary_eligible(address, "udgt", false)? == write.spendable_after_udgt,
                "Governance account write differs from staged spendable DGT"
            );
        }
        staged.governance = Some(plan.next_state.clone());
        *self = staged;
        Ok(())
    }
    /// Read the same staged accounts that a governance fee plan would change.
    /// The clone keeps failed or read-only planning out of the write overlay.
    pub(crate) fn governance_financial_snapshot(
        &self,
        book: &RecoveryBook,
    ) -> Result<FinancialState> {
        book.validate()?;
        let mut staged = self.clone();
        let mut snapshot = FinancialState {
            balances: BTreeMap::new(),
            eligible: BTreeMap::new(),
            native_nonces: BTreeMap::new(),
            withheld_udrt: staged.ordinary_fee_total()?,
        };
        for registered in book.accounts.values() {
            let address = &registered.address;
            let owner = registered.recovery.domain.account_id;
            let account = staged.account(address)?.clone();
            anyhow::ensure!(
                account.nonce == registered.recovery.spending_nonce,
                "Governance native and recovery nonces differ"
            );
            snapshot.native_nonces.insert(address.clone(), account.nonce);
            for (denomination, name) in [
                (Denomination::Udgt, "udgt"),
                (Denomination::Udrt, "udrt"),
            ] {
                let asset = Asset { owner, denomination };
                let balance = account.balance_of(name);
                let eligible = staged.ordinary_eligible(address, name, false)?;
                anyhow::ensure!(
                    eligible <= balance,
                    "Governance eligible amount exceeds account balance"
                );
                snapshot.balances.insert(asset, balance);
                snapshot.eligible.insert(asset, eligible);
            }
        }
        Ok(snapshot)
    }
    /// Apply the paid v3 fee and both nonce mirrors to this block overlay.
    /// The consensus caller still owns the receipt and the returned RecoveryBook.
    pub(crate) fn apply_governance_fee_plan(
        &mut self,
        assessment: &PreadmissionAssessment,
        plan: &GovernanceFeePlan,
        book: &RecoveryBook,
        block_index: u32,
    ) -> Result<RecoveryBook> {
        assessment.bind_fee_receipt(&plan.receipt)?;
        book.validate()?;
        let mut staged = self.clone();
        let receipt = &plan.receipt;
        anyhow::ensure!(
            receipt.block_height == book.last_height
                && receipt.block_index == block_index
                && plan.reservation.asset.owner == receipt.actor
                && plan.reservation.asset.denomination == Denomination::Udrt
                && plan.reservation.cap == receipt.reserved_cap
                && matches!(
                    (receipt.outcome, plan.disposition),
                    (GovernanceFeeOutcome::Success, ActionDisposition::KeepProposed)
                        | (GovernanceFeeOutcome::ApplicationFailure | GovernanceFeeOutcome::OutOfGas,
                            ActionDisposition::DiscardAll)
                ),
            "Governance fee plan identity or disposition differs"
        );
        let registered = book
            .accounts
            .get(&hex::encode(receipt.actor))
            .context("Governance fee actor is not registered")?;
        let address = &registered.address;
        let before_balance = staged.account(address)?.balance_of("udrt");
        let before_eligible = staged.ordinary_eligible(address, "udrt", false)?;
        let before_nonce = staged.account(address)?.nonce;
        let before_withheld = staged.ordinary_fee_total()?;
        let after_balance = before_balance
            .checked_sub(receipt.charge)
            .context("Governance fee exceeds current uDRT balance")?;
        let after_eligible = before_eligible
            .checked_sub(receipt.charge)
            .context("Governance fee exceeds eligible uDRT")?;
        let expected_nonce = before_nonce
            .checked_add(1)
            .context("Governance fee nonce overflow")?;
        anyhow::ensure!(
            registered.recovery.spending_nonce == before_nonce
                && receipt.nonce_before == before_nonce
                && receipt.nonce_after == expected_nonce
                && plan.reservation.balance_before == before_balance
                && plan.reservation.eligible_before == before_eligible
                && plan.reservation.balance_while_reserved
                    == before_balance.checked_sub(receipt.reserved_cap)
                        .context("Governance fee cap exceeds current balance")?
                && plan.reservation.eligible_while_reserved
                    == before_eligible.checked_sub(receipt.reserved_cap)
                        .context("Governance fee cap exceeds eligible balance")?
                && plan.fee_checkpoint.balances.get(&plan.reservation.asset)
                    == Some(&after_balance)
                && plan.fee_checkpoint.eligible.get(&plan.reservation.asset)
                    == Some(&after_eligible)
                && plan.fee_checkpoint.native_nonces.get(address) == Some(&expected_nonce)
                && before_withheld.checked_add(receipt.charge)
                    == Some(plan.fee_checkpoint.withheld_udrt),
            "Governance fee checkpoint differs from staged account"
        );
        let mut expected_book = book.clone();
        expected_book
            .accounts
            .get_mut(&hex::encode(receipt.actor))
            .context("Governance fee actor disappeared")?
            .recovery
            .spending_nonce = expected_nonce;
        anyhow::ensure!(
            expected_book == plan.authority_checkpoint,
            "Governance authority checkpoint differs from fee nonce"
        );
        let account = staged.account(address)?;
        account.set_balance("udrt", after_balance);
        account.nonce = expected_nonce;
        staged.fee_total = Some(plan.fee_checkpoint.withheld_udrt);
        *self = staged;
        Ok(expected_book)
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
    pub(crate) fn bond_funding(&mut self, owner: &str, amount: u128) -> Result<u128> {
        let rewards = self.reward_plan()?;
        let liquid = self.account(owner)?.balance_of("udgt");
        let timestamp = self.reward_timestamp.context("Missing reward timestamp")?;
        let spendable = if rewards
            .locks
            .get(owner)
            .is_some_and(|lock| !lock.permits_staking)
        {
            rewards.liquid_spendable(
                owner,
                liquid,
                rewards
                    .owner_bonded(owner)?
                    .checked_add(self.pending_bond(owner)?)
                    .context("Owner bond custody exceeds u128")?,
                rewards.unbonding.get(owner).copied().unwrap_or(0),
                timestamp,
            )?
        } else {
            liquid
        };
        if amount > spendable {
            return Err(violation("Bond exceeds permitted liquid DGT").into());
        }
        liquid
            .checked_sub(amount)
            .ok_or_else(|| violation("Insufficient bond funding").into())
    }
    fn validator_plan(
        &self,
        owner: &str,
        nonce: u64,
        operation: Operation,
    ) -> Result<(LifecycleState, Option<PenaltyState>)> {
        let before = self.validators.clone().ok_or_else(|| {
            violation("Validator operations require the validator lifecycle profile")
        })?;
        let mut penalties = self.penalties.clone();
        if let Some(penalties) = &mut penalties {
            penalties.sync_lifecycle(&before, &before)?;
            match &operation {
                Operation::Register { validator, .. }
                | Operation::Bond { validator, .. }
                | Operation::Rotate { validator, .. } => penalties
                    .ensure_validator_allowed(validator)
                    .map_err(|e| violation(&e.to_string()))?,
                Operation::Unbond { .. } | Operation::Exit { .. } => {}
            }
        }
        let mut next = before.clone();
        let height = self.reward_plan()?.last_height;
        next.schedule(height, owner, nonce, operation)
            .map_err(|e| violation(&e.to_string()))?;
        if let Some(penalties) = &mut penalties {
            penalties.sync_lifecycle(&before, &next)?;
        }
        Ok((next, penalties))
    }
    pub(crate) fn reward_bond(&mut self, owner: &str, validator: &str, amount: u128) -> Result<()> {
        let balance = self.bond_funding(owner, amount)?;
        if self.validators.is_some() {
            let (next, penalties) = self.validator_plan(
                owner,
                0,
                Operation::Bond {
                    validator: validator.into(),
                    amount,
                },
            )?;
            self.account(owner)?.set_balance("udgt", balance);
            self.validators = Some(next);
            self.penalties = penalties;
        } else {
            let mut next = self.reward_plan()?;
            next.bond(owner, validator, amount)
                .map_err(|e| violation(&e.to_string()))?;
            self.account(owner)?.set_balance("udgt", balance);
            self.rewards = Some(next);
        }
        Ok(())
    }
    pub(crate) fn reward_begin_unbond(
        &mut self,
        owner: &str,
        validator: &str,
        amount: u128,
    ) -> Result<()> {
        if self.validators.is_some() {
            let (next, penalties) = self.validator_plan(
                owner,
                0,
                Operation::Unbond {
                    validator: validator.into(),
                    amount,
                },
            )?;
            self.validators = Some(next);
            self.penalties = penalties;
        } else {
            let mut next = self.reward_plan()?;
            next.begin_unbond(owner, validator, amount)
                .map_err(|e| violation(&e.to_string()))?;
            self.rewards = Some(next);
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn validator_register(
        &mut self,
        owner: &str,
        nonce: u64,
        validator: &str,
        pubkey_base64: &str,
        proof_base64: &str,
        expires_at_height: u64,
        amount: u128,
    ) -> Result<()> {
        let balance = self.bond_funding(owner, amount)?;
        let (next, penalties) = self.validator_plan(
            owner,
            nonce,
            Operation::Register {
                validator: validator.into(),
                pubkey_base64: pubkey_base64.into(),
                proof_base64: proof_base64.into(),
                expires_at_height,
                amount,
            },
        )?;
        self.account(owner)?.set_balance("udgt", balance);
        self.validators = Some(next);
        self.penalties = penalties;
        Ok(())
    }
    pub(crate) fn validator_rotate_key(
        &mut self,
        owner: &str,
        nonce: u64,
        validator: &str,
        pubkey_base64: &str,
        proof_base64: &str,
        expires_at_height: u64,
    ) -> Result<()> {
        let (next, penalties) = self.validator_plan(
            owner,
            nonce,
            Operation::Rotate {
                validator: validator.into(),
                pubkey_base64: pubkey_base64.into(),
                proof_base64: proof_base64.into(),
                expires_at_height,
            },
        )?;
        self.validators = Some(next);
        self.penalties = penalties;
        Ok(())
    }
    pub(crate) fn validator_exit(
        &mut self,
        owner: &str,
        nonce: u64,
        validator: &str,
    ) -> Result<()> {
        let (next, penalties) = self.validator_plan(
            owner,
            nonce,
            Operation::Exit {
                validator: validator.into(),
            },
        )?;
        self.validators = Some(next);
        self.penalties = penalties;
        Ok(())
    }
    pub(crate) fn validator_withdraw(&mut self, owner: &str, unbond_id: &str) -> Result<()> {
        let mut penalties = self.penalties.clone().ok_or_else(|| {
            violation("Validator withdrawals require qualified evidence and penalty accounting")
        })?;
        let validators = self
            .validators
            .as_ref()
            .context("Penalty custody requires validator lifecycle")?;
        penalties.sync_lifecycle(validators, validators)?;
        let parent_height = self
            .reward_plan()?
            .last_height
            .checked_sub(1)
            .context("Withdrawal requires a settled block interval")?;
        let parent_seconds = crate::consensus_settlement::committed_parent_time(
            &self.storage,
            parent_height
                .checked_add(1)
                .context("Withdrawal height overflow")?,
        )?
        .0;
        let amount = penalties
            .withdraw(owner, unbond_id, parent_height, parent_seconds, validators)
            .map_err(|e| violation(&e.to_string()))?;
        let balance = self
            .account(owner)?
            .balance_of("udgt")
            .checked_add(amount)
            .ok_or_else(|| violation("Withdrawal recipient balance exceeds u128"))?;
        self.account(owner)?.set_balance("udgt", balance);
        self.penalties = Some(penalties);
        Ok(())
    }
    /// Apply engine-validated evidence before any transaction in this block.
    pub(crate) fn apply_validator_evidence(
        &mut self,
        fact: &EvidenceFact,
        historical_time: (u64, i32),
        parent_height: u64,
        parent_time: (u64, i32),
    ) -> Result<()> {
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
    pub(crate) fn reward_claim(&mut self, owner: &str) -> Result<u128> {
        let mut next = self.reward_plan()?;
        let amount = next.claim(owner).map_err(|e| violation(&e.to_string()))?;
        let pool = self
            .reward_pool()?
            .checked_sub(amount)
            .context("Reward liability exceeds pool backing")?;
        let balance = self
            .account(owner)?
            .balance_of("udrt")
            .checked_add(amount)
            .ok_or_else(|| violation("Reward recipient balance exceeds u128"))?;
        self.account(owner)?.set_balance("udrt", balance);
        self.lifecycle.insert(
            b"emission:pool:staking_rewards".to_vec(),
            bincode::serialize(&pool)?,
        );
        self.rewards = Some(next);
        Ok(amount)
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
    pub(crate) fn charge(&mut self, address: &str, fee: u128) -> Result<()> {
        let current = match self.fee_total {
            Some(total) => total,
            None => read::<u128>(&self.storage, FEE_KEY)?.unwrap_or(0),
        };
        let total = current
            .checked_add(fee)
            .context("Withheld fee total exceeds u128")?;
        let account = self.account(address)?;
        let next_nonce = account
            .nonce
            .checked_add(1)
            .context("Account nonce exhausted")?;
        let balance = account
            .balance_of("udrt")
            .checked_sub(fee)
            .context("Insufficient fee balance")?;
        account.set_balance("udrt", balance);
        account.nonce = next_nonce;
        self.fee_total = Some(total);
        Ok(())
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
    pub(crate) fn transfer(
        &mut self,
        from: &str,
        to: &str,
        denom: &str,
        amount: u128,
    ) -> Result<(u128, u128, u128, u128)> {
        let old_from = self.account(from)?.balance_of(denom);
        if denom == "udgt" && from != to {
            if let Some(rewards) = &self.rewards {
                let available = rewards.liquid_spendable(
                    from,
                    old_from,
                    rewards
                        .owner_bonded(from)?
                        .checked_add(self.pending_bond(from)?)
                        .context("Owner bond custody exceeds u128")?,
                    rewards.unbonding.get(from).copied().unwrap_or(0),
                    self.reward_timestamp.context("Missing reward timestamp")?,
                )?;
                if amount > available {
                    return Err(violation("Transfer exceeds vested liquid DGT").into());
                }
            }
        }
        let new_from = old_from
            .checked_sub(amount)
            .ok_or_else(|| violation("Insufficient transfer balance"))?;
        if from == to {
            return Ok((old_from, old_from, old_from, old_from));
        }
        let old_to = self.account(to)?.balance_of(denom);
        let new_to = old_to
            .checked_add(amount)
            .ok_or_else(|| violation("Recipient balance exceeds u128"))?;
        self.account(from)?.set_balance(denom, new_from);
        self.account(to)?.set_balance(denom, new_to);
        Ok((old_from, new_from, old_to, new_to))
    }
    pub(crate) fn register(
        &mut self,
        owner: &str,
        beneficiary: &str,
        period: u64,
        height: u64,
    ) -> Result<()> {
        if period == 0 || owner == beneficiary {
            return Err(violation("Invalid inactivity-switch configuration").into());
        }
        height
            .checked_add(period)
            .ok_or_else(|| violation("Inactivity-switch deadline exceeds u64"))?;
        self.switches.insert(
            owner.into(),
            DeadManSwitchConfig {
                beneficiary: beneficiary.into(),
                period_blocks: period,
                last_active_block: height,
            },
        );
        Ok(())
    }
    fn switch(&mut self, owner: &str) -> Result<DeadManSwitchConfig> {
        if let Some(config) = self.switches.get(owner) {
            return Ok(config.clone());
        }
        read(&self.storage, &dms_key(owner))?
            .ok_or_else(|| violation("No dead man switch registered").into())
    }
    pub(crate) fn ping(&mut self, owner: &str, height: u64) -> Result<()> {
        let mut config = self.switch(owner)?;
        height
            .checked_add(config.period_blocks)
            .ok_or_else(|| violation("Inactivity-switch deadline exceeds u64"))?;
        if height < config.last_active_block {
            return Err(violation("Inactivity-switch height regressed").into());
        }
        config.last_active_block = height;
        self.switches.insert(owner.into(), config);
        Ok(())
    }
    pub(crate) fn beneficiary(&mut self, owner: &str, caller: &str, height: u64) -> Result<String> {
        let config = self.switch(owner)?;
        if config.beneficiary != caller || owner == caller {
            return Err(violation("Caller is not a distinct beneficiary").into());
        }
        let deadline = config
            .last_active_block
            .checked_add(config.period_blocks)
            .ok_or_else(|| violation("Inactivity-switch deadline exceeds u64"))?;
        if height < deadline {
            return Err(violation("Switch has not triggered yet").into());
        }
        Ok(config.beneficiary)
    }
    pub(crate) fn commit(
        self,
        state: &mut State,
        tx: &Transaction,
        receipt: &TxReceipt,
    ) -> Result<()> {
        self.commit_with(state, tx, receipt, |storage, batch| {
            let mut options = WriteOptions::default();
            options.set_sync(true);
            storage.db.write_opt(batch, &options)?;
            Ok(())
        })
    }
    pub(crate) fn writes(&self) -> Result<BTreeMap<Vec<u8>, Vec<u8>>> {
        let mut writes = self.lifecycle.clone();
        if let Some(governance) = &self.governance {
            writes.insert(
                GOVERNANCE_STATE_KEY.as_bytes().to_vec(),
                governance.encode()?,
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
        Ok(writes)
    }
    pub(crate) fn append_receipt(
        storage: &Storage,
        batch: &mut WriteBatch,
        tx: &Transaction,
        receipt: &TxReceipt,
    ) -> Result<()> {
        Self::append_receipt_record(
            batch,
            tx,
            receipt,
            storage.planned_transaction_record(tx, None)?,
        )
    }
    /// The consensus verifier supplies the retained transaction envelope.
    /// This path does not depend on a local pending transaction record.
    pub(crate) fn append_verified_receipt(
        _storage: &Storage,
        batch: &mut WriteBatch,
        tx: &Transaction,
        receipt: &TxReceipt,
        record: &crate::storage::transaction_record::TransactionRecord,
    ) -> Result<()> {
        anyhow::ensure!(
            record.matches(tx)?,
            "Verified record differs from transaction"
        );
        record.require_original_if_signed()?;
        Self::append_receipt_record(batch, tx, receipt, record.encode()?)
    }
    fn append_receipt_record(
        batch: &mut WriteBatch,
        tx: &Transaction,
        receipt: &TxReceipt,
        encoded_record: Vec<u8>,
    ) -> Result<()> {
        anyhow::ensure!(
            receipt.tx_hash == tx.hash,
            "Receipt transaction hash differs"
        );
        let encoded_receipt = serde_json::to_vec(receipt)?;
        let encoded_execution = serde_json::to_vec(&Record {
            version: 1,
            digest: digest(tx)?,
            receipt: receipt.clone(),
        })?;
        batch.put(format!("tx:{}", tx.hash), encoded_record);
        batch.put(format!("rcpt:{}", tx.hash), encoded_receipt);
        batch.put(record_key(&tx.hash), encoded_execution);
        Ok(())
    }
    pub(crate) fn publish(self, state: &mut State) {
        for (address, account) in self.accounts {
            state.accounts.insert(address, account);
        }
    }
    fn commit_with(
        self,
        state: &mut State,
        tx: &Transaction,
        receipt: &TxReceipt,
        write: impl FnOnce(&Storage, WriteBatch) -> Result<()>,
    ) -> Result<()> {
        crate::genesis::reject_consensus_state(&self.storage)?;
        let mut batch = WriteBatch::default();
        for (key, value) in self.writes()? {
            batch.put(key, value);
        }
        Self::append_receipt(&self.storage, &mut batch, tx, receipt)?;
        write(&self.storage, batch)?;
        // Durable state is authoritative. No cache change occurs before commit.
        for (address, account) in self.accounts {
            state.accounts.insert(address, account);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{execution::execute_transaction, gas::GasSchedule, storage::tx::TxMessage};
    use rocksdb::IteratorMode;
    #[test]
    fn verified_receipt_retains_supplied_envelope_without_local_pending_record() {
        use crate::storage::transaction_record::{SignedEnvelope, TransactionRecord};
        let (state, _dir) = state();
        let original = dytallix_protocol_types::Tx {
            chain_id: "test".into(),
            nonce: 0,
            msgs: vec![dytallix_protocol_types::Msg::Data {
                from: "alice".into(),
                data: "payload".into(),
            }],
            fee: 1,
            memo: "memo".into(),
        };
        // Storage fixture only. Signature verification belongs to the caller.
        let envelope = SignedEnvelope {
            tx: original.clone(),
            public_key: "fixture-public-key".into(),
            signature: "fixture-signature".into(),
            algorithm: "fixture-algorithm".into(),
            version: 1,
        };
        let mut tx = Transaction::new(
            original.tx_hash().unwrap(),
            "alice",
            "alice",
            0,
            1,
            0,
            Some(envelope.signature.clone()),
        )
        .with_pqc(&envelope.public_key, "test", "memo");
        tx.messages = Some(vec![TxMessage::Data {
            from: "alice".into(),
            data: "payload".into(),
        }]);
        let record = TransactionRecord::new(tx.clone(), Some(envelope.clone())).unwrap();
        let receipt = TxReceipt::pending(&tx);
        assert!(state
            .storage
            .db
            .get(format!("tx:{}", tx.hash))
            .unwrap()
            .is_none());
        let mut batch = WriteBatch::default();
        Settlement::append_verified_receipt(&state.storage, &mut batch, &tx, &receipt, &record)
            .unwrap();
        state.storage.db.write(batch).unwrap();
        let raw = state
            .storage
            .db
            .get(format!("tx:{}", tx.hash))
            .unwrap()
            .unwrap();
        let stored = TransactionRecord::decode(&tx.hash, &raw).unwrap();
        assert_eq!(stored.signed_envelope, Some(envelope));
        assert!(stored.matches(&tx).unwrap());
    }
    #[test]
    fn verified_receipt_rejects_mismatched_record_or_receipt_before_batch_writes() {
        use crate::storage::transaction_record::TransactionRecord;
        let (state, _dir) = state();
        let tx = tx("verified");
        let mut receipt = TxReceipt::pending(&tx);
        let mut different = tx.clone();
        different.amount += 1;
        let mut batch = WriteBatch::default();
        assert!(Settlement::append_verified_receipt(
            &state.storage,
            &mut batch,
            &tx,
            &receipt,
            &TransactionRecord::new(different, None).unwrap()
        )
        .is_err());
        assert_eq!(batch.len(), 0);
        receipt.tx_hash = "different".into();
        assert!(Settlement::append_verified_receipt(
            &state.storage,
            &mut batch,
            &tx,
            &receipt,
            &TransactionRecord::new(tx.clone(), None).unwrap()
        )
        .is_err());
        assert_eq!(batch.len(), 0);
        let mut signed = tx;
        signed.signature = Some("missing-envelope".into());
        receipt = TxReceipt::pending(&signed);
        assert!(Settlement::append_verified_receipt(
            &state.storage,
            &mut batch,
            &signed,
            &receipt,
            &TransactionRecord::new(signed.clone(), None).unwrap()
        )
        .is_err());
        assert_eq!(batch.len(), 0);
    }
    fn state() -> (State, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mut state = State::new(Arc::new(Storage::open(dir.path().join("db")).unwrap()));
        state.set_balance("alice", "udgt", 1000);
        state.set_balance("alice", "udrt", 1_000_000);
        state.set_balance("bob", "udgt", 100);
        state.set_balance("bob", "udrt", 1_000_000);
        (state, dir)
    }
    fn tx(hash: &str) -> Transaction {
        Transaction::new(hash, "alice", "bob", 100, 50_000, 0, None).with_gas(50_000, 1)
    }
    fn snapshot(state: &State) -> BTreeMap<Vec<u8>, Vec<u8>> {
        state
            .storage
            .db
            .iterator(IteratorMode::Start)
            .map(|item| {
                let (k, v) = item.unwrap();
                (k.to_vec(), v.to_vec())
            })
            .collect()
    }
    fn run(state: &mut State, tx: &Transaction, height: u64) -> crate::execution::ExecutionResult {
        execute_transaction(tx, state, height, 0, &GasSchedule::default(), None).unwrap()
    }
    fn fee(state: &State) -> u128 {
        read(&state.storage, FEE_KEY).unwrap().unwrap_or(0)
    }
    fn send(from: &str, to: &str, denom: &str, amount: u128) -> TxMessage {
        TxMessage::Send {
            from: from.into(),
            to: to.into(),
            denom: denom.into(),
            amount,
        }
    }
    #[test]
    fn validator_withdrawal_is_disabled_without_mutating_custody() {
        let (state, _dir) = state();
        let before = snapshot(&state);
        let mut staged = Settlement::new(state.storage.clone());
        let error = staged.validator_withdraw("alice", "entry-1").unwrap_err();
        assert!(error.downcast_ref::<RuleViolation>().is_some());
        assert!(staged.writes().unwrap().is_empty());
        assert_eq!(snapshot(&state), before);
    }
    #[test]
    fn legacy_and_multi_message_self_transfers_conserve_balances() {
        for denom in ["udgt", "udrt"] {
            for multi in [false, true] {
                let (mut state, _dir) = state();
                let mut tx = tx("self");
                tx.to = "alice".into();
                tx.denom = denom.into();
                if multi {
                    tx.messages = Some(vec![send("alice", "alice", denom, 100)]);
                }
                assert!(run(&mut state, &tx, 10).success);
                assert_eq!(state.get_balance("alice", "udgt"), 1000);
                assert_eq!(state.get_balance("alice", "udrt"), 950_000);
                assert_eq!(state.nonce_of("alice"), 1);
                assert_eq!(fee(&state), 50_000);
            }
        }
    }
    #[test]
    fn repeated_accounts_use_prior_staged_changes() {
        let (mut state, _dir) = state();
        let mut tx = tx("multi");
        tx.messages = Some(vec![
            send("alice", "bob", "udgt", 100),
            send("alice", "carol", "udgt", 200),
        ]);
        assert!(run(&mut state, &tx, 10).success);
        assert_eq!(state.get_balance("alice", "udgt"), 700);
        assert_eq!(state.get_balance("bob", "udgt"), 200);
        assert_eq!(state.get_balance("carol", "udgt"), 200);
    }
    #[test]
    fn debit_and_credit_limits_revert_messages_but_keep_fee_and_nonce() {
        for overflow in [false, true] {
            let (mut state, _dir) = state();
            let mut tx = tx("bounds");
            if overflow {
                state.set_balance("bob", "udgt", u128::MAX);
            } else {
                tx.amount = 1001;
            }
            let before = state.get_balance("bob", "udgt");
            let result = run(&mut state, &tx, 10);
            assert!(!result.success);
            assert_eq!(state.get_balance("alice", "udgt"), 1000);
            assert_eq!(state.get_balance("bob", "udgt"), before);
            assert_eq!(state.get_balance("alice", "udrt"), 950_000);
            assert_eq!(state.nonce_of("alice"), 1);
            assert_eq!(fee(&state), 50_000);
            assert!(state.storage.get_receipt("bounds").is_some());
        }
    }
    #[test]
    fn later_message_failure_discards_switch_and_transfer_changes() {
        let (mut state, _dir) = state();
        let mut tx = tx("rollback");
        tx.messages = Some(vec![
            TxMessage::DmsRegister {
                from: "alice".into(),
                beneficiary: "bob".into(),
                period: 5,
            },
            send("alice", "bob", "udgt", 100),
            send("alice", "bob", "udgt", 1000),
        ]);
        assert!(!run(&mut state, &tx, 10).success);
        assert!(state.storage.db.get(dms_key("alice")).unwrap().is_none());
        assert_eq!(state.get_balance("alice", "udgt"), 1000);
        assert_eq!(state.get_balance("bob", "udgt"), 100);
        assert_eq!(state.nonce_of("alice"), 1);
        assert_eq!(fee(&state), 50_000);
    }
    #[test]
    fn switch_register_ping_and_claim_share_staged_state() {
        let (mut state, _dir) = state();
        let mut registration = tx("register");
        registration.messages = Some(vec![
            TxMessage::DmsRegister {
                from: "alice".into(),
                beneficiary: "bob".into(),
                period: 5,
            },
            TxMessage::DmsPing {
                from: "alice".into(),
            },
        ]);
        assert!(run(&mut state, &registration, 10).success);
        let mut claim =
            Transaction::new("claim", "bob", "alice", 0, 50_000, 0, None).with_gas(50_000, 1);
        claim.messages = Some(vec![TxMessage::DmsClaim {
            from: "bob".into(),
            owner: "alice".into(),
        }]);
        assert!(run(&mut state, &claim, 15).success);
        assert_eq!(state.get_balance("alice", "udgt"), 0);
        assert_eq!(state.get_balance("bob", "udgt"), 1100);
        assert_eq!(state.get_balance("alice", "udrt"), 0);
        assert_eq!(state.get_balance("bob", "udrt"), 1_900_000);
        assert_eq!(fee(&state), 100_000);
    }
    #[test]
    fn switch_period_and_deadline_limits_are_checked() {
        for (period, height) in [(u128::MAX, 10), (5, u64::MAX)] {
            let (mut state, _dir) = state();
            let mut tx = tx("period");
            tx.messages = Some(vec![TxMessage::DmsRegister {
                from: "alice".into(),
                beneficiary: "bob".into(),
                period,
            }]);
            assert!(!run(&mut state, &tx, height).success);
            assert!(state.storage.db.get(dms_key("alice")).unwrap().is_none());
        }
    }
    #[test]
    fn rejection_before_fee_acceptance_changes_no_state() {
        for case in 0..5 {
            let (mut state, _dir) = state();
            let mut tx = tx("reject");
            match case {
                0 => tx.nonce = 1,
                1 => {
                    tx.nonce = u64::MAX;
                    state.storage.set_nonce_db("alice", u64::MAX).unwrap();
                }
                2 => tx.messages = Some(vec![send("bob", "alice", "udgt", 1)]),
                3 => {
                    tx.gas_limit = 0;
                    tx.fee = u128::MAX;
                }
                _ => state.set_balance("alice", "udrt", 1),
            }
            let before = snapshot(&state);
            let cache = serde_json::to_value(&state.accounts).unwrap();
            assert!(!run(&mut state, &tx, 10).success);
            assert_eq!(snapshot(&state), before);
            assert_eq!(serde_json::to_value(&state.accounts).unwrap(), cache);
        }
    }
    #[test]
    fn corrupt_reads_fail_without_committing_a_fee_checkpoint() {
        for key in [
            "acct:balances:alice",
            "acct:nonce:alice",
            FEE_KEY,
            "dms:config:alice",
        ] {
            let (mut state, _dir) = state();
            state.storage.db.put(key, b"invalid").unwrap();
            let mut tx = tx("corrupt");
            tx.messages = Some(vec![TxMessage::DmsPing {
                from: "alice".into(),
            }]);
            let before = snapshot(&state);
            let cache = serde_json::to_value(&state.accounts).unwrap();
            assert!(
                execute_transaction(&tx, &mut state, 10, 0, &GasSchedule::default(), None).is_err()
            );
            assert_eq!(snapshot(&state), before);
            assert_eq!(serde_json::to_value(&state.accounts).unwrap(), cache);
        }
    }
    #[test]
    fn consensus_records_reject_standalone_transaction_and_commit_without_writes() {
        for key in ["consensus:v1:config", "consensus:unknown:orphan"] {
            let (mut state, _dir) = state();
            state.storage.db.put(key, b"fixture").unwrap();
            let before = snapshot(&state);
            let cache = serde_json::to_value(&state.accounts).unwrap();
            let tx = tx("consensus-exclusion");
            assert!(
                execute_transaction(&tx, &mut state, 10, 0, &GasSchedule::default(), None).is_err()
            );
            let mut staged = Settlement::new(state.storage.clone());
            staged.charge("alice", 50_000).unwrap();
            let receipt = TxReceipt::success(&tx, 1000, 50_000, 1, 10, 0);
            assert!(staged
                .commit_with(&mut state, &tx, &receipt, |_, _| {
                    panic!("Consensus state reached standalone writer")
                })
                .is_err());
            assert_eq!(snapshot(&state), before);
            assert_eq!(serde_json::to_value(&state.accounts).unwrap(), cache);
        }
    }
    #[test]
    fn failed_batch_write_publishes_neither_storage_nor_cache() {
        let (mut state, _dir) = state();
        let before = snapshot(&state);
        let cache = serde_json::to_value(&state.accounts).unwrap();
        let mut staged = Settlement::new(state.storage.clone());
        staged.charge("alice", 50_000).unwrap();
        staged.transfer("alice", "bob", "udgt", 100).unwrap();
        staged.register("alice", "bob", 5, 10).unwrap();
        let tx = tx("write-failure");
        let receipt = TxReceipt::success(&tx, 1000, 50_000, 1, 10, 0);
        assert!(staged
            .commit_with(&mut state, &tx, &receipt, |_, batch| {
                assert!(batch.len() >= 7);
                bail!("Injected pre-write failure")
            })
            .is_err());
        assert_eq!(snapshot(&state), before);
        assert_eq!(serde_json::to_value(&state.accounts).unwrap(), cache);
    }
    #[test]
    fn reopen_and_matching_retry_do_not_charge_twice() {
        let (mut state, dir) = state();
        let mut tx = tx("restart");
        tx.messages = Some(vec![
            TxMessage::DmsRegister {
                from: "alice".into(),
                beneficiary: "bob".into(),
                period: 5,
            },
            send("alice", "bob", "udgt", 100),
        ]);
        let result = run(&mut state, &tx, 10);
        assert!(result.success);
        let before = snapshot(&state);
        drop(state);
        let mut state = State::new(Arc::new(Storage::open(dir.path().join("db")).unwrap()));
        let retry = run(&mut state, &tx, 10);
        assert!(retry.success);
        assert_eq!(snapshot(&state), before);
        assert_eq!(state.get_balance("alice", "udgt"), 900);
        assert_eq!(state.get_balance("alice", "udrt"), 950_000);
        assert_eq!(state.nonce_of("alice"), 1);
        assert_eq!(fee(&state), 50_000);
        assert!(state.storage.get_receipt("restart").unwrap().success);
        assert!(
            read::<DeadManSwitchConfig>(&state.storage, &dms_key("alice"))
                .unwrap()
                .is_some()
        );
        let mut changed = tx.clone();
        changed.amount += 1;
        assert!(
            execute_transaction(&changed, &mut state, 10, 0, &GasSchedule::default(), None)
                .is_err()
        );
        assert!(
            execute_transaction(&tx, &mut state, 11, 0, &GasSchedule::default(), None).is_err()
        );
        assert_eq!(snapshot(&state), before);
    }
    #[test]
    fn concurrent_state_handles_serialize_one_settlement() {
        let (state, _dir) = state();
        let other = state.clone();
        let tx = tx("concurrent");
        let other_tx = tx.clone();
        let one = std::thread::spawn(move || {
            let mut state = state;
            assert!(run(&mut state, &tx, 10).success);
            state
        });
        let two = std::thread::spawn(move || {
            let mut state = other;
            assert!(run(&mut state, &other_tx, 10).success);
        });
        let mut state = one.join().unwrap();
        two.join().unwrap();
        state.accounts.clear();
        assert_eq!(state.get_balance("alice", "udgt"), 900);
        assert_eq!(state.nonce_of("alice"), 1);
        assert_eq!(fee(&state), 50_000);
    }
    #[test]
    fn fee_counter_overflow_changes_no_state() {
        let (mut state, _dir) = state();
        state
            .storage
            .db
            .put(FEE_KEY, bincode::serialize(&u128::MAX).unwrap())
            .unwrap();
        let before = snapshot(&state);
        assert!(execute_transaction(
            &tx("fee-overflow"),
            &mut state,
            10,
            0,
            &GasSchedule::default(),
            None
        )
        .is_err());
        assert_eq!(snapshot(&state), before);
        assert_eq!(state.nonce_of("alice"), 0);
    }
    #[test]
    fn burn_diagnostic_is_updated_once_for_a_matching_retry() {
        let (mut state, _dir) = state();
        let tx = tx("diagnostic");
        let mut engine = crate::runtime::fee_burn::FeeBurnEngine::new();
        for _ in 0..2 {
            assert!(
                execute_transaction(
                    &tx,
                    &mut state,
                    10,
                    0,
                    &GasSchedule::default(),
                    Some(&mut engine)
                )
                .unwrap()
                .success
            );
        }
        assert_eq!(engine.burn_events.len(), 1);
        assert_eq!(engine.get_total_burned("udgt"), 12_500);
        assert_eq!(state.get_balance("alice", "udgt"), 900);
        assert_eq!(fee(&state), 50_000);
    }
}

#[cfg(test)]
#[path = "validator_custody_tests.rs"]
mod validator_custody_tests;
