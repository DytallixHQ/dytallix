//! Restart on a new release after a halt (E04 gap 18, `docs/architecture/restart-v1.md`).
//! The handover authority signs a switch of the active release at the halted
//! height; the adapter applies it before that block's transactions. It
//! changes no state but the handover state and adds one receipt.
use super::{decode, hash, valid_hex, Policy, Rejected, State, Verifier, SIGNATURE_BYTES};
use crate::emergency_freeze as emergency;
use anyhow::{ensure, Context as _, Result};
use serde::{Deserialize, Serialize};

pub const KIND: &str = "dytallix-release-restart-v1";
/// Inside the handover namespace, so handover history accounts for it.
pub const PREFIX: &str = "consensus:release-handover:v1:restart:";
const DOMAIN: &[u8] = b"DYTALLIX/RELEASE-RESTART/v1\0";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Payload {
    pub schema: u16,
    pub chain_id: String,
    pub genesis_sha256: String,
    pub policy_sha256: String,
    pub authority_epoch: u64,
    pub sequence: u64,
    pub source_release_sha512: String,
    pub target_release_sha512: String,
    /// The active state schema, which the restart preserves.
    pub state_schema: u16,
    pub parent_height: u64,
    pub parent_app_hash: String,
    pub halted_height: u64,
    /// Block H's engine hash when it was decided; absent when it never was.
    pub halted_block_hash: Option<String>,
    pub emergency_receipt_sha256: Option<String>,
    pub pending_admission_receipt_sha256: Option<String>,
    pub evidence_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authorization {
    pub kind: String,
    pub payload: Payload,
    pub signatures: Vec<emergency::ControlSignature>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: u16,
    pub policy_sha256: String,
    pub authorization: Authorization,
    pub previous_receipt_sha256: Option<String>,
}
impl Receipt {
    pub fn sha256(&self) -> Result<String> {
        Ok(hash(&serde_json::to_vec(self)?))
    }
    pub fn sequence(&self) -> u64 {
        self.authorization.payload.sequence
    }
    pub fn halted_height(&self) -> u64 {
        self.authorization.payload.halted_height
    }
}

/// The committed facts at the halt, from the adapter's own state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checkpoint {
    pub height: u64,
    pub app_hash: String,
    pub emergency_receipt_sha256: Option<String>,
}

/// A restart checked against the committed state, with its signatures
/// verified. Only `verify` and `replay` construct it.
#[derive(Clone, Debug)]
pub struct Verified {
    authorization: Authorization,
    state: State,
    receipt: Receipt,
}
impl Verified {
    pub fn target_release_sha512(&self) -> &str {
        &self.authorization.payload.target_release_sha512
    }
    pub fn halted_height(&self) -> u64 {
        self.authorization.payload.halted_height
    }
    /// Whether this restart applies to the block `(height, hash)`.
    pub fn applies_to(&self, height: u64, block_hash: Option<&str>) -> bool {
        let payload = &self.authorization.payload;
        height == payload.halted_height
            && payload
                .halted_block_hash
                .as_deref()
                .is_none_or(|expected| block_hash == Some(expected))
    }
    /// The handover state after the switch.
    pub fn state(&self) -> &State {
        &self.state
    }
    pub fn receipt(&self) -> &Receipt {
        &self.receipt
    }
}

pub fn artifact_bytes(payload: &Payload) -> Result<Vec<u8>> {
    let mut bytes = DOMAIN.to_vec();
    bytes.extend_from_slice(&serde_json::to_vec(payload)?);
    Ok(bytes)
}
pub fn key(sequence: u64) -> String {
    format!("{PREFIX}{sequence:020}")
}
pub fn encode_receipt(receipt: &Receipt) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(receipt)?)
}

pub fn decode_authorization(policy: &Policy, bytes: &[u8]) -> Result<Authorization> {
    policy.validate()?;
    ensure!(
        bytes.len() <= policy.max_control_bytes,
        "Restart authorization exceeds byte bound"
    );
    let authorization: Authorization = decode(bytes)?;
    ensure!(
        authorization.kind == KIND && authorization.signatures.len() <= policy.max_signatures,
        "Restart kind/signature bound"
    );
    Ok(authorization)
}
pub fn decode_receipt(policy: &Policy, bytes: &[u8]) -> Result<Receipt> {
    let receipt: Receipt = decode(bytes)?;
    ensure!(
        receipt.schema == 1 && receipt.policy_sha256 == policy.sha256()?,
        "Restart receipt policy differs"
    );
    decode_authorization(policy, &serde_json::to_vec(&receipt.authorization)?)?;
    if let Some(digest) = &receipt.previous_receipt_sha256 {
        valid_hex(digest, 32)?;
    }
    Ok(receipt)
}

/// Check a restart against the committed state and verify its signatures
/// with the root helper.
pub fn verify(
    policy: &Policy,
    state: &State,
    checkpoint: &Checkpoint,
    raw: &[u8],
    verifier: &dyn Verifier,
) -> Result<Verified> {
    let authorization = decode_authorization(policy, raw).map_err(|e| Rejected(e.to_string()))?;
    validate(policy, state, checkpoint, &authorization).map_err(|e| Rejected(e.to_string()))?;
    verify_signatures(policy, &authorization, verifier)?;
    transition(policy, state, authorization)
}

/// Replay a committed restart receipt at its block. Startup supplies the
/// real signature verifier.
pub fn replay(
    policy: &Policy,
    state: &State,
    checkpoint: &Checkpoint,
    receipt: &Receipt,
    verifier: Option<&dyn Verifier>,
) -> Result<Verified> {
    let authorization = decode_authorization(policy, &serde_json::to_vec(&receipt.authorization)?)?;
    validate(policy, state, checkpoint, &authorization)?;
    if let Some(verifier) = verifier {
        verify_signatures(policy, &authorization, verifier)?;
    }
    let verified = transition(policy, state, authorization)?;
    ensure!(
        &verified.receipt == receipt,
        "Restart receipt replay differs"
    );
    Ok(verified)
}

fn validate(
    policy: &Policy,
    state: &State,
    checkpoint: &Checkpoint,
    authorization: &Authorization,
) -> Result<()> {
    state.validate(policy)?;
    let payload = &authorization.payload;
    ensure!(
        payload.schema == 1
            && payload.chain_id == policy.chain_id
            && payload.genesis_sha256 == policy.genesis_sha256
            && payload.policy_sha256 == policy.sha256()?
            && payload.authority_epoch == policy.authority_epoch,
        "Restart identity/policy binding"
    );
    ensure!(
        payload.sequence == state.next_sequence && payload.sequence < u64::MAX,
        "Restart sequence mismatch/exhaustion"
    );
    valid_hex(&payload.source_release_sha512, 64)?;
    valid_hex(&payload.target_release_sha512, 64)?;
    ensure!(
        payload.source_release_sha512 == state.active_release_sha512
            && payload.target_release_sha512 != payload.source_release_sha512,
        "Restart release binding"
    );
    // Release only: the schema stays (P01, 28 September 2026).
    ensure!(
        payload.state_schema == state.active_schema,
        "Restart must preserve the active schema"
    );
    valid_hex(&payload.parent_app_hash, 32)?;
    ensure!(
        payload.parent_height == checkpoint.height
            && payload.parent_app_hash == checkpoint.app_hash
            && payload.parent_height.checked_add(1) == Some(payload.halted_height),
        "Restart checkpoint binding"
    );
    ensure!(
        state
            .last_control_height
            .is_none_or(|h| h < payload.halted_height),
        "Restart height already consumed"
    );
    if let Some(block) = &payload.halted_block_hash {
        valid_hex(block, 32)?;
    }
    for digest in [
        &payload.emergency_receipt_sha256,
        &payload.pending_admission_receipt_sha256,
    ]
    .into_iter()
    .flatten()
    {
        valid_hex(digest, 32)?;
    }
    valid_hex(&payload.evidence_sha256, 32)?;
    ensure!(
        payload.emergency_receipt_sha256 == checkpoint.emergency_receipt_sha256,
        "Restart emergency history differs"
    );
    ensure!(
        payload.pending_admission_receipt_sha256.as_ref()
            == state.pending.as_ref().map(|p| &p.admission_receipt_sha256),
        "Restart pending admission differs"
    );
    ensure!(
        authorization.signatures.len() >= policy.authority.threshold,
        "Restart signature threshold"
    );
    let mut previous: Option<&str> = None;
    for signature in &authorization.signatures {
        ensure!(
            previous.is_none_or(|p| p < signature.key_id.as_str()),
            "Restart signatures must be strictly sorted"
        );
        ensure!(
            policy
                .authority
                .keys
                .iter()
                .any(|key| key.key_id == signature.key_id),
            "Restart authority key missing"
        );
        valid_hex(&signature.signature_hex, SIGNATURE_BYTES)?;
        previous = Some(&signature.key_id);
    }
    Ok(())
}

fn verify_signatures(
    policy: &Policy,
    authorization: &Authorization,
    verifier: &dyn Verifier,
) -> Result<()> {
    let payload = &authorization.payload;
    let artifact = artifact_bytes(payload)?;
    let height = payload.halted_height;
    for signature in &authorization.signatures {
        let key = policy
            .authority
            .keys
            .iter()
            .find(|key| key.key_id == signature.key_id)
            .context("Restart trusted key absent")?;
        let valid = verifier.verify(
            &policy.chain_id,
            &hex::decode(&key.public_key_hex)?,
            payload.sequence,
            height,
            height,
            height,
            &artifact,
            &hex::decode(&signature.signature_hex)?,
        )?;
        ensure!(valid, Rejected("Restart signature rejected".into()));
    }
    Ok(())
}

fn transition(policy: &Policy, state: &State, authorization: Authorization) -> Result<Verified> {
    let receipt = Receipt {
        schema: 1,
        policy_sha256: policy.sha256()?,
        authorization: authorization.clone(),
        previous_receipt_sha256: state.last_receipt_sha256.clone(),
    };
    let digest = receipt.sha256()?;
    let payload = &authorization.payload;
    let mut next = state.clone();
    // A pending admission named the source release; the restart supersedes it.
    next.pending = None;
    next.active_release_sha512 = payload.target_release_sha512.clone();
    next.activation_receipt_sha256 = Some(digest.clone());
    next.next_sequence = next
        .next_sequence
        .checked_add(1)
        .context("Restart sequence overflow")?;
    next.last_receipt_sha256 = Some(digest);
    next.last_control_height = Some(payload.halted_height);
    next.validate(policy)?;
    Ok(Verified {
        authorization,
        state: next,
        receipt,
    })
}

#[cfg(test)]
#[path = "restart_tests.rs"]
mod tests;
