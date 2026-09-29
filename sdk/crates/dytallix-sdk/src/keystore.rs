//! File-backed keystore support for Dytallix keypairs.
//!
//! Version 2 (E04 gap 16, P01 28 September 2026) encrypts each private key
//! at rest: an Argon2id key from the passphrase (parameters and salt in the
//! file), AES-256-GCM (NIST SP 800-38D) with a random 96-bit nonce per
//! entry, and the entry's public metadata as associated data, so editing a
//! name, address or public key fails decryption. Names, addresses and public
//! keys stay readable. Both are symmetric: a 256-bit key keeps its strength
//! against quantum search; no public-key algorithm is involved.
//! Version 1 files (plaintext keys) still open and list; their keys are
//! refused until `migrate`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use dytallix_core::address::DAddr;
use dytallix_core::keypair::{DytallixKeypair, KeyScheme};
use zeroize::Zeroizing;

use crate::error::SdkError;
use crate::KeystoreEntry;

/// The keystore file format written by this version (interfaces v1).
/// Version 1 held private keys in plaintext; a file without a version is
/// version 1.
pub const KEYSTORE_VERSION: u32 = 2;
const PLAINTEXT_VERSION: u32 = 1;

/// Argon2id parameters written for new keystores: 64 MiB, three passes, one
/// lane (an RFC 9106 profile sized for a command-line tool). A file below
/// them is refused; the ceilings bound the work a file can demand.
pub const KDF_MEMORY_KIB: u32 = 64 * 1024;
pub const KDF_ITERATIONS: u32 = 3;
pub const KDF_PARALLELISM: u32 = 1;
const KDF_MEMORY_CEILING_KIB: u32 = 4 * 1024 * 1024;
const KDF_ITERATIONS_CEILING: u32 = 64;
const KDF_PARALLELISM_CEILING: u32 = 16;
const SALT_BYTES: usize = 16;
const DOMAIN: &[u8] = b"dytallix-keystore-v2\0";
const CHECK_PLAINTEXT: &[u8] = b"dytallix-keystore-v2 passphrase check";

/// File-backed keystore for named Dytallix keypairs.
#[derive(Clone)]
pub struct Keystore {
    path: PathBuf,
    format: Format,
    entries: Vec<Stored>,
    active_name: Option<String>,
}

impl std::fmt::Debug for Keystore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Keystore")
            .field("path", &self.path)
            .field("version", &self.version())
            .field("entries", &self.list())
            .field("active", &self.active_name)
            .finish()
    }
}

#[derive(Clone)]
enum Format {
    Plaintext,
    Encrypted {
        kdf: Kdf,
        check: Sealed,
        key: Option<Zeroizing<[u8; 32]>>,
    },
}

#[derive(Clone)]
struct Stored {
    meta: KeystoreEntry,
    secret: Secret,
}

#[derive(Clone)]
enum Secret {
    Plain(Zeroizing<Vec<u8>>),
    Sealed(Sealed),
}

impl Keystore {
    /// Creates a new empty version 2 keystore, encrypted under `passphrase`.
    /// Nothing is written until `save`.
    pub fn create(path: PathBuf, passphrase: &[u8]) -> Result<Self, SdkError> {
        Self::create_with(
            path,
            passphrase,
            Kdf::fresh(KDF_MEMORY_KIB, KDF_ITERATIONS)?,
        )
    }

    fn create_with(path: PathBuf, passphrase: &[u8], kdf: Kdf) -> Result<Self, SdkError> {
        ensure_parent_dir(&path)?;
        let key = kdf.derive(passphrase)?;
        let check = Sealed::seal(&key, CHECK_PLAINTEXT, &check_aad())?;
        Ok(Self {
            path,
            format: Format::Encrypted {
                kdf,
                check,
                key: Some(key),
            },
            entries: Vec::new(),
            active_name: None,
        })
    }

    /// Opens an existing keystore from disk. An encrypted keystore opens
    /// locked: its metadata is readable, its keys need `unlock`.
    pub fn open(path: PathBuf) -> Result<Self, SdkError> {
        if !path.exists() {
            return Err(SdkError::KeystoreNotFound(path));
        }
        let contents = fs::read_to_string(&path)?;
        let value: serde_json::Value = serde_json::from_str(&contents)
            .map_err(|err| SdkError::KeystoreCorrupt(err.to_string()))?;
        let version = match value.get("version") {
            None => PLAINTEXT_VERSION,
            Some(version) => version
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(|| SdkError::KeystoreCorrupt("invalid keystore version".into()))?,
        };
        let corrupt = |err: serde_json::Error| SdkError::KeystoreCorrupt(err.to_string());
        match version {
            PLAINTEXT_VERSION => {
                let file: PlaintextFile = serde_json::from_value(value).map_err(corrupt)?;
                Ok(Self {
                    path,
                    format: Format::Plaintext,
                    entries: file
                        .entries
                        .into_iter()
                        .map(|entry| Stored {
                            secret: Secret::Plain(Zeroizing::new(entry.private_key)),
                            meta: KeystoreEntry {
                                name: entry.name,
                                address: entry.address,
                                public_key: entry.public_key,
                                scheme: entry.scheme,
                                created_at: entry.created_at,
                            },
                        })
                        .collect(),
                    active_name: file.active,
                })
            }
            KEYSTORE_VERSION => {
                let file: EncryptedFile = serde_json::from_value(value).map_err(corrupt)?;
                if file.cipher != CIPHER {
                    return Err(SdkError::KeystoreCorrupt(format!(
                        "unsupported keystore cipher {}",
                        file.cipher
                    )));
                }
                file.kdf.validate()?;
                Ok(Self {
                    path,
                    format: Format::Encrypted {
                        kdf: file.kdf,
                        check: file.check,
                        key: None,
                    },
                    entries: file
                        .entries
                        .into_iter()
                        .map(|entry| Stored {
                            meta: KeystoreEntry {
                                name: entry.name,
                                address: entry.address,
                                public_key: entry.public_key,
                                scheme: entry.scheme,
                                created_at: entry.created_at,
                            },
                            secret: Secret::Sealed(Sealed {
                                nonce: entry.nonce,
                                ciphertext: entry.ciphertext,
                            }),
                        })
                        .collect(),
                    active_name: file.active,
                })
            }
            other => Err(SdkError::KeystoreCorrupt(format!(
                "unsupported keystore version {other}"
            ))),
        }
    }

    /// The file format version: 1 (plaintext keys) or 2 (encrypted).
    pub fn version(&self) -> u32 {
        match self.format {
            Format::Plaintext => PLAINTEXT_VERSION,
            Format::Encrypted { .. } => KEYSTORE_VERSION,
        }
    }

    /// Whether an encrypted keystore has been unlocked.
    pub fn is_unlocked(&self) -> bool {
        matches!(&self.format, Format::Encrypted { key: Some(_), .. })
    }

    /// Unlocks an encrypted keystore. A wrong passphrase is refused.
    pub fn unlock(&mut self, passphrase: &[u8]) -> Result<(), SdkError> {
        let Format::Encrypted { kdf, check, key } = &mut self.format else {
            return Err(SdkError::KeystorePlaintext);
        };
        let derived = kdf.derive(passphrase)?;
        check
            .open(&derived, &check_aad())
            .ok()
            .filter(|plain| plain.as_slice() == CHECK_PLAINTEXT)
            .ok_or(SdkError::KeystorePassphrase)?;
        *key = Some(derived);
        Ok(())
    }

    /// Encrypts a version 1 keystore under `passphrase`, with a fresh salt.
    /// `save` then replaces the plaintext file; no plaintext copy is kept.
    pub fn migrate(&mut self, passphrase: &[u8]) -> Result<(), SdkError> {
        self.migrate_with(passphrase, Kdf::fresh(KDF_MEMORY_KIB, KDF_ITERATIONS)?)
    }

    fn migrate_with(&mut self, passphrase: &[u8], kdf: Kdf) -> Result<(), SdkError> {
        if !matches!(self.format, Format::Plaintext) {
            return Err(SdkError::KeystoreCorrupt(
                "keystore is already encrypted".into(),
            ));
        }
        let key = kdf.derive(passphrase)?;
        let mut entries = Vec::with_capacity(self.entries.len());
        for stored in &self.entries {
            let Secret::Plain(private_key) = &stored.secret else {
                unreachable!("a plaintext keystore holds plaintext keys");
            };
            entries.push(Stored {
                meta: stored.meta.clone(),
                secret: Secret::Sealed(Sealed::seal(&key, private_key, &entry_aad(&stored.meta)?)?),
            });
        }
        let check = Sealed::seal(&key, CHECK_PLAINTEXT, &check_aad())?;
        self.entries = entries;
        self.format = Format::Encrypted {
            kdf,
            check,
            key: Some(key),
        };
        Ok(())
    }

    /// Adds or replaces a named keypair entry. Requires an unlocked
    /// version 2 keystore.
    pub fn add_keypair(&mut self, keypair: &DytallixKeypair, name: &str) -> Result<(), SdkError> {
        keypair.scheme().require_production_operational()?;
        let meta = KeystoreEntry {
            name: name.to_owned(),
            address: DAddr::from_public_key(keypair.public_key())?,
            public_key: keypair.public_key().to_vec(),
            scheme: keypair.scheme(),
            created_at: unix_timestamp(),
        };
        let key = self.key()?;
        let secret = Secret::Sealed(Sealed::seal(
            key,
            keypair.private_key(),
            &entry_aad(&meta)?,
        )?);
        let stored = Stored { meta, secret };
        if let Some(existing) = self.entries.iter_mut().find(|item| item.meta.name == name) {
            *existing = stored;
        } else {
            self.entries.push(stored);
        }
        if self.active_name.is_none() {
            self.active_name = Some(name.to_owned());
        }
        Ok(())
    }

    /// Reconstructs a keypair from a named entry of an unlocked keystore.
    pub fn get_keypair(&self, name: &str) -> Result<DytallixKeypair, SdkError> {
        let key = self.key()?;
        self.keypair(name, key)
    }

    /// Reconstructs a keypair with `passphrase`, without unlocking the
    /// keystore.
    pub fn open_keypair(&self, name: &str, passphrase: &[u8]) -> Result<DytallixKeypair, SdkError> {
        let mut unlocked = self.clone();
        unlocked.unlock(passphrase)?;
        unlocked.get_keypair(name)
    }

    fn keypair(&self, name: &str, key: &[u8; 32]) -> Result<DytallixKeypair, SdkError> {
        let stored = self
            .entries
            .iter()
            .find(|item| item.meta.name == name)
            .ok_or_else(|| SdkError::KeystoreCorrupt(format!("missing keypair entry: {name}")))?;
        let entry = &stored.meta;
        entry.scheme.require_production_operational()?;
        let Secret::Sealed(sealed) = &stored.secret else {
            return Err(SdkError::KeystorePlaintext);
        };
        let private_key = sealed
            .open(key, &entry_aad(entry)?)
            .map_err(|_| SdkError::KeystoreCorrupt(format!("entry {name} fails authentication")))?;
        let keypair = DytallixKeypair::from_keypair(entry.scheme, &entry.public_key, &private_key)?;
        if DAddr::from_public_key(keypair.public_key())? != entry.address {
            return Err(SdkError::KeystoreCorrupt(format!(
                "address mismatch for entry: {name}"
            )));
        }
        Ok(keypair)
    }

    fn key(&self) -> Result<&[u8; 32], SdkError> {
        match &self.format {
            Format::Plaintext => Err(SdkError::KeystorePlaintext),
            Format::Encrypted { key: None, .. } => Err(SdkError::KeystoreLocked),
            Format::Encrypted { key: Some(key), .. } => Ok(key),
        }
    }

    /// Lists every entry's public metadata.
    pub fn list(&self) -> Vec<&KeystoreEntry> {
        self.entries.iter().map(|stored| &stored.meta).collect()
    }

    /// Removes a named keystore entry.
    pub fn remove(&mut self, name: &str) -> Result<(), SdkError> {
        let original_len = self.entries.len();
        self.entries.retain(|stored| stored.meta.name != name);
        if self.entries.len() == original_len {
            return Err(SdkError::KeystoreCorrupt(format!(
                "missing keypair entry: {name}"
            )));
        }
        if self.active_name.as_deref() == Some(name) {
            self.active_name = self.entries.first().map(|stored| stored.meta.name.clone());
        }
        Ok(())
    }

    /// Returns the active keystore entry, if one is set.
    pub fn active(&self) -> Option<&KeystoreEntry> {
        self.active_name.as_deref().and_then(|name| {
            self.entries
                .iter()
                .map(|stored| &stored.meta)
                .find(|entry| entry.name == name)
        })
    }

    /// Marks the named keystore entry as active.
    pub fn set_active(&mut self, name: &str) -> Result<(), SdkError> {
        if self.entries.iter().any(|stored| stored.meta.name == name) {
            self.active_name = Some(name.to_owned());
            Ok(())
        } else {
            Err(SdkError::KeystoreCorrupt(format!(
                "missing keypair entry: {name}"
            )))
        }
    }

    /// Saves the keystore owner-only, replacing the file atomically.
    pub fn save(&self) -> Result<(), SdkError> {
        ensure_parent_dir(&self.path)?;
        let serialize = |err: serde_json::Error| SdkError::Serialization(err.to_string());
        let json = match &self.format {
            Format::Plaintext => serde_json::to_string_pretty(&PlaintextFile {
                version: PLAINTEXT_VERSION,
                active: self.active_name.clone(),
                entries: self
                    .entries
                    .iter()
                    .map(|stored| {
                        let Secret::Plain(private_key) = &stored.secret else {
                            unreachable!("a plaintext keystore holds plaintext keys");
                        };
                        PlaintextEntry {
                            name: stored.meta.name.clone(),
                            address: stored.meta.address.clone(),
                            public_key: stored.meta.public_key.clone(),
                            private_key: private_key.to_vec(),
                            scheme: stored.meta.scheme,
                            created_at: stored.meta.created_at,
                        }
                    })
                    .collect(),
            }),
            Format::Encrypted { kdf, check, .. } => serde_json::to_string_pretty(&EncryptedFile {
                version: KEYSTORE_VERSION,
                cipher: CIPHER.into(),
                kdf: kdf.clone(),
                check: check.clone(),
                active: self.active_name.clone(),
                entries: self
                    .entries
                    .iter()
                    .map(|stored| {
                        let Secret::Sealed(sealed) = &stored.secret else {
                            unreachable!("an encrypted keystore holds sealed keys");
                        };
                        EncryptedEntry {
                            name: stored.meta.name.clone(),
                            address: stored.meta.address.clone(),
                            public_key: stored.meta.public_key.clone(),
                            scheme: stored.meta.scheme,
                            created_at: stored.meta.created_at,
                            nonce: sealed.nonce.clone(),
                            ciphertext: sealed.ciphertext.clone(),
                        }
                    })
                    .collect(),
            }),
        }
        .map_err(serialize)?;
        write_private(&self.path, json.as_bytes())
    }

    /// Returns the canonical default keystore path.
    pub fn default_path() -> PathBuf {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        home.join(".dytallix").join("keystore.json")
    }
}

const CIPHER: &str = "aes-256-gcm";
const NONCE_BYTES: usize = 12;

// Version 1 is read as leniently as before.
#[derive(serde::Serialize, serde::Deserialize)]
struct PlaintextFile {
    #[serde(default = "first_version")]
    version: u32,
    active: Option<String>,
    entries: Vec<PlaintextEntry>,
}
fn first_version() -> u32 {
    PLAINTEXT_VERSION
}
#[derive(serde::Serialize, serde::Deserialize)]
struct PlaintextEntry {
    name: String,
    address: DAddr,
    public_key: Vec<u8>,
    private_key: Vec<u8>,
    scheme: KeyScheme,
    created_at: u64,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct EncryptedFile {
    version: u32,
    cipher: String,
    kdf: Kdf,
    check: Sealed,
    active: Option<String>,
    entries: Vec<EncryptedEntry>,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct EncryptedEntry {
    name: String,
    address: DAddr,
    public_key: Vec<u8>,
    scheme: KeyScheme,
    created_at: u64,
    nonce: String,
    ciphertext: String,
}

/// Argon2id parameters and salt, stored in the file.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Kdf {
    algorithm: String,
    version: u32,
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
    salt: String,
}
impl Kdf {
    fn fresh(memory_kib: u32, iterations: u32) -> Result<Self, SdkError> {
        let mut salt = [0u8; SALT_BYTES];
        aes_gcm::aead::rand_core::RngCore::fill_bytes(&mut OsRng, &mut salt);
        Ok(Self {
            algorithm: "argon2id".into(),
            version: 0x13,
            memory_kib,
            iterations,
            parallelism: KDF_PARALLELISM,
            salt: B64.encode(salt),
        })
    }
    fn validate(&self) -> Result<(), SdkError> {
        let within = |value: u32, floor: u32, ceiling: u32| (floor..=ceiling).contains(&value);
        if self.algorithm != "argon2id"
            || self.version != 0x13
            || !within(self.memory_kib, KDF_MEMORY_KIB, KDF_MEMORY_CEILING_KIB)
            || !within(self.iterations, KDF_ITERATIONS, KDF_ITERATIONS_CEILING)
            || !within(self.parallelism, KDF_PARALLELISM, KDF_PARALLELISM_CEILING)
        {
            return Err(SdkError::KeystoreCorrupt(
                "keystore key derivation parameters are outside the supported range".into(),
            ));
        }
        self.salt_bytes().map(|_| ())
    }
    fn salt_bytes(&self) -> Result<Vec<u8>, SdkError> {
        B64.decode(&self.salt)
            .ok()
            .filter(|salt| salt.len() == SALT_BYTES)
            .ok_or_else(|| SdkError::KeystoreCorrupt("invalid keystore salt".into()))
    }
    fn derive(&self, passphrase: &[u8]) -> Result<Zeroizing<[u8; 32]>, SdkError> {
        if passphrase.is_empty() {
            return Err(SdkError::KeystorePassphrase);
        }
        let params =
            argon2::Params::new(self.memory_kib, self.iterations, self.parallelism, Some(32))
                .map_err(|err| SdkError::KeystoreCorrupt(err.to_string()))?;
        let argon =
            argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
        let mut key = Zeroizing::new([0u8; 32]);
        argon
            .hash_password_into(passphrase, &self.salt_bytes()?, key.as_mut())
            .map_err(|err| SdkError::KeystoreCorrupt(err.to_string()))?;
        Ok(key)
    }
}

/// An AES-256-GCM ciphertext and its random nonce. Each keystore has its own
/// key (a fresh salt) and holds few entries, far below the random-nonce limit.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Sealed {
    nonce: String,
    ciphertext: String,
}
impl Sealed {
    fn seal(key: &[u8; 32], plaintext: &[u8], aad: &[u8]) -> Result<Self, SdkError> {
        let cipher = Aes256Gcm::new(key.into());
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .map_err(|_| SdkError::Serialization("keystore encryption failed".into()))?;
        Ok(Self {
            nonce: B64.encode(nonce),
            ciphertext: B64.encode(ciphertext),
        })
    }
    fn open(&self, key: &[u8; 32], aad: &[u8]) -> Result<Zeroizing<Vec<u8>>, SdkError> {
        let corrupt = || SdkError::KeystoreCorrupt("invalid keystore ciphertext".into());
        let nonce = B64.decode(&self.nonce).map_err(|_| corrupt())?;
        if nonce.len() != NONCE_BYTES {
            return Err(corrupt());
        }
        let ciphertext = B64.decode(&self.ciphertext).map_err(|_| corrupt())?;
        Aes256Gcm::new(key.into())
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad,
                },
            )
            .map(Zeroizing::new)
            .map_err(|_| corrupt())
    }
}

/// The entry's public metadata, bound to its ciphertext.
fn entry_aad(entry: &KeystoreEntry) -> Result<Vec<u8>, SdkError> {
    let mut aad = DOMAIN.to_vec();
    aad.extend_from_slice(
        &serde_json::to_vec(entry).map_err(|err| SdkError::Serialization(err.to_string()))?,
    );
    Ok(aad)
}
fn check_aad() -> Vec<u8> {
    [DOMAIN, b"check"].concat()
}

/// Write the keystore owner-readable only (0600 on Unix), through a
/// temporary file and a rename, so it is never readable by other users.
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), SdkError> {
    use std::io::Write as _;
    let temporary = path.with_extension("json.tmp");
    let _ = fs::remove_file(&temporary);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)?;
    Ok(())
}

fn ensure_parent_dir(path: &Path) -> Result<(), SdkError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "keystore_tests.rs"]
mod tests;
