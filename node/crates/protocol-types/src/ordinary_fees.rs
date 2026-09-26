//! Ordinary fee contract 1 profile codec. Every limit and price is explicit.
//! Profile syntax does not authenticate the validator-role profile or activate fees.
//! Activation must call `validate_validator_profile` with its committed role record.
use crate::ordinary::{self, Denomination, Limits};
use crate::recovery_wire::WireError;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};
use std::collections::{BTreeMap, BTreeSet};
pub const FORMAT_VERSION: u16 = 1;
pub const ORDINARY_FEE_CONTRACT_VERSION: u16 = 1;
pub const PROFILE_PREFIX: &[u8] = b"DYTALLIX/ORDINARY-FEE-PROFILE\0";
pub const MAX_PROFILE_BYTES: usize = 512;
type Result<T> = std::result::Result<T, WireError>;
fn need(ok: bool, message: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(WireError(message.into()))
    }
}
fn code(name: &str) -> Result<u16> {
    match name {
        "mldsa65" => Ok(1),
        "mldsa87" => Ok(2),
        _ => Err(WireError(
            "unsupported exact fee algorithm identifier".into(),
        )),
    }
}
fn name(code: u16) -> Result<&'static str> {
    match code {
        1 => Ok("mldsa65"),
        2 => Ok("mldsa87"),
        _ => Err(WireError("unsupported fee algorithm code".into())),
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeeProfile {
    pub ordinary_fee_contract_version: u16,
    #[serde(with = "decimal_u64")]
    pub version: u64,
    #[serde(with = "decimal_u64")]
    pub activation_height: u64,
    pub denomination: Denomination,
    #[serde(with = "decimal_u64")]
    pub gas_price: u64,
    #[serde(with = "decimal_u64")]
    pub minimum_gas: u64,
    #[serde(with = "decimal_u64")]
    pub max_transaction_gas: u64,
    #[serde(with = "decimal_u64")]
    pub max_block_transaction_gas: u64,
    #[serde(with = "decimal_u64")]
    pub max_block_transaction_bytes: u64,
    #[serde(with = "decimal_u64")]
    pub max_block_signature_checks: u64,
    #[serde(with = "decimal_u128")]
    pub max_fee_cap: u128,
    #[serde(with = "limits_view")]
    pub limits: Limits,
    #[serde(with = "decimal_u64")]
    pub transaction_overhead: u64,
    #[serde(with = "decimal_u64")]
    pub receipt_metadata_cost: u64,
    #[serde(with = "decimal_u64")]
    pub wire_byte_cost: u64,
    #[serde(with = "decimal_u64")]
    pub read_byte_cost: u64,
    #[serde(with = "decimal_u64")]
    pub write_byte_cost: u64,
    #[serde(with = "action_costs_view")]
    pub action_costs: [u64; 12],
    #[serde(with = "costs_view")]
    pub signature_costs: BTreeMap<String, u64>,
    pub validator_proof_profile_digest: [u8; 32],
    #[serde(with = "costs_view")]
    pub validator_proof_costs: BTreeMap<String, u64>,
}
impl FeeProfile {
    pub fn validate(&self) -> Result<()> {
        need(
            self.ordinary_fee_contract_version == ORDINARY_FEE_CONTRACT_VERSION,
            "unsupported ordinary fee contract",
        )?;
        need(
            self.denomination == Denomination::Udrt,
            "ordinary fees require udrt",
        )?;
        self.limits.validate()?;
        need(
            self.gas_price > 0 && self.max_transaction_gas > 0,
            "positive gas price and transaction gas required",
        )?;
        need(
            self.minimum_gas <= self.max_transaction_gas
                && self.max_block_transaction_gas >= self.max_transaction_gas,
            "inconsistent ordinary gas limits",
        )?;
        need(
            self.max_block_transaction_bytes >= u64::from(self.limits.max_wire_bytes)
                && self.max_block_signature_checks > 0
                && self.max_fee_cap > 0,
            "inconsistent ordinary capacity limits",
        )?;
        validate_costs(&self.signature_costs)?;
        validate_costs(&self.validator_proof_costs)?;
        need(
            self.signature_costs
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>()
                == self.limits.allowed_algorithms,
            "ordinary costs differ from account-role algorithms",
        )?;
        Ok(())
    }
    /// Check signed fee bounds. Input failures do not authorize a fee or nonce use.
    pub fn validate_request(&self, gas_limit: u64, maximum_fee: u128) -> Result<()> {
        self.validate()?;
        need(
            gas_limit > 0 && gas_limit <= self.max_transaction_gas && self.minimum_gas <= gas_limit,
            "signed gas limit outside ordinary profile",
        )?;
        let required = u128::from(gas_limit)
            .checked_mul(u128::from(self.gas_price))
            .ok_or_else(|| WireError("fee bound overflow".into()))?;
        need(
            required <= maximum_fee && maximum_fee <= self.max_fee_cap,
            "signed fee cap outside ordinary profile",
        )
    }
    /// Codec support is not permission to expand a validator role.
    /// The caller must supply the digest and exact algorithm set from that role's
    /// authenticated committed profile. Missing records must prevent activation.
    pub fn validate_validator_profile(
        &self,
        digest: [u8; 32],
        algorithms: &BTreeSet<String>,
    ) -> Result<()> {
        self.validate()?;
        need(
            self.validator_proof_profile_digest == digest,
            "validator proof profile digest differs",
        )?;
        need(
            self.validator_proof_costs
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>()
                == *algorithms,
            "validator proof costs differ from exact role algorithms",
        )
    }
}
fn validate_costs(costs: &BTreeMap<String, u64>) -> Result<()> {
    need(
        !costs.is_empty() && costs.len() <= 2,
        "explicit enabled algorithm costs required",
    )?;
    for algorithm in costs.keys() {
        code(algorithm)?;
    }
    Ok(())
}
pub fn profile_bytes(p: &FeeProfile) -> Result<Vec<u8>> {
    p.validate()?;
    let mut b = PROFILE_PREFIX.to_vec();
    b.extend_from_slice(&FORMAT_VERSION.to_be_bytes());
    b.extend_from_slice(&p.ordinary_fee_contract_version.to_be_bytes());
    b.extend_from_slice(&p.version.to_be_bytes());
    b.extend_from_slice(&p.activation_height.to_be_bytes());
    b.push(2);
    for v in [
        p.gas_price,
        p.minimum_gas,
        p.max_transaction_gas,
        p.max_block_transaction_gas,
        p.max_block_transaction_bytes,
        p.max_block_signature_checks,
    ] {
        b.extend_from_slice(&v.to_be_bytes());
    }
    b.extend_from_slice(&p.max_fee_cap.to_be_bytes());
    b.extend_from_slice(&p.limits.max_wire_bytes.to_be_bytes());
    b.extend_from_slice(&p.limits.max_actions.to_be_bytes());
    b.extend_from_slice(&p.limits.max_identifier_bytes.to_be_bytes());
    for v in [
        p.limits.max_data_bytes,
        p.limits.max_memo_bytes,
        p.limits.max_consensus_key_bytes,
        p.limits.max_proof_bytes,
    ] {
        b.extend_from_slice(&v.to_be_bytes());
    }
    b.extend_from_slice(&p.limits.max_expiry_lifetime.to_be_bytes());
    for v in [
        p.transaction_overhead,
        p.receipt_metadata_cost,
        p.wire_byte_cost,
        p.read_byte_cost,
        p.write_byte_cost,
    ] {
        b.extend_from_slice(&v.to_be_bytes());
    }
    for v in p.action_costs {
        b.extend_from_slice(&v.to_be_bytes());
    }
    append_costs(&mut b, &p.signature_costs)?;
    b.extend_from_slice(&p.validator_proof_profile_digest);
    append_costs(&mut b, &p.validator_proof_costs)?;
    need(
        b.len() <= MAX_PROFILE_BYTES,
        "ordinary fee profile exceeds codec bound",
    )?;
    Ok(b)
}
fn append_costs(b: &mut Vec<u8>, costs: &BTreeMap<String, u64>) -> Result<()> {
    b.push(u8::try_from(costs.len()).map_err(|_| WireError("algorithm count overflow".into()))?);
    for (algorithm, cost) in costs {
        b.extend_from_slice(&code(algorithm)?.to_be_bytes());
        b.extend_from_slice(&cost.to_be_bytes());
    }
    Ok(())
}
pub fn profile_digest(p: &FeeProfile) -> Result<[u8; 32]> {
    Ok(Sha3_256::digest(profile_bytes(p)?).into())
}
struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Reader<'a> {
    fn raw(&mut self, n: usize) -> Result<&'a [u8]> {
        need(
            n <= self.bytes.len().saturating_sub(self.offset),
            "truncated ordinary fee profile",
        )?;
        let bytes = &self.bytes[self.offset..self.offset + n];
        self.offset += n;
        Ok(bytes)
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
    fn costs(&mut self) -> Result<BTreeMap<String, u64>> {
        let count = self.u8()?;
        need(count > 0 && count <= 2, "invalid ordinary cost count")?;
        let mut costs = BTreeMap::new();
        let mut previous = 0;
        for _ in 0..count {
            let c = self.u16()?;
            need(c > previous, "fee algorithm codes are not strictly ordered")?;
            let n = name(c)?;
            costs.insert(n.into(), self.u64()?);
            previous = c;
        }
        Ok(costs)
    }
}
pub fn decode_profile(bytes: &[u8]) -> Result<FeeProfile> {
    need(
        bytes.len() <= MAX_PROFILE_BYTES,
        "ordinary fee profile exceeds codec bound",
    )?;
    let mut r = Reader { bytes, offset: 0 };
    need(
        r.raw(PROFILE_PREFIX.len())? == PROFILE_PREFIX,
        "ordinary fee prefix differs",
    )?;
    need(
        r.u16()? == FORMAT_VERSION,
        "unsupported ordinary fee format",
    )?;
    let ordinary_fee_contract_version = r.u16()?;
    let version = r.u64()?;
    let activation_height = r.u64()?;
    need(r.u8()? == 2, "ordinary fee denomination code differs")?;
    let mut p = FeeProfile {
        ordinary_fee_contract_version,
        version,
        activation_height,
        denomination: Denomination::Udrt,
        gas_price: r.u64()?,
        minimum_gas: r.u64()?,
        max_transaction_gas: r.u64()?,
        max_block_transaction_gas: r.u64()?,
        max_block_transaction_bytes: r.u64()?,
        max_block_signature_checks: r.u64()?,
        max_fee_cap: r.u128()?,
        limits: Limits {
            max_wire_bytes: r.u32()?,
            max_actions: r.u16()?,
            max_identifier_bytes: r.u16()?,
            max_data_bytes: r.u32()?,
            max_memo_bytes: r.u32()?,
            max_consensus_key_bytes: r.u32()?,
            max_proof_bytes: r.u32()?,
            max_expiry_lifetime: r.u64()?,
            allowed_algorithms: BTreeSet::new(),
        },
        transaction_overhead: r.u64()?,
        receipt_metadata_cost: r.u64()?,
        wire_byte_cost: r.u64()?,
        read_byte_cost: r.u64()?,
        write_byte_cost: r.u64()?,
        action_costs: [0; 12],
        signature_costs: BTreeMap::new(),
        validator_proof_profile_digest: [0; 32],
        validator_proof_costs: BTreeMap::new(),
    };
    for c in &mut p.action_costs {
        *c = r.u64()?;
    }
    p.signature_costs = r.costs()?;
    p.limits.allowed_algorithms = p.signature_costs.keys().cloned().collect();
    p.validator_proof_profile_digest = r.id()?;
    p.validator_proof_costs = r.costs()?;
    need(
        r.offset == bytes.len(),
        "trailing ordinary fee profile bytes",
    )?;
    need(
        profile_bytes(&p)? == bytes,
        "noncanonical ordinary fee profile",
    )?;
    Ok(p)
}
mod decimal_u64 {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        let text = String::deserialize(d)?;
        super::ordinary::parse_decimal_u64(&text).map_err(serde::de::Error::custom)
    }
}
mod decimal_u128 {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &u128, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u128, D::Error> {
        let text = String::deserialize(d)?;
        super::ordinary::parse_decimal_u128(&text).map_err(serde::de::Error::custom)
    }
}
mod action_costs_view {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S: Serializer>(v: &[u64; 12], s: S) -> Result<S::Ok, S::Error> {
        v.map(|n| n.to_string()).serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u64; 12], D::Error> {
        let raw = <[String; 12]>::deserialize(d)?;
        let mut values = [0; 12];
        for (i, text) in raw.iter().enumerate() {
            values[i] =
                super::ordinary::parse_decimal_u64(text).map_err(serde::de::Error::custom)?;
        }
        Ok(values)
    }
}
mod costs_view {
    use serde::{
        de::{MapAccess, Visitor},
        ser::SerializeMap,
        Deserializer, Serializer,
    };
    use std::{collections::BTreeMap, fmt};
    pub fn serialize<S: Serializer>(v: &BTreeMap<String, u64>, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(v.len()))?;
        for (name, cost) in v {
            map.serialize_entry(name, &cost.to_string())?;
        }
        map.end()
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<BTreeMap<String, u64>, D::Error> {
        struct Costs;
        impl<'de> Visitor<'de> for Costs {
            type Value = BTreeMap<String, u64>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("unique exact algorithm costs as decimal strings")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                let mut map = BTreeMap::new();
                while let Some((name, text)) = a.next_entry::<String, String>()? {
                    let cost = super::ordinary::parse_decimal_u64(&text)
                        .map_err(serde::de::Error::custom)?;
                    if map.insert(name, cost).is_some() {
                        return Err(serde::de::Error::custom("duplicate cost algorithm"));
                    }
                }
                Ok(map)
            }
        }
        d.deserialize_map(Costs)
    }
}
mod limits_view {
    use super::{decimal_u64, Limits};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeSet;
    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct View {
        max_wire_bytes: u32,
        max_actions: u16,
        max_identifier_bytes: u16,
        max_data_bytes: u32,
        max_memo_bytes: u32,
        max_consensus_key_bytes: u32,
        max_proof_bytes: u32,
        #[serde(with = "decimal_u64")]
        max_expiry_lifetime: u64,
        allowed_algorithms: BTreeSet<String>,
    }
    pub fn serialize<S: Serializer>(v: &Limits, s: S) -> Result<S::Ok, S::Error> {
        View {
            max_wire_bytes: v.max_wire_bytes,
            max_actions: v.max_actions,
            max_identifier_bytes: v.max_identifier_bytes,
            max_data_bytes: v.max_data_bytes,
            max_memo_bytes: v.max_memo_bytes,
            max_consensus_key_bytes: v.max_consensus_key_bytes,
            max_proof_bytes: v.max_proof_bytes,
            max_expiry_lifetime: v.max_expiry_lifetime,
            allowed_algorithms: v.allowed_algorithms.clone(),
        }
        .serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Limits, D::Error> {
        let v = View::deserialize(d)?;
        Ok(Limits {
            max_wire_bytes: v.max_wire_bytes,
            max_actions: v.max_actions,
            max_identifier_bytes: v.max_identifier_bytes,
            max_data_bytes: v.max_data_bytes,
            max_memo_bytes: v.max_memo_bytes,
            max_consensus_key_bytes: v.max_consensus_key_bytes,
            max_proof_bytes: v.max_proof_bytes,
            max_expiry_lifetime: v.max_expiry_lifetime,
            allowed_algorithms: v.allowed_algorithms,
        })
    }
}
