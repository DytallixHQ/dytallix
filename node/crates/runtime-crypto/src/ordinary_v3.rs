//! Provisional ordinary-v3 signature verification. No consensus activation.
//! Verification proves body authenticity only. The node must check current
//! authority, nonce, expiry, fee policy, and governance eligibility separately.

use dytallix_protocol_types::ordinary_v3::{
    self as wire, OrdinaryTransaction, SignedOrdinary, V3Limits,
};

#[derive(Debug, thiserror::Error)]
pub enum OrdinaryV3VerificationError {
    #[error("invalid ordinary-v3 wire or explicit profile: {0}")]
    Wire(String),
    #[error("ordinary-v3 signatures require the pqc-fips204 backend")]
    BackendUnavailable,
    #[error("unsupported ordinary-v3 signature algorithm")]
    UnsupportedAlgorithm,
    #[error("invalid ordinary-v3 public key")]
    InvalidPublicKey,
    #[error("invalid ordinary-v3 signature")]
    InvalidSignature,
}

/// The private fields prevent callers from constructing an authenticated body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedOrdinaryV3 {
    body: OrdinaryTransaction,
    transaction_id: [u8; 32],
    envelope_hash: [u8; 32],
}

impl VerifiedOrdinaryV3 {
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

pub fn verify_signed(
    signed: &SignedOrdinary,
    limits: &V3Limits,
) -> Result<VerifiedOrdinaryV3, OrdinaryV3VerificationError> {
    wire::encode(signed, limits).map_err(|e| OrdinaryV3VerificationError::Wire(e.to_string()))?;
    verify_backend(signed, limits)?;
    Ok(VerifiedOrdinaryV3 {
        body: signed.body.clone(),
        transaction_id: wire::transaction_id(&signed.body, limits)
            .map_err(|e| OrdinaryV3VerificationError::Wire(e.to_string()))?,
        envelope_hash: wire::envelope_hash(signed, limits)
            .map_err(|e| OrdinaryV3VerificationError::Wire(e.to_string()))?,
    })
}

pub fn verify_bytes(
    bytes: &[u8],
    limits: &V3Limits,
) -> Result<VerifiedOrdinaryV3, OrdinaryV3VerificationError> {
    let signed = wire::decode(bytes, limits)
        .map_err(|e| OrdinaryV3VerificationError::Wire(e.to_string()))?;
    verify_signed(&signed, limits)
}


fn verify_backend(
    signed: &SignedOrdinary,
    limits: &V3Limits,
) -> Result<(), OrdinaryV3VerificationError> {
    use fips204::{
        ml_dsa_65,
        traits::{SerDes, Verifier},
    };

    let message = wire::signing_bytes(&signed.body, limits)
        .map_err(|e| OrdinaryV3VerificationError::Wire(e.to_string()))?;
    let key = &signed.body.key;
    if signed.body.domain.network == 1 && key.algorithm != "mldsa65" {
        return Err(OrdinaryV3VerificationError::UnsupportedAlgorithm);
    }
    // Pure ML-DSA signs the complete versioned body with an empty FIPS context.
    let valid = match key.algorithm.as_str() {
        "mldsa65" => {
            let bytes: [u8; ml_dsa_65::PK_LEN] = key
                .public_key
                .as_slice()
                .try_into()
                .map_err(|_| OrdinaryV3VerificationError::InvalidPublicKey)?;
            let public = ml_dsa_65::PublicKey::try_from_bytes(bytes)
                .map_err(|_| OrdinaryV3VerificationError::InvalidPublicKey)?;
            let signature: [u8; ml_dsa_65::SIG_LEN] = signed
                .signature
                .as_slice()
                .try_into()
                .map_err(|_| OrdinaryV3VerificationError::InvalidSignature)?;
            public.verify(&message, &signature, &[])
        }
        _ => return Err(OrdinaryV3VerificationError::UnsupportedAlgorithm),
    };
    if !valid {
        return Err(OrdinaryV3VerificationError::InvalidSignature);
    }
    Ok(())
}
