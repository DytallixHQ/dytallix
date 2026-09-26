//! Proposal deposit-stage plans for a future consensus-owned governance state.
//! These plans do not write storage or authorize a governance action class.
use super::governance_ballot::{Ballot, BondSnapshot, Rules as BallotRules};
use super::governance_escrow::{DepositEscrow, EscrowStatus, OwnerId};
use anyhow::{ensure, Context, Result};
use bincode::Options;
use dytallix_protocol_types::ordinary_v3::governance_action_digest;
use serde::{Deserialize, Serialize};

const DGT_SUPPLY: u128 = dytallix_protocol_types::units::DGT_TOTAL_BASE_UNITS;
const MAX_STATE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DepositRules {
    pub deposit_period_blocks: u64,
    pub minimum_deposit_udgt: u128,
    pub max_action_bytes: u32,
}

impl DepositRules {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.deposit_period_blocks > 0,
            "Deposit period must be positive"
        );
        ensure!(
            self.minimum_deposit_udgt > 0 && self.minimum_deposit_udgt <= DGT_SUPPLY,
            "Minimum governance deposit is outside DGT supply"
        );
        ensure!(
            self.max_action_bytes > 0,
            "Governance action bound must be positive"
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DepositStageStatus {
    Collecting,
    Rejected,
    Voting,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DepositStage {
    rules: DepositRules,
    proposal_id: u64,
    action_class: u16,
    action_data: Vec<u8>,
    action_digest: [u8; 32],
    admitted_height: u64,
    close_height: u64,
    status: DepositStageStatus,
    closed_height: Option<u64>,
}

/// The caller must commit every part of a close plan in one state transition.
pub enum DepositClosePlan {
    RejectAndRefund {
        next_stage: DepositStage,
        rejection: DepositStageRejection,
    },
    StartVoting {
        next_stage: DepositStage,
        ballot: Ballot,
    },
}

/// A typed refund authority for a proposal that did not meet its deposit minimum.
/// Only `DepositStage::plan_close` constructs this value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DepositStageRejection {
    proposal_id: u64,
    close_height: u64,
    minimum_deposit_udgt: u128,
    actual_deposit_udgt: u128,
    action_digest: [u8; 32],
}

impl DepositStageRejection {
    pub fn proposal_id(&self) -> u64 {
        self.proposal_id
    }

    pub fn close_height(&self) -> u64 {
        self.close_height
    }

    pub fn minimum_deposit_udgt(&self) -> u128 {
        self.minimum_deposit_udgt
    }

    pub fn actual_deposit_udgt(&self) -> u128 {
        self.actual_deposit_udgt
    }

    pub fn action_digest(&self) -> [u8; 32] {
        self.action_digest
    }

    /// Recheck the escrow at refund time. A deposit after planning invalidates
    /// this rejection plan. The caller must commit the stage, escrow, and credits
    /// from one state transition.
    pub fn validate_escrow(&self, escrow: &DepositEscrow) -> Result<()> {
        escrow.validate()?;
        ensure!(
            escrow.proposal_id() == self.proposal_id
                && escrow.status() == EscrowStatus::Holding
                && escrow.total_udgt() == self.actual_deposit_udgt
                && self.actual_deposit_udgt < self.minimum_deposit_udgt,
            "Governance rejection and escrow differ"
        );
        Ok(())
    }
}

impl DepositStage {
    /// The admission height is the first block of the deposit period. The
    /// first block after that period is `admitted_height + period`.
    pub fn new(
        rules: DepositRules,
        proposal_id: u64,
        action_class: u16,
        action_data: Vec<u8>,
        action_digest: [u8; 32],
        admitted_height: u64,
    ) -> Result<Self> {
        rules.validate()?;
        ensure!(
            proposal_id > 0 && admitted_height > 0,
            "Invalid proposal admission"
        );
        ensure!(
            action_data.len() <= rules.max_action_bytes as usize,
            "Governance action exceeds supplied bound"
        );
        ensure!(
            governance_action_digest(action_class, &action_data)? == action_digest,
            "Governance action digest differs"
        );
        let close_height = admitted_height
            .checked_add(rules.deposit_period_blocks)
            .context("Governance deposit close height overflow")?;
        let stage = Self {
            rules,
            proposal_id,
            action_class,
            action_data,
            action_digest,
            admitted_height,
            close_height,
            status: DepositStageStatus::Collecting,
            closed_height: None,
        };
        stage.validate()?;
        stage.ensure_encoded_size()?;
        Ok(stage)
    }

    pub fn proposal_id(&self) -> u64 {
        self.proposal_id
    }

    pub fn rules(&self) -> &DepositRules {
        &self.rules
    }

    pub fn action_class(&self) -> u16 {
        self.action_class
    }

    pub fn action_data(&self) -> &[u8] {
        &self.action_data
    }

    pub fn action_digest(&self) -> [u8; 32] {
        self.action_digest
    }

    pub fn admitted_height(&self) -> u64 {
        self.admitted_height
    }

    pub fn close_height(&self) -> u64 {
        self.close_height
    }

    pub fn status(&self) -> DepositStageStatus {
        self.status
    }

    pub fn validate(&self) -> Result<()> {
        self.rules.validate()?;
        ensure!(
            self.proposal_id > 0 && self.admitted_height > 0,
            "Invalid governance deposit-stage identity"
        );
        ensure!(
            self.admitted_height
                .checked_add(self.rules.deposit_period_blocks)
                == Some(self.close_height),
            "Governance deposit close height differs"
        );
        ensure!(
            self.action_data.len() <= self.rules.max_action_bytes as usize,
            "Governance action exceeds supplied bound"
        );
        ensure!(
            governance_action_digest(self.action_class, &self.action_data)? == self.action_digest,
            "Governance action digest differs"
        );
        ensure!(
            match self.status {
                DepositStageStatus::Collecting => self.closed_height.is_none(),
                DepositStageStatus::Rejected | DepositStageStatus::Voting => {
                    self.closed_height == Some(self.close_height)
                }
            },
            "Governance deposit-stage status and close height differ"
        );
        Ok(())
    }

    fn ensure_encoded_size(&self) -> Result<()> {
        ensure!(
            bincode::serialized_size(self)? <= MAX_STATE_BYTES as u64,
            "Governance deposit stage exceeds size limit"
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
            "Governance deposit stage exceeds size limit"
        );
        let stage: Self = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_limit(MAX_STATE_BYTES as u64)
            .reject_trailing_bytes()
            .deserialize(bytes)?;
        stage.validate()?;
        ensure!(
            stage.encode()? == bytes,
            "Noncanonical governance deposit stage"
        );
        Ok(stage)
    }

    /// Plan a deposit during the supplied deposit period. The returned escrow
    /// and balance are a single atomic state change for the consensus caller.
    pub fn plan_deposit(
        &self,
        escrow: &DepositEscrow,
        owner: OwnerId,
        amount_udgt: u128,
        current_balance_udgt: u128,
        spendable_balance_udgt: u128,
        finalized_height: u64,
    ) -> Result<(DepositEscrow, u128)> {
        self.validate()?;
        ensure!(
            self.status == DepositStageStatus::Collecting,
            "Deposit stage is closed"
        );
        ensure!(
            (self.admitted_height..self.close_height).contains(&finalized_height),
            "Deposit height is outside deposit period"
        );
        ensure!(
            escrow.proposal_id() == self.proposal_id,
            "Governance escrow and proposal differ"
        );
        escrow.plan_deposit(
            owner,
            amount_udgt,
            current_balance_udgt,
            spendable_balance_udgt,
        )
    }

    /// Close only on the first finalized block after the deposit period.
    /// For voting, the supplied snapshot must be from its finalized parent.
    /// For rejection, the caller must use the rejection proof to plan a refund.
    pub fn plan_close(
        &self,
        escrow: &DepositEscrow,
        finalized_height: u64,
        voting: Option<(BallotRules, BondSnapshot)>,
    ) -> Result<DepositClosePlan> {
        self.validate()?;
        ensure!(
            self.status == DepositStageStatus::Collecting,
            "Deposit stage is closed"
        );
        ensure!(
            finalized_height == self.close_height,
            "Governance deposit stage must close at exact height"
        );
        escrow.validate()?;
        ensure!(
            escrow.proposal_id() == self.proposal_id && escrow.status() == EscrowStatus::Holding,
            "Governance escrow and proposal differ"
        );
        let mut next_stage = self.clone();
        if escrow.total_udgt() < self.rules.minimum_deposit_udgt {
            next_stage.status = DepositStageStatus::Rejected;
            next_stage.closed_height = Some(finalized_height);
            next_stage.validate()?;
            let rejection = DepositStageRejection {
                proposal_id: self.proposal_id,
                close_height: self.close_height,
                minimum_deposit_udgt: self.rules.minimum_deposit_udgt,
                actual_deposit_udgt: escrow.total_udgt(),
                action_digest: self.action_digest,
            };
            return Ok(DepositClosePlan::RejectAndRefund {
                next_stage,
                rejection,
            });
        }
        let (ballot_rules, parent_snapshot) =
            voting.context("Voting rules and parent snapshot are required")?;
        let ballot = Ballot::start(
            ballot_rules,
            self.proposal_id,
            self.action_digest,
            parent_snapshot,
            finalized_height,
        )?;
        next_stage.status = DepositStageStatus::Voting;
        next_stage.closed_height = Some(finalized_height);
        next_stage.validate()?;
        Ok(DepositClosePlan::StartVoting { next_stage, ballot })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn stage(minimum_deposit_udgt: u128) -> DepositStage {
        let action_data = vec![7, 8, 9];
        let digest = governance_action_digest(17, &action_data).unwrap();
        DepositStage::new(
            DepositRules {
                deposit_period_blocks: 3,
                minimum_deposit_udgt,
                max_action_bytes: 3,
            },
            4,
            17,
            action_data,
            digest,
            10,
        )
        .unwrap()
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

    fn parent_snapshot(parent_height: u64) -> BondSnapshot {
        BondSnapshot::new(
            parent_height,
            [2; 32],
            BTreeMap::from([("owner".into(), 1)]),
            1,
        )
        .unwrap()
    }

    #[test]
    fn admission_requires_explicit_positive_rules_and_matching_action_digest() {
        let mut rules = DepositRules {
            deposit_period_blocks: 0,
            minimum_deposit_udgt: 1,
            max_action_bytes: 3,
        };
        assert!(rules.validate().is_err());
        rules.deposit_period_blocks = 3;
        rules.minimum_deposit_udgt = 0;
        assert!(rules.validate().is_err());
        rules.minimum_deposit_udgt = 1;
        rules.max_action_bytes = 0;
        assert!(rules.validate().is_err());
        rules.max_action_bytes = 3;
        assert!(DepositStage::new(rules.clone(), 4, 17, vec![7], [0; 32], 10).is_err());
        let digest = governance_action_digest(17, &[7, 8, 9]).unwrap();
        assert!(DepositStage::new(rules.clone(), 4, 17, vec![7, 8, 9, 10], digest, 10).is_err());
        assert!(DepositStage::new(rules, 4, 17, vec![7, 8, 9], digest, u64::MAX).is_err());
        let admitted = stage(1);
        assert_eq!(admitted.action_class(), 17);
        assert_eq!(admitted.action_data(), &[7, 8, 9]);
        assert_eq!(admitted.action_digest(), digest);
        assert_eq!(admitted.admitted_height(), 10);
        assert_eq!(admitted.close_height(), 13);
    }

    #[test]
    fn deposit_window_and_exact_close_height() {
        let stage = stage(5);
        let escrow = DepositEscrow::new(4, 1).unwrap();
        let owner = [3; 32];
        assert!(stage.plan_deposit(&escrow, owner, 2, 10, 10, 9).is_err());
        let (funded, balance) = stage.plan_deposit(&escrow, owner, 2, 10, 10, 10).unwrap();
        assert_eq!(balance, 8);
        assert!(stage.plan_deposit(&funded, owner, 1, 8, 8, 13).is_err());
        assert!(stage.plan_close(&funded, 12, None).is_err());
        assert!(stage.plan_close(&funded, 14, None).is_err());
        assert_eq!(stage.status(), DepositStageStatus::Collecting);
        assert_eq!(escrow.total_udgt(), 0);
        assert!(stage
            .plan_close(&DepositEscrow::new(5, 1).unwrap(), 13, None)
            .is_err());
    }

    #[test]
    fn below_minimum_produces_bound_refund_proof() {
        let stage = stage(5);
        let owner = [3; 32];
        let escrow = DepositEscrow::new(4, 1).unwrap();
        let (funded, _) = stage.plan_deposit(&escrow, owner, 4, 10, 10, 12).unwrap();
        let DepositClosePlan::RejectAndRefund {
            next_stage,
            rejection,
        } = stage.plan_close(&funded, 13, None).unwrap()
        else {
            panic!("expected rejection");
        };
        assert_eq!(next_stage.status(), DepositStageStatus::Rejected);
        assert_eq!(rejection.proposal_id(), 4);
        assert_eq!(rejection.close_height(), 13);
        assert_eq!(rejection.minimum_deposit_udgt(), 5);
        assert_eq!(rejection.actual_deposit_udgt(), 4);
        assert_eq!(rejection.action_digest(), stage.action_digest());
        rejection.validate_escrow(&funded).unwrap();
        assert!(rejection.validate_escrow(&escrow).is_err());
        assert!(next_stage.plan_close(&funded, 13, None).is_err());
        assert!(next_stage
            .plan_deposit(&funded, owner, 1, 6, 6, 12)
            .is_err());
    }

    #[test]
    fn threshold_starts_ballot_from_immediately_preceding_parent() {
        let stage = stage(5);
        let owner = [3; 32];
        let escrow = DepositEscrow::new(4, 1).unwrap();
        let (funded, _) = stage.plan_deposit(&escrow, owner, 5, 10, 10, 12).unwrap();
        assert!(stage.plan_close(&funded, 13, None).is_err());
        assert!(stage
            .plan_close(&funded, 13, Some((ballot_rules(), parent_snapshot(11))))
            .is_err());
        let DepositClosePlan::StartVoting { next_stage, ballot } = stage
            .plan_close(&funded, 13, Some((ballot_rules(), parent_snapshot(12))))
            .unwrap()
        else {
            panic!("expected voting");
        };
        assert_eq!(next_stage.status(), DepositStageStatus::Voting);
        assert_eq!(ballot.proposal_id(), 4);
        assert_eq!(ballot.snapshot().finalized_height, 12);
        assert_eq!(ballot.action_digest(), stage.action_digest());
        assert_eq!(funded.total_udgt(), 5);
    }

    #[test]
    fn persisted_stage_rejects_corruption_and_trailing_bytes() {
        let stage = stage(5);
        let encoded = stage.encode().unwrap();
        assert_eq!(DepositStage::decode(&encoded).unwrap(), stage);
        let mut trailing = encoded.clone();
        trailing.push(0);
        assert!(DepositStage::decode(&trailing).is_err());

        let mut bad_digest = stage.clone();
        bad_digest.action_digest[0] ^= 1;
        assert!(bad_digest.encode().is_err());
        assert!(DepositStage::decode(&bincode::serialize(&bad_digest).unwrap()).is_err());

        let mut bad_close = stage.clone();
        bad_close.close_height += 1;
        assert!(DepositStage::decode(&bincode::serialize(&bad_close).unwrap()).is_err());

        let mut bad_status = stage;
        bad_status.status = DepositStageStatus::Rejected;
        assert!(DepositStage::decode(&bincode::serialize(&bad_status).unwrap()).is_err());
        bad_status.closed_height = Some(bad_status.close_height);
        assert_eq!(
            DepositStage::decode(&bad_status.encode().unwrap()).unwrap(),
            bad_status
        );
    }
}
