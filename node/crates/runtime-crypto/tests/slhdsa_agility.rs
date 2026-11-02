//! Crypto-agility spike: a hash-based account algorithm (FIPS 205
//! SLH-DSA-SHAKE-192s, code 3) through the ordinary-v2 verifier. Development
//! networks accept it; mainnet policy still refuses it. These tests measure the
//! swap path only and approve nothing for production.

use dytallix_protocol_types::{
    address::{AccountAddress, AddressNetwork, OriginKeyAlgorithm},
    algorithms,
    ordinary::{self as wire, Action, Denomination, Limits, OrdinaryTransaction, SignedOrdinary},
    recovery::{KeyIdentity, RecoveryDomain},
};
use dytallix_runtime_crypto::ordinary::{verify_bytes, verify_signed, OrdinaryVerificationError};
use fips205::slh_dsa_shake_192s;
use fips205::traits::{SerDes, Signer};
use std::collections::BTreeSet;
use std::time::Instant;

const SLH: &str = "slhdsa-shake-192s";

fn limits() -> Limits {
    Limits {
        max_wire_bytes: 65_536,
        max_actions: 4,
        max_identifier_bytes: 64,
        max_data_bytes: 256,
        max_memo_bytes: 64,
        max_consensus_key_bytes: 4096,
        max_proof_bytes: 8192,
        max_expiry_lifetime: 100,
        allowed_algorithms: BTreeSet::from(["mldsa65".into(), SLH.into()]),
    }
}

fn body(network: u8, key: KeyIdentity) -> OrdinaryTransaction {
    OrdinaryTransaction {
        domain: RecoveryDomain {
            network,
            chain_id: "agility-fixture".into(),
            genesis_digest: [1; 32],
            account_id: [2; 32],
        },
        authorization_generation: 0,
        spending_nonce: 0,
        key,
        expiry_height: 80,
        ordinary_fee_contract_version: 1,
        fee_profile_version: 1,
        fee_profile_digest: [7; 32],
        fee_denomination: Denomination::Udrt,
        maximum_fee: 50_000,
        gas_limit: 10_000,
        memo: String::new(),
        actions: vec![Action::Send {
            recipient: [8; 32],
            denomination: Denomination::Udrt,
            amount: 9,
        }],
    }
}

fn signed(network: u8) -> SignedOrdinary {
    let (public, secret) = slh_dsa_shake_192s::try_keygen().unwrap();
    let key = KeyIdentity {
        algorithm: SLH.into(),
        public_key: public.into_bytes().to_vec(),
    };
    let body = body(network, key);
    let message = wire::signing_bytes(&body, &limits()).unwrap();
    let started = Instant::now();
    let signature = secret.try_sign(&message, &[], false).unwrap().to_vec();
    eprintln!("slh-dsa-shake-192s sign: {:?}", started.elapsed());
    SignedOrdinary { body, signature }
}

#[test]
fn development_network_accepts_hash_based_account_signature() {
    let tx = signed(3);
    let bytes = wire::encode(&tx, &limits()).unwrap();
    eprintln!("ordinary-v2 envelope with one Send: {} bytes", bytes.len());
    let started = Instant::now();
    let verified = verify_bytes(&bytes, &limits()).unwrap();
    eprintln!("slh-dsa-shake-192s verify: {:?}", started.elapsed());
    assert_eq!(verified.body(), &tx.body);
}

#[test]
fn mainnet_policy_still_refuses_it() {
    let tx = signed(1);
    assert!(matches!(
        verify_signed(&tx, &limits()),
        Err(OrdinaryVerificationError::UnsupportedAlgorithm)
    ));
    assert!(!algorithms::mainnet_approved(SLH));
    assert!(algorithms::mainnet_approved("mldsa65"));
}

#[test]
fn tampered_or_truncated_signatures_reject() {
    let mut tx = signed(3);
    tx.signature[100] ^= 1;
    assert!(matches!(
        verify_signed(&tx, &limits()),
        Err(OrdinaryVerificationError::InvalidSignature)
    ));
    tx.signature.pop();
    assert!(verify_signed(&tx, &limits()).is_err());
}

#[test]
fn role_allowlist_still_gates_the_new_algorithm() {
    let tx = signed(3);
    let mut only_mldsa = limits();
    only_mldsa.allowed_algorithms = BTreeSet::from(["mldsa65".into()]);
    assert!(verify_signed(&tx, &only_mldsa).is_err());
}

#[test]
fn address_commits_to_algorithm_code() {
    let (public, _) = slh_dsa_shake_192s::try_keygen().unwrap();
    let key = public.into_bytes();
    let a = AccountAddress::from_origin_key(
        AddressNetwork::Development,
        "agility-fixture",
        OriginKeyAlgorithm::SlhDsaShake192s,
        &key,
    )
    .unwrap();
    assert_eq!(
        OriginKeyAlgorithm::SlhDsaShake192s.code(),
        algorithms::SLHDSA_SHAKE_192S.code
    );
    // The same 48 bytes cannot pose as another algorithm's origin key.
    assert!(AccountAddress::from_origin_key(
        AddressNetwork::Development,
        "agility-fixture",
        OriginKeyAlgorithm::MlDsa65,
        &key,
    )
    .is_err());
    assert_eq!(a.network(), AddressNetwork::Development);
}

/// Size and speed of each candidate against ML-DSA-65. Run with
/// `cargo test --release ... -- --ignored --nocapture parameter_sets`.
#[test]
#[ignore]
fn parameter_sets() {
    use fips205::traits::{KeyGen, Verifier};
    const ROUNDS: u32 = 5;
    let message = [0x5a_u8; 512];
    macro_rules! slh {
        ($m:ident) => {{
            let (pk, sk) = fips205::$m::KG::try_keygen().unwrap();
            let t = Instant::now();
            let mut sig = sk.try_sign(&message, &[], false).unwrap();
            for _ in 1..ROUNDS {
                sig = sk.try_sign(&message, &[], false).unwrap();
            }
            let sign = t.elapsed() / ROUNDS;
            let t = Instant::now();
            for _ in 0..ROUNDS {
                assert!(pk.verify(&message, &sig, &[]));
            }
            let verify = t.elapsed() / ROUNDS;
            eprintln!(
                "{:<20} pk {:>5} B  sig {:>6} B  sign {:>10.2?}  verify {:>9.2?}",
                stringify!($m),
                fips205::$m::PK_LEN,
                fips205::$m::SIG_LEN,
                sign,
                verify
            );
        }};
    }
    {
        use fips204::ml_dsa_65;
        use fips204::traits::{KeyGen as _, Signer as _, Verifier as _};
        let (pk, sk) = ml_dsa_65::KG::try_keygen().unwrap();
        let t = Instant::now();
        let mut sig = sk.try_sign(&message, &[]).unwrap();
        for _ in 1..ROUNDS {
            sig = sk.try_sign(&message, &[]).unwrap();
        }
        let sign = t.elapsed() / ROUNDS;
        let t = Instant::now();
        for _ in 0..ROUNDS {
            assert!(pk.verify(&message, &sig, &[]));
        }
        eprintln!(
            "{:<20} pk {:>5} B  sig {:>6} B  sign {:>10.2?}  verify {:>9.2?}",
            "ml_dsa_65",
            ml_dsa_65::PK_LEN,
            ml_dsa_65::SIG_LEN,
            sign,
            t.elapsed() / ROUNDS
        );
    }
    slh!(slh_dsa_shake_128f);
    slh!(slh_dsa_shake_128s);
    slh!(slh_dsa_shake_192f);
    slh!(slh_dsa_shake_192s);
    slh!(slh_dsa_shake_256f);
    slh!(slh_dsa_shake_256s);
}
