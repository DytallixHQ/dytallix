//! CometBFT application execution for bounded local qualification.
//! The engine supplies finality. This module never creates a finality certificate.
use crate::emergency_freeze::{self as emergency, ControlVerifier};
use crate::emergency_verifier::{EmergencyVerifier, EmergencyVerifierConfig};
use crate::governance_signed_admission::{AdmissionParent, PreadmissionAssessment};
use crate::ordinary_execution::{self as ordinary_runtime, OrdinaryExecutionResult};
use crate::ordinary_fee_settlement::Outcome as OrdinaryOutcome;
use crate::ordinary_meter::{RecoveryCeilings, SharedBlockMeter};
use crate::ordinary_reservations::{ReservationId, ReservationLedger, ReservationRequest};
use crate::ordinary_state::{self, OrdinaryConfig, OrdinaryState, STATE_KEY as ORDINARY_STATE_KEY};
use crate::recovery_fees::{RecoveryBook, RecoveryResult, STATE_KEY as RECOVERY_STATE_KEY};
use crate::{
    block_lifecycle::{self, Writes},
    execution::stage_transaction,
    gas::GasSchedule,
    runtime::{
        emission::EmissionEngine,
        governance_candidate::GovernanceCandidateConfig,
        governance_state::{
            GovernanceState, GOVERNANCE_STATE_VERSION, STATE_KEY as GOVERNANCE_STATE_KEY,
        },
        issuance_timing::{EpochObservation, TimingState, TIMING_STATE_KEY},
        penalty_custody::{
            EvidenceFact, PenaltyConfig, PenaltyState, STATE_KEY as PENALTY_STATE_KEY,
        },
        reward_runtime::{RewardState, REWARD_STATE_KEY},
        validator_lifecycle::{
            LifecycleConfig, LifecycleState, ValidatorIdentity, ValidatorUpdate,
            STATE_KEY as LIFECYCLE_STATE_KEY,
        },
    },
    settlement::{self, Settlement},
    state::State,
    storage::{
        receipts::TxReceipt,
        state::Storage,
        transaction_record::{SignedEnvelope, TransactionRecord},
    },
    types::SignedTx,
};
use crate::{release_handover as handover, upgrade};
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use dytallix_protocol_types::recovery_sponsor as sponsor_wire;
use dytallix_protocol_types::ordinary_v3::SignedOrdinary as SignedOrdinaryV3;
use dytallix_protocol_types::{ordinary as ordinary_wire, ordinary_fees as ordinary_fee_wire};
use dytallix_storage::adaptive::PreparedJournalUpdate;
use rocksdb::{IteratorMode, WriteBatch, WriteOptions};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{Arc, Mutex},
};

pub const MODE_KEY: &str = "consensus:v1:config";
const HEAD_KEY: &str = "consensus:v1:head";
const BLOCK_PREFIX: &str = "consensus:v1:block:";
const GENESIS_SOURCE_KEY: &str = "consensus:v1:genesis_source";
const GENESIS_APP_HASH_KEY: &str = "consensus:v1:genesis_app_hash";
// This local qualification profile stops before an unbounded history scan.
const MAX_HISTORY: u64 = 100_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidatorConfig {
    pub pubkey_type: String,
    pub pubkey_base64: String,
    pub power: i64,
    pub reward_address: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsensusConfig {
    pub profile: String,
    pub engine: String,
    pub chain_id: String,
    pub app_state_sha256: String,
    pub gas_price: u64,
    pub max_tx_bytes: usize,
    pub max_block_bytes: usize,
    pub max_txs: usize,
    pub validators: Vec<ValidatorConfig>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "explicit_lifecycle"
    )]
    pub lifecycle: Option<LifecycleConfig>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "explicit_penalty"
    )]
    pub penalty: Option<PenaltyConfig>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "explicit_recovery"
    )]
    pub recovery: Option<RecoveryBook>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "explicit_ordinary"
    )]
    pub ordinary: Option<OrdinaryConfig>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "explicit_governance"
    )]
    pub governance: Option<GovernanceCandidateConfig>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "explicit_emergency"
    )]
    pub emergency: Option<emergency::Policy>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "explicit_upgrade"
    )]
    pub upgrade: Option<upgrade::Policy>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "explicit_handover"
    )]
    pub release_handover: Option<handover::Policy>,
}
fn explicit_handover<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<handover::Policy>, D::Error> {
    handover::Policy::deserialize(deserializer).map(Some)
}
fn explicit_upgrade<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<upgrade::Policy>, D::Error> {
    upgrade::Policy::deserialize(deserializer).map(Some)
}
fn explicit_emergency<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<emergency::Policy>, D::Error> {
    emergency::Policy::deserialize(deserializer).map(Some)
}
fn explicit_ordinary<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<OrdinaryConfig>, D::Error> {
    OrdinaryConfig::deserialize(deserializer).map(Some)
}
fn explicit_governance<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<GovernanceCandidateConfig>, D::Error> {
    GovernanceCandidateConfig::deserialize(deserializer).map(Some)
}
fn explicit_recovery<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<RecoveryBook>, D::Error> {
    RecoveryBook::deserialize(deserializer).map(Some)
}
fn explicit_penalty<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<PenaltyConfig>, D::Error> {
    PenaltyConfig::deserialize(deserializer).map(Some)
}
fn explicit_lifecycle<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<LifecycleConfig>, D::Error> {
    LifecycleConfig::deserialize(deserializer).map(Some)
}
impl ConsensusConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            matches!(
                self.profile.as_str(),
                "cometbft-local-qualification"
                    | "cometbft-lifecycle-local-qualification"
                    | "cometbft-penalty-local-qualification"
            ) && self.engine == "cometbft-v0.40.0",
            "Production or unsupported engine profile is disabled"
        );
        match (&self.lifecycle, self.profile.as_str()) {
            (None, "cometbft-local-qualification") => {}
            (
                Some(lifecycle),
                "cometbft-lifecycle-local-qualification" | "cometbft-penalty-local-qualification",
            ) => {
                lifecycle.validate()?;
                ensure!(
                    lifecycle.chain_id == self.chain_id
                        && lifecycle.profile == "cometbft-lifecycle-local-qualification",
                    "Lifecycle configuration binding differs"
                );
                ensure!(
                    self.validators.len() <= lifecycle.max_active && lifecycle.max_active <= 64,
                    "Lifecycle validator capacity exceeds configured or engine bound"
                );
                for validator in &self.validators {
                    ensure!(
                        lifecycle
                            .approved_operators
                            .contains_key(&validator.reward_address),
                        "Genesis validator has no approved operator"
                    );
                }
            }
            _ => anyhow::bail!(
                "Lifecycle requires its explicit qualification profile and configuration"
            ),
        }
        match (&self.penalty, self.profile.as_str()) {
            (Some(penalty), "cometbft-penalty-local-qualification") => {
                penalty.validate()?;
                ensure!(
                    penalty.chain_id == self.chain_id && penalty.profile == self.profile,
                    "Penalty configuration binding differs"
                );
            }
            (None, "cometbft-local-qualification" | "cometbft-lifecycle-local-qualification") => {}
            _ => anyhow::bail!(
                "Penalty processing requires its explicit qualification profile and configuration"
            ),
        }
        ensure!(
            !self.chain_id.is_empty()
                && self.chain_id.len() <= 50
                && !self.chain_id.chars().any(char::is_control),
            "Invalid consensus chain ID"
        );
        valid_hash(&self.app_state_sha256)?;
        if let Some(policy) = &self.emergency {
            policy.validate()?;
            if let Some(v2) = &policy.v2 {
                ensure!(
                    v2.genesis_sha256 == self.app_state_sha256,
                    "Emergency genesis binding differs"
                );
            }
            ensure!(
                policy.chain_id == self.chain_id,
                "Emergency policy chain differs"
            );
            ensure!(
                policy.max_control_bytes <= self.max_tx_bytes,
                "Emergency control exceeds transaction bound"
            );
        }
        if let Some(policy) = &self.upgrade {
            upgrade::validate_registry()?;
            policy.validate()?;
            let emergency = self
                .emergency
                .as_ref()
                .context("Upgrade requires emergency policy and root verifier")?;
            ensure!(
                policy.chain_id == self.chain_id
                    && policy.genesis_sha256 == self.app_state_sha256
                    && policy.source_release_sha512 == emergency.release_sha512,
                "Upgrade chain/genesis/release binding differs"
            );
            ensure!(
                policy.max_control_bytes <= self.max_tx_bytes,
                "Upgrade control exceeds transaction bound"
            );
        }
        if let Some(policy) = &self.release_handover {
            policy.validate()?;
            let upgrade = self
                .upgrade
                .as_ref()
                .context("Handover requires registered upgrade policy")?;
            ensure!(
                policy.chain_id == self.chain_id
                    && policy.genesis_sha256 == self.app_state_sha256
                    && policy.initial_release_sha512 == upgrade.source_release_sha512
                    && policy.initial_schema == 0,
                "Handover genesis/root/schema binding differs"
            );
            ensure!(
                policy.max_control_bytes <= self.max_tx_bytes,
                "Handover wire bound exceeds transaction limit"
            );
        }
        if let Some(book) = &self.recovery {
            ensure!(
                cfg!(feature = "pqc-fips204"),
                "Recovery requires the real FIPS backend"
            );
            book.validate()?;
            ensure!(
                book.last_height == 0
                    && book.sponsor_receipts.is_empty()
                    && book.operation_success.is_empty()
                    && book.expiry_index.is_empty(),
                "Recovery config requires fresh genesis state"
            );
            ensure!(
                book.profile.max_transaction_gas <= i64::MAX as u64
                    && book.profile.max_block_gas <= i64::MAX as u64,
                "Recovery gas exceeds engine result range"
            );
            for account in book.accounts.values() {
                ensure!(
                    account.sponsor_nonce == 0
                        && account.recovery.last_height == 0
                        && account.recovery.pending_recovery.is_none()
                        && account.recovery.pending_policy.is_none(),
                    "Recovery genesis contains prior execution"
                );
                ensure!(
                    account.recovery.domain.chain_id == self.chain_id
                        && hex::encode(account.recovery.domain.genesis_digest)
                            == self.app_state_sha256,
                    "Recovery genesis domain differs"
                );
            }
        }
        if let Some(ordinary) = &self.ordinary {
            ordinary.validate(
                self.lifecycle
                    .as_ref()
                    .context("Ordinary execution requires lifecycle")?,
                self.recovery
                    .as_ref()
                    .context("Ordinary execution requires recovery")?,
            )?;
            ensure!(
                ordinary.fee_profile.max_transaction_gas <= i64::MAX as u64
                    && ordinary.fee_profile.max_block_transaction_gas <= i64::MAX as u64,
                "Ordinary gas exceeds engine result range"
            );
            ensure!(
                ordinary.max_transport_bytes <= self.max_tx_bytes as u64,
                "Ordinary transport bound exceeds consensus transaction bound"
            );
        }
        if let Some(governance) = &self.governance {
            governance.validate_shape()?;
            ensure!(
                self.lifecycle.is_some() && self.recovery.is_some() && self.ordinary.is_some(),
                "Governance requires lifecycle, recovery and ordinary profiles"
            );
            ensure!(
                governance.chain_id == self.chain_id
                    && hex::encode(governance.genesis_digest) == self.app_state_sha256,
                "Governance candidate chain or genesis differs"
            );
            governance.validate_for_activation()?;
        }
        ensure!(
            self.gas_price > 0 && self.gas_price <= i64::MAX as u64,
            "Invalid pinned gas price"
        );
        ensure!(
            (1..=262_144).contains(&self.max_tx_bytes)
                && (1..=1_048_576).contains(&self.max_block_bytes)
                && self.max_tx_bytes <= self.max_block_bytes
                && (1..=1000).contains(&self.max_txs),
            "Invalid consensus resource limits"
        );
        ensure!(
            !self.validators.is_empty() && self.validators.len() <= 1000,
            "Invalid validator count"
        );
        let mut keys = BTreeSet::new();
        let mut addresses = BTreeSet::new();
        let mut power = 0i64;
        for validator in &self.validators {
            let key = B64
                .decode(&validator.pubkey_base64)
                .context("Invalid validator public key")?;
            ensure!(
                validator.pubkey_type == "ml_dsa_65"
                    && key.len() == 1952
                    && B64.encode(&key) == validator.pubkey_base64,
                "Only canonical ML-DSA-65 validator keys are supported"
            );
            ensure!(
                keys.insert(key) && addresses.insert(&validator.reward_address),
                "Duplicate validator key or reward address"
            );
            ensure!(
                !validator.reward_address.is_empty()
                    && validator.reward_address.len() <= 256
                    && !validator
                        .reward_address
                        .chars()
                        .any(|c| c.is_whitespace() || c.is_control()),
                "Invalid validator reward address"
            );
            ensure!(validator.power > 0, "Validator power must be positive");
            power = power
                .checked_add(validator.power)
                .context("Validator power exceeds i64")?;
        }
        ensure!(
            power <= i64::MAX / 8,
            "Validator power exceeds engine bound"
        );
        ensure!(
            serde_json::to_vec(self)?.len() <= 65_536,
            "Consensus configuration exceeds storage bound"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinalizedBlockInput {
    pub height: u64,
    pub time_seconds: i64,
    pub time_nanos: i32,
    pub hash: String,
    pub txs: Vec<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub misbehavior: Vec<EvidenceFact>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WireTransaction {
    Signed {
        envelope: SignedTx,
    },
    Recovery {
        envelope_base64: String,
    },
    OrdinaryV2 {
        envelope_base64: String,
    },
    OrdinaryV3 {
        envelope_base64: String,
    },
    EpochObservation {
        observation: EpochObservation,
    },
    #[serde(skip)]
    EmergencyControl {
        control: Vec<u8>,
    },
    #[serde(skip)]
    HandoverControl {
        control: Vec<u8>,
    },
    #[serde(skip)]
    UpgradeControl {
        control: Vec<u8>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Info {
    pub height: u64,
    pub app_hash: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TxResult {
    pub code: u32,
    pub gas_wanted: i64,
    pub gas_used: i64,
    pub log: String,
}
impl TxResult {
    fn invalid(log: &str) -> Self {
        Self {
            code: 1,
            gas_wanted: 0,
            gas_used: 0,
            log: log.into(),
        }
    }
    fn from_recovery(result: &RecoveryResult) -> Result<Self> {
        if !result.charged {
            let gas = i64::try_from(result.gas_used)?;
            return Ok(Self {
                code: 1,
                gas_wanted: gas,
                gas_used: gas,
                log: result
                    .error
                    .clone()
                    .unwrap_or_else(|| "Rejected recovery".into()),
            });
        }
        let receipt = result
            .receipt
            .as_ref()
            .context("Charged recovery has no receipt")?;
        Ok(Self {
            code: if result.success { 0 } else { 3 },
            gas_wanted: i64::try_from(receipt.gas_limit)?,
            gas_used: i64::try_from(result.gas_used)?,
            log: result.error.clone().unwrap_or_default(),
        })
    }
    fn from_ordinary(result: &OrdinaryExecutionResult) -> Result<Self> {
        Ok(Self {
            code: if !result.accepted {
                1
            } else if result.success {
                0
            } else {
                3
            },
            gas_wanted: i64::try_from(
                result
                    .receipt
                    .as_ref()
                    .map_or(result.gas_used, |r| r.gas_limit()),
            )?,
            gas_used: i64::try_from(result.gas_used)?,
            log: result.error.clone().unwrap_or_default(),
        })
    }
    fn observation() -> Self {
        Self {
            code: 0,
            gas_wanted: 0,
            gas_used: 0,
            log: String::new(),
        }
    }
    fn from_receipt(receipt: &TxReceipt, accepted: bool) -> Result<Self> {
        Ok(Self {
            code: if receipt.success {
                0
            } else if accepted {
                3
            } else {
                2
            },
            gas_wanted: i64::try_from(receipt.gas_limit)?,
            gas_used: i64::try_from(receipt.gas_used)?,
            log: receipt.error.clone().unwrap_or_default(),
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinalizeResult {
    pub app_hash: String,
    pub tx_results: Vec<TxResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub validator_updates: Vec<ValidatorUpdate>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Anchor {
    version: u32,
    height: u64,
    engine_hash: String,
    parent_engine_hash: String,
    time_seconds: i64,
    time_nanos: i32,
    input_digest: String,
    result_digest: String,
    prior_app_hash: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Head {
    anchor: Anchor,
    state_digest: String,
    app_hash: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Accepted {
    index: u32,
    record: TransactionRecord,
    receipt: TxReceipt,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BlockRecord {
    input: FinalizedBlockInput,
    result: FinalizeResult,
    head: Head,
    accepted: Vec<Accepted>,
}
struct Prepared {
    expected_info: Info,
    expected_state_digest: String,
    writes: Writes,
    journal: Option<PreparedJournalUpdate>,
    record: BlockRecord,
}
/// Local paths are inputs only. The expected digest comes from verified chain history.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevelopmentCandidateInput {
    pub manifest_path: std::path::PathBuf,
    pub max_manifest_bytes: usize,
    pub max_executable_bytes: u64,
}
struct PreparedRuntimeCandidateInput {
    input: crate::runtime_candidate_v2::RuntimeCandidateInput,
    root: crate::root_genesis::DevelopmentRootGenesis,
    verifier: EmergencyVerifierConfig,
}

/// This verifier is independent of every candidate/live-helper setting. The
/// original root configuration supplies its own immutable execution policy.
fn bootstrap_history_verifier(
    root: &crate::root_genesis::DevelopmentRootGenesis,
) -> Result<EmergencyVerifier> {
    EmergencyVerifier::new(EmergencyVerifierConfig {
        helper_path: root.helper_path.clone(),
        // Observed immutable execution never reads or executes a scratch path.
        helper_scratch_path: root.helper_scratch_path.clone().unwrap_or_default(),
        helper_execution: root.helper_execution.clone(),
        helper_sha256: root.helper_sha256.clone(),
        max_helper_bytes: root.max_helper_bytes,
        max_request_bytes: root.max_request_bytes,
        timeout_ms: root.timeout_ms,
    })
}

/// Release authority obtained from the verified root and committed signed
/// history. It has no deserializer or caller-supplied manifest constructor.
#[derive(Clone, Debug)]
pub struct VerifiedReleaseAuthority {
    expected: crate::runtime_candidate::ExpectedCandidate,
    committed_info: Option<Info>,
    active_schema: Option<u16>,
}
impl VerifiedReleaseAuthority {
    pub fn expected_candidate(&self) -> &crate::runtime_candidate::ExpectedCandidate {
        &self.expected
    }
    pub fn committed_info(&self) -> Option<&Info> {
        self.committed_info.as_ref()
    }
    pub fn active_schema(&self) -> Option<u16> {
        self.active_schema
    }
}

fn check_stored_startup(
    storage: &Storage,
    config: &ConsensusConfig,
    genesis_bytes: &[u8],
    root_genesis: Option<&crate::root_genesis::PreparedRootGenesis>,
    emergency_verifier: Option<&EmergencyVerifier>,
) -> Result<bool> {
    if let Some(stored) = storage.db.get(MODE_KEY)? {
        ensure!(stored == serde_json::to_vec(config)?, "Consensus configuration differs from storage");
        ensure!(storage.db.get(GENESIS_SOURCE_KEY)?.as_deref() == Some(genesis_bytes),
            "Stored application genesis source differs");
        crate::root_genesis::check_receipt(
            storage.db.get(crate::root_genesis::STATE_KEY)?.as_deref(), root_genesis)?;
        let history = HistoryRead::new(storage);
        let trace = verify_recovery_with(&history)?;
        if let (Some(policy), Some(verifier)) = (&config.emergency, emergency_verifier) {
            ensure!(emergency::recover(policy, &trace.records, verifier)?
                == emergency_state(storage, config)?.context("Emergency state missing")?,
                "Emergency cryptographic replay differs");
            let outcomes = upgrade_history(&history, config, &trace, Some(verifier))?;
            handover_history(&history, config, &trace, &outcomes, Some(verifier))?;
        }
        Ok(true)
    } else {
        // Read errors must not be mistaken for an empty database.
        match storage.db.iterator(IteratorMode::Start).next() {
            Some(entry) => { entry?; anyhow::bail!("Existing non-consensus database requires migration"); }
            None => Ok(false),
        }
    }
}

fn preflight_prepared_release(
    path: &Path,
    config: &ConsensusConfig,
    genesis_bytes: &[u8],
    root_genesis: &crate::root_genesis::PreparedRootGenesis,
    verifier: &EmergencyVerifier,
) -> Result<VerifiedReleaseAuthority> {
    let root_release = &config.emergency.as_ref().context("Candidate requires root release")?.release_sha512;
    let mut authority = VerifiedReleaseAuthority {
        expected: crate::runtime_candidate::ExpectedCandidate {
            manifest_sha512: root_release.clone(), chain_id: config.chain_id.clone(),
            app_genesis_sha256: config.app_state_sha256.clone(),
            migration_registry_sha256: upgrade::registry_sha256(),
        }, committed_info: None, active_schema: None,
    };
    if !path.join("CURRENT").try_exists()? {
        if path.try_exists()? {
            ensure!(path.is_dir(), "Database bootstrap path is not a directory");
            ensure!(std::fs::read_dir(path)?.next().transpose()?.is_none(),
                "Nonempty database path has no RocksDB CURRENT; read-only preflight refused");
        }
        return Ok(authority);
    }
    let storage = Storage::open_read_only(path.to_path_buf())?;
    if check_stored_startup(&storage, config, genesis_bytes, Some(root_genesis), Some(verifier))? {
        authority.expected.manifest_sha512 = handover_state(&storage, config)?
            .map(|state| state.active_release_sha512().to_owned())
            .unwrap_or_else(|| root_release.clone());
        authority.committed_info = Some(current_info(&storage)?);
        authority.active_schema = upgrade_state(&storage, config)?.map(|state| state.active_schema());
    }
    Ok(authority)
}
// Cache lifetime is one locked operation. The byte budget limits reuse only;
// overflow never changes acceptance and never skips canonical decoding.
const HISTORY_CACHE_BYTES: usize = 8 * 1024 * 1024;
struct HistoryRead<'a> {
    storage: &'a Storage,
    blocks: std::cell::RefCell<BTreeMap<u64, Arc<BlockRecord>>>,
    cached_bytes: std::cell::Cell<usize>,
    budget: usize,
    release_trace: std::cell::RefCell<Option<(String, Vec<(u64, String)>)>>,
    #[cfg(test)]
    block_decodes: std::cell::Cell<usize>,
    #[cfg(test)]
    emergency_steps: std::cell::Cell<usize>,
}
impl<'a> HistoryRead<'a> {
    fn new(storage: &'a Storage) -> Self {
        Self::with_budget(storage, HISTORY_CACHE_BYTES)
    }
    fn with_budget(storage: &'a Storage, budget: usize) -> Self {
        Self {
            storage,
            blocks: Default::default(),
            cached_bytes: Default::default(),
            budget,
            release_trace: Default::default(),
            #[cfg(test)]
            block_decodes: Default::default(),
            #[cfg(test)]
            emergency_steps: Default::default(),
        }
    }
    fn block(&self, height: u64) -> Result<Arc<BlockRecord>> {
        if let Some(record) = self.blocks.borrow().get(&height) {
            return Ok(record.clone());
        }
        let bytes = self
            .storage
            .db
            .get(record_key(height))?
            .context("Historical block missing")?;
        let record = Arc::new(decode::<BlockRecord>(&bytes)?);
        #[cfg(test)]
        self.block_decodes.set(self.block_decodes.get() + 1);
        // Budget uses serialized bytes, not an RSS claim. Entries are bounded
        // additionally by the existing block and history limits.
        if bytes.len() <= self.budget.saturating_sub(self.cached_bytes.get()) {
            self.cached_bytes.set(self.cached_bytes.get() + bytes.len());
            self.blocks.borrow_mut().insert(height, record.clone());
        }
        Ok(record)
    }
    fn anchor(&self, height: u64, parent_height: u64) -> Result<emergency::FinalizedAnchor> {
        ensure!(height <= parent_height, "Control anchor is not finalized");
        if height == 0 {
            return finalized_anchor(self.storage, height, parent_height);
        }
        let record = self.block(height)?;
        ensure!(
            record.input.height == height && record.head.anchor.height == height,
            "Control anchor block height differs"
        );
        valid_hash(&record.result.app_hash)?;
        Ok(emergency::FinalizedAnchor {
            height,
            app_hash: record.result.app_hash.clone(),
        })
    }
}
struct EmergencyTrace {
    records: Vec<(emergency::Receipt, emergency::BlockContext)>,
    // Index n is the state after exactly n records. No prefix is replayed twice.
    states: Vec<emergency::State>,
}

#[derive(Clone, Copy)]
pub enum QueryRequest<'a> {
    Status,
    OrdinaryProfile,
    OrdinaryAccount(&'a str),
    OrdinaryReceipt(&'a str),
    EmergencyReceipt(&'a str),
}
#[cfg(test)]
thread_local! { static RECOVERY_PASSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
pub struct ConsensusApplication {
    storage: Arc<Storage>,
    config: ConsensusConfig,
    genesis_bytes: Vec<u8>,
    root_genesis: Option<crate::root_genesis::PreparedRootGenesis>,
    emergency_verifier: Option<EmergencyVerifier>,
    runtime_candidate: Option<crate::runtime_candidate_v2::VerifiedRuntimeCandidate>,
    pending: Option<Prepared>,
    admission: Mutex<crate::ordinary_admission::OrdinaryAdmissionQueue>,
}

fn valid_hash(value: &str) -> Result<()> {
    ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
        "Expected a lowercase 32-byte hexadecimal hash"
    );
    Ok(())
}
fn digest(domain: &[u8], value: &impl Serialize) -> Result<String> {
    let mut h = Sha256::new();
    h.update(domain);
    h.update(serde_json::to_vec(value)?);
    Ok(hex::encode(h.finalize()))
}
fn decode<T: Serialize + DeserializeOwned>(raw: &[u8]) -> Result<T> {
    let value: T = serde_json::from_slice(raw)?;
    ensure!(
        serde_json::to_vec(&value)? == raw,
        "Noncanonical consensus record"
    );
    Ok(value)
}
fn read_head(storage: &Storage) -> Result<Option<Head>> {
    storage
        .db
        .get(HEAD_KEY)?
        .map(|raw| decode(&raw))
        .transpose()
}
fn record_key(height: u64) -> String {
    format!("{BLOCK_PREFIX}{height:016x}")
}
fn selected(key: &[u8], governance_enabled: bool) -> bool {
    [
        b"acct:".as_slice(),
        b"dms:config:",
        b"emission:",
        b"staking:",
        b"rewards:",
        b"issuance:",
        b"adaptive:",
        b"lifecycle:",
        b"penalty:",
        b"recovery:",
        b"supply:",
        b"genesis:",
        b"root:authorization:",
        b"ordinary:v1:",
        b"consensus:emergency:",
        b"consensus:upgrade:",
        b"consensus:release-handover:",
    ]
    .iter()
    .any(|prefix| key.starts_with(prefix))
        || (governance_enabled && key == GOVERNANCE_STATE_KEY.as_bytes())
        || key == MODE_KEY.as_bytes()
        || key == b"meta:chain_id"
        || key == b"execution:v1:withheld_udrt"
}
fn governance_key_permitted(key: &[u8], governance_enabled: bool) -> bool {
    if key.starts_with(b"governance:") {
        return governance_enabled && key == GOVERNANCE_STATE_KEY.as_bytes();
    }
    !key.starts_with(b"gov:")
}
fn state_digest(storage: &Storage, writes: &Writes, governance_enabled: bool) -> Result<String> {
    let mut values = BTreeMap::new();
    for item in storage.db.iterator(IteratorMode::Start) {
        let (k, v) = item?;
        ensure!(
            governance_key_permitted(&k, governance_enabled),
            "Governance state requires a configured consensus profile"
        );
        if selected(&k, governance_enabled) {
            values.insert(k.to_vec(), v.to_vec());
        }
    }
    for (k, v) in writes {
        ensure!(
            governance_key_permitted(k, governance_enabled),
            "Governance writes require a configured consensus profile"
        );
        if selected(k, governance_enabled) {
            values.insert(k.clone(), v.clone());
        }
    }
    digest(
        b"dytallix-cometbft-state-v1",
        &values.iter().collect::<Vec<_>>(),
    )
}
fn app_hash(state: &str, anchor: &Anchor) -> Result<String> {
    digest(b"dytallix-cometbft-app-v1", &(state, anchor))
}
fn genesis_info(storage: &Storage) -> Result<Info> {
    let app_hash = String::from_utf8(
        storage
            .db
            .get(GENESIS_APP_HASH_KEY)?
            .context("Genesis application commitment missing")?,
    )?;
    valid_hash(&app_hash)?;
    Ok(Info {
        height: 0,
        app_hash,
    })
}

/// Add the original source and initial application commitment to the same
/// genesis batch. The caller has already staged all monetary initialization.
pub(crate) fn append_genesis_anchor(
    storage: &Storage,
    batch: &mut WriteBatch,
    source: &[u8],
) -> Result<()> {
    struct Capture {
        writes: Writes,
        deleted: bool,
    }
    impl rocksdb::WriteBatchIterator for Capture {
        fn put(&mut self, key: Box<[u8]>, value: Box<[u8]>) {
            self.writes.insert(key.into_vec(), value.into_vec());
        }
        fn delete(&mut self, _key: Box<[u8]>) {
            self.deleted = true;
        }
    }
    let mut capture = Capture {
        writes: Writes::new(),
        deleted: false,
    };
    batch.iterate(&mut capture);
    ensure!(
        !capture.deleted
            && capture.writes.contains_key(MODE_KEY.as_bytes())
            && capture
                .writes
                .contains_key(b"genesis:monetary:v1".as_slice()),
        "Incomplete consensus genesis batch"
    );
    ensure!(
        !capture.writes.contains_key(GENESIS_SOURCE_KEY.as_bytes())
            && !capture.writes.contains_key(GENESIS_APP_HASH_KEY.as_bytes()),
        "Genesis anchor already staged"
    );
    // Legacy internal genesis tests use an opaque mode value. The application
    // entry point validates the complete configuration before this helper runs.
    let mut governance_enabled = false;
    if let Ok(config) =
        serde_json::from_slice::<ConsensusConfig>(&capture.writes[MODE_KEY.as_bytes()])
    {
        if let Some(policy) = &config.emergency {
            config.validate()?;
            ensure!(
                capture.writes.contains_key(crate::root_genesis::STATE_KEY),
                "Emergency genesis requires root authorization in the same batch"
            );
            ensure!(
                !storage.db.iterator(IteratorMode::Start).any(|item| item
                    .map(|(key, _)| key.starts_with(b"consensus:emergency:"))
                    .unwrap_or(true)),
                "Emergency genesis state already exists or storage scan failed"
            );
            let encoded = emergency::encode_state(&emergency::State::new(policy)?)?;
            ensure!(
                capture
                    .writes
                    .insert(emergency::STATE_KEY.as_bytes().to_vec(), encoded.clone())
                    .is_none(),
                "Emergency genesis already staged"
            );
            batch.put(emergency::STATE_KEY, encoded);
        }
        if let Some(policy) = &config.upgrade {
            config.validate()?;
            ensure!(
                capture.writes.contains_key(crate::root_genesis::STATE_KEY),
                "Upgrade genesis requires root authorization"
            );
            let bytes = upgrade::encode_state(&upgrade::State::new(policy)?)?;
            ensure!(
                capture
                    .writes
                    .insert(upgrade::STATE_KEY.as_bytes().to_vec(), bytes.clone())
                    .is_none(),
                "Upgrade genesis already staged"
            );
            batch.put(upgrade::STATE_KEY, bytes);
        }
        if let Some(policy) = &config.release_handover {
            config.validate()?;
            let bytes = handover::encode_state(&handover::State::new(policy)?)?;
            ensure!(
                capture
                    .writes
                    .insert(handover::STATE_KEY.as_bytes().to_vec(), bytes.clone())
                    .is_none(),
                "Handover genesis already staged"
            );
            batch.put(handover::STATE_KEY, bytes);
        }
        if let Some(book) = &config.recovery {
            config.validate()?;
            for account in book.accounts.values() {
                ensure!(
                    capture
                        .writes
                        .contains_key(format!("acct:balances:{}", account.address).as_bytes()),
                    "Recovery account missing from monetary genesis"
                );
            }
            let encoded = book.encode()?;
            ensure!(
                capture
                    .writes
                    .insert(RECOVERY_STATE_KEY.as_bytes().to_vec(), encoded.clone())
                    .is_none(),
                "Recovery genesis already staged"
            );
            batch.put(RECOVERY_STATE_KEY, encoded);
        }
        if let Some(ordinary_config) = &config.ordinary {
            ordinary_state::assert_absent(storage)?;
            let book = config
                .recovery
                .as_ref()
                .context("Ordinary recovery genesis missing")?;
            let mut native_nonces = BTreeMap::new();
            for account in book.accounts.values() {
                ensure!(
                    account.recovery.spending_nonce == 0 && account.recovery.active_generation == 0,
                    "Ordinary fresh genesis requires zero authority counters"
                );
                let key = format!("acct:nonce:{}", account.address).into_bytes();
                let bytes = bincode::serialize(&0u64)?;
                if let Some(existing) = capture.writes.get(&key) {
                    ensure!(*existing == bytes, "Conflicting ordinary genesis nonce");
                }
                capture.writes.insert(key.clone(), bytes.clone());
                batch.put(&key, &bytes);
                native_nonces.insert(account.address.clone(), 0);
            }
            let ordinary = OrdinaryState::genesis(
                ordinary_config.clone(),
                config
                    .lifecycle
                    .as_ref()
                    .context("Ordinary lifecycle genesis missing")?,
                book,
                &native_nonces,
            )?;
            let encoded = ordinary.encode()?;
            ensure!(
                capture
                    .writes
                    .insert(ORDINARY_STATE_KEY.as_bytes().to_vec(), encoded.clone())
                    .is_none(),
                "Ordinary genesis already staged"
            );
            batch.put(ORDINARY_STATE_KEY, encoded);
        }
        if let Some(lifecycle_config) = &config.lifecycle {
            config.validate()?;
            let reward = RewardState::decode(
                capture
                    .writes
                    .get(REWARD_STATE_KEY.as_bytes())
                    .context("Lifecycle genesis requires reward state")?,
            )?;
            let identities = initial_identities(&config)?;
            let lifecycle = LifecycleState::new(lifecycle_config.clone(), identities, &reward)?;
            if config.ordinary.is_some() {
                validate_ordinary_principals(
                    config
                        .recovery
                        .as_ref()
                        .context("Recovery genesis missing")?,
                    &reward,
                    Some(&lifecycle),
                )?;
            }
            ensure!(
                validator_tuple_map(&lifecycle.validator_set(1)?)
                    == configured_validator_map(&config),
                "Genesis consensus power differs from effective bonded stake"
            );
            let encoded = lifecycle.encode()?;
            ensure!(
                capture
                    .writes
                    .insert(LIFECYCLE_STATE_KEY.as_bytes().to_vec(), encoded.clone())
                    .is_none(),
                "Lifecycle genesis already staged"
            );
            batch.put(LIFECYCLE_STATE_KEY, encoded);
            if let Some(penalty_config) = &config.penalty {
                let penalties =
                    PenaltyState::initialize(penalty_config.clone(), &lifecycle, &reward)?;
                let encoded = penalties.encode()?;
                ensure!(
                    capture
                        .writes
                        .insert(PENALTY_STATE_KEY.as_bytes().to_vec(), encoded.clone())
                        .is_none(),
                    "Penalty genesis already staged"
                );
                batch.put(PENALTY_STATE_KEY, encoded);
            }
        }
        if let Some(governance) = &config.governance {
            config.validate()?;
            let state = GovernanceState::new(
                GOVERNANCE_STATE_VERSION,
                governance.chain_id.clone(),
                governance.genesis_digest,
                0,
            )?;
            let encoded = state.encode()?;
            ensure!(
                capture
                    .writes
                    .insert(GOVERNANCE_STATE_KEY.as_bytes().to_vec(), encoded.clone())
                    .is_none(),
                "Governance genesis already staged"
            );
            batch.put(GOVERNANCE_STATE_KEY, encoded);
            governance_enabled = true;
        }
    }
    let root = digest(
        b"dytallix-cometbft-genesis-v1",
        &state_digest(storage, &capture.writes, governance_enabled)?,
    )?;
    batch.put(GENESIS_SOURCE_KEY, source);
    batch.put(GENESIS_APP_HASH_KEY, root.as_bytes());
    Ok(())
}
fn current_info(storage: &Storage) -> Result<Info> {
    if let Some(head) = read_head(storage)? {
        Ok(Info {
            height: head.anchor.height,
            app_hash: head.app_hash,
        })
    } else if storage.db.get(MODE_KEY)?.is_some() {
        genesis_info(storage)
    } else {
        Ok(Info {
            height: 0,
            app_hash: String::new(),
        })
    }
}
/// Read one governance parent from the committed consensus state. The caller
/// holds the execution lock. This does not activate an E04 candidate.
struct GovernanceCommittedParent {
    height: u64,
    app_hash: [u8; 32],
    candidate: GovernanceCandidateConfig,
    recovery: RecoveryBook,
    lifecycle: LifecycleState,
    governance: GovernanceState,
    native_nonces: BTreeMap<String, u64>,
}
impl GovernanceCommittedParent {
    fn assess_signed(&self, signed: &SignedOrdinaryV3) -> Result<PreadmissionAssessment> {
        ensure!(
            self.height == self.governance.finalized_height()
                && self.candidate.chain_id == self.governance.chain_id()
                && self.candidate.genesis_digest == self.governance.genesis_digest(),
            "Governance candidate differs from verified committed parent"
        );
        crate::governance_signed_admission::assess_signed(
            signed,
            &AdmissionParent {
                candidate: &self.candidate,
                recovery: &self.recovery,
                native_nonces: &self.native_nonces,
                lifecycle: &self.lifecycle,
                governance: &self.governance,
                app_hash: self.app_hash,
            },
        )
    }
}
fn committed_governance_parent(
    storage: Arc<Storage>,
    config: &ConsensusConfig,
    genesis_digest: [u8; 32],
) -> Result<GovernanceCommittedParent> {
    let stored_config: ConsensusConfig = decode(
        &storage
            .db
            .get(MODE_KEY)?
            .context("Committed consensus configuration missing")?,
    )?;
    ensure!(stored_config == *config, "Governance candidate differs from committed configuration");
    let candidate = config
        .governance
        .as_ref()
        .context("Governance candidate is not configured")?;
    candidate.validate_shape()?;
    ensure!(
        storage.get_chain_id().as_deref() == Some(config.chain_id.as_str())
            && hex::encode(genesis_digest) == config.app_state_sha256
            && candidate.chain_id == config.chain_id
            && candidate.genesis_digest == genesis_digest,
        "Governance parent chain or genesis differs"
    );
    let info = current_info(&storage)?;
    valid_hash(&info.app_hash)?;
    ensure!(
        block_lifecycle::height(&storage, "meta:height")? == info.height,
        "Governance parent height differs from committed metadata"
    );
    let state = state_digest(&storage, &Writes::new(), true)?;
    if let Some(head) = read_head(&storage)? {
        let record: BlockRecord = decode(
            &storage
                .db
                .get(record_key(info.height))?
                .context("Governance parent block record missing")?,
        )?;
        ensure!(
            head.anchor.height == info.height
                && head.state_digest == state
                && head.app_hash == info.app_hash
                && record.input.height == info.height
                && record.result.app_hash == info.app_hash
                && serde_json::to_vec(&record.head)? == serde_json::to_vec(&head)?
                && app_hash(&state, &head.anchor)? == info.app_hash,
            "Governance parent app hash differs from committed state"
        );
    } else {
        ensure!(
            info.height == 0
                && digest(b"dytallix-cometbft-genesis-v1", &state)? == info.app_hash,
            "Governance genesis app hash differs from committed state"
        );
    }
    let app_hash: [u8; 32] = hex::decode(&info.app_hash)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Governance parent app hash length differs"))?;
    let recovery = recovery_book(&storage, config)?.context("Governance recovery state missing")?;
    let lifecycle = lifecycle_state(&storage)?.context("Governance lifecycle state missing")?;
    let governance = GovernanceState::decode(
        &storage
            .db
            .get(GOVERNANCE_STATE_KEY)?
            .context("Governance state missing")?,
    )?;
    ensure!(
        config.lifecycle.as_ref() == Some(&lifecycle.config)
            && lifecycle.config.chain_id == config.chain_id
            && lifecycle.last_height == info.height
            && recovery.last_height == info.height
            && governance.chain_id() == config.chain_id
            && governance.genesis_digest() == genesis_digest
            && governance.finalized_height() == info.height
            && recovery.accounts.values().all(|account| {
                account.recovery.domain.chain_id == config.chain_id
                    && account.recovery.domain.genesis_digest == genesis_digest
            }),
        "Governance parent state, authority, or lifecycle differs"
    );
    let ordinary = load_ordinary(&storage, config, Some(&recovery))?
        .context("Governance ordinary state missing")?;
    ensure!(
        ordinary.last_height == info.height,
        "Governance ordinary state height differs"
    );
    let mut staged = Settlement::new(storage);
    let native_nonces = staged_nonces(&mut staged, &recovery)?;
    crate::ordinary_authority::validate_nonce_mirrors(&recovery, &native_nonces)?;
    Ok(GovernanceCommittedParent {
        height: info.height,
        app_hash,
        candidate: candidate.clone(),
        recovery,
        lifecycle,
        governance,
        native_nonces,
    })
}
fn timing(storage: &Storage) -> Result<TimingState> {
    TimingState::decode(
        &storage
            .db
            .get(TIMING_STATE_KEY)?
            .context("Issuance timing genesis required")?,
    )
}
fn check_reward_validators(storage: &Storage, config: &ConsensusConfig) -> Result<()> {
    let reward = RewardState::decode(
        &storage
            .db
            .get(REWARD_STATE_KEY)?
            .context("Reward-v2 genesis required")?,
    )?;
    if let Some(lifecycle_config) = &config.lifecycle {
        let lifecycle = lifecycle_state(storage)?.context("Lifecycle state missing")?;
        ensure!(
            &lifecycle.config == lifecycle_config,
            "Stored lifecycle policy differs"
        );
        lifecycle.validate_rewards(&reward)?;
        match (&config.penalty, penalty_state(storage)?) {
            (Some(expected), Some(penalties)) => {
                ensure!(
                    &penalties.config == expected && reward.locks.is_empty(),
                    "Stored penalty configuration or vesting state differs"
                );
                penalties.validate(&lifecycle)?;
            }
            (None, None) => {}
            _ => anyhow::bail!("Penalty state and consensus profile differ"),
        }
        ensure!(
            validator_tuple_map(&lifecycle.historical_validator_set(1)?)
                == configured_validator_map(config),
            "Lifecycle genesis validator set differs"
        );
        ensure!(
            reward.config.chain_id == config.chain_id
                && timing(storage)?.chain_id == config.chain_id,
            "Consensus chain binding differs"
        );
        return Ok(());
    }
    ensure!(
        lifecycle_state(storage)?.is_none() && penalty_state(storage)?.is_none(),
        "Lifecycle or penalty state supplied for fixed-validator profile"
    );
    let expected: BTreeSet<_> = config
        .validators
        .iter()
        .map(|v| v.reward_address.as_str())
        .collect();
    ensure!(
        reward
            .validators
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            == expected
            && reward.validators.values().all(|v| v.active && !v.jailed),
        "Consensus and active reward validators differ"
    );
    ensure!(
        reward.config.chain_id == config.chain_id && timing(storage)?.chain_id == config.chain_id,
        "Consensus chain binding differs"
    );
    Ok(())
}
fn lifecycle_state(storage: &Storage) -> Result<Option<LifecycleState>> {
    storage
        .db
        .get(LIFECYCLE_STATE_KEY)?
        .map(|raw| LifecycleState::decode(&raw))
        .transpose()
}
fn penalty_state(storage: &Storage) -> Result<Option<PenaltyState>> {
    storage
        .db
        .get(PENALTY_STATE_KEY)?
        .map(|raw| PenaltyState::decode(&raw))
        .transpose()
}
fn initial_identities(config: &ConsensusConfig) -> Result<BTreeMap<String, ValidatorIdentity>> {
    let lifecycle = config
        .lifecycle
        .as_ref()
        .context("Lifecycle configuration required")?;
    config
        .validators
        .iter()
        .map(|validator| {
            Ok((
                validator.reward_address.clone(),
                ValidatorIdentity {
                    owner: lifecycle
                        .approved_operators
                        .get(&validator.reward_address)
                        .context("Genesis operator missing")?
                        .clone(),
                    pubkey_base64: validator.pubkey_base64.clone(),
                },
            ))
        })
        .collect()
}
fn validator_tuple_map(updates: &[ValidatorUpdate]) -> BTreeMap<(String, String), i64> {
    updates
        .iter()
        .map(|v| ((v.pubkey_type.clone(), v.pubkey_base64.clone()), v.power))
        .collect()
}
fn configured_validator_map(config: &ConsensusConfig) -> BTreeMap<(String, String), i64> {
    config
        .validators
        .iter()
        .map(|v| ((v.pubkey_type.clone(), v.pubkey_base64.clone()), v.power))
        .collect()
}
fn results_digest(
    config: &ConsensusConfig,
    results: &[TxResult],
    updates: &[ValidatorUpdate],
) -> Result<String> {
    if config.lifecycle.is_some() {
        digest(
            b"dytallix-cometbft-lifecycle-results-v1",
            &(results, updates),
        )
    } else {
        ensure!(
            updates.is_empty(),
            "Fixed-validator profile cannot emit updates"
        );
        digest(b"dytallix-cometbft-results-v1", &results)
    }
}
fn source_binding(config: &ConsensusConfig, source: &[u8]) -> Result<()> {
    ensure!(
        source.len() <= 16 * 1024 * 1024
            && hex::encode(Sha256::digest(source)) == config.app_state_sha256,
        "Application genesis hash differs"
    );
    let value: serde_json::Value = serde_json::from_slice(source)?;
    ensure!(
        value.get("chain_id").and_then(|v| v.as_str()) == Some(config.chain_id.as_str()),
        "Genesis chain differs"
    );
    ensure!(
        value
            .get("adaptive_issuance")
            .is_some_and(|v| v.is_object()),
        "Issuance timing genesis required"
    );
    if config.penalty.is_some() {
        let accounts = value
            .get("accounts")
            .and_then(serde_json::Value::as_array)
            .context("Penalty qualification requires explicit unlocked genesis accounts")?;
        ensure!(
            accounts
                .iter()
                .all(|account| account.get("vesting")
                    == Some(&serde_json::json!({"kind":"unlocked"}))),
            "Penalty qualification does not support genesis vesting locks"
        );
    }
    let validators = value
        .get("reward_v2")
        .and_then(|v| v.get("validators"))
        .and_then(|v| v.as_array())
        .context("Reward-v2 validators required")?;
    let mut found = BTreeSet::new();
    for validator in validators {
        let address = validator
            .get("address")
            .and_then(|v| v.as_str())
            .context("Missing reward validator address")?;
        ensure!(
            found.insert(address)
                && validator.get("active").and_then(|v| v.as_bool()) == Some(true)
                && validator.get("jailed").and_then(|v| v.as_bool()) == Some(false),
            "Reward validators must be distinct and active"
        );
    }
    ensure!(
        found
            == config
                .validators
                .iter()
                .map(|v| v.reward_address.as_str())
                .collect(),
        "Genesis reward validators differ from consensus configuration"
    );
    if let Some(lifecycle) = &config.lifecycle {
        let positions = value
            .get("reward_v2")
            .and_then(|v| v.get("positions"))
            .and_then(serde_json::Value::as_array)
            .context("Lifecycle genesis positions required")?;
        for validator in &config.validators {
            let operator = lifecycle
                .approved_operators
                .get(&validator.reward_address)
                .context("Approved genesis operator missing")?;
            let mut total = 0u128;
            let mut self_bond = 0u128;
            for position in positions {
                if position
                    .get("validator")
                    .and_then(serde_json::Value::as_str)
                    == Some(validator.reward_address.as_str())
                {
                    let amount: u128 = position
                        .get("amount_udgt")
                        .and_then(serde_json::Value::as_str)
                        .context("Genesis position amount must be a string")?
                        .parse()?;
                    total = total
                        .checked_add(amount)
                        .context("Genesis validator stake exceeds u128")?;
                    if position.get("owner").and_then(serde_json::Value::as_str)
                        == Some(operator.as_str())
                    {
                        self_bond = self_bond
                            .checked_add(amount)
                            .context("Genesis validator self-bond exceeds u128")?;
                    }
                }
            }
            ensure!(
                self_bond >= lifecycle.min_self_bond && i64::try_from(total)? == validator.power,
                "Genesis consensus power or self-bond differs from effective stake"
            );
        }
    }
    Ok(())
}
fn normalize_record(config: &ConsensusConfig, envelope: SignedTx) -> Result<TransactionRecord> {
    let transaction = if config.penalty.is_some() {
        crate::signed_transaction::normalize_penalty(&envelope, config.gas_price)?
    } else {
        crate::signed_transaction::normalize(&envelope, config.gas_price)?
    };
    ensure!(
        transaction.gas_limit <= i64::MAX as u64,
        "Transaction gas exceeds engine limit"
    );
    let record = TransactionRecord::new(
        transaction,
        Some(SignedEnvelope {
            tx: envelope.tx,
            public_key: envelope.public_key,
            signature: envelope.signature,
            algorithm: envelope.algorithm,
            version: envelope.version,
        }),
    )?;
    if config.penalty.is_some() {
        crate::signed_transaction::verify_penalty_mode_record(&record, Some(&config.chain_id))?;
    } else {
        crate::signed_transaction::verify_reward_mode_record(&record, Some(&config.chain_id))?;
    }
    Ok(record)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OrdinaryOuter {
    #[serde(rename = "type")]
    kind: String,
    envelope_base64: String,
}
fn wire(config: &ConsensusConfig, raw: &[u8]) -> Result<WireTransaction> {
    ensure!(
        !raw.is_empty() && raw.len() <= config.max_tx_bytes,
        "Transaction wire bound exceeded"
    );
    if let Some(policy) = &config.emergency {
        if let Ok(control) = emergency::decode_control(policy, raw) {
            return Ok(WireTransaction::EmergencyControl {
                control: serde_json::to_vec(&control)?,
            });
        }
    }
    if let Some(policy) = &config.release_handover {
        if let Ok(control) = handover::decode_control(policy, raw) {
            return Ok(WireTransaction::HandoverControl {
                control: serde_json::to_vec(&control)?,
            });
        }
    }
    if let Some(policy) = &config.upgrade {
        if let Ok(control) = upgrade::decode_control(policy, raw) {
            return Ok(WireTransaction::UpgradeControl {
                control: serde_json::to_vec(&control)?,
            });
        }
    }
    if let Ok(value) = serde_json::from_slice::<OrdinaryOuter>(raw) {
        return match value.kind.as_str() {
            "ordinary_v2" => {
                let ordinary = config
                    .ordinary
                    .as_ref()
                    .context("Ordinary profile is not configured")?;
                ensure!(
                    raw.len() as u64 <= ordinary.max_transport_bytes,
                    "Ordinary transport bound differs"
                );
                Ok(WireTransaction::OrdinaryV2 {
                    envelope_base64: value.envelope_base64,
                })
            }
            "ordinary_v3" => {
                let governance = config
                    .governance
                    .as_ref()
                    .context("Governance profile is not configured")?;
                governance.validate_for_activation()?;
                let bytes = recovery_bytes(&value.envelope_base64)?;
                dytallix_protocol_types::ordinary_v3::decode(
                    &bytes,
                    &governance.fee_profile.limits(),
                )?;
                Ok(WireTransaction::OrdinaryV3 {
                    envelope_base64: value.envelope_base64,
                })
            }
            _ => anyhow::bail!("Unsupported ordinary transport type"),
        };
    }
    let value: WireTransaction = serde_json::from_slice(raw)?;
    ensure!(
        !matches!(
            value,
            WireTransaction::OrdinaryV2 { .. } | WireTransaction::OrdinaryV3 { .. }
        ),
        "Ordinary transport requires type discriminator"
    );
    ensure!(
        config.recovery.is_none() || !matches!(value, WireTransaction::Signed { .. }),
        "Legacy signed requests disabled under recovery profile"
    );
    ensure!(
        config.recovery.is_some() || !matches!(value, WireTransaction::Recovery { .. }),
        "Recovery profile is not configured"
    );
    Ok(value)
}
fn ordinary_signed(config: &OrdinaryConfig, value: &str) -> Result<ordinary_wire::SignedOrdinary> {
    ordinary_wire::decode(&recovery_bytes(value)?, &config.fee_profile.limits).map_err(Into::into)
}
fn load_ordinary(
    storage: &Storage,
    config: &ConsensusConfig,
    book: Option<&RecoveryBook>,
) -> Result<Option<OrdinaryState>> {
    match &config.ordinary {
        Some(expected) => {
            let book = book.context("Ordinary recovery state missing")?;
            let nonces = ordinary_state::read_native_nonces(storage, book)?;
            let rewards = RewardState::decode(
                &storage
                    .db
                    .get(REWARD_STATE_KEY)?
                    .context("Ordinary reward state missing")?,
            )?;
            let lifecycle = lifecycle_state(storage)?;
            validate_ordinary_principals(book, &rewards, lifecycle.as_ref())?;
            Ok(Some(ordinary_state::load(
                storage,
                expected,
                config
                    .lifecycle
                    .as_ref()
                    .context("Ordinary lifecycle missing")?,
                book,
                &nonces,
            )?))
        }
        None => {
            for entry in storage.db.iterator(IteratorMode::From(
                b"ordinary:",
                rocksdb::Direction::Forward,
            )) {
                let (key, _) = entry?;
                ensure!(
                    !key.starts_with(b"ordinary:"),
                    "Unconfigured ordinary state"
                );
                break;
            }
            Ok(None)
        }
    }
}
fn shared_meter(
    ordinary: &OrdinaryConfig,
    recovery: &crate::recovery_fees::RecoveryBlock,
) -> Result<SharedBlockMeter> {
    let profile = &recovery.book.profile;
    let mut meter = SharedBlockMeter::new(
        &ordinary.fee_profile,
        RecoveryCeilings {
            max_gas: profile.max_block_gas,
            max_bytes: profile.max_block_recovery_bytes,
            max_signatures: profile.max_block_recovery_signatures,
            mandatory_expiry_gas: profile.mandatory_expiry_gas_budget,
        },
    )?;
    meter.charge_expiry(recovery.expiry_gas_used)?;
    Ok(meter)
}
fn committed_ordinary_settlement(storage: Arc<Storage>) -> Result<Settlement> {
    let mut writes = Writes::new();
    for key in [
        REWARD_STATE_KEY,
        LIFECYCLE_STATE_KEY,
        PENALTY_STATE_KEY,
        "emission:pool:staking_rewards",
    ] {
        if let Some(value) = storage.db.get(key)? {
            writes.insert(key.as_bytes().to_vec(), value);
        }
    }
    // Existing monetary genesis represents an unused emission pool by absence.
    // Use the same logical zero read as the interval planner, without a DB write.
    let pool_key = "emission:pool:staking_rewards";
    let pool: u128 = block_lifecycle::read(&storage, pool_key)?;
    writes.insert(pool_key.as_bytes().to_vec(), bincode::serialize(&pool)?);
    let timestamp = read_head(&storage)?.map_or(0, |head| head.anchor.time_seconds as u64);
    let mut staged = Settlement::new(storage);
    staged.attach_reward_lifecycle(writes, timestamp)?;
    Ok(staged)
}
fn staged_nonces(staged: &mut Settlement, book: &RecoveryBook) -> Result<BTreeMap<String, u64>> {
    book.accounts
        .values()
        .map(|account| {
            Ok((
                account.address.clone(),
                staged.account(&account.address)?.nonce,
            ))
        })
        .collect()
}
fn recovery_bytes(value: &str) -> Result<Vec<u8>> {
    let raw = B64.decode(value).context("Invalid recovery base64")?;
    ensure!(B64.encode(&raw) == value, "Noncanonical recovery base64");
    Ok(raw)
}
fn recovery_book(storage: &Storage, config: &ConsensusConfig) -> Result<Option<RecoveryBook>> {
    let stored = storage.db.get(RECOVERY_STATE_KEY)?;
    match (&config.recovery, stored) {
        (None, None) => Ok(None),
        (Some(initial), Some(raw)) => {
            let current = RecoveryBook::decode(&raw)?;
            ensure!(
                current.profile == initial.profile
                    && current.accounts.keys().eq(initial.accounts.keys()),
                "Recovery profile or account inventory differs"
            );
            for (id, account) in &current.accounts {
                let origin = &initial.accounts[id];
                ensure!(
                    account.address == origin.address
                        && account.recovery.domain == origin.recovery.domain
                        && account.recovery.config == origin.recovery.config,
                    "Recovery origin or configuration differs"
                );
            }
            Ok(Some(current))
        }
        _ => anyhow::bail!("Recovery state and configuration differ"),
    }
}
fn validate_ordinary_principals(
    book: &RecoveryBook,
    rewards: &RewardState,
    lifecycle: Option<&LifecycleState>,
) -> Result<()> {
    let registered: BTreeSet<&str> = book.accounts.values().map(|a| a.address.as_str()).collect();
    let owner = |value: &str| -> Result<()> {
        ensure!(
            registered.contains(value),
            "Ordinary principal owner lacks registered stable account"
        );
        Ok(())
    };
    for value in rewards
        .positions
        .keys()
        .chain(rewards.unbonding.keys())
        .chain(rewards.unpaid.keys())
        .chain(rewards.locks.keys())
    {
        owner(value)?;
    }
    if let Some(lifecycle) = lifecycle {
        let view = |view: &crate::runtime::validator_lifecycle::ValidatorView| -> Result<()> {
            for id in view.positions.keys() {
                owner(id)?;
            }
            for validator in view.validators.values() {
                owner(&validator.owner)?;
            }
            Ok(())
        };
        view(&lifecycle.effective)?;
        for history in lifecycle.history.values() {
            view(history)?;
        }
        for schedule in lifecycle.schedules.values() {
            view(&schedule.view)?;
            for pending in &schedule.additions {
                owner(&pending.owner)?;
            }
            for unbond in &schedule.removals {
                owner(&unbond.owner)?;
            }
        }
        for value in lifecycle.unbonding.values() {
            owner(&value.owner)?;
        }
        for value in &lifecycle.reserved_owners {
            owner(value)?;
        }
    }
    Ok(())
}
fn runtime_limits(config: &OrdinaryConfig) -> ordinary_runtime::RuntimeLimits {
    ordinary_runtime::RuntimeLimits {
        max_receipts: config.max_receipts as usize,
        max_retained_profiles: config.max_retained_profiles as usize,
        max_grants: config.max_grants as usize,
    }
}
fn combined_transaction(
    config: &ConsensusConfig,
    raw: &[u8],
    height: u64,
    index: u32,
    state: &mut OrdinaryState,
    recovery: &mut crate::recovery_fees::RecoveryBlock,
    staged: &mut Settlement,
    shared: &mut SharedBlockMeter,
) -> Result<(TxResult, Option<ReservationRequest>)> {
    match wire(config, raw) {
        Ok(WireTransaction::OrdinaryV2 { envelope_base64 }) => {
            let signed = match ordinary_signed(&state.config, &envelope_base64) {
                Ok(signed) => signed,
                Err(_) => {
                    let before = shared.usage().gas;
                    shared.rejected_wire(raw.len() as u64, &state.config.fee_profile)?;
                    let gas = i64::try_from(shared.usage().gas - before)?;
                    return Ok((
                        TxResult {
                            code: 1,
                            gas_wanted: gas,
                            gas_used: gas,
                            log: "Malformed ordinary envelope".into(),
                        },
                        None,
                    ));
                }
            };
            let result = ordinary_runtime::execute_signed(
                &signed,
                &state.config.fee_profile,
                &mut recovery.book,
                &mut state.grants,
                &mut state.history,
                staged,
                height,
                index,
                runtime_limits(&state.config),
                shared,
            )?;
            Ok((TxResult::from_ordinary(&result)?, result.reservation))
        }
        Ok(WireTransaction::Recovery { envelope_base64 }) => {
            let bytes = recovery_bytes(&envelope_base64).unwrap_or_else(|_| raw.to_vec());
            let before = recovery.book.clone();
            let signatures_before = shared.usage().signatures;
            let result = recovery.execute_with_shared(height, index, &bytes, staged, shared)?;
            if result.charged {
                ordinary_runtime::sync_recovery_mirrors(&before, &recovery.book, staged)?;
            }
            let reservation = if result.charged {
                let signed = sponsor_wire::decode(&bytes)?;
                Some(ReservationRequest {
                    context_digest: ordinary_fee_wire::profile_digest(&state.config.fee_profile)?,
                    id: ReservationId::RecoverySponsorship(sponsor_wire::authorization_id(
                        &signed.sponsor,
                    )?),
                    payer: signed.sponsor.sponsor_account_id,
                    nonce: signed.sponsor.sponsor_nonce,
                    fee_cap_udrt: signed.sponsor.maximum_charge,
                    action_debits: vec![],
                    unrestricted_debits: vec![],
                    wire_bytes: bytes.len() as u64,
                    signature_work: shared.usage().signatures - signatures_before,
                })
            } else {
                None
            };
            Ok((TxResult::from_recovery(&result)?, reservation))
        }
        Ok(WireTransaction::EpochObservation { .. }) => Ok((TxResult::observation(), None)),
        _ => {
            let before = shared.usage().gas;
            shared.rejected_wire(raw.len() as u64, &state.config.fee_profile)?;
            let gas = i64::try_from(shared.usage().gas - before)?;
            Ok((
                TxResult {
                    code: 1,
                    gas_wanted: gas,
                    gas_used: gas,
                    log: "Malformed transaction".into(),
                },
                None,
            ))
        }
    }
}
fn input_limits(config: &ConsensusConfig, input: &FinalizedBlockInput) -> Result<()> {
    ensure!(
        (1..=MAX_HISTORY).contains(&input.height),
        "Height exceeds local qualification bound"
    );
    ensure!(
        input.time_seconds >= 0 && (0..1_000_000_000).contains(&input.time_nanos),
        "Invalid engine timestamp"
    );
    valid_hash(&input.hash)?;
    ensure!(input.misbehavior.len() <= 64, "Too many evidence facts");
    ensure!(
        config.penalty.is_some() || input.misbehavior.is_empty(),
        "Evidence requires the penalty qualification profile"
    );
    ensure!(
        input.txs.len() <= config.max_txs,
        "Too many block transactions"
    );
    let mut total = 0usize;
    for tx in &input.txs {
        ensure!(
            tx.len() <= config.max_tx_bytes,
            "Transaction exceeds byte bound"
        );
        total = total
            .checked_add(tx.len())
            .context("Block byte count overflow")?;
    }
    ensure!(total <= config.max_block_bytes, "Block exceeds byte bound");
    Ok(())
}

pub(crate) fn committed_parent_time(storage: &Storage, next: u64) -> Result<(u64, i32)> {
    ensure!(next > 0, "Invalid next block height");
    if next == 1 {
        return Ok((0, 0));
    }
    let parent: BlockRecord = decode(
        &storage
            .db
            .get(record_key(next - 1))?
            .context("Committed parent block missing")?,
    )?;
    ensure!(
        parent.input.height == next - 1
            && parent.head.anchor.height == next - 1
            && parent.input.time_seconds == parent.head.anchor.time_seconds
            && parent.input.time_nanos == parent.head.anchor.time_nanos,
        "Committed parent time metadata differs"
    );
    Ok((
        u64::try_from(parent.input.time_seconds)?,
        parent.input.time_nanos,
    ))
}
fn evidence_times(
    storage: &Storage,
    config: &ConsensusConfig,
    height: u64,
    facts: &[EvidenceFact],
) -> Result<Vec<(u64, i32)>> {
    ensure!(
        facts.len() <= 64 && (config.penalty.is_some() || facts.is_empty()),
        "Unsupported evidence batch"
    );
    if facts.is_empty() {
        return Ok(Vec::new());
    }
    let mut times = Vec::with_capacity(facts.len());
    let lifecycle = lifecycle_state(storage)?;
    for fact in facts {
        ensure!(
            fact.kind == "duplicate_vote"
                && fact.height > 0
                && fact.height < height
                && fact.validator_address.len() == 40
                && fact
                    .validator_address
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                && (0..1_000_000_000).contains(&fact.time_nanos)
                && fact.power > 0
                && fact.total_power >= fact.power,
            "Malformed or unsupported evidence fact"
        );
        let historical: BlockRecord = decode(
            &storage
                .db
                .get(record_key(fact.height))?
                .context("Evidence height has no committed block")?,
        )?;
        ensure!(
            historical.input.height == fact.height
                && historical.head.anchor.height == fact.height
                && historical.input.time_seconds == historical.head.anchor.time_seconds
                && historical.input.time_nanos == historical.head.anchor.time_nanos,
            "Evidence historical block metadata differs"
        );
        let time = (
            u64::try_from(historical.input.time_seconds)?,
            historical.input.time_nanos,
        );
        ensure!(
            time == (fact.time_seconds, fact.time_nanos),
            "Evidence time differs from committed block"
        );
        let lifecycle = lifecycle
            .as_ref()
            .context("Evidence requires lifecycle history")?;
        let parent_time = committed_parent_time(storage, height)?;
        let block_expired = height - 1 - fact.height > lifecycle.config.evidence_max_age_blocks;
        let time_limit = (
            fact.time_seconds
                .checked_add(lifecycle.config.evidence_max_age_seconds)
                .context("Evidence time limit overflow")?,
            fact.time_nanos,
        );
        ensure!(
            !(block_expired && parent_time > time_limit),
            "Evidence expired before the committed parent"
        );
        let set = lifecycle.historical_validator_set(fact.height)?;
        let mut total = 0i64;
        let mut found = None;
        for validator in set {
            total = total
                .checked_add(validator.power)
                .context("Historical evidence power overflow")?;
            if crate::runtime::validator_lifecycle::consensus_address(&validator.pubkey_base64)?
                == fact.validator_address
            {
                ensure!(
                    found.replace(validator.power).is_none(),
                    "Historical consensus address collision"
                );
            }
        }
        ensure!(
            found == Some(fact.power) && total == fact.total_power,
            "Evidence power differs from committed validator history"
        );
        times.push(time);
    }
    Ok(times)
}

const EMERGENCY_FROZEN: &str = "Emergency transaction freeze";
fn emergency_result() -> TxResult {
    TxResult {
        code: 0,
        gas_wanted: 0,
        gas_used: 0,
        log: "Emergency control recorded".into(),
    }
}
fn emergency_state(
    storage: &Storage,
    config: &ConsensusConfig,
) -> Result<Option<emergency::State>> {
    match (&config.emergency, storage.db.get(emergency::STATE_KEY)?) {
        (Some(policy), Some(bytes)) => Ok(Some(emergency::decode_state(policy, &bytes)?)),
        (None, None) => Ok(None),
        _ => anyhow::bail!("Emergency state/configuration mismatch"),
    }
}
fn finalized_anchor(
    storage: &Storage,
    height: u64,
    parent_height: u64,
) -> Result<emergency::FinalizedAnchor> {
    ensure!(height <= parent_height, "Control anchor is not finalized");
    let app_hash = if height == 0 {
        let raw = storage
            .db
            .get(GENESIS_APP_HASH_KEY)?
            .context("Genesis anchor missing")?;
        String::from_utf8(raw.to_vec())?
    } else {
        let record: BlockRecord = decode(
            &storage
                .db
                .get(record_key(height))?
                .context("Control anchor block missing")?,
        )?;
        ensure!(
            record.input.height == height && record.head.anchor.height == height,
            "Control anchor block height differs"
        );
        record.result.app_hash
    };
    valid_hash(&app_hash)?;
    Ok(emergency::FinalizedAnchor { height, app_hash })
}
/// Bind every immutable control receipt to its actual block, input and result.
/// This structural scan does not establish signature validity. Startup performs
/// cryptographic replay separately with the configured helper.
fn emergency_history(
    storage: &Storage,
    config: &ConsensusConfig,
) -> Result<Vec<(emergency::Receipt, emergency::BlockContext)>> {
    Ok(emergency_history_with(&HistoryRead::new(storage), config)?.records)
}
fn emergency_history_with(
    history: &HistoryRead<'_>,
    config: &ConsensusConfig,
) -> Result<EmergencyTrace> {
    let storage = history.storage;
    let mut keys = BTreeSet::new();
    for item in storage.db.iterator(IteratorMode::Start) {
        let (key, _) = item?;
        if key.starts_with(b"consensus:emergency:")
            && !key.starts_with(upgrade::INDEX_PREFIX.as_bytes())
            && key.as_ref() != upgrade::INDEX_STATE_KEY.as_bytes()
        {
            keys.insert(key.to_vec());
        }
    }
    let Some(policy) = &config.emergency else {
        ensure!(keys.is_empty(), "Unconfigured emergency state");
        return Ok(EmergencyTrace {
            records: Vec::new(),
            states: Vec::new(),
        });
    };
    ensure!(
        storage.db.get(crate::root_genesis::STATE_KEY)?.is_some(),
        "Emergency root authorization missing"
    );
    ensure!(
        keys.remove(emergency::STATE_KEY.as_bytes()),
        "Emergency state missing"
    );
    let height = block_lifecycle::height(storage, "meta:height")?;
    ensure!(
        height <= MAX_HISTORY,
        "Emergency history exceeds consensus history bound"
    );
    let mut receipts = Vec::new();
    let mut frozen = false;
    for h in 1..=height {
        let record = history.block(h)?;
        let controls = record
            .input
            .txs
            .iter()
            .enumerate()
            .filter_map(|(index, raw)| match wire(config, raw) {
                Ok(WireTransaction::EmergencyControl { control }) => Some((index, control)),
                _ => None,
            })
            .collect::<Vec<_>>();
        ensure!(controls.len() <= 1, "Multiple committed emergency controls");
        let mut next_frozen = frozen;
        let mut control_index = None;
        if let Some((index, raw)) = controls.first() {
            let control = emergency::decode_control(policy, raw)?;
            let key = emergency::receipt_key(control.payload.sequence);
            ensure!(
                keys.remove(key.as_bytes()),
                "Emergency receipt missing or reused"
            );
            let receipt = emergency::decode_receipt(
                policy,
                &storage.db.get(&key)?.context("Emergency receipt missing")?,
            )?;
            let context = emergency::BlockContext {
                active_release_sha512: recorded_release_at(history, config, h - 1)?,
                height: h,
                parent_height: h - 1,
                parent_app_hash: record.head.anchor.prior_app_hash.clone(),
                finalized_anchor: control
                    .payload
                    .v2
                    .as_ref()
                    .map(|_| history.anchor(control.payload.parent_height, h - 1))
                    .transpose()?,
            };
            ensure!(
                receipt.control == control
                    && receipt.context == context
                    && record.result.tx_results.get(*index) == Some(&emergency_result()),
                "Emergency receipt differs from committed control position"
            );
            next_frozen = control.payload.action == emergency::Action::Freeze;
            control_index = Some(*index);
            receipts.push((receipt, context));
        }
        for (index, raw) in record.input.txs.iter().enumerate() {
            if control_index == Some(index) {
                continue;
            }
            if matches!(
                wire(config, raw),
                Ok(WireTransaction::EpochObservation { .. })
            ) {
                continue;
            }
            if frozen || next_frozen {
                ensure!(
                    record.result.tx_results.get(index)
                        == Some(&TxResult::invalid(EMERGENCY_FROZEN)),
                    "User transaction executed in an emergency-frozen block"
                );
            }
        }
        frozen = next_frozen;
    }
    ensure!(keys.is_empty(), "Unknown or orphan emergency record");
    let mut states = vec![emergency::State::new(policy)?];
    for (receipt, context) in &receipts {
        let next = emergency::replay_recorded_step(
            policy,
            states.last().context("Emergency trace missing")?,
            receipt,
            context,
        )?;
        #[cfg(test)]
        history
            .emergency_steps
            .set(history.emergency_steps.get() + 1);
        states.push(next);
    }
    ensure!(
        states.last().cloned() == emergency_state(storage, config)?,
        "Emergency state/history differs"
    );
    Ok(EmergencyTrace {
        records: receipts,
        states,
    })
}

fn handover_result() -> TxResult {
    TxResult {
        code: 0,
        gas_wanted: 0,
        gas_used: 0,
        log: "Release handover control recorded".into(),
    }
}
fn handover_state(storage: &Storage, config: &ConsensusConfig) -> Result<Option<handover::State>> {
    match (
        &config.release_handover,
        storage.db.get(handover::STATE_KEY)?,
    ) {
        (Some(policy), Some(raw)) => Ok(Some(handover::decode_state(policy, &raw)?)),
        (None, None) => Ok(None),
        _ => anyhow::bail!("Handover state/configuration mismatch"),
    }
}
/// Extract only structurally bound committed release transitions. This is a
/// dependency for historical emergency replay, not startup authorization.
/// Startup independently verifies every handover signature and paired migration.
fn recorded_release_at(
    history: &HistoryRead<'_>,
    config: &ConsensusConfig,
    parent: u64,
) -> Result<Option<String>> {
    let Some(policy) = &config.release_handover else {
        return Ok(None);
    };
    let policy_hash = policy.sha256()?;
    if let Some((cached_policy, trace)) = history.release_trace.borrow().as_ref() {
        ensure!(
            *cached_policy == policy_hash,
            "Release trace policy differs"
        );
        let end = trace.partition_point(|(height, _)| *height <= parent);
        return Ok(end
            .checked_sub(1)
            .map(|i| trace[i].1.clone())
            .filter(|release| *release != policy.initial_release_sha512));
    }
    let storage = history.storage;
    let mut trace = Vec::new();
    let mut prior_height = 0;
    let mut active = policy.initial_release_sha512.clone();
    for item in storage.db.prefix_iterator(handover::RECEIPT_PREFIX) {
        let (key, bytes) = item?;
        if !key.starts_with(handover::RECEIPT_PREFIX.as_bytes()) {
            break;
        }
        let receipt = handover::decode_receipt(policy, &bytes)?;
        ensure!(
            key.as_ref() == handover::receipt_key(receipt.sequence()).as_bytes(),
            "Handover history sequence key differs"
        );
        let h = receipt.context.height;
        ensure!(h > prior_height, "Handover history height order differs");
        prior_height = h;
        let record = history.block(h)?;
        let raw = serde_json::to_vec(&receipt.control)?;
        let indices: Vec<_> = record
            .input
            .txs
            .iter()
            .enumerate()
            .filter(|(_, tx)| **tx == raw)
            .map(|(i, _)| i)
            .collect();
        ensure!(
            indices.len() == 1
                && record.result.tx_results.get(indices[0]) == Some(&handover_result())
                && receipt.context.parent_app_hash == record.head.anchor.prior_app_hash
                && receipt.control.payload.source_release_sha512 == active,
            "Handover release history differs from committed input/result"
        );
        if let handover::Action::Activate { plan, .. } = &receipt.control.payload.action {
            active = plan.target_release_sha512.clone();
            trace.push((h, active.clone()));
        }
    }
    let end = trace.partition_point(|(height, _)| *height <= parent);
    let selected = end
        .checked_sub(1)
        .map(|i| trace[i].1.clone())
        .filter(|release| *release != policy.initial_release_sha512);
    *history.release_trace.borrow_mut() = Some((policy_hash, trace));
    Ok(selected)
}
fn handover_context(
    height: u64,
    parent_app_hash: String,
    source_schema: u16,
    state: &emergency::State,
    control_present: bool,
) -> Result<handover::BlockContext> {
    Ok(handover::BlockContext {
        height,
        parent_height: height.checked_sub(1).context("Handover genesis action")?,
        parent_app_hash,
        source_schema,
        emergency_frozen: state.frozen(),
        emergency_control_present: control_present,
        emergency_upgrade_hold: state.blocks_upgrade(),
        emergency_receipt_sha256: state.last_receipt_sha256().map(str::to_owned),
    })
}
fn handover_history(
    history: &HistoryRead<'_>,
    config: &ConsensusConfig,
    emergency_trace: &EmergencyTrace,
    upgrade_outcomes: &BTreeMap<u64, upgrade::BlockPlan>,
    verifier: Option<&dyn handover::Verifier>,
) -> Result<()> {
    let storage = history.storage;
    let emergency_records = &emergency_trace.records;
    let mut actual = BTreeMap::new();
    for item in storage.db.prefix_iterator(b"consensus:release-handover:") {
        let (key, bytes) = item?;
        if !key.starts_with(b"consensus:release-handover:") {
            break;
        }
        actual.insert(key.to_vec(), bytes.to_vec());
    }
    let Some(policy) = &config.release_handover else {
        ensure!(actual.is_empty(), "Unconfigured handover state");
        return Ok(());
    };

    let up = config
        .upgrade
        .as_ref()
        .context("Handover upgrade policy missing")?;
    let mut state = handover::State::new(policy)?;
    let mut schema = upgrade::State::new(up)?.active_schema();
    let mut expected = BTreeMap::new();
    let height = block_lifecycle::height(storage, "meta:height")?;
    ensure!(height <= MAX_HISTORY, "Handover history bound");
    for h in 1..=height {
        let record = history.block(h)?;
        let prior_end = emergency_records.partition_point(|(_, c)| c.height < h);
        let current_end = emergency_records.partition_point(|(_, c)| c.height <= h);
        let emergency_state = &emergency_trace.states[prior_end];
        let context = handover_context(
            h,
            record.head.anchor.prior_app_hash.clone(),
            schema,
            &emergency_state,
            prior_end != current_end,
        )?;
        let outcome = upgrade_outcomes.get(&h);
        let migration = outcome
            .filter(|p| p.migration.is_some())
            .map(|p| {
                let raw = serde_json::to_vec(
                    &p.receipt
                        .as_ref()
                        .context("Migration receipt missing")?
                        .control,
                )?;
                handover::PreparedUpgradeOutcome::from_verified_upgrade(&raw, p)
            })
            .transpose()?;
        let controls: Vec<_> = record
            .input
            .txs
            .iter()
            .enumerate()
            .filter_map(|(i, raw)| match wire(config, raw) {
                Ok(WireTransaction::HandoverControl { control }) => Some((i, control)),
                _ => None,
            })
            .collect();
        if context.emergency_frozen || context.emergency_control_present {
            ensure!(migration.is_none(), "Emergency block migrated state");
            for (index, _) in &controls {
                ensure!(
                    record.result.tx_results.get(*index)
                        == Some(&TxResult::invalid(EMERGENCY_FROZEN)),
                    "Blocked handover result differs"
                );
            }
        } else {
            ensure!(controls.len() <= 1, "Multiple committed handover controls");
            if let Some((index, raw)) = controls.first() {
                let control = handover::decode_control(policy, raw)?;
                let key = handover::receipt_key(control.payload.sequence);
                let bytes = storage.db.get(&key)?.context("Handover receipt missing")?;
                let receipt = handover::decode_receipt(policy, &bytes)?;
                ensure!(
                    receipt.control == control
                        && record.result.tx_results.get(*index) == Some(&handover_result()),
                    "Handover receipt input/result differs"
                );
                let plan = handover::replay_record(
                    policy,
                    &state,
                    &receipt,
                    &context,
                    migration.as_ref(),
                    verifier,
                )?;
                ensure!(
                    expected.insert(key.into_bytes(), bytes.to_vec()).is_none(),
                    "Handover receipt reused"
                );
                state = plan.state;
            } else {
                ensure!(migration.is_none(), "Unpaired historical migration");
            }
        }
        if let Some(outcome) = outcome {
            schema = outcome.state.active_schema();
        }
        ensure!(
            state.active_schema() == schema,
            "Historical handover schema differs from migration"
        );
    }
    expected.insert(
        handover::STATE_KEY.as_bytes().to_vec(),
        handover::encode_state(&state)?,
    );
    ensure!(
        expected == actual,
        "Handover state or receipts differ from verified history"
    );
    Ok(())
}

fn upgrade_result() -> TxResult {
    TxResult {
        code: 0,
        gas_wanted: 0,
        gas_used: 0,
        log: "Upgrade control recorded".into(),
    }
}
fn upgrade_state(storage: &Storage, config: &ConsensusConfig) -> Result<Option<upgrade::State>> {
    match (&config.upgrade, storage.db.get(upgrade::STATE_KEY)?) {
        (Some(policy), Some(raw)) => Ok(Some(upgrade::decode_state(policy, &raw)?)),
        (None, None) => Ok(None),
        _ => anyhow::bail!("Upgrade state/configuration mismatch"),
    }
}
fn upgrade_context(
    height: u64,
    parent_app_hash: String,
    state: &emergency::State,
    control_present: bool,
) -> Result<upgrade::BlockContext> {
    Ok(upgrade::BlockContext {
        height,
        parent_height: height.checked_sub(1).context("Upgrade genesis action")?,
        parent_app_hash,
        emergency_frozen: state.frozen(),
        emergency_control_present: control_present,
        emergency_upgrade_hold: state.blocks_upgrade(),
        emergency_receipt_sha256: state.last_receipt_sha256().map(str::to_owned),
    })
}
/// Recreate every upgrade transition from actual committed controls. All migration
/// writes are compared byte-for-byte; this function never repairs missing state.
fn upgrade_history(
    history: &HistoryRead<'_>,
    config: &ConsensusConfig,
    emergency_trace: &EmergencyTrace,
    verifier: Option<&dyn upgrade::Verifier>,
) -> Result<BTreeMap<u64, upgrade::BlockPlan>> {
    let storage = history.storage;
    let emergency_records = &emergency_trace.records;
    let mut outcomes = BTreeMap::new();
    let mut actual = BTreeMap::new();
    for item in storage.db.iterator(IteratorMode::Start) {
        let (key, value) = item?;
        if key.starts_with(b"consensus:upgrade:")
            || key.starts_with(upgrade::INDEX_PREFIX.as_bytes())
            || key.as_ref() == upgrade::INDEX_STATE_KEY.as_bytes()
        {
            actual.insert(key.to_vec(), value.to_vec());
        }
    }
    let Some(policy) = &config.upgrade else {
        ensure!(actual.is_empty(), "Unconfigured upgrade/index state");
        return Ok(outcomes);
    };
    let emergency_policy = config
        .emergency
        .as_ref()
        .context("Upgrade emergency policy missing")?;
    let mut state = upgrade::State::new(policy)?;
    let mut expected = BTreeMap::new();
    let height = block_lifecycle::height(storage, "meta:height")?;
    ensure!(height <= MAX_HISTORY, "Upgrade history bound");
    for h in 1..=height {
        let record = history.block(h)?;
        let prior_end = emergency_records.partition_point(|(_, context)| context.height < h);
        let current_end = emergency_records.partition_point(|(_, context)| context.height <= h);
        let prior_records = &emergency_records[..prior_end];
        let emergency_present = current_end != prior_end;
        let frozen = prior_records.last().is_some_and(|(receipt, _)| {
            receipt.control.payload.action == emergency::Action::Freeze
        });
        let controls: Vec<_> = record
            .input
            .txs
            .iter()
            .enumerate()
            .filter_map(|(index, raw)| match wire(config, raw) {
                Ok(WireTransaction::UpgradeControl { control }) => Some((index, control)),
                _ => None,
            })
            .collect();
        if frozen || emergency_present {
            for (index, _) in &controls {
                ensure!(
                    record.result.tx_results.get(*index)
                        == Some(&TxResult::invalid(EMERGENCY_FROZEN)),
                    "Blocked upgrade result differs"
                );
            }
        } else {
            ensure!(controls.len() <= 1, "Multiple committed upgrade controls");
            if let Some((index, raw)) = controls.first() {
                let emergency_state = &emergency_trace.states[prior_end];
                let history = upgrade::VerifiedEmergencyHistory::from_records(
                    emergency_policy,
                    prior_records,
                    &emergency_state,
                )?;
                let context = upgrade_context(
                    h,
                    record.head.anchor.prior_app_hash.clone(),
                    &emergency_state,
                    false,
                )?;
                let control = upgrade::decode_control(policy, raw)?;
                let key = upgrade::receipt_key(control.payload.sequence);
                let bytes = storage.db.get(&key)?.context("Upgrade receipt missing")?;
                let receipt = upgrade::decode_receipt(policy, &bytes)?;
                ensure!(
                    receipt.control == control
                        && record.result.tx_results.get(*index) == Some(&upgrade_result()),
                    "Upgrade receipt differs from committed input/result"
                );
                let plan =
                    upgrade::replay_record(policy, &state, &receipt, &context, &history, verifier)?;
                ensure!(
                    expected.insert(key.into_bytes(), bytes.to_vec()).is_none(),
                    "Reused upgrade receipt"
                );
                outcomes.insert(h, plan.clone());
                if let Some(migration) = plan.migration {
                    for (key, value) in migration.writes {
                        ensure!(
                            expected.insert(key, value).is_none(),
                            "Reused migration key"
                        );
                    }
                }
                state = plan.state;
            }
        }
        if state.active_schema() == 1 {
            for (receipt, _) in &emergency_records[prior_end..current_end] {
                ensure!(
                    expected
                        .insert(
                            upgrade::index_key(&receipt.sha256()?)?.into_bytes(),
                            receipt.sequence().to_be_bytes().to_vec()
                        )
                        .is_none(),
                    "Duplicate indexed emergency receipt"
                );
            }
        }
    }
    expected.insert(
        upgrade::STATE_KEY.as_bytes().to_vec(),
        upgrade::encode_state(&state)?,
    );
    ensure!(
        actual == expected,
        "Upgrade state, receipts or digest index differ from committed history"
    );
    Ok(outcomes)
}

impl ConsensusApplication {
    fn ensure_active_runtime(&self) -> Result<()> {
        if let Some(policy) = &self.config.release_handover {
            let candidate = self
                .runtime_candidate
                .as_ref()
                .context("Handover runtime candidate missing")?;
            let active = if self.storage.db.get(MODE_KEY)?.is_some() {
                handover_state(&self.storage, &self.config)?
                    .context("Handover state missing")?
                    .active_release_sha512()
                    .to_owned()
            } else {
                policy.initial_release_sha512.clone()
            };
            ensure!(
                candidate.manifest_sha512() == active,
                "Committed release requires a different runtime executable"
            );
        }
        Ok(())
    }
    fn handover_plan(
        &self,
        height: u64,
        txs: &[Vec<u8>],
        emergency_plan: Option<&emergency::BlockPlan>,
        upgrade_plan: Option<&upgrade::BlockPlan>,
    ) -> Result<Option<handover::BlockPlan>> {
        let Some(policy) = &self.config.release_handover else {
            return Ok(None);
        };
        let state =
            handover_state(&self.storage, &self.config)?.context("Handover state missing")?;
        let emergency_state = emergency_state(&self.storage, &self.config)?
            .context("Handover emergency state missing")?;
        let present = emergency_plan.is_some_and(|p| p.receipt.is_some());
        if emergency_state.frozen() || present {
            ensure!(
                !upgrade_plan.is_some_and(|p| p.migration.is_some()),
                "Emergency blocked migration"
            );
            return Ok(Some(handover::BlockPlan {
                state,
                receipt: None,
                activated_release_sha512: None,
            }));
        }
        let controls: Vec<_> = txs
            .iter()
            .filter_map(|raw| match wire(&self.config, raw) {
                Ok(WireTransaction::HandoverControl { control }) => Some(control),
                _ => None,
            })
            .collect();
        ensure!(
            controls.len() <= 1,
            "Only one handover control is permitted per block"
        );
        let migration = upgrade_plan
            .filter(|p| p.migration.is_some())
            .map(|p| {
                let raw = serde_json::to_vec(
                    &p.receipt
                        .as_ref()
                        .context("Migration receipt missing")?
                        .control,
                )?;
                handover::PreparedUpgradeOutcome::from_verified_upgrade(&raw, p)
            })
            .transpose()?;
        let context = handover_context(
            height,
            current_info(&self.storage)?.app_hash,
            upgrade_state(&self.storage, &self.config)?
                .context("Handover upgrade state missing")?
                .active_schema(),
            &emergency_state,
            false,
        )?;
        Ok(Some(handover::plan_block(
            policy,
            &state,
            &context,
            controls.first().map(Vec::as_slice),
            migration.as_ref(),
            self.emergency_verifier
                .as_ref()
                .context("Handover verifier missing")?,
        )?))
    }
    fn emergency_context(&self, height: u64) -> Result<emergency::BlockContext> {
        let info = current_info(&self.storage)?;
        ensure!(
            info.height.checked_add(1) == Some(height),
            "Emergency block is not next"
        );
        Ok(emergency::BlockContext {
            active_release_sha512: recorded_release_at(
                &HistoryRead::new(&self.storage),
                &self.config,
                height - 1,
            )?,
            height,
            parent_height: info.height,
            parent_app_hash: info.app_hash,
            finalized_anchor: None,
        })
    }
    fn emergency_plan(&self, height: u64, txs: &[Vec<u8>]) -> Result<Option<emergency::BlockPlan>> {
        let Some(policy) = &self.config.emergency else {
            return Ok(None);
        };
        let state =
            emergency_state(&self.storage, &self.config)?.context("Emergency state missing")?;
        let mut control = None;
        for raw in txs {
            if let Ok(WireTransaction::EmergencyControl { control: bytes }) =
                wire(&self.config, raw)
            {
                ensure!(
                    control.replace(bytes).is_none(),
                    "Only one emergency control is permitted per block"
                );
            }
        }
        let mut context = self.emergency_context(height)?;
        if let Some(raw) = &control {
            let decoded = emergency::decode_control(policy, raw)?;
            if decoded.payload.v2.is_some() {
                context.finalized_anchor = Some(finalized_anchor(
                    &self.storage,
                    decoded.payload.parent_height,
                    context.parent_height,
                )?);
            }
        }
        Ok(Some(emergency::plan_block(
            policy,
            &state,
            &context,
            control.as_deref(),
            self.emergency_verifier
                .as_ref()
                .context("Emergency verifier unavailable")? as &dyn ControlVerifier,
        )?))
    }
    fn upgrade_plan(
        &self,
        height: u64,
        txs: &[Vec<u8>],
        emergency_plan: Option<&emergency::BlockPlan>,
    ) -> Result<Option<upgrade::BlockPlan>> {
        let Some(policy) = &self.config.upgrade else {
            return Ok(None);
        };
        let state = upgrade_state(&self.storage, &self.config)?.context("Upgrade state missing")?;
        let emergency_state = emergency_state(&self.storage, &self.config)?
            .context("Upgrade emergency state missing")?;
        let emergency_present = emergency_plan.is_some_and(|plan| plan.receipt.is_some());
        // A same-block emergency action has priority. No upgrade sequence,
        // receipt, pending plan or migration write changes in this branch.
        if emergency_state.frozen() || emergency_present {
            return Ok(Some(upgrade::BlockPlan {
                state,
                receipt: None,
                migration: None,
            }));
        }
        let mut control = None;
        for raw in txs {
            if let Ok(WireTransaction::UpgradeControl { control: bytes }) = wire(&self.config, raw)
            {
                ensure!(
                    control.replace(bytes).is_none(),
                    "Only one upgrade control is permitted per block"
                );
            }
        }
        let info = current_info(&self.storage)?;
        ensure!(
            info.height.checked_add(1) == Some(height),
            "Upgrade block is not next"
        );
        if control.is_none() {
            return Ok(Some(upgrade::BlockPlan {
                state,
                receipt: None,
                migration: None,
            }));
        }
        let context = upgrade_context(height, info.app_hash, &emergency_state, false)?;
        let records = emergency_history(&self.storage, &self.config)?;
        let history = upgrade::VerifiedEmergencyHistory::from_records(
            self.config
                .emergency
                .as_ref()
                .context("Emergency policy missing")?,
            &records,
            &emergency_state,
        )?;
        Ok(Some(upgrade::plan_block(
            policy,
            &state,
            &context,
            control.as_deref(),
            &history,
            self.emergency_verifier
                .as_ref()
                .context("Upgrade verifier unavailable")?,
        )?))
    }
    /// Give emergency input priority, then select one admissible upgrade.
    /// A required epoch observation remains first. User candidates are omitted.
    fn prepare_emergency_proposal(
        &self,
        height: u64,
        txs: &mut Vec<Vec<u8>>,
        max_bytes: u64,
    ) -> Result<Option<Vec<Vec<u8>>>> {
        if self.config.emergency.is_none() {
            return Ok(None);
        }
        let state =
            emergency_state(&self.storage, &self.config)?.context("Emergency state missing")?;
        let timing = timing(&self.storage)?;
        let boundary = height > 1 && (height - 1) % timing.config.epoch_blocks == 0;
        let parent =
            read_head(&self.storage)?.map_or_else(|| "genesis".into(), |h| h.anchor.engine_hash);
        let limit = usize::try_from(max_bytes)
            .unwrap_or(usize::MAX)
            .min(self.config.max_block_bytes);
        let mut observation = None;
        for raw in txs.iter().take(self.config.max_txs.saturating_mul(4)) {
            if let Ok(WireTransaction::EpochObservation { observation: obs }) =
                wire(&self.config, raw)
            {
                if boundary
                    && observation.is_none()
                    && crate::runtime::issuance_timing::plan_block(
                        &self.storage,
                        &timing,
                        height,
                        &parent,
                        Some(&obs),
                    )
                    .is_ok()
                {
                    observation = Some(raw.clone());
                }
            }
        }
        let observation_size = observation.as_ref().map_or(0, Vec::len);
        let mut selected_controls = Vec::new();
        for raw in txs.iter().take(self.config.max_txs.saturating_mul(4)) {
            if matches!(
                wire(&self.config, raw),
                Ok(WireTransaction::EmergencyControl { .. })
            ) {
                if raw.len().saturating_add(observation_size) > limit
                    || usize::from(observation.is_some()) + 1 > self.config.max_txs
                {
                    continue;
                }
                match self.emergency_plan(height, std::slice::from_ref(raw)) {
                    Ok(Some(_)) => {
                        selected_controls = vec![raw.clone()];
                        break;
                    }
                    Err(error) if crate::emergency_verifier::is_infrastructure_error(&error) => {
                        return Err(error)
                    }
                    _ => {}
                }
            }
        }
        if !state.frozen() && selected_controls.is_empty() {
            if let Some(policy) = &self.config.release_handover {
                for raw in txs.iter().take(self.config.max_txs.saturating_mul(4)) {
                    let Ok(control) = handover::decode_control(policy, raw) else {
                        continue;
                    };
                    let mut pair = Vec::new();
                    if let handover::Action::Activate {
                        upgrade_activation_sha256: Some(digest),
                        ..
                    } = &control.payload.action
                    {
                        let Some(companion) = txs
                            .iter()
                            .take(self.config.max_txs.saturating_mul(4))
                            .find(|tx| {
                                hex::encode(Sha256::digest(tx)) == *digest
                                    && matches!(
                                        wire(&self.config, tx),
                                        Ok(WireTransaction::UpgradeControl { .. })
                                    )
                            })
                        else {
                            continue;
                        };
                        pair.push(companion.clone());
                    }
                    pair.push(raw.clone());
                    if pair
                        .iter()
                        .map(Vec::len)
                        .sum::<usize>()
                        .saturating_add(observation_size)
                        > limit
                        || pair.len() + usize::from(observation.is_some()) > self.config.max_txs
                    {
                        continue;
                    }
                    let checked = (|| -> Result<()> {
                        let upgrade = self.upgrade_plan(height, &pair, None)?;
                        let plan = self
                            .handover_plan(height, &pair, None, upgrade.as_ref())?
                            .context("Handover policy missing")?;
                        ensure!(plan.receipt.is_some(), "Handover receipt missing");
                        Ok(())
                    })();
                    match checked {
                        Ok(()) => {
                            selected_controls = pair;
                            break;
                        }
                        Err(error)
                            if crate::emergency_verifier::is_infrastructure_error(&error) =>
                        {
                            return Err(error)
                        }
                        _ => {}
                    }
                }
            }
        }
        if !state.frozen() && selected_controls.is_empty() && self.config.upgrade.is_some() {
            for raw in txs.iter().take(self.config.max_txs.saturating_mul(4)) {
                if !matches!(
                    wire(&self.config, raw),
                    Ok(WireTransaction::UpgradeControl { .. })
                ) {
                    continue;
                }
                if raw.len().saturating_add(observation_size) > limit
                    || usize::from(observation.is_some()) + 1 > self.config.max_txs
                {
                    continue;
                }
                match self.upgrade_plan(height, std::slice::from_ref(raw), None) {
                    Ok(Some(plan)) if plan.receipt.is_some() => {
                        match self.handover_plan(
                            height,
                            std::slice::from_ref(raw),
                            None,
                            Some(&plan),
                        ) {
                            Ok(_) => {
                                selected_controls = vec![raw.clone()];
                                break;
                            }
                            Err(error)
                                if crate::emergency_verifier::is_infrastructure_error(&error) =>
                            {
                                return Err(error)
                            }
                            _ => {}
                        }
                    }
                    Err(error) if crate::emergency_verifier::is_infrastructure_error(&error) => {
                        return Err(error)
                    }
                    _ => {}
                }
            }
        }
        txs.retain(|raw| {
            !matches!(
                wire(&self.config, raw),
                Ok(WireTransaction::EmergencyControl { .. }
                    | WireTransaction::UpgradeControl { .. }
                    | WireTransaction::HandoverControl { .. })
            )
        });
        if !state.frozen() && selected_controls.is_empty() {
            return Ok(None);
        }
        if boundary && observation.is_none() || observation_size > limit {
            return Ok(Some(Vec::new()));
        }
        let mut result = Vec::new();
        if let Some(raw) = observation {
            result.push(raw);
        }
        result.extend(selected_controls);
        Ok(Some(result))
    }
    fn validate_evidence(&self, height: u64, facts: &[EvidenceFact]) -> Result<Vec<(u64, i32)>> {
        evidence_times(&self.storage, &self.config, height, facts)
    }
    pub fn open(
        path: impl AsRef<Path>,
        config: ConsensusConfig,
        genesis_bytes: Vec<u8>,
    ) -> Result<Self> {
        Self::open_inner(path, config, genesis_bytes, None, None, None)
    }
    /// Explicit development API. The ordinary open path remains unchanged.
    /// Production profiles are rejected by existing configuration validation.
    pub fn open_with_development_root(
        path: impl AsRef<Path>,
        config: ConsensusConfig,
        genesis_bytes: Vec<u8>,
        consensus_source: &[u8],
        authorization: crate::root_genesis::DevelopmentRootGenesis,
    ) -> Result<Self> {
        config.validate()?;
        source_binding(&config, &genesis_bytes)?;
        ensure!(
            !consensus_source.is_empty() && consensus_source.len() <= 65_536,
            "Exact consensus source exceeds supported bound"
        );
        let supplied: ConsensusConfig = serde_json::from_slice(consensus_source)?;
        ensure!(
            supplied == config,
            "Exact consensus source differs from selected runtime configuration"
        );
        let prepared = authorization.prepare(&config.chain_id, &genesis_bytes, consensus_source)?;
        Self::open_inner(path, config, genesis_bytes, Some(prepared), None, None)
    }
    /// Root-bound development configuration with an explicit local verifier.
    pub fn open_with_development_emergency(
        path: impl AsRef<Path>,
        config: ConsensusConfig,
        genesis_bytes: Vec<u8>,
        consensus_source: &[u8],
        authorization: crate::root_genesis::DevelopmentRootGenesis,
        verifier: EmergencyVerifierConfig,
    ) -> Result<Self> {
        Self::open_with_development_candidate(
            path,
            config,
            genesis_bytes,
            consensus_source,
            authorization,
            verifier,
            None,
        )
    }
    pub fn open_with_development_candidate(
        path: impl AsRef<Path>,
        config: ConsensusConfig,
        genesis_bytes: Vec<u8>,
        consensus_source: &[u8],
        authorization: crate::root_genesis::DevelopmentRootGenesis,
        verifier: EmergencyVerifierConfig,
        candidate: Option<DevelopmentCandidateInput>,
    ) -> Result<Self> {
        Self::open_with_development_runtime_candidate(
            path, config, genesis_bytes, consensus_source, authorization, verifier,
            candidate.map(crate::runtime_candidate_v2::RuntimeCandidateInput::V1),
        )
    }
    /// Additive runtime dispatch. The existing V1 public entry point and JSON
    /// configuration retain their original shape. Candidate input supplies
    /// paths and bounds only; authority comes from verified root/chain state.
    pub fn open_with_development_runtime_candidate(
        path: impl AsRef<Path>,
        config: ConsensusConfig,
        genesis_bytes: Vec<u8>,
        consensus_source: &[u8],
        authorization: crate::root_genesis::DevelopmentRootGenesis,
        verifier: EmergencyVerifierConfig,
        candidate: Option<crate::runtime_candidate_v2::RuntimeCandidateInput>,
    ) -> Result<Self> {
        config.validate()?;
        ensure!(
            config
                .emergency
                .as_ref()
                .context("Emergency policy required")?
                .release_sha512
                == authorization.release_manifest_sha512,
            "Emergency release differs from root-authorized release manifest"
        );
        source_binding(&config, &genesis_bytes)?;
        ensure!(
            !consensus_source.is_empty() && consensus_source.len() <= 65_536,
            "Exact consensus source exceeds supported bound"
        );
        ensure!(
            serde_json::from_slice::<ConsensusConfig>(consensus_source)? == config,
            "Exact consensus source differs from selected runtime configuration"
        );
        let bootstrap_verifier = if matches!(candidate.as_ref(),
            Some(crate::runtime_candidate_v2::RuntimeCandidateInput::V2(_))) {
            // V2 live settings cannot select the executable used to establish
            // the authority that will later authorize that same executable.
            Some(bootstrap_history_verifier(&authorization)?)
        } else {
            None
        };
        // V2 independent scratch and bounds are checked before even the
        // original helper can execute through genesis authorization.
        let prepared = authorization.prepare(&config.chain_id, &genesis_bytes, consensus_source)?;
        let prepared_verifier = if let Some(bootstrap) = bootstrap_verifier {
            bootstrap
        } else {
            // Preserve the explicit legacy V1 helper configuration path.
            EmergencyVerifier::new(verifier.clone())?
        };
        let candidate = candidate.map(|input| PreparedRuntimeCandidateInput {
            input, root: authorization, verifier,
        });
        Self::open_inner(
            path,
            config,
            genesis_bytes,
            Some(prepared),
            Some(prepared_verifier),
            candidate,
        )
    }
    /// Verify release selection without opening writable storage or inspecting
    /// a caller-selected candidate manifest. The caller must keep its exclusive
    /// lifecycle lease through subsequent candidate validation and launch.
    /// This result does not itself acquire that lease or authorize production.
    /// Compatibility API: the live verifier argument is ignored. Authority
    /// replay requires the original root's explicit private scratch directory.
    pub fn preflight_development_release(
        path: impl AsRef<Path>,
        config: &ConsensusConfig,
        genesis_bytes: &[u8],
        consensus_source: &[u8],
        authorization: &crate::root_genesis::DevelopmentRootGenesis,
        _verifier: &EmergencyVerifierConfig,
    ) -> Result<VerifiedReleaseAuthority> {
        // Compatibility wrapper only. The supplied live configuration is
        // deliberately ignored; it cannot establish committed release authority.
        Self::preflight_development_release_from_root(
            path, config, genesis_bytes, consensus_source, authorization,
        )
    }
    /// Replay authority with only the original independent root helper. Its
    /// explicit private scratch and resource bounds must come from that root
    /// configuration, never from a candidate/live-helper configuration.
    pub fn preflight_development_release_from_root(
        path: impl AsRef<Path>,
        config: &ConsensusConfig,
        genesis_bytes: &[u8],
        consensus_source: &[u8],
        authorization: &crate::root_genesis::DevelopmentRootGenesis,
    ) -> Result<VerifiedReleaseAuthority> {
        config.validate()?;
        ensure!(config.emergency.as_ref().context("Emergency policy required")?.release_sha512
            == authorization.release_manifest_sha512,
            "Emergency release differs from root-authorized release manifest");
        source_binding(config, genesis_bytes)?;
        ensure!(!consensus_source.is_empty() && consensus_source.len() <= 65_536,
            "Exact consensus source exceeds supported bound");
        ensure!(serde_json::from_slice::<ConsensusConfig>(consensus_source)? == *config,
            "Exact consensus source differs from selected runtime configuration");
        let verifier = bootstrap_history_verifier(authorization)?;
        let prepared = authorization.prepare(&config.chain_id, genesis_bytes, consensus_source)?;
        preflight_prepared_release(path.as_ref(), config, genesis_bytes, &prepared, &verifier)
    }
    fn open_inner(
        path: impl AsRef<Path>,
        config: ConsensusConfig,
        genesis_bytes: Vec<u8>,
        root_genesis: Option<crate::root_genesis::PreparedRootGenesis>,
        mut emergency_verifier: Option<EmergencyVerifier>,
        candidate: Option<PreparedRuntimeCandidateInput>,
    ) -> Result<Self> {
        config.validate()?;
        source_binding(&config, &genesis_bytes)?;
        ensure!(
            config.emergency.is_some() == emergency_verifier.is_some(),
            "Emergency policy requires its explicit local verifier"
        );
        ensure!(
            config.emergency.is_none() || root_genesis.is_some(),
            "Emergency policy requires root-authorized genesis"
        );
        ensure!(
            config.release_handover.is_none() || candidate.is_some(),
            "Handover requires actual runtime candidate verification"
        );
        // Candidate authority is established before a writable RocksDB handle
        // exists. Read-only preflight cannot exclude another process's writer.
        // The native owner holds its lifecycle lease across this transition;
        // this application also repeats all checks after the writable open.
        if let Some(input) = &candidate {
            let authority = preflight_prepared_release(
                path.as_ref(), &config, &genesis_bytes,
                root_genesis.as_ref().context("Candidate requires verified root genesis")?,
                emergency_verifier.as_ref().context("Candidate requires emergency verifier")?,
            )?;
            crate::runtime_candidate_v2::verify_runtime_candidate(
                &input.input, &authority, &input.root, &input.verifier,
            )?;
            if matches!(&input.input, crate::runtime_candidate_v2::RuntimeCandidateInput::V2(_)) {
                // Catalog, configured helper binding and current application
                // identity have passed. Only now may the live verifier exist.
                emergency_verifier = Some(EmergencyVerifier::new(input.verifier.clone())?);
            }
        }
        let storage = Arc::new(Storage::open(path.as_ref().to_path_buf())?);
        let initialized = check_stored_startup(&storage, &config, &genesis_bytes,
            root_genesis.as_ref(), emergency_verifier.as_ref())?;
        let runtime_candidate = candidate
            .map(|input| {
                let expected_release = if initialized {
                    handover_state(&storage, &config)?.map(|s| s.active_release_sha512().to_owned())
                } else {
                    None
                }
                .or_else(|| config.emergency.as_ref().map(|p| p.release_sha512.clone()))
                .context("Candidate requires root-authorized release")?;
                let authority = VerifiedReleaseAuthority {
                    expected: crate::runtime_candidate::ExpectedCandidate {
                        manifest_sha512: expected_release,
                        chain_id: config.chain_id.clone(),
                        app_genesis_sha256: config.app_state_sha256.clone(),
                        migration_registry_sha256: upgrade::registry_sha256(),
                    },
                    committed_info: if initialized { Some(current_info(&storage)?) } else { None },
                    active_schema: if initialized {
                        upgrade_state(&storage, &config)?.map(|state| state.active_schema())
                    } else { None },
                };
                crate::runtime_candidate_v2::verify_runtime_candidate(
                    &input.input, &authority, &input.root, &input.verifier,
                )
            })
            .transpose()?;
        Ok(Self {
            storage,
            config,
            genesis_bytes,
            root_genesis,
            emergency_verifier,
            runtime_candidate,
            pending: None,
            admission: Mutex::new(crate::ordinary_admission::OrdinaryAdmissionQueue::default()),
        })
    }
    pub fn info(&self) -> Result<Info> {
        let _guard = self.storage.lock_execution()?;
        if self.storage.db.get(MODE_KEY)?.is_some() {
            verify_recovery(&self.storage)?;
        }
        current_info(&self.storage)
    }
    pub fn init_chain(
        &mut self,
        chain_id: &str,
        initial_height: u64,
        app_bytes: &[u8],
        validators: &[ValidatorConfig],
    ) -> Result<Info> {
        ensure!(
            self.pending.is_none(),
            "Cannot initialize with a pending block"
        );
        ensure!(
            chain_id == self.config.chain_id
                && initial_height == 1
                && app_bytes == self.genesis_bytes,
            "Engine genesis binding differs"
        );
        let as_map = |values: &[ValidatorConfig]| -> BTreeMap<String, ValidatorConfig> {
            values
                .iter()
                .map(|v| (v.pubkey_base64.clone(), v.clone()))
                .collect()
        };
        ensure!(
            validators.len() == self.config.validators.len()
                && as_map(validators) == as_map(&self.config.validators),
            "Engine validator set differs"
        );
        let config_bytes = serde_json::to_vec(&self.config)?;
        crate::genesis::initialize_consensus_with_root(
            Arc::get_mut(&mut self.storage).context("Initialization storage is shared")?,
            chain_id,
            app_bytes,
            &config_bytes,
            self.root_genesis.as_ref(),
        )?;
        self.info()
    }
    pub fn check_tx(&self, raw: &[u8]) -> TxResult {
        self.check_tx_result(raw).unwrap_or_else(|error| TxResult {
            code: 2,
            gas_wanted: 0,
            gas_used: 0,
            log: format!("Control verifier infrastructure failure: {error}"),
        })
    }
    pub fn check_tx_result(&self, raw: &[u8]) -> Result<TxResult> {
        self.ensure_active_runtime()?;
        let checked = (|| -> Result<Option<TxResult>> {
            let _guard = self.storage.lock_execution()?;
            ensure!(
                self.storage.db.get(MODE_KEY)?.is_some(),
                "InitChain required"
            );
            if self.config.emergency.is_some() {
                verify_recovery(&self.storage)?;
                let item = wire(&self.config, raw)?;
                if matches!(item, WireTransaction::EmergencyControl { .. }) {
                    let height = current_info(&self.storage)?
                        .height
                        .checked_add(1)
                        .context("Height exhausted")?;
                    self.emergency_plan(height, &[raw.to_vec()])?;
                    return Ok(Some(emergency_result()));
                }
                if matches!(item, WireTransaction::HandoverControl { .. }) {
                    let height = current_info(&self.storage)?
                        .height
                        .checked_add(1)
                        .context("Height exhausted")?;
                    let state = handover_state(&self.storage, &self.config)?
                        .context("Handover state missing")?;
                    let emergency = emergency_state(&self.storage, &self.config)?
                        .context("Handover emergency state missing")?;
                    let context = handover_context(
                        height,
                        current_info(&self.storage)?.app_hash,
                        upgrade_state(&self.storage, &self.config)?
                            .context("Upgrade state missing")?
                            .active_schema(),
                        &emergency,
                        false,
                    )?;
                    handover::check_admission(
                        self.config
                            .release_handover
                            .as_ref()
                            .context("Handover policy missing")?,
                        &state,
                        &context,
                        raw,
                        self.emergency_verifier
                            .as_ref()
                            .context("Handover verifier missing")?,
                    )?;
                    return Ok(Some(handover_result()));
                }
                if matches!(item, WireTransaction::UpgradeControl { .. }) {
                    ensure!(
                        !emergency_state(&self.storage, &self.config)?
                            .context("Emergency state missing")?
                            .frozen(),
                        EMERGENCY_FROZEN
                    );
                    let height = current_info(&self.storage)?
                        .height
                        .checked_add(1)
                        .context("Height exhausted")?;
                    let plan = self
                        .upgrade_plan(height, &[raw.to_vec()], None)?
                        .context("Upgrade policy missing")?;
                    ensure!(plan.receipt.is_some(), "Upgrade action not admitted");
                    return Ok(Some(upgrade_result()));
                }
                ensure!(
                    !emergency_state(&self.storage, &self.config)?
                        .context("Emergency state missing")?
                        .frozen()
                        || matches!(item, WireTransaction::EpochObservation { .. }),
                    EMERGENCY_FROZEN
                );
            }
            if self.config.ordinary.is_some() {
                verify_recovery(&self.storage)?;
                let book =
                    recovery_book(&self.storage, &self.config)?.context("Recovery missing")?;
                let mut state = load_ordinary(&self.storage, &self.config, Some(&book))?
                    .context("Ordinary missing")?;
                let height = book
                    .last_height
                    .checked_add(1)
                    .context("Height exhausted")?;
                let mut recovery = book.begin_block(height)?;
                let mut shared = shared_meter(&state.config, &recovery)?;
                let mut staged = committed_ordinary_settlement(self.storage.clone())?;
                let liquidity = ordinary_runtime::eligible_liquidity(&mut staged, &recovery.book)?;
                let head = current_info(&self.storage)?.app_hash;
                self.admission
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Admission queue lock poisoned"))?
                    .reset(&head, &state.config)?;
                if let Ok(WireTransaction::OrdinaryV2 { envelope_base64 }) = wire(&self.config, raw)
                {
                    if let Ok(signed) = ordinary_signed(&state.config, &envelope_base64) {
                        let result = ordinary_runtime::admission_only(
                            &signed,
                            &state.config.fee_profile,
                            &mut recovery.book,
                            &mut state.grants,
                            &mut state.history,
                            &mut staged,
                            height,
                            0,
                            runtime_limits(&state.config),
                            &mut shared,
                        )?;
                        let mut output = TxResult::from_ordinary(&result)?;
                        if result.accepted {
                            let request = result
                                .reservation
                                .as_ref()
                                .context("Accepted admission lacks reservation")?;
                            match self
                                .admission
                                .lock()
                                .map_err(|_| anyhow::anyhow!("Admission queue lock poisoned"))?
                                .reserve(request, &liquidity)
                            {
                                Ok(_) => output.code = 0,
                                Err(error) => {
                                    output.code = 1;
                                    output.log = format!("Admission reservation rejected: {error}");
                                }
                            }
                        }
                        return Ok(Some(output));
                    }
                }
                if !matches!(
                    wire(&self.config, raw),
                    Ok(WireTransaction::EpochObservation { .. })
                ) {
                    let (mut result, reservation) = combined_transaction(
                        &self.config,
                        raw,
                        height,
                        0,
                        &mut state,
                        &mut recovery,
                        &mut staged,
                        &mut shared,
                    )?;
                    if let Some(request) = reservation.as_ref() {
                        match self
                            .admission
                            .lock()
                            .map_err(|_| anyhow::anyhow!("Admission queue lock poisoned"))?
                            .reserve(request, &liquidity)
                        {
                            Ok(_) => result.code = 0,
                            Err(error) => {
                                result.code = 1;
                                result.log = format!("Admission reservation rejected: {error}");
                            }
                        }
                    }
                    return Ok(Some(result));
                }
            }
            match wire(&self.config, raw)? {
                WireTransaction::EmergencyControl { .. }
                | WireTransaction::UpgradeControl { .. }
                | WireTransaction::HandoverControl { .. } => {
                    anyhow::bail!("Control profile missing")
                }
                WireTransaction::OrdinaryV2 { .. } => anyhow::bail!("Ordinary profile missing"),
                WireTransaction::OrdinaryV3 { .. } => anyhow::bail!("Governance profile missing"),
                WireTransaction::Recovery { envelope_base64 } => {
                    verify_recovery(&self.storage)?;
                    let book = recovery_book(&self.storage, &self.config)?
                        .context("Recovery state missing")?;
                    let height = book
                        .last_height
                        .checked_add(1)
                        .context("Height exhausted")?;
                    let mut block = book.begin_block(height)?;
                    let mut staged = Settlement::new(self.storage.clone());
                    let result = block.execute(
                        height,
                        0,
                        &recovery_bytes(&envelope_base64)?,
                        &mut staged,
                    )?;
                    let mut admission = TxResult::from_recovery(&result)?;
                    if result.charged {
                        // Admission uses temporary state, including predicted out-of-gas.
                        admission.code = 0;
                    }
                    return Ok(Some(admission));
                }
                WireTransaction::Signed { envelope } => {
                    let record = normalize_record(&self.config, envelope)?;
                    ensure!(
                        !self.has_committed_receipt(&record.transaction.hash)?,
                        "Already committed transaction"
                    );
                }
                WireTransaction::EpochObservation { observation } => {
                    verify_recovery(&self.storage)?;
                    let state = timing(&self.storage)?;
                    let parent = read_head(&self.storage)?
                        .map_or_else(|| "genesis".into(), |h| h.anchor.engine_hash);
                    crate::runtime::issuance_timing::plan_block(
                        &self.storage,
                        &state,
                        state
                            .last_height
                            .checked_add(1)
                            .context("Height exhausted")?,
                        &parent,
                        Some(&observation),
                    )?;
                }
            }
            Ok(None)
        })();
        match checked {
            Ok(Some(result)) => Ok(result),
            Ok(None) => Ok(TxResult::observation()),
            Err(error) if crate::emergency_verifier::is_infrastructure_error(&error) => Err(error),
            Err(error) => Ok(TxResult::invalid(&format!(
                "Invalid transaction or observation: {error}"
            ))),
        }
    }
    /// Trusted local queue maintenance. This method changes no committed state.
    /// No unauthenticated transport route exposes this operation.
    pub fn evict_ordinary_admission(&self, id: &str, recovery_sponsorship: bool) -> Result<bool> {
        valid_hash(id)?;
        let bytes: [u8; 32] = hex::decode(id)?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid admission ID"))?;
        let id = if recovery_sponsorship {
            ReservationId::RecoverySponsorship(bytes)
        } else {
            ReservationId::Ordinary(bytes)
        };
        let _guard = self.storage.lock_execution()?;
        verify_recovery(&self.storage)?;
        let config = self
            .config
            .ordinary
            .as_ref()
            .context("Ordinary profile missing")?;
        let head = current_info(&self.storage)?.app_hash;
        let mut queue = self
            .admission
            .lock()
            .map_err(|_| anyhow::anyhow!("Admission queue lock poisoned"))?;
        queue.reset(&head, config)?;
        Ok(queue.evict(id)?)
    }
    /// Trusted engine recheck explicitly releases the old intent reservation and
    /// performs current admission again. Ordinary CheckTx never releases entries.
    pub fn recheck_ordinary_admission(&self, raw: &[u8]) -> TxResult {
        self.recheck_ordinary_admission_result(raw)
            .unwrap_or_else(|error| TxResult {
                code: 2,
                gas_wanted: 0,
                gas_used: 0,
                log: format!("Control verifier infrastructure failure: {error}"),
            })
    }
    pub fn recheck_ordinary_admission_result(&self, raw: &[u8]) -> Result<TxResult> {
        self.ensure_active_runtime()?;
        if self.config.emergency.is_some() {
            match emergency_state(&self.storage, &self.config) {
                Ok(Some(state)) if state.frozen() => return self.check_tx_result(raw),
                Err(_) => return Ok(TxResult::invalid("Emergency admission state invalid")),
                _ => {}
            }
        }
        if matches!(
            wire(&self.config, raw),
            Ok(WireTransaction::EpochObservation { .. }
                | WireTransaction::EmergencyControl { .. }
                | WireTransaction::UpgradeControl { .. }
                | WireTransaction::HandoverControl { .. })
        ) {
            return self.check_tx_result(raw);
        }
        let id = (|| -> Result<(String, bool)> {
            match wire(&self.config, raw)? {
                WireTransaction::OrdinaryV2 { envelope_base64 } => {
                    let config = self
                        .config
                        .ordinary
                        .as_ref()
                        .context("Ordinary profile missing")?;
                    let signed = ordinary_signed(config, &envelope_base64)?;
                    Ok((
                        hex::encode(ordinary_wire::transaction_id(
                            &signed.body,
                            &config.fee_profile.limits,
                        )?),
                        false,
                    ))
                }
                WireTransaction::Recovery { envelope_base64 } => {
                    let signed = sponsor_wire::decode(&recovery_bytes(&envelope_base64)?)?;
                    Ok((
                        hex::encode(sponsor_wire::authorization_id(&signed.sponsor)?),
                        true,
                    ))
                }
                _ => anyhow::bail!("Recheck requires ordinary or recovery intent"),
            }
        })();
        match id {
            Ok((id, sponsor)) => {
                if self.evict_ordinary_admission(&id, sponsor).is_err() {
                    return Ok(TxResult::invalid("Admission recheck state invalid"));
                }
                self.check_tx_result(raw)
            }
            Err(_) => Ok(TxResult::invalid("Malformed admission recheck")),
        }
    }
    pub fn ordinary_admission_count(&self) -> Result<usize> {
        let _guard = self.storage.lock_execution()?;
        verify_recovery(&self.storage)?;
        let config = self
            .config
            .ordinary
            .as_ref()
            .context("Ordinary profile missing")?;
        let head = current_info(&self.storage)?.app_hash;
        let mut queue = self
            .admission
            .lock()
            .map_err(|_| anyhow::anyhow!("Admission queue lock poisoned"))?;
        queue.reset(&head, config)?;
        Ok(queue.len())
    }
    // Call with the execution lock held. Commit publishes this key atomically.
    // Any stored value blocks admission, including a malformed receipt.
    fn has_committed_receipt(&self, hash: &str) -> Result<bool> {
        Ok(self
            .storage
            .db
            .get(format!("execution:v1:receipt:{hash}"))?
            .is_some())
    }
    pub fn prepare_proposal(
        &self,
        height: u64,
        time_seconds: i64,
        time_nanos: i32,
        txs: Vec<Vec<u8>>,
        max_bytes: u64,
    ) -> Result<Vec<Vec<u8>>> {
        self.prepare_proposal_with_evidence(
            height,
            time_seconds,
            time_nanos,
            txs,
            max_bytes,
            Vec::new(),
        )
    }
    pub fn prepare_proposal_with_evidence(
        &self,
        height: u64,
        time_seconds: i64,
        time_nanos: i32,
        txs: Vec<Vec<u8>>,
        max_bytes: u64,
        misbehavior: Vec<EvidenceFact>,
    ) -> Result<Vec<Vec<u8>>> {
        let _guard = self.storage.lock_execution()?;
        verify_recovery(&self.storage)?;
        let mut txs = txs;
        if self.config.emergency.is_some() {
            self.ensure_active_runtime()?;
            self.validate_evidence(height, &misbehavior)?;
            self.emergency_context(height)?;
            ensure!(
                time_seconds >= 0 && (0..1_000_000_000).contains(&time_nanos),
                "Invalid proposal timestamp"
            );
            if let Some(head) = read_head(&self.storage)? {
                ensure!(
                    (time_seconds, time_nanos)
                        >= (head.anchor.time_seconds, head.anchor.time_nanos),
                    "Proposal timestamp regressed"
                );
            }
            if let Some(selected) = self.prepare_emergency_proposal(height, &mut txs, max_bytes)? {
                return Ok(selected);
            }
        }
        if self.config.ordinary.is_some() {
            return self.prepare_combined_proposal(
                height,
                time_seconds,
                time_nanos,
                txs,
                max_bytes,
                misbehavior,
            );
        }
        self.validate_evidence(height, &misbehavior)?;
        let state = timing(&self.storage)?;
        ensure!(
            state.last_height.checked_add(1) == Some(height),
            "Proposal height differs"
        );
        ensure!(
            time_seconds >= 0 && (0..1_000_000_000).contains(&time_nanos),
            "Invalid proposal timestamp"
        );
        let boundary = height > 1 && (height - 1) % state.config.epoch_blocks == 0;
        let parent =
            read_head(&self.storage)?.map_or_else(|| "genesis".into(), |h| h.anchor.engine_hash);
        let limit = usize::try_from(max_bytes)
            .unwrap_or(usize::MAX)
            .min(self.config.max_block_bytes);
        let mut observation = None;
        let mut signed = Vec::new();
        let mut seen = BTreeSet::new();
        for raw in txs.into_iter().take(self.config.max_txs.saturating_mul(4)) {
            match wire(&self.config, &raw) {
                Ok(WireTransaction::EpochObservation { observation: obs })
                    if boundary && observation.is_none() =>
                {
                    if crate::runtime::issuance_timing::plan_block(
                        &self.storage,
                        &state,
                        height,
                        &parent,
                        Some(&obs),
                    )
                    .is_ok()
                    {
                        observation = Some(raw);
                    }
                }
                Ok(WireTransaction::Recovery { envelope_base64 }) => {
                    if let Ok(bytes) = recovery_bytes(&envelope_base64) {
                        if let Ok(envelope) = sponsor_wire::decode(&bytes) {
                            let id =
                                hex::encode(sponsor_wire::authorization_id(&envelope.sponsor)?);
                            if seen.insert(id) {
                                signed.push(raw);
                            }
                        }
                    }
                }
                Ok(WireTransaction::Signed { envelope }) => {
                    if let Ok(record) = normalize_record(&self.config, envelope) {
                        if !self.has_committed_receipt(&record.transaction.hash)?
                            && seen.insert(record.transaction.hash.clone())
                        {
                            signed.push(raw);
                        }
                    }
                }
                _ => {}
            }
        }
        if boundary && observation.is_none() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        let mut size = 0usize;
        if let Some(raw) = observation {
            if raw.len() > limit {
                return Ok(Vec::new());
            }
            size += raw.len();
            out.push(raw);
        }
        let proposal_recovery = recovery_book(&self.storage, &self.config)?;
        let mut reservations = crate::recovery_fees::RecoveryReservations::default();
        let mut reservation_state = Settlement::new(self.storage.clone());
        let mut proposal_meter = proposal_recovery
            .as_ref()
            .map(|book| book.begin_block(height))
            .transpose()?;
        let mut proposal_state = Settlement::new(self.storage.clone());
        for raw in signed {
            if out.len() >= self.config.max_txs {
                break;
            }
            if raw.len() <= limit - size {
                if let Some(book) = &proposal_recovery {
                    let keep = (|| -> Result<bool> {
                        let WireTransaction::Recovery { envelope_base64 } =
                            wire(&self.config, &raw)?
                        else {
                            return Ok(false);
                        };
                        let signed = sponsor_wire::decode(&recovery_bytes(&envelope_base64)?)?;
                        let auth = &signed.sponsor;
                        let account = book
                            .accounts
                            .get(&hex::encode(auth.sponsor_account_id))
                            .context("Unknown sponsor")?;
                        ensure!(
                            account.recovery.outgoing_allowed()
                                && account.recovery.active_key == auth.sponsor_key
                                && account.recovery.active_generation == auth.sponsor_generation
                                && account.sponsor_nonce == auth.sponsor_nonce
                                && height < auth.expiry_height,
                            "Stale sponsor authority"
                        );
                        ensure!(
                            auth.fee_profile_digest == sponsor_wire::profile_digest(&book.profile)?
                                && auth.fee_profile_version == book.profile.version
                                && auth.maximum_charge <= book.profile.max_fee_cap,
                            "Invalid sponsor profile"
                        );
                        let liquid = reservation_state
                            .account(&account.address)?
                            .balance_of("udrt");
                        let mut candidate = reservations.clone();
                        let reserved = candidate.reserve_sponsor(
                            sponsor_wire::authorization_id(auth)?,
                            &account.address,
                            auth.sponsor_nonce,
                            auth.maximum_charge,
                            liquid,
                        )?;
                        Ok(reserved)
                    })();
                    if !matches!(keep, Ok(true)) {
                        continue;
                    }
                    let WireTransaction::Recovery { envelope_base64 } = wire(&self.config, &raw)?
                    else {
                        unreachable!("Recovery profile rejects ordinary transactions");
                    };
                    let bytes = recovery_bytes(&envelope_base64)?;
                    // Every attempted verification uses one bounded proposal overlay.
                    // Rejected candidates retain their measured work in this pass.
                    let result = proposal_meter
                        .as_mut()
                        .context("Proposal meter missing")?
                        .execute(
                            height,
                            u32::try_from(out.len())?,
                            &bytes,
                            &mut proposal_state,
                        )?;
                    if !result.charged {
                        continue;
                    }
                    let signed = sponsor_wire::decode(&bytes)?;
                    let auth = &signed.sponsor;
                    let account = book
                        .accounts
                        .get(&hex::encode(auth.sponsor_account_id))
                        .context("Unknown sponsor")?;
                    let liquid = reservation_state
                        .account(&account.address)?
                        .balance_of("udrt");
                    ensure!(
                        reservations.reserve_sponsor(
                            sponsor_wire::authorization_id(auth)?,
                            &account.address,
                            auth.sponsor_nonce,
                            auth.maximum_charge,
                            liquid,
                        )?,
                        "Validated proposal reservation changed"
                    );
                }
                size += raw.len();
                out.push(raw);
            }
        }
        if self.config.penalty.is_some() || self.config.recovery.is_some() {
            let hash = digest(
                b"dytallix-penalty-proposal-check-v1",
                &(height, time_seconds, time_nanos, &out, &misbehavior),
            )?;
            self.prepare(FinalizedBlockInput {
                height,
                time_seconds,
                time_nanos,
                hash,
                txs: out.clone(),
                misbehavior,
            })?;
        }
        Ok(out)
    }
    // Caller holds the execution lock. Candidate effects and reservations are
    // temporary; rejected candidate work remains in the shared block meter.
    fn prepare_combined_proposal(
        &self,
        height: u64,
        time_seconds: i64,
        time_nanos: i32,
        txs: Vec<Vec<u8>>,
        max_bytes: u64,
        misbehavior: Vec<EvidenceFact>,
    ) -> Result<Vec<Vec<u8>>> {
        let timing = timing(&self.storage)?;
        ensure!(
            timing.last_height.checked_add(1) == Some(height),
            "Proposal height differs"
        );
        if let Some(candidate) = &self.config.governance {
            let verified = committed_governance_parent(
                self.storage.clone(),
                &self.config,
                candidate.genesis_digest,
            )?;
            ensure!(
                verified.height.checked_add(1) == Some(height),
                "Governance proposal parent height differs"
            );
        }
        ensure!(
            time_seconds >= 0 && (0..1_000_000_000).contains(&time_nanos),
            "Invalid proposal timestamp"
        );
        let head = read_head(&self.storage)?;
        if let Some(head) = &head {
            ensure!(
                (time_seconds, time_nanos) >= (head.anchor.time_seconds, head.anchor.time_nanos),
                "Proposal timestamp regressed"
            );
        }
        let parent = head
            .as_ref()
            .map_or_else(|| "genesis".into(), |h| h.anchor.engine_hash.clone());
        let parent_time = head.as_ref().map_or((0, 0), |h| {
            (h.anchor.time_seconds as u64, h.anchor.time_nanos)
        });
        let historical_times = self.validate_evidence(height, &misbehavior)?;
        let boundary = height > 1 && (height - 1) % timing.config.epoch_blocks == 0;
        let limit = usize::try_from(max_bytes)
            .unwrap_or(usize::MAX)
            .min(self.config.max_block_bytes);
        let mut candidates = Vec::new();
        let mut observation = None;
        for raw in txs.into_iter().take(self.config.max_txs.saturating_mul(4)) {
            if let Ok(WireTransaction::EpochObservation { observation: obs }) =
                wire(&self.config, &raw)
            {
                if boundary
                    && observation.is_none()
                    && crate::runtime::issuance_timing::plan_block(
                        &self.storage,
                        &timing,
                        height,
                        &parent,
                        Some(&obs),
                    )
                    .is_ok()
                {
                    observation = Some((raw, obs));
                }
            } else {
                candidates.push(raw);
            }
        }
        if boundary && observation.is_none() {
            return Ok(vec![]);
        }
        let mut out = Vec::new();
        let mut size = 0usize;
        if let Some((raw, _)) = &observation {
            if raw.len() > limit {
                return Ok(vec![]);
            }
            size = raw.len();
            out.push(raw.clone());
        }
        let engine = EmissionEngine::new(
            self.storage.clone(),
            Arc::new(Mutex::new(State::new(self.storage.clone()))),
        );
        let (lifecycle, _, _) = block_lifecycle::prepare_adaptive_interval(
            &engine,
            height,
            u64::try_from(time_seconds)?,
            &parent,
            observation.as_ref().map(|(_, obs)| obs),
        )?;
        let mut staged = Settlement::new(self.storage.clone());
        staged.attach_reward_lifecycle(lifecycle, u64::try_from(time_seconds)?)?;
        for (fact, historical_time) in misbehavior.iter().zip(historical_times) {
            staged.apply_validator_evidence(fact, historical_time, height - 1, parent_time)?;
        }
        if self.config.penalty.is_some() {
            staged.finalize_validator_evidence()?;
        }
        let book = recovery_book(&self.storage, &self.config)?.context("Recovery missing")?;
        let mut state =
            load_ordinary(&self.storage, &self.config, Some(&book))?.context("Ordinary missing")?;
        let mut recovery = book.begin_block(height)?;
        let mut shared = shared_meter(&state.config, &recovery)?;
        let liquidity = ordinary_runtime::eligible_liquidity(&mut staged, &recovery.book)?;
        let mut reservations = ReservationLedger::new(
            ordinary_fee_wire::profile_digest(&state.config.fee_profile)?,
            state.config.queue_limits()?,
        )?;
        for raw in candidates {
            if out.len() >= self.config.max_txs {
                break;
            }
            if size.checked_add(raw.len()).is_none_or(|v| v > limit) {
                continue;
            }
            let prior_staged = staged.clone();
            let prior_book = recovery.book.clone();
            let prior_state = state.clone();
            let (result, request) = combined_transaction(
                &self.config,
                &raw,
                height,
                u32::try_from(out.len())?,
                &mut state,
                &mut recovery,
                &mut staged,
                &mut shared,
            )?;
            let selected = if result.code == 0 || result.code == 3 {
                match request.as_ref() {
                    Some(request) => reservations.reserve(request, &liquidity).is_ok(),
                    None => false,
                }
            } else {
                false
            };
            if selected {
                size += raw.len();
                out.push(raw);
            } else {
                staged = prior_staged;
                recovery.book = prior_book;
                state = prior_state;
            }
        }
        state.last_height = height;
        validate_ordinary_principals(
            &recovery.book,
            staged.rewards.as_ref().context("Reward state missing")?,
            staged.validators.as_ref(),
        )?;
        state.validate(
            self.config
                .lifecycle
                .as_ref()
                .context("Lifecycle missing")?,
            &recovery.book,
            &staged_nonces(&mut staged, &recovery.book)?,
        )?;
        // No second candidate verification here. ProcessProposal and FinalizeBlock
        // independently repeat the selected ordered block against committed state.
        Ok(out)
    }
    pub fn process_proposal(&self, input: FinalizedBlockInput) -> Result<bool> {
        self.ensure_active_runtime()?;
        // Invalid boundary data rejects the proposal. It does not create an observation.
        let _guard = self.storage.lock_execution()?;
        verify_recovery(&self.storage)?;
        match self.prepare(input) {
            Ok(_) => Ok(true),
            Err(error) if crate::emergency_verifier::is_infrastructure_error(&error) => Err(error),
            Err(_) => Ok(false),
        }
    }
    pub fn finalize_block(&mut self, input: FinalizedBlockInput) -> Result<FinalizeResult> {
        let _guard = self.storage.lock_execution()?;
        verify_recovery(&self.storage)?;
        if let Some(pending) = &self.pending {
            ensure!(
                pending.record.input == input,
                "Pending finalized input differs"
            );
            return Ok(pending.record.result.clone());
        }
        if let Some(head) = read_head(&self.storage)? {
            if input.height == head.anchor.height {
                let record: BlockRecord = decode(
                    &self
                        .storage
                        .db
                        .get(record_key(input.height))?
                        .context("Committed block missing")?,
                )?;
                ensure!(
                    record.input == input,
                    "Committed block replay input differs"
                );
                return Ok(record.result);
            }
        }
        let prepared = self.prepare(input)?;
        let result = prepared.record.result.clone();
        self.pending = Some(prepared);
        Ok(result)
    }
    fn prepare(&self, input: FinalizedBlockInput) -> Result<Prepared> {
        self.ensure_active_runtime()?;
        input_limits(&self.config, &input)?;
        let expected_info = current_info(&self.storage)?;
        ensure!(
            expected_info.height.checked_add(1) == Some(input.height),
            "Finalized height is not next"
        );
        if let Some(candidate) = &self.config.governance {
            let verified = committed_governance_parent(
                self.storage.clone(),
                &self.config,
                candidate.genesis_digest,
            )?;
            ensure!(
                verified.height.checked_add(1) == Some(input.height),
                "Governance block parent height differs"
            );
        }
        ensure!(
            self.storage.db.get(MODE_KEY)?.is_some(),
            "InitChain required"
        );
        let head = read_head(&self.storage)?;
        if let Some(head) = &head {
            ensure!(
                (input.time_seconds, input.time_nanos)
                    >= (head.anchor.time_seconds, head.anchor.time_nanos),
                "Engine timestamp regressed"
            );
            ensure!(
                input.hash != head.anchor.engine_hash,
                "Engine block hash repeated"
            );
        }
        let parent_time = head.as_ref().map_or((0, 0), |h| {
            (h.anchor.time_seconds as u64, h.anchor.time_nanos)
        });
        let historical_times = self.validate_evidence(input.height, &input.misbehavior)?;
        let parent = head.map_or_else(|| "genesis".into(), |h| h.anchor.engine_hash);
        let timing = timing(&self.storage)?;
        let boundary = input.height > 1 && (input.height - 1) % timing.config.epoch_blocks == 0;
        let mut decoded = Vec::with_capacity(input.txs.len());
        let mut observation = None;
        for (index, raw) in input.txs.iter().enumerate() {
            let item = wire(&self.config, raw);
            if let Ok(WireTransaction::EpochObservation { observation: obs }) = &item {
                ensure!(
                    boundary && index == 0 && observation.is_none(),
                    "Unexpected epoch observation position"
                );
                observation = Some(obs.clone());
            }
            decoded.push(item);
        }
        ensure!(
            boundary == observation.is_some(),
            "Completed parent epoch observation required at boundary only"
        );
        let emergency_plan = self.emergency_plan(input.height, &input.txs)?;
        let upgrade_plan = self.upgrade_plan(input.height, &input.txs, emergency_plan.as_ref())?;
        let handover_plan = self.handover_plan(
            input.height,
            &input.txs,
            emergency_plan.as_ref(),
            upgrade_plan.as_ref(),
        )?;
        // The adaptive planner reads only this handle's storage. Its explicit
        // TimingState controls every amount; legacy engine defaults are unused.
        let engine = EmissionEngine::new(
            self.storage.clone(),
            Arc::new(Mutex::new(State::new(self.storage.clone()))),
        );
        let (lifecycle, _, journal) = block_lifecycle::prepare_adaptive_interval(
            &engine,
            input.height,
            u64::try_from(input.time_seconds)?,
            &parent,
            observation.as_ref(),
        )?;
        let mut staged = Settlement::new(self.storage.clone());
        staged.attach_reward_lifecycle(lifecycle, u64::try_from(input.time_seconds)?)?;
        for (fact, historical_time) in input.misbehavior.iter().zip(historical_times) {
            staged.apply_validator_evidence(
                fact,
                historical_time,
                input.height - 1,
                parent_time,
            )?;
        }
        if self.config.penalty.is_some() {
            staged.finalize_validator_evidence()?;
        }
        let committed_recovery = recovery_book(&self.storage, &self.config)?;
        let mut ordinary = load_ordinary(&self.storage, &self.config, committed_recovery.as_ref())?;
        let mut recovery = committed_recovery
            .map(|book| book.begin_block(input.height))
            .transpose()?;
        let mut shared = ordinary
            .as_ref()
            .map(|state| {
                shared_meter(
                    &state.config,
                    recovery.as_ref().expect("validated recovery"),
                )
            })
            .transpose()?;
        let mut results = Vec::with_capacity(input.txs.len());
        let mut accepted = Vec::new();
        let mut seen = BTreeSet::new();
        for (index, item) in decoded.into_iter().enumerate() {
            if matches!(item, Ok(WireTransaction::EmergencyControl { .. })) {
                ensure!(
                    emergency_plan
                        .as_ref()
                        .is_some_and(|plan| plan.receipt.is_some()),
                    "Emergency control has no verified plan"
                );
                results.push(emergency_result());
                continue;
            }
            if emergency_plan
                .as_ref()
                .is_some_and(|plan| plan.reject_user_transactions)
                && !matches!(item, Ok(WireTransaction::EpochObservation { .. }))
            {
                results.push(TxResult::invalid(EMERGENCY_FROZEN));
                continue;
            }
            if matches!(item, Ok(WireTransaction::HandoverControl { .. })) {
                ensure!(
                    handover_plan.as_ref().is_some_and(|p| p.receipt.is_some()),
                    "Handover control lacks verified plan"
                );
                results.push(handover_result());
                continue;
            }
            if matches!(item, Ok(WireTransaction::UpgradeControl { .. })) {
                ensure!(
                    upgrade_plan
                        .as_ref()
                        .is_some_and(|plan| plan.receipt.is_some()),
                    "Upgrade control has no verified plan"
                );
                results.push(upgrade_result());
                continue;
            }
            if let Some(state) = ordinary.as_mut() {
                let (result, _) = combined_transaction(
                    &self.config,
                    &input.txs[index],
                    input.height,
                    u32::try_from(index)?,
                    state,
                    recovery.as_mut().context("Recovery missing")?,
                    &mut staged,
                    shared.as_mut().context("Shared meter missing")?,
                )?;
                results.push(result);
                continue;
            }
            let envelope = match item {
                Ok(
                    WireTransaction::EmergencyControl { .. }
                    | WireTransaction::UpgradeControl { .. }
                    | WireTransaction::HandoverControl { .. },
                ) => {
                    anyhow::bail!("Control escaped bounded path")
                }
                Ok(WireTransaction::OrdinaryV2 { .. }) => anyhow::bail!("Ordinary profile missing"),
                Ok(WireTransaction::OrdinaryV3 { .. }) => anyhow::bail!("Governance profile missing"),
                Ok(WireTransaction::EpochObservation { .. }) => {
                    results.push(TxResult::observation());
                    continue;
                }
                Ok(WireTransaction::Recovery { envelope_base64 }) => {
                    let block = recovery.as_mut().context("Recovery block missing")?;
                    // Malformed transport still consumes bounded block parsing work.
                    let bytes = recovery_bytes(&envelope_base64)
                        .unwrap_or_else(|_| input.txs[index].clone());
                    let result =
                        block.execute(input.height, u32::try_from(index)?, &bytes, &mut staged)?;
                    results.push(TxResult::from_recovery(&result)?);
                    continue;
                }
                Ok(WireTransaction::Signed { envelope }) => envelope,
                Err(_) => {
                    if let Some(block) = &mut recovery {
                        let result = block.execute(
                            input.height,
                            u32::try_from(index)?,
                            &input.txs[index],
                            &mut staged,
                        )?;
                        ensure!(
                            !result.charged,
                            "Malformed outer envelope accepted as recovery"
                        );
                        results.push(TxResult::from_recovery(&result)?);
                        continue;
                    }
                    results.push(TxResult::invalid("Malformed transaction"));
                    continue;
                }
            };
            let record = match normalize_record(&self.config, envelope) {
                Ok(record) => record,
                Err(_) => {
                    results.push(TxResult::invalid("Invalid signed transaction"));
                    continue;
                }
            };
            let tx = &record.transaction;
            if !seen.insert(tx.hash.clone())
                || self
                    .storage
                    .db
                    .get(format!("execution:v1:receipt:{}", tx.hash))?
                    .is_some()
            {
                results.push(TxResult::invalid(
                    "Duplicate or already committed transaction",
                ));
                continue;
            }
            let checkpoint = staged.clone();
            let execution = stage_transaction(
                tx,
                &mut staged,
                input.height,
                u32::try_from(index)?,
                &GasSchedule::default(),
            )?;
            let result = TxResult::from_receipt(&execution.result.receipt, execution.accepted)?;
            if execution.accepted {
                accepted.push(Accepted {
                    index: u32::try_from(index)?,
                    record,
                    receipt: execution.result.receipt,
                });
            } else {
                staged = checkpoint;
            }
            results.push(result);
        }
        if let Some(state) = ordinary.as_mut() {
            state.last_height = input.height;
            let book = &recovery.as_ref().context("Recovery missing")?.book;
            validate_ordinary_principals(
                book,
                staged.rewards.as_ref().context("Reward state missing")?,
                staged.validators.as_ref(),
            )?;
            state.validate(
                self.config
                    .lifecycle
                    .as_ref()
                    .context("Lifecycle missing")?,
                book,
                &staged_nonces(&mut staged, book)?,
            )?;
        }
        let mut writes = staged.writes()?;
        if let Some(plan) = emergency_plan {
            writes.insert(
                emergency::STATE_KEY.as_bytes().to_vec(),
                emergency::encode_state(&plan.state)?,
            );
            if let Some(receipt) = plan.receipt {
                let key = emergency::receipt_key(receipt.sequence());
                ensure!(
                    self.storage.db.get(&key)?.is_none(),
                    "Emergency receipt already exists"
                );
                writes.insert(key.into_bytes(), emergency::encode_receipt(&receipt)?);
                if upgrade_plan
                    .as_ref()
                    .is_some_and(|plan| plan.state.active_schema() == 1)
                {
                    let index = upgrade::index_key(&receipt.sha256()?)?;
                    ensure!(
                        self.storage.db.get(&index)?.is_none(),
                        "Emergency digest index already exists"
                    );
                    writes.insert(
                        index.into_bytes(),
                        receipt.sequence().to_be_bytes().to_vec(),
                    );
                }
            }
        }
        if let Some(plan) = handover_plan {
            writes.insert(
                handover::STATE_KEY.as_bytes().to_vec(),
                handover::encode_state(&plan.state)?,
            );
            if let Some(receipt) = plan.receipt {
                let key = handover::receipt_key(receipt.sequence());
                ensure!(
                    self.storage.db.get(&key)?.is_none(),
                    "Handover receipt already exists"
                );
                writes.insert(key.into_bytes(), handover::encode_receipt(&receipt)?);
            }
        }
        if let Some(plan) = upgrade_plan {
            writes.insert(
                upgrade::STATE_KEY.as_bytes().to_vec(),
                upgrade::encode_state(&plan.state)?,
            );
            if let Some(receipt) = plan.receipt {
                let key = upgrade::receipt_key(receipt.sequence());
                ensure!(
                    self.storage.db.get(&key)?.is_none(),
                    "Upgrade receipt already exists"
                );
                writes.insert(key.into_bytes(), upgrade::encode_receipt(&receipt)?);
            }
            if let Some(migration) = plan.migration {
                for (key, value) in migration.writes {
                    ensure!(
                        self.storage.db.get(&key)?.is_none() && !writes.contains_key(&key),
                        "Migration would overwrite existing state"
                    );
                    writes.insert(key, value);
                }
            }
        }
        if let Some(state) = ordinary.as_ref() {
            ordinary_state::append_writes(state, &mut writes)?;
        }
        if let Some(block) = recovery {
            writes.insert(RECOVERY_STATE_KEY.as_bytes().to_vec(), block.book.encode()?);
        }
        crate::supply::validate_native(&self.storage, &writes)?;
        let validator_updates = if self.config.lifecycle.is_some() {
            let lifecycle = LifecycleState::decode(
                writes
                    .get(LIFECYCLE_STATE_KEY.as_bytes())
                    .context("Lifecycle block state missing")?,
            )?;
            let updates = lifecycle.validator_updates(input.height)?;
            ensure!(
                updates == lifecycle.historical_validator_updates(input.height)?,
                "Prepared lifecycle update history differs"
            );
            updates
        } else {
            Vec::new()
        };
        let anchor = Anchor {
            version: 1,
            height: input.height,
            engine_hash: input.hash.clone(),
            parent_engine_hash: parent,
            time_seconds: input.time_seconds,
            time_nanos: input.time_nanos,
            input_digest: digest(b"dytallix-cometbft-input-v1", &input)?,
            result_digest: results_digest(&self.config, &results, &validator_updates)?,
            prior_app_hash: expected_info.app_hash.clone(),
        };
        let next_state_digest =
            state_digest(&self.storage, &writes, self.config.governance.is_some())?;
        let next_app_hash = app_hash(&next_state_digest, &anchor)?;
        let head = Head {
            anchor,
            state_digest: next_state_digest,
            app_hash: next_app_hash.clone(),
        };
        writes.insert(b"meta:height".to_vec(), input.height.to_be_bytes().to_vec());
        writes.insert(b"meta:best_hash".to_vec(), input.hash.as_bytes().to_vec());
        Ok(Prepared {
            expected_info,
            expected_state_digest: state_digest(
                &self.storage,
                &Writes::new(),
                self.config.governance.is_some(),
            )?,
            writes,
            journal,
            record: BlockRecord {
                input,
                result: FinalizeResult {
                    app_hash: next_app_hash,
                    tx_results: results,
                    validator_updates,
                },
                head,
                accepted,
            },
        })
    }
    pub fn commit(&mut self) -> Result<Info> {
        self.commit_with(|storage, batch| {
            let mut options = WriteOptions::default();
            options.set_sync(true);
            storage.db.write_opt(batch, &options)?;
            Ok(())
        })
    }
    pub(crate) fn commit_with(
        &mut self,
        write: impl FnOnce(&Storage, WriteBatch) -> Result<()>,
    ) -> Result<Info> {
        let guard = self.storage.lock_execution()?;
        verify_recovery(&self.storage)?;
        let Some(prepared) = &self.pending else {
            return current_info(&self.storage);
        };
        let current = current_info(&self.storage)?;
        let expected_next = Info {
            height: prepared.record.input.height,
            app_hash: prepared.record.result.app_hash.clone(),
        };
        if current == expected_next {
            let stored: BlockRecord = decode(
                &self
                    .storage
                    .db
                    .get(record_key(current.height))?
                    .context("Committed retry block missing")?,
            )?;
            ensure!(
                serde_json::to_vec(&stored)? == serde_json::to_vec(&prepared.record)?,
                "Committed retry record differs"
            );
            self.pending = None;
            return Ok(current);
        }
        ensure!(
            current == prepared.expected_info
                && state_digest(
                    &self.storage,
                    &Writes::new(),
                    self.config.governance.is_some()
                )? == prepared.expected_state_digest,
            "Prepared block predecessor changed"
        );
        let mut batch = WriteBatch::default();
        let adaptive: BTreeMap<_, _> = prepared
            .writes
            .iter()
            .filter(|(k, _)| k.starts_with(b"adaptive:"))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if let Some(journal) = &prepared.journal {
            ensure!(
                adaptive == journal.writes().iter().cloned().collect(),
                "Prepared controller writes differ"
            );
            journal.append_checked(&self.storage, &guard, &mut batch)?;
        } else {
            ensure!(adaptive.is_empty(), "Unexpected controller journal writes");
        }
        for (key, value) in &prepared.writes {
            if !key.starts_with(b"adaptive:") {
                batch.put(key, value);
            }
        }
        for item in &prepared.record.accepted {
            Settlement::append_verified_receipt(
                &self.storage,
                &mut batch,
                &item.record.transaction,
                &item.receipt,
                &item.record,
            )?;
        }
        batch.put(
            record_key(prepared.record.input.height),
            serde_json::to_vec(&prepared.record)?,
        );
        batch.put(HEAD_KEY, serde_json::to_vec(&prepared.record.head)?);
        write(&self.storage, batch)?;
        self.pending = None;
        Ok(expected_next)
    }
    // Caller holds the execution lock and has validated the committed state.
    // Context is reported RPC metadata; it is not a light-client proof.
    fn ordinary_client_context(
        &self,
    ) -> Result<dytallix_protocol_types::ordinary_client::CommittedContext> {
        let info = current_info(&self.storage)?;
        Ok(dytallix_protocol_types::ordinary_client::CommittedContext {
            chain_id: self.config.chain_id.clone(),
            genesis_digest: hex::decode(&self.config.app_state_sha256)?
                .try_into()
                .map_err(|_| anyhow::anyhow!("Invalid committed genesis digest"))?,
            height: info.height,
            app_hash: hex::decode(info.app_hash)?
                .try_into()
                .map_err(|_| anyhow::anyhow!("Invalid committed application hash"))?,
        })
    }
    /// Read only the committed digest index. Missing index state is never built
    /// by startup, this query or another read path.
    /// Return anchor and value from one fully validated, locked committed view.
    pub fn query_at(
        &self,
        request: QueryRequest<'_>,
        wanted_height: i64,
    ) -> Result<(Info, serde_json::Value)> {
        let _guard = self.storage.lock_execution()?;
        let history = HistoryRead::new(&self.storage);
        verify_recovery_with(&history)?;
        let info = current_info(&self.storage)?;
        ensure!(
            wanted_height == 0 || u64::try_from(wanted_height).ok() == Some(info.height),
            "Historical queries are not supported"
        );
        let value = match request {
            QueryRequest::Status => self.query_validated()?,
            QueryRequest::OrdinaryProfile => self.query_ordinary_profile_validated()?,
            QueryRequest::OrdinaryAccount(id) => self.query_ordinary_account_validated(id)?,
            QueryRequest::OrdinaryReceipt(id) => self.query_ordinary_receipt_validated(id)?,
            QueryRequest::EmergencyReceipt(id) => self.query_emergency_receipt_validated(id)?,
        };
        Ok((info, value))
    }
    pub fn query_emergency_receipt(&self, digest: &str) -> Result<serde_json::Value> {
        self.query_at(QueryRequest::EmergencyReceipt(digest), 0)
            .map(|(_, value)| value)
    }
    fn query_emergency_receipt_validated(&self, digest: &str) -> Result<serde_json::Value> {
        valid_hash(digest)?;
        let info = current_info(&self.storage)?;
        let context = serde_json::json!({"chain_id": self.config.chain_id, "genesis_sha256": self.config.app_state_sha256,
            "height": info.height, "app_hash": info.app_hash});
        if !upgrade_state(&self.storage, &self.config)?
            .is_some_and(|state| state.active_schema() == 1)
        {
            return Ok(
                serde_json::json!({"status": "index_unavailable", "receipt_sha256": digest, "height": info.height, "context": context}),
            );
        }
        let Some(index) = self.storage.db.get(upgrade::index_key(digest)?)? else {
            return Ok(
                serde_json::json!({"status": "receipt_absent", "receipt_sha256": digest, "height": info.height, "context": context}),
            );
        };
        let sequence = u64::from_be_bytes(
            index
                .as_slice()
                .try_into()
                .map_err(|_| anyhow::anyhow!("Emergency receipt index width differs"))?,
        );
        let policy = self
            .config
            .emergency
            .as_ref()
            .context("Indexed emergency policy missing")?;
        let raw = self
            .storage
            .db
            .get(emergency::receipt_key(sequence))?
            .context("Indexed emergency receipt missing")?;
        let receipt = emergency::decode_receipt(policy, &raw)?;
        ensure!(
            receipt.sequence() == sequence && receipt.sha256()? == digest,
            "Indexed emergency receipt binding differs"
        );
        Ok(
            serde_json::json!({"status": "committed_receipt_reported", "receipt_sha256": digest, "sequence": sequence,
            "height": info.height, "context": context, "receipt": receipt}),
        )
    }
    pub fn query_ordinary_profile(&self) -> Result<serde_json::Value> {
        self.query_at(QueryRequest::OrdinaryProfile, 0)
            .map(|(_, value)| value)
    }
    fn query_ordinary_profile_validated(&self) -> Result<serde_json::Value> {
        use dytallix_protocol_types::ordinary_client::{ProfileView, CLIENT_VIEW_VERSION};
        let book = recovery_book(&self.storage, &self.config)?;
        let state = load_ordinary(&self.storage, &self.config, book.as_ref())?;
        Ok(serde_json::to_value(ProfileView {
            version: CLIENT_VIEW_VERSION,
            enabled: state.is_some(),
            context: self.ordinary_client_context()?,
            config: state.map(|state| state.config.client_view()),
        })?)
    }
    pub fn query_ordinary_account(&self, id: &str) -> Result<serde_json::Value> {
        self.query_at(QueryRequest::OrdinaryAccount(id), 0)
            .map(|(_, value)| value)
    }
    fn query_ordinary_account_validated(&self, id: &str) -> Result<serde_json::Value> {
        use dytallix_protocol_types::ordinary_client::{
            AccountDomain, AccountView, CLIENT_VIEW_VERSION,
        };
        valid_hash(id)?;
        let book = recovery_book(&self.storage, &self.config)?;
        let state = load_ordinary(&self.storage, &self.config, book.as_ref())?;
        let Some(state) = state else {
            return Ok(serde_json::Value::Null);
        };
        let Some(account) = book.as_ref().and_then(|book| book.accounts.get(id)) else {
            return Ok(serde_json::Value::Null);
        };
        let recovery = &account.recovery;
        Ok(serde_json::to_value(AccountView {
            version: CLIENT_VIEW_VERSION,
            context: self.ordinary_client_context()?,
            domain: AccountDomain {
                network: recovery.domain.network,
                chain_id: recovery.domain.chain_id.clone(),
                genesis_digest: recovery.domain.genesis_digest,
                account_id: recovery.domain.account_id,
            },
            account_id: recovery.domain.account_id,
            address: account.address.clone(),
            current_key: recovery.active_key.clone(),
            authorization_generation: recovery.active_generation,
            spending_nonce: recovery.spending_nonce,
            protected: !recovery.outgoing_allowed(),
            profile_digest: ordinary_fee_wire::profile_digest(&state.config.fee_profile)?,
        })?)
    }
    pub fn query_ordinary_receipt(&self, id: &str) -> Result<serde_json::Value> {
        self.query_at(QueryRequest::OrdinaryReceipt(id), 0)
            .map(|(_, value)| value)
    }
    fn query_ordinary_receipt_validated(&self, id: &str) -> Result<serde_json::Value> {
        valid_hash(id)?;
        let id: [u8; 32] = hex::decode(id)?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid ordinary transaction ID"))?;
        let book = recovery_book(&self.storage, &self.config)?;
        let state = load_ordinary(&self.storage, &self.config, book.as_ref())?;
        match state.as_ref().and_then(|state| state.history.receipt(id)) {
            Some(receipt) => Ok(serde_json::to_value(
                receipt.client_view(self.ordinary_client_context()?),
            )?),
            None => Ok(serde_json::Value::Null),
        }
    }
    pub fn query(&self) -> Result<serde_json::Value> {
        self.query_at(QueryRequest::Status, 0)
            .map(|(_, value)| value)
    }
    fn query_validated(&self) -> Result<serde_json::Value> {
        let info = current_info(&self.storage)?;
        let supply = crate::supply::validate_native(&self.storage, &Writes::new())?;
        let engine_hash =
            read_head(&self.storage)?.map_or_else(|| "genesis".into(), |h| h.anchor.engine_hash);
        let mut response = serde_json::json!({"height": info.height, "app_hash": info.app_hash, "engine_hash": engine_hash,
            "issuance_timing": supply.drt.issuance_timing.clone(), "supply": supply.drt.response(), "dgt_supply": supply.dgt.response()});
        if let Some(lifecycle) = lifecycle_state(&self.storage)? {
            response["last_commit_validators"] = serde_json::to_value(if info.height > 1 {
                lifecycle.historical_validator_set(info.height - 1)?
            } else {
                Vec::new()
            })?;
            response["effective_validators"] =
                serde_json::to_value(lifecycle.validator_set(info.height.max(1))?)?;
            response["next_validators"] = serde_json::to_value(
                lifecycle.validator_set(info.height.checked_add(1).context("Height exhausted")?)?,
            )?;
            response["scheduled_validators"] = serde_json::to_value(
                lifecycle.validator_set(info.height.checked_add(2).context("Height exhausted")?)?,
            )?;
            response["validator_lifecycle"] = serde_json::to_value(lifecycle)?;
        }
        if let Some(state) = emergency_state(&self.storage, &self.config)? {
            response["emergency_control"] = serde_json::json!({
                "development_only": true,
                "frozen": state.frozen(),
                "blocks_upgrade": state.blocks_upgrade(),
                "next_sequence": state.next_sequence(),
                "last_receipt_sha256": state.last_receipt_sha256(),
                "automatic_transition_policy": "continue_existing"
            });
        }
        if let Some(state) = handover_state(&self.storage, &self.config)? {
            response["release_handover"] = serde_json::json!({
                "active_release_sha512":state.active_release_sha512(), "active_schema":state.active_schema(),
                "next_sequence":state.next_sequence(), "pending":state.pending(),
                "last_receipt_sha256":state.last_receipt_sha256(), "activation_receipt_sha256":state.activation_receipt_sha256()
            });
        }
        if let Some(state) = upgrade_state(&self.storage, &self.config)? {
            response["upgrade"] = serde_json::json!({
                "development_only": true,
                "active_schema": state.active_schema(),
                "active_release_sha512": state.active_release_sha512(),
                "next_sequence": state.next_sequence(),
                "pending": state.pending(),
                "last_receipt_sha256": state.last_receipt_sha256(),
                "activation_receipt_sha256": state.activation_receipt_sha256(),
                "migration_sha256": upgrade::migration_sha256()
            });
        }
        if let Some(penalties) = penalty_state(&self.storage)? {
            response["validator_penalties"] = serde_json::to_value(penalties)?;
        }
        Ok(response)
    }
}

/// Validate durable application state without entering the development journal.
pub fn verify_recovery(storage: &Storage) -> Result<()> {
    verify_recovery_with(&HistoryRead::new(storage)).map(|_| ())
}
fn verify_recovery_with(history: &HistoryRead<'_>) -> Result<EmergencyTrace> {
    #[cfg(test)]
    RECOVERY_PASSES.with(|n| n.set(n.get() + 1));
    let storage = history.storage;
    let raw = storage
        .db
        .get(MODE_KEY)?
        .context("Consensus mode is not initialized")?;
    let config: ConsensusConfig = decode(&raw)?;
    config.validate()?;
    let source = storage
        .db
        .get(GENESIS_SOURCE_KEY)?
        .context("Original consensus genesis source missing")?;
    source_binding(&config, &source)?;
    let mut source_marker = Sha256::new();
    source_marker.update(b"dytallix-selected-node-genesis-monetary-v1");
    source_marker.update((config.chain_id.len() as u64).to_be_bytes());
    source_marker.update(config.chain_id.as_bytes());
    source_marker.update([1]);
    source_marker.update(&source);
    let mut expected_marker = vec![1];
    expected_marker.extend_from_slice(&source_marker.finalize());
    ensure!(
        storage.db.get(b"genesis:monetary:v1")?.as_deref() == Some(expected_marker.as_slice()),
        "Monetary genesis marker differs from original source"
    );
    let initial_info = genesis_info(storage)?;
    ensure!(
        storage.get_chain_id().as_deref() == Some(&config.chain_id),
        "Stored consensus chain differs"
    );
    crate::supply::validate_native(storage, &Writes::new())?;
    check_reward_validators(storage, &config)?;
    let lifecycle = lifecycle_state(storage)?;
    let penalties = penalty_state(storage)?;
    let state = timing(storage)?;
    let current = read_head(storage)?;
    let height = block_lifecycle::height(storage, "meta:height")?;
    if let Some(governance_config) = &config.governance {
        let raw = storage
            .db
            .get(GOVERNANCE_STATE_KEY)?
            .context("Configured governance state missing")?;
        let governance = GovernanceState::decode(&raw)?;
        ensure!(
            governance.chain_id() == config.chain_id
                && governance.genesis_digest() == governance_config.genesis_digest
                && governance.finalized_height() == height,
            "Governance state differs from committed chain or height"
        );
    }
    let recovery = recovery_book(storage, &config)?;
    if let Some(book) = &recovery {
        ensure!(
            book.last_height == height,
            "Recovery height differs from committed head"
        );
    }
    let ordinary = load_ordinary(storage, &config, recovery.as_ref())?;
    let mut ordinary_positions = BTreeMap::new();
    if let Some(state) = &ordinary {
        ensure!(
            state.last_height == height,
            "Ordinary height differs from committed head"
        );
        for receipt in state.history.receipts() {
            ensure!(
                ordinary_positions
                    .insert((receipt.block_height(), receipt.block_index()), receipt)
                    .is_none(),
                "Duplicate ordinary receipt position"
            );
        }
    }
    let mut recovery_positions = BTreeMap::new();
    if let Some(book) = &recovery {
        for receipt in book.sponsor_receipts.values() {
            ensure!(
                recovery_positions
                    .insert((receipt.block_height, receipt.block_index), receipt)
                    .is_none(),
                "Duplicate recovery receipt position"
            );
        }
    }
    let mut known = BTreeSet::from([
        MODE_KEY.as_bytes().to_vec(),
        GENESIS_SOURCE_KEY.as_bytes().to_vec(),
        GENESIS_APP_HASH_KEY.as_bytes().to_vec(),
    ]);
    let mut transactions = BTreeSet::new();
    let mut recorded_evidence = BTreeMap::new();
    if let Some(head) = &current {
        ensure!(
            (1..=MAX_HISTORY).contains(&height)
                && head.anchor.height == height
                && state.last_height == height
                && block_lifecycle::height(storage, "emission:last_height")? == height,
            "Consensus height counters differ"
        );
        ensure!(
            storage.db.get("meta:best_hash")?.as_deref()
                == Some(head.anchor.engine_hash.as_bytes()),
            "Engine head metadata differs"
        );
        known.insert(HEAD_KEY.as_bytes().to_vec());
        let mut previous: Option<Head> = None;
        for h in 1..=height {
            let key = record_key(h);
            known.insert(key.as_bytes().to_vec());
            let record = history.block(h)?;
            input_limits(&config, &record.input)?;
            evidence_times(storage, &config, h, &record.input.misbehavior)?;
            for fact in &record.input.misbehavior {
                // ABCI omits vote rounds and full evidence hashes. Different
                // engine evidence can therefore produce the same fact.
                recorded_evidence
                    .entry(serde_json::to_vec(fact)?)
                    .or_insert(h);
            }
            let anchor = &record.head.anchor;
            ensure!(
                anchor.version == 1
                    && record.input.height == h
                    && anchor.height == h
                    && anchor.engine_hash == record.input.hash
                    && anchor.time_seconds == record.input.time_seconds
                    && anchor.time_nanos == record.input.time_nanos,
                "Consensus record metadata differs"
            );
            ensure!(
                anchor.input_digest == digest(b"dytallix-cometbft-input-v1", &record.input)?
                    && anchor.result_digest
                        == results_digest(
                            &config,
                            &record.result.tx_results,
                            &record.result.validator_updates
                        )?
                    && record.result.tx_results.len() == record.input.txs.len(),
                "Consensus input or result commitment differs"
            );
            if let Some(lifecycle) = &lifecycle {
                ensure!(
                    record.result.validator_updates == lifecycle.historical_validator_updates(h)?,
                    "Committed validator updates differ from lifecycle history"
                );
                let mut projected = validator_tuple_map(
                    &lifecycle.validator_set(h.checked_add(1).context("Height exhausted")?)?,
                );
                for update in &record.result.validator_updates {
                    let key = (update.pubkey_type.clone(), update.pubkey_base64.clone());
                    if update.power == 0 {
                        ensure!(
                            projected.remove(&key).is_some(),
                            "Validator removal has no predecessor"
                        );
                    } else {
                        ensure!(update.power > 0, "Negative validator power");
                        projected.insert(key, update.power);
                    }
                }
                ensure!(
                    projected
                        == validator_tuple_map(
                            &lifecycle
                                .validator_set(h.checked_add(2).context("Height exhausted")?)?
                        ),
                    "Validator update activation height differs"
                );
            }
            valid_hash(&record.head.state_digest)?;
            valid_hash(&anchor.prior_app_hash)?;
            ensure!(
                record.result.app_hash == record.head.app_hash
                    && record.head.app_hash == app_hash(&record.head.state_digest, anchor)?,
                "Consensus application hash differs"
            );
            if let Some(prior) = &previous {
                ensure!(
                    anchor.parent_engine_hash == prior.anchor.engine_hash
                        && anchor.prior_app_hash == prior.app_hash
                        && (anchor.time_seconds, anchor.time_nanos)
                            >= (prior.anchor.time_seconds, prior.anchor.time_nanos),
                    "Consensus parent or timestamp differs"
                );
            } else {
                ensure!(
                    anchor.parent_engine_hash == "genesis"
                        && anchor.prior_app_hash == initial_info.app_hash,
                    "Initial consensus parent or application commitment differs"
                );
            }
            let boundary = h > 1 && (h - 1) % state.config.epoch_blocks == 0;
            let mut observations = 0usize;
            for (index, raw) in record.input.txs.iter().enumerate() {
                if let Ok(WireTransaction::EpochObservation { observation }) = wire(&config, raw) {
                    ensure!(
                        boundary
                            && index == 0
                            && observation.parent_hash == anchor.parent_engine_hash,
                        "Committed epoch observation parent differs"
                    );
                    let encoded = storage
                        .db
                        .get(crate::runtime::issuance_timing::observation_key(
                            observation.epoch,
                        ))?
                        .context("Committed observation missing")?;
                    let stored = crate::runtime::issuance_timing::decode_observation(&encoded)?;
                    ensure!(
                        stored == observation
                            && record.result.tx_results[index] == TxResult::observation(),
                        "Committed observation differs"
                    );
                    observations += 1;
                }
            }
            ensure!(
                observations == usize::from(boundary),
                "Committed boundary observation count differs"
            );
            let mut indices = BTreeSet::new();
            for (index, raw) in record.input.txs.iter().enumerate() {
                if let Some(receipt) = recovery_positions.remove(&(h, u32::try_from(index)?)) {
                    let WireTransaction::Recovery { envelope_base64 } = wire(&config, raw)? else {
                        anyhow::bail!("Recovery receipt has no matching input");
                    };
                    let envelope = sponsor_wire::decode(&recovery_bytes(&envelope_base64)?)?;
                    dytallix_runtime_crypto::recovery_sponsor::verify_signed(&envelope)?;
                    ensure!(
                        receipt.envelope_hash == sponsor_wire::envelope_hash(&envelope)?
                            && receipt.operation_id == envelope.sponsor.operation_id
                            && receipt.sponsor_authorization_id
                                == sponsor_wire::authorization_id(&envelope.sponsor)?
                            && receipt.sponsor_account_id == envelope.sponsor.sponsor_account_id
                            && receipt.target_account_id == envelope.sponsor.domain.account_id
                            && receipt.gas_limit == envelope.sponsor.gas_limit
                            && receipt.reserved_cap == envelope.sponsor.maximum_charge
                            && receipt.sponsor_counter_before == envelope.sponsor.sponsor_nonce,
                        "Recovery receipt differs from signed input"
                    );
                    let result = &record.result.tx_results[index];
                    ensure!(
                        result.code == if receipt.success { 0 } else { 3 },
                        "Recovery outcome differs"
                    );
                    ensure!(
                        result.gas_wanted == i64::try_from(receipt.gas_limit)?
                            && result.gas_used == i64::try_from(receipt.gas_used)?,
                        "Recovery gas result differs"
                    );
                    indices.insert(index);
                }
            }
            for (index, raw) in record.input.txs.iter().enumerate() {
                if let Some(receipt) = ordinary_positions.remove(&(h, u32::try_from(index)?)) {
                    let WireTransaction::OrdinaryV2 { envelope_base64 } = wire(&config, raw)?
                    else {
                        anyhow::bail!("Ordinary receipt has no matching input");
                    };
                    let state = ordinary.as_ref().context("Ordinary state missing")?;
                    let signed = ordinary_signed(&state.config, &envelope_base64)?;
                    let verified = dytallix_runtime_crypto::ordinary::verify_signed(
                        &signed,
                        &state.config.fee_profile.limits,
                    )?;
                    let body = verified.body();
                    ensure!(
                        receipt.transaction_id() == verified.transaction_id()
                            && receipt.envelope_hash() == verified.envelope_hash()
                            && receipt.actor() == body.domain.account_id
                            && receipt.nonce_before() == body.spending_nonce
                            && receipt.nonce_after()
                                == body
                                    .spending_nonce
                                    .checked_add(1)
                                    .context("Ordinary nonce exhausted")?
                            && receipt.gas_limit() == body.gas_limit
                            && receipt.reserved_cap() == body.maximum_fee
                            && receipt.profile_digest() == body.fee_profile_digest
                            && receipt.profile_version() == body.fee_profile_version
                            && receipt.contract_version() == body.ordinary_fee_contract_version,
                        "Ordinary receipt differs from authenticated input"
                    );
                    let actor = recovery
                        .as_ref()
                        .context("Recovery missing")?
                        .accounts
                        .get(&hex::encode(body.domain.account_id))
                        .context("Ordinary actor missing")?;
                    ensure!(
                        body.domain == actor.recovery.domain && body.expiry_height > h,
                        "Ordinary committed domain or expiry differs"
                    );
                    crate::ordinary_validator::verify_historical_proofs(
                        &signed,
                        config
                            .lifecycle
                            .as_ref()
                            .context("Ordinary lifecycle missing")?,
                        &actor.address,
                        h,
                    )?;
                    let result = &record.result.tx_results[index];
                    ensure!(
                        result.code
                            == if receipt.outcome() == OrdinaryOutcome::Success {
                                0
                            } else {
                                3
                            }
                            && result.gas_wanted == i64::try_from(receipt.gas_limit())?
                            && result.gas_used == i64::try_from(receipt.gas_used())?,
                        "Ordinary committed result differs from receipt"
                    );
                    ensure!(
                        indices.insert(index),
                        "Duplicate accepted ordinary position"
                    );
                }
            }
            for accepted in &record.accepted {
                let index = usize::try_from(accepted.index)?;
                ensure!(
                    index < record.input.txs.len()
                        && indices.insert(index)
                        && transactions.insert(accepted.record.transaction.hash.clone()),
                    "Duplicate committed transaction or index"
                );
                let WireTransaction::Signed { envelope } = wire(&config, &record.input.txs[index])?
                else {
                    anyhow::bail!("Accepted transaction is not signed");
                };
                let verified = normalize_record(&config, envelope)?;
                ensure!(
                    verified.encode()? == accepted.record.encode()?,
                    "Committed original transaction differs"
                );
                let stored = storage
                    .get_transaction_record(&verified.transaction.hash)?
                    .context("Committed transaction missing")?;
                ensure!(
                    stored.encode()? == verified.encode()?,
                    "Stored transaction differs"
                );
                let receipt =
                    settlement::existing(storage, &verified.transaction, h, accepted.index)?
                        .context("Committed settlement missing")?;
                ensure!(
                    serde_json::to_vec(&receipt)? == serde_json::to_vec(&accepted.receipt)?
                        && record.result.tx_results[index]
                            == TxResult::from_receipt(&receipt, true)?,
                    "Committed receipt or result differs"
                );
                ensure!(
                    storage
                        .db
                        .get(format!("rcpt:{}", verified.transaction.hash))?
                        .as_deref()
                        == Some(serde_json::to_vec(&receipt)?.as_slice()),
                    "Receipt index differs"
                );
            }
            for (index, result) in record.result.tx_results.iter().enumerate() {
                ensure!(
                    result.gas_used >= 0
                        && result.gas_wanted >= 0
                        && result.gas_used <= result.gas_wanted,
                    "Invalid committed gas result"
                );
                if result.code == 0 || result.code == 3 {
                    ensure!(
                        indices.contains(&index)
                            || (boundary && index == 0 && result == &TxResult::observation())
                            || (result == &emergency_result()
                                && matches!(
                                    wire(&config, &record.input.txs[index]),
                                    Ok(WireTransaction::EmergencyControl { .. })
                                ))
                            || (result == &handover_result()
                                && matches!(
                                    wire(&config, &record.input.txs[index]),
                                    Ok(WireTransaction::HandoverControl { .. })
                                ))
                            || (result == &upgrade_result()
                                && matches!(
                                    wire(&config, &record.input.txs[index]),
                                    Ok(WireTransaction::UpgradeControl { .. })
                                )),
                        "Successful result lacks settlement"
                    );
                }
            }
            previous = Some(record.head.clone());
        }
        ensure!(
            previous.as_ref() == Some(head)
                && state_digest(storage, &Writes::new(), config.governance.is_some())? == head.state_digest,
            "Consensus committed state differs"
        );
    } else {
        ensure!(
            height == 0
                && state.last_height == 0
                && block_lifecycle::height(storage, "emission:last_height")? == 0,
            "Unmarked consensus history"
        );
        ensure!(
            initial_info.app_hash
                == digest(
                    b"dytallix-cometbft-genesis-v1",
                    &state_digest(storage, &Writes::new(), config.governance.is_some())?
                )?,
            "Initial consensus monetary state differs"
        );
        if let Some(best) = storage.db.get("meta:best_hash")? {
            ensure!(best == b"genesis", "Unmarked engine head");
        }
    }
    if let Some(penalties) = &penalties {
        ensure!(
            penalties.last_height == height
                && penalties.evidence_processed_height == height
                && penalties.parent_time == committed_parent_time(storage, height.max(1))?,
            "Penalty height, evidence completion or committed parent time differs"
        );
        let receipts: BTreeMap<_, _> = penalties
            .incidents
            .values()
            .map(|incident| {
                Ok((
                    serde_json::to_vec(&incident.fact)?,
                    incident.admitted_height,
                ))
            })
            .collect::<Result<_>>()?;
        ensure!(
            receipts.len() == penalties.incidents.len() && receipts == recorded_evidence,
            "Penalty receipts differ from committed evidence inputs"
        );
    } else {
        ensure!(
            recorded_evidence.is_empty(),
            "Committed evidence lacks penalty state"
        );
    }
    ensure!(recovery_positions.is_empty(), "Orphan recovery receipt");
    ensure!(ordinary_positions.is_empty(), "Orphan ordinary receipt");
    for item in storage.db.iterator(IteratorMode::Start) {
        let (key, _) = item?;
        ensure!(
            governance_key_permitted(&key, config.governance.is_some()),
            "Unknown or unconfigured governance state"
        );
        if key.starts_with(b"ordinary:") {
            ensure!(
                config.ordinary.is_some() && key.as_ref() == ORDINARY_STATE_KEY.as_bytes(),
                "Unknown or unconfigured ordinary state"
            );
        }
        if key.starts_with(b"recovery:") {
            ensure!(
                config.recovery.is_some() && key.as_ref() == RECOVERY_STATE_KEY.as_bytes(),
                "Unknown or unconfigured recovery state"
            );
        }
        if key.starts_with(b"consensus:")
            && !key.starts_with(b"consensus:emergency:")
            && !key.starts_with(b"consensus:upgrade:")
            && !key.starts_with(b"consensus:release-handover:")
        {
            ensure!(known.contains(key.as_ref()), "Unknown consensus record");
        }
        if key.starts_with(b"penalty:") {
            ensure!(
                config.penalty.is_some() && key.as_ref() == PENALTY_STATE_KEY.as_bytes(),
                "Unknown or unconfigured penalty state"
            );
        }
        if key.starts_with(b"lifecycle:") {
            ensure!(
                config.lifecycle.is_some() && key.as_ref() == LIFECYCLE_STATE_KEY.as_bytes(),
                "Unknown or unconfigured lifecycle state"
            );
        }
        ensure!(
            !key.starts_with(b"execution:block:") && !key.starts_with(b"blk_"),
            "Development block history in consensus mode"
        );
        for prefix in [b"tx:".as_slice(), b"rcpt:", b"execution:v1:receipt:"] {
            if let Some(suffix) = key.strip_prefix(prefix) {
                ensure!(
                    transactions.contains(std::str::from_utf8(suffix)?),
                    "Orphan or pending consensus transaction"
                );
            }
        }
    }
    let emergency_trace = emergency_history_with(history, &config)?;
    let outcomes = upgrade_history(history, &config, &emergency_trace, None)?;
    handover_history(history, &config, &emergency_trace, &outcomes, None)?;
    Ok(emergency_trace)
}

#[cfg(all(test, feature = "pqc-fips204"))]
#[path = "consensus_settlement_tests.rs"]
mod tests;

#[cfg(all(test, feature = "pqc-fips204"))]
#[path = "validator_lifecycle_settlement_tests.rs"]
mod lifecycle_tests;

#[cfg(all(test, feature = "pqc-fips204"))]
#[path = "penalty_settlement_tests.rs"]
mod penalty_tests;

#[cfg(all(test, feature = "pqc-fips204"))]
#[path = "recovery_consensus_tests.rs"]
mod recovery_tests;

#[cfg(all(test, feature = "pqc-fips204"))]
#[path = "ordinary_consensus_tests.rs"]
mod ordinary_tests;
