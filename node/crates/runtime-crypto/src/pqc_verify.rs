//! Post-Quantum Cryptographic signature verification module
//!
//! This module provides unified signature verification for multiple PQC algorithms:
//! - ML-DSA-65 (operational default)
//! - Explicit ML-DSA-87 for isolated development compatibility
//! - Legacy Dilithium5 only with the pqc-real backend
//! - ML-DSA-65
//! - Falcon1024 (feature-gated)
//! - SPHINCS+ SHA2-128s-simple (feature-gated)
//!
//! The module supports feature flags to enable/disable specific algorithms
//! and provides structured error handling for unsupported algorithms or malformed inputs.

use std::str::FromStr;
use thiserror::Error;

#[cfg(feature = "pqc-real")]
use pqcrypto_dilithium::dilithium5;
#[cfg(all(feature = "pqc-real", feature = "falcon"))]
use pqcrypto_falcon::falcon1024;
#[cfg(all(feature = "pqc-real", feature = "sphincs"))]
use pqcrypto_sphincsplus::sphincssha2128ssimple;
#[cfg(feature = "pqc-real")]
use pqcrypto_traits::sign::{DetachedSignature as _, PublicKey as SignPublicKey, SignedMessage};

#[cfg(feature = "pqc-fips204")]
use fips204::ml_dsa_65;
#[cfg(feature = "mldsa87-development")]
use fips204::ml_dsa_87;
#[cfg(feature = "pqc-fips204")]
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
            #[cfg(all(
                feature = "pqc-mock",
                not(any(feature = "pqc-real", feature = "pqc-fips204"))
            ))]
            // Explicit development mock builds only.
            "mock-blake3" => Ok(PQCAlgorithm::Dilithium5),
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
    fn non_pqc_builds_fail_closed_unless_mock_enabled() {
        // This test is only meaningful for builds where neither pqc-real nor pqc-fips204 are enabled.
        // In those builds:
        // - if pqc-mock is enabled, verification is allowed (dev/testing only)
        // - otherwise, verification must fail closed
        #[cfg(all(
            not(feature = "pqc-real"),
            not(feature = "pqc-fips204"),
            not(feature = "pqc-mock")
        ))]
        {
            use super::*;
            let result = verify(b"pk", b"msg", b"sig", PQCAlgorithm::Dilithium5);
            assert!(matches!(
                result,
                Err(PQCVerifyError::FeatureNotCompiled { .. })
            ));
        }
    }

    #[test]
    fn fips204_operational_lengths_are_mldsa65() {
        // The operational profile fixes exact FIPS 204 encodings.
        #[cfg(feature = "pqc-fips204")]
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
    #[cfg(all(
        feature = "pqc-mock",
        not(any(feature = "pqc-real", feature = "pqc-fips204"))
    ))]
    {
        // Mock verification for development/testing only.
        if pubkey.is_empty() || msg.is_empty() || sig.is_empty() {
            return Err(PQCVerifyError::InvalidSignature {
                algorithm: alg.as_str().to_string(),
                details: "Empty input".to_string(),
            });
        }
        tracing::error!(
            "PQC verification is running in pqc-mock mode; this MUST NOT be used in production"
        );
        return Ok(());
    }

    #[cfg(all(
        not(feature = "pqc-mock"),
        not(any(feature = "pqc-real", feature = "pqc-fips204"))
    ))]
    {
        return Err(PQCVerifyError::FeatureNotCompiled {
            feature: "pqc-fips204 or pqc-real".to_string(),
        });
    }

    #[cfg(feature = "pqc-fips204")]
    {
        return match alg {
            // Never reinterpret pre-standard Dilithium bytes as FIPS 204 ML-DSA.
            #[cfg(feature = "mldsa87-development")]
            PQCAlgorithm::MlDsa87 => verify_mldsa87_fips204(pubkey, msg, sig, "mldsa87"),
            PQCAlgorithm::MlDsa65 => verify_mldsa65_fips204(pubkey, msg, sig),
            _ => Err(PQCVerifyError::UnsupportedAlgorithm(
                alg.as_str().to_string(),
            )),
        };
    }

    #[cfg(all(feature = "pqc-real", not(feature = "pqc-fips204")))]
    {
        match alg {
            PQCAlgorithm::Dilithium5 => verify_dilithium5(pubkey, msg, sig),
            PQCAlgorithm::MlDsa65 | PQCAlgorithm::MlDsa87 => {
                Err(PQCVerifyError::FeatureNotCompiled {
                    feature: "pqc-fips204".to_string(),
                })
            }
            PQCAlgorithm::Falcon1024 => {
                #[cfg(feature = "falcon")]
                {
                    verify_falcon1024(pubkey, msg, sig)
                }
                #[cfg(not(feature = "falcon"))]
                {
                    Err(PQCVerifyError::FeatureNotCompiled {
                        feature: "falcon".to_string(),
                    })
                }
            }
            PQCAlgorithm::SphincsPlus => {
                #[cfg(feature = "sphincs")]
                {
                    verify_sphincs_plus(pubkey, msg, sig)
                }
                #[cfg(not(feature = "sphincs"))]
                {
                    Err(PQCVerifyError::FeatureNotCompiled {
                        feature: "sphincs".to_string(),
                    })
                }
            }
        }
    }
}

#[cfg(feature = "pqc-real")]
fn verify_dilithium5(pubkey: &[u8], msg: &[u8], sig: &[u8]) -> Result<(), PQCVerifyError> {
    let pk = dilithium5::PublicKey::from_bytes(pubkey).map_err(|_| {
        PQCVerifyError::InvalidPublicKey {
            algorithm: "dilithium5".to_string(),
            details: format!(
                "Expected {} bytes, got {}",
                dilithium5::public_key_bytes(),
                pubkey.len()
            ),
        }
    })?;

    // Try detached signature first (standard format for FIPS 204 / ML-DSA)
    if let Ok(detached_sig) = dilithium5::DetachedSignature::from_bytes(sig) {
        return match dilithium5::verify_detached_signature(&detached_sig, msg, &pk) {
            Ok(_) => Ok(()),
            Err(_) => Err(PQCVerifyError::VerificationFailed {
                algorithm: "dilithium5".to_string(),
            }),
        };
    }

    // Fallback: try SignedMessage format (legacy compatibility)
    let signed_msg = dilithium5::SignedMessage::from_bytes(sig).map_err(|_| {
        PQCVerifyError::InvalidSignature {
            algorithm: "dilithium5".to_string(),
            details: "Invalid signature format (neither detached nor signed message)".to_string(),
        }
    })?;

    match dilithium5::open(&signed_msg, &pk) {
        Ok(opened_msg) => {
            if opened_msg == msg {
                Ok(())
            } else {
                Err(PQCVerifyError::VerificationFailed {
                    algorithm: "dilithium5".to_string(),
                })
            }
        }
        Err(_) => Err(PQCVerifyError::VerificationFailed {
            algorithm: "dilithium5".to_string(),
        }),
    }
}

#[cfg(all(feature = "pqc-real", feature = "falcon"))]
fn verify_falcon1024(pubkey: &[u8], msg: &[u8], sig: &[u8]) -> Result<(), PQCVerifyError> {
    let pk = falcon1024::PublicKey::from_bytes(pubkey).map_err(|_| {
        PQCVerifyError::InvalidPublicKey {
            algorithm: "falcon1024".to_string(),
            details: format!(
                "Expected {} bytes, got {}",
                falcon1024::public_key_bytes(),
                pubkey.len()
            ),
        }
    })?;

    let signed_msg = falcon1024::SignedMessage::from_bytes(sig).map_err(|_| {
        PQCVerifyError::InvalidSignature {
            algorithm: "falcon1024".to_string(),
            details: "Invalid signed message format".to_string(),
        }
    })?;

    match falcon1024::open(&signed_msg, &pk) {
        Ok(opened_msg) => {
            if opened_msg == msg {
                tracing::debug!("Falcon1024 signature verification successful");
                Ok(())
            } else {
                tracing::warn!("Falcon1024 signature verification failed: message mismatch");
                Err(PQCVerifyError::VerificationFailed {
                    algorithm: "falcon1024".to_string(),
                })
            }
        }
        Err(_) => {
            tracing::warn!("Falcon1024 signature verification failed: invalid signature");
            Err(PQCVerifyError::VerificationFailed {
                algorithm: "falcon1024".to_string(),
            })
        }
    }
}

#[cfg(all(feature = "pqc-real", feature = "sphincs"))]
fn verify_sphincs_plus(pubkey: &[u8], msg: &[u8], sig: &[u8]) -> Result<(), PQCVerifyError> {
    let pk = sphincssha2128ssimple::PublicKey::from_bytes(pubkey).map_err(|_| {
        PQCVerifyError::InvalidPublicKey {
            algorithm: "sphincs_sha2_128s_simple".to_string(),
            details: format!(
                "Expected {} bytes, got {}",
                sphincssha2128ssimple::public_key_bytes(),
                pubkey.len()
            ),
        }
    })?;

    let signed_msg = sphincssha2128ssimple::SignedMessage::from_bytes(sig).map_err(|_| {
        PQCVerifyError::InvalidSignature {
            algorithm: "sphincs_sha2_128s_simple".to_string(),
            details: "Invalid signed message format".to_string(),
        }
    })?;

    match sphincssha2128ssimple::open(&signed_msg, &pk) {
        Ok(opened_msg) => {
            if opened_msg == msg {
                tracing::debug!("SPHINCS+ signature verification successful");
                Ok(())
            } else {
                tracing::warn!("SPHINCS+ signature verification failed: message mismatch");
                Err(PQCVerifyError::VerificationFailed {
                    algorithm: "sphincs_sha2_128s_simple".to_string(),
                })
            }
        }
        Err(_) => {
            tracing::warn!("SPHINCS+ signature verification failed: invalid signature");
            Err(PQCVerifyError::VerificationFailed {
                algorithm: "sphincs_sha2_128s_simple".to_string(),
            })
        }
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
        #[cfg(all(
            feature = "pqc-mock",
            not(any(feature = "pqc-real", feature = "pqc-fips204"))
        ))]
        {
            // Mock should succeed with non-empty inputs
            assert!(verify(&[1], &[2], &[3], PQCAlgorithm::Dilithium5).is_ok());

            // Mock should fail with empty inputs
            assert!(verify(&[], &[2], &[3], PQCAlgorithm::Dilithium5).is_err());
            assert!(verify(&[1], &[], &[3], PQCAlgorithm::Dilithium5).is_err());
            assert!(verify(&[1], &[2], &[], PQCAlgorithm::Dilithium5).is_err());
        }
    }

    #[test]
    fn test_default_verify_compatibility() {
        // Test the compatibility function with mock data
        assert!(!verify_default(&[], &[], &[])); // Should fail for empty inputs

        #[cfg(all(
            feature = "pqc-mock",
            not(any(feature = "pqc-real", feature = "pqc-fips204"))
        ))]
        assert!(verify_default(&[1], &[2], &[3])); // Mock should succeed
    }

    #[cfg(all(feature = "pqc-real", not(feature = "falcon")))]
    #[test]
    fn test_feature_not_compiled_falcon() {
        let result = verify(
            b"pubkey",
            b"message",
            b"signature",
            PQCAlgorithm::Falcon1024,
        );

        match result {
            Err(PQCVerifyError::FeatureNotCompiled { feature }) => {
                assert_eq!(feature, "falcon");
            }
            _ => panic!("Expected FeatureNotCompiled error"),
        }
    }

    #[cfg(all(feature = "pqc-real", not(feature = "sphincs")))]
    #[test]
    fn test_feature_not_compiled_sphincs() {
        let result = verify(
            b"pubkey",
            b"message",
            b"signature",
            PQCAlgorithm::SphincsPlus,
        );

        match result {
            Err(PQCVerifyError::FeatureNotCompiled { feature }) => {
                assert_eq!(feature, "sphincs");
            }
            _ => panic!("Expected FeatureNotCompiled error"),
        }
    }

    #[cfg(feature = "pqc-fips204")]
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

#[cfg(feature = "mldsa87-development")]
fn verify_mldsa87_fips204(
    pubkey: &[u8],
    msg: &[u8],
    sig: &[u8],
    algorithm: &str,
) -> Result<(), PQCVerifyError> {
    // FIPS 204 ML-DSA-87. The caller supplies the explicit or legacy wire label.
    let pk_array: [u8; ml_dsa_87::PK_LEN] =
        pubkey
            .try_into()
            .map_err(|_| PQCVerifyError::InvalidPublicKey {
                algorithm: algorithm.to_string(),
                details: format!("Expected {} bytes, got {}", ml_dsa_87::PK_LEN, pubkey.len()),
            })?;

    let pk_obj = ml_dsa_87::PublicKey::try_from_bytes(pk_array).map_err(|_| {
        PQCVerifyError::InvalidPublicKey {
            algorithm: algorithm.to_string(),
            details: "Invalid public key format".to_string(),
        }
    })?;

    let sig_array: [u8; ml_dsa_87::SIG_LEN] =
        sig.try_into()
            .map_err(|_| PQCVerifyError::InvalidSignature {
                algorithm: algorithm.to_string(),
                details: format!("Expected {} bytes, got {}", ml_dsa_87::SIG_LEN, sig.len()),
            })?;

    if pk_obj.verify(msg, &sig_array, &[]) {
        Ok(())
    } else {
        Err(PQCVerifyError::VerificationFailed {
            algorithm: algorithm.to_string(),
        })
    }
}

#[cfg(feature = "pqc-fips204")]
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
