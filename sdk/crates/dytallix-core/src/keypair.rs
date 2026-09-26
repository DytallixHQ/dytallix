use blake3::Hasher as Blake3Hasher;
use fips204::ml_dsa_65;
#[cfg(feature = "compatibility")]
use fips204::ml_dsa_87;
use fips204::traits::{KeyGen, SerDes, Signer};
#[cfg(feature = "compatibility")]
use pqcrypto_sphincsplus::sphincsshake192ssimple;
#[cfg(feature = "compatibility")]
use pqcrypto_traits::sign::{
    DetachedSignature as DetachedSignatureTrait, PublicKey as PublicKeyTrait,
    SecretKey as SecretKeyTrait,
};

use crate::error::DytallixError;

const MLDSA65_PRIVATE_KEY_BYTES: usize = 4_032;
const MLDSA65_SIGNATURE_BYTES: usize = 3_309;
const MLDSA87_PUBLIC_KEY_BYTES: usize = 2_592;
const MLDSA87_PRIVATE_KEY_BYTES: usize = 4_896;
#[cfg(feature = "compatibility")]
const MLDSA87_SIGNATURE_BYTES: usize = 4_627;
const SLHDSA_PUBLIC_KEY_BYTES: usize = 48;
const SLHDSA_PRIVATE_KEY_BYTES: usize = 96;
#[cfg(feature = "compatibility")]
const SLHDSA_SIGNATURE_BYTES: usize = 16_224;

/// Supported signature schemes for Dytallix keypairs.
///
/// `MlDsa65` remains the default. `MlDsa87` requires explicit selection.
/// `SlhDsa` is a legacy SPHINCS+ compatibility name. It is not FIPS 205.
///
/// # Examples
///
/// ```rust
/// use dytallix_core::keypair::{DytallixKeypair, KeyScheme};
///
/// let keypair = DytallixKeypair::generate();
/// assert_eq!(keypair.scheme(), KeyScheme::MlDsa65);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum KeyScheme {
    /// ML-DSA-65 (FIPS 204), the canonical Dytallix signing scheme.
    MlDsa65,
    /// Legacy SPHINCS+-SHAKE-192s-simple. This is not FIPS 205 SLH-DSA.
    /// Preserve the Rust variant and old serialized name for existing keys.
    #[serde(rename = "LegacySphincsPlusShake192sSimple", alias = "SlhDsa")]
    SlhDsa,
    /// ML-DSA-87 (FIPS 204), available through explicit selection.
    MlDsa87,
}

impl KeyScheme {
    /// Reject an algorithm whose implementation is absent from this build.
    pub fn require_available(self) -> Result<(), DytallixError> {
        #[cfg(not(feature = "compatibility"))]
        if self != Self::MlDsa65 {
            return Err(DytallixError::CryptoError(
                "this build supports only ML-DSA-65".into(),
            ));
        }
        Ok(())
    }

    /// Returns the backend identity without treating legacy SPHINCS+ as FIPS 205.
    pub fn algorithm_id(self) -> &'static str {
        match self {
            Self::MlDsa65 => "mldsa65",
            Self::MlDsa87 => "mldsa87",
            Self::SlhDsa => "legacy-sphincsplus-shake-192s-simple",
        }
    }

    /// Rejects schemes outside the approved production operational profile.
    /// This check does not qualify a complete protocol or authorize mainnet.
    pub fn require_production_operational(self) -> Result<(), DytallixError> {
        if self == Self::MlDsa65 {
            Ok(())
        } else {
            Err(DytallixError::CryptoError(
                "production operational signatures require exact ML-DSA-65".into(),
            ))
        }
    }
}

/// In-memory Dytallix keypair.
///
/// The keypair stores raw public and private key bytes plus the active scheme.
///
/// # Examples
///
/// ```rust
/// use dytallix_core::keypair::{DytallixKeypair, KeyScheme};
///
/// let keypair = DytallixKeypair::generate();
/// assert_eq!(keypair.scheme(), KeyScheme::MlDsa65);
/// assert_eq!(keypair.public_key().len(), 1952);
/// ```
pub struct DytallixKeypair {
    public_key: Vec<u8>,
    private_key: Vec<u8>,
    scheme: KeyScheme,
}

impl DytallixKeypair {
    /// Generates a new ML-DSA-65 keypair.
    ///
    /// The returned public key is exactly 1,952 bytes and the private key is
    /// exactly 4,032 bytes.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use dytallix_core::keypair::DytallixKeypair;
    ///
    /// let keypair = DytallixKeypair::generate();
    /// assert_eq!(keypair.public_key().len(), 1952);
    /// assert_eq!(keypair.private_key().len(), 4032);
    /// ```
    pub fn generate() -> Self {
        let (public_key, private_key) = ml_dsa_65::KG::try_keygen()
            .expect("ML-DSA-65 key generation should succeed with the OS RNG");

        Self {
            public_key: public_key.into_bytes().to_vec(),
            private_key: private_key.into_bytes().to_vec(),
            scheme: KeyScheme::MlDsa65,
        }
    }

    /// Generates an explicit ML-DSA-87 keypair with 2,592 public-key bytes
    /// and 4,896 private-key bytes. This does not select an account address.
    #[cfg(feature = "compatibility")]
    pub fn generate_mldsa87() -> Self {
        let (public_key, private_key) = ml_dsa_87::KG::try_keygen()
            .expect("ML-DSA-87 key generation should succeed with the OS RNG");
        Self {
            public_key: public_key.into_bytes().to_vec(),
            private_key: private_key.into_bytes().to_vec(),
            scheme: KeyScheme::MlDsa87,
        }
    }

    /// Legacy alias for SPHINCS+-SHAKE-192s-simple key generation.
    /// This method does not generate a FIPS 205 root authorization key.
    ///
    /// The returned public key is exactly 48 bytes and the private key is
    /// exactly 96 bytes.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use dytallix_core::keypair::{DytallixKeypair, KeyScheme};
    ///
    /// let keypair = DytallixKeypair::generate_slh_dsa();
    /// assert_eq!(keypair.scheme(), KeyScheme::SlhDsa);
    /// assert_eq!(keypair.public_key().len(), 48);
    /// assert_eq!(keypair.private_key().len(), 96);
    /// ```
    #[cfg(feature = "compatibility")]
    pub fn generate_slh_dsa() -> Self {
        let (public_key, private_key) = sphincsshake192ssimple::keypair();

        Self {
            public_key: public_key.as_bytes().to_vec(),
            private_key: private_key.as_bytes().to_vec(),
            scheme: KeyScheme::SlhDsa,
        }
    }

    /// Reconstructs a keypair from raw private key bytes.
    ///
    /// Byte lengths are matched against the canonical Dytallix schemes. For
    /// ML-DSA-65, the public key is reconstructed from the packed secret key.
    /// That legacy path retains the pinned backend's malformed-secret limitation:
    /// public-key derivation can assert on inconsistent secret components.
    /// Use `from_keypair` for new imports of either ML-DSA scheme.
    /// Private-only ML-DSA-87 import fails closed because the pinned backend
    /// cannot safely derive a public key from arbitrary packed private bytes.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use dytallix_core::keypair::DytallixKeypair;
    ///
    /// let original = DytallixKeypair::generate();
    /// let restored = DytallixKeypair::from_private_key(original.private_key()).unwrap();
    /// assert_eq!(restored.public_key(), original.public_key());
    /// ```
    pub fn from_private_key(bytes: &[u8]) -> Result<Self, DytallixError> {
        match bytes.len() {
            MLDSA65_PRIVATE_KEY_BYTES => {
                let private_key = ml_dsa_65_private_key_from_bytes(bytes)?;
                let public_key = private_key.get_public_key();

                Ok(Self {
                    public_key: public_key.into_bytes().to_vec(),
                    private_key: private_key.into_bytes().to_vec(),
                    scheme: KeyScheme::MlDsa65,
                })
            }
            MLDSA87_PRIVATE_KEY_BYTES => Err(DytallixError::InvalidKeypair(
                "ML-DSA-87 import requires the matching public key; use from_keypair".into(),
            )),
            #[cfg(feature = "compatibility")]
            SLHDSA_PRIVATE_KEY_BYTES => {
                let private_key =
                    <sphincsshake192ssimple::SecretKey as SecretKeyTrait>::from_bytes(bytes)
                        .map_err(|err| DytallixError::InvalidKeypair(err.to_string()))?;
                let public_key = private_key.as_bytes()
                    [SLHDSA_PRIVATE_KEY_BYTES - SLHDSA_PUBLIC_KEY_BYTES..]
                    .to_vec();
                <sphincsshake192ssimple::PublicKey as PublicKeyTrait>::from_bytes(&public_key)
                    .map_err(|err| DytallixError::InvalidKeypair(err.to_string()))?;

                Ok(Self {
                    public_key,
                    private_key: private_key.as_bytes().to_vec(),
                    scheme: KeyScheme::SlhDsa,
                })
            }
            got => Err(DytallixError::InvalidKeypair(format!(
                "unknown private key length: {got} bytes"
            ))),
        }
    }

    /// Imports an explicitly declared keypair and checks possession against
    /// the supplied public key with an independent signature verification.
    ///
    /// ML-DSA imports never call the pinned backend's private-to-public
    /// derivation, which can assert on inconsistent packed secret components.
    /// Keys use exact scheme sizes and an empty FIPS context. This method does
    /// not derive an account address or establish current account authority.
    pub fn from_keypair(
        scheme: KeyScheme,
        public_key: &[u8],
        private_key: &[u8],
    ) -> Result<Self, DytallixError> {
        scheme.require_available()?;
        let (public_size, private_size, scheme_id) = match scheme {
            KeyScheme::MlDsa65 => (1_952, MLDSA65_PRIVATE_KEY_BYTES, b"mldsa65".as_slice()),
            KeyScheme::MlDsa87 => (
                MLDSA87_PUBLIC_KEY_BYTES,
                MLDSA87_PRIVATE_KEY_BYTES,
                b"mldsa87".as_slice(),
            ),
            KeyScheme::SlhDsa => (
                SLHDSA_PUBLIC_KEY_BYTES,
                SLHDSA_PRIVATE_KEY_BYTES,
                b"slhdsa".as_slice(),
            ),
        };
        if public_key.len() != public_size {
            return Err(DytallixError::InvalidKeySize {
                expected: public_size,
                got: public_key.len(),
            });
        }
        if private_key.len() != private_size {
            return Err(DytallixError::InvalidKeySize {
                expected: private_size,
                got: private_key.len(),
            });
        }
        let candidate = Self {
            public_key: public_key.to_vec(),
            private_key: private_key.to_vec(),
            scheme,
        };
        let mut challenge = b"dytallix-keypair-import-v1\0".to_vec();
        challenge.extend_from_slice(scheme_id);
        challenge.extend_from_slice(public_key);
        let signature = candidate.sign(&challenge)?;
        if !crate::signature::verify_for_scheme(scheme, public_key, &challenge, &signature)? {
            return Err(DytallixError::InvalidKeypair(
                "public and private key do not match".into(),
            ));
        }
        Ok(candidate)
    }

    /// Signs a message with the active scheme.
    ///
    /// ML-DSA signatures use an empty FIPS context. ML-DSA-65 signatures have
    /// 3,309 bytes and ML-DSA-87 signatures have 4,627 bytes. Both use separate
    /// deterministic signing seeds. Legacy SPHINCS+ signatures have 16,224 bytes.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use dytallix_core::keypair::DytallixKeypair;
    ///
    /// let keypair = DytallixKeypair::generate();
    /// let sig = keypair.sign(b"hello dytallix").unwrap();
    /// assert_eq!(sig.len(), 3309);
    /// ```
    pub fn sign(&self, message: &[u8]) -> Result<Vec<u8>, DytallixError> {
        match self.scheme {
            #[cfg(not(feature = "compatibility"))]
            KeyScheme::MlDsa87 | KeyScheme::SlhDsa => Err(DytallixError::CryptoError(
                "this build supports only ML-DSA-65".into(),
            )),
            KeyScheme::MlDsa65 => {
                let private_key = ml_dsa_65_private_key_from_bytes(&self.private_key)?;
                let seed = deterministic_mldsa65_seed(&self.private_key, message);
                let signature = private_key
                    .try_sign_with_seed(&seed, message, &[])
                    .map_err(|err| DytallixError::CryptoError(err.to_string()))?;
                let bytes = signature.to_vec();

                if bytes.len() != MLDSA65_SIGNATURE_BYTES {
                    return Err(DytallixError::InvalidSignatureSize {
                        expected: MLDSA65_SIGNATURE_BYTES,
                        got: bytes.len(),
                    });
                }

                Ok(bytes)
            }
            #[cfg(feature = "compatibility")]
            KeyScheme::MlDsa87 => {
                let private_key = ml_dsa_87_private_key_from_bytes(&self.private_key)?;
                let seed = deterministic_mldsa87_seed(&self.private_key, message);
                let signature = private_key
                    .try_sign_with_seed(&seed, message, &[])
                    .map_err(|err| DytallixError::CryptoError(err.to_string()))?;
                let bytes = signature.to_vec();
                if bytes.len() != MLDSA87_SIGNATURE_BYTES {
                    return Err(DytallixError::InvalidSignatureSize {
                        expected: MLDSA87_SIGNATURE_BYTES,
                        got: bytes.len(),
                    });
                }
                Ok(bytes)
            }
            #[cfg(feature = "compatibility")]
            KeyScheme::SlhDsa => {
                let private_key =
                    <sphincsshake192ssimple::SecretKey as SecretKeyTrait>::from_bytes(
                        &self.private_key,
                    )
                    .map_err(|err| DytallixError::InvalidKeypair(err.to_string()))?;
                let signature = sphincsshake192ssimple::detached_sign(message, &private_key);
                let bytes = signature.as_bytes().to_vec();

                if bytes.len() != SLHDSA_SIGNATURE_BYTES {
                    return Err(DytallixError::InvalidSignatureSize {
                        expected: SLHDSA_SIGNATURE_BYTES,
                        got: bytes.len(),
                    });
                }

                Ok(bytes)
            }
        }
    }

    /// Returns the raw public key bytes.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use dytallix_core::keypair::DytallixKeypair;
    ///
    /// let keypair = DytallixKeypair::generate();
    /// assert_eq!(keypair.public_key().len(), 1952);
    /// ```
    pub fn public_key(&self) -> &[u8] {
        &self.public_key
    }

    /// Returns the raw private key bytes.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use dytallix_core::keypair::DytallixKeypair;
    ///
    /// let keypair = DytallixKeypair::generate();
    /// assert_eq!(keypair.private_key().len(), 4032);
    /// ```
    pub fn private_key(&self) -> &[u8] {
        &self.private_key
    }

    /// Returns the active signing scheme.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use dytallix_core::keypair::{DytallixKeypair, KeyScheme};
    ///
    /// let keypair = DytallixKeypair::generate();
    /// assert_eq!(keypair.scheme(), KeyScheme::MlDsa65);
    /// ```
    pub fn scheme(&self) -> KeyScheme {
        self.scheme
    }
}

fn ml_dsa_65_private_key_from_bytes(bytes: &[u8]) -> Result<ml_dsa_65::PrivateKey, DytallixError> {
    let private_key = bytes
        .try_into()
        .map_err(|_| DytallixError::InvalidKeySize {
            expected: MLDSA65_PRIVATE_KEY_BYTES,
            got: bytes.len(),
        })?;
    ml_dsa_65::PrivateKey::try_from_bytes(private_key)
        .map_err(|err| DytallixError::InvalidKeypair(err.to_string()))
}

fn deterministic_mldsa65_seed(private_key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut hasher = Blake3Hasher::new();
    hasher.update(b"dytallix-ml-dsa-65-seed");
    hasher.update(private_key);
    hasher.update(message);
    *hasher.finalize().as_bytes()
}

#[cfg(feature = "compatibility")]
fn ml_dsa_87_private_key_from_bytes(bytes: &[u8]) -> Result<ml_dsa_87::PrivateKey, DytallixError> {
    let private_key = bytes
        .try_into()
        .map_err(|_| DytallixError::InvalidKeySize {
            expected: MLDSA87_PRIVATE_KEY_BYTES,
            got: bytes.len(),
        })?;
    ml_dsa_87::PrivateKey::try_from_bytes(private_key)
        .map_err(|err| DytallixError::InvalidKeypair(err.to_string()))
}

#[cfg(feature = "compatibility")]
fn deterministic_mldsa87_seed(private_key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut hasher = Blake3Hasher::new();
    hasher.update(b"dytallix-ml-dsa-87-seed");
    hasher.update(private_key);
    hasher.update(message);
    *hasher.finalize().as_bytes()
}
