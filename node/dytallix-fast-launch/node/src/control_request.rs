//! Root control requests (E05; P01, 6 October 2026). A control (an emergency
//! freeze or resume, an upgrade, a release handover or a kit replacement) is
//! prepared online from the chain's application configuration and the
//! node's `/status`, signed offline by three custodian keys (`dytallix-root-sign
//! sign-control`), and assembled here into the exact transaction the node
//! admits. Preparation chooses nothing the node checks for itself: every
//! field comes from the configuration, the status or the operator's digests.
//! A restart on a fixed release after a halt (restart v1) is signed the same
//! way, but prepared and assembled on the stopped node (`restart_request`,
//! `assemble_restart`, used by `dytallix-state-check`).
//!
//! The keys that sign are those of the root authority epoch in force at the
//! next block (root kit replacement v1): the configuration's until a kit
//! replacement takes effect, then the epoch's that the status reports.
use crate::consensus_settlement::ConsensusConfig;
use crate::emergency_freeze as emergency;
use crate::kit_replacement as replacement;
use crate::release_handover as handover;
use crate::upgrade::{self, v2 as upgrade_v2};
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
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
    /// The root authority epoch in force at the next block and its keys;
    /// reported by nodes with the three schema 2 root policies.
    pub root_authority: Option<RootAuthorityStatus>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct RootAuthorityStatus {
    pub authority_epoch: u64,
    pub keys: replacement::Roles<emergency::AuthorityPolicy>,
    pub pending: Option<PendingEpoch>,
}
/// A kit replacement admitted and waiting for its effect height.
#[derive(Clone, Debug, Deserialize)]
pub struct PendingEpoch {
    pub authority_epoch: u64,
    pub effect_height: u64,
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
    /// One seat's keys replaced: the leaving key IDs and the new kit's
    /// public keys, one per role.
    KitReplacement {
        replaced: replacement::Roles<String>,
        keys: replacement::Roles<emergency::AuthorityKey>,
    },
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
            Self::KitReplacement { .. } => "kit-replacement",
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
/// the policy's validity length and within its anchor age. It ends before a
/// pending kit replacement's effect height, where the signing keys change.
fn window(
    anchor: u64,
    max_validity: u64,
    max_anchor_age: u64,
    blocks: Option<u64>,
    effect: Option<u64>,
) -> Result<(u64, u64)> {
    let limit = max_validity.min(max_anchor_age);
    let blocks = blocks.unwrap_or(limit);
    ensure!(
        (1..=limit).contains(&blocks),
        "The window must be 1 to {limit} blocks"
    );
    let first = anchor.checked_add(1).context("Height exhausted")?;
    let mut last = anchor.checked_add(blocks).context("Height exhausted")?;
    if let Some(effect) = effect.filter(|&effect| effect <= last) {
        ensure!(
            effect > first,
            "A kit replacement takes effect at height {effect}; prepare the control after it"
        );
        last = effect - 1;
    }
    Ok((first, last))
}

/// The root authority in force at the next block, as the status reports it;
/// `None` before kit replacement exists for this configuration, when the
/// configuration's keys sign.
fn in_force<'a>(
    config: &ConsensusConfig,
    status: &'a Status,
) -> Result<Option<&'a RootAuthorityStatus>> {
    let replaceable = replacement::Policies::of(
        config.emergency.as_ref(),
        config.upgrade.as_ref(),
        config.release_handover.as_ref(),
    )
    .is_some();
    match (&status.root_authority, replaceable) {
        (Some(current), true) => Ok(Some(current)),
        (None, false) => Ok(None),
        (None, true) => bail!(
            "The status view has no root authority epoch; save it again from a current node"
        ),
        (Some(_), false) => {
            bail!("The status reports a root authority this configuration does not have")
        }
    }
}
fn effect(current: Option<&RootAuthorityStatus>) -> Option<u64> {
    current
        .and_then(|current| current.pending.as_ref())
        .map(|pending| pending.effect_height)
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
        Operation::KitReplacement { .. } => {
            prepare_replacement(config, status, operation, anchor, blocks)
        }
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
    let current = in_force(config, status)?;
    let (first, last) = window(
        anchor.0,
        v2.max_validity_blocks,
        v2.max_anchor_age_blocks,
        blocks,
        effect(current),
    )?;
    let (action, incident, resume, authority, purpose) = match operation {
        Operation::Freeze { incident_sha256 } => {
            ensure!(!state.frozen, "The chain is already frozen");
            (
                emergency::Action::Freeze,
                incident_sha256,
                None,
                current.map_or(&policy.freeze_authority, |c| &c.keys.freeze),
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
                current.map_or(&policy.resume_authority, |c| &c.keys.resume),
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
            authority_epoch: current.map_or(v2.authority_epoch, |c| c.authority_epoch),
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
    let current = in_force(config, status)?;
    let authority = current.map_or(&policy.authority, |c| &c.keys.upgrade);
    let (first, last) = window(
        anchor.0,
        policy.max_validity_blocks,
        policy.max_anchor_age_blocks,
        blocks,
        effect(current),
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
        authority_epoch: current.map_or(policy.authority_epoch, |c| c.authority_epoch),
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
            threshold: authority.threshold,
            max_signatures: policy.max_signatures,
            keys: authority.keys.clone(),
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
    let current = in_force(config, status)?;
    let authority = current.map_or(&policy.authority, |c| &c.keys.upgrade);
    let (first, last) = window(
        anchor.0,
        v2.max_validity_blocks,
        v2.max_anchor_age_blocks,
        blocks,
        effect(current),
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
        authority_epoch: current.map_or(policy.authority_epoch, |c| c.authority_epoch),
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
            threshold: authority.threshold,
            max_signatures: policy.max_signatures,
            keys: authority.keys.clone(),
        },
    ))
}

/// A kit replacement (root kit replacement v1): the current upgrade keys
/// sign it, and the request also lists the new kit's keys, which sign the
/// same request as their proofs of possession.
fn prepare_replacement(
    config: &ConsensusConfig,
    status: &Status,
    operation: &Operation,
    anchor: (u64, &str),
    blocks: Option<u64>,
) -> Result<Request> {
    let Operation::KitReplacement { replaced, keys } = operation else {
        unreachable!()
    };
    let policy = upgrade_policy(config)?;
    let current = in_force(config, status)?.context(
        "A kit replacement needs the emergency, upgrade and handover policies at schema 2",
    )?;
    if let Some(pending) = &current.pending {
        bail!(
            "A kit replacement is pending; it takes effect at height {}",
            pending.effect_height
        );
    }
    ensure!(
        !status
            .emergency_control
            .as_ref()
            .context("The status has no emergency control state")?
            .frozen,
        "The chain is frozen; no kit replacement is admitted until it resumes"
    );
    let sets = current.keys.each();
    for ((set, id), role) in sets
        .into_iter()
        .zip(replaced.each())
        .zip(["upgrade", "freeze", "resume"])
    {
        ensure!(
            set.keys.iter().any(|key| &key.key_id == id),
            "Key {id} is not a current {role} key"
        );
    }
    for key in keys.each() {
        lower_hex(&key.public_key_hex, 64, "A new public key")?;
        ensure!(
            key.key_id == sha256_hex(&hex::decode(&key.public_key_hex)?),
            "Key {} is not its public key's SHA-256",
            key.key_id
        );
        ensure!(
            sets.iter()
                .all(|set| set.keys.iter().all(|held| held.key_id != key.key_id)),
            "Key {} is already a root key",
            key.key_id
        );
    }
    let (first, last) = window(
        anchor.0,
        policy.max_validity_blocks,
        policy.max_anchor_age_blocks,
        blocks,
        None,
    )?;
    let payload = replacement::Payload {
        schema: 1,
        chain_id: policy.chain_id.clone(),
        genesis_sha256: policy.genesis_sha256.clone(),
        policy_sha256: policy.sha256()?,
        authority_epoch: current.authority_epoch,
        sequence: current
            .authority_epoch
            .checked_add(1)
            .context("Authority epoch exhausted")?,
        anchor_height: anchor.0,
        anchor_app_hash: anchor.1.into(),
        not_before_height: first,
        not_after_height: last,
        replaced: replaced.clone(),
        keys: keys.clone(),
    };
    let artifact = replacement::artifact_bytes(&payload)?;
    let mut signers = current.keys.upgrade.keys.clone();
    signers.extend(keys.each().into_iter().cloned());
    Ok(request(
        operation,
        replacement::CONTROL_KIND,
        anchor,
        serde_json::to_value(&payload)?,
        artifact,
        (&policy.chain_id, "upgrade", payload.sequence, first, last),
        Authority {
            purpose: "upgrade".into(),
            threshold: current.keys.upgrade.threshold,
            max_signatures: policy.max_signatures,
            keys: signers,
        },
    ))
}

/// Assembles the signed control: the request's payload (re-encoded and
/// checked against the signed artifact) with the signatures sorted by key
/// ID. Only keys of the authority in force for the request's purpose count:
/// the configuration's, or after a kit replacement the epoch's that `status`
/// reports, and the request must name that epoch. A kit replacement also
/// takes each new key's proof. The node's own decoder checks the result; the
/// signatures themselves are verified by the node (dry-run with `dytallix
/// control check` before submitting).
pub fn assemble(
    config: &ConsensusConfig,
    status: &Status,
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
    let current = in_force(config, status)?;
    let upgrade_keys = |configured: &emergency::AuthorityPolicy| {
        current.map_or_else(|| configured.clone(), |c| c.keys.upgrade.clone())
    };
    let signed = |matches: bool| {
        ensure!(matches, "The payload is not the signed artifact");
        Ok(())
    };
    let (authority, max_signatures, epoch, proofs) = match request.kind.as_str() {
        emergency::CONTROL_KIND_V2 => {
            let policy = config
                .emergency
                .as_ref()
                .context("The configuration has no emergency policy")?;
            let payload: emergency::Payload = serde_json::from_value(request.payload.clone())?;
            signed(emergency::artifact_bytes(&payload)? == artifact)?;
            let authority = match (payload.action, current) {
                (emergency::Action::Freeze, Some(c)) => c.keys.freeze.clone(),
                (emergency::Action::Resume, Some(c)) => c.keys.resume.clone(),
                (emergency::Action::Freeze, None) => policy.freeze_authority.clone(),
                (emergency::Action::Resume, None) => policy.resume_authority.clone(),
            };
            let epoch = payload.v2.as_ref().map(|v2| v2.authority_epoch);
            (authority, policy.max_signatures, epoch, None)
        }
        upgrade_v2::CONTROL_KIND => {
            let policy = upgrade_policy(config)?;
            let payload: upgrade_v2::Payload = serde_json::from_value(request.payload.clone())?;
            signed(upgrade_v2::artifact_bytes(&payload)? == artifact)?;
            let epoch = Some(payload.authority_epoch);
            (upgrade_keys(&policy.authority), policy.max_signatures, epoch, None)
        }
        handover::CONTROL_KIND_V2 => {
            let policy = config
                .release_handover
                .as_ref()
                .context("The configuration has no release handover policy")?;
            let payload: handover::Payload = serde_json::from_value(request.payload.clone())?;
            signed(handover::artifact_bytes(&payload)? == artifact)?;
            let epoch = Some(payload.authority_epoch);
            (upgrade_keys(&policy.authority), policy.max_signatures, epoch, None)
        }
        replacement::CONTROL_KIND => {
            let policy = upgrade_policy(config)?;
            let payload: replacement::Payload = serde_json::from_value(request.payload.clone())?;
            signed(replacement::artifact_bytes(&payload)? == artifact)?;
            let current = current.context("A kit replacement is assembled with a root authority status")?;
            let epoch = Some(payload.authority_epoch);
            (current.keys.upgrade.clone(), policy.max_signatures, epoch, Some(payload.keys))
        }
        handover::restart::KIND => {
            bail!("A restart is assembled on the stopped node: dytallix-state-check --restart-assemble")
        }
        other => bail!("Unsupported control kind {other}"),
    };
    if let (Some(current), Some(epoch)) = (current, epoch) {
        ensure!(
            epoch == current.authority_epoch,
            "The request names authority epoch {epoch}, but epoch {} is in force; prepare it again",
            current.authority_epoch
        );
    }
    let provers: BTreeSet<&str> = proofs
        .iter()
        .flat_map(|keys| keys.each().map(|key| key.key_id.as_str()))
        .collect();
    let (signatures, proved) = choose(request, signatures, &authority, max_signatures, &provers)?;
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
        handover::CONTROL_KIND_V2 => {
            let control = handover::Control {
                kind: request.kind.clone(),
                payload: serde_json::from_value(request.payload.clone())?,
                signatures,
            };
            let bytes = serde_json::to_vec(&control)?;
            handover::decode_control(config.release_handover.as_ref().unwrap(), &bytes)?;
            bytes
        }
        _ => {
            // A kit replacement carries its signatures in base64.
            let base64 = |signature_hex: &str| -> Result<String> {
                Ok(B64.encode(hex::decode(signature_hex)?))
            };
            let keys = proofs.context("Kit replacement keys missing")?;
            let proof = |key: &emergency::AuthorityKey| base64(&proved[&key.key_id]);
            let control = replacement::Control {
                kind: request.kind.clone(),
                payload: serde_json::from_value(request.payload.clone())?,
                signatures: signatures
                    .iter()
                    .map(|s| {
                        Ok(replacement::Signature {
                            key_id: s.key_id.clone(),
                            signature_base64: base64(&s.signature_hex)?,
                        })
                    })
                    .collect::<Result<_>>()?,
                proofs: replacement::Roles {
                    upgrade: proof(&keys.upgrade)?,
                    freeze: proof(&keys.freeze)?,
                    resume: proof(&keys.resume)?,
                },
            };
            let bytes = serde_json::to_vec(&control)?;
            let policies = replacement::Policies::of(
                config.emergency.as_ref(),
                config.upgrade.as_ref(),
                config.release_handover.as_ref(),
            )
            .context("Kit replacement policies missing")?;
            replacement::decode_control(&policies, &bytes)?;
            bytes
        }
    };
    Ok(bytes)
}

/// The signatures of the authority's keys, sorted by key ID, between the
/// threshold and the maximum, and each new key's proof (a kit replacement).
fn choose(
    request: &Request,
    signatures: &[SignatureRecord],
    authority: &emergency::AuthorityPolicy,
    max_signatures: usize,
    provers: &BTreeSet<&str>,
) -> Result<(
    Vec<emergency::ControlSignature>,
    std::collections::BTreeMap<String, String>,
)> {
    let allowed: BTreeSet<&str> = authority.keys.iter().map(|k| k.key_id.as_str()).collect();
    let mut chosen = std::collections::BTreeMap::new();
    let mut proved = std::collections::BTreeMap::new();
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
        let target = if allowed.contains(record.key_id.as_str()) {
            &mut chosen
        } else if provers.contains(record.key_id.as_str()) {
            &mut proved
        } else {
            bail!("Key {} is not in this control's authority", record.key_id)
        };
        lower_hex(&record.signature_hex, SIGNATURE_HEX / 2, "A signature")?;
        ensure!(
            target
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
    ensure!(
        proved.len() == provers.len(),
        "{} of the {} new keys signed; each new key signs the request as its proof",
        proved.len(),
        provers.len()
    );
    let signatures = chosen
        .into_iter()
        .map(|(key_id, signature_hex)| emergency::ControlSignature {
            key_id,
            signature_hex,
        })
        .collect();
    Ok((signatures, proved))
}

/// The signing request for a restart on a fixed release (restart v1): the
/// payload `dytallix-state-check` built from the stopped node, under the
/// handover policy in force there. The upgrade keys sign it under the
/// `upgrade` action, with the window the halted height alone.
pub fn restart_request(
    policy: &handover::Policy,
    payload: &handover::restart::Payload,
) -> Result<Request> {
    let artifact = handover::restart::artifact_bytes(payload)?;
    let height = payload.halted_height;
    Ok(Request {
        schema: REQUEST_SCHEMA.into(),
        operation: "restart".into(),
        kind: handover::restart::KIND.into(),
        anchor_height: payload.parent_height,
        anchor_app_hash: payload.parent_app_hash.clone(),
        payload: serde_json::to_value(payload)?,
        envelope: Envelope {
            chain_id: payload.chain_id.clone(),
            action: "upgrade".into(),
            sequence: payload.sequence,
            not_before_height: height,
            not_after_height: height,
            artifact_sha512: hex::encode(Sha512::digest(&artifact)),
        },
        artifact_hex: hex::encode(artifact),
        authority: Authority {
            purpose: "upgrade".into(),
            threshold: policy.authority.threshold,
            max_signatures: policy.max_signatures,
            keys: policy.authority.keys.clone(),
        },
    })
}

/// Assembles a restart authorization from its request and signatures. Only
/// keys of `policy`, the handover policy in force on the stopped node
/// (`consensus_settlement::restart_policy`), count; the payload must be the
/// signed artifact and name that policy and epoch. The node's decoder checks
/// the result; the application verifies the signatures with the root helper
/// when it starts with the authorization.
pub fn assemble_restart(
    policy: &handover::Policy,
    request: &Request,
    signatures: &[SignatureRecord],
) -> Result<Vec<u8>> {
    ensure!(
        request.schema == REQUEST_SCHEMA && request.kind == handover::restart::KIND,
        "Not a restart signing request"
    );
    let artifact = hex::decode(&request.artifact_hex).context("Request artifact hex")?;
    ensure!(
        hex::encode(Sha512::digest(&artifact)) == request.envelope.artifact_sha512,
        "The request's artifact digest differs from its artifact"
    );
    let payload: handover::restart::Payload = serde_json::from_value(request.payload.clone())?;
    ensure!(
        handover::restart::artifact_bytes(&payload)? == artifact,
        "The payload is not the signed artifact"
    );
    ensure!(
        payload.policy_sha256 == policy.sha256()? && payload.authority_epoch == policy.authority_epoch,
        "The request names authority epoch {}, but epoch {} is in force on this node; build it again",
        payload.authority_epoch,
        policy.authority_epoch
    );
    let (signatures, _) = choose(
        request,
        signatures,
        &policy.authority,
        policy.max_signatures,
        &BTreeSet::new(),
    )?;
    let bytes = serde_json::to_vec(&handover::restart::Authorization {
        kind: handover::restart::KIND.into(),
        payload,
        signatures,
    })?;
    handover::restart::decode_authorization(policy, &bytes)?;
    Ok(bytes)
}

#[cfg(test)]
#[path = "control_request_tests.rs"]
mod tests;
