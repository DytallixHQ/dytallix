//! Signing contexts read from a node and checked against a pinned chain
//! (clients v1, K-c): registered accounts, first spends and later-height
//! refreshes.
use dytallix_protocol_types::ordinary_client::AccountDomain;
use dytallix_sdk::{
    ordinary_v2::*,
    ordinary_v3::{self, FeeProfileV3, GovernanceProfileView},
    DytallixKeypair,
};
use serde_json::Value;
use std::sync::OnceLock;

const CHAIN: &str = "sdk-node-context";

fn key() -> &'static DytallixKeypair {
    static KEY: OnceLock<DytallixKeypair> = OnceLock::new();
    KEY.get_or_init(DytallixKeypair::generate)
}
fn identity(k: &DytallixKeypair) -> KeyIdentity {
    KeyIdentity {
        algorithm: "mldsa65".into(),
        public_key: k.public_key().to_vec(),
    }
}
fn pin() -> ChainPin {
    ChainPin {
        network: AddressNetwork::Development,
        chain_id: CHAIN.into(),
        genesis_digest: [7; 32],
    }
}
fn committed(height: u64) -> CommittedContext {
    CommittedContext {
        chain_id: CHAIN.into(),
        genesis_digest: [7; 32],
        height,
        app_hash: [height as u8; 32],
    }
}
fn v3_profile() -> FeeProfileV3 {
    let v: Value = serde_json::from_str(include_str!(
        "../../../vendor/dytallix-protocol-types/tests/fixtures/ordinary_v3_vectors.json"
    ))
    .unwrap();
    serde_json::from_value(v["fee_profile"]["input"].clone()).unwrap()
}
fn fee_profile() -> FeeProfile {
    v3_profile().base
}
fn profile_view(height: u64) -> ProfileView {
    ProfileView {
        version: 1,
        enabled: true,
        context: committed(height),
        config: Some(PublicOrdinaryConfig {
            version: 1,
            fee_profile: fee_profile(),
            max_state_bytes: 1_000_000,
            max_grants: 100,
            max_receipts: 100,
            max_retained_profiles: 10,
            max_transport_bytes: 200_000,
            queue_max_entries: 100,
            queue_max_wire_bytes: 1_000_000,
            queue_max_signature_work: 100,
        }),
    }
}
fn origin() -> AccountAddress {
    pin().origin_address(&identity(key())).unwrap()
}
fn account_view(height: u64) -> AccountView {
    let id = *origin().account_id();
    AccountView {
        version: 1,
        context: committed(height),
        domain: AccountDomain {
            network: 3,
            chain_id: CHAIN.into(),
            genesis_digest: [7; 32],
            account_id: id,
        },
        account_id: id,
        address: origin().encode(),
        current_key: identity(key()),
        authorization_generation: 1,
        spending_nonce: 4,
        protected: false,
        profile_digest: profile_digest(&fee_profile()).unwrap(),
    }
}
fn governance_view(height: u64) -> GovernanceProfileView {
    GovernanceProfileView {
        version: 1,
        enabled: true,
        context: committed(height),
        fee_profile: Some(v3_profile()),
        next_proposal_id: 2,
    }
}

#[test]
fn a_registered_account_context_comes_from_its_record() {
    let id = *origin().account_id();
    let (context, profile) = context_from_views(
        &pin(),
        &profile_view(40),
        &id,
        Some(&account_view(40)),
        &identity(key()),
    )
    .unwrap();
    assert_eq!(profile, fee_profile());
    assert_eq!(
        (context.authorization_generation, context.spending_nonce),
        (1, 4)
    );
    assert_eq!(context.committed, committed(40));
    let signed = prepare(
        &profile,
        &context,
        vec![Action::RewardClaim],
        String::new(),
        100,
        100,
        200,
    )
    .unwrap()
    .sign(&KeypairSigner::new(key()).unwrap())
    .unwrap();
    verify_signature(&signed, &profile.limits).unwrap();

    let mut other = pin();
    other.genesis_digest[0] ^= 1;
    let refused = context_from_views(
        &other,
        &profile_view(40),
        &id,
        Some(&account_view(40)),
        &identity(key()),
    );
    assert!(refused.unwrap_err().to_string().contains("pinned"));
    let mut other = pin();
    other.chain_id.push('x');
    assert!(context_from_views(
        &other,
        &profile_view(40),
        &id,
        Some(&account_view(40)),
        &identity(key())
    )
    .is_err());
    let stranger = identity(&DytallixKeypair::generate());
    assert!(context_from_views(
        &pin(),
        &profile_view(40),
        &id,
        Some(&account_view(40)),
        &stranger
    )
    .is_err());
    assert!(context_from_views(
        &pin(),
        &profile_view(40),
        &id,
        Some(&account_view(41)),
        &identity(key())
    )
    .is_err());
    let mut rotated = account_view(40);
    rotated.current_key = stranger.clone();
    assert!(context_from_views(
        &pin(),
        &profile_view(40),
        &id,
        Some(&rotated),
        &identity(key())
    )
    .is_err());
    let mut protected = account_view(40);
    protected.protected = true;
    assert!(context_from_views(
        &pin(),
        &profile_view(40),
        &id,
        Some(&protected),
        &identity(key())
    )
    .is_err());
}

#[test]
fn a_first_spend_signs_with_the_origin_key_at_generation_and_nonce_zero() {
    let id = *origin().account_id();
    let (context, profile) =
        context_from_views(&pin(), &profile_view(40), &id, None, &identity(key())).unwrap();
    assert_eq!(
        context.domain,
        RecoveryDomain {
            network: 3,
            chain_id: CHAIN.into(),
            genesis_digest: [7; 32],
            account_id: id,
        }
    );
    assert_eq!(
        (
            context.authorization_generation,
            context.spending_nonce,
            context.protected
        ),
        (0, 0, false)
    );
    assert_eq!(context.profile_digest, profile_digest(&profile).unwrap());
    let recipient = *AccountAddress::from_origin_key(
        AddressNetwork::Development,
        CHAIN,
        OriginKeyAlgorithm::MlDsa65,
        DytallixKeypair::generate().public_key(),
    )
    .unwrap()
    .account_id();
    let signed = prepare(
        &profile,
        &context,
        vec![Action::Send {
            recipient,
            denomination: Denomination::Udrt,
            amount: 5,
        }],
        String::new(),
        100,
        100,
        200,
    )
    .unwrap()
    .sign(&KeypairSigner::new(key()).unwrap())
    .unwrap();
    assert_eq!(signed.body.spending_nonce, 0);
    validate_first_spend_views(&context, &profile_view(40)).unwrap();

    // Only the origin key can make the first spend, and only at zero counters.
    let stranger = identity(&DytallixKeypair::generate());
    let refused = context_from_views(&pin(), &profile_view(40), &id, None, &stranger);
    assert!(refused.unwrap_err().to_string().contains("origin key"));
    let mut counted = context.clone();
    counted.spending_nonce = 1;
    assert!(validate_first_spend_views(&counted, &profile_view(40)).is_err());
    let mut elsewhere = context;
    elsewhere.committed.height = 41;
    assert!(validate_first_spend_views(&elsewhere, &profile_view(40)).is_err());
}

#[test]
fn refreshed_views_may_be_later_but_must_show_the_captured_authority() {
    let id = *origin().account_id();
    let (captured, _) = context_from_views(
        &pin(),
        &profile_view(40),
        &id,
        Some(&account_view(40)),
        &identity(key()),
    )
    .unwrap();
    let (fresh, profile) = refresh_views(&captured, &profile_view(45), &account_view(45)).unwrap();
    assert_eq!(fresh.committed, committed(45));
    assert_eq!(profile, fee_profile());
    assert!(refresh_views(&captured, &profile_view(39), &account_view(39)).is_err());
    let error = refresh_views(&captured, &profile_view(45), &account_view(46)).unwrap_err();
    assert!(error.to_string().contains("advanced"), "{error}");
    let mut spent = account_view(45);
    spent.spending_nonce += 1;
    assert!(refresh_views(&captured, &profile_view(45), &spent).is_err());
    let mut repriced = profile_view(45);
    repriced.config.as_mut().unwrap().fee_profile.gas_price += 1;
    assert!(refresh_views(&captured, &repriced, &account_view(45)).is_err());
}

#[test]
fn governance_contexts_need_a_record_and_an_unchanged_v3_profile() {
    let id = *origin().account_id();
    let key = identity(key());
    let refused = ordinary_v3::context_from_views(
        &pin(),
        &profile_view(40),
        &governance_view(40),
        &id,
        None,
        &key,
    );
    assert!(refused.unwrap_err().to_string().contains("first spend"));
    let (captured, profile) = ordinary_v3::context_from_views(
        &pin(),
        &profile_view(40),
        &governance_view(40),
        &id,
        Some(&account_view(40)),
        &key,
    )
    .unwrap();
    assert_eq!(profile, v3_profile());
    let (fresh, _) = ordinary_v3::refresh_views(
        &captured,
        &profile,
        &profile_view(44),
        &governance_view(44),
        &account_view(44),
    )
    .unwrap();
    assert_eq!(fresh.committed.height, 44);
    let mut changed = governance_view(44);
    changed
        .fee_profile
        .as_mut()
        .unwrap()
        .governance_action_costs[0] += 1;
    assert!(ordinary_v3::refresh_views(
        &captured,
        &profile,
        &profile_view(44),
        &changed,
        &account_view(44)
    )
    .is_err());
    assert!(ordinary_v3::refresh_views(
        &captured,
        &profile,
        &profile_view(44),
        &governance_view(45),
        &account_view(44)
    )
    .is_err());
}
