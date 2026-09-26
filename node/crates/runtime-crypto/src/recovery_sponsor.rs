//! Verify the signatures in the bounded sponsored recovery envelope.
//!
//! This proves signature authenticity and envelope bindings only. Execution must
//! check current account authority, profile, counters, protection, expiry and fees.
//! No mock or legacy backend can verify this format.


use dytallix_protocol_types::{
    recovery::AuthenticatedFacts,
    recovery_sponsor::{self, SponsoredRecovery},
};

#[derive(Debug, thiserror::Error)]
pub enum SponsorVerificationError {
    #[error("invalid sponsored recovery wire: {0}")]
    Wire(String),
    #[error("sponsor signatures require the pqc-fips204 backend")]
    BackendUnavailable,
    #[error("unsupported sponsor signature algorithm")]
    UnsupportedAlgorithm,
    #[error("invalid sponsor public key")]
    InvalidPublicKey,
    #[error("invalid sponsor signature")]
    InvalidSignature,
    #[error("invalid inner recovery signatures: {0}")]
    Recovery(#[from] crate::recovery::RecoveryVerificationError),
}

/// Check canonical encoding and all operation/manifest bindings before crypto.
/// The codec rejects inconsistent domain, version, operation and manifest fields.
/// Returned facts contain only inner recovery roles; the sponsor gains no role.
pub fn verify_signed(
    signed: &SponsoredRecovery,
) -> Result<AuthenticatedFacts, SponsorVerificationError> {
    recovery_sponsor::encode(signed).map_err(|e| SponsorVerificationError::Wire(e.to_string()))?;
    verify_backend(signed)
}

/// Decode the bounded canonical envelope and verify every included signature.
pub fn verify_bytes(wire: &[u8]) -> Result<AuthenticatedFacts, SponsorVerificationError> {
    let signed = recovery_sponsor::decode(wire)
        .map_err(|e| SponsorVerificationError::Wire(e.to_string()))?;
    verify_signed(&signed)
}


fn verify_backend(
    signed: &SponsoredRecovery,
) -> Result<AuthenticatedFacts, SponsorVerificationError> {
    use fips204::{
        ml_dsa_65,
        traits::{SerDes, Verifier},
    };

    // Preserve the specified meter order: inner signatures precede the sponsor.
    let facts = crate::recovery::verify_signed(&signed.recovery)?;
    let message = recovery_sponsor::sponsor_signing_bytes(&signed.sponsor)
        .map_err(|e| SponsorVerificationError::Wire(e.to_string()))?;
    let key = &signed.sponsor.sponsor_key;
    if signed.recovery.operation.domain.network == 1 && key.algorithm != "mldsa65" {
        return Err(SponsorVerificationError::UnsupportedAlgorithm);
    }
    // Exact identifiers only. Pure ML-DSA uses the empty FIPS context. The signed
    // bytes already contain the distinct recovery sponsor domain separator.
    let valid = match key.algorithm.as_str() {
        "mldsa65" => {
            let bytes: [u8; ml_dsa_65::PK_LEN] = key
                .public_key
                .as_slice()
                .try_into()
                .map_err(|_| SponsorVerificationError::InvalidPublicKey)?;
            let public = ml_dsa_65::PublicKey::try_from_bytes(bytes)
                .map_err(|_| SponsorVerificationError::InvalidPublicKey)?;
            let signature: [u8; ml_dsa_65::SIG_LEN] = signed
                .signature
                .as_slice()
                .try_into()
                .map_err(|_| SponsorVerificationError::InvalidSignature)?;
            public.verify(&message, &signature, &[])
        }
        _ => return Err(SponsorVerificationError::UnsupportedAlgorithm),
    };
    if !valid {
        return Err(SponsorVerificationError::InvalidSignature);
    }
    Ok(facts)
}
