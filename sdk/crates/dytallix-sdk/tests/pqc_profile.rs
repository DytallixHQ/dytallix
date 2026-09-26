//! Production profile and wallet persistence boundaries.
#[cfg(feature = "compatibility")]
use dytallix_sdk::keystore::Keystore;
#[cfg(feature = "compatibility")]
use dytallix_sdk::transaction::{Message, Transaction};
use dytallix_sdk::{DytallixKeypair, KeyScheme};
#[cfg(feature = "compatibility")]
use std::{
    fs,
    sync::atomic::{AtomicUsize, Ordering},
};

#[cfg(feature = "compatibility")]
fn transaction() -> Transaction {
    Transaction {
        chain_id: "pqc-profile-local".into(),
        nonce: 1,
        msgs: vec![Message::Data {
            from: "fixture".into(),
            data: "profile".into(),
        }],
        fee: 1,
        memo: String::new(),
        c_gas_limit: 1,
        b_gas_limit: 1,
    }
}

#[cfg(feature = "compatibility")]
#[test]
fn legacy_transaction_never_mislabels_another_scheme_as_mldsa65() {
    let key = DytallixKeypair::generate();
    let signed = transaction().sign(&key).unwrap();
    assert_eq!(signed.algorithm, "mldsa65");
    assert!(transaction()
        .sign(&DytallixKeypair::generate_mldsa87())
        .is_err());
    assert!(transaction()
        .sign(&DytallixKeypair::generate_slh_dsa())
        .is_err());
}

#[test]
fn legacy_identity_is_explicit_and_old_serialized_keys_still_decode() {
    assert_eq!(
        serde_json::from_str::<KeyScheme>("\"SlhDsa\"").unwrap(),
        KeyScheme::SlhDsa
    );
    assert_eq!(
        serde_json::to_string(&KeyScheme::SlhDsa).unwrap(),
        "\"LegacySphincsPlusShake192sSimple\""
    );
    assert_eq!(
        KeyScheme::SlhDsa.algorithm_id(),
        "legacy-sphincsplus-shake-192s-simple"
    );
    assert!(KeyScheme::SlhDsa.require_production_operational().is_err());
    assert!(KeyScheme::MlDsa87.require_production_operational().is_err());
    assert!(KeyScheme::MlDsa65.require_production_operational().is_ok());
}

#[cfg(feature = "compatibility")]
#[test]
fn wallet_restart_preserves_key_and_rejects_scheme_address_and_key_mismatch() {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let directory = std::env::temp_dir().join(format!(
        "dytallix-pqc-wallet-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&directory).unwrap();
    let path = directory.join("keystore.json");
    let key = DytallixKeypair::generate();
    let mut store = Keystore::new(path.clone()).unwrap();
    store.add_keypair(&key, "test").unwrap();
    assert!(store
        .add_keypair(&DytallixKeypair::generate_mldsa87(), "rejected")
        .is_err());
    store.save().unwrap();
    let restored = Keystore::open(path.clone())
        .unwrap()
        .get_keypair("test")
        .unwrap();
    assert_eq!(restored.private_key(), key.private_key());
    assert_eq!(restored.public_key(), key.public_key());
    let original: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for kind in ["scheme", "address", "private_key"] {
        let mut changed = original.clone();
        match kind {
            "scheme" => changed["entries"][0]["scheme"] = serde_json::json!("SlhDsa"),
            "address" => {
                let other = DytallixKeypair::generate();
                changed["entries"][0]["address"] = serde_json::to_value(
                    dytallix_sdk::DAddr::from_public_key(other.public_key()).unwrap(),
                )
                .unwrap();
            }
            _ => changed["entries"][0]["private_key"] = serde_json::json!(vec![0u8; 4032]),
        }
        fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(
            Keystore::open(path.clone())
                .unwrap()
                .get_keypair("test")
                .is_err(),
            "{kind}"
        );
    }
    fs::remove_dir_all(directory).unwrap();
}

#[cfg(feature = "strict-local-mldsa65")]
#[test]
fn strict_core_signs_65_and_refuses_unsupported_implementations() {
    let key = DytallixKeypair::generate();
    let message = b"strict local profile fixture";
    let signature = key.sign(message).unwrap();
    assert!(
        dytallix_core::signature::verify_mldsa65(key.public_key(), message, &signature).unwrap()
    );
    for scheme in [KeyScheme::MlDsa87, KeyScheme::SlhDsa] {
        assert!(scheme.require_available().is_err());
        assert!(DytallixKeypair::from_keypair(scheme, &[], &[]).is_err());
        assert!(dytallix_core::signature::verify_for_scheme(scheme, &[], message, &[]).is_err());
    }
}
