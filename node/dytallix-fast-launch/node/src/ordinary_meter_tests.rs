use super::*;
use dytallix_protocol_types::{
    ordinary::{Denomination, Limits},
    recovery::{KeyIdentity, RecoveryDomain},
};
fn profile() -> FeeProfile {
    FeeProfile {
        ordinary_fee_contract_version: 1,
        version: 1,
        activation_height: 1,
        denomination: Denomination::Udrt,
        gas_price: 2,
        minimum_gas: 1,
        max_transaction_gas: 100_000,
        max_block_transaction_gas: 300_000,
        max_block_transaction_bytes: 200_000,
        max_block_signature_checks: 20,
        max_fee_cap: 1_000_000,
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
        wire_byte_cost: 0,
        read_byte_cost: 1,
        write_byte_cost: 1,
        action_costs: [7; 12],
        signature_costs: BTreeMap::from([("mldsa65".into(), 3)]),
        validator_proof_profile_digest: [9; 32],
        validator_proof_costs: BTreeMap::from([("mldsa65".into(), 11)]),
    }
}
fn ceilings() -> RecoveryCeilings {
    RecoveryCeilings {
        max_gas: 100_000,
        max_bytes: 100_000,
        max_signatures: 10,
        mandatory_expiry_gas: 50,
    }
}
fn body(gas: u64) -> OrdinaryTransaction {
    OrdinaryTransaction {
        domain: RecoveryDomain {
            network: 3,
            chain_id: "meter-test".into(),
            genesis_digest: [1; 32],
            account_id: [2; 32],
        },
        authorization_generation: 0,
        spending_nonce: 0,
        key: KeyIdentity {
            algorithm: "mldsa65".into(),
            public_key: vec![3; 1952],
        },
        expiry_height: 20,
        ordinary_fee_contract_version: 1,
        fee_profile_version: 1,
        fee_profile_digest: [8; 32],
        fee_denomination: Denomination::Udrt,
        maximum_fee: 200_000,
        gas_limit: gas,
        memo: String::new(),
        actions: vec![Action::Data { data: "x".into() }, Action::RewardClaim],
    }
}
fn record(key: &[u8], size: usize) -> LogicalRecord {
    LogicalRecord::new(
        key,
        &[LogicalField {
            id: 1,
            value: LogicalValue::Bytes(vec![4; size]),
        }],
        4096,
    )
    .unwrap()
}
#[test]
fn metadata_reserved_once_and_exact_action_budget_exhausts_terminally() {
    let p = profile();
    let mut block = SharedBlockMeter::new(&p, ceilings()).unwrap();
    let mut meter = OrdinaryMeter::new(&p, &body(17), &mut block).unwrap();
    meter.ordinary_signature().unwrap();
    meter.accept().unwrap();
    meter.action(0).unwrap();
    assert_eq!(meter.action(1), Err(MeterError::AcceptedOutOfGas));
    let result = meter.finish().unwrap();
    assert!(result.accepted() && result.exhausted());
    assert_eq!(result.used_gas(), 17);
    assert_eq!(result.metadata_gas(), 5);
    assert_eq!(result.processed_actions(), 1);
    assert_eq!(block.usage().gas, 17);
    let mut block = SharedBlockMeter::new(&p, ceilings()).unwrap();
    let mut meter = OrdinaryMeter::new(&p, &body(9), &mut block).unwrap();
    meter.ordinary_signature().unwrap();
    assert!(matches!(
        meter.accept(),
        Err(MeterError::PreAcceptanceRejected(_))
    ));
    let result = meter.finish().unwrap();
    assert!(!result.accepted());
    assert_eq!(result.used_gas(), 10);
    assert_eq!(block.usage().gas, 10);
}
#[test]
fn successful_meter_counts_wire_signature_actions_and_metadata_exactly_once() {
    let mut p = profile();
    p.wire_byte_cost = 2;
    let b = body(100_000);
    let bytes = ordinary::encode(
        &SignedOrdinary {
            body: b.clone(),
            signature: vec![0; 3309],
        },
        &p.limits,
    )
    .unwrap()
    .len() as u64;
    let mut block = SharedBlockMeter::new(&p, ceilings()).unwrap();
    let mut meter = OrdinaryMeter::new(&p, &b, &mut block).unwrap();
    meter.ordinary_signature().unwrap();
    meter.accept().unwrap();
    meter.action(0).unwrap();
    meter.action(1).unwrap();
    let result = meter.finish().unwrap();
    assert_eq!(result.used_gas(), bytes * 2 + 2 + 3 + 5 + 14);
    assert_eq!(result.metadata_gas(), 5);
    assert_eq!(result.action_count(), 2);
    assert!(!result.exhausted());
    assert_eq!(
        block.usage(),
        ResourceUsage {
            gas: result.used_gas(),
            bytes,
            signatures: 1
        }
    );
}
#[test]
fn logical_reads_deduplicate_and_final_write_size_replaces_prior_proposal() {
    let p = profile();
    let mut block = SharedBlockMeter::new(&p, ceilings()).unwrap();
    let a = record(b"account", 20);
    let larger = record(b"account", 40);
    let smaller = record(b"account", 1);
    let mut meter = OrdinaryMeter::new(&p, &body(1000), &mut block).unwrap();
    meter.ordinary_signature().unwrap();
    meter.read(&a).unwrap();
    meter.read(&larger).unwrap();
    meter.accept().unwrap();
    meter.action(0).unwrap();
    meter.write(&a).unwrap();
    meter.write(&larger).unwrap();
    meter.write(&smaller).unwrap();
    meter.action(1).unwrap();
    let result = meter.finish().unwrap();
    assert_eq!(
        result.used_gas(),
        2 + 3 + 5 + 14 + a.byte_len() + smaller.byte_len()
    );
    assert_eq!(block.usage().gas, result.used_gas());
}
#[test]
fn failed_action_keeps_measured_writes_and_cannot_resume_after_exhaustion() {
    let p = profile();
    let mut block = SharedBlockMeter::new(&p, ceilings()).unwrap();
    let a = record(b"balance", 2);
    let mut meter = OrdinaryMeter::new(&p, &body(1000), &mut block).unwrap();
    meter.ordinary_signature().unwrap();
    meter.accept().unwrap();
    meter.action(0).unwrap();
    meter.write(&a).unwrap();
    // Caller discards action state on a typed application failure. Meter remains intact.
    let failure = meter.finish().unwrap();
    assert!(failure.accepted());
    assert_eq!(failure.used_gas(), 17 + a.byte_len());
    assert_eq!(failure.processed_actions(), 1);
    let mut meter = OrdinaryMeter::new(&p, &body(17), &mut block).unwrap();
    meter.ordinary_signature().unwrap();
    meter.accept().unwrap();
    meter.action(0).unwrap();
    assert_eq!(meter.write(&a), Err(MeterError::AcceptedOutOfGas));
    assert!(matches!(
        meter.write(&record(b"balance", 0)),
        Err(MeterError::Internal(_))
    ));
    assert!(meter.finish().is_err());
}
#[test]
fn signature_proofs_and_actions_require_exact_order_before_work() {
    let p = profile();
    let mut b = body(1000);
    b.actions = vec![
        Action::ValidatorRotateKey {
            validator_id: "v".into(),
            consensus_key: vec![1],
            proof: vec![2],
            proof_expiry_height: 20,
        },
        Action::Data { data: "x".into() },
    ];
    let mut block = SharedBlockMeter::new(&p, ceilings()).unwrap();
    let mut meter = OrdinaryMeter::new(&p, &b, &mut block).unwrap();
    meter.ordinary_signature().unwrap();
    meter.validator_proof(0, "mldsa65").unwrap();
    meter.accept().unwrap();
    meter.action(0).unwrap();
    meter.action(1).unwrap();
    let result = meter.finish().unwrap();
    assert_eq!(result.used_gas(), 35);
    assert_eq!(block.usage().signatures, 2);
    let mut block = SharedBlockMeter::new(&p, ceilings()).unwrap();
    let mut meter = OrdinaryMeter::new(&p, &b, &mut block).unwrap();
    meter.ordinary_signature().unwrap();
    assert!(matches!(meter.accept(), Err(MeterError::Internal(_))));
    assert!(meter.finish().is_err());
    let mut block = SharedBlockMeter::new(&p, ceilings()).unwrap();
    let mut meter = OrdinaryMeter::new(&p, &body(1000), &mut block).unwrap();
    meter.ordinary_signature().unwrap();
    meter.accept().unwrap();
    assert!(matches!(meter.action(1), Err(MeterError::Internal(_))));
    assert!(meter.finish().is_err());
}
#[test]
fn rejected_validation_work_persists_and_shared_and_recovery_ceilings_are_separate() {
    let mut p = profile();
    p.max_transaction_gas = 20;
    p.max_block_transaction_gas = 20;
    let mut block = SharedBlockMeter::new(&p, ceilings()).unwrap();
    {
        let mut meter = OrdinaryMeter::new(&p, &body(3), &mut block).unwrap();
        assert!(matches!(
            meter.ordinary_signature(),
            Err(MeterError::PreAcceptanceRejected(_))
        ));
        assert!(!meter.finish().unwrap().accepted());
    }
    assert_eq!(block.usage().gas, 5);
    block.charge_recovery(15, 10, 1).unwrap();
    assert_eq!(block.usage().gas, 20);
    block.charge_expiry(50).unwrap();
    assert_eq!(block.expiry_gas(), 50);
    assert_eq!(
        block.charge_recovery(1, 0, 0),
        Err(MeterError::BlockCapacity)
    );
    assert_eq!(block.usage().gas, 20);
    assert!(block.charge_recovery(0, 0, 0).is_err());
    let p = profile();
    let mut limits = ceilings();
    limits.max_gas = 3;
    let mut block = SharedBlockMeter::new(&p, limits).unwrap();
    assert_eq!(
        block.charge_recovery(4, 0, 0),
        Err(MeterError::BlockCapacity)
    );
    assert_eq!(block.usage(), ResourceUsage::default());
}
#[test]
fn gas_byte_signature_and_expiry_overflow_stop_without_partial_counter_updates() {
    let p = profile();
    for kind in [0, 1, 2] {
        let mut c = ceilings();
        c.max_gas = u64::MAX;
        c.max_bytes = u64::MAX;
        c.max_signatures = u64::MAX;
        let mut block = SharedBlockMeter::new(&p, c).unwrap();
        let result = match kind {
            0 => block.charge_recovery(u64::MAX, 0, 0),
            1 => block.charge_recovery(0, u64::MAX, 0),
            _ => block.charge_recovery(0, 0, u64::MAX),
        };
        assert_eq!(result, Err(MeterError::BlockCapacity));
        assert_eq!(block.usage(), ResourceUsage::default());
        assert_eq!(block.recovery_usage(), ResourceUsage::default());
    }
    let mut block = SharedBlockMeter::new(&p, ceilings()).unwrap();
    assert_eq!(block.charge_expiry(51), Err(MeterError::BlockCapacity));
    assert_eq!(block.expiry_gas(), 0);
    let mut p = profile();
    p.wire_byte_cost = u64::MAX;
    let mut block = SharedBlockMeter::new(&p, ceilings()).unwrap();
    assert!(matches!(
        block.rejected_wire(2, &p),
        Err(MeterError::Internal(_))
    ));
    assert_eq!(block.usage(), ResourceUsage::default());
}
#[test]
fn fixed_logical_record_vectors_use_wide_values_and_reject_noncanonical_fields() {
    let fields = [
        LogicalField {
            id: 1,
            value: LogicalValue::U64(1),
        },
        LogicalField {
            id: 2,
            value: LogicalValue::U128(2),
        },
    ];
    let r = LogicalRecord::new(b"k", &fields, 1024).unwrap();
    let mut expected = b"DYTALLIX/ORDINARY-LOGICAL\0\0\x01\0\x01k\0\x02\0\x01\x01".to_vec();
    expected.extend_from_slice(&1u64.to_be_bytes());
    expected.extend_from_slice(&[0, 2, 2]);
    expected.extend_from_slice(&2u128.to_be_bytes());
    assert_eq!(r.canonical_bytes(), expected);
    let wide = LogicalRecord::new(
        b"k",
        &[
            LogicalField {
                id: 1,
                value: LogicalValue::U64(u64::MAX),
            },
            LogicalField {
                id: 2,
                value: LogicalValue::U128(u128::MAX),
            },
        ],
        1024,
    )
    .unwrap();
    assert_eq!(r.byte_len(), wide.byte_len());
    assert!(LogicalRecord::new(b"k", &[fields[1].clone(), fields[0].clone()], 1024).is_err());
    assert!(LogicalRecord::new(b"k", &[fields[0].clone(), fields[0].clone()], 1024).is_err());
    assert!(LogicalRecord::new(b"", &fields, 1024).is_err());
    assert!(LogicalRecord::new(b"k", &fields, 4).is_err());
}
#[test]
fn mismatched_profile_and_arithmetic_fault_cannot_produce_settleable_summary() {
    let p = profile();
    let mut changed = p.clone();
    changed.signature_costs.insert("mldsa65".into(), 4);
    let mut block = SharedBlockMeter::new(&p, ceilings()).unwrap();
    assert!(OrdinaryMeter::new(&changed, &body(1000), &mut block).is_err());
    assert_eq!(block.usage(), ResourceUsage::default());
    let mut p = profile();
    p.read_byte_cost = u64::MAX;
    let mut block = SharedBlockMeter::new(&p, ceilings()).unwrap();
    let mut meter = OrdinaryMeter::new(&p, &body(1000), &mut block).unwrap();
    assert!(matches!(
        meter.read(&record(b"key", 2)),
        Err(MeterError::Internal(_))
    ));
    assert!(meter.finish().is_err());
}
