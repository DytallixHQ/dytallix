//! Ordinary-v3 governance work meter. It has no execution permit, authority
//! check, database writer, or consensus route. Signature metering counts work;
//! the caller must verify the signature and current account state. This meter
//! borrows the same block counter used by ordinary-v2 and recovery work.

use crate::ordinary_meter::{
    LogicalRecord, MeterError as SharedMeterError, ResourceUsage as Usage, SharedBlockMeter,
};

use dytallix_protocol_types::{
    ordinary_fees_v3::FeeProfileV3,
    ordinary_v3::{self as v3, Action, SignedOrdinary},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MeterError {
    Rejected(&'static str),
    AcceptedOutOfGas,
    BlockCapacity,
    Internal(&'static str),
}

impl From<SharedMeterError> for MeterError {
    fn from(error: SharedMeterError) -> Self {
        match error {
            SharedMeterError::PreAcceptanceRejected(message) => Self::Rejected(message),
            SharedMeterError::AcceptedOutOfGas => Self::AcceptedOutOfGas,
            SharedMeterError::BlockCapacity => Self::BlockCapacity,
            SharedMeterError::Internal(message) => Self::Internal(message),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Validation,
    Accepted,
    Rejected,
    Exhausted,
    Fault,
}

/// The caller must supply verified logical state records in execution order.
/// This summary alone cannot authorize a fee charge, receipt, or nonce advance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Summary {
    pub used_gas: u64,
    pub metadata_gas: u64,
    pub accepted: bool,
    pub exhausted: bool,
    pub processed_actions: u16,
    pub action_count: u16,
}

/// This meter keeps its exclusive block borrow through `finish`. It cannot be
/// cloned, reset, or used again after rejection or exhaustion.
pub(crate) struct V3GovernanceMeter<'a> {
    block: &'a mut SharedBlockMeter,
    limit: u64,
    usage: Usage,
    base_gas: u64,
    write_gas: u64,
    metadata_gas: u64,
    metadata_price: u64,
    signature_price: u64,
    action_price: u64,
    read_price: u64,
    write_price: u64,
    read_keys: BTreeSet<Vec<u8>>,
    writes: BTreeMap<Vec<u8>, u64>,
    signature_counted: bool,
    action_counted: bool,
    phase: Phase,
}

impl<'a> V3GovernanceMeter<'a> {
    pub(crate) fn new(
        profile: &FeeProfileV3,
        signed: &SignedOrdinary,
        height: u64,
        block: &'a mut SharedBlockMeter,
    ) -> Result<Self, MeterError> {
        block
            .bind_governance_v3(profile)
            .map_err(MeterError::from)?;
        let wire_bytes = v3::encode(signed, &profile.limits())
            .map_err(|_| MeterError::Rejected("v3 envelope outside selected limits"))?
            .len() as u64;
        let body = &signed.body;
        let action_tag = match body.actions.as_slice() {
            [Action::GovernanceProposal { .. }] => 13,
            [Action::GovernanceDeposit { .. }] => 14,
            [Action::GovernanceVote { .. }] => 15,
            _ => return Err(MeterError::Rejected("v3 requires one governance action")),
        };
        let signature_price = profile
            .base
            .signature_costs
            .get(&body.key.algorithm)
            .copied()
            .ok_or(MeterError::Rejected("signature algorithm lacks price"))?;
        let action_price = profile
            .action_cost(action_tag)
            .map_err(|_| MeterError::Internal("governance action lacks price"))?;
        let mut meter = Self {
            block,
            limit: body.gas_limit,
            usage: Usage::default(),
            base_gas: 0,
            write_gas: 0,
            metadata_gas: 0,
            metadata_price: profile.base.receipt_metadata_cost,
            signature_price,
            action_price,
            read_price: profile.base.read_byte_cost,
            write_price: profile.base.write_byte_cost,
            read_keys: BTreeSet::new(),
            writes: BTreeMap::new(),
            signature_counted: false,
            action_counted: false,
            phase: Phase::Validation,
        };
        let initial = wire_bytes
            .checked_mul(profile.base.wire_byte_cost)
            .and_then(|value| value.checked_add(profile.base.transaction_overhead))
            .ok_or(MeterError::Internal("v3 initial gas overflow"))?;
        meter.update(initial, 0, wire_bytes, 0)?;
        profile
            .validate_signed_request(body, height)
            .map_err(|_| MeterError::Rejected("signed v3 profile or fee bound differs"))?;
        Ok(meter)
    }

    fn fault<T>(&mut self, error: MeterError) -> Result<T, MeterError> {
        self.phase = Phase::Fault;
        self.block.fault_governance_v3();
        Err(error)
    }

    fn update(
        &mut self,
        base: u64,
        metadata: u64,
        bytes: u64,
        signatures: u64,
    ) -> Result<(), MeterError> {
        if !matches!(self.phase, Phase::Validation | Phase::Accepted) {
            return self.fault(MeterError::Internal("terminal v3 meter cannot resume"));
        }
        let total = match base
            .checked_add(self.write_gas)
            .and_then(|value| value.checked_add(metadata))
        {
            Some(value) => value,
            None => return self.fault(MeterError::Internal("v3 gas addition overflow")),
        };
        let over = total > self.limit;
        let next = Usage {
            gas: if over && self.phase == Phase::Accepted {
                self.limit
            } else {
                total
            },
            bytes,
            signatures,
        };
        if let Err(error) = self.block.adjust_governance_v3(self.usage, next) {
            return self.fault(error.into());
        }
        self.usage = next;
        if over {
            if self.phase == Phase::Accepted {
                self.phase = Phase::Exhausted;
                return Err(MeterError::AcceptedOutOfGas);
            }
            self.phase = Phase::Rejected;
            return Err(MeterError::Rejected("validation gas exceeds signed limit"));
        }
        self.base_gas = base;
        self.metadata_gas = metadata;
        Ok(())
    }

    pub(crate) fn signature(&mut self) -> Result<(), MeterError> {
        if self.phase != Phase::Validation || self.signature_counted {
            return self.fault(MeterError::Internal("v3 signature phase differs"));
        }
        let Some(base) = self.base_gas.checked_add(self.signature_price) else {
            return self.fault(MeterError::Internal("v3 signature gas overflow"));
        };
        let Some(signatures) = self.usage.signatures.checked_add(1) else {
            return self.fault(MeterError::Internal("v3 signature count overflow"));
        };
        self.update(base, self.metadata_gas, self.usage.bytes, signatures)?;
        self.signature_counted = true;
        Ok(())
    }

    /// Charge one logical state read before a caller uses that state. A later
    /// read of the same logical key costs no additional gas in this transaction.
    pub(crate) fn read(&mut self, record: &LogicalRecord) -> Result<(), MeterError> {
        if !matches!(self.phase, Phase::Validation | Phase::Accepted) {
            return self.fault(MeterError::Internal("terminal v3 read"));
        }
        if self.read_keys.contains(record.key()) {
            return Ok(());
        }
        let Some(cost) = record.byte_len().checked_mul(self.read_price) else {
            return self.fault(MeterError::Internal("v3 read cost overflow"));
        };
        let Some(base) = self.base_gas.checked_add(cost) else {
            return self.fault(MeterError::Internal("v3 read gas overflow"));
        };
        self.update(
            base,
            self.metadata_gas,
            self.usage.bytes,
            self.usage.signatures,
        )?;
        self.read_keys.insert(record.key().to_vec());
        Ok(())
    }

    pub(crate) fn accept(&mut self) -> Result<(), MeterError> {
        if self.phase != Phase::Validation || !self.signature_counted {
            return self.fault(MeterError::Internal("v3 acceptance phase differs"));
        }
        self.update(
            self.base_gas,
            self.metadata_price,
            self.usage.bytes,
            self.usage.signatures,
        )?;
        self.phase = Phase::Accepted;
        Ok(())
    }

    pub(crate) fn action(&mut self) -> Result<(), MeterError> {
        if self.phase != Phase::Accepted || self.action_counted {
            return self.fault(MeterError::Internal("v3 action phase differs"));
        }
        let Some(base) = self.base_gas.checked_add(self.action_price) else {
            return self.fault(MeterError::Internal("v3 action gas overflow"));
        };
        self.update(
            base,
            self.metadata_gas,
            self.usage.bytes,
            self.usage.signatures,
        )?;
        self.action_counted = true;
        Ok(())
    }

    /// Charge the latest proposed logical representation. Replacing a proposal
    /// does not reset other measured work, and an accepted failure keeps gas.
    pub(crate) fn write(&mut self, record: &LogicalRecord) -> Result<(), MeterError> {
        if self.phase != Phase::Accepted || !self.action_counted {
            return self.fault(MeterError::Internal("v3 write before accepted action"));
        }
        let Some(next_cost) = record.byte_len().checked_mul(self.write_price) else {
            return self.fault(MeterError::Internal("v3 write cost overflow"));
        };
        let prior = self.writes.get(record.key()).copied().unwrap_or(0);
        let Some(proposed) = self
            .write_gas
            .checked_sub(prior)
            .and_then(|value| value.checked_add(next_cost))
        else {
            return self.fault(MeterError::Internal("v3 write gas overflow"));
        };
        self.write_gas = proposed;
        self.update(
            self.base_gas,
            self.metadata_gas,
            self.usage.bytes,
            self.usage.signatures,
        )?;
        self.writes.insert(record.key().to_vec(), next_cost);
        Ok(())
    }

    pub(crate) fn finish(self) -> Result<Summary, MeterError> {
        if self.phase == Phase::Fault || self.block.is_faulted() {
            return Err(MeterError::Internal("faulted v3 meter cannot settle"));
        }
        Ok(Summary {
            used_gas: self.usage.gas,
            metadata_gas: self.metadata_gas,
            accepted: matches!(self.phase, Phase::Accepted | Phase::Exhausted),
            exhausted: self.phase == Phase::Exhausted,
            processed_actions: u16::from(self.action_counted),
            action_count: 1,
        })
    }
}

#[cfg(test)]
#[path = "governance_v3_meter_tests.rs"]
mod tests;
