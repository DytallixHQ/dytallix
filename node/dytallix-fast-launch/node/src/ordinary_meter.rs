//! Deterministic ordinary fee-meter component. No execution or activation permit.
//! Call each increment before its corresponding operation. Signature/proof
//! methods count work; they do not authenticate it. The caller checks the signed
//! profile binding, current authority and proof role against committed state.
//!
//! Logical record v1: ASCII `DYTALLIX/ORDINARY-LOGICAL`, zero, u16be version 1,
//! u16be nonempty key length/key, u16be field count; strictly increasing u16be
//! field IDs, u8 type, value. Types: 1=u64be, 2=u128be, 3=u32be blob,
//! 4=u32be strict UTF-8, 5=32-byte digest, 6=u8. Counters use eight bytes and
//! balances sixteen, independent of magnitude. Tags and framing also count.
//! A logical key identifies one state record; adapters must use the same key for
//! its read and all proposed writes. Receipt/fee records use fixed metadata gas
//! instead of this encoding. Concrete state adapters must freeze field IDs.
//!
//! Reads count once. Writes count the latest proposed representation once, even
//! if its size shrinks. Replacing a proposed write is not a transaction rollback.
//! The meter is not Clone. Rejection/exhaustion is terminal; reverting an action
//! overlay must retain this meter and its proposed-write costs. This prevents a
//! failed action from erasing measured work. Shared counters survive meter drop.
use dytallix_protocol_types::{
    ordinary::{self, Action, OrdinaryTransaction, SignedOrdinary},
    ordinary_fees::{self, FeeProfile},
    ordinary_fees_v3::{self, FeeProfileV3},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MeterError {
    PreAcceptanceRejected(&'static str),
    AcceptedOutOfGas,
    BlockCapacity,
    Internal(&'static str),
}
impl std::fmt::Display for MeterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for MeterError {}
type Result<T> = std::result::Result<T, MeterError>;
fn add(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b)
        .ok_or(MeterError::Internal("meter addition overflow"))
}
fn mul(a: u64, b: u64) -> Result<u64> {
    a.checked_mul(b)
        .ok_or(MeterError::Internal("meter multiplication overflow"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LogicalValue {
    U64(u64),
    U128(u128),
    Bytes(Vec<u8>),
    Utf8(String),
    Digest([u8; 32]),
    U8(u8),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LogicalField {
    pub id: u16,
    pub value: LogicalValue,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LogicalRecord {
    key: Vec<u8>,
    bytes: Vec<u8>,
}
impl LogicalRecord {
    pub(crate) fn new(key: &[u8], fields: &[LogicalField], max_bytes: u32) -> Result<Self> {
        if key.is_empty() || key.len() > u16::MAX as usize || fields.len() > u16::MAX as usize {
            return Err(MeterError::Internal("logical record shape outside bounds"));
        }
        let mut bytes = b"DYTALLIX/ORDINARY-LOGICAL\0\0\x01".to_vec();
        bytes.extend_from_slice(&(key.len() as u16).to_be_bytes());
        bytes.extend_from_slice(key);
        bytes.extend_from_slice(&(fields.len() as u16).to_be_bytes());
        let mut previous = None;
        for field in fields {
            if previous.is_some_and(|id| field.id <= id) {
                return Err(MeterError::Internal("logical fields not strictly ordered"));
            }
            previous = Some(field.id);
            bytes.extend_from_slice(&field.id.to_be_bytes());
            match &field.value {
                LogicalValue::U64(v) => {
                    bytes.push(1);
                    bytes.extend_from_slice(&v.to_be_bytes());
                }
                LogicalValue::U128(v) => {
                    bytes.push(2);
                    bytes.extend_from_slice(&v.to_be_bytes());
                }
                LogicalValue::Bytes(v) => {
                    bytes.push(3);
                    append_blob(&mut bytes, v, max_bytes)?;
                }
                LogicalValue::Utf8(v) => {
                    bytes.push(4);
                    append_blob(&mut bytes, v.as_bytes(), max_bytes)?;
                }
                LogicalValue::Digest(v) => {
                    bytes.push(5);
                    bytes.extend_from_slice(v);
                }
                LogicalValue::U8(v) => {
                    bytes.push(6);
                    bytes.push(*v);
                }
            }
            if bytes.len() as u128 > u128::from(max_bytes) {
                return Err(MeterError::Internal("logical record exceeds bound"));
            }
        }
        if bytes.len() as u128 > u128::from(max_bytes) {
            return Err(MeterError::Internal("logical record exceeds bound"));
        }
        Ok(Self {
            key: key.to_vec(),
            bytes,
        })
    }
    pub(crate) fn canonical_bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub(crate) fn key(&self) -> &[u8] {
        &self.key
    }
    pub(crate) fn byte_len(&self) -> u64 {
        self.bytes.len() as u64
    }
}
fn append_blob(out: &mut Vec<u8>, value: &[u8], max: u32) -> Result<()> {
    let len = u32::try_from(value.len())
        .map_err(|_| MeterError::Internal("logical blob length overflow"))?;
    if out
        .len()
        .checked_add(4)
        .and_then(|v| v.checked_add(value.len()))
        .map_or(true, |v| v as u128 > u128::from(max))
    {
        return Err(MeterError::Internal("logical record exceeds bound"));
    }
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(value);
    Ok(())
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ResourceUsage {
    pub gas: u64,
    pub bytes: u64,
    pub signatures: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RecoveryCeilings {
    pub max_gas: u64,
    pub max_bytes: u64,
    pub max_signatures: u64,
    pub mandatory_expiry_gas: u64,
}
/// One block's cumulative counters. No reset or Clone permits rollback of work.
#[derive(Debug)]
pub(crate) struct SharedBlockMeter {
    limits: ResourceUsage,
    recovery_limits: RecoveryCeilings,
    usage: ResourceUsage,
    recovery: ResourceUsage,
    expiry: u64,
    faulted: bool,
    profile_digest: [u8; 32],
    governance_profile_digest: Option<[u8; 32]>,
}
impl SharedBlockMeter {
    pub(crate) fn new(profile: &FeeProfile, recovery: RecoveryCeilings) -> Result<Self> {
        profile
            .validate()
            .map_err(|_| MeterError::Internal("invalid ordinary fee profile"))?;
        if recovery.max_gas == 0
            || recovery.max_bytes == 0
            || recovery.max_signatures == 0
            || recovery.mandatory_expiry_gas == 0
        {
            return Err(MeterError::Internal("missing recovery ceilings"));
        }
        Ok(Self {
            limits: ResourceUsage {
                gas: profile.max_block_transaction_gas,
                bytes: profile.max_block_transaction_bytes,
                signatures: profile.max_block_signature_checks,
            },
            recovery_limits: recovery,
            usage: ResourceUsage::default(),
            recovery: ResourceUsage::default(),
            expiry: 0,
            faulted: false,
            profile_digest: ordinary_fees::profile_digest(profile)
                .map_err(|_| MeterError::Internal("invalid fee profile digest"))?,
            governance_profile_digest: None,
        })
    }
    pub(crate) fn usage(&self) -> ResourceUsage {
        self.usage
    }
    pub(crate) fn recovery_usage(&self) -> ResourceUsage {
        self.recovery
    }
    pub(crate) fn expiry_gas(&self) -> u64 {
        self.expiry
    }
    /// Bind v3 to the exact v2 profile that owns this block's shared limits.
    /// A different v3 profile in the same block invalidates the block meter.
    pub(crate) fn bind_governance_v3(&mut self, profile: &FeeProfileV3) -> Result<()> {
        if self.faulted {
            return Err(MeterError::Internal("block meter is faulted"));
        }
        let result = (|| {
            let base = ordinary_fees::profile_digest(&profile.base)
                .map_err(|_| MeterError::Internal("invalid v3 base fee profile"))?;
            let digest = ordinary_fees_v3::profile_digest(profile)
                .map_err(|_| MeterError::Internal("invalid v3 fee profile"))?;
            if base != self.profile_digest {
                return Err(MeterError::Internal("v3 base fee profile differs"));
            }
            if self
                .governance_profile_digest
                .is_some_and(|bound| bound != digest)
            {
                return Err(MeterError::Internal("v3 fee profile changed in block"));
            }
            self.governance_profile_digest = Some(digest);
            Ok(())
        })();
        if result.is_err() {
            self.faulted = true;
        }
        result
    }
    pub(crate) fn adjust_governance_v3(
        &mut self,
        old: ResourceUsage,
        new: ResourceUsage,
    ) -> Result<()> {
        if self.governance_profile_digest.is_none() {
            self.faulted = true;
            return Err(MeterError::Internal("v3 fee profile is not bound"));
        }
        self.adjust(old, new)
    }
    pub(crate) fn fault_governance_v3(&mut self) {
        self.faulted = true;
    }
    pub(crate) fn is_faulted(&self) -> bool {
        self.faulted
    }
    fn adjust(&mut self, old: ResourceUsage, new: ResourceUsage) -> Result<()> {
        if self.faulted {
            return Err(MeterError::Internal("block meter is faulted"));
        }
        let result = self.adjust_inner(old, new);
        if result.is_err() {
            self.faulted = true;
        }
        result
    }
    fn adjust_inner(&mut self, old: ResourceUsage, new: ResourceUsage) -> Result<()> {
        let change = |total: u64, old: u64, new: u64| {
            add(
                total
                    .checked_sub(old)
                    .ok_or(MeterError::Internal("resource ownership mismatch"))?,
                new,
            )
        };
        let next = ResourceUsage {
            gas: change(self.usage.gas, old.gas, new.gas)?,
            bytes: change(self.usage.bytes, old.bytes, new.bytes)?,
            signatures: change(self.usage.signatures, old.signatures, new.signatures)?,
        };
        if next.gas > self.limits.gas
            || next.bytes > self.limits.bytes
            || next.signatures > self.limits.signatures
        {
            return Err(MeterError::BlockCapacity);
        }
        self.usage = next;
        Ok(())
    }
    /// Feed incremental work from the recovery meter, including rejected requests.
    /// Recovery's per-transaction limits remain enforced by that existing meter.
    pub(crate) fn charge_recovery(&mut self, gas: u64, bytes: u64, signatures: u64) -> Result<()> {
        if self.faulted {
            return Err(MeterError::Internal("block meter is faulted"));
        }
        let result = self.charge_recovery_inner(gas, bytes, signatures);
        if result.is_err() {
            self.faulted = true;
        }
        result
    }
    fn charge_recovery_inner(&mut self, gas: u64, bytes: u64, signatures: u64) -> Result<()> {
        let next = ResourceUsage {
            gas: add(self.recovery.gas, gas)?,
            bytes: add(self.recovery.bytes, bytes)?,
            signatures: add(self.recovery.signatures, signatures)?,
        };
        if next.gas > self.recovery_limits.max_gas
            || next.bytes > self.recovery_limits.max_bytes
            || next.signatures > self.recovery_limits.max_signatures
        {
            return Err(MeterError::BlockCapacity);
        }
        self.adjust(
            ResourceUsage::default(),
            ResourceUsage {
                gas,
                bytes,
                signatures,
            },
        )?;
        self.recovery = next;
        Ok(())
    }
    pub(crate) fn charge_expiry(&mut self, gas: u64) -> Result<()> {
        if self.faulted {
            return Err(MeterError::Internal("block meter is faulted"));
        }
        let result = self.charge_expiry_inner(gas);
        if result.is_err() {
            self.faulted = true;
        }
        result
    }
    fn charge_expiry_inner(&mut self, gas: u64) -> Result<()> {
        let next = add(self.expiry, gas)?;
        if next > self.recovery_limits.mandatory_expiry_gas {
            return Err(MeterError::BlockCapacity);
        }
        self.expiry = next;
        Ok(())
    }
    /// Malformed ordinary framing has no transaction body or signed gas limit.
    /// The transport must already enforce its explicit input-size bound.
    pub(crate) fn rejected_wire(&mut self, bytes: u64, profile: &FeeProfile) -> Result<()> {
        if self.faulted {
            return Err(MeterError::Internal("block meter is faulted"));
        }
        let result = self.rejected_wire_inner(bytes, profile);
        if result.is_err() {
            self.faulted = true;
        }
        result
    }
    fn rejected_wire_inner(&mut self, bytes: u64, profile: &FeeProfile) -> Result<()> {
        profile
            .validate()
            .map_err(|_| MeterError::Internal("invalid ordinary fee profile"))?;
        if ordinary_fees::profile_digest(profile)
            .map_err(|_| MeterError::Internal("invalid profile digest"))?
            != self.profile_digest
        {
            return Err(MeterError::Internal("block fee profile differs"));
        }
        self.adjust(
            ResourceUsage::default(),
            ResourceUsage {
                gas: mul(bytes, profile.wire_byte_cost)?,
                bytes,
                signatures: 0,
            },
        )
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MeterSummary {
    used_gas: u64,
    gas_limit: u64,
    metadata_gas: u64,
    accepted: bool,
    exhausted: bool,
    processed_actions: u16,
    action_count: u16,
}
impl MeterSummary {
    pub(crate) fn used_gas(&self) -> u64 {
        self.used_gas
    }
    pub(crate) fn gas_limit(&self) -> u64 {
        self.gas_limit
    }
    pub(crate) fn metadata_gas(&self) -> u64 {
        self.metadata_gas
    }
    pub(crate) fn accepted(&self) -> bool {
        self.accepted
    }
    pub(crate) fn exhausted(&self) -> bool {
        self.exhausted
    }
    pub(crate) fn processed_actions(&self) -> u16 {
        self.processed_actions
    }
    pub(crate) fn action_count(&self) -> u16 {
        self.action_count
    }
}
/// Exclusive block borrow prevents copying or resetting a transaction's resource ownership.
pub(crate) struct OrdinaryMeter<'a> {
    block: &'a mut SharedBlockMeter,
    profile: FeeProfile,
    limit: u64,
    phase: Phase,
    algorithm: String,
    usage: ResourceUsage,
    base_gas: u64,
    write_gas: u64,
    metadata: u64,
    read_keys: BTreeSet<Vec<u8>>,
    writes: BTreeMap<Vec<u8>, u64>,
    actions: Vec<usize>,
    next_action: u16,
    proofs: Vec<u16>,
    next_proof: usize,
    signature: bool,
}
fn action_tag(action: &Action) -> usize {
    match action {
        Action::Send { .. } => 0,
        Action::Data { .. } => 1,
        Action::DmsRegister { .. } => 2,
        Action::DmsPing => 3,
        Action::DmsClaim { .. } => 4,
        Action::RewardBond { .. } => 5,
        Action::RewardBeginUnbond { .. } => 6,
        Action::RewardClaim => 7,
        Action::ValidatorRegister { .. } => 8,
        Action::ValidatorRotateKey { .. } => 9,
        Action::ValidatorExit { .. } => 10,
        Action::ValidatorWithdraw { .. } => 11,
    }
}
impl<'a> OrdinaryMeter<'a> {
    pub(crate) fn new(
        profile: &FeeProfile,
        body: &OrdinaryTransaction,
        block: &'a mut SharedBlockMeter,
    ) -> Result<Self> {
        profile
            .validate()
            .map_err(|_| MeterError::Internal("invalid ordinary fee profile"))?;
        if block.faulted
            || ordinary_fees::profile_digest(profile)
                .map_err(|_| MeterError::Internal("invalid profile digest"))?
                != block.profile_digest
        {
            return Err(MeterError::Internal(
                "block fee profile differs or block is faulted",
            ));
        }
        let signed = SignedOrdinary {
            body: body.clone(),
            signature: vec![
                0;
                ordinary::signature_size(&body.key.algorithm).map_err(|_| {
                    MeterError::PreAcceptanceRejected("unsupported account algorithm")
                })?
            ],
        };
        let bytes = ordinary::encode(&signed, &profile.limits)
            .map_err(|_| {
                MeterError::PreAcceptanceRejected("ordinary body outside selected limits")
            })?
            .len() as u64;
        let mut meter = Self {
            block,
            profile: profile.clone(),
            limit: body.gas_limit,
            phase: Phase::Validation,
            algorithm: body.key.algorithm.clone(),
            usage: ResourceUsage::default(),
            base_gas: 0,
            write_gas: 0,
            metadata: 0,
            read_keys: BTreeSet::new(),
            writes: BTreeMap::new(),
            actions: body.actions.iter().map(action_tag).collect(),
            next_action: 0,
            proofs: body
                .actions
                .iter()
                .enumerate()
                .filter_map(|(i, a)| {
                    matches!(
                        a,
                        Action::ValidatorRegister { .. } | Action::ValidatorRotateKey { .. }
                    )
                    .then_some(i as u16)
                })
                .collect(),
            next_proof: 0,
            signature: false,
        };
        let initial = add(
            mul(bytes, profile.wire_byte_cost)?,
            profile.transaction_overhead,
        )?;
        meter.update(initial, 0, 0, bytes, 0)?;
        if profile
            .validate_request(body.gas_limit, body.maximum_fee)
            .is_err()
        {
            return Err(MeterError::PreAcceptanceRejected(
                "invalid signed gas or cap",
            ));
        }
        Ok(meter)
    }
    fn live(&self) -> Result<()> {
        if matches!(self.phase, Phase::Validation | Phase::Accepted) {
            Ok(())
        } else {
            Err(MeterError::Internal("terminal meter cannot resume"))
        }
    }
    fn fault<T>(&mut self, error: MeterError) -> Result<T> {
        self.phase = Phase::Fault;
        Err(error)
    }
    fn update(
        &mut self,
        base: u64,
        writes: u64,
        metadata: u64,
        bytes: u64,
        signatures: u64,
    ) -> Result<()> {
        self.live()?;
        let total = match add(base, writes).and_then(|value| add(value, metadata)) {
            Ok(v) => v,
            Err(e) => return self.fault(e),
        };
        let over = total > self.limit;
        let gas = if over && self.phase == Phase::Accepted {
            self.limit
        } else {
            total
        };
        let next = ResourceUsage {
            gas,
            bytes,
            signatures,
        };
        if let Err(e) = self.block.adjust(self.usage, next) {
            return self.fault(e);
        }
        self.usage = next;
        if over {
            if self.phase == Phase::Accepted {
                self.phase = Phase::Exhausted;
                return Err(MeterError::AcceptedOutOfGas);
            }
            self.phase = Phase::Rejected;
            return Err(MeterError::PreAcceptanceRejected(
                "validation gas exceeds signed limit",
            ));
        }
        self.base_gas = base;
        self.write_gas = writes;
        self.metadata = metadata;
        Ok(())
    }
    pub(crate) fn ordinary_signature(&mut self) -> Result<()> {
        let result = self.ordinary_signature_inner();
        if matches!(
            &result,
            Err(MeterError::Internal(_) | MeterError::BlockCapacity)
        ) {
            self.phase = Phase::Fault;
            self.block.faulted = true;
        }
        result
    }
    fn ordinary_signature_inner(&mut self) -> Result<()> {
        self.live()?;
        if self.phase != Phase::Validation || self.signature {
            return self.fault(MeterError::Internal(
                "ordinary signature phase/order mismatch",
            ));
        }
        // The body key is fixed at construction. Its selected price is recorded below.
        let price = self.ordinary_signature_price()?;
        self.update(
            add(self.base_gas, price)?,
            self.write_gas,
            self.metadata,
            self.usage.bytes,
            add(self.usage.signatures, 1)?,
        )?;
        self.signature = true;
        Ok(())
    }
    fn ordinary_signature_price(&self) -> Result<u64> {
        // The exact body algorithm is fixed privately at construction.
        self.profile
            .signature_costs
            .get(self.account_algorithm())
            .copied()
            .ok_or(MeterError::Internal("ordinary signature cost missing"))
    }
    fn account_algorithm(&self) -> &str {
        &self.algorithm
    }
    pub(crate) fn validator_proof(&mut self, index: u16, algorithm: &str) -> Result<()> {
        let result = self.validator_proof_inner(index, algorithm);
        if matches!(
            &result,
            Err(MeterError::Internal(_) | MeterError::BlockCapacity)
        ) {
            self.phase = Phase::Fault;
            self.block.faulted = true;
        }
        result
    }
    fn validator_proof_inner(&mut self, index: u16, algorithm: &str) -> Result<()> {
        self.live()?;
        if self.phase != Phase::Validation
            || !self.signature
            || self.proofs.get(self.next_proof) != Some(&index)
        {
            return self.fault(MeterError::Internal("validator proof phase/order mismatch"));
        }
        let Some(price) = self.profile.validator_proof_costs.get(algorithm).copied() else {
            self.phase = Phase::Rejected;
            return Err(MeterError::PreAcceptanceRejected(
                "proof algorithm outside explicit profile",
            ));
        };
        self.update(
            add(self.base_gas, price)?,
            self.write_gas,
            self.metadata,
            self.usage.bytes,
            add(self.usage.signatures, 1)?,
        )?;
        self.next_proof += 1;
        Ok(())
    }
    pub(crate) fn read(&mut self, record: &LogicalRecord) -> Result<()> {
        let result = self.read_inner(record);
        if matches!(
            &result,
            Err(MeterError::Internal(_) | MeterError::BlockCapacity)
        ) {
            self.phase = Phase::Fault;
            self.block.faulted = true;
        }
        result
    }
    fn read_inner(&mut self, record: &LogicalRecord) -> Result<()> {
        self.live()?;
        if self.read_keys.contains(&record.key) {
            return Ok(());
        }
        self.update(
            add(
                self.base_gas,
                mul(record.byte_len(), self.profile.read_byte_cost)?,
            )?,
            self.write_gas,
            self.metadata,
            self.usage.bytes,
            self.usage.signatures,
        )?;
        self.read_keys.insert(record.key.clone());
        Ok(())
    }
    pub(crate) fn accept(&mut self) -> Result<()> {
        let result = self.accept_inner();
        if matches!(
            &result,
            Err(MeterError::Internal(_) | MeterError::BlockCapacity)
        ) {
            self.phase = Phase::Fault;
            self.block.faulted = true;
        }
        result
    }
    fn accept_inner(&mut self) -> Result<()> {
        self.live()?;
        if self.phase != Phase::Validation
            || !self.signature
            || self.next_proof != self.proofs.len()
        {
            return self.fault(MeterError::Internal(
                "acceptance before complete signature metering",
            ));
        }
        self.update(
            self.base_gas,
            self.write_gas,
            self.profile.receipt_metadata_cost,
            self.usage.bytes,
            self.usage.signatures,
        )?;
        self.phase = Phase::Accepted;
        Ok(())
    }
    pub(crate) fn action(&mut self, index: u16) -> Result<()> {
        let result = self.action_inner(index);
        if matches!(
            &result,
            Err(MeterError::Internal(_) | MeterError::BlockCapacity)
        ) {
            self.phase = Phase::Fault;
            self.block.faulted = true;
        }
        result
    }
    fn action_inner(&mut self, index: u16) -> Result<()> {
        self.live()?;
        if self.phase != Phase::Accepted
            || index != self.next_action
            || usize::from(index) >= self.actions.len()
        {
            return self.fault(MeterError::Internal("ordinary action phase/order mismatch"));
        }
        let price = self.profile.action_costs[self.actions[usize::from(index)]];
        self.update(
            add(self.base_gas, price)?,
            self.write_gas,
            self.metadata,
            self.usage.bytes,
            self.usage.signatures,
        )?;
        self.next_action += 1;
        Ok(())
    }
    pub(crate) fn write(&mut self, record: &LogicalRecord) -> Result<()> {
        let result = self.write_inner(record);
        if matches!(
            &result,
            Err(MeterError::Internal(_) | MeterError::BlockCapacity)
        ) {
            self.phase = Phase::Fault;
            self.block.faulted = true;
        }
        result
    }
    fn write_inner(&mut self, record: &LogicalRecord) -> Result<()> {
        self.live()?;
        if self.phase != Phase::Accepted || self.next_action == 0 {
            return self.fault(MeterError::Internal("write before accepted action"));
        }
        let old = self.writes.get(&record.key).copied().unwrap_or(0);
        let new = mul(record.byte_len(), self.profile.write_byte_cost)?;
        let proposed = add(
            self.write_gas
                .checked_sub(old)
                .ok_or(MeterError::Internal("write meter invariant"))?,
            new,
        )?;
        self.update(
            self.base_gas,
            proposed,
            self.metadata,
            self.usage.bytes,
            self.usage.signatures,
        )?;
        self.writes.insert(record.key.clone(), new);
        Ok(())
    }
    pub(crate) fn finish(self) -> Result<MeterSummary> {
        if self.phase == Phase::Fault || self.block.faulted {
            return Err(MeterError::Internal("cannot settle faulted meter"));
        }
        Ok(MeterSummary {
            used_gas: self.usage.gas,
            gas_limit: self.limit,
            metadata_gas: self.metadata,
            accepted: matches!(self.phase, Phase::Accepted | Phase::Exhausted),
            exhausted: self.phase == Phase::Exhausted,
            processed_actions: self.next_action,
            action_count: self.actions.len() as u16,
        })
    }
}
#[cfg(test)]
#[path = "ordinary_meter_tests.rs"]
mod tests;
