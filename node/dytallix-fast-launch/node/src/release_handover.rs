//! Release handover controls. Schema 1 binds one exact block (development
//! qualification). Schema 2 (production activation v1, step A3) uses the
//! upgrade custodians' three-of-five authority, signs over an anchored height
//! window as emergency freeze v2 does, and activates at least the policy's
//! notice after admission. Runtime artifact checks and the post-commit source
//! execution barrier belong to the consensus adapter.
use crate::{emergency_freeze as emergency, upgrade};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// Restart on a new release after a halt (E04 gap 18).
pub mod restart;

pub const CONTROL_KIND: &str = "dytallix-release-handover-v1";
pub const CONTROL_KIND_V2: &str = "dytallix-release-handover-v2";
pub const STATE_KEY: &str = "consensus:release-handover:v1:state";
pub const RECEIPT_PREFIX: &str = "consensus:release-handover:v1:receipt:";
const KEY_BYTES: usize = 64;
const SIGNATURE_BYTES: usize = 29_792;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub schema: u16,
    pub development_only: bool,
    pub chain_id: String,
    pub genesis_sha256: String,
    pub initial_release_sha512: String,
    pub initial_schema: u16,
    pub authority_epoch: u64,
    pub authority: emergency::AuthorityPolicy,
    pub initial_sequence: u64,
    pub max_control_bytes: usize,
    pub max_signatures: usize,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_non_null"
    )]
    pub v2: Option<PolicyV2>,
}
/// Schema 2 bounds. The authority must equal the upgrade policy's (the
/// adapter's configuration check).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyV2 {
    /// Blocks from admission to the earliest activation.
    pub min_notice_blocks: u64,
    /// Inclusive heights a signed window may span.
    pub max_validity_blocks: u64,
    /// Blocks from the signed anchor to the commit height.
    pub max_anchor_age_blocks: u64,
}
impl Policy {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (self.schema == 1 && self.v2.is_none()) || (self.schema == 2 && self.v2.is_some()),
            "Handover policy schema"
        );
        // Each build accepts only its own policies (production activation v1,
        // A4): production requires schema 2.
        ensure!(
            self.development_only != crate::build_profile::PRODUCTION,
            "Handover policy development flag differs from this build"
        );
        ensure!(
            !crate::build_profile::PRODUCTION || self.v2.is_some(),
            "A production handover policy is schema 2"
        );
        ensure!(
            !self.chain_id.is_empty()
                && self.chain_id.len() <= 128
                && self
                    .chain_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)),
            "Handover chain identity"
        );
        valid_hex(&self.genesis_sha256, 32)?;
        valid_hex(&self.initial_release_sha512, 64)?;
        ensure!(
            self.initial_schema <= 1,
            "Unsupported initial handover schema"
        );
        ensure!(
            self.authority_epoch > 0
                && self.initial_sequence > 0
                && self.initial_sequence < u64::MAX,
            "Handover authority epoch/sequence required"
        );
        ensure!(
            self.max_control_bytes > 0 && self.max_signatures > 0,
            "Handover wire bounds required"
        );
        ensure!(
            self.authority.threshold > 0
                && self.authority.threshold <= self.authority.keys.len()
                && self.authority.threshold <= self.max_signatures,
            "Handover authority threshold"
        );
        let mut previous: Option<&str> = None;
        let mut materials = BTreeSet::new();
        for key in &self.authority.keys {
            ensure!(
                !key.key_id.is_empty() && previous.is_none_or(|p| p < key.key_id.as_str()),
                "Handover keys must be strictly sorted"
            );
            valid_hex(&key.public_key_hex, KEY_BYTES)?;
            ensure!(
                materials.insert(&key.public_key_hex),
                "Duplicate handover authority key"
            );
            previous = Some(&key.key_id);
        }
        if let Some(v2) = &self.v2 {
            ensure!(
                self.authority.keys.len() == 5 && self.authority.threshold == 3,
                "V2 handover authority requires three of five keys"
            );
            for key in &self.authority.keys {
                ensure!(
                    key.key_id == hash(&hex::decode(&key.public_key_hex)?),
                    "V2 handover key ID must be the SHA-256 of its public key"
                );
            }
            ensure!(
                v2.min_notice_blocks > 0
                    && v2.max_validity_blocks > 0
                    && v2.max_anchor_age_blocks > 0,
                "Explicit v2 handover notice and window bounds required"
            );
        }
        Ok(())
    }
    /// The policy's hash. A schema 2 policy hashes its rules, not its keys:
    /// the key sets and the authority epoch are left out (root kit
    /// replacement v1), so a replacement changes neither this hash nor the
    /// states, receipts and payloads that bind it; every schema 2 payload
    /// signs its authority epoch, and only that epoch's keys verify it.
    pub fn sha256(&self) -> Result<String> {
        self.validate()?;
        let mut rules = self.clone();
        if rules.v2.is_some() {
            rules.authority_epoch = 0;
            rules.authority.keys.clear();
        }
        Ok(hash(&serde_json::to_vec(&rules)?))
    }
    fn control_kind(&self) -> &'static str {
        if self.v2.is_some() {
            CONTROL_KIND_V2
        } else {
            CONTROL_KIND
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "transition", rename_all = "snake_case", deny_unknown_fields)]
pub enum Transition {
    ReceiptIndexV1 {
        migration_sha256: String,
    },
    /// A release change with no state-schema migration. Never label it migration.
    SchemaPreserving {
        schema: u16,
    },
}
impl Transition {
    fn schemas(&self) -> (u16, u16) {
        match self {
            Self::ReceiptIndexV1 { .. } => (0, 1),
            Self::SchemaPreserving { schema } => (*schema, *schema),
        }
    }
    fn validate(&self, policy: &Policy) -> Result<()> {
        match self {
            Self::ReceiptIndexV1 { migration_sha256 } => {
                upgrade::validate_registry()?;
                // The paired upgrade runs the migration: v1 under schema 1,
                // v2 under schema 2.
                let expected = if policy.v2.is_some() {
                    upgrade::v2::migration_sha256()
                } else {
                    upgrade::v1::migration_sha256()
                };
                ensure!(
                    *migration_sha256 == expected,
                    "Handover migration version differs"
                );
            }
            Self::SchemaPreserving { schema } => {
                ensure!(*schema <= 1, "Unsupported preserved schema")
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleasePlan {
    pub target_release_sha512: String,
    pub transition: Transition,
    pub authorization_sha256: String,
}
impl ReleasePlan {
    pub fn sha256(&self) -> Result<String> {
        Ok(hash(&serde_json::to_vec(self)?))
    }
    fn validate(&self, policy: &Policy, state: &State) -> Result<()> {
        valid_hex(&self.target_release_sha512, 64)?;
        valid_hex(&self.authorization_sha256, 32)?;
        ensure!(
            self.target_release_sha512 != state.active_release_sha512,
            "Handover target must differ from source release"
        );
        self.transition.validate(policy)?;
        ensure!(
            self.transition.schemas().0 == state.active_schema,
            "Handover source schema differs"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Admit {
        plan: ReleasePlan,
    },
    Activate {
        plan: ReleasePlan,
        admission_receipt_sha256: String,
        emergency_receipt_sha256: Option<String>,
        evidence_sha256: String,
        upgrade_activation_sha256: Option<String>,
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
    pub source_release_sha512: String,
    pub authority_epoch: u64,
    pub sequence: u64,
    /// Schema 1: the block's parent. Schema 2: the signed finalized anchor.
    pub parent_height: u64,
    pub parent_app_hash: String,
    /// Schema 1: the block. Schema 2: the window's first height.
    pub target_height: u64,
    pub action: Action,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_non_null"
    )]
    pub v2: Option<PayloadV2>,
}
/// The signed window of a schema 2 control (emergency freeze v2's scheme).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PayloadV2 {
    pub not_before_height: u64,
    pub not_after_height: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    pub kind: String,
    pub payload: Payload,
    pub signatures: Vec<emergency::ControlSignature>,
}
pub fn artifact_bytes(payload: &Payload) -> Result<Vec<u8>> {
    let domain: &[u8] = match (payload.schema, payload.v2.is_some()) {
        (1, false) => b"DYTALLIX/RELEASE-HANDOVER/v1\0",
        (2, true) => b"DYTALLIX/RELEASE-HANDOVER/v2\0",
        _ => anyhow::bail!("Handover artifact schema"),
    };
    let mut bytes = domain.to_vec();
    bytes.extend_from_slice(&serde_json::to_vec(payload)?);
    Ok(bytes)
}
pub use upgrade::Verifier;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockContext {
    pub height: u64,
    pub parent_height: u64,
    pub parent_app_hash: String,
    pub source_schema: u16,
    pub emergency_frozen: bool,
    pub emergency_control_present: bool,
    pub emergency_upgrade_hold: bool,
    pub emergency_receipt_sha256: Option<String>,
    /// The anchor a schema 2 control names, read from committed history by
    /// the adapter. Never taken from the control itself.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_non_null"
    )]
    pub finalized_anchor: Option<emergency::FinalizedAnchor>,
}
impl BlockContext {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.parent_height.checked_add(1) == Some(self.height),
            "Handover block continuity"
        );
        valid_hex(&self.parent_app_hash, 32)?;
        if let Some(anchor) = &self.finalized_anchor {
            ensure!(
                anchor.height <= self.parent_height,
                "Handover anchor is not finalized"
            );
            valid_hex(&anchor.app_hash, 32)?;
            ensure!(
                anchor.height != self.parent_height || anchor.app_hash == self.parent_app_hash,
                "Handover parent anchor mismatch"
            );
        }
        if let Some(value) = &self.emergency_receipt_sha256 {
            valid_hex(value, 32)?;
        }
        ensure!(
            self.emergency_upgrade_hold == self.emergency_receipt_sha256.is_some(),
            "Handover hold/history mismatch"
        );
        ensure!(
            !self.emergency_frozen || self.emergency_upgrade_hold,
            "Frozen handover context requires history"
        );
        ensure!(
            self.source_schema <= 1,
            "Unsupported handover context schema"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingPlan {
    pub plan: ReleasePlan,
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
    active_release_sha512: String,
    active_schema: u16,
    last_receipt_sha256: Option<String>,
    last_control_height: Option<u64>,
    activation_receipt_sha256: Option<String>,
}
impl State {
    pub fn new(policy: &Policy) -> Result<Self> {
        Ok(Self {
            schema: 1,
            policy_sha256: policy.sha256()?,
            next_sequence: policy.initial_sequence,
            pending: None,
            active_release_sha512: policy.initial_release_sha512.clone(),
            active_schema: policy.initial_schema,
            last_receipt_sha256: None,
            last_control_height: None,
            activation_receipt_sha256: None,
        })
    }
    pub fn validate(&self, policy: &Policy) -> Result<()> {
        ensure!(
            self.schema == 1 && self.policy_sha256 == policy.sha256()?,
            "Handover state policy binding"
        );
        ensure!(
            self.next_sequence >= policy.initial_sequence && self.active_schema <= 1,
            "Handover sequence/schema invalid"
        );
        valid_hex(&self.active_release_sha512, 64)?;
        let initial = self.next_sequence == policy.initial_sequence;
        ensure!(
            initial == self.last_receipt_sha256.is_none()
                && initial == self.last_control_height.is_none(),
            "Handover state history mismatch"
        );
        ensure!(
            !initial || (self.pending.is_none() && self.activation_receipt_sha256.is_none()),
            "Initial handover history altered"
        );
        ensure!(
            self.activation_receipt_sha256.is_some()
                || (self.active_release_sha512 == policy.initial_release_sha512
                    && self.active_schema == policy.initial_schema),
            "Handover active release lacks activation"
        );
        for digest in [&self.last_receipt_sha256, &self.activation_receipt_sha256]
            .into_iter()
            .flatten()
        {
            valid_hex(digest, 32)?;
        }
        if let Some(pending) = &self.pending {
            ensure!(
                pending.admitted_height > 0
                    && Some(pending.admitted_height) <= self.last_control_height,
                "Handover pending height invalid"
            );
            pending.plan.validate(policy, self)?;
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
    pub fn active_release_sha512(&self) -> &str {
        &self.active_release_sha512
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

/// The trusted adapter constructs this only after successful signed upgrade
/// planning or verified upgrade history replay, of either upgrade version. It
/// is not a wire input or a signature verifier.
#[derive(Clone, Debug)]
pub struct PreparedUpgradeOutcome {
    chain_id: String,
    genesis_sha256: String,
    control_sha256: String,
    receipt_sha256: String,
    migration_sha256: String,
    source_schema: u16,
    target_schema: u16,
    context: PairedContext,
}
/// The block facts both controls of a pair must share.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PairedContext {
    height: u64,
    parent_height: u64,
    parent_app_hash: String,
    emergency_frozen: bool,
    emergency_control_present: bool,
    emergency_upgrade_hold: bool,
    emergency_receipt_sha256: Option<String>,
}
/// The facts of one prepared activation, whichever upgrade version made it.
struct Activation<'a> {
    chain_id: &'a str,
    genesis_sha256: &'a str,
    migration_id: &'a str,
    migration_sha256: &'a str,
    source_schema: u16,
    target_schema: u16,
    source_digest: Option<&'a String>,
    index_digest: Option<&'a String>,
    migrated_receipts: Option<u64>,
    context: PairedContext,
}
impl PreparedUpgradeOutcome {
    pub(crate) fn from_verified_upgrade(raw: &[u8], prepared: &upgrade::BlockPlan) -> Result<Self> {
        let receipt = prepared
            .receipt
            .as_ref()
            .context("Prepared upgrade receipt required")?;
        ensure!(
            receipt.control_bytes()? == raw,
            "Prepared upgrade control bytes differ"
        );
        let (activation, expected_migration) = match receipt {
            upgrade::Receipt::V1(r) => {
                let upgrade::v1::Action::Activate { plan, .. } = &r.control.payload.action else {
                    anyhow::bail!("Prepared upgrade is not activation");
                };
                let c = &r.context;
                (
                    Activation {
                        chain_id: &r.control.payload.chain_id,
                        genesis_sha256: &r.control.payload.genesis_sha256,
                        migration_id: &plan.migration_id,
                        migration_sha256: &plan.migration_sha256,
                        source_schema: plan.source_schema,
                        target_schema: plan.target_schema,
                        source_digest: r.migration_source_digest.as_ref(),
                        index_digest: r.migration_index_digest.as_ref(),
                        migrated_receipts: r.migrated_receipts,
                        context: PairedContext {
                            height: c.height,
                            parent_height: c.parent_height,
                            parent_app_hash: c.parent_app_hash.clone(),
                            emergency_frozen: c.emergency_frozen,
                            emergency_control_present: c.emergency_control_present,
                            emergency_upgrade_hold: c.emergency_upgrade_hold,
                            emergency_receipt_sha256: c.emergency_receipt_sha256.clone(),
                        },
                    },
                    upgrade::v1::migration_sha256(),
                )
            }
            upgrade::Receipt::V2(r) => {
                let upgrade::v2::Action::Activate { plan, .. } = &r.control.payload.action else {
                    anyhow::bail!("Prepared upgrade is not activation");
                };
                let c = &r.context;
                (
                    Activation {
                        chain_id: &r.control.payload.chain_id,
                        genesis_sha256: &r.control.payload.genesis_sha256,
                        migration_id: &plan.migration_id,
                        migration_sha256: &plan.migration_sha256,
                        source_schema: plan.source_schema,
                        target_schema: plan.target_schema,
                        source_digest: r.migration_source_digest.as_ref(),
                        index_digest: r.migration_index_digest.as_ref(),
                        migrated_receipts: r.migrated_receipts,
                        context: PairedContext {
                            height: c.height,
                            parent_height: c.parent_height,
                            parent_app_hash: c.parent_app_hash.clone(),
                            emergency_frozen: c.emergency_frozen,
                            emergency_control_present: c.emergency_control_present,
                            emergency_upgrade_hold: c.emergency_upgrade_hold,
                            emergency_receipt_sha256: c.emergency_receipt_sha256.clone(),
                        },
                    },
                    upgrade::v2::migration_sha256(),
                )
            }
        };
        let migration = prepared
            .migration
            .as_ref()
            .context("Prepared actual migration required")?;
        let receipt_sha256 = receipt.sha256()?;
        ensure!(
            activation.migration_id == upgrade::MIGRATION_ID
                && activation.migration_sha256 == expected_migration
                && activation.source_schema == 0
                && activation.target_schema == 1,
            "Prepared migration version differs"
        );
        ensure!(
            prepared.state.active_schema() == activation.target_schema
                && prepared.state.activation_receipt_sha256() == Some(receipt_sha256.as_str()),
            "Prepared upgrade state differs"
        );
        ensure!(
            activation.source_digest == Some(&migration.source_digest)
                && activation.index_digest == Some(&migration.resulting_index_digest)
                && activation.migrated_receipts == Some(migration.receipt_count),
            "Prepared migration receipt outcome differs"
        );
        Ok(Self {
            chain_id: activation.chain_id.to_owned(),
            genesis_sha256: activation.genesis_sha256.to_owned(),
            control_sha256: hash(raw),
            receipt_sha256,
            migration_sha256: activation.migration_sha256.to_owned(),
            source_schema: activation.source_schema,
            target_schema: activation.target_schema,
            context: activation.context,
        })
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
    pub upgrade_receipt_sha256: Option<String>,
}
impl Receipt {
    pub fn sha256(&self) -> Result<String> {
        Ok(hash(&serde_json::to_vec(self)?))
    }
    pub fn sequence(&self) -> u64 {
        self.control.payload.sequence
    }
}
#[derive(Clone, Debug)]
pub struct BlockPlan {
    pub state: State,
    pub receipt: Option<Receipt>,
    /// A restart applied before the block's transactions (restart v1).
    pub restart: Option<restart::Receipt>,
    /// Commit this handover before stopping source execution. No binary writes.
    pub activated_release_sha512: Option<String>,
}
#[derive(Debug)]
pub struct Rejected(pub String);
impl std::fmt::Display for Rejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for Rejected {}
pub fn is_rejection(error: &anyhow::Error) -> bool {
    error.downcast_ref::<Rejected>().is_some()
}
pub fn receipt_key(sequence: u64) -> String {
    format!("{RECEIPT_PREFIX}{sequence:020}")
}
pub fn encode_state(state: &State) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(state)?)
}
pub fn decode_state(policy: &Policy, bytes: &[u8]) -> Result<State> {
    let state: State = decode(bytes)?;
    state.validate(policy)?;
    Ok(state)
}
pub fn encode_receipt(receipt: &Receipt) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(receipt)?)
}
pub fn decode_receipt(policy: &Policy, bytes: &[u8]) -> Result<Receipt> {
    let receipt: Receipt = decode(bytes)?;
    ensure!(
        receipt.schema == policy.schema && receipt.policy_sha256 == policy.sha256()?,
        "Handover receipt policy differs"
    );
    decode_control(policy, &serde_json::to_vec(&receipt.control)?)?;
    receipt.context.validate()?;
    for digest in [
        &receipt.previous_receipt_sha256,
        &receipt.upgrade_receipt_sha256,
    ]
    .into_iter()
    .flatten()
    {
        valid_hex(digest, 32)?;
    }
    Ok(receipt)
}
pub fn decode_control(policy: &Policy, bytes: &[u8]) -> Result<Control> {
    policy.validate()?;
    ensure!(
        bytes.len() <= policy.max_control_bytes,
        "Handover control exceeds byte bound"
    );
    let control: Control = decode(bytes)?;
    ensure!(
        control.kind == policy.control_kind() && control.signatures.len() <= policy.max_signatures,
        "Handover kind/signature bound"
    );
    Ok(control)
}

/// Check one mempool control without changing state. This verifies its complete
/// signed admission context. Only the existence and binding of the actual
/// same-block prepared v1 migration is deferred to `plan_block`.
pub fn check_admission(
    policy: &Policy,
    state: &State,
    context: &BlockContext,
    raw: &[u8],
    verifier: &dyn Verifier,
) -> Result<()> {
    state.validate(policy)?;
    context.validate()?;
    ensure!(
        state.active_schema == context.source_schema,
        "Handover source state/schema mismatch"
    );
    let control = decode_control(policy, raw).map_err(|e| Rejected(e.to_string()))?;
    validate_admission(policy, state, context, &control).map_err(|e| Rejected(e.to_string()))?;
    verify_signatures(policy, context, &control, verifier)
}

pub fn plan_block(
    policy: &Policy,
    state: &State,
    context: &BlockContext,
    raw: Option<&[u8]>,
    migration: Option<&PreparedUpgradeOutcome>,
    verifier: &dyn Verifier,
) -> Result<BlockPlan> {
    state.validate(policy)?;
    context.validate()?;
    ensure!(
        state.active_schema == context.source_schema,
        "Handover source state/schema mismatch"
    );
    let Some(raw) = raw else {
        ensure!(migration.is_none(), "Unpaired schema-changing upgrade");
        return Ok(BlockPlan {
            state: state.clone(),
            receipt: None,
            restart: None,
            activated_release_sha512: None,
        });
    };
    let control = decode_control(policy, raw).map_err(|e| Rejected(e.to_string()))?;
    validate_control(policy, state, context, &control, migration)
        .map_err(|e| Rejected(e.to_string()))?;
    verify_signatures(policy, context, &control, verifier)?;
    transition(policy, state, context, control, migration)
}
fn verify_signatures(
    policy: &Policy,
    context: &BlockContext,
    control: &Control,
    verifier: &dyn Verifier,
) -> Result<()> {
    let artifact = artifact_bytes(&control.payload)?;
    // Schema 2 signs a window; schema 1 the one block.
    let (first, last) = control
        .payload
        .v2
        .as_ref()
        .map_or((context.height, context.height), |window| {
            (window.not_before_height, window.not_after_height)
        });
    for signature in &control.signatures {
        let key = policy
            .authority
            .keys
            .iter()
            .find(|key| key.key_id == signature.key_id)
            .context("Handover trusted key absent")?;
        let valid = verifier.verify(
            &policy.chain_id,
            &hex::decode(&key.public_key_hex)?,
            control.payload.sequence,
            context.height,
            first,
            last,
            &artifact,
            &hex::decode(&signature.signature_hex)?,
        )?;
        ensure!(valid, Rejected("Handover signature rejected".into()));
    }
    Ok(())
}
fn validate_admission(
    policy: &Policy,
    state: &State,
    context: &BlockContext,
    control: &Control,
) -> Result<()> {
    ensure!(
        !context.emergency_frozen && !context.emergency_control_present,
        "Emergency control blocks handover"
    );
    ensure!(
        state.last_control_height.is_none_or(|h| h < context.height),
        "Handover height already consumed"
    );
    let payload = &control.payload;
    ensure!(
        payload.schema == policy.schema
            && payload.v2.is_some() == policy.v2.is_some()
            && payload.chain_id == policy.chain_id
            && payload.genesis_sha256 == policy.genesis_sha256
            && payload.policy_sha256 == policy.sha256()?
            && payload.source_release_sha512 == state.active_release_sha512
            && payload.authority_epoch == policy.authority_epoch,
        "Handover identity/policy binding"
    );
    ensure!(
        payload.sequence == state.next_sequence && payload.sequence < u64::MAX,
        "Handover sequence mismatch/exhaustion"
    );
    match (&policy.v2, &payload.v2) {
        (Some(bounds), Some(window)) => validate_window(bounds, context, payload, window)?,
        _ => {
            ensure!(
                context.finalized_anchor.is_none(),
                "A v1 handover context carries no anchor"
            );
            ensure!(
                payload.parent_height == context.parent_height
                    && payload.parent_app_hash == context.parent_app_hash
                    && payload.target_height == context.height,
                "Handover finalized parent/target binding"
            );
        }
    }
    ensure!(
        control.signatures.len() >= policy.authority.threshold,
        "Handover signature threshold"
    );
    let mut previous: Option<&str> = None;
    for signature in &control.signatures {
        ensure!(
            previous.is_none_or(|p| p < signature.key_id.as_str()),
            "Handover signatures must be strictly sorted"
        );
        ensure!(
            policy
                .authority
                .keys
                .iter()
                .any(|key| key.key_id == signature.key_id),
            "Handover authority key missing"
        );
        valid_hex(&signature.signature_hex, SIGNATURE_BYTES)?;
        previous = Some(&signature.key_id);
    }
    match &payload.action {
        Action::Admit { plan } => {
            ensure!(state.pending.is_none(), "Handover pending plan exists");
            ensure!(
                payload.sequence < u64::MAX - 1,
                "Admission must preserve a completion sequence"
            );
            plan.validate(policy, state)?;
        }
        Action::Cancel {
            plan_sha256,
            admission_receipt_sha256,
        } => {
            let pending = state.pending.as_ref().context("No admitted handover")?;
            ensure!(
                pending.plan.sha256()? == *plan_sha256
                    && pending.admission_receipt_sha256 == *admission_receipt_sha256,
                "Handover cancellation plan/receipt differs"
            );
        }
        Action::Activate {
            plan,
            admission_receipt_sha256,
            emergency_receipt_sha256,
            evidence_sha256,
            upgrade_activation_sha256,
        } => {
            plan.validate(policy, state)?;
            valid_hex(evidence_sha256, 32)?;
            let pending = state.pending.as_ref().context("No admitted handover")?;
            ensure!(
                &pending.plan == plan
                    && &pending.admission_receipt_sha256 == admission_receipt_sha256
                    && pending.admitted_height < context.height,
                "Handover activation plan/receipt/height differs"
            );
            // Operators install the named release before activation (D11-Q03,
            // P01 1 October 2026: the notice applies to handovers too).
            if let Some(bounds) = &policy.v2 {
                ensure!(
                    pending
                        .admitted_height
                        .checked_add(bounds.min_notice_blocks)
                        .is_some_and(|earliest| context.height >= earliest),
                    "Handover activation precedes its notice"
                );
            }
            ensure!(
                emergency_receipt_sha256 == &context.emergency_receipt_sha256,
                "Handover emergency clearance differs"
            );
            match &plan.transition {
                Transition::ReceiptIndexV1 { .. } => {
                    let digest = upgrade_activation_sha256
                        .as_deref()
                        .context("Handover paired upgrade control hash required")?;
                    valid_hex(digest, 32)?;
                }
                Transition::SchemaPreserving { .. } => ensure!(
                    upgrade_activation_sha256.is_none(),
                    "Schema-preserving handover cannot name a migration"
                ),
            }
        }
    }
    Ok(())
}
fn validate_control(
    policy: &Policy,
    state: &State,
    context: &BlockContext,
    control: &Control,
    migration: Option<&PreparedUpgradeOutcome>,
) -> Result<()> {
    validate_admission(policy, state, context, control)?;
    match &control.payload.action {
        Action::Activate {
            plan:
                ReleasePlan {
                    transition: Transition::ReceiptIndexV1 { migration_sha256 },
                    ..
                },
            upgrade_activation_sha256,
            ..
        } => {
            let prepared = migration.context("Handover requires verified paired migration")?;
            ensure!(
                prepared.chain_id == policy.chain_id
                    && prepared.genesis_sha256 == policy.genesis_sha256,
                "Paired migration chain/genesis differs"
            );
            ensure!(
                upgrade_activation_sha256.as_deref() == Some(prepared.control_sha256.as_str()),
                "Paired upgrade control hash differs"
            );
            ensure!(
                prepared.migration_sha256 == *migration_sha256
                    && (prepared.source_schema, prepared.target_schema) == (0, 1),
                "Paired migration schema/version differs"
            );
            let actual = &prepared.context;
            ensure!(
                actual.height == context.height
                    && actual.parent_height == context.parent_height
                    && actual.parent_app_hash == context.parent_app_hash
                    && actual.emergency_frozen == context.emergency_frozen
                    && actual.emergency_control_present == context.emergency_control_present
                    && actual.emergency_upgrade_hold == context.emergency_upgrade_hold
                    && actual.emergency_receipt_sha256 == context.emergency_receipt_sha256,
                "Paired migration finalized context differs"
            );
        }
        _ => ensure!(
            migration.is_none(),
            "Handover action cannot consume a migration"
        ),
    }
    Ok(())
}

fn transition(
    policy: &Policy,
    state: &State,
    context: &BlockContext,
    control: Control,
    migration: Option<&PreparedUpgradeOutcome>,
) -> Result<BlockPlan> {
    let receipt = Receipt {
        schema: policy.schema,
        policy_sha256: policy.sha256()?,
        control: control.clone(),
        context: context.clone(),
        previous_receipt_sha256: state.last_receipt_sha256.clone(),
        upgrade_receipt_sha256: migration.map(|m| m.receipt_sha256.clone()),
    };
    let digest = receipt.sha256()?;
    let mut next = state.clone();
    let activated_release_sha512 = match &control.payload.action {
        Action::Admit { plan } => {
            next.pending = Some(PendingPlan {
                plan: plan.clone(),
                admission_receipt_sha256: digest.clone(),
                admitted_height: context.height,
            });
            None
        }
        Action::Cancel { .. } => {
            next.pending = None;
            None
        }
        Action::Activate { plan, .. } => {
            next.pending = None;
            next.active_release_sha512 = plan.target_release_sha512.clone();
            next.active_schema = plan.transition.schemas().1;
            next.activation_receipt_sha256 = Some(digest.clone());
            Some(plan.target_release_sha512.clone())
        }
    };
    next.next_sequence = next
        .next_sequence
        .checked_add(1)
        .context("Handover sequence overflow")?;
    next.last_receipt_sha256 = Some(digest);
    next.last_control_height = Some(context.height);
    next.validate(policy)?;
    Ok(BlockPlan {
        state: next,
        receipt: Some(receipt),
        restart: None,
        activated_release_sha512,
    })
}
/// The adapter supplies actual historical context and independently replayed
/// migration outcome. Startup must supply the real signature verifier.
pub fn replay_record(
    policy: &Policy,
    state: &State,
    receipt: &Receipt,
    actual_context: &BlockContext,
    migration: Option<&PreparedUpgradeOutcome>,
    verifier: Option<&dyn Verifier>,
) -> Result<BlockPlan> {
    state.validate(policy)?;
    actual_context.validate()?;
    ensure!(
        state.active_schema == actual_context.source_schema && &receipt.context == actual_context,
        "Handover receipt actual context differs"
    );
    let control = decode_control(policy, &serde_json::to_vec(&receipt.control)?)?;
    validate_control(policy, state, actual_context, &control, migration)?;
    if let Some(verifier) = verifier {
        verify_signatures(policy, actual_context, &control, verifier)?;
    }
    let expected = transition(policy, state, actual_context, control, migration)?;
    ensure!(
        expected.receipt.as_ref() == Some(receipt),
        "Handover receipt replay differs"
    );
    Ok(expected)
}
/// The anchored window of a schema 2 control: the payload's parent is the
/// signed finalized anchor and its target the window's first height.
fn validate_window(
    bounds: &PolicyV2,
    context: &BlockContext,
    payload: &Payload,
    window: &PayloadV2,
) -> Result<()> {
    ensure!(
        window.not_before_height > 0
            && payload.target_height == window.not_before_height
            && window.not_after_height >= window.not_before_height
            && window.not_after_height - window.not_before_height < bounds.max_validity_blocks,
        "Handover validity window bound"
    );
    ensure!(
        context.height >= window.not_before_height && context.height <= window.not_after_height,
        "Handover outside its validity window"
    );
    let anchor = context
        .finalized_anchor
        .as_ref()
        .context("Finalized handover anchor required")?;
    ensure!(
        payload.parent_height == anchor.height && payload.parent_app_hash == anchor.app_hash,
        "Handover finalized anchor binding"
    );
    ensure!(
        anchor.height < window.not_before_height
            && context
                .height
                .checked_sub(anchor.height)
                .is_some_and(|age| age <= bounds.max_anchor_age_blocks),
        "Handover anchor age bound"
    );
    Ok(())
}
/// The anchor height a control names: schema 2 only.
pub fn control_anchor_height(policy: &Policy, raw: &[u8]) -> Result<Option<u64>> {
    let control = decode_control(policy, raw)?;
    Ok(control
        .payload
        .v2
        .is_some()
        .then_some(control.payload.parent_height))
}
fn present_non_null<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    // Missing fields use serde's default. Explicit null is not a missing field.
    T::deserialize(deserializer).map(Some)
}
fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn valid_hex(value: &str, bytes: usize) -> Result<()> {
    ensure!(
        value.len() == bytes * 2
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "Noncanonical handover hex"
    );
    Ok(())
}
fn decode<T: Serialize + for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T> {
    let value: T = serde_json::from_slice(bytes)?;
    ensure!(
        serde_json::to_vec(&value)? == bytes,
        "Noncanonical handover JSON"
    );
    Ok(value)
}
#[cfg(test)]
#[path = "release_handover_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "release_handover_v2_tests.rs"]
mod v2_tests;
