//! Deterministic account recovery policy component. No network or signature verifier.
//!
//! `AuthenticatedFacts` is a TRUST BOUNDARY, not a proof format. A future trusted
//! adapter must verify signatures and possession proofs over the exact domain and
//! complete action, with separate operation/proof domains and approved algorithms.
//! Constructing facts in Rust or deserializing them does not authenticate anything.
//! This component does not authorize fees, persist balances, or provide an RPC path.
//! The adapter must atomically couple its output to fees, receipts and account state.
//! Commit `advance_height` at block start, even if every later transaction fails.
//! `transition` only accepts the already processed height and returns a new state.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub type Result<T> = std::result::Result<T, RecoveryError>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryError(pub String);
impl std::fmt::Display for RecoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for RecoveryError {}
fn require(ok: bool, message: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(RecoveryError(message.into()))
    }
}
fn add(value: u64, amount: u64) -> Result<u64> {
    value
        .checked_add(amount)
        .ok_or_else(|| RecoveryError("counter or height overflow".into()))
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyIdentity {
    pub algorithm: String,
    pub public_key: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Guardian {
    pub key: KeyIdentity,
    pub control_group: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryPolicy {
    pub threshold: u16,
    pub guardians: Vec<Guardian>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryDomain {
    pub network: u8,
    pub chain_id: String,
    pub genesis_digest: [u8; 32],
    pub account_id: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryConfig {
    pub timing_version: u64,
    pub recovery_delay: u64,
    pub finalization_window: u64,
    pub policy_delay: u64,
    pub policy_window: u64,
    pub submission_lifetime: u64,
    /// Explicit verifier profile: exact algorithm identifier -> public key length.
    /// Supplying this map is not production algorithm approval or conformance proof.
    pub algorithms: BTreeMap<String, usize>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActiveAuthorization {
    pub generation: u64,
    pub nonce: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryAuthorization {
    pub policy_version: u64,
    pub sequence: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyAuthorization {
    pub policy_version: u64,
    pub sequence: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub submission_expiry: u64,
    pub kind: ActionKind,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ActionKind {
    Enroll {
        active: ActiveAuthorization,
        policy: RecoveryPolicy,
    },
    Spend {
        active: ActiveAuthorization,
    },
    Rotate {
        active: ActiveAuthorization,
        replacement: KeyIdentity,
    },
    Start {
        recovery: RecoveryAuthorization,
        request_id: [u8; 32],
        replacement: KeyIdentity,
        timing_version: u64,
    },
    Finalize {
        recovery: RecoveryAuthorization,
        request_id: [u8; 32],
    },
    Cancel {
        recovery: RecoveryAuthorization,
        request_id: [u8; 32],
    },
    Resume {
        recovery: RecoveryAuthorization,
        active_key: KeyIdentity,
    },
    StagePolicy {
        active: ActiveAuthorization,
        authorization: PolicyAuthorization,
        update_id: [u8; 32],
        policy: RecoveryPolicy,
        timing_version: u64,
    },
    ActivatePolicy {
        active: ActiveAuthorization,
        authorization: PolicyAuthorization,
        update_id: [u8; 32],
    },
    CancelPolicy {
        authorization: PolicyAuthorization,
        update_id: [u8; 32],
    },
}
/// Trusted verifier output only. Never accept these fields directly from a client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthenticatedFacts {
    pub domain: RecoveryDomain,
    pub action: Action,
    pub signers: Vec<KeyIdentity>,
    pub proofs: Vec<KeyIdentity>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryStatus {
    Normal,
    PendingRecovery,
    RecoveryLocked,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingRecovery {
    pub request_id: [u8; 32],
    pub replacement: KeyIdentity,
    pub policy_version: u64,
    pub consumed_sequence: u64,
    pub start_height: u64,
    pub activation_height: u64,
    pub expiry_height: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingPolicy {
    pub update_id: [u8; 32],
    pub policy: RecoveryPolicy,
    pub policy_version: u64,
    pub consumed_sequence: u64,
    pub start_height: u64,
    pub activation_height: u64,
    pub expiry_height: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RecoveryStateWire")]
pub struct RecoveryState {
    pub domain: RecoveryDomain,
    pub config: RecoveryConfig,
    pub active_key: KeyIdentity,
    pub active_generation: u64,
    pub spending_nonce: u64,
    pub policy: Option<RecoveryPolicy>,
    pub policy_version: u64,
    pub recovery_sequence: u64,
    pub policy_change_sequence: u64,
    pub status: RecoveryStatus,
    pub pending_recovery: Option<PendingRecovery>,
    pub pending_policy: Option<PendingPolicy>,
    pub last_height: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryStateWire {
    domain: RecoveryDomain,
    config: RecoveryConfig,
    active_key: KeyIdentity,
    active_generation: u64,
    spending_nonce: u64,
    policy: Option<RecoveryPolicy>,
    policy_version: u64,
    recovery_sequence: u64,
    policy_change_sequence: u64,
    status: RecoveryStatus,
    pending_recovery: Option<PendingRecovery>,
    pending_policy: Option<PendingPolicy>,
    last_height: u64,
}
impl TryFrom<RecoveryStateWire> for RecoveryState {
    type Error = RecoveryError;
    fn try_from(w: RecoveryStateWire) -> Result<Self> {
        let s = Self {
            domain: w.domain,
            config: w.config,
            active_key: w.active_key,
            active_generation: w.active_generation,
            spending_nonce: w.spending_nonce,
            policy: w.policy,
            policy_version: w.policy_version,
            recovery_sequence: w.recovery_sequence,
            policy_change_sequence: w.policy_change_sequence,
            status: w.status,
            pending_recovery: w.pending_recovery,
            pending_policy: w.pending_policy,
            last_height: w.last_height,
        };
        s.validate()?;
        Ok(s)
    }
}

impl RecoveryConfig {
    pub fn validate(&self) -> Result<()> {
        require(
            self.timing_version > 0,
            "timing version must be explicit and positive",
        )?;
        require(
            [
                self.recovery_delay,
                self.finalization_window,
                self.policy_delay,
                self.policy_window,
                self.submission_lifetime,
            ]
            .iter()
            .all(|v| *v > 0),
            "timing values must be explicit and positive",
        )?;
        require(!self.algorithms.is_empty(), "algorithm profile is required")?;
        for (name, len) in &self.algorithms {
            require(
                !name.is_empty()
                    && name.bytes().all(|c| {
                        c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-'
                    })
                    && *len > 0,
                "invalid explicit algorithm profile",
            )?;
        }
        Ok(())
    }
    fn key(&self, key: &KeyIdentity) -> Result<()> {
        let size = self
            .algorithms
            .get(&key.algorithm)
            .ok_or_else(|| RecoveryError("algorithm absent from explicit profile".into()))?;
        require(
            *size == key.public_key.len(),
            "public key length differs from profile",
        )
    }
}
impl RecoveryPolicy {
    pub fn validate(&self, config: &RecoveryConfig, active_key: &KeyIdentity) -> Result<()> {
        require(
            self.threshold == 2 && self.guardians.len() == 3,
            "selected recovery profile requires two of three guardians",
        )?;
        let mut keys = BTreeSet::new();
        let mut groups = BTreeSet::new();
        for guardian in &self.guardians {
            config.key(&guardian.key)?;
            require(
                keys.insert(&guardian.key.public_key),
                "duplicate guardian key",
            )?;
            require(
                guardian.key.public_key != active_key.public_key,
                "active key cannot be a guardian",
            )?;
            require(
                !guardian.control_group.is_empty()
                    && guardian.control_group.trim() == guardian.control_group
                    && groups.insert(&guardian.control_group),
                "guardian control groups must be distinct and nonempty",
            )?;
        }
        Ok(())
    }
}
impl RecoveryState {
    pub fn new(
        domain: RecoveryDomain,
        config: RecoveryConfig,
        active_key: KeyIdentity,
        height: u64,
    ) -> Result<Self> {
        let state = Self {
            domain,
            config,
            active_key,
            active_generation: 0,
            spending_nonce: 0,
            policy: None,
            policy_version: 0,
            recovery_sequence: 0,
            policy_change_sequence: 0,
            status: RecoveryStatus::Normal,
            pending_recovery: None,
            pending_policy: None,
            last_height: height,
        };
        state.validate()?;
        Ok(state)
    }
    /// Validate restored state structurally. Storage must additionally authenticate
    /// the committed state root; this does not detect a coherent malicious rewrite.
    pub fn validate(&self) -> Result<()> {
        self.config.validate()?;
        require(
            (1..=3).contains(&self.domain.network),
            "unknown network code",
        )?;
        require(
            !self.domain.chain_id.is_empty() && self.domain.chain_id.len() <= 255,
            "invalid chain identifier length",
        )?;
        self.config.key(&self.active_key)?;
        if let Some(policy) = &self.policy {
            require(
                self.policy_version > 0,
                "enrolled policy must have a version",
            )?;
            policy.validate(&self.config, &self.active_key)?;
        } else {
            require(
                self.policy_version == 0
                    && self.recovery_sequence == 0
                    && self.policy_change_sequence == 0
                    && self.status == RecoveryStatus::Normal
                    && self.pending_recovery.is_none()
                    && self.pending_policy.is_none(),
                "unenrolled account contains recovery state",
            )?;
        }
        require(
            (self.status == RecoveryStatus::PendingRecovery) == self.pending_recovery.is_some(),
            "pending recovery status differs from record",
        )?;
        if self.status != RecoveryStatus::Normal {
            require(
                self.policy.is_some() && self.pending_policy.is_none(),
                "protected account cannot stage policy changes",
            )?;
        }
        if let Some(p) = &self.pending_recovery {
            self.config.key(&p.replacement)?;
            self.policy
                .as_ref()
                .ok_or_else(|| RecoveryError("missing recovery policy".into()))?
                .validate(&self.config, &p.replacement)?;
            require(
                p.policy_version == self.policy_version
                    && add(p.consumed_sequence, 1)? == self.recovery_sequence,
                "pending recovery authority differs from state",
            )?;
            require(
                p.start_height <= self.last_height
                    && p.activation_height == add(p.start_height, self.config.recovery_delay)?
                    && p.expiry_height
                        == add(p.activation_height, self.config.finalization_window)?
                    && self.last_height < p.expiry_height,
                "invalid pending recovery timing",
            )?;
            require(
                self.active_generation > 0,
                "pending recovery has no generation transition",
            )?;
            add(self.active_generation, 1)?;
            add(self.recovery_sequence, 1)?;
        }
        if let Some(p) = &self.pending_policy {
            require(
                self.status == RecoveryStatus::Normal && self.policy.is_some(),
                "invalid staged policy state",
            )?;
            p.policy.validate(&self.config, &self.active_key)?;
            require(
                p.policy_version == self.policy_version
                    && add(p.consumed_sequence, 1)? == self.policy_change_sequence,
                "staged policy authority differs from state",
            )?;
            require(
                p.start_height <= self.last_height
                    && p.activation_height == add(p.start_height, self.config.policy_delay)?
                    && p.expiry_height == add(p.activation_height, self.config.policy_window)?
                    && self.last_height < p.expiry_height,
                "invalid staged policy timing",
            )?;
            add(self.policy_change_sequence, 1)?;
            add(self.policy_version, 1)?;
        }
        Ok(())
    }
    /// Advance block-start state before executing any transaction. Commit the
    /// returned state even if subsequent transactions fail. Repeating a processed
    /// height is harmless. Skipping empty heights applies every due expiry once.
    pub fn advance_height(&self, height: u64) -> Result<Self> {
        self.validate()?;
        require(
            height >= self.last_height,
            "committed height cannot roll back",
        )?;
        let mut next = self.clone();
        if next
            .pending_recovery
            .as_ref()
            .is_some_and(|p| height >= p.expiry_height)
        {
            next.pending_recovery = None;
            next.active_generation = add(next.active_generation, 1)?;
            next.recovery_sequence = add(next.recovery_sequence, 1)?;
            next.status = RecoveryStatus::RecoveryLocked;
        }
        if next
            .pending_policy
            .as_ref()
            .is_some_and(|p| height >= p.expiry_height)
        {
            next.pending_policy = None;
            next.policy_change_sequence = add(next.policy_change_sequence, 1)?;
        }
        next.last_height = height;
        next.validate()?;
        Ok(next)
    }
    /// Authority-only internal operation. `Spend` is not a transaction schema: it
    /// omits destination, amount and fees. The adapter must verify the COMPLETE
    /// external signed operation and atomically apply its effects, then bind its
    /// verified facts to this internal action. These serde bytes are not an approved
    /// signing envelope and are insufficient external spending authorization.
    pub fn transition(
        &self,
        height: u64,
        action: &Action,
        facts: &AuthenticatedFacts,
    ) -> Result<Self> {
        self.validate()?;
        require(
            height == self.last_height,
            "process block-start height before transactions",
        )?;
        require(
            facts.domain == self.domain && facts.action == *action,
            "authenticated facts do not bind this domain and action",
        )?;
        require(
            height < action.submission_expiry,
            "action submission expired",
        )?;
        require(
            action.submission_expiry - height <= self.config.submission_lifetime,
            "action submission lifetime exceeds profile",
        )?;
        for list in [&facts.signers, &facts.proofs] {
            let mut unique = BTreeSet::new();
            for key in list {
                self.config.key(key)?;
                require(unique.insert(key), "duplicate authenticated key fact")?;
            }
        }
        let mut next = self.clone();
        next.apply(action, facts)?;
        next.validate()?;
        Ok(next)
    }
    pub fn outgoing_allowed(&self) -> bool {
        self.status == RecoveryStatus::Normal
    }
    fn normal(&self) -> Result<()> {
        require(
            self.outgoing_allowed(),
            "outgoing account actions are protected",
        )
    }
    fn active(&self, auth: &ActiveAuthorization, facts: &AuthenticatedFacts) -> Result<()> {
        self.normal()?;
        require(
            auth.generation == self.active_generation && auth.nonce == self.spending_nonce,
            "active authorization generation or nonce differs",
        )?;
        require(
            facts.signers.contains(&self.active_key),
            "current active key authorization required",
        )
    }
    fn recovery(&self, auth: &RecoveryAuthorization) -> Result<()> {
        require(
            self.policy.is_some()
                && auth.policy_version == self.policy_version
                && auth.sequence == self.recovery_sequence,
            "recovery authority differs",
        )
    }
    fn policy_authority(&self, auth: &PolicyAuthorization) -> Result<()> {
        require(
            self.policy.is_some()
                && auth.policy_version == self.policy_version
                && auth.sequence == self.policy_change_sequence,
            "policy-change authority differs",
        )
    }
    fn only_signer(&self, facts: &AuthenticatedFacts, key: &KeyIdentity) -> Result<()> {
        require(
            facts.signers.len() == 1 && facts.signers.first() == Some(key),
            "wrong signing role",
        )
    }
    fn quorum(&self, facts: &AuthenticatedFacts, allow_active: bool) -> Result<()> {
        let policy = self
            .policy
            .as_ref()
            .ok_or_else(|| RecoveryError("account has no recovery policy".into()))?;
        let mut count = 0;
        for key in &facts.signers {
            if policy.guardians.iter().any(|g| g.key == *key) {
                count += 1;
            } else {
                require(
                    allow_active && key == &self.active_key,
                    "signer is not a current guardian",
                )?;
            }
        }
        require(
            count >= usize::from(policy.threshold),
            "guardian quorum required",
        )
    }
    fn proofs(&self, facts: &AuthenticatedFacts, expected: &[KeyIdentity]) -> Result<()> {
        let actual: BTreeSet<_> = facts.proofs.iter().collect();
        let wanted: BTreeSet<_> = expected.iter().collect();
        require(
            actual == wanted && actual.len() == facts.proofs.len(),
            "exact replacement or enrollment possession proofs required",
        )
    }
    fn pending(&self, request_id: &[u8; 32]) -> Result<&PendingRecovery> {
        let p = self
            .pending_recovery
            .as_ref()
            .ok_or_else(|| RecoveryError("no pending recovery".into()))?;
        require(
            &p.request_id == request_id,
            "pending recovery identifier differs",
        )?;
        Ok(p)
    }
    fn staged(&self, update_id: &[u8; 32]) -> Result<&PendingPolicy> {
        let p = self
            .pending_policy
            .as_ref()
            .ok_or_else(|| RecoveryError("no staged policy update".into()))?;
        require(
            &p.update_id == update_id,
            "staged policy identifier differs",
        )?;
        Ok(p)
    }
    fn apply(&mut self, action: &Action, facts: &AuthenticatedFacts) -> Result<()> {
        match &action.kind {
            ActionKind::Enroll { active, policy } => {
                self.active(active, facts)?;
                self.only_signer(facts, &self.active_key)?;
                require(self.policy.is_none(), "recovery policy is already enrolled")?;
                policy.validate(&self.config, &self.active_key)?;
                self.proofs(
                    facts,
                    &policy
                        .guardians
                        .iter()
                        .map(|g| g.key.clone())
                        .collect::<Vec<_>>(),
                )?;
                add(self.active_generation, 3)?;
                self.active_generation = add(self.active_generation, 1)?;
                self.spending_nonce = add(self.spending_nonce, 1)?;
                self.policy = Some(policy.clone());
                self.policy_version = 1;
            }
            ActionKind::Spend { active } => {
                self.active(active, facts)?;
                self.only_signer(facts, &self.active_key)?;
                self.proofs(facts, &[])?;
                self.spending_nonce = add(self.spending_nonce, 1)?;
            }
            ActionKind::Rotate {
                active,
                replacement,
            } => {
                self.active(active, facts)?;
                self.only_signer(facts, &self.active_key)?;
                self.config.key(replacement)?;
                self.proofs(facts, std::slice::from_ref(replacement))?;
                // A compromised active key must not exhaust the generation space
                // required for Start and one terminal recovery transition.
                add(
                    self.active_generation,
                    if self.policy.is_some() { 3 } else { 1 },
                )?;
                self.active_generation = add(self.active_generation, 1)?;
                self.spending_nonce = add(self.spending_nonce, 1)?;
                self.active_key = replacement.clone();
            }
            ActionKind::Start {
                recovery,
                request_id,
                replacement,
                timing_version,
            } => {
                self.recovery(recovery)?;
                self.quorum(facts, false)?;
                self.config.key(replacement)?;
                self.proofs(facts, std::slice::from_ref(replacement))?;
                require(
                    self.pending_recovery.is_none(),
                    "recovery is already pending",
                )?;
                require(
                    *timing_version == self.config.timing_version,
                    "timing policy version differs",
                )?;
                self.policy
                    .as_ref()
                    .expect("recovery checked")
                    .validate(&self.config, replacement)?;
                add(self.active_generation, 2)?;
                add(self.recovery_sequence, 2)?;
                let activation = add(self.last_height, self.config.recovery_delay)?;
                let expiry = add(activation, self.config.finalization_window)?;
                self.pending_recovery = Some(PendingRecovery {
                    request_id: *request_id,
                    replacement: replacement.clone(),
                    policy_version: self.policy_version,
                    consumed_sequence: self.recovery_sequence,
                    start_height: self.last_height,
                    activation_height: activation,
                    expiry_height: expiry,
                });
                self.active_generation = add(self.active_generation, 1)?;
                self.recovery_sequence = add(self.recovery_sequence, 1)?;
                self.status = RecoveryStatus::PendingRecovery;
                if self.pending_policy.take().is_some() {
                    self.policy_change_sequence = add(self.policy_change_sequence, 1)?;
                }
            }
            ActionKind::Finalize {
                recovery,
                request_id,
            } => {
                self.recovery(recovery)?;
                let p = self.pending(request_id)?.clone();
                require(
                    self.last_height >= p.activation_height && self.last_height < p.expiry_height,
                    "recovery finalization is outside its window",
                )?;
                self.only_signer(facts, &p.replacement)?;
                self.proofs(facts, &[])?;
                self.active_key = p.replacement;
                self.active_generation = add(self.active_generation, 1)?;
                self.recovery_sequence = add(self.recovery_sequence, 1)?;
                self.pending_recovery = None;
                self.status = RecoveryStatus::Normal;
            }
            ActionKind::Cancel {
                recovery,
                request_id,
            } => {
                self.recovery(recovery)?;
                self.pending(request_id)?;
                self.quorum(facts, false)?;
                self.proofs(facts, &[])?;
                self.active_generation = add(self.active_generation, 1)?;
                self.recovery_sequence = add(self.recovery_sequence, 1)?;
                self.pending_recovery = None;
                self.status = RecoveryStatus::RecoveryLocked;
            }
            ActionKind::Resume {
                recovery,
                active_key,
            } => {
                self.recovery(recovery)?;
                require(
                    self.status == RecoveryStatus::RecoveryLocked,
                    "resume requires a protected lock",
                )?;
                require(
                    *active_key == self.active_key,
                    "resume must bind the exact current active key",
                )?;
                self.quorum(facts, false)?;
                self.proofs(facts, &[])?;
                self.active_generation = add(self.active_generation, 1)?;
                self.recovery_sequence = add(self.recovery_sequence, 1)?;
                self.status = RecoveryStatus::Normal;
            }
            ActionKind::StagePolicy {
                active,
                authorization,
                update_id,
                policy,
                timing_version,
            } => {
                self.active(active, facts)?;
                self.policy_authority(authorization)?;
                self.quorum(facts, true)?;
                require(
                    self.pending_policy.is_none(),
                    "a policy update is already staged",
                )?;
                require(
                    *timing_version == self.config.timing_version,
                    "timing policy version differs",
                )?;
                policy.validate(&self.config, &self.active_key)?;
                self.proofs(
                    facts,
                    &policy
                        .guardians
                        .iter()
                        .map(|g| g.key.clone())
                        .collect::<Vec<_>>(),
                )?;
                add(self.policy_change_sequence, 2)?;
                add(self.policy_version, 1)?;
                let activation = add(self.last_height, self.config.policy_delay)?;
                let expiry = add(activation, self.config.policy_window)?;
                self.pending_policy = Some(PendingPolicy {
                    update_id: *update_id,
                    policy: policy.clone(),
                    policy_version: self.policy_version,
                    consumed_sequence: self.policy_change_sequence,
                    start_height: self.last_height,
                    activation_height: activation,
                    expiry_height: expiry,
                });
                self.policy_change_sequence = add(self.policy_change_sequence, 1)?;
                self.spending_nonce = add(self.spending_nonce, 1)?;
            }
            ActionKind::ActivatePolicy {
                active,
                authorization,
                update_id,
            } => {
                self.active(active, facts)?;
                self.policy_authority(authorization)?;
                self.quorum(facts, true)?;
                self.proofs(facts, &[])?;
                let p = self.staged(update_id)?.clone();
                require(
                    self.last_height >= p.activation_height && self.last_height < p.expiry_height,
                    "policy activation is outside its window",
                )?;
                self.policy = Some(p.policy);
                self.policy_version = add(self.policy_version, 1)?;
                self.policy_change_sequence = add(self.policy_change_sequence, 1)?;
                self.spending_nonce = add(self.spending_nonce, 1)?;
                self.pending_policy = None;
            }
            ActionKind::CancelPolicy {
                authorization,
                update_id,
            } => {
                self.normal()?;
                self.policy_authority(authorization)?;
                self.staged(update_id)?;
                self.quorum(facts, false)?;
                self.proofs(facts, &[])?;
                self.pending_policy = None;
                self.policy_change_sequence = add(self.policy_change_sequence, 1)?;
            }
        }
        Ok(())
    }
}
