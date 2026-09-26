#![cfg(feature = "compatibility")]
use dytallix_core::{
    address::DAddr,
    keypair::{DytallixKeypair, KeyScheme},
    signature::{verify_for_scheme, verify_mldsa65, verify_mldsa87, verify_slhdsa},
    DytallixError,
};
use fips204::{
    ml_dsa_65, ml_dsa_87,
    traits::{KeyGen, SerDes, Signer, Verifier},
};

#[test]
fn generated_65_and_87_signatures_pass_independent_fips_verification() {
    let message = b"complete ordinary signing bytes\0with trailing data";
    let key65 = DytallixKeypair::generate();
    let key87 = DytallixKeypair::generate_mldsa87();
    assert_eq!(key65.scheme(), KeyScheme::MlDsa65);
    assert_eq!(key87.scheme(), KeyScheme::MlDsa87);
    assert_eq!(key65.public_key().len(), 1952);
    assert_eq!(key65.private_key().len(), 4032);
    assert_eq!(key87.public_key().len(), 2592);
    assert_eq!(key87.private_key().len(), 4896);
    let signature65 = key65.sign(message).unwrap();
    let signature87 = key87.sign(message).unwrap();
    assert_eq!(signature65.len(), 3309);
    assert_eq!(signature87.len(), 4627);
    let pk65 =
        ml_dsa_65::PublicKey::try_from_bytes(key65.public_key().try_into().unwrap()).unwrap();
    let pk87 =
        ml_dsa_87::PublicKey::try_from_bytes(key87.public_key().try_into().unwrap()).unwrap();
    let sig65 = signature65.as_slice().try_into().unwrap();
    let sig87 = signature87.as_slice().try_into().unwrap();
    assert!(pk65.verify(message, &sig65, b""));
    assert!(pk87.verify(message, &sig87, b""));
    assert!(!pk65.verify(message, &sig65, b"different context"));
    assert!(!pk87.verify(message, &sig87, b"different context"));
}

#[test]
fn independently_generated_keypairs_import_and_verify_external_signatures() {
    let (pk65, sk65) = ml_dsa_65::KG::keygen_from_seed(&[65; 32]);
    let (pk87, sk87) = ml_dsa_87::KG::keygen_from_seed(&[87; 32]);
    let message = b"independent FIPS signatures";
    let sig65 = sk65.try_sign_with_seed(&[1; 32], message, b"").unwrap();
    let sig87 = sk87.try_sign_with_seed(&[2; 32], message, b"").unwrap();
    let public65 = pk65.into_bytes();
    let public87 = pk87.into_bytes();
    let secret65 = sk65.into_bytes();
    let secret87 = sk87.into_bytes();
    let imported65 =
        DytallixKeypair::from_keypair(KeyScheme::MlDsa65, &public65, &secret65).unwrap();
    let imported87 =
        DytallixKeypair::from_keypair(KeyScheme::MlDsa87, &public87, &secret87).unwrap();
    assert_eq!(imported65.public_key(), public65);
    assert_eq!(imported87.public_key(), public87);
    // Fixed assertion text prevents a failing test from printing private bytes.
    assert!(
        imported65.private_key() == secret65,
        "ML-DSA-65 private bytes changed"
    );
    assert!(
        imported87.private_key() == secret87,
        "ML-DSA-87 private bytes changed"
    );
    assert!(verify_for_scheme(imported65.scheme(), &public65, message, &sig65).unwrap());
    assert!(verify_for_scheme(imported87.scheme(), &public87, message, &sig87).unwrap());
    assert!(verify_mldsa65(&public65, message, &imported65.sign(message).unwrap()).unwrap());
    assert!(verify_mldsa87(&public87, message, &imported87.sign(message).unwrap()).unwrap());
}

#[test]
fn explicit_import_rejects_wrong_scheme_sizes_and_unrelated_public_key() {
    let (pk65, sk65) = ml_dsa_65::KG::keygen_from_seed(&[3; 32]);
    let (other65, _) = ml_dsa_65::KG::keygen_from_seed(&[4; 32]);
    let (pk87, sk87) = ml_dsa_87::KG::keygen_from_seed(&[5; 32]);
    let (other87, _) = ml_dsa_87::KG::keygen_from_seed(&[6; 32]);
    let public65 = pk65.into_bytes();
    let public87 = pk87.into_bytes();
    let secret65 = sk65.into_bytes();
    let secret87 = sk87.into_bytes();
    assert!(matches!(
        DytallixKeypair::from_keypair(KeyScheme::MlDsa87, &public65, &secret65),
        Err(DytallixError::InvalidKeySize {
            expected: 2592,
            got: 1952
        })
    ));
    assert!(matches!(
        DytallixKeypair::from_keypair(KeyScheme::MlDsa65, &public87, &secret87),
        Err(DytallixError::InvalidKeySize {
            expected: 1952,
            got: 2592
        })
    ));
    for (scheme, public, secret, other) in [
        (
            KeyScheme::MlDsa65,
            public65.as_slice(),
            secret65.as_slice(),
            other65.into_bytes().to_vec(),
        ),
        (
            KeyScheme::MlDsa87,
            public87.as_slice(),
            secret87.as_slice(),
            other87.into_bytes().to_vec(),
        ),
    ] {
        assert!(matches!(
            DytallixKeypair::from_keypair(scheme, public, &secret[..secret.len() - 1]),
            Err(DytallixError::InvalidKeySize { .. })
        ));
        assert!(matches!(
            DytallixKeypair::from_keypair(scheme, &public[..public.len() - 1], secret),
            Err(DytallixError::InvalidKeySize { .. })
        ));
        assert!(matches!(
            DytallixKeypair::from_keypair(scheme, &other, secret),
            Err(DytallixError::InvalidKeypair(_))
        ));
        // Packed small-coefficient fields cannot use an all-ones encoding.
        // Deserialization must reject it before signing or public derivation.
        assert!(matches!(
            DytallixKeypair::from_keypair(scheme, public, &vec![0xff; secret.len()]),
            Err(DytallixError::InvalidKeypair(_))
        ));
    }
}

#[test]
fn paired_import_rejects_inconsistent_retained_public_key_hash() {
    let (pk65, sk65) = ml_dsa_65::KG::keygen_from_seed(&[7; 32]);
    let (pk87, sk87) = ml_dsa_87::KG::keygen_from_seed(&[8; 32]);
    for (scheme, public, mut secret) in [
        (
            KeyScheme::MlDsa65,
            pk65.into_bytes().to_vec(),
            sk65.into_bytes().to_vec(),
        ),
        (
            KeyScheme::MlDsa87,
            pk87.into_bytes().to_vec(),
            sk87.into_bytes().to_vec(),
        ),
    ] {
        // rho and K occupy the first 64 bytes. tr stores the public-key hash.
        // This test uses the paired path, which never invokes private-to-public derivation.
        secret[64] ^= 1;
        assert!(matches!(
            DytallixKeypair::from_keypair(scheme, &public, &secret),
            Err(DytallixError::InvalidKeypair(_))
        ));
    }
}

#[test]
fn verifiers_reject_tampering_truncation_and_cross_scheme_inputs() {
    let key65 = DytallixKeypair::generate();
    let key87 = DytallixKeypair::generate_mldsa87();
    let message = b"exact signing bytes";
    let signature65 = key65.sign(message).unwrap();
    let signature87 = key87.sign(message).unwrap();
    for (key, signature) in [(&key65, &signature65), (&key87, &signature87)] {
        assert!(!verify_for_scheme(
            key.scheme(),
            key.public_key(),
            b"changed signing bytes",
            signature
        )
        .unwrap());
        let mut changed = signature.clone();
        changed[0] ^= 1;
        assert!(!verify_for_scheme(key.scheme(), key.public_key(), message, &changed).unwrap());
        assert!(matches!(
            verify_for_scheme(
                key.scheme(),
                key.public_key(),
                message,
                &signature[..signature.len() - 1]
            ),
            Err(DytallixError::InvalidSignatureSize { .. })
        ));
        assert!(matches!(
            verify_for_scheme(
                key.scheme(),
                &key.public_key()[..key.public_key().len() - 1],
                message,
                signature
            ),
            Err(DytallixError::InvalidKeySize { .. })
        ));
    }
    assert!(verify_for_scheme(
        KeyScheme::MlDsa65,
        key87.public_key(),
        message,
        &signature87
    )
    .is_err());
    assert!(verify_for_scheme(
        KeyScheme::MlDsa87,
        key65.public_key(),
        message,
        &signature65
    )
    .is_err());
}

#[test]
fn default_and_legacy_address_remain_65_and_private_only_87_fails_closed() {
    let key65 = DytallixKeypair::generate();
    let imported65 = DytallixKeypair::from_private_key(key65.private_key()).unwrap();
    assert_eq!(imported65.scheme(), KeyScheme::MlDsa65);
    assert_eq!(imported65.public_key(), key65.public_key());
    assert_eq!(
        DAddr::from_public_key(imported65.public_key()).unwrap(),
        DAddr::from_public_key(key65.public_key()).unwrap()
    );
    let key87 = DytallixKeypair::generate_mldsa87();
    assert!(
        matches!(DytallixKeypair::from_private_key(key87.private_key()),
        Err(DytallixError::InvalidKeypair(message)) if message.contains("matching public key"))
    );
    assert!(DAddr::from_public_key(key87.public_key()).is_err());
    assert!(DytallixKeypair::from_private_key(&[0; 4895]).is_err());
    assert!(DytallixKeypair::from_private_key(&[0; 4897]).is_err());
}

#[test]
fn slhdsa_generation_import_and_explicit_verification_remain_available() {
    let key = DytallixKeypair::generate_slh_dsa();
    let restored = DytallixKeypair::from_private_key(key.private_key()).unwrap();
    assert_eq!(restored.scheme(), KeyScheme::SlhDsa);
    assert_eq!(restored.public_key(), key.public_key());
    let signature = restored.sign(b"legacy cold storage").unwrap();
    assert!(verify_slhdsa(key.public_key(), b"legacy cold storage", &signature).unwrap());
    assert!(verify_for_scheme(
        KeyScheme::SlhDsa,
        key.public_key(),
        b"legacy cold storage",
        &signature
    )
    .unwrap());
}
