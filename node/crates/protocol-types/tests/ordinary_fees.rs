//! Independent codec evidence. These synthetic values do not select production fees.
use dytallix_protocol_types::ordinary::Denomination;
use dytallix_protocol_types::ordinary_fees::*;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
fn vectors() -> Value {
    serde_json::from_str(include_str!("fixtures/ordinary_fee_v1_vectors.json")).unwrap()
}
fn profile() -> FeeProfile {
    serde_json::from_value(vectors()["profile"].clone()).unwrap()
}
fn offset(pointer: &str) -> usize {
    vectors()["offsets"][pointer][0].as_u64().unwrap() as usize
}
#[test]
fn independent_profile_vector_and_all_field_mutations_match_exact_bytes() {
    let data = vectors();
    let original = profile();
    let bytes = hex::decode(data["expected_profile_hex"].as_str().unwrap()).unwrap();
    assert_eq!(bytes.len(), 347);
    assert_eq!(profile_bytes(&original).unwrap(), bytes);
    assert_eq!(
        hex::encode(profile_digest(&original).unwrap()),
        data["expected_digest"]
    );
    assert_eq!(decode_profile(&bytes).unwrap(), original);
    assert_eq!(serde_json::to_value(&original).unwrap(), data["profile"]);
    assert_eq!(data["mutations"].as_array().unwrap().len(), 38);
    for case in data["mutations"].as_array().unwrap() {
        let pointer = case["pointer"].as_str().unwrap();
        let mut json = data["profile"].clone();
        *json.pointer_mut(pointer).unwrap() = case["value"].clone();
        let value: FeeProfile = serde_json::from_value(json).unwrap();
        let expected = hex::decode(case["expected_profile_hex"].as_str().unwrap()).unwrap();
        assert_eq!(profile_bytes(&value).unwrap(), expected, "{pointer}");
        assert_eq!(
            hex::encode(profile_digest(&value).unwrap()),
            case["expected_digest"],
            "{pointer}"
        );
        assert_ne!(
            profile_digest(&value).unwrap(),
            profile_digest(&original).unwrap(),
            "{pointer}"
        );
        assert_eq!(decode_profile(&expected).unwrap(), value, "{pointer}");
    }
}
#[test]
fn format_contract_denomination_and_exact_algorithm_codes_reject_aliases() {
    let original = profile_bytes(&profile()).unwrap();
    for value in [0u16, 2, u16::MAX] {
        for start in [PROFILE_PREFIX.len(), PROFILE_PREFIX.len() + 2] {
            let mut bytes = original.clone();
            bytes[start..start + 2].copy_from_slice(&value.to_be_bytes());
            assert!(decode_profile(&bytes).is_err());
        }
        let mut p = profile();
        p.ordinary_fee_contract_version = value;
        assert!(p.validate().is_err());
    }
    for denomination in [0, 1, 3, 255] {
        let mut bytes = original.clone();
        bytes[offset("/denomination")] = denomination;
        assert!(decode_profile(&bytes).is_err());
    }
    let mut p = profile();
    p.denomination = Denomination::Udgt;
    assert!(profile_bytes(&p).is_err());
    for code in [0u16, 3, u16::MAX] {
        for path in [
            "/signature_costs/mldsa65/code",
            "/validator_proof_costs/mldsa87/code",
        ] {
            let mut bytes = original.clone();
            let start = offset(path);
            bytes[start..start + 2].copy_from_slice(&code.to_be_bytes());
            assert!(decode_profile(&bytes).is_err());
        }
    }
    for algorithm in ["dilithium3", "dilithium5", "mock-blake3", "MLDSA87", ""] {
        let mut p = profile();
        p.validator_proof_costs = BTreeMap::from([(algorithm.into(), 1)]);
        assert!(p.validate().is_err());
    }
}
#[test]
fn costs_have_one_order_and_must_match_the_exact_account_and_validator_roles() {
    let mut p = profile();
    p.signature_costs.clear();
    p.signature_costs.insert("mldsa87".into(), 300);
    p.signature_costs.insert("mldsa65".into(), 200);
    assert_eq!(
        profile_bytes(&p).unwrap(),
        profile_bytes(&profile()).unwrap()
    );
    let original = profile_bytes(&p).unwrap();
    let first = offset("/signature_costs/mldsa65/code");
    let second = offset("/signature_costs/mldsa87/code");
    let mut reordered = original.clone();
    reordered[first..first + 10].copy_from_slice(&original[second..second + 10]);
    reordered[second..second + 10].copy_from_slice(&original[first..first + 10]);
    assert!(decode_profile(&reordered).is_err());
    let mut duplicate = original;
    duplicate[second..second + 2].copy_from_slice(&1u16.to_be_bytes());
    assert!(decode_profile(&duplicate).is_err());
    p.signature_costs.remove("mldsa65");
    assert!(p.validate().is_err());
    p.limits.allowed_algorithms.remove("mldsa65");
    assert!(p.validate().is_ok());
    assert!(p
        .validate_validator_profile([9; 32], &BTreeSet::from(["mldsa87".into()]))
        .is_ok());
    assert!(p
        .validate_validator_profile([8; 32], &BTreeSet::from(["mldsa87".into()]))
        .is_err());
    assert!(p
        .validate_validator_profile(
            [9; 32],
            &BTreeSet::from(["mldsa65".into(), "mldsa87".into()])
        )
        .is_err());
    assert!(p
        .validate_validator_profile([9; 32], &BTreeSet::new())
        .is_err());
    // Codec support for ML-DSA-65 does not add it to the committed validator role.
    p.validator_proof_costs.insert("mldsa65".into(), 100);
    assert!(p.validate().is_ok());
    assert!(p
        .validate_validator_profile([9; 32], &BTreeSet::from(["mldsa87".into()]))
        .is_err());
}
#[test]
fn bounded_profile_decoder_rejects_each_truncation_trailing_data_and_cost_counts() {
    let original = profile_bytes(&profile()).unwrap();
    for end in 0..original.len() {
        assert!(
            decode_profile(&original[..end]).is_err(),
            "truncation {end}"
        );
    }
    let mut bytes = original.clone();
    bytes.push(0);
    assert!(decode_profile(&bytes).is_err());
    assert!(decode_profile(&vec![0; MAX_PROFILE_BYTES + 1]).is_err());
    for count in [0, 3, 255] {
        for path in ["/signature_costs/count", "/validator_proof_costs/count"] {
            let mut bytes = original.clone();
            bytes[offset(path)] = count;
            assert!(decode_profile(&bytes).is_err());
        }
    }
    let mut bytes = original;
    bytes[0] ^= 1;
    assert!(decode_profile(&bytes).is_err());
}
#[test]
fn required_capacity_bounds_and_explicit_zero_costs_remain_distinct() {
    let invalid: &[fn(&mut FeeProfile)] = &[
        |p| p.gas_price = 0,
        |p| p.max_transaction_gas = 0,
        |p| p.minimum_gas = p.max_transaction_gas + 1,
        |p| p.max_block_transaction_gas = p.max_transaction_gas - 1,
        |p| p.max_block_transaction_bytes = u64::from(p.limits.max_wire_bytes) - 1,
        |p| p.max_block_signature_checks = 0,
        |p| p.max_fee_cap = 0,
        |p| p.signature_costs.clear(),
        |p| p.validator_proof_costs.clear(),
        |p| p.limits.max_expiry_lifetime = 0,
    ];
    for (i, change) in invalid.iter().enumerate() {
        let mut p = profile();
        change(&mut p);
        assert!(p.validate().is_err(), "case {i}");
        assert!(profile_bytes(&p).is_err());
    }
    let mut p = profile();
    p.minimum_gas = 0;
    p.transaction_overhead = 0;
    p.receipt_metadata_cost = 0;
    p.wire_byte_cost = 0;
    p.read_byte_cost = 0;
    p.write_byte_cost = 0;
    p.action_costs = [0; 12];
    for v in p.signature_costs.values_mut() {
        *v = 0;
    }
    for v in p.validator_proof_costs.values_mut() {
        *v = 0;
    }
    assert_eq!(decode_profile(&profile_bytes(&p).unwrap()).unwrap(), p);
}
#[test]
fn signed_gas_and_fee_cap_boundaries_use_wide_exact_arithmetic() {
    let p = profile();
    assert!(p.validate_request(10, 20).is_ok());
    assert!(p.validate_request(100000, 200000).is_ok());
    for (gas, cap) in [
        (0, 200000),
        (9, 200000),
        (100001, 200000),
        (10, 19),
        (10, 200001),
    ] {
        assert!(p.validate_request(gas, cap).is_err(), "gas {gas} cap {cap}");
    }
    let mut p = profile();
    p.gas_price = u64::MAX;
    p.max_transaction_gas = u64::MAX;
    p.max_block_transaction_gas = u64::MAX;
    p.max_fee_cap = u128::MAX;
    p.minimum_gas = 0;
    let exact = u128::from(u64::MAX) * u128::from(u64::MAX);
    assert!(exact > u128::from(u64::MAX));
    assert!(p.validate_request(u64::MAX, exact).is_ok());
    assert!(p.validate_request(u64::MAX, exact - 1).is_err());
    assert!(p.validate_request(u64::MAX, u128::MAX).is_ok());
    assert!(p.validate_request(1, u128::from(u64::MAX)).is_ok());
    let bytes = profile_bytes(&p).unwrap();
    assert_eq!(decode_profile(&bytes).unwrap(), p);
}
#[test]
fn profile_client_values_require_decimal_strings_and_all_fields() {
    let json = vectors()["profile"].clone();
    for name in json.as_object().unwrap().keys() {
        let mut missing = json.clone();
        missing.as_object_mut().unwrap().remove(name);
        assert!(
            serde_json::from_value::<FeeProfile>(missing).is_err(),
            "missing {name}"
        );
    }
    for name in json["limits"].as_object().unwrap().keys() {
        let mut missing = json.clone();
        missing["limits"].as_object_mut().unwrap().remove(name);
        assert!(
            serde_json::from_value::<FeeProfile>(missing).is_err(),
            "missing limits {name}"
        );
    }
    for text in [
        "",
        "00",
        "01",
        "+1",
        "-1",
        " 1",
        "1.0",
        "18446744073709551616",
    ] {
        for path in [
            "/gas_price",
            "/action_costs/0",
            "/signature_costs/mldsa65",
            "/validator_proof_costs/mldsa87",
            "/limits/max_expiry_lifetime",
        ] {
            let mut changed = json.clone();
            *changed.pointer_mut(path).unwrap() = text.into();
            assert!(
                serde_json::from_value::<FeeProfile>(changed).is_err(),
                "{path}={text}"
            );
        }
    }
    let mut changed = json.clone();
    changed["max_fee_cap"] = "340282366920938463463374607431768211456".into();
    assert!(serde_json::from_value::<FeeProfile>(changed).is_err());
    let mut changed = json.clone();
    changed["gas_price"] = 2.into();
    assert!(serde_json::from_value::<FeeProfile>(changed).is_err());
    let mut changed = json.clone();
    changed["unknown"] = true.into();
    assert!(serde_json::from_value::<FeeProfile>(changed).is_err());
    for count in [0, 11, 13] {
        let mut changed = json.clone();
        changed["action_costs"] = serde_json::json!(vec!["0"; count]);
        assert!(serde_json::from_value::<FeeProfile>(changed).is_err());
    }
    let mut p = profile();
    p.max_fee_cap = u128::MAX;
    p.gas_price = u64::MAX;
    let view = serde_json::to_value(&p).unwrap();
    assert_eq!(view["max_fee_cap"], u128::MAX.to_string());
    assert_eq!(view["gas_price"], u64::MAX.to_string());
    assert_eq!(serde_json::from_value::<FeeProfile>(view).unwrap(), p);
}
#[test]
fn duplicate_client_cost_entries_do_not_override_an_explicit_price() {
    let original = serde_json::to_string(&profile()).unwrap();
    for (needle, replacement) in [
        (
            "\"mldsa65\":\"200\"",
            "\"mldsa65\":\"200\",\"mldsa65\":\"0\"",
        ),
        (
            "\"mldsa87\":\"400\"",
            "\"mldsa87\":\"400\",\"mldsa87\":\"0\"",
        ),
        (
            "\"gas_price\":\"2\"",
            "\"gas_price\":\"2\",\"gas_price\":\"0\"",
        ),
    ] {
        let changed = original.replacen(needle, replacement, 1);
        assert_ne!(changed, original);
        assert!(serde_json::from_str::<FeeProfile>(&changed).is_err());
    }
}
