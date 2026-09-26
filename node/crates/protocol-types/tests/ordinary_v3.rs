//! Provisional codec tests. These do not authenticate signatures or activate governance.
use dytallix_protocol_types::{ordinary as v2, ordinary_v3 as v3};
use serde_json::Value;

fn vectors() -> Value {
    serde_json::from_str(include_str!("fixtures/ordinary_v2_vectors.json")).unwrap()
}

fn limits() -> v3::V3Limits {
    v3::V3Limits {
        ordinary: serde_json::from_value(vectors()["limits"].clone()).unwrap(),
        max_governance_action_bytes: 128,
    }
}

fn fixture(index: usize) -> v3::SignedOrdinary {
    serde_json::from_value(vectors()["vectors"][index]["input"].clone()).unwrap()
}

fn proposal() -> v3::Action {
    let action_data = vec![0, 1, 0xff, 7];
    v3::Action::GovernanceProposal {
        proposal_id: 23,
        action_class: 17,
        action_digest: v3::governance_action_digest(17, &action_data).unwrap(),
        action_data,
    }
}

fn governance_fixture() -> v3::SignedOrdinary {
    let mut signed = fixture(0);
    signed.body.actions = vec![
        proposal(),
        v3::Action::GovernanceDeposit {
            proposal_id: 23,
            amount_udgt: 100,
        },
        v3::Action::GovernanceVote {
            proposal_id: 23,
            choice: v3::VoteChoice::NoWithVeto,
        },
    ];
    signed
}

#[test]
fn legacy_actions_have_separate_versioned_bytes_and_ids() {
    let limits = limits();
    let v2_value: v2::SignedOrdinary =
        serde_json::from_value(vectors()["vectors"][1]["input"].clone()).unwrap();
    let v3_value = fixture(1);
    assert_eq!(v3_value.body.actions.len(), 12);
    let body2 = v2::signing_bytes(&v2_value.body, &limits.ordinary).unwrap();
    let body3 = v3::signing_bytes(&v3_value.body, &limits).unwrap();
    assert_eq!(body2.len(), body3.len());
    assert_eq!(
        body2[0..v2::SIGNING_PREFIX.len()],
        body3[0..v3::SIGNING_PREFIX.len()]
    );
    assert_eq!(
        body2[v2::SIGNING_PREFIX.len()..v2::SIGNING_PREFIX.len() + 2],
        2u16.to_be_bytes()
    );
    assert_eq!(
        body3[v3::SIGNING_PREFIX.len()..v3::SIGNING_PREFIX.len() + 2],
        3u16.to_be_bytes()
    );
    assert_eq!(
        body2[v2::SIGNING_PREFIX.len() + 2..],
        body3[v3::SIGNING_PREFIX.len() + 2..]
    );
    let wire2 = v2::encode(&v2_value, &limits.ordinary).unwrap();
    let wire3 = v3::encode(&v3_value, &limits).unwrap();
    assert_eq!(v2::decode(&wire2, &limits.ordinary).unwrap(), v2_value);
    assert_eq!(v3::decode(&wire3, &limits).unwrap(), v3_value);
    assert!(v2::decode(&wire3, &limits.ordinary).is_err());
    assert!(v3::decode(&wire2, &limits).is_err());
    assert!(v2::decode_body(&body3, &limits.ordinary).is_err());
    assert!(v3::decode_body(&body2, &limits).is_err());
    assert_ne!(
        v2::transaction_id(&v2_value.body, &limits.ordinary).unwrap(),
        v3::transaction_id(&v3_value.body, &limits).unwrap()
    );
}

#[test]
fn governance_digest_and_all_three_actions_roundtrip() {
    let limits = limits();
    let signed = governance_fixture();
    let data = [0, 1, 0xff, 7];
    let mut independently_framed = v3::GOVERNANCE_ACTION_PREFIX.to_vec();
    independently_framed.extend_from_slice(&17u16.to_be_bytes());
    independently_framed.extend_from_slice(&(data.len() as u32).to_be_bytes());
    independently_framed.extend_from_slice(&data);
    assert_eq!(
        v3::governance_action_digest(17, &data).unwrap(),
        dytallix_protocol_types::sha3_256(&independently_framed)
    );
    assert_eq!(
        hex::encode(v3::governance_action_digest(17, &data).unwrap()),
        "b3410581b5553cf5a072a90f1b3450cb7fbe41f053126fc7dbd74866cc4e5e31"
    );
    let body = v3::signing_bytes(&signed.body, &limits).unwrap();
    let wire = v3::encode(&signed, &limits).unwrap();
    assert_eq!(v3::decode_body(&body, &limits).unwrap(), signed.body);
    assert_eq!(v3::decode(&wire, &limits).unwrap(), signed);
    assert_eq!(
        v3::envelope_hash(&signed, &limits).unwrap(),
        dytallix_protocol_types::sha3_256(&wire)
    );
    assert!(v2::decode(&wire, &limits.ordinary).is_err());
    let view = serde_json::to_value(&signed).unwrap();
    assert_eq!(view["body"]["actions"][2]["choice"], "no_with_veto");
    assert_eq!(
        serde_json::from_value::<v3::SignedOrdinary>(view).unwrap(),
        signed
    );
}

#[test]
fn governance_digest_and_explicit_bounds_reject_invalid_values() {
    let mut signed = governance_fixture();
    let limits = limits();
    if let v3::Action::GovernanceProposal { action_digest, .. } = &mut signed.body.actions[0] {
        action_digest[0] ^= 1;
    }
    assert!(v3::encode(&signed, &limits).is_err());
    signed = governance_fixture();
    if let v3::Action::GovernanceProposal { action_data, .. } = &mut signed.body.actions[0] {
        action_data.push(9);
    }
    assert!(v3::encode(&signed, &limits).is_err());
    signed = governance_fixture();
    let mut short = limits.clone();
    short.max_governance_action_bytes = 3;
    let wire = v3::encode(&signed, &limits).unwrap();
    assert!(v3::encode(&signed, &short).is_err());
    assert!(v3::decode(&wire, &short).is_err());
    short.max_governance_action_bytes = 0;
    assert!(short.validate().is_err());
    short.max_governance_action_bytes = short.max_wire_bytes + 1;
    assert!(short.validate().is_err());
    let mut exact = limits;
    exact.ordinary.max_wire_bytes = wire.len() as u32;
    assert!(v3::encode(&signed, &exact).is_ok());
    exact.ordinary.max_wire_bytes -= 1;
    assert!(v3::encode(&signed, &exact).is_err());
    assert!(v3::decode(&wire, &exact).is_err());
}

#[test]
fn malformed_versions_tags_vote_codes_lengths_and_trailing_bytes_reject() {
    let limits = limits();
    let signed = governance_fixture();
    let body = v3::signing_bytes(&signed.body, &limits).unwrap();
    let wire = v3::encode(&signed, &limits).unwrap();
    for version in [0u16, 2, 4, u16::MAX] {
        let mut changed = wire.clone();
        changed[v3::WIRE_PREFIX.len()..v3::WIRE_PREFIX.len() + 2]
            .copy_from_slice(&version.to_be_bytes());
        assert!(v3::decode(&changed, &limits).is_err());
        let mut changed = body.clone();
        changed[v3::SIGNING_PREFIX.len()..v3::SIGNING_PREFIX.len() + 2]
            .copy_from_slice(&version.to_be_bytes());
        assert!(v3::decode_body(&changed, &limits).is_err());
    }
    for byte in [0, 5, 255] {
        let mut changed = body.clone();
        *changed.last_mut().unwrap() = byte;
        assert!(v3::decode_body(&changed, &limits).is_err());
    }
    let mut changed = body.clone();
    changed[body.len() - 10] = 16; // The final action tag precedes u64 ID and u8 choice.
    assert!(v3::decode_body(&changed, &limits).is_err());
    let marker = [0, 0, 0, 4, 0, 1, 255, 7];
    let position = body
        .windows(marker.len())
        .position(|part| part == marker)
        .unwrap();
    let mut changed = body.clone();
    changed[position..position + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(v3::decode_body(&changed, &limits).is_err());
    let digest = v3::governance_action_digest(17, &[0, 1, 255, 7]).unwrap();
    let position = body.windows(32).position(|part| part == digest).unwrap();
    let mut changed = body.clone();
    changed[position] ^= 1;
    assert!(v3::decode_body(&changed, &limits).is_err());
    let mut changed = wire.clone();
    changed[v3::WIRE_PREFIX.len() + 2..v3::WIRE_PREFIX.len() + 6]
        .copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(v3::decode(&changed, &limits).is_err());
    let mut changed = body.clone();
    changed.push(0);
    assert!(v3::decode_body(&changed, &limits).is_err());
    let mut changed = wire.clone();
    changed.push(0);
    assert!(v3::decode(&changed, &limits).is_err());
    for end in [0, 1, wire.len() - 1] {
        assert!(v3::decode(&wire[..end], &limits).is_err());
    }
    let mut view = serde_json::to_value(&signed).unwrap();
    view["body"]["actions"][2]["choice"] = "maybe".into();
    assert!(serde_json::from_value::<v3::SignedOrdinary>(view).is_err());
    let mut view = serde_json::to_value(&signed).unwrap();
    view["body"]["actions"][0]["unknown"] = true.into();
    assert!(serde_json::from_value::<v3::SignedOrdinary>(view).is_err());
    let mut view = serde_json::to_value(&signed).unwrap();
    view["body"]["actions"][0]
        .as_object_mut()
        .unwrap()
        .remove("action_digest");
    assert!(serde_json::from_value::<v3::SignedOrdinary>(view).is_err());
}

#[test]
fn action_fields_change_transaction_identity() {
    let limits = limits();
    let signed = governance_fixture();
    let original = v3::transaction_id(&signed.body, &limits).unwrap();
    let mut changed = signed.clone();
    if let v3::Action::GovernanceProposal {
        action_class,
        action_digest,
        action_data,
        ..
    } = &mut changed.body.actions[0]
    {
        *action_class += 1;
        *action_digest = v3::governance_action_digest(*action_class, action_data).unwrap();
    }
    assert_ne!(
        v3::transaction_id(&changed.body, &limits).unwrap(),
        original
    );
    let mut changed = signed.clone();
    if let v3::Action::GovernanceDeposit { amount_udgt, .. } = &mut changed.body.actions[1] {
        *amount_udgt += 1;
    }
    assert_ne!(
        v3::transaction_id(&changed.body, &limits).unwrap(),
        original
    );
    let mut changed = signed;
    if let v3::Action::GovernanceVote { choice, .. } = &mut changed.body.actions[2] {
        *choice = v3::VoteChoice::Abstain;
    }
    assert_ne!(
        v3::transaction_id(&changed.body, &limits).unwrap(),
        original
    );
}
