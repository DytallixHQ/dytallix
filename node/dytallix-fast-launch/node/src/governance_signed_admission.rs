//! Read-only E04 ordinary-v3 governance preadmission.
//! A successful assessment is not a fee charge, reservation, execution permit,
//! or consensus activation. The caller must recheck the committed parent before use.

use crate::{
    governance_v3_fee_settlement::V3FeeReceipt,
    ordinary_authority::validate_nonce_mirrors,
    recovery_fees::RecoveryBook,
    runtime::{
        governance_ballot::{BondSnapshot, VoteChoice as BallotChoice},
        governance_candidate::GovernanceCandidateConfig,
        governance_deposit_stage::DepositStageStatus,
        governance_ordered_admission::AdmissionAction,
        governance_state::{GovernanceState, ProposalRecord},
        validator_lifecycle::LifecycleState,
    },
};
use anyhow::{ensure, Context, Result};
use dytallix_protocol_types::ordinary_v3::{self as v3, Action, SignedOrdinary};
use dytallix_runtime_crypto::ordinary_v3::verify_signed;
use std::collections::BTreeMap;

/// Every reference must describe the same committed parent. The caller supplies
/// the app hash; this module cannot prove its provenance.
pub struct AdmissionParent<'a> {
    pub candidate: &'a GovernanceCandidateConfig,
    pub recovery: &'a RecoveryBook,
    pub native_nonces: &'a BTreeMap<String, u64>,
    pub lifecycle: &'a LifecycleState,
    pub governance: &'a GovernanceState,
    pub app_hash: [u8; 32],
}

/// One ordered block checkpoint. The consensus caller must derive this view
/// from the verified parent plus earlier accepted transactions in this block.
pub struct AdmissionStage<'a> {
    pub recovery: &'a RecoveryBook,
    pub native_nonces: &'a BTreeMap<String, u64>,
    pub proposals: &'a BTreeMap<u64, ProposalRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssessedAction {
    Proposal {
        proposal_id: u64,
        action_class: u16,
        action_data: Vec<u8>,
        action_digest: [u8; 32],
    },
    Deposit {
        proposal_id: u64,
        amount_udgt: u128,
    },
    Vote {
        proposal_id: u64,
        choice: BallotChoice,
    },
}

/// Authentication and current-state checks only. It carries no mutable state
/// or settlement authority. Fee liquidity and DGT custody remain unchecked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreadmissionAssessment {
    transaction_id: [u8; 32],
    envelope_hash: [u8; 32],
    actor: [u8; 32],
    admission_height: u64,
    contract_version: u16,
    profile_version: u64,
    profile_digest: [u8; 32],
    gas_limit: u64,
    maximum_fee: u128,
    nonce_before: u64,
    nonce_after: u64,
    action: AssessedAction,
}

impl PreadmissionAssessment {
    pub fn transaction_id(&self) -> [u8; 32] {
        self.transaction_id
    }

    pub fn envelope_hash(&self) -> [u8; 32] {
        self.envelope_hash
    }

    pub fn actor(&self) -> [u8; 32] {
        self.actor
    }

    pub fn admission_height(&self) -> u64 {
        self.admission_height
    }

    pub fn nonce_before(&self) -> u64 {
        self.nonce_before
    }

    pub fn nonce_after(&self) -> u64 {
        self.nonce_after
    }

    pub fn action(&self) -> &AssessedAction {
        &self.action
    }

    /// Carry the verified actor and action into the ordered state planner.
    /// This does not authorize fees, custody changes, or a consensus commit.
    pub fn ordered_action(&self) -> AdmissionAction {
        match &self.action {
            AssessedAction::Proposal {
                proposal_id,
                action_class,
                action_data,
                action_digest,
            } => AdmissionAction::Proposal {
                proposal_id: *proposal_id,
                action_class: *action_class,
                action_data: action_data.clone(),
                action_digest: *action_digest,
            },
            AssessedAction::Deposit {
                proposal_id,
                amount_udgt,
            } => AdmissionAction::Deposit {
                proposal_id: *proposal_id,
                owner: self.actor,
                amount_udgt: *amount_udgt,
            },
            AssessedAction::Vote {
                proposal_id,
                choice,
            } => AdmissionAction::Vote {
                proposal_id: *proposal_id,
                owner: self.actor,
                choice: *choice,
            },
        }
    }

    /// Reject a fee receipt produced for a different signed admission. The
    /// executor must also apply the receipt's success or rollback disposition.
    pub(crate) fn bind_fee_receipt(&self, receipt: &V3FeeReceipt) -> Result<()> {
        ensure!(
            receipt.version == 2
                && receipt.transaction_id == self.transaction_id
                && receipt.envelope_hash == self.envelope_hash
                && receipt.actor == self.actor
                && receipt.block_height == self.admission_height
                && receipt.contract_version == self.contract_version
                && receipt.profile_version == self.profile_version
                && receipt.profile_digest == self.profile_digest
                && receipt.gas_limit == self.gas_limit
                && receipt.gas_used <= self.gas_limit
                && receipt.reserved_cap == self.maximum_fee
                && receipt.reserved_cap.checked_sub(receipt.charge) == Some(receipt.released_cap)
                && receipt.nonce_before == self.nonce_before
                && receipt.nonce_after == self.nonce_after,
            "Governance fee receipt differs from signed admission"
        );
        Ok(())
    }
}

/// Check one signed governance action against explicit candidate rules and the
/// current parent. This does not call `validate_for_activation`: E04 state
/// transitions and action-class approval remain pending.
pub fn assess_signed(
    signed: &SignedOrdinary,
    parent: &AdmissionParent<'_>,
) -> Result<PreadmissionAssessment> {
    let admission_height = parent
        .recovery
        .last_height
        .checked_add(1)
        .context("Governance height exhausted")?;
    assess_signed_view(
        signed,
        parent,
        parent.recovery,
        parent.native_nonces,
        parent.governance.proposals(),
        parent.governance.next_proposal_id(),
        admission_height,
    )
}

/// Check one signed transaction against earlier staged block effects while
/// keeping proposer voting weight fixed at the committed parent. This is not
/// a consensus permit: the caller must prove that `stage` came from `parent`.
pub fn assess_signed_staged(
    signed: &SignedOrdinary,
    parent: &AdmissionParent<'_>,
    stage: &AdmissionStage<'_>,
) -> Result<PreadmissionAssessment> {
    let admission_height = parent
        .recovery
        .last_height
        .checked_add(1)
        .context("Governance height exhausted")?;
    stage.recovery.validate()?;
    ensure!(
        stage.recovery.last_height == admission_height
            && stage.recovery.profile == parent.recovery.profile
            && stage
                .recovery
                .accounts
                .keys()
                .eq(parent.recovery.accounts.keys()),
        "Governance staged account inventory or height differs"
    );
    for (id, account) in &stage.recovery.accounts {
        let origin = &parent.recovery.accounts[id];
        ensure!(
            account.address == origin.address
                && account.recovery.domain == origin.recovery.domain
                && account.recovery.config == origin.recovery.config,
            "Governance staged account origin differs"
        );
    }
    let staged_governance = parent
        .governance
        .plan_commit(admission_height, stage.proposals.clone())?;
    assess_signed_view(
        signed,
        parent,
        stage.recovery,
        stage.native_nonces,
        stage.proposals,
        staged_governance.next_proposal_id(),
        admission_height,
    )
}

fn assess_signed_view(
    signed: &SignedOrdinary,
    parent: &AdmissionParent<'_>,
    current_recovery: &RecoveryBook,
    current_nonces: &BTreeMap<String, u64>,
    current_proposals: &BTreeMap<u64, ProposalRecord>,
    expected_next_proposal_id: u64,
    admission_height: u64,
) -> Result<PreadmissionAssessment> {
    let config = parent.candidate;
    config.validate_shape()?;
    parent.governance.validate()?;
    validate_nonce_mirrors(parent.recovery, parent.native_nonces)?;
    validate_nonce_mirrors(current_recovery, current_nonces)?;
    let height = parent.recovery.last_height;
    ensure!(
        height.checked_add(1) == Some(admission_height),
        "Governance admission height differs from parent"
    );
    ensure!(
        parent.governance.finalized_height() == height
            && parent.lifecycle.last_height == height
            && parent.app_hash != [0; 32],
        "Governance inputs are not one committed parent"
    );
    parent.lifecycle.validate()?;
    ensure!(
        parent.governance.chain_id() == config.chain_id
            && parent.governance.genesis_digest() == config.genesis_digest
            && parent.lifecycle.config.chain_id == config.chain_id,
        "Governance candidate and parent chain differ"
    );
    let limits = config.fee_profile.limits();
    let verified = verify_signed(signed, &limits)?;
    let body = verified.body();
    config
        .fee_profile
        .validate_signed_request(body, admission_height)?;
    ensure!(
        body.domain.chain_id == config.chain_id
            && body.domain.genesis_digest == config.genesis_digest,
        "Governance signed domain differs from candidate"
    );
    let actor = body.domain.account_id;
    let account = current_recovery
        .accounts
        .get(&hex::encode(actor))
        .context("Unregistered governance actor")?;
    let recovery = &account.recovery;
    ensure!(
        recovery.domain == body.domain,
        "Governance signed domain differs from account"
    );
    ensure!(
        recovery.active_key == body.key
            && recovery.active_generation == body.authorization_generation,
        "Governance key or generation is stale"
    );
    ensure!(
        recovery.spending_nonce == body.spending_nonce,
        "Governance spending nonce is stale"
    );
    let nonce_after = body
        .spending_nonce
        .checked_add(1)
        .context("Governance nonce exhausted")?;
    ensure!(
        admission_height < body.expiry_height
            && body.expiry_height - admission_height <= limits.max_expiry_lifetime,
        "Governance request expired or exceeds configured lifetime"
    );
    ensure!(
        recovery.outgoing_allowed(),
        "Protected account cannot authorize governance"
    );

    let owner = hex::encode(actor);
    let action = match body.actions.as_slice() {
        [Action::GovernanceProposal {
            proposal_id,
            action_class,
            action_data,
            action_digest,
        }] => {
            ensure!(
                *proposal_id == expected_next_proposal_id,
                "Governance proposal ID differs"
            );
            let class = config
                .action_classes
                .iter()
                .find(|item| item.class == *action_class)
                .context("Governance action class lacks candidate approval")?;
            ensure!(
                action_data.len() <= class.max_data_bytes as usize
                    && v3::governance_action_digest(*action_class, action_data)? == *action_digest,
                "Governance action exceeds class bound or digest differs"
            );
            let snapshot = BondSnapshot::from_finalized_registered_accounts(
                parent.lifecycle,
                parent.recovery,
                height,
                parent.app_hash,
                config.ballot.max_voters,
            )?;
            ensure!(
                snapshot.owner_weights.contains_key(&owner),
                "Proposer has no effective registered bond"
            );
            AssessedAction::Proposal {
                proposal_id: *proposal_id,
                action_class: *action_class,
                action_data: action_data.clone(),
                action_digest: *action_digest,
            }
        }
        [Action::GovernanceDeposit {
            proposal_id,
            amount_udgt,
        }] => {
            ensure!(*amount_udgt > 0, "Governance deposit is zero");
            let record = current_proposals
                .get(proposal_id)
                .context("Governance deposit proposal is absent")?;
            ensure!(
                record.stage().rules() == &config.deposit
                    && record.stage().status() == DepositStageStatus::Collecting
                    && admission_height < record.stage().close_height(),
                "Governance deposit rules differ or period is closed"
            );
            AssessedAction::Deposit {
                proposal_id: *proposal_id,
                amount_udgt: *amount_udgt,
            }
        }
        [Action::GovernanceVote {
            proposal_id,
            choice,
        }] => {
            let record = current_proposals
                .get(proposal_id)
                .context("Governance vote proposal is absent")?;
            let ballot = record.ballot().context("Governance ballot is absent")?;
            ensure!(
                ballot.rules() == &config.ballot,
                "Governance ballot rules differ from candidate"
            );
            let mut test = ballot.clone();
            let choice = match choice {
                v3::VoteChoice::Yes => BallotChoice::Yes,
                v3::VoteChoice::No => BallotChoice::No,
                v3::VoteChoice::NoWithVeto => BallotChoice::NoWithVeto,
                v3::VoteChoice::Abstain => BallotChoice::Abstain,
            };
            test.vote(&owner, choice, admission_height)?;
            AssessedAction::Vote {
                proposal_id: *proposal_id,
                choice,
            }
        }
        _ => anyhow::bail!("Ordinary-v3 requires exactly one governance action"),
    };
    Ok(PreadmissionAssessment {
        transaction_id: verified.transaction_id(),
        envelope_hash: verified.envelope_hash(),
        actor,
        admission_height,
        contract_version: body.ordinary_fee_contract_version,
        profile_version: body.fee_profile_version,
        profile_digest: body.fee_profile_digest,
        gas_limit: body.gas_limit,
        maximum_fee: body.maximum_fee,
        nonce_before: body.spending_nonce,
        nonce_after,
        action,
    })
}

#[cfg(all(test, feature = "pqc-fips204"))]
#[path = "governance_signed_admission_tests.rs"]
mod tests;
