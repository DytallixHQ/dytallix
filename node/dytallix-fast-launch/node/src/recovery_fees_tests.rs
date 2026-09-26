use super::*;
use dytallix_protocol_types::recovery::{
    Action, ActiveAuthorization, AuthenticatedFacts, Guardian, RecoveryAuthorization,
    RecoveryConfig, RecoveryDomain, RecoveryPolicy,
};

fn key(byte: u8) -> KeyIdentity {
    KeyIdentity {
        algorithm: "mldsa65".into(),
        public_key: vec![byte; 1952],
    }
}
fn profile() -> FeeProfile {
    FeeProfile {
        version: 1,
        activation_height: 1,
        denomination: "udrt".into(),
        gas_price: 2,
        minimum_gas: 10,
        max_transaction_gas: 1_000_000,
        max_block_gas: 4_000_000,
        max_block_recovery_bytes: 1_000_000,
        max_block_recovery_signatures: 64,
        max_fee_cap: 2_000_000,
        max_pending_accounts: 2,
        max_due_expiry_events_per_height: 2,
        mandatory_expiry_gas_budget: 200,
        expiry_event_gas_cost: 100,
        action_costs: [10; 9],
        wire_byte_cost: 1,
        read_byte_cost: 1,
        write_byte_cost: 1,
        signature_costs: BTreeMap::from([("mldsa65".into(), 1)]),
    }
}
fn account(id: u8) -> RecoveryAccount {
    RecoveryAccount {
        address: format!("account-{id}"),
        recovery: RecoveryState::new(
            RecoveryDomain {
                network: 3,
                chain_id: "local".into(),
                genesis_digest: [3; 32],
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
            key(id),
            0,
        )
        .unwrap(),
        sponsor_nonce: 0,
    }
}
fn enrolled(mut account: RecoveryAccount) -> RecoveryAccount {
    let guardians = vec![
        Guardian {
            key: key(10),
            control_group: "A".into(),
        },
        Guardian {
            key: key(11),
            control_group: "B".into(),
        },
        Guardian {
            key: key(12),
            control_group: "C".into(),
        },
    ];
    let action = Action {
        submission_expiry: 90,
        kind: ActionKind::Enroll {
            active: ActiveAuthorization {
                generation: 0,
                nonce: 0,
            },
            policy: RecoveryPolicy {
                threshold: 2,
                guardians: guardians.clone(),
            },
        },
    };
    let facts = AuthenticatedFacts {
        domain: account.recovery.domain.clone(),
        action: action.clone(),
        signers: vec![account.recovery.active_key.clone()],
        proofs: guardians.into_iter().map(|g| g.key).collect(),
    };
    account.recovery = account.recovery.transition(0, &action, &facts).unwrap();
    account
}
fn start(account: &mut RecoveryAccount) {
    let action = Action {
        submission_expiry: 90,
        kind: ActionKind::Start {
            recovery: RecoveryAuthorization {
                policy_version: account.recovery.policy_version,
                sequence: account.recovery.recovery_sequence,
            },
            request_id: [8; 32],
            replacement: key(20),
            timing_version: 1,
        },
    };
    let facts = AuthenticatedFacts {
        domain: account.recovery.domain.clone(),
        action: action.clone(),
        signers: vec![key(10), key(11)],
        proofs: vec![key(20)],
    };
    account.recovery = account
        .recovery
        .transition(account.recovery.last_height, &action, &facts)
        .unwrap();
}
#[test]
fn fresh_book_roundtrip_and_mapping_rejection() {
    let book = RecoveryBook::new(profile(), vec![account(1), account(2)]).unwrap();
    assert_eq!(RecoveryBook::decode(&book.encode().unwrap()).unwrap(), book);
    let mut duplicate = account(2);
    duplicate.address = account(1).address;
    assert!(RecoveryBook::new(profile(), vec![account(1), duplicate]).is_err());
    let mut different = account(2);
    different.recovery.domain.genesis_digest = [9; 32];
    assert!(RecoveryBook::new(profile(), vec![account(1), different]).is_err());
    let mut missing_history = book.clone();
    missing_history
        .accounts
        .get_mut(&hex::encode([2; 32]))
        .unwrap()
        .sponsor_nonce = 1;
    assert!(missing_history.validate().is_err());
    let mut whitespace = book.encode().unwrap();
    whitespace.push(b' ');
    assert!(RecoveryBook::decode(&whitespace).is_err());
}
#[test]
fn logical_meter_uses_fixed_width_counters_and_counts_policy_bytes() {
    let mut a = account(1);
    let size = logical_account_bytes(&a).unwrap();
    a.sponsor_nonce = u64::MAX;
    a.recovery.spending_nonce = u64::MAX;
    assert_eq!(logical_account_bytes(&a).unwrap(), size);
    let with_policy = enrolled(account(1));
    assert_eq!(
        logical_account_bytes(&with_policy).unwrap() - size,
        3 + 3 * (3 + 1952 + 2 + 1)
    );
}
#[test]
fn mandatory_expiry_is_reserved_and_applied_without_transaction_or_fee() {
    let initial = RecoveryBook::new(profile(), vec![enrolled(account(1)), account(2)]).unwrap();
    let mut block = initial.begin_block(1).unwrap();
    start(block.book.accounts.get_mut(&hex::encode([1; 32])).unwrap());
    block.book.expiry_index = block.book.expected_expiries().unwrap();
    block.book.validate().unwrap();
    assert_eq!(
        block.book.expiry_index.keys().copied().collect::<Vec<_>>(),
        vec![6]
    );
    let mut book = block.book;
    for height in 2..=6 {
        let next = book.begin_block(height).unwrap();
        assert_eq!(next.expiry_gas_used, if height == 6 { 100 } else { 0 });
        book = next.book;
    }
    let target = &book.accounts[&hex::encode([1; 32])];
    assert!(!target.recovery.outgoing_allowed());
    assert!(target.recovery.pending_recovery.is_none());
    assert!(book.expiry_index.is_empty());
    assert!(book.sponsor_receipts.is_empty());
    assert_eq!(book.accounts[&hex::encode([2; 32])].sponsor_nonce, 0);
    assert_eq!(initial.last_height, 0);
}
#[test]
fn inconsistent_or_oversubscribed_expiry_index_is_integrity_failure() {
    let mut book = RecoveryBook::new(profile(), vec![enrolled(account(1)), enrolled(account(2))])
        .unwrap()
        .begin_block(1)
        .unwrap()
        .book;
    for a in book.accounts.values_mut() {
        start(a);
    }
    assert!(book.validate().is_err());
    book.expiry_index = book.expected_expiries().unwrap();
    book.validate().unwrap();
    book.profile.max_due_expiry_events_per_height = 1;
    assert!(book.validate().is_err());
    book.profile.max_due_expiry_events_per_height = 2;
    book.profile.mandatory_expiry_gas_budget = 100;
    assert!(book.validate().is_err());
}
#[test]
fn rejected_framing_consumes_block_work_and_excess_is_fatal() {
    let mut p = profile();
    p.max_block_recovery_bytes = 4;
    let mut block = RecoveryBook::new(p, vec![account(1), account(2)])
        .unwrap()
        .begin_block(1)
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut settlement = Settlement::new(std::sync::Arc::new(
        crate::storage::state::Storage::open(dir.path().join("db")).unwrap(),
    ));
    let rejected = block.execute(1, 0, b"bad", &mut settlement).unwrap();
    assert!(!rejected.charged);
    assert_eq!(rejected.gas_used, 3);
    assert_eq!(block.bytes_used, 3);
    assert!(block.execute(1, 1, b"bad", &mut settlement).is_err());
    assert!(block.book.sponsor_receipts.is_empty());
    assert!(settlement.writes().unwrap().is_empty());
}
#[test]
fn shared_queue_reserves_once_and_rejects_duplicate_nonce_and_overspend() {
    let mut queue = RecoveryReservations::default();
    queue.reserve_ordinary("payer", 30, 100).unwrap();
    assert!(queue.reserve_sponsor([1; 32], "payer", 0, 50, 100).unwrap());
    assert!(!queue.reserve_sponsor([1; 32], "payer", 0, 50, 100).unwrap());
    assert!(queue.reserve_sponsor([2; 32], "payer", 0, 10, 100).is_err());
    assert!(queue.reserve_sponsor([2; 32], "payer", 1, 21, 100).is_err());
    assert!(queue.reserve_ordinary("payer", 21, 100).is_err());
    assert!(queue.reserve_sponsor([2; 32], "payer", 1, 20, 100).unwrap());
    assert_eq!(queue.reserved("payer").unwrap(), 100);
    assert!(queue.reserve_sponsor([1; 32], "payer", 0, 1, 100).is_err());
}
#[test]
fn sponsored_charge_preserves_ordinary_nonce_and_updates_same_custody() {
    let dir = tempfile::tempdir().unwrap();
    let storage =
        std::sync::Arc::new(crate::storage::state::Storage::open(dir.path().join("db")).unwrap());
    let mut state = crate::state::State::new(storage.clone());
    state.set_balance("payer", "udrt", 1000);
    let mut settlement = Settlement::new(storage);
    settlement.account("payer").unwrap().nonce = 7;
    settlement.charge_sponsored("payer", 12).unwrap();
    assert_eq!(settlement.account("payer").unwrap().nonce, 7);
    assert_eq!(settlement.account("payer").unwrap().balance_of("udrt"), 988);
    let writes = settlement.writes().unwrap();
    let custody: u128 =
        bincode::deserialize(writes.get(crate::settlement::FEE_KEY.as_bytes()).unwrap()).unwrap();
    assert_eq!(custody, 12);
}

#[test]
fn charged_recovery_receipt_reconciles_each_native_delta() {
    let dir = tempfile::tempdir().unwrap();
    let storage =
        std::sync::Arc::new(crate::storage::state::Storage::open(dir.path().join("db")).unwrap());
    let mut before = Settlement::new(storage);
    before
        .account("account-2")
        .unwrap()
        .set_balance("udrt", 100);
    before.account("account-1").unwrap().set_balance("udgt", 30);
    let mut after = before.clone();
    after.charge_sponsored("account-2", 20).unwrap();
    let sponsor = account(2);
    let mut receipt = SponsorReceipt {
        operation_id: [3; 32],
        sponsor_authorization_id: [4; 32],
        envelope_hash: [5; 32],
        sponsor_account_id: [2; 32],
        target_account_id: [1; 32],
        block_height: 1,
        block_index: 0,
        success: true,
        profile_version: 1,
        profile_digest: wire::profile_digest(&profile()).unwrap(),
        gas_limit: 100,
        gas_used: 10,
        reserved_cap: 100,
        settled_fee: 20,
        released_reserve: 80,
        sponsor_counter_before: 0,
        sponsor_counter_after: 1,
        target_state_digest: [6; 32],
        record_hash: [0; 32],
    };
    receipt.record_hash = receipt.hash().unwrap();
    for success in [true, false] {
        receipt.success = success;
        assert!(reconcile_sponsor_charge(&mut before, &mut after, &sponsor, &receipt, 2).is_ok());
    }
    let mut changed = after.clone();
    changed.account("account-2").unwrap().nonce = 1;
    assert!(reconcile_sponsor_charge(&mut before, &mut changed, &sponsor, &receipt, 2).is_err());
    let mut changed = after.clone();
    changed
        .account("account-1")
        .unwrap()
        .set_balance("udgt", 29);
    assert!(reconcile_sponsor_charge(&mut before, &mut changed, &sponsor, &receipt, 2).is_err());
    let mut changed = after.clone();
    changed.fee_total = Some(21);
    assert!(reconcile_sponsor_charge(&mut before, &mut changed, &sponsor, &receipt, 2).is_err());
    receipt.released_reserve = 79;
    assert!(reconcile_sponsor_charge(&mut before, &mut after, &sponsor, &receipt, 2).is_err());
}

#[test]
fn future_fee_activation_allows_blocks_but_rejects_requests_without_charge() {
    use dytallix_protocol_types::recovery_sponsor::SponsorAuthorization;
    use dytallix_protocol_types::recovery_wire::{
        RecoveryOperation, RecoverySignature, SignatureRole, SignedRecovery,
    };
    let mut p = profile();
    p.activation_height = 3;
    let target = account(1);
    let sponsor = account(2);
    let recovery = SignedRecovery {
        operation: RecoveryOperation {
            domain: target.recovery.domain.clone(),
            action: Action {
                submission_expiry: 90,
                kind: ActionKind::Finalize {
                    recovery: RecoveryAuthorization {
                        policy_version: 0,
                        sequence: 0,
                    },
                    request_id: [8; 32],
                },
            },
        },
        signatures: vec![RecoverySignature {
            role: SignatureRole::Operation,
            key: key(1),
            signature: vec![0; 3309],
        }],
    };
    // Canonical framing fixture. Activation rejection occurs before signature checks.
    let authorization = SponsorAuthorization {
        domain: target.recovery.domain.clone(),
        recovery_version: 1,
        operation_id: wire::operation_id(&recovery.operation).unwrap(),
        signer_manifest_digest: wire::signer_manifest_digest(&recovery).unwrap(),
        sponsor_account_id: sponsor.recovery.domain.account_id,
        sponsor_generation: 0,
        sponsor_nonce: 0,
        sponsor_key: sponsor.recovery.active_key.clone(),
        fee_profile_version: p.version,
        fee_profile_digest: wire::profile_digest(&p).unwrap(),
        denomination: "udrt".into(),
        maximum_charge: 2_000_000,
        gas_limit: 1_000_000,
        expiry_height: 90,
    };
    let raw = wire::encode(&SponsoredRecovery {
        recovery,
        sponsor: authorization,
        signature: vec![0; 3309],
    })
    .unwrap();
    let mut book = RecoveryBook::new(p, vec![target, sponsor]).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut settlement = Settlement::new(std::sync::Arc::new(
        crate::storage::state::Storage::open(dir.path().join("db")).unwrap(),
    ));
    for height in 1..=3 {
        let mut block = book.begin_block(height).unwrap();
        let before = block.book.clone();
        let result = block.execute(height, 0, &raw, &mut settlement).unwrap();
        assert!(!result.charged);
        assert_eq!(
            result.error.as_deref() == Some("Recovery fee profile not active"),
            height < 3
        );
        assert_eq!(block.book, before);
        assert!(settlement.writes().unwrap().is_empty());
        book = block.book;
    }
    assert_eq!(book.last_height, 3);
}

#[test]
fn reservation_arithmetic_rejects_overflow_without_consuming_capacity() {
    let mut queue = RecoveryReservations::default();
    queue
        .reserve_ordinary("payer", u128::MAX - 10, u128::MAX)
        .unwrap();
    queue
        .reserve_sponsor([1; 32], "payer", 0, 10, u128::MAX)
        .unwrap();
    assert_eq!(queue.reserved("payer").unwrap(), u128::MAX);
    assert!(queue
        .reserve_sponsor([2; 32], "payer", 1, 1, u128::MAX)
        .is_err());
    assert!(queue.reserve_ordinary("payer", 1, u128::MAX).is_err());
    assert_eq!(queue.reserved("payer").unwrap(), u128::MAX);
    // Rejected operations reserve neither identity nor nonce.
    assert!(queue
        .reserve_sponsor([2; 32], "payer", 1, 0, u128::MAX)
        .unwrap());
    assert!(!queue
        .reserve_sponsor([2; 32], "payer", 1, 0, u128::MAX)
        .unwrap());
    assert!(queue
        .reserve_sponsor([2; 32], "other", 1, 0, u128::MAX)
        .is_err());
    assert_eq!(queue.reserved("other").unwrap(), 0);
}

#[test]
fn fee_custody_overflow_and_insufficient_funds_preserve_staged_balances() {
    let dir = tempfile::tempdir().unwrap();
    let storage =
        std::sync::Arc::new(crate::storage::state::Storage::open(dir.path().join("db")).unwrap());
    let mut state = crate::state::State::new(storage.clone());
    state.set_balance("payer", "udrt", u128::MAX);
    storage
        .db
        .put(
            crate::settlement::FEE_KEY,
            bincode::serialize(&(u128::MAX - 1)).unwrap(),
        )
        .unwrap();
    let mut settlement = Settlement::new(storage.clone());
    settlement.charge_sponsored("payer", 1).unwrap();
    let before = settlement.writes().unwrap();
    assert_eq!(
        settlement.account("payer").unwrap().balance_of("udrt"),
        u128::MAX - 1
    );
    assert!(settlement.charge_sponsored("payer", 1).is_err());
    assert_eq!(settlement.writes().unwrap(), before);
    let mut insufficient = Settlement::new(storage);
    insufficient
        .account("payer")
        .unwrap()
        .set_balance("udrt", 0);
    let before = insufficient.writes().unwrap();
    assert!(insufficient.charge_sponsored("payer", 1).is_err());
    assert_eq!(insufficient.writes().unwrap(), before);
}

#[test]
fn exact_rejected_work_limits_and_meter_overflow_have_no_state_effects() {
    for constrain_bytes in [false, true] {
        let mut p = profile();
        p.minimum_gas = 1;
        p.max_transaction_gas = 10;
        p.max_block_gas = if constrain_bytes { 100 } else { 20 };
        p.max_block_recovery_bytes = if constrain_bytes { 10 } else { 100 };
        p.wire_byte_cost = 2;
        let mut block = RecoveryBook::new(p, vec![account(1), account(2)])
            .unwrap()
            .begin_block(1)
            .unwrap();
        let before = block.book.clone();
        let dir = tempfile::tempdir().unwrap();
        let mut settlement = Settlement::new(std::sync::Arc::new(
            crate::storage::state::Storage::open(dir.path().join("db")).unwrap(),
        ));
        for index in 0..2 {
            let r = block.execute(1, index, b"wrong", &mut settlement).unwrap();
            assert!(!r.charged);
            assert_eq!(r.gas_used, 10);
        }
        assert_eq!((block.gas_used, block.bytes_used), (20, 10));
        assert!(block.execute(1, 2, b"x", &mut settlement).is_err());
        assert_eq!((block.gas_used, block.bytes_used), (20, 10));
        assert_eq!(block.book, before);
        assert!(settlement.writes().unwrap().is_empty());
    }
    let mut p = profile();
    p.wire_byte_cost = u64::MAX;
    let mut block = RecoveryBook::new(p, vec![account(1), account(2)])
        .unwrap()
        .begin_block(1)
        .unwrap();
    let before = block.book.clone();
    let dir = tempfile::tempdir().unwrap();
    let mut settlement = Settlement::new(std::sync::Arc::new(
        crate::storage::state::Storage::open(dir.path().join("db")).unwrap(),
    ));
    assert!(block.execute(1, 0, b"xx", &mut settlement).is_err());
    assert_eq!((block.gas_used, block.bytes_used), (0, 0));
    assert_eq!(block.book, before);
    assert!(settlement.writes().unwrap().is_empty());
}

#[test]
fn retained_history_rejects_unreachable_exhausted_sponsor_counters() {
    let initial = RecoveryBook::new(profile(), vec![account(1), account(2)]).unwrap();
    // Every nonce increment requires a retained receipt, and the book retains
    // at most MAX_RECEIPTS. A u64::MAX durable nonce is not a reachable state.
    assert!((MAX_RECEIPTS as u128) < u128::from(u64::MAX));
    for nonce in [MAX_RECEIPTS as u64 + 1, u64::MAX] {
        let mut corrupt = initial.clone();
        corrupt
            .accounts
            .get_mut(&hex::encode([2; 32]))
            .unwrap()
            .sponsor_nonce = nonce;
        assert!(corrupt.validate().is_err());
        assert!(RecoveryBook::decode(&serde_json::to_vec(&corrupt).unwrap()).is_err());
    }
    assert_eq!(initial.begin_block(1).unwrap().book.last_height, 1);
}

fn ordinary_shared_profile() -> dytallix_protocol_types::ordinary_fees::FeeProfile {
    use dytallix_protocol_types::ordinary::{Denomination, Limits};
    dytallix_protocol_types::ordinary_fees::FeeProfile {
        ordinary_fee_contract_version: 1,
        version: 1,
        activation_height: 1,
        denomination: Denomination::Udrt,
        gas_price: 2,
        minimum_gas: 1,
        max_transaction_gas: 1_000_000,
        max_block_transaction_gas: 4_000_000,
        max_block_transaction_bytes: 1_000_000,
        max_block_signature_checks: 64,
        max_fee_cap: 2_000_000,
        limits: Limits {
            max_wire_bytes: 65_536,
            max_actions: 16,
            max_identifier_bytes: 64,
            max_data_bytes: 1024,
            max_memo_bytes: 128,
            max_consensus_key_bytes: 4096,
            max_proof_bytes: 8192,
            max_expiry_lifetime: 100,
            allowed_algorithms: BTreeSet::from(["mldsa65".into()]),
        },
        transaction_overhead: 2,
        receipt_metadata_cost: 5,
        wire_byte_cost: 1,
        read_byte_cost: 1,
        write_byte_cost: 1,
        action_costs: [7; 12],
        signature_costs: BTreeMap::from([("mldsa65".into(), 3)]),
        validator_proof_profile_digest: [9; 32],
        validator_proof_costs: BTreeMap::from([("mldsa65".into(), 11)]),
    }
}
fn recovery_shared_meter(
    profile: &dytallix_protocol_types::ordinary_fees::FeeProfile,
) -> crate::ordinary_meter::SharedBlockMeter {
    crate::ordinary_meter::SharedBlockMeter::new(
        profile,
        crate::ordinary_meter::RecoveryCeilings {
            max_gas: 4_000_000,
            max_bytes: 1_000_000,
            max_signatures: 64,
            mandatory_expiry_gas: 200,
        },
    )
    .unwrap()
}
#[test]
fn combined_recovery_rejected_work_uses_shared_capacity_without_native_effects() {
    let mut ordinary = ordinary_shared_profile();
    ordinary.max_block_transaction_bytes = 65_536;
    let mut shared = recovery_shared_meter(&ordinary);
    shared.rejected_wire(65_531, &ordinary).unwrap();
    let mut block = RecoveryBook::new(profile(), vec![account(1), account(2)])
        .unwrap()
        .begin_block(1)
        .unwrap();
    let before = block.book.clone();
    let dir = tempfile::tempdir().unwrap();
    let mut settlement = Settlement::new(std::sync::Arc::new(
        crate::storage::state::Storage::open(dir.path().join("db")).unwrap(),
    ));
    let result = block
        .execute_with_shared(1, 0, b"wrong", &mut settlement, &mut shared)
        .unwrap();
    assert!(!result.charged);
    assert_eq!(
        (
            shared.usage().gas,
            shared.usage().bytes,
            shared.usage().signatures
        ),
        (65_536, 65_536, 0)
    );
    assert_eq!(
        (
            shared.recovery_usage().gas,
            shared.recovery_usage().bytes,
            shared.recovery_usage().signatures
        ),
        (5, 5, 0)
    );
    assert_eq!(shared.expiry_gas(), 0);
    let error = block
        .execute_with_shared(1, 1, b"x", &mut settlement, &mut shared)
        .unwrap_err();
    assert_eq!(
        error.downcast_ref::<crate::ordinary_meter::MeterError>(),
        Some(&crate::ordinary_meter::MeterError::BlockCapacity)
    );
    assert_eq!(
        (block.gas_used, block.bytes_used, block.signatures_used),
        (5, 5, 0)
    );
    assert_eq!(block.book, before);
    assert!(settlement.writes().unwrap().is_empty());
}
#[test]
fn combined_recovery_signature_ceiling_stops_before_verification_and_preserves_legacy_result() {
    use dytallix_protocol_types::recovery_sponsor::SponsorAuthorization;
    use dytallix_protocol_types::recovery_wire::{
        RecoveryOperation, RecoverySignature, SignatureRole, SignedRecovery,
    };
    let p = profile();
    let target = account(1);
    let sponsor = account(2);
    let recovery = SignedRecovery {
        operation: RecoveryOperation {
            domain: target.recovery.domain.clone(),
            action: Action {
                submission_expiry: 90,
                kind: ActionKind::Finalize {
                    recovery: RecoveryAuthorization {
                        policy_version: 0,
                        sequence: 0,
                    },
                    request_id: [8; 32],
                },
            },
        },
        signatures: vec![RecoverySignature {
            role: SignatureRole::Operation,
            key: key(1),
            signature: vec![0; 3309],
        }],
    };
    let authorization = SponsorAuthorization {
        domain: target.recovery.domain.clone(),
        recovery_version: 1,
        operation_id: wire::operation_id(&recovery.operation).unwrap(),
        signer_manifest_digest: wire::signer_manifest_digest(&recovery).unwrap(),
        sponsor_account_id: sponsor.recovery.domain.account_id,
        sponsor_generation: 0,
        sponsor_nonce: 0,
        sponsor_key: sponsor.recovery.active_key.clone(),
        fee_profile_version: p.version,
        fee_profile_digest: wire::profile_digest(&p).unwrap(),
        denomination: "udrt".into(),
        maximum_charge: 2_000_000,
        gas_limit: 1_000_000,
        expiry_height: 90,
    };
    let raw = wire::encode(&SponsoredRecovery {
        recovery,
        sponsor: authorization,
        signature: vec![0; 3309],
    })
    .unwrap();
    let mut block = RecoveryBook::new(p, vec![target, sponsor])
        .unwrap()
        .begin_block(1)
        .unwrap();
    let before = block.book.clone();
    let mut legacy = block.clone();
    let dir = tempfile::tempdir().unwrap();
    let mut settlement = Settlement::new(std::sync::Arc::new(
        crate::storage::state::Storage::open(dir.path().join("db")).unwrap(),
    ));
    let mut ordinary = ordinary_shared_profile();
    ordinary.max_block_signature_checks = 1;
    let mut limited = recovery_shared_meter(&ordinary);
    let error = block
        .execute_with_shared(1, 0, &raw, &mut settlement, &mut limited)
        .unwrap_err();
    assert_eq!(
        error.downcast_ref::<crate::ordinary_meter::MeterError>(),
        Some(&crate::ordinary_meter::MeterError::BlockCapacity)
    );
    assert_eq!(block.signatures_used, 0);
    assert_eq!(limited.usage().signatures, 0);
    assert_eq!(block.book, before);
    assert!(settlement.writes().unwrap().is_empty());
    let mut combined = legacy.clone();
    ordinary.max_block_signature_checks = 2;
    let mut sufficient = recovery_shared_meter(&ordinary);
    let expected = legacy.execute(1, 0, &raw, &mut settlement).unwrap();
    let actual = combined
        .execute_with_shared(1, 0, &raw, &mut settlement, &mut sufficient)
        .unwrap();
    assert!(!actual.charged);
    assert_eq!(actual, expected);
    assert_eq!(sufficient.usage().signatures, 2);
    assert_eq!(sufficient.usage().gas, combined.gas_used);
    assert_eq!(sufficient.usage().bytes, raw.len() as u64);
    assert_eq!(combined.book, before);
    assert!(settlement.writes().unwrap().is_empty());
}
