//! Verify complete ordinary-v2 signing bytes with the selected explicit limits.
//!
//! Verification establishes authenticity only. The node must resolve the current
//! account key, generation, nonce, protection, expiry and ordinary fee contract
//! from committed or staged state before execution. No fallback backend is used.

#[cfg(feature = "mldsa87-development")]
use fips204::ml_dsa_87;

use dytallix_protocol_types::ordinary::{
    self as wire, Limits, OrdinaryTransaction, SignedOrdinary,
};

#[derive(Debug, thiserror::Error)]
pub enum OrdinaryVerificationError {
    #[error("invalid ordinary wire or explicit profile: {0}")]
    Wire(String),
    #[error("ordinary signatures require the pqc-fips204 backend")]
    BackendUnavailable,
    #[error("unsupported ordinary signature algorithm")]
    UnsupportedAlgorithm,
    #[error("invalid ordinary public key")]
    InvalidPublicKey,
    #[error("invalid ordinary signature")]
    InvalidSignature,
}

/// Immutable authenticated body. No constructor or deserializer bypasses crypto.
/// Cloning preserves the same body and its identifiers; it grants no state authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedOrdinary {
    body: OrdinaryTransaction,
    transaction_id: [u8; 32],
    envelope_hash: [u8; 32],
}

impl VerifiedOrdinary {
    pub fn body(&self) -> &OrdinaryTransaction {
        &self.body
    }

    pub fn transaction_id(&self) -> [u8; 32] {
        self.transaction_id
    }

    pub fn envelope_hash(&self) -> [u8; 32] {
        self.envelope_hash
    }
}

/// Validate the bounded canonical envelope and selected algorithm before crypto.
/// Limits are mandatory. There is no implicit account-role or capacity profile.
pub fn verify_signed(
    signed: &SignedOrdinary,
    limits: &Limits,
) -> Result<VerifiedOrdinary, OrdinaryVerificationError> {
    wire::encode(signed, limits).map_err(|e| OrdinaryVerificationError::Wire(e.to_string()))?;
    verify_backend(signed, limits)?;
    Ok(VerifiedOrdinary {
        body: signed.body.clone(),
        transaction_id: wire::transaction_id(&signed.body, limits)
            .map_err(|e| OrdinaryVerificationError::Wire(e.to_string()))?,
        envelope_hash: wire::envelope_hash(signed, limits)
            .map_err(|e| OrdinaryVerificationError::Wire(e.to_string()))?,
    })
}

/// Decode and verify canonical bytes against the explicit account-role profile.
pub fn verify_bytes(
    bytes: &[u8],
    limits: &Limits,
) -> Result<VerifiedOrdinary, OrdinaryVerificationError> {
    let signed =
        wire::decode(bytes, limits).map_err(|e| OrdinaryVerificationError::Wire(e.to_string()))?;
    verify_signed(&signed, limits)
}

#[cfg(not(feature = "pqc-fips204"))]
fn verify_backend(
    _signed: &SignedOrdinary,
    _limits: &Limits,
) -> Result<(), OrdinaryVerificationError> {
    Err(OrdinaryVerificationError::BackendUnavailable)
}

#[cfg(feature = "pqc-fips204")]
fn verify_backend(
    signed: &SignedOrdinary,
    limits: &Limits,
) -> Result<(), OrdinaryVerificationError> {
    use fips204::{
        ml_dsa_65,
        traits::{SerDes, Verifier},
    };
    let message = wire::signing_bytes(&signed.body, limits)
        .map_err(|e| OrdinaryVerificationError::Wire(e.to_string()))?;
    let key = &signed.body.key;
    if signed.body.domain.network == 1 && key.algorithm != "mldsa65" {
        return Err(OrdinaryVerificationError::UnsupportedAlgorithm);
    }
    // Pure ML-DSA over all signing bytes, not an application-hash substitute.
    // The complete body contains its domain prefix. The FIPS context is empty.
    let valid = match key.algorithm.as_str() {
        "mldsa65" => {
            let bytes: [u8; ml_dsa_65::PK_LEN] = key
                .public_key
                .as_slice()
                .try_into()
                .map_err(|_| OrdinaryVerificationError::InvalidPublicKey)?;
            let public = ml_dsa_65::PublicKey::try_from_bytes(bytes)
                .map_err(|_| OrdinaryVerificationError::InvalidPublicKey)?;
            let signature: [u8; ml_dsa_65::SIG_LEN] = signed
                .signature
                .as_slice()
                .try_into()
                .map_err(|_| OrdinaryVerificationError::InvalidSignature)?;
            public.verify(&message, &signature, &[])
        }
        #[cfg(feature = "mldsa87-development")]
        "mldsa87" => {
            let bytes: [u8; ml_dsa_87::PK_LEN] = key
                .public_key
                .as_slice()
                .try_into()
                .map_err(|_| OrdinaryVerificationError::InvalidPublicKey)?;
            let public = ml_dsa_87::PublicKey::try_from_bytes(bytes)
                .map_err(|_| OrdinaryVerificationError::InvalidPublicKey)?;
            let signature: [u8; ml_dsa_87::SIG_LEN] = signed
                .signature
                .as_slice()
                .try_into()
                .map_err(|_| OrdinaryVerificationError::InvalidSignature)?;
            public.verify(&message, &signature, &[])
        }
        _ => return Err(OrdinaryVerificationError::UnsupportedAlgorithm),
    };
    if !valid {
        return Err(OrdinaryVerificationError::InvalidSignature);
    }
    Ok(())
}
