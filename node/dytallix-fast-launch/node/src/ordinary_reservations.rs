//! OF03 queue accounting for a future combined ordinary/recovery admission path.
//!
//! Inputs are trusted planning data, not authority proofs. The caller must bind
//! the context, stable ID, counter, payer and every debit to authenticated intent.
//! It must obtain eligible balances from current state after vesting/custody checks.
//! Protection, grant generation and ownership checks belong to the authority
//! component. No result here permits execution or proves those checks occurred.
//! Execution must repeat authority and funding checks against staged state.
//! This module does not change the existing admission routes or chain state.
use std::collections::BTreeMap;

pub(crate) type Owner = [u8; 32];
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Denomination {
    Udgt,
    Udrt,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Asset {
    pub owner: Owner,
    pub denomination: Denomination,
}
pub(crate) type EligibleLiquidity = BTreeMap<Asset, u128>;
/// Both constraints use actual assets. Total includes liquid DGT whose lock
/// permits staking. Unrestricted excludes every lock that prohibits transfers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Eligibility {
    pub total: EligibleLiquidity,
    pub unrestricted: EligibleLiquidity,
}

/// IDs are canonical unsigned intent IDs, never randomized envelope hashes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ReservationId {
    Ordinary([u8; 32]),
    /// The fee-profile digest is part of v3 intent identity. Both ordinary
    /// versions still use the same actor spending-nonce namespace.
    OrdinaryV3 {
        transaction_id: [u8; 32],
        profile_digest: [u8; 32],
    },
    RecoverySponsorship([u8; 32]),
}
impl ReservationId {
    fn counter_namespace(self) -> u8 {
        match self {
            Self::Ordinary(_) | Self::OrdinaryV3 { .. } => 1,
            Self::RecoverySponsorship(_) => 2,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DebitKind {
    Outflow,
    SelfTransfer,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ActionDebit {
    pub asset: Asset,
    pub amount: u128,
    pub kind: DebitKind,
}

/// Queue resource inputs must all be explicit and positive. There is no default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct QueueLimits {
    pub max_entries: usize,
    pub max_wire_bytes: u64,
    pub max_signature_work: u64,
    pub max_action_debits_per_entry: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReservationRequest {
    pub context_digest: [u8; 32],
    pub id: ReservationId,
    /// Ordinary actor or recovery sponsor. The fee denomination is always udrt.
    pub payer: Owner,
    /// Ordinary spending nonce or independent recovery sponsor nonce.
    pub nonce: u64,
    pub fee_cap_udrt: u128,
    /// Ordered outgoing requirements only. Never include expected incoming funds.
    /// DmsClaim uses its actual owner's current authorized bound in each asset.
    pub action_debits: Vec<ActionDebit>,
    /// Ordered Send/Dms debit subset. The caller authenticates this classification.
    /// Fee funding is automatically included in both liquidity constraints.
    pub unrestricted_debits: Vec<ActionDebit>,
    pub wire_bytes: u64,
    pub signature_work: u64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ReservedAmounts {
    pub fee: u128,
    pub action: u128,
}
impl ReservedAmounts {
    pub(crate) fn total(self) -> Result<u128, ReservationError> {
        self.fee
            .checked_add(self.action)
            .ok_or(ReservationError::Overflow)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReservationStatus {
    Added,
    AlreadyReserved,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ReservationError {
    #[error("Reservation limits must be explicit and positive")]
    InvalidLimits,
    #[error("Reservation context mismatch")]
    ContextMismatch,
    #[error("Invalid reservation resource input or exhausted counter")]
    InvalidRequest,
    #[error("Recovery sponsorship cannot reserve ordinary action debits")]
    SponsorActionDebits,
    #[error("Unrestricted debits must be an ordered subset of total debits")]
    InvalidDebitSubset,
    #[error("Unrestricted eligibility exceeds total eligibility")]
    InvalidEligibility,
    #[error("Canonical intent ID has inconsistent reservation inputs")]
    IdentityMismatch,
    #[error("Counter is already reserved; explicit eviction is required")]
    NonceConflict,
    #[error("Queue resource capacity exceeded")]
    Capacity,
    #[error("Reservation arithmetic overflow")]
    Overflow,
    #[error("Current eligible liquidity does not cover all reservations")]
    InsufficientLiquidity,
    #[error("Reservation bookkeeping invariant failed")]
    Invariant,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    request: ReservationRequest,
    requirements: BTreeMap<Asset, ReservedAmounts>,
    unrestricted_requirements: BTreeMap<Asset, ReservedAmounts>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReservationLedger {
    context_digest: [u8; 32],
    limits: QueueLimits,
    entries: BTreeMap<ReservationId, Entry>,
    wire_bytes: u64,
    signature_work: u64,
}
impl ReservationLedger {
    pub(crate) fn new(
        context_digest: [u8; 32],
        limits: QueueLimits,
    ) -> Result<Self, ReservationError> {
        if limits.max_entries == 0
            || limits.max_wire_bytes == 0
            || limits.max_signature_work == 0
            || limits.max_action_debits_per_entry == 0
        {
            return Err(ReservationError::InvalidLimits);
        }
        Ok(Self {
            context_digest,
            limits,
            entries: BTreeMap::new(),
            wire_bytes: 0,
            signature_work: 0,
        })
    }
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
    pub(crate) fn resources(&self) -> (u64, u64) {
        (self.wire_bytes, self.signature_work)
    }
    pub(crate) fn reserved(&self, asset: Asset) -> Result<ReservedAmounts, ReservationError> {
        self.reserved_constraint(asset, false)
    }
    pub(crate) fn reserved_unrestricted(
        &self,
        asset: Asset,
    ) -> Result<ReservedAmounts, ReservationError> {
        self.reserved_constraint(asset, true)
    }
    fn reserved_constraint(
        &self,
        asset: Asset,
        unrestricted: bool,
    ) -> Result<ReservedAmounts, ReservationError> {
        let mut total = ReservedAmounts::default();
        for entry in self.entries.values() {
            if let Some(amount) = (if unrestricted {
                &entry.unrestricted_requirements
            } else {
                &entry.requirements
            })
            .get(&asset)
            {
                total.fee = total
                    .fee
                    .checked_add(amount.fee)
                    .ok_or(ReservationError::Overflow)?;
                total.action = total
                    .action
                    .checked_add(amount.action)
                    .ok_or(ReservationError::Overflow)?;
            }
        }
        total.total()?;
        Ok(total)
    }
    /// Failure leaves the complete ledger unchanged. Missing liquidity means zero.
    /// A duplicate returns the existing reservation, without changing its resources.
    /// This is deduplication only, not a current-state revalidation result.
    pub(crate) fn reserve(
        &mut self,
        request: &ReservationRequest,
        liquidity: &Eligibility,
    ) -> Result<ReservationStatus, ReservationError> {
        if request.context_digest != self.context_digest {
            return Err(ReservationError::ContextMismatch);
        }
        if request.wire_bytes == 0
            || request.signature_work == 0
            || request.fee_cap_udrt == 0
            || request.nonce == u64::MAX
        {
            return Err(ReservationError::InvalidRequest);
        }
        if request.action_debits.len() > self.limits.max_action_debits_per_entry
            || request.unrestricted_debits.len() > self.limits.max_action_debits_per_entry
        {
            return Err(ReservationError::Capacity);
        }
        if matches!(request.id, ReservationId::RecoverySponsorship(_))
            && (!request.action_debits.is_empty() || !request.unrestricted_debits.is_empty())
        {
            return Err(ReservationError::SponsorActionDebits);
        }
        let mut remaining = request.action_debits.as_slice();
        for debit in &request.unrestricted_debits {
            let index = remaining
                .iter()
                .position(|item| item == debit)
                .ok_or(ReservationError::InvalidDebitSubset)?;
            remaining = &remaining[index + 1..];
        }
        if let Some(old) = self.entries.get(&request.id) {
            return if old.request == *request {
                Ok(ReservationStatus::AlreadyReserved)
            } else {
                Err(ReservationError::IdentityMismatch)
            };
        }
        if self.entries.values().any(|entry| {
            entry.request.payer == request.payer
                && entry.request.nonce == request.nonce
                && entry.request.id.counter_namespace() == request.id.counter_namespace()
        }) {
            return Err(ReservationError::NonceConflict);
        }
        let count = self
            .entries
            .len()
            .checked_add(1)
            .ok_or(ReservationError::Overflow)?;
        let bytes = self
            .wire_bytes
            .checked_add(request.wire_bytes)
            .ok_or(ReservationError::Overflow)?;
        let signatures = self
            .signature_work
            .checked_add(request.signature_work)
            .ok_or(ReservationError::Overflow)?;
        if count > self.limits.max_entries
            || bytes > self.limits.max_wire_bytes
            || signatures > self.limits.max_signature_work
        {
            return Err(ReservationError::Capacity);
        }
        let requirements = calculate_requirements(
            request.payer,
            request.fee_cap_udrt,
            &request.action_debits,
            self.limits.max_action_debits_per_entry,
        )?;
        let unrestricted_requirements = calculate_requirements(
            request.payer,
            request.fee_cap_udrt,
            &request.unrestricted_debits,
            self.limits.max_action_debits_per_entry,
        )?;
        for (asset, unrestricted) in &liquidity.unrestricted {
            if *unrestricted > liquidity.total.get(asset).copied().unwrap_or(0) {
                return Err(ReservationError::InvalidEligibility);
            }
        }
        // Recheck every reserved asset against each fresh eligibility constraint.
        // Complete both checks before changing any retained queue state.
        check_liquidity(
            self.entries
                .values()
                .map(|entry| &entry.requirements)
                .chain(std::iter::once(&requirements)),
            &liquidity.total,
        )?;
        check_liquidity(
            self.entries
                .values()
                .map(|entry| &entry.unrestricted_requirements)
                .chain(std::iter::once(&unrestricted_requirements)),
            &liquidity.unrestricted,
        )?;
        self.entries.insert(
            request.id,
            Entry {
                request: request.clone(),
                requirements,
                unrestricted_requirements,
            },
        );
        self.wire_bytes = bytes;
        self.signature_work = signatures;
        Ok(ReservationStatus::Added)
    }
    /// Explicit queue eviction, not a fee refund or nonce update. Repetition is a no-op.
    pub(crate) fn evict(&mut self, id: ReservationId) -> Result<bool, ReservationError> {
        let Some(entry) = self.entries.get(&id) else {
            return Ok(false);
        };
        let bytes = self
            .wire_bytes
            .checked_sub(entry.request.wire_bytes)
            .ok_or(ReservationError::Invariant)?;
        let signatures = self
            .signature_work
            .checked_sub(entry.request.signature_work)
            .ok_or(ReservationError::Invariant)?;
        self.entries.remove(&id);
        self.wire_bytes = bytes;
        self.signature_work = signatures;
        Ok(true)
    }
}

fn check_liquidity<'a>(
    requirements: impl Iterator<Item = &'a BTreeMap<Asset, ReservedAmounts>>,
    liquidity: &EligibleLiquidity,
) -> Result<(), ReservationError> {
    let mut totals: BTreeMap<Asset, ReservedAmounts> = BTreeMap::new();
    for amounts in requirements {
        for (asset, amount) in amounts {
            let total = totals.entry(*asset).or_default();
            total.fee = total
                .fee
                .checked_add(amount.fee)
                .ok_or(ReservationError::Overflow)?;
            total.action = total
                .action
                .checked_add(amount.action)
                .ok_or(ReservationError::Overflow)?;
            if total.total()? > liquidity.get(asset).copied().unwrap_or(0) {
                return Err(ReservationError::InsufficientLiquidity);
            }
        }
    }
    Ok(())
}

/// Fresh funding calculation shared with the staged fee engine. This function
/// checks accounting only. The caller binds actions and balances to current authority.
/// Expected incoming funds have no representation in this interface.
pub(crate) fn calculate_requirements(
    payer: Owner,
    fee_cap_udrt: u128,
    actions: &[ActionDebit],
    max_action_debits: usize,
) -> Result<BTreeMap<Asset, ReservedAmounts>, ReservationError> {
    if max_action_debits == 0 {
        return Err(ReservationError::InvalidLimits);
    }
    if fee_cap_udrt == 0 {
        return Err(ReservationError::InvalidRequest);
    }
    if actions.len() > max_action_debits {
        return Err(ReservationError::Capacity);
    }
    let mut requirements: BTreeMap<Asset, ReservedAmounts> = BTreeMap::new();
    let mut outflows: BTreeMap<Asset, u128> = BTreeMap::new();
    for debit in actions {
        let net = outflows.get(&debit.asset).copied().unwrap_or(0);
        let peak = net
            .checked_add(debit.amount)
            .ok_or(ReservationError::Overflow)?;
        let amount = requirements.entry(debit.asset).or_default();
        amount.action = amount.action.max(peak);
        if debit.kind == DebitKind::Outflow {
            outflows.insert(debit.asset, peak);
        }
    }
    requirements
        .entry(Asset {
            owner: payer,
            denomination: Denomination::Udrt,
        })
        .or_default()
        .fee = fee_cap_udrt;
    for amount in requirements.values() {
        amount.total()?;
    }
    Ok(requirements)
}

#[cfg(test)]
#[path = "ordinary_reservations_tests.rs"]
mod tests;
