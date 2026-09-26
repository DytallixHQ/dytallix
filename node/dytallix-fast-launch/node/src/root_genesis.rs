//! Explicit development-only root authorization for the existing genesis batch.
//! No signing key, upgrade route, emergency route, or second replay ledger.
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(any(target_os = "linux", test))]
#[path = "root_genesis/helper_failure.rs"]
mod helper_failure;

pub(crate) const STATE_KEY: &[u8] = b"root:authorization:v1";
const PROFILE: &str = "SLH-DSA-SHAKE-256s";
pub const OBSERVED_HELPER_PROFILE: &str = "linux-immutable-observed-helper-v1";

// One bounded local diagnostic. A failed attempt removes the prior receipt.
// This is never deserialized as authority and is not consensus state.
#[derive(Default)]
struct HelperAdmissionState {
    attempts: u64,
    completed: u64,
    receipt: Option<serde_json::Value>,
}
thread_local! {
    static HELPER_ADMISSION: std::cell::RefCell<HelperAdmissionState> =
        std::cell::RefCell::new(HelperAdmissionState::default());
}
fn begin_helper_admission() -> Result<u64> {
    HELPER_ADMISSION.with(|state| {
        let mut state = state
            .try_borrow_mut()
            .context("Helper receipt already borrowed")?;
        state.receipt = None;
        state.attempts = state
            .attempts
            .checked_add(1)
            .context("Helper attempt counter exhausted")?;
        Ok(state.attempts)
    })
}
fn complete_helper_admission(
    attempt: u64,
    mut receipt: serde_json::Value,
    check: impl Fn() -> Result<()>,
) -> Result<()> {
    HELPER_ADMISSION.with(|state| {
        let mut state = state
            .try_borrow_mut()
            .context("Helper receipt already borrowed")?;
        ensure!(
            state.attempts == attempt && state.receipt.is_none(),
            "Helper receipt attempt changed"
        );
        let completed = state
            .completed
            .checked_add(1)
            .context("Helper completion counter exhausted")?;
        receipt["attempt"] = attempt.into();
        receipt["completed_count"] = completed.into();
        ensure!(
            serde_json::to_vec(&receipt)?.len() <= 4096,
            "Helper admission receipt exceeds fixed bound"
        );
        check()?;
        state.receipt = Some(receipt);
        if let Err(error) = check() {
            state.receipt = None;
            return Err(error);
        }
        state.completed = completed;
        Ok(())
    })
}
/// Latest completed helper attempt on this caller thread. None also means the
/// latest attempt failed. Callers must not treat this diagnostic as authority.
pub fn helper_admission_receipt() -> Option<serde_json::Value> {
    HELPER_ADMISSION.with(|state| state.try_borrow().ok()?.receipt.clone())
}

#[cfg(test)]
mod admission_receipt_tests {
    use super::*;
    fn reset() {
        HELPER_ADMISSION.with(|state| *state.borrow_mut() = HelperAdmissionState::default());
    }
    #[test]
    fn next_attempt_and_counter_failure_remove_previous_success() {
        reset();
        let first = begin_helper_admission().unwrap();
        complete_helper_admission(first, serde_json::json!({"status":"VERIFIED"}), || Ok(()))
            .unwrap();
        assert!(helper_admission_receipt().is_some());
        begin_helper_admission().unwrap();
        assert!(helper_admission_receipt().is_none());
        HELPER_ADMISSION.with(|state| state.borrow_mut().attempts = u64::MAX);
        assert!(begin_helper_admission().is_err());
        assert!(helper_admission_receipt().is_none());
    }
    #[test]
    fn late_cancel_or_expired_publication_never_leaves_success() {
        for fail_at in [1, 2] {
            reset();
            let attempt = begin_helper_admission().unwrap();
            let calls = std::cell::Cell::new(0);
            assert!(complete_helper_admission(
                attempt,
                serde_json::json!({"status":"VERIFIED"}),
                || {
                    calls.set(calls.get() + 1);
                    ensure!(
                        calls.get() != fail_at,
                        "Injected late cancellation or deadline"
                    );
                    Ok(())
                }
            )
            .is_err());
            assert!(helper_admission_receipt().is_none());
            HELPER_ADMISSION.with(|state| assert_eq!(state.borrow().completed, 0));
        }
    }
    #[test]
    fn overflow_or_oversize_completion_is_not_published() {
        reset();
        let attempt = begin_helper_admission().unwrap();
        HELPER_ADMISSION.with(|state| state.borrow_mut().completed = u64::MAX);
        assert!(complete_helper_admission(attempt, serde_json::json!({}), || Ok(())).is_err());
        assert!(helper_admission_receipt().is_none());
        reset();
        let attempt = begin_helper_admission().unwrap();
        assert!(complete_helper_admission(
            attempt,
            serde_json::json!({"unexpected":"x".repeat(4096)}),
            || Ok(())
        )
        .is_err());
        assert!(helper_admission_receipt().is_none());
    }
}

/// Independent trusted local execution policy. It never comes from a candidate
/// catalog, transaction, helper response, or serialized observation receipt.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedHelperPolicy {
    pub profile: String,
    pub helper_bytes: u64,
    pub helper_sha512: String,
    pub owner_security: dytallix_release_runtime::ownership_security::HelperSecurityPolicy,
    pub max_program_headers: usize,
    pub max_section_headers: usize,
    pub max_stat_bytes: usize,
    pub max_maps_bytes: usize,
    pub max_map_entries: usize,
    pub max_path_bytes: usize,
    pub max_unique_files: usize,
    pub max_file_bytes: u64,
    pub max_total_file_bytes: u64,
    pub max_elapsed_ms: u64,
    pub max_output_bytes: usize,
    pub cleanup_timeout_ms: u64,
}
impl ObservedHelperPolicy {
    fn validate(&self, max_helper_bytes: usize, timeout_ms: u64) -> Result<()> {
        ensure!(
            self.profile == OBSERVED_HELPER_PROFILE,
            "Unsupported observed helper execution profile"
        );
        digest_bytes(&self.helper_sha512, 64)?;
        self.owner_security.validate()?;
        ensure!(
            self.helper_bytes > 0
                && self.helper_bytes <= u64::try_from(max_helper_bytes)?
                && self.helper_bytes <= self.max_file_bytes
                && self.max_file_bytes <= u64::try_from(max_helper_bytes)?
                && self.max_total_file_bytes >= self.helper_bytes,
            "Observed helper file bounds differ"
        );
        ensure!(
            [
                self.max_program_headers,
                self.max_section_headers,
                self.max_stat_bytes,
                self.max_maps_bytes,
                self.max_map_entries,
                self.max_path_bytes,
                self.max_unique_files,
                self.max_output_bytes
            ]
            .iter()
            .all(|v| *v > 0),
            "Observed helper bounds must be explicit and nonzero"
        );
        ensure!(
            self.max_elapsed_ms > 0
                && self.cleanup_timeout_ms > 0
                && self.cleanup_timeout_ms < timeout_ms,
            "Observed helper deadlines must reserve bounded cleanup within total timeout"
        );
        Ok(())
    }
}
pub(crate) fn validate_helper_execution(
    policy: Option<&ObservedHelperPolicy>,
    max_helper_bytes: usize,
    timeout_ms: u64,
) -> Result<()> {
    if let Some(policy) = policy {
        return policy.validate(max_helper_bytes, timeout_ms);
    }
    #[cfg(test)]
    return Ok(());
    #[cfg(not(test))]
    bail!("Explicit immutable observed helper execution policy is required; historical snapshot launch is test-only")
}

/// All fields must be supplied explicitly by the trusted development caller.
/// Policy is independent of request bytes. No production entry point uses this.
#[derive(Clone, Debug)]
pub struct DevelopmentRootGenesis {
    pub enabled: bool,
    pub profile: String,
    pub helper_path: PathBuf,
    /// Optional private execution directory supplied by trusted local configuration.
    /// None preserves the existing temporary-directory behavior.
    pub helper_scratch_path: Option<PathBuf>,
    pub helper_execution: Option<ObservedHelperPolicy>,
    pub helper_sha256: String,
    pub max_helper_bytes: usize,
    pub max_request_bytes: usize,
    pub timeout_ms: u64,
    pub policy_json: Vec<u8>,
    pub request_json: Vec<u8>,
    pub engine_genesis_path: PathBuf,
    pub max_engine_genesis_bytes: usize,
    pub engine_genesis_sha512: String,
    pub release_manifest_path: PathBuf,
    pub max_release_manifest_bytes: usize,
    pub release_manifest_sha512: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    #[serde(rename = "TrustedPublicKey")]
    public_key: String,
    #[serde(rename = "ChainID")]
    chain_id: String,
    #[serde(rename = "Action")]
    action: String,
    #[serde(rename = "ExpectedSequence")]
    sequence: u64,
    #[serde(rename = "CurrentHeight")]
    height: u64,
    #[serde(rename = "ExpectedArtifactDigest")]
    artifact_digest: Vec<u8>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    #[serde(rename = "Version")]
    version: u16,
    #[serde(rename = "Profile")]
    profile: String,
    #[serde(rename = "ChainID")]
    chain_id: String,
    #[serde(rename = "Action")]
    action: String,
    #[serde(rename = "Sequence")]
    sequence: u64,
    #[serde(rename = "NotBeforeHeight")]
    not_before: u64,
    #[serde(rename = "NotAfterHeight")]
    not_after: u64,
    #[serde(rename = "ArtifactDigest")]
    artifact_digest: Vec<u8>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Request {
    #[serde(rename = "Envelope")]
    envelope: Envelope,
    #[serde(rename = "Signature")]
    signature: String,
    #[serde(rename = "Artifact")]
    artifact: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Response {
    #[serde(rename = "Status")]
    pub(crate) status: String,
    #[serde(rename = "RequestSHA256")]
    pub(crate) request_sha256: String,
    #[serde(rename = "ArtifactSHA512")]
    pub(crate) artifact_sha512: String,
    #[serde(rename = "ChainID")]
    pub(crate) chain_id: String,
    #[serde(rename = "Action")]
    pub(crate) action: String,
    #[serde(rename = "Sequence")]
    pub(crate) sequence: u64,
    #[serde(rename = "ProductionQualified")]
    pub(crate) production_qualified: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u16,
    scope: String,
    profile: String,
    chain_id: String,
    action: String,
    consumed_sequence: u64,
    root_public_key_sha256: String,
    artifact_sha512: String,
    envelope_sha256: String,
    signature_sha256: String,
}

/// Construction requires successful verification. No public constructor or
/// writable fields can bypass the consumer's validation path.
#[derive(Clone)]
pub(crate) struct PreparedRootGenesis {
    encoded: Vec<u8>,
}
impl PreparedRootGenesis {
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.encoded
    }
}

fn digest_bytes(value: &str, size: usize) -> Result<Vec<u8>> {
    ensure!(
        value.len() == size * 2
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "Noncanonical artifact digest"
    );
    Ok(hex::decode(value)?)
}

pub(crate) fn bundle(
    app: &[u8],
    config: &[u8],
    engine_digest: &[u8],
    release_digest: &[u8],
) -> Result<Vec<u8>> {
    ensure!(
        engine_digest.len() == 64 && release_digest.len() == 64,
        "Root bundle digest length"
    );
    let mut output = b"DYTALLIX/ROOT-GENESIS/v1\0".to_vec();
    for value in [app, config] {
        output.extend_from_slice(&u64::try_from(value.len())?.to_be_bytes());
        output.extend_from_slice(value);
    }
    output.extend_from_slice(engine_digest);
    output.extend_from_slice(release_digest);
    Ok(output)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DevelopmentInput {
    enabled: bool,
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
    request_path: PathBuf,
    engine_genesis_path: PathBuf,
    max_engine_genesis_bytes: usize,
    engine_genesis_sha512: String,
    release_manifest_path: PathBuf,
    max_release_manifest_bytes: usize,
    release_manifest_sha512: String,
}

fn read_public_input(path: &Path, limit: usize) -> Result<Vec<u8>> {
    ensure!(
        path.is_absolute() && limit > 0,
        "Root input requires an absolute path and explicit bound"
    );
    let metadata = std::fs::symlink_metadata(path)?;
    ensure!(
        metadata.file_type().is_file() && metadata.len() <= u64::try_from(limit)?,
        "Root input is not a bounded regular file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        ensure!(
            metadata.permissions().mode() & 0o022 == 0,
            "Root input is writable by group or others"
        );
    }
    let file = std::fs::File::open(path)?;
    let opened = file.metadata()?;
    ensure!(
        opened.is_file() && opened.len() <= u64::try_from(limit)?,
        "Opened root input is not a bounded regular file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        ensure!(
            opened.dev() == metadata.dev()
                && opened.ino() == metadata.ino()
                && opened.permissions().mode() & 0o022 == 0,
            "Root input changed before opening or has unsafe permissions"
        );
    }
    let mut bytes = Vec::new();
    file.take(
        u64::try_from(limit)?
            .checked_add(1)
            .context("Root input bound overflow")?,
    )
    .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= limit, "Root input grew beyond caller bound");
    Ok(bytes)
}

fn verified_artifact_digest(path: &Path, limit: usize, expected: &str) -> Result<Vec<u8>> {
    let expected = digest_bytes(expected, 64)?;
    let bytes = read_public_input(path, limit)?;
    ensure!(!bytes.is_empty(), "Root artifact is empty");
    let actual = Sha512::digest(&bytes).to_vec();
    ensure!(
        actual == expected,
        "Root artifact SHA-512 differs from expected digest"
    );
    Ok(actual)
}

impl DevelopmentRootGenesis {
    /// Load explicit development opt-in and public inputs. The configuration and
    /// policy files belong to trusted launch configuration, not RPC input.
    pub fn from_development_config(path: &Path) -> Result<Self> {
        let input: DevelopmentInput = serde_json::from_slice(&read_public_input(path, 65536)?)?;
        ensure!(input.enabled, "Development root genesis is not enabled");
        validate_helper_execution(
            input.helper_execution.as_ref(),
            input.max_helper_bytes,
            input.timeout_ms,
        )?;
        let policy_json = read_public_input(&input.policy_path, 16384)?;
        let request_json = read_public_input(&input.request_path, input.max_request_bytes)?;
        Ok(Self {
            enabled: input.enabled,
            profile: input.profile,
            helper_path: input.helper_path,
            helper_scratch_path: input.helper_scratch_path,
            helper_execution: input.helper_execution,
            helper_sha256: input.helper_sha256,
            max_helper_bytes: input.max_helper_bytes,
            max_request_bytes: input.max_request_bytes,
            timeout_ms: input.timeout_ms,
            policy_json,
            request_json,
            engine_genesis_path: input.engine_genesis_path,
            max_engine_genesis_bytes: input.max_engine_genesis_bytes,
            engine_genesis_sha512: input.engine_genesis_sha512,
            release_manifest_path: input.release_manifest_path,
            max_release_manifest_bytes: input.max_release_manifest_bytes,
            release_manifest_sha512: input.release_manifest_sha512,
        })
    }
    pub(crate) fn prepare(
        &self,
        chain: &str,
        app: &[u8],
        config: &[u8],
    ) -> Result<PreparedRootGenesis> {
        ensure!(
            self.enabled && self.profile == PROFILE,
            "Explicit development root profile required"
        );
        validate_helper_execution(
            self.helper_execution.as_ref(),
            self.max_helper_bytes,
            self.timeout_ms,
        )?;
        ensure!(
            self.max_helper_bytes > 0 && self.max_request_bytes > 0 && self.timeout_ms > 0,
            "Explicit verifier resource bounds required"
        );
        ensure!(
            self.request_json.len() <= self.max_request_bytes && self.policy_json.len() <= 16384,
            "Root verification input exceeds caller bound"
        );
        let policy: Policy = serde_json::from_slice(&self.policy_json)?;
        let request: Request = serde_json::from_slice(&self.request_json)?;
        ensure!(
            serde_json::to_vec(&policy)? == self.policy_json
                && serde_json::to_vec(&request)? == self.request_json,
            "Root input must use exact canonical helper JSON"
        );
        ensure!(
            policy.chain_id == chain
                && policy.action == "genesis"
                && policy.sequence > 0
                && policy.height == 0,
            "Root genesis trusted policy differs"
        );
        let public = B64.decode(&policy.public_key)?;
        ensure!(
            public.len() == 64 && B64.encode(&public) == policy.public_key,
            "Root public key encoding differs"
        );
        let signature = B64.decode(&request.signature)?;
        ensure!(
            signature.len() == 29792 && B64.encode(&signature) == request.signature,
            "Root signature encoding differs"
        );
        // Hash the actual external inputs on every open, including restart.
        // Digest strings alone cannot authorize absent or changed artifacts.
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
        let encoded_size = app
            .len()
            .checked_add(config.len())
            .and_then(|n| n.checked_add(b"DYTALLIX/ROOT-GENESIS/v1\0".len() + 16 + 128))
            .context("Root bundle length overflow")?;
        ensure!(
            encoded_size <= self.max_request_bytes,
            "Root bundle exceeds caller bound"
        );
        let artifact = bundle(app, config, &engine, &release)?;
        ensure!(
            artifact.len() <= self.max_request_bytes,
            "Root bundle exceeds caller bound"
        );
        ensure!(
            B64.encode(&artifact) == request.artifact
                && policy.artifact_digest == Sha512::digest(&artifact).to_vec(),
            "Root bundle does not bind current application and configuration"
        );
        let expected_request = hex::encode(Sha256::digest(&self.request_json));
        let expected_artifact = hex::encode(Sha512::digest(&artifact));
        let response = self.run_helper()?;
        ensure!(
            response.status == "VERIFIED"
                && !response.production_qualified
                && response.request_sha256 == expected_request
                && response.artifact_sha512 == expected_artifact
                && response.chain_id == chain
                && response.action == "genesis"
                && response.sequence == policy.sequence,
            "Root verifier response differs from request"
        );
        let receipt = Receipt {
            version: 1,
            scope: "development-only".into(),
            profile: self.profile.clone(),
            chain_id: chain.into(),
            action: "genesis".into(),
            consumed_sequence: policy.sequence,
            root_public_key_sha256: hex::encode(Sha256::digest(&public)),
            artifact_sha512: expected_artifact,
            envelope_sha256: hex::encode(Sha256::digest(serde_json::to_vec(&request.envelope)?)),
            signature_sha256: hex::encode(Sha256::digest(&signature)),
        };
        Ok(PreparedRootGenesis {
            encoded: serde_json::to_vec(&receipt)?,
        })
    }

    fn helper_config(&self) -> VerifiedHelperConfig<'_> {
        VerifiedHelperConfig {
            profile: &self.profile,
            helper_path: &self.helper_path,
            helper_scratch_path: self.helper_scratch_path.as_deref(),
            helper_execution: self.helper_execution.as_ref(),
            helper_sha256: &self.helper_sha256,
            max_helper_bytes: self.max_helper_bytes,
            max_request_bytes: self.max_request_bytes,
            timeout_ms: self.timeout_ms,
            policy_json: &self.policy_json,
            request_json: &self.request_json,
        }
    }
    fn run_helper(&self) -> Result<Response> {
        match run_verified_helper(&self.helper_config())?.outcome {
            HelperOutcome::Verified(response) => Ok(response),
            HelperOutcome::Rejected => bail!("Root verifier rejected authorization"),
        }
    }

    /// Qualification-only helper protocol execution. This does not prepare or
    /// commit genesis. The caller owns any evidence sink and its I/O limits.
    #[cfg(any(test, feature = "helper-qualification"))]
    pub fn execute_helper_for_qualification(&self) -> Result<HelperQualificationResult> {
        ensure!(
            self.enabled && self.profile == PROFILE && self.helper_execution.is_some(),
            "Helper qualification requires explicit observed execution policy"
        );
        let execution = run_verified_helper(&self.helper_config())?;
        let status = match execution.outcome {
            HelperOutcome::Verified(_) => HelperQualificationStatus::HelperVerified,
            HelperOutcome::Rejected => HelperQualificationStatus::HelperRejected,
        };
        Ok(HelperQualificationResult {
            status,
            evidence: execution
                .evidence
                .context("Observed helper evidence missing")?,
        })
    }
}

/// Bounded local observation evidence. No deserializer or public constructor
/// can turn caller JSON into an observed helper result. It is not authority.
#[derive(Debug)]
pub struct HelperExecutionEvidence {
    encoded: Vec<u8>,
}
impl HelperExecutionEvidence {
    fn encode(report: &serde_json::Value, bound: usize) -> Result<Self> {
        let encoded = serde_json::to_vec(report)?;
        ensure!(
            !encoded.is_empty() && encoded.len() <= bound,
            "Observed helper returned evidence exceeds configured output bound"
        );
        Ok(Self { encoded })
    }
    pub fn bytes(&self) -> &[u8] {
        &self.encoded
    }
}
#[cfg(any(test, feature = "helper-qualification"))]
#[derive(Debug, PartialEq, Eq)]
pub enum HelperQualificationStatus {
    HelperVerified,
    HelperRejected,
}
#[cfg(any(test, feature = "helper-qualification"))]
#[derive(Debug)]
pub struct HelperQualificationResult {
    pub status: HelperQualificationStatus,
    pub evidence: HelperExecutionEvidence,
}

/// Shared local helper execution settings. These are never derived from a transaction.
pub(crate) struct VerifiedHelperConfig<'a> {
    pub(crate) profile: &'a str,
    pub(crate) helper_path: &'a Path,
    pub(crate) helper_scratch_path: Option<&'a Path>,
    pub(crate) helper_execution: Option<&'a ObservedHelperPolicy>,
    pub(crate) helper_sha256: &'a str,
    pub(crate) max_helper_bytes: usize,
    pub(crate) max_request_bytes: usize,
    pub(crate) timeout_ms: u64,
    pub(crate) policy_json: &'a [u8],
    pub(crate) request_json: &'a [u8],
}
pub(crate) enum HelperOutcome {
    Verified(Response),
    Rejected,
}
pub(crate) struct HelperExecution {
    pub(crate) outcome: HelperOutcome,
    // Normal consensus consumers intentionally discard this local evidence.
    #[cfg_attr(not(any(test, feature = "helper-qualification")), allow(dead_code))]
    evidence: Option<HelperExecutionEvidence>,
}

pub(crate) fn run_verified_helper(config: &VerifiedHelperConfig<'_>) -> Result<HelperExecution> {
    // Clear stale success before validation, platform checks or spawn can fail.
    let admission_attempt = begin_helper_admission()?;
    validate_helper_execution(
        config.helper_execution,
        config.max_helper_bytes,
        config.timeout_ms,
    )?;
    if let Some(policy) = config.helper_execution {
        #[cfg(target_os = "linux")]
        return observed_execution::run(config, policy, admission_attempt);
        #[cfg(not(target_os = "linux"))]
        {
            let _ = policy;
            bail!("Immutable observed helper execution requires Linux");
        }
    }
    #[cfg(all(test, unix))]
    return run_historical_test_helper(config).map(|outcome| HelperExecution {
        outcome,
        evidence: None,
    });
    #[cfg(not(all(test, unix)))]
    bail!("Historical helper snapshot launcher is not present in this build")
}

#[cfg(target_os = "linux")]
mod observed_execution {
    use super::*;
    use dytallix_release_runtime::observation::{self, controlled_pause, static_helper};
    use dytallix_release_runtime::ownership::{self, Bootstrap, OwnerThread, Role};
    use std::os::fd::{AsRawFd, RawFd};
    use std::os::unix::process::CommandExt;
    use std::process::{ChildStderr, ChildStdin, ChildStdout, ExitStatus};

    const READY: &[u8] = b"DYTALLIX-ROOT-READY-v1\n";
    const ACK: &[u8] = b"DYTALLIX-ROOT-ACK-v1\n";
    const REJECTION: &[u8] = b"root verification rejected\n";
    struct OwnedHelper {
        child: Child,
        reaped: bool,
    }
    impl Drop for OwnedHelper {
        fn drop(&mut self) {
            if !self.reaped {
                // No unbounded wait or worker join in a destructor. The normal
                // error path already attempted bounded kill/reap below.
                let _ = self.child.kill();
                if self.child.try_wait().ok().flatten().is_some() {
                    self.reaped = true;
                }
            }
        }
    }
    struct Channel {
        input: Option<ChildStdin>,
        output: ChildStdout,
        errors: ChildStderr,
        error_bytes: Vec<u8>,
        total_output: usize,
        max_output: usize,
        output_eof: bool,
        error_eof: bool,
        check_cancel: fn() -> Result<()>,
    }
    fn nonblocking(fd: RawFd) -> Result<()> {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        ensure!(
            flags >= 0 && unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == 0,
            "Cannot make helper pipe nonblocking"
        );
        Ok(())
    }
    fn remaining(deadline: Instant) -> Result<Duration> {
        let value = deadline.saturating_duration_since(Instant::now());
        if value.is_zero() {
            return Err(helper_failure::ObservedHelperDeadlineExpired {
                deadline,
                checked_monotonic_ns: ownership::monotonic_now_ns()?,
            }.into());
        }
        Ok(value)
    }
    #[cfg(test)]
    #[test]
    fn actual_remaining_check_preserves_original_deadline_type_and_timestamp() {
        let expired = Instant::now().checked_sub(Duration::from_millis(1)).unwrap();
        let before = ownership::monotonic_now_ns().unwrap();
        let error = remaining(expired).unwrap_err().context("actual deadline check");
        let typed = error.downcast_ref::<helper_failure::ObservedHelperDeadlineExpired>().unwrap();
        assert_eq!(typed.deadline, expired);
        assert!(typed.checked_monotonic_ns >= before);
        assert!(typed.checked_monotonic_ns <= ownership::monotonic_now_ns().unwrap());
        let future = Instant::now() + Duration::from_secs(1);
        assert!(!remaining(future).unwrap().is_zero());
    }
    fn execution_remaining(deadline: Instant) -> Result<Duration> {
        ownership::check_cancellation()?;
        remaining(deadline)
    }
    fn pause(deadline: Instant) -> Result<()> {
        std::thread::sleep(remaining(deadline)?.min(Duration::from_millis(1)));
        Ok(())
    }
    impl Channel {
        fn new(child: &mut Child, max_output: usize) -> Result<Self> {
            let input = child
                .stdin
                .take()
                .context("Missing observed helper input")?;
            let output = child
                .stdout
                .take()
                .context("Missing observed helper output")?;
            let errors = child
                .stderr
                .take()
                .context("Missing observed helper stderr")?;
            for fd in [input.as_raw_fd(), output.as_raw_fd(), errors.as_raw_fd()] {
                nonblocking(fd)?;
            }
            Ok(Self {
                input: Some(input),
                output,
                errors,
                error_bytes: Vec::new(),
                total_output: 0,
                max_output,
                output_eof: false,
                error_eof: false,
                check_cancel: ownership::check_cancellation,
            })
        }
        fn count(&mut self, count: usize) -> Result<()> {
            self.total_output = self
                .total_output
                .checked_add(count)
                .context("Helper output count overflow")?;
            ensure!(
                self.total_output <= self.max_output,
                "Observed helper total output exceeded bound"
            );
            Ok(())
        }
        fn drain_errors(&mut self, deadline: Instant) -> Result<()> {
            loop {
                (self.check_cancel)()?;
                remaining(deadline)?;
                let mut bytes = [0; 1024];
                match self.errors.read(&mut bytes) {
                    Ok(0) => {
                        self.error_eof = true;
                        return Ok(());
                    }
                    Ok(count) => {
                        self.count(count)?;
                        self.error_bytes.extend_from_slice(&bytes[..count]);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) => return Err(error.into()),
                }
            }
        }
        fn read_exact(&mut self, bytes: &mut [u8], deadline: Instant) -> Result<()> {
            let mut offset = 0;
            while offset < bytes.len() {
                self.drain_errors(deadline)?;
                (self.check_cancel)()?;
                remaining(deadline)?;
                match self.output.read(&mut bytes[offset..]) {
                    Ok(0) => bail!("Observed helper output ended before complete protocol frame"),
                    Ok(count) => {
                        self.count(count)?;
                        offset += count;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        pause(deadline)?
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) => return Err(error.into()),
                }
            }
            Ok(())
        }
        fn write_all(&mut self, bytes: &[u8], deadline: Instant) -> Result<()> {
            let mut offset = 0;
            while offset < bytes.len() {
                self.drain_errors(deadline)?;
                (self.check_cancel)()?;
                remaining(deadline)?;
                match self
                    .input
                    .as_mut()
                    .context("Observed helper input already closed")?
                    .write(&bytes[offset..])
                {
                    Ok(0) => bail!("Observed helper input closed during protocol"),
                    Ok(count) => offset += count,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        pause(deadline)?
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) => return Err(error.into()),
                }
            }
            Ok(())
        }
        fn drain_after_ack(&mut self, deadline: Instant) -> Result<()> {
            self.drain_errors(deadline)?;
            let mut byte = [0; 1];
            match self.output.read(&mut byte) {
                Ok(0) => self.output_eof = true,
                Ok(count) => {
                    self.count(count)?;
                    bail!("Observed helper emitted trailing output");
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error.into()),
            }
            Ok(())
        }
    }
    fn observe_bounds(
        policy: &ObservedHelperPolicy,
        deadline: Instant,
    ) -> Result<observation::Bounds> {
        Ok(observation::Bounds {
            max_stat_bytes: policy.max_stat_bytes,
            max_maps_bytes: policy.max_maps_bytes,
            max_map_entries: policy.max_map_entries,
            max_path_bytes: policy.max_path_bytes,
            max_unique_files: policy.max_unique_files,
            max_file_bytes: policy.max_file_bytes,
            max_total_file_bytes: policy.max_total_file_bytes,
            max_elapsed: execution_remaining(deadline)?
                .min(Duration::from_millis(policy.max_elapsed_ms)),
            allowed_owner_uids: [0].into_iter().collect(),
        })
    }
    fn reap(helper: &mut OwnedHelper, deadline: Instant) -> Result<()> {
        if helper.reaped {
            return Ok(());
        }
        if helper.child.try_wait()?.is_some() {
            helper.reaped = true;
            return Ok(());
        }
        if let Err(error) = helper.child.kill() {
            if helper.child.try_wait()?.is_some() {
                helper.reaped = true;
                return Ok(());
            }
            return Err(error).context("Cannot terminate failed observed helper");
        }
        loop {
            if helper.child.try_wait()?.is_some() {
                helper.reaped = true;
                return Ok(());
            }
            pause(deadline)
                .context("Observed helper cleanup did not reap within total deadline")?;
        }
    }
    fn finish_protocol(
        helper: &mut OwnedHelper,
        channel: &mut Channel,
        deadline: Instant,
    ) -> Result<ExitStatus> {
        let mut status = None;
        loop {
            channel.drain_after_ack(deadline)?;
            if status.is_none() {
                if let Some(exited) = helper.child.try_wait()? {
                    helper.reaped = true;
                    status = Some(exited);
                }
            }
            if channel.output_eof && channel.error_eof {
                if let Some(status) = status {
                    return Ok(status);
                }
            }
            pause(deadline)?;
        }
    }
    pub(super) fn run(
        config: &VerifiedHelperConfig<'_>,
        policy: &ObservedHelperPolicy,
        admission_attempt: u64,
    ) -> Result<HelperExecution> {
        let started = Instant::now();
        let owner = OwnerThread::new()?;
        ownership::check_cancellation()?;
        policy.validate(config.max_helper_bytes, config.timeout_ms)?;
        let owner_security = dytallix_release_runtime::ownership_security::capture_current()?;
        let helper_security = policy.owner_security.expected_child()?;
        ensure!(
            config.profile == PROFILE
                && config.max_request_bytes > 0
                && !config.request_json.is_empty()
                && config.request_json.len() <= config.max_request_bytes
                && config.policy_json.len() <= 16384,
            "Observed helper profile or request bounds invalid"
        );
        let total_deadline = started
            .checked_add(Duration::from_millis(config.timeout_ms))
            .context("Observed helper total deadline overflow")?;
        let execution_deadline = total_deadline
            .checked_sub(Duration::from_millis(policy.cleanup_timeout_ms))
            .context("Observed helper cleanup reservation overflow")?;
        let file_policy = static_helper::StaticHelperPolicy {
            max_file_bytes: u64::try_from(config.max_helper_bytes)?,
            max_path_bytes: policy.max_path_bytes,
            max_program_headers: policy.max_program_headers,
            max_section_headers: policy.max_section_headers,
            max_elapsed: remaining(execution_deadline)?
                .min(Duration::from_millis(policy.max_elapsed_ms)),
        };
        let file =
            static_helper::verify_file(config.helper_path, config.helper_sha256, &file_policy)?;
        ensure!(
            file.digest().bytes == policy.helper_bytes
                && file.digest().sha512 == policy.helper_sha512,
            "Observed helper differs from independent byte/SHA-512 pin"
        );
        execution_remaining(execution_deadline)?;
        let helper_context: [u8; 64] = digest_bytes(&policy.helper_sha512, 64)?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Helper digest length differs"))?;
        // Convert the remaining execution interval once. Never renew the deadline.
        let deadline_ns = ownership::monotonic_deadline(remaining(execution_deadline)?)?;
        let owner_bounds = observe_bounds(policy, execution_deadline)?;
        let mut owner_stat = Vec::new();
        std::fs::File::open("/proc/self/stat")?
            .take(u64::try_from(policy.max_stat_bytes)?.checked_add(1).context("Owner stat bound overflow")?)
            .read_to_end(&mut owner_stat)?;
        let owner_start = observation::parse_stat(&owner_stat, owner.pid(), &owner_bounds)?;
        let mut bootstrap = Bootstrap::prepare(&owner, Role::Helper, helper_context, deadline_ns)?;
        let parent = unsafe { libc::getpid() };
        let mut command = Command::new(file.path());
        command
            .env_clear()
            .arg("--profile")
            .arg(config.profile)
            .arg("--execution-profile")
            .arg(OBSERVED_HELPER_PROFILE)
            .arg("--policy-json")
            .arg(std::str::from_utf8(config.policy_json)?)
            .arg("--max-input-bytes")
            .arg(config.max_request_bytes.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        unsafe {
            command.pre_exec(move || {
                libc::umask(0o077);
                if libc::setpgid(0, 0) != 0
                    || libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::getppid() != parent {
                    return Err(std::io::Error::from_raw_os_error(libc::ECHILD));
                }
                if libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 4u32) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut helper = OwnedHelper {
            child: bootstrap
                .spawn(&mut command)
                .context("Cannot start immutable observed helper")?,
            reaped: false,
        };
        drop(command);
        let mut failure_binding = None;
        let result = (|| {
            let mut channel = Channel::new(&mut helper.child, policy.max_output_bytes)?;
            bootstrap.await_ready_with_cancel(ownership::check_cancellation)?;
            owner.check()?;
            let identity = static_helper::capture_owned_child(
                &mut helper.child,
                &file,
                &observe_bounds(policy, execution_deadline)?,
            )?;
            helper_security.check_owned(identity.owned_identity())?;
            let pause_budget = controlled_pause::Budget {
                total: remaining(total_deadline)?.min(Duration::from_secs(60)),
                cleanup_reserve: Duration::from_millis(policy.cleanup_timeout_ms),
            };
            let (owner_admission, owner_pause_timing) = controlled_pause::observe_paused_static_helper(
                &mut helper.child,
                &identity,
                &file,
                &observe_bounds(policy, execution_deadline)?,
                pause_budget,
                ownership::check_cancellation,
            )?;
            failure_binding = Some(serde_json::json!({
                "owner_pid":owner.pid(), "owner_tid":owner.pid(),
                "owner_start_ticks":owner_start.start_ticks,
                "owner_label":owner_security.apparmor_label,
                "helper_pid":identity.start().pid,
                "helper_start_ticks":identity.start().start_ticks,
                "helper_label":helper_security.apparmor_label,
                "uid":helper_security.uid, "gid":helper_security.gid,
                "mount_namespace":helper_security.mount_namespace,
                "no_new_privileges":1, "seccomp":2,
                "helper_sha512":file.digest().sha512,
                "helper_bytes":file.digest().bytes,
                "helper_device":file.device(), "helper_inode":file.inode(),
                "policy_sha256":hex::encode(Sha256::digest(config.policy_json)),
                "request_sha256":hex::encode(Sha256::digest(config.request_json)),
                "deadline_monotonic_ns":deadline_ns,
                "total_timeout_ms":config.timeout_ms,
                "cleanup_reserve_ms":policy.cleanup_timeout_ms,
                "guard_ready_before_observation":true,
                "go_after_owner_admission":false,
                "owner_admission_maps_sha256":owner_admission.maps_sha256()
            }));
            // The helper cannot decode policy or emit its protocol READY until
            // the parent inspects the retained child and releases the owner gate.
            bootstrap.release_with_cancel(ownership::check_cancellation)?;
            if let Some(binding) = failure_binding.as_mut() {
                binding["go_after_owner_admission"] = serde_json::json!(true);
            }
            let mut ready = vec![0; READY.len()];
            channel.read_exact(&mut ready, execution_deadline)?;
            ensure!(
                ready == READY && channel.error_bytes.is_empty(),
                "Observed helper READY protocol differs"
            );
            owner.check()?;
            helper_security.check_owned(identity.owned_identity())?;
            let pre_request = static_helper::observe_owned_child(
                &mut helper.child,
                &identity,
                &file,
                &observe_bounds(policy, execution_deadline)?,
            )?;
            let length = u32::try_from(config.request_json.len())
                .context("Observed helper request exceeds framing range")?;
            channel.write_all(&length.to_be_bytes(), execution_deadline)?;
            channel.write_all(config.request_json, execution_deadline)?;
            let mut prefix = [0u8; 5];
            channel.read_exact(&mut prefix, execution_deadline)?;
            let length = u32::from_be_bytes(prefix[1..].try_into().unwrap()) as usize;
            ensure!(
                (prefix[0] == 0 && (1..=4096).contains(&length)) || (prefix[0] == 2 && length == 0),
                "Observed helper result frame differs"
            );
            ensure!(
                length <= policy.max_output_bytes.saturating_sub(channel.total_output),
                "Observed helper result exceeds output bound"
            );
            let mut raw = vec![0; length];
            channel.read_exact(&mut raw, execution_deadline)?;
            channel.drain_errors(execution_deadline)?;
            ensure!(
                channel.error_bytes.is_empty(),
                "Observed helper emitted diagnostics before ACK"
            );
            owner.check()?;
            helper_security.check_owned(identity.owned_identity())?;
            let post_result = static_helper::observe_owned_child(
                &mut helper.child,
                &identity,
                &file,
                &observe_bounds(policy, execution_deadline)?,
            )?;
            channel.write_all(ACK, execution_deadline)?;
            channel.input.take();
            let status = finish_protocol(&mut helper, &mut channel, execution_deadline)?;
            owner.check()?;
            let outcome = if prefix[0] == 2 {
                ensure!(
                    status.code() == Some(2) && channel.error_bytes == REJECTION,
                    "Observed helper refusal lifecycle differs"
                );
                HelperOutcome::Rejected
            } else {
                ensure!(
                    status.success() && channel.error_bytes.is_empty(),
                    "Observed helper success lifecycle differs"
                );
                let response: Response = serde_json::from_slice(&raw)?;
                ensure!(
                    serde_json::to_vec(&response)? == raw,
                    "Observed helper response is not canonical"
                );
                HelperOutcome::Verified(response)
            };
            // Local evidence only. No runtime report becomes signed approval or
            // durable authority. The caller still verifies exact response bytes.
            let report = serde_json::json!({"event":"immutable_helper_observed","execution_profile":OBSERVED_HELPER_PROFILE,"helper_sha256":file.digest().sha256,"helper_sha512":file.digest().sha512,"helper_bytes":file.digest().bytes,"request_sha256":hex::encode(Sha256::digest(config.request_json)),"canonical_result_sha256":if prefix[0]==0 {Some(hex::encode(Sha256::digest(&raw)))} else {None},"result_status":if prefix[0]==0 {"VERIFIED"} else {"REJECTED"},"result_payload_bytes":raw.len(),"owner_admission":owner_admission.report(),"owner_pause_timing":owner_pause_timing,"pre_request":pre_request.report(),"post_result":post_result.report(),"lifecycle":{"ready_before_request":true,"same_owned_child":true,"ack_after_observation":true,"natural_exit_code":status.code(),"reaped":helper.reaped,"total_elapsed_ms":started.elapsed().as_millis()},"scratch_used":false,"continuous_enforcement":false,"production_qualified":false});
            let evidence = HelperExecutionEvidence::encode(&report, policy.max_output_bytes)?;
            remaining(total_deadline)?;
            // Publish only after the same owned child completed and was reaped.
            // Fixed fields omit request bytes, keys and unbounded map histories.
            complete_helper_admission(
                admission_attempt,
                serde_json::json!({
                    "schema":1, "scope":"LOCAL_COMPLETED_HELPER_ADMISSION_V1",
                    "production_qualified":false, "release_authority":false,
                    "owner_pid":owner.pid(), "owner_tid":owner.pid(),
                    "owner_label":owner_security.apparmor_label,
                    "helper_pid":identity.start().pid,
                    "helper_start_ticks":identity.start().start_ticks,
                    "helper_label":helper_security.apparmor_label,
                    "uid":helper_security.uid, "gid":helper_security.gid,
                    "mount_namespace":helper_security.mount_namespace,
                    "no_new_privileges":1, "seccomp":2,
                    "helper_sha512":file.digest().sha512,
                    "helper_bytes":file.digest().bytes,
                    "helper_device":file.device(), "helper_inode":file.inode(),
                    "policy_sha256":hex::encode(Sha256::digest(config.policy_json)),
                    "request_sha256":hex::encode(Sha256::digest(config.request_json)),
                    "owner_admission_maps_sha256":owner_admission.maps_sha256(),
                    "pre_request_maps_sha256":pre_request.maps_sha256(),
                    "post_result_maps_sha256":post_result.maps_sha256(),
                    "deadline_monotonic_ns":deadline_ns,
                    "total_timeout_ms":config.timeout_ms,
                    "cleanup_reserve_ms":policy.cleanup_timeout_ms,
                    "total_elapsed_ms":u64::try_from(started.elapsed().as_millis())?,
                    "status":if prefix[0]==0 {"VERIFIED"} else {"REJECTED"},
                    "guard_ready_before_observation":true, "go_after_owner_admission":true,
                    "protocol_ready_after_go":true, "ack_after_observation":true,
                    "same_owned_child":true, "natural_exit_code":status.code(), "reaped":helper.reaped
                }),
                || {
                    owner.check()?;
                    ownership::check_cancellation()?;
                    remaining(total_deadline)?;
                    Ok(())
                },
            )?;
            Ok(HelperExecution {
                outcome,
                evidence: Some(evidence),
            })
        })();
        match result {
            Ok(outcome) => Ok(outcome),
            Err(error) => {
                let cleanup = reap(&mut helper, total_deadline);
                // Diagnostic failure never replaces the original error or cleanup priority.
                if let Some(binding) = failure_binding {
                    use std::os::unix::process::ExitStatusExt;
                    let exit_status = if helper.reaped {
                        helper.child.try_wait().ok().flatten().map(ExitStatusExt::into_raw)
                    } else { None };
                    let _ = helper_failure::emit(
                        binding, &error, admission_attempt, cleanup.is_ok(), helper.reaped,
                        exit_status, started, total_deadline, execution_deadline,
                    );
                }
                Err(helper_failure::preserve_error(error, cleanup))
            },
        }
    }
    #[cfg(test)]
    #[path = "observed_helper_tests.rs"]
    mod tests;
}

#[cfg(all(test, unix))]
fn run_historical_test_helper(config: &VerifiedHelperConfig<'_>) -> Result<HelperOutcome> {
    ensure!(
        config.profile == PROFILE
            && config.max_helper_bytes > 0
            && config.max_request_bytes > 0
            && config.timeout_ms > 0,
        "Explicit verifier profile and resource bounds required"
    );
    ensure!(
        config.policy_json.len() <= 16384 && config.request_json.len() <= config.max_request_bytes,
        "Root verification input exceeds caller bound"
    );
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
    ensure!(
        config.helper_path.is_absolute(),
        "Root helper path must be absolute"
    );
    let metadata = std::fs::symlink_metadata(&config.helper_path)?;
    ensure!(
        metadata.file_type().is_file() && metadata.len() <= u64::try_from(config.max_helper_bytes)?,
        "Root helper file is invalid or oversized"
    );
    let mut bytes = Vec::new();
    std::fs::File::open(&config.helper_path)?
        .take(
            u64::try_from(config.max_helper_bytes)?
                .checked_add(1)
                .context("Helper bound overflow")?,
        )
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= config.max_helper_bytes
            && hex::encode(Sha256::digest(&bytes)) == config.helper_sha256
            && digest_bytes(&config.helper_sha256, 32)?.len() == 32,
        "Root helper hash differs"
    );
    // Execute a private snapshot of the exact bytes checked above. This
    // removes replacement of the source path between hash and process start.
    let scratch = config
        .helper_scratch_path
        .map(Path::to_path_buf)
        .unwrap_or_else(std::env::temp_dir);
    let configured_metadata = config
        .helper_scratch_path
        .as_ref()
        .map(|path| {
            ensure!(
                path.is_absolute(),
                "Root helper scratch path must be absolute"
            );
            let metadata = std::fs::symlink_metadata(path)
                .context("Root helper scratch directory is unavailable")?;
            ensure!(
                metadata.file_type().is_dir() && metadata.permissions().mode() & 0o077 == 0,
                "Root helper scratch must be a private directory, not a symlink"
            );
            Ok::<_, anyhow::Error>(metadata)
        })
        .transpose()?;
    let directory = scratch.join(format!("dyt-root-verifier-{}", uuid::Uuid::new_v4()));
    std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
    let cleanup = PrivateHelperDirectory(directory);
    if let Some(before) = configured_metadata {
        use std::os::unix::fs::MetadataExt;
        let current = std::fs::symlink_metadata(&scratch)?;
        let created = std::fs::symlink_metadata(&cleanup.0)?;
        ensure!(
            current.file_type().is_dir()
                && current.permissions().mode() & 0o077 == 0
                && before.dev() == current.dev()
                && before.ino() == current.ino()
                && current.uid() == created.uid(),
            "Root helper scratch changed or is not owned by the executing user"
        );
    }
    let executable = cleanup.0.join("verify");
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o500)
        .open(&executable)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o500))?;
    let mut process = ChildGuard(
            Command::new(&executable)
                .env_clear()
                .arg("--profile")
                .arg(&config.profile)
                .arg("--policy-json")
                .arg(std::str::from_utf8(&config.policy_json)?)
                .arg("--max-input-bytes")
                .arg(config.max_request_bytes.to_string())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .context("Cannot execute verified root helper snapshot; check scratch mount execution policy")?,
        );
    let mut input = process.0.stdin.take().context("Missing verifier input")?;
    let output = process.0.stdout.take().context("Missing verifier output")?;
    let stderr = process
        .0
        .stderr
        .take()
        .context("Missing verifier error output")?;
    let request = config.request_json.to_vec();
    let writer = std::thread::spawn(move || input.write_all(&request));
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        output.take(4097).read_to_end(&mut bytes).map(|_| bytes)
    });
    let errors = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.take(4097).read_to_end(&mut bytes).map(|_| bytes)
    });
    let started = Instant::now();
    let outcome = loop {
        match process.0.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {}
            Err(error) => {
                let _ = process.0.kill();
                let _ = process.0.wait();
                break Err(error.into());
            }
        }
        if started.elapsed() >= Duration::from_millis(config.timeout_ms) {
            let _ = process.0.kill();
            let _ = process.0.wait();
            break Err(anyhow::anyhow!("Root verifier deadline exceeded"));
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    // Guard kills and waits on early process errors. Normal and timeout
    // paths reap before joining bounded pipe workers.
    let written = writer
        .join()
        .map_err(|_| anyhow::anyhow!("Root verifier writer failed"))?;
    let read = reader
        .join()
        .map_err(|_| anyhow::anyhow!("Root verifier reader failed"))?;
    let errors = errors
        .join()
        .map_err(|_| anyhow::anyhow!("Root verifier error reader failed"))?;
    let status = outcome?;
    let stderr = errors.context("Root verifier error read failed")?;
    ensure!(
        stderr.len() <= 4096,
        "Root verifier error output exceeded bound"
    );
    let raw = read.context("Root verifier output read failed")?;
    ensure!(raw.len() <= 4096, "Root verifier output exceeded bound");
    if status.code() == Some(2) && stderr == b"root verification rejected\n" && raw.is_empty() {
        // The pinned helper consumes the complete bounded request before this exit.
        written.context("Root verifier input write failed before refusal")?;
        return Ok(HelperOutcome::Rejected);
    }
    ensure!(
        status.success(),
        "Root verifier exited unsuccessfully: {status}"
    );
    written.context("Root verifier input write failed")?;
    ensure!(
        stderr.is_empty(),
        "Successful root verifier emitted diagnostics"
    );
    let response: Response = serde_json::from_slice(&raw)?;
    ensure!(
        serde_json::to_vec(&response)? == raw,
        "Root verifier output is not canonical"
    );
    Ok(HelperOutcome::Verified(response))
}

#[cfg(all(test, unix))]
struct PrivateHelperDirectory(PathBuf);
#[cfg(all(test, unix))]
impl Drop for PrivateHelperDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[cfg(all(test, unix))]
struct ChildGuard(Child);
#[cfg(all(test, unix))]
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

pub(crate) fn check_receipt(
    stored: Option<&[u8]>,
    expected: Option<&PreparedRootGenesis>,
) -> Result<()> {
    match (stored, expected) {
        (None, None) => Ok(()),
        (Some(actual), Some(expected)) if actual == expected.bytes() => Ok(()),
        _ => bail!(
            "Root genesis authorization is missing or differs; explicit migration is required"
        ),
    }
}

#[cfg(test)]
#[path = "root_genesis_tests.rs"]
mod tests;
