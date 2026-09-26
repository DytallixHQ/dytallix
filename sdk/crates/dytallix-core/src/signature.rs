use fips204::ml_dsa_65;
#[cfg(feature = "compatibility")]
use fips204::ml_dsa_87;
use fips204::traits::{SerDes, Verifier};
#[cfg(feature = "compatibility")]
use pqcrypto_sphincsplus::sphincsshake192ssimple;
#[cfg(feature = "compatibility")]
use pqcrypto_traits::sign::{
    DetachedSignature as DetachedSignatureTrait, PublicKey as PublicKeyTrait,
};

use crate::error::DytallixError;

const MLDSA65_PUBLIC_KEY_BYTES: usize = 1_952;
const MLDSA65_SIGNATURE_BYTES: usize = 3_309;
#[cfg(feature = "compatibility")]
const MLDSA87_PUBLIC_KEY_BYTES: usize = 2_592;
#[cfg(feature = "compatibility")]
const MLDSA87_SIGNATURE_BYTES: usize = 4_627;
#[cfg(feature = "compatibility")]
const SLHDSA_PUBLIC_KEY_BYTES: usize = 48;
#[cfg(feature = "compatibility")]
const SLHDSA_SIGNATURE_BYTES: usize = 16_224;

/// Verifies an ML-DSA-65 detached signature.
///
/// Returns an error when the public key or signature length is incorrect,
/// `Ok(false)` when verification fails, and `Ok(true)` when verification succeeds.
///
/// # Examples
///
/// ```rust
/// use dytallix_core::keypair::DytallixKeypair;
/// use dytallix_core::signature::verify_mldsa65;
///
/// let keypair = DytallixKeypair::generate();
/// let message = b"hello dytallix";
/// let signature = keypair.sign(message).unwrap();
/// assert!(verify_mldsa65(keypair.public_key(), message, &signature).unwrap());
/// ```
pub fn verify_mldsa65(
    pubkey: &[u8],
    message: &[u8],
    signature: &[u8],
) -> Result<bool, DytallixError> {
    if pubkey.len() != MLDSA65_PUBLIC_KEY_BYTES {
        return Err(DytallixError::InvalidKeySize {
            expected: MLDSA65_PUBLIC_KEY_BYTES,
            got: pubkey.len(),
        });
    }

    if signature.len() != MLDSA65_SIGNATURE_BYTES {
        return Err(DytallixError::InvalidSignatureSize {
            expected: MLDSA65_SIGNATURE_BYTES,
            got: signature.len(),
        });
    }

    let public_key = ml_dsa_65_public_key_from_bytes(pubkey)?;
    let detached = ml_dsa_65_signature_from_bytes(signature)?;

    Ok(public_key.verify(message, &detached, &[]))
}

/// Verifies an exact ML-DSA-87 signature with an empty FIPS context.
/// Incorrect lengths return an error. Failed authentication returns `Ok(false)`.
#[cfg(feature = "compatibility")]
pub fn verify_mldsa87(
    pubkey: &[u8],
    message: &[u8],
    signature: &[u8],
) -> Result<bool, DytallixError> {
    let public_bytes = pubkey
        .try_into()
        .map_err(|_| DytallixError::InvalidKeySize {
            expected: MLDSA87_PUBLIC_KEY_BYTES,
            got: pubkey.len(),
        })?;
    let detached: [u8; ml_dsa_87::SIG_LEN] =
        signature
            .try_into()
            .map_err(|_| DytallixError::InvalidSignatureSize {
                expected: MLDSA87_SIGNATURE_BYTES,
                got: signature.len(),
            })?;
    let public_key = ml_dsa_87::PublicKey::try_from_bytes(public_bytes)
        .map_err(|err| DytallixError::InvalidKeypair(err.to_string()))?;
    Ok(public_key.verify(message, &detached, &[]))
}

/// Verifies only the explicitly selected scheme. No size-based fallback occurs.
pub fn verify_for_scheme(
    scheme: crate::keypair::KeyScheme,
    pubkey: &[u8],
    message: &[u8],
    signature: &[u8],
) -> Result<bool, DytallixError> {
    use crate::keypair::KeyScheme;
    match scheme {
        KeyScheme::MlDsa65 => verify_mldsa65(pubkey, message, signature),
        KeyScheme::MlDsa87 => verify_mldsa87(pubkey, message, signature),
        KeyScheme::SlhDsa => verify_slhdsa(pubkey, message, signature),
    }
}

/// Legacy alias that verifies SPHINCS+-SHAKE-192s-simple signatures.
/// This method does not verify FIPS 205 SLH-DSA root authorizations.
///
/// Returns an error when the public key or signature length is incorrect,
/// `Ok(false)` when verification fails, and `Ok(true)` when verification succeeds.
///
/// # Examples
///
/// ```rust
/// use dytallix_core::keypair::DytallixKeypair;
/// use dytallix_core::signature::verify_slhdsa;
///
/// let keypair = DytallixKeypair::generate_slh_dsa();
/// let message = b"cold storage";
/// let signature = keypair.sign(message).unwrap();
/// assert!(verify_slhdsa(keypair.public_key(), message, &signature).unwrap());
/// ```
#[cfg(feature = "compatibility")]
pub fn verify_slhdsa(
    pubkey: &[u8],
    message: &[u8],
    signature: &[u8],
) -> Result<bool, DytallixError> {
    if pubkey.len() != SLHDSA_PUBLIC_KEY_BYTES {
        return Err(DytallixError::InvalidKeySize {
            expected: SLHDSA_PUBLIC_KEY_BYTES,
            got: pubkey.len(),
        });
    }

    if signature.len() != SLHDSA_SIGNATURE_BYTES {
        return Err(DytallixError::InvalidSignatureSize {
            expected: SLHDSA_SIGNATURE_BYTES,
            got: signature.len(),
        });
    }

    let public_key = <sphincsshake192ssimple::PublicKey as PublicKeyTrait>::from_bytes(pubkey)
        .map_err(|err| DytallixError::InvalidKeypair(err.to_string()))?;
    let detached =
        <sphincsshake192ssimple::DetachedSignature as DetachedSignatureTrait>::from_bytes(
            signature,
        )
        .map_err(|err| DytallixError::InvalidSignature(err.to_string()))?;

    Ok(sphincsshake192ssimple::verify_detached_signature(&detached, message, &public_key).is_ok())
}

/// Verifies a batch of ML-DSA-65 signatures.
///
/// Every item is checked even if earlier items fail. Inputs with invalid sizes
/// or invalid signatures are reported as `false` in the returned vector.
///
/// # Examples
///
/// ```rust
/// use dytallix_core::keypair::DytallixKeypair;
/// use dytallix_core::signature::batch_verify_mldsa65;
///
/// let keypair = DytallixKeypair::generate();
/// let message = b"hello dytallix".to_vec();
/// let signature = keypair.sign(&message).unwrap();
/// let results = batch_verify_mldsa65(&[(
///     keypair.public_key().to_vec(),
///     message,
///     signature,
/// )])
/// .unwrap();
/// assert_eq!(results, vec![true]);
/// ```
pub fn batch_verify_mldsa65(
    items: &[(Vec<u8>, Vec<u8>, Vec<u8>)],
) -> Result<Vec<bool>, DytallixError> {
    let mut results = Vec::with_capacity(items.len());

    for (pubkey, message, signature) in items {
        let valid = verify_mldsa65(pubkey, message, signature).unwrap_or(false);
        results.push(valid);
    }

    Ok(results)
}

fn ml_dsa_65_public_key_from_bytes(pubkey: &[u8]) -> Result<ml_dsa_65::PublicKey, DytallixError> {
    let public_key = pubkey
        .try_into()
        .map_err(|_| DytallixError::InvalidKeySize {
            expected: MLDSA65_PUBLIC_KEY_BYTES,
            got: pubkey.len(),
        })?;
    ml_dsa_65::PublicKey::try_from_bytes(public_key)
        .map_err(|err| DytallixError::InvalidKeypair(err.to_string()))
}

fn ml_dsa_65_signature_from_bytes(
    signature: &[u8],
) -> Result<[u8; ml_dsa_65::SIG_LEN], DytallixError> {
    signature
        .try_into()
        .map_err(|_| DytallixError::InvalidSignatureSize {
            expected: MLDSA65_SIGNATURE_BYTES,
            got: signature.len(),
        })
}

#[cfg(not(feature = "compatibility"))]
pub fn verify_mldsa87(
    _pubkey: &[u8],
    _message: &[u8],
    _signature: &[u8],
) -> Result<bool, DytallixError> {
    Err(DytallixError::CryptoError(
        "this build supports only ML-DSA-65".into(),
    ))
}

#[cfg(not(feature = "compatibility"))]
pub fn verify_slhdsa(
    _pubkey: &[u8],
    _message: &[u8],
    _signature: &[u8],
) -> Result<bool, DytallixError> {
    Err(DytallixError::CryptoError(
        "this build supports only ML-DSA-65".into(),
    ))
}
