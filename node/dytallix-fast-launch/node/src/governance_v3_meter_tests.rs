use super::*;
use crate::ordinary_meter::{LogicalField, LogicalRecord, LogicalValue, RecoveryCeilings};
use dytallix_protocol_types::{
    ordinary::{self, Limits},
    ordinary_fees::FeeProfile,
    ordinary_fees_v3::{self, ORDINARY_FEE_CONTRACT_VERSION},
    recovery::{KeyIdentity, RecoveryDomain},
};
use std::collections::{BTreeMap, BTreeSet};

fn profile() -> FeeProfileV3 {
    FeeProfileV3 {
        base: FeeProfile {
            ordinary_fee_contract_version: 1,
            version: 1,
            activation_height: 1,
            denomination: ordinary::Denomination::Udrt,
            gas_price: 2,
            minimum_gas: 1,
            max_transaction_gas: 100_000,
            max_block_transaction_gas: 300_000,
            max_block_transaction_bytes: 200_000,
            max_block_signature_checks: 20,
            max_fee_cap: 1_000_000,
            limits: Limits {
                max_wire_bytes: 65_536,
                max_actions: 1,
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
            action_costs: [17; 12],
            signature_costs: BTreeMap::from([("mldsa65".into(), 3)]),
            validator_proof_profile_digest: [9; 32],
            validator_proof_costs: BTreeMap::from([("mldsa65".into(), 11)]),
        },
        version: 2,
        activation_height: 2,
        max_governance_action_bytes: 1024,
        governance_action_costs: [7, 11, 13],
    }
}

fn shared_block(profile: &FeeProfileV3) -> SharedBlockMeter {
    SharedBlockMeter::new(
        &profile.base,
        RecoveryCeilings {
            max_gas: 100_000,
            max_bytes: 50_000,
            max_signatures: 10,
            mandatory_expiry_gas: 1,
        },
    )
    .unwrap()
}

fn make_signed(profile: &FeeProfileV3, action: Action, gas_limit: u64) -> SignedOrdinary {
    SignedOrdinary {
        body: v3::OrdinaryTransaction {
            domain: RecoveryDomain {
                network: 3,
                chain_id: "v3-meter-test".into(),
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
            ordinary_fee_contract_version: ORDINARY_FEE_CONTRACT_VERSION,
            fee_profile_version: profile.version,
            fee_profile_digest: ordinary_fees_v3::profile_digest(profile).unwrap(),
            fee_denomination: v3::Denomination::Udrt,
            maximum_fee: 200_000,
            gas_limit,
            memo: String::new(),
            actions: vec![action],
        },
        // The work meter does not authenticate. The verifier owns that check.
        signature: vec![0; 3309],
    }
}

fn actions() -> [Action; 3] {
    let data = vec![1, 2, 3];
    [
        Action::GovernanceProposal {
            proposal_id: 1,
            action_class: 17,
            action_digest: v3::governance_action_digest(17, &data).unwrap(),
            action_data: data,
        },
        Action::GovernanceDeposit {
            proposal_id: 1,
            amount_udgt: 10,
        },
        Action::GovernanceVote {
            proposal_id: 1,
            choice: v3::VoteChoice::Yes,
        },
    ]
}

#[test]
fn v3_wire_and_all_governance_tags_use_exact_profile_prices() {
    let mut profile = profile();
    profile.base.wire_byte_cost = 2;
    for (index, action) in actions().into_iter().enumerate() {
        let signed = make_signed(&profile, action, 100_000);
        let wire_bytes = v3::encode(&signed, &profile.limits()).unwrap().len() as u64;
        let mut block = shared_block(&profile);
        let mut meter = V3GovernanceMeter::new(&profile, &signed, 2, &mut block).unwrap();
        meter.signature().unwrap();
        meter.accept().unwrap();
        meter.action().unwrap();
        let summary = meter.finish().unwrap();
        assert_eq!(
            summary.used_gas,
            wire_bytes * 2 + 2 + 3 + 5 + profile.governance_action_costs[index]
        );
        assert_eq!(summary.metadata_gas, 5);
        assert!(summary.accepted && !summary.exhausted);
        assert_eq!((summary.processed_actions, summary.action_count), (1, 1));
        assert_eq!(
            block.usage(),
            Usage {
                gas: summary.used_gas,
                bytes: wire_bytes,
                signatures: 1,
            }
        );
    }
}

#[test]
fn preacceptance_rejection_and_accepted_exhaustion_are_terminal() {
    let profile = profile();
    let signed = make_signed(&profile, actions()[0].clone(), 15);
    let mut block = shared_block(&profile);
    let mut meter = V3GovernanceMeter::new(&profile, &signed, 2, &mut block).unwrap();
    meter.signature().unwrap();
    meter.accept().unwrap();
    assert_eq!(meter.action(), Err(MeterError::AcceptedOutOfGas));
    let summary = meter.finish().unwrap();
    assert_eq!(summary.used_gas, 15);
    assert!(summary.accepted && summary.exhausted);
    assert_eq!(summary.processed_actions, 0);
    assert_eq!(block.usage().gas, 15);

    let signed = make_signed(&profile, actions()[0].clone(), 9);
    let mut block = shared_block(&profile);
    let mut meter = V3GovernanceMeter::new(&profile, &signed, 2, &mut block).unwrap();
    meter.signature().unwrap();
    assert!(matches!(meter.accept(), Err(MeterError::Rejected(_))));
    let summary = meter.finish().unwrap();
    assert!(!summary.accepted);
    assert_eq!(summary.used_gas, 10);
    assert_eq!(block.usage().gas, 10);
}

#[test]
fn profile_binding_order_and_block_capacity_fail_closed() {
    let profile = profile();
    let mut changed = profile.clone();
    changed.base.gas_price += 1;
    let transaction = make_signed(&profile, actions()[0].clone(), 100_000);
    let mut block = shared_block(&profile);
    assert!(matches!(
        V3GovernanceMeter::new(&changed, &transaction, 2, &mut block),
        Err(MeterError::Internal(_))
    ));
    assert_eq!(block.usage(), Usage::default());

    let mut block = shared_block(&profile);
    let mut meter = V3GovernanceMeter::new(&profile, &transaction, 2, &mut block).unwrap();
    assert!(matches!(meter.accept(), Err(MeterError::Internal(_))));
    assert!(meter.finish().is_err());

    let mut limited = profile.clone();
    limited.base.max_block_transaction_gas = limited.base.max_transaction_gas;
    limited.base.max_block_signature_checks = 1;
    let signed = make_signed(&limited, actions()[0].clone(), 100_000);
    let mut block = shared_block(&limited);
    let mut meter = V3GovernanceMeter::new(&limited, &signed, 2, &mut block).unwrap();
    meter.signature().unwrap();
    meter.accept().unwrap();
    meter.action().unwrap();
    meter.finish().unwrap();
    let next = make_signed(&limited, actions()[1].clone(), 100_000);
    let mut meter = V3GovernanceMeter::new(&limited, &next, 2, &mut block).unwrap();
    assert_eq!(meter.signature(), Err(MeterError::BlockCapacity));
    assert!(meter.finish().is_err());
    assert_eq!(block.usage().signatures, 1);
}

#[test]
fn signed_profile_and_action_class_are_required() {
    let profile = profile();
    let mut block = shared_block(&profile);
    let mut signed = make_signed(&profile, actions()[0].clone(), 100_000);
    signed.body.fee_profile_digest[0] ^= 1;
    assert!(matches!(
        V3GovernanceMeter::new(&profile, &signed, 2, &mut block),
        Err(MeterError::Rejected(_))
    ));
    let signed = make_signed(&profile, Action::Data { data: "x".into() }, 100_000);
    assert!(matches!(
        V3GovernanceMeter::new(&profile, &signed, 2, &mut block),
        Err(MeterError::Rejected(_))
    ));
}

#[test]
fn v2_recovery_and_v3_consume_one_block_budget() {
    let mut profile = profile();
    profile.base.wire_byte_cost = 1;
    let transaction = make_signed(&profile, actions()[0].clone(), 100_000);
    let mut shared = shared_block(&profile);
    shared.rejected_wire(5, &profile.base).unwrap();
    shared.charge_recovery(7, 3, 1).unwrap();
    let prior = shared.usage();
    let mut meter = V3GovernanceMeter::new(&profile, &transaction, 2, &mut shared).unwrap();
    meter.signature().unwrap();
    meter.accept().unwrap();
    meter.action().unwrap();
    let summary = meter.finish().unwrap();
    let wire_bytes = v3::encode(&transaction, &profile.limits()).unwrap().len() as u64;
    assert_eq!(
        prior,
        Usage {
            gas: 12,
            bytes: 8,
            signatures: 1
        }
    );
    assert_eq!(shared.usage().gas, prior.gas + summary.used_gas);
    assert_eq!(shared.usage().bytes, prior.bytes + wire_bytes);
    assert_eq!(shared.usage().signatures, prior.signatures + 1);
}

#[test]
fn second_v3_profile_in_one_block_faults_shared_budget() {
    let profile = profile();
    let transaction = make_signed(&profile, actions()[0].clone(), 100_000);
    let mut shared = shared_block(&profile);
    let meter = V3GovernanceMeter::new(&profile, &transaction, 2, &mut shared).unwrap();
    meter.finish().unwrap();
    let mut changed = profile.clone();
    changed.governance_action_costs[0] += 1;
    let next = make_signed(&changed, actions()[0].clone(), 100_000);
    assert!(matches!(
        V3GovernanceMeter::new(&changed, &next, 2, &mut shared),
        Err(MeterError::Internal(_))
    ));
    assert!(shared.is_faulted());
}

fn state_record(bytes: usize) -> LogicalRecord {
    LogicalRecord::new(
        b"governance:v1:state",
        &[LogicalField {
            id: 1,
            value: LogicalValue::Bytes(vec![7; bytes]),
        }],
        4096,
    )
    .unwrap()
}

#[test]
fn governance_logical_read_once_and_latest_write_have_exact_cost() {
    let mut profile = profile();
    profile.base.read_byte_cost = 2;
    profile.base.write_byte_cost = 3;
    let transaction = make_signed(&profile, actions()[0].clone(), 100_000);
    let read = state_record(10);
    let first_write = state_record(20);
    let final_write = state_record(30);
    let mut shared = shared_block(&profile);
    let mut meter = V3GovernanceMeter::new(&profile, &transaction, 2, &mut shared).unwrap();
    meter.signature().unwrap();
    meter.read(&read).unwrap();
    meter.read(&read).unwrap();
    meter.accept().unwrap();
    meter.action().unwrap();
    meter.write(&first_write).unwrap();
    meter.write(&final_write).unwrap();
    let summary = meter.finish().unwrap();
    assert_eq!(
        summary.used_gas,
        profile.base.transaction_overhead
            + profile.base.signature_costs["mldsa65"]
            + read.byte_len() * profile.base.read_byte_cost
            + profile.base.receipt_metadata_cost
            + profile.governance_action_costs[0]
            + final_write.byte_len() * profile.base.write_byte_cost
    );
    assert_eq!(shared.usage().gas, summary.used_gas);
}

#[test]
fn accepted_governance_write_out_of_gas_retains_signed_limit() {
    let profile = profile();
    let transaction = make_signed(&profile, actions()[0].clone(), 18);
    let mut shared = shared_block(&profile);
    let mut meter = V3GovernanceMeter::new(&profile, &transaction, 2, &mut shared).unwrap();
    meter.signature().unwrap();
    meter.accept().unwrap();
    meter.action().unwrap();
    assert_eq!(
        meter.write(&state_record(20)),
        Err(MeterError::AcceptedOutOfGas)
    );
    let summary = meter.finish().unwrap();
    assert!(summary.accepted && summary.exhausted);
    assert_eq!(summary.used_gas, 18);
    assert_eq!(shared.usage().gas, 18);
}
