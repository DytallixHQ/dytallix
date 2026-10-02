use super::*;
use crate::runtime::governance_candidate::{tests::example, Bounds};

fn fees(candidate: &GovernanceCandidateConfig) -> FeeValues {
    let base = &candidate.fee_profile.base;
    FeeValues {
        gas_price: base.gas_price,
        transaction_overhead: base.transaction_overhead,
        receipt_metadata_cost: base.receipt_metadata_cost,
        wire_byte_cost: base.wire_byte_cost,
        read_byte_cost: base.read_byte_cost,
        write_byte_cost: base.write_byte_cost,
        action_costs: base.action_costs,
        signature_costs: base.signature_costs.clone(),
        validator_proof_costs: base.validator_proof_costs.clone(),
        governance_action_costs: candidate.fee_profile.governance_action_costs,
        account_creation_fee_udrt: base.account_creation_fee_udrt,
    }
}
fn check(candidate: &GovernanceCandidateConfig, change: &ParameterChange) -> Result<(), Rule> {
    validate_proposal(candidate, CLASS_PARAMETER_CHANGE, &encode(change).unwrap())
}

#[test]
fn proposals_need_an_enabled_class_canonical_bytes_and_genesis_bounds() {
    let mut candidate = example();
    // A fee change carries every cost, so it needs a larger class bound.
    candidate.action_classes[0].max_data_bytes = 1_000;
    assert_eq!(
        check(&candidate, &ParameterChange::MinSelfBond(1_000)),
        Ok(())
    );
    assert_eq!(
        check(&candidate, &ParameterChange::MinSelfBond(1_001)),
        Err(Rule("GOVERNANCE_PARAMETER_OUT_OF_BOUNDS"))
    );
    assert_eq!(
        check(&candidate, &ParameterChange::MaxActive(0)),
        Err(Rule("GOVERNANCE_PARAMETER_OUT_OF_BOUNDS"))
    );
    assert_eq!(
        check(&candidate, &ParameterChange::Fees(fees(&candidate))),
        Ok(())
    );
    for mutate in [
        |v: &mut FeeValues| v.gas_price = 11,
        |v: &mut FeeValues| v.write_byte_cost = 101,
        |v: &mut FeeValues| v.governance_action_costs[1] = 0,
        |v: &mut FeeValues| v.account_creation_fee_udrt = 10_001,
    ] {
        let mut values = fees(&candidate);
        mutate(&mut values);
        assert_eq!(
            check(&candidate, &ParameterChange::Fees(values)),
            Err(Rule("GOVERNANCE_PARAMETER_OUT_OF_BOUNDS"))
        );
    }
    let mut bytes = encode(&ParameterChange::MaxActive(3)).unwrap();
    bytes.push(0);
    assert_eq!(
        validate_proposal(&candidate, CLASS_PARAMETER_CHANGE, &bytes),
        Err(Rule("GOVERNANCE_ACTION_MALFORMED"))
    );
    // The registry class is not enabled in this candidate.
    let add = encode(&RegistryChange::Add {
        validator_id: "v".into(),
        owner: "o".into(),
    })
    .unwrap();
    assert_eq!(
        validate_proposal(&candidate, CLASS_VALIDATOR_REGISTRY, &add),
        Err(Rule("GOVERNANCE_CLASS_NOT_ENABLED"))
    );
    let mut small = candidate.clone();
    small.action_classes[0].max_data_bytes = 2;
    assert_eq!(
        check(&small, &ParameterChange::MaxActive(3)),
        Err(Rule("GOVERNANCE_ACTION_TOO_LARGE"))
    );
}

/// The approved profile shape (P01, 30 September 2026) at `gas_price`:
/// floor pricing, so the reference basic Send pays the minimum gas.
fn approved(candidate: &mut GovernanceCandidateConfig, gas_price: u64) -> FeeValues {
    candidate.action_classes[0].max_data_bytes = 1_000;
    candidate.fee_profile.base.minimum_gas = 100_000;
    candidate.parameter_bounds.gas_price = Bounds { min: 1, max: 1_000 };
    candidate.parameter_bounds.resource_cost = Bounds { min: 0, max: 100_000 };
    candidate.parameter_bounds.reference_send_fee_udrt = Bounds {
        min: 100_000,
        max: 10_000_000,
    };
    let mut values = fees(candidate);
    values.gas_price = gas_price;
    values.transaction_overhead = 10_000;
    values.receipt_metadata_cost = 1_000;
    values.wire_byte_cost = 2;
    values.read_byte_cost = 0;
    values.write_byte_cost = 1;
    values.action_costs = [5_000; 12];
    values.signature_costs.insert("mldsa65".into(), 20_000);
    values
}

#[test]
fn a_fee_change_keeps_the_reference_send_inside_its_genesis_bound() {
    let mut candidate = example();
    // 47,616 gas pays the 100,000 floor: 1 DRT at price 10, the approved
    // range's ends at prices 1 and 100.
    for (price, fee) in [(1, 100_000), (10, 1_000_000), (100, 10_000_000)] {
        let values = approved(&mut candidate, price);
        assert_eq!(reference_send_fee(&values, 100_000), Some(fee));
        assert_eq!(check(&candidate, &ParameterChange::Fees(values)), Ok(()), "{price}");
    }
    let out = Err(Rule("GOVERNANCE_REFERENCE_SEND_FEE_OUT_OF_BOUNDS"));
    let above = approved(&mut candidate, 101);
    assert_eq!(check(&candidate, &ParameterChange::Fees(above)), out);
    // Per-byte costs move it too, though each stays inside its own bound:
    // reads at 1,000 a byte add 5,536,000 gas, about 55.8 DRT at price 10;
    // at 100 a byte the Send costs about 6.01 DRT.
    let mut values = approved(&mut candidate, 10);
    values.read_byte_cost = 1_000;
    assert_eq!(reference_send_fee(&values, 100_000), Some((5_536_000 + 47_616) * 10));
    assert_eq!(check(&candidate, &ParameterChange::Fees(values.clone())), out);
    values.read_byte_cost = 100;
    assert_eq!(reference_send_fee(&values, 100_000), Some((553_600 + 47_616) * 10));
    assert_eq!(check(&candidate, &ParameterChange::Fees(values)), Ok(()));
    // Under the floor, the minimum gas sets the fee; a profile without the
    // ML-DSA-65 price cannot be priced and is refused.
    let mut values = approved(&mut candidate, 10);
    values.wire_byte_cost = 0;
    assert_eq!(reference_send_fee(&values, 100_000), Some(1_000_000));
    values.signature_costs.clear();
    values.signature_costs.insert("mldsa87".into(), 20_000);
    assert_eq!(reference_send_fee(&values, 100_000), None);
    assert_eq!(check(&candidate, &ParameterChange::Fees(values)), out);
    // The bound itself is a genesis value with a floor of 1 uDRT.
    candidate.parameter_bounds.reference_send_fee_udrt = Bounds { min: 0, max: 1 };
    assert!(candidate.parameter_bounds.validate().is_err());
    candidate.parameter_bounds.reference_send_fee_udrt = Bounds { min: 5, max: 4 };
    assert!(candidate.parameter_bounds.validate().is_err());
}

#[test]
fn a_fee_change_keeps_every_ungoverned_profile_field() {
    let candidate = example();
    let base = &candidate.fee_profile.base;
    let mut values = fees(&candidate);
    values.gas_price = 5;
    values.account_creation_fee_udrt = 7;
    let next = with_fees(base, &values, 40).unwrap();
    assert_eq!(
        (next.version, next.activation_height),
        (base.version + 1, 40)
    );
    assert_eq!((next.gas_price, next.account_creation_fee_udrt), (5, 7));
    let mut same = next.clone();
    same.version = base.version;
    same.activation_height = base.activation_height;
    same.gas_price = base.gas_price;
    same.account_creation_fee_udrt = base.account_creation_fee_udrt;
    assert_eq!(&same, base);
    values.signature_costs.insert("mldsa87".into(), 1);
    assert_eq!(
        with_fees(base, &values, 40),
        Err(Rule("GOVERNANCE_FEE_ALGORITHMS_DIFFER"))
    );
}

/// Clients build action data with the protocol-types encoder (clients v1);
/// it must equal the chain's bincode bytes and decode back canonically.
#[test]
fn client_action_data_matches_the_chain_encoding() {
    use dytallix_protocol_types::governance_action as client;
    assert_eq!(client::CLASS_PARAMETER_CHANGE, CLASS_PARAMETER_CHANGE);
    assert_eq!(client::CLASS_VALIDATOR_REGISTRY, CLASS_VALIDATOR_REGISTRY);
    let mut values = fees(&example());
    values.signature_costs.insert("mldsa87".into(), 9);
    values.validator_proof_costs.insert("zeta-é".into(), u64::MAX);
    values.account_creation_fee_udrt = u128::MAX - 7;
    let parameters = [
        ParameterChange::Fees(values),
        ParameterChange::MinSelfBond(u128::MAX),
        ParameterChange::MaxActive(16),
    ];
    for change in parameters {
        let data = change.action_data();
        assert_eq!(data, encode(&change).unwrap(), "{change:?}");
        assert_eq!(decode::<ParameterChange>(&data), Ok(change));
    }
    let registry = [
        RegistryChange::Add {
            validator_id: "validator-é".into(),
            owner: "dyt1owner".into(),
        },
        RegistryChange::Remove {
            validator_id: String::new(),
        },
    ];
    for change in registry {
        let data = change.action_data();
        assert_eq!(data, encode(&change).unwrap(), "{change:?}");
        assert_eq!(decode::<RegistryChange>(&data), Ok(change));
    }
}
