//! Approved operational signatures. No persisted legacy key is translated.
use dytallix_runtime_crypto::{verify, ActivePQC, PQCAlgorithm, PQC};
use fips204::{
    ml_dsa_65,
    traits::{KeyGen, SerDes, Signer},
};

#[test]
fn operational_signatures_use_exact_identifier_sizes_and_message() {
    let (secret, public) = ActivePQC::keypair();
    let signature = ActivePQC::sign(&secret, b"operational profile test");
    assert_eq!(ActivePQC::ALG, "mldsa65");
    assert_eq!(
        (public.len(), secret.len(), signature.len()),
        (1952, 4032, 3309)
    );
    verify(
        &public,
        b"operational profile test",
        &signature,
        PQCAlgorithm::MlDsa65,
    )
    .unwrap();
    assert!(!ActivePQC::verify(&public, b"changed message", &signature));
    assert!(verify(
        &public,
        b"operational profile test",
        &signature,
        PQCAlgorithm::MlDsa87
    )
    .is_err());
    assert!(verify(
        &public,
        b"operational profile test",
        &signature,
        PQCAlgorithm::Dilithium5
    )
    .is_err());
}


#[test]
fn operational_context_does_not_accept_oracle_signatures() {
    let (public, secret) = ml_dsa_65::KG::keygen_from_seed(&[92; 32]);
    let signature = secret
        .try_sign(b"same message", b"dytallix-oracle")
        .unwrap();
    assert!(!ActivePQC::verify(
        &public.into_bytes(),
        b"same message",
        &signature
    ));
}
