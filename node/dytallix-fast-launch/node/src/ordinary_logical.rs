//! OF01 logical-record adapters. These bytes measure typed state, not physical storage.
//! All bounds are explicit. Encoding does not validate authority or module invariants.
//! Payloads use fixed-width little-endian bincode; outer LogicalRecord framing is
//! big-endian. Ordered maps/sets retain their declared order. Numeric serde display
//! adapters are excluded by the lifecycle views below. Changes require new vectors.
use crate::{
    ordinary_authority::DiscretionaryGrant,
    ordinary_meter::{LogicalField, LogicalRecord, LogicalValue, MeterError},
    recovery_fees::RecoveryAccount,
    runtime::{
        governance_state::{GovernanceState, STATE_KEY as GOVERNANCE_STATE_KEY},
        penalty_custody::{self, PenaltyState},
        reward_runtime::{RewardState, REWARD_STATE_KEY},
        validator_lifecycle::{
            self, LifecycleConfig, LifecycleState, PendingBond, ScheduledChange, UnbondEntry,
            ValidatorUpdate, ValidatorView,
        },
    },
    state::AccountState,
};
use bincode::Options;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

type Result<T> = std::result::Result<T, MeterError>;
fn fixed<T: Serialize + ?Sized>(value: &T, max: u32) -> Result<Vec<u8>> {
    if max == 0 {
        return Err(MeterError::Internal("logical bound must be positive"));
    }
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .with_limit(u64::from(max))
        .serialize(value)
        .map_err(|_| MeterError::Internal("typed logical payload encoding failed"))
}
fn field(id: u16, value: LogicalValue) -> LogicalField {
    LogicalField { id, value }
}
fn payload<T: Serialize + ?Sized>(key: &str, value: &T, max: u32) -> Result<LogicalRecord> {
    LogicalRecord::new(
        key.as_bytes(),
        &[field(1, LogicalValue::Bytes(fixed(value, max)?))],
        max,
    )
}
pub(crate) fn native_account(
    address: &str,
    account: &AccountState,
    max: u32,
) -> Result<LogicalRecord> {
    if account
        .balances
        .keys()
        .any(|denom| denom != "udgt" && denom != "udrt")
    {
        return Err(MeterError::Internal(
            "unsupported logical native denomination",
        ));
    }
    // Fee reservation can remove a zero udrt map entry. Always encode both native
    // denominations so restoring unused cap cannot change the logical write size.
    LogicalRecord::new(
        format!("acct:account:{address}").as_bytes(),
        &[
            field(1, LogicalValue::Utf8(address.into())),
            field(2, LogicalValue::U128(account.balance_of("udgt"))),
            field(3, LogicalValue::U128(account.balance_of("udrt"))),
            field(4, LogicalValue::U64(account.nonce)),
        ],
        max,
    )
}
pub(crate) fn recovery_account(
    id: &[u8; 32],
    account: &RecoveryAccount,
    max: u32,
) -> Result<LogicalRecord> {
    if id != &account.recovery.domain.account_id {
        return Err(MeterError::Internal("logical recovery account ID differs"));
    }
    payload(
        &format!("recovery:account:{}", hex::encode(id)),
        account,
        max,
    )
}
pub(crate) fn grant(
    id: &[u8; 32],
    grant: Option<&DiscretionaryGrant>,
    max: u32,
) -> Result<LogicalRecord> {
    let mut fields = vec![field(1, LogicalValue::U8(u8::from(grant.is_some())))];
    if let Some(grant) = grant {
        if id != &grant.owner || grant.version != 1 {
            return Err(MeterError::Internal(
                "logical grant identity or version differs",
            ));
        }
        fields.extend([
            field(2, LogicalValue::Digest(grant.owner)),
            field(3, LogicalValue::Digest(grant.beneficiary)),
            field(4, LogicalValue::U64(grant.owner_generation)),
            field(5, LogicalValue::U64(grant.period_blocks)),
            field(6, LogicalValue::U64(grant.last_active_height)),
        ]);
    }
    LogicalRecord::new(
        format!("ordinary:grant:{}", hex::encode(id)).as_bytes(),
        &fields,
        max,
    )
}
pub(crate) fn reward(state: &RewardState, max: u32) -> Result<LogicalRecord> {
    payload(REWARD_STATE_KEY, state, max)
}
pub(crate) fn lifecycle(state: &LifecycleState, max: u32) -> Result<LogicalRecord> {
    let view = LifecycleLogical {
        config: ConfigLogical::from(&state.config),
        effective: &state.effective,
        schedules: state
            .schedules
            .iter()
            .map(|(h, s)| (h, ScheduleLogical::from(s)))
            .collect(),
        unbonding: state
            .unbonding
            .iter()
            .map(|(id, u)| (id.as_str(), UnbondLogical::from(u)))
            .collect(),
        history: &state.history,
        update_history: &state.update_history,
        last_height: state.last_height,
        max_positions: state.max_positions,
        next_unbond_id: state.next_unbond_id,
        reserved_owners: &state.reserved_owners,
    };
    payload(validator_lifecycle::STATE_KEY, &view, max)
}
pub(crate) fn penalty(state: &PenaltyState, max: u32) -> Result<LogicalRecord> {
    payload(penalty_custody::STATE_KEY, state, max)
}
pub(crate) fn governance_state(state: &GovernanceState, max: u32) -> Result<LogicalRecord> {
    let bytes = state
        .encode()
        .map_err(|_| MeterError::Internal("governance logical state encoding failed"))?;
    LogicalRecord::new(
        GOVERNANCE_STATE_KEY.as_bytes(),
        &[field(1, LogicalValue::Bytes(bytes))],
        max,
    )
}
pub(crate) fn staking_pool(amount: u128, max: u32) -> Result<LogicalRecord> {
    LogicalRecord::new(
        b"emission:pool:staking_rewards",
        &[field(1, LogicalValue::U128(amount))],
        max,
    )
}

// This explicit view prevents LifecycleConfig's JSON decimal display adapter from
// making min_self_bond magnitude-dependent. Repeat it in every evidence snapshot.
#[derive(Serialize)]
struct ConfigLogical<'a> {
    version: u32,
    profile: &'a str,
    chain_id: &'a str,
    approved_operators: &'a BTreeMap<String, String>,
    min_self_bond: u128,
    max_active: usize,
    evidence_max_age_blocks: u64,
    evidence_max_age_seconds: u64,
    processing_margin_blocks: u64,
    processing_margin_seconds: u64,
}
impl<'a> From<&'a LifecycleConfig> for ConfigLogical<'a> {
    fn from(c: &'a LifecycleConfig) -> Self {
        Self {
            version: c.version,
            profile: &c.profile,
            chain_id: &c.chain_id,
            approved_operators: &c.approved_operators,
            min_self_bond: c.min_self_bond,
            max_active: c.max_active,
            evidence_max_age_blocks: c.evidence_max_age_blocks,
            evidence_max_age_seconds: c.evidence_max_age_seconds,
            processing_margin_blocks: c.processing_margin_blocks,
            processing_margin_seconds: c.processing_margin_seconds,
        }
    }
}
#[derive(Serialize)]
struct UnbondLogical<'a> {
    id: &'a str,
    owner: &'a str,
    validator: &'a str,
    amount: u128,
    request_height: u64,
    effective_height: u64,
    last_exposure_height: Option<u64>,
    last_exposure_time_seconds: Option<u64>,
    evidence_config: ConfigLogical<'a>,
}
impl<'a> From<&'a UnbondEntry> for UnbondLogical<'a> {
    fn from(u: &'a UnbondEntry) -> Self {
        Self {
            id: &u.id,
            owner: &u.owner,
            validator: &u.validator,
            amount: u.amount,
            request_height: u.request_height,
            effective_height: u.effective_height,
            last_exposure_height: u.last_exposure_height,
            last_exposure_time_seconds: u.last_exposure_time_seconds,
            evidence_config: ConfigLogical::from(&u.evidence_config),
        }
    }
}
#[derive(Serialize)]
struct ScheduleLogical<'a> {
    request_height: u64,
    view: &'a ValidatorView,
    additions: &'a [PendingBond],
    removals: Vec<UnbondLogical<'a>>,
}
impl<'a> From<&'a ScheduledChange> for ScheduleLogical<'a> {
    fn from(s: &'a ScheduledChange) -> Self {
        Self {
            request_height: s.request_height,
            view: &s.view,
            additions: &s.additions,
            removals: s.removals.iter().map(UnbondLogical::from).collect(),
        }
    }
}
#[derive(Serialize)]
struct LifecycleLogical<'a> {
    config: ConfigLogical<'a>,
    effective: &'a ValidatorView,
    schedules: BTreeMap<&'a u64, ScheduleLogical<'a>>,
    unbonding: BTreeMap<&'a str, UnbondLogical<'a>>,
    history: &'a BTreeMap<u64, ValidatorView>,
    update_history: &'a BTreeMap<u64, Vec<ValidatorUpdate>>,
    last_height: u64,
    max_positions: usize,
    next_unbond_id: u64,
    reserved_owners: &'a BTreeSet<String>,
}

#[cfg(test)]
#[path = "ordinary_logical_tests.rs"]
mod tests;
