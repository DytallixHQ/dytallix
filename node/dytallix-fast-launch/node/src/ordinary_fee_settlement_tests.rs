use super::*;
use crate::{
    crypto::{ActivePQC, PQC},
    ordinary_authority::DiscretionaryGrant,
    ordinary_meter::RecoveryCeilings,
    recovery_fees::RecoveryAccount,
};
use dytallix_protocol_types::{
    address::{AccountAddress, AddressNetwork},
    ordinary::{Limits, OrdinaryTransaction, SignedOrdinary},
    recovery::{KeyIdentity, RecoveryConfig, RecoveryDomain, RecoveryState},
    recovery_sponsor::FeeProfile as RecoveryProfile,
};
use std::collections::BTreeSet;
use std::sync::OnceLock;
fn keys() -> &'static (Vec<u8>, Vec<u8>) {
    static KEYS: OnceLock<(Vec<u8>, Vec<u8>)> = OnceLock::new();
    KEYS.get_or_init(ActivePQC::keypair)
}
fn profile() -> FeeProfile {
    FeeProfile {
        ordinary_fee_contract_version: 1,
        version: 1,
        activation_height: 1,
        denomination: ordinary::Denomination::Udrt,
        gas_price: 2,
        minimum_gas: 1,
        max_transaction_gas: 1000,
        max_block_transaction_gas: 10_000,
        max_block_transaction_bytes: 1_000_000,
        max_block_signature_checks: 100,
        max_fee_cap: 2000,
        limits: Limits {
            max_wire_bytes: 262_144,
            max_actions: 16,
            max_identifier_bytes: 128,
            max_data_bytes: 1024,
            max_memo_bytes: 1024,
            max_consensus_key_bytes: 4096,
            max_proof_bytes: 8192,
            max_expiry_lifetime: 100,
            allowed_algorithms: BTreeSet::from(["mldsa65".into()]),
        },
        transaction_overhead: 2,
        receipt_metadata_cost: 5,
        wire_byte_cost: 0,
        read_byte_cost: 0,
        write_byte_cost: 0,
        action_costs: [10; 12],
        signature_costs: BTreeMap::from([("mldsa65".into(), 3)]),
        validator_proof_profile_digest: [4; 32],
        validator_proof_costs: BTreeMap::from([("mldsa65".into(), 4)]),
    }
}
fn shared(p: &FeeProfile) -> SharedBlockMeter {
    SharedBlockMeter::new(
        p,
        RecoveryCeilings {
            max_gas: 10_000,
            max_bytes: 1_000_000,
            max_signatures: 100,
            mandatory_expiry_gas: 100,
        },
    )
    .unwrap()
}
fn fixture() -> (RecoveryBook, FinancialState) {
    let rp = RecoveryProfile {
        version: 1,
        activation_height: 1,
        denomination: "udrt".into(),
        gas_price: 1,
        minimum_gas: 1,
        max_transaction_gas: 1000,
        max_block_gas: 10_000,
        max_block_recovery_bytes: 1_000_000,
        max_block_recovery_signatures: 100,
        max_fee_cap: 2000,
        max_pending_accounts: 2,
        max_due_expiry_events_per_height: 2,
        mandatory_expiry_gas_budget: 100,
        expiry_event_gas_cost: 1,
        action_costs: [1; 9],
        wire_byte_cost: 0,
        read_byte_cost: 0,
        write_byte_cost: 0,
        signature_costs: BTreeMap::from([("mldsa65".into(), 1)]),
    };
    let accounts = (1..=2)
        .map(|id| RecoveryAccount {
            address: AccountAddress::from_account_id(AddressNetwork::Development, [id; 32])
                .encode(),
            recovery: RecoveryState::new(
                RecoveryDomain {
                    network: 3,
                    chain_id: "ordinary-fee-model".into(),
                    genesis_digest: [7; 32],
                    account_id: [id; 32],
                },
                RecoveryConfig {
                    timing_version: 1,
                    recovery_delay: 2,
                    finalization_window: 3,
                    policy_delay: 2,
                    policy_window: 3,
                    submission_lifetime: 100,
                    algorithms: BTreeMap::from([("mldsa65".into(), 1952)]),
                },
                KeyIdentity {
                    algorithm: "mldsa65".into(),
                    public_key: keys().1.clone(),
                },
                0,
            )
            .unwrap(),
            sponsor_nonce: 0,
        })
        .collect();
    let mut book = RecoveryBook::new(rp, accounts).unwrap();
    for a in book.accounts.values_mut() {
        a.recovery = a.recovery.advance_height(1).unwrap();
    }
    book.last_height = 1;
    let balances = BTreeMap::from([
        (asset([1; 32], Denomination::Udgt), 100),
        (asset([1; 32], Denomination::Udrt), 1000),
        (asset([2; 32], Denomination::Udgt), 50),
        (asset([2; 32], Denomination::Udrt), 50),
    ]);
    let state = FinancialState {
        eligible: balances.clone(),
        balances,
        native_nonces: book
            .accounts
            .values()
            .map(|a| (a.address.clone(), 0))
            .collect(),
        withheld_udrt: 17,
    };
    (book, state)
}
fn signed(
    book: &RecoveryBook,
    p: &FeeProfile,
    actions: Vec<Action>,
    limit: u64,
) -> (SignedOrdinary, VerifiedOrdinary) {
    let a = &book.accounts[&hex::encode([1; 32])].recovery;
    let body = OrdinaryTransaction {
        domain: a.domain.clone(),
        authorization_generation: a.active_generation,
        spending_nonce: a.spending_nonce,
        key: a.active_key.clone(),
        expiry_height: 50,
        ordinary_fee_contract_version: p.ordinary_fee_contract_version,
        fee_profile_version: p.version,
        fee_profile_digest: ordinary_fees::profile_digest(p).unwrap(),
        fee_denomination: ordinary::Denomination::Udrt,
        maximum_fee: 200,
        gas_limit: limit,
        memo: String::new(),
        actions,
    };
    let signed = SignedOrdinary {
        signature: ActivePQC::sign(
            &keys().0,
            &ordinary::signing_bytes(&body, &p.limits).unwrap(),
        ),
        body,
    };
    let verified = dytallix_runtime_crypto::ordinary::verify_signed(&signed, &p.limits).unwrap();
    (signed, verified)
}
fn trace(count: usize) -> AccountingTrace {
    AccountingTrace {
        validation_reads: vec![],
        actions: vec![
            ActionWork {
                reads: vec![],
                writes: vec![],
                end: AccountingEnd::Applied
            };
            count
        ],
    }
}
fn prepared(result: PlanningResult) -> FeePlan {
    match result {
        PlanningResult::Prepared(plan) => plan,
        _ => panic!("expected new plan"),
    }
}
fn data() -> Action {
    Action::Data {
        data: "payload".into(),
    }
}
#[test]
fn measured_success_releases_cap_and_preserves_supply_and_independent_counters() {
    let (book, state) = fixture();
    let before = (book.clone(), state.clone());
    let p = profile();
    let (_, v) = signed(&book, &p, vec![data()], 100);
    let request = reservation_request(&v, &p, &state).unwrap();
    let mut meter = shared(&p);
    let plan = prepared(
        plan_fee_accounting(
            &v,
            &p,
            &book,
            &Grants::new(),
            &state,
            &FeeHistory::default(),
            &request,
            1,
            0,
            &trace(1),
            &mut meter,
        )
        .unwrap(),
    );
    assert_eq!(plan.receipt.gas_used, 20);
    assert_eq!(plan.receipt.charge, 40);
    assert_eq!(plan.receipt.released_cap, 160);
    assert_eq!(plan.receipt.metadata_gas, 5);
    assert_eq!(plan.fee_checkpoint.withheld_udrt, 57);
    assert_eq!(
        balance(&plan.fee_checkpoint, [1; 32], Denomination::Udrt),
        960
    );
    assert_eq!(plan.action_disposition(), ActionDisposition::KeepProposed);
    assert_eq!(
        plan.authority_checkpoint.accounts[&hex::encode([1; 32])]
            .recovery
            .spending_nonce,
        1
    );
    assert_eq!(
        plan.authority_checkpoint.accounts[&hex::encode([1; 32])].sponsor_nonce,
        0
    );
    assert_eq!(
        plan.authority_checkpoint.accounts[&hex::encode([1; 32])]
            .recovery
            .recovery_sequence,
        0
    );
    assert!(!plan.is_executable());
    assert_eq!((book, state), before);
}
#[test]
fn immature_claim_is_measured_failure_and_discards_earlier_grant_preparation() {
    let (book, state) = fixture();
    let p = profile();
    let grants = Grants::from([(
        hex::encode([2; 32]),
        DiscretionaryGrant {
            version: 1,
            owner: [2; 32],
            beneficiary: [1; 32],
            owner_generation: 0,
            period_blocks: 5,
            last_active_height: 1,
        },
    )]);
    let actions = vec![
        Action::DmsRegister {
            beneficiary: [2; 32],
            period_blocks: 5,
        },
        Action::DmsClaim {
            owner: [2; 32],
            expected_grant_generation: 0,
        },
    ];
    let (_, v) = signed(&book, &p, actions, 100);
    let request = reservation_request(&v, &p, &state).unwrap();
    let plan = prepared(
        plan_fee_accounting(
            &v,
            &p,
            &book,
            &grants,
            &state,
            &FeeHistory::default(),
            &request,
            1,
            0,
            &trace(2),
            &mut shared(&p),
        )
        .unwrap(),
    );
    assert_eq!(plan.receipt.outcome, Outcome::ApplicationFailure);
    assert_eq!(
        plan.receipt.rule_code.as_deref(),
        Some("DMS_INACTIVITY_DELAY")
    );
    assert_eq!(plan.receipt.failing_action, Some(1));
    assert_eq!(plan.receipt.gas_used, 30);
    assert_eq!(plan.receipt.charge, 60);
    assert_eq!(plan.disposition, ActionDisposition::DiscardAll);
    assert_eq!(plan.grant_checkpoint, grants);
    assert_eq!(
        balance(&plan.fee_checkpoint, [2; 32], Denomination::Udrt),
        50
    );
    assert_eq!(
        balance(&plan.fee_checkpoint, [2; 32], Denomination::Udgt),
        50
    );
}
#[test]
fn minimum_charge_is_separate_from_measured_work_on_success_and_application_failure() {
    for failure in [false, true] {
        let (book, state) = fixture();
        let mut p = profile();
        p.minimum_gas = 50;
        let grants = Grants::from([(
            hex::encode([2; 32]),
            DiscretionaryGrant {
                version: 1,
                owner: [2; 32],
                beneficiary: [1; 32],
                owner_generation: 0,
                period_blocks: 5,
                last_active_height: 1,
            },
        )]);
        let action = if failure {
            Action::DmsClaim {
                owner: [2; 32],
                expected_grant_generation: 0,
            }
        } else {
            data()
        };
        let (_, v) = signed(&book, &p, vec![action], 100);
        let request = reservation_request(&v, &p, &state).unwrap();
        let mut block = shared(&p);
        let plan = prepared(
            plan_fee_accounting(
                &v,
                &p,
                &book,
                &grants,
                &state,
                &FeeHistory::default(),
                &request,
                1,
                0,
                &trace(1),
                &mut block,
            )
            .unwrap(),
        );
        assert_eq!(plan.receipt.gas_used, 20);
        assert_eq!(block.usage().gas, 20);
        assert_eq!(plan.receipt.charge, 100);
        assert_eq!(plan.receipt.released_cap, 100);
        plan.history.validate().unwrap();
    }
}
#[test]
fn accepted_out_of_gas_charges_limit_once_and_preaccept_metadata_shortfall_rejects() {
    let (book, state) = fixture();
    let p = profile();
    let (_, v) = signed(&book, &p, vec![data()], 15);
    let request = reservation_request(&v, &p, &state).unwrap();
    let plan = prepared(
        plan_fee_accounting(
            &v,
            &p,
            &book,
            &Grants::new(),
            &state,
            &FeeHistory::default(),
            &request,
            1,
            0,
            &trace(1),
            &mut shared(&p),
        )
        .unwrap(),
    );
    assert_eq!(plan.receipt.outcome, Outcome::OutOfGas);
    assert_eq!(plan.receipt.gas_used, 15);
    assert_eq!(plan.receipt.charge, 30);
    assert_eq!(plan.receipt.released_cap, 170);
    assert_eq!(plan.receipt.metadata_gas, 5);
    assert_eq!(plan.disposition, ActionDisposition::DiscardAll);
    let (_, short) = signed(&book, &p, vec![data()], 7);
    let req = reservation_request(&short, &p, &state).unwrap();
    let mut block = shared(&p);
    assert!(matches!(
        plan_fee_accounting(
            &short,
            &p,
            &book,
            &Grants::new(),
            &state,
            &FeeHistory::default(),
            &req,
            1,
            0,
            &trace(1),
            &mut block
        ),
        Err(PlanError::Rejected(_))
    ));
    assert_eq!(
        book.accounts[&hex::encode([1; 32])].recovery.spending_nonce,
        0
    );
}
#[test]
fn exact_retained_retry_returns_old_receipt_and_resigning_cannot_charge_twice() {
    let (book, state) = fixture();
    let p = profile();
    let (signed, v) = signed(&book, &p, vec![data()], 100);
    let request = reservation_request(&v, &p, &state).unwrap();
    let plan = prepared(
        plan_fee_accounting(
            &v,
            &p,
            &book,
            &Grants::new(),
            &state,
            &FeeHistory::default(),
            &request,
            1,
            0,
            &trace(1),
            &mut shared(&p),
        )
        .unwrap(),
    );
    let result = plan_fee_accounting(
        &v,
        &p,
        &plan.authority_checkpoint,
        &plan.grant_checkpoint,
        &plan.fee_checkpoint,
        &plan.history,
        &request,
        1,
        0,
        &trace(1),
        &mut shared(&p),
    )
    .unwrap();
    assert_eq!(
        result,
        PlanningResult::ExactRetainedRetry(plan.receipt.clone())
    );
    let resigned = SignedOrdinary {
        signature: ActivePQC::sign(
            &keys().0,
            &ordinary::signing_bytes(&signed.body, &p.limits).unwrap(),
        ),
        body: signed.body,
    };
    let resigned = dytallix_runtime_crypto::ordinary::verify_signed(&resigned, &p.limits).unwrap();
    assert_eq!(v.transaction_id(), resigned.transaction_id());
    assert!(matches!(
        plan_fee_accounting(
            &resigned,
            &p,
            &plan.authority_checkpoint,
            &plan.grant_checkpoint,
            &plan.fee_checkpoint,
            &plan.history,
            &request,
            1,
            1,
            &trace(1),
            &mut shared(&p)
        ),
        Err(PlanError::Rejected(_))
    ));
}
#[test]
fn reservation_binding_rejects_every_changed_identity_field() {
    let (book, state) = fixture();
    let p = profile();
    let (_, v) = signed(&book, &p, vec![data()], 100);
    let request = reservation_request(&v, &p, &state).unwrap();
    for field in 0..8 {
        let mut r = request.clone();
        match field {
            0 => r.context_digest = [9; 32],
            1 => r.id = ReservationId::Ordinary([9; 32]),
            2 => r.payer = [2; 32],
            3 => r.nonce = 1,
            4 => r.fee_cap_udrt = 199,
            5 => r.wire_bytes += 1,
            6 => r.signature_work += 1,
            _ => r.action_debits.push(ActionDebit {
                asset: asset([1; 32], Denomination::Udgt),
                amount: 1,
                kind: DebitKind::Outflow,
            }),
        };
        assert!(matches!(
            plan_fee_accounting(
                &v,
                &p,
                &book,
                &Grants::new(),
                &state,
                &FeeHistory::default(),
                &r,
                1,
                0,
                &trace(1),
                &mut shared(&p)
            ),
            Err(PlanError::Rejected(_))
        ));
    }
}
#[test]
fn mismatched_mirror_is_internal_stale_authority_rejects_and_profile_drift_rejects() {
    let (mut book, mut state) = fixture();
    let p = profile();
    let (_, v) = signed(&book, &p, vec![data()], 100);
    let request = reservation_request(&v, &p, &state).unwrap();
    let address = book.accounts[&hex::encode([1; 32])].address.clone();
    state.native_nonces.insert(address.clone(), 1);
    assert!(matches!(
        plan_fee_accounting(
            &v,
            &p,
            &book,
            &Grants::new(),
            &state,
            &FeeHistory::default(),
            &request,
            1,
            0,
            &trace(1),
            &mut shared(&p)
        ),
        Err(PlanError::Internal(_))
    ));
    book.accounts
        .get_mut(&hex::encode([1; 32]))
        .unwrap()
        .recovery
        .spending_nonce = 1;
    assert!(matches!(
        plan_fee_accounting(
            &v,
            &p,
            &book,
            &Grants::new(),
            &state,
            &FeeHistory::default(),
            &request,
            1,
            0,
            &trace(1),
            &mut shared(&p)
        ),
        Err(PlanError::Rejected(_))
    ));
    let (book, state) = fixture();
    let mut changed = p.clone();
    changed.version = 2;
    assert!(matches!(
        plan_fee_accounting(
            &v,
            &changed,
            &book,
            &Grants::new(),
            &state,
            &FeeHistory::default(),
            &request,
            1,
            0,
            &trace(1),
            &mut shared(&changed)
        ),
        Err(PlanError::Rejected(_))
    ));
}
#[test]
fn internal_fault_overflow_unqualified_capacity_and_registry_inputs_never_create_a_plan() {
    let (book, mut state) = fixture();
    let p = profile();
    let (_, v) = signed(&book, &p, vec![data()], 100);
    let request = reservation_request(&v, &p, &state).unwrap();
    let before = state.clone();
    let mut fault = trace(1);
    fault.actions[0].end = AccountingEnd::InternalFault;
    assert!(matches!(
        plan_fee_accounting(
            &v,
            &p,
            &book,
            &Grants::new(),
            &state,
            &FeeHistory::default(),
            &request,
            1,
            0,
            &fault,
            &mut shared(&p)
        ),
        Err(PlanError::Internal(_))
    ));
    assert_eq!(state, before);
    state.withheld_udrt = u128::MAX;
    assert!(matches!(
        plan_fee_accounting(
            &v,
            &p,
            &book,
            &Grants::new(),
            &state,
            &FeeHistory::default(),
            &request,
            1,
            0,
            &trace(1),
            &mut shared(&p)
        ),
        Err(PlanError::Internal(_))
    ));
    let (book, state) = fixture();
    let (_, v) = signed(
        &book,
        &p,
        vec![Action::Send {
            recipient: [2; 32],
            denomination: ordinary::Denomination::Udgt,
            amount: 1,
        }],
        100,
    );
    let r = reservation_request(&v, &p, &state).unwrap();
    let mut capacity = trace(1);
    capacity.actions[0].end = AccountingEnd::ApplicationFailure(ApplicationRule::RecipientCapacity);
    assert!(matches!(
        plan_fee_accounting(
            &v,
            &p,
            &book,
            &Grants::new(),
            &state,
            &FeeHistory::default(),
            &r,
            1,
            0,
            &capacity,
            &mut shared(&p)
        ),
        Err(PlanError::IntegrationUnavailable(_))
    ));
    let (_, v) = signed(&book, &p, vec![Action::RewardClaim], 100);
    let r = reservation_request(&v, &p, &state).unwrap();
    assert!(matches!(
        plan_fee_accounting(
            &v,
            &p,
            &book,
            &Grants::new(),
            &state,
            &FeeHistory::default(),
            &r,
            1,
            0,
            &trace(1),
            &mut shared(&p)
        ),
        Err(PlanError::IntegrationUnavailable(_))
    ));
}
#[test]
fn combined_block_overflow_has_no_partial_plan_and_funding_never_uses_incoming_credit() {
    let (book, state) = fixture();
    let mut p = profile();
    p.max_transaction_gas = 100;
    p.max_block_transaction_gas = 100;
    let (_, v) = signed(&book, &p, vec![data()], 100);
    let r = reservation_request(&v, &p, &state).unwrap();
    let mut block = shared(&p);
    block.charge_recovery(90, 1, 1).unwrap();
    assert_eq!(
        plan_fee_accounting(
            &v,
            &p,
            &book,
            &Grants::new(),
            &state,
            &FeeHistory::default(),
            &r,
            1,
            0,
            &trace(1),
            &mut block
        ),
        Err(PlanError::BlockCapacity)
    );
    let p = profile();
    let (_, v) = signed(
        &book,
        &p,
        vec![Action::Send {
            recipient: [2; 32],
            denomination: ordinary::Denomination::Udgt,
            amount: 101,
        }],
        100,
    );
    let r = reservation_request(&v, &p, &state).unwrap();
    assert!(matches!(
        plan_fee_accounting(
            &v,
            &p,
            &book,
            &Grants::new(),
            &state,
            &FeeHistory::default(),
            &r,
            1,
            0,
            &trace(1),
            &mut shared(&p)
        ),
        Err(PlanError::Rejected(_))
    ));
}

#[test]
fn accepted_failure_retry_requires_next_nonce_body_and_preserves_earlier_checkpoint() {
    let (book, state) = fixture();
    let p = profile();
    let grants = Grants::from([(
        hex::encode([2; 32]),
        DiscretionaryGrant {
            version: 1,
            owner: [2; 32],
            beneficiary: [1; 32],
            owner_generation: 0,
            period_blocks: 5,
            last_active_height: 1,
        },
    )]);
    let action = Action::DmsClaim {
        owner: [2; 32],
        expected_grant_generation: 0,
    };
    let (_, first) = signed(&book, &p, vec![action.clone()], 100);
    let request = reservation_request(&first, &p, &state).unwrap();
    let mut meter = shared(&p);
    let one = prepared(
        plan_fee_accounting(
            &first,
            &p,
            &book,
            &grants,
            &state,
            &FeeHistory::default(),
            &request,
            1,
            0,
            &trace(1),
            &mut meter,
        )
        .unwrap(),
    );
    let (_, next) = signed(one.authority_checkpoint(), &p, vec![action], 100);
    assert_ne!(first.transaction_id(), next.transaction_id());
    let request = reservation_request(&next, &p, one.fee_checkpoint()).unwrap();
    let two = prepared(
        plan_fee_accounting(
            &next,
            &p,
            one.authority_checkpoint(),
            one.grant_checkpoint(),
            one.fee_checkpoint(),
            one.history(),
            &request,
            1,
            1,
            &trace(1),
            &mut meter,
        )
        .unwrap(),
    );
    assert_eq!(two.receipt.nonce_before, 1);
    assert_eq!(two.receipt.nonce_after, 2);
    assert_eq!(two.receipt.outcome, Outcome::ApplicationFailure);
    assert_eq!(two.history.receipts.len(), 2);
    assert_eq!(one.history.receipts.len(), 1);
    assert_eq!(
        two.fee_checkpoint.withheld_udrt,
        17 + one.receipt.charge + two.receipt.charge
    );
    assert_eq!(two.grant_checkpoint, grants);
    assert_eq!(
        book.accounts[&hex::encode([1; 32])].recovery.spending_nonce,
        0
    );
}
