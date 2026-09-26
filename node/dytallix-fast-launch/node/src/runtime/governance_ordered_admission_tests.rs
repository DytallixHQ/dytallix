use super::*;
use crate::{
    addr::{AccountAddress, AddressNetwork},
    recovery_fees::RecoveryAccount,
    runtime::{
        governance_ballot::{Rules as BallotRules, VoteChoice},
        governance_escrow::{EscrowStatus, TerminalOutcome},
        reward_runtime::{RewardConfig, RewardState, ValidatorStatus},
        validator_lifecycle::{LifecycleConfig, ValidatorIdentity, PROFILE},
    },
    settlement::Settlement,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use dytallix_protocol_types::{
    ordinary_v3::governance_action_digest,
    recovery::{KeyIdentity, RecoveryConfig, RecoveryDomain, RecoveryState},
    recovery_sponsor::FeeProfile as RecoveryFeeProfile,
};
use fips204::{
    ml_dsa_65,
    traits::{KeyGen, SerDes},
};

fn parent() -> GovernanceState {
    GovernanceState::new(1, "ordered-test".into(), [1; 32], 0).unwrap()
}

fn rules() -> DepositRules {
    DepositRules {
        deposit_period_blocks: 1,
        minimum_deposit_udgt: 10,
        max_action_bytes: 32,
    }
}

fn proposal(id: u64) -> AdmissionAction {
    let data = vec![id as u8];
    AdmissionAction::Proposal {
        proposal_id: id,
        action_class: 17,
        action_digest: governance_action_digest(17, &data).unwrap(),
        action_data: data,
    }
}

fn account() -> BTreeMap<OwnerId, AccountDgt> {
    BTreeMap::from([(
        [3; 32],
        AccountDgt {
            balance_udgt: 100,
            spendable_udgt: 100,
        },
    )])
}

fn finalized_bond_parent() -> (RecoveryBook, LifecycleState, BallotRules) {
    let (public, _) = ml_dsa_65::KG::try_keygen_with_rng(&mut rand_core::OsRng).unwrap();
    let key = KeyIdentity {
        algorithm: "mldsa65".into(),
        public_key: public.into_bytes().to_vec(),
    };
    let actor = [3; 32];
    let owner = hex::encode(actor);
    let domain = RecoveryDomain {
        network: 3,
        chain_id: "ordered-test".into(),
        genesis_digest: [1; 32],
        account_id: actor,
    };
    let recovery = RecoveryState::new(
        domain,
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
    let address = AccountAddress::from_account_id(AddressNetwork::Development, actor).encode();
    let mut book = RecoveryBook::new(
        RecoveryFeeProfile {
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
        },
        vec![RecoveryAccount {
            address,
            recovery,
            sponsor_nonce: 0,
        }],
    )
    .unwrap();
    book.accounts.get_mut(&owner).unwrap().recovery =
        book.accounts[&owner].recovery.advance_height(1).unwrap();
    book.last_height = 1;
    book.validate().unwrap();

    let mut rewards = RewardState::new(
        RewardConfig {
            version: 2,
            activation_height: 1,
            decimals: 6,
            profile: "development".into(),
            chain_id: "ordered-test".into(),
            genesis_digest: "01".repeat(32),
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
        LifecycleConfig {
            version: 1,
            profile: PROFILE.into(),
            chain_id: "ordered-test".into(),
            approved_operators: BTreeMap::from([("validator".into(), owner.clone())]),
            min_self_bond: 1,
            max_active: 1,
            evidence_max_age_blocks: 1,
            evidence_max_age_seconds: 1,
            processing_margin_blocks: 1,
            processing_margin_seconds: 1,
        },
        BTreeMap::from([(
            "validator".into(),
            ValidatorIdentity {
                owner,
                pubkey_base64: B64.encode(&key.public_key),
            },
        )]),
        &rewards,
    )
    .unwrap();
    lifecycle.advance(1, 100, &mut rewards).unwrap();
    let ballot_rules = BallotRules {
        version: 1,
        chain_id: "ordered-test".into(),
        genesis_digest: [1; 32],
        quorum_bps: 5000,
        approval_bps: 5000,
        veto_bps: 5000,
        voting_period_blocks: 2,
        timelock_blocks: 1,
        max_voters: 1,
    };
    (book, lifecycle, ballot_rules)
}

#[test]
fn two_same_block_proposals_use_staged_ids() {
    let plan = plan_ordered_admission_block(
        &parent(),
        1,
        &rules(),
        3,
        &BTreeMap::new(),
        None,
        &[proposal(1), proposal(2)],
    )
    .unwrap();
    assert_eq!(plan.admitted_proposal_ids, vec![1, 2]);
    assert_eq!(plan.next_state.next_proposal_id(), 3);
    assert_eq!(plan.next_state.proposals().len(), 2);
    assert!(plan.account_writes.is_empty());
    assert_eq!(
        GovernanceState::decode(&plan.state_bytes).unwrap(),
        plan.next_state
    );
}

#[test]
fn proposal_then_deposit_in_same_block_conserves_dgt() {
    let plan = plan_ordered_admission_block(
        &parent(),
        1,
        &rules(),
        3,
        &account(),
        None,
        &[
            proposal(1),
            AdmissionAction::Deposit {
                proposal_id: 1,
                owner: [3; 32],
                amount_udgt: 15,
            },
        ],
    )
    .unwrap();
    assert_eq!(plan.next_state.held_total_udgt().unwrap(), 15);
    assert_eq!(plan.account_writes.len(), 1);
    assert_eq!(plan.account_writes[0].balance_after_udgt, 85);
    assert_eq!(plan.account_writes[0].spendable_after_udgt, 85);
    assert_eq!(plan.next_state.next_proposal_id(), 2);
}

#[test]
fn ordered_plan_stages_dgt_and_governance_record_together() {
    let parent = parent();
    let plan = plan_ordered_admission_block(
        &parent,
        1,
        &rules(),
        3,
        &account(),
        None,
        &[
            proposal(1),
            AdmissionAction::Deposit {
                proposal_id: 1,
                owner: [3; 32],
                amount_udgt: 15,
            },
        ],
    )
    .unwrap();
    let (book, _, _) = finalized_bond_parent();
    let address = book.accounts[&hex::encode([3; 32])].address.clone();
    let dir = tempfile::tempdir().unwrap();
    let storage =
        std::sync::Arc::new(crate::storage::state::Storage::open(dir.path().join("db")).unwrap());
    storage
        .db
        .put(
            crate::runtime::governance_state::STATE_KEY,
            parent.encode().unwrap(),
        )
        .unwrap();
    let mut settlement = Settlement::new(storage);
    settlement
        .account(&address)
        .unwrap()
        .set_balance("udgt", 100);
    settlement.account(&address).unwrap().nonce = 7;
    let initial_writes = settlement.writes().unwrap();
    let mut wrong_balance = settlement.clone();
    wrong_balance
        .account(&address)
        .unwrap()
        .set_balance("udgt", 99);
    let wrong_writes = wrong_balance.writes().unwrap();
    assert!(wrong_balance
        .apply_ordered_governance(&parent, &plan, &book)
        .is_err());
    assert_eq!(wrong_balance.writes().unwrap(), wrong_writes);
    let mut mismatched = plan.clone();
    mismatched.account_writes[0].balance_before_udgt = 99;
    assert!(settlement
        .apply_ordered_governance(&parent, &mismatched, &book)
        .is_err());
    assert_eq!(settlement.writes().unwrap(), initial_writes);
    let mut mismatched = plan.clone();
    mismatched.account_writes[0].balance_after_udgt = 84;
    assert!(settlement
        .apply_ordered_governance(&parent, &mismatched, &book)
        .is_err());
    assert_eq!(settlement.writes().unwrap(), initial_writes);
    settlement
        .apply_ordered_governance(&parent, &plan, &book)
        .unwrap();
    assert_eq!(settlement.account(&address).unwrap().balance_of("udgt"), 85);
    assert_eq!(settlement.account(&address).unwrap().nonce, 7);
    assert_eq!(settlement.governance.as_ref(), Some(&plan.next_state));
    let writes = settlement.writes().unwrap();
    assert_eq!(
        writes[crate::runtime::governance_state::STATE_KEY.as_bytes()],
        plan.state_bytes
    );
    let account_key = format!("acct:balances:{address}");
    let balances: BTreeMap<String, u128> =
        bincode::deserialize(&writes[account_key.as_bytes()]).unwrap();
    assert_eq!(balances["udgt"], 85);
    assert!(settlement
        .apply_ordered_governance(&parent, &plan, &book)
        .is_err());
}

#[test]
fn wrong_order_duplicate_id_and_overspend_reject() {
    let deposit = AdmissionAction::Deposit {
        proposal_id: 1,
        owner: [3; 32],
        amount_udgt: 15,
    };
    assert!(plan_ordered_admission_block(
        &parent(),
        1,
        &rules(),
        3,
        &account(),
        None,
        &[deposit.clone(), proposal(1)],
    )
    .is_err());
    assert!(plan_ordered_admission_block(
        &parent(),
        1,
        &rules(),
        3,
        &account(),
        None,
        &[proposal(1), proposal(1)],
    )
    .is_err());
    assert!(plan_ordered_admission_block(
        &parent(),
        1,
        &rules(),
        3,
        &account(),
        None,
        &[
            proposal(1),
            deposit,
            AdmissionAction::Deposit {
                proposal_id: 1,
                owner: [3; 32],
                amount_udgt: 90,
            },
        ],
    )
    .is_err());
}

#[test]
fn below_minimum_closes_and_refunds_before_transactions() {
    let admitted = plan_ordered_admission_block(
        &parent(),
        1,
        &rules(),
        3,
        &account(),
        None,
        &[
            proposal(1),
            AdmissionAction::Deposit {
                proposal_id: 1,
                owner: [3; 32],
                amount_udgt: 5,
            },
        ],
    )
    .unwrap();
    let after_deposit = BTreeMap::from([(
        [3; 32],
        AccountDgt {
            balance_udgt: 95,
            spendable_udgt: 95,
        },
    )]);
    let closed = plan_ordered_admission_block(
        &admitted.next_state,
        2,
        &rules(),
        3,
        &after_deposit,
        None,
        &[],
    )
    .unwrap();
    assert_eq!(closed.rejected_proposal_ids, vec![1]);
    assert!(closed.started_voting_proposal_ids.is_empty());
    assert_eq!(closed.account_writes[0].balance_after_udgt, 100);
    assert_eq!(closed.account_writes[0].spendable_after_udgt, 100);
    assert_eq!(closed.next_state.held_total_udgt().unwrap(), 0);
    let record = &closed.next_state.proposals()[&1];
    assert_eq!(record.stage().status(), DepositStageStatus::Rejected);
    assert_eq!(
        record.escrow().status(),
        EscrowStatus::Refunded(TerminalOutcome::Rejected)
    );
    assert!(plan_ordered_admission_block(
        &admitted.next_state,
        2,
        &rules(),
        3,
        &after_deposit,
        None,
        &[AdmissionAction::Deposit {
            proposal_id: 1,
            owner: [3; 32],
            amount_udgt: 1,
        }],
    )
    .is_err());
}

#[test]
fn passed_ballot_keeps_escrow_and_due_execution_fails_closed() {
    let admitted = plan_ordered_admission_block(
        &parent(),
        1,
        &rules(),
        3,
        &account(),
        None,
        &[
            proposal(1),
            AdmissionAction::Deposit {
                proposal_id: 1,
                owner: [3; 32],
                amount_udgt: 15,
            },
        ],
    )
    .unwrap();
    let (book, lifecycle, ballot_rules) = finalized_bond_parent();
    let context = ClosureParent {
        lifecycle: &lifecycle,
        recovery: &book,
        app_hash: [9; 32],
        ballot_rules: &ballot_rules,
    };
    let after_deposit = BTreeMap::from([(
        [3; 32],
        AccountDgt {
            balance_udgt: 85,
            spendable_udgt: 85,
        },
    )]);
    let voting = plan_ordered_admission_block(
        &admitted.next_state,
        2,
        &rules(),
        3,
        &after_deposit,
        Some(&context),
        &[],
    )
    .unwrap();
    let vote = AdmissionAction::Vote {
        proposal_id: 1,
        owner: [3; 32],
        choice: VoteChoice::Yes,
    };
    let voted = plan_ordered_admission_block(
        &voting.next_state,
        3,
        &rules(),
        3,
        &after_deposit,
        None,
        &[vote.clone()],
    )
    .unwrap();
    assert_eq!(
        voted.next_state.proposals()[&1].ballot().unwrap().votes()[&hex::encode([3; 32])],
        VoteChoice::Yes
    );
    assert!(voted.account_writes.is_empty());
    assert_eq!(voted.next_state.held_total_udgt().unwrap(), 15);
    assert!(plan_ordered_admission_block(
        &voting.next_state,
        3,
        &rules(),
        3,
        &after_deposit,
        None,
        &[vote.clone(), vote],
    )
    .is_err());
    assert!(plan_ordered_admission_block(
        &voting.next_state,
        3,
        &rules(),
        3,
        &after_deposit,
        None,
        &[AdmissionAction::Vote {
            proposal_id: 1,
            owner: [4; 32],
            choice: VoteChoice::Yes,
        }],
    )
    .is_err());
    let block4 =
        plan_ordered_admission_block(&voted.next_state, 4, &rules(), 3, &after_deposit, None, &[])
            .unwrap();
    let passed = plan_ordered_admission_block(
        &block4.next_state,
        5,
        &rules(),
        3,
        &after_deposit,
        None,
        &[],
    )
    .unwrap();
    assert_eq!(passed.passed_ballot_proposal_ids, vec![1]);
    assert!(passed.account_writes.is_empty());
    assert_eq!(passed.next_state.held_total_udgt().unwrap(), 15);
    assert!(plan_ordered_admission_block(
        &passed.next_state,
        6,
        &rules(),
        3,
        &after_deposit,
        None,
        &[],
    )
    .is_err());
}

#[test]
fn funded_close_starts_ballot_from_registered_finalized_parent() {
    let admitted = plan_ordered_admission_block(
        &parent(),
        1,
        &rules(),
        3,
        &account(),
        None,
        &[
            proposal(1),
            AdmissionAction::Deposit {
                proposal_id: 1,
                owner: [3; 32],
                amount_udgt: 15,
            },
        ],
    )
    .unwrap();
    let (book, lifecycle, ballot_rules) = finalized_bond_parent();
    let context = ClosureParent {
        lifecycle: &lifecycle,
        recovery: &book,
        app_hash: [9; 32],
        ballot_rules: &ballot_rules,
    };
    let after_deposit = BTreeMap::from([(
        [3; 32],
        AccountDgt {
            balance_udgt: 85,
            spendable_udgt: 85,
        },
    )]);
    assert!(plan_ordered_admission_block(
        &admitted.next_state,
        2,
        &rules(),
        3,
        &after_deposit,
        None,
        &[],
    )
    .is_err());
    let closed = plan_ordered_admission_block(
        &admitted.next_state,
        2,
        &rules(),
        3,
        &after_deposit,
        Some(&context),
        &[],
    )
    .unwrap();
    assert_eq!(closed.started_voting_proposal_ids, vec![1]);
    assert!(closed.rejected_proposal_ids.is_empty());
    assert!(closed.account_writes.is_empty());
    assert_eq!(closed.next_state.held_total_udgt().unwrap(), 15);
    let ballot = closed.next_state.proposals()[&1].ballot().unwrap();
    assert_eq!(ballot.snapshot().finalized_height, 1);
    assert_eq!(ballot.snapshot().source_app_hash, [9; 32]);
    assert_eq!(ballot.snapshot().owner_weights[&hex::encode([3; 32])], 10);
    assert!(plan_ordered_admission_block(
        &admitted.next_state,
        2,
        &rules(),
        3,
        &after_deposit,
        Some(&context),
        &[proposal(2)],
    )
    .unwrap_err()
    .to_string()
    .contains("automatic and user-action order needs approval"));
    let bad_context = ClosureParent {
        app_hash: [0; 32],
        ..context
    };
    assert!(plan_ordered_admission_block(
        &admitted.next_state,
        2,
        &rules(),
        3,
        &after_deposit,
        Some(&bad_context),
        &[],
    )
    .is_err());

    let block3 = plan_ordered_admission_block(
        &closed.next_state,
        3,
        &rules(),
        3,
        &after_deposit,
        None,
        &[],
    )
    .unwrap();
    let block4 = plan_ordered_admission_block(
        &block3.next_state,
        4,
        &rules(),
        3,
        &after_deposit,
        None,
        &[],
    )
    .unwrap();
    let ballot_closed = plan_ordered_admission_block(
        &block4.next_state,
        5,
        &rules(),
        3,
        &after_deposit,
        None,
        &[],
    )
    .unwrap();
    assert_eq!(ballot_closed.rejected_ballot_proposal_ids, vec![1]);
    assert_eq!(ballot_closed.account_writes[0].balance_after_udgt, 100);
    assert_eq!(ballot_closed.next_state.held_total_udgt().unwrap(), 0);
    assert_eq!(
        ballot_closed.next_state.proposals()[&1].escrow().status(),
        EscrowStatus::Refunded(TerminalOutcome::Rejected)
    );
    let refunded = BTreeMap::from([(
        [3; 32],
        AccountDgt {
            balance_udgt: 100,
            spendable_udgt: 100,
        },
    )]);
    let later = plan_ordered_admission_block(
        &ballot_closed.next_state,
        6,
        &rules(),
        3,
        &refunded,
        None,
        &[],
    )
    .unwrap();
    assert!(later.account_writes.is_empty());
}
