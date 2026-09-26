//! Versioned governance ballot mechanics for consensus integration.
//! This module has no transaction decoder, balance authority, or activation path.
use super::validator_lifecycle::LifecycleState;
use crate::recovery_fees::RecoveryBook;
use anyhow::{ensure, Context, Result};
use bincode::Options;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const BALLOT_KEY_PREFIX: &str = "governance:v1:ballot:";
const MAX_STATE_BYTES: usize = 8 * 1024 * 1024;

pub fn ballot_key(proposal_id: u64) -> Result<String> {
    ensure!(proposal_id > 0, "Invalid governance proposal ID");
    Ok(format!("{BALLOT_KEY_PREFIX}{proposal_id:020}"))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    pub version: u16,
    pub chain_id: String,
    pub genesis_digest: [u8; 32],
    pub quorum_bps: u16,
    pub approval_bps: u16,
    pub veto_bps: u16,
    pub voting_period_blocks: u64,
    pub timelock_blocks: u64,
    pub max_voters: u32,
}
impl Rules {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "Unsupported governance ballot version");
        ensure!(
            !self.chain_id.is_empty()
                && self.chain_id.len() <= 128
                && !self.chain_id.chars().any(char::is_control),
            "Invalid governance chain ID"
        );
        ensure!(
            self.genesis_digest != [0; 32],
            "Governance genesis digest is absent"
        );
        ensure!(
            self.quorum_bps <= 10_000 && self.approval_bps <= 10_000 && self.veto_bps <= 10_000,
            "Governance basis points exceed 10000"
        );
        ensure!(
            self.voting_period_blocks > 0 && self.timelock_blocks > 0 && self.max_voters > 0,
            "Governance periods and voter capacity must be explicit and positive"
        );
        Ok(())
    }
}

fn required_weight(total: u128, bps: u16) -> Result<u128> {
    ensure!(bps <= 10_000, "Governance basis points exceed 10000");
    let bps = u128::from(bps);
    let whole = (total / 10_000)
        .checked_mul(bps)
        .context("Governance required weight overflow")?;
    let remainder = (total % 10_000) * bps;
    whole
        .checked_add(remainder / 10_000)
        .and_then(|value| value.checked_add(u128::from(remainder % 10_000 != 0)))
        .context("Governance required weight overflow")
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BondSnapshot {
    pub finalized_height: u64,
    pub source_app_hash: [u8; 32],
    pub owner_weights: BTreeMap<String, u128>,
    pub total_weight: u128,
}
impl BondSnapshot {
    /// Include every registered account with an effective bond. The stable
    /// account ID is the vote owner. Unregistered lifecycle owners and accounts
    /// without an effective bond do not enter the electorate or denominator.
    /// Consensus must read both records and the hash from one committed head.
    pub fn from_finalized_registered_accounts(
        lifecycle: &LifecycleState,
        book: &RecoveryBook,
        parent_height: u64,
        parent_app_hash: [u8; 32],
        max_voters: u32,
    ) -> Result<Self> {
        book.validate()?;
        ensure!(
            book.last_height == parent_height
                && book
                    .accounts
                    .values()
                    .all(|account| account.recovery.domain.chain_id == lifecycle.config.chain_id),
            "Governance account registry differs from finalized lifecycle parent"
        );
        let eligible_owners = book
            .accounts
            .keys()
            .filter(|id| lifecycle.effective.positions.contains_key(*id))
            .cloned()
            .collect();
        Self::from_finalized_lifecycle(
            lifecycle,
            parent_height,
            parent_app_hash,
            &eligible_owners,
            max_voters,
        )
    }

    /// Derive voting weights from the effective bonds at the finalized parent.
    /// The consensus caller must read `parent_app_hash` and `lifecycle` from the
    /// same verified committed head. This function cannot prove that provenance.
    pub fn from_finalized_lifecycle(
        lifecycle: &LifecycleState,
        parent_height: u64,
        parent_app_hash: [u8; 32],
        eligible_owners: &BTreeSet<String>,
        max_voters: u32,
    ) -> Result<Self> {
        ensure!(
            parent_height > 0 && lifecycle.last_height == parent_height,
            "Governance lifecycle height differs from finalized parent"
        );
        let weights =
            lifecycle.bonded_owner_weights_at_finalized_height(parent_height, eligible_owners)?;
        Self::new(parent_height, parent_app_hash, weights, max_voters)
    }

    pub fn new(
        finalized_height: u64,
        source_app_hash: [u8; 32],
        owner_weights: BTreeMap<String, u128>,
        max_voters: u32,
    ) -> Result<Self> {
        let total_weight = owner_weights.values().try_fold(0u128, |sum, weight| {
            sum.checked_add(*weight)
                .context("Governance snapshot weight overflow")
        })?;
        let snapshot = Self {
            finalized_height,
            source_app_hash,
            owner_weights,
            total_weight,
        };
        snapshot.validate(max_voters)?;
        Ok(snapshot)
    }
    fn validate(&self, max_voters: u32) -> Result<()> {
        ensure!(
            self.source_app_hash != [0; 32],
            "Finalized snapshot commitment is absent"
        );
        ensure!(
            !self.owner_weights.is_empty() && self.owner_weights.len() <= max_voters as usize,
            "Governance snapshot owner count outside limit"
        );
        let total = self
            .owner_weights
            .iter()
            .try_fold(0u128, |sum, (owner, weight)| {
                ensure!(
                    !owner.is_empty() && owner.len() <= 256 && !owner.chars().any(char::is_control),
                    "Invalid governance bond owner"
                );
                ensure!(*weight > 0, "Zero governance bond weight");
                sum.checked_add(*weight)
                    .context("Governance snapshot weight overflow")
            })?;
        ensure!(
            total == self.total_weight && total > 0,
            "Governance snapshot total differs"
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoteChoice {
    Yes,
    No,
    NoWithVeto,
    Abstain,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BallotStatus {
    Voting,
    Rejected,
    Passed { execute_at_height: u64 },
    Executed,
    FailedExecution,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tally {
    pub yes: u128,
    pub no: u128,
    pub no_with_veto: u128,
    pub abstain: u128,
    pub turnout: u128,
    pub decisive: u128,
    pub passes: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ballot {
    rules: Rules,
    proposal_id: u64,
    action_digest: [u8; 32],
    snapshot: BondSnapshot,
    voting_start_height: u64,
    voting_end_height: u64,
    votes: BTreeMap<String, VoteChoice>,
    status: BallotStatus,
    final_tally: Option<Tally>,
    closed_height: Option<u64>,
    execution_height: Option<u64>,
}
impl Ballot {
    pub fn rules(&self) -> &Rules {
        &self.rules
    }

    pub fn proposal_id(&self) -> u64 {
        self.proposal_id
    }
    pub fn snapshot(&self) -> &BondSnapshot {
        &self.snapshot
    }
    pub fn votes(&self) -> &BTreeMap<String, VoteChoice> {
        &self.votes
    }
    pub fn status(&self) -> &BallotStatus {
        &self.status
    }
    pub fn action_digest(&self) -> [u8; 32] {
        self.action_digest
    }
    pub fn execution_height(&self) -> Option<u64> {
        self.execution_height
    }
    /// The caller must bind the supplied snapshot to the finalized parent state.
    pub fn start(
        rules: Rules,
        proposal_id: u64,
        action_digest: [u8; 32],
        snapshot: BondSnapshot,
        voting_start_height: u64,
    ) -> Result<Self> {
        rules.validate()?;
        snapshot.validate(rules.max_voters)?;
        ensure!(proposal_id > 0, "Invalid governance proposal ID");
        ensure!(
            action_digest != [0; 32],
            "Governance action digest is absent"
        );
        ensure!(
            snapshot.finalized_height.checked_add(1) == Some(voting_start_height),
            "Governance snapshot is not the finalized parent"
        );
        let voting_end_height = voting_start_height
            .checked_add(rules.voting_period_blocks)
            .context("Governance voting end height overflow")?;
        let ballot = Self {
            rules,
            proposal_id,
            action_digest,
            snapshot,
            voting_start_height,
            voting_end_height,
            votes: BTreeMap::new(),
            status: BallotStatus::Voting,
            final_tally: None,
            closed_height: None,
            execution_height: None,
        };
        ballot.validate_full_lifecycle_capacity()?;
        ballot.encode()?;
        Ok(ballot)
    }
    pub fn vote(&mut self, owner: &str, choice: VoteChoice, height: u64) -> Result<()> {
        self.validate()?;
        ensure!(self.status == BallotStatus::Voting, "Voting is closed");
        ensure!(
            (self.voting_start_height..=self.voting_end_height).contains(&height),
            "Vote height outside voting window"
        );
        ensure!(
            self.snapshot.owner_weights.contains_key(owner),
            "Voter has no eligible bonded stake in the finalized snapshot"
        );
        ensure!(!self.votes.contains_key(owner), "Owner already voted");
        self.votes.insert(owner.to_owned(), choice);
        self.validate()
    }
    pub fn tally(&self) -> Result<Tally> {
        self.validate()?;
        let mut yes = 0u128;
        let mut no = 0u128;
        let mut no_with_veto = 0u128;
        let mut abstain = 0u128;
        for (owner, choice) in &self.votes {
            let weight = self.snapshot.owner_weights[owner];
            let bucket = match choice {
                VoteChoice::Yes => &mut yes,
                VoteChoice::No => &mut no,
                VoteChoice::NoWithVeto => &mut no_with_veto,
                VoteChoice::Abstain => &mut abstain,
            };
            *bucket = bucket
                .checked_add(weight)
                .context("Governance tally overflow")?;
        }
        let decisive = yes
            .checked_add(no)
            .and_then(|sum| sum.checked_add(no_with_veto))
            .context("Governance decisive weight overflow")?;
        let turnout = decisive
            .checked_add(abstain)
            .context("Governance turnout overflow")?;
        ensure!(
            turnout <= self.snapshot.total_weight,
            "Governance turnout exceeds snapshot"
        );
        let passes = turnout >= required_weight(self.snapshot.total_weight, self.rules.quorum_bps)?
            && decisive > 0
            && no_with_veto < required_weight(decisive, self.rules.veto_bps)?
            && yes >= required_weight(decisive, self.rules.approval_bps)?;
        Ok(Tally {
            yes,
            no,
            no_with_veto,
            abstain,
            turnout,
            decisive,
            passes,
        })
    }
    pub fn close(&mut self, finalized_height: u64) -> Result<Tally> {
        ensure!(self.status == BallotStatus::Voting, "Ballot already closed");
        ensure!(
            self.voting_end_height.checked_add(1) == Some(finalized_height),
            "Governance ballot must close at the next finalized height"
        );
        let tally = self.tally()?;
        let status = if tally.passes {
            BallotStatus::Passed {
                execute_at_height: finalized_height
                    .checked_add(self.rules.timelock_blocks)
                    .context("Governance execution height overflow")?,
            }
        } else {
            BallotStatus::Rejected
        };
        let mut next = self.clone();
        next.status = status;
        next.final_tally = Some(tally.clone());
        next.closed_height = Some(finalized_height);
        next.encode()?;
        *self = next;
        Ok(tally)
    }
    /// Returns only the pre-vote action digest. A separate executor must verify
    /// the exact action bytes and commit the result with every monetary effect.
    pub fn due_action(&self, finalized_height: u64) -> Result<[u8; 32]> {
        self.validate()?;
        match self.status {
            BallotStatus::Passed { execute_at_height } if finalized_height >= execute_at_height => {
                Ok(self.action_digest)
            }
            _ => anyhow::bail!("Governance action is not due"),
        }
    }
    pub fn record_execution(
        &mut self,
        finalized_height: u64,
        action_digest: [u8; 32],
        success: bool,
    ) -> Result<()> {
        ensure!(
            self.due_action(finalized_height)? == action_digest,
            "Governance action digest differs"
        );
        let mut next = self.clone();
        next.status = if success {
            BallotStatus::Executed
        } else {
            BallotStatus::FailedExecution
        };
        next.execution_height = Some(finalized_height);
        next.encode()?;
        *self = next;
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        self.rules.validate()?;
        self.snapshot.validate(self.rules.max_voters)?;
        ensure!(
            self.proposal_id > 0 && self.action_digest != [0; 32],
            "Invalid governance ballot identity"
        );
        ensure!(
            self.snapshot.finalized_height.checked_add(1) == Some(self.voting_start_height)
                && self
                    .voting_start_height
                    .checked_add(self.rules.voting_period_blocks)
                    == Some(self.voting_end_height),
            "Governance voting window differs"
        );
        ensure!(
            self.votes.len() <= self.snapshot.owner_weights.len()
                && self
                    .votes
                    .keys()
                    .all(|owner| self.snapshot.owner_weights.contains_key(owner)),
            "Governance vote owner is outside snapshot"
        );
        ensure!(
            matches!(self.status, BallotStatus::Voting) == self.final_tally.is_none()
                && self.final_tally.is_none() == self.closed_height.is_none(),
            "Governance final tally status differs"
        );
        ensure!(
            !matches!(self.status, BallotStatus::Voting) || self.execution_height.is_none(),
            "Voting ballot has an execution height"
        );
        if let Some(tally) = &self.final_tally {
            let closed = self
                .closed_height
                .context("Governance closing height absent")?;
            ensure!(
                self.voting_end_height.checked_add(1) == Some(closed),
                "Governance ballot closing height differs"
            );
            let mut pending = self.clone();
            pending.status = BallotStatus::Voting;
            pending.final_tally = None;
            pending.closed_height = None;
            pending.execution_height = None;
            ensure!(pending.tally()? == *tally, "Governance final tally differs");
            ensure!(
                tally.passes
                    == matches!(
                        self.status,
                        BallotStatus::Passed { .. }
                            | BallotStatus::Executed
                            | BallotStatus::FailedExecution
                    ),
                "Governance outcome differs from tally"
            );
            if let BallotStatus::Passed { execute_at_height } = self.status {
                ensure!(
                    closed.checked_add(self.rules.timelock_blocks) == Some(execute_at_height),
                    "Governance timelock height differs"
                );
            }
            if let Some(executed) = self.execution_height {
                ensure!(
                    matches!(
                        self.status,
                        BallotStatus::Executed | BallotStatus::FailedExecution
                    ) && closed
                        .checked_add(self.rules.timelock_blocks)
                        .is_some_and(|due| executed >= due),
                    "Governance execution height differs"
                );
            } else {
                ensure!(
                    !matches!(
                        self.status,
                        BallotStatus::Executed | BallotStatus::FailedExecution
                    ),
                    "Governance execution height absent"
                );
            }
        }
        Ok(())
    }
    /// Reserve enough encoded space for every snapshot owner to vote and for
    /// the largest terminal record. Check once when state enters this module.
    fn validate_full_lifecycle_capacity(&self) -> Result<()> {
        let mut full = self.clone();
        for owner in self.snapshot.owner_weights.keys() {
            full.votes.entry(owner.clone()).or_insert(VoteChoice::Yes);
        }
        full.status = BallotStatus::Passed {
            execute_at_height: 0,
        };
        full.final_tally = Some(Tally {
            yes: 0,
            no: 0,
            no_with_veto: 0,
            abstain: 0,
            turnout: 0,
            decisive: 0,
            passes: false,
        });
        full.closed_height = Some(0);
        full.execution_height = Some(0);
        ensure!(
            bincode::serialized_size(&full)? <= MAX_STATE_BYTES as u64,
            "Governance ballot full lifecycle exceeds size limit"
        );
        Ok(())
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let bytes = bincode::serialize(self)?;
        ensure!(
            bytes.len() <= MAX_STATE_BYTES,
            "Governance ballot exceeds size limit"
        );
        Ok(bytes)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= MAX_STATE_BYTES,
            "Governance ballot exceeds size limit"
        );
        let value: Self = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_limit(MAX_STATE_BYTES as u64)
            .reject_trailing_bytes()
            .deserialize(bytes)?;
        value.validate()?;
        value.validate_full_lifecycle_capacity()?;
        ensure!(
            bincode::serialize(&value)? == bytes,
            "Noncanonical governance ballot"
        );
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ballot() -> Ballot {
        let rules = Rules {
            version: 1,
            chain_id: "development-chain".into(),
            genesis_digest: [1; 32],
            quorum_bps: 6700,
            approval_bps: 5000,
            veto_bps: 3333,
            voting_period_blocks: 10,
            timelock_blocks: 3,
            max_voters: 4,
        };
        let snapshot = BondSnapshot::new(
            9,
            [2; 32],
            BTreeMap::from([("a".into(), 6), ("b".into(), 5), ("c".into(), 100)]),
            4,
        )
        .unwrap();
        Ballot::start(rules, 1, [3; 32], snapshot, 10).unwrap()
    }
    #[test]
    fn snapshot_owner_vote_and_restart_are_fixed() {
        assert_eq!(
            ballot_key(1).unwrap(),
            "governance:v1:ballot:00000000000000000001"
        );
        assert!(!ballot_key(10).unwrap().starts_with(&ballot_key(1).unwrap()));
        let mut ballot = ballot();
        assert!(ballot.vote("outside", VoteChoice::Yes, 10).is_err());
        ballot.vote("a", VoteChoice::Yes, 10).unwrap();
        assert!(ballot.vote("a", VoteChoice::No, 11).is_err());
        let restored = Ballot::decode(&ballot.encode().unwrap()).unwrap();
        assert_eq!(restored.snapshot.owner_weights["a"], 6);
        assert_eq!(restored.tally().unwrap().yes, 6);
        let mut corrupt = restored;
        corrupt.snapshot.owner_weights.insert("a".into(), 7);
        assert!(corrupt.encode().is_err());
    }
    #[test]
    fn voting_ballot_rejects_forged_execution_height() {
        let mut ballot = ballot();
        ballot.execution_height = Some(24);
        assert!(ballot.encode().is_err());
        let bytes = bincode::serialize(&ballot).unwrap();
        assert!(Ballot::decode(&bytes).is_err());
    }
    #[test]
    fn abstentions_count_for_quorum_but_veto_uses_decisive_weight() {
        let mut ballot = ballot();
        ballot.vote("a", VoteChoice::Yes, 10).unwrap();
        ballot.vote("b", VoteChoice::NoWithVeto, 10).unwrap();
        ballot.vote("c", VoteChoice::Abstain, 10).unwrap();
        let tally = ballot.close(21).unwrap();
        assert_eq!(tally.turnout, 111);
        assert_eq!(tally.decisive, 11);
        assert!(!tally.passes);
        assert_eq!(ballot.status, BallotStatus::Rejected);
    }
    #[test]
    fn approved_action_waits_for_timelock_and_executes_once() {
        let mut ballot = ballot();
        ballot.vote("c", VoteChoice::Yes, 20).unwrap();
        let tally = ballot.close(21).unwrap();
        assert!(tally.passes);
        let mut altered = ballot.clone();
        altered.status = BallotStatus::Passed {
            execute_at_height: 22,
        };
        assert!(altered.encode().is_err());
        assert!(ballot.due_action(23).is_err());
        assert_eq!(ballot.due_action(24).unwrap(), [3; 32]);
        assert!(ballot.record_execution(24, [4; 32], true).is_err());
        ballot.record_execution(24, [3; 32], true).unwrap();
        assert!(ballot.due_action(25).is_err());
        assert!(ballot.record_execution(25, [3; 32], true).is_err());
    }
    #[test]
    fn exact_quorum_rounds_up_and_rejects_insufficient_turnout() {
        let mut ballot = ballot();
        ballot.snapshot.owner_weights = BTreeMap::from([("a".into(), 2), ("b".into(), 1)]);
        ballot.snapshot.total_weight = 3;
        ballot.vote("a", VoteChoice::Yes, 10).unwrap();
        assert_eq!(required_weight(3, 6700).unwrap(), 3);
        assert!(!ballot.close(21).unwrap().passes);
        assert_eq!(required_weight(u128::MAX, 10_000).unwrap(), u128::MAX);
    }

    #[test]
    fn snapshot_must_reserve_space_for_all_votes_and_terminal_state() {
        fn owner(index: usize) -> String {
            format!("{index:08x}{}", "x".repeat(248))
        }
        let mut rules = ballot().rules;
        let one = BondSnapshot::new(9, [2; 32], BTreeMap::from([(owner(0), 1)]), 1).unwrap();
        rules.max_voters = 1;
        let sample = Ballot::start(rules.clone(), 1, [3; 32], one, 10).unwrap();
        // Each owner adds 280 snapshot bytes and 268 maximum vote bytes.
        let base = usize::try_from(
            bincode::serialized_size(&{
                let mut full = sample.clone();
                full.votes.insert(owner(0), VoteChoice::Yes);
                full.status = BallotStatus::Passed {
                    execute_at_height: 0,
                };
                full.final_tally = Some(Tally {
                    yes: 0,
                    no: 0,
                    no_with_veto: 0,
                    abstain: 0,
                    turnout: 0,
                    decisive: 0,
                    passes: false,
                });
                full.closed_height = Some(0);
                full.execution_height = Some(0);
                full
            })
            .unwrap(),
        )
        .unwrap()
            - 548;
        let count = (MAX_STATE_BYTES - base) / 548;
        rules.max_voters = u32::try_from(count).unwrap();
        let owners = (0..count).map(|index| (owner(index), 1)).collect();
        let snapshot = BondSnapshot::new(9, [2; 32], owners, rules.max_voters).unwrap();
        let mut full = Ballot::start(rules.clone(), 1, [3; 32], snapshot, 10).unwrap();
        full.vote(&owner(0), VoteChoice::Yes, 10).unwrap();
        full.vote(&owner(1), VoteChoice::Abstain, 10).unwrap();
        assert!(full.encode().unwrap().len() <= MAX_STATE_BYTES);
        rules.max_voters += 1;
        let oversized = (0..=count).map(|index| (owner(index), 1)).collect();
        let snapshot = BondSnapshot::new(9, [2; 32], oversized, rules.max_voters).unwrap();
        assert!(Ballot::start(rules, 2, [3; 32], snapshot, 10)
            .unwrap_err()
            .to_string()
            .contains("full lifecycle exceeds size limit"));
    }
}
