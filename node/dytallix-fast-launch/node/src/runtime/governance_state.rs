//! Bounded governance state for a future consensus-owned state transition.
//! This module does not debit accounts, execute actions, or write storage.
use super::governance_ballot::{Ballot, BallotStatus};
use super::governance_deposit_stage::{DepositStage, DepositStageStatus};
use super::governance_escrow::{DepositEscrow, EscrowStatus, TerminalOutcome};
use anyhow::{ensure, Context, Result};
use bincode::Options;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const GOVERNANCE_STATE_VERSION: u16 = 1;
pub const STATE_KEY: &str = "governance:v1:state";
pub const MAX_GOVERNANCE_STATE_BYTES: usize = 32 * 1024 * 1024;

/// All records for one proposal. The escrow remains after a refund for audit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalRecord {
    stage: DepositStage,
    escrow: DepositEscrow,
    ballot: Option<Ballot>,
}

impl ProposalRecord {
    pub fn new(stage: DepositStage, escrow: DepositEscrow, ballot: Option<Ballot>) -> Self {
        Self {
            stage,
            escrow,
            ballot,
        }
    }

    pub fn stage(&self) -> &DepositStage {
        &self.stage
    }

    pub fn escrow(&self) -> &DepositEscrow {
        &self.escrow
    }

    pub fn ballot(&self) -> Option<&Ballot> {
        self.ballot.as_ref()
    }

    fn validate(&self, proposal_id: u64, state: &GovernanceState) -> Result<()> {
        self.stage.validate()?;
        self.escrow.validate()?;
        ensure!(
            self.stage.proposal_id() == proposal_id && self.escrow.proposal_id() == proposal_id,
            "Governance proposal record identities differ"
        );
        ensure!(
            self.stage.admitted_height() <= state.finalized_height,
            "Governance admission is after state height"
        );
        match self.stage.status() {
            DepositStageStatus::Collecting => {
                ensure!(
                    state.finalized_height < self.stage.close_height()
                        && self.ballot.is_none()
                        && self.escrow.status() == EscrowStatus::Holding,
                    "Collecting proposal has closed or has terminal records"
                );
            }
            DepositStageStatus::Rejected => {
                ensure!(
                    state.finalized_height >= self.stage.close_height()
                        && self.escrow.total_udgt() < self.stage.rules().minimum_deposit_udgt
                        && self.ballot.is_none()
                        && self.escrow.status()
                            == EscrowStatus::Refunded(TerminalOutcome::Rejected),
                    "Rejected deposit stage and refund differ"
                );
            }
            DepositStageStatus::Voting => {
                ensure!(
                    state.finalized_height >= self.stage.close_height()
                        && self.escrow.total_udgt() >= self.stage.rules().minimum_deposit_udgt,
                    "Voting deposit stage and escrow differ"
                );
                let ballot = self.ballot.as_ref().context("Voting ballot is absent")?;
                ballot.validate()?;
                ensure!(
                    ballot.proposal_id() == proposal_id
                        && ballot.action_digest() == self.stage.action_digest()
                        && ballot.rules().chain_id == state.chain_id
                        && ballot.rules().genesis_digest == state.genesis_digest
                        && ballot.snapshot().finalized_height.checked_add(1)
                            == Some(self.stage.close_height()),
                    "Governance ballot and proposal identity or chain differ"
                );
                let voting_end = self
                    .stage
                    .close_height()
                    .checked_add(ballot.rules().voting_period_blocks)
                    .context("Governance voting end height overflow")?;
                let closing_height = voting_end
                    .checked_add(1)
                    .context("Governance voting close height overflow")?;
                let required_escrow = match ballot.status() {
                    BallotStatus::Voting => {
                        ensure!(
                            state.finalized_height <= voting_end,
                            "Open ballot is past voting end"
                        );
                        EscrowStatus::Holding
                    }
                    BallotStatus::Passed { .. } => {
                        ensure!(
                            state.finalized_height >= closing_height,
                            "Passed ballot is before voting close"
                        );
                        EscrowStatus::Holding
                    }
                    BallotStatus::Rejected => {
                        ensure!(
                            state.finalized_height >= closing_height,
                            "Rejected ballot is before voting close"
                        );
                        EscrowStatus::Refunded(TerminalOutcome::Rejected)
                    }
                    BallotStatus::Executed => {
                        ensure!(
                            ballot
                                .execution_height()
                                .is_some_and(|h| h <= state.finalized_height),
                            "Executed ballot is after state height"
                        );
                        EscrowStatus::Refunded(TerminalOutcome::Executed)
                    }
                    BallotStatus::FailedExecution => {
                        ensure!(
                            ballot
                                .execution_height()
                                .is_some_and(|h| h <= state.finalized_height),
                            "Failed ballot is after state height"
                        );
                        EscrowStatus::Refunded(TerminalOutcome::FailedExecution)
                    }
                };
                ensure!(
                    self.escrow.status() == required_escrow,
                    "Governance ballot and refund outcome differ"
                );
            }
        }
        Ok(())
    }
}

/// Retains every admitted proposal so proposal IDs cannot be reused.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernanceState {
    version: u16,
    chain_id: String,
    genesis_digest: [u8; 32],
    finalized_height: u64,
    next_proposal_id: u64,
    proposals: BTreeMap<u64, ProposalRecord>,
}

impl GovernanceState {
    pub fn new(
        version: u16,
        chain_id: String,
        genesis_digest: [u8; 32],
        finalized_height: u64,
    ) -> Result<Self> {
        let state = Self {
            version,
            chain_id,
            genesis_digest,
            finalized_height,
            next_proposal_id: 1,
            proposals: BTreeMap::new(),
        };
        state.encode()?;
        Ok(state)
    }

    pub fn version(&self) -> u16 {
        self.version
    }

    pub fn chain_id(&self) -> &str {
        &self.chain_id
    }

    pub fn genesis_digest(&self) -> [u8; 32] {
        self.genesis_digest
    }

    pub fn finalized_height(&self) -> u64 {
        self.finalized_height
    }

    pub fn last_height(&self) -> u64 {
        self.finalized_height
    }

    pub fn next_proposal_id(&self) -> u64 {
        self.next_proposal_id
    }

    pub fn proposals(&self) -> &BTreeMap<u64, ProposalRecord> {
        &self.proposals
    }

    pub fn held_total_udgt(&self) -> Result<u128> {
        self.validate()?;
        self.proposals.values().try_fold(0u128, |held, record| {
            if record.escrow.status() == EscrowStatus::Holding {
                held.checked_add(record.escrow.total_udgt())
                    .context("Governance held deposit total overflow")
            } else {
                Ok(held)
            }
        })
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == GOVERNANCE_STATE_VERSION,
            "Unsupported governance state version"
        );
        ensure!(
            !self.chain_id.is_empty()
                && self.chain_id.len() <= 128
                && !self.chain_id.chars().any(char::is_control),
            "Invalid governance state chain ID"
        );
        ensure!(
            self.genesis_digest != [0; 32],
            "Governance state genesis digest is absent"
        );
        ensure!(self.next_proposal_id > 0, "Governance proposal ID overflow");
        let count = u64::try_from(self.proposals.len()).context("Proposal count overflow")?;
        ensure!(
            count.checked_add(1) == Some(self.next_proposal_id),
            "Governance proposal ID sequence differs"
        );
        for (expected, (id, record)) in (1..self.next_proposal_id).zip(&self.proposals) {
            ensure!(*id == expected, "Governance proposal ID gap");
            record.validate(*id, self)?;
        }
        let mut held = 0u128;
        for record in self.proposals.values() {
            if record.escrow.status() == EscrowStatus::Holding {
                held = held
                    .checked_add(record.escrow.total_udgt())
                    .context("Governance held deposit total overflow")?;
            }
        }
        ensure!(
            held <= dytallix_protocol_types::units::DGT_TOTAL_BASE_UNITS,
            "Governance held deposits exceed DGT supply"
        );
        Ok(())
    }

    /// Prepare one finalized block. The caller commits this state with account
    /// debits, credits, and other effects in one consensus storage transaction.
    /// A new proposal can include deposits made after its admission transaction
    /// in the same finalized block. The caller must verify transaction order and
    /// debit each depositor in that same storage transaction.
    pub fn plan_commit(
        &self,
        finalized_height: u64,
        proposals: BTreeMap<u64, ProposalRecord>,
    ) -> Result<Self> {
        self.validate()?;
        ensure!(
            self.finalized_height.checked_add(1) == Some(finalized_height),
            "Governance state height must advance by one"
        );
        for (id, old) in &self.proposals {
            let next = proposals
                .get(id)
                .context("Governance proposal history was removed")?;
            validate_transition(old, next, finalized_height)?;
        }
        for (id, record) in proposals.range(self.next_proposal_id..) {
            ensure!(
                record.stage.admitted_height() == finalized_height
                    && record.stage.status() == DepositStageStatus::Collecting
                    && record.escrow.status() == EscrowStatus::Holding
                    && record.ballot.is_none(),
                "New governance proposal is not an admission at this height"
            );
            ensure!(
                *id >= self.next_proposal_id,
                "Governance proposal ID reused"
            );
        }
        let next_proposal_id = u64::try_from(proposals.len())
            .context("Proposal count overflow")?
            .checked_add(1)
            .context("Governance proposal ID overflow")?;
        ensure!(
            next_proposal_id >= self.next_proposal_id,
            "Governance proposal ID moved backward"
        );
        let next = Self {
            version: self.version,
            chain_id: self.chain_id.clone(),
            genesis_digest: self.genesis_digest,
            finalized_height,
            next_proposal_id,
            proposals,
        };
        next.encode()?;
        Ok(next)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let bytes = bincode::serialize(self)?;
        ensure!(
            bytes.len() <= MAX_GOVERNANCE_STATE_BYTES,
            "Governance state exceeds size limit"
        );
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= MAX_GOVERNANCE_STATE_BYTES,
            "Governance state exceeds size limit"
        );
        let value: Self = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_limit(MAX_GOVERNANCE_STATE_BYTES as u64)
            .reject_trailing_bytes()
            .deserialize(bytes)?;
        ensure!(value.encode()? == bytes, "Noncanonical governance state");
        Ok(value)
    }
}

fn validate_transition(old: &ProposalRecord, next: &ProposalRecord, height: u64) -> Result<()> {
    ensure!(
        old.stage.rules() == next.stage.rules()
            && old.stage.proposal_id() == next.stage.proposal_id()
            && old.stage.action_class() == next.stage.action_class()
            && old.stage.action_data() == next.stage.action_data()
            && old.stage.action_digest() == next.stage.action_digest()
            && old.stage.admitted_height() == next.stage.admitted_height()
            && old.stage.close_height() == next.stage.close_height(),
        "Governance proposal admission changed"
    );
    let stage_change = (old.stage.status(), next.stage.status());
    ensure!(
        matches!(
            stage_change,
            (
                DepositStageStatus::Collecting,
                DepositStageStatus::Collecting
            ) | (DepositStageStatus::Collecting, DepositStageStatus::Rejected)
                | (DepositStageStatus::Collecting, DepositStageStatus::Voting)
                | (DepositStageStatus::Rejected, DepositStageStatus::Rejected)
                | (DepositStageStatus::Voting, DepositStageStatus::Voting)
        ),
        "Governance deposit stage moved backward"
    );
    if old.stage.status() != next.stage.status() {
        ensure!(
            height == old.stage.close_height(),
            "Governance deposit stage closed late"
        );
    }
    if old.stage.status() != DepositStageStatus::Collecting {
        ensure!(
            old.stage == next.stage,
            "Closed governance deposit stage changed"
        );
    }
    ensure!(
        old.escrow.proposal_id() == next.escrow.proposal_id()
            && old.escrow.max_depositors() == next.escrow.max_depositors(),
        "Governance escrow identity changed"
    );
    for (owner, amount) in old.escrow.deposits() {
        ensure!(
            next.escrow.deposited_by(*owner) >= *amount,
            "Governance depositor amount was reduced"
        );
    }
    if old.stage.status() != DepositStageStatus::Collecting
        || next.stage.status() != DepositStageStatus::Collecting
        || old.escrow.status() != EscrowStatus::Holding
    {
        ensure!(
            old.escrow.deposits() == next.escrow.deposits(),
            "Governance deposits changed after collection"
        );
    }
    if let EscrowStatus::Refunded(_) = old.escrow.status() {
        ensure!(old.escrow == next.escrow, "Governance refund changed");
    }
    match (&old.ballot, &next.ballot) {
        (None, None) => {}
        (None, Some(_)) => {
            ensure!(
                stage_change == (DepositStageStatus::Collecting, DepositStageStatus::Voting),
                "Governance ballot was created outside deposit close"
            );
        }
        (Some(_), None) => anyhow::bail!("Governance ballot was removed"),
        (Some(before), Some(after)) => {
            ensure!(
                before.rules() == after.rules()
                    && before.proposal_id() == after.proposal_id()
                    && before.action_digest() == after.action_digest()
                    && before.snapshot() == after.snapshot(),
                "Governance ballot identity changed"
            );
            for (owner, vote) in before.votes() {
                ensure!(
                    after.votes().get(owner) == Some(vote),
                    "Governance prior vote changed"
                );
            }
            if before.votes() != after.votes() {
                ensure!(
                    matches!(before.status(), BallotStatus::Voting)
                        && matches!(after.status(), BallotStatus::Voting),
                    "Governance votes changed after voting"
                );
            }
            let valid_status = match (before.status(), after.status()) {
                (BallotStatus::Voting, BallotStatus::Voting)
                | (BallotStatus::Voting, BallotStatus::Rejected)
                | (BallotStatus::Voting, BallotStatus::Passed { .. })
                | (BallotStatus::Passed { .. }, BallotStatus::Executed)
                | (BallotStatus::Passed { .. }, BallotStatus::FailedExecution) => true,
                (a, b) => a == b,
            };
            ensure!(valid_status, "Governance ballot status moved backward");
            if matches!(before.status(), BallotStatus::Voting)
                && !matches!(after.status(), BallotStatus::Voting)
            {
                let close = before
                    .snapshot()
                    .finalized_height
                    .checked_add(2)
                    .and_then(|h| h.checked_add(before.rules().voting_period_blocks))
                    .context("Governance voting close height overflow")?;
                ensure!(height == close, "Governance ballot closed late");
            }
            if !matches!(
                before.status(),
                BallotStatus::Voting | BallotStatus::Passed { .. }
            ) {
                ensure!(before == after, "Terminal governance ballot changed");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::governance_ballot::{BondSnapshot, Rules as BallotRules, VoteChoice};
    use crate::runtime::governance_deposit_stage::{DepositClosePlan, DepositRules, DepositStage};
    use dytallix_protocol_types::ordinary_v3::governance_action_digest;

    fn state() -> GovernanceState {
        GovernanceState::new(1, "candidate-chain".into(), [1; 32], 0).unwrap()
    }

    fn record(id: u64, admitted: u64, minimum: u128) -> ProposalRecord {
        let action = vec![id as u8];
        let digest = governance_action_digest(17, &action).unwrap();
        let stage = DepositStage::new(
            DepositRules {
                deposit_period_blocks: 2,
                minimum_deposit_udgt: minimum,
                max_action_bytes: 1,
            },
            id,
            17,
            action,
            digest,
            admitted,
        )
        .unwrap();
        ProposalRecord::new(stage, DepositEscrow::new(id, 2).unwrap(), None)
    }

    fn ballot_rules() -> BallotRules {
        BallotRules {
            version: 1,
            chain_id: "candidate-chain".into(),
            genesis_digest: [1; 32],
            quorum_bps: 5_000,
            approval_bps: 5_000,
            veto_bps: 5_000,
            voting_period_blocks: 2,
            timelock_blocks: 1,
            max_voters: 1,
        }
    }

    fn advance(
        state: &GovernanceState,
        changes: impl FnOnce(&mut BTreeMap<u64, ProposalRecord>),
    ) -> GovernanceState {
        let mut proposals = state.proposals.clone();
        changes(&mut proposals);
        state
            .plan_commit(state.finalized_height + 1, proposals)
            .unwrap()
    }

    #[test]
    fn full_record_lifecycle_tracks_held_dgt_and_never_reuses_id() {
        let initial = state();
        let one = advance(&initial, |records| {
            records.insert(1, record(1, 1, 5));
        });
        assert_eq!(one.next_proposal_id(), 2);
        let two = advance(&one, |records| {
            let first = records.get_mut(&1).unwrap();
            first.escrow = first
                .stage
                .plan_deposit(&first.escrow, [7; 32], 5, 10, 10, 2)
                .unwrap()
                .0;
            records.insert(2, record(2, 2, 5));
        });
        assert_eq!(two.held_total_udgt().unwrap(), 5);
        let three = advance(&two, |records| {
            let first = records.get_mut(&1).unwrap();
            let snapshot =
                BondSnapshot::new(2, [2; 32], BTreeMap::from([("voter".into(), 10)]), 1).unwrap();
            let DepositClosePlan::StartVoting { next_stage, ballot } = first
                .stage
                .plan_close(&first.escrow, 3, Some((ballot_rules(), snapshot)))
                .unwrap()
            else {
                panic!("expected ballot")
            };
            first.stage = next_stage;
            first.ballot = Some(ballot);
        });
        let mut wrong_chain = three.clone();
        wrong_chain.chain_id = "different-chain".into();
        assert!(wrong_chain.encode().is_err());
        let mut wrong_genesis = three.clone();
        wrong_genesis.genesis_digest = [3; 32];
        assert!(wrong_genesis.encode().is_err());
        let four = advance(&three, |records| {
            let second = records.get_mut(&2).unwrap();
            let DepositClosePlan::RejectAndRefund {
                next_stage,
                rejection,
            } = second.stage.plan_close(&second.escrow, 4, None).unwrap()
            else {
                panic!("expected rejection")
            };
            second.escrow = second
                .escrow
                .plan_rejected_deposit_refund(&rejection, &BTreeMap::new())
                .unwrap()
                .0;
            second.stage = next_stage;
            records
                .get_mut(&1)
                .unwrap()
                .ballot
                .as_mut()
                .unwrap()
                .vote("voter", VoteChoice::Yes, 4)
                .unwrap();
        });
        assert_eq!(four.held_total_udgt().unwrap(), 5);
        let five = advance(&four, |_| {});
        let six = advance(&five, |records| {
            let first = records.get_mut(&1).unwrap();
            let ballot = first.ballot.as_mut().unwrap();
            assert!(ballot.close(6).unwrap().passes);
        });
        assert_eq!(six.held_total_udgt().unwrap(), 5);
        let seven = advance(&six, |records| {
            let first = records.get_mut(&1).unwrap();
            let ballot = first.ballot.as_mut().unwrap();
            ballot
                .record_execution(7, first.stage.action_digest(), true)
                .unwrap();
            first.escrow = first
                .escrow
                .plan_refund(ballot, &BTreeMap::from([([7; 32], 5)]))
                .unwrap()
                .0;
        });
        assert_eq!(seven.held_total_udgt().unwrap(), 0);
        let mut missing_refund = seven.clone();
        missing_refund.proposals.get_mut(&1).unwrap().escrow = six.proposals[&1].escrow.clone();
        assert!(missing_refund.encode().is_err());
        assert_eq!(seven.proposals.len(), 2);
        assert_eq!(seven.next_proposal_id(), 3);
        assert_eq!(
            GovernanceState::decode(&seven.encode().unwrap()).unwrap(),
            seven
        );
    }

    #[test]
    fn one_block_period_accepts_same_block_deposit_and_closes_next_height() {
        let genesis = state();
        let owner = [7; 32];
        let action = vec![17];
        let digest = governance_action_digest(17, &action).unwrap();
        let stage = DepositStage::new(
            DepositRules {
                deposit_period_blocks: 1,
                minimum_deposit_udgt: 10,
                max_action_bytes: 1,
            },
            1,
            17,
            action,
            digest,
            1,
        )
        .unwrap();
        let escrow = DepositEscrow::new(1, 1).unwrap();
        let (escrow, after_debit) = stage.plan_deposit(&escrow, owner, 5, 10, 10, 1).unwrap();
        assert_eq!(after_debit, 5);
        let admitted = genesis
            .plan_commit(
                1,
                BTreeMap::from([(1, ProposalRecord::new(stage, escrow, None))]),
            )
            .unwrap();
        assert_eq!(admitted.held_total_udgt().unwrap(), 5);

        let record = &admitted.proposals()[&1];
        let DepositClosePlan::RejectAndRefund {
            next_stage,
            rejection,
        } = record.stage().plan_close(record.escrow(), 2, None).unwrap()
        else {
            panic!("expected below-minimum rejection")
        };
        let (refunded, balances) = record
            .escrow()
            .plan_rejected_deposit_refund(&rejection, &BTreeMap::from([(owner, after_debit)]))
            .unwrap();
        assert_eq!(balances[&owner], 10);
        let closed = admitted
            .plan_commit(
                2,
                BTreeMap::from([(1, ProposalRecord::new(next_stage, refunded, None))]),
            )
            .unwrap();
        assert_eq!(closed.held_total_udgt().unwrap(), 0);
        assert_eq!(
            closed.proposals()[&1].stage().status(),
            DepositStageStatus::Rejected
        );
    }

    #[test]
    fn rejects_cross_record_status_identity_and_chain_mismatches() {
        let admitted = advance(&state(), |records| {
            records.insert(1, record(1, 1, 5));
        });
        let mut forged = admitted.clone();
        forged.proposals.get_mut(&1).unwrap().escrow = DepositEscrow::new(2, 1).unwrap();
        assert!(forged.encode().is_err());
        let mut forged = admitted.clone();
        forged.proposals.get_mut(&1).unwrap().ballot = Some(
            Ballot::start(
                ballot_rules(),
                1,
                forged.proposals[&1].stage.action_digest(),
                BondSnapshot::new(2, [2; 32], BTreeMap::from([("voter".into(), 1)]), 1).unwrap(),
                3,
            )
            .unwrap(),
        );
        assert!(forged.encode().is_err());
        let mut forged = admitted.clone();
        forged.next_proposal_id = 3;
        assert!(forged.encode().is_err());
    }

    #[test]
    fn commits_require_contiguous_ids_one_height_and_unchanged_history() {
        let admitted = advance(&state(), |records| {
            records.insert(1, record(1, 1, 5));
        });
        assert!(admitted.plan_commit(3, admitted.proposals.clone()).is_err());
        assert!(admitted.plan_commit(2, BTreeMap::new()).is_err());
        let mut gap = admitted.proposals.clone();
        gap.insert(3, record(3, 2, 5));
        assert!(admitted.plan_commit(2, gap).is_err());
        let mut changed = admitted.proposals.clone();
        changed.get_mut(&1).unwrap().stage = record(1, 1, 6).stage;
        assert!(admitted.plan_commit(2, changed).is_err());
        let mut replaced = admitted.proposals.clone();
        replaced.get_mut(&1).unwrap().escrow = DepositEscrow::new(1, 3).unwrap();
        assert!(admitted.plan_commit(2, replaced).is_err());
    }

    #[test]
    fn encoding_rejects_trailing_bytes_unknown_version_and_oversize() {
        assert!(GovernanceState::new(2, "chain".into(), [1; 32], 0).is_err());
        assert!(GovernanceState::new(1, "".into(), [1; 32], 0).is_err());
        assert!(GovernanceState::new(1, "chain".into(), [0; 32], 0).is_err());
        let bytes = state().encode().unwrap();
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(GovernanceState::decode(&trailing).is_err());
        assert!(GovernanceState::decode(&vec![0; MAX_GOVERNANCE_STATE_BYTES + 1]).is_err());
    }
}
