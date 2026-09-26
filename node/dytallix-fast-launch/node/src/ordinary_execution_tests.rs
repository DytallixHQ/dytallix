//! Independent runtime checks use real account signatures and staged local state.
use super::*;
use crate::{
    crypto::{ActivePQC, PQC},
    ordinary_meter::RecoveryCeilings,
    recovery_fees::RecoveryAccount,
    storage::state::Storage,
};
use dytallix_protocol_types::{
    address::{AccountAddress, AddressNetwork},
    ordinary::{Limits, OrdinaryTransaction},
    recovery::{KeyIdentity, RecoveryConfig, RecoveryDomain, RecoveryState},
    recovery_sponsor::FeeProfile as RecoveryProfile,
};
use std::sync::{Arc, OnceLock};
const ACTOR: [u8; 32] = [1; 32];
const OWNER: [u8; 32] = [2; 32];
const OTHER: [u8; 32] = [3; 32];
fn keys() -> &'static (Vec<u8>, Vec<u8>) {
    static KEY: OnceLock<(Vec<u8>, Vec<u8>)> = OnceLock::new();
    KEY.get_or_init(ActivePQC::keypair)
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
        max_block_transaction_gas: 10000,
        max_block_transaction_bytes: 1_000_000,
        max_block_signature_checks: 100,
        max_fee_cap: 2000,
        limits: Limits {
            max_wire_bytes: 262144,
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
            max_gas: p.max_block_transaction_gas,
            max_bytes: p.max_block_transaction_bytes,
            max_signatures: p.max_block_signature_checks,
            mandatory_expiry_gas: 100,
        },
    )
    .unwrap()
}
fn runtime_limits() -> RuntimeLimits {
    RuntimeLimits {
        max_receipts: 65536,
        max_retained_profiles: 65536,
        max_grants: 3,
    }
}
struct Fixture {
    book: RecoveryBook,
    grants: Grants,
    history: FeeHistory,
    settlement: Settlement,
    _temp: tempfile::TempDir,
}
fn fixture() -> Fixture {
    let rp = RecoveryProfile {
        version: 1,
        activation_height: 1,
        denomination: "udrt".into(),
        gas_price: 1,
        minimum_gas: 1,
        max_transaction_gas: 1000,
        max_block_gas: 10000,
        max_block_recovery_bytes: 1_000_000,
        max_block_recovery_signatures: 100,
        max_fee_cap: 2000,
        max_pending_accounts: 3,
        max_due_expiry_events_per_height: 3,
        mandatory_expiry_gas_budget: 100,
        expiry_event_gas_cost: 1,
        action_costs: [1; 9],
        wire_byte_cost: 0,
        read_byte_cost: 0,
        write_byte_cost: 0,
        signature_costs: BTreeMap::from([("mldsa65".into(), 1)]),
    };
    let accounts = [ACTOR, OWNER, OTHER]
        .into_iter()
        .map(|id| RecoveryAccount {
            address: AccountAddress::from_account_id(AddressNetwork::Development, id).encode(),
            recovery: RecoveryState::new(
                RecoveryDomain {
                    network: 3,
                    chain_id: "ordinary-runtime-test".into(),
                    genesis_digest: [7; 32],
                    account_id: id,
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
    let temp = tempfile::tempdir().unwrap();
    let storage = Arc::new(Storage::open(temp.path().join("db")).unwrap());
    let mut settlement = Settlement::new(storage);
    for a in book.accounts.values() {
        let account = settlement.account(&a.address).unwrap();
        account.nonce = 0;
        account.set_balance("udgt", 100);
        account.set_balance(
            "udrt",
            if a.recovery.domain.account_id == ACTOR {
                1000
            } else {
                50
            },
        );
    }
    settlement.fee_total = Some(17);
    Fixture {
        book,
        grants: Grants::new(),
        history: FeeHistory::default(),
        settlement,
        _temp: temp,
    }
}
fn addr(f: &Fixture, id: [u8; 32]) -> String {
    f.book.accounts[&hex::encode(id)].address.clone()
}
fn set(f: &mut Fixture, id: [u8; 32], denom: &str, amount: u128) {
    let address = addr(f, id);
    f.settlement
        .account(&address)
        .unwrap()
        .set_balance(denom, amount);
}
fn balance(f: &Fixture, id: [u8; 32], denom: &str) -> u128 {
    f.settlement.accounts[&addr(f, id)].balance_of(denom)
}
fn signed(
    f: &Fixture,
    p: &FeeProfile,
    actions: Vec<Action>,
    gas: u64,
    cap: u128,
) -> SignedOrdinary {
    let a = &f.book.accounts[&hex::encode(ACTOR)];
    let body = OrdinaryTransaction {
        domain: a.recovery.domain.clone(),
        authorization_generation: a.recovery.active_generation,
        spending_nonce: a.recovery.spending_nonce,
        key: a.recovery.active_key.clone(),
        expiry_height: 100,
        ordinary_fee_contract_version: p.ordinary_fee_contract_version,
        fee_profile_version: p.version,
        fee_profile_digest: ordinary_fees::profile_digest(p).unwrap(),
        fee_denomination: ordinary::Denomination::Udrt,
        maximum_fee: cap,
        gas_limit: gas,
        memo: String::new(),
        actions,
    };
    let signature = ActivePQC::sign(
        &keys().0,
        &ordinary::signing_bytes(&body, &p.limits).unwrap(),
    );
    SignedOrdinary { body, signature }
}
fn resign(s: &mut SignedOrdinary, p: &FeeProfile) {
    s.signature = ActivePQC::sign(
        &keys().0,
        &ordinary::signing_bytes(&s.body, &p.limits).unwrap(),
    );
}
fn run(
    f: &mut Fixture,
    p: &FeeProfile,
    s: &SignedOrdinary,
    index: u32,
    block: &mut SharedBlockMeter,
) -> Result<OrdinaryExecutionResult> {
    execute_signed(
        s,
        p,
        &mut f.book,
        &mut f.grants,
        &mut f.history,
        &mut f.settlement,
        1,
        index,
        runtime_limits(),
        block,
    )
}
fn fingerprint(f: &Fixture) -> Vec<u8> {
    serde_json::to_vec(&(
        &f.book,
        &f.grants,
        &f.history,
        &f.settlement.accounts,
        f.settlement.fee_total,
        &f.settlement.rewards,
        &f.settlement.validators,
        &f.settlement.penalties,
    ))
    .unwrap()
}
fn send(recipient: [u8; 32], amount: u128) -> Action {
    Action::Send {
        recipient,
        denomination: ordinary::Denomination::Udrt,
        amount,
    }
}
fn claim_grant(f: &mut Fixture, beneficiary: [u8; 32], period: u64) {
    f.grants.insert(
        hex::encode(OWNER),
        DiscretionaryGrant {
            version: 1,
            owner: OWNER,
            beneficiary,
            owner_generation: 0,
            period_blocks: period,
            last_active_height: 0,
        },
    );
}
#[test]
fn current_fee_cap_and_action_liquidity_are_required_without_future_claim_credit() {
    let p = profile();
    let mut f = fixture();
    claim_grant(&mut f, ACTOR, 1);
    set(&mut f, ACTOR, "udrt", 199);
    let s = signed(
        &f,
        &p,
        vec![
            Action::DmsClaim {
                owner: OWNER,
                expected_grant_generation: 0,
            },
            send(OTHER, 10),
        ],
        100,
        200,
    );
    let before = fingerprint(&f);
    let result = run(&mut f, &p, &s, 0, &mut shared(&p)).unwrap();
    assert!(!result.accepted);
    assert_eq!(result.fee, 0);
    assert_eq!(fingerprint(&f), before);
    let mut f = fixture();
    let s = signed(&f, &p, vec![send(OWNER, 801)], 100, 200);
    let before = fingerprint(&f);
    assert!(!run(&mut f, &p, &s, 0, &mut shared(&p)).unwrap().accepted);
    assert_eq!(fingerprint(&f), before);
}
#[test]
fn invalid_signature_stale_generation_and_wrong_beneficiary_are_zero_charge() {
    let p = profile();
    let mut f = fixture();
    claim_grant(&mut f, OTHER, 100);
    let wrong = signed(
        &f,
        &p,
        vec![Action::DmsClaim {
            owner: OWNER,
            expected_grant_generation: 0,
        }],
        100,
        200,
    );
    let before = fingerprint(&f);
    let result = run(&mut f, &p, &wrong, 0, &mut shared(&p)).unwrap();
    assert!(!result.accepted);
    assert!(result.receipt.is_none());
    assert_eq!(fingerprint(&f), before);
    let mut s = signed(
        &f,
        &p,
        vec![Action::Data {
            data: "no debit".into(),
        }],
        100,
        200,
    );
    s.signature[0] ^= 1;
    assert!(!run(&mut f, &p, &s, 0, &mut shared(&p)).unwrap().accepted);
    assert_eq!(fingerprint(&f), before);
    let mut s = signed(
        &f,
        &p,
        vec![Action::Data {
            data: "stale".into(),
        }],
        100,
        200,
    );
    s.body.authorization_generation = 1;
    resign(&mut s, &p);
    assert!(!run(&mut f, &p, &s, 0, &mut shared(&p)).unwrap().accepted);
    assert_eq!(fingerprint(&f), before);
}
#[test]
fn minimum_fee_and_metadata_are_charged_once_with_exact_cap_release() {
    let mut p = profile();
    p.minimum_gas = 50;
    let mut f = fixture();
    let s = signed(
        &f,
        &p,
        vec![Action::Data {
            data: "meter only".into(),
        }],
        100,
        200,
    );
    let result = run(&mut f, &p, &s, 0, &mut shared(&p)).unwrap();
    assert!(result.success);
    assert_eq!(result.gas_used, 20);
    assert_eq!(result.fee, 100);
    assert_eq!(result.receipt.unwrap().released_cap(), 100);
    assert_eq!(balance(&f, ACTOR, "udrt"), 900);
    assert_eq!(f.settlement.fee_total, Some(117));
    assert_eq!(
        f.book.accounts[&hex::encode(ACTOR)].recovery.spending_nonce,
        1
    );
    assert_eq!(
        f.settlement
            .storage
            .db
            .iterator(rocksdb::IteratorMode::Start)
            .count(),
        0
    );
}
#[test]
fn each_accepted_receipt_matches_its_own_account_and_fee_transitions() {
    let p = profile();
    for outcome in [
        Outcome::Success,
        Outcome::ApplicationFailure,
        Outcome::OutOfGas,
    ] {
        let mut f = fixture();
        if outcome == Outcome::ApplicationFailure {
            claim_grant(&mut f, ACTOR, 100);
        }
        let actions = match outcome {
            Outcome::Success => vec![send(OWNER, 10)],
            Outcome::ApplicationFailure => vec![
                send(OWNER, 10),
                Action::DmsClaim {
                    owner: OWNER,
                    expected_grant_generation: 0,
                },
            ],
            Outcome::OutOfGas => vec![
                send(OWNER, 10),
                Action::Data {
                    data: "exhaust".into(),
                },
            ],
        };
        let (gas_limit, cap) = if outcome == Outcome::OutOfGas {
            (25, 50)
        } else {
            (100, 200)
        };
        let signed = signed(&f, &p, actions, gas_limit, cap);
        let before = f.settlement.clone();
        let result = run(&mut f, &p, &signed, 0, &mut shared(&p)).unwrap();
        let receipt = result.receipt.unwrap();
        assert_eq!(receipt.outcome(), outcome);
        let actor = addr(&f, ACTOR);
        let owner = addr(&f, OWNER);
        let mut reserved = before.clone();
        debit(&mut reserved, &actor, "udrt", cap).unwrap();
        let mut actions = reserved.clone();
        if outcome == Outcome::Success {
            transfer(&mut actions, &actor, &owner, "udrt", 10).unwrap();
        }
        reconcile_receipt(
            &before,
            &reserved.accounts,
            reserved.ordinary_fee_total().unwrap(),
            &actions.accounts,
            actions.ordinary_fee_total().unwrap(),
            &f.settlement,
            &f.book,
            &receipt,
        )
        .unwrap();

        // Moving one unit between accounts preserves aggregate custody. It
        // must still fail this transaction's account reconciliation.
        let mut misallocated = f.settlement.clone();
        let actor_balance = misallocated.account(&actor).unwrap().balance_of("udrt");
        misallocated
            .account(&actor)
            .unwrap()
            .set_balance("udrt", actor_balance - 1);
        let owner_balance = misallocated.account(&owner).unwrap().balance_of("udrt");
        misallocated
            .account(&owner)
            .unwrap()
            .set_balance("udrt", owner_balance + 1);
        assert!(reconcile_receipt(
            &before,
            &reserved.accounts,
            reserved.ordinary_fee_total().unwrap(),
            &actions.accounts,
            actions.ordinary_fee_total().unwrap(),
            &misallocated,
            &f.book,
            &receipt
        )
        .is_err());

        let mut wrong_nonce = f.settlement.clone();
        wrong_nonce.account(&owner).unwrap().nonce += 1;
        assert!(reconcile_receipt(
            &before,
            &reserved.accounts,
            reserved.ordinary_fee_total().unwrap(),
            &actions.accounts,
            actions.ordinary_fee_total().unwrap(),
            &wrong_nonce,
            &f.book,
            &receipt
        )
        .is_err());

        let mut wrong_fee_total = f.settlement.clone();
        wrong_fee_total.fee_total = Some(f.settlement.ordinary_fee_total().unwrap() - 1);
        assert!(reconcile_receipt(
            &before,
            &reserved.accounts,
            reserved.ordinary_fee_total().unwrap(),
            &actions.accounts,
            actions.ordinary_fee_total().unwrap(),
            &wrong_fee_total,
            &f.book,
            &receipt
        )
        .is_err());
    }
}
#[test]
fn insufficient_metadata_budget_rejects_execution_and_admission_before_acceptance() {
    let p = profile();
    for admission in [false, true] {
        let mut f = fixture();
        let s = signed(&f, &p, vec![Action::Data { data: "x".into() }], 9, 18);
        let before = fingerprint(&f);
        let mut block = shared(&p);
        let result = if admission {
            admission_only(
                &s,
                &p,
                &mut f.book,
                &mut f.grants,
                &mut f.history,
                &mut f.settlement,
                1,
                0,
                runtime_limits(),
                &mut block,
            )
        } else {
            run(&mut f, &p, &s, 0, &mut block)
        }
        .unwrap();
        assert!(!result.accepted);
        assert_eq!(result.fee, 0);
        assert!(result.receipt.is_none());
        assert_eq!(fingerprint(&f), before);
    }
}
#[test]
fn late_out_of_gas_rolls_back_all_actions_and_keeps_prior_transaction_effects() {
    let p = profile();
    let mut f = fixture();
    let mut block = shared(&p);
    let first = signed(&f, &p, vec![send(OWNER, 10)], 100, 200);
    assert!(run(&mut f, &p, &first, 0, &mut block).unwrap().success);
    let before_owner = balance(&f, OWNER, "udrt");
    let second = signed(
        &f,
        &p,
        vec![
            send(OWNER, 20),
            Action::Data {
                data: "exhaust".into(),
            },
        ],
        25,
        50,
    );
    let result = run(&mut f, &p, &second, 1, &mut block).unwrap();
    assert!(result.accepted && !result.success);
    assert_eq!(result.gas_used, 25);
    assert_eq!(result.fee, 50);
    assert_eq!(
        result.receipt.as_ref().unwrap().outcome(),
        Outcome::OutOfGas
    );
    assert_eq!(result.receipt.unwrap().released_cap(), 0);
    assert_eq!(balance(&f, OWNER, "udrt"), before_owner);
    assert_eq!(balance(&f, ACTOR, "udrt"), 900);
    assert_eq!(f.settlement.fee_total, Some(107));
    assert_eq!(
        f.book.accounts[&hex::encode(ACTOR)].recovery.spending_nonce,
        2
    );
}
#[test]
fn immature_owned_claim_is_paid_but_internal_faults_publish_no_checkpoint() {
    let p = profile();
    let mut f = fixture();
    claim_grant(&mut f, ACTOR, 100);
    let s = signed(
        &f,
        &p,
        vec![
            send(OTHER, 10),
            Action::DmsClaim {
                owner: OWNER,
                expected_grant_generation: 0,
            },
        ],
        100,
        200,
    );
    let result = run(&mut f, &p, &s, 0, &mut shared(&p)).unwrap();
    assert!(result.accepted && !result.success);
    assert_eq!(result.fee, 60);
    assert_eq!(
        result.receipt.unwrap().outcome(),
        Outcome::ApplicationFailure
    );
    assert_eq!(balance(&f, OTHER, "udrt"), 50);
    assert_eq!(balance(&f, OWNER, "udrt"), 50);
    assert_eq!(balance(&f, ACTOR, "udrt"), 940);
    let mut f = fixture();
    set(&mut f, OWNER, "udrt", u128::MAX);
    let s = signed(&f, &p, vec![send(OWNER, 1)], 100, 200);
    let before = fingerprint(&f);
    assert!(matches!(
        run(&mut f, &p, &s, 0, &mut shared(&p)),
        Err(PlanError::Internal(_))
    ));
    assert_eq!(fingerprint(&f), before);
    let mut f = fixture();
    let a = addr(&f, ACTOR);
    f.settlement.account(&a).unwrap().nonce = 1;
    let s = signed(
        &f,
        &p,
        vec![Action::Data {
            data: "mirror mismatch".into(),
        }],
        100,
        200,
    );
    let before = fingerprint(&f);
    assert!(matches!(
        run(&mut f, &p, &s, 0, &mut shared(&p)),
        Err(PlanError::Internal(_))
    ));
    assert_eq!(fingerprint(&f), before);
}
#[test]
fn signed_input_reservation_overflow_is_zero_charge_rejection_not_block_fault() {
    let p = profile();
    let mut f = fixture();
    let s = signed(
        &f,
        &p,
        vec![
            Action::Send {
                recipient: OWNER,
                denomination: ordinary::Denomination::Udgt,
                amount: u128::MAX,
            },
            Action::Send {
                recipient: OTHER,
                denomination: ordinary::Denomination::Udgt,
                amount: 1,
            },
        ],
        100,
        200,
    );
    let before = fingerprint(&f);
    let result = run(&mut f, &p, &s, 0, &mut shared(&p)).unwrap();
    assert!(!result.accepted);
    assert_eq!(result.fee, 0);
    assert_eq!(fingerprint(&f), before);
}
#[test]
fn self_transfer_reserves_peak_plus_fee_but_has_no_net_action_debit() {
    let p = profile();
    let mut f = fixture();
    let s = signed(&f, &p, vec![send(ACTOR, 900)], 50, 100);
    let result = run(&mut f, &p, &s, 0, &mut shared(&p)).unwrap();
    assert!(result.success);
    assert_eq!(result.fee, 40);
    assert_eq!(balance(&f, ACTOR, "udrt"), 960);
    assert_eq!(balance(&f, OWNER, "udrt"), 50);
    let mut f = fixture();
    let s = signed(&f, &p, vec![send(ACTOR, 901)], 50, 100);
    let before = fingerprint(&f);
    assert!(!run(&mut f, &p, &s, 0, &mut shared(&p)).unwrap().accepted);
    assert_eq!(fingerprint(&f), before);
}
#[test]
fn repeated_claim_uses_current_owner_bound_once_and_preserves_zero_claim_success() {
    let p = profile();
    let mut f = fixture();
    claim_grant(&mut f, ACTOR, 1);
    let s = signed(
        &f,
        &p,
        vec![
            Action::DmsClaim {
                owner: OWNER,
                expected_grant_generation: 0,
            },
            Action::DmsClaim {
                owner: OWNER,
                expected_grant_generation: 0,
            },
        ],
        100,
        200,
    );
    let result = run(&mut f, &p, &s, 0, &mut shared(&p)).unwrap();
    assert!(result.success);
    assert_eq!(result.fee, 60);
    assert_eq!(balance(&f, ACTOR, "udrt"), 990);
    assert_eq!(balance(&f, ACTOR, "udgt"), 200);
    assert_eq!(balance(&f, OWNER, "udrt"), 0);
    assert_eq!(balance(&f, OWNER, "udgt"), 0);
    let empty = signed(
        &f,
        &p,
        vec![Action::DmsClaim {
            owner: OWNER,
            expected_grant_generation: 0,
        }],
        100,
        200,
    );
    let result = run(&mut f, &p, &empty, 1, &mut shared(&p)).unwrap();
    assert!(result.success);
    assert_eq!(result.fee, 40);
    assert_eq!(balance(&f, ACTOR, "udrt"), 950);
}
#[test]
fn explicit_receipt_capacity_rejects_new_acceptance_without_charging() {
    let p = profile();
    let mut f = fixture();
    let mut block = shared(&p);
    let first = signed(
        &f,
        &p,
        vec![Action::Data {
            data: "first".into(),
        }],
        100,
        200,
    );
    let caps = RuntimeLimits {
        max_receipts: 1,
        max_retained_profiles: 1,
        max_grants: 3,
    };
    let result = execute_signed(
        &first,
        &p,
        &mut f.book,
        &mut f.grants,
        &mut f.history,
        &mut f.settlement,
        1,
        0,
        caps,
        &mut block,
    )
    .unwrap();
    assert!(result.success);
    let second = signed(
        &f,
        &p,
        vec![Action::Data {
            data: "second".into(),
        }],
        100,
        200,
    );
    let before = fingerprint(&f);
    let result = execute_signed(
        &second,
        &p,
        &mut f.book,
        &mut f.grants,
        &mut f.history,
        &mut f.settlement,
        1,
        1,
        caps,
        &mut block,
    )
    .unwrap();
    assert!(!result.accepted);
    assert_eq!(result.fee, 0);
    assert!(result.receipt.is_none());
    assert_eq!(fingerprint(&f), before);
}
#[test]
fn positive_logical_costs_charge_each_read_final_write_and_metadata_once() {
    let mut p = profile();
    p.max_transaction_gas = 1_000_000;
    p.max_block_transaction_gas = 10_000_000;
    p.max_fee_cap = 2_000_000;
    p.wire_byte_cost = 1;
    p.read_byte_cost = 2;
    p.write_byte_cost = 3;
    let mut f = fixture();
    set(&mut f, ACTOR, "udrt", 3_000_000);
    let s = signed(&f, &p, vec![send(OWNER, 10)], 1_000_000, 2_000_000);
    let wire = ordinary::encode(&s, &p.limits).unwrap().len() as u64;
    let mut read_bytes = 0;
    for a in f.book.accounts.values() {
        read_bytes += logical::native_account(
            &a.address,
            &f.settlement.accounts[&a.address],
            MAX_LOGICAL_BYTES,
        )
        .unwrap()
        .byte_len();
        read_bytes +=
            logical::recovery_account(&a.recovery.domain.account_id, a, MAX_LOGICAL_BYTES)
                .unwrap()
                .byte_len();
        read_bytes += logical::grant(&a.recovery.domain.account_id, None, MAX_LOGICAL_BYTES)
            .unwrap()
            .byte_len();
    }
    // Native logical amounts and nonces have fixed width. Their values do not
    // change these final write sizes after fee reservation or transfer.
    let write_bytes = [ACTOR, OWNER]
        .into_iter()
        .map(|id| {
            let a = addr(&f, id);
            logical::native_account(&a, &f.settlement.accounts[&a], MAX_LOGICAL_BYTES)
                .unwrap()
                .byte_len()
        })
        .sum::<u64>();
    let expected = p.transaction_overhead
        + wire * p.wire_byte_cost
        + p.signature_costs["mldsa65"]
        + read_bytes * p.read_byte_cost
        + p.receipt_metadata_cost
        + p.action_costs[0]
        + write_bytes * p.write_byte_cost;
    let result = run(&mut f, &p, &s, 0, &mut shared(&p)).unwrap();
    assert!(result.success);
    assert_eq!(result.gas_used, expected);
    assert_eq!(result.fee, u128::from(expected) * 2);
    assert_eq!(
        result.receipt.unwrap().released_cap(),
        2_000_000 - result.fee
    );
}
#[test]
fn cumulative_operator_self_unbond_checks_every_affected_protected_principal_owner() {
    use crate::runtime::validator_lifecycle::{
        LifecycleConfig, LifecycleState, ValidatorIdentity, ValidatorView,
    };
    use dytallix_protocol_types::recovery::{Guardian, RecoveryPolicy, RecoveryStatus};
    let mut f = fixture();
    let actor = addr(&f, ACTOR);
    let delegator = addr(&f, OWNER);
    let protected = &mut f
        .book
        .accounts
        .get_mut(&hex::encode(OWNER))
        .unwrap()
        .recovery;
    protected.policy = Some(RecoveryPolicy {
        threshold: 2,
        guardians: (10..13)
            .map(|n| Guardian {
                key: KeyIdentity {
                    algorithm: "mldsa65".into(),
                    public_key: vec![n; 1952],
                },
                control_group: format!("group-{n}"),
            })
            .collect(),
    });
    protected.policy_version = 1;
    protected.status = RecoveryStatus::RecoveryLocked;
    f.book.validate().unwrap();
    let view = ValidatorView {
        validators: BTreeMap::from([(
            "validator".into(),
            ValidatorIdentity {
                owner: actor.clone(),
                pubkey_base64: "projection-only-key".into(),
            },
        )]),
        positions: BTreeMap::from([
            (actor.clone(), BTreeMap::from([("validator".into(), 100)])),
            (delegator, BTreeMap::from([("validator".into(), 50)])),
        ]),
    };
    // This isolates the owner projection. It does not claim a validated consensus
    // key, validator transition, or fee acceptance; integration tests cover those.
    f.settlement.validators = Some(LifecycleState {
        config: LifecycleConfig {
            version: 1,
            profile: "cometbft-lifecycle-local-qualification".into(),
            chain_id: "ordinary-runtime-test".into(),
            approved_operators: BTreeMap::from([("validator".into(), actor.clone())]),
            min_self_bond: 90,
            max_active: 4,
            evidence_max_age_blocks: 3,
            evidence_max_age_seconds: 3,
            processing_margin_blocks: 2,
            processing_margin_seconds: 2,
        },
        effective: view.clone(),
        schedules: BTreeMap::new(),
        unbonding: BTreeMap::new(),
        history: BTreeMap::from([(1, view)]),
        update_history: BTreeMap::new(),
        last_height: 1,
        max_positions: 8,
        next_unbond_id: 0,
        reserved_owners: BTreeSet::new(),
    });
    let first = Action::RewardBeginUnbond {
        validator_id: "validator".into(),
        amount_udgt: 5,
    };
    let second = Action::RewardBeginUnbond {
        validator_id: "validator".into(),
        amount_udgt: 6,
    };
    assert!(principal_owners(&f.book, &f.settlement, &actor, &[first.clone()]).is_ok());
    let before = fingerprint(&f);
    assert!(matches!(
        principal_owners(&f.book, &f.settlement, &actor, &[first, second]),
        Err(PlanError::Rejected(_))
    ));
    assert_eq!(fingerprint(&f), before);
}
