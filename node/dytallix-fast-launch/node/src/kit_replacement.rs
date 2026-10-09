//! The root kit replacement control (root kit replacement v1, K2b; P01, 8
//! October 2026, `docs/architecture/root-kit-replacement-v1.md`).
//!
//! Three of the five current upgrade keys replace one seat's upgrade, freeze
//! and resume keys, and the three new keys sign the same control as their
//! proofs of possession. Admitted at height A, the new authority epoch is in
//! force from A plus the release handover notice; until then the replaced
//! keys keep signing. The consensus adapter refuses a replacement while the
//! chain is frozen or in a block with an emergency control, as it refuses
//! upgrades and handovers (P01, 8 October 2026).
//!
//! A replacement's sequence is the authority epoch it creates. One signed for
//! an epoch is refused once another replacement is pending or that epoch is
//! in force, so it is never applied twice.

use crate::emergency_freeze::{self as emergency, AuthorityKey, AuthorityPolicy, FinalizedAnchor};
use crate::root_authority::{self, Epoch, Record};
use crate::{release_handover as handover, upgrade};
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const CONTROL_KIND: &str = "dytallix-root-kit-replacement-v1";
/// Each replacement's receipt, by its sequence.
pub const RECEIPT_PREFIX: &str = "consensus:root-authority:receipt:";
const DOMAIN: &[u8] = b"DYTALLIX/ROOT-KIT-REPLACEMENT/v1\0";
const SCHEMA: u16 = 1;
const KEY_BYTES: usize = 64;
const SIGNATURE_BYTES: usize = 29_792;

/// One value per root role.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Roles<T> {
    pub upgrade: T,
    pub freeze: T,
    pub resume: T,
}
impl<T> Roles<T> {
    /// The upgrade, freeze and resume values, in that order.
    pub fn each(&self) -> [&T; 3] {
        [&self.upgrade, &self.freeze, &self.resume]
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Payload {
    pub schema: u16,
    pub chain_id: String,
    pub genesis_sha256: String,
    /// The upgrade policy's hash, which covers its rules and not its keys.
    pub policy_sha256: String,
    /// The epoch whose upgrade keys sign.
    pub authority_epoch: u64,
    /// The epoch this replacement creates: `authority_epoch + 1`.
    pub sequence: u64,
    /// A finalized block the signers saw: its height and application hash.
    pub anchor_height: u64,
    pub anchor_app_hash: String,
    /// The inclusive heights at which the control may commit.
    pub not_before_height: u64,
    pub not_after_height: u64,
    /// The seat's key IDs that leave, one per role.
    pub replaced: Roles<String>,
    /// The new kit's keys, one per role. A key ID is its key's SHA-256.
    pub keys: Roles<AuthorityKey>,
}

/// An SLH-DSA signature in base64: six in hex would not fit the transaction
/// bound.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signature {
    pub key_id: String,
    pub signature_base64: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    pub kind: String,
    pub payload: Payload,
    /// At least the threshold of current upgrade keys, sorted by key ID.
    pub signatures: Vec<Signature>,
    /// Each new key's signature of the same artifact, in base64.
    pub proofs: Roles<String>,
}

pub fn artifact_bytes(payload: &Payload) -> Result<Vec<u8>> {
    let mut bytes = DOMAIN.to_vec();
    bytes.extend_from_slice(&serde_json::to_vec(payload)?);
    Ok(bytes)
}

/// The block a replacement is checked at. The adapter reads the anchor the
/// payload names from committed history, never from the control.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockContext {
    pub height: u64,
    pub finalized_anchor: Option<FinalizedAnchor>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: u16,
    pub control: Control,
    pub context: BlockContext,
    /// The first height the new epoch signs for.
    pub effect_height: u64,
}

/// The record and receipt a replacement writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockPlan {
    pub record: Record,
    pub receipt: Receipt,
}

/// The configured policies a replacement reads: all three at schema 2.
#[derive(Clone, Copy, Debug)]
pub struct Policies<'a> {
    pub emergency: &'a emergency::Policy,
    pub upgrade: &'a upgrade::Policy,
    pub handover: &'a handover::Policy,
    upgrade_v2: &'a upgrade::v2::Policy,
    notice: u64,
}
impl<'a> Policies<'a> {
    /// `None` unless the emergency, upgrade and handover policies are all
    /// configured at schema 2.
    pub fn of(
        emergency: Option<&'a emergency::Policy>,
        upgrade: Option<&'a upgrade::Policy>,
        handover: Option<&'a handover::Policy>,
    ) -> Option<Self> {
        let emergency = emergency.filter(|policy| policy.v2.is_some())?;
        let upgrade_policy = upgrade?;
        let handover_policy = handover?;
        Some(Self {
            emergency,
            upgrade: upgrade_policy,
            handover: handover_policy,
            upgrade_v2: upgrade_policy.as_v2()?,
            notice: handover_policy.v2.as_ref()?.min_notice_blocks,
        })
    }
    /// The configuration's epoch, the record's first: in force from genesis.
    pub fn configured_epoch(&self) -> Epoch {
        Epoch {
            authority_epoch: self.upgrade_v2.authority_epoch,
            from_height: 0,
            upgrade: self.upgrade_v2.authority.clone(),
            freeze: self.emergency.freeze_authority.clone(),
            resume: self.emergency.resume_authority.clone(),
        }
    }
    /// The record in force: the stored one, or the configuration's epoch.
    pub fn current(&self, record: Option<&Record>) -> Record {
        record.cloned().unwrap_or_else(|| Record {
            schema: root_authority::SCHEMA,
            epochs: vec![self.configured_epoch()],
        })
    }
    /// The anchor age bound, the upgrade policy's.
    pub fn max_anchor_age_blocks(&self) -> u64 {
        self.upgrade_v2.max_anchor_age_blocks
    }
}

pub fn receipt_key(sequence: u64) -> String {
    format!("{RECEIPT_PREFIX}{sequence:020}")
}
pub fn encode_receipt(receipt: &Receipt) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(receipt)?)
}
pub fn decode_receipt(policies: &Policies, bytes: &[u8]) -> Result<Receipt> {
    ensure!(
        bytes.len()
            <= policies
                .upgrade_v2
                .max_control_bytes
                .checked_add(4096)
                .context("Kit replacement receipt bound overflow")?,
        "Kit replacement receipt byte limit"
    );
    let receipt: Receipt = canonical(bytes)?;
    ensure!(receipt.schema == SCHEMA, "Kit replacement receipt schema");
    Ok(receipt)
}
pub fn decode_control(policies: &Policies, bytes: &[u8]) -> Result<Control> {
    ensure!(
        bytes.len() <= policies.upgrade_v2.max_control_bytes,
        "Kit replacement byte limit"
    );
    let control: Control = serde_json::from_slice(bytes)?;
    ensure!(
        control.kind == CONTROL_KIND
            && !control.signatures.is_empty()
            && control.signatures.len() <= policies.upgrade_v2.max_signatures,
        "Kit replacement kind/signature count"
    );
    Ok(control)
}
/// A control's canonical bytes, as blocks and receipts hold them.
pub fn canonical_control(policies: &Policies, raw: &[u8]) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(&decode_control(policies, raw)?)?)
}
pub fn control_anchor_height(policies: &Policies, raw: &[u8]) -> Result<u64> {
    Ok(decode_control(policies, raw)?.payload.anchor_height)
}
pub fn control_sequence(policies: &Policies, raw: &[u8]) -> Result<u64> {
    Ok(decode_control(policies, raw)?.payload.sequence)
}

/// Check a replacement at its block and give the record it writes. Without
/// a verifier only the structure is checked, as a history replay without
/// the root helper does.
pub fn plan_block(
    policies: &Policies,
    record: Option<&Record>,
    context: &BlockContext,
    bytes: &[u8],
    verifier: Option<&dyn upgrade::Verifier>,
) -> Result<BlockPlan> {
    let control = decode_control(policies, bytes)?;
    plan_control(policies, record, context, control, verifier)
}

/// Replay a committed receipt at its block: the same checks give the same
/// receipt and record.
pub fn replay(
    policies: &Policies,
    record: Option<&Record>,
    receipt: &Receipt,
    context: &BlockContext,
    verifier: Option<&dyn upgrade::Verifier>,
) -> Result<BlockPlan> {
    ensure!(
        receipt.context == *context,
        "Kit replacement receipt context differs"
    );
    let plan = plan_control(policies, record, context, receipt.control.clone(), verifier)?;
    ensure!(
        plan.receipt == *receipt,
        "Kit replacement receipt replay differs"
    );
    Ok(plan)
}

fn plan_control(
    policies: &Policies,
    record: Option<&Record>,
    context: &BlockContext,
    control: Control,
    verifier: Option<&dyn upgrade::Verifier>,
) -> Result<BlockPlan> {
    let record = policies.current(record);
    let epoch = validate(policies, &record, context, &control)?;
    if let Some(verifier) = verifier {
        verify(policies, &epoch, context, &control, verifier)?;
    }
    transition(policies, record, &epoch, context, control)
}

/// Every check that needs no signature, cheapest first. Returns the epoch
/// whose keys sign.
fn validate(
    policies: &Policies,
    record: &Record,
    context: &BlockContext,
    control: &Control,
) -> Result<Epoch> {
    let base = policies.upgrade_v2;
    let epoch = record.at(context.height)?.clone();
    ensure!(
        record.epochs.last() == Some(&epoch),
        "A kit replacement is already pending"
    );
    let p = &control.payload;
    ensure!(
        p.schema == SCHEMA
            && p.chain_id == base.chain_id
            && p.genesis_sha256 == base.genesis_sha256
            && p.policy_sha256 == base.sha256()?
            && p.authority_epoch == epoch.authority_epoch,
        "Kit replacement identity/policy binding"
    );
    ensure!(
        epoch.authority_epoch.checked_add(1) == Some(p.sequence),
        "Kit replacement sequence is not the next epoch"
    );
    // The anchored window, as the upgrade control's.
    ensure!(
        p.not_before_height > 0
            && p.not_after_height >= p.not_before_height
            && p.not_after_height - p.not_before_height < base.max_validity_blocks,
        "Kit replacement validity window bound"
    );
    ensure!(
        context.height >= p.not_before_height && context.height <= p.not_after_height,
        "Kit replacement outside its validity window"
    );
    let anchor = context
        .finalized_anchor
        .as_ref()
        .context("Finalized kit replacement anchor required")?;
    ensure!(
        p.anchor_height == anchor.height && p.anchor_app_hash == anchor.app_hash,
        "Kit replacement finalized anchor binding"
    );
    ensure!(
        anchor.height < p.not_before_height
            && context
                .height
                .checked_sub(anchor.height)
                .is_some_and(|age| age <= base.max_anchor_age_blocks),
        "Kit replacement anchor age bound"
    );
    ensure!(
        control.signatures.len() >= epoch.upgrade.threshold,
        "Kit replacement signature threshold"
    );
    let mut previous: Option<&str> = None;
    for signature in &control.signatures {
        ensure!(
            previous.is_none_or(|id| id < signature.key_id.as_str()),
            "Kit replacement signatures must be strictly sorted"
        );
        ensure!(
            epoch
                .upgrade
                .keys
                .iter()
                .any(|key| key.key_id == signature.key_id),
            "Kit replacement signer is not a current upgrade key"
        );
        signature_bytes(&signature.signature_base64)?;
        previous = Some(&signature.key_id);
    }
    for (set, id) in [&epoch.upgrade, &epoch.freeze, &epoch.resume]
        .into_iter()
        .zip(p.replaced.each())
    {
        ensure!(
            set.keys.iter().any(|key| &key.key_id == id),
            "Kit replacement names a key its role does not hold"
        );
    }
    // A new key differs from every key the record holds or has held.
    let mut ids = BTreeSet::new();
    let mut material = BTreeSet::new();
    for held in &record.epochs {
        for set in [&held.upgrade, &held.freeze, &held.resume] {
            for key in &set.keys {
                ids.insert(key.key_id.as_str());
                material.insert(key.public_key_hex.as_str());
            }
        }
    }
    for key in p.keys.each() {
        let public = canonical_hex(&key.public_key_hex, KEY_BYTES)?;
        ensure!(
            key.key_id == hex::encode(Sha256::digest(&public)),
            "Kit replacement key ID is not its key's SHA-256"
        );
        ensure!(
            ids.insert(key.key_id.as_str()) && material.insert(key.public_key_hex.as_str()),
            "Kit replacement reuses a key"
        );
    }
    for proof in control.proofs.each() {
        signature_bytes(proof)?;
    }
    Ok(epoch)
}

/// The current upgrade keys' signatures, then the new keys' proofs, each by
/// the root helper under the upgrade action.
fn verify(
    policies: &Policies,
    epoch: &Epoch,
    context: &BlockContext,
    control: &Control,
    verifier: &dyn upgrade::Verifier,
) -> Result<()> {
    let p = &control.payload;
    let artifact = artifact_bytes(p)?;
    let check = |public_key_hex: &str, signature: &str| -> Result<bool> {
        verifier.verify(
            &policies.upgrade_v2.chain_id,
            &hex::decode(public_key_hex)?,
            p.sequence,
            context.height,
            p.not_before_height,
            p.not_after_height,
            &artifact,
            &signature_bytes(signature)?,
        )
    };
    for signature in &control.signatures {
        let key = epoch
            .upgrade
            .keys
            .iter()
            .find(|key| key.key_id == signature.key_id)
            .context("Kit replacement signer missing")?;
        ensure!(
            check(&key.public_key_hex, &signature.signature_base64)?,
            "Kit replacement signature rejected"
        );
    }
    for (key, proof) in p.keys.each().into_iter().zip(control.proofs.each()) {
        ensure!(
            check(&key.public_key_hex, proof)?,
            "Kit replacement proof of possession rejected"
        );
    }
    Ok(())
}

/// The record with the new epoch, in force from the admission height plus
/// the release handover notice.
fn transition(
    policies: &Policies,
    mut record: Record,
    epoch: &Epoch,
    context: &BlockContext,
    control: Control,
) -> Result<BlockPlan> {
    let p = &control.payload;
    let replace = |set: &AuthorityPolicy, id: &str, key: &AuthorityKey| -> Result<AuthorityPolicy> {
        let mut set = set.clone();
        *set.keys
            .iter_mut()
            .find(|held| held.key_id == id)
            .context("Replaced key missing")? = key.clone();
        set.keys.sort_by(|a, b| a.key_id.cmp(&b.key_id));
        Ok(set)
    };
    let effect_height = context
        .height
        .checked_add(policies.notice)
        .context("Kit replacement effect height overflow")?;
    let next = Epoch {
        authority_epoch: p.sequence,
        from_height: effect_height,
        upgrade: replace(&epoch.upgrade, &p.replaced.upgrade, &p.keys.upgrade)?,
        freeze: replace(&epoch.freeze, &p.replaced.freeze, &p.keys.freeze)?,
        resume: replace(&epoch.resume, &p.replaced.resume, &p.keys.resume)?,
    };
    // Each policy of the new epoch is a valid configuration.
    root_authority::emergency_policy(policies.emergency, Some(&next))?;
    root_authority::upgrade_policy(policies.upgrade, Some(&next))?;
    root_authority::handover_policy(policies.handover, Some(&next))?;
    record.epochs.push(next);
    record.validate()?;
    Ok(BlockPlan {
        record,
        receipt: Receipt {
            schema: SCHEMA,
            control,
            context: context.clone(),
            effect_height,
        },
    })
}

fn signature_bytes(value: &str) -> Result<Vec<u8>> {
    let bytes = B64
        .decode(value)
        .context("Kit replacement signature is not base64")?;
    ensure!(
        bytes.len() == SIGNATURE_BYTES && B64.encode(&bytes) == value,
        "Kit replacement signature size or encoding"
    );
    Ok(bytes)
}
fn canonical_hex(value: &str, length: usize) -> Result<Vec<u8>> {
    let bytes = hex::decode(value).context("Kit replacement key is not hex")?;
    ensure!(
        bytes.len() == length && hex::encode(&bytes) == value,
        "Kit replacement key size or encoding"
    );
    Ok(bytes)
}
fn canonical<T: Serialize + for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T> {
    let value: T = serde_json::from_slice(bytes)?;
    ensure!(
        serde_json::to_vec(&value)? == bytes,
        "Kit replacement record is not canonical"
    );
    Ok(value)
}

#[cfg(test)]
#[path = "kit_replacement_tests.rs"]
mod tests;
