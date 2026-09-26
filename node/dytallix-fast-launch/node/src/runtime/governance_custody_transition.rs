//! Pure DGT custody plan for deposits and below-minimum proposal refunds.
//! The consensus caller must obtain all inputs from one finalized parent and
//! commit every returned write in one batch. This module has no storage access.

use super::{
    governance_deposit_stage::DepositClosePlan,
    governance_escrow::OwnerId,
    governance_state::{GovernanceState, ProposalRecord, STATE_KEY},
};
use anyhow::{ensure, Context, Result};
use std::collections::{BTreeMap, BTreeSet};

const DGT_SUPPLY: u128 = dytallix_protocol_types::units::DGT_TOTAL_BASE_UNITS;

/// Total and spendable DGT from the same finalized parent as governance state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccountDgt {
    pub balance_udgt: u128,
    pub spendable_udgt: u128,
}

impl AccountDgt {
    fn validate(self) -> Result<()> {
        ensure!(
            self.spendable_udgt <= self.balance_udgt && self.balance_udgt <= DGT_SUPPLY,
            "Invalid governance account DGT snapshot"
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deposit {
    pub proposal_id: u64,
    pub owner: OwnerId,
    pub amount_udgt: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountWrite {
    pub owner: OwnerId,
    pub balance_before_udgt: u128,
    pub balance_after_udgt: u128,
    pub spendable_before_udgt: u128,
    pub spendable_after_udgt: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CustodyEffect {
    Deposit {
        proposal_id: u64,
        owner: OwnerId,
        amount_udgt: u128,
    },
    Refund {
        proposal_id: u64,
        owner: OwnerId,
        amount_udgt: u128,
    },
}

/// These bytes and account writes form one indivisible storage update.
/// The caller must also settle authentication, nonce, and fee effects atomically.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustodyPlan {
    pub state_key: &'static str,
    pub parent_state_bytes: Vec<u8>,
    pub state_bytes: Vec<u8>,
    pub next_state: GovernanceState,
    pub account_writes: Vec<AccountWrite>,
    pub effects: Vec<CustodyEffect>,
}

/// Plan deposits and below-minimum refunds at exactly one next finalized height.
/// The input account map must contain every affected owner. Deposit instructions
/// must follow finalized transaction order. This subset does not admit proposals,
/// start ballots, record votes, or execute actions. A future consensus caller
/// must place proposal admission before any same-block deposit for that proposal
/// and include both effects in one atomic block transition.
pub fn plan_custody_block(
    parent: &GovernanceState,
    finalized_height: u64,
    account_snapshots: &BTreeMap<OwnerId, AccountDgt>,
    deposits: Vec<Deposit>,
    rejected_proposals: &BTreeSet<u64>,
) -> Result<CustodyPlan> {
    parent.validate()?;
    ensure!(
        parent.finalized_height().checked_add(1) == Some(finalized_height),
        "Governance custody height must advance by one"
    );
    for account in account_snapshots.values() {
        account.validate()?;
    }
    let mut records = parent.proposals().clone();
    let mut accounts = account_snapshots.clone();
    let mut affected = BTreeSet::new();
    let mut effects = Vec::new();
    for deposit in deposits {
        let record = records
            .get_mut(&deposit.proposal_id)
            .context("Governance deposit proposal is absent")?;
        let account = accounts
            .get_mut(&deposit.owner)
            .context("Governance deposit owner snapshot is absent")?;
        let (escrow, balance) = record.stage().plan_deposit(
            record.escrow(),
            deposit.owner,
            deposit.amount_udgt,
            account.balance_udgt,
            account.spendable_udgt,
            finalized_height,
        )?;
        account.balance_udgt = balance;
        account.spendable_udgt -= deposit.amount_udgt;
        *record = ProposalRecord::new(record.stage().clone(), escrow, record.ballot().cloned());
        affected.insert(deposit.owner);
        effects.push(CustodyEffect::Deposit {
            proposal_id: deposit.proposal_id,
            owner: deposit.owner,
            amount_udgt: deposit.amount_udgt,
        });
    }

    for proposal_id in rejected_proposals {
        let record = records
            .get_mut(proposal_id)
            .context("Governance rejected proposal is absent")?;
        let DepositClosePlan::RejectAndRefund {
            next_stage,
            rejection,
        } = record
            .stage()
            .plan_close(record.escrow(), finalized_height, None)?
        else {
            anyhow::bail!("Governance proposal met its deposit minimum; ballot rules are required")
        };
        let mut current = BTreeMap::new();
        for owner in record.escrow().deposits().keys() {
            current.insert(
                *owner,
                accounts
                    .get(owner)
                    .context("Governance refund owner snapshot is absent")?
                    .balance_udgt,
            );
        }
        let (escrow, credited) = record
            .escrow()
            .plan_rejected_deposit_refund(&rejection, &current)?;
        for (owner, balance) in credited {
            let account = accounts
                .get_mut(&owner)
                .expect("refund owner checked above");
            let amount = record.escrow().deposited_by(owner);
            account.balance_udgt = balance;
            account.spendable_udgt = account
                .spendable_udgt
                .checked_add(amount)
                .context("Governance refund spendable DGT overflow")?;
            account.validate()?;
            affected.insert(owner);
            effects.push(CustodyEffect::Refund {
                proposal_id: *proposal_id,
                owner,
                amount_udgt: amount,
            });
        }
        *record = ProposalRecord::new(next_stage, escrow, None);
    }

    let next_state = parent.plan_commit(finalized_height, records)?;
    let before_accounts = affected.iter().try_fold(0u128, |sum, owner| {
        sum.checked_add(account_snapshots[owner].balance_udgt)
            .context("Governance account sum overflow")
    })?;
    let after_accounts = affected.iter().try_fold(0u128, |sum, owner| {
        sum.checked_add(accounts[owner].balance_udgt)
            .context("Governance account sum overflow")
    })?;
    let before_custody = before_accounts
        .checked_add(parent.held_total_udgt()?)
        .context("Governance before-custody sum overflow")?;
    let after_custody = after_accounts
        .checked_add(next_state.held_total_udgt()?)
        .context("Governance after-custody sum overflow")?;
    ensure!(
        before_custody == after_custody && before_custody <= DGT_SUPPLY,
        "Governance custody DGT conservation failed"
    );
    let account_writes = affected
        .into_iter()
        .map(|owner| AccountWrite {
            owner,
            balance_before_udgt: account_snapshots[&owner].balance_udgt,
            balance_after_udgt: accounts[&owner].balance_udgt,
            spendable_before_udgt: account_snapshots[&owner].spendable_udgt,
            spendable_after_udgt: accounts[&owner].spendable_udgt,
        })
        .collect();
    let parent_state_bytes = parent.encode()?;
    let state_bytes = next_state.encode()?;
    Ok(CustodyPlan {
        state_key: STATE_KEY,
        parent_state_bytes,
        state_bytes,
        next_state,
        account_writes,
        effects,
    })
}

#[cfg(test)]
#[path = "governance_custody_transition_tests.rs"]
mod tests;
