//! SLH-DSA emergency and upgrade verification through the existing pinned local helper.
//! Keys and policy come from committed state. These settings only control local execution.
use crate::emergency_freeze::ControlVerifier;
use crate::root_genesis::{
    run_verified_helper, validate_helper_execution, HelperOutcome, ObservedHelperPolicy,
    VerifiedHelperConfig,
};
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};
use std::path::PathBuf;

const PROFILE: &str = "SLH-DSA-SHAKE-256s";

// Only trait implementations select this action. It never comes from input.
#[derive(Clone, Copy)]
enum TrustedAction {
    Emergency,
    Upgrade,
}
impl TrustedAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Emergency => "emergency",
            Self::Upgrade => "upgrade",
        }
    }
}

/// Trusted node settings, separate from the signed control and committed authority.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EmergencyVerifierConfig {
    pub helper_path: PathBuf,
    pub helper_scratch_path: PathBuf,
    #[serde(default)]
    pub helper_execution: Option<ObservedHelperPolicy>,
    pub helper_sha256: String,
    pub max_helper_bytes: usize,
    pub max_request_bytes: usize,
    pub timeout_ms: u64,
}

#[derive(Clone, Debug)]
pub struct EmergencyVerifier {
    config: EmergencyVerifierConfig,
}

/// A local verifier failure is not a deterministic rejection of a proposed block.
#[derive(Debug, thiserror::Error)]
#[error("Emergency root verifier infrastructure failed: {0}")]
pub struct EmergencyVerifierInfrastructureError(#[source] anyhow::Error);

pub fn is_infrastructure_error(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.is::<EmergencyVerifierInfrastructureError>())
}

#[derive(Serialize)]
#[allow(non_snake_case)]
struct Policy<'a> {
    TrustedPublicKey: String,
    ChainID: &'a str,
    Action: &'static str,
    ExpectedSequence: u64,
    CurrentHeight: u64,
    ExpectedArtifactDigest: Vec<u8>,
}
#[derive(Serialize)]
#[allow(non_snake_case)]
struct Envelope<'a> {
    Version: u16,
    Profile: &'static str,
    ChainID: &'a str,
    Action: &'static str,
    Sequence: u64,
    NotBeforeHeight: u64,
    NotAfterHeight: u64,
    ArtifactDigest: Vec<u8>,
}
#[derive(Serialize)]
#[allow(non_snake_case)]
struct Request<'a> {
    Envelope: Envelope<'a>,
    Signature: String,
    Artifact: String,
}

impl EmergencyVerifier {
    pub fn new(config: EmergencyVerifierConfig) -> Result<Self> {
        validate_helper_execution(
            config.helper_execution.as_ref(),
            config.max_helper_bytes,
            config.timeout_ms,
        )?;
        ensure!(
            config.helper_path.is_absolute()
                && (config.helper_execution.is_some() || config.helper_scratch_path.is_absolute()),
            "Emergency helper and private scratch paths must be absolute"
        );
        ensure!(
            config.helper_sha256.len() == 64
                && config
                    .helper_sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "Emergency helper SHA-256 must be canonical"
        );
        ensure!(
            config.max_helper_bytes > 0 && config.max_request_bytes > 0 && config.timeout_ms > 0,
            "Emergency helper resource bounds must be explicit"
        );
        Ok(Self { config })
    }

    #[allow(clippy::too_many_arguments)]
    fn verify_inner(
        &self,
        action: TrustedAction,
        chain_id: &str,
        public_key: &[u8],
        sequence: u64,
        current_height: u64,
        not_before_height: u64,
        not_after_height: u64,
        artifact: &[u8],
        signature: &[u8],
    ) -> Result<bool> {
        // The caller validates authority policy. Malformed signature size is a
        // deterministic refusal and does not start a helper process.
        ensure!(
            public_key.len() == 64,
            "Committed emergency public key has invalid length"
        );
        if signature.len() != 29792 {
            return Ok(false);
        }
        // This is a local limit. An insufficient node limit must stop verification,
        // rather than make a locally configured node reject a valid proposal.
        ensure!(
            artifact.len() <= self.config.max_request_bytes,
            "Emergency artifact exceeds local helper resource bound"
        );
        let digest = Sha512::digest(artifact).to_vec();
        let policy_json = serde_json::to_vec(&Policy {
            TrustedPublicKey: B64.encode(public_key),
            ChainID: chain_id,
            Action: action.as_str(),
            ExpectedSequence: sequence,
            CurrentHeight: current_height,
            ExpectedArtifactDigest: digest.clone(),
        })?;
        let request_json = serde_json::to_vec(&Request {
            Envelope: Envelope {
                Version: 1,
                Profile: PROFILE,
                ChainID: chain_id,
                Action: action.as_str(),
                Sequence: sequence,
                NotBeforeHeight: not_before_height,
                NotAfterHeight: not_after_height,
                ArtifactDigest: digest,
            },
            Signature: B64.encode(signature),
            Artifact: B64.encode(artifact),
        })?;
        ensure!(
            request_json.len() <= self.config.max_request_bytes,
            "Emergency request exceeds local helper resource bound"
        );
        let response = run_verified_helper(&VerifiedHelperConfig {
            profile: PROFILE,
            helper_path: &self.config.helper_path,
            helper_scratch_path: Some(&self.config.helper_scratch_path),
            helper_execution: self.config.helper_execution.as_ref(),
            helper_sha256: &self.config.helper_sha256,
            max_helper_bytes: self.config.max_helper_bytes,
            max_request_bytes: self.config.max_request_bytes,
            timeout_ms: self.config.timeout_ms,
            policy_json: &policy_json,
            request_json: &request_json,
        })
        .context("Emergency helper execution failed")?;
        match response.outcome {
            HelperOutcome::Rejected => Ok(false),
            HelperOutcome::Verified(response) => {
                ensure!(
                    response.status == "VERIFIED"
                        && !response.production_qualified
                        && response.request_sha256 == hex::encode(Sha256::digest(&request_json))
                        && response.artifact_sha512 == hex::encode(Sha512::digest(artifact))
                        && response.chain_id == chain_id
                        && response.action == action.as_str()
                        && response.sequence == sequence,
                    "Emergency verifier response differs from exact request"
                );
                Ok(true)
            }
        }
    }
}
impl ControlVerifier for EmergencyVerifier {
    fn verify(
        &self,
        chain_id: &str,
        public_key: &[u8],
        sequence: u64,
        current_height: u64,
        not_before_height: u64,
        not_after_height: u64,
        artifact: &[u8],
        signature: &[u8],
    ) -> Result<bool> {
        self.verify_inner(
            TrustedAction::Emergency,
            chain_id,
            public_key,
            sequence,
            current_height,
            not_before_height,
            not_after_height,
            artifact,
            signature,
        )
        .map_err(|error| EmergencyVerifierInfrastructureError(error).into())
    }
}

impl crate::upgrade::Verifier for EmergencyVerifier {
    fn verify(
        &self,
        chain_id: &str,
        public_key: &[u8],
        sequence: u64,
        current_height: u64,
        not_before_height: u64,
        not_after_height: u64,
        artifact: &[u8],
        signature: &[u8],
    ) -> Result<bool> {
        self.verify_inner(
            TrustedAction::Upgrade,
            chain_id,
            public_key,
            sequence,
            current_height,
            not_before_height,
            not_after_height,
            artifact,
            signature,
        )
        .map_err(|error| EmergencyVerifierInfrastructureError(error).into())
    }
}

#[cfg(all(test, unix))]
#[path = "emergency_verifier_tests.rs"]
mod tests;
