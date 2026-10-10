//! Post-Quantum Cryptographic signature verification module
//!
//! Signature verification uses the pure-Rust FIPS 204 backend. ML-DSA-65 is
//! the only approved algorithm, so it is the only one this module can name.
//! Every other label, including pre-standard Dilithium and ML-DSA-87, fails
//! to parse with `UnsupportedAlgorithm`.

use std::str::FromStr;
use thiserror::Error;

use fips204::ml_dsa_65;
use fips204::traits::{SerDes, Verifier};

/// Approved PQC signature algorithms.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PQCAlgorithm {
    #[default]
    MlDsa65,
}

impl PQCAlgorithm {
    /// Get algorithm identifier string
    pub fn as_str(&self) -> &'static str {
        match self {
            PQCAlgorithm::MlDsa65 => "mldsa65",
        }
    }
}

impl FromStr for PQCAlgorithm {
    type Err = PQCVerifyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "mldsa65" => Ok(PQCAlgorithm::MlDsa65),
            _ => Err(PQCVerifyError::UnsupportedAlgorithm(s.to_string())),
        }
    }
}

/// Structured errors for PQC verification
#[derive(Error, Debug)]
pub enum PQCVerifyError {
    #[error("Unsupported algorithm: {0}")]
    UnsupportedAlgorithm(String),

    #[error("Invalid public key format for {algorithm}: {details}")]
    InvalidPublicKey { algorithm: String, details: String },

    #[error("Invalid signature format for {algorithm}: {details}")]
    InvalidSignature { algorithm: String, details: String },

    #[error("Signature verification failed for {algorithm}")]
    VerificationFailed { algorithm: String },

    #[error("PQC feature not compiled: {feature}")]
    FeatureNotCompiled { feature: String },
}

#[cfg(test)]
mod fail_closed_tests {
    #[test]
    fn fips204_operational_lengths_are_mldsa65() {
        // The operational profile fixes exact FIPS 204 encodings.
        {
            use fips204::ml_dsa_65;
            assert_eq!(ml_dsa_65::PK_LEN, 1952);
            assert_eq!(ml_dsa_65::SIG_LEN, 3309);
            assert_eq!(ml_dsa_65::SK_LEN, 4032);
        }
    }
}

/// Verify a signature with an approved PQC algorithm.
///
/// # Returns
/// * `Ok(())` if verification succeeds
/// * `Err(PQCVerifyError)` with structured error information
pub fn verify(
    pubkey: &[u8],
    msg: &[u8],
    sig: &[u8],
    alg: PQCAlgorithm,
) -> Result<(), PQCVerifyError> {
    match alg {
        PQCAlgorithm::MlDsa65 => verify_mldsa65_fips204(pubkey, msg, sig),
    }
}

/// Verify the operational default algorithm (ML-DSA-65).
/// This maintains backward compatibility with existing ActivePQC::verify calls
pub fn verify_default(pubkey: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    match verify(pubkey, msg, sig, PQCAlgorithm::default()) {
        Ok(()) => true,
        Err(e) => {
            tracing::error!("PQC verification failed: {}", e);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_approved_algorithms_parse() {
        assert_eq!(PQCAlgorithm::from_str("mldsa65").unwrap(), PQCAlgorithm::MlDsa65);
        assert_eq!(PQCAlgorithm::default(), PQCAlgorithm::MlDsa65);
        assert_eq!(PQCAlgorithm::MlDsa65.as_str(), "mldsa65");
        for label in [
            "dilithium5",
            "dilithium3",
            "mldsa87",
            "mldsa44",
            "falcon1024",
            "sphincs_sha2_128s_simple",
            "ed25519",
            "secp256k1",
            "MLDSA65",
            "",
        ] {
            assert!(matches!(
                PQCAlgorithm::from_str(label),
                Err(PQCVerifyError::UnsupportedAlgorithm(_))
            ));
        }
    }

    #[test]
    fn malformed_inputs_fail() {
        assert!(!verify_default(&[], &[], &[]));
        assert!(matches!(
            verify(b"pubkey", b"message", b"signature", PQCAlgorithm::MlDsa65),
            Err(PQCVerifyError::InvalidPublicKey { .. })
        ));
    }
}

fn verify_mldsa65_fips204(pubkey: &[u8], msg: &[u8], sig: &[u8]) -> Result<(), PQCVerifyError> {
    let pk_array: [u8; ml_dsa_65::PK_LEN] =
        pubkey
            .try_into()
            .map_err(|_| PQCVerifyError::InvalidPublicKey {
                algorithm: "mldsa65".to_string(),
                details: format!("Expected {} bytes, got {}", ml_dsa_65::PK_LEN, pubkey.len()),
            })?;

    let pk_obj = ml_dsa_65::PublicKey::try_from_bytes(pk_array).map_err(|_| {
        PQCVerifyError::InvalidPublicKey {
            algorithm: "mldsa65".to_string(),
            details: "Invalid public key format".to_string(),
        }
    })?;

    let sig_array: [u8; ml_dsa_65::SIG_LEN] =
        sig.try_into()
            .map_err(|_| PQCVerifyError::InvalidSignature {
                algorithm: "mldsa65".to_string(),
                details: format!("Expected {} bytes, got {}", ml_dsa_65::SIG_LEN, sig.len()),
            })?;

    if crate::stats::measured(|| pk_obj.verify(msg, &sig_array, &[])) {
        Ok(())
    } else {
        Err(PQCVerifyError::VerificationFailed {
            algorithm: "mldsa65".to_string(),
        })
    }
}
