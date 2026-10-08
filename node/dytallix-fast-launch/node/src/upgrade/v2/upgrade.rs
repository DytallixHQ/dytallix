//! Production upgrade control, schema 2 (production activation v1, step A3).
//! Retained beside v1, which stays byte for byte for replay. Five custodians
//! sign, three at a time; activation comes at least the policy's notice after
//! admission (D11-Q03); every signature binds a finalized anchor block and a
//! bounded height window, as emergency freeze v2 does. The one compiled
//! migration is the emergency receipt digest index (state schema 0 to 1), run
//! by the active release. No installation, schedule or global hold reset.
use crate::emergency_freeze as emergency;
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const CONTROL_KIND: &str = "dytallix-upgrade-control-v2";
pub const STATE_KEY: &str = "consensus:upgrade:v2:state";
pub const RECEIPT_PREFIX: &str = "consensus:upgrade:v2:receipt:";
/// The migration writes the same digest index as v1.
pub const INDEX_PREFIX: &str = "consensus:emergency:v1:receipt-by-sha256:";
pub const INDEX_STATE_KEY: &str = "consensus:emergency:v1:receipt-index";
pub const MIGRATION_ID: &str = "emergency-receipt-digest-index-v1";
const DOMAIN: &[u8] = b"DYTALLIX/CHAIN-UPGRADE/v2\0";
const KEY_BYTES: usize = 64;
const SIGNATURE_BYTES: usize = 29_792;
/// Five custodians, three signatures (D11-Q03).
pub const KEYS: usize = 5;
pub const THRESHOLD: usize = 3;

/// Binds the compiled implementation, not an arbitrary submitted digest.
pub fn migration_sha256() -> String {
    hash(include_bytes!("upgrade.rs"))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationBounds {
    pub max_receipts: u64,
    pub max_receipt_bytes: u64,
    pub max_write_bytes: u64,
}
impl MigrationBounds {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.max_receipts > 0 && self.max_receipt_bytes > 0 && self.max_write_bytes > 0,
            "Explicit migration bounds required"
        );
        Ok(())
    }
    fn within(&self, ceiling: &Self) -> bool {
        self.max_receipts <= ceiling.max_receipts
            && self.max_receipt_bytes <= ceiling.max_receipt_bytes
            && self.max_write_bytes <= ceiling.max_write_bytes
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub schema: u16,
    pub chain_id: String,
    pub genesis_sha256: String,
    /// The genesis release; later releases come from committed handovers.
    pub initial_release_sha512: String,
    pub authority_epoch: u64,
    pub authority: emergency::AuthorityPolicy,
    pub initial_sequence: u64,
    pub max_control_bytes: usize,
    pub max_signatures: usize,
    pub migration_bounds: MigrationBounds,
    /// Blocks from admission to the earliest activation.
    pub min_notice_blocks: u64,
    /// Inclusive heights a signed window may span.
    pub max_validity_blocks: u64,
    /// Blocks from the signed anchor to the commit height.
    pub max_anchor_age_blocks: u64,
}
impl Policy {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema == 2, "Upgrade policy schema");
        ensure!(
            !self.chain_id.is_empty()
                && self.chain_id.len() <= 128
                && self
                    .chain_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)),
            "Upgrade chain identity"
        );
        valid_hex(&self.genesis_sha256, 32)?;
        valid_hex(&self.initial_release_sha512, 64)?;
        ensure!(
            self.authority_epoch > 0
                && self.initial_sequence > 0
                && self.initial_sequence < u64::MAX,
            "Upgrade authority epoch/sequence required"
        );
        ensure!(
            self.max_control_bytes > 0 && self.max_signatures >= THRESHOLD,
            "Upgrade wire bounds required"
        );
        ensure!(
            self.authority.keys.len() == KEYS && self.authority.threshold == THRESHOLD,
            "Upgrade authority requires three of five keys"
        );
        let mut previous: Option<&str> = None;
        let mut materials = BTreeSet::new();
        for key in &self.authority.keys {
            valid_hex(&key.public_key_hex, KEY_BYTES)?;
            ensure!(
                key.key_id == hash(&hex::decode(&key.public_key_hex)?),
                "Upgrade key ID must be the SHA-256 of its public key"
            );
            ensure!(
                previous.is_none_or(|p| p < key.key_id.as_str()),
                "Upgrade keys must be strictly sorted"
            );
            ensure!(
                materials.insert(&key.public_key_hex),
                "Duplicate upgrade authority key"
            );
            previous = Some(&key.key_id);
        }
        ensure!(
            self.min_notice_blocks > 0
                && self.max_validity_blocks > 0
                && self.max_anchor_age_blocks > 0,
            "Explicit upgrade notice and window bounds required"
        );
        self.migration_bounds.validate()
    }
    /// The policy's hash. A schema 2 policy hashes its rules, not its keys:
    /// the key sets and the authority epoch are left out (root kit
    /// replacement v1), so a replacement changes neither this hash nor the
    /// states, receipts and payloads that bind it; every schema 2 payload
    /// signs its authority epoch, and only that epoch's keys verify it.
    pub fn sha256(&self) -> Result<String> {
        self.validate()?;
        let mut rules = self.clone();
        rules.authority_epoch = 0;
        rules.authority.keys.clear();
        Ok(hash(&serde_json::to_vec(&rules)?))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationPlan {
    pub target_release_sha512: String,
    pub migration_id: String,
    pub migration_sha256: String,
    pub source_schema: u16,
    pub target_schema: u16,
    pub bounds: MigrationBounds,
    pub authorization_sha256: String,
}
impl MigrationPlan {
    pub fn sha256(&self) -> Result<String> {
        Ok(hash(&serde_json::to_vec(self)?))
    }
    fn validate(&self, policy: &Policy, active_release: &str) -> Result<()> {
        ensure!(
            self.target_release_sha512 == active_release,
            "Migration requires the active release"
        );
        ensure!(
            self.migration_id == MIGRATION_ID && self.migration_sha256 == migration_sha256(),
            "Unknown compiled migration"
        );
        ensure!(
            self.source_schema == 0 && self.target_schema == 1,
            "Unsupported migration schema"
        );
        valid_hex(&self.authorization_sha256, 32)?;
        self.bounds.validate()?;
        ensure!(
            self.bounds.within(&policy.migration_bounds),
            "Migration exceeds policy bounds"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Admit {
        plan: MigrationPlan,
    },
    Activate {
        plan: MigrationPlan,
        admission_receipt_sha256: String,
        emergency_receipt_sha256: Option<String>,
        evidence_sha256: String,
    },
    Cancel {
        plan_sha256: String,
        admission_receipt_sha256: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Payload {
    pub schema: u16,
    pub chain_id: String,
    pub genesis_sha256: String,
    pub policy_sha256: String,
    /// The active release when the custodians signed.
    pub source_release_sha512: String,
    pub authority_epoch: u64,
    pub sequence: u64,
    /// A finalized block the custodians saw: its height and application hash.
    pub anchor_height: u64,
    pub anchor_app_hash: String,
    /// The inclusive heights at which the control may commit.
    pub not_before_height: u64,
    pub not_after_height: u64,
    pub action: Action,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    pub kind: String,
    pub payload: Payload,
    pub signatures: Vec<emergency::ControlSignature>,
}
pub fn artifact_bytes(payload: &Payload) -> Result<Vec<u8>> {
    let mut bytes = DOMAIN.to_vec();
    bytes.extend_from_slice(&serde_json::to_vec(payload)?);
    Ok(bytes)
}
pub use super::v1::Verifier;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockContext {
    pub height: u64,
    pub parent_height: u64,
    pub parent_app_hash: String,
    /// The release selected by committed handover history at this block.
    pub active_release_sha512: String,
    /// The anchor a control names, read from committed history by the
    /// adapter. Never taken from the control itself.
    pub finalized_anchor: Option<emergency::FinalizedAnchor>,
    pub emergency_frozen: bool,
    pub emergency_control_present: bool,
    pub emergency_upgrade_hold: bool,
    pub emergency_receipt_sha256: Option<String>,
}
impl BlockContext {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.parent_height.checked_add(1) == Some(self.height),
            "Upgrade block continuity"
        );
        valid_hex(&self.parent_app_hash, 32)?;
        valid_hex(&self.active_release_sha512, 64)?;
        if let Some(anchor) = &self.finalized_anchor {
            ensure!(
                anchor.height <= self.parent_height,
                "Upgrade anchor is not finalized"
            );
            valid_hex(&anchor.app_hash, 32)?;
            ensure!(
                anchor.height != self.parent_height || anchor.app_hash == self.parent_app_hash,
                "Upgrade parent anchor mismatch"
            );
        }
        if let Some(value) = &self.emergency_receipt_sha256 {
            valid_hex(value, 32)?;
        }
        ensure!(
            self.emergency_upgrade_hold == self.emergency_receipt_sha256.is_some(),
            "Upgrade hold/history mismatch"
        );
        ensure!(
            !self.emergency_frozen || self.emergency_upgrade_hold,
            "Frozen context requires emergency history"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingPlan {
    pub plan: MigrationPlan,
    pub admission_receipt_sha256: String,
    pub admitted_height: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    schema: u16,
    policy_sha256: String,
    next_sequence: u64,
    pending: Option<PendingPlan>,
    active_schema: u16,
    last_receipt_sha256: Option<String>,
    last_control_height: Option<u64>,
    activation_receipt_sha256: Option<String>,
}
impl State {
    pub fn new(policy: &Policy) -> Result<Self> {
        Ok(Self {
            schema: 2,
            policy_sha256: policy.sha256()?,
            next_sequence: policy.initial_sequence,
            pending: None,
            active_schema: 0,
            last_receipt_sha256: None,
            last_control_height: None,
            activation_receipt_sha256: None,
        })
    }
    pub fn validate(&self, policy: &Policy) -> Result<()> {
        ensure!(
            self.schema == 2 && self.policy_sha256 == policy.sha256()?,
            "Upgrade state policy binding"
        );
        ensure!(
            self.next_sequence >= policy.initial_sequence,
            "Upgrade sequence rollback"
        );
        let initial = self.next_sequence == policy.initial_sequence;
        ensure!(
            initial == self.last_receipt_sha256.is_none()
                && initial == self.last_control_height.is_none(),
            "Upgrade state history mismatch"
        );
        ensure!(self.active_schema <= 1, "Upgrade active schema");
        ensure!(
            (self.active_schema == 1) == self.activation_receipt_sha256.is_some(),
            "Upgrade activation history mismatch"
        );
        ensure!(
            !initial || (self.pending.is_none() && self.active_schema == 0),
            "Initial upgrade state altered"
        );
        for digest in [&self.last_receipt_sha256, &self.activation_receipt_sha256]
            .into_iter()
            .flatten()
        {
            valid_hex(digest, 32)?;
        }
        if let Some(pending) = &self.pending {
            ensure!(
                self.active_schema == 0
                    && Some(pending.admitted_height) <= self.last_control_height,
                "Invalid pending upgrade state"
            );
            valid_hex(&pending.plan.target_release_sha512, 64)?;
            pending
                .plan
                .validate(policy, &pending.plan.target_release_sha512)?;
            valid_hex(&pending.admission_receipt_sha256, 32)?;
        }
        Ok(())
    }
    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
    }
    pub fn pending(&self) -> Option<&PendingPlan> {
        self.pending.as_ref()
    }
    pub fn active_schema(&self) -> u16 {
        self.active_schema
    }
    pub fn last_receipt_sha256(&self) -> Option<&str> {
        self.last_receipt_sha256.as_deref()
    }
    pub fn activation_receipt_sha256(&self) -> Option<&str> {
        self.activation_receipt_sha256.as_deref()
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
    pub migration_source_digest: Option<String>,
    pub migration_index_digest: Option<String>,
    pub migrated_receipts: Option<u64>,
}
impl Receipt {
    pub fn sha256(&self) -> Result<String> {
        Ok(hash(&serde_json::to_vec(self)?))
    }
    pub fn sequence(&self) -> u64 {
        self.control.payload.sequence
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexState {
    pub schema: u16,
    pub activation_height: u64,
    pub activation_receipt_sha256: String,
    pub backfilled_receipts: u64,
}
#[derive(Clone, Debug)]
pub struct PreparedMigration {
    pub writes: BTreeMap<Vec<u8>, Vec<u8>>,
    pub receipt_count: u64,
    pub source_digest: String,
    pub resulting_index_digest: String,
}
#[derive(Clone, Debug)]
pub struct BlockPlan {
    pub state: State,
    pub receipt: Option<Receipt>,
    pub migration: Option<PreparedMigration>,
}

/// The adapter must bind every record/context to actual finalized block bytes.
/// This wrapper additionally proves canonical receipt encoding and ordered replay.
#[derive(Clone, Debug)]
pub struct VerifiedEmergencyHistory {
    receipts: Vec<emergency::Receipt>,
    last_receipt_sha256: Option<String>,
    frozen: bool,
    upgrade_hold: bool,
}
impl VerifiedEmergencyHistory {
    /// Each record is checked with the policy in force at its block (root
    /// kit replacement v1).
    pub fn from_records(
        policy: &emergency::Policy,
        policy_at: &dyn Fn(u64) -> Result<emergency::Policy>,
        records: &[(emergency::Receipt, emergency::BlockContext)],
        expected_state: &emergency::State,
    ) -> Result<Self> {
        ensure!(
            emergency::recover_recorded_at(policy, policy_at, records)? == *expected_state,
            "Migration emergency history/state mismatch"
        );
        for (receipt, _) in records {
            ensure!(
                emergency::decode_receipt(policy, &emergency::encode_receipt(receipt)?)?
                    == *receipt,
                "Migration receipt encoding mismatch"
            );
        }
        Ok(Self {
            receipts: records.iter().map(|(r, _)| r.clone()).collect(),
            last_receipt_sha256: expected_state.last_receipt_sha256().map(str::to_owned),
            frozen: expected_state.frozen(),
            upgrade_hold: expected_state.blocks_upgrade(),
        })
    }
    pub fn empty() -> Self {
        Self {
            receipts: Vec::new(),
            last_receipt_sha256: None,
            frozen: false,
            upgrade_hold: false,
        }
    }
    fn matches_context(&self, context: &BlockContext) -> Result<()> {
        ensure!(
            self.last_receipt_sha256 == context.emergency_receipt_sha256
                && self.frozen == context.emergency_frozen
                && self.upgrade_hold == context.emergency_upgrade_hold,
            "Migration emergency context/history differs"
        );
        ensure!(
            self.receipts
                .last()
                .is_none_or(|r| r.context.height <= context.parent_height),
            "Migration includes unfinalized history"
        );
        Ok(())
    }
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

pub fn receipt_key(sequence: u64) -> String {
    format!("{RECEIPT_PREFIX}{sequence:020}")
}
pub fn index_key(digest: &str) -> Result<String> {
    valid_hex(digest, 32)?;
    Ok(format!("{INDEX_PREFIX}{digest}"))
}
pub fn encode_state(state: &State) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(state)?)
}
pub fn decode_state(policy: &Policy, bytes: &[u8]) -> Result<State> {
    ensure!(
        bytes.len()
            <= policy
                .max_control_bytes
                .checked_add(4096)
                .context("Upgrade state bound overflow")?,
        "Upgrade state byte limit"
    );
    let state: State = decode(bytes)?;
    state.validate(policy)?;
    Ok(state)
}
pub fn encode_receipt(receipt: &Receipt) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(receipt)?)
}
pub fn decode_receipt(policy: &Policy, bytes: &[u8]) -> Result<Receipt> {
    ensure!(
        bytes.len()
            <= policy
                .max_control_bytes
                .checked_add(8192)
                .context("Upgrade receipt bound overflow")?,
        "Upgrade receipt byte limit"
    );
    let receipt: Receipt = decode(bytes)?;
    ensure!(
        receipt.schema == 2 && receipt.policy_sha256 == policy.sha256()?,
        "Upgrade receipt policy differs"
    );
    Ok(receipt)
}
pub fn decode_control(policy: &Policy, bytes: &[u8]) -> Result<Control> {
    ensure!(
        bytes.len() <= policy.max_control_bytes,
        "Upgrade control byte limit"
    );
    let control: Control = decode(bytes)?;
    ensure!(
        control.kind == CONTROL_KIND
            && !control.signatures.is_empty()
            && control.signatures.len() <= policy.max_signatures,
        "Upgrade control kind/signature count"
    );
    Ok(control)
}

pub fn plan_block(
    policy: &Policy,
    state: &State,
    context: &BlockContext,
    control_bytes: Option<&[u8]>,
    history: &VerifiedEmergencyHistory,
    verifier: &dyn Verifier,
) -> Result<BlockPlan> {
    state.validate(policy)?;
    context.validate()?;
    let Some(bytes) = control_bytes else {
        return Ok(BlockPlan {
            state: state.clone(),
            receipt: None,
            migration: None,
        });
    };
    let control = decode_control(policy, bytes).map_err(rejected)?;
    validate_control(policy, state, context, history, &control).map_err(rejected)?;
    verify_signatures(policy, context, &control, verifier)?;
    transition(policy, state, context, history, control).map_err(rejected)
}

fn verify_signatures(
    policy: &Policy,
    context: &BlockContext,
    control: &Control,
    verifier: &dyn Verifier,
) -> Result<()> {
    let artifact = artifact_bytes(&control.payload)?;
    let payload = &control.payload;
    for signature in &control.signatures {
        let key = policy
            .authority
            .keys
            .iter()
            .find(|k| k.key_id == signature.key_id)
            .context("Upgrade authority missing")?;
        let valid = verifier.verify(
            &policy.chain_id,
            &hex::decode(&key.public_key_hex)?,
            payload.sequence,
            context.height,
            payload.not_before_height,
            payload.not_after_height,
            &artifact,
            &hex::decode(&signature.signature_hex)?,
        )?;
        ensure!(valid, Rejected("Upgrade signature rejected".into()));
    }
    Ok(())
}
fn validate_control(
    policy: &Policy,
    state: &State,
    context: &BlockContext,
    history: &VerifiedEmergencyHistory,
    control: &Control,
) -> Result<()> {
    history.matches_context(context)?;
    ensure!(
        !context.emergency_frozen && !context.emergency_control_present,
        "Emergency control blocks upgrade actions"
    );
    ensure!(
        state.last_control_height.is_none_or(|h| h < context.height),
        "Upgrade block already consumed"
    );
    let p = &control.payload;
    ensure!(
        p.schema == 2
            && p.chain_id == policy.chain_id
            && p.genesis_sha256 == policy.genesis_sha256
            && p.policy_sha256 == policy.sha256()?
            && p.source_release_sha512 == context.active_release_sha512
            && p.authority_epoch == policy.authority_epoch,
        "Upgrade identity/policy binding"
    );
    ensure!(
        p.sequence == state.next_sequence && p.sequence < u64::MAX,
        "Upgrade sequence mismatch/exhaustion"
    );
    // The anchored window (emergency freeze v2's scheme).
    ensure!(
        p.not_before_height > 0
            && p.not_after_height >= p.not_before_height
            && p.not_after_height - p.not_before_height < policy.max_validity_blocks,
        "Upgrade validity window bound"
    );
    ensure!(
        context.height >= p.not_before_height && context.height <= p.not_after_height,
        "Upgrade outside its validity window"
    );
    let anchor = context
        .finalized_anchor
        .as_ref()
        .context("Finalized upgrade anchor required")?;
    ensure!(
        p.anchor_height == anchor.height && p.anchor_app_hash == anchor.app_hash,
        "Upgrade finalized anchor binding"
    );
    ensure!(
        anchor.height < p.not_before_height
            && context
                .height
                .checked_sub(anchor.height)
                .is_some_and(|age| age <= policy.max_anchor_age_blocks),
        "Upgrade anchor age bound"
    );
    ensure!(
        control.signatures.len() >= policy.authority.threshold,
        "Upgrade signature threshold"
    );
    let mut previous: Option<&str> = None;
    for signature in &control.signatures {
        ensure!(
            previous.is_none_or(|p| p < signature.key_id.as_str()),
            "Upgrade signatures must be strictly sorted"
        );
        ensure!(
            policy
                .authority
                .keys
                .iter()
                .any(|k| k.key_id == signature.key_id),
            "Upgrade authority key missing"
        );
        valid_hex(&signature.signature_hex, SIGNATURE_BYTES)?;
        previous = Some(&signature.key_id);
    }
    match &p.action {
        Action::Admit { plan } => {
            ensure!(
                state.pending.is_none() && state.active_schema == 0,
                "Pending or activated upgrade exists"
            );
            ensure!(
                p.sequence < u64::MAX - 1,
                "Admission must preserve an activation/cancellation sequence"
            );
            plan.validate(policy, &context.active_release_sha512)?;
        }
        Action::Activate {
            plan,
            admission_receipt_sha256,
            emergency_receipt_sha256,
            evidence_sha256,
        } => {
            plan.validate(policy, &context.active_release_sha512)?;
            valid_hex(evidence_sha256, 32)?;
            let pending = state.pending.as_ref().context("No admitted upgrade")?;
            ensure!(
                &pending.plan == plan
                    && &pending.admission_receipt_sha256 == admission_receipt_sha256,
                "Activation admission/plan differs"
            );
            ensure!(
                pending.admitted_height < context.height
                    && state.active_schema == plan.source_schema,
                "Activation height/source schema differs"
            );
            ensure!(
                pending
                    .admitted_height
                    .checked_add(policy.min_notice_blocks)
                    .is_some_and(|earliest| context.height >= earliest),
                "Upgrade activation precedes its notice"
            );
            ensure!(
                emergency_receipt_sha256 == &context.emergency_receipt_sha256,
                "Activation emergency clearance differs"
            );
        }
        Action::Cancel {
            plan_sha256,
            admission_receipt_sha256,
        } => {
            let pending = state.pending.as_ref().context("No admitted upgrade")?;
            ensure!(
                &pending.plan.sha256()? == plan_sha256
                    && &pending.admission_receipt_sha256 == admission_receipt_sha256,
                "Cancellation admission/plan differs"
            );
        }
    }
    Ok(())
}

fn prepare_index(
    history: &VerifiedEmergencyHistory,
    bounds: &MigrationBounds,
) -> Result<PreparedMigration> {
    let receipt_count = u64::try_from(history.receipts.len())?;
    ensure!(
        receipt_count <= bounds.max_receipts,
        "Migration receipt count limit"
    );
    let mut source = Sha256::new();
    source.update(b"DYTALLIX/EMERGENCY-INDEX-SOURCE/v1\0");
    let mut writes = BTreeMap::new();
    let mut source_bytes = 0u64;
    for receipt in &history.receipts {
        let bytes = emergency::encode_receipt(receipt)?;
        source_bytes = source_bytes
            .checked_add(u64::try_from(bytes.len())?)
            .context("Migration source size overflow")?;
        ensure!(
            source_bytes <= bounds.max_receipt_bytes,
            "Migration source byte limit"
        );
        source.update(u64::try_from(bytes.len())?.to_be_bytes());
        source.update(&bytes);
        let key = index_key(&receipt.sha256()?)?.into_bytes();
        ensure!(
            writes
                .insert(key, receipt.sequence().to_be_bytes().to_vec())
                .is_none(),
            "Duplicate emergency receipt digest"
        );
    }
    let mut index = Sha256::new();
    index.update(b"DYTALLIX/EMERGENCY-INDEX-MAPPINGS/v1\0");
    for (key, value) in &writes {
        index.update(u64::try_from(key.len())?.to_be_bytes());
        index.update(key);
        index.update(value);
    }
    Ok(PreparedMigration {
        writes,
        receipt_count,
        source_digest: hex::encode(source.finalize()),
        resulting_index_digest: hex::encode(index.finalize()),
    })
}
fn transition(
    policy: &Policy,
    state: &State,
    context: &BlockContext,
    history: &VerifiedEmergencyHistory,
    control: Control,
) -> Result<BlockPlan> {
    let mut migration = match &control.payload.action {
        Action::Activate { plan, .. } => Some(prepare_index(history, &plan.bounds)?),
        _ => None,
    };
    let receipt = Receipt {
        schema: 2,
        policy_sha256: policy.sha256()?,
        control: control.clone(),
        context: context.clone(),
        previous_receipt_sha256: state.last_receipt_sha256.clone(),
        migration_source_digest: migration.as_ref().map(|m| m.source_digest.clone()),
        migration_index_digest: migration.as_ref().map(|m| m.resulting_index_digest.clone()),
        migrated_receipts: migration.as_ref().map(|m| m.receipt_count),
    };
    let receipt_sha256 = receipt.sha256()?;
    let mut next = state.clone();
    match &control.payload.action {
        Action::Admit { plan } => {
            next.pending = Some(PendingPlan {
                plan: plan.clone(),
                admission_receipt_sha256: receipt_sha256.clone(),
                admitted_height: context.height,
            })
        }
        Action::Activate { plan, .. } => {
            next.pending = None;
            next.active_schema = plan.target_schema;
            next.activation_receipt_sha256 = Some(receipt_sha256.clone());
            let m = migration.as_mut().context("Activation migration missing")?;
            let marker = IndexState {
                schema: 1,
                activation_height: context.height,
                activation_receipt_sha256: receipt_sha256.clone(),
                backfilled_receipts: m.receipt_count,
            };
            m.writes.insert(
                INDEX_STATE_KEY.as_bytes().to_vec(),
                serde_json::to_vec(&marker)?,
            );
            let write_bytes = m.writes.iter().try_fold(0u64, |n, (k, v)| -> Result<u64> {
                n.checked_add(u64::try_from(k.len())?)
                    .and_then(|n| n.checked_add(v.len() as u64))
                    .context("Migration write size overflow")
            })?;
            ensure!(
                write_bytes <= plan.bounds.max_write_bytes,
                "Migration write byte limit"
            );
        }
        Action::Cancel { .. } => next.pending = None,
    }
    next.next_sequence = next
        .next_sequence
        .checked_add(1)
        .context("Upgrade sequence overflow")?;
    next.last_receipt_sha256 = Some(receipt_sha256);
    next.last_control_height = Some(context.height);
    next.validate(policy)?;
    Ok(BlockPlan {
        state: next,
        receipt: Some(receipt),
        migration,
    })
}

/// Reconstruct one immutable receipt using actual finalized facts supplied by the
/// adapter. None performs structural replay only; startup must supply a verifier.
pub fn replay_record(
    policy: &Policy,
    state: &State,
    receipt: &Receipt,
    actual_context: &BlockContext,
    history: &VerifiedEmergencyHistory,
    verifier: Option<&dyn Verifier>,
) -> Result<BlockPlan> {
    state.validate(policy)?;
    actual_context.validate()?;
    ensure!(
        &receipt.context == actual_context,
        "Upgrade receipt actual block differs"
    );
    let control = decode_control(policy, &serde_json::to_vec(&receipt.control)?)?;
    validate_control(policy, state, actual_context, history, &control)?;
    if let Some(verifier) = verifier {
        verify_signatures(policy, actual_context, &control, verifier)?;
    }
    let expected = transition(policy, state, actual_context, history, control)?;
    ensure!(
        expected.receipt.as_ref() == Some(receipt),
        "Upgrade receipt replay differs"
    );
    Ok(expected)
}
fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn valid_hex(value: &str, length: usize) -> Result<()> {
    ensure!(
        value.len() == length * 2
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "Noncanonical upgrade hex"
    );
    Ok(())
}
fn decode<T: Serialize + for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T> {
    let value: T = serde_json::from_slice(bytes)?;
    ensure!(
        serde_json::to_vec(&value)? == bytes,
        "Noncanonical upgrade JSON"
    );
    Ok(value)
}
#[cfg(test)]
#[path = "upgrade_tests.rs"]
mod tests;
