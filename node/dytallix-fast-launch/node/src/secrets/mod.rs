use crate::addr::{AccountAddress, AddressNetwork, OriginKeyAlgorithm};
pub mod providers;
use crate::secrets::providers::KeyProvider; // bring trait into scope
use base64::{engine::general_purpose::STANDARD as B64, Engine};
#[cfg(feature = "pqc-fips204")]
use fips204::ml_dsa_65;
#[cfg(feature = "mldsa87-development")]
use fips204::ml_dsa_87;
#[cfg(feature = "pqc-fips204")]
use fips204::traits::{SerDes, Signer};

use once_cell::sync::OnceCell;
use zeroize::Zeroizing;

// Global holder for the validator private key material (zeroized on drop)
static VALIDATOR_KEY: OnceCell<Zeroizing<Vec<u8>>> = OnceCell::new();
static VALIDATOR_PUBLIC_KEY: OnceCell<Vec<u8>> = OnceCell::new();
static VALIDATOR_ADDRESS: OnceCell<String> = OnceCell::new();
static VALIDATOR_ALGORITHM: OnceCell<String> = OnceCell::new();

#[cfg(feature = "pqc-fips204")]
fn derive_identity(
    secret_key: &[u8],
    network: AddressNetwork,
    chain_id: &str,
) -> Option<(Vec<u8>, String, &'static str)> {
    if secret_key.len() == ml_dsa_65::SK_LEN {
        let mut sk_bytes = [0u8; ml_dsa_65::SK_LEN];
        sk_bytes.copy_from_slice(secret_key);
        if let Ok(sk) = ml_dsa_65::PrivateKey::try_from_bytes(sk_bytes) {
            let pk = sk.get_public_key().into_bytes().to_vec();
            let address =
                crate::addr::initial_address(network, chain_id, OriginKeyAlgorithm::MlDsa65, &pk)
                    .ok()?;
            return Some((pk, address, "mldsa65"));
        }
    }

    // Keep exact ML-DSA-87 identities available for local compatibility only.
    #[cfg(feature = "mldsa87-development")]
    if network != AddressNetwork::Mainnet && secret_key.len() == ml_dsa_87::SK_LEN {
        let mut sk_bytes = [0u8; ml_dsa_87::SK_LEN];
        sk_bytes.copy_from_slice(secret_key);
        if let Ok(sk) = ml_dsa_87::PrivateKey::try_from_bytes(sk_bytes) {
            let pk = sk.get_public_key().into_bytes().to_vec();
            let address =
                crate::addr::initial_address(network, chain_id, OriginKeyAlgorithm::MlDsa87, &pk)
                    .ok()?;
            return Some((pk, address, "mldsa87"));
        }
    }

    None
}

#[cfg(not(feature = "pqc-fips204"))]
fn derive_identity(
    _secret_key: &[u8],
    _network: AddressNetwork,
    _chain_id: &str,
) -> Option<(Vec<u8>, String, &'static str)> {
    None
}

/// True when the service requires an existing key and fatal initialization errors.
pub fn require_existing_validator_key() -> bool {
    std::env::var("DYT_REQUIRE_EXISTING_VALIDATOR_KEY").as_deref() == Ok("1")
}

/// Initialize the validator key from the configured provider.
/// Return the loaded key length, no key, or a provider error.
pub async fn init_validator_key(
    network: AddressNetwork,
    chain_id: &str,
) -> anyhow::Result<Option<usize>> {
    anyhow::ensure!(network != AddressNetwork::Mainnet,
        "NO GO: Mainnet validator key activation requires reviewed PQC transport and root authorization evidence");
    let validator_id = std::env::var("VALIDATOR_ID").unwrap_or_else(|_| "default".to_string());

    // Prefer DYTALLIX_ envs, fallback to generic
    let vault_url = std::env::var("DYTALLIX_VAULT_URL")
        .ok()
        .or_else(|| std::env::var("VAULT_URL").ok());
    let vault_token = std::env::var("DYTALLIX_VAULT_TOKEN")
        .ok()
        .or_else(|| std::env::var("VAULT_TOKEN").ok());

    let maybe_key = if let (Some(url), Some(token)) = (vault_url, vault_token) {
        // Vault path config
        let mount =
            std::env::var("DYTALLIX_VAULT_KV_MOUNT").unwrap_or_else(|_| "secret".to_string());
        let base = std::env::var("DYTALLIX_VAULT_PATH_BASE")
            .unwrap_or_else(|_| "dytallix/validators".to_string());
        let provider = providers::VaultProvider::new(url, token, mount, base);
        Some(provider.get_validator_key(&validator_id).await?)
    } else {
        let dir = std::env::var("DYT_KEYSTORE_DIR").unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            format!("{home}/.dytallix/keystore")
        });
        let provider = if require_existing_validator_key() {
            providers::SealedKeystoreProvider::read_only(dir)
        } else {
            providers::SealedKeystoreProvider::new(dir)
        };
        Some(provider.get_validator_key(&validator_id).await?)
    };

    if let Some(bytes) = maybe_key {
        let len = bytes.len();
        if let Some((public_key, address, algorithm)) = derive_identity(&bytes, network, chain_id) {
            let _ = VALIDATOR_PUBLIC_KEY.set(public_key.clone());
            let _ = VALIDATOR_ADDRESS.set(address);
            let _ = VALIDATOR_ALGORITHM.set(algorithm.to_string());
        } else if let Ok(address) = std::env::var("DYT_VALIDATOR_ADDRESS") {
            AccountAddress::decode(network, &address)?;
            let _ = VALIDATOR_ADDRESS.set(address);
            let _ = VALIDATOR_ALGORITHM.set("configured-address".to_string());
        }
        let secret = Zeroizing::new(bytes);
        let _ = VALIDATOR_KEY.set(secret); // ignore if already set
        return Ok(Some(len));
    }
    Ok(None)
}

/// Returns a reference to the loaded validator key (if any).
pub fn validator_key() -> Option<&'static Zeroizing<Vec<u8>>> {
    VALIDATOR_KEY.get()
}

pub fn validator_public_key_b64() -> Option<String> {
    VALIDATOR_PUBLIC_KEY.get().map(|pk| B64.encode(pk))
}

pub fn validator_address() -> Option<&'static str> {
    VALIDATOR_ADDRESS.get().map(|address| address.as_str())
}

pub fn validator_algorithm() -> Option<&'static str> {
    VALIDATOR_ALGORITHM
        .get()
        .map(|algorithm| algorithm.as_str())
}

#[cfg(all(test, feature = "mldsa87-development"))]
mod identity_tests {
    use super::*;
    use fips204::traits::KeyGen;

    #[test]
    fn validator_identity_has_explicit_algorithm_and_network() {
        let (_, sk65) = ml_dsa_65::KG::keygen_from_seed(&[11; 32]);
        let (_, sk87) = ml_dsa_87::KG::keygen_from_seed(&[12; 32]);
        for (secret, expected) in [
            (sk65.into_bytes().to_vec(), "mldsa65"),
            (sk87.into_bytes().to_vec(), "mldsa87"),
        ] {
            for network in [
                AddressNetwork::Mainnet,
                AddressNetwork::Testnet,
                AddressNetwork::Development,
            ] {
                if network == AddressNetwork::Mainnet && expected == "mldsa87" {
                    assert!(derive_identity(&secret, network, "identity-local").is_none());
                    continue;
                }
                let (pk, address, algorithm) =
                    derive_identity(&secret, network, "identity-local").unwrap();
                assert_eq!(algorithm, expected);
                let parsed = AccountAddress::decode(network, &address).unwrap();
                assert_eq!(parsed.network(), network);
                let signature = if expected == "mldsa65" {
                    let key =
                        ml_dsa_65::PrivateKey::try_from_bytes(secret.clone().try_into().unwrap())
                            .unwrap();
                    key.try_sign(b"identity control", &[]).unwrap().to_vec()
                } else {
                    let key =
                        ml_dsa_87::PrivateKey::try_from_bytes(secret.clone().try_into().unwrap())
                            .unwrap();
                    key.try_sign(b"identity control", &[]).unwrap().to_vec()
                };
                crate::crypto::verify(
                    &pk,
                    b"identity control",
                    &signature,
                    algorithm.parse().unwrap(),
                )
                .unwrap();
            }
        }
    }
    #[test]
    fn identity_derivation_requires_valid_key_and_chain() {
        assert!(derive_identity(b"invalid", AddressNetwork::Development, "chain").is_none());
        let (_, sk) = ml_dsa_87::KG::keygen_from_seed(&[13; 32]);
        assert!(derive_identity(&sk.into_bytes(), AddressNetwork::Development, "").is_none());
    }
}
