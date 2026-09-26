//! Retained, in-memory CheckTx reservations for one committed application head.
//!
//! The caller holds its admission mutex and supplies the current committed head
//! and validated configuration. It must run fresh signature, authority, expiry,
//! ownership and funding checks before every reserve call, including duplicates.
//! No queue result is an execution permit. This helper has no chain-state writes
//! and no external eviction route. Proposal/execution use their own fresh ledger.
use crate::{
    ordinary_reservations::{
        Eligibility, ReservationId, ReservationLedger, ReservationRequest, ReservationStatus,
    },
    ordinary_state::OrdinaryConfig,
};
use anyhow::{ensure, Context, Result};
use dytallix_protocol_types::{ordinary_fees, sha3_256};

#[derive(Clone, Debug, PartialEq, Eq)]
struct BoundQueue {
    head: String,
    config_digest: [u8; 32],
    ledger: ReservationLedger,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct OrdinaryAdmissionQueue {
    bound: Option<BoundQueue>,
}
impl OrdinaryAdmissionQueue {
    /// Reset only after a valid new context is available. A failed reset preserves
    /// the previous queue. Changed heads require the caller to recheck entries.
    pub(crate) fn reset(&mut self, head: &str, config: &OrdinaryConfig) -> Result<()> {
        ensure!(
            head.len() == 64
                && head
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "Admission queue requires a committed lowercase 32-byte app hash"
        );
        ensure!(
            config.version == 1,
            "Unsupported ordinary queue configuration"
        );
        let profile_digest = ordinary_fees::profile_digest(&config.fee_profile)?;
        let limits = config.queue_limits()?;
        let ledger = ReservationLedger::new(profile_digest, limits)?;
        // This local queue binding is not a protocol digest or an authority proof.
        let mut bytes = b"DYTALLIX/ORDINARY-ADMISSION-CONFIG\0".to_vec();
        bytes.extend_from_slice(&serde_json::to_vec(config)?);
        let config_digest = sha3_256(&bytes);
        if self
            .bound
            .as_ref()
            .is_some_and(|bound| bound.head == head && bound.config_digest == config_digest)
        {
            return Ok(());
        }
        self.bound = Some(BoundQueue {
            head: head.to_owned(),
            config_digest,
            ledger,
        });
        Ok(())
    }
    /// Fresh authentication belongs to the caller; cached entries cannot supply it.
    pub(crate) fn reserve(
        &mut self,
        request: &ReservationRequest,
        liquidity: &Eligibility,
    ) -> Result<ReservationStatus> {
        Ok(self
            .bound
            .as_mut()
            .context("Admission queue has no committed context")?
            .ledger
            .reserve(request, liquidity)?)
    }
    /// Trusted explicit removal. This does not charge fees or consume counters.
    pub(crate) fn evict(&mut self, id: ReservationId) -> Result<bool> {
        Ok(self
            .bound
            .as_mut()
            .context("Admission queue has no committed context")?
            .ledger
            .evict(id)?)
    }
    pub(crate) fn len(&self) -> usize {
        self.bound.as_ref().map_or(0, |bound| bound.ledger.len())
    }
}

#[cfg(test)]
#[path = "ordinary_admission_tests.rs"]
mod tests;
