//! Local format evidence. Values are synthetic and do not set production fees.
use dytallix_protocol_types::{
    ordinary_fees::{self as v1, FeeProfile},
    ordinary_fees_v3::{self as v3_fees, FeeProfileV3},
    ordinary_v3::{self as v3, Action, OrdinaryTransaction, VoteChoice},
};
use serde_json::Value;

fn base_profile() -> FeeProfile {
    let vectors: Value =
        serde_json::from_str(include_str!("fixtures/ordinary_fee_v1_vectors.json")).unwrap();
    serde_json::from_value(vectors["profile"].clone()).unwrap()
}

fn profile() -> FeeProfileV3 {
    FeeProfileV3 {
        base: base_profile(),
        version: 9,
        activation_height: 30,
        max_governance_action_bytes: 128,
        governance_action_costs: [31, 32, 33],
    }
}

fn body(profile: &FeeProfileV3) -> OrdinaryTransaction {
    let vectors: Value =
        serde_json::from_str(include_str!("fixtures/ordinary_v2_vectors.json")).unwrap();
    let mut body: OrdinaryTransaction =
        serde_json::from_value(vectors["vectors"][0]["input"]["body"].clone()).unwrap();
    body.ordinary_fee_contract_version = v3_fees::ORDINARY_FEE_CONTRACT_VERSION;
    body.fee_profile_version = profile.version;
    body.fee_profile_digest = v3_fees::profile_digest(profile).unwrap();
    body.gas_limit = 100;
    body.maximum_fee = 200;
    body.actions = vec![Action::GovernanceVote {
        proposal_id: 23,
        choice: VoteChoice::Yes,
    }];
    body
}

#[test]
fn profile_roundtrip_and_action_costs_are_version_separated() {
    let profile = profile();
    let bytes = v3_fees::profile_bytes(&profile).unwrap();
    assert_eq!(v3_fees::decode_profile(&bytes).unwrap(), profile);
    assert_eq!(
        v3_fees::profile_digest(&profile).unwrap(),
        dytallix_protocol_types::sha3_256(&bytes)
    );
    assert!(v1::decode_profile(&bytes).is_err());
    assert!(v3_fees::decode_profile(&v1::profile_bytes(&profile.base).unwrap()).is_err());
    for tag in 1..=12 {
        assert_eq!(
            profile.action_cost(tag).unwrap(),
            profile.base.action_costs[usize::from(tag - 1)]
        );
    }
    for (tag, expected) in [(13, 31), (14, 32), (15, 33)] {
        assert_eq!(profile.action_cost(tag).unwrap(), expected);
    }
    for tag in [0, 16, u8::MAX] {
        assert!(profile.action_cost(tag).is_err());
    }
}

#[test]
fn signed_request_requires_exact_profile_and_active_height() {
    let profile = profile();
    let original = body(&profile);
    assert!(v3::signing_bytes(&original, &profile.limits()).is_ok());
    assert!(profile.validate_signed_request(&original, 30).is_ok());
    assert!(profile.validate_signed_request(&original, 29).is_err());

    let mut changed = original.clone();
    changed.ordinary_fee_contract_version = 1;
    assert!(profile.validate_signed_request(&changed, 30).is_err());
    changed = original.clone();
    changed.fee_profile_version += 1;
    assert!(profile.validate_signed_request(&changed, 30).is_err());
    changed = original.clone();
    changed.fee_profile_digest[0] ^= 1;
    assert!(profile.validate_signed_request(&changed, 30).is_err());
    changed = original.clone();
    changed.maximum_fee -= 1;
    assert!(profile.validate_signed_request(&changed, 30).is_err());
    changed = original;
    changed.gas_limit = 0;
    assert!(profile.validate_signed_request(&changed, 30).is_err());

    let mut changed = body(&profile);
    changed.actions = vec![Action::GovernanceProposal {
        proposal_id: 23,
        action_class: 17,
        action_data: vec![1],
        action_digest: [0; 32],
    }];
    assert!(profile.validate_signed_request(&changed, 30).is_err());
}

#[test]
fn v3_fee_admission_rejects_ordinary_and_mixed_actions() {
    let profile = profile();
    let original = body(&profile);
    assert!(profile.validate_signed_request(&original, 30).is_ok());

    let mut changed = original.clone();
    changed.actions.clear();
    assert!(profile.validate_signed_request(&changed, 30).is_err());

    changed = original.clone();
    changed.actions = vec![Action::Data { data: "x".into() }];
    assert!(profile.validate_signed_request(&changed, 30).is_err());

    changed = original.clone();
    changed.actions.push(Action::Data { data: "x".into() });
    assert!(profile.validate_signed_request(&changed, 30).is_err());

    changed = original.clone();
    changed.actions.push(Action::GovernanceVote {
        proposal_id: 24,
        choice: VoteChoice::No,
    });
    assert!(profile.validate_signed_request(&changed, 30).is_err());

    changed = original;
    changed.actions = vec![Action::GovernanceDeposit {
        proposal_id: 23,
        amount_udgt: 1,
    }];
    assert!(profile.validate_signed_request(&changed, 30).is_ok());
}

#[test]
fn every_extension_field_changes_digest_and_bad_binary_rejects() {
    let profile = profile();
    let original = v3_fees::profile_bytes(&profile).unwrap();
    let digest = v3_fees::profile_digest(&profile).unwrap();
    let mut variants = Vec::new();
    let mut changed = profile.clone();
    changed.version += 1;
    variants.push(changed);
    let mut changed = profile.clone();
    changed.activation_height += 1;
    variants.push(changed);
    let mut changed = profile.clone();
    changed.max_governance_action_bytes += 1;
    variants.push(changed);
    for index in 0..3 {
        let mut changed = profile.clone();
        changed.governance_action_costs[index] += 1;
        variants.push(changed);
    }
    let mut changed = profile.clone();
    changed.base.gas_price += 1;
    variants.push(changed);
    for variant in variants {
        assert_ne!(v3_fees::profile_digest(&variant).unwrap(), digest);
    }
    for end in 0..original.len() {
        assert!(
            v3_fees::decode_profile(&original[..end]).is_err(),
            "truncation {end}"
        );
    }
    let mut trailing = original.clone();
    trailing.push(0);
    assert!(v3_fees::decode_profile(&trailing).is_err());
    let mut wrong_format = original.clone();
    wrong_format[v3_fees::PROFILE_PREFIX.len() + 1] = 2;
    assert!(v3_fees::decode_profile(&wrong_format).is_err());
    let mut wrong_contract = original;
    wrong_contract[v3_fees::PROFILE_PREFIX.len() + 3] = 1;
    assert!(v3_fees::decode_profile(&wrong_contract).is_err());
}

#[test]
fn explicit_json_and_bounds_have_no_defaults() {
    let profile = profile();
    let view = serde_json::to_value(&profile).unwrap();
    assert_eq!(view["version"], "9");
    assert_eq!(
        view["governance_action_costs"],
        serde_json::json!(["31", "32", "33"])
    );
    assert_eq!(
        serde_json::from_value::<FeeProfileV3>(view.clone()).unwrap(),
        profile
    );
    for key in [
        "base",
        "version",
        "activation_height",
        "max_governance_action_bytes",
        "governance_action_costs",
    ] {
        let mut missing = view.clone();
        missing.as_object_mut().unwrap().remove(key);
        assert!(serde_json::from_value::<FeeProfileV3>(missing).is_err());
    }
    let mut unknown = view.clone();
    unknown["extra"] = serde_json::json!(1);
    assert!(serde_json::from_value::<FeeProfileV3>(unknown).is_err());
    let mut bad = profile.clone();
    bad.version = 0;
    assert!(bad.validate().is_err());
    bad = profile.clone();
    bad.activation_height = bad.base.activation_height - 1;
    assert!(bad.validate().is_err());
    bad = profile.clone();
    bad.max_governance_action_bytes = 0;
    assert!(bad.validate().is_err());
    bad.max_governance_action_bytes = bad.base.limits.max_wire_bytes + 1;
    assert!(bad.validate().is_err());
}
