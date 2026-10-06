//! Root control requests (E05; P01, 6 October 2026). A control (an emergency
//! freeze or resume, an upgrade or a release handover) is prepared online
//! from the chain's application configuration and the node's `/status`,
//! signed offline by three custodian keys (`dytallix-root-sign
//! sign-control`), and assembled here into the exact transaction the node
//! admits. Preparation chooses nothing the node checks for itself: every
//! field comes from the configuration, the status or the operator's digests.
use crate::consensus_settlement::ConsensusConfig;
use crate::emergency_freeze as emergency;
use crate::release_handover as handover;
use crate::upgrade::{self, v2 as upgrade_v2};
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};
use std::collections::BTreeSet;

pub const REQUEST_SCHEMA: &str = "dytallix.control-request.v1";
pub const SIGNATURE_SCHEMA: &str = "dytallix.control-signature.v1";
const SIGNATURE_HEX: usize = 2 * 29_792;

/// The root envelope every custodian signs (root-authorization
/// `Envelope`): the artifact's SHA-512 under the chain, action, sequence and
/// window. The node maps freeze and resume to `emergency`, and upgrades and
/// handovers to `upgrade`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub chain_id: String,
    pub action: String,
    pub sequence: u64,
    pub not_before_height: u64,
    pub not_after_height: u64,
    pub artifact_sha512: String,
}

/// The keys that may sign, for the signer's information. Assembly checks the
/// signatures against the configuration, never against this copy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authority {
    pub purpose: String,
    pub threshold: usize,
    pub max_signatures: usize,
    pub keys: Vec<emergency::AuthorityKey>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub operation: String,
    pub kind: String,
    pub anchor_height: u64,
    pub anchor_app_hash: String,
    /// The node's payload for `kind`; its canonical bytes follow the domain
    /// in `artifact_hex`.
    pub payload: serde_json::Value,
    pub artifact_hex: String,
    pub envelope: Envelope,
    pub authority: Authority,
}

/// One custodian's signature, as `dytallix-root-sign sign-control` writes it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignatureRecord {
    pub schema: String,
    pub key_id: String,
    pub artifact_sha512: String,
    pub sequence: u64,
    pub signature_hex: String,
}

/// The parts of the node's `/status` view that preparation reads.
#[derive(Clone, Debug, Deserialize)]
pub struct Status {
    pub height: u64,
    pub app_hash: String,
    pub emergency_control: Option<EmergencyStatus>,
    pub release_handover: Option<HandoverStatus>,
    pub upgrade: Option<UpgradeStatus>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct EmergencyStatus {
    pub frozen: bool,
    pub next_sequence: u64,
    pub last_receipt_sha256: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct HandoverStatus {
    pub active_release_sha512: String,
    pub active_schema: u16,
    pub next_sequence: u64,
    pub pending: Option<handover::PendingPlan>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct UpgradeStatus {
    pub active_schema: u16,
    pub active_release_sha512: String,
    pub next_sequence: u64,
    pub pending: Option<upgrade_v2::PendingPlan>,
}

/// A handover's transition: the receipt index migration (paired with the
/// upgrade activation) or a release change that keeps the state schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionChoice {
    ReceiptIndexV1,
    SchemaPreserving,
}

/// What to prepare. Digests are lowercase SHA-256 hex of the operator's
/// public documents (incident, readiness evidence, authorization, evidence).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operation {
    Freeze {
        incident_sha256: String,
    },
    Resume {
        incident_sha256: String,
        readiness_evidence_sha256: String,
    },
    UpgradeAdmit {
        authorization_sha256: String,
    },
    UpgradeActivate {
        evidence_sha256: String,
    },
    UpgradeCancel,
    HandoverAdmit {
        target_release_sha512: String,
        transition: TransitionChoice,
        authorization_sha256: String,
    },
    HandoverActivate {
        evidence_sha256: String,
        /// SHA-256 of the complete signed upgrade Activate control, for the
        /// receipt index transition.
        upgrade_activation_sha256: Option<String>,
    },
    HandoverCancel,
}
impl Operation {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Freeze { .. } => "freeze",
            Self::Resume { .. } => "resume",
            Self::UpgradeAdmit { .. } => "upgrade-admit",
            Self::UpgradeActivate { .. } => "upgrade-activate",
            Self::UpgradeCancel => "upgrade-cancel",
            Self::HandoverAdmit { .. } => "handover-admit",
            Self::HandoverActivate { .. } => "handover-activate",
            Self::HandoverCancel => "handover-cancel",
        }
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn lower_hex(value: &str, bytes: usize, what: &str) -> Result<()> {
    ensure!(
        value.len() == 2 * bytes
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "{what} must be {} lowercase hex digits",
        2 * bytes
    );
    Ok(())
}

/// The window the signatures cover: from the block after the anchor, at most
/// the policy's validity length and within its anchor age.
fn window(
    anchor: u64,
    max_validity: u64,
    max_anchor_age: u64,
    blocks: Option<u64>,
) -> Result<(u64, u64)> {
    let limit = max_validity.min(max_anchor_age);
    let blocks = blocks.unwrap_or(limit);
    ensure!(
        (1..=limit).contains(&blocks),
        "The window must be 1 to {limit} blocks"
    );
    let first = anchor.checked_add(1).context("Height exhausted")?;
    let last = anchor.checked_add(blocks).context("Height exhausted")?;
    Ok((first, last))
}

fn request(
    operation: &Operation,
    kind: &str,
    anchor: (u64, &str),
    payload: serde_json::Value,
    artifact: Vec<u8>,
    envelope: (&str, &str, u64, u64, u64),
    authority: Authority,
) -> Request {
    let (chain_id, action, sequence, first, last) = envelope;
    Request {
        schema: REQUEST_SCHEMA.into(),
        operation: operation.name().into(),
        kind: kind.into(),
        anchor_height: anchor.0,
        anchor_app_hash: anchor.1.into(),
        payload,
        envelope: Envelope {
            chain_id: chain_id.into(),
            action: action.into(),
            sequence,
            not_before_height: first,
            not_after_height: last,
            artifact_sha512: hex::encode(Sha512::digest(&artifact)),
        },
        artifact_hex: hex::encode(artifact),
        authority,
    }
}

/// Prepares a signing request. `blocks` narrows the window; by default it is
/// as long as the policy allows (P01, 6 October 2026: two days).
pub fn prepare(
    config: &ConsensusConfig,
    status: &Status,
    operation: &Operation,
    blocks: Option<u64>,
) -> Result<Request> {
    lower_hex(&status.app_hash, 32, "The status app hash")?;
    let anchor = (status.height, status.app_hash.as_str());
    match operation {
        Operation::Freeze { .. } | Operation::Resume { .. } => {
            prepare_emergency(config, status, operation, anchor, blocks)
        }
        Operation::UpgradeAdmit { .. }
        | Operation::UpgradeActivate { .. }
        | Operation::UpgradeCancel => prepare_upgrade(config, status, operation, anchor, blocks),
        _ => prepare_handover(config, status, operation, anchor, blocks),
    }
}

fn prepare_emergency(
    config: &ConsensusConfig,
    status: &Status,
    operation: &Operation,
    anchor: (u64, &str),
    blocks: Option<u64>,
) -> Result<Request> {
    let policy = config
        .emergency
        .as_ref()
        .context("The configuration has no emergency policy")?;
    let v2 = policy
        .v2
        .as_ref()
        .context("Controls are prepared for emergency schema 2 only")?;
    let state = status
        .emergency_control
        .as_ref()
        .context("The status has no emergency control state")?;
    let release = status.release_handover.as_ref().map_or_else(
        || policy.release_sha512.clone(),
        |h| h.active_release_sha512.clone(),
    );
    let (first, last) = window(
        anchor.0,
        v2.max_validity_blocks,
        v2.max_anchor_age_blocks,
        blocks,
    )?;
    let (action, incident, resume, authority, purpose) = match operation {
        Operation::Freeze { incident_sha256 } => {
            ensure!(!state.frozen, "The chain is already frozen");
            (
                emergency::Action::Freeze,
                incident_sha256,
                None,
                &policy.freeze_authority,
                "freeze",
            )
        }
        Operation::Resume {
            incident_sha256,
            readiness_evidence_sha256,
        } => {
            ensure!(state.frozen, "The chain is not frozen");
            lower_hex(
                readiness_evidence_sha256,
                32,
                "The readiness evidence digest",
            )?;
            let freeze = state
                .last_receipt_sha256
                .clone()
                .context("A frozen chain reports its freeze receipt")?;
            (
                emergency::Action::Resume,
                incident_sha256,
                Some(emergency::ResumeBinding {
                    freeze_receipt_sha256: freeze,
                    // The resume restores the finalized anchor it names.
                    restored_state_sha256: anchor.1.into(),
                    readiness_evidence_sha256: readiness_evidence_sha256.clone(),
                }),
                &policy.resume_authority,
                "resume",
            )
        }
        _ => unreachable!(),
    };
    lower_hex(incident, 32, "The incident digest")?;
    let payload = emergency::Payload {
        schema: 2,
        chain_id: policy.chain_id.clone(),
        release_sha512: release,
        action,
        sequence: state.next_sequence,
        parent_height: anchor.0,
        parent_app_hash: anchor.1.into(),
        target_height: first,
        v2: Some(emergency::PayloadV2 {
            genesis_sha256: v2.genesis_sha256.clone(),
            authority_epoch: v2.authority_epoch,
            policy_sha256: policy.sha256()?,
            not_before_height: first,
            not_after_height: last,
            incident_sha256: incident.clone(),
            resume,
        }),
    };
    let artifact = emergency::artifact_bytes(&payload)?;
    Ok(request(
        operation,
        emergency::CONTROL_KIND_V2,
        anchor,
        serde_json::to_value(&payload)?,
        artifact,
        (&policy.chain_id, "emergency", payload.sequence, first, last),
        Authority {
            purpose: purpose.into(),
            threshold: authority.threshold,
            max_signatures: policy.max_signatures,
            keys: authority.keys.clone(),
        },
    ))
}

fn upgrade_policy(config: &ConsensusConfig) -> Result<&upgrade_v2::Policy> {
    match config
        .upgrade
        .as_ref()
        .context("The configuration has no upgrade policy")?
    {
        upgrade::Policy::V2(policy) => Ok(policy),
        upgrade::Policy::V1(_) => bail!("Controls are prepared for upgrade schema 2 only"),
    }
}

fn notice_passed(admitted: u64, notice: u64, first: u64) -> Result<()> {
    let earliest = admitted.checked_add(notice).context("Height exhausted")?;
    ensure!(
        first >= earliest,
        "The notice period ends at height {earliest}; prepare the activation after it"
    );
    Ok(())
}

fn prepare_upgrade(
    config: &ConsensusConfig,
    status: &Status,
    operation: &Operation,
    anchor: (u64, &str),
    blocks: Option<u64>,
) -> Result<Request> {
    let policy = upgrade_policy(config)?;
    let state = status
        .upgrade
        .as_ref()
        .context("The status has no upgrade state")?;
    let (first, last) = window(
        anchor.0,
        policy.max_validity_blocks,
        policy.max_anchor_age_blocks,
        blocks,
    )?;
    let action = match operation {
        Operation::UpgradeAdmit {
            authorization_sha256,
        } => {
            ensure!(
                state.pending.is_none() && state.active_schema == 0,
                "An upgrade is pending or the migration already ran"
            );
            lower_hex(authorization_sha256, 32, "The authorization digest")?;
            upgrade_v2::Action::Admit {
                plan: upgrade_v2::MigrationPlan {
                    target_release_sha512: state.active_release_sha512.clone(),
                    migration_id: upgrade_v2::MIGRATION_ID.into(),
                    migration_sha256: upgrade_v2::migration_sha256(),
                    source_schema: 0,
                    target_schema: 1,
                    bounds: policy.migration_bounds.clone(),
                    authorization_sha256: authorization_sha256.clone(),
                },
            }
        }
        Operation::UpgradeActivate { evidence_sha256 } => {
            let pending = state.pending.as_ref().context("No upgrade is pending")?;
            notice_passed(pending.admitted_height, policy.min_notice_blocks, first)?;
            lower_hex(evidence_sha256, 32, "The evidence digest")?;
            upgrade_v2::Action::Activate {
                plan: pending.plan.clone(),
                admission_receipt_sha256: pending.admission_receipt_sha256.clone(),
                emergency_receipt_sha256: emergency_receipt(status),
                evidence_sha256: evidence_sha256.clone(),
            }
        }
        Operation::UpgradeCancel => {
            let pending = state.pending.as_ref().context("No upgrade is pending")?;
            upgrade_v2::Action::Cancel {
                plan_sha256: pending.plan.sha256()?,
                admission_receipt_sha256: pending.admission_receipt_sha256.clone(),
            }
        }
        _ => unreachable!(),
    };
    let payload = upgrade_v2::Payload {
        schema: 2,
        chain_id: policy.chain_id.clone(),
        genesis_sha256: policy.genesis_sha256.clone(),
        policy_sha256: policy.sha256()?,
        source_release_sha512: state.active_release_sha512.clone(),
        authority_epoch: policy.authority_epoch,
        sequence: state.next_sequence,
        anchor_height: anchor.0,
        anchor_app_hash: anchor.1.into(),
        not_before_height: first,
        not_after_height: last,
        action,
    };
    let artifact = upgrade_v2::artifact_bytes(&payload)?;
    Ok(request(
        operation,
        upgrade_v2::CONTROL_KIND,
        anchor,
        serde_json::to_value(&payload)?,
        artifact,
        (&policy.chain_id, "upgrade", payload.sequence, first, last),
        Authority {
            purpose: "upgrade".into(),
            threshold: policy.authority.threshold,
            max_signatures: policy.max_signatures,
            keys: policy.authority.keys.clone(),
        },
    ))
}

/// Activations bind the latest emergency receipt, or none.
fn emergency_receipt(status: &Status) -> Option<String> {
    status
        .emergency_control
        .as_ref()
        .and_then(|e| e.last_receipt_sha256.clone())
}

fn prepare_handover(
    config: &ConsensusConfig,
    status: &Status,
    operation: &Operation,
    anchor: (u64, &str),
    blocks: Option<u64>,
) -> Result<Request> {
    let policy = config
        .release_handover
        .as_ref()
        .context("The configuration has no release handover policy")?;
    let v2 = policy
        .v2
        .as_ref()
        .context("Controls are prepared for handover schema 2 only")?;
    let state = status
        .release_handover
        .as_ref()
        .context("The status has no release handover state")?;
    let (first, last) = window(
        anchor.0,
        v2.max_validity_blocks,
        v2.max_anchor_age_blocks,
        blocks,
    )?;
    let action = match operation {
        Operation::HandoverAdmit {
            target_release_sha512,
            transition,
            authorization_sha256,
        } => {
            ensure!(state.pending.is_none(), "A handover is pending");
            lower_hex(target_release_sha512, 64, "The target release digest")?;
            lower_hex(authorization_sha256, 32, "The authorization digest")?;
            handover::Action::Admit {
                plan: handover::ReleasePlan {
                    target_release_sha512: target_release_sha512.clone(),
                    transition: match transition {
                        TransitionChoice::ReceiptIndexV1 => handover::Transition::ReceiptIndexV1 {
                            migration_sha256: upgrade_v2::migration_sha256(),
                        },
                        TransitionChoice::SchemaPreserving => {
                            handover::Transition::SchemaPreserving {
                                schema: state.active_schema,
                            }
                        }
                    },
                    authorization_sha256: authorization_sha256.clone(),
                },
            }
        }
        Operation::HandoverActivate {
            evidence_sha256,
            upgrade_activation_sha256,
        } => {
            let pending = state.pending.as_ref().context("No handover is pending")?;
            notice_passed(pending.admitted_height, v2.min_notice_blocks, first)?;
            lower_hex(evidence_sha256, 32, "The evidence digest")?;
            let paired = matches!(
                pending.plan.transition,
                handover::Transition::ReceiptIndexV1 { .. }
            );
            ensure!(
                paired == upgrade_activation_sha256.is_some(),
                "The receipt index handover binds its signed upgrade activation; others bind none"
            );
            if let Some(digest) = upgrade_activation_sha256 {
                lower_hex(digest, 32, "The upgrade activation digest")?;
            }
            handover::Action::Activate {
                plan: pending.plan.clone(),
                admission_receipt_sha256: pending.admission_receipt_sha256.clone(),
                emergency_receipt_sha256: emergency_receipt(status),
                evidence_sha256: evidence_sha256.clone(),
                upgrade_activation_sha256: upgrade_activation_sha256.clone(),
            }
        }
        Operation::HandoverCancel => {
            let pending = state.pending.as_ref().context("No handover is pending")?;
            handover::Action::Cancel {
                plan_sha256: pending.plan.sha256()?,
                admission_receipt_sha256: pending.admission_receipt_sha256.clone(),
            }
        }
        _ => unreachable!(),
    };
    let payload = handover::Payload {
        schema: 2,
        chain_id: policy.chain_id.clone(),
        genesis_sha256: policy.genesis_sha256.clone(),
        policy_sha256: policy.sha256()?,
        source_release_sha512: state.active_release_sha512.clone(),
        authority_epoch: policy.authority_epoch,
        sequence: state.next_sequence,
        parent_height: anchor.0,
        parent_app_hash: anchor.1.into(),
        target_height: first,
        action,
        v2: Some(handover::PayloadV2 {
            not_before_height: first,
            not_after_height: last,
        }),
    };
    let artifact = handover::artifact_bytes(&payload)?;
    Ok(request(
        operation,
        handover::CONTROL_KIND_V2,
        anchor,
        serde_json::to_value(&payload)?,
        artifact,
        (&policy.chain_id, "upgrade", payload.sequence, first, last),
        Authority {
            purpose: "upgrade".into(),
            threshold: policy.authority.threshold,
            max_signatures: policy.max_signatures,
            keys: policy.authority.keys.clone(),
        },
    ))
}

/// Assembles the signed control: the request's payload (re-encoded and
/// checked against the signed artifact) with the signatures sorted by key
/// ID. Only keys of the configuration's authority for the request's purpose
/// count. The node's own decoder checks the result; the signatures
/// themselves are verified by the node (dry-run with `dytallix control
/// check` before submitting).
pub fn assemble(
    config: &ConsensusConfig,
    request: &Request,
    signatures: &[SignatureRecord],
) -> Result<Vec<u8>> {
    ensure!(
        request.schema == REQUEST_SCHEMA,
        "Unsupported request schema"
    );
    let artifact = hex::decode(&request.artifact_hex).context("Request artifact hex")?;
    ensure!(
        hex::encode(Sha512::digest(&artifact)) == request.envelope.artifact_sha512,
        "The request's artifact digest differs from its artifact"
    );
    let (authority, max_signatures) = match request.kind.as_str() {
        emergency::CONTROL_KIND_V2 => {
            let policy = config
                .emergency
                .as_ref()
                .context("The configuration has no emergency policy")?;
            let payload: emergency::Payload = serde_json::from_value(request.payload.clone())?;
            ensure!(
                emergency::artifact_bytes(&payload)? == artifact,
                "The payload is not the signed artifact"
            );
            let authority = match payload.action {
                emergency::Action::Freeze => &policy.freeze_authority,
                emergency::Action::Resume => &policy.resume_authority,
            };
            (authority, policy.max_signatures)
        }
        upgrade_v2::CONTROL_KIND => {
            let policy = upgrade_policy(config)?;
            let payload: upgrade_v2::Payload = serde_json::from_value(request.payload.clone())?;
            ensure!(
                upgrade_v2::artifact_bytes(&payload)? == artifact,
                "The payload is not the signed artifact"
            );
            (&policy.authority, policy.max_signatures)
        }
        handover::CONTROL_KIND_V2 => {
            let policy = config
                .release_handover
                .as_ref()
                .context("The configuration has no release handover policy")?;
            let payload: handover::Payload = serde_json::from_value(request.payload.clone())?;
            ensure!(
                handover::artifact_bytes(&payload)? == artifact,
                "The payload is not the signed artifact"
            );
            (&policy.authority, policy.max_signatures)
        }
        other => bail!("Unsupported control kind {other}"),
    };
    let allowed: BTreeSet<&str> = authority.keys.iter().map(|k| k.key_id.as_str()).collect();
    let mut chosen = std::collections::BTreeMap::new();
    for record in signatures {
        ensure!(
            record.schema == SIGNATURE_SCHEMA,
            "Unsupported signature schema"
        );
        ensure!(
            record.artifact_sha512 == request.envelope.artifact_sha512
                && record.sequence == request.envelope.sequence,
            "Signature {} is for another control",
            record.key_id
        );
        ensure!(
            allowed.contains(record.key_id.as_str()),
            "Key {} is not in this control's authority",
            record.key_id
        );
        lower_hex(&record.signature_hex, SIGNATURE_HEX / 2, "A signature")?;
        ensure!(
            chosen
                .insert(record.key_id.clone(), record.signature_hex.clone())
                .is_none(),
            "Key {} signed twice",
            record.key_id
        );
    }
    ensure!(
        chosen.len() >= authority.threshold && chosen.len() <= max_signatures,
        "{} signatures given; this control needs {} to {}",
        chosen.len(),
        authority.threshold,
        max_signatures
    );
    let signatures: Vec<emergency::ControlSignature> = chosen
        .into_iter()
        .map(|(key_id, signature_hex)| emergency::ControlSignature {
            key_id,
            signature_hex,
        })
        .collect();
    // Re-encode through the node's types, then let its decoder refuse anything
    // it would not admit as a control.
    let bytes = match request.kind.as_str() {
        emergency::CONTROL_KIND_V2 => {
            let control = emergency::Control {
                kind: request.kind.clone(),
                payload: serde_json::from_value(request.payload.clone())?,
                signatures,
            };
            let bytes = serde_json::to_vec(&control)?;
            emergency::decode_control(config.emergency.as_ref().unwrap(), &bytes)?;
            bytes
        }
        upgrade_v2::CONTROL_KIND => {
            let control = upgrade_v2::Control {
                kind: request.kind.clone(),
                payload: serde_json::from_value(request.payload.clone())?,
                signatures,
            };
            let bytes = serde_json::to_vec(&control)?;
            upgrade_v2::decode_control(upgrade_policy(config)?, &bytes)?;
            bytes
        }
        _ => {
            let control = handover::Control {
                kind: request.kind.clone(),
                payload: serde_json::from_value(request.payload.clone())?,
                signatures,
            };
            let bytes = serde_json::to_vec(&control)?;
            handover::decode_control(config.release_handover.as_ref().unwrap(), &bytes)?;
            bytes
        }
    };
    Ok(bytes)
}

#[cfg(test)]
#[path = "control_request_tests.rs"]
mod tests;
