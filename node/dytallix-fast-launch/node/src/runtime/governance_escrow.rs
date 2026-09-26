//! Accounted DGT deposit plans for a future consensus-owned governance state.
//! A caller must commit the returned escrow and account balances in one batch.
use super::governance_ballot::{Ballot, BallotStatus};
use super::governance_deposit_stage::DepositStageRejection;
use anyhow::{ensure, Context, Result};
use bincode::Options;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const MAX_STATE_BYTES: usize = 8 * 1024 * 1024;
const DGT_SUPPLY: u128 = dytallix_protocol_types::units::DGT_TOTAL_BASE_UNITS;
pub type OwnerId = [u8; 32];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalOutcome {
    Rejected,
    Executed,
    FailedExecution,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EscrowStatus {
    Holding,
    Refunded(TerminalOutcome),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepositEscrow {
    proposal_id: u64,
    max_depositors: u32,
    deposits: BTreeMap<OwnerId, u128>,
    total_udgt: u128,
    status: EscrowStatus,
}

impl DepositEscrow {
    pub fn new(proposal_id: u64, max_depositors: u32) -> Result<Self> {
        ensure!(proposal_id > 0, "Invalid governance proposal ID");
        ensure!(
            max_depositors > 0,
            "Governance depositor capacity is absent"
        );
        let value = Self {
            proposal_id,
            max_depositors,
            deposits: BTreeMap::new(),
            total_udgt: 0,
            status: EscrowStatus::Holding,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn proposal_id(&self) -> u64 {
        self.proposal_id
    }

    pub fn total_udgt(&self) -> u128 {
        self.total_udgt
    }

    pub fn deposits(&self) -> &BTreeMap<OwnerId, u128> {
        &self.deposits
    }

    pub fn max_depositors(&self) -> u32 {
        self.max_depositors
    }

    pub fn status(&self) -> EscrowStatus {
        self.status
    }

    pub fn deposited_by(&self, owner: OwnerId) -> u128 {
        self.deposits.get(&owner).copied().unwrap_or(0)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.proposal_id > 0 && self.max_depositors > 0,
            "Invalid governance escrow identity"
        );
        ensure!(
            self.deposits.len() <= self.max_depositors as usize,
            "Governance depositor capacity exceeded"
        );
        let total = self.deposits.values().try_fold(0u128, |sum, amount| {
            ensure!(*amount > 0, "Zero governance deposit record");
            sum.checked_add(*amount)
                .context("Governance deposit total overflow")
        })?;
        ensure!(
            total == self.total_udgt && total <= DGT_SUPPLY,
            "Governance deposit conservation failed"
        );
        Ok(())
    }

    /// Plan one debit. The caller must use the current spendable DGT balance.
    /// A successful plan does not write either account or escrow state.
    pub fn plan_deposit(
        &self,
        owner: OwnerId,
        amount_udgt: u128,
        current_balance_udgt: u128,
        spendable_balance_udgt: u128,
    ) -> Result<(Self, u128)> {
        self.validate()?;
        ensure!(
            self.status == EscrowStatus::Holding,
            "Governance escrow is terminal"
        );
        ensure!(amount_udgt > 0, "Governance deposit is zero");
        ensure!(
            current_balance_udgt <= DGT_SUPPLY,
            "Governance owner balance exceeds DGT supply"
        );
        ensure!(
            spendable_balance_udgt <= current_balance_udgt && amount_udgt <= spendable_balance_udgt,
            "Insufficient spendable DGT for governance deposit"
        );
        let next_balance = current_balance_udgt
            .checked_sub(amount_udgt)
            .context("Governance deposit exceeds current DGT balance")?;
        let mut next = self.clone();
        let prior = next.deposited_by(owner);
        let accumulated = prior
            .checked_add(amount_udgt)
            .context("Governance owner deposit overflow")?;
        next.deposits.insert(owner, accumulated);
        next.total_udgt = next
            .total_udgt
            .checked_add(amount_udgt)
            .context("Governance deposit total overflow")?;
        next.validate()?;
        next.ensure_encoded_size()?;
        Ok((next, next_balance))
    }

    /// Plan one terminal refund. Every returned balance must be committed with
    /// the returned escrow state. No burn, sweep, or automatic recipient exists.
    pub fn plan_refund(
        &self,
        ballot: &Ballot,
        current_balances_udgt: &BTreeMap<OwnerId, u128>,
    ) -> Result<(Self, BTreeMap<OwnerId, u128>)> {
        self.validate()?;
        ballot.validate()?;
        ensure!(
            self.status == EscrowStatus::Holding,
            "Governance escrow already refunded"
        );
        ensure!(
            ballot.proposal_id() == self.proposal_id,
            "Governance ballot and escrow differ"
        );
        let outcome = match ballot.status() {
            BallotStatus::Rejected => TerminalOutcome::Rejected,
            BallotStatus::Executed => TerminalOutcome::Executed,
            BallotStatus::FailedExecution => TerminalOutcome::FailedExecution,
            BallotStatus::Voting | BallotStatus::Passed { .. } => {
                anyhow::bail!("Governance ballot is not terminal")
            }
        };
        self.plan_refund_outcome(outcome, current_balances_udgt)
    }

    /// Refund a proposal that failed the approved deposit-stage minimum.
    /// The typed rejection proves the exact proposal, closing height, and
    /// escrow total. The caller must commit credited balances, escrow, and
    /// proposal terminal state in one consensus batch.
    pub fn plan_rejected_deposit_refund(
        &self,
        rejection: &DepositStageRejection,
        current_balances_udgt: &BTreeMap<OwnerId, u128>,
    ) -> Result<(Self, BTreeMap<OwnerId, u128>)> {
        self.validate()?;
        rejection.validate_escrow(self)?;
        self.plan_refund_outcome(TerminalOutcome::Rejected, current_balances_udgt)
    }

    fn plan_refund_outcome(
        &self,
        outcome: TerminalOutcome,
        current_balances_udgt: &BTreeMap<OwnerId, u128>,
    ) -> Result<(Self, BTreeMap<OwnerId, u128>)> {
        ensure!(
            self.status == EscrowStatus::Holding,
            "Governance escrow already refunded"
        );
        let mut balances = BTreeMap::new();
        for (owner, amount) in &self.deposits {
            let current = current_balances_udgt
                .get(owner)
                .copied()
                .context("Governance refund owner balance snapshot is missing")?;
            ensure!(
                current <= DGT_SUPPLY,
                "Governance refund owner balance exceeds DGT supply"
            );
            let credited = current
                .checked_add(*amount)
                .context("Governance refund balance overflow")?;
            ensure!(
                credited <= DGT_SUPPLY,
                "Governance refund would exceed DGT supply"
            );
            balances.insert(*owner, credited);
        }
        let mut next = self.clone();
        next.status = EscrowStatus::Refunded(outcome);
        next.validate()?;
        next.ensure_encoded_size()?;
        Ok((next, balances))
    }

    fn ensure_encoded_size(&self) -> Result<()> {
        ensure!(
            bincode::serialized_size(self)? <= MAX_STATE_BYTES as u64,
            "Governance escrow exceeds size limit"
        );
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        self.ensure_encoded_size()?;
        Ok(bincode::serialize(self)?)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= MAX_STATE_BYTES,
            "Governance escrow exceeds size limit"
        );
        let value: Self = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_limit(MAX_STATE_BYTES as u64)
            .reject_trailing_bytes()
            .deserialize(bytes)?;
        value.validate()?;
        ensure!(value.encode()? == bytes, "Noncanonical governance escrow");
        Ok(value)
    }
}

/// Validate a complete proposal-indexed escrow view before supply accounting.
/// Refunded deposits remain in history but are no longer held in custody.
pub fn held_total_udgt(escrows: &BTreeMap<u64, DepositEscrow>) -> Result<u128> {
    let mut held = 0u128;
    for (proposal_id, escrow) in escrows {
        escrow.validate()?;
        ensure!(
            *proposal_id == escrow.proposal_id(),
            "Governance escrow index and proposal differ"
        );
        if escrow.status() == EscrowStatus::Holding {
            held = held
                .checked_add(escrow.total_udgt())
                .context("Governance held deposit total overflow")?;
            ensure!(
                held <= DGT_SUPPLY,
                "Governance held deposits exceed DGT supply"
            );
        }
    }
    Ok(held)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::governance_ballot::{BondSnapshot, Rules};
    use crate::runtime::governance_deposit_stage::{DepositClosePlan, DepositRules, DepositStage};

    fn voting_ballot(proposal_id: u64) -> Ballot {
        let rules = Rules {
            version: 1,
            chain_id: "development-chain".into(),
            genesis_digest: [1; 32],
            quorum_bps: 10_000,
            approval_bps: 10_000,
            veto_bps: 10_000,
            voting_period_blocks: 1,
            timelock_blocks: 1,
            max_voters: 1,
        };
        let snapshot =
            BondSnapshot::new(1, [2; 32], BTreeMap::from([("owner".into(), 1)]), 1).unwrap();
        Ballot::start(rules, proposal_id, [3; 32], snapshot, 2).unwrap()
    }

    fn rejected_ballot(proposal_id: u64) -> Ballot {
        let mut ballot = voting_ballot(proposal_id);
        ballot.close(4).unwrap();
        ballot
    }

    fn passed_ballot(proposal_id: u64) -> Ballot {
        let mut ballot = voting_ballot(proposal_id);
        ballot
            .vote(
                "owner",
                crate::runtime::governance_ballot::VoteChoice::Yes,
                2,
            )
            .unwrap();
        assert!(ballot.close(4).unwrap().passes);
        ballot
    }

    #[test]
    fn repeated_deposit_and_terminal_refund_conserve_dgt_once() {
        let owner = [1; 32];
        let another = [2; 32];
        let empty = DepositEscrow::new(7, 2).unwrap();
        let (one, remaining) = empty.plan_deposit(owner, 11, 50, 50).unwrap();
        assert_eq!(remaining, 39);
        let (two, remaining) = one.plan_deposit(owner, 9, remaining, remaining).unwrap();
        assert_eq!(remaining, 30);
        let (three, other_balance) = two.plan_deposit(another, 5, 20, 20).unwrap();
        assert_eq!(other_balance, 15);
        assert_eq!(three.total_udgt(), 25);
        assert_eq!(three.deposited_by(owner), 20);
        let restored = DepositEscrow::decode(&three.encode().unwrap()).unwrap();
        let balances = BTreeMap::from([(owner, 30), (another, 15)]);
        let ballot = rejected_ballot(7);
        let (refunded, credits) = restored.plan_refund(&ballot, &balances).unwrap();
        assert_eq!(credits, BTreeMap::from([(owner, 50), (another, 20)]));
        assert_eq!(
            refunded.status(),
            EscrowStatus::Refunded(TerminalOutcome::Rejected)
        );
        assert_eq!(refunded.total_udgt(), 25);
        assert!(refunded.plan_refund(&ballot, &credits).is_err());
        assert!(refunded.plan_deposit(owner, 1, 50, 50).is_err());
    }

    #[test]
    fn deposit_stage_rejection_refunds_once_with_bound_escrow() {
        let owner = [1; 32];
        let action_data = [7u8, 8, 9];
        let digest =
            dytallix_protocol_types::ordinary_v3::governance_action_digest(17, &action_data)
                .unwrap();
        let stage = DepositStage::new(
            DepositRules {
                deposit_period_blocks: 3,
                minimum_deposit_udgt: 5,
                max_action_bytes: 3,
            },
            4,
            17,
            action_data.to_vec(),
            digest,
            10,
        )
        .unwrap();
        let escrow = DepositEscrow::new(4, 1).unwrap();
        let (funded, balance) = stage.plan_deposit(&escrow, owner, 4, 10, 10, 12).unwrap();
        let DepositClosePlan::RejectAndRefund { rejection, .. } =
            stage.plan_close(&funded, 13, None).unwrap()
        else {
            panic!("expected deposit rejection");
        };
        assert!(escrow
            .plan_rejected_deposit_refund(&rejection, &BTreeMap::from([(owner, balance)]))
            .is_err());
        let (refunded, balances) = funded
            .plan_rejected_deposit_refund(&rejection, &BTreeMap::from([(owner, balance)]))
            .unwrap();
        assert_eq!(balances[&owner], 10);
        assert_eq!(
            refunded.status(),
            EscrowStatus::Refunded(TerminalOutcome::Rejected)
        );
        assert!(refunded
            .plan_rejected_deposit_refund(&rejection, &balances)
            .is_err());
    }

    #[test]
    fn errors_leave_input_escrow_unchanged() {
        let owner = [1; 32];
        let escrow = DepositEscrow::new(1, 1).unwrap();
        let before = escrow.encode().unwrap();
        assert!(escrow.plan_deposit(owner, 0, 10, 10).is_err());
        assert!(escrow.plan_deposit(owner, 11, 10, 10).is_err());
        assert!(escrow.plan_deposit(owner, 5, 10, 4).is_err());
        assert!(escrow
            .plan_deposit(owner, 1, DGT_SUPPLY + 1, DGT_SUPPLY + 1)
            .is_err());
        assert_eq!(escrow.plan_deposit(owner, 3, 100, 10).unwrap().1, 97);
        assert!(escrow
            .plan_deposit(owner, DGT_SUPPLY + 1, DGT_SUPPLY + 1, DGT_SUPPLY + 1)
            .is_err());
        assert_eq!(escrow.encode().unwrap(), before);
        let (funded, _) = escrow.plan_deposit(owner, 2, 10, 10).unwrap();
        let prior = funded.encode().unwrap();
        let mut malformed = serde_json::to_value(rejected_ballot(1)).unwrap();
        malformed["final_tally"] = serde_json::Value::Null;
        let malformed: Ballot = serde_json::from_value(malformed).unwrap();
        assert!(funded
            .plan_refund(&malformed, &BTreeMap::from([(owner, 8)]))
            .is_err());
        assert!(funded
            .plan_refund(&voting_ballot(1), &BTreeMap::from([(owner, 8)]))
            .is_err());
        assert!(funded
            .plan_refund(&passed_ballot(1), &BTreeMap::from([(owner, 8)]))
            .is_err());
        assert!(funded
            .plan_refund(&rejected_ballot(2), &BTreeMap::from([(owner, 8)]))
            .is_err());
        assert!(funded
            .plan_refund(&rejected_ballot(1), &BTreeMap::new())
            .is_err());
        assert!(funded
            .plan_refund(&rejected_ballot(1), &BTreeMap::from([(owner, u128::MAX)]))
            .is_err());
        assert!(funded
            .plan_refund(&rejected_ballot(1), &BTreeMap::from([(owner, DGT_SUPPLY)]))
            .is_err());
        assert_eq!(funded.encode().unwrap(), prior);
        let corrupt = DepositEscrow {
            total_udgt: 1,
            ..funded
        };
        assert!(corrupt.encode().is_err());
    }

    #[test]
    fn collection_counts_only_held_deposits_and_rejects_excess() {
        let owner = [1; 32];
        let (first, first_balance) = DepositEscrow::new(1, 1)
            .unwrap()
            .plan_deposit(owner, 600_000_000_000_000, DGT_SUPPLY, DGT_SUPPLY)
            .unwrap();
        let (second, _) = DepositEscrow::new(2, 1)
            .unwrap()
            .plan_deposit(owner, 500_000_000_000_000, DGT_SUPPLY, DGT_SUPPLY)
            .unwrap();
        let mut escrows = BTreeMap::from([(1, first.clone()), (2, second)]);
        assert!(held_total_udgt(&escrows).is_err());
        let (refunded, _) = first
            .plan_refund(
                &rejected_ballot(1),
                &BTreeMap::from([(owner, first_balance)]),
            )
            .unwrap();
        escrows.insert(1, refunded.clone());
        assert_eq!(held_total_udgt(&escrows).unwrap(), 500_000_000_000_000);
        escrows.remove(&1);
        escrows.insert(3, refunded);
        assert!(held_total_udgt(&escrows).is_err());
    }
}
