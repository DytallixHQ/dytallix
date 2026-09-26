use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose;
use base64::Engine as _;
#[cfg(feature = "pqc-fips204")]
use fips204::ml_dsa_65;
#[cfg(feature = "mldsa87-development")]
use fips204::ml_dsa_87;
#[cfg(feature = "pqc-fips204")]
use fips204::traits::{KeyGen, SerDes};
#[cfg(not(feature = "pqc-fips204"))]
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};

#[cfg(feature = "pqc-fips204")]
fn generate_validator_secret_key() -> Vec<u8> {
    let (_pk, sk) = ml_dsa_65::KG::try_keygen().expect("validator key generation failed");
    sk.into_bytes().to_vec()
}

#[cfg(not(feature = "pqc-fips204"))]
fn generate_validator_secret_key() -> Vec<u8> {
    let mut key_bytes = vec![0u8; 64];
    OsRng.fill_bytes(&mut key_bytes);
    key_bytes
}

#[cfg(feature = "pqc-fips204")]
fn is_supported_validator_secret(key: &[u8]) -> bool {
    if key.len() == ml_dsa_65::SK_LEN {
        let mut sk = [0u8; ml_dsa_65::SK_LEN];
        sk.copy_from_slice(key);
        return ml_dsa_65::PrivateKey::try_from_bytes(sk).is_ok();
    }

    #[cfg(feature = "mldsa87-development")]
    if key.len() == ml_dsa_87::SK_LEN {
        let mut sk = [0u8; ml_dsa_87::SK_LEN];
        sk.copy_from_slice(key);
        return ml_dsa_87::PrivateKey::try_from_bytes(sk).is_ok();
    }

    false
}

#[cfg(not(feature = "pqc-fips204"))]
fn is_supported_validator_secret(key: &[u8]) -> bool {
    !key.is_empty()
}

// Async traits for providers
#[async_trait::async_trait]
pub trait KeyProvider {
    async fn get_validator_key(&self, id: &str) -> Result<Vec<u8>>;
    async fn put_validator_key(&self, id: &str, key: &[u8]) -> Result<()>;
}

// -------------------- Vault Provider --------------------
pub struct VaultProvider {
    base_url: String,
    token: String,
    kv_mount: String,
    path_base: String,
    client: reqwest::Client,
}

impl VaultProvider {
    pub fn new(base_url: String, token: String, kv_mount: String, path_base: String) -> Self {
        let client = reqwest::Client::new();
        Self {
            base_url,
            token,
            kv_mount,
            path_base,
            client,
        }
    }

    fn data_url(&self, id: &str) -> String {
        // KV v2 path: /v1/<mount>/data/<path>
        format!(
            "{}/v1/{}/data/{}/{}",
            self.base_url.trim_end_matches('/'),
            self.kv_mount.trim_matches('/'),
            self.path_base.trim_matches('/'),
            id
        )
    }
}

#[derive(Serialize, Debug)]
struct VaultWrite<'a> {
    data: VaultData<'a>,
}
#[derive(Serialize, Debug)]
struct VaultData<'a> {
    private_key: &'a str,
}
#[derive(Deserialize, Debug)]
struct VaultReadOuter {
    data: VaultReadData,
}
#[derive(Deserialize, Debug)]
struct VaultReadData {
    data: VaultReadInner,
}
#[derive(Deserialize, Debug)]
struct VaultReadInner {
    private_key: String,
}

#[async_trait::async_trait]
impl KeyProvider for VaultProvider {
    async fn get_validator_key(&self, id: &str) -> Result<Vec<u8>> {
        let url = self.data_url(id);
        let res = self
            .client
            .get(url)
            .header("X-Vault-Token", &self.token)
            .send()
            .await
            .context("vault get request failed")?;
        if !res.status().is_success() {
            return Err(anyhow!("vault get failed: {}", res.status()));
        }
        let body: VaultReadOuter = res.json().await.context("invalid vault json")?;
        let decoded = general_purpose::STANDARD
            .decode(body.data.data.private_key)
            .context("invalid base64 in vault secret")?;
        Ok(decoded)
    }

    async fn put_validator_key(&self, id: &str, key: &[u8]) -> Result<()> {
        let url = self.data_url(id);
        let b64 = general_purpose::STANDARD.encode(key);
        let payload = VaultWrite {
            data: VaultData { private_key: &b64 },
        };
        let res = self
            .client
            .post(url)
            .header("X-Vault-Token", &self.token)
            .json(&payload)
            .send()
            .await
            .context("vault put request failed")?;
        if !res.status().is_success() {
            return Err(anyhow!("vault put failed: {}", res.status()));
        }
        Ok(())
    }
}

// -------------------- Plain Keystore Provider (no passphrase) --------------------
use std::fs;
use std::path::PathBuf;

// Backward-compat evidence format note: we still write a proof file alongside keys.
// Not used for encryption anymore.
pub struct SealedKeystoreProvider {
    dir: PathBuf,
    read_only: bool,
}

impl SealedKeystoreProvider {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            read_only: false,
        }
    }

    /// Load an existing key without provisioning, replacement, or proof-file writes.
    pub fn read_only(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            read_only: true,
        }
    }

    fn file_path(&self, id: &str) -> PathBuf {
        let mut p = self.dir.clone();
        p.push(format!("validator-{id}.seal"));
        p
    }

    fn write_proof(&self, path: &PathBuf) {
        let proof_dir = PathBuf::from("launch-evidence/secrets");
        let _ = fs::create_dir_all(&proof_dir);
        let proof_path = proof_dir.join("keystore_proof.txt");
        if let Ok(meta) = fs::metadata(path) {
            let size = meta.len();
            use sha2::Digest;
            let sha = sha2::Sha256::digest(fs::read(path).unwrap_or_default());
            let _ = fs::write(
                proof_path,
                format!(
                    "path: {}\nsize: {} bytes\nsha256: 0x{}\n",
                    path.display(),
                    size,
                    hex::encode(sha)
                ),
            );
        }
    }
}

#[async_trait::async_trait]
impl KeyProvider for SealedKeystoreProvider {
    async fn get_validator_key(&self, id: &str) -> Result<Vec<u8>> {
        let path = self.file_path(id);
        if self.read_only {
            let key = fs::read(&path).context("read existing validator key failed")?;
            if !is_supported_validator_secret(&key) {
                return Err(anyhow!(
                    "existing validator key is not supported by the selected backend"
                ));
            }
            return Ok(key);
        }
        fs::create_dir_all(&self.dir).ok();
        if !path.exists() {
            let key_bytes = generate_validator_secret_key();
            fs::write(&path, &key_bytes).context("write keystore failed")?;
            self.write_proof(&path);
            Ok(key_bytes)
        } else {
            let pt = fs::read(&path).context("read keystore failed")?;
            if !is_supported_validator_secret(&pt) {
                return Err(anyhow!(
                    "Unsupported existing validator key; explicit migration required"
                ));
            }
            let key_bytes = pt;
            self.write_proof(&path);
            Ok(key_bytes)
        }
    }

    async fn put_validator_key(&self, id: &str, key: &[u8]) -> Result<()> {
        if self.read_only {
            return Err(anyhow!("validator key provider is read-only"));
        }
        let path = self.file_path(id);
        fs::create_dir_all(&self.dir).ok();
        fs::write(&path, key).context("write keystore failed")?;
        self.write_proof(&path);
        Ok(())
    }
}
// Note: interactive passphrase prompting removed; keystore is unencrypted (dev-only).

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "pqc-fips204")]
    #[tokio::test]
    async fn writable_provider_preserves_unsupported_existing_key() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("validator-existing.seal");
        let original = b"unsupported legacy key fixture";
        fs::write(&path, original).unwrap();
        let provider = SealedKeystoreProvider::new(temp.path());
        assert!(provider.get_validator_key("existing").await.is_err());
        assert_eq!(fs::read(path).unwrap(), original);
    }

    #[cfg(feature = "pqc-fips204")]
    #[test]
    fn new_validator_key_uses_operational_profile() {
        let secret = generate_validator_secret_key();
        assert_eq!(secret.len(), ml_dsa_65::SK_LEN);
        assert!(is_supported_validator_secret(&secret));
    }

    #[tokio::test]
    async fn read_only_provider_does_not_provision_missing_keys() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("missing");
        let provider = SealedKeystoreProvider::read_only(&directory);
        assert!(provider.get_validator_key("existing").await.is_err());
        assert!(!directory.exists());
    }

    #[cfg(feature = "pqc-fips204")]
    #[tokio::test]
    async fn read_only_provider_preserves_unsupported_existing_key() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("validator-existing.seal");
        let original = b"unsupported legacy key fixture";
        fs::write(&path, original).unwrap();
        let provider = SealedKeystoreProvider::read_only(temp.path());
        assert!(provider.get_validator_key("existing").await.is_err());
        assert_eq!(fs::read(path).unwrap(), original);
    }

    #[tokio::test]
    async fn read_only_provider_loads_valid_key_and_rejects_writes() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("validator-existing.seal");
        let original = generate_validator_secret_key();
        fs::write(&path, &original).unwrap();
        let provider = SealedKeystoreProvider::read_only(temp.path());
        assert_eq!(
            provider.get_validator_key("existing").await.unwrap(),
            original
        );
        assert!(provider
            .put_validator_key("existing", b"replacement")
            .await
            .is_err());
        assert_eq!(fs::read(path).unwrap(), original);
    }
}
