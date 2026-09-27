use super::*;
use crate::runtime::governance_candidate::tests::example;

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
