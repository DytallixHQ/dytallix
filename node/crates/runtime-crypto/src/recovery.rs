//! Signature verification for the bounded recovery wire format.
//!
//! Verified facts establish signature authenticity, not account authorization.
//! `RecoveryState::transition` checks current roles, counters and protection.
//! There is no fee handling, persistence, RPC route or implicit height advance.
//! Recovery verification requires `pqc-fips204`, even when `pqc-mock` is enabled.

#[cfg(feature = "mldsa87-development")]
use fips204::ml_dsa_87;

use dytallix_protocol_types::{
    recovery::{AuthenticatedFacts, RecoveryState},
    recovery_wire::{self, SignedRecovery},
};

#[derive(Debug, thiserror::Error)]
pub enum RecoveryVerificationError {
    #[error("invalid recovery wire: {0}")]
    Wire(String),
    #[error("recovery signatures require the pqc-fips204 backend")]
    BackendUnavailable,
    #[error("unsupported recovery signature algorithm")]
    UnsupportedAlgorithm,
    #[error("invalid recovery public key")]
    InvalidPublicKey,
    #[error("invalid recovery signature")]
    InvalidSignature,
    #[error("recovery transition rejected: {0}")]
    State(String),
}

/// Validate the complete canonical envelope before any cryptographic work.
/// The codec bounds the operation, keys, signature count and signature sizes.
/// Every listed signature must verify over its exact operation, role and key.
pub fn verify_signed(
    signed: &SignedRecovery,
) -> Result<AuthenticatedFacts, RecoveryVerificationError> {
    recovery_wire::encode(signed).map_err(|e| RecoveryVerificationError::Wire(e.to_string()))?;
    verify_backend(signed)
}

/// Decode the bounded canonical wire envelope and verify all signatures.
pub fn verify_bytes(wire: &[u8]) -> Result<AuthenticatedFacts, RecoveryVerificationError> {
    let signed =
        recovery_wire::decode(wire).map_err(|e| RecoveryVerificationError::Wire(e.to_string()))?;
    verify_signed(&signed)
}

/// Verify signatures and current account authority, then return a new state.
/// The caller must persist that state with its fees and receipts in one commit.
/// The caller must already have committed `advance_height(height)` at block start.
/// Rejection does not change the supplied state or process an expiry implicitly.
pub fn verify_and_transition(
    state: &RecoveryState,
    height: u64,
    wire: &[u8],
) -> Result<RecoveryState, RecoveryVerificationError> {
    let facts = verify_bytes(wire)?;
    state
        .transition(height, &facts.action, &facts)
        .map_err(|e| RecoveryVerificationError::State(e.to_string()))
}

#[cfg(not(feature = "pqc-fips204"))]
fn verify_backend(
    _signed: &SignedRecovery,
) -> Result<AuthenticatedFacts, RecoveryVerificationError> {
    Err(RecoveryVerificationError::BackendUnavailable)
}

#[cfg(feature = "pqc-fips204")]
fn verify_backend(
    signed: &SignedRecovery,
) -> Result<AuthenticatedFacts, RecoveryVerificationError> {
    use dytallix_protocol_types::recovery_wire::SignatureRole;

    let mut signers = Vec::new();
    let mut proofs = Vec::new();
    for entry in &signed.signatures {
        if signed.operation.domain.network == 1 && entry.key.algorithm != "mldsa65" {
            return Err(RecoveryVerificationError::UnsupportedAlgorithm);
        }
        let message =
            recovery_wire::signing_bytes(&signed.operation, entry.role.clone(), &entry.key)
                .map_err(|e| RecoveryVerificationError::Wire(e.to_string()))?;
        verify_signature(&entry.key, &message, &entry.signature)?;
        match &entry.role {
            SignatureRole::Operation => signers.push(entry.key.clone()),
            SignatureRole::Possession => proofs.push(entry.key.clone()),
        }
    }
    Ok(AuthenticatedFacts {
        domain: signed.operation.domain.clone(),
        action: signed.operation.action.clone(),
        signers,
        proofs,
    })
}

#[cfg(feature = "pqc-fips204")]
fn verify_signature(
    key: &dytallix_protocol_types::recovery::KeyIdentity,
    message: &[u8],
    signature: &[u8],
) -> Result<(), RecoveryVerificationError> {
    use fips204::{
        ml_dsa_65,
        traits::{SerDes, Verifier},
    };

    // Do not use legacy algorithm aliases or the mock-capable verifier dispatch.
    // Signing bytes carry the operation/proof domain. FIPS context is empty.
    let valid = match key.algorithm.as_str() {
        "mldsa65" => {
            let bytes: [u8; ml_dsa_65::PK_LEN] = key
                .public_key
                .as_slice()
                .try_into()
                .map_err(|_| RecoveryVerificationError::InvalidPublicKey)?;
            let public_key = ml_dsa_65::PublicKey::try_from_bytes(bytes)
                .map_err(|_| RecoveryVerificationError::InvalidPublicKey)?;
            let signature: [u8; ml_dsa_65::SIG_LEN] = signature
                .try_into()
                .map_err(|_| RecoveryVerificationError::InvalidSignature)?;
            public_key.verify(message, &signature, &[])
        }
        #[cfg(feature = "mldsa87-development")]
        "mldsa87" => {
            let bytes: [u8; ml_dsa_87::PK_LEN] = key
                .public_key
                .as_slice()
                .try_into()
                .map_err(|_| RecoveryVerificationError::InvalidPublicKey)?;
            let public_key = ml_dsa_87::PublicKey::try_from_bytes(bytes)
                .map_err(|_| RecoveryVerificationError::InvalidPublicKey)?;
            let signature: [u8; ml_dsa_87::SIG_LEN] = signature
                .try_into()
                .map_err(|_| RecoveryVerificationError::InvalidSignature)?;
            public_key.verify(message, &signature, &[])
        }
        _ => return Err(RecoveryVerificationError::UnsupportedAlgorithm),
    };
    if valid {
        Ok(())
    } else {
        Err(RecoveryVerificationError::InvalidSignature)
    }
}
