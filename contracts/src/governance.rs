use crate::tokenomics::TokenomicsProposal;
use crate::types::{Address, Amount, BlockNumber};
use scale::{Decode, Encode};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub struct GovernanceConfig {
    pub min_deposit: Amount,
    pub deposit_period: BlockNumber,
    pub voting_period: BlockNumber,
    pub timelock_period: BlockNumber,
    pub quorum_bps: u16,
    pub pass_threshold_bps: u16,
    pub veto_threshold_bps: u16,
}

impl Default for GovernanceConfig {
    fn default() -> Self {
        Self {
            min_deposit: 1_000_000,
            deposit_period: 300,
            voting_period: 300,
            timelock_period: 100,
            quorum_bps: 2_500,
            pass_threshold_bps: 5_000,
            veto_threshold_bps: 3_334,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub enum ProposalKind {
    Tokenomics(TokenomicsProposal),
    ParameterChange {
        module: String,
        key: String,
        value: String,
    },
    TreasuryTransfer {
        to: Address,
        amount: Amount,
        denom: String,
    },
    Custom {
        label: String,
        payload: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub enum ProposalStatus {
    DepositPeriod,
    VotingPeriod,
    Timelock,
    Rejected,
    Vetoed,
    Expired,
    Executed,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub enum VoteOption {
    Yes,
    No,
    NoWithVeto,
    Abstain,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize, Default)]
pub struct Tally {
    pub yes: Amount,
    pub no: Amount,
    pub no_with_veto: Amount,
    pub abstain: Amount,
}

impl Tally {
    pub fn participation(&self) -> Amount {
        self.yes + self.no + self.no_with_veto + self.abstain
    }

    pub fn decisive_votes(&self) -> Amount {
        self.yes + self.no + self.no_with_veto
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub struct VoteRecord {
    pub voter: Address,
    pub option: VoteOption,
    pub weight: Amount,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub struct Proposal {
    pub id: u64,
    pub proposer: Address,
    pub title: String,
    pub description: String,
    pub kind: ProposalKind,
    pub status: ProposalStatus,
    pub submit_block: BlockNumber,
    pub deposit_end_block: BlockNumber,
    pub voting_start_block: Option<BlockNumber>,
    pub voting_end_block: Option<BlockNumber>,
    pub executable_after_block: Option<BlockNumber>,
    pub total_deposit: Amount,
    pub deposits: BTreeMap<Address, Amount>,
    pub votes: BTreeMap<Address, VoteRecord>,
    pub tally: Tally,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub enum GovernanceAction {
    Tokenomics(TokenomicsProposal),
    ParameterChange {
        module: String,
        key: String,
        value: String,
    },
    TreasuryTransfer {
        to: Address,
        amount: Amount,
        denom: String,
    },
    Custom {
        label: String,
        payload: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GovernanceError {
    #[error("proposal not found")]
    ProposalNotFound,
    #[error("proposal is not in deposit period")]
    NotInDepositPeriod,
    #[error("proposal is not in voting period")]
    NotInVotingPeriod,
    #[error("proposal is not ready for execution")]
    NotReadyForExecution,
    #[error("proposal already finalized")]
    ProposalFinalized,
    #[error("deposit amount must be positive")]
    InvalidDeposit,
    #[error("voting weight must be positive")]
    InvalidVotingWeight,
    #[error("duplicate vote")]
    DuplicateVote,
    #[error("total eligible voting power must be positive")]
    InvalidEligibleVotingPower,
}

pub type GovernanceResult<T> = Result<T, GovernanceError>;

#[derive(Debug, Clone, Encode, Decode, Serialize, Deserialize)]
pub struct GovernanceContract {
    pub config: GovernanceConfig,
    pub next_proposal_id: u64,
    pub proposals: BTreeMap<u64, Proposal>,
}

impl GovernanceContract {
    pub fn new(config: GovernanceConfig) -> Self {
        Self {
            config,
            next_proposal_id: 1,
            proposals: BTreeMap::new(),
        }
    }

    pub fn submit_proposal(
        &mut self,
        proposer: Address,
        title: String,
        description: String,
        kind: ProposalKind,
        current_block: BlockNumber,
    ) -> u64 {
        let id = self.next_proposal_id;
        self.next_proposal_id += 1;

        self.proposals.insert(
            id,
            Proposal {
                id,
                proposer,
                title,
                description,
                kind,
                status: ProposalStatus::DepositPeriod,
                submit_block: current_block,
                deposit_end_block: current_block + self.config.deposit_period,
                voting_start_block: None,
                voting_end_block: None,
                executable_after_block: None,
                total_deposit: 0,
                deposits: BTreeMap::new(),
                votes: BTreeMap::new(),
                tally: Tally::default(),
            },
        );

        id
    }

    pub fn deposit(
        &mut self,
        proposal_id: u64,
        depositor: Address,
        amount: Amount,
        current_block: BlockNumber,
    ) -> GovernanceResult<()> {
        if amount == 0 {
            return Err(GovernanceError::InvalidDeposit);
        }

        let proposal = self
            .proposals
            .get_mut(&proposal_id)
            .ok_or(GovernanceError::ProposalNotFound)?;

        if proposal.status != ProposalStatus::DepositPeriod
            || current_block > proposal.deposit_end_block
        {
            return Err(GovernanceError::NotInDepositPeriod);
        }

        proposal.total_deposit += amount;
        *proposal.deposits.entry(depositor).or_insert(0) += amount;

        if proposal.total_deposit >= self.config.min_deposit {
            proposal.status = ProposalStatus::VotingPeriod;
            proposal.voting_start_block = Some(current_block);
            proposal.voting_end_block = Some(current_block + self.config.voting_period);
        }

        Ok(())
    }

    pub fn cast_vote(
        &mut self,
        proposal_id: u64,
        voter: Address,
        option: VoteOption,
        weight: Amount,
        current_block: BlockNumber,
    ) -> GovernanceResult<()> {
        if weight == 0 {
            return Err(GovernanceError::InvalidVotingWeight);
        }

        let proposal = self
            .proposals
            .get_mut(&proposal_id)
            .ok_or(GovernanceError::ProposalNotFound)?;

        if proposal.status != ProposalStatus::VotingPeriod
            || proposal
                .voting_end_block
                .is_some_and(|voting_end| current_block > voting_end)
        {
            return Err(GovernanceError::NotInVotingPeriod);
        }

        if proposal.votes.contains_key(&voter) {
            return Err(GovernanceError::DuplicateVote);
        }

        let vote = VoteRecord {
            voter: voter.clone(),
            option: option.clone(),
            weight,
        };
        proposal.votes.insert(voter, vote);

        match option {
            VoteOption::Yes => proposal.tally.yes += weight,
            VoteOption::No => proposal.tally.no += weight,
            VoteOption::NoWithVeto => proposal.tally.no_with_veto += weight,
            VoteOption::Abstain => proposal.tally.abstain += weight,
        }

        Ok(())
    }

    pub fn finalize(
        &mut self,
        proposal_id: u64,
        current_block: BlockNumber,
        total_eligible_voting_power: Amount,
    ) -> GovernanceResult<ProposalStatus> {
        let proposal = self
            .proposals
            .get_mut(&proposal_id)
            .ok_or(GovernanceError::ProposalNotFound)?;

        if proposal.status != ProposalStatus::VotingPeriod {
            return Err(GovernanceError::ProposalFinalized);
        }

        let voting_end = proposal
            .voting_end_block
            .ok_or(GovernanceError::NotInVotingPeriod)?;
        if current_block <= voting_end {
            return Err(GovernanceError::NotInVotingPeriod);
        }

        // A zero eligible voting power would make the quorum zero and let any
        // single vote finalize a proposal. Reject it outright.
        if total_eligible_voting_power == 0 {
            return Err(GovernanceError::InvalidEligibleVotingPower);
        }

        let participation = proposal.tally.participation();
        let decisive_votes = proposal.tally.decisive_votes();
        let quorum = total_eligible_voting_power * self.config.quorum_bps as u128 / 10_000;
        let veto_threshold = participation * self.config.veto_threshold_bps as u128 / 10_000;
        let pass_threshold = decisive_votes * self.config.pass_threshold_bps as u128 / 10_000;

        if participation < quorum {
            proposal.status = ProposalStatus::Rejected;
        } else if proposal.tally.no_with_veto >= veto_threshold && veto_threshold > 0 {
            proposal.status = ProposalStatus::Vetoed;
        } else if proposal.tally.yes >= pass_threshold && proposal.tally.yes > proposal.tally.no {
            proposal.status = ProposalStatus::Timelock;
            proposal.executable_after_block = Some(current_block + self.config.timelock_period);
        } else {
            proposal.status = ProposalStatus::Rejected;
        }

        Ok(proposal.status.clone())
    }

    pub fn execute(
        &mut self,
        proposal_id: u64,
        current_block: BlockNumber,
    ) -> GovernanceResult<GovernanceAction> {
        let proposal = self
            .proposals
            .get_mut(&proposal_id)
            .ok_or(GovernanceError::ProposalNotFound)?;

        if proposal.status != ProposalStatus::Timelock
            || proposal
                .executable_after_block
                .is_some_and(|execute_after| current_block < execute_after)
        {
            return Err(GovernanceError::NotReadyForExecution);
        }

        proposal.status = ProposalStatus::Executed;

        let action = match proposal.kind.clone() {
            ProposalKind::Tokenomics(proposal) => GovernanceAction::Tokenomics(proposal),
            ProposalKind::ParameterChange { module, key, value } => {
                GovernanceAction::ParameterChange { module, key, value }
            }
            ProposalKind::TreasuryTransfer { to, amount, denom } => {
                GovernanceAction::TreasuryTransfer { to, amount, denom }
            }
            ProposalKind::Custom { label, payload } => GovernanceAction::Custom { label, payload },
        };

        Ok(action)
    }

    pub fn proposal(&self, proposal_id: u64) -> Option<&Proposal> {
        self.proposals.get(&proposal_id)
    }
}

impl Default for GovernanceContract {
    fn default() -> Self {
        Self::new(GovernanceConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proposal_transitions_from_deposit_to_execution() {
        let mut governance = GovernanceContract::default();
        let proposal_id = governance.submit_proposal(
            "alice".into(),
            "Raise emission floor".into(),
            "Keep validator rewards predictable".into(),
            ProposalKind::ParameterChange {
                module: "tokenomics".into(),
                key: "min_emission_rate".into(),
                value: "500".into(),
            },
            10,
        );

        governance
            .deposit(proposal_id, "alice".into(), 1_000_000, 11)
            .unwrap();
        governance
            .cast_vote(proposal_id, "alice".into(), VoteOption::Yes, 5_000_000, 12)
            .unwrap();
        governance
            .cast_vote(
                proposal_id,
                "bob".into(),
                VoteOption::Abstain,
                1_000_000,
                13,
            )
            .unwrap();

        let status = governance.finalize(proposal_id, 400, 6_000_000).unwrap();
        assert_eq!(status, ProposalStatus::Timelock);

        let action = governance.execute(proposal_id, 500).unwrap();
        assert!(matches!(action, GovernanceAction::ParameterChange { .. }));
    }

    #[test]
    fn vetoed_proposal_does_not_execute() {
        let mut governance = GovernanceContract::default();
        let proposal_id = governance.submit_proposal(
            "alice".into(),
            "Dangerous change".into(),
            "Should be vetoed".into(),
            ProposalKind::Custom {
                label: "danger".into(),
                payload: vec![1, 2, 3],
            },
            1,
        );

        governance
            .deposit(proposal_id, "alice".into(), 1_000_000, 2)
            .unwrap();
        governance
            .cast_vote(
                proposal_id,
                "alice".into(),
                VoteOption::NoWithVeto,
                7_000_000,
                3,
            )
            .unwrap();

        let status = governance.finalize(proposal_id, 500, 7_000_000).unwrap();
        assert_eq!(status, ProposalStatus::Vetoed);
        assert!(governance.execute(proposal_id, 600).is_err());
    }
}
