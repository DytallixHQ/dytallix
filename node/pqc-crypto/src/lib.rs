//! Dytallix PQC Cryptography Library
//!
//! Provides key generation, signing, and verification for Dilithium, Falcon, and SPHINCS+.

pub enum PQCAlgorithm {
    Dilithium,
    Falcon,
    SphincsPlus,
}

use pqcrypto_dilithium::{dilithium3, dilithium5};
use pqcrypto_falcon::falcon1024;
// Correct SPHINCS+ import: crate provides sphincssha2128ssimple (not sphincssha256128ssimple)
use pqcrypto_kyber::kyber1024;
use pqcrypto_sphincsplus::sphincssha2128ssimple;
use pqcrypto_traits::kem::{
    Ciphertext, PublicKey as KemPublicKey, SecretKey as KemSecretKey, SharedSecret,
};
use pqcrypto_traits::sign::{
    PublicKey as SignPublicKey, SecretKey as SignSecretKey, SignedMessage,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Error, Debug)]
pub enum PQCError {
    #[error("Invalid key format: {0}")]
    InvalidKey(String),
    #[error("Invalid signature format: {0}")]
    InvalidSignature(String),
    #[error("Signature verification failed")]
    VerificationFailed,
    #[error("Unsupported algorithm: {0}")]
    UnsupportedAlgorithm(String),
    #[error("Key generation failed")]
    KeyGenerationFailed,
}

pub use dytallix_protocol_types::signature_algorithm::SignatureAlgorithm;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KeyExchangeAlgorithm {
    Kyber1024,
}

#[derive(Debug, Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct KeyPair {
    pub public_key: Vec<u8>,
    /// Secret key material (zeroized on drop)
    #[serde(skip)]
    pub secret_key: Vec<u8>,
    #[zeroize(skip)] // Algorithm enum doesn't need zeroization
    pub algorithm: SignatureAlgorithm,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signature {
    pub data: Vec<u8>,
    pub algorithm: SignatureAlgorithm,
}

#[derive(Debug, Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct KeyExchangeKeyPair {
    #[zeroize(skip)] // Public keys don't need zeroization
    pub public_key: Vec<u8>,
    #[serde(skip)]
    pub secret_key: Vec<u8>, // This will be zeroized on drop
    #[zeroize(skip)] // Algorithm enum doesn't need zeroization
    pub algorithm: KeyExchangeAlgorithm,
}

// Explicit private persistence representation. Public KeyPair JSON omits secrets.
#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
struct StoredKeys {
    signature_keypair: StoredSignatureKeyPair,
    key_exchange_keypair: StoredKeyExchangeKeyPair,
}

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
struct StoredSignatureKeyPair {
    public_key: Vec<u8>,
    secret_key: Vec<u8>,
    #[zeroize(skip)]
    algorithm: SignatureAlgorithm,
}

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
struct StoredKeyExchangeKeyPair {
    public_key: Vec<u8>,
    secret_key: Vec<u8>,
    #[zeroize(skip)]
    algorithm: KeyExchangeAlgorithm,
}

#[derive(Debug, Clone)]
pub struct PQCManager {
    signature_keypair: KeyPair,
    key_exchange_keypair: KeyExchangeKeyPair,
    /// Stored previous signature keypairs for rotation/algorithm upgrades
    signature_key_backups: Vec<KeyPair>,
    /// Stored previous key exchange keypairs
    key_exchange_key_backups: Vec<KeyExchangeKeyPair>,
}

impl PQCManager {
    pub fn new() -> Result<Self, PQCError> {
        Self::new_with_algorithms(
            SignatureAlgorithm::Dilithium3,
            KeyExchangeAlgorithm::Kyber1024,
        )
    }

    pub fn new_with_algorithms(
        sig_alg: SignatureAlgorithm,
        kex_alg: KeyExchangeAlgorithm,
    ) -> Result<Self, PQCError> {
        let signature_keypair = generate_signature_keypair(&sig_alg)?;
        let key_exchange_keypair = generate_key_exchange_keypair(&kex_alg)?;

        Ok(Self {
            signature_keypair,
            key_exchange_keypair,
            signature_key_backups: Vec::new(),
            key_exchange_key_backups: Vec::new(),
        })
    }

    /// Construct a manager from explicit key material.
    pub fn from_keypair(
        signature_keypair: KeyPair,
        key_exchange_keypair: KeyExchangeKeyPair,
    ) -> Self {
        Self {
            signature_keypair,
            key_exchange_keypair,
            signature_key_backups: Vec::new(),
            key_exchange_key_backups: Vec::new(),
        }
    }

    /// Load explicit private key storage and verify both key pairs.
    /// Incomplete files fail without replacing the existing identity.
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, PQCError> {
        let data = zeroize::Zeroizing::new(
            fs::read_to_string(&path)
                .map_err(|_| PQCError::InvalidKey("Failed to read key file".into()))?,
        );
        let stored: StoredKeys = serde_json::from_str(&data).map_err(|_| {
            PQCError::InvalidKey(
                "Key file is incomplete or invalid; restore the original keys".into(),
            )
        })?;
        let manager = Self::from_keypair(
            KeyPair {
                public_key: stored.signature_keypair.public_key.clone(),
                secret_key: stored.signature_keypair.secret_key.clone(),
                algorithm: stored.signature_keypair.algorithm.clone(),
            },
            KeyExchangeKeyPair {
                public_key: stored.key_exchange_keypair.public_key.clone(),
                secret_key: stored.key_exchange_keypair.secret_key.clone(),
                algorithm: stored.key_exchange_keypair.algorithm.clone(),
            },
        );
        manager.validate_keys()?;
        let (expected, ciphertext) = manager.encapsulate(manager.get_key_exchange_public_key())?;
        let expected = zeroize::Zeroizing::new(expected);
        let actual = zeroize::Zeroizing::new(manager.decapsulate(&ciphertext)?);
        if *actual != *expected {
            return Err(PQCError::InvalidKey(
                "Key-exchange pair does not match".into(),
            ));
        }
        Ok(manager)
    }

    /// Save a private plaintext key file. Restrict its parent directory as well.
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<(), PQCError> {
        self.write_key_file(path.as_ref(), false)
    }

    fn write_key_file(&self, path: &Path, only_if_absent: bool) -> Result<(), PQCError> {
        use std::io::Write;
        let stored = StoredKeys {
            signature_keypair: StoredSignatureKeyPair {
                public_key: self.signature_keypair.public_key.clone(),
                secret_key: self.signature_keypair.secret_key.clone(),
                algorithm: self.signature_keypair.algorithm.clone(),
            },
            key_exchange_keypair: StoredKeyExchangeKeyPair {
                public_key: self.key_exchange_keypair.public_key.clone(),
                secret_key: self.key_exchange_keypair.secret_key.clone(),
                algorithm: self.key_exchange_keypair.algorithm.clone(),
            },
        };
        let json = zeroize::Zeroizing::new(
            serde_json::to_vec_pretty(&stored)
                .map_err(|_| PQCError::InvalidKey("Failed to serialize key file".into()))?,
        );
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut file =
            tempfile::NamedTempFile::new_in(parent).map_err(|_| PQCError::KeyGenerationFailed)?;
        file.write_all(&json)
            .map_err(|_| PQCError::KeyGenerationFailed)?;
        file.as_file()
            .sync_all()
            .map_err(|_| PQCError::KeyGenerationFailed)?;
        if only_if_absent {
            file.persist_noclobber(path)
                .map_err(|_| PQCError::KeyGenerationFailed)?;
        } else {
            file.persist(path)
                .map_err(|_| PQCError::KeyGenerationFailed)?;
        }
        Ok(())
    }

    /// Load an existing identity, or create one only when the path is absent.
    pub fn load_or_generate<P: AsRef<Path>>(path: P) -> Result<Self, PQCError> {
        match fs::symlink_metadata(path.as_ref()) {
            Ok(_) => Self::load_from_file(&path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let manager = Self::new()?;
                manager.write_key_file(path.as_ref(), true)?;
                Ok(manager)
            }
            Err(_) => Err(PQCError::InvalidKey("Cannot inspect key file".into())),
        }
    }

    /// Validate key pairs by signing and verifying a test message
    pub fn validate_keys(&self) -> Result<(), PQCError> {
        let msg = b"dytallix_key_validation";
        let sig = self.sign(msg)?;
        if self.verify(msg, &sig, self.get_signature_public_key())? {
            Ok(())
        } else {
            Err(PQCError::VerificationFailed)
        }
    }

    /// Sign a message with the current active signature algorithm
    ///
    /// SECURITY CONSIDERATIONS:
    /// - Uses deterministic signing where possible to prevent nonce reuse attacks
    /// - Secret key operations may be vulnerable to side-channel attacks
    /// - Memory containing signature computation should be zeroized after use
    ///
    /// POTENTIAL ATTACK VECTORS:
    /// - Timing attacks during secret key operations (especially Falcon)
    /// - Memory analysis attacks if intermediate values are not cleared
    /// - Fault injection attacks during signature generation
    pub fn sign(&self, message: &[u8]) -> Result<Signature, PQCError> {
        match self.signature_keypair.algorithm {
            SignatureAlgorithm::Dilithium3 => {
                let sk = dilithium3::SecretKey::from_bytes(&self.signature_keypair.secret_key)
                    .map_err(|_| {
                        PQCError::InvalidKey("Invalid Dilithium3 secret key".to_string())
                    })?;
                let signed_message = dilithium3::sign(message, &sk);
                Ok(Signature {
                    data: signed_message.as_bytes().to_vec(),
                    algorithm: SignatureAlgorithm::Dilithium3,
                })
            }
            SignatureAlgorithm::Dilithium5 => {
                let sk = dilithium5::SecretKey::from_bytes(&self.signature_keypair.secret_key)
                    .map_err(|_| {
                        PQCError::InvalidKey("Invalid Dilithium5 secret key".to_string())
                    })?;

                let signed_message = dilithium5::sign(message, &sk);

                Ok(Signature {
                    data: signed_message.as_bytes().to_vec(),
                    algorithm: SignatureAlgorithm::Dilithium5,
                })
            }
            SignatureAlgorithm::Falcon1024 => {
                let sk = falcon1024::SecretKey::from_bytes(&self.signature_keypair.secret_key)
                    .map_err(|_| {
                        PQCError::InvalidKey("Invalid Falcon1024 secret key".to_string())
                    })?;

                let signed_message = falcon1024::sign(message, &sk);

                Ok(Signature {
                    data: signed_message.as_bytes().to_vec(),
                    algorithm: SignatureAlgorithm::Falcon1024,
                })
            }
            SignatureAlgorithm::SphincsSha256128s => {
                let sk = sphincssha2128ssimple::SecretKey::from_bytes(
                    &self.signature_keypair.secret_key,
                )
                .map_err(|_| PQCError::InvalidKey("Invalid SPHINCS+ secret key".to_string()))?;

                let signed_message = sphincssha2128ssimple::sign(message, &sk);

                Ok(Signature {
                    data: signed_message.as_bytes().to_vec(),
                    algorithm: SignatureAlgorithm::SphincsSha256128s,
                })
            }
        }
    }

    /// Verify a signature against a message and public key
    ///
    /// SECURITY CONSIDERATIONS:
    /// - Signature verification should be constant-time to prevent timing attacks
    /// - Invalid signatures must be rejected without leaking information about failure reason
    /// - Public key validation should be performed to prevent malformed key attacks
    ///
    /// POTENTIAL ATTACK VECTORS:
    /// - Timing side-channel attacks through verification time differences
    /// - Invalid curve point attacks with malformed public keys
    /// - Signature malleability if not properly validated
    ///
    /// CRITICAL: This function must return constant time regardless of signature validity
    pub fn verify(
        &self,
        message: &[u8],
        signature: &Signature,
        public_key: &[u8],
    ) -> Result<bool, PQCError> {
        match signature.algorithm {
            SignatureAlgorithm::Dilithium3 => {
                let pk = dilithium3::PublicKey::from_bytes(public_key).map_err(|_| {
                    PQCError::InvalidKey("Invalid Dilithium3 public key".to_string())
                })?;
                let signed_message = dilithium3::SignedMessage::from_bytes(&signature.data)
                    .map_err(|_| {
                        PQCError::InvalidSignature("Invalid Dilithium3 signature".to_string())
                    })?;
                match dilithium3::open(&signed_message, &pk) {
                    Ok(verified_message) => Ok(verified_message == message),
                    Err(_) => Ok(false),
                }
            }
            SignatureAlgorithm::Dilithium5 => {
                let pk = dilithium5::PublicKey::from_bytes(public_key).map_err(|_| {
                    PQCError::InvalidKey("Invalid Dilithium5 public key".to_string())
                })?;

                let signed_message = dilithium5::SignedMessage::from_bytes(&signature.data)
                    .map_err(|_| {
                        PQCError::InvalidSignature("Invalid Dilithium5 signature".to_string())
                    })?;

                match dilithium5::open(&signed_message, &pk) {
                    Ok(verified_message) => Ok(verified_message == message),
                    Err(_) => Ok(false),
                }
            }
            SignatureAlgorithm::Falcon1024 => {
                let pk = falcon1024::PublicKey::from_bytes(public_key).map_err(|_| {
                    PQCError::InvalidKey("Invalid Falcon1024 public key".to_string())
                })?;

                let signed_message = falcon1024::SignedMessage::from_bytes(&signature.data)
                    .map_err(|_| {
                        PQCError::InvalidSignature("Invalid Falcon1024 signature".to_string())
                    })?;

                match falcon1024::open(&signed_message, &pk) {
                    Ok(verified_message) => Ok(verified_message == message),
                    Err(_) => Ok(false),
                }
            }
            SignatureAlgorithm::SphincsSha256128s => {
                let pk = sphincssha2128ssimple::PublicKey::from_bytes(public_key)
                    .map_err(|_| PQCError::InvalidKey("Invalid SPHINCS+ public key".to_string()))?;

                let signed_message =
                    sphincssha2128ssimple::SignedMessage::from_bytes(&signature.data).map_err(
                        |_| PQCError::InvalidSignature("Invalid SPHINCS+ signature".to_string()),
                    )?;

                match sphincssha2128ssimple::open(&signed_message, &pk) {
                    Ok(verified_message) => Ok(verified_message == message),
                    Err(_) => Ok(false),
                }
            }
        }
    }

    /// Verify a signature using any known key (active or backups)
    pub fn verify_with_known_keys(
        &self,
        message: &[u8],
        signature: &Signature,
    ) -> Result<bool, PQCError> {
        // Try active key first (collapse nested if)
        if self.signature_keypair.algorithm == signature.algorithm
            && self.verify(message, signature, &self.signature_keypair.public_key)?
        {
            return Ok(true);
        }

        // Try backups in reverse (newest first) (collapse nested if)
        for kp in self.signature_key_backups.iter().rev() {
            if kp.algorithm == signature.algorithm
                && self.verify(message, signature, &kp.public_key)?
            {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Return (shared secret, ciphertext), preserving the existing tuple order.
    pub fn encapsulate(&self, peer_public_key: &[u8]) -> Result<(Vec<u8>, Vec<u8>), PQCError> {
        match self.key_exchange_keypair.algorithm {
            KeyExchangeAlgorithm::Kyber1024 => {
                let pk = KemPublicKey::from_bytes(peer_public_key).map_err(|_| {
                    PQCError::InvalidKey("Invalid Kyber1024 public key".to_string())
                })?;

                let (ciphertext, shared_secret) = kyber1024::encapsulate(&pk);

                // Convert to byte vectors using the specific type methods
                Ok((
                    ciphertext.as_bytes().to_vec(),
                    shared_secret.as_bytes().to_vec(),
                ))
            }
        }
    }

    pub fn decapsulate(&self, ciphertext: &[u8]) -> Result<Vec<u8>, PQCError> {
        match self.key_exchange_keypair.algorithm {
            KeyExchangeAlgorithm::Kyber1024 => {
                let sk = kyber1024::SecretKey::from_bytes(&self.key_exchange_keypair.secret_key)
                    .map_err(|_| {
                        PQCError::InvalidKey("Invalid Kyber1024 secret key".to_string())
                    })?;

                let ct = kyber1024::Ciphertext::from_bytes(ciphertext).map_err(|_| {
                    PQCError::InvalidKey("Invalid Kyber1024 ciphertext".to_string())
                })?;

                let shared_secret = kyber1024::decapsulate(&ct, &sk);

                Ok(shared_secret.as_bytes().to_vec())
            }
        }
    }

    pub fn get_signature_public_key(&self) -> &[u8] {
        &self.signature_keypair.public_key
    }

    #[allow(dead_code)]
    pub fn get_key_exchange_public_key(&self) -> &[u8] {
        &self.key_exchange_keypair.public_key
    }

    pub fn get_key_exchange_keypair(&self) -> KeyExchangeKeyPair {
        self.key_exchange_keypair.clone()
    }

    pub fn get_signature_algorithm(&self) -> &SignatureAlgorithm {
        &self.signature_keypair.algorithm
    }

    #[allow(dead_code)]
    pub fn get_key_exchange_algorithm(&self) -> &KeyExchangeAlgorithm {
        &self.key_exchange_keypair.algorithm
    }

    // Crypto-agility: Switch signature algorithms
    pub fn switch_signature_algorithm(
        &mut self,
        algorithm: SignatureAlgorithm,
    ) -> Result<(), PQCError> {
        // Preserve current keypair for backward compatibility
        self.signature_key_backups
            .push(self.signature_keypair.clone());
        self.signature_keypair = generate_signature_keypair(&algorithm)?;
        log::info!("Switched to signature algorithm: {algorithm:?}");
        Ok(())
    }

    // Crypto-agility: Switch key exchange algorithms
    pub fn switch_key_exchange_algorithm(
        &mut self,
        algorithm: KeyExchangeAlgorithm,
    ) -> Result<(), PQCError> {
        // Preserve current keypair
        self.key_exchange_key_backups
            .push(self.key_exchange_keypair.clone());
        self.key_exchange_keypair = generate_key_exchange_keypair(&algorithm)?;
        log::info!("Switched to key exchange algorithm: {algorithm:?}");
        Ok(())
    }

    /// Rotate the active signature key, keeping only the old *public* key as a
    /// backup so previously issued signatures still verify.
    ///
    /// The previous secret key is NOT retained. A rotated signing key is never
    /// used to sign again, and `verify_with_backups` only needs the public key,
    /// so keeping old secret-key material — even encrypted alongside its key in
    /// the same process — only adds attack surface (memory-dump / on-disk backup
    /// recovery) with no functional benefit. We therefore store the backup with
    /// an empty secret key and rely on `KeyPair`'s `ZeroizeOnDrop` to wipe the
    /// live secret material when the active keypair is replaced.
    ///
    /// This replaces the previous backup scheme, which XOR'd the old secret key
    /// with a constant-derived keystream (`blake3::hash(b"dytallix-key-rotation")`)
    /// — a deterministic, unauthenticated, trivially reversible transformation
    /// that left backups effectively in plaintext.
    pub fn rotate_signature_key(&mut self) -> Result<(), PQCError> {
        let algorithm = self.signature_keypair.algorithm.clone();
        // Generate the replacement first so a failure leaves state unchanged.
        let new_keypair = generate_signature_keypair(&algorithm)?;

        // Retain only the public half of the outgoing key.
        let backup = KeyPair {
            public_key: self.signature_keypair.public_key.clone(),
            secret_key: Vec::new(),
            algorithm: algorithm.clone(),
        };
        self.signature_key_backups.push(backup);

        // Overwriting the active keypair drops the old one, zeroizing its secret.
        self.signature_keypair = new_keypair;
        log::info!(
            "Rotated signature key for algorithm: {algorithm:?} (old secret key zeroized & discarded)"
        );
        Ok(())
    }

    /// Rotate the active key exchange key and store previous key
    pub fn rotate_key_exchange_key(&mut self) -> Result<(), PQCError> {
        let algorithm = self.key_exchange_keypair.algorithm.clone();
        self.key_exchange_key_backups
            .push(self.key_exchange_keypair.clone());
        self.key_exchange_keypair = generate_key_exchange_keypair(&algorithm)?;
        log::info!("Rotated key exchange key for algorithm: {algorithm:?}");
        Ok(())
    }

    /// Backup all keys to a JSON file
    pub fn backup_to_file<P: AsRef<std::path::Path>>(&self, path: P) -> Result<(), PQCError> {
        #[derive(Serialize, Deserialize)]
        struct Backup {
            active_signature: KeyPair,
            active_kex: KeyExchangeKeyPair,
            signature_backups: Vec<KeyPair>,
            kex_backups: Vec<KeyExchangeKeyPair>,
        }

        let backup = Backup {
            active_signature: self.signature_keypair.clone(),
            active_kex: self.key_exchange_keypair.clone(),
            signature_backups: self.signature_key_backups.clone(),
            kex_backups: self.key_exchange_key_backups.clone(),
        };

        let data = serde_json::to_vec_pretty(&backup)
            .map_err(|_| PQCError::InvalidKey("Failed to serialize backup".to_string()))?;
        std::fs::write(path, data).map_err(|_| PQCError::KeyGenerationFailed)?;
        Ok(())
    }

    /// Restore keys from a backup file
    pub fn restore_from_file<P: AsRef<std::path::Path>>(
        &mut self,
        path: P,
    ) -> Result<(), PQCError> {
        #[derive(Serialize, Deserialize)]
        struct Backup {
            active_signature: KeyPair,
            active_kex: KeyExchangeKeyPair,
            signature_backups: Vec<KeyPair>,
            kex_backups: Vec<KeyExchangeKeyPair>,
        }

        let data = std::fs::read(path)
            .map_err(|_| PQCError::InvalidKey("Failed to read backup file".to_string()))?;
        let backup: Backup = serde_json::from_slice(&data)
            .map_err(|_| PQCError::InvalidKey("Failed to parse backup".to_string()))?;

        self.signature_keypair = backup.active_signature;
        self.key_exchange_keypair = backup.active_kex;
        self.signature_key_backups = backup.signature_backups;
        self.key_exchange_key_backups = backup.kex_backups;
        Ok(())
    }

    /// Generate keypair for the specified algorithm
    pub fn generate_keypair(&self, algorithm: &SignatureAlgorithm) -> Result<KeyPair, PQCError> {
        match algorithm {
            SignatureAlgorithm::Dilithium3 => {
                let (pk, sk) = dilithium3::keypair();
                Ok(KeyPair {
                    public_key: pqcrypto_traits::sign::PublicKey::as_bytes(&pk).to_vec(),
                    secret_key: pqcrypto_traits::sign::SecretKey::as_bytes(&sk).to_vec(),
                    algorithm: algorithm.clone(),
                })
            }
            SignatureAlgorithm::Dilithium5 => {
                let (pk, sk) = dilithium5::keypair();
                Ok(KeyPair {
                    public_key: pqcrypto_traits::sign::PublicKey::as_bytes(&pk).to_vec(),
                    secret_key: pqcrypto_traits::sign::SecretKey::as_bytes(&sk).to_vec(),
                    algorithm: algorithm.clone(),
                })
            }
            SignatureAlgorithm::Falcon1024 => {
                let (pk, sk) = falcon1024::keypair();
                Ok(KeyPair {
                    public_key: pqcrypto_traits::sign::PublicKey::as_bytes(&pk).to_vec(),
                    secret_key: pqcrypto_traits::sign::SecretKey::as_bytes(&sk).to_vec(),
                    algorithm: algorithm.clone(),
                })
            }
            SignatureAlgorithm::SphincsSha256128s => {
                let (pk, sk) = sphincssha2128ssimple::keypair();
                Ok(KeyPair {
                    public_key: pqcrypto_traits::sign::PublicKey::as_bytes(&pk).to_vec(),
                    secret_key: pqcrypto_traits::sign::SecretKey::as_bytes(&sk).to_vec(),
                    algorithm: algorithm.clone(),
                })
            }
        }
    }

    /// Sign a message with the specified algorithm
    pub fn sign_with_algorithm(
        &self,
        message: &[u8],
        secret_key: &[u8],
        algorithm: &SignatureAlgorithm,
    ) -> Result<Vec<u8>, PQCError> {
        match algorithm {
            SignatureAlgorithm::Dilithium3 => {
                let sk = dilithium3::SecretKey::from_bytes(secret_key).map_err(|_| {
                    PQCError::InvalidKey("Invalid Dilithium3 secret key".to_string())
                })?;
                let signature = dilithium3::sign(message, &sk);
                Ok(signature.as_bytes().to_vec())
            }
            SignatureAlgorithm::Dilithium5 => {
                let sk = dilithium5::SecretKey::from_bytes(secret_key).map_err(|_| {
                    PQCError::InvalidKey("Invalid Dilithium5 secret key".to_string())
                })?;
                let signature = dilithium5::sign(message, &sk);
                Ok(signature.as_bytes().to_vec())
            }
            SignatureAlgorithm::Falcon1024 => {
                let sk = falcon1024::SecretKey::from_bytes(secret_key).map_err(|_| {
                    PQCError::InvalidKey("Invalid Falcon1024 secret key".to_string())
                })?;
                let signature = falcon1024::sign(message, &sk);
                Ok(signature.as_bytes().to_vec())
            }
            SignatureAlgorithm::SphincsSha256128s => {
                let sk = sphincssha2128ssimple::SecretKey::from_bytes(secret_key)
                    .map_err(|_| PQCError::InvalidKey("Invalid SPHINCS+ secret key".to_string()))?;
                let signature = sphincssha2128ssimple::sign(message, &sk);
                Ok(signature.as_bytes().to_vec())
            }
        }
    }

    /// Verify signature with the specified algorithm
    pub fn verify_with_algorithm(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key: &[u8],
        algorithm: &SignatureAlgorithm,
    ) -> Result<bool, PQCError> {
        match algorithm {
            SignatureAlgorithm::Dilithium3 => {
                let pk = dilithium3::PublicKey::from_bytes(public_key).map_err(|_| {
                    PQCError::InvalidKey("Invalid Dilithium3 public key".to_string())
                })?;
                let sig = dilithium3::SignedMessage::from_bytes(signature).map_err(|_| {
                    PQCError::InvalidSignature("Invalid Dilithium3 signature".to_string())
                })?;
                match dilithium3::open(&sig, &pk) {
                    Ok(verified_message) => Ok(verified_message == message),
                    Err(_) => Ok(false),
                }
            }
            SignatureAlgorithm::Dilithium5 => {
                let pk = dilithium5::PublicKey::from_bytes(public_key).map_err(|_| {
                    PQCError::InvalidKey("Invalid Dilithium5 public key".to_string())
                })?;
                let sig = dilithium5::SignedMessage::from_bytes(signature).map_err(|_| {
                    PQCError::InvalidSignature("Invalid Dilithium5 signature".to_string())
                })?;
                match dilithium5::open(&sig, &pk) {
                    Ok(verified_message) => Ok(verified_message == message),
                    Err(_) => Ok(false),
                }
            }
            SignatureAlgorithm::Falcon1024 => {
                let pk = falcon1024::PublicKey::from_bytes(public_key).map_err(|_| {
                    PQCError::InvalidKey("Invalid Falcon1024 public key".to_string())
                })?;
                let sig = falcon1024::SignedMessage::from_bytes(signature).map_err(|_| {
                    PQCError::InvalidSignature("Invalid Falcon1024 signature".to_string())
                })?;
                match falcon1024::open(&sig, &pk) {
                    Ok(verified_message) => Ok(verified_message == message),
                    Err(_) => Ok(false),
                }
            }
            SignatureAlgorithm::SphincsSha256128s => {
                let pk = sphincssha2128ssimple::PublicKey::from_bytes(public_key)
                    .map_err(|_| PQCError::InvalidKey("Invalid SPHINCS+ public key".to_string()))?;
                let sig =
                    sphincssha2128ssimple::SignedMessage::from_bytes(signature).map_err(|_| {
                        PQCError::InvalidSignature("Invalid SPHINCS+ signature".to_string())
                    })?;
                match sphincssha2128ssimple::open(&sig, &pk) {
                    Ok(verified_message) => Ok(verified_message == message),
                    Err(_) => Ok(false),
                }
            }
        }
    }
}

// Helper functions for key generation
fn generate_signature_keypair(algorithm: &SignatureAlgorithm) -> Result<KeyPair, PQCError> {
    match algorithm {
        SignatureAlgorithm::Dilithium3 => {
            let (pk, sk) = dilithium3::keypair();
            Ok(KeyPair {
                public_key: pqcrypto_traits::sign::PublicKey::as_bytes(&pk).to_vec(),
                secret_key: pqcrypto_traits::sign::SecretKey::as_bytes(&sk).to_vec(),
                algorithm: algorithm.clone(),
            })
        }
        SignatureAlgorithm::Dilithium5 => {
            let (pk, sk) = dilithium5::keypair();
            Ok(KeyPair {
                public_key: pqcrypto_traits::sign::PublicKey::as_bytes(&pk).to_vec(),
                secret_key: pqcrypto_traits::sign::SecretKey::as_bytes(&sk).to_vec(),
                algorithm: algorithm.clone(),
            })
        }
        SignatureAlgorithm::Falcon1024 => {
            let (pk, sk) = falcon1024::keypair();
            Ok(KeyPair {
                public_key: pqcrypto_traits::sign::PublicKey::as_bytes(&pk).to_vec(),
                secret_key: pqcrypto_traits::sign::SecretKey::as_bytes(&sk).to_vec(),
                algorithm: algorithm.clone(),
            })
        }
        SignatureAlgorithm::SphincsSha256128s => {
            let (pk, sk) = sphincssha2128ssimple::keypair();
            Ok(KeyPair {
                public_key: pqcrypto_traits::sign::PublicKey::as_bytes(&pk).to_vec(),
                secret_key: pqcrypto_traits::sign::SecretKey::as_bytes(&sk).to_vec(),
                algorithm: algorithm.clone(),
            })
        }
    }
}

fn generate_key_exchange_keypair(
    algorithm: &KeyExchangeAlgorithm,
) -> Result<KeyExchangeKeyPair, PQCError> {
    match algorithm {
        KeyExchangeAlgorithm::Kyber1024 => {
            let (pk, sk) = kyber1024::keypair();
            Ok(KeyExchangeKeyPair {
                public_key: pqcrypto_traits::kem::PublicKey::as_bytes(&pk).to_vec(),
                secret_key: pqcrypto_traits::kem::SecretKey::as_bytes(&sk).to_vec(),
                algorithm: algorithm.clone(),
            })
        }
    }
}

/// Crypto-agility framework for seamless algorithm upgrades
#[derive(Debug, Clone)]
pub struct CryptoAgilityManager {
    preferred_algorithm: SignatureAlgorithm,
    supported_algorithms: Vec<SignatureAlgorithm>,
    migration_schedule: Option<AlgorithmMigration>,
}

#[derive(Debug, Clone)]
pub struct AlgorithmMigration {
    from_algorithm: SignatureAlgorithm,
    to_algorithm: SignatureAlgorithm,
    migration_deadline: chrono::DateTime<chrono::Utc>,
    deprecation_warning_period: chrono::Duration,
}

impl CryptoAgilityManager {
    pub fn new() -> Self {
        Self {
            preferred_algorithm: SignatureAlgorithm::Dilithium3,
            supported_algorithms: vec![
                SignatureAlgorithm::Dilithium3,
                SignatureAlgorithm::Dilithium5,
                SignatureAlgorithm::Falcon1024,
                SignatureAlgorithm::SphincsSha256128s,
            ],
            migration_schedule: None,
        }
    }

    /// Check if an algorithm is supported
    pub fn is_algorithm_supported(&self, algorithm: &SignatureAlgorithm) -> bool {
        self.supported_algorithms.contains(algorithm)
    }

    /// Get the preferred algorithm for new operations
    pub fn get_preferred_algorithm(&self) -> SignatureAlgorithm {
        self.preferred_algorithm.clone()
    }

    /// Schedule an algorithm migration
    pub fn schedule_migration(
        &mut self,
        from: SignatureAlgorithm,
        to: SignatureAlgorithm,
        deadline: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), PQCError> {
        if !self.is_algorithm_supported(&from) || !self.is_algorithm_supported(&to) {
            return Err(PQCError::UnsupportedAlgorithm(
                "Migration algorithms not supported".to_string(),
            ));
        }

        self.migration_schedule = Some(AlgorithmMigration {
            from_algorithm: from,
            to_algorithm: to,
            migration_deadline: deadline,
            deprecation_warning_period: chrono::Duration::days(90),
        });

        Ok(())
    }

    /// Apply scheduled migration if deadline has passed (now also uses all migration fields to avoid unused-field warnings)
    pub fn apply_migration(&mut self) {
        if let Some(migration) = &self.migration_schedule {
            // Use deprecation_warning_period & from_algorithm for pre-migration logging
            let now = chrono::Utc::now();
            if now + migration.deprecation_warning_period >= migration.migration_deadline {
                log::warn!(
                    "Algorithm {:?} will be deprecated; migrating to {:?} by {:?}",
                    migration.from_algorithm,
                    migration.to_algorithm,
                    migration.migration_deadline
                );
            } else {
                log::debug!(
                    "Migration scheduled from {:?} to {:?} (deadline {:?})",
                    migration.from_algorithm,
                    migration.to_algorithm,
                    migration.migration_deadline
                );
            }
            if now >= migration.migration_deadline {
                self.preferred_algorithm = migration.to_algorithm.clone();
                self.migration_schedule = None;
                log::info!(
                    "Applied algorithm migration to {:?}",
                    self.preferred_algorithm
                );
            }
        }
    }
}

impl Default for CryptoAgilityManager {
    fn default() -> Self {
        Self::new()
    }
}

// Bridge-specific PQC functionality
pub mod bridge;

// Performance benchmarking module
pub mod performance;

pub use bridge::{
    AddressFormat, BridgePQCManager, BridgeSignature, ChainConfig, CrossChainPayload,
    HashAlgorithm, MultiSigValidationResult, SignatureFormat,
};

pub use performance::{
    run_pqc_performance_benchmarks, GasCostEstimation, PQCBenchmarkResults,
    PQCPerformanceBenchmark, PerformanceAnalysis,
};

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_algorithm_switch_and_legacy_verify() {
        let mut manager = PQCManager::new().unwrap();
        let msg1 = b"legacy";
        let sig1 = manager.sign(msg1).unwrap();

        manager
            .switch_signature_algorithm(SignatureAlgorithm::Falcon1024)
            .unwrap();
        let msg2 = b"new";
        let sig2 = manager.sign(msg2).unwrap();

        assert!(manager.verify_with_known_keys(msg1, &sig1).unwrap());
        assert!(manager.verify_with_known_keys(msg2, &sig2).unwrap());
    }

    #[test]
    fn test_key_rotation_and_backup_restore() {
        let mut manager = PQCManager::new().unwrap();
        let msg = b"rotate";
        let sig_old = manager.sign(msg).unwrap();
        manager.rotate_signature_key().unwrap();
        let sig_new = manager.sign(msg).unwrap();
        assert!(manager.verify_with_known_keys(msg, &sig_old).unwrap());
        assert!(manager.verify_with_known_keys(msg, &sig_new).unwrap());

        let file = NamedTempFile::new().unwrap();
        manager.backup_to_file(file.path()).unwrap();

        let mut restored = PQCManager::new().unwrap();
        restored.restore_from_file(file.path()).unwrap();
        assert!(restored.verify_with_known_keys(msg, &sig_old).unwrap());
        assert!(restored.verify_with_known_keys(msg, &sig_new).unwrap());
    }

    #[test]
    fn test_rotation_discards_old_secret_key() {
        let mut manager = PQCManager::new().unwrap();
        let msg = b"rotate-secret";
        let sig_old = manager.sign(msg).unwrap();
        manager.rotate_signature_key().unwrap();

        // The backup must keep the public key (so old signatures still verify)
        // but must NOT retain any secret-key material.
        assert_eq!(manager.signature_key_backups.len(), 1);
        let backup = &manager.signature_key_backups[0];
        assert!(
            backup.secret_key.is_empty(),
            "rotated secret key must not be retained"
        );
        assert!(!backup.public_key.is_empty());
        assert!(manager.verify_with_known_keys(msg, &sig_old).unwrap());
    }

    #[test]
    fn test_load_or_generate_and_validate() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("keys.json");
        let path = path.as_path();

        let manager = PQCManager::load_or_generate(path).expect("create manager");
        manager.validate_keys().expect("validate keys");

        let manager2 = PQCManager::load_or_generate(path).expect("load manager");
        manager2.validate_keys().expect("validate keys");
        assert_eq!(
            manager.get_signature_public_key(),
            manager2.get_signature_public_key()
        );
        assert_eq!(
            manager.get_key_exchange_public_key(),
            manager2.get_key_exchange_public_key()
        );
        let (expected, ciphertext) = manager
            .encapsulate(manager2.get_key_exchange_public_key())
            .unwrap();
        assert_eq!(manager2.decapsulate(&ciphertext).unwrap(), expected);

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    #[ignore = "requires an explicit PQClean metadata fixture; this is not a known-answer test"]
    fn test_dilithium3_metadata_declared_hash_matches() {
        let meta = std::env::var_os("DYT_DILITHIUM3_META")
            .expect("set DYT_DILITHIUM3_META to the reviewed META.yml fixture");
        let contents = std::fs::read_to_string(meta).expect("read META.yml");
        let mut found: Option<String> = None;
        for line in contents.lines() {
            let t = line.trim();
            if t.starts_with("nistkat-sha256:") {
                let val = t.splitn(2, ':').nth(1).unwrap_or("").trim();
                found = Some(val.to_lowercase());
                break;
            }
        }
        let hash = found.expect("nistkat-sha256 not found in META.yml");
        assert_eq!(
            hash,
            "4ae9921a12524a31599550f2b4e57b6db1b133987c348f07e12d20fc4aa426d5".to_string()
        );
    }
}
