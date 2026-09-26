//! Ordered paid ordinary execution for the explicit combined local profile.
//! All mutable inputs are block-local checkpoints. Only an accepted result
//! publishes a new checkpoint. Internal failures require the caller to abort.
use crate::{
    ordinary_authority::{self, DiscretionaryGrant, Grants},
    ordinary_fee_settlement::{
        self as fees, FeeHistory, FeeReceipt, FinancialState, Outcome, PlanError,
    },
    ordinary_logical as logical,
    ordinary_meter::{MeterError, OrdinaryMeter, SharedBlockMeter},
    ordinary_reservations::{
        self as reservations, Asset, Denomination, Eligibility, ReservationRequest,
    },
    ordinary_validator::{self as validator, ValidatedValidatorAction, ValidatorError},
    recovery_fees::RecoveryBook,
    settlement::Settlement,
    state::AccountState,
};
use dytallix_protocol_types::{
    ordinary::{self, Action, SignedOrdinary},
    ordinary_fees::{self, FeeProfile},
};
use std::collections::{BTreeMap, BTreeSet};
type Result<T> = std::result::Result<T, PlanError>;
const MAX_LOGICAL_BYTES: u32 = 64 * 1024 * 1024;
#[derive(Clone, Copy, Debug)]
pub(crate) struct RuntimeLimits {
    pub max_receipts: usize,
    pub max_retained_profiles: usize,
    pub max_grants: usize,
}
#[derive(Clone, Debug)]
pub(crate) struct OrdinaryExecutionResult {
    pub success: bool,
    pub accepted: bool,
    pub gas_used: u64,
    pub fee: u128,
    pub error: Option<String>,
    pub receipt: Option<FeeReceipt>,
    pub reservation: Option<ReservationRequest>,
}
fn internal(e: impl std::fmt::Display) -> PlanError {
    PlanError::Internal(e.to_string())
}
fn reject(e: impl std::fmt::Display) -> PlanError {
    PlanError::Rejected(e.to_string())
}
fn denied(gas: u64, e: impl std::fmt::Display) -> OrdinaryExecutionResult {
    OrdinaryExecutionResult {
        success: false,
        accepted: false,
        gas_used: gas,
        fee: 0,
        error: Some(e.to_string()),
        receipt: None,
        reservation: None,
    }
}
fn address(book: &RecoveryBook, id: &[u8; 32]) -> Result<String> {
    book.accounts
        .get(&hex::encode(id))
        .map(|a| a.address.clone())
        .ok_or_else(|| reject("Unregistered ordinary account"))
}
fn asset(owner: [u8; 32], denomination: Denomination) -> Asset {
    Asset {
        owner,
        denomination,
    }
}
fn snapshot(settlement: &mut Settlement, book: &RecoveryBook) -> Result<FinancialState> {
    let mut state = FinancialState {
        balances: BTreeMap::new(),
        eligible: BTreeMap::new(),
        native_nonces: BTreeMap::new(),
        withheld_udrt: settlement.ordinary_fee_total().map_err(internal)?,
    };
    for a in book.accounts.values() {
        let id = a.recovery.domain.account_id;
        let account = settlement.account(&a.address).map_err(internal)?.clone();
        state.native_nonces.insert(a.address.clone(), account.nonce);
        for (d, n) in [(Denomination::Udgt, "udgt"), (Denomination::Udrt, "udrt")] {
            state.balances.insert(asset(id, d), account.balance_of(n));
            state.eligible.insert(
                asset(id, d),
                settlement
                    .ordinary_eligible(&a.address, n, true)
                    .map_err(internal)?,
            );
        }
    }
    Ok(state)
}
/// Check the ordered reservation, action, and fee transitions for one receipt.
/// The action checkpoint is kept only for a successful transaction. A paid
/// failure must have the reservation checkpoint before its fee is applied.
fn reconcile_receipt(
    before: &Settlement,
    reserved_accounts: &BTreeMap<String, AccountState>,
    reserved_fee_total: u128,
    action_accounts: &BTreeMap<String, AccountState>,
    action_fee_total: u128,
    after: &Settlement,
    book: &RecoveryBook,
    receipt: &FeeReceipt,
) -> Result<()> {
    let actor = book
        .accounts
        .get(&hex::encode(receipt.actor()))
        .ok_or_else(|| internal("Receipt actor has no registered account"))?
        .address
        .as_str();
    let cap = receipt.reserved_cap();
    let charge = receipt.charge();
    let before_fee_total = before.ordinary_fee_total().map_err(internal)?;
    if receipt.released_cap()
        != cap
            .checked_sub(charge)
            .ok_or_else(|| internal("Receipt charge exceeds cap"))?
        || before_fee_total != reserved_fee_total
        || before_fee_total != action_fee_total
        || before_fee_total.checked_add(charge)
            != Some(after.ordinary_fee_total().map_err(internal)?)
    {
        return Err(internal(
            "Ordinary receipt fee custody differs from transaction delta",
        ));
    }
    for registered in book.accounts.values() {
        let address = registered.address.as_str();
        let initial = before
            .accounts
            .get(address)
            .ok_or_else(|| internal("Ordinary before account missing"))?;
        let held = reserved_accounts
            .get(address)
            .ok_or_else(|| internal("Ordinary reserved account missing"))?;
        let applied = action_accounts
            .get(address)
            .ok_or_else(|| internal("Ordinary action account missing"))?;
        let final_account = after
            .accounts
            .get(address)
            .ok_or_else(|| internal("Ordinary final account missing"))?;
        let is_actor = address == actor;
        let mut expected_held = initial.clone();
        if is_actor {
            expected_held.set_balance(
                "udrt",
                initial
                    .balance_of("udrt")
                    .checked_sub(cap)
                    .ok_or_else(|| internal("Ordinary cap reservation exceeds balance"))?,
            );
        }
        if held.nonce != initial.nonce
            || held.balances != expected_held.balances
            || applied.nonce != initial.nonce
            || (receipt.outcome() != Outcome::Success
                && (applied.balances != held.balances || applied.nonce != held.nonce))
        {
            return Err(internal(
                "Ordinary receipt reservation or rollback differs from account delta",
            ));
        }
        let mut expected_final = applied.clone();
        if is_actor {
            expected_final.set_balance(
                "udrt",
                applied
                    .balance_of("udrt")
                    .checked_add(receipt.released_cap())
                    .ok_or_else(|| internal("Ordinary cap release exceeds balance range"))?,
            );
        }
        let expected_nonce = if is_actor {
            if initial.nonce != receipt.nonce_before() {
                return Err(internal("Ordinary receipt nonce predecessor differs"));
            }
            receipt.nonce_after()
        } else {
            initial.nonce
        };
        if final_account.balances != expected_final.balances
            || final_account.nonce != expected_nonce
            || (is_actor && initial.nonce.checked_add(1) != Some(expected_nonce))
        {
            return Err(internal("Ordinary receipt final account delta differs"));
        }
    }
    Ok(())
}
pub(crate) fn eligible_liquidity(
    settlement: &mut Settlement,
    book: &RecoveryBook,
) -> Result<Eligibility> {
    let total = snapshot(settlement, book)?.eligible;
    let mut unrestricted = BTreeMap::new();
    for a in book.accounts.values() {
        for (denomination, name) in [(Denomination::Udgt, "udgt"), (Denomination::Udrt, "udrt")] {
            unrestricted.insert(
                asset(a.recovery.domain.account_id, denomination),
                settlement
                    .ordinary_eligible(&a.address, name, false)
                    .map_err(internal)?,
            );
        }
    }
    Ok(Eligibility {
        total,
        unrestricted,
    })
}
/// Apply recovery counter changes only when the previous combined mirror agrees.
pub(crate) fn sync_recovery_mirrors(
    before: &RecoveryBook,
    after: &RecoveryBook,
    settlement: &mut Settlement,
) -> Result<()> {
    if before.accounts.keys().ne(after.accounts.keys()) {
        return Err(internal("Recovery account registry changed"));
    }
    let mut next = settlement.clone();
    for (id, a) in &before.accounts {
        let b = &after.accounts[id];
        if a.address != b.address
            || next.account(&a.address).map_err(internal)?.nonce != a.recovery.spending_nonce
        {
            return Err(internal("Recovery native nonce mirror differs"));
        }
        next.account(&a.address).map_err(internal)?.nonce = b.recovery.spending_nonce;
    }
    *settlement = next;
    Ok(())
}
fn validator_action(a: &Action) -> bool {
    matches!(
        a,
        Action::RewardBond { .. }
            | Action::RewardBeginUnbond { .. }
            | Action::ValidatorRegister { .. }
            | Action::ValidatorRotateKey { .. }
            | Action::ValidatorExit { .. }
            | Action::ValidatorWithdraw { .. }
    )
}
fn meter_records(
    meter: &mut OrdinaryMeter<'_>,
    settlement: &mut Settlement,
    book: &RecoveryBook,
    grants: &Grants,
) -> Result<()> {
    for a in book.accounts.values() {
        meter.read(&logical::native_account(
            &a.address,
            settlement.account(&a.address).map_err(internal)?,
            MAX_LOGICAL_BYTES,
        )?)?;
        meter.read(&logical::recovery_account(
            &a.recovery.domain.account_id,
            a,
            MAX_LOGICAL_BYTES,
        )?)?;
        meter.read(&logical::grant(
            &a.recovery.domain.account_id,
            grants.get(&hex::encode(a.recovery.domain.account_id)),
            MAX_LOGICAL_BYTES,
        )?)?;
    }
    if let Some(s) = &settlement.rewards {
        meter.read(&logical::reward(s, MAX_LOGICAL_BYTES)?)?;
        meter.read(&logical::staking_pool(
            settlement.reward_pool().map_err(internal)?,
            MAX_LOGICAL_BYTES,
        )?)?;
    }
    if let Some(s) = &settlement.validators {
        meter.read(&logical::lifecycle(s, MAX_LOGICAL_BYTES)?)?;
    }
    if let Some(s) = &settlement.penalties {
        meter.read(&logical::penalty(s, MAX_LOGICAL_BYTES)?)?;
    }
    Ok(())
}
fn write_changes(
    meter: &mut OrdinaryMeter<'_>,
    before: &Settlement,
    after: &Settlement,
    old_grants: &Grants,
    new_grants: &Grants,
) -> std::result::Result<(), MeterError> {
    for (address, a) in &after.accounts {
        if before
            .accounts
            .get(address)
            .map(|old| (&old.balances, old.nonce))
            != Some((&a.balances, a.nonce))
        {
            meter.write(&logical::native_account(address, a, MAX_LOGICAL_BYTES)?)?;
        }
    }
    if before.rewards != after.rewards {
        if let Some(s) = &after.rewards {
            meter.write(&logical::reward(s, MAX_LOGICAL_BYTES)?)?;
            meter.write(&logical::staking_pool(
                after
                    .reward_pool()
                    .map_err(|_| MeterError::Internal("Reward pool read failed"))?,
                MAX_LOGICAL_BYTES,
            )?)?;
        }
    }
    if before.validators != after.validators {
        if let Some(s) = &after.validators {
            meter.write(&logical::lifecycle(s, MAX_LOGICAL_BYTES)?)?;
        }
    }
    if before.penalties != after.penalties {
        if let Some(s) = &after.penalties {
            meter.write(&logical::penalty(s, MAX_LOGICAL_BYTES)?)?;
        }
    }
    for id in old_grants
        .keys()
        .chain(new_grants.keys())
        .collect::<BTreeSet<_>>()
    {
        if old_grants.get(id) != new_grants.get(id) {
            let bytes: [u8; 32] = hex::decode(id)
                .map_err(|_| MeterError::Internal("Grant key invalid"))?
                .try_into()
                .map_err(|_| MeterError::Internal("Grant key length"))?;
            meter.write(&logical::grant(
                &bytes,
                new_grants.get(id),
                MAX_LOGICAL_BYTES,
            )?)?;
        }
    }
    Ok(())
}
fn protected_owner(book: &RecoveryBook, address: &str) -> Result<()> {
    let a = book
        .accounts
        .values()
        .find(|a| a.address == address)
        .ok_or_else(|| internal("Validator principal owner missing recovery account"))?;
    if !a.recovery.outgoing_allowed() {
        return Err(reject("Protected validator principal owner"));
    }
    Ok(())
}
/// Include cumulative self-unbond effects before acceptance. This projection only
/// resolves debit owners; it neither schedules operations nor checks paid rules.
fn principal_owners(
    book: &RecoveryBook,
    settlement: &Settlement,
    actor: &str,
    actions: &[Action],
) -> Result<()> {
    let Some(state) = &settlement.validators else {
        return Ok(());
    };
    let activation = book
        .last_height
        .checked_add(2)
        .ok_or_else(|| internal("Activation overflow"))?;
    let mut view = state
        .schedules
        .range(..=activation)
        .next_back()
        .map(|(_, s)| s.view.clone())
        .unwrap_or_else(|| state.effective.clone());
    for a in actions {
        match a {
            Action::RewardBond {
                validator_id,
                amount_udgt,
            }
            | Action::ValidatorRegister {
                validator_id,
                amount_udgt,
                ..
            } => {
                let old = view
                    .positions
                    .entry(actor.into())
                    .or_default()
                    .entry(validator_id.clone())
                    .or_default();
                *old = old
                    .checked_add(*amount_udgt)
                    .ok_or_else(|| reject("Requested principal overflow"))?;
            }
            Action::RewardBeginUnbond {
                validator_id,
                amount_udgt,
            } => {
                let p = view
                    .positions
                    .get(actor)
                    .and_then(|p| p.get(validator_id))
                    .copied()
                    .unwrap_or(0);
                if let Some(left) = p.checked_sub(*amount_udgt) {
                    if view
                        .validators
                        .get(validator_id)
                        .is_some_and(|v| v.owner == actor)
                        && left < state.config.min_self_bond
                    {
                        for (owner, positions) in &view.positions {
                            if positions.contains_key(validator_id) {
                                protected_owner(book, owner)?;
                            }
                        }
                    }
                    if let Some(positions) = view.positions.get_mut(actor) {
                        positions.insert(validator_id.clone(), left);
                    }
                }
            }
            Action::ValidatorExit { validator_id } => {
                for (owner, positions) in &view.positions {
                    if positions.contains_key(validator_id) {
                        protected_owner(book, owner)?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_signed(
    signed: &SignedOrdinary,
    profile: &FeeProfile,
    book: &mut RecoveryBook,
    grants: &mut Grants,
    history: &mut FeeHistory,
    settlement: &mut Settlement,
    height: u64,
    index: u32,
    limits: RuntimeLimits,
    block: &mut SharedBlockMeter,
) -> Result<OrdinaryExecutionResult> {
    execute(
        signed, profile, book, grants, history, settlement, height, index, limits, block, false,
    )
}
/// CheckTx is provisional. It checks current authority and eligible funding, but
/// does not predict the next mandatory interval or perform application effects.
#[allow(clippy::too_many_arguments)]
pub(crate) fn admission_only(
    signed: &SignedOrdinary,
    profile: &FeeProfile,
    book: &mut RecoveryBook,
    grants: &mut Grants,
    history: &mut FeeHistory,
    settlement: &mut Settlement,
    height: u64,
    index: u32,
    limits: RuntimeLimits,
    block: &mut SharedBlockMeter,
) -> Result<OrdinaryExecutionResult> {
    execute(
        signed, profile, book, grants, history, settlement, height, index, limits, block, true,
    )
}
#[allow(clippy::too_many_arguments)]
fn execute(
    signed: &SignedOrdinary,
    profile: &FeeProfile,
    book: &mut RecoveryBook,
    grants: &mut Grants,
    history: &mut FeeHistory,
    settlement: &mut Settlement,
    height: u64,
    index: u32,
    limits: RuntimeLimits,
    block: &mut SharedBlockMeter,
    admission: bool,
) -> Result<OrdinaryExecutionResult> {
    if limits.max_receipts == 0
        || limits.max_receipts > 65_536
        || limits.max_retained_profiles == 0
        || limits.max_retained_profiles > 65_536
        || limits.max_grants == 0
    {
        return Err(internal("Invalid explicit ordinary retention limits"));
    }
    let body = &signed.body;
    let gas_before = block.usage().gas;
    let mut meter = match OrdinaryMeter::new(profile, body, block) {
        Ok(m) => m,
        Err(MeterError::PreAcceptanceRejected(e)) => {
            return Ok(denied(
                block
                    .usage()
                    .gas
                    .checked_sub(gas_before)
                    .ok_or_else(|| internal("Block gas regressed"))?,
                e,
            ))
        }
        Err(e) => return Err(e.into()),
    };
    macro_rules! pre {
        ($value:expr) => {
            match $value {
                Ok(v) => v,
                Err(PlanError::Rejected(e)) => {
                    let g = meter.finish()?.used_gas();
                    return Ok(denied(g, e));
                }
                Err(e) => return Err(e),
            }
        };
    }
    pre!(meter.ordinary_signature().map_err(PlanError::from));
    let verified = pre!(
        dytallix_runtime_crypto::ordinary::verify_signed(signed, &profile.limits).map_err(|e|match e {dytallix_runtime_crypto::ordinary::OrdinaryVerificationError::BackendUnavailable=>internal(e),other=>reject(other)})
    );
    if body.ordinary_fee_contract_version != profile.ordinary_fee_contract_version
        || body.fee_profile_version != profile.version
        || body.fee_profile_digest != ordinary_fees::profile_digest(profile).map_err(internal)?
        || body.fee_denomination != profile.denomination
        || height < profile.activation_height
    {
        pre!(Err::<(), _>(reject(
            "Ordinary fee profile or activation mismatch"
        )));
    }
    history.validate()?;
    if history.receipts().count() > limits.max_receipts
        || history.profiles().len() > limits.max_retained_profiles
        || grants.len() > limits.max_grants
    {
        return Err(internal("Committed ordinary retention bound exceeded"));
    }
    if history.receipt(verified.transaction_id()).is_some() {
        pre!(Err::<(), _>(reject(
            "Ordinary transaction ID already accepted"
        )));
    }
    if history.receipts().count() >= limits.max_receipts {
        pre!(Err::<(), _>(reject(
            "Ordinary receipt retention capacity exhausted"
        )));
    }
    let mut baseline = settlement.clone();
    pre!(meter_records(&mut meter, &mut baseline, book, grants));
    let financial = snapshot(&mut baseline, book)?;
    ordinary_authority::validate_nonce_mirrors(book, &financial.native_nonces).map_err(internal)?;
    ordinary_authority::validate_grants(book, grants).map_err(internal)?;
    let assessment = pre!(ordinary_authority::check_verified_preacceptance(
        book,
        &financial.native_nonces,
        grants,
        height,
        &verified,
        &profile.limits
    )
    .map_err(reject));
    let profile_key = hex::encode(body.fee_profile_digest);
    if (!history.profiles().contains_key(&profile_key)
        && history.profiles().len() >= limits.max_retained_profiles)
        || assessment.prospective_grants.len() > limits.max_grants
    {
        pre!(Err::<(), _>(reject(
            "Ordinary retained profile or grant capacity exhausted"
        )));
    }
    let actor = address(book, &body.domain.account_id)?;
    let mut tokens: BTreeMap<usize, ValidatedValidatorAction> = BTreeMap::new();
    let mut ordered = Vec::new();
    for (i, a) in body.actions.iter().enumerate() {
        if let Action::ValidatorRegister { .. } | Action::ValidatorRotateKey { .. } = a {
            pre!(meter
                .validator_proof(i as u16, "mldsa65")
                .map_err(PlanError::from));
        }
        if validator_action(a) {
            let state = baseline
                .validators
                .as_ref()
                .ok_or_else(|| internal("Ordinary validator lifecycle unavailable"))?;
            let rewards = baseline
                .rewards
                .as_ref()
                .ok_or_else(|| internal("Ordinary reward lifecycle unavailable"))?;
            let checked = if admission {
                validator::precheck_admission_ordered(
                    state,
                    rewards,
                    &actor,
                    body.spending_nonce,
                    a,
                    height,
                    &ordered,
                )
            } else {
                validator::precheck_ordered(
                    state,
                    rewards,
                    &actor,
                    body.spending_nonce,
                    a,
                    height,
                    &ordered,
                )
            };
            let t = pre!(checked.map_err(|e| match e {
                ValidatorError::AuthorityRejected(s) => reject(s),
                other => internal(other),
            }));
            ordered.push(t.clone());
            tokens.insert(i, t);
        }
    }
    pre!(principal_owners(book, &baseline, &actor, &body.actions));
    let request = fees::reservation_request(&verified, profile, &financial)?;
    let required = pre!(reservations::calculate_requirements(
        request.payer,
        request.fee_cap_udrt,
        &request.action_debits,
        usize::from(profile.limits.max_actions) * 2,
    )
    .map_err(reject));
    for (asset, amount) in required {
        let available = financial.eligible.get(&asset).copied().unwrap_or(0);
        if pre!(amount.total().map_err(reject)) > available {
            pre!(Err::<(), _>(reject(
                "Insufficient current eligible funding for fee cap and actions"
            )));
        }
    }
    // Staking-enabled vesting permits principal bonding, not ordinary spending.
    // Check unrestricted debits separately from the combined balance peak.
    let spending = &request.unrestricted_debits;
    let spending_requirements = pre!(reservations::calculate_requirements(
        request.payer,
        request.fee_cap_udrt,
        spending,
        usize::from(profile.limits.max_actions) * 2
    )
    .map_err(reject));
    for (asset, amount) in spending_requirements {
        let owner = address(book, &asset.owner)?;
        let denom = match asset.denomination {
            Denomination::Udgt => "udgt",
            Denomination::Udrt => "udrt",
        };
        let available = baseline
            .ordinary_eligible(&owner, denom, false)
            .map_err(internal)?;
        if pre!(amount.total().map_err(reject)) > available {
            pre!(Err::<(), _>(reject(
                "Ordinary spending exceeds vested eligible liquidity"
            )));
        }
    }
    pre!(meter.accept().map_err(PlanError::from));
    if admission {
        let g = meter.finish()?.used_gas();
        return Ok(OrdinaryExecutionResult {
            success: true,
            accepted: true,
            gas_used: g,
            fee: 0,
            error: None,
            receipt: None,
            reservation: Some(request),
        });
    }
    let mut reserved = baseline.clone();
    debit(&mut reserved, &actor, "udrt", body.maximum_fee)?;
    let mut effects = reserved.clone();
    let mut next_grants = grants.clone();
    let mut claim_remaining = financial.balances.clone();
    let mut outcome = Outcome::Success;
    let mut failure = None;
    let parent_height = height
        .checked_sub(1)
        .ok_or_else(|| internal("Ordinary height zero"))?;
    let parent_seconds = if !tokens.is_empty() {
        crate::consensus_settlement::committed_parent_time(&baseline.storage, height)
            .map_err(internal)?
            .0
    } else {
        0
    };
    for (i, a) in body.actions.iter().enumerate() {
        match meter.action(i as u16) {
            Ok(()) => {}
            Err(MeterError::AcceptedOutOfGas) => {
                outcome = Outcome::OutOfGas;
                failure = Some((i as u16, "ACTION", "", "OUT_OF_GAS"));
                break;
            }
            Err(e) => return Err(e.into()),
        }
        let mut proposed = effects.clone();
        let mut proposed_grants = next_grants.clone();
        let applied = apply_action(
            a,
            tokens.get(&i),
            book,
            &actor,
            body.authorization_generation,
            height,
            parent_height,
            parent_seconds,
            &mut proposed,
            &mut proposed_grants,
            &mut claim_remaining,
        );
        match applied {
            Ok(()) => {}
            Err(ActionError::Rule(class, code)) => {
                outcome = Outcome::ApplicationFailure;
                failure = Some((i as u16, "APPLICATION", class, code));
                break;
            }
            Err(ActionError::Internal(e)) => return Err(internal(e)),
        }
        match write_changes(
            &mut meter,
            &effects,
            &proposed,
            &next_grants,
            &proposed_grants,
        ) {
            Ok(()) => {
                effects = proposed;
                next_grants = proposed_grants;
            }
            Err(MeterError::AcceptedOutOfGas) => {
                outcome = Outcome::OutOfGas;
                failure = Some((i as u16, "WRITE", "", "OUT_OF_GAS"));
                break;
            }
            Err(e) => return Err(e.into()),
        }
    }
    let summary = meter.finish()?;
    let (next_history, receipt) = fees::retain_runtime_receipt(
        history, &verified, profile, &summary, height, index, outcome, failure,
    )?;
    let reserved_accounts = reserved.accounts.clone();
    let reserved_fee_total = reserved.ordinary_fee_total().map_err(internal)?;
    if outcome != Outcome::Success {
        effects = reserved;
        next_grants = grants.clone();
    }
    let action_accounts = effects.accounts.clone();
    let action_fee_total = effects.ordinary_fee_total().map_err(internal)?;
    credit(&mut effects, &actor, "udrt", body.maximum_fee)?;
    effects
        .charge_sponsored(&actor, receipt.charge())
        .map_err(internal)?;
    effects.account(&actor).map_err(internal)?.nonce = receipt.nonce_after();
    reconcile_receipt(
        &baseline,
        &reserved_accounts,
        reserved_fee_total,
        &action_accounts,
        action_fee_total,
        &effects,
        book,
        &receipt,
    )?;
    let mut next_book = book.clone();
    next_book
        .accounts
        .get_mut(&hex::encode(body.domain.account_id))
        .ok_or_else(|| internal("Actor disappeared"))?
        .recovery
        .spending_nonce = receipt.nonce_after();
    ordinary_authority::validate_nonce_mirrors(
        &next_book,
        &snapshot(&mut effects, &next_book)?.native_nonces,
    )
    .map_err(internal)?;
    ordinary_authority::validate_grants(&next_book, &next_grants).map_err(internal)?;
    let result = OrdinaryExecutionResult {
        success: outcome == Outcome::Success,
        accepted: true,
        gas_used: receipt.gas_used(),
        fee: receipt.charge(),
        error: failure.map(|(_, _, _, code)| code.into()),
        receipt: Some(receipt),
        reservation: Some(request),
    };
    *settlement = effects;
    *book = next_book;
    *grants = next_grants;
    *history = next_history;
    Ok(result)
}
fn debit(s: &mut Settlement, a: &str, d: &str, n: u128) -> Result<()> {
    let old = s.account(a).map_err(internal)?.balance_of(d);
    let next = old
        .checked_sub(n)
        .ok_or_else(|| internal("Reserved action funds unavailable"))?;
    s.account(a).map_err(internal)?.set_balance(d, next);
    Ok(())
}
fn credit(s: &mut Settlement, a: &str, d: &str, n: u128) -> Result<()> {
    let old = s.account(a).map_err(internal)?.balance_of(d);
    let next = old
        .checked_add(n)
        .ok_or_else(|| internal("Native recipient arithmetic overflow"))?;
    s.account(a).map_err(internal)?.set_balance(d, next);
    Ok(())
}
fn transfer(s: &mut Settlement, from: &str, to: &str, d: &str, n: u128) -> Result<()> {
    if from == to {
        return Ok(());
    }
    debit(s, from, d, n)?;
    credit(s, to, d, n)
}
enum ActionError {
    Rule(&'static str, &'static str),
    Internal(String),
}
impl From<PlanError> for ActionError {
    fn from(e: PlanError) -> Self {
        Self::Internal(format!("{e:?}"))
    }
}
fn action_internal(e: impl std::fmt::Display) -> ActionError {
    ActionError::Internal(e.to_string())
}
#[allow(clippy::too_many_arguments)]
fn apply_action(
    a: &Action,
    token: Option<&ValidatedValidatorAction>,
    book: &RecoveryBook,
    actor: &str,
    generation: u64,
    height: u64,
    parent_height: u64,
    parent_seconds: u64,
    s: &mut Settlement,
    grants: &mut Grants,
    claim_remaining: &mut BTreeMap<Asset, u128>,
) -> std::result::Result<(), ActionError> {
    let actor_id = book
        .accounts
        .values()
        .find(|a| a.address == actor)
        .ok_or_else(|| action_internal("Actor missing"))?
        .recovery
        .domain
        .account_id;
    match a {
        Action::Send {
            recipient,
            denomination,
            amount,
        } => transfer(
            s,
            actor,
            &address(book, recipient)?,
            match denomination {
                ordinary::Denomination::Udgt => "udgt",
                ordinary::Denomination::Udrt => "udrt",
            },
            *amount,
        )?,
        Action::Data { .. } => {}
        Action::DmsRegister {
            beneficiary,
            period_blocks,
        } => {
            grants.insert(
                hex::encode(actor_id),
                DiscretionaryGrant {
                    version: 1,
                    owner: actor_id,
                    beneficiary: *beneficiary,
                    owner_generation: generation,
                    period_blocks: *period_blocks,
                    last_active_height: height,
                },
            );
        }
        Action::DmsPing => {
            grants
                .get_mut(&hex::encode(actor_id))
                .ok_or_else(|| action_internal("Grant disappeared"))?
                .last_active_height = height;
        }
        Action::DmsClaim { owner, .. } => {
            let g = grants
                .get(&hex::encode(owner))
                .ok_or_else(|| action_internal("Grant disappeared"))?;
            let deadline = g
                .last_active_height
                .checked_add(g.period_blocks)
                .ok_or_else(|| action_internal("Grant deadline overflow"))?;
            if height < deadline {
                return Err(ActionError::Rule(
                    "ACTION_STATE_PRECONDITION",
                    "DMS_INACTIVITY_DELAY",
                ));
            }
            let owner_address = address(book, owner)?;
            for (denomination, d) in [(Denomination::Udgt, "udgt"), (Denomination::Udrt, "udrt")] {
                let amount = s
                    .account(&owner_address)
                    .map_err(action_internal)?
                    .balance_of(d)
                    .min(
                        claim_remaining
                            .get(&asset(*owner, denomination))
                            .copied()
                            .unwrap_or(0),
                    );
                transfer(s, &owner_address, actor, d, amount)?;
                *claim_remaining
                    .entry(asset(*owner, denomination))
                    .or_default() -= amount;
            }
        }
        Action::RewardClaim => {
            s.reward_claim(actor).map_err(action_internal)?;
        }
        _ => {
            let token =
                token.ok_or_else(|| action_internal("Validator authority token missing"))?;
            let state = s
                .validators
                .as_ref()
                .ok_or_else(|| action_internal("Validator lifecycle missing"))?;
            let rewards = s
                .rewards
                .as_ref()
                .ok_or_else(|| action_internal("Reward lifecycle missing"))?;
            let plan = validator::apply(
                state,
                s.penalties.as_ref(),
                rewards,
                token,
                parent_height,
                parent_seconds,
            )
            .map_err(|e| match e {
                ValidatorError::ApplicationFailure(rule) => ActionError::Rule(
                    if rule.is_capacity() {
                        "ACTION_CAPACITY"
                    } else {
                        "ACTION_STATE_PRECONDITION"
                    },
                    rule.code(),
                ),
                other => action_internal(other),
            })?;
            if let Action::RewardBond { amount_udgt, .. }
            | Action::ValidatorRegister { amount_udgt, .. } = a
            {
                debit(s, actor, "udgt", *amount_udgt)?;
            }
            if let Some(amount) = plan.withdrawal_amount {
                credit(s, actor, "udgt", amount)?;
            }
            s.validators = Some(plan.lifecycle);
            s.penalties = plan.penalties;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "ordinary_execution_tests.rs"]
mod tests;
