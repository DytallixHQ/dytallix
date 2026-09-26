//! Versioned fee-profile format for the approved ordinary-v3 wire boundary.
//! Prices and activation height are explicit inputs. This codec does not select
//! production values or activate governance in consensus. The approved entry
//! rule admits exactly one governance action per v3 transaction.

use crate::{ordinary, ordinary_fees as v1, ordinary_v3 as v3, recovery_wire::WireError};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};

pub const FORMAT_VERSION: u16 = 1;
pub const ORDINARY_FEE_CONTRACT_VERSION: u16 = 2;
pub const PROFILE_PREFIX: &[u8] = b"DYTALLIX/ORDINARY-FEE-PROFILE-V3\0";
pub const MAX_PROFILE_BYTES: usize = 640;
type Result<T> = std::result::Result<T, WireError>;

fn need(ok: bool, message: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(WireError(message.into()))
    }
}

/// The first twelve action costs are inherited from the exact committed v1
/// profile. The three explicit extension costs cover v3 tags 13, 14, and 15.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeeProfileV3 {
    pub base: v1::FeeProfile,
    #[serde(with = "decimal_u64")]
    pub version: u64,
    #[serde(with = "decimal_u64")]
    pub activation_height: u64,
    pub max_governance_action_bytes: u32,
    #[serde(with = "governance_costs_view")]
    pub governance_action_costs: [u64; 3],
}

impl FeeProfileV3 {
    pub fn limits(&self) -> v3::V3Limits {
        v3::V3Limits {
            ordinary: self.base.limits.clone(),
            max_governance_action_bytes: self.max_governance_action_bytes,
        }
    }

    pub fn validate(&self) -> Result<()> {
        self.base.validate()?;
        need(
            self.version > 0
                && self.activation_height > 0
                && self.base.activation_height <= self.activation_height,
            "ordinary-v3 version or activation height is invalid",
        )?;
        self.limits().validate()?;
        Ok(())
    }

    /// Require the exact signed v3 profile and active finalized height.
    /// Account authority and signature checks belong to the caller.
    pub fn validate_signed_request(
        &self,
        body: &v3::OrdinaryTransaction,
        finalized_height: u64,
    ) -> Result<()> {
        self.validate()?;
        v3::signing_bytes(body, &self.limits())?;
        need(
            matches!(
                body.actions.as_slice(),
                [v3::Action::GovernanceProposal { .. }]
                    | [v3::Action::GovernanceDeposit { .. }]
                    | [v3::Action::GovernanceVote { .. }]
            ),
            "ordinary-v3 requires exactly one governance action",
        )?;
        need(
            finalized_height >= self.activation_height
                && body.ordinary_fee_contract_version == ORDINARY_FEE_CONTRACT_VERSION
                && body.fee_profile_version == self.version
                && body.fee_profile_digest == profile_digest(self)?
                && body.fee_denomination == v3::Denomination::Udrt,
            "signed ordinary-v3 fee profile or activation differs",
        )?;
        self.base
            .validate_request(body.gas_limit, body.maximum_fee)?;
        Ok(())
    }

    /// Tags are one-based on the wire; return no implicit price for an unknown tag.
    pub fn action_cost(&self, tag: u8) -> Result<u64> {
        self.validate()?;
        match tag {
            1..=12 => Ok(self.base.action_costs[usize::from(tag - 1)]),
            13..=15 => Ok(self.governance_action_costs[usize::from(tag - 13)]),
            _ => Err(WireError("unsupported ordinary-v3 action cost tag".into())),
        }
    }
}

pub fn profile_bytes(profile: &FeeProfileV3) -> Result<Vec<u8>> {
    profile.validate()?;
    let base = v1::profile_bytes(&profile.base)?;
    let mut bytes = Vec::with_capacity(PROFILE_PREFIX.len() + 60 + base.len());
    bytes.extend_from_slice(PROFILE_PREFIX);
    bytes.extend_from_slice(&FORMAT_VERSION.to_be_bytes());
    bytes.extend_from_slice(&ORDINARY_FEE_CONTRACT_VERSION.to_be_bytes());
    bytes.extend_from_slice(&profile.version.to_be_bytes());
    bytes.extend_from_slice(&profile.activation_height.to_be_bytes());
    bytes.extend_from_slice(&profile.max_governance_action_bytes.to_be_bytes());
    for cost in profile.governance_action_costs {
        bytes.extend_from_slice(&cost.to_be_bytes());
    }
    bytes.extend_from_slice(
        &u32::try_from(base.len())
            .map_err(|_| WireError("base fee profile length overflow".into()))?
            .to_be_bytes(),
    );
    bytes.extend_from_slice(&base);
    need(
        bytes.len() <= MAX_PROFILE_BYTES,
        "ordinary-v3 fee profile exceeds codec bound",
    )?;
    Ok(bytes)
}

pub fn profile_digest(profile: &FeeProfileV3) -> Result<[u8; 32]> {
    Ok(Sha3_256::digest(profile_bytes(profile)?).into())
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Reader<'a> {
    fn raw(&mut self, count: usize) -> Result<&'a [u8]> {
        need(
            count <= self.bytes.len().saturating_sub(self.offset),
            "truncated ordinary-v3 fee profile",
        )?;
        let raw = &self.bytes[self.offset..self.offset + count];
        self.offset += count;
        Ok(raw)
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
}

pub fn decode_profile(bytes: &[u8]) -> Result<FeeProfileV3> {
    need(
        bytes.len() <= MAX_PROFILE_BYTES,
        "ordinary-v3 fee profile exceeds codec bound",
    )?;
    let mut reader = Reader { bytes, offset: 0 };
    need(
        reader.raw(PROFILE_PREFIX.len())? == PROFILE_PREFIX,
        "ordinary-v3 fee prefix differs",
    )?;
    need(
        reader.u16()? == FORMAT_VERSION,
        "unsupported ordinary-v3 fee format",
    )?;
    need(
        reader.u16()? == ORDINARY_FEE_CONTRACT_VERSION,
        "unsupported ordinary-v3 fee contract",
    )?;
    let version = reader.u64()?;
    let activation_height = reader.u64()?;
    let max_governance_action_bytes = reader.u32()?;
    let mut governance_action_costs = [0; 3];
    for cost in &mut governance_action_costs {
        *cost = reader.u64()?;
    }
    let base_len = usize::try_from(reader.u32()?)
        .map_err(|_| WireError("base fee profile length overflow".into()))?;
    need(
        base_len <= v1::MAX_PROFILE_BYTES,
        "base fee profile exceeds bound",
    )?;
    let base = v1::decode_profile(reader.raw(base_len)?)?;
    need(
        reader.offset == bytes.len(),
        "trailing ordinary-v3 fee profile bytes",
    )?;
    let profile = FeeProfileV3 {
        base,
        version,
        activation_height,
        max_governance_action_bytes,
        governance_action_costs,
    };
    need(
        profile_bytes(&profile)? == bytes,
        "noncanonical ordinary-v3 fee profile",
    )?;
    Ok(profile)
}

mod decimal_u64 {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        let text = String::deserialize(deserializer)?;
        super::ordinary::parse_decimal_u64(&text).map_err(serde::de::Error::custom)
    }
}

mod governance_costs_view {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S: Serializer>(value: &[u64; 3], serializer: S) -> Result<S::Ok, S::Error> {
        value.map(|cost| cost.to_string()).serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<[u64; 3], D::Error> {
        let raw = <[String; 3]>::deserialize(deserializer)?;
        let mut costs = [0; 3];
        for (index, text) in raw.iter().enumerate() {
            costs[index] =
                super::ordinary::parse_decimal_u64(text).map_err(serde::de::Error::custom)?;
        }
        Ok(costs)
    }
}
