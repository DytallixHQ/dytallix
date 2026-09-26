//! Ordered proposal and deposit admission for one future governance block.
//! Inputs are already authenticated planning facts, not an execution permit.
//! The caller must bind each action to a signed v3 transaction and obtain all
//! account snapshots from the verified staged state. This module writes nothing.

use super::{
    governance_ballot::{BallotStatus, BondSnapshot, Rules as BallotRules, VoteChoice},
    governance_custody_transition::{AccountDgt, AccountWrite},
    governance_deposit_stage::{DepositClosePlan, DepositRules, DepositStage, DepositStageStatus},
    governance_escrow::{DepositEscrow, OwnerId},
    governance_state::{GovernanceState, ProposalRecord},
    validator_lifecycle::LifecycleState,
};
use crate::recovery_fees::RecoveryBook;
use anyhow::{ensure, Context, Result};
use std::collections::{BTreeMap, BTreeSet};

const DGT_SUPPLY: u128 = dytallix_protocol_types::units::DGT_TOTAL_BASE_UNITS;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdmissionAction {
    Proposal {
        proposal_id: u64,
        action_class: u16,
        action_data: Vec<u8>,
        action_digest: [u8; 32],
    },
    Deposit {
        proposal_id: u64,
        owner: OwnerId,
        amount_udgt: u128,
    },
    Vote {
        proposal_id: u64,
        owner: OwnerId,
        choice: VoteChoice,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderedAdmissionPlan {
    pub parent_state_bytes: Vec<u8>,
    pub state_bytes: Vec<u8>,
    pub next_state: GovernanceState,
    pub admitted_proposal_ids: Vec<u64>,
    pub rejected_proposal_ids: Vec<u64>,
    pub started_voting_proposal_ids: Vec<u64>,
    pub rejected_ballot_proposal_ids: Vec<u64>,
    pub passed_ballot_proposal_ids: Vec<u64>,
    pub account_writes: Vec<AccountWrite>,
}

/// The consensus caller must read all fields from the same verified committed
/// parent. This pure planner checks their height and chain bindings but cannot
/// prove the application hash came from storage.
pub struct ClosureParent<'a> {
    pub lifecycle: &'a LifecycleState,
    pub recovery: &'a RecoveryBook,
    pub app_hash: [u8; 32],
    pub ballot_rules: &'a BallotRules,
}

fn credit_refund(
    old_escrow: &DepositEscrow,
    credited: BTreeMap<OwnerId, u128>,
    accounts: &mut BTreeMap<OwnerId, AccountDgt>,
    affected: &mut BTreeSet<OwnerId>,
) -> Result<()> {
    for (owner, balance) in credited {
        let account = accounts
            .get_mut(&owner)
            .context("Governance refund owner disappeared")?;
        let amount = old_escrow.deposited_by(owner);
        account.balance_udgt = balance;
        account.spendable_udgt = account
            .spendable_udgt
            .checked_add(amount)
            .context("Governance refund spendable DGT overflow")?;
        ensure!(
            account.spendable_udgt <= account.balance_udgt,
            "Governance refund exceeds DGT balance"
        );
        affected.insert(owner);
    }
    Ok(())
}

/// Plan due automatic transitions and ordered transaction actions. A proposal
/// admitted in a block without a due transition can receive a later deposit
/// in that block. Mixing due transitions with user actions remains disabled
/// until their exact order and same-block spending rule are approved.
/// The planner does not execute proposal actions, settle fees/nonces, or write
/// storage. The caller must bind every action and owner to a signed v3 request.
pub fn plan_ordered_admission_block(
    parent: &GovernanceState,
    finalized_height: u64,
    rules: &DepositRules,
    max_depositors: u32,
    account_snapshots: &BTreeMap<OwnerId, AccountDgt>,
    closure_parent: Option<&ClosureParent<'_>>,
    actions: &[AdmissionAction],
) -> Result<OrderedAdmissionPlan> {
    parent.validate()?;
    rules.validate()?;
    ensure!(max_depositors > 0, "Governance depositor bound is absent");
    ensure!(
        parent.finalized_height().checked_add(1) == Some(finalized_height),
        "Governance admission height must advance by one"
    );
    for account in account_snapshots.values() {
        ensure!(
            account.spendable_udgt <= account.balance_udgt && account.balance_udgt <= DGT_SUPPLY,
            "Invalid governance DGT account snapshot"
        );
    }

    let mut records = parent.proposals().clone();
    let mut accounts = account_snapshots.clone();
    let mut next_id = parent.next_proposal_id();
    let mut admitted_proposal_ids = Vec::new();
    let mut rejected_proposal_ids = Vec::new();
    let mut started_voting_proposal_ids = Vec::new();
    let mut rejected_ballot_proposal_ids = Vec::new();
    let mut passed_ballot_proposal_ids = Vec::new();
    let mut affected = BTreeSet::new();
    let funded_close_due = records.values().any(|record| {
        record.stage().status() == DepositStageStatus::Collecting
            && record.stage().close_height() == finalized_height
            && record.escrow().total_udgt() >= record.stage().rules().minimum_deposit_udgt
    });
    let voting_snapshot = if funded_close_due {
        let source = closure_parent.context("Funded governance close requires finalized parent")?;
        source.ballot_rules.validate()?;
        ensure!(
            source.ballot_rules.chain_id == parent.chain_id()
                && source.ballot_rules.genesis_digest == parent.genesis_digest()
                && source.lifecycle.config.chain_id == parent.chain_id()
                && source.recovery.accounts.values().all(|account| {
                    account.recovery.domain.chain_id == parent.chain_id()
                        && account.recovery.domain.genesis_digest == parent.genesis_digest()
                }),
            "Governance close parent chain differs"
        );
        Some(BondSnapshot::from_finalized_registered_accounts(
            source.lifecycle,
            source.recovery,
            parent.finalized_height(),
            source.app_hash,
            source.ballot_rules.max_voters,
        )?)
    } else {
        None
    };
    for (proposal_id, record) in &mut records {
        if record.stage().status() != DepositStageStatus::Collecting
            || record.stage().close_height() != finalized_height
        {
            continue;
        }
        let voting = voting_snapshot.as_ref().map(|snapshot| {
            (
                closure_parent
                    .expect("funded close parent checked")
                    .ballot_rules
                    .clone(),
                snapshot.clone(),
            )
        });
        match record
            .stage()
            .plan_close(record.escrow(), finalized_height, voting)?
        {
            DepositClosePlan::RejectAndRefund {
                next_stage,
                rejection,
            } => {
                let mut balances = BTreeMap::new();
                for owner in record.escrow().deposits().keys() {
                    balances.insert(
                        *owner,
                        accounts
                            .get(owner)
                            .context("Governance refund owner snapshot is absent")?
                            .balance_udgt,
                    );
                }
                let (escrow, credited) = record
                    .escrow()
                    .plan_rejected_deposit_refund(&rejection, &balances)?;
                credit_refund(record.escrow(), credited, &mut accounts, &mut affected)?;
                *record = ProposalRecord::new(next_stage, escrow, None);
                rejected_proposal_ids.push(*proposal_id);
            }
            DepositClosePlan::StartVoting { next_stage, ballot } => {
                *record = ProposalRecord::new(next_stage, record.escrow().clone(), Some(ballot));
                started_voting_proposal_ids.push(*proposal_id);
            }
        }
    }
    for (proposal_id, record) in &mut records {
        let Some(ballot) = record.ballot() else {
            continue;
        };
        if ballot.status() != &BallotStatus::Voting {
            continue;
        }
        let closing_height = ballot
            .snapshot()
            .finalized_height
            .checked_add(2)
            .and_then(|height| height.checked_add(ballot.rules().voting_period_blocks))
            .context("Governance ballot closing height overflow")?;
        if closing_height != finalized_height {
            continue;
        }
        let mut next_ballot = ballot.clone();
        next_ballot.close(finalized_height)?;
        let escrow = if next_ballot.status() == &BallotStatus::Rejected {
            let mut balances = BTreeMap::new();
            for owner in record.escrow().deposits().keys() {
                balances.insert(
                    *owner,
                    accounts
                        .get(owner)
                        .context("Governance ballot refund owner snapshot is absent")?
                        .balance_udgt,
                );
            }
            let (escrow, credited) = record.escrow().plan_refund(&next_ballot, &balances)?;
            credit_refund(record.escrow(), credited, &mut accounts, &mut affected)?;
            rejected_ballot_proposal_ids.push(*proposal_id);
            escrow
        } else {
            passed_ballot_proposal_ids.push(*proposal_id);
            record.escrow().clone()
        };
        *record = ProposalRecord::new(record.stage().clone(), escrow, Some(next_ballot));
    }
    ensure!(
        actions.is_empty()
            || (rejected_proposal_ids.is_empty()
                && started_voting_proposal_ids.is_empty()
                && rejected_ballot_proposal_ids.is_empty()
                && passed_ballot_proposal_ids.is_empty()),
        "Governance automatic and user-action order needs approval"
    );
    ensure!(
        records.values().all(|record| {
            !matches!(
                record.ballot().map(|ballot| ballot.status()),
                Some(BallotStatus::Passed { execute_at_height }) if *execute_at_height <= finalized_height
            )
        }),
        "Due governance action requires an approved execution transition"
    );
    for action in actions {
        match action {
            AdmissionAction::Proposal {
                proposal_id,
                action_class,
                action_data,
                action_digest,
            } => {
                ensure!(
                    *proposal_id == next_id,
                    "Governance proposal ID differs from staged sequence"
                );
                next_id = next_id
                    .checked_add(1)
                    .context("Governance proposal ID overflow")?;
                let stage = DepositStage::new(
                    rules.clone(),
                    *proposal_id,
                    *action_class,
                    action_data.clone(),
                    *action_digest,
                    finalized_height,
                )?;
                let escrow = DepositEscrow::new(*proposal_id, max_depositors)?;
                ensure!(
                    records
                        .insert(*proposal_id, ProposalRecord::new(stage, escrow, None))
                        .is_none(),
                    "Governance proposal ID reused"
                );
                admitted_proposal_ids.push(*proposal_id);
            }
            AdmissionAction::Deposit {
                proposal_id,
                owner,
                amount_udgt,
            } => {
                let record = records
                    .get_mut(proposal_id)
                    .context("Governance deposit precedes its proposal")?;
                let account = accounts
                    .get_mut(owner)
                    .context("Governance deposit account snapshot is absent")?;
                let (escrow, balance) = record.stage().plan_deposit(
                    record.escrow(),
                    *owner,
                    *amount_udgt,
                    account.balance_udgt,
                    account.spendable_udgt,
                    finalized_height,
                )?;
                account.balance_udgt = balance;
                account.spendable_udgt = account
                    .spendable_udgt
                    .checked_sub(*amount_udgt)
                    .context("Governance deposit exceeds spendable DGT")?;
                *record =
                    ProposalRecord::new(record.stage().clone(), escrow, record.ballot().cloned());
                affected.insert(*owner);
            }
            AdmissionAction::Vote {
                proposal_id,
                owner,
                choice,
            } => {
                let record = records
                    .get_mut(proposal_id)
                    .context("Governance vote proposal is absent")?;
                let mut ballot = record
                    .ballot()
                    .context("Governance vote ballot is absent")?
                    .clone();
                ballot.vote(&hex::encode(owner), *choice, finalized_height)?;
                *record = ProposalRecord::new(
                    record.stage().clone(),
                    record.escrow().clone(),
                    Some(ballot),
                );
            }
        }
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
        "Governance ordered DGT custody does not conserve supply"
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
    Ok(OrderedAdmissionPlan {
        parent_state_bytes: parent.encode()?,
        state_bytes: next_state.encode()?,
        next_state,
        admitted_proposal_ids,
        rejected_proposal_ids,
        started_voting_proposal_ids,
        rejected_ballot_proposal_ids,
        passed_ballot_proposal_ids,
        account_writes,
    })
}

#[cfg(test)]
#[path = "governance_ordered_admission_tests.rs"]
mod tests;
