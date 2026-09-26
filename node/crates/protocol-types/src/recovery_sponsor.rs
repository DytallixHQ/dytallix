//! Canonical recovery sponsorship and explicit fee profiles, local format 1.
//! No default production price, algorithm selection, or activation is supplied.
//! Codec validity does not establish signatures, liquidity, or current authority.
use crate::recovery::{KeyIdentity, RecoveryDomain};
use crate::recovery_wire::{self, RecoveryOperation, SignatureRole, SignedRecovery, WireError};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};
use std::collections::BTreeMap;

pub const VERSION: u16 = 1;
pub const MAX_WIRE_BYTES: usize = 262_144;
pub const MAX_SPONSOR_BYTES: usize = 4_096;
pub const MAX_PROFILE_BYTES: usize = 512;
pub const SPONSOR_PREFIX: &[u8] = b"DYTALLIX/RECOVERY-SPONSOR\0";
pub const WIRE_PREFIX: &[u8] = b"DYTALLIX/RECOVERY-SPONSORED\0";
pub const PROFILE_PREFIX: &[u8] = b"DYTALLIX/RECOVERY-FEE-PROFILE\0";
type Result<T> = std::result::Result<T, WireError>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeeProfile {
    pub version: u64,
    pub activation_height: u64,
    pub denomination: String,
    pub gas_price: u64,
    pub minimum_gas: u64,
    pub max_transaction_gas: u64,
    pub max_block_gas: u64,
    pub max_block_recovery_bytes: u64,
    pub max_block_recovery_signatures: u64,
    pub max_fee_cap: u128,
    pub max_pending_accounts: u64,
    pub max_due_expiry_events_per_height: u64,
    pub mandatory_expiry_gas_budget: u64,
    pub expiry_event_gas_cost: u64,
    pub action_costs: [u64; 9],
    pub wire_byte_cost: u64,
    pub read_byte_cost: u64,
    pub write_byte_cost: u64,
    pub signature_costs: BTreeMap<String, u64>,
}
impl FeeProfile {
    pub fn validate(&self) -> Result<()> {
        denomination(&self.denomination)?;
        need(
            self.gas_price > 0 && self.max_transaction_gas > 0,
            "gas price and transaction capacity must be positive",
        )?;
        need(
            self.minimum_gas <= self.max_transaction_gas
                && self.max_block_gas >= self.max_transaction_gas,
            "inconsistent gas bounds",
        )?;
        need(
            self.max_block_recovery_bytes > 0
                && self.max_block_recovery_signatures > 0
                && self.max_fee_cap > 0
                && self.max_pending_accounts > 0
                && self.max_due_expiry_events_per_height > 0
                && self.mandatory_expiry_gas_budget > 0,
            "capacity bounds must be positive",
        )?;
        need(
            !self.signature_costs.is_empty() && self.signature_costs.len() <= 2,
            "explicit enabled signature costs required",
        )?;
        for name in self.signature_costs.keys() {
            algorithm(name)?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SponsorAuthorization {
    pub domain: RecoveryDomain,
    pub recovery_version: u16,
    pub operation_id: [u8; 32],
    pub signer_manifest_digest: [u8; 32],
    pub sponsor_account_id: [u8; 32],
    pub sponsor_generation: u64,
    pub sponsor_nonce: u64,
    pub sponsor_key: KeyIdentity,
    pub fee_profile_version: u64,
    pub fee_profile_digest: [u8; 32],
    pub denomination: String,
    pub maximum_charge: u128,
    pub gas_limit: u64,
    pub expiry_height: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SponsoredRecovery {
    pub recovery: SignedRecovery,
    pub sponsor: SponsorAuthorization,
    pub signature: Vec<u8>,
}
fn need(ok: bool, reason: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(WireError(reason.into()))
    }
}
fn hash(bytes: &[u8]) -> [u8; 32] {
    Sha3_256::digest(bytes).into()
}
fn algorithm(name: &str) -> Result<(u8, usize, usize)> {
    match name {
        "mldsa65" => Ok((1, 1952, 3309)),
        "mldsa87" => Ok((2, 2592, 4627)),
        _ => Err(WireError("unsupported exact algorithm identifier".into())),
    }
}
pub fn signature_size(name: &str) -> Result<usize> {
    Ok(algorithm(name)?.2)
}
fn denomination(value: &str) -> Result<()> {
    need(
        !value.is_empty() && value.len() <= 16 && value.bytes().all(|b| b.is_ascii_lowercase()),
        "invalid denomination encoding",
    )?;
    need(value == "udrt", "unsupported recovery fee denomination")
}
fn string(out: &mut Vec<u8>, value: &str, maximum: usize) -> Result<()> {
    need(
        !value.is_empty() && value.len() <= maximum,
        "string length outside bounds",
    )?;
    out.extend_from_slice(&(value.len() as u16).to_be_bytes());
    out.extend_from_slice(value.as_bytes());
    Ok(())
}
fn key(out: &mut Vec<u8>, value: &KeyIdentity) -> Result<()> {
    let (code, length, _) = algorithm(&value.algorithm)?;
    need(
        value.public_key.len() == length,
        "public key length differs from algorithm",
    )?;
    out.push(code);
    out.extend_from_slice(&(length as u16).to_be_bytes());
    out.extend_from_slice(&value.public_key);
    Ok(())
}
fn prefix(value: &[u8]) -> Vec<u8> {
    let mut out = value.to_vec();
    out.extend_from_slice(&VERSION.to_be_bytes());
    out
}
/// Intent excludes signatures and sponsorship. It includes submission expiry.
pub fn operation_id(value: &RecoveryOperation) -> Result<[u8; 32]> {
    let mut out = prefix(b"DYTALLIX/RECOVERY-INTENT\0");
    out.extend_from_slice(&recovery_wire::operation_bytes(value)?);
    Ok(hash(&out))
}
/// The canonical signer set is fixed without committing randomized signatures.
pub fn signer_manifest_digest(value: &SignedRecovery) -> Result<[u8; 32]> {
    recovery_wire::encode(value)?;
    let mut out = prefix(b"DYTALLIX/RECOVERY-SIGNERS\0");
    out.push(value.signatures.len() as u8);
    for entry in &value.signatures {
        out.push(match entry.role {
            SignatureRole::Operation => 0,
            SignatureRole::Possession => 1,
        });
        key(&mut out, &entry.key)?;
    }
    Ok(hash(&out))
}
pub fn sponsor_signing_bytes(value: &SponsorAuthorization) -> Result<Vec<u8>> {
    need(
        (1..=3).contains(&value.domain.network),
        "unsupported network code",
    )?;
    need(
        value.recovery_version == recovery_wire::VERSION,
        "unsupported recovery version",
    )?;
    denomination(&value.denomination)?;
    let mut out = prefix(SPONSOR_PREFIX);
    out.push(value.domain.network);
    string(
        &mut out,
        &value.domain.chain_id,
        recovery_wire::MAX_STRING_BYTES,
    )?;
    out.extend_from_slice(&value.domain.genesis_digest);
    out.extend_from_slice(&value.domain.account_id);
    out.extend_from_slice(&value.recovery_version.to_be_bytes());
    out.extend_from_slice(&value.operation_id);
    out.extend_from_slice(&value.signer_manifest_digest);
    out.extend_from_slice(&value.sponsor_account_id);
    out.extend_from_slice(&value.sponsor_generation.to_be_bytes());
    out.extend_from_slice(&value.sponsor_nonce.to_be_bytes());
    key(&mut out, &value.sponsor_key)?;
    out.extend_from_slice(&value.fee_profile_version.to_be_bytes());
    out.extend_from_slice(&value.fee_profile_digest);
    string(&mut out, &value.denomination, 16)?;
    out.extend_from_slice(&value.maximum_charge.to_be_bytes());
    out.extend_from_slice(&value.gas_limit.to_be_bytes());
    out.extend_from_slice(&value.expiry_height.to_be_bytes());
    need(
        out.len() <= MAX_SPONSOR_BYTES,
        "sponsor message exceeds limit",
    )?;
    Ok(out)
}
pub fn authorization_id(value: &SponsorAuthorization) -> Result<[u8; 32]> {
    Ok(hash(&sponsor_signing_bytes(value)?))
}
pub fn encode(value: &SponsoredRecovery) -> Result<Vec<u8>> {
    need(
        value.sponsor.domain == value.recovery.operation.domain,
        "sponsor domain differs from operation",
    )?;
    need(
        value.sponsor.operation_id == operation_id(&value.recovery.operation)?,
        "sponsor intent differs",
    )?;
    need(
        value.sponsor.signer_manifest_digest == signer_manifest_digest(&value.recovery)?,
        "sponsor manifest differs",
    )?;
    let inner = recovery_wire::encode(&value.recovery)?;
    let message = sponsor_signing_bytes(&value.sponsor)?;
    need(
        value.signature.len() == signature_size(&value.sponsor.sponsor_key.algorithm)?,
        "sponsor signature length differs",
    )?;
    let mut out = prefix(WIRE_PREFIX);
    out.extend_from_slice(&(inner.len() as u32).to_be_bytes());
    out.extend_from_slice(&inner);
    out.extend_from_slice(&(message.len() as u32).to_be_bytes());
    out.extend_from_slice(&message);
    out.extend_from_slice(&(value.signature.len() as u16).to_be_bytes());
    out.extend_from_slice(&value.signature);
    need(out.len() <= MAX_WIRE_BYTES, "sponsored wire exceeds limit")?;
    Ok(out)
}
pub fn envelope_hash(value: &SponsoredRecovery) -> Result<[u8; 32]> {
    let mut out = b"DYTALLIX/RECOVERY-SPONSORED-HASH\0".to_vec();
    out.extend_from_slice(&encode(value)?);
    Ok(hash(&out))
}
pub fn profile_bytes(value: &FeeProfile) -> Result<Vec<u8>> {
    value.validate()?;
    let mut out = prefix(PROFILE_PREFIX);
    out.extend_from_slice(&value.version.to_be_bytes());
    out.extend_from_slice(&value.activation_height.to_be_bytes());
    string(&mut out, &value.denomination, 16)?;
    for n in [
        value.gas_price,
        value.minimum_gas,
        value.max_transaction_gas,
        value.max_block_gas,
        value.max_block_recovery_bytes,
        value.max_block_recovery_signatures,
    ] {
        out.extend_from_slice(&n.to_be_bytes());
    }
    out.extend_from_slice(&value.max_fee_cap.to_be_bytes());
    for n in [
        value.max_pending_accounts,
        value.max_due_expiry_events_per_height,
        value.mandatory_expiry_gas_budget,
        value.expiry_event_gas_cost,
    ] {
        out.extend_from_slice(&n.to_be_bytes());
    }
    for n in value.action_costs {
        out.extend_from_slice(&n.to_be_bytes());
    }
    for n in [
        value.wire_byte_cost,
        value.read_byte_cost,
        value.write_byte_cost,
    ] {
        out.extend_from_slice(&n.to_be_bytes());
    }
    out.push(value.signature_costs.len() as u8);
    for (name, cost) in &value.signature_costs {
        out.push(algorithm(name)?.0);
        out.extend_from_slice(&cost.to_be_bytes());
    }
    Ok(out)
}
pub fn profile_digest(value: &FeeProfile) -> Result<[u8; 32]> {
    Ok(hash(&profile_bytes(value)?))
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Reader<'a> {
    fn raw(&mut self, length: usize) -> Result<&'a [u8]> {
        need(
            length <= self.bytes.len().saturating_sub(self.offset),
            "truncated sponsor wire",
        )?;
        let out = &self.bytes[self.offset..self.offset + length];
        self.offset += length;
        Ok(out)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.raw(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.raw(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.raw(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(self.raw(8)?.try_into().unwrap()))
    }
    fn u128(&mut self) -> Result<u128> {
        Ok(u128::from_be_bytes(self.raw(16)?.try_into().unwrap()))
    }
    fn id(&mut self) -> Result<[u8; 32]> {
        Ok(self.raw(32)?.try_into().unwrap())
    }
    fn string(&mut self, maximum: usize) -> Result<String> {
        let length = usize::from(self.u16()?);
        need(
            length > 0 && length <= maximum,
            "string length outside bounds",
        )?;
        Ok(std::str::from_utf8(self.raw(length)?)
            .map_err(|_| WireError("invalid UTF-8".into()))?
            .to_owned())
    }
    fn key(&mut self) -> Result<KeyIdentity> {
        let (name, length) = match self.u8()? {
            1 => ("mldsa65", 1952),
            2 => ("mldsa87", 2592),
            _ => return Err(WireError("unsupported algorithm code".into())),
        };
        need(
            usize::from(self.u16()?) == length,
            "public key length differs",
        )?;
        Ok(KeyIdentity {
            algorithm: name.into(),
            public_key: self.raw(length)?.to_vec(),
        })
    }
    fn check_prefix(&mut self, value: &[u8]) -> Result<()> {
        need(
            self.raw(value.len())? == value,
            "sponsor format prefix differs",
        )?;
        need(self.u16()? == VERSION, "unsupported sponsor format version")
    }
    fn finish(&self) -> Result<()> {
        need(self.offset == self.bytes.len(), "trailing sponsor bytes")
    }
}
pub fn decode_sponsor(bytes: &[u8]) -> Result<SponsorAuthorization> {
    need(
        bytes.len() <= MAX_SPONSOR_BYTES,
        "sponsor message exceeds limit",
    )?;
    let mut r = Reader { bytes, offset: 0 };
    r.check_prefix(SPONSOR_PREFIX)?;
    let value = SponsorAuthorization {
        domain: RecoveryDomain {
            network: r.u8()?,
            chain_id: r.string(recovery_wire::MAX_STRING_BYTES)?,
            genesis_digest: r.id()?,
            account_id: r.id()?,
        },
        recovery_version: r.u16()?,
        operation_id: r.id()?,
        signer_manifest_digest: r.id()?,
        sponsor_account_id: r.id()?,
        sponsor_generation: r.u64()?,
        sponsor_nonce: r.u64()?,
        sponsor_key: r.key()?,
        fee_profile_version: r.u64()?,
        fee_profile_digest: r.id()?,
        denomination: r.string(16)?,
        maximum_charge: r.u128()?,
        gas_limit: r.u64()?,
        expiry_height: r.u64()?,
    };
    r.finish()?;
    need(
        sponsor_signing_bytes(&value)? == bytes,
        "noncanonical sponsor message",
    )?;
    Ok(value)
}
pub fn decode(bytes: &[u8]) -> Result<SponsoredRecovery> {
    need(
        bytes.len() <= MAX_WIRE_BYTES,
        "sponsored wire exceeds limit",
    )?;
    let mut r = Reader { bytes, offset: 0 };
    r.check_prefix(WIRE_PREFIX)?;
    let inner_len =
        usize::try_from(r.u32()?).map_err(|_| WireError("inner length overflow".into()))?;
    need(
        inner_len <= recovery_wire::MAX_WIRE_BYTES,
        "inner wire exceeds limit",
    )?;
    let recovery = recovery_wire::decode(r.raw(inner_len)?)?;
    let message_len =
        usize::try_from(r.u32()?).map_err(|_| WireError("sponsor length overflow".into()))?;
    need(
        message_len <= MAX_SPONSOR_BYTES,
        "sponsor message exceeds limit",
    )?;
    let sponsor = decode_sponsor(r.raw(message_len)?)?;
    let signature_len = usize::from(r.u16()?);
    need(
        signature_len == signature_size(&sponsor.sponsor_key.algorithm)?,
        "sponsor signature length differs",
    )?;
    let signature = r.raw(signature_len)?.to_vec();
    r.finish()?;
    let value = SponsoredRecovery {
        recovery,
        sponsor,
        signature,
    };
    need(encode(&value)? == bytes, "noncanonical sponsored wire")?;
    Ok(value)
}
pub fn decode_profile(bytes: &[u8]) -> Result<FeeProfile> {
    need(
        bytes.len() <= MAX_PROFILE_BYTES,
        "fee profile exceeds limit",
    )?;
    let mut r = Reader { bytes, offset: 0 };
    r.check_prefix(PROFILE_PREFIX)?;
    let mut value = FeeProfile {
        version: r.u64()?,
        activation_height: r.u64()?,
        denomination: r.string(16)?,
        gas_price: r.u64()?,
        minimum_gas: r.u64()?,
        max_transaction_gas: r.u64()?,
        max_block_gas: r.u64()?,
        max_block_recovery_bytes: r.u64()?,
        max_block_recovery_signatures: r.u64()?,
        max_fee_cap: r.u128()?,
        max_pending_accounts: r.u64()?,
        max_due_expiry_events_per_height: r.u64()?,
        mandatory_expiry_gas_budget: r.u64()?,
        expiry_event_gas_cost: r.u64()?,
        action_costs: [0; 9],
        wire_byte_cost: 0,
        read_byte_cost: 0,
        write_byte_cost: 0,
        signature_costs: BTreeMap::new(),
    };
    for cost in &mut value.action_costs {
        *cost = r.u64()?;
    }
    value.wire_byte_cost = r.u64()?;
    value.read_byte_cost = r.u64()?;
    value.write_byte_cost = r.u64()?;
    let count = r.u8()?;
    need(count > 0 && count <= 2, "invalid algorithm count")?;
    let mut previous = 0;
    for _ in 0..count {
        let code = r.u8()?;
        need(code > previous, "algorithm codes are not strictly ordered")?;
        let name = match code {
            1 => "mldsa65",
            2 => "mldsa87",
            _ => return Err(WireError("unsupported algorithm code".into())),
        };
        value.signature_costs.insert(name.into(), r.u64()?);
        previous = code;
    }
    r.finish()?;
    need(profile_bytes(&value)? == bytes, "noncanonical fee profile")?;
    Ok(value)
}
