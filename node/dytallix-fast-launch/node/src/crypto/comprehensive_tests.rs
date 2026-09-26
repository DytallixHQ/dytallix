//! Tests for PQC signature verification module
//!
//! These tests verify the multi-algorithm signature verification functionality
//! including error handling for unsupported algorithms and malformed inputs.

use crate::crypto::pqc_verify::{verify, PQCAlgorithm, PQCVerifyError};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_algorithm_parsing() {
        // Test valid algorithm parsing
        assert_eq!(
            PQCAlgorithm::from_str("dilithium5").unwrap(),
            PQCAlgorithm::Dilithium5
        );

        #[cfg(feature = "falcon")]
        assert_eq!(
            PQCAlgorithm::from_str("falcon1024").unwrap(),
            PQCAlgorithm::Falcon1024
        );

        #[cfg(feature = "sphincs")]
        assert_eq!(
            PQCAlgorithm::from_str("sphincs_sha2_128s_simple").unwrap(),
            PQCAlgorithm::SphincsPlus
        );

        // Test invalid algorithm parsing
        match PQCAlgorithm::from_str("invalid_algorithm") {
            Err(PQCVerifyError::UnsupportedAlgorithm(alg)) => {
                assert_eq!(alg, "invalid_algorithm");
            }
            _ => panic!("Expected UnsupportedAlgorithm error"),
        }
    }

    #[test]
    fn test_algorithm_string_conversion() {
        assert_eq!(PQCAlgorithm::Dilithium5.as_str(), "dilithium5");

        #[cfg(feature = "falcon")]
        assert_eq!(PQCAlgorithm::Falcon1024.as_str(), "falcon1024");

        #[cfg(feature = "sphincs")]
        assert_eq!(
            PQCAlgorithm::SphincsPlus.as_str(),
            "sphincs_sha2_128s_simple"
        );
    }

    #[test]
    fn test_default_algorithm() {
        assert_eq!(PQCAlgorithm::default(), PQCAlgorithm::Dilithium5);
    }

    #[test]
    fn test_empty_input_validation() {
        // Test empty public key
        let result = verify(&[], b"message", b"signature", PQCAlgorithm::Dilithium5);
        assert!(result.is_err());

        // Test empty message
        let result = verify(b"pubkey", &[], b"signature", PQCAlgorithm::Dilithium5);
        assert!(result.is_err());

        // Test empty signature
        let result = verify(b"pubkey", b"message", &[], PQCAlgorithm::Dilithium5);
        assert!(result.is_err());
    }

    #[cfg(feature = "pqc-real")]
    #[test]
    fn test_invalid_public_key_format() {
        // Test with invalid public key length for Dilithium5
        let short_pubkey = vec![0u8; 10]; // Too short
        let result = verify(
            &short_pubkey,
            b"test message",
            b"test signature",
            PQCAlgorithm::Dilithium5,
        );

        match result {
            Err(PQCVerifyError::InvalidPublicKey { algorithm, .. }) => {
                assert_eq!(algorithm, "dilithium5");
            }
            _ => panic!("Expected InvalidPublicKey error"),
        }
    }

    #[cfg(feature = "pqc-real")]
    #[test]
    fn test_invalid_signature_format() {
        use pqcrypto_dilithium::dilithium5;
        use pqcrypto_traits::sign::PublicKey as _;

        // Generate a valid public key
        let (pk, _sk) = dilithium5::keypair();
        let pk_bytes = pk.as_bytes();

        // Test with invalid signature format
        let invalid_sig = vec![0u8; 10]; // Too short and invalid format
        let result = verify(
            pk_bytes,
            b"test message",
            &invalid_sig,
            PQCAlgorithm::Dilithium5,
        );

        match result {
            Err(PQCVerifyError::InvalidSignature { algorithm, .. }) => {
                assert_eq!(algorithm, "dilithium5");
            }
            _ => panic!("Expected InvalidSignature error"),
        }
    }

    #[cfg(not(feature = "falcon"))]
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

    #[cfg(not(feature = "sphincs"))]
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

    #[cfg(not(feature = "pqc-real"))]
    #[test]
    fn test_mock_verification_success() {
        // Mock should succeed with non-empty inputs
        let result = verify(
            b"mock_pubkey",
            b"mock_message",
            b"mock_signature",
            PQCAlgorithm::Dilithium5,
        );
        assert!(result.is_ok());
    }

    #[cfg(not(feature = "pqc-real"))]
    #[test]
    fn test_mock_verification_failure() {
        // Mock should fail with empty inputs
        let result = verify(&[], b"message", b"signature", PQCAlgorithm::Dilithium5);
        assert!(result.is_err());

        let result = verify(b"pubkey", &[], b"signature", PQCAlgorithm::Dilithium5);
        assert!(result.is_err());

        let result = verify(b"pubkey", b"message", &[], PQCAlgorithm::Dilithium5);
        assert!(result.is_err());
    }

    #[test]
    fn test_compatibility_function() {
        use crate::crypto::pqc_verify::verify_default;

        // Test with empty inputs (should fail)
        assert!(!verify_default(&[], &[], &[]));

        #[cfg(not(feature = "pqc-real"))]
        {
            // Mock should succeed with non-empty inputs
            assert!(verify_default(b"pubkey", b"message", b"signature"));
        }
    }

    #[cfg(feature = "pqc-real")]
    #[test]
    fn test_dilithium_verification_flow() {
        use pqcrypto_dilithium::dilithium5;
        use pqcrypto_traits::sign::{PublicKey as _, SecretKey as _, SignedMessage as _};

        // Generate a real keypair
        let (pk, sk) = dilithium5::keypair();
        let message = b"Test message for Dilithium5";

        // Sign the message
        let signed_msg = dilithium5::sign(message, &sk);

        // Verify using our function
        let result = verify(
            pk.as_bytes(),
            message,
            signed_msg.as_bytes(),
            PQCAlgorithm::Dilithium5,
        );

        assert!(result.is_ok(), "Valid Dilithium5 signature should verify");

        // Test with wrong message (should fail)
        let wrong_message = b"Wrong message";
        let result = verify(
            pk.as_bytes(),
            wrong_message,
            signed_msg.as_bytes(),
            PQCAlgorithm::Dilithium5,
        );

        match result {
            Err(PQCVerifyError::VerificationFailed { algorithm }) => {
                assert_eq!(algorithm, "dilithium5");
            }
            _ => panic!("Expected VerificationFailed error for wrong message"),
        }
    }

    #[cfg(all(feature = "pqc-real", feature = "falcon"))]
    #[test]
    fn test_falcon_verification_flow() {
        use pqcrypto_falcon::falcon1024;
        use pqcrypto_traits::sign::{PublicKey as _, SecretKey as _, SignedMessage as _};

        // Generate a real keypair
        let (pk, sk) = falcon1024::keypair();
        let message = b"Test message for Falcon1024";

        // Sign the message
        let signed_msg = falcon1024::sign(message, &sk);

        // Verify using our function
        let result = verify(
            pk.as_bytes(),
            message,
            signed_msg.as_bytes(),
            PQCAlgorithm::Falcon1024,
        );

        assert!(result.is_ok(), "Valid Falcon1024 signature should verify");
    }

    #[cfg(all(feature = "pqc-real", feature = "sphincs"))]
    #[test]
    fn test_sphincs_verification_flow() {
        use pqcrypto_sphincsplus::sphincssha2128ssimple;
        use pqcrypto_traits::sign::{PublicKey as _, SecretKey as _, SignedMessage as _};

        // Generate a real keypair
        let (pk, sk) = sphincssha2128ssimple::keypair();
        let message = b"Test message for SPHINCS+";

        // Sign the message
        let signed_msg = sphincssha2128ssimple::sign(message, &sk);

        // Verify using our function
        let result = verify(
            pk.as_bytes(),
            message,
            signed_msg.as_bytes(),
            PQCAlgorithm::SphincsPlus,
        );

        assert!(
            result.is_ok(),
            "Valid SPHINCS+ signature should verify"
        );
    }

    #[test]
    fn test_error_display() {
        let error = PQCVerifyError::UnsupportedAlgorithm("test_alg".to_string());
        assert_eq!(format!("{}", error), "Unsupported algorithm: test_alg");

        let error = PQCVerifyError::InvalidPublicKey {
            algorithm: "dilithium5".to_string(),
            details: "wrong length".to_string(),
        };
        assert_eq!(
            format!("{}", error),
            "Invalid public key format for dilithium5: wrong length"
        );

        let error = PQCVerifyError::VerificationFailed {
            algorithm: "dilithium5".to_string(),
        };
        assert_eq!(
            format!("{}", error),
            "Signature verification failed for dilithium5"
        );
    }
}