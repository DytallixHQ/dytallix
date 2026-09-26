//! Pure ordinary-v3 governance fee, nonce and receipt planning.
//! No caller-provided gas summary is accepted. This planner creates and drives
//! the v3 meter for the same signed envelope it checks against signature proof.
//! The work trace is still a trusted adapter input: a future consensus executor
//! must supply actual reads, proposed writes and application outcome.
//! This plan has no database writer or consensus execution permit.

use crate::{
    governance_v3_meter::{MeterError, V3GovernanceMeter},
    ordinary_authority,
    ordinary_fee_settlement::FinancialState,
    ordinary_meter::{LogicalRecord, SharedBlockMeter},
    ordinary_reservations::{Asset, Denomination},
    recovery_fees::RecoveryBook,
    settlement::Settlement,
};
use dytallix_protocol_types::{
    ordinary_fees_v3::{self, FeeProfileV3, ORDINARY_FEE_CONTRACT_VERSION},
    ordinary_v3::{self as v3, SignedOrdinary},
};
use dytallix_runtime_crypto::ordinary_v3::VerifiedOrdinaryV3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PlanError {
    Rejected(String),
    Internal(String),
    BlockCapacity,
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rejected(message) => write!(f, "Rejected: {message}"),
            Self::Internal(message) => write!(f, "Internal: {message}"),
            Self::BlockCapacity => f.write_str("Block capacity exhausted"),
        }
    }
}

impl std::error::Error for PlanError {}
type Result<T> = std::result::Result<T, PlanError>;

fn rejected(message: impl std::fmt::Display) -> PlanError {
    PlanError::Rejected(message.to_string())
}

fn internal(message: impl std::fmt::Display) -> PlanError {
    PlanError::Internal(message.to_string())
}

impl From<MeterError> for PlanError {
    fn from(error: MeterError) -> Self {
        match error {
            MeterError::Rejected(message) => rejected(message),
            MeterError::BlockCapacity => Self::BlockCapacity,
            MeterError::AcceptedOutOfGas => {
                internal("Accepted out-of-gas escaped v3 action handling")
            }
            MeterError::Internal(message) => internal(message),
        }
    }
}

/// The executor must derive this result from its proposed governance transition.
/// Failed action effects are never present in the fee checkpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActionEnd {
    Applied,
    ApplicationFailure,
}

/// The executor must enumerate every actual logical read and proposed write.
/// The planner charges this work. It cannot prove that the trace is complete.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ActionWork {
    pub validation_reads: Vec<LogicalRecord>,
    pub action_reads: Vec<LogicalRecord>,
    pub proposed_writes: Vec<LogicalRecord>,
    pub end: ActionEnd,
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

/// The receipt has a distinct version and fee-profile namespace from v2.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct V3FeeReceipt {
    pub version: u16,
    pub transaction_id: [u8; 32],
    pub envelope_hash: [u8; 32],
    pub actor: [u8; 32],
    pub block_height: u64,
    pub block_index: u32,
    pub contract_version: u16,
    pub profile_version: u64,
    pub profile_digest: [u8; 32],
    pub outcome: Outcome,
    pub failure_code: Option<String>,
    pub gas_limit: u64,
    pub gas_used: u64,
    pub metadata_gas: u64,
    pub reserved_cap: u128,
    pub charge: u128,
    pub released_cap: u128,
    pub nonce_before: u64,
    pub nonce_after: u64,
}

/// The full signed cap must be available before accepted work begins.
/// This record describes a temporary block-local reservation, not a durable
/// asset transfer. Only the charged amount enters fee custody.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FeeReservation {
    pub asset: Asset,
    pub cap: u128,
    pub balance_before: u128,
    pub eligible_before: u128,
    pub balance_while_reserved: u128,
    pub eligible_while_reserved: u128,
}

#[derive(Clone, Debug)]
pub(crate) struct FeePlan {
    pub reservation: FeeReservation,
    pub receipt: V3FeeReceipt,
    pub disposition: ActionDisposition,
    pub fee_checkpoint: FinancialState,
    pub authority_checkpoint: RecoveryBook,
}

/// Compute the accounting plan from a verified signature and a RecoveryBook
/// already advanced to this block height. The caller must separately complete
/// governance preadmission against the committed parent,
/// candidate binding, DGT deposit funding and action-state validation.
/// Any error after meter creation requires the caller to discard the block
/// plan. The returned checkpoints must be committed atomically with the
/// selected governance action state, receipt history and block head.
#[allow(clippy::too_many_arguments)]
pub(crate) fn plan_governance_fee_from_settlement(
    signed: &SignedOrdinary,
    verified: &VerifiedOrdinaryV3,
    profile: &FeeProfileV3,
    book: &RecoveryBook,
    settlement: &Settlement,
    block_height: u64,
    block_index: u32,
    work: &ActionWork,
    block: &mut SharedBlockMeter,
) -> Result<FeePlan> {
    let financial = settlement.governance_financial_snapshot(book).map_err(internal)?;
    plan_governance_fee(
        signed,
        verified,
        profile,
        book,
        &financial,
        block_height,
        block_index,
        work,
        block,
    )
}

#[allow(clippy::too_many_arguments)]
fn plan_governance_fee(
    signed: &SignedOrdinary,
    verified: &VerifiedOrdinaryV3,
    profile: &FeeProfileV3,
    book: &RecoveryBook,
    financial: &FinancialState,
    block_height: u64,
    block_index: u32,
    work: &ActionWork,
    block: &mut SharedBlockMeter,
) -> Result<FeePlan> {
    let body = verified.body();
    let limits = profile.limits();
    // RecoveryBook::begin_block has already advanced all accounts to this
    // height. Mixed ordinary/recovery transactions then share this staged book.
    if block_height == 0 || book.last_height != block_height {
        return Err(rejected("Governance staged recovery height differs"));
    }
    profile
        .validate_signed_request(body, block_height)
        .map_err(rejected)?;
    if signed.body != *body
        || verified.transaction_id() != v3::transaction_id(body, &limits).map_err(rejected)?
        || verified.envelope_hash() != v3::envelope_hash(signed, &limits).map_err(rejected)?
    {
        return Err(rejected("V3 signature proof differs from metered envelope"));
    }
    if block_height >= body.expiry_height
        || body.expiry_height - block_height > limits.max_expiry_lifetime
    {
        return Err(rejected("V3 transaction expired or exceeds lifetime"));
    }
    ordinary_authority::validate_nonce_mirrors(book, &financial.native_nonces).map_err(internal)?;
    let account_asset_bound = book
        .accounts
        .len()
        .checked_mul(2)
        .ok_or_else(|| internal("Governance financial snapshot bound overflow"))?;
    if financial.native_nonces.len() != book.accounts.len()
        || financial.balances.len() > account_asset_bound
        || financial.eligible.len() > account_asset_bound
    {
        return Err(internal(
            "Governance financial snapshot exceeds account bounds",
        ));
    }
    for (asset, amount) in &financial.eligible {
        if !book.accounts.contains_key(&hex::encode(asset.owner))
            || *amount > financial.balances.get(asset).copied().unwrap_or(0)
        {
            return Err(internal(
                "Governance eligible liquidity differs from balances",
            ));
        }
    }
    let actor = body.domain.account_id;
    let account = book
        .accounts
        .get(&hex::encode(actor))
        .ok_or_else(|| rejected("Unregistered governance actor"))?;
    if account.recovery.domain != body.domain
        || account.recovery.active_key != body.key
        || account.recovery.active_generation != body.authorization_generation
        || account.recovery.spending_nonce != body.spending_nonce
        || !account.recovery.outgoing_allowed()
    {
        return Err(rejected("Governance account authority or nonce differs"));
    }
    let nonce_after = body
        .spending_nonce
        .checked_add(1)
        .ok_or_else(|| rejected("Governance spending nonce exhausted"))?;
    let asset = Asset {
        owner: actor,
        denomination: Denomination::Udrt,
    };
    let balance = financial.balances.get(&asset).copied().unwrap_or(0);
    let eligible = financial.eligible.get(&asset).copied().unwrap_or(0);
    if eligible > balance {
        return Err(internal(
            "Governance fee eligible balance exceeds account balance",
        ));
    }
    let cap = body.maximum_fee;
    let reservation = FeeReservation {
        asset,
        cap,
        balance_before: balance,
        eligible_before: eligible,
        balance_while_reserved: balance
            .checked_sub(cap)
            .ok_or_else(|| rejected("Insufficient uDRT balance for full signed fee cap"))?,
        eligible_while_reserved: eligible
            .checked_sub(cap)
            .ok_or_else(|| rejected("Insufficient eligible uDRT for full signed fee cap"))?,
    };
    if work.validation_reads.len() > 16
        || work.action_reads.len() > 16
        || work.proposed_writes.len() > 16
    {
        return Err(rejected("Governance work trace exceeds record bound"));
    }

    let mut meter = V3GovernanceMeter::new(profile, signed, block_height, block)?;
    meter.signature()?;
    for record in &work.validation_reads {
        meter.read(record)?;
    }
    meter.accept()?;
    let mut outcome = match meter.action() {
        Ok(()) => Outcome::Success,
        Err(MeterError::AcceptedOutOfGas) => Outcome::OutOfGas,
        Err(error) => return Err(error.into()),
    };
    if outcome != Outcome::OutOfGas {
        for record in &work.action_reads {
            match meter.read(record) {
                Ok(()) => {}
                Err(MeterError::AcceptedOutOfGas) => {
                    outcome = Outcome::OutOfGas;
                    break;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
    if outcome != Outcome::OutOfGas {
        for record in &work.proposed_writes {
            match meter.write(record) {
                Ok(()) => {}
                Err(MeterError::AcceptedOutOfGas) => {
                    outcome = Outcome::OutOfGas;
                    break;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
    if outcome != Outcome::OutOfGas && work.end == ActionEnd::ApplicationFailure {
        outcome = Outcome::ApplicationFailure;
    }
    let summary = meter.finish()?;
    if !summary.accepted
        || summary.action_count != 1
        || summary.metadata_gas != profile.base.receipt_metadata_cost
        || summary.used_gas > body.gas_limit
        || summary.exhausted != (outcome == Outcome::OutOfGas)
        || (outcome != Outcome::OutOfGas && summary.processed_actions != 1)
    {
        return Err(internal("V3 measured work differs from accepted outcome"));
    }
    let charge = u128::from(summary.used_gas.max(profile.base.minimum_gas))
        .checked_mul(u128::from(profile.base.gas_price))
        .ok_or_else(|| internal("V3 fee multiplication overflow"))?;
    let released_cap = cap
        .checked_sub(charge)
        .ok_or_else(|| internal("V3 charge exceeds signed reservation"))?;

    let mut fee_checkpoint = financial.clone();
    fee_checkpoint.balances.insert(
        asset,
        reservation
            .balance_while_reserved
            .checked_add(released_cap)
            .ok_or_else(|| internal("V3 balance release overflow"))?,
    );
    fee_checkpoint.eligible.insert(
        asset,
        reservation
            .eligible_while_reserved
            .checked_add(released_cap)
            .ok_or_else(|| internal("V3 eligible release overflow"))?,
    );
    fee_checkpoint.withheld_udrt = financial
        .withheld_udrt
        .checked_add(charge)
        .ok_or_else(|| internal("V3 withheld fee custody overflow"))?;
    fee_checkpoint
        .native_nonces
        .insert(account.address.clone(), nonce_after);

    let mut authority_checkpoint = book.clone();
    authority_checkpoint
        .accounts
        .get_mut(&hex::encode(actor))
        .ok_or_else(|| internal("Accepted governance actor disappeared"))?
        .recovery
        .spending_nonce = nonce_after;
    ordinary_authority::validate_nonce_mirrors(
        &authority_checkpoint,
        &fee_checkpoint.native_nonces,
    )
    .map_err(internal)?;

    let digest = ordinary_fees_v3::profile_digest(profile).map_err(internal)?;
    let receipt = V3FeeReceipt {
        version: 2,
        transaction_id: verified.transaction_id(),
        envelope_hash: verified.envelope_hash(),
        actor,
        block_height,
        block_index,
        contract_version: ORDINARY_FEE_CONTRACT_VERSION,
        profile_version: profile.version,
        profile_digest: digest,
        outcome,
        failure_code: match outcome {
            Outcome::Success => None,
            Outcome::ApplicationFailure => Some("GOVERNANCE_APPLICATION_FAILURE".into()),
            Outcome::OutOfGas => Some("OUT_OF_GAS".into()),
        },
        gas_limit: body.gas_limit,
        gas_used: summary.used_gas,
        metadata_gas: summary.metadata_gas,
        reserved_cap: cap,
        charge,
        released_cap,
        nonce_before: body.spending_nonce,
        nonce_after,
    };
    Ok(FeePlan {
        reservation,
        receipt,
        disposition: if outcome == Outcome::Success {
            ActionDisposition::KeepProposed
        } else {
            ActionDisposition::DiscardAll
        },
        fee_checkpoint,
        authority_checkpoint,
    })
}

#[cfg(all(test, feature = "pqc-fips204"))]
#[path = "governance_v3_fee_settlement_tests.rs"]
mod tests;
