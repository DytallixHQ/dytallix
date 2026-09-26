use super::*;
use crate::{
    addr::{AccountAddress, AddressNetwork},
    recovery_fees::RecoveryAccount,
    runtime::{
        governance_ballot::Rules,
        governance_candidate::{
            ActionClassLimit, Cancellation, EntryPolicy, PendingPolicies, PolicyDecision,
            ProposerEligibility, ValidatorVoting, VoteDelegation, CANDIDATE_SCHEMA_VERSION,
        },
        governance_deposit_stage::{DepositRules, DepositStage},
        governance_escrow::DepositEscrow,
        governance_state::{ProposalRecord, GOVERNANCE_STATE_VERSION},
        reward_runtime::{RewardConfig, RewardState, ValidatorStatus},
        validator_lifecycle::{LifecycleConfig, ValidatorIdentity, PROFILE},
    },
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use dytallix_protocol_types::{
    ordinary::{self, Limits},
    ordinary_fees::FeeProfile as OrdinaryFeeProfile,
    ordinary_fees_v3::{self, FeeProfileV3, ORDINARY_FEE_CONTRACT_VERSION},
    recovery::{KeyIdentity, RecoveryConfig, RecoveryDomain, RecoveryState},
    recovery_sponsor::FeeProfile as RecoveryFeeProfile,
};
use fips204::{
    ml_dsa_65,
    traits::{KeyGen, SerDes, Signer},
};
use std::collections::BTreeSet;

struct Fixture {
    secret: ml_dsa_65::PrivateKey,
    signed: SignedOrdinary,
    candidate: GovernanceCandidateConfig,
    book: RecoveryBook,
    nonces: BTreeMap<String, u64>,
    lifecycle: LifecycleState,
    governance: GovernanceState,
}

impl Fixture {
    fn new() -> Self {
        let (public, secret) = ml_dsa_65::KG::try_keygen_with_rng(&mut rand_core::OsRng).unwrap();
        let key = KeyIdentity {
            algorithm: "mldsa65".into(),
            public_key: public.into_bytes().to_vec(),
        };
        let id = [3; 32];
        let domain = RecoveryDomain {
            network: 3,
            chain_id: "governance-test".into(),
            genesis_digest: [7; 32],
            account_id: id,
        };
        let recovery = RecoveryState::new(
            domain.clone(),
            RecoveryConfig {
                timing_version: 1,
                recovery_delay: 2,
                finalization_window: 3,
                policy_delay: 2,
                policy_window: 3,
                submission_lifetime: 100,
                algorithms: BTreeMap::from([("mldsa65".into(), 1952)]),
            },
            key.clone(),
            0,
        )
        .unwrap();
        let address = AccountAddress::from_account_id(AddressNetwork::Development, id).encode();
        let recovery_profile = RecoveryFeeProfile {
            version: 1,
            activation_height: 1,
            denomination: "udrt".into(),
            gas_price: 1,
            minimum_gas: 1,
            max_transaction_gas: 100_000,
            max_block_gas: 200_000,
            max_block_recovery_bytes: 100_000,
            max_block_recovery_signatures: 1,
            max_fee_cap: 100_000,
            max_pending_accounts: 1,
            max_due_expiry_events_per_height: 1,
            mandatory_expiry_gas_budget: 1,
            expiry_event_gas_cost: 1,
            action_costs: [1; 9],
            wire_byte_cost: 1,
            read_byte_cost: 1,
            write_byte_cost: 1,
            signature_costs: BTreeMap::from([("mldsa65".into(), 1)]),
        };
        let account = RecoveryAccount {
            address: address.clone(),
            recovery,
            sponsor_nonce: 0,
        };
        let mut book = RecoveryBook::new(recovery_profile, vec![account]).unwrap();
        book.accounts.get_mut(&hex::encode(id)).unwrap().recovery = book.accounts[&hex::encode(id)]
            .recovery
            .advance_height(1)
            .unwrap();
        book.last_height = 1;
        book.validate().unwrap();
        let limits = Limits {
            max_wire_bytes: 100_000,
            max_actions: 2,
            max_identifier_bytes: 100,
            max_data_bytes: 100,
            max_memo_bytes: 100,
            max_consensus_key_bytes: 100,
            max_proof_bytes: 100,
            max_expiry_lifetime: 100,
            allowed_algorithms: BTreeSet::from(["mldsa65".into()]),
        };
        let base = OrdinaryFeeProfile {
            ordinary_fee_contract_version: 1,
            version: 1,
            activation_height: 1,
            denomination: ordinary::Denomination::Udrt,
            gas_price: 1,
            minimum_gas: 1,
            max_transaction_gas: 1000,
            max_block_transaction_gas: 1000,
            max_block_transaction_bytes: 100_000,
            max_block_signature_checks: 1,
            max_fee_cap: 1000,
            limits,
            transaction_overhead: 1,
            receipt_metadata_cost: 1,
            wire_byte_cost: 1,
            read_byte_cost: 1,
            write_byte_cost: 1,
            action_costs: [1; 12],
            signature_costs: BTreeMap::from([("mldsa65".into(), 1)]),
            validator_proof_profile_digest: [1; 32],
            validator_proof_costs: BTreeMap::from([("mldsa65".into(), 1)]),
        };
        let candidate = GovernanceCandidateConfig {
            schema_version: CANDIDATE_SCHEMA_VERSION,
            chain_id: domain.chain_id.clone(),
            genesis_digest: domain.genesis_digest,
            activation_height: 2,
            fee_profile: FeeProfileV3 {
                base,
                version: 2,
                activation_height: 2,
                max_governance_action_bytes: 100,
                governance_action_costs: [1; 3],
            },
            ballot: Rules {
                version: 1,
                chain_id: domain.chain_id.clone(),
                genesis_digest: domain.genesis_digest,
                quorum_bps: 1,
                approval_bps: 1,
                veto_bps: 1,
                voting_period_blocks: 1,
                timelock_blocks: 1,
                max_voters: 1,
            },
            deposit: DepositRules {
                deposit_period_blocks: 1,
                minimum_deposit_udgt: 1,
                max_action_bytes: 100,
            },
            action_classes: vec![],
            entry_policy: EntryPolicy {
                proposer_eligibility:
                    ProposerEligibility::RegisteredOwnerWithEffectiveBondAtFinalizedParent,
                validator_voting: ValidatorVoting::OwnEffectiveBondOnly,
                vote_delegation: VoteDelegation::Disabled,
                cancellation: Cancellation::Disabled,
            },
            pending_policies: PendingPolicies {
                exact_state_transitions: PolicyDecision::Pending,
            },
        };
        let action_data = vec![1, 2, 3];
        let signed = SignedOrdinary {
            body: v3::OrdinaryTransaction {
                domain,
                authorization_generation: 0,
                spending_nonce: 0,
                key,
                expiry_height: 4,
                ordinary_fee_contract_version: ORDINARY_FEE_CONTRACT_VERSION,
                fee_profile_version: candidate.fee_profile.version,
                fee_profile_digest: ordinary_fees_v3::profile_digest(&candidate.fee_profile)
                    .unwrap(),
                fee_denomination: v3::Denomination::Udrt,
                maximum_fee: 1000,
                gas_limit: 1000,
                memo: String::new(),
                actions: vec![Action::GovernanceProposal {
                    proposal_id: 1,
                    action_class: 17,
                    action_digest: v3::governance_action_digest(17, &action_data).unwrap(),
                    action_data,
                }],
            },
            signature: vec![],
        };
        let owner = hex::encode(id);
        let lifecycle_config = LifecycleConfig {
            version: 1,
            profile: PROFILE.into(),
            chain_id: "governance-test".into(),
            approved_operators: BTreeMap::from([("validator".into(), owner.clone())]),
            min_self_bond: 1,
            max_active: 1,
            evidence_max_age_blocks: 1,
            evidence_max_age_seconds: 1,
            processing_margin_blocks: 1,
            processing_margin_seconds: 1,
        };
        let mut rewards = RewardState::new(
            RewardConfig {
                version: 2,
                activation_height: 1,
                decimals: 6,
                profile: "development".into(),
                chain_id: "governance-test".into(),
                genesis_digest: "07".repeat(32),
                max_validators: 1,
                max_positions: 1,
            },
            BTreeMap::from([(
                "validator".into(),
                ValidatorStatus {
                    active: true,
                    jailed: false,
                },
            )]),
            BTreeMap::from([(owner.clone(), BTreeMap::from([("validator".into(), 10)]))]),
            BTreeMap::new(),
        )
        .unwrap();
        let mut lifecycle = LifecycleState::new(
            lifecycle_config,
            BTreeMap::from([(
                "validator".into(),
                ValidatorIdentity {
                    owner,
                    pubkey_base64: B64.encode(&signed.body.key.public_key),
                },
            )]),
            &rewards,
        )
        .unwrap();
        lifecycle.advance(1, 100, &mut rewards).unwrap();
        let governance = GovernanceState::new(
            GOVERNANCE_STATE_VERSION,
            "governance-test".into(),
            [7; 32],
            1,
        )
        .unwrap();
        let nonces = BTreeMap::from([(address, 0)]);
        let mut fixture = Self {
            secret,
            signed,
            candidate,
            book,
            nonces,
            lifecycle,
            governance,
        };
        fixture.resign();
        fixture
    }

    fn resign(&mut self) {
        let bytes =
            v3::signing_bytes(&self.signed.body, &self.candidate.fee_profile.limits()).unwrap();
        self.signed.signature = self.secret.try_sign(&bytes, &[]).unwrap().to_vec();
    }

    fn assess(&self) -> Result<PreadmissionAssessment> {
        assess_signed(
            &self.signed,
            &AdmissionParent {
                candidate: &self.candidate,
                recovery: &self.book,
                native_nonces: &self.nonces,
                lifecycle: &self.lifecycle,
                governance: &self.governance,
                app_hash: [9; 32],
            },
        )
    }
}

#[test]
fn signature_and_current_authority_are_both_required() {
    let mut f = Fixture::new();
    assert!(f
        .assess()
        .unwrap_err()
        .to_string()
        .contains("candidate approval"));
    f.candidate.action_classes.push(ActionClassLimit {
        class: 17,
        max_data_bytes: 3,
        approval_digest: [8; 32],
    });
    let assessed = f.assess().unwrap();
    assert!(matches!(
        assessed.ordered_action(),
        AdmissionAction::Proposal { proposal_id: 1, .. }
    ));
    f.signed.signature[0] ^= 1;
    assert!(f.assess().is_err());
    f.resign();
    f.book
        .accounts
        .get_mut(&hex::encode([3; 32]))
        .unwrap()
        .recovery
        .spending_nonce = 1;
    f.nonces.values_mut().next().map(|nonce| *nonce = 1);
    assert!(f
        .assess()
        .unwrap_err()
        .to_string()
        .contains("nonce is stale"));
}

#[test]
fn signed_proposal_fee_and_governance_share_one_staged_overlay() {
    use crate::{
        governance_v3_fee_settlement::{
            plan_governance_fee_from_settlement, ActionEnd, ActionWork,
        },
        ordinary_meter::{RecoveryCeilings, SharedBlockMeter},
        runtime::governance_ordered_admission::plan_ordered_admission_block,
        settlement::{Settlement, FEE_KEY},
        storage::state::Storage,
    };
    use std::sync::Arc;

    let mut f = Fixture::new();
    f.candidate.action_classes.push(ActionClassLimit {
        class: 17,
        max_data_bytes: 3,
        approval_digest: [8; 32],
    });
    f.candidate.fee_profile.base.wire_byte_cost = 0;
    f.signed.body.fee_profile_digest =
        ordinary_fees_v3::profile_digest(&f.candidate.fee_profile).unwrap();
    f.resign();
    let assessment = f.assess().unwrap();
    let staged_book = f.book.begin_block(2).unwrap().book;
    let verified = verify_signed(&f.signed, &f.candidate.fee_profile.limits()).unwrap();
    let actor = [3; 32];
    let address = &staged_book.accounts[&hex::encode(actor)].address;
    let dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(Storage::open(dir.path().join("db")).unwrap());
    storage
        .db
        .put(crate::runtime::governance_state::STATE_KEY, f.governance.encode().unwrap())
        .unwrap();
    let mut staged = Settlement::new(storage.clone());
    staged.account(address).unwrap().set_balance("udrt", 10_000);
    let mut wrong_nonce = staged.clone();
    wrong_nonce.account(address).unwrap().nonce = 1;
    let before_snapshot = wrong_nonce.writes().unwrap();
    assert!(wrong_nonce
        .governance_financial_snapshot(&staged_book)
        .is_err());
    assert_eq!(wrong_nonce.writes().unwrap(), before_snapshot);
    let mut meter = SharedBlockMeter::new(
        &f.candidate.fee_profile.base,
        RecoveryCeilings {
            max_gas: 3_000,
            max_bytes: 100_000,
            max_signatures: 10,
            mandatory_expiry_gas: 1,
        },
    )
    .unwrap();
    let work = ActionWork {
        validation_reads: Vec::new(),
        action_reads: Vec::new(),
        proposed_writes: Vec::new(),
        end: ActionEnd::Applied,
    };
    let fee = plan_governance_fee_from_settlement(
        &f.signed,
        &verified,
        &f.candidate.fee_profile,
        &staged_book,
        &staged,
        2,
        7,
        &work,
        &mut meter,
    )
    .unwrap();
    let governance = plan_ordered_admission_block(
        &f.governance,
        2,
        &f.candidate.deposit,
        3,
        &BTreeMap::new(),
        None,
        &[assessment.ordered_action()],
    )
    .unwrap();
    let mut empty = Settlement::new(storage);
    assert!(empty
        .apply_governance_fee_plan(&assessment, &fee, &staged_book, 7)
        .is_err());
    assert!(empty.writes().unwrap().is_empty());
    let initial = staged.writes().unwrap();
    assert!(staged
        .apply_governance_fee_plan(&assessment, &fee, &staged_book, 8)
        .is_err());
    assert_eq!(staged.writes().unwrap(), initial);
    let mut corrupt_fee = fee.clone();
    corrupt_fee.fee_checkpoint.withheld_udrt += 1;
    assert!(staged
        .apply_governance_fee_plan(&assessment, &corrupt_fee, &staged_book, 7)
        .is_err());
    assert_eq!(staged.writes().unwrap(), initial);
    let authority = staged
        .apply_governance_fee_plan(&assessment, &fee, &staged_book, 7)
        .unwrap();
    assert_eq!(authority, fee.authority_checkpoint);
    let charged = staged.writes().unwrap();
    assert!(staged
        .apply_governance_fee_plan(&assessment, &fee, &staged_book, 7)
        .is_err());
    assert_eq!(staged.writes().unwrap(), charged);
    staged
        .apply_ordered_governance(&f.governance, &governance, &authority)
        .unwrap();
    let writes = staged.writes().unwrap();
    assert_eq!(staged.account(address).unwrap().nonce, 1);
    assert_eq!(staged.account(address).unwrap().balance_of("udrt"), 10_000 - fee.receipt.charge);
    assert_eq!(staged.ordinary_fee_total().unwrap(), fee.receipt.charge);
    assert!(writes.contains_key(FEE_KEY.as_bytes()));
    assert_eq!(
        writes.get(crate::runtime::governance_state::STATE_KEY.as_bytes()),
        Some(&governance.state_bytes)
    );
}

#[test]
fn registered_proposer_requires_own_effective_bond() {
    let mut f = Fixture::new();
    f.candidate.action_classes.push(ActionClassLimit {
        class: 17,
        max_data_bytes: 3,
        approval_digest: [8; 32],
    });
    let bonded_owner = hex::encode([3; 32]);
    let snapshot = BondSnapshot::from_finalized_registered_accounts(
        &f.lifecycle,
        &f.book,
        1,
        [9; 32],
        f.candidate.ballot.max_voters,
    )
    .unwrap();
    assert_eq!(snapshot.owner_weights, BTreeMap::from([(bonded_owner, 10)]));

    let unbonded_id = [4; 32];
    let mut unbonded_domain = f.signed.body.domain.clone();
    unbonded_domain.account_id = unbonded_id;
    let unbonded_address =
        AccountAddress::from_account_id(AddressNetwork::Development, unbonded_id).encode();
    let unbonded_recovery = RecoveryState::new(
        unbonded_domain.clone(),
        f.book.accounts[&hex::encode([3; 32])]
            .recovery
            .config
            .clone(),
        f.signed.body.key.clone(),
        0,
    )
    .unwrap()
    .advance_height(1)
    .unwrap();
    f.book.accounts.insert(
        hex::encode(unbonded_id),
        RecoveryAccount {
            address: unbonded_address.clone(),
            recovery: unbonded_recovery,
            sponsor_nonce: 0,
        },
    );
    f.nonces.insert(unbonded_address, 0);
    f.book.validate().unwrap();
    f.signed.body.domain = unbonded_domain;
    f.resign();
    assert!(f
        .assess()
        .unwrap_err()
        .to_string()
        .contains("Proposer has no effective registered bond"));
}

#[test]
fn profile_expiry_and_single_action_fail_closed() {
    let mut f = Fixture::new();
    f.signed.body.expiry_height = 2;
    f.resign();
    assert!(f.assess().unwrap_err().to_string().contains("expired"));
    f.signed.body.expiry_height = 4;
    f.signed.body.fee_profile_digest = [0; 32];
    f.resign();
    assert!(f.assess().unwrap_err().to_string().contains("fee profile"));
    f.signed.body.fee_profile_digest =
        ordinary_fees_v3::profile_digest(&f.candidate.fee_profile).unwrap();
    f.signed.body.actions.push(Action::GovernanceVote {
        proposal_id: 1,
        choice: v3::VoteChoice::Yes,
    });
    f.resign();
    assert!(f
        .assess()
        .unwrap_err()
        .to_string()
        .contains("exactly one governance action"));
}

#[test]
fn nonce_mirror_and_parent_binding_are_required() {
    let mut f = Fixture::new();
    f.nonces.clear();
    assert!(f.assess().unwrap_err().to_string().contains("nonce mirror"));
    f.nonces = f
        .book
        .accounts
        .values()
        .map(|a| (a.address.clone(), a.recovery.spending_nonce))
        .collect();
    f.lifecycle.last_height = 2;
    assert!(f
        .assess()
        .unwrap_err()
        .to_string()
        .contains("one committed parent"));
}

#[test]
fn deposit_requires_existing_open_stage_and_positive_amount() {
    let mut f = Fixture::new();
    f.candidate.deposit.deposit_period_blocks = 2;
    f.signed.body.actions = vec![Action::GovernanceDeposit {
        proposal_id: 1,
        amount_udgt: 5,
    }];
    f.resign();
    assert!(f
        .assess()
        .unwrap_err()
        .to_string()
        .contains("proposal is absent"));
    let data = vec![1, 2, 3];
    let stage = DepositStage::new(
        f.candidate.deposit.clone(),
        1,
        17,
        data.clone(),
        v3::governance_action_digest(17, &data).unwrap(),
        1,
    )
    .unwrap();
    let record = ProposalRecord::new(stage, DepositEscrow::new(1, 1).unwrap(), None);
    let initial = GovernanceState::new(
        GOVERNANCE_STATE_VERSION,
        "governance-test".into(),
        [7; 32],
        0,
    )
    .unwrap();
    f.governance = initial
        .plan_commit(1, BTreeMap::from([(1, record)]))
        .unwrap();
    assert_eq!(
        f.assess().unwrap().ordered_action(),
        AdmissionAction::Deposit {
            proposal_id: 1,
            owner: [3; 32],
            amount_udgt: 5
        }
    );
    f.signed.body.actions = vec![Action::GovernanceDeposit {
        proposal_id: 1,
        amount_udgt: 0,
    }];
    f.resign();
    assert!(f
        .assess()
        .unwrap_err()
        .to_string()
        .contains("deposit is zero"));
}

#[test]
fn ordered_vote_uses_the_authenticated_actor_not_a_supplied_voter() {
    let assessed = PreadmissionAssessment {
        transaction_id: [1; 32],
        envelope_hash: [2; 32],
        actor: [3; 32],
        admission_height: 2,
        contract_version: ORDINARY_FEE_CONTRACT_VERSION,
        profile_version: 2,
        profile_digest: [4; 32],
        gas_limit: 1000,
        maximum_fee: 1000,
        nonce_before: 0,
        nonce_after: 1,
        action: AssessedAction::Vote {
            proposal_id: 4,
            choice: BallotChoice::NoWithVeto,
        },
    };
    assert_eq!(
        assessed.ordered_action(),
        AdmissionAction::Vote {
            proposal_id: 4,
            owner: [3; 32],
            choice: BallotChoice::NoWithVeto,
        }
    );
}

#[test]
fn fee_receipt_must_match_the_signed_admission_identity() {
    let mut f = Fixture::new();
    f.candidate.action_classes.push(ActionClassLimit {
        class: 17,
        max_data_bytes: 3,
        approval_digest: [8; 32],
    });
    let assessed = f.assess().unwrap();
    let receipt = V3FeeReceipt {
        version: 2,
        transaction_id: assessed.transaction_id(),
        envelope_hash: assessed.envelope_hash(),
        actor: assessed.actor(),
        block_height: assessed.admission_height(),
        block_index: 0,
        contract_version: ORDINARY_FEE_CONTRACT_VERSION,
        profile_version: f.candidate.fee_profile.version,
        profile_digest: ordinary_fees_v3::profile_digest(&f.candidate.fee_profile).unwrap(),
        outcome: crate::governance_v3_fee_settlement::Outcome::Success,
        failure_code: None,
        gas_limit: 1000,
        gas_used: 10,
        metadata_gas: 1,
        reserved_cap: 1000,
        charge: 10,
        released_cap: 990,
        nonce_before: assessed.nonce_before(),
        nonce_after: assessed.nonce_after(),
    };
    assessed.bind_fee_receipt(&receipt).unwrap();
    let mut other = receipt.clone();
    other.transaction_id = [0; 32];
    assert!(assessed.bind_fee_receipt(&other).is_err());
    let mut other = receipt.clone();
    other.envelope_hash = [0; 32];
    assert!(assessed.bind_fee_receipt(&other).is_err());
    let mut other = receipt.clone();
    other.actor = [4; 32];
    assert!(assessed.bind_fee_receipt(&other).is_err());
    let mut other = receipt.clone();
    other.block_height += 1;
    assert!(assessed.bind_fee_receipt(&other).is_err());
    let mut other = receipt.clone();
    other.profile_digest = [0; 32];
    assert!(assessed.bind_fee_receipt(&other).is_err());
    let mut other = receipt.clone();
    other.reserved_cap -= 1;
    assert!(assessed.bind_fee_receipt(&other).is_err());
    let mut other = receipt;
    other.nonce_after += 1;
    assert!(assessed.bind_fee_receipt(&other).is_err());
}

#[test]
fn staged_admission_uses_ordered_nonce_proposal_id_and_new_deposit_stage() {
    let mut f = Fixture::new();
    f.candidate.action_classes.push(ActionClassLimit {
        class: 17,
        max_data_bytes: 3,
        approval_digest: [8; 32],
    });
    let mut stage_book = f.book.begin_block(2).unwrap().book;
    let mut stage_nonces = f.nonces.clone();
    let data = vec![1, 2, 3];
    let stage = DepositStage::new(
        f.candidate.deposit.clone(),
        1,
        17,
        data.clone(),
        v3::governance_action_digest(17, &data).unwrap(),
        2,
    )
    .unwrap();
    let proposals = BTreeMap::from([(
        1,
        ProposalRecord::new(stage, DepositEscrow::new(1, 1).unwrap(), None),
    )]);
    stage_book
        .accounts
        .get_mut(&hex::encode([3; 32]))
        .unwrap()
        .recovery
        .spending_nonce = 1;
    *stage_nonces.values_mut().next().unwrap() = 1;
    stage_book.validate().unwrap();

    f.signed.body.spending_nonce = 1;
    f.signed.body.actions = vec![Action::GovernanceProposal {
        proposal_id: 2,
        action_class: 17,
        action_data: data.clone(),
        action_digest: v3::governance_action_digest(17, &data).unwrap(),
    }];
    f.resign();
    let parent = AdmissionParent {
        candidate: &f.candidate,
        recovery: &f.book,
        native_nonces: &f.nonces,
        lifecycle: &f.lifecycle,
        governance: &f.governance,
        app_hash: [9; 32],
    };
    let staged = AdmissionStage {
        recovery: &stage_book,
        native_nonces: &stage_nonces,
        proposals: &proposals,
    };
    assert_eq!(
        assess_signed_staged(&f.signed, &parent, &staged)
            .unwrap()
            .action(),
        &AssessedAction::Proposal {
            proposal_id: 2,
            action_class: 17,
            action_data: data,
            action_digest: v3::governance_action_digest(17, &[1, 2, 3]).unwrap(),
        }
    );
    assert!(assess_signed(&f.signed, &parent).is_err());

    f.signed.body.actions = vec![Action::GovernanceDeposit {
        proposal_id: 1,
        amount_udgt: 5,
    }];
    f.resign();
    let parent = AdmissionParent {
        candidate: &f.candidate,
        recovery: &f.book,
        native_nonces: &f.nonces,
        lifecycle: &f.lifecycle,
        governance: &f.governance,
        app_hash: [9; 32],
    };
    assert_eq!(
        assess_signed_staged(&f.signed, &parent, &staged)
            .unwrap()
            .action(),
        &AssessedAction::Deposit {
            proposal_id: 1,
            amount_udgt: 5,
        }
    );
    assert!(assess_signed(&f.signed, &parent).is_err());
}
