use thiserror::Error;

/// Canonical error type for dytallix-core operations.
///
/// # Examples
///
/// ```rust
/// use dytallix_core::DytallixError;
///
/// let err = DytallixError::InvalidAddress("checksum failed — check for typos".to_owned());
/// assert!(err.to_string().contains("Invalid address"));
/// ```
#[derive(Debug, Error)]
pub enum DytallixError {
    /// Address text failed validation.
    #[error("Invalid address: {0}")]
    InvalidAddress(String),
    /// Signature bytes were malformed or failed validation.
    #[error("Invalid signature: {0}")]
    InvalidSignature(String),
    /// Keypair bytes were malformed or could not be reconstructed.
    #[error("Invalid keypair: {0}")]
    InvalidKeypair(String),
    /// Bech32m encoding or decoding failed.
    #[error("Bech32m encoding error: {0}")]
    Bech32Error(String),
    /// Hashing failed.
    #[error("Hash error: {0}")]
    HashError(String),
    /// An underlying cryptographic primitive returned an error.
    #[error("Cryptography error: {0}")]
    CryptoError(String),
    /// Key material had an unexpected byte length.
    #[error("Invalid key size: expected {expected} bytes, got {got} bytes")]
    InvalidKeySize { expected: usize, got: usize },
    /// Signature material had an unexpected byte length.
    #[error("Invalid signature size: expected {expected} bytes, got {got} bytes")]
    InvalidSignatureSize { expected: usize, got: usize },
}
