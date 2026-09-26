//! dytallix-core — cryptographic primitives for the Dytallix PQC-native blockchain.
//!
//! Provides explicit ML-DSA-65/87 keypairs and signing, legacy Bech32m address derivation
//! and validation, signature verification, BLAKE3 hashing, and canonical error types.
//!
//! # Quick Example
//!
//! ```rust
//! use dytallix_core::address::DAddr;
//! use dytallix_core::keypair::DytallixKeypair;
//! use dytallix_core::signature::verify_mldsa65;
//!
//! let keypair = DytallixKeypair::generate();
//! let addr = DAddr::from_public_key(keypair.public_key()).unwrap();
//! let message = b"hello dytallix";
//! let sig = keypair.sign(message).unwrap();
//!
//! assert!(addr.as_str().starts_with("dytallix1"));
//! assert!(verify_mldsa65(keypair.public_key(), message, &sig).unwrap());
//! ```
#[cfg(all(feature = "strict-mldsa65", feature = "compatibility"))]
compile_error!(
    "strict-mldsa65 cannot include compatibility implementations; disable default features"
);
pub mod address;
pub mod error;
pub mod hash;
pub mod keypair;
pub mod signature;

pub use error::DytallixError;

#[cfg(test)]
mod tests {
    use bech32::primitives::decode::CheckedHrpstring;
    use bech32::{Bech32m, Hrp};

    use super::address::DAddr;
    use super::keypair::DytallixKeypair;
    use super::signature::verify_mldsa65;

    #[test]
    fn keypair_sizes() {
        let keypair = DytallixKeypair::generate();

        assert_eq!(keypair.public_key().len(), 1_952);
        assert_eq!(keypair.private_key().len(), 4_032);
    }

    #[test]
    fn signature_size() {
        let keypair = DytallixKeypair::generate();
        let message = [0xAB; 32];
        let signature = keypair.sign(&message).unwrap();

        assert_eq!(signature.len(), 3_309);
    }

    #[test]
    fn repeat_signing_stays_verifiable() {
        let keypair = DytallixKeypair::generate();
        let message = b"deterministic dytallix signing";

        let signature_a = keypair.sign(message).unwrap();
        let signature_b = keypair.sign(message).unwrap();

        assert_eq!(signature_a.len(), 3_309);
        assert_eq!(signature_b.len(), 3_309);
        assert!(verify_mldsa65(keypair.public_key(), message, &signature_a).unwrap());
        assert!(verify_mldsa65(keypair.public_key(), message, &signature_b).unwrap());
    }

    #[test]
    fn address_generation() {
        let keypair = DytallixKeypair::generate();
        let addr = DAddr::from_public_key(keypair.public_key()).unwrap();

        assert!(addr.as_str().starts_with("dytallix1"));

        let checked = CheckedHrpstring::new::<Bech32m>(addr.as_str()).unwrap();
        let decoded = checked.byte_iter().collect::<Vec<u8>>();

        assert_eq!(checked.hrp(), Hrp::parse("dytallix").unwrap());
        assert_eq!(decoded.len(), 32);
    }

    #[test]
    fn address_validation_catches_typos() {
        let keypair = DytallixKeypair::generate();
        let addr = DAddr::from_public_key(keypair.public_key()).unwrap();
        let mut chars = addr.as_str().chars().collect::<Vec<char>>();
        let index = "dytallix1".len();
        chars[index] = if chars[index] == 'q' { 'p' } else { 'q' };
        let typo = chars.into_iter().collect::<String>();

        let err = DAddr::from_str(&typo).unwrap_err();
        assert!(err.to_string().contains("checksum failed"));
    }

    #[test]
    fn signature_verification_round_trip() {
        let keypair = DytallixKeypair::generate();
        let message = b"round trip verification";
        let mut signature = keypair.sign(message).unwrap();

        assert!(verify_mldsa65(keypair.public_key(), message, &signature).unwrap());

        signature[0] ^= 0x01;
        assert!(!verify_mldsa65(keypair.public_key(), message, &signature).unwrap());
    }
}
