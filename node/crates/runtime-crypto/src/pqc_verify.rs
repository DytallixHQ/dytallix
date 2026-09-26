//! Post-Quantum Cryptographic signature verification module
//!
//! Signature verification uses the pure-Rust FIPS 204 backend. ML-DSA-65 is
//! the operational algorithm. Other algorithm labels are parsed so that they
//! can be rejected with structured errors.

use std::str::FromStr;
use thiserror::Error;


use fips204::ml_dsa_65;
use fips204::traits::{SerDes, Verifier};

/// PQC algorithm identifiers
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PQCAlgorithm {
    Dilithium5,
    MlDsa87,
    #[default]
    MlDsa65,
    Falcon1024,
    SphincsPlus,
}

impl PQCAlgorithm {
    /// Get algorithm identifier string
    pub fn as_str(&self) -> &'static str {
        match self {
            PQCAlgorithm::Dilithium5 => "dilithium5",
            PQCAlgorithm::MlDsa87 => "mldsa87",
            PQCAlgorithm::MlDsa65 => "mldsa65",
            PQCAlgorithm::Falcon1024 => "falcon1024",
            PQCAlgorithm::SphincsPlus => "sphincs_sha2_128s_simple",
        }
    }
}

impl FromStr for PQCAlgorithm {
    type Err = PQCVerifyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "dilithium5" => Ok(PQCAlgorithm::Dilithium5),
            "mldsa87" => Ok(PQCAlgorithm::MlDsa87),
            "mldsa65" => Ok(PQCAlgorithm::MlDsa65),
            "falcon1024" => Ok(PQCAlgorithm::Falcon1024),
            "sphincs_sha2_128s_simple" => Ok(PQCAlgorithm::SphincsPlus),
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

/// Main verification function supporting multiple PQC algorithms
///
/// # Arguments
/// * `pubkey` - The public key bytes
/// * `msg` - The message that was signed
/// * `sig` - The signature bytes
/// * `alg` - The algorithm to use for verification
///
/// # Returns
/// * `Ok(())` if verification succeeds
/// * `Err(PQCVerifyError)` with structured error information
///
/// # Example
/// ```rust,ignore
/// use dytallix_fast_node::crypto::pqc_verify::{verify, PQCAlgorithm};
///
/// let result = verify(
///     &public_key_bytes,
///     &message_bytes,
///     &signature_bytes,
///     PQCAlgorithm::Dilithium5,
/// );
/// ```
pub fn verify(
    pubkey: &[u8],
    msg: &[u8],
    sig: &[u8],
    alg: PQCAlgorithm,
) -> Result<(), PQCVerifyError> {


    {
        return match alg {
            // Never reinterpret pre-standard Dilithium bytes as FIPS 204 ML-DSA.
            PQCAlgorithm::MlDsa65 => verify_mldsa65_fips204(pubkey, msg, sig),
            _ => Err(PQCVerifyError::UnsupportedAlgorithm(
                alg.as_str().to_string(),
            )),
        };
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
    fn test_algorithm_parsing() {
        assert_eq!(
            PQCAlgorithm::from_str("dilithium5").unwrap(),
            PQCAlgorithm::Dilithium5
        );
        assert_eq!(
            PQCAlgorithm::from_str("falcon1024").unwrap(),
            PQCAlgorithm::Falcon1024
        );
        assert_eq!(
            PQCAlgorithm::from_str("mldsa65").unwrap(),
            PQCAlgorithm::MlDsa65
        );
        assert_eq!(
            PQCAlgorithm::from_str("sphincs_sha2_128s_simple").unwrap(),
            PQCAlgorithm::SphincsPlus
        );
        assert!(PQCAlgorithm::from_str("unknown").is_err());
        assert!(PQCAlgorithm::from_str("dilithium3").is_err());
        assert_eq!(PQCAlgorithm::default(), PQCAlgorithm::MlDsa65);
    }

    #[test]
    fn test_algorithm_strings() {
        assert_eq!(PQCAlgorithm::Dilithium5.as_str(), "dilithium5");
        assert_eq!(PQCAlgorithm::MlDsa65.as_str(), "mldsa65");
        assert_eq!(PQCAlgorithm::Falcon1024.as_str(), "falcon1024");
        assert_eq!(
            PQCAlgorithm::SphincsPlus.as_str(),
            "sphincs_sha2_128s_simple"
        );
    }

    #[test]
    fn test_mock_verification() {
    }

    #[test]
    fn test_default_verify_compatibility() {
        // Test the compatibility function with mock data
        assert!(!verify_default(&[], &[], &[])); // Should fail for empty inputs

         // Mock should succeed
    }



    #[test]
    fn test_fips204_build_rejects_non_dilithium_algorithms() {
        let result = verify(
            b"pubkey",
            b"message",
            b"signature",
            PQCAlgorithm::Falcon1024,
        );
        assert!(matches!(
            result,
            Err(PQCVerifyError::UnsupportedAlgorithm(_))
        ));

        let result = verify(
            b"pubkey",
            b"message",
            b"signature",
            PQCAlgorithm::SphincsPlus,
        );
        assert!(matches!(
            result,
            Err(PQCVerifyError::UnsupportedAlgorithm(_))
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

    if pk_obj.verify(msg, &sig_array, &[]) {
        Ok(())
    } else {
        Err(PQCVerifyError::VerificationFailed {
            algorithm: "mldsa65".to_string(),
        })
    }
}
