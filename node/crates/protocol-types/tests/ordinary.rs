//! Synthetic codec vectors do not prove valid signatures, paid execution or deployment.
use dytallix_protocol_types::ordinary::*;
use serde_json::Value;
use std::collections::BTreeSet;
fn vectors() -> Value {
    serde_json::from_str(include_str!("fixtures/ordinary_v2_vectors.json")).unwrap()
}
fn limits() -> Limits {
    serde_json::from_value(vectors()["limits"].clone()).unwrap()
}
fn fixture(index: usize) -> SignedOrdinary {
    serde_json::from_value(vectors()["vectors"][index]["input"].clone()).unwrap()
}
#[test]
fn independent_shared_vectors_cover_minimal_body_and_all_twelve_actions() {
    let data = vectors();
    let l = limits();
    for vector in data["vectors"].as_array().unwrap() {
        let value: SignedOrdinary = serde_json::from_value(vector["input"].clone()).unwrap();
        let body = hex::decode(vector["expected_signing_bytes_hex"].as_str().unwrap()).unwrap();
        let envelope = hex::decode(vector["expected_envelope_hex"].as_str().unwrap()).unwrap();
        assert_eq!(signing_bytes(&value.body, &l).unwrap(), body);
        assert_eq!(encode(&value, &l).unwrap(), envelope);
        assert_eq!(decode_body(&body, &l).unwrap(), value.body);
        assert_eq!(decode(&envelope, &l).unwrap(), value);
        assert_eq!(
            hex::encode(transaction_id(&value.body, &l).unwrap()),
            vector["expected_transaction_id"]
        );
        assert_eq!(
            hex::encode(envelope_hash(&value, &l).unwrap()),
            vector["expected_envelope_hash"]
        );
        assert_eq!(
            hex::encode(key_id(&value.body.key).unwrap()),
            vector["expected_key_id"]
        );
        assert_eq!(serde_json::to_value(&value).unwrap(), vector["input"]);
    }
    assert_eq!(fixture(1).body.actions.len(), 12);
    assert_eq!(dytallix_protocol_types::TRANSACTION_FORMAT_VERSION, 1);
}
#[test]
fn every_outer_authorization_field_changes_the_signed_transaction_id() {
    let l = limits();
    let original = fixture(0).body;
    let changes: &[fn(&mut OrdinaryTransaction)] = &[
        |b| b.domain.network = 2,
        |b| b.domain.chain_id.push('x'),
        |b| b.domain.genesis_digest[0] ^= 1,
        |b| b.domain.account_id[0] ^= 1,
        |b| b.authorization_generation += 1,
        |b| b.spending_nonce += 1,
        |b| b.key.public_key[0] ^= 1,
        |b| {
            b.key.algorithm = "mldsa87".into();
            b.key.public_key = vec![8; 2592];
        },
        |b| b.expiry_height += 1,
        |b| b.ordinary_fee_contract_version += 1,
        |b| b.fee_profile_version += 1,
        |b| b.fee_profile_digest[0] ^= 1,
        |b| b.maximum_fee += 1,
        |b| b.gas_limit += 1,
        |b| b.memo.push('x'),
        |b| b.actions.push(Action::RewardClaim),
    ];
    let id = transaction_id(&original, &l).unwrap();
    for (index, change) in changes.iter().enumerate() {
        let mut body = original.clone();
        change(&mut body);
        assert_ne!(transaction_id(&body, &l).unwrap(), id, "field {index}");
    }
    let mut body = original.clone();
    body.fee_denomination = Denomination::Udgt;
    assert!(signing_bytes(&body, &l).is_err());
    let mut body = original;
    body.ordinary_fee_contract_version = 0;
    assert!(signing_bytes(&body, &l).is_err());
}
#[test]
fn action_field_mutations_are_signed_without_sorting_or_normalization() {
    let l = limits();
    let original = fixture(1).body;
    let expected = transaction_id(&original, &l).unwrap();
    // Mutate every field in each typed action independently through the client view.
    // The fixed shared vector above separately establishes the approved encoding.
    for (index, action) in original.actions.iter().enumerate() {
        let view = serde_json::to_value(action).unwrap();
        for (name, field) in view.as_object().unwrap() {
            if name == "type" {
                continue;
            }
            let mut changed = view.clone();
            changed[name] = match field {
                Value::Array(values) => {
                    let mut values = values.clone();
                    values[0] = ((values[0].as_u64().unwrap() + 1) % 256).into();
                    Value::Array(values)
                }
                Value::String(_) if name == "denomination" => "udrt".into(),
                Value::String(text) if text.bytes().all(|b| b.is_ascii_digit()) => {
                    ((text.parse::<u128>().unwrap() + 1).to_string()).into()
                }
                Value::String(text) => (text.clone() + "x").into(),
                _ => panic!("unexpected field {name}"),
            };
            let mut body = original.clone();
            body.actions[index] = serde_json::from_value(changed).unwrap();
            assert_ne!(
                transaction_id(&body, &l).unwrap(),
                expected,
                "action {index} field {name}"
            );
        }
    }
    let mut body = original.clone();
    body.actions.swap(0, 1);
    assert_ne!(transaction_id(&body, &l).unwrap(), expected);
    let mut body = original;
    body.actions.reverse();
    assert_ne!(transaction_id(&body, &l).unwrap(), expected);
}
#[test]
fn signatures_change_evidence_hash_but_not_transaction_identity() {
    let l = limits();
    let first = fixture(0);
    let mut second = first.clone();
    second.signature[0] ^= 1;
    assert_eq!(
        transaction_id(&first.body, &l),
        transaction_id(&second.body, &l)
    );
    assert_ne!(
        envelope_hash(&first, &l).unwrap(),
        envelope_hash(&second, &l).unwrap()
    );
    second.signature.pop();
    assert!(encode(&second, &l).is_err());
}
#[test]
fn framing_rejects_unknown_versions_lengths_tags_and_trailing_bytes() {
    let l = limits();
    let value = fixture(0);
    let body = signing_bytes(&value.body, &l).unwrap();
    let envelope = encode(&value, &l).unwrap();
    for version in [0u16, 1, 3, u16::MAX] {
        let mut bytes = envelope.clone();
        bytes[WIRE_PREFIX.len()..WIRE_PREFIX.len() + 2].copy_from_slice(&version.to_be_bytes());
        assert!(decode(&bytes, &l).is_err());
        let mut bytes = body.clone();
        bytes[SIGNING_PREFIX.len()..SIGNING_PREFIX.len() + 2]
            .copy_from_slice(&version.to_be_bytes());
        assert!(decode_body(&bytes, &l).is_err());
    }
    for count in [0u16, 33, u16::MAX] {
        let mut bytes = body.clone();
        let offset = bytes.len() - 3;
        bytes[offset..offset + 2].copy_from_slice(&count.to_be_bytes());
        assert!(decode_body(&bytes, &l).is_err());
    }
    for tag in [0, 13, 255] {
        let mut bytes = body.clone();
        *bytes.last_mut().unwrap() = tag;
        assert!(decode_body(&bytes, &l).is_err());
    }
    for end in [0, 1, WIRE_PREFIX.len() + 2, envelope.len() - 1] {
        assert!(decode(&envelope[..end], &l).is_err());
    }
    let mut bytes = body;
    bytes.push(0);
    assert!(decode_body(&bytes, &l).is_err());
    let mut bytes = envelope.clone();
    bytes.push(0);
    assert!(decode(&bytes, &l).is_err());
    let mut bytes = envelope;
    let offset = WIRE_PREFIX.len() + 2;
    bytes[offset..offset + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(decode(&bytes, &l).is_err());
}
#[test]
fn explicit_limits_bound_every_variable_field_before_signing() {
    let value = fixture(1);
    let l = limits();
    let wire = encode(&value, &l).unwrap();
    let mut exact = l.clone();
    exact.max_wire_bytes = wire.len() as u32;
    assert!(encode(&value, &exact).is_ok());
    exact.max_wire_bytes -= 1;
    assert!(encode(&value, &exact).is_err());
    assert!(decode(&wire, &exact).is_err());
    let changes: &[fn(&mut Limits)] = &[
        |l| l.max_actions = 11,
        |l| l.max_identifier_bytes = 1,
        |l| l.max_data_bytes = 1,
        |l| l.max_memo_bytes = 1,
        |l| l.max_consensus_key_bytes = 1,
        |l| l.max_proof_bytes = 1,
    ];
    for change in changes {
        let mut limited = l.clone();
        change(&mut limited);
        assert!(encode(&value, &limited).is_err());
        assert!(decode(&wire, &limited).is_err());
    }
    let mut invalid = l.clone();
    invalid.max_expiry_lifetime = 0;
    assert!(invalid.validate().is_err());
    let mut invalid = l;
    invalid.max_actions = 0;
    assert!(invalid.validate().is_err());
}
#[test]
fn exact_account_algorithm_roles_and_key_sizes_are_required() {
    let l = limits();
    let original = fixture(0);
    for name in ["dilithium3", "dilithium5", "mock-blake3", "MLDSA65", ""] {
        let mut value = original.clone();
        value.body.key.algorithm = name.into();
        assert!(encode(&value, &l).is_err());
        assert!(key_id(&value.body.key).is_err());
    }
    let mut one = l.clone();
    one.allowed_algorithms = BTreeSet::from(["mldsa87".into()]);
    assert!(encode(&original, &one).is_err());
    let mut empty = l.clone();
    empty.allowed_algorithms.clear();
    assert!(empty.validate().is_err());
    let mut aliases = l.clone();
    aliases.allowed_algorithms.insert("dilithium3".into());
    assert!(aliases.validate().is_err());
    let mut value = original;
    value.body.key.public_key.pop();
    assert!(encode(&value, &l).is_err());
}
#[test]
fn large_client_values_are_strict_unsigned_decimal_strings() {
    let mut value = fixture(0);
    value.body.maximum_fee = u128::MAX;
    value.body.spending_nonce = u64::MAX;
    let view = serde_json::to_value(&value).unwrap();
    assert_eq!(view["body"]["maximum_fee"], u128::MAX.to_string());
    assert_eq!(view["body"]["spending_nonce"], u64::MAX.to_string());
    assert_eq!(
        serde_json::from_value::<SignedOrdinary>(view.clone()).unwrap(),
        value
    );
    assert_eq!(
        decode(&encode(&value, &limits()).unwrap(), &limits()).unwrap(),
        value
    );
    for text in ["", "00", "01", "+1", "-1", " 1", "1 ", "1.0", "1e0", "١"] {
        assert!(parse_decimal_u128(text).is_err());
        assert!(parse_decimal_u64(text).is_err());
        let mut changed = view.clone();
        changed["body"]["maximum_fee"] = text.into();
        assert!(serde_json::from_value::<SignedOrdinary>(changed).is_err());
    }
    assert!(parse_decimal_u64("18446744073709551616").is_err());
    assert!(parse_decimal_u128("340282366920938463463374607431768211456").is_err());
    let mut changed = view;
    changed["body"]["spending_nonce"] = 9.into();
    assert!(serde_json::from_value::<SignedOrdinary>(changed).is_err());
}
#[test]
fn profile_and_client_view_reject_missing_unknown_or_duplicate_fields() {
    let base = serde_json::to_value(limits()).unwrap();
    for name in base.as_object().unwrap().keys() {
        let mut changed = base.clone();
        changed.as_object_mut().unwrap().remove(name);
        assert!(
            serde_json::from_value::<Limits>(changed).is_err(),
            "missing {name}"
        );
    }
    let view = serde_json::to_value(fixture(0)).unwrap();
    for name in view["body"].as_object().unwrap().keys() {
        let mut changed = view.clone();
        changed["body"].as_object_mut().unwrap().remove(name);
        assert!(
            serde_json::from_value::<SignedOrdinary>(changed).is_err(),
            "missing {name}"
        );
    }
    let mut changed = view.clone();
    changed["body"]["unknown"] = true.into();
    assert!(serde_json::from_value::<SignedOrdinary>(changed).is_err());
    let mut changed = view.clone();
    changed["body"]["actions"][0]["unknown"] = true.into();
    assert!(serde_json::from_value::<SignedOrdinary>(changed).is_err());
    let json = serde_json::to_string(&view).unwrap();
    let duplicate = json.replacen(
        "\"spending_nonce\":\"9\"",
        "\"spending_nonce\":\"9\",\"spending_nonce\":\"10\"",
        1,
    );
    assert_ne!(json, duplicate);
    assert!(serde_json::from_str::<SignedOrdinary>(&duplicate).is_err());
}
#[test]
fn exact_utf8_and_identifier_meaning_survive_roundtrip() {
    let l = limits();
    let mut a = fixture(0);
    a.body.memo = "é".into();
    let mut b = a.clone();
    b.body.memo = "e\u{301}".into();
    assert_ne!(
        transaction_id(&a.body, &l).unwrap(),
        transaction_id(&b.body, &l).unwrap()
    );
    a.body.domain.chain_id = "é".repeat(64);
    assert!(encode(&a, &l).is_ok());
    a.body.domain.chain_id.push('x');
    assert!(encode(&a, &l).is_err());
    let mut a = fixture(0);
    a.body.actions = vec![Action::ValidatorWithdraw {
        unbond_id: "Registry-ID/É".into(),
    }];
    assert_eq!(decode(&encode(&a, &l).unwrap(), &l).unwrap(), a);
    a.body.actions = vec![Action::ValidatorWithdraw {
        unbond_id: String::new(),
    }];
    assert!(encode(&a, &l).is_err());
    let mut bytes = signing_bytes(&fixture(0).body, &l).unwrap();
    let offset = SIGNING_PREFIX.len() + 2 + 1 + 2;
    bytes[offset] = 255;
    assert!(decode_body(&bytes, &l).is_err());
}
#[test]
fn nonzero_action_amounts_and_periods_do_not_get_implicit_defaults() {
    let mut value = fixture(0);
    let l = limits();
    for action in [
        Action::Send {
            recipient: [1; 32],
            denomination: Denomination::Udrt,
            amount: 0,
        },
        Action::DmsRegister {
            beneficiary: [2; 32],
            period_blocks: 0,
        },
        Action::RewardBond {
            validator_id: "v".into(),
            amount_udgt: 0,
        },
        Action::RewardBeginUnbond {
            validator_id: "v".into(),
            amount_udgt: 0,
        },
        Action::ValidatorRegister {
            validator_id: "v".into(),
            consensus_key: vec![1],
            proof: vec![2],
            proof_expiry_height: 100,
            amount_udgt: 0,
        },
    ] {
        value.body.actions = vec![action];
        assert!(encode(&value, &l).is_err());
    }
    value.body.actions.clear();
    assert!(encode(&value, &l).is_err());
}
