//! Root genesis signed three of five (production activation v1, step A2).
//!
//! Five genesis signers, separate from the emergency and upgrade custodians,
//! each sign the same genesis envelope over the root bundle with the offline
//! signer (`consensus/root-authorization/cmd/dytallix-root-sign`). Two public
//! records reach every node unchanged: the signer policy (five keys, threshold
//! three) and the combined signatures file. The node verifies every listed
//! signature through the pinned helper, one helper run per signature, and
//! commits a version 2 receipt listing the signing key IDs. The receipt is
//! consensus state, so every node must read the same signatures file.
//!
//! This is the only open path of a production build. Emergency, upgrade and
//! handover controls join it in step A4.
use super::*;

/// Genesis signers in the policy.
pub const SIGNERS: usize = 5;
/// Signatures required.
pub const THRESHOLD: usize = 3;
const RECORD_SCHEMA: u16 = 1;
const SEQUENCE: u64 = 1;
const MAX_POLICY_BYTES: usize = 16_384;
const MAX_SIGNATURES_BYTES: usize = 1 << 20;
const MAX_CONFIG_BYTES: usize = 65_536;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GenesisKey {
    /// The lowercase SHA-256 of the public key bytes.
    pub key_id: String,
    pub public_key_hex: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GenesisAuthority {
    /// Strictly sorted by key ID.
    pub keys: Vec<GenesisKey>,
    pub threshold: usize,
}

/// The public genesis signer record for one chain.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GenesisPolicy {
    pub schema: u16,
    pub chain_id: String,
    pub authority: GenesisAuthority,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GenesisSignature {
    pub key_id: String,
    pub signature_hex: String,
}

/// The combined signatures file: signatures strictly sorted by key ID.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GenesisSignatures {
    pub schema: u16,
    pub chain_id: String,
    pub bundle_sha512: String,
    pub signatures: Vec<GenesisSignature>,
}

impl GenesisPolicy {
    pub fn validate(&self, chain_id: &str) -> Result<()> {
        ensure!(
            self.schema == RECORD_SCHEMA && self.chain_id == chain_id,
            "Root genesis signer policy schema or chain differs"
        );
        ensure!(
            self.authority.keys.len() == SIGNERS && self.authority.threshold == THRESHOLD,
            "Root genesis requires three of five genesis signers"
        );
        let mut previous: Option<&str> = None;
        for key in &self.authority.keys {
            let public = digest_bytes(&key.public_key_hex, 64)
                .context("Root genesis public key must be 64 bytes of lowercase hex")?;
            ensure!(
                key.key_id == hex::encode(Sha256::digest(&public)),
                "Root genesis key ID must be the SHA-256 of its public key"
            );
            ensure!(
                previous.is_none_or(|id| id < key.key_id.as_str()),
                "Root genesis keys must be strictly sorted by key ID"
            );
            previous = Some(&key.key_id);
        }
        Ok(())
    }

    /// The public key material, for the separation from other authorities.
    pub fn public_keys(&self) -> impl Iterator<Item = &str> {
        self.authority
            .keys
            .iter()
            .map(|key| key.public_key_hex.as_str())
    }

    fn key(&self, key_id: &str) -> Option<&GenesisKey> {
        self.authority.keys.iter().find(|key| key.key_id == key_id)
    }

    fn sha256(&self) -> Result<String> {
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(self)?)))
    }
}

impl GenesisSignatures {
    fn validate(&self, policy: &GenesisPolicy, artifact_sha512: &str) -> Result<()> {
        ensure!(
            self.schema == RECORD_SCHEMA
                && self.chain_id == policy.chain_id
                && self.bundle_sha512 == artifact_sha512,
            "Root genesis signatures are for another chain or root bundle"
        );
        ensure!(
            (THRESHOLD..=SIGNERS).contains(&self.signatures.len()),
            "Root genesis needs three to five signatures"
        );
        let mut previous: Option<&str> = None;
        for signature in &self.signatures {
            ensure!(
                policy.key(&signature.key_id).is_some(),
                "Root genesis signature by a key outside the signer policy"
            );
            ensure!(
                previous.is_none_or(|id| id < signature.key_id.as_str()),
                "Root genesis signatures must be strictly sorted by key ID"
            );
            digest_bytes(&signature.signature_hex, 29_792)
                .context("Root genesis signature must be 29,792 bytes of lowercase hex")?;
            previous = Some(&signature.key_id);
        }
        Ok(())
    }
}

/// Decode a public record that must be exact canonical JSON.
fn canonical<T: Serialize + serde::de::DeserializeOwned>(raw: &[u8], name: &str) -> Result<T> {
    let value: T = serde_json::from_slice(raw).with_context(|| format!("Invalid {name}"))?;
    ensure!(
        serde_json::to_vec(&value)? == raw,
        "The {name} must be exact canonical JSON"
    );
    Ok(value)
}

/// Trusted local settings for the threshold root genesis: the helper, the two
/// public records and the external artifacts the bundle binds.
#[derive(Clone, Debug)]
pub struct RootGenesis {
    pub helper_path: PathBuf,
    /// Only the test launcher uses a scratch directory.
    pub helper_scratch_path: Option<PathBuf>,
    /// Required outside test builds.
    pub helper_execution: Option<ObservedHelperPolicy>,
    pub helper_sha256: String,
    pub max_helper_bytes: usize,
    pub max_request_bytes: usize,
    pub timeout_ms: u64,
    pub policy_json: Vec<u8>,
    pub signatures_json: Vec<u8>,
    pub engine_genesis_path: PathBuf,
    pub max_engine_genesis_bytes: usize,
    pub engine_genesis_sha512: String,
    pub release_manifest_path: PathBuf,
    pub max_release_manifest_bytes: usize,
    pub release_manifest_sha512: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RootGenesisInput {
    profile: String,
    helper_path: PathBuf,
    #[serde(default)]
    helper_scratch_path: Option<PathBuf>,
    #[serde(default)]
    helper_execution: Option<ObservedHelperPolicy>,
    helper_sha256: String,
    max_helper_bytes: usize,
    max_request_bytes: usize,
    timeout_ms: u64,
    policy_path: PathBuf,
    signatures_path: PathBuf,
    engine_genesis_path: PathBuf,
    max_engine_genesis_bytes: usize,
    engine_genesis_sha512: String,
    release_manifest_path: PathBuf,
    max_release_manifest_bytes: usize,
    release_manifest_sha512: String,
}

#[derive(Serialize)]
struct ReceiptV2<'a> {
    version: u16,
    scope: &'static str,
    profile: &'static str,
    chain_id: &'a str,
    action: &'static str,
    consumed_sequence: u64,
    policy_sha256: String,
    threshold: usize,
    signing_key_ids: Vec<&'a str>,
    artifact_sha512: &'a str,
    envelope_sha256: String,
    signatures_sha256: String,
}

impl RootGenesis {
    /// Load the trusted root configuration and the two public records it
    /// names. The files belong to launch configuration, never to RPC input.
    pub fn from_config(path: &Path) -> Result<Self> {
        let input: RootGenesisInput =
            serde_json::from_slice(&read_public_input(path, MAX_CONFIG_BYTES)?)?;
        ensure!(
            input.profile == PROFILE,
            "Root genesis requires the SLH-DSA-SHAKE-256s profile"
        );
        validate_helper_execution(
            input.helper_execution.as_ref(),
            input.max_helper_bytes,
            input.timeout_ms,
        )?;
        Ok(Self {
            helper_path: input.helper_path,
            helper_scratch_path: input.helper_scratch_path,
            helper_execution: input.helper_execution,
            helper_sha256: input.helper_sha256,
            max_helper_bytes: input.max_helper_bytes,
            max_request_bytes: input.max_request_bytes,
            timeout_ms: input.timeout_ms,
            policy_json: read_public_input(&input.policy_path, MAX_POLICY_BYTES)?,
            signatures_json: read_public_input(&input.signatures_path, MAX_SIGNATURES_BYTES)?,
            engine_genesis_path: input.engine_genesis_path,
            max_engine_genesis_bytes: input.max_engine_genesis_bytes,
            engine_genesis_sha512: input.engine_genesis_sha512,
            release_manifest_path: input.release_manifest_path,
            max_release_manifest_bytes: input.max_release_manifest_bytes,
            release_manifest_sha512: input.release_manifest_sha512,
        })
    }

    /// The signer policy, checked against the chain.
    pub fn policy(&self, chain: &str) -> Result<GenesisPolicy> {
        ensure!(
            self.policy_json.len() <= MAX_POLICY_BYTES,
            "Root genesis signer policy exceeds its bound"
        );
        let policy: GenesisPolicy = canonical(&self.policy_json, "root genesis signer policy")?;
        policy.validate(chain)?;
        Ok(policy)
    }

    /// Verify the signatures over the exact bundle and prepare the receipt.
    /// Runs on every open, including restart, against the actual files.
    pub(crate) fn prepare(
        &self,
        chain: &str,
        app: &[u8],
        config: &[u8],
    ) -> Result<PreparedRootGenesis> {
        validate_helper_execution(
            self.helper_execution.as_ref(),
            self.max_helper_bytes,
            self.timeout_ms,
        )?;
        ensure!(
            self.max_helper_bytes > 0 && self.max_request_bytes > 0 && self.timeout_ms > 0,
            "Explicit verifier resource bounds required"
        );
        let policy = self.policy(chain)?;
        ensure!(
            self.signatures_json.len() <= MAX_SIGNATURES_BYTES,
            "Root genesis signatures exceed their bound"
        );
        let signatures: GenesisSignatures =
            canonical(&self.signatures_json, "root genesis signatures file")?;
        let engine = verified_artifact_digest(
            &self.engine_genesis_path,
            self.max_engine_genesis_bytes,
            &self.engine_genesis_sha512,
        )
        .context("Engine genesis artifact verification failed")?;
        let release = verified_artifact_digest(
            &self.release_manifest_path,
            self.max_release_manifest_bytes,
            &self.release_manifest_sha512,
        )
        .context("Release manifest artifact verification failed")?;
        let artifact = bundle(app, config, &engine, &release)?;
        let digest = Sha512::digest(&artifact).to_vec();
        let artifact_sha512 = hex::encode(&digest);
        signatures.validate(&policy, &artifact_sha512)?;
        let envelope = Envelope {
            version: 1,
            profile: PROFILE.into(),
            chain_id: chain.into(),
            action: "genesis".into(),
            sequence: SEQUENCE,
            not_before: 0,
            not_after: 0,
            artifact_digest: digest.clone(),
        };
        let envelope_sha256 = hex::encode(Sha256::digest(serde_json::to_vec(&envelope)?));
        let encoded_artifact = B64.encode(&artifact);
        let mut request = Request {
            envelope,
            signature: String::new(),
            artifact: encoded_artifact,
        };
        for signature in &signatures.signatures {
            let key = policy
                .key(&signature.key_id)
                .context("Root genesis key missing")?;
            let policy_json = serde_json::to_vec(&Policy {
                public_key: B64.encode(hex::decode(&key.public_key_hex)?),
                chain_id: chain.into(),
                action: "genesis".into(),
                sequence: SEQUENCE,
                height: 0,
                artifact_digest: digest.clone(),
            })?;
            request.signature = B64.encode(hex::decode(&signature.signature_hex)?);
            let request_json = serde_json::to_vec(&request)?;
            ensure!(
                request_json.len() <= self.max_request_bytes,
                "Root genesis request exceeds the local helper bound"
            );
            let execution = run_verified_helper(&VerifiedHelperConfig {
                profile: PROFILE,
                helper_path: &self.helper_path,
                helper_scratch_path: self.helper_scratch_path.as_deref(),
                helper_execution: self.helper_execution.as_ref(),
                helper_sha256: &self.helper_sha256,
                max_helper_bytes: self.max_helper_bytes,
                max_request_bytes: self.max_request_bytes,
                timeout_ms: self.timeout_ms,
                policy_json: &policy_json,
                request_json: &request_json,
            })?;
            let HelperOutcome::Verified(response) = execution.outcome else {
                bail!(
                    "Root genesis signature by key {} was rejected",
                    signature.key_id
                );
            };
            ensure!(
                response.status == "VERIFIED"
                    && response.request_sha256 == hex::encode(Sha256::digest(&request_json))
                    && response.artifact_sha512 == artifact_sha512
                    && response.chain_id == chain
                    && response.action == "genesis"
                    && response.sequence == SEQUENCE,
                "Root verifier response differs from request"
            );
        }
        let receipt = ReceiptV2 {
            version: 2,
            scope: if crate::build_profile::PRODUCTION {
                "production"
            } else {
                "development"
            },
            profile: PROFILE,
            chain_id: chain,
            action: "genesis",
            consumed_sequence: SEQUENCE,
            policy_sha256: policy.sha256()?,
            threshold: THRESHOLD,
            signing_key_ids: signatures
                .signatures
                .iter()
                .map(|signature| signature.key_id.as_str())
                .collect(),
            artifact_sha512: &artifact_sha512,
            envelope_sha256,
            signatures_sha256: hex::encode(Sha256::digest(&self.signatures_json)),
        };
        Ok(PreparedRootGenesis {
            encoded: serde_json::to_vec(&receipt)?,
            threshold: true,
        })
    }
}

#[cfg(all(test, unix))]
#[path = "threshold_tests.rs"]
mod tests;
