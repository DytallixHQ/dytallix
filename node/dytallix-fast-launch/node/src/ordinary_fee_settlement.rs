//! Approved OF01-OF05 immutable accounting plans. This is not an action executor.
//!
//! Financial snapshots and logical work records are trusted planning inputs.
//! Eligible liquidity must come from current custody/vesting checks. Plans have
//! no database writer and no execution permit. The selected profile is trusted
//! planning input. Combined activation must validate its validator-proof digest
//! and roles against an authenticated committed role record. Success describes the supplied
//! accounting trace, not proof that module effects happened. Unresolved registry
//! and validator-proof requirements prevent an accepted plan.
use crate::{
    ordinary_authority::{self, DeferredApplicationCheck, Grants},
    ordinary_meter::{LogicalRecord, MeterError, OrdinaryMeter, SharedBlockMeter},
    ordinary_reservations::{
        calculate_requirements, ActionDebit, Asset, DebitKind, Denomination, ReservationId,
        ReservationRequest,
    },
    recovery_fees::RecoveryBook,
};
use dytallix_protocol_types::{
    ordinary::{self, Action},
    ordinary_fees::{self, FeeProfile},
    sha3_256,
};
use dytallix_runtime_crypto::ordinary::VerifiedOrdinary;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
const MAX_RECEIPTS: usize = 65_536;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PlanError {
    Rejected(String),
    Internal(String),
    BlockCapacity,
    IntegrationUnavailable(String),
}
impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rejected(s) => write!(f, "Rejected: {s}"),
            Self::Internal(s) => write!(f, "Internal: {s}"),
            Self::BlockCapacity => f.write_str("Block capacity exhausted"),
            Self::IntegrationUnavailable(s) => write!(f, "Integration unavailable: {s}"),
        }
    }
}
impl std::error::Error for PlanError {}
type Result<T> = std::result::Result<T, PlanError>;
fn internal(e: impl std::fmt::Display) -> PlanError {
    PlanError::Internal(e.to_string())
}
fn rejected(e: impl std::fmt::Display) -> PlanError {
    PlanError::Rejected(e.to_string())
}
impl From<MeterError> for PlanError {
    fn from(e: MeterError) -> Self {
        match e {
            MeterError::PreAcceptanceRejected(s) => rejected(s),
            MeterError::BlockCapacity => Self::BlockCapacity,
            other => internal(other),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FinancialState {
    pub balances: BTreeMap<Asset, u128>,
    pub eligible: BTreeMap<Asset, u128>,
    pub native_nonces: BTreeMap<String, u64>,
    pub withheld_udrt: u128,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ApplicationRule {
    InactivityDelay,
    RecipientCapacity,
}
impl ApplicationRule {
    fn class(self) -> &'static str {
        match self {
            Self::InactivityDelay => "ACTION_STATE_PRECONDITION",
            Self::RecipientCapacity => "ACTION_CAPACITY",
        }
    }
    fn code(self) -> &'static str {
        match self {
            Self::InactivityDelay => "DMS_INACTIVITY_DELAY",
            Self::RecipientCapacity => "SEND_RECIPIENT_CAPACITY",
        }
    }
}
/// Typed accounting facts from a future reviewed module adapter. No success bool
/// grants authority. InactivityDelay is determined here from the current grant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AccountingEnd {
    Applied,
    ApplicationFailure(ApplicationRule),
    InternalFault,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ActionWork {
    pub reads: Vec<LogicalRecord>,
    pub writes: Vec<LogicalRecord>,
    pub end: AccountingEnd,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AccountingTrace {
    pub validation_reads: Vec<LogicalRecord>,
    pub actions: Vec<ActionWork>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Outcome {
    Success,
    ApplicationFailure,
    OutOfGas,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActionDisposition {
    KeepProposed,
    DiscardAll,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FeeReceipt {
    version: u16,
    transaction_id: [u8; 32],
    envelope_hash: [u8; 32],
    actor: [u8; 32],
    block_height: u64,
    block_index: u32,
    contract_version: u16,
    profile_version: u64,
    profile_digest: [u8; 32],
    outcome: Outcome,
    failing_action: Option<u16>,
    failure_phase: Option<String>,
    rule_class: Option<String>,
    rule_code: Option<String>,
    gas_limit: u64,
    gas_used: u64,
    metadata_gas: u64,
    reserved_cap: u128,
    charge: u128,
    released_cap: u128,
    nonce_before: u64,
    nonce_after: u64,
}
impl FeeReceipt {
    /// Convert public RPC fields without changing this record's durable encoding.
    pub(crate) fn client_view(
        &self,
        context: dytallix_protocol_types::ordinary_client::CommittedContext,
    ) -> dytallix_protocol_types::ordinary_client::ReceiptView {
        use dytallix_protocol_types::ordinary_client::{
            ReceiptOutcome, ReceiptView, CLIENT_VIEW_VERSION,
        };
        ReceiptView {
            version: CLIENT_VIEW_VERSION,
            context,
            transaction_id: self.transaction_id,
            envelope_hash: self.envelope_hash,
            actor: self.actor,
            block_height: self.block_height,
            block_index: self.block_index,
            contract_version: self.contract_version,
            profile_version: self.profile_version,
            profile_digest: self.profile_digest,
            outcome: match self.outcome {
                Outcome::Success => ReceiptOutcome::Success,
                Outcome::ApplicationFailure => ReceiptOutcome::ApplicationFailure,
                Outcome::OutOfGas => ReceiptOutcome::OutOfGas,
            },
            failing_action: self.failing_action,
            failure_phase: self.failure_phase.clone(),
            rule_class: self.rule_class.clone(),
            rule_code: self.rule_code.clone(),
            gas_limit: self.gas_limit,
            gas_used: self.gas_used,
            metadata_gas: self.metadata_gas,
            reserved_cap: self.reserved_cap,
            charge: self.charge,
            released_cap: self.released_cap,
            nonce_before: self.nonce_before,
            nonce_after: self.nonce_after,
        }
    }
    pub(crate) fn actor(&self) -> [u8; 32] {
        self.actor
    }
    pub(crate) fn block_height(&self) -> u64 {
        self.block_height
    }
    pub(crate) fn block_index(&self) -> u32 {
        self.block_index
    }
    pub(crate) fn envelope_hash(&self) -> [u8; 32] {
        self.envelope_hash
    }
    pub(crate) fn nonce_before(&self) -> u64 {
        self.nonce_before
    }
    pub(crate) fn nonce_after(&self) -> u64 {
        self.nonce_after
    }
    pub(crate) fn gas_limit(&self) -> u64 {
        self.gas_limit
    }
    pub(crate) fn reserved_cap(&self) -> u128 {
        self.reserved_cap
    }
    pub(crate) fn profile_digest(&self) -> [u8; 32] {
        self.profile_digest
    }
    pub(crate) fn profile_version(&self) -> u64 {
        self.profile_version
    }
    pub(crate) fn contract_version(&self) -> u16 {
        self.contract_version
    }
    pub(crate) fn transaction_id(&self) -> [u8; 32] {
        self.transaction_id
    }
    pub(crate) fn outcome(&self) -> Outcome {
        self.outcome
    }
    pub(crate) fn gas_used(&self) -> u64 {
        self.gas_used
    }
    pub(crate) fn charge(&self) -> u128 {
        self.charge
    }
    pub(crate) fn released_cap(&self) -> u128 {
        self.released_cap
    }
}
/// Retained evidence. The combined state adapter commits this with both nonce
/// mirrors, money, ordered block result and head, without pruning.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FeeHistory {
    receipts: BTreeMap<String, FeeReceipt>,
    profiles: BTreeMap<String, FeeProfile>,
}
impl FeeHistory {
    pub(crate) fn receipts(&self) -> impl Iterator<Item = &FeeReceipt> {
        self.receipts.values()
    }
    pub(crate) fn profiles(&self) -> &BTreeMap<String, FeeProfile> {
        &self.profiles
    }
    pub(crate) fn receipt(&self, id: [u8; 32]) -> Option<&FeeReceipt> {
        self.receipts.get(&hex::encode(id))
    }
    pub(crate) fn validate(&self) -> Result<()> {
        if self.receipts.len() > MAX_RECEIPTS || self.profiles.len() > MAX_RECEIPTS {
            return Err(internal("Retained fee history exceeds component bound"));
        }
        for (digest, profile) in &self.profiles {
            if *digest != hex::encode(ordinary_fees::profile_digest(profile).map_err(internal)?) {
                return Err(internal("Retained fee profile digest mismatch"));
            }
        }
        let mut positions = std::collections::BTreeSet::new();
        let mut nonces = std::collections::BTreeSet::new();
        for (id, r) in &self.receipts {
            let p = self
                .profiles
                .get(&hex::encode(r.profile_digest))
                .ok_or_else(|| internal("Retained receipt requires missing profile"))?;
            if r.version != 1
                || *id != hex::encode(r.transaction_id)
                || r.contract_version != p.ordinary_fee_contract_version
                || r.profile_version != p.version
                || r.block_height == 0
                || r.gas_used > r.gas_limit
                || r.metadata_gas != p.receipt_metadata_cost
                || r.nonce_before.checked_add(1) != Some(r.nonce_after)
                || !positions.insert((r.block_height, r.block_index))
                || !nonces.insert((r.actor, r.nonce_before))
            {
                return Err(internal("Retained ordinary receipt invariant failed"));
            }
            p.validate_request(r.gas_limit, r.reserved_cap)
                .map_err(internal)?;
            if r.charge != u128::from(r.gas_used.max(p.minimum_gas)) * u128::from(p.gas_price)
                || r.reserved_cap.checked_sub(r.charge) != Some(r.released_cap)
            {
                return Err(internal("Retained ordinary fee conservation failed"));
            }
            match r.outcome {
                Outcome::Success
                    if r.failing_action.is_none()
                        && r.rule_code.is_none()
                        && r.failure_phase.is_none()
                        && r.rule_class.is_none() => {}
                Outcome::OutOfGas
                    if r.gas_used == r.gas_limit
                        && r.rule_code.as_deref() == Some("OUT_OF_GAS")
                        && r.rule_class.is_none()
                        && r.failing_action.is_some()
                        && matches!(
                            r.failure_phase.as_deref(),
                            Some("action" | "ACTION" | "WRITE")
                        ) => {}
                Outcome::ApplicationFailure
                    if r.failing_action.is_some()
                        && matches!(r.failure_phase.as_deref(), Some("action" | "APPLICATION"))
                        && matches!(
                            (r.rule_class.as_deref(), r.rule_code.as_deref()),
                            (
                                Some("ACTION_STATE_PRECONDITION"),
                                Some("DMS_INACTIVITY_DELAY")
                            ) | (
                                Some("ACTION_CAPACITY"),
                                Some("SEND_RECIPIENT_CAPACITY" | "LIFECYCLE_CAPACITY")
                            ) | (
                                Some("ACTION_STATE_PRECONDITION"),
                                Some(
                                    "UNBOND_ALREADY_RELEASED"
                                        | "UNBOND_PENALTIES_PENDING"
                                        | "UNBOND_NOT_MATURE"
                                        | "BOND_TARGET_UNAVAILABLE"
                                        | "UNBOND_PRINCIPAL_UNAVAILABLE"
                                        | "VALIDATOR_ALREADY_REGISTERED"
                                        | "VALIDATOR_TARGET_UNAVAILABLE"
                                        | "VALIDATOR_EXPOSURE_BARRED"
                                        | "CONSENSUS_KEY_ALREADY_USED"
                                        | "BOND_PRINCIPAL_ZERO"
                                        | "VALIDATOR_SELF_BOND_MINIMUM"
                                        | "EXIT_PENDING_ADDITIONS"
                                        | "VALIDATOR_SET_EMPTY"
                                )
                            )
                        ) => {}
                _ => return Err(internal("Retained ordinary failure classification invalid")),
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FeePlan {
    receipt: FeeReceipt,
    disposition: ActionDisposition,
    fee_checkpoint: FinancialState,
    authority_checkpoint: RecoveryBook,
    grant_checkpoint: Grants,
    history: FeeHistory,
    predecessor_digest: [u8; 32],
}
impl FeePlan {
    pub(crate) fn receipt(&self) -> &FeeReceipt {
        &self.receipt
    }
    pub(crate) fn action_disposition(&self) -> ActionDisposition {
        self.disposition
    }
    /// Fee/nonce checkpoint only. Successful value-bearing actions are not executed.
    pub(crate) fn fee_checkpoint(&self) -> &FinancialState {
        &self.fee_checkpoint
    }
    pub(crate) fn authority_checkpoint(&self) -> &RecoveryBook {
        &self.authority_checkpoint
    }
    pub(crate) fn grant_checkpoint(&self) -> &Grants {
        &self.grant_checkpoint
    }
    pub(crate) fn history(&self) -> &FeeHistory {
        &self.history
    }
    pub(crate) fn predecessor_digest(&self) -> [u8; 32] {
        self.predecessor_digest
    }
    pub(crate) fn is_executable(&self) -> bool {
        false
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PlanningResult {
    Prepared(FeePlan),
    ExactRetainedRetry(FeeReceipt),
}
fn asset(owner: [u8; 32], denomination: Denomination) -> Asset {
    Asset {
        owner,
        denomination,
    }
}
fn balance(state: &FinancialState, owner: [u8; 32], denomination: Denomination) -> u128 {
    state
        .balances
        .get(&asset(owner, denomination))
        .copied()
        .unwrap_or(0)
}
fn expected_debits(
    verified: &VerifiedOrdinary,
    state: &FinancialState,
    include_staking: bool,
) -> Vec<ActionDebit> {
    let actor = verified.body().domain.account_id;
    let mut values = Vec::new();
    let mut claimed = std::collections::BTreeSet::new();
    for action in &verified.body().actions {
        match action {
            Action::Send {
                recipient,
                denomination,
                amount,
            } => values.push(ActionDebit {
                asset: asset(
                    actor,
                    match denomination {
                        ordinary::Denomination::Udgt => Denomination::Udgt,
                        ordinary::Denomination::Udrt => Denomination::Udrt,
                    },
                ),
                amount: *amount,
                kind: if *recipient == actor {
                    DebitKind::SelfTransfer
                } else {
                    DebitKind::Outflow
                },
            }),
            Action::RewardBond { amount_udgt, .. }
            | Action::ValidatorRegister { amount_udgt, .. }
                if include_staking =>
            {
                values.push(ActionDebit {
                    asset: asset(actor, Denomination::Udgt),
                    amount: *amount_udgt,
                    kind: DebitKind::Outflow,
                })
            }
            Action::DmsClaim { owner, .. } => {
                if !claimed.insert(*owner) {
                    continue;
                }
                for denomination in [Denomination::Udgt, Denomination::Udrt] {
                    let amount = balance(state, *owner, denomination);
                    if amount > 0 {
                        values.push(ActionDebit {
                            asset: asset(*owner, denomination),
                            amount,
                            kind: DebitKind::Outflow,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    values
}
/// Fresh reservation request derived from authenticated intent and the supplied
/// current balance snapshot. This is planning data, not an execution permit.
pub(crate) fn reservation_request(
    verified: &VerifiedOrdinary,
    profile: &FeeProfile,
    state: &FinancialState,
) -> Result<ReservationRequest> {
    let body = verified.body();
    let body_len = ordinary::signing_bytes(body, &profile.limits)
        .map_err(rejected)?
        .len();
    let wire_bytes = ordinary::WIRE_PREFIX.len()
        + 2
        + 4
        + body_len
        + 4
        + ordinary::signature_size(&body.key.algorithm).map_err(rejected)?;
    let proofs = body
        .actions
        .iter()
        .filter(|a| {
            matches!(
                a,
                Action::ValidatorRegister { .. } | Action::ValidatorRotateKey { .. }
            )
        })
        .count() as u64;
    Ok(ReservationRequest {
        context_digest: ordinary_fees::profile_digest(profile).map_err(internal)?,
        id: ReservationId::Ordinary(verified.transaction_id()),
        payer: body.domain.account_id,
        nonce: body.spending_nonce,
        fee_cap_udrt: body.maximum_fee,
        action_debits: expected_debits(verified, state, true),
        unrestricted_debits: expected_debits(verified, state, false),
        wire_bytes: wire_bytes as u64,
        signature_work: 1 + proofs,
    })
}
fn snapshot_digest(
    book: &RecoveryBook,
    grants: &Grants,
    state: &FinancialState,
) -> Result<[u8; 32]> {
    let assets: Vec<_> = state
        .balances
        .iter()
        .map(|(a, v)| {
            (
                a.owner,
                match a.denomination {
                    Denomination::Udgt => 1u8,
                    Denomination::Udrt => 2u8,
                },
                *v,
            )
        })
        .collect();
    let eligible: Vec<_> = state
        .eligible
        .iter()
        .map(|(a, v)| {
            (
                a.owner,
                match a.denomination {
                    Denomination::Udgt => 1u8,
                    Denomination::Udrt => 2u8,
                },
                *v,
            )
        })
        .collect();
    let bytes = serde_json::to_vec(&(
        book,
        grants,
        assets,
        eligible,
        &state.native_nonces,
        state.withheld_udrt,
    ))
    .map_err(internal)?;
    Ok(sha3_256(&bytes))
}
/// Rejected requests create no plan. Internal, BlockCapacity and
/// IntegrationUnavailable require the caller to discard the whole block plan.
/// Planner-origin faults do not reset or roll back the shared work meter.
#[allow(clippy::too_many_arguments)]
pub(crate) fn plan_fee_accounting(
    verified: &VerifiedOrdinary,
    profile: &FeeProfile,
    book: &RecoveryBook,
    grants: &Grants,
    state: &FinancialState,
    history: &FeeHistory,
    request: &ReservationRequest,
    height: u64,
    index: u32,
    trace: &AccountingTrace,
    block: &mut SharedBlockMeter,
) -> Result<PlanningResult> {
    profile.validate().map_err(internal)?;
    history.validate()?;
    for receipt in history.receipts.values() {
        let actor = book
            .accounts
            .get(&hex::encode(receipt.actor))
            .ok_or_else(|| internal("Retained receipt actor is absent from current state"))?;
        if receipt.block_height > book.last_height
            || receipt.nonce_after > actor.recovery.spending_nonce
        {
            return Err(internal(
                "Retained receipt is ahead of the staged authority checkpoint",
            ));
        }
    }
    ordinary_authority::validate_nonce_mirrors(book, &state.native_nonces).map_err(internal)?;
    ordinary_authority::validate_grants(book, grants).map_err(internal)?;
    let body = verified.body();
    let id = verified.transaction_id();
    if let Some(prior) = history.receipt(id) {
        if prior.block_height == height
            && prior.block_index == index
            && prior.envelope_hash == verified.envelope_hash()
        {
            return Ok(PlanningResult::ExactRetainedRetry(prior.clone()));
        }
        return Err(rejected("Ordinary transaction ID already accepted"));
    }
    if history.receipts.len() == MAX_RECEIPTS {
        return Err(rejected("Ordinary receipt retention capacity exhausted"));
    }
    let mut meter = OrdinaryMeter::new(profile, body, block)?;
    meter.ordinary_signature()?;
    let digest = ordinary_fees::profile_digest(profile).map_err(internal)?;
    if body.ordinary_fee_contract_version != profile.ordinary_fee_contract_version
        || body.fee_profile_version != profile.version
        || body.fee_profile_digest != digest
        || body.fee_denomination != profile.denomination
        || height < profile.activation_height
    {
        return Err(rejected(
            "Ordinary fee profile binding or activation mismatch",
        ));
    }
    let assessment = ordinary_authority::check_verified_preacceptance(
        book,
        &state.native_nonces,
        grants,
        height,
        verified,
        &profile.limits,
    )
    .map_err(rejected)?;
    if !assessment.registry_requirements.is_empty() {
        return Err(PlanError::IntegrationUnavailable(
            "Ordinary module ownership, entitlement or validator-proof adapter is not integrated"
                .into(),
        ));
    }
    if state.balances.len() > book.accounts.len() * 2
        || state.eligible.len() > book.accounts.len() * 2
        || state.native_nonces.len() > book.accounts.len()
    {
        return Err(internal(
            "Financial snapshot exceeds registered account capacity",
        ));
    }
    for (a, eligible) in &state.eligible {
        if !book.accounts.contains_key(&hex::encode(a.owner))
            || *eligible > state.balances.get(a).copied().unwrap_or(0)
        {
            return Err(internal(
                "Eligible liquidity differs from native financial snapshot",
            ));
        }
    }
    if *request != reservation_request(verified, profile, state)? {
        return Err(rejected(
            "Reservation inputs differ from authenticated ordinary intent or current debits",
        ));
    }
    let max_debits = usize::from(profile.limits.max_actions)
        .checked_mul(2)
        .ok_or_else(|| internal("Action debit capacity overflow"))?;
    let requirements = calculate_requirements(
        request.payer,
        request.fee_cap_udrt,
        &request.action_debits,
        max_debits,
    )
    .map_err(rejected)?;
    for (a, amounts) in requirements {
        if amounts.total().map_err(rejected)? > state.eligible.get(&a).copied().unwrap_or(0) {
            return Err(rejected(
                "Current eligible liquidity cannot fund signed cap and action debits",
            ));
        }
    }
    if trace.actions.len() != body.actions.len() {
        return Err(internal(
            "Accounting trace must cover the complete signed action list",
        ));
    }
    let max_records = usize::from(profile.limits.max_actions) * 16;
    if trace.validation_reads.len() > max_records
        || trace
            .actions
            .iter()
            .any(|a| a.reads.len() > 16 || a.writes.len() > 16)
    {
        return Err(internal("Accounting trace exceeds component record bound"));
    }
    for record in &trace.validation_reads {
        meter.read(record)?;
    }
    meter.accept()?;
    let mut outcome = Outcome::Success;
    let mut failing = None;
    let mut rule = None;
    'actions: for (i, work) in trace.actions.iter().enumerate() {
        let action_index = i as u16;
        match meter.action(action_index) {
            Ok(()) => {}
            Err(MeterError::AcceptedOutOfGas) => {
                outcome = Outcome::OutOfGas;
                failing = Some(action_index);
                break;
            }
            Err(e) => return Err(e.into()),
        }
        for deferred in &assessment.deferred_application_checks {
            match deferred {
                DeferredApplicationCheck::DmsClaimMaturity {
                    action_index: expected,
                    deadline_height,
                } if *expected == action_index && height < *deadline_height => {
                    outcome = Outcome::ApplicationFailure;
                    failing = Some(action_index);
                    rule = Some(ApplicationRule::InactivityDelay);
                    break 'actions;
                }
                _ => {}
            }
        }
        for record in &work.reads {
            match meter.read(record) {
                Ok(()) => {}
                Err(MeterError::AcceptedOutOfGas) => {
                    outcome = Outcome::OutOfGas;
                    failing = Some(action_index);
                    break 'actions;
                }
                Err(e) => return Err(e.into()),
            }
        }
        for record in &work.writes {
            match meter.write(record) {
                Ok(()) => {}
                Err(MeterError::AcceptedOutOfGas) => {
                    outcome = Outcome::OutOfGas;
                    failing = Some(action_index);
                    break 'actions;
                }
                Err(e) => return Err(e.into()),
            }
        }
        match work.end {
            AccountingEnd::Applied => {}
            AccountingEnd::InternalFault => {
                return Err(internal(
                    "Unclassified application failure stops block preparation",
                ))
            }
            AccountingEnd::ApplicationFailure(_) => {
                return Err(PlanError::IntegrationUnavailable(
                    "No reviewed module capacity predicate is integrated; caller classification cannot authorize a paid failure".into()));
            }
        }
    }
    let summary = meter.finish()?;
    if !summary.accepted()
        || (outcome == Outcome::OutOfGas) != summary.exhausted()
        || (outcome == Outcome::Success && summary.processed_actions() != summary.action_count())
    {
        return Err(internal("Meter outcome differs from accounting outcome"));
    }
    let charge = u128::from(summary.used_gas().max(profile.minimum_gas))
        .checked_mul(u128::from(profile.gas_price))
        .ok_or_else(|| internal("Ordinary fee multiplication overflow"))?;
    let released = body
        .maximum_fee
        .checked_sub(charge)
        .ok_or_else(|| internal("Ordinary charge exceeds signed reservation"))?;
    let mut financial = state.clone();
    let payer = asset(body.domain.account_id, Denomination::Udrt);
    let next_balance = state
        .balances
        .get(&payer)
        .copied()
        .unwrap_or(0)
        .checked_sub(charge)
        .ok_or_else(|| internal("Accepted fee reservation invariant failed"))?;
    financial.balances.insert(payer, next_balance);
    financial.eligible.insert(
        payer,
        state
            .eligible
            .get(&payer)
            .copied()
            .unwrap_or(0)
            .checked_sub(charge)
            .ok_or_else(|| internal("Accepted fee eligibility invariant failed"))?,
    );
    financial.withheld_udrt = state
        .withheld_udrt
        .checked_add(charge)
        .ok_or_else(|| internal("Withheld ordinary fee custody overflow"))?;
    let mut authority = book.clone();
    let actor = authority
        .accounts
        .get_mut(&hex::encode(body.domain.account_id))
        .ok_or_else(|| internal("Accepted actor disappeared"))?;
    actor.recovery.spending_nonce = assessment.prospective_nonce.after;
    financial
        .native_nonces
        .insert(actor.address.clone(), assessment.prospective_nonce.after);
    ordinary_authority::validate_nonce_mirrors(&authority, &financial.native_nonces)
        .map_err(internal)?;
    let keep = outcome == Outcome::Success;
    let receipt = FeeReceipt {
        version: 1,
        transaction_id: id,
        envelope_hash: verified.envelope_hash(),
        actor: body.domain.account_id,
        block_height: height,
        block_index: index,
        contract_version: profile.ordinary_fee_contract_version,
        profile_version: profile.version,
        profile_digest: digest,
        outcome,
        failing_action: failing,
        failure_phase: if keep { None } else { Some("action".into()) },
        rule_class: rule.map(|r| r.class().into()),
        rule_code: if outcome == Outcome::OutOfGas {
            Some("OUT_OF_GAS".into())
        } else {
            rule.map(|r| r.code().into())
        },
        gas_limit: body.gas_limit,
        gas_used: summary.used_gas(),
        metadata_gas: summary.metadata_gas(),
        reserved_cap: body.maximum_fee,
        charge,
        released_cap: released,
        nonce_before: body.spending_nonce,
        nonce_after: assessment.prospective_nonce.after,
    };
    let mut retained = history.clone();
    retained
        .profiles
        .insert(hex::encode(digest), profile.clone());
    retained.receipts.insert(hex::encode(id), receipt.clone());
    retained.validate()?;
    Ok(PlanningResult::Prepared(FeePlan {
        receipt,
        disposition: if keep {
            ActionDisposition::KeepProposed
        } else {
            ActionDisposition::DiscardAll
        },
        fee_checkpoint: financial,
        authority_checkpoint: authority,
        grant_checkpoint: if keep {
            assessment.prospective_grants
        } else {
            grants.clone()
        },
        history: retained,
        predecessor_digest: snapshot_digest(book, grants, state)?,
    }))
}
#[cfg(all(test, feature = "pqc-fips204"))]
#[path = "ordinary_fee_settlement_tests.rs"]
mod tests;

/// Used only by the metered runtime after it determines the action outcome.
pub(crate) fn retain_runtime_receipt(
    history: &FeeHistory,
    verified: &VerifiedOrdinary,
    profile: &FeeProfile,
    summary: &crate::ordinary_meter::MeterSummary,
    height: u64,
    index: u32,
    outcome: Outcome,
    failure: Option<(u16, &'static str, &'static str, &'static str)>,
) -> Result<(FeeHistory, FeeReceipt)> {
    if !summary.accepted()
        || (outcome == Outcome::OutOfGas) != summary.exhausted()
        || (outcome == Outcome::Success && summary.processed_actions() != summary.action_count())
        || failure.is_some_and(|f| usize::from(f.0) >= verified.body().actions.len())
        || history.receipt(verified.transaction_id()).is_some()
        || history.receipts.len() >= MAX_RECEIPTS
    {
        return Err(internal("Invalid runtime receipt predecessor"));
    }
    let b = verified.body();
    let charge = u128::from(summary.used_gas().max(profile.minimum_gas))
        .checked_mul(u128::from(profile.gas_price))
        .ok_or_else(|| internal("Fee overflow"))?;
    let receipt = FeeReceipt {
        version: 1,
        transaction_id: verified.transaction_id(),
        envelope_hash: verified.envelope_hash(),
        actor: b.domain.account_id,
        block_height: height,
        block_index: index,
        contract_version: b.ordinary_fee_contract_version,
        profile_version: profile.version,
        profile_digest: ordinary_fees::profile_digest(profile).map_err(internal)?,
        outcome,
        failing_action: failure.map(|v| v.0),
        failure_phase: failure.map(|v| v.1.into()),
        rule_class: failure.and_then(|v| {
            if v.2.is_empty() {
                None
            } else {
                Some(v.2.into())
            }
        }),
        rule_code: failure.map(|v| v.3.into()),
        gas_limit: b.gas_limit,
        gas_used: summary.used_gas(),
        metadata_gas: summary.metadata_gas(),
        reserved_cap: b.maximum_fee,
        charge,
        released_cap: b
            .maximum_fee
            .checked_sub(charge)
            .ok_or_else(|| internal("Charge exceeds cap"))?,
        nonce_before: b.spending_nonce,
        nonce_after: b
            .spending_nonce
            .checked_add(1)
            .ok_or_else(|| internal("Nonce exhausted"))?,
    };
    let mut next = history.clone();
    next.profiles
        .insert(hex::encode(receipt.profile_digest), profile.clone());
    next.receipts
        .insert(hex::encode(receipt.transaction_id), receipt.clone());
    next.validate()?;
    Ok((next, receipt))
}

pub(crate) fn expected_spending_debits(
    verified: &VerifiedOrdinary,
    state: &FinancialState,
) -> Vec<ActionDebit> {
    expected_debits(verified, state, false)
}
