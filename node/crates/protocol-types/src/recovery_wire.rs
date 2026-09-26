//! Bounded canonical binary recovery codec, local version 1 candidate.
//!
//! This does not change the global transaction format or authorize production use.
//! It carries no fee authorization. `Spend` is rejected because that internal
//! authority placeholder omits the value-bearing transaction body.
//!
//! Wire: WIRE_PREFIX, u16be version, operation, u8 signature count, signatures.
//! Operation: u8 network, u16be UTF-8 chain length/string, genesis[32], account[32],
//! u64be submission expiry, u8 action tag, action fields in declaration order.
//! Every authority counter and timing version is u64be. IDs are exact 32 bytes.
//! Key: u8 algorithm code, u16be key length, key bytes. Signature: u8 role, key,
//! u16be signature length, signature bytes. Policy: u16be threshold, u8 guardian
//! count, then each key and its u16be UTF-8 control-group length/string.
//!
//! Guardians must already be strictly ordered by KeyIdentity. Signatures must
//! already be strictly ordered by (role, KeyIdentity). No input is normalized.
//! A signer may provide both operation and possession signatures with one key.
//! Authorization thresholds, custody truth and current authority belong to the
//! recovery state machine and trusted verifier, not this syntactic codec.

use crate::recovery::{
    Action, ActionKind, ActiveAuthorization, Guardian, KeyIdentity, PolicyAuthorization,
    RecoveryAuthorization, RecoveryDomain, RecoveryPolicy,
};
use std::collections::BTreeSet;

pub const VERSION: u16 = 1;
pub const WIRE_PREFIX: &[u8] = b"DYTALLIX/RECOVERY-WIRE\0";
pub const OPERATION_PREFIX: &[u8] = b"DYTALLIX/RECOVERY-OPERATION\0";
pub const POSSESSION_PREFIX: &[u8] = b"DYTALLIX/RECOVERY-POSSESSION\0";
pub const MAX_WIRE_BYTES: usize = 128 * 1024;
pub const MAX_STRING_BYTES: usize = 128;
pub const MAX_SIGNATURES: usize = 7;
pub const MAX_GUARDIANS: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryOperation {
    pub domain: RecoveryDomain,
    pub action: Action,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SignatureRole {
    Operation,
    Possession,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoverySignature {
    pub role: SignatureRole,
    pub key: KeyIdentity,
    pub signature: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedRecovery {
    pub operation: RecoveryOperation,
    pub signatures: Vec<RecoverySignature>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WireError(pub String);
impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for WireError {}
type Result<T> = std::result::Result<T, WireError>;
fn need(ok: bool, reason: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(WireError(reason.into()))
    }
}
fn algorithm(name: &str) -> Result<(u8, usize, usize)> {
    match name {
        "mldsa65" => Ok((1, 1952, 3309)),
        "mldsa87" => Ok((2, 2592, 4627)),
        _ => Err(WireError("unsupported exact algorithm identifier".into())),
    }
}
struct Writer {
    bytes: Vec<u8>,
}
impl Writer {
    fn new() -> Self {
        Self { bytes: Vec::new() }
    }
    fn raw(&mut self, bytes: &[u8]) -> Result<()> {
        need(
            bytes.len() <= MAX_WIRE_BYTES.saturating_sub(self.bytes.len()),
            "wire size exceeds limit",
        )?;
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
    fn u8(&mut self, value: u8) -> Result<()> {
        self.raw(&[value])
    }
    fn u16(&mut self, value: u16) -> Result<()> {
        self.raw(&value.to_be_bytes())
    }
    fn u64(&mut self, value: u64) -> Result<()> {
        self.raw(&value.to_be_bytes())
    }
    fn string(&mut self, value: &str) -> Result<()> {
        need(
            !value.is_empty() && value.len() <= MAX_STRING_BYTES,
            "string length outside bounds",
        )?;
        self.u16(value.len() as u16)?;
        self.raw(value.as_bytes())
    }
    fn key(&mut self, key: &KeyIdentity) -> Result<()> {
        let (code, size, _) = algorithm(&key.algorithm)?;
        need(
            key.public_key.len() == size,
            "public key length differs from algorithm",
        )?;
        self.u8(code)?;
        self.u16(size as u16)?;
        self.raw(&key.public_key)
    }
    fn active(&mut self, a: &ActiveAuthorization) -> Result<()> {
        self.u64(a.generation)?;
        self.u64(a.nonce)
    }
    fn recovery(&mut self, a: &RecoveryAuthorization) -> Result<()> {
        self.u64(a.policy_version)?;
        self.u64(a.sequence)
    }
    fn policy_auth(&mut self, a: &PolicyAuthorization) -> Result<()> {
        self.u64(a.policy_version)?;
        self.u64(a.sequence)
    }
    fn policy(&mut self, policy: &RecoveryPolicy) -> Result<()> {
        need(
            policy.guardians.len() <= MAX_GUARDIANS,
            "too many guardians",
        )?;
        for pair in policy.guardians.windows(2) {
            need(
                pair[0].key < pair[1].key,
                "guardian keys are not strictly ordered",
            )?;
        }
        let mut groups = BTreeSet::new();
        self.u16(policy.threshold)?;
        self.u8(policy.guardians.len() as u8)?;
        for guardian in &policy.guardians {
            need(
                groups.insert(&guardian.control_group),
                "duplicate guardian control group",
            )?;
            self.key(&guardian.key)?;
            self.string(&guardian.control_group)?;
        }
        Ok(())
    }
    fn operation(&mut self, operation: &RecoveryOperation) -> Result<()> {
        let domain = &operation.domain;
        need(
            (1..=3).contains(&domain.network),
            "unsupported network code",
        )?;
        self.u8(domain.network)?;
        self.string(&domain.chain_id)?;
        self.raw(&domain.genesis_digest)?;
        self.raw(&domain.account_id)?;
        self.u64(operation.action.submission_expiry)?;
        match &operation.action.kind {
            ActionKind::Enroll { active, policy } => {
                self.u8(1)?;
                self.active(active)?;
                self.policy(policy)?;
            }
            ActionKind::Rotate {
                active,
                replacement,
            } => {
                self.u8(2)?;
                self.active(active)?;
                self.key(replacement)?;
            }
            ActionKind::Start {
                recovery,
                request_id,
                replacement,
                timing_version,
            } => {
                self.u8(3)?;
                self.recovery(recovery)?;
                self.raw(request_id)?;
                self.key(replacement)?;
                self.u64(*timing_version)?;
            }
            ActionKind::Finalize {
                recovery,
                request_id,
            } => {
                self.u8(4)?;
                self.recovery(recovery)?;
                self.raw(request_id)?;
            }
            ActionKind::Cancel {
                recovery,
                request_id,
            } => {
                self.u8(5)?;
                self.recovery(recovery)?;
                self.raw(request_id)?;
            }
            ActionKind::Resume {
                recovery,
                active_key,
            } => {
                self.u8(6)?;
                self.recovery(recovery)?;
                self.key(active_key)?;
            }
            ActionKind::StagePolicy {
                active,
                authorization,
                update_id,
                policy,
                timing_version,
            } => {
                self.u8(7)?;
                self.active(active)?;
                self.policy_auth(authorization)?;
                self.raw(update_id)?;
                self.policy(policy)?;
                self.u64(*timing_version)?;
            }
            ActionKind::ActivatePolicy {
                active,
                authorization,
                update_id,
            } => {
                self.u8(8)?;
                self.active(active)?;
                self.policy_auth(authorization)?;
                self.raw(update_id)?;
            }
            ActionKind::CancelPolicy {
                authorization,
                update_id,
            } => {
                self.u8(9)?;
                self.policy_auth(authorization)?;
                self.raw(update_id)?;
            }
            ActionKind::Spend { .. } => {
                return Err(WireError(
                    "Spend lacks a complete signed transaction body".into(),
                ))
            }
        }
        Ok(())
    }
}

/// Canonical operation fields without a signing prefix or signature list.
/// This preserves the existing recovery version 1 operation encoding.
pub fn operation_bytes(operation: &RecoveryOperation) -> Result<Vec<u8>> {
    let mut writer = Writer::new();
    writer.operation(operation)?;
    Ok(writer.bytes)
}

/// Encode the supplied canonical order. Never sorts or normalizes callers' data.
/// An empty signature list is syntactically allowed for unsigned local fixtures;
/// this function does not establish authority or verify a signature.
pub fn encode(value: &SignedRecovery) -> Result<Vec<u8>> {
    need(
        value.signatures.len() <= MAX_SIGNATURES,
        "too many signatures",
    )?;
    for pair in value.signatures.windows(2) {
        need(
            (pair[0].role, &pair[0].key) < (pair[1].role, &pair[1].key),
            "signatures are not strictly ordered by role and key",
        )?;
    }
    let mut writer = Writer::new();
    writer.raw(WIRE_PREFIX)?;
    writer.u16(VERSION)?;
    writer.operation(&value.operation)?;
    writer.u8(value.signatures.len() as u8)?;
    for signature in &value.signatures {
        let (_, _, size) = algorithm(&signature.key.algorithm)?;
        need(
            signature.signature.len() == size,
            "signature length differs from algorithm",
        )?;
        writer.u8(match signature.role {
            SignatureRole::Operation => 0,
            SignatureRole::Possession => 1,
        })?;
        writer.key(&signature.key)?;
        writer.u16(size as u16)?;
        writer.raw(&signature.signature)?;
    }
    Ok(writer.bytes)
}

/// Domain-bound pure-ML-DSA message candidate. The verifier must use the selected
/// standard signing mode consistently; this is not a HashML-DSA prehash format.
/// Every operation approval signs identical bytes. The verifier authenticates
/// each listed key through its ML-DSA signature, not a repeated key suffix.
/// Possession proofs use their separate domain and include the exact proof key.
/// The supplied key must have a supported exact identifier and length for both roles.
pub fn signing_bytes(
    operation: &RecoveryOperation,
    role: SignatureRole,
    key: &KeyIdentity,
) -> Result<Vec<u8>> {
    let (_, key_size, _) = algorithm(&key.algorithm)?;
    need(
        key.public_key.len() == key_size,
        "public key length differs from algorithm",
    )?;
    let mut writer = Writer::new();
    writer.raw(match role {
        SignatureRole::Operation => OPERATION_PREFIX,
        SignatureRole::Possession => POSSESSION_PREFIX,
    })?;
    writer.u16(VERSION)?;
    writer.operation(operation)?;
    if role == SignatureRole::Possession {
        writer.key(key)?;
    }
    Ok(writer.bytes)
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Reader<'a> {
    fn raw(&mut self, count: usize) -> Result<&'a [u8]> {
        need(
            count <= self.bytes.len().saturating_sub(self.offset),
            "truncated recovery wire",
        )?;
        let value = &self.bytes[self.offset..self.offset + count];
        self.offset += count;
        Ok(value)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.raw(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(
            self.raw(2)?
                .try_into()
                .map_err(|_| WireError("u16 length".into()))?,
        ))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(
            self.raw(8)?
                .try_into()
                .map_err(|_| WireError("u64 length".into()))?,
        ))
    }
    fn id(&mut self) -> Result<[u8; 32]> {
        self.raw(32)?
            .try_into()
            .map_err(|_| WireError("identifier length".into()))
    }
    fn string(&mut self) -> Result<String> {
        let count = usize::from(self.u16()?);
        need(
            count > 0 && count <= MAX_STRING_BYTES,
            "string length outside bounds",
        )?;
        let value = std::str::from_utf8(self.raw(count)?)
            .map_err(|_| WireError("invalid UTF-8 string".into()))?;
        Ok(value.to_owned())
    }
    fn key(&mut self) -> Result<KeyIdentity> {
        let (name, size) = match self.u8()? {
            1 => ("mldsa65", 1952),
            2 => ("mldsa87", 2592),
            _ => return Err(WireError("unsupported algorithm code".into())),
        };
        need(
            usize::from(self.u16()?) == size,
            "public key length differs from algorithm",
        )?;
        Ok(KeyIdentity {
            algorithm: name.into(),
            public_key: self.raw(size)?.to_vec(),
        })
    }
    fn active(&mut self) -> Result<ActiveAuthorization> {
        Ok(ActiveAuthorization {
            generation: self.u64()?,
            nonce: self.u64()?,
        })
    }
    fn recovery(&mut self) -> Result<RecoveryAuthorization> {
        Ok(RecoveryAuthorization {
            policy_version: self.u64()?,
            sequence: self.u64()?,
        })
    }
    fn policy_auth(&mut self) -> Result<PolicyAuthorization> {
        Ok(PolicyAuthorization {
            policy_version: self.u64()?,
            sequence: self.u64()?,
        })
    }
    fn policy(&mut self) -> Result<RecoveryPolicy> {
        let threshold = self.u16()?;
        let count = usize::from(self.u8()?);
        need(count <= MAX_GUARDIANS, "too many guardians")?;
        let mut guardians = Vec::with_capacity(count);
        for _ in 0..count {
            guardians.push(Guardian {
                key: self.key()?,
                control_group: self.string()?,
            });
        }
        Ok(RecoveryPolicy {
            threshold,
            guardians,
        })
    }
    fn operation(&mut self) -> Result<RecoveryOperation> {
        let domain = RecoveryDomain {
            network: self.u8()?,
            chain_id: self.string()?,
            genesis_digest: self.id()?,
            account_id: self.id()?,
        };
        let submission_expiry = self.u64()?;
        let kind = match self.u8()? {
            1 => ActionKind::Enroll {
                active: self.active()?,
                policy: self.policy()?,
            },
            2 => ActionKind::Rotate {
                active: self.active()?,
                replacement: self.key()?,
            },
            3 => ActionKind::Start {
                recovery: self.recovery()?,
                request_id: self.id()?,
                replacement: self.key()?,
                timing_version: self.u64()?,
            },
            4 => ActionKind::Finalize {
                recovery: self.recovery()?,
                request_id: self.id()?,
            },
            5 => ActionKind::Cancel {
                recovery: self.recovery()?,
                request_id: self.id()?,
            },
            6 => ActionKind::Resume {
                recovery: self.recovery()?,
                active_key: self.key()?,
            },
            7 => ActionKind::StagePolicy {
                active: self.active()?,
                authorization: self.policy_auth()?,
                update_id: self.id()?,
                policy: self.policy()?,
                timing_version: self.u64()?,
            },
            8 => ActionKind::ActivatePolicy {
                active: self.active()?,
                authorization: self.policy_auth()?,
                update_id: self.id()?,
            },
            9 => ActionKind::CancelPolicy {
                authorization: self.policy_auth()?,
                update_id: self.id()?,
            },
            _ => return Err(WireError("unsupported recovery action tag".into())),
        };
        Ok(RecoveryOperation {
            domain,
            action: Action {
                submission_expiry,
                kind,
            },
        })
    }
}
/// Parse bounded binary data and reject noncanonical order and trailing bytes.
/// No serde/JSON decoding or legacy-format fallback exists on this path.
pub fn decode(bytes: &[u8]) -> Result<SignedRecovery> {
    need(bytes.len() <= MAX_WIRE_BYTES, "wire size exceeds limit")?;
    let mut reader = Reader { bytes, offset: 0 };
    need(
        reader.raw(WIRE_PREFIX.len())? == WIRE_PREFIX,
        "recovery wire prefix differs",
    )?;
    need(
        reader.u16()? == VERSION,
        "unsupported recovery wire version",
    )?;
    let operation = reader.operation()?;
    let count = usize::from(reader.u8()?);
    need(count <= MAX_SIGNATURES, "too many signatures")?;
    let mut signatures = Vec::with_capacity(count);
    for _ in 0..count {
        let role = match reader.u8()? {
            0 => SignatureRole::Operation,
            1 => SignatureRole::Possession,
            _ => return Err(WireError("unsupported signature role".into())),
        };
        let key = reader.key()?;
        let (_, _, size) = algorithm(&key.algorithm)?;
        need(
            usize::from(reader.u16()?) == size,
            "signature length differs from algorithm",
        )?;
        signatures.push(RecoverySignature {
            role,
            key,
            signature: reader.raw(size)?.to_vec(),
        });
    }
    need(reader.offset == bytes.len(), "trailing recovery wire bytes")?;
    let decoded = SignedRecovery {
        operation,
        signatures,
    };
    need(encode(&decoded)? == bytes, "noncanonical recovery wire")?;
    Ok(decoded)
}
