use dytallix_protocol_types::{ordinary_client::AccountDomain, ordinary_v3 as wire};
use dytallix_sdk::{
    ordinary_v2::{
        self, AccountAddress, AccountView, AddressNetwork, CommittedContext, FeeProfile,
        KeyIdentity, KeypairSigner, OriginKeyAlgorithm, ProfileView, PublicOrdinaryConfig,
        RecoveryDomain, SigningContext,
    },
    ordinary_v3::*,
    DytallixKeypair,
};
use serde_json::Value;
use std::sync::OnceLock;

fn key() -> &'static DytallixKeypair {
    static KEY: OnceLock<DytallixKeypair> = OnceLock::new();
    KEY.get_or_init(DytallixKeypair::generate)
}
fn vectors() -> Value {
    serde_json::from_str(include_str!(
        "../../../vendor/dytallix-protocol-types/tests/fixtures/ordinary_v3_vectors.json"
    ))
    .unwrap()
}
/// The vector profile: activation 30, gas limit 100 and fee cap 200 are valid.
fn profile() -> FeeProfileV3 {
    serde_json::from_value(vectors()["fee_profile"]["input"].clone()).unwrap()
}
fn context(p: &FeeProfileV3, k: &DytallixKeypair) -> SigningContext {
    let a = AccountAddress::from_origin_key(
        AddressNetwork::Development,
        "sdk-governance",
        OriginKeyAlgorithm::MlDsa65,
        k.public_key(),
    )
    .unwrap();
    SigningContext {
        domain: RecoveryDomain {
            network: 3,
            chain_id: "sdk-governance".into(),
            genesis_digest: [7; 32],
            account_id: *a.account_id(),
        },
        current_key: KeyIdentity {
            algorithm: "mldsa65".into(),
            public_key: k.public_key().to_vec(),
        },
        authorization_generation: 2,
        spending_nonce: 5,
        committed: CommittedContext {
            chain_id: "sdk-governance".into(),
            genesis_digest: [7; 32],
            height: 40,
            app_hash: [8; 32],
        },
        profile_digest: ordinary_v2::profile_digest(&p.base).unwrap(),
        protected: false,
    }
}
fn prepared(p: &FeeProfileV3, c: &SigningContext, action: Action) -> PreparedGovernance {
    prepare(p, c, action, "governance".into(), 100, 100, 200).unwrap()
}
fn hex(b: &[u8]) -> String {
    b.iter().map(|n| format!("{n:02x}")).collect()
}

#[test]
fn shared_v3_vectors_match_exact_node_bytes() {
    let v = vectors();
    let p = profile();
    assert_eq!(
        hex(&profile_digest(&p).unwrap()),
        v["fee_profile"]["expected_digest"].as_str().unwrap()
    );
    let limits: V3Limits = serde_json::from_value(v["limits"].clone()).unwrap();
    assert_eq!(limits, p.limits());
    for vector in v["vectors"].as_array().unwrap() {
        let signed: SignedOrdinary = serde_json::from_value(vector["input"].clone()).unwrap();
        assert_eq!(
            hex(&wire::signing_bytes(&signed.body, &limits).unwrap()),
            vector["expected_signing_bytes_hex"].as_str().unwrap()
        );
        assert_eq!(
            hex(&transaction_id(&signed.body, &limits).unwrap()),
            vector["expected_transaction_id"].as_str().unwrap()
        );
        assert_eq!(
            hex(&envelope_hash(&signed, &limits).unwrap()),
            vector["expected_envelope_hash"].as_str().unwrap()
        );
        let raw = encode_transport(&signed, &limits, 200_000).unwrap();
        assert_eq!(decode_transport(&raw, &limits, 200_000).unwrap(), signed);
        let envelope: Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(envelope["type"], "ordinary_v3");
        p.validate_signed_request(&signed.body, 30).unwrap();
        // Placeholder signatures of the right size.
        assert!(verify_signature(&signed, &limits).is_err());
        if let [Action::GovernanceProposal {
            proposal_id,
            action_class,
            action_data,
            ..
        }] = signed.body.actions.as_slice()
        {
            let rebuilt = proposal(*proposal_id, *action_class, action_data.clone()).unwrap();
            assert_eq!(rebuilt, signed.body.actions[0]);
            let Action::GovernanceProposal { action_digest, .. } = rebuilt else {
                unreachable!()
            };
            assert_eq!(
                hex(&action_digest),
                vector["expected_governance_action_digest"]
                    .as_str()
                    .unwrap()
            );
        }
    }
}

#[test]
fn each_governance_action_signs_and_rejects_tamper() {
    let p = profile();
    let c = context(&p, key());
    let change = ParameterChange::MaxActive(16);
    for action in [
        parameter_change(3, &change).unwrap(),
        registry_change(
            3,
            &RegistryChange::Remove {
                validator_id: "v1".into(),
            },
        )
        .unwrap(),
        deposit(3, 250),
        vote(3, VoteChoice::NoWithVeto),
    ] {
        let prepared = prepared(&p, &c, action);
        let body = prepared.body();
        assert_eq!(body.ordinary_fee_contract_version, 2);
        assert_eq!(body.fee_profile_digest, profile_digest(&p).unwrap());
        let signed = prepared.sign(&KeypairSigner::new(key()).unwrap()).unwrap();
        verify_signature(&signed, &p.limits()).unwrap();
        assert!(prepared
            .sign(&KeypairSigner::new(&DytallixKeypair::generate()).unwrap())
            .is_err());
        let mut tampered = signed.clone();
        tampered.body.spending_nonce += 1;
        assert!(verify_signature(&tampered, &p.limits()).is_err());
    }
    let Action::GovernanceProposal { action_data, .. } = parameter_change(3, &change).unwrap()
    else {
        unreachable!()
    };
    assert_eq!(action_data, change.action_data());
}

#[test]
fn bodies_outside_the_v3_contract_are_refused_before_signing() {
    let p = profile();
    let c = context(&p, key());
    let b = prepared(&p, &c, vote(1, VoteChoice::Yes)).body().clone();
    let mut mutations = Vec::new();
    let mut v = b.clone();
    v.actions.push(deposit(1, 1));
    mutations.push(v);
    let mut v = b.clone();
    v.actions = vec![Action::RewardClaim];
    mutations.push(v);
    let mut v = b.clone();
    v.actions.clear();
    mutations.push(v);
    let mut v = b.clone();
    v.ordinary_fee_contract_version = 1;
    mutations.push(v);
    let mut v = b.clone();
    v.fee_profile_digest[0] ^= 1;
    mutations.push(v);
    let mut v = b.clone();
    v.fee_profile_version += 1;
    mutations.push(v);
    let mut v = b.clone();
    v.spending_nonce += 1;
    mutations.push(v);
    let mut v = b.clone();
    v.expiry_height = 41;
    mutations.push(v);
    let mut v = b.clone();
    v.expiry_height = 1042;
    mutations.push(v);
    let mut v = b.clone();
    v.maximum_fee = 1;
    mutations.push(v);
    for body in mutations {
        assert!(
            PreparedGovernance::from_body(body.clone(), &p, &c).is_err(),
            "{body:?}"
        );
    }
    // The profile activates at 30: committed height 28 targets 29.
    let mut early = c.clone();
    early.committed.height = 28;
    assert!(prepare(
        &p,
        &early,
        vote(1, VoteChoice::Yes),
        String::new(),
        40,
        100,
        200
    )
    .is_err());
    let mut protected = c;
    protected.protected = true;
    assert!(PreparedGovernance::from_body(b, &p, &protected).is_err());
}

#[test]
fn views_require_governance_enabled_at_the_expected_context() {
    let p = profile();
    let c = context(&p, key());
    let base: FeeProfile = p.base.clone();
    let pv = ProfileView {
        version: 1,
        enabled: true,
        context: c.committed.clone(),
        config: Some(PublicOrdinaryConfig {
            version: 1,
            fee_profile: base,
            max_state_bytes: 1_000_000,
            max_grants: 100,
            max_receipts: 100,
            max_retained_profiles: 10,
            max_transport_bytes: 200_000,
            queue_max_entries: 100,
            queue_max_wire_bytes: 1_000_000,
            queue_max_signature_work: 100,
        }),
    };
    let gv = GovernanceProfileView {
        version: 1,
        enabled: true,
        context: c.committed.clone(),
        fee_profile: Some(p.clone()),
        next_proposal_id: 4,
    };
    let av = AccountView {
        version: 1,
        context: c.committed.clone(),
        domain: AccountDomain {
            network: 3,
            chain_id: c.domain.chain_id.clone(),
            genesis_digest: c.domain.genesis_digest,
            account_id: c.domain.account_id,
        },
        account_id: c.domain.account_id,
        address: AccountAddress::from_account_id(AddressNetwork::Development, c.domain.account_id)
            .encode(),
        current_key: c.current_key.clone(),
        authorization_generation: c.authorization_generation,
        spending_nonce: c.spending_nonce,
        protected: false,
        profile_digest: c.profile_digest,
    };
    assert_eq!(validate_views(&c, &pv, &gv, &av).unwrap(), p);
    let raw = serde_json::to_string(&gv).unwrap();
    assert!(raw.contains("\"next_proposal_id\":\"4\""));
    assert_eq!(
        serde_json::from_str::<GovernanceProfileView>(&raw).unwrap(),
        gv
    );
    let mut bad = gv.clone();
    bad.context.height += 1;
    assert!(validate_views(&c, &pv, &bad, &av).is_err());
    let mut bad = gv.clone();
    bad.enabled = false;
    assert!(validate_views(&c, &pv, &bad, &av).is_err());
    let mut bad = gv.clone();
    bad.next_proposal_id = 0;
    assert!(validate_views(&c, &pv, &bad, &av).is_err());
    let mut bad = gv.clone();
    bad.fee_profile = None;
    assert!(validate_views(&c, &pv, &bad, &av).is_err());
    let mut bad = av.clone();
    bad.spending_nonce += 1;
    assert!(validate_views(&c, &pv, &gv, &bad).is_err());
}

#[test]
fn transport_type_and_bounds_are_exact() {
    let p = profile();
    let c = context(&p, key());
    let signed = prepared(&p, &c, vote(1, VoteChoice::Abstain))
        .sign(&KeypairSigner::new(key()).unwrap())
        .unwrap();
    let raw = encode_transport(&signed, &p.limits(), 200_000).unwrap();
    assert!(encode_transport(&signed, &p.limits(), raw.len() - 1).is_err());
    assert!(decode_transport(&raw, &p.limits(), raw.len() - 1).is_err());
    let v2 = String::from_utf8(raw)
        .unwrap()
        .replace("ordinary_v3", "ordinary_v2");
    assert!(decode_transport(v2.as_bytes(), &p.limits(), 200_000).is_err());
}
