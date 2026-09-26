//! Approved ordinary-v2 signing codec. This module does not activate paid execution.
//! Binary bytes are authoritative. Serde representations are client views only.
//! Current authority, expiry, fee outcomes and typed registry rules require state.
use crate::recovery::{KeyIdentity, RecoveryDomain};
use crate::recovery_wire::WireError;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};
use std::collections::BTreeSet;
pub const VERSION: u16 = 2;
pub const SIGNING_PREFIX: &[u8] = b"DYTALLIX/ACCOUNT-TRANSACTION\0";
pub const WIRE_PREFIX: &[u8] = b"DYTALLIX/ACCOUNT-WIRE\0";
pub const KEY_PREFIX: &[u8] = b"DYTALLIX/ACCOUNT-KEY\0";
pub const MAX_CHAIN_BYTES: usize = 128;
type Result<T> = std::result::Result<T, WireError>;
fn need(ok: bool, message: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(WireError(message.into()))
    }
}
fn hash(bytes: &[u8]) -> [u8; 32] {
    Sha3_256::digest(bytes).into()
}
fn algorithm(name: &str) -> Result<(u16, usize, usize)> {
    match name {
        "mldsa65" => Ok((1, 1952, 3309)),
        "mldsa87" => Ok((2, 2592, 4627)),
        _ => Err(WireError("unsupported exact account algorithm".into())),
    }
}
pub fn signature_size(name: &str) -> Result<usize> {
    Ok(algorithm(name)?.2)
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub max_wire_bytes: u32,
    pub max_actions: u16,
    pub max_identifier_bytes: u16,
    pub max_data_bytes: u32,
    pub max_memo_bytes: u32,
    pub max_consensus_key_bytes: u32,
    pub max_proof_bytes: u32,
    pub max_expiry_lifetime: u64,
    pub allowed_algorithms: BTreeSet<String>,
}
impl Limits {
    pub fn validate(&self) -> Result<()> {
        need(
            self.max_wire_bytes > 0
                && self.max_actions > 0
                && self.max_identifier_bytes > 0
                && self.max_consensus_key_bytes > 0
                && self.max_proof_bytes > 0
                && self.max_expiry_lifetime > 0,
            "ordinary limits require explicit positive capacities",
        )?;
        need(
            !self.allowed_algorithms.is_empty() && self.allowed_algorithms.len() <= 2,
            "explicit account-role algorithms required",
        )?;
        for name in &self.allowed_algorithms {
            algorithm(name)?;
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Denomination {
    #[serde(rename = "udgt")]
    Udgt,
    #[serde(rename = "udrt")]
    Udrt,
}
impl Denomination {
    fn code(self) -> u8 {
        match self {
            Self::Udgt => 1,
            Self::Udrt => 2,
        }
    }
    fn from_code(code: u8) -> Result<Self> {
        match code {
            1 => Ok(Self::Udgt),
            2 => Ok(Self::Udrt),
            _ => Err(WireError("unsupported denomination code".into())),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrdinaryTransaction {
    pub domain: RecoveryDomain,
    #[serde(with = "decimal_u64")]
    pub authorization_generation: u64,
    #[serde(with = "decimal_u64")]
    pub spending_nonce: u64,
    pub key: KeyIdentity,
    #[serde(with = "decimal_u64")]
    pub expiry_height: u64,
    pub ordinary_fee_contract_version: u16,
    #[serde(with = "decimal_u64")]
    pub fee_profile_version: u64,
    pub fee_profile_digest: [u8; 32],
    pub fee_denomination: Denomination,
    #[serde(with = "decimal_u128")]
    pub maximum_fee: u128,
    #[serde(with = "decimal_u64")]
    pub gas_limit: u64,
    pub memo: String,
    pub actions: Vec<Action>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedOrdinary {
    pub body: OrdinaryTransaction,
    pub signature: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum Action {
    Send {
        recipient: [u8; 32],
        denomination: Denomination,
        #[serde(with = "decimal_u128")]
        amount: u128,
    },
    Data {
        data: String,
    },
    DmsRegister {
        beneficiary: [u8; 32],
        #[serde(with = "decimal_u64")]
        period_blocks: u64,
    },
    DmsPing,
    DmsClaim {
        owner: [u8; 32],
        #[serde(with = "decimal_u64")]
        expected_grant_generation: u64,
    },
    RewardBond {
        validator_id: String,
        #[serde(with = "decimal_u128")]
        amount_udgt: u128,
    },
    RewardBeginUnbond {
        validator_id: String,
        #[serde(with = "decimal_u128")]
        amount_udgt: u128,
    },
    RewardClaim,
    ValidatorRegister {
        validator_id: String,
        consensus_key: Vec<u8>,
        proof: Vec<u8>,
        #[serde(with = "decimal_u64")]
        proof_expiry_height: u64,
        #[serde(with = "decimal_u128")]
        amount_udgt: u128,
    },
    ValidatorRotateKey {
        validator_id: String,
        consensus_key: Vec<u8>,
        proof: Vec<u8>,
        #[serde(with = "decimal_u64")]
        proof_expiry_height: u64,
    },
    ValidatorExit {
        validator_id: String,
    },
    ValidatorWithdraw {
        unbond_id: String,
    },
}
// Serde's internally tagged unit visitor ignores extra fields. Empty struct
// variants enforce the same strict client schema while preserving the public API.
#[derive(Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
enum ActionView {
    Send {
        recipient: [u8; 32],
        denomination: Denomination,
        #[serde(with = "decimal_u128")]
        amount: u128,
    },
    Data {
        data: String,
    },
    DmsRegister {
        beneficiary: [u8; 32],
        #[serde(with = "decimal_u64")]
        period_blocks: u64,
    },
    DmsPing {},
    DmsClaim {
        owner: [u8; 32],
        #[serde(with = "decimal_u64")]
        expected_grant_generation: u64,
    },
    RewardBond {
        validator_id: String,
        #[serde(with = "decimal_u128")]
        amount_udgt: u128,
    },
    RewardBeginUnbond {
        validator_id: String,
        #[serde(with = "decimal_u128")]
        amount_udgt: u128,
    },
    RewardClaim {},
    ValidatorRegister {
        validator_id: String,
        consensus_key: Vec<u8>,
        proof: Vec<u8>,
        #[serde(with = "decimal_u64")]
        proof_expiry_height: u64,
        #[serde(with = "decimal_u128")]
        amount_udgt: u128,
    },
    ValidatorRotateKey {
        validator_id: String,
        consensus_key: Vec<u8>,
        proof: Vec<u8>,
        #[serde(with = "decimal_u64")]
        proof_expiry_height: u64,
    },
    ValidatorExit {
        validator_id: String,
    },
    ValidatorWithdraw {
        unbond_id: String,
    },
}
impl<'de> Deserialize<'de> for Action {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        Ok(match ActionView::deserialize(deserializer)? {
            ActionView::Send {
                recipient,
                denomination,
                amount,
            } => Self::Send {
                recipient,
                denomination,
                amount,
            },
            ActionView::Data { data } => Self::Data { data },
            ActionView::DmsRegister {
                beneficiary,
                period_blocks,
            } => Self::DmsRegister {
                beneficiary,
                period_blocks,
            },
            ActionView::DmsPing {} => Self::DmsPing,
            ActionView::DmsClaim {
                owner,
                expected_grant_generation,
            } => Self::DmsClaim {
                owner,
                expected_grant_generation,
            },
            ActionView::RewardBond {
                validator_id,
                amount_udgt,
            } => Self::RewardBond {
                validator_id,
                amount_udgt,
            },
            ActionView::RewardBeginUnbond {
                validator_id,
                amount_udgt,
            } => Self::RewardBeginUnbond {
                validator_id,
                amount_udgt,
            },
            ActionView::RewardClaim {} => Self::RewardClaim,
            ActionView::ValidatorRegister {
                validator_id,
                consensus_key,
                proof,
                proof_expiry_height,
                amount_udgt,
            } => Self::ValidatorRegister {
                validator_id,
                consensus_key,
                proof,
                proof_expiry_height,
                amount_udgt,
            },
            ActionView::ValidatorRotateKey {
                validator_id,
                consensus_key,
                proof,
                proof_expiry_height,
            } => Self::ValidatorRotateKey {
                validator_id,
                consensus_key,
                proof,
                proof_expiry_height,
            },
            ActionView::ValidatorExit { validator_id } => Self::ValidatorExit { validator_id },
            ActionView::ValidatorWithdraw { unbond_id } => Self::ValidatorWithdraw { unbond_id },
        })
    }
}

fn decimal(text: &str) -> Result<()> {
    need(
        !text.is_empty()
            && text.bytes().all(|b| b.is_ascii_digit())
            && (text == "0" || !text.starts_with('0')),
        "noncanonical unsigned decimal string",
    )
}
pub fn parse_decimal_u64(text: &str) -> Result<u64> {
    decimal(text)?;
    text.parse()
        .map_err(|_| WireError("u64 decimal overflow".into()))
}
pub fn parse_decimal_u128(text: &str) -> Result<u128> {
    decimal(text)?;
    text.parse()
        .map_err(|_| WireError("u128 decimal overflow".into()))
}
mod decimal_u64 {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(value: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&value.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        let text = String::deserialize(d)?;
        super::parse_decimal_u64(&text).map_err(serde::de::Error::custom)
    }
}
mod decimal_u128 {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(value: &u128, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&value.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u128, D::Error> {
        let text = String::deserialize(d)?;
        super::parse_decimal_u128(&text).map_err(serde::de::Error::custom)
    }
}
struct Writer {
    bytes: Vec<u8>,
    maximum: usize,
}
impl Writer {
    fn raw(&mut self, bytes: &[u8]) -> Result<()> {
        need(
            bytes.len() <= self.maximum.saturating_sub(self.bytes.len()),
            "ordinary wire exceeds profile limit",
        )?;
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
    fn u8(&mut self, v: u8) -> Result<()> {
        self.raw(&[v])
    }
    fn u16(&mut self, v: u16) -> Result<()> {
        self.raw(&v.to_be_bytes())
    }
    fn u32(&mut self, v: u32) -> Result<()> {
        self.raw(&v.to_be_bytes())
    }
    fn u64(&mut self, v: u64) -> Result<()> {
        self.raw(&v.to_be_bytes())
    }
    fn u128(&mut self, v: u128) -> Result<()> {
        self.raw(&v.to_be_bytes())
    }
    fn text(&mut self, v: &str, maximum: usize) -> Result<()> {
        need(
            !v.is_empty() && v.len() <= maximum,
            "identifier length outside profile",
        )?;
        self.u16(u16::try_from(v.len()).map_err(|_| WireError("text length overflow".into()))?)?;
        self.raw(v.as_bytes())
    }
    fn blob(&mut self, v: &[u8], maximum: usize) -> Result<()> {
        need(v.len() <= maximum, "blob length exceeds profile")?;
        self.u32(u32::try_from(v.len()).map_err(|_| WireError("blob length overflow".into()))?)?;
        self.raw(v)
    }
    fn key(&mut self, v: &KeyIdentity) -> Result<()> {
        let (code, size, _) = algorithm(&v.algorithm)?;
        need(
            v.public_key.len() == size,
            "account public key length differs",
        )?;
        self.u16(code)?;
        self.blob(&v.public_key, size)
    }
    fn action(&mut self, v: &Action, l: &Limits) -> Result<()> {
        let text = usize::from(l.max_identifier_bytes);
        match v {
            Action::Send {
                recipient,
                denomination,
                amount,
            } => {
                need(*amount > 0, "zero send amount")?;
                self.u8(1)?;
                self.raw(recipient)?;
                self.u8(denomination.code())?;
                self.u128(*amount)
            }
            Action::Data { data } => {
                self.u8(2)?;
                self.blob(data.as_bytes(), capacity(l.max_data_bytes)?)
            }
            Action::DmsRegister {
                beneficiary,
                period_blocks,
            } => {
                need(*period_blocks > 0, "zero Dms period")?;
                self.u8(3)?;
                self.raw(beneficiary)?;
                self.u64(*period_blocks)
            }
            Action::DmsPing => self.u8(4),
            Action::DmsClaim {
                owner,
                expected_grant_generation,
            } => {
                self.u8(5)?;
                self.raw(owner)?;
                self.u64(*expected_grant_generation)
            }
            Action::RewardBond {
                validator_id,
                amount_udgt,
            } => {
                need(*amount_udgt > 0, "zero bond amount")?;
                self.u8(6)?;
                self.text(validator_id, text)?;
                self.u128(*amount_udgt)
            }
            Action::RewardBeginUnbond {
                validator_id,
                amount_udgt,
            } => {
                need(*amount_udgt > 0, "zero unbond amount")?;
                self.u8(7)?;
                self.text(validator_id, text)?;
                self.u128(*amount_udgt)
            }
            Action::RewardClaim => self.u8(8),
            Action::ValidatorRegister {
                validator_id,
                consensus_key,
                proof,
                proof_expiry_height,
                amount_udgt,
            } => {
                need(*amount_udgt > 0, "zero validator amount")?;
                self.u8(9)?;
                self.text(validator_id, text)?;
                self.blob(consensus_key, capacity(l.max_consensus_key_bytes)?)?;
                self.blob(proof, capacity(l.max_proof_bytes)?)?;
                self.u64(*proof_expiry_height)?;
                self.u128(*amount_udgt)
            }
            Action::ValidatorRotateKey {
                validator_id,
                consensus_key,
                proof,
                proof_expiry_height,
            } => {
                self.u8(10)?;
                self.text(validator_id, text)?;
                self.blob(consensus_key, capacity(l.max_consensus_key_bytes)?)?;
                self.blob(proof, capacity(l.max_proof_bytes)?)?;
                self.u64(*proof_expiry_height)
            }
            Action::ValidatorExit { validator_id } => {
                self.u8(11)?;
                self.text(validator_id, text)
            }
            Action::ValidatorWithdraw { unbond_id } => {
                self.u8(12)?;
                self.text(unbond_id, text)
            }
        }
    }
}
fn capacity(value: u32) -> Result<usize> {
    usize::try_from(value).map_err(|_| WireError("capacity conversion overflow".into()))
}
fn checked_limits(l: &Limits) -> Result<usize> {
    l.validate()?;
    usize::try_from(l.max_wire_bytes)
        .map_err(|_| WireError("wire capacity conversion overflow".into()))
}
/// Full pure-ML-DSA message. No prehash and no implicit profile defaults.
pub fn signing_bytes(body: &OrdinaryTransaction, l: &Limits) -> Result<Vec<u8>> {
    let maximum = checked_limits(l)?;
    need(
        (1..=3).contains(&body.domain.network),
        "unsupported network code",
    )?;
    need(
        l.allowed_algorithms.contains(&body.key.algorithm),
        "algorithm not enabled for account role",
    )?;
    need(
        body.ordinary_fee_contract_version != 0,
        "ordinary fee contract absent",
    )?;
    need(
        body.fee_denomination == Denomination::Udrt,
        "ordinary fee denomination must be udrt",
    )?;
    need(
        !body.actions.is_empty() && body.actions.len() <= usize::from(l.max_actions),
        "action count outside profile",
    )?;
    let mut w = Writer {
        bytes: Vec::new(),
        maximum,
    };
    w.raw(SIGNING_PREFIX)?;
    w.u16(VERSION)?;
    w.u8(body.domain.network)?;
    w.text(&body.domain.chain_id, MAX_CHAIN_BYTES)?;
    w.raw(&body.domain.genesis_digest)?;
    w.raw(&body.domain.account_id)?;
    w.u64(body.authorization_generation)?;
    w.u64(body.spending_nonce)?;
    w.key(&body.key)?;
    w.u64(body.expiry_height)?;
    w.u16(body.ordinary_fee_contract_version)?;
    w.u64(body.fee_profile_version)?;
    w.raw(&body.fee_profile_digest)?;
    w.u8(body.fee_denomination.code())?;
    w.u128(body.maximum_fee)?;
    w.u64(body.gas_limit)?;
    w.blob(body.memo.as_bytes(), capacity(l.max_memo_bytes)?)?;
    w.u16(
        u16::try_from(body.actions.len()).map_err(|_| WireError("action count overflow".into()))?,
    )?;
    for action in &body.actions {
        w.action(action, l)?;
    }
    // A body that cannot fit its exact signed framing is not signable in this profile.
    let overhead = WIRE_PREFIX
        .len()
        .checked_add(2 + 4 + 4)
        .and_then(|n| n.checked_add(signature_size(&body.key.algorithm).ok()?))
        .ok_or_else(|| WireError("ordinary framing overflow".into()))?;
    need(
        w.bytes.len() <= maximum.saturating_sub(overhead) && overhead <= maximum,
        "signed ordinary envelope exceeds profile",
    )?;
    Ok(w.bytes)
}
pub fn encode(value: &SignedOrdinary, l: &Limits) -> Result<Vec<u8>> {
    let maximum = checked_limits(l)?;
    let body = signing_bytes(&value.body, l)?;
    need(
        value.signature.len() == signature_size(&value.body.key.algorithm)?,
        "ordinary signature length differs",
    )?;
    let mut w = Writer {
        bytes: Vec::new(),
        maximum,
    };
    w.raw(WIRE_PREFIX)?;
    w.u16(VERSION)?;
    w.blob(&body, maximum)?;
    w.blob(&value.signature, value.signature.len())?;
    Ok(w.bytes)
}
pub fn transaction_id(body: &OrdinaryTransaction, l: &Limits) -> Result<[u8; 32]> {
    Ok(hash(&signing_bytes(body, l)?))
}
pub fn envelope_hash(value: &SignedOrdinary, l: &Limits) -> Result<[u8; 32]> {
    Ok(hash(&encode(value, l)?))
}
pub fn key_id(key: &KeyIdentity) -> Result<[u8; 32]> {
    let mut w = Writer {
        bytes: Vec::new(),
        maximum: KEY_PREFIX.len() + 6 + 2592,
    };
    w.raw(KEY_PREFIX)?;
    w.key(key)?;
    Ok(hash(&w.bytes))
}
struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Reader<'a> {
    fn raw(&mut self, n: usize) -> Result<&'a [u8]> {
        need(
            n <= self.bytes.len().saturating_sub(self.offset),
            "truncated ordinary wire",
        )?;
        let out = &self.bytes[self.offset..self.offset + n];
        self.offset += n;
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
    fn text(&mut self, maximum: usize) -> Result<String> {
        let n = usize::from(self.u16()?);
        need(n > 0 && n <= maximum, "identifier length outside profile")?;
        Ok(std::str::from_utf8(self.raw(n)?)
            .map_err(|_| WireError("invalid ordinary UTF-8".into()))?
            .to_owned())
    }
    fn blob(&mut self, maximum: usize) -> Result<&'a [u8]> {
        let n =
            usize::try_from(self.u32()?).map_err(|_| WireError("blob length overflow".into()))?;
        need(n <= maximum, "blob length exceeds profile")?;
        self.raw(n)
    }
    fn utf8(&mut self, maximum: usize) -> Result<String> {
        Ok(std::str::from_utf8(self.blob(maximum)?)
            .map_err(|_| WireError("invalid ordinary UTF-8".into()))?
            .to_owned())
    }
    fn key(&mut self) -> Result<KeyIdentity> {
        let (name, size) = match self.u16()? {
            1 => ("mldsa65", 1952),
            2 => ("mldsa87", 2592),
            _ => return Err(WireError("unsupported account algorithm code".into())),
        };
        let bytes = self.blob(size)?;
        need(bytes.len() == size, "account public key length differs")?;
        Ok(KeyIdentity {
            algorithm: name.into(),
            public_key: bytes.to_vec(),
        })
    }
    fn prefix(&mut self, p: &[u8]) -> Result<()> {
        need(self.raw(p.len())? == p, "ordinary prefix differs")?;
        need(self.u16()? == VERSION, "unsupported ordinary version")
    }
    fn finish(&self) -> Result<()> {
        need(self.offset == self.bytes.len(), "trailing ordinary bytes")
    }
    fn action(&mut self, l: &Limits) -> Result<Action> {
        let text = usize::from(l.max_identifier_bytes);
        Ok(match self.u8()? {
            1 => Action::Send {
                recipient: self.id()?,
                denomination: Denomination::from_code(self.u8()?)?,
                amount: self.u128()?,
            },
            2 => Action::Data {
                data: self.utf8(capacity(l.max_data_bytes)?)?,
            },
            3 => Action::DmsRegister {
                beneficiary: self.id()?,
                period_blocks: self.u64()?,
            },
            4 => Action::DmsPing,
            5 => Action::DmsClaim {
                owner: self.id()?,
                expected_grant_generation: self.u64()?,
            },
            6 => Action::RewardBond {
                validator_id: self.text(text)?,
                amount_udgt: self.u128()?,
            },
            7 => Action::RewardBeginUnbond {
                validator_id: self.text(text)?,
                amount_udgt: self.u128()?,
            },
            8 => Action::RewardClaim,
            9 => Action::ValidatorRegister {
                validator_id: self.text(text)?,
                consensus_key: self.blob(capacity(l.max_consensus_key_bytes)?)?.to_vec(),
                proof: self.blob(capacity(l.max_proof_bytes)?)?.to_vec(),
                proof_expiry_height: self.u64()?,
                amount_udgt: self.u128()?,
            },
            10 => Action::ValidatorRotateKey {
                validator_id: self.text(text)?,
                consensus_key: self.blob(capacity(l.max_consensus_key_bytes)?)?.to_vec(),
                proof: self.blob(capacity(l.max_proof_bytes)?)?.to_vec(),
                proof_expiry_height: self.u64()?,
            },
            11 => Action::ValidatorExit {
                validator_id: self.text(text)?,
            },
            12 => Action::ValidatorWithdraw {
                unbond_id: self.text(text)?,
            },
            _ => return Err(WireError("unsupported ordinary action tag".into())),
        })
    }
}
pub fn decode_body(bytes: &[u8], l: &Limits) -> Result<OrdinaryTransaction> {
    let maximum = checked_limits(l)?;
    need(bytes.len() <= maximum, "ordinary body exceeds profile")?;
    let mut r = Reader { bytes, offset: 0 };
    r.prefix(SIGNING_PREFIX)?;
    let domain = RecoveryDomain {
        network: r.u8()?,
        chain_id: r.text(MAX_CHAIN_BYTES)?,
        genesis_digest: r.id()?,
        account_id: r.id()?,
    };
    let mut body = OrdinaryTransaction {
        domain,
        authorization_generation: r.u64()?,
        spending_nonce: r.u64()?,
        key: r.key()?,
        expiry_height: r.u64()?,
        ordinary_fee_contract_version: r.u16()?,
        fee_profile_version: r.u64()?,
        fee_profile_digest: r.id()?,
        fee_denomination: Denomination::from_code(r.u8()?)?,
        maximum_fee: r.u128()?,
        gas_limit: r.u64()?,
        memo: r.utf8(capacity(l.max_memo_bytes)?)?,
        actions: Vec::new(),
    };
    let n = usize::from(r.u16()?);
    need(
        n > 0 && n <= usize::from(l.max_actions),
        "action count outside profile",
    )?;
    for _ in 0..n {
        body.actions.push(r.action(l)?);
    }
    r.finish()?;
    need(
        signing_bytes(&body, l)? == bytes,
        "noncanonical ordinary body",
    )?;
    Ok(body)
}
pub fn decode(bytes: &[u8], l: &Limits) -> Result<SignedOrdinary> {
    let maximum = checked_limits(l)?;
    need(bytes.len() <= maximum, "ordinary envelope exceeds profile")?;
    let mut r = Reader { bytes, offset: 0 };
    r.prefix(WIRE_PREFIX)?;
    let body = decode_body(r.blob(maximum)?, l)?;
    let size = signature_size(&body.key.algorithm)?;
    let signature = r.blob(size)?.to_vec();
    need(signature.len() == size, "ordinary signature length differs")?;
    r.finish()?;
    let value = SignedOrdinary { body, signature };
    need(
        encode(&value, l)? == bytes,
        "noncanonical ordinary envelope",
    )?;
    Ok(value)
}
