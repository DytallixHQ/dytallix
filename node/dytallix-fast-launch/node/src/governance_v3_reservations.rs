//! Funding request for an authenticated ordinary-v3 governance transaction.
//!
//! The verifier proves the signature. The caller must separately check current
//! authority, the committed parent, the account nonce, and custody eligibility.
//! This builder does not reserve funds, charge a fee, or activate v3 in consensus.

use crate::ordinary_reservations::{
    ActionDebit, Asset, DebitKind, Denomination, ReservationId, ReservationRequest,
};
use anyhow::{ensure, Context, Result};
use dytallix_protocol_types::{
    ordinary_fees_v3::{self, FeeProfileV3},
    ordinary_v3::{self as v3, Action, SignedOrdinary},
    sha3_256,
};
use dytallix_runtime_crypto::ordinary_v3::VerifiedOrdinaryV3;

/// Derive every debit from the signed body. The queue context is the caller's
/// committed-head configuration binding. A combined v2/v3 queue must use one
/// context for both versions. The v3 fee-profile digest remains in the ID.
pub(crate) fn reservation_request(
    signed: &SignedOrdinary,
    verified: &VerifiedOrdinaryV3,
    profile: &FeeProfileV3,
    queue_context_digest: [u8; 32],
    admission_height: u64,
) -> Result<ReservationRequest> {
    ensure!(
        signed.body == *verified.body(),
        "Governance reservation body differs from verified body"
    );
    let limits = profile.limits();
    let wire = v3::encode(signed, &limits)?;
    ensure!(
        sha3_256(&wire) == verified.envelope_hash()
            && v3::transaction_id(verified.body(), &limits)? == verified.transaction_id(),
        "Governance reservation differs from verified envelope"
    );
    let body = verified.body();
    profile.validate_signed_request(body, admission_height)?;
    ensure!(body.spending_nonce < u64::MAX, "Governance nonce exhausted");
    let profile_digest = ordinary_fees_v3::profile_digest(profile)?;
    ensure!(
        body.fee_profile_digest == profile_digest,
        "Governance fee profile differs from signed body"
    );
    let action_debits = match body.actions.as_slice() {
        [Action::GovernanceDeposit { amount_udgt, .. }] => {
            ensure!(*amount_udgt > 0, "Governance deposit is zero");
            vec![ActionDebit {
                asset: Asset {
                    owner: body.domain.account_id,
                    denomination: Denomination::Udgt,
                },
                amount: *amount_udgt,
                kind: DebitKind::Outflow,
            }]
        }
        [Action::GovernanceProposal { .. } | Action::GovernanceVote { .. }] => vec![],
        _ => anyhow::bail!("Ordinary-v3 requires exactly one governance action"),
    };
    let wire_bytes = u64::try_from(wire.len()).context("Governance wire length overflow")?;
    ensure!(wire_bytes > 0, "Governance wire is empty");
    Ok(ReservationRequest {
        context_digest: queue_context_digest,
        id: ReservationId::OrdinaryV3 {
            transaction_id: verified.transaction_id(),
            profile_digest,
        },
        payer: body.domain.account_id,
        nonce: body.spending_nonce,
        fee_cap_udrt: body.maximum_fee,
        unrestricted_debits: action_debits.clone(),
        action_debits,
        wire_bytes,
        signature_work: 1,
    })
}

#[cfg(test)]
#[path = "governance_v3_reservations_tests.rs"]
mod tests;
