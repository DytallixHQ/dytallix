//! Deterministic emergency transaction control for explicit development policy.
//! The consensus adapter persists state and immutable receipts in its own batch.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const CONTROL_KIND: &str = "dytallix-emergency-control-v1";
pub const CONTROL_KIND_V2: &str = "dytallix-emergency-control-v2";
pub const STATE_KEY: &str = "consensus:emergency:v1:state";
pub const RECEIPT_PREFIX: &str = "consensus:emergency:v1:receipt:";
const SCHEMA: u16 = 1;
const KEY_BYTES: usize = 64;
const SIGNATURE_BYTES: usize = 29_792;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomaticTransitionPolicy {
    /// Development qualification only. This is not a production economic choice.
    ContinueExisting,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityKey {
    pub key_id: String,
    pub public_key_hex: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityPolicy {
    pub keys: Vec<AuthorityKey>,
    pub threshold: usize,
}

/// Explicit fresh-genesis development qualification parameters. These values
/// are not defaults or approved production timing limits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyV2 {
    pub genesis_sha256: String,
    pub authority_epoch: u64,
    /// Number of inclusive eligible heights, not the difference of endpoints.
    pub max_validity_blocks: u64,
    pub max_anchor_age_blocks: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub schema: u16,
    pub development_only: bool,
    pub chain_id: String,
    pub release_sha512: String,
    pub initial_sequence: u64,
    pub freeze_authority: AuthorityPolicy,
    pub resume_authority: AuthorityPolicy,
    pub max_control_bytes: usize,
    pub max_signatures: usize,
    pub automatic_transition_policy: AutomaticTransitionPolicy,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_non_null"
    )]
    pub v2: Option<PolicyV2>,
}

impl Policy {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (self.schema == SCHEMA && self.v2.is_none()) || (self.schema == 2 && self.v2.is_some()),
            "Emergency policy schema"
        );
        ensure!(
            self.development_only,
            "Production emergency activation is not qualified"
        );
        ensure!(
            !self.chain_id.is_empty()
                && self.chain_id.len() <= 128
                && self
                    .chain_id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte)),
            "Emergency chain id must match root envelope rules"
        );
        canonical_hex(&self.release_sha512, 64)?;
        ensure!(
            self.max_control_bytes > 0 && self.max_signatures > 0,
            "Explicit emergency bounds required"
        );
        ensure!(
            self.initial_sequence > 0 && self.initial_sequence < u64::MAX,
            "Emergency sequence invalid/exhausted"
        );
        for authority in [&self.freeze_authority, &self.resume_authority] {
            ensure!(
                authority.threshold > 0
                    && authority.threshold <= authority.keys.len()
                    && authority.threshold <= self.max_signatures,
                "Emergency authority threshold"
            );
            let mut previous = None;
            let mut material = BTreeSet::new();
            for key in &authority.keys {
                ensure!(!key.key_id.is_empty(), "Emergency key id required");
                ensure!(
                    previous.is_none_or(|id: &str| id < key.key_id.as_str()),
                    "Emergency keys must be strictly sorted"
                );
                canonical_hex(&key.public_key_hex, KEY_BYTES)?;
                ensure!(
                    material.insert(&key.public_key_hex),
                    "Duplicate emergency authority key"
                );
                previous = Some(key.key_id.as_str());
            }
        }
        if let Some(v2) = &self.v2 {
            canonical_hex(&v2.genesis_sha256, 32)?;
            ensure!(
                v2.authority_epoch > 0
                    && v2.max_validity_blocks > 0
                    && v2.max_anchor_age_blocks > 0,
                "Explicit v2 epoch and height bounds required"
            );
            let mut all_material = BTreeSet::new();
            let mut all_ids = BTreeSet::new();
            for authority in [&self.freeze_authority, &self.resume_authority] {
                ensure!(
                    authority.keys.len() == 5 && authority.threshold == 3,
                    "V2 emergency authority requires three of five keys"
                );
                for key in &authority.keys {
                    ensure!(
                        all_material.insert(&key.public_key_hex) && all_ids.insert(&key.key_id),
                        "V2 emergency purpose keys must be disjoint"
                    );
                }
            }
        }
        Ok(())
    }

    pub fn sha256(&self) -> Result<String> {
        self.validate()?;
        Ok(digest(&serde_json::to_vec(self)?))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Freeze,
    Resume,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResumeBinding {
    pub freeze_receipt_sha256: String,
    /// Commitment to the restored finalized checkpoint, not an assertion that
    /// the present state or incident containment was independently verified.
    pub restored_state_sha256: String,
    pub readiness_evidence_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PayloadV2 {
    pub genesis_sha256: String,
    pub authority_epoch: u64,
    pub policy_sha256: String,
    pub not_before_height: u64,
    pub not_after_height: u64,
    pub incident_sha256: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_non_null"
    )]
    pub resume: Option<ResumeBinding>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Payload {
    pub schema: u16,
    pub chain_id: String,
    pub release_sha512: String,
    pub action: Action,
    pub sequence: u64,
    pub parent_height: u64,
    pub parent_app_hash: String,
    pub target_height: u64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_non_null"
    )]
    pub v2: Option<PayloadV2>,
}

/// Artifact signed by the existing SLH-DSA root emergency envelope.
/// This domain does not introduce an alternate signature envelope.
pub fn artifact_bytes(payload: &Payload) -> Result<Vec<u8>> {
    let domain: &[u8] = match (payload.schema, payload.v2.is_some()) {
        (1, false) => b"DYTALLIX/EMERGENCY-TRANSACTION-FREEZE/v1\0",
        (2, true) => b"DYTALLIX/EMERGENCY-TRANSACTION-FREEZE/v2\0",
        _ => anyhow::bail!("Emergency artifact schema"),
    };
    let mut artifact = domain.to_vec();
    artifact.extend_from_slice(&serde_json::to_vec(payload)?);
    Ok(artifact)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlSignature {
    pub key_id: String,
    pub signature_hex: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    pub kind: String,
    pub payload: Payload,
    pub signatures: Vec<ControlSignature>,
}

/// A finalized block commitment independently obtained from the chain store.
/// The caller must never construct this value solely from the control payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinalizedAnchor {
    pub height: u64,
    pub app_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockContext {
    pub height: u64,
    pub parent_height: u64,
    pub parent_app_hash: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_non_null"
    )]
    pub finalized_anchor: Option<FinalizedAnchor>,
    /// Release selected by independently reconstructed committed handover
    /// history at this block. Never derive this from the control payload or
    /// local runtime candidate. None means the immutable genesis release.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_non_null"
    )]
    pub active_release_sha512: Option<String>,
}

impl BlockContext {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.parent_height.checked_add(1) == Some(self.height),
            "Emergency block height continuity"
        );
        canonical_hex(&self.parent_app_hash, 32)?;
        if let Some(release) = &self.active_release_sha512 {
            canonical_hex(release, 64)?;
        }
        if let Some(anchor) = &self.finalized_anchor {
            ensure!(
                anchor.height <= self.parent_height,
                "Emergency anchor is not finalized"
            );
            canonical_hex(&anchor.app_hash, 32)?;
            ensure!(
                anchor.height != self.parent_height || anchor.app_hash == self.parent_app_hash,
                "Emergency parent anchor mismatch"
            );
        }
        Ok(())
    }
}

/// No wire field can select a verifier or override verification.
pub trait ControlVerifier: Send + Sync {
    #[allow(clippy::too_many_arguments)]
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
    ) -> Result<bool>;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    schema: u16,
    policy_sha256: String,
    frozen: bool,
    upgrade_hold: bool,
    next_sequence: u64,
    last_control_height: Option<u64>,
    last_receipt_sha256: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_non_null"
    )]
    incident_sha256: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_non_null"
    )]
    freeze_receipt_sha256: Option<String>,
}

impl State {
    pub fn new(policy: &Policy) -> Result<Self> {
        Ok(Self {
            schema: SCHEMA,
            policy_sha256: policy.sha256()?,
            frozen: false,
            upgrade_hold: false,
            next_sequence: policy.initial_sequence,
            last_control_height: None,
            last_receipt_sha256: None,
            incident_sha256: None,
            freeze_receipt_sha256: None,
        })
    }
    pub fn frozen(&self) -> bool {
        self.frozen
    }
    pub fn blocks_upgrade(&self) -> bool {
        self.frozen || self.upgrade_hold
    }
    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
    }
    pub fn last_receipt_sha256(&self) -> Option<&str> {
        self.last_receipt_sha256.as_deref()
    }
    pub fn incident_sha256(&self) -> Option<&str> {
        self.incident_sha256.as_deref()
    }
    pub fn freeze_receipt_sha256(&self) -> Option<&str> {
        self.freeze_receipt_sha256.as_deref()
    }
    pub fn validate(&self, policy: &Policy) -> Result<()> {
        ensure!(
            self.schema == SCHEMA && self.policy_sha256 == policy.sha256()?,
            "Emergency state policy/schema mismatch"
        );
        ensure!(
            self.next_sequence >= policy.initial_sequence,
            "Emergency state sequence rollback"
        );
        let initial = self.next_sequence == policy.initial_sequence;
        ensure!(
            initial == self.last_receipt_sha256.is_none()
                && initial == self.last_control_height.is_none(),
            "Emergency state history mismatch"
        );
        ensure!(
            !initial || !self.frozen,
            "Initial emergency state cannot be frozen"
        );
        ensure!(
            self.upgrade_hold != initial,
            "Emergency upgrade hold history mismatch"
        );
        let requires_v2_history = policy.v2.is_some() && !initial;
        ensure!(
            self.incident_sha256.is_some() == requires_v2_history
                && self.freeze_receipt_sha256.is_some() == requires_v2_history,
            "Emergency v2 incident history mismatch"
        );
        if let Some(hash) = &self.incident_sha256 {
            canonical_hex(hash, 32)?;
        }
        if let Some(hash) = &self.freeze_receipt_sha256 {
            canonical_hex(hash, 32)?;
        }
        if let Some(hash) = &self.last_receipt_sha256 {
            canonical_hex(hash, 32)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: u16,
    pub policy_sha256: String,
    pub control: Control,
    pub context: BlockContext,
    pub previous_receipt_sha256: Option<String>,
}

impl Receipt {
    pub fn sha256(&self) -> Result<String> {
        Ok(digest(&serde_json::to_vec(self)?))
    }
    pub fn sequence(&self) -> u64 {
        self.control.payload.sequence
    }
}

pub fn receipt_key(sequence: u64) -> String {
    format!("{RECEIPT_PREFIX}{sequence:020}")
}
pub fn encode_state(state: &State) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(state)?)
}
pub fn decode_state(policy: &Policy, bytes: &[u8]) -> Result<State> {
    // State is fixed in shape and contains only digest-sized text.
    ensure!(bytes.len() <= 1024, "Emergency state byte limit");
    let state: State = canonical_decode(bytes)?;
    state.validate(policy)?;
    Ok(state)
}
pub fn encode_receipt(receipt: &Receipt) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(receipt)?)
}
pub fn decode_receipt(policy: &Policy, bytes: &[u8]) -> Result<Receipt> {
    let maximum = policy
        .max_control_bytes
        .checked_add(1024)
        .context("Emergency receipt bound overflow")?;
    ensure!(bytes.len() <= maximum, "Emergency receipt byte limit");
    canonical_decode(bytes)
}

#[derive(Debug)]
pub struct Rejected(pub String);
impl std::fmt::Display for Rejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Rejected {}
pub fn is_rejection(error: &anyhow::Error) -> bool {
    error.downcast_ref::<Rejected>().is_some()
}
fn rejected(error: anyhow::Error) -> anyhow::Error {
    Rejected(error.to_string()).into()
}

#[derive(Clone, Debug)]
pub struct BlockPlan {
    pub state: State,
    pub receipt: Option<Receipt>,
    pub reject_user_transactions: bool,
    /// Prevents pending upgrades from gaining activation through emergency controls.
    /// An authorized resume is not an authorization to activate a pending upgrade.
    pub block_upgrade_activation: bool,
}

pub fn decode_control(policy: &Policy, bytes: &[u8]) -> Result<Control> {
    ensure!(
        bytes.len() <= policy.max_control_bytes,
        "Emergency control byte limit"
    );
    let control: Control = canonical_decode(bytes)?;
    ensure!(
        control.kind
            == if policy.v2.is_some() {
                CONTROL_KIND_V2
            } else {
                CONTROL_KIND
            },
        "Emergency control discriminator"
    );
    ensure!(
        !control.signatures.is_empty() && control.signatures.len() <= policy.max_signatures,
        "Emergency signature count"
    );
    Ok(control)
}

pub fn plan_block(
    policy: &Policy,
    state: &State,
    context: &BlockContext,
    control_bytes: Option<&[u8]>,
    verifier: &dyn ControlVerifier,
) -> Result<BlockPlan> {
    state.validate(policy)?;
    context.validate()?;
    ensure!(
        state
            .last_control_height
            .is_none_or(|height| height < context.height),
        "Emergency block already consumed"
    );
    let Some(bytes) = control_bytes else {
        return Ok(BlockPlan {
            state: state.clone(),
            receipt: None,
            reject_user_transactions: state.frozen,
            block_upgrade_activation: state.blocks_upgrade(),
        });
    };
    let control = decode_control(policy, bytes).map_err(rejected)?;
    validate_admission(policy, state, context, &control).map_err(rejected)?;
    let authority = match control.payload.action {
        Action::Freeze => &policy.freeze_authority,
        Action::Resume => &policy.resume_authority,
    };
    let artifact = artifact_bytes(&control.payload)?;
    for signature in &control.signatures {
        let key = authority
            .keys
            .iter()
            .find(|key| key.key_id == signature.key_id)
            .context("Emergency authority key missing")
            .map_err(rejected)?;
        // All supplied signatures must verify. Additional invalid signatures do
        // not become a way to consume variable work after threshold admission.
        let verified = verifier.verify(
            &policy.chain_id,
            &hex::decode(&key.public_key_hex)?,
            control.payload.sequence,
            context.height,
            control
                .payload
                .v2
                .as_ref()
                .map_or(context.height, |v2| v2.not_before_height),
            control
                .payload
                .v2
                .as_ref()
                .map_or(context.height, |v2| v2.not_after_height),
            &artifact,
            &hex::decode(&signature.signature_hex)?,
        )?;
        ensure!(verified, Rejected("Emergency signature rejected".into()));
    }
    recorded_transition(policy, state, context, control)
}

fn recorded_transition(
    policy: &Policy,
    state: &State,
    context: &BlockContext,
    control: Control,
) -> Result<BlockPlan> {
    let receipt = Receipt {
        schema: SCHEMA,
        policy_sha256: policy.sha256()?,
        control: control.clone(),
        context: context.clone(),
        previous_receipt_sha256: state.last_receipt_sha256.clone(),
    };
    let mut next = state.clone();
    next.frozen = control.payload.action == Action::Freeze;
    next.upgrade_hold = true;
    next.next_sequence = next
        .next_sequence
        .checked_add(1)
        .context("Emergency sequence overflow")?;
    next.last_control_height = Some(context.height);
    next.last_receipt_sha256 = Some(receipt.sha256()?);
    if let Some(v2) = &control.payload.v2 {
        if control.payload.action == Action::Freeze {
            next.incident_sha256 = Some(v2.incident_sha256.clone());
            next.freeze_receipt_sha256 = next.last_receipt_sha256.clone();
        }
    }
    next.validate(policy)?;
    Ok(BlockPlan {
        reject_user_transactions: state.frozen || next.frozen,
        block_upgrade_activation: true,
        state: next,
        receipt: Some(receipt),
    })
}

fn validate_admission(
    policy: &Policy,
    state: &State,
    context: &BlockContext,
    control: &Control,
) -> Result<()> {
    let payload = &control.payload;
    let active_release = context
        .active_release_sha512
        .as_deref()
        .unwrap_or(&policy.release_sha512);
    ensure!(
        context.active_release_sha512.is_none() || active_release != policy.release_sha512,
        "Emergency genesis release context must be omitted"
    );
    ensure!(
        payload.schema == policy.schema
            && payload.v2.is_some() == policy.v2.is_some()
            && payload.chain_id == policy.chain_id
            && payload.release_sha512 == active_release,
        "Emergency payload policy binding"
    );
    ensure!(
        payload.sequence == state.next_sequence && payload.sequence < u64::MAX,
        "Emergency sequence mismatch/exhaustion"
    );
    if let (Some(bounds), Some(v2)) = (&policy.v2, &payload.v2) {
        validate_v2_admission(policy, bounds, state, context, payload, v2)?;
    } else {
        ensure!(
            context.finalized_anchor.is_none(),
            "V1 emergency context cannot carry v2 anchor"
        );
        ensure!(
            payload.parent_height == context.parent_height
                && payload.target_height == context.height
                && payload.parent_app_hash == context.parent_app_hash,
            "Emergency finalized parent binding"
        );
    }
    let authority = match payload.action {
        Action::Freeze => {
            ensure!(!state.frozen, "Emergency already frozen");
            ensure!(
                payload.sequence < u64::MAX - 1,
                "Freeze must preserve a resume sequence"
            );
            &policy.freeze_authority
        }
        Action::Resume => {
            ensure!(state.frozen, "Emergency not frozen");
            &policy.resume_authority
        }
    };
    ensure!(
        control.signatures.len() >= authority.threshold,
        "Emergency signature threshold"
    );
    let mut previous = None;
    for signature in &control.signatures {
        ensure!(
            previous.is_none_or(|id: &str| id < signature.key_id.as_str()),
            "Emergency signatures must be strictly sorted"
        );
        ensure!(
            authority
                .keys
                .iter()
                .any(|key| key.key_id == signature.key_id),
            "Emergency authority key missing"
        );
        canonical_hex(&signature.signature_hex, SIGNATURE_BYTES)?;
        previous = Some(signature.key_id.as_str());
    }
    Ok(())
}

fn validate_v2_admission(
    policy: &Policy,
    bounds: &PolicyV2,
    state: &State,
    context: &BlockContext,
    payload: &Payload,
    v2: &PayloadV2,
) -> Result<()> {
    ensure!(
        v2.genesis_sha256 == bounds.genesis_sha256
            && v2.authority_epoch == bounds.authority_epoch
            && v2.policy_sha256 == policy.sha256()?,
        "Emergency v2 policy/genesis/epoch binding"
    );
    canonical_hex(&v2.incident_sha256, 32)?;
    ensure!(
        v2.not_before_height > 0
            && payload.target_height == v2.not_before_height
            && v2.not_after_height >= v2.not_before_height
            && v2.not_after_height - v2.not_before_height < bounds.max_validity_blocks,
        "Emergency v2 validity window bound"
    );
    ensure!(
        context.height >= v2.not_before_height && context.height <= v2.not_after_height,
        "Emergency v2 outside validity window"
    );
    let anchor = context
        .finalized_anchor
        .as_ref()
        .context("Validated finalized emergency anchor required")?;
    ensure!(
        payload.parent_height == anchor.height && payload.parent_app_hash == anchor.app_hash,
        "Emergency v2 finalized anchor binding"
    );
    ensure!(
        anchor.height < v2.not_before_height
            && context
                .height
                .checked_sub(anchor.height)
                .is_some_and(|age| age <= bounds.max_anchor_age_blocks),
        "Emergency v2 anchor age bound"
    );
    match payload.action {
        Action::Freeze => ensure!(v2.resume.is_none(), "Freeze cannot carry resume evidence"),
        Action::Resume => {
            let resume = v2
                .resume
                .as_ref()
                .context("Emergency resume evidence required")?;
            canonical_hex(&resume.freeze_receipt_sha256, 32)?;
            canonical_hex(&resume.restored_state_sha256, 32)?;
            canonical_hex(&resume.readiness_evidence_sha256, 32)?;
            ensure!(
                state.incident_sha256() == Some(v2.incident_sha256.as_str())
                    && state.freeze_receipt_sha256() == Some(resume.freeze_receipt_sha256.as_str()),
                "Emergency resume incident/freeze receipt binding"
            );
            ensure!(
                resume.restored_state_sha256 == anchor.app_hash,
                "Emergency resume restored checkpoint binding"
            );
            ensure!(
                anchor.height
                    >= state
                        .last_control_height
                        .context("Emergency freeze height required")?,
                "Emergency resume checkpoint predates freeze"
            );
        }
    }
    Ok(())
}

/// Caller supplies actual stored block facts for every immutable receipt.
/// Active release contexts must come from the independently reconstructed
/// committed handover history at each block, not from the recorded payload.
/// Startup must verify that handover history before accepting recovered state.
/// Caller must also check that each block contains the same control bytes and
/// that no receipt key or control-bearing block was omitted from this ordered set.
pub fn recover(
    policy: &Policy,
    receipts: &[(Receipt, BlockContext)],
    verifier: &dyn ControlVerifier,
) -> Result<State> {
    let mut state = State::new(policy)?;
    for (receipt, actual_block) in receipts {
        ensure!(
            &receipt.context == actual_block,
            "Emergency receipt block mismatch"
        );
        let wire = serde_json::to_vec(&receipt.control)?;
        let planned = plan_block(policy, &state, actual_block, Some(&wire), verifier)?;
        ensure!(
            planned.receipt.as_ref() == Some(receipt),
            "Emergency receipt replay mismatch"
        );
        state = planned.state;
    }
    Ok(state)
}

/// Structural recovery only. This does not admit new controls or establish
/// signature validity. Application startup must also call `recover` with the
/// real verifier. The caller must bind all records to actual stored blocks.
pub(crate) fn replay_recorded_step(
    policy: &Policy,
    state: &State,
    receipt: &Receipt,
    actual_block: &BlockContext,
) -> Result<State> {
    ensure!(
        &receipt.context == actual_block,
        "Emergency receipt block mismatch"
    );
    state.validate(policy)?;
    actual_block.validate()?;
    ensure!(
        state
            .last_control_height
            .is_none_or(|height| height < actual_block.height),
        "Emergency block already consumed"
    );
    let wire = serde_json::to_vec(&receipt.control)?;
    let control = decode_control(policy, &wire)?;
    validate_admission(policy, &state, actual_block, &control)?;
    let planned = recorded_transition(policy, &state, actual_block, control)?;
    ensure!(
        planned.receipt.as_ref() == Some(receipt),
        "Emergency receipt replay mismatch"
    );
    Ok(planned.state)
}

pub fn recover_recorded(policy: &Policy, receipts: &[(Receipt, BlockContext)]) -> Result<State> {
    let mut state = State::new(policy)?;
    for (receipt, actual_block) in receipts {
        state = replay_recorded_step(policy, &state, receipt, actual_block)?;
    }
    Ok(state)
}

fn present_non_null<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    // Missing fields use serde's default. Explicit null is not a missing field.
    T::deserialize(deserializer).map(Some)
}

fn canonical_decode<T: for<'de> Deserialize<'de> + Serialize>(bytes: &[u8]) -> Result<T> {
    let value: T = serde_json::from_slice(bytes)?;
    ensure!(
        serde_json::to_vec(&value)? == bytes,
        "Noncanonical emergency JSON"
    );
    Ok(value)
}
fn canonical_hex(value: &str, bytes: usize) -> Result<()> {
    ensure!(
        value.len() == bytes * 2
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "Noncanonical emergency hex"
    );
    Ok(())
}
fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[cfg(test)]
#[path = "emergency_freeze_tests.rs"]
mod tests;
