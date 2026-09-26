//! Runtime signature backends. Feature selection preserves the existing node API.

#[allow(clippy::upper_case_acronyms)]
pub trait PQC {
    fn keypair() -> (Vec<u8>, Vec<u8>); // (sk, pk)
    fn sign(sk: &[u8], msg: &[u8]) -> Vec<u8>;
    fn verify(pk: &[u8], msg: &[u8], sig: &[u8]) -> bool;
    const ALG: &'static str;
}


mod dilithium_fips204;
pub use dilithium_fips204::MlDsa65 as ActivePQC;


// New multi-algorithm PQC verification module
pub mod pqc_verify;
pub use pqc_verify::{verify, verify_default, PQCAlgorithm, PQCVerifyError};

pub use dytallix_protocol_types::{canonical_json, sha3_256};

/// Recovery signature verification with a mandatory FIPS 204 backend.
pub mod recovery;

/// Independent fee sponsor verification with a mandatory FIPS 204 backend.
pub mod recovery_sponsor;

/// Complete ordinary account signatures with explicit limits and a FIPS backend.
pub mod ordinary;

/// Provisional ordinary-v3 signatures. No consensus activation.
pub mod ordinary_v3;
