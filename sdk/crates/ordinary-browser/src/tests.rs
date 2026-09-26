//! Public synthetic vectors. Placeholder signatures are never validity evidence.
use super::*;
use dytallix_protocol_types::ordinary_client::{
    AccountDomain, CommittedContext, PublicOrdinaryConfig, ReceiptOutcome,
};
use serde_json::Value;
fn wire_vectors() -> Value {
    serde_json::from_str(include_str!(
        "../../../vendor/dytallix-protocol-types/tests/fixtures/ordinary_v2_vectors.json"
    ))
    .unwrap()
}
fn fee_vectors() -> Value {
    serde_json::from_str(include_str!(
        "../../../vendor/dytallix-protocol-types/tests/fixtures/ordinary_fee_v1_vectors.json"
    ))
    .unwrap()
}
fn json_string(v: &impl Serialize) -> String {
    serde_json::to_string(v).unwrap()
}
fn browser_limits(value: &Value) -> String {
    let mut v = value.clone();
    v["max_expiry_lifetime"] = json!(v["max_expiry_lifetime"].as_u64().unwrap().to_string());
    json_string(&v)
}
struct Inputs {
    profile: FeeProfile,
    view: ProfileView,
    account: AccountView,
    anchor: Anchor,
    key: KeyIdentity,
    intent: Intent,
}
fn inputs() -> Inputs {
    let profile: FeeProfile = serde_json::from_value(fee_vectors()["profile"].clone()).unwrap();
    let signed: SignedOrdinary =
        serde_json::from_value(wire_vectors()["vectors"][0]["input"].clone()).unwrap();
    let domain = signed.body.domain;
    let digest = ordinary_fees::profile_digest(&profile).unwrap();
    let committed = CommittedContext {
        chain_id: domain.chain_id.clone(),
        genesis_digest: domain.genesis_digest,
        height: 19,
        app_hash: [4; 32],
    };
    let account = AccountView {
        version: 1,
        context: committed.clone(),
        domain: AccountDomain {
            network: domain.network,
            chain_id: domain.chain_id.clone(),
            genesis_digest: domain.genesis_digest,
            account_id: domain.account_id,
        },
        account_id: domain.account_id,
        address: AccountAddress::from_account_id(AddressNetwork::Development, domain.account_id)
            .encode(),
        current_key: signed.body.key.clone(),
        authorization_generation: 9_007_199_254_740_993,
        spending_nonce: 9_007_199_254_740_995,
        protected: false,
        profile_digest: digest,
    };
    let view = ProfileView {
        version: 1,
        enabled: true,
        context: committed.clone(),
        config: Some(PublicOrdinaryConfig {
            version: 1,
            fee_profile: profile.clone(),
            max_state_bytes: 1_000_000,
            max_grants: 100,
            max_receipts: 100,
            max_retained_profiles: 4,
            max_transport_bytes: 1_000_000,
            queue_max_entries: 100,
            queue_max_wire_bytes: 1_000_000,
            queue_max_signature_work: 100,
        }),
    };
    Inputs {
        profile,
        view,
        account,
        anchor: Anchor {
            domain,
            committed,
            profile_digest: digest,
        },
        key: signed.body.key,
        intent: Intent {
            actions: vec![wire::Action::Data {
                data: "browser vector".into(),
            }],
            memo: "exact bytes".into(),
            expiry_height: 30,
            gas_limit: 100_000,
            maximum_fee: 200_000,
        },
    }
}
fn context_json(i: &Inputs) -> String {
    context(
        &json_string(&i.anchor),
        &json_string(&i.view),
        &json_string(&i.account),
        &json_string(&i.key),
    )
    .unwrap()
}
fn prepared(i: &Inputs) -> Value {
    serde_json::from_str(
        &prepare(
            &context_json(i),
            &json_string(&i.view),
            &json_string(&i.account),
            &json_string(&i.anchor),
            &json_string(&i.intent),
            &json_string(&i.key),
        )
        .unwrap(),
    )
    .unwrap()
}
fn signed(i: &Inputs) -> SignedOrdinary {
    SignedOrdinary {
        body: serde_json::from_value(prepared(i)["body"].clone()).unwrap(),
        signature: vec![3; wire::signature_size(&i.key.algorithm).unwrap()],
    }
}
fn receipt(i: &Inputs, signed: &SignedOrdinary) -> ReceiptView {
    ReceiptView {
        version: 1,
        context: CommittedContext {
            height: 20,
            ..i.anchor.committed.clone()
        },
        transaction_id: wire::transaction_id(&signed.body, &i.profile.limits).unwrap(),
        envelope_hash: wire::envelope_hash(signed, &i.profile.limits).unwrap(),
        actor: signed.body.domain.account_id,
        block_height: 20,
        block_index: 0,
        contract_version: i.profile.ordinary_fee_contract_version,
        profile_version: i.profile.version,
        profile_digest: i.anchor.profile_digest,
        outcome: ReceiptOutcome::Success,
        failing_action: None,
        failure_phase: None,
        rule_class: None,
        rule_code: None,
        gas_limit: signed.body.gas_limit,
        gas_used: 10_000,
        metadata_gas: i.profile.receipt_metadata_cost,
        reserved_cap: signed.body.maximum_fee,
        charge: 20_000,
        released_cap: 180_000,
        nonce_before: signed.body.spending_nonce,
        nonce_after: signed.body.spending_nonce + 1,
    }
}
#[test]
fn independent_full_wire_vectors_cover_both_algorithms_and_all_twelve_actions() {
    let data = wire_vectors();
    let limits = browser_limits(&data["limits"]);
    for vector in data["vectors"].as_array().unwrap() {
        let signed_json = json_string(&vector["input"]);
        let body_json = json_string(&vector["input"]["body"]);
        let message = signing_bytes(&body_json, &limits).unwrap();
        let encoded = encode_signed(&signed_json, &limits).unwrap();
        assert_eq!(hex::encode(&message), vector["expected_signing_bytes_hex"]);
        assert_eq!(hex::encode(&encoded), vector["expected_envelope_hex"]);
        assert_eq!(
            serde_json::from_str::<Value>(&decode_envelope(encoded.clone(), &limits).unwrap())
                .unwrap(),
            vector["input"]
        );
        assert_eq!(
            hex::encode(dytallix_protocol_types::sha3_256(&message)),
            vector["expected_transaction_id"]
        );
        assert_eq!(
            hex::encode(dytallix_protocol_types::sha3_256(&encoded)),
            vector["expected_envelope_hash"]
        );
        let signed: SignedOrdinary = serde_json::from_value(vector["input"].clone()).unwrap();
        assert_eq!(
            hex::encode(wire::key_id(&signed.body.key).unwrap()),
            vector["expected_key_id"]
        );
    }
    assert_eq!(
        data["vectors"][1]["input"]["body"]["actions"]
            .as_array()
            .unwrap()
            .len(),
        12
    );
}
#[test]
fn independent_profile_vector_and_38_mutations_are_exact() {
    let v = fee_vectors();
    assert_eq!(
        profile_digest(&json_string(&v["profile"])).unwrap(),
        v["expected_digest"]
    );
    assert_eq!(
        hex::encode(profile_bytes(&json_string(&v["profile"])).unwrap()),
        v["expected_profile_hex"]
    );
    assert_eq!(v["mutations"].as_array().unwrap().len(), 38);
    for m in v["mutations"].as_array().unwrap() {
        let mut p = v["profile"].clone();
        *p.pointer_mut(m["pointer"].as_str().unwrap()).unwrap() = m["value"].clone();
        let bytes = profile_bytes(&json_string(&p)).unwrap();
        assert_eq!(hex::encode(&bytes), m["expected_profile_hex"]);
        assert_eq!(
            profile_digest(&json_string(&p)).unwrap(),
            m["expected_digest"]
        );
        assert_eq!(
            serde_json::from_str::<Value>(&decode_profile(bytes).unwrap()).unwrap(),
            p
        );
    }
}
#[test]
fn independent_address_and_origin_vectors_preserve_stable_identity() {
    let v: Value = serde_json::from_str(include_str!(
        "../../../vendor/dytallix-protocol-types/tests/fixtures/address-v1.json"
    ))
    .unwrap();
    let code = |s: &str| match s {
        "mainnet" => 1,
        "testnet" => 2,
        "development" => 3,
        _ => panic!("unknown fixture network"),
    };
    for c in v["encoding"].as_array().unwrap() {
        let n = code(c["network"].as_str().unwrap());
        assert_eq!(
            account_address(n, c["account_id"].as_str().unwrap()).unwrap(),
            c["address"]
        );
        assert_eq!(
            decode_address(n, c["address"].as_str().unwrap()).unwrap(),
            c["account_id"]
        );
    }
    for c in v["origins"].as_array().unwrap() {
        let n = code(c["network"].as_str().unwrap());
        let public = (0..c["public_key_len"].as_u64().unwrap())
            .map(|i| (i % 251) as u8)
            .collect();
        let result = origin_address(
            n,
            c["chain_id"].as_str().unwrap(),
            c["algorithm"].as_str().unwrap(),
            public,
        );
        if c["algorithm"] == "legacy_dilithium5" {
            assert!(result.is_err());
        } else {
            let value: Value = serde_json::from_str(&result.unwrap()).unwrap();
            assert_eq!(value["account_id"], c["account_id"]);
            assert_eq!(value["address"], c["address"]);
        }
    }
    assert_eq!(v["invalid"].as_array().unwrap().len(), 10);
    for c in v["invalid"].as_array().unwrap() {
        assert!(
            decode_address(
                code(c["network"].as_str().unwrap()),
                c["address"].as_str().unwrap()
            )
            .is_err(),
            "{}",
            c["name"]
        );
    }
    assert!(decode_address(2, &account_address(3, &"00".repeat(32)).unwrap()).is_err());
}
#[test]
fn prepare_checks_pinned_anchor_current_key_and_precision_without_signing() {
    let i = inputs();
    let result = prepared(&i);
    assert_eq!(
        result["body"]["authorization_generation"],
        "9007199254740993"
    );
    assert_eq!(result["body"]["spending_nonce"], "9007199254740995");
    assert_eq!(result["signature_verified"], false);
    let mut changed = i.anchor.clone();
    changed.committed.app_hash[0] ^= 1;
    assert!(prepare(
        &context_json(&i),
        &json_string(&i.view),
        &json_string(&i.account),
        &json_string(&changed),
        &json_string(&i.intent),
        &json_string(&i.key)
    )
    .is_err());
    let mut changed = i.key.clone();
    changed.public_key[0] ^= 1;
    assert!(prepare(
        &context_json(&i),
        &json_string(&i.view),
        &json_string(&i.account),
        &json_string(&i.anchor),
        &json_string(&i.intent),
        &json_string(&changed)
    )
    .is_err());
    let mut changed = i.account.clone();
    changed.spending_nonce += 1;
    assert!(prepare(
        &context_json(&i),
        &json_string(&i.view),
        &json_string(&changed),
        &json_string(&i.anchor),
        &json_string(&i.intent),
        &json_string(&i.key)
    )
    .is_err());
}
#[test]
fn protected_exhausted_not_yet_active_and_expired_contexts_reject() {
    let mut i = inputs();
    i.account.protected = true;
    assert!(context(
        &json_string(&i.anchor),
        &json_string(&i.view),
        &json_string(&i.account),
        &json_string(&i.key)
    )
    .is_err());
    let mut i = inputs();
    i.account.spending_nonce = u64::MAX;
    assert!(context(
        &json_string(&i.anchor),
        &json_string(&i.view),
        &json_string(&i.account),
        &json_string(&i.key)
    )
    .is_err());
    let mut i = inputs();
    i.view.context.height = 18;
    i.account.context.height = 18;
    i.anchor.committed.height = 18;
    assert!(context(
        &json_string(&i.anchor),
        &json_string(&i.view),
        &json_string(&i.account),
        &json_string(&i.key)
    )
    .is_err());
    let mut i = inputs();
    i.intent.expiry_height = 20;
    assert!(prepare(
        &context_json(&i),
        &json_string(&i.view),
        &json_string(&i.account),
        &json_string(&i.anchor),
        &json_string(&i.intent),
        &json_string(&i.key)
    )
    .is_err());
}
#[test]
fn attached_placeholder_is_never_marked_verified_and_hashes_bind_exact_transport() {
    let i = inputs();
    let s = signed(&i);
    let a: Value = serde_json::from_str(
        &attach_signature(
            &json_string(&s.body),
            &json_string(&i.view),
            s.signature.clone(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(a["signature_verified"], false);
    let b: Value =
        serde_json::from_str(&inspect_signed(&json_string(&s), &json_string(&i.view)).unwrap())
            .unwrap();
    assert_eq!(a, b);
    let bytes: Vec<u8> = serde_json::from_value(a["transport_bytes"].clone()).unwrap();
    assert_eq!(a["comet_hash"], hex::encode(Sha256::digest(&bytes)));
    let raw = String::from_utf8(bytes).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&decode_transport(&raw, &json_string(&i.view)).unwrap())
            .unwrap(),
        a
    );
    assert!(decode_transport(&(raw + " "), &json_string(&i.view)).is_err());
    let mut changed = s.clone();
    changed.signature[0] ^= 1;
    let b: Value = serde_json::from_str(
        &inspect_signed(&json_string(&changed), &json_string(&i.view)).unwrap(),
    )
    .unwrap();
    assert_eq!(a["transaction_id"], b["transaction_id"]);
    assert_ne!(a["envelope_hash"], b["envelope_hash"]);
    assert_ne!(a["comet_hash"], b["comet_hash"]);
    assert!(attach_signature(&json_string(&s.body), &json_string(&i.view), vec![0; 3308]).is_err());
}
#[test]
fn receipt_validation_checks_all_outcomes_and_explicit_nonproof_boundary() {
    let mut i = inputs();
    i.intent.actions = vec![wire::Action::DmsClaim {
        owner: [6; 32],
        expected_grant_generation: 0,
    }];
    let s = signed(&i);
    let mut r = receipt(&i, &s);
    let check = |r: &ReceiptView| {
        validate_receipt(&json_string(r), &json_string(&s), &json_string(&i.profile))
    };
    let result: Value = serde_json::from_str(&check(&r).unwrap()).unwrap();
    assert_eq!(
        result,
        json!({"consistent":true,"transaction_id":hex::encode(r.transaction_id),"signature_verified":false,"consensus_verified":false})
    );
    r.outcome = ReceiptOutcome::ApplicationFailure;
    r.failing_action = Some(0);
    r.failure_phase = Some("APPLICATION".into());
    r.rule_class = Some("ACTION_STATE_PRECONDITION".into());
    r.rule_code = Some("DMS_INACTIVITY_DELAY".into());
    assert!(check(&r).is_ok());
    r.outcome = ReceiptOutcome::OutOfGas;
    r.failure_phase = Some("WRITE".into());
    r.rule_class = None;
    r.rule_code = Some("OUT_OF_GAS".into());
    r.gas_used = r.gas_limit;
    r.charge = r.reserved_cap;
    r.released_cap = 0;
    assert!(check(&r).is_ok());
    r.charge -= 1;
    assert!(check(&r).is_err());
    r.charge += 1;
    r.nonce_after += 1;
    assert!(check(&r).is_err());
    let mut r = receipt(&i, &s);
    r.envelope_hash[0] ^= 1;
    assert!(check(&r).is_err());
    let mut r = receipt(&i, &s);
    r.metadata_gas += 1;
    assert!(check(&r).is_err());
    let mut r = receipt(&i, &s);
    r.gas_used = 0;
    r.charge = 20;
    r.released_cap = r.reserved_cap - 20;
    assert!(check(&r).is_err());
}
#[test]
fn structured_input_rejects_duplicates_unknown_fields_bad_numbers_and_unicode() {
    for input in [
        r#"{"a":1,"a":2}"#,
        r#"{"a":{"x":1,"x":2}}"#,
        r#"{"a":"\ud800"}"#,
    ] {
        assert!(parse_json(input).is_err());
    }
    let i = inputs();
    let mut key = serde_json::to_value(&i.key).unwrap();
    key["private_key"] = json!([1, 2, 3]);
    assert!(context(
        &json_string(&i.anchor),
        &json_string(&i.view),
        &json_string(&i.account),
        &json_string(&key)
    )
    .is_err());
    for number in [
        json!(1),
        json!("01"),
        json!("+1"),
        json!("1e2"),
        json!("18446744073709551616"),
    ] {
        let mut intent = serde_json::to_value(&i.intent).unwrap();
        intent["expiry_height"] = number;
        assert!(prepare(
            &context_json(&i),
            &json_string(&i.view),
            &json_string(&i.account),
            &json_string(&i.anchor),
            &json_string(&intent),
            &json_string(&i.key)
        )
        .is_err());
    }
    assert!(parse_json(&" ".repeat(strict_json::MAX_JSON_BYTES + 1)).is_err());
    assert_eq!(
        parse_token_units("9007199254740993.000001").unwrap(),
        "9007199254740993000001"
    );
    assert!(parse_token_units("340282366920938463463374607431768211455").is_err());
    assert!(parse_token_units("1.0000001").is_err());
    let duplicate =
        json_string(&i.profile).replacen("\"mldsa65\":", "\"mldsa65\":\"1\",\"mldsa65\":", 1);
    assert!(profile_digest(&duplicate).is_err());
}
#[test]
fn binary_bounds_and_large_unsigned_values_remain_exact() {
    let data = wire_vectors();
    let mut l = data["limits"].clone();
    l["max_expiry_lifetime"] = json!("18446744073709551615");
    let mut body = data["vectors"][0]["input"]["body"].clone();
    body["authorization_generation"] = json!(u64::MAX.to_string());
    body["spending_nonce"] = json!(u64::MAX.to_string());
    body["maximum_fee"] = json!(u128::MAX.to_string());
    let bytes = signing_bytes(&json_string(&body), &json_string(&l)).unwrap();
    let decoded = wire::decode_body(&bytes, &limits(&json_string(&l)).unwrap()).unwrap();
    assert_eq!(decoded.authorization_generation, u64::MAX);
    assert_eq!(decoded.maximum_fee, u128::MAX);
    l["max_wire_bytes"] = json!(MAX_BINARY_BYTES + 1);
    assert!(signing_bytes(&json_string(&body), &json_string(&l)).is_err());
    assert!(decode_profile(vec![0; 513]).is_err());
    assert!(decode_envelope(
        vec![0; MAX_BINARY_BYTES + 1],
        &browser_limits(&data["limits"])
    )
    .is_err());
}
