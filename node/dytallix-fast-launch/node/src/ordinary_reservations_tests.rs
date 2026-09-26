use super::*;
const CONTEXT: [u8; 32] = [9; 32];
const ALICE: Owner = [1; 32];
const BOB: Owner = [2; 32];
fn asset(owner: Owner, denomination: Denomination) -> Asset {
    Asset {
        owner,
        denomination,
    }
}
fn limits() -> QueueLimits {
    QueueLimits {
        max_entries: 8,
        max_wire_bytes: 800,
        max_signature_work: 16,
        max_action_debits_per_entry: 8,
    }
}
fn ledger() -> ReservationLedger {
    ReservationLedger::new(CONTEXT, limits()).unwrap()
}
fn ordinary(id: u8, nonce: u64) -> ReservationRequest {
    ReservationRequest {
        context_digest: CONTEXT,
        id: ReservationId::Ordinary([id; 32]),
        payer: ALICE,
        nonce,
        fee_cap_udrt: 10,
        action_debits: vec![],
        unrestricted_debits: vec![],
        wire_bytes: 100,
        signature_work: 2,
    }
}
fn sponsor(id: u8, nonce: u64) -> ReservationRequest {
    ReservationRequest {
        id: ReservationId::RecoverySponsorship([id; 32]),
        ..ordinary(id, nonce)
    }
}
fn debit(owner: Owner, denom: Denomination, amount: u128, kind: DebitKind) -> ActionDebit {
    ActionDebit {
        asset: asset(owner, denom),
        amount,
        kind,
    }
}
fn liquid(a: u128, b: u128) -> Eligibility {
    let total = BTreeMap::from([
        (asset(ALICE, Denomination::Udrt), a),
        (asset(BOB, Denomination::Udrt), b),
        (asset(ALICE, Denomination::Udgt), 100),
        (asset(BOB, Denomination::Udgt), 100),
    ]);
    Eligibility {
        unrestricted: total.clone(),
        total,
    }
}
fn reject_unchanged(
    ledger: &mut ReservationLedger,
    request: &ReservationRequest,
    liquidity: &Eligibility,
    expected: ReservationError,
) {
    let before = ledger.clone();
    assert_eq!(ledger.reserve(request, liquidity), Err(expected));
    assert_eq!(*ledger, before);
}
#[test]
fn ordinary_and_sponsor_share_liquidity_but_have_distinct_counter_namespaces() {
    let mut l = ledger();
    let o = ordinary(1, 0);
    let r = sponsor(1, 0);
    assert_eq!(l.reserve(&o, &liquid(20, 0)), Ok(ReservationStatus::Added));
    assert_eq!(l.reserve(&r, &liquid(20, 0)), Ok(ReservationStatus::Added));
    assert_eq!(l.len(), 2);
    assert_eq!(l.resources(), (200, 4));
    assert_eq!(
        l.reserved(asset(ALICE, Denomination::Udrt)).unwrap(),
        ReservedAmounts { fee: 20, action: 0 }
    );
    reject_unchanged(
        &mut l,
        &ordinary(2, 1),
        &liquid(20, 0),
        ReservationError::InsufficientLiquidity,
    );
    reject_unchanged(
        &mut l,
        &sponsor(2, 1),
        &liquid(20, 0),
        ReservationError::InsufficientLiquidity,
    );
}
#[test]
fn v2_and_v3_share_one_spending_nonce_namespace_but_not_sponsor_nonce() {
    let mut l = ledger();
    let v2 = ordinary(1, 4);
    let v3 = ReservationRequest {
        id: ReservationId::OrdinaryV3 {
            transaction_id: [2; 32],
            profile_digest: [3; 32],
        },
        ..ordinary(2, 4)
    };
    let sponsor = sponsor(3, 4);
    l.reserve(&v2, &liquid(30, 0)).unwrap();
    reject_unchanged(&mut l, &v3, &liquid(30, 0), ReservationError::NonceConflict);
    l.reserve(&sponsor, &liquid(30, 0)).unwrap();
    l.evict(v2.id).unwrap();
    l.reserve(&v3, &liquid(30, 0)).unwrap();
    reject_unchanged(&mut l, &v2, &liquid(30, 0), ReservationError::NonceConflict);
    let another_v3 = ReservationRequest {
        id: ReservationId::OrdinaryV3 {
            transaction_id: [4; 32],
            profile_digest: [5; 32],
        },
        ..ordinary(4, 4)
    };
    reject_unchanged(
        &mut l,
        &another_v3,
        &liquid(30, 0),
        ReservationError::NonceConflict,
    );
    assert_eq!(
        l.reserve(&v3, &liquid(0, 0)),
        Ok(ReservationStatus::AlreadyReserved)
    );
}
#[test]
fn exact_intent_deduplicates_and_conflicting_body_requires_explicit_eviction() {
    let mut l = ledger();
    let o = ordinary(1, 0);
    l.reserve(&o, &liquid(20, 0)).unwrap();
    let before = l.clone();
    // Signature envelope bytes are intentionally absent from request identity.
    assert_eq!(
        l.reserve(&o, &liquid(0, 0)),
        Ok(ReservationStatus::AlreadyReserved)
    );
    assert_eq!(l, before); // This duplicate does not claim revalidation.
    reject_unchanged(
        &mut l,
        &ordinary(2, 0),
        &liquid(20, 0),
        ReservationError::NonceConflict,
    );
    let mut inconsistent = o.clone();
    inconsistent.fee_cap_udrt = 11;
    reject_unchanged(
        &mut l,
        &inconsistent,
        &liquid(20, 0),
        ReservationError::IdentityMismatch,
    );
    assert!(l.evict(o.id).unwrap());
    assert!(!l.evict(o.id).unwrap());
    assert_eq!(l.resources(), (0, 0));
    assert_eq!(l.len(), 0);
    assert_eq!(
        l.reserved(asset(ALICE, Denomination::Udrt)).unwrap(),
        ReservedAmounts::default()
    );
    l.reserve(&ordinary(2, 0), &liquid(10, 0)).unwrap();
}
#[test]
fn sponsor_identity_conflict_and_eviction_release_only_its_own_cap() {
    let mut l = ledger();
    let o = ordinary(1, 0);
    let r = sponsor(1, 0);
    l.reserve(&o, &liquid(20, 0)).unwrap();
    l.reserve(&r, &liquid(20, 0)).unwrap();
    reject_unchanged(
        &mut l,
        &sponsor(2, 0),
        &liquid(20, 0),
        ReservationError::NonceConflict,
    );
    assert_eq!(
        l.reserve(&r, &liquid(20, 0)),
        Ok(ReservationStatus::AlreadyReserved)
    );
    assert!(l.evict(r.id).unwrap());
    assert!(!l.evict(r.id).unwrap());
    assert_eq!(l.resources(), (100, 2));
    assert_eq!(
        l.reserved(asset(ALICE, Denomination::Udrt)).unwrap().fee,
        10
    );
    l.reserve(&sponsor(2, 0), &liquid(20, 0)).unwrap();
}
#[test]
fn ordered_self_transfer_peak_never_spends_reserved_fee_or_accumulates_net() {
    let mut o = ordinary(1, 0);
    o.action_debits = vec![
        debit(ALICE, Denomination::Udrt, 50, DebitKind::SelfTransfer),
        debit(ALICE, Denomination::Udrt, 30, DebitKind::Outflow),
        debit(ALICE, Denomination::Udrt, 40, DebitKind::SelfTransfer),
        debit(ALICE, Denomination::Udrt, 20, DebitKind::Outflow),
    ];
    let mut l = ledger();
    reject_unchanged(
        &mut l,
        &o,
        &liquid(79, 0),
        ReservationError::InsufficientLiquidity,
    );
    l.reserve(&o, &liquid(80, 0)).unwrap();
    assert_eq!(
        l.reserved(asset(ALICE, Denomination::Udrt)).unwrap(),
        ReservedAmounts {
            fee: 10,
            action: 70
        }
    );
    assert!(l.evict(o.id).unwrap());
    assert_eq!(
        l.reserved(asset(ALICE, Denomination::Udrt))
            .unwrap()
            .total()
            .unwrap(),
        0
    );
}
#[test]
fn beneficiary_fee_and_actual_owner_claim_bound_compete_with_owner_sponsorship() {
    // These are synthetic trusted accounting inputs. This test makes no claim
    // that Bob is unprotected or that Alice has a valid discretionary grant.
    let mut claim = ordinary(1, 0);
    claim.action_debits = vec![
        debit(BOB, Denomination::Udrt, 30, DebitKind::Outflow),
        debit(BOB, Denomination::Udgt, 70, DebitKind::Outflow),
    ];
    let mut r = sponsor(1, 0);
    r.payer = BOB;
    let mut l = ledger();
    l.reserve(&claim, &liquid(10, 40)).unwrap();
    l.reserve(&r, &liquid(10, 40)).unwrap();
    assert_eq!(
        l.reserved(asset(BOB, Denomination::Udrt)).unwrap(),
        ReservedAmounts {
            fee: 10,
            action: 30
        }
    );
    assert_eq!(
        l.reserved(asset(BOB, Denomination::Udgt)).unwrap().action,
        70
    );
    let mut more = sponsor(2, 1);
    more.payer = BOB;
    reject_unchanged(
        &mut l,
        &more,
        &liquid(1000, 40),
        ReservationError::InsufficientLiquidity,
    );
    assert!(l.evict(claim.id).unwrap());
    assert_eq!(
        l.reserved(asset(BOB, Denomination::Udrt))
            .unwrap()
            .total()
            .unwrap(),
        10
    );
    l.reserve(&more, &liquid(10, 40)).unwrap();
}
#[test]
fn uncommitted_transfer_claim_and_unbond_credit_cannot_fund_new_reservations() {
    let mut transfer = ordinary(1, 0);
    transfer.action_debits = vec![debit(ALICE, Denomination::Udrt, 20, DebitKind::Outflow)];
    let mut l = ledger();
    l.reserve(&transfer, &liquid(30, 0)).unwrap();
    let mut bob = sponsor(2, 0);
    bob.payer = BOB;
    reject_unchanged(
        &mut l,
        &bob,
        &liquid(30, 0),
        ReservationError::InsufficientLiquidity,
    );
    // A prospective claim has no incoming-credit field in the accounting API.
    let mut claim = ordinary(2, 1);
    claim.action_debits = vec![debit(BOB, Denomination::Udgt, 20, DebitKind::Outflow)];
    reject_unchanged(
        &mut l,
        &claim,
        &liquid(30, 0),
        ReservationError::InsufficientLiquidity,
    );
    // Current eligible funds, not nominal liquid/locked supply, are the only input.
    let mut locked = ordinary(3, 2);
    locked.action_debits = vec![debit(ALICE, Denomination::Udgt, 1, DebitKind::Outflow)];
    let mut eligibility = liquid(100, 100);
    eligibility.total.remove(&asset(ALICE, Denomination::Udgt));
    eligibility
        .unrestricted
        .remove(&asset(ALICE, Denomination::Udgt));
    reject_unchanged(
        &mut l,
        &locked,
        &eligibility,
        ReservationError::InsufficientLiquidity,
    );
}
#[test]
fn resource_limits_are_explicit_checked_and_atomic() {
    for invalid in [
        QueueLimits {
            max_entries: 0,
            ..limits()
        },
        QueueLimits {
            max_wire_bytes: 0,
            ..limits()
        },
        QueueLimits {
            max_signature_work: 0,
            ..limits()
        },
        QueueLimits {
            max_action_debits_per_entry: 0,
            ..limits()
        },
    ] {
        assert_eq!(
            ReservationLedger::new(CONTEXT, invalid),
            Err(ReservationError::InvalidLimits)
        );
    }
    for bounded in [
        QueueLimits {
            max_entries: 1,
            ..limits()
        },
        QueueLimits {
            max_wire_bytes: 100,
            ..limits()
        },
        QueueLimits {
            max_signature_work: 2,
            ..limits()
        },
    ] {
        let mut l = ReservationLedger::new(CONTEXT, bounded).unwrap();
        l.reserve(&ordinary(1, 0), &liquid(100, 100)).unwrap();
        reject_unchanged(
            &mut l,
            &sponsor(2, 0),
            &liquid(100, 100),
            ReservationError::Capacity,
        );
        l.evict(ordinary(1, 0).id).unwrap();
        l.reserve(&sponsor(2, 0), &liquid(100, 100)).unwrap();
    }
    let mut l = ledger();
    let mut too_many = ordinary(1, 0);
    too_many.action_debits = vec![debit(ALICE, Denomination::Udgt, 0, DebitKind::Outflow); 9];
    reject_unchanged(
        &mut l,
        &too_many,
        &liquid(100, 100),
        ReservationError::Capacity,
    );
}
#[test]
fn arithmetic_and_malformed_inputs_leave_every_index_and_resource_unchanged() {
    let mut l = ledger();
    let mut request = ordinary(1, 0);
    request.action_debits = vec![debit(
        ALICE,
        Denomination::Udrt,
        u128::MAX,
        DebitKind::SelfTransfer,
    )];
    reject_unchanged(
        &mut l,
        &request,
        &liquid(u128::MAX, 0),
        ReservationError::Overflow,
    );
    request.action_debits = vec![
        debit(ALICE, Denomination::Udgt, u128::MAX, DebitKind::Outflow),
        debit(ALICE, Denomination::Udgt, 1, DebitKind::Outflow),
    ];
    reject_unchanged(
        &mut l,
        &request,
        &liquid(u128::MAX, 0),
        ReservationError::Overflow,
    );
    for bad in [
        ReservationRequest {
            context_digest: [8; 32],
            ..ordinary(1, 0)
        },
        ReservationRequest {
            nonce: u64::MAX,
            ..ordinary(1, 0)
        },
        ReservationRequest {
            wire_bytes: 0,
            ..ordinary(1, 0)
        },
        ReservationRequest {
            signature_work: 0,
            ..ordinary(1, 0)
        },
        ReservationRequest {
            fee_cap_udrt: 0,
            ..ordinary(1, 0)
        },
    ] {
        let expected = if bad.context_digest != CONTEXT {
            ReservationError::ContextMismatch
        } else {
            ReservationError::InvalidRequest
        };
        reject_unchanged(&mut l, &bad, &liquid(100, 100), expected);
    }
    let mut bad = sponsor(1, 0);
    bad.action_debits
        .push(debit(ALICE, Denomination::Udgt, 0, DebitKind::Outflow));
    reject_unchanged(
        &mut l,
        &bad,
        &liquid(100, 100),
        ReservationError::SponsorActionDebits,
    );
    assert_eq!(
        l.reserve(&ordinary(1, 0), &liquid(10, 0)),
        Ok(ReservationStatus::Added)
    );
}
#[test]
fn aggregate_overflow_and_stale_other_owner_liquidity_reject_atomically() {
    let mut l = ReservationLedger::new(
        CONTEXT,
        QueueLimits {
            max_wire_bytes: u64::MAX,
            max_signature_work: u64::MAX,
            ..limits()
        },
    )
    .unwrap();
    let first = ReservationRequest {
        fee_cap_udrt: u128::MAX,
        wire_bytes: u64::MAX,
        signature_work: u64::MAX,
        ..ordinary(1, 0)
    };
    l.reserve(&first, &liquid(u128::MAX, 0)).unwrap();
    reject_unchanged(
        &mut l,
        &sponsor(2, 0),
        &liquid(u128::MAX, 0),
        ReservationError::Overflow,
    );
    let mut l = ledger();
    let first = ReservationRequest {
        fee_cap_udrt: u128::MAX,
        ..ordinary(1, 0)
    };
    l.reserve(&first, &liquid(u128::MAX, 0)).unwrap();
    reject_unchanged(
        &mut l,
        &sponsor(2, 0),
        &liquid(u128::MAX, 0),
        ReservationError::Overflow,
    );
    let mut l = ledger();
    l.reserve(&ordinary(1, 0), &liquid(10, 10)).unwrap();
    let mut bob = sponsor(1, 0);
    bob.payer = BOB;
    reject_unchanged(
        &mut l,
        &bob,
        &liquid(9, 10),
        ReservationError::InsufficientLiquidity,
    );
}

#[test]
fn staking_permitted_locked_dgt_cannot_cover_aggregate_unrestricted_sends() {
    let mut funds = liquid(100, 100);
    funds
        .unrestricted
        .insert(asset(ALICE, Denomination::Udgt), 10);
    let mut first = ordinary(1, 0);
    first.action_debits = vec![debit(ALICE, Denomination::Udgt, 6, DebitKind::Outflow)];
    first.unrestricted_debits = first.action_debits.clone();
    let mut second = ordinary(2, 1);
    second.action_debits = first.action_debits.clone();
    second.unrestricted_debits = second.action_debits.clone();
    let mut l = ledger();
    l.reserve(&first, &funds).unwrap();
    reject_unchanged(
        &mut l,
        &second,
        &funds,
        ReservationError::InsufficientLiquidity,
    );
    assert_eq!(
        l.reserved(asset(ALICE, Denomination::Udgt)).unwrap().action,
        6
    );
    assert_eq!(
        l.reserved_unrestricted(asset(ALICE, Denomination::Udgt))
            .unwrap()
            .action,
        6
    );
    assert!(l.evict(first.id).unwrap());
    assert!(!l.evict(first.id).unwrap());
    l.reserve(&second, &funds).unwrap();
}
#[test]
fn locked_bond_and_unrestricted_send_enforce_both_constraints_and_release_atomically() {
    let mut funds = liquid(100, 100);
    funds
        .unrestricted
        .insert(asset(ALICE, Denomination::Udgt), 10);
    let mut bond = ordinary(1, 0);
    bond.action_debits = vec![debit(ALICE, Denomination::Udgt, 90, DebitKind::Outflow)];
    let mut send = ordinary(2, 1);
    send.action_debits = vec![debit(ALICE, Denomination::Udgt, 10, DebitKind::Outflow)];
    send.unrestricted_debits = send.action_debits.clone();
    let mut l = ledger();
    l.reserve(&bond, &funds).unwrap();
    l.reserve(&send, &funds).unwrap();
    assert_eq!(
        l.reserved(asset(ALICE, Denomination::Udgt)).unwrap().action,
        100
    );
    assert_eq!(
        l.reserved_unrestricted(asset(ALICE, Denomination::Udgt))
            .unwrap()
            .action,
        10
    );
    assert_eq!(
        l.reserved_unrestricted(asset(ALICE, Denomination::Udrt))
            .unwrap()
            .fee,
        20
    );
    let mut extra = ordinary(3, 2);
    extra.action_debits = vec![debit(ALICE, Denomination::Udgt, 1, DebitKind::Outflow)];
    reject_unchanged(
        &mut l,
        &extra,
        &funds,
        ReservationError::InsufficientLiquidity,
    );
    l.evict(bond.id).unwrap();
    l.reserve(&extra, &funds).unwrap();
    assert_eq!(
        l.reserved_unrestricted(asset(ALICE, Denomination::Udgt))
            .unwrap()
            .action,
        10
    );
    l.evict(send.id).unwrap();
    assert_eq!(
        l.reserved_unrestricted(asset(ALICE, Denomination::Udgt))
            .unwrap()
            .action,
        0
    );
}
#[test]
fn unrestricted_owner_bound_self_transfer_peak_and_fee_cannot_use_locked_liquidity() {
    let mut funds = liquid(100, 100);
    funds.unrestricted.insert(asset(BOB, Denomination::Udgt), 7);
    let mut claim = ordinary(1, 0);
    claim.action_debits = vec![
        debit(BOB, Denomination::Udgt, 7, DebitKind::SelfTransfer),
        debit(BOB, Denomination::Udgt, 7, DebitKind::SelfTransfer),
    ];
    claim.unrestricted_debits = claim.action_debits.clone();
    let mut l = ledger();
    l.reserve(&claim, &funds).unwrap();
    assert_eq!(
        l.reserved_unrestricted(asset(BOB, Denomination::Udgt))
            .unwrap()
            .action,
        7
    );
    let mut transfer = ordinary(2, 1);
    transfer.action_debits = vec![debit(BOB, Denomination::Udgt, 1, DebitKind::Outflow)];
    transfer.unrestricted_debits = transfer.action_debits.clone();
    reject_unchanged(
        &mut l,
        &transfer,
        &funds,
        ReservationError::InsufficientLiquidity,
    );
    funds
        .unrestricted
        .insert(asset(ALICE, Denomination::Udrt), 19);
    reject_unchanged(
        &mut l,
        &sponsor(1, 0),
        &funds,
        ReservationError::InsufficientLiquidity,
    );
    assert_eq!(
        l.reserved_unrestricted(asset(ALICE, Denomination::Udrt))
            .unwrap()
            .fee,
        10
    );
}
#[test]
fn invalid_unrestricted_classification_or_eligibility_cannot_mutate_reservations() {
    let mut l = ledger();
    let mut request = ordinary(1, 0);
    request.action_debits = vec![debit(ALICE, Denomination::Udgt, 5, DebitKind::Outflow)];
    request.unrestricted_debits = vec![debit(ALICE, Denomination::Udgt, 6, DebitKind::Outflow)];
    reject_unchanged(
        &mut l,
        &request,
        &liquid(100, 100),
        ReservationError::InvalidDebitSubset,
    );
    request.unrestricted_debits = vec![request.action_debits[0]; 2];
    reject_unchanged(
        &mut l,
        &request,
        &liquid(100, 100),
        ReservationError::InvalidDebitSubset,
    );
    request.unrestricted_debits = request.action_debits.clone();
    let mut funds = liquid(100, 100);
    funds
        .unrestricted
        .insert(asset(ALICE, Denomination::Udgt), 101);
    reject_unchanged(
        &mut l,
        &request,
        &funds,
        ReservationError::InvalidEligibility,
    );
    l.reserve(&request, &liquid(100, 100)).unwrap();
    let before = l.clone();
    request.unrestricted_debits.clear();
    assert_eq!(
        l.reserve(&request, &liquid(100, 100)),
        Err(ReservationError::IdentityMismatch)
    );
    assert_eq!(l, before);
}
