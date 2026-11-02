//! Crypto-agility spike: a FIPS 205 SLH-DSA-SHAKE-192s wallet key, from key
//! generation through the encrypted keystore to a signed development-network
//! ordinary-v2 transaction. Mainnet refusal is the node's (runtime-crypto).
#![cfg(feature = "compatibility")]

use dytallix_core::signature::{verify_for_scheme, verify_slhdsa};
use dytallix_sdk::keystore::Keystore;
use dytallix_sdk::{ordinary_v2::*, DytallixKeypair, KeyScheme};
use serde_json::Value;

const SLH: &str = "slhdsa-shake-192s";

fn profile() -> FeeProfile {
    let v: Value = serde_json::from_str(include_str!(
        "../../../vendor/dytallix-protocol-types/tests/fixtures/ordinary_fee_v1_vectors.json"
    ))
    .unwrap();
    let mut p: FeeProfile = serde_json::from_value(v["profile"].clone()).unwrap();
    // An account role that enables the fallback beside ML-DSA-65.
    p.limits.allowed_algorithms = ["mldsa65".to_owned(), SLH.to_owned()].into();
    p.signature_costs = [("mldsa65".to_owned(), 1), (SLH.to_owned(), 5)].into();
    p.limits.max_wire_bytes = p.limits.max_wire_bytes.max(65_536);
    p.wire_byte_cost = 0;
    p.validate().unwrap();
    p
}

fn context(p: &FeeProfile, k: &DytallixKeypair) -> SigningContext {
    let a = AccountAddress::from_origin_key(
        AddressNetwork::Development,
        "sdk-agility",
        OriginKeyAlgorithm::SlhDsaShake192s,
        k.public_key(),
    )
    .unwrap();
    SigningContext {
        domain: RecoveryDomain {
            network: 3,
            chain_id: "sdk-agility".into(),
            genesis_digest: [7; 32],
            account_id: *a.account_id(),
        },
        current_key: KeyIdentity {
            algorithm: SLH.into(),
            public_key: k.public_key().to_vec(),
        },
        authorization_generation: 0,
        spending_nonce: 0,
        committed: CommittedContext {
            chain_id: "sdk-agility".into(),
            genesis_digest: [7; 32],
            height: 19,
            app_hash: [8; 32],
        },
        profile_digest: profile_digest(p).unwrap(),
        protected: false,
    }
}

#[test]
fn keypair_is_fips205_and_never_confused_with_legacy_sphincs() {
    let k = DytallixKeypair::generate_slh_dsa_shake_192s();
    assert_eq!(k.scheme(), KeyScheme::SlhDsaShake192s);
    assert_eq!(k.scheme().algorithm_id(), SLH);
    assert_eq!((k.public_key().len(), k.private_key().len()), (48, 96));
    let sig = k.sign(b"agility").unwrap();
    assert_eq!(sig.len(), 16_224);
    assert!(verify_for_scheme(KeyScheme::SlhDsaShake192s, k.public_key(), b"agility", &sig).unwrap());

    // Same lengths as legacy SPHINCS+, different algorithm: a length-based
    // import reads it as legacy, and the legacy verifier rejects its signature.
    let guessed = DytallixKeypair::from_private_key(k.private_key()).unwrap();
    assert_eq!(guessed.scheme(), KeyScheme::SlhDsa);
    assert!(!verify_slhdsa(k.public_key(), b"agility", &sig).unwrap());

    // Explicit import checks possession.
    let named =
        DytallixKeypair::from_keypair(KeyScheme::SlhDsaShake192s, k.public_key(), k.private_key())
            .unwrap();
    assert_eq!(named.public_key(), k.public_key());
    let other = DytallixKeypair::generate_slh_dsa_shake_192s();
    assert!(DytallixKeypair::from_keypair(
        KeyScheme::SlhDsaShake192s,
        k.public_key(),
        other.private_key()
    )
    .is_err());
}

#[test]
fn keystore_holds_the_fallback_key_and_still_refuses_legacy_sphincs() {
    let dir = std::env::temp_dir().join(format!("dytallix-agility-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("keystore.json");
    let _ = std::fs::remove_file(&path);
    let mut keystore = Keystore::create(path.clone(), b"agility passphrase").unwrap();
    let k = DytallixKeypair::generate_slh_dsa_shake_192s();
    keystore.add_keypair(&k, "fallback").unwrap();
    assert!(keystore
        .add_keypair(&DytallixKeypair::generate_slh_dsa(), "legacy")
        .is_err());
    keystore.save().unwrap();
    let reopened = Keystore::open(path)
        .unwrap()
        .open_keypair("fallback", b"agility passphrase")
        .unwrap();
    assert_eq!(reopened.scheme(), KeyScheme::SlhDsaShake192s);
    assert_eq!(reopened.private_key(), k.private_key());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn development_transaction_signs_and_verifies() {
    let p = profile();
    let k = DytallixKeypair::generate_slh_dsa_shake_192s();
    let c = context(&p, &k);
    let prepared = prepare(
        &p,
        &c,
        vec![Action::Data {
            data: "hash-based fallback".into(),
        }],
        String::new(),
        25,
        1000,
        3000,
    )
    .unwrap();
    let signed = prepared.sign(&KeypairSigner::new(&k).unwrap()).unwrap();
    assert_eq!(signed.body.key.algorithm, SLH);
    verify_signature(&signed, &p.limits).unwrap();
    let mut tampered = signed;
    tampered.signature[0] ^= 1;
    assert!(verify_signature(&tampered, &p.limits).is_err());
}
