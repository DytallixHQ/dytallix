//! Explicit development qualification of a chain-atomic receipt-index upgrade.
//! No production authority, installation, automatic schedule or global hold reset.
use crate::emergency_freeze as emergency;
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const CONTROL_KIND: &str = "dytallix-upgrade-control-v1";
pub const STATE_KEY: &str = "consensus:upgrade:v1:state";
pub const RECEIPT_PREFIX: &str = "consensus:upgrade:v1:receipt:";
pub const INDEX_PREFIX: &str = "consensus:emergency:v1:receipt-by-sha256:";
pub const INDEX_STATE_KEY: &str = "consensus:emergency:v1:receipt-index";
pub const MIGRATION_ID: &str = "emergency-receipt-digest-index-v1";
const KEY_BYTES: usize = 64;
const SIGNATURE_BYTES: usize = 29_792;

/// Binds the compiled migration source, not an arbitrary submitted digest.
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
    pub development_only: bool,
    pub chain_id: String,
    pub genesis_sha256: String,
    pub source_release_sha512: String,
    pub authority_epoch: u64,
    pub authority: emergency::AuthorityPolicy,
    pub initial_sequence: u64,
    pub max_control_bytes: usize,
    pub max_signatures: usize,
    pub migration_bounds: MigrationBounds,
}
impl Policy {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema == 1 && self.development_only,
            "Production upgrade activation is not qualified"
        );
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
        valid_hex(&self.source_release_sha512, 64)?;
        ensure!(
            self.authority_epoch > 0
                && self.initial_sequence > 0
                && self.initial_sequence < u64::MAX,
            "Upgrade authority epoch/sequence required"
        );
        ensure!(
            self.max_control_bytes > 0 && self.max_signatures > 0,
            "Upgrade wire bounds required"
        );
        ensure!(
            self.authority.threshold > 0
                && self.authority.threshold <= self.authority.keys.len()
                && self.authority.threshold <= self.max_signatures,
            "Upgrade authority threshold"
        );
        let mut previous: Option<&str> = None;
        let mut materials = BTreeSet::new();
        for key in &self.authority.keys {
            ensure!(
                !key.key_id.is_empty() && previous.is_none_or(|p| p < key.key_id.as_str()),
                "Upgrade keys must be strictly sorted"
            );
            valid_hex(&key.public_key_hex, KEY_BYTES)?;
            ensure!(
                materials.insert(&key.public_key_hex),
                "Duplicate upgrade authority key"
            );
            previous = Some(&key.key_id);
        }
        self.migration_bounds.validate()
    }
    pub fn sha256(&self) -> Result<String> {
        self.validate()?;
        Ok(hash(&serde_json::to_vec(self)?))
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
    fn validate(&self, policy: &Policy) -> Result<()> {
        ensure!(
            self.target_release_sha512 == policy.source_release_sha512,
            "Migration requires the root-bound running candidate"
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
    pub source_release_sha512: String,
    pub authority_epoch: u64,
    pub sequence: u64,
    pub parent_height: u64,
    pub parent_app_hash: String,
    pub target_height: u64,
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
    let mut bytes = b"DYTALLIX/CHAIN-UPGRADE/v1\0".to_vec();
    bytes.extend_from_slice(&serde_json::to_vec(payload)?);
    Ok(bytes)
}
pub trait Verifier: Send + Sync {
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
pub struct BlockContext {
    pub height: u64,
    pub parent_height: u64,
    pub parent_app_hash: String,
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
    active_release_sha512: String,
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
            active_schema: 0,
            active_release_sha512: policy.source_release_sha512.clone(),
            last_receipt_sha256: None,
            last_control_height: None,
            activation_receipt_sha256: None,
        })
    }
    pub fn validate(&self, policy: &Policy) -> Result<()> {
        ensure!(
            self.schema == 1 && self.policy_sha256 == policy.sha256()?,
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
        ensure!(
            self.active_release_sha512 == policy.source_release_sha512 && self.active_schema <= 1,
            "Upgrade active schema/candidate mismatch"
        );
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
            pending.plan.validate(policy)?;
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
    pub fn active_release_sha512(&self) -> &str {
        &self.active_release_sha512
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
    pub fn from_records(
        policy: &emergency::Policy,
        records: &[(emergency::Receipt, emergency::BlockContext)],
        expected_state: &emergency::State,
    ) -> Result<Self> {
        ensure!(
            emergency::recover_recorded(policy, records)? == *expected_state,
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
                .checked_add(4096)
                .context("Upgrade receipt bound overflow")?,
        "Upgrade receipt byte limit"
    );
    decode(bytes)
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
            control.payload.sequence,
            context.height,
            context.height,
            context.height,
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
        p.schema == 1
            && p.chain_id == policy.chain_id
            && p.genesis_sha256 == policy.genesis_sha256
            && p.source_release_sha512 == policy.source_release_sha512
            && p.authority_epoch == policy.authority_epoch,
        "Upgrade identity/policy binding"
    );
    ensure!(
        p.sequence == state.next_sequence && p.sequence < u64::MAX,
        "Upgrade sequence mismatch/exhaustion"
    );
    ensure!(
        p.parent_height == context.parent_height
            && p.parent_app_hash == context.parent_app_hash
            && p.target_height == context.height,
        "Upgrade finalized parent/target binding"
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
            plan.validate(policy)?;
        }
        Action::Activate {
            plan,
            admission_receipt_sha256,
            emergency_receipt_sha256,
            evidence_sha256,
        } => {
            plan.validate(policy)?;
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
        schema: 1,
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
