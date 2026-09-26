use crate::error::DytallixError;

/// Returns the 32-byte BLAKE3 hash of arbitrary input bytes.
///
/// # Examples
///
/// ```rust
/// use dytallix_core::hash::blake3_hash;
///
/// let digest = blake3_hash(b"dytallix");
/// assert_eq!(digest.len(), 32);
/// ```
pub fn blake3_hash(input: &[u8]) -> [u8; 32] {
    *blake3::hash(input).as_bytes()
}

/// Returns the canonical 32-byte BLAKE3 hash of a public key.
///
/// # Examples
///
/// ```rust
/// use dytallix_core::hash::hash_public_key;
/// use dytallix_core::keypair::DytallixKeypair;
///
/// let keypair = DytallixKeypair::generate();
/// let digest = hash_public_key(keypair.public_key());
/// assert_eq!(digest.len(), 32);
/// ```
pub fn hash_public_key(pubkey: &[u8]) -> [u8; 32] {
    blake3_hash(pubkey)
}

#[allow(dead_code)]
fn _never_construct_hash_error(_: DytallixError) {}
