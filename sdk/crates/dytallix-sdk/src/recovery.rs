//! Recovery transactions (E04 gap 17, T-c2; P01 29 September 2026): every
//! recovery action, built from the node's `RecoveryAccountView`, signed by
//! each party on their own machine, assembled, then paid for by a separate
//! sponsor account. The codecs and signed bytes are the node's own
//! (`dytallix-protocol-types`). Nothing here submits a transaction.
//!
//! Flow: `build` gives the operation and who must sign it; each signer calls
//! `sign`; `assemble` checks and orders the signatures; `sponsor` adds the
//! paying account's signature within the fee bounds; `transaction` gives the
//! bytes to submit.

use aes_gcm::aead::{rand_core::RngCore, OsRng};
use dytallix_core::keypair::{DytallixKeypair, KeyScheme};
/// The node's typed recovery view (`/recovery/account/{account_id}`).
pub use dytallix_protocol_types::ordinary_client::{
    PendingPolicyView, PendingRecoveryView, RecoveryAccountView, RecoveryFeeView,
    RecoveryTimingView,
};
pub use dytallix_protocol_types::recovery::{
    Action, ActionKind, ActiveAuthorization, Guardian, KeyIdentity, PolicyAuthorization,
    RecoveryAuthorization, RecoveryDomain, RecoveryPolicy, RecoveryStatus,
};
pub use dytallix_protocol_types::recovery_sponsor::{SponsorAuthorization, SponsoredRecovery};
pub use dytallix_protocol_types::recovery_wire::{
    RecoveryOperation, RecoverySignature, SignatureRole, SignedRecovery,
};
use dytallix_protocol_types::{recovery_sponsor as sponsor_wire, recovery_wire};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};

use crate::ordinary_v2::{Error, Result};

/// The only algorithm the node verifies for recovery signatures.
const ALGORITHM: &str = "mldsa65";

/// What a client asks for; the counters and pending IDs come from the view.
/// Variants without fields are empty structs, so unknown fields are refused.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum RecoveryRequest {
    /// Enroll a guardian policy (the account has none yet).
    Enroll { policy: RecoveryPolicy },
    /// Replace the active key with the active key's signature.
    Rotate { replacement: KeyIdentity },
    /// Guardians start a recovery to `replacement`. A random request ID is
    /// chosen when none is given.
    Start {
        replacement: KeyIdentity,
        #[serde(default, with = "optional_hex32")]
        request_id: Option<[u8; 32]>,
    },
    /// The replacement key completes the pending recovery after its delay.
    Finalize {},
    /// Guardians cancel the pending recovery; the account locks.
    Cancel {},
    /// Guardians unlock a locked account with its current key.
    Resume {},
    /// Stage a new guardian policy (active key and current guardians).
    StagePolicy {
        policy: RecoveryPolicy,
        #[serde(default, with = "optional_hex32")]
        update_id: Option<[u8; 32]>,
    },
    /// Activate the staged policy after its delay.
    ActivatePolicy {},
    /// Guardians cancel the staged policy.
    CancelPolicy {},
}

/// Who must sign an operation, as the node checks it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requirements {
    /// Keys that must each sign in the operation role.
    pub operation_keys: Vec<KeyIdentity>,
    /// At least this many of `guardians` must also sign in the operation role.
    pub guardian_threshold: u16,
    pub guardians: Vec<KeyIdentity>,
    /// Keys that must each sign in the possession role: exactly this set.
    pub possession_keys: Vec<KeyIdentity>,
}

/// An operation ready for signing, with its requirements.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prepared {
    pub operation: RecoveryOperation,
    pub requirements: Requirements,
}

fn err(message: impl Into<String>) -> Error {
    Error(message.into())
}
fn random_id() -> [u8; 32] {
    let mut id = [0u8; 32];
    OsRng.fill_bytes(&mut id);
    id
}
fn guardian_keys(policy: &RecoveryPolicy) -> Vec<KeyIdentity> {
    policy.guardians.iter().map(|g| g.key.clone()).collect()
}
fn sorted(mut keys: Vec<KeyIdentity>) -> Vec<KeyIdentity> {
    keys.sort();
    keys
}

/// Build the operation for `request` from the account's committed view.
/// `submission_expiry` is the height the operation must be included before,
/// within the account's submission lifetime.
pub fn build(
    view: &RecoveryAccountView,
    request: &RecoveryRequest,
    submission_expiry: u64,
) -> Result<Prepared> {
    let height = view.context.height;
    if submission_expiry <= height || submission_expiry - height > view.timing.submission_lifetime {
        return Err(err(format!(
            "submission expiry must be above height {height} and within {} blocks",
            view.timing.submission_lifetime
        )));
    }
    let active = ActiveAuthorization {
        generation: view.active_generation,
        nonce: view.spending_nonce,
    };
    let recovery = RecoveryAuthorization {
        policy_version: view.policy_version,
        sequence: view.recovery_sequence,
    };
    let policy_authorization = PolicyAuthorization {
        policy_version: view.policy_version,
        sequence: view.policy_change_sequence,
    };
    let normal = || {
        (view.status == RecoveryStatus::Normal)
            .then_some(())
            .ok_or_else(|| err("the account is not in normal status"))
    };
    let current = || {
        view.policy
            .as_ref()
            .ok_or_else(|| err("the account has no guardian policy"))
    };
    let pending_recovery = || {
        view.pending_recovery
            .as_ref()
            .ok_or_else(|| err("the account has no pending recovery"))
    };
    let pending_policy = || {
        view.pending_policy
            .as_ref()
            .ok_or_else(|| err("the account has no staged policy"))
    };
    let only = |keys: Vec<KeyIdentity>| Requirements {
        operation_keys: keys,
        guardian_threshold: 0,
        guardians: Vec::new(),
        possession_keys: Vec::new(),
    };
    let quorum = |policy: &RecoveryPolicy, with_active: bool| Requirements {
        operation_keys: if with_active {
            vec![view.active_key.clone()]
        } else {
            Vec::new()
        },
        guardian_threshold: policy.threshold,
        guardians: guardian_keys(policy),
        possession_keys: Vec::new(),
    };
    let (kind, requirements) = match request {
        RecoveryRequest::Enroll { policy } => {
            normal()?;
            if view.policy.is_some() {
                return Err(err("the account already has a guardian policy"));
            }
            let requirements = Requirements {
                possession_keys: sorted(guardian_keys(policy)),
                ..only(vec![view.active_key.clone()])
            };
            (
                ActionKind::Enroll {
                    active,
                    policy: policy.clone(),
                },
                requirements,
            )
        }
        RecoveryRequest::Rotate { replacement } => {
            normal()?;
            let requirements = Requirements {
                possession_keys: vec![replacement.clone()],
                ..only(vec![view.active_key.clone()])
            };
            (
                ActionKind::Rotate {
                    active,
                    replacement: replacement.clone(),
                },
                requirements,
            )
        }
        RecoveryRequest::Start {
            replacement,
            request_id,
        } => {
            if view.pending_recovery.is_some() {
                return Err(err("a recovery is already pending"));
            }
            let requirements = Requirements {
                possession_keys: vec![replacement.clone()],
                ..quorum(current()?, false)
            };
            (
                ActionKind::Start {
                    recovery,
                    request_id: request_id.unwrap_or_else(random_id),
                    replacement: replacement.clone(),
                    timing_version: view.timing.timing_version,
                },
                requirements,
            )
        }
        RecoveryRequest::Finalize {} => {
            let pending = pending_recovery()?;
            if height + 1 < pending.activation_height || height + 1 >= pending.expiry_height {
                return Err(err(format!(
                    "the recovery finalizes from height {} and before {}",
                    pending.activation_height, pending.expiry_height
                )));
            }
            (
                ActionKind::Finalize {
                    recovery,
                    request_id: pending.request_id,
                },
                only(vec![pending.replacement.clone()]),
            )
        }
        RecoveryRequest::Cancel {} => (
            ActionKind::Cancel {
                recovery,
                request_id: pending_recovery()?.request_id,
            },
            quorum(current()?, false),
        ),
        RecoveryRequest::Resume {} => {
            if view.status != RecoveryStatus::RecoveryLocked {
                return Err(err("only a locked account resumes"));
            }
            (
                ActionKind::Resume {
                    recovery,
                    active_key: view.active_key.clone(),
                },
                quorum(current()?, false),
            )
        }
        RecoveryRequest::StagePolicy { policy, update_id } => {
            normal()?;
            if view.pending_policy.is_some() {
                return Err(err("a policy change is already staged"));
            }
            let requirements = Requirements {
                possession_keys: sorted(guardian_keys(policy)),
                ..quorum(current()?, true)
            };
            (
                ActionKind::StagePolicy {
                    active,
                    authorization: policy_authorization,
                    update_id: update_id.unwrap_or_else(random_id),
                    policy: policy.clone(),
                    timing_version: view.timing.timing_version,
                },
                requirements,
            )
        }
        RecoveryRequest::ActivatePolicy {} => {
            normal()?;
            let pending = pending_policy()?;
            if height + 1 < pending.activation_height || height + 1 >= pending.expiry_height {
                return Err(err(format!(
                    "the staged policy activates from height {} and before {}",
                    pending.activation_height, pending.expiry_height
                )));
            }
            (
                ActionKind::ActivatePolicy {
                    active,
                    authorization: policy_authorization,
                    update_id: pending.update_id,
                },
                quorum(current()?, true),
            )
        }
        RecoveryRequest::CancelPolicy {} => {
            normal()?;
            (
                ActionKind::CancelPolicy {
                    authorization: policy_authorization,
                    update_id: pending_policy()?.update_id,
                },
                quorum(current()?, false),
            )
        }
    };
    let d = &view.domain;
    let operation = RecoveryOperation {
        domain: RecoveryDomain {
            network: d.network,
            chain_id: d.chain_id.clone(),
            genesis_digest: d.genesis_digest,
            account_id: d.account_id,
        },
        action: Action {
            submission_expiry,
            kind,
        },
    };
    // The codec refuses what the node could not decode.
    unsigned_bytes(&operation)?;
    Ok(Prepared {
        operation,
        requirements,
    })
}

/// The unsigned operation's canonical bytes (the wire with no signatures).
pub fn unsigned_bytes(operation: &RecoveryOperation) -> Result<Vec<u8>> {
    recovery_wire::encode(&SignedRecovery {
        operation: operation.clone(),
        signatures: Vec::new(),
    })
    .map_err(|e| err(e.to_string()))
}
/// Decode an unsigned operation's bytes.
pub fn decode_unsigned(bytes: &[u8]) -> Result<RecoveryOperation> {
    let signed = recovery_wire::decode(bytes).map_err(|e| err(e.to_string()))?;
    if !signed.signatures.is_empty() {
        return Err(err("expected an unsigned operation"));
    }
    Ok(signed.operation)
}
/// The operation's identity: what every signature and the sponsor bind.
pub fn operation_id(operation: &RecoveryOperation) -> Result<[u8; 32]> {
    sponsor_wire::operation_id(operation).map_err(|e| err(e.to_string()))
}
/// The key identity of an ML-DSA-65 keypair.
pub fn identity(keypair: &DytallixKeypair) -> Result<KeyIdentity> {
    if keypair.scheme() != KeyScheme::MlDsa65 {
        return Err(err("recovery keys are ML-DSA-65"));
    }
    Ok(KeyIdentity {
        algorithm: ALGORITHM.into(),
        public_key: keypair.public_key().to_vec(),
    })
}

/// Sign `operation` in `role` with `keypair` (ML-DSA-65, empty context).
pub fn sign(
    operation: &RecoveryOperation,
    role: SignatureRole,
    keypair: &DytallixKeypair,
) -> Result<RecoverySignature> {
    let key = identity(keypair)?;
    let bytes =
        recovery_wire::signing_bytes(operation, role, &key).map_err(|e| err(e.to_string()))?;
    let signature = keypair.sign(&bytes).map_err(|e| err(e.to_string()))?;
    Ok(RecoverySignature {
        role,
        key,
        signature,
    })
}
fn verify(operation: &RecoveryOperation, signature: &RecoverySignature) -> Result<()> {
    if signature.key.algorithm != ALGORITHM {
        return Err(err("recovery signatures are ML-DSA-65"));
    }
    let bytes = recovery_wire::signing_bytes(operation, signature.role, &signature.key)
        .map_err(|e| err(e.to_string()))?;
    match dytallix_core::signature::verify_mldsa65(
        &signature.key.public_key,
        &bytes,
        &signature.signature,
    ) {
        Ok(true) => Ok(()),
        _ => Err(err("a signature does not verify for this operation")),
    }
}

/// Check every signature, drop duplicates, check `requirements` and order
/// the signatures as the wire requires.
pub fn assemble(
    operation: &RecoveryOperation,
    requirements: &Requirements,
    signatures: Vec<RecoverySignature>,
) -> Result<SignedRecovery> {
    let mut unique: Vec<RecoverySignature> = Vec::new();
    for signature in signatures {
        verify(operation, &signature)?;
        if !unique
            .iter()
            .any(|s| s.role == signature.role && s.key == signature.key)
        {
            unique.push(signature);
        }
    }
    let signed_by = |role: SignatureRole, key: &KeyIdentity| {
        unique.iter().any(|s| s.role == role && &s.key == key)
    };
    for key in &requirements.operation_keys {
        if !signed_by(SignatureRole::Operation, key) {
            return Err(err("a required operation signature is missing"));
        }
    }
    let guardians = requirements
        .guardians
        .iter()
        .filter(|key| signed_by(SignatureRole::Operation, key))
        .count();
    if guardians < usize::from(requirements.guardian_threshold) {
        return Err(err(format!(
            "{guardians} of the {} guardian signatures required",
            requirements.guardian_threshold
        )));
    }
    for signature in &unique {
        let allowed = match signature.role {
            SignatureRole::Operation => {
                requirements.operation_keys.contains(&signature.key)
                    || requirements.guardians.contains(&signature.key)
            }
            SignatureRole::Possession => requirements.possession_keys.contains(&signature.key),
        };
        if !allowed {
            return Err(err("a signature is from a key the operation does not name"));
        }
    }
    for key in &requirements.possession_keys {
        if !signed_by(SignatureRole::Possession, key) {
            return Err(err("a required possession proof is missing"));
        }
    }
    unique.sort_by(|a, b| a.role.cmp(&b.role).then(a.key.cmp(&b.key)));
    let signed = SignedRecovery {
        operation: operation.clone(),
        signatures: unique,
    };
    recovery_wire::encode(&signed).map_err(|e| err(e.to_string()))?;
    Ok(signed)
}

/// Bounds the sponsor signs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SponsorBounds {
    pub gas_limit: u64,
    pub maximum_charge: u128,
    pub expiry_height: u64,
}

/// Wrap `signed` with the sponsor's signature. `target` is the recovering
/// account's view (its fee profile) and `sponsor` the paying account's; the
/// target never pays for itself.
pub fn sponsor(
    signed: &SignedRecovery,
    target: &RecoveryAccountView,
    sponsor: &RecoveryAccountView,
    keypair: &DytallixKeypair,
    bounds: SponsorBounds,
) -> Result<SponsoredRecovery> {
    if target.domain.account_id != signed.operation.domain.account_id {
        return Err(err("the target view is not the operation's account"));
    }
    if sponsor.domain.account_id == target.domain.account_id {
        return Err(err("an account cannot sponsor its own recovery"));
    }
    if sponsor.status != RecoveryStatus::Normal {
        return Err(err("the sponsor account is not in normal status"));
    }
    let key = identity(keypair)?;
    if key != sponsor.active_key {
        return Err(err("the key is not the sponsor account's active key"));
    }
    check_bounds(&target.fee, bounds, target.context.height)?;
    let authorization = SponsorAuthorization {
        domain: signed.operation.domain.clone(),
        recovery_version: recovery_wire::VERSION,
        operation_id: operation_id(&signed.operation)?,
        signer_manifest_digest: sponsor_wire::signer_manifest_digest(signed)
            .map_err(|e| err(e.to_string()))?,
        sponsor_account_id: sponsor.domain.account_id,
        sponsor_generation: sponsor.active_generation,
        sponsor_nonce: sponsor.sponsor_nonce,
        sponsor_key: key,
        fee_profile_version: target.fee.profile_version,
        fee_profile_digest: target.fee.profile_digest,
        denomination: target.fee.denomination.clone(),
        maximum_charge: bounds.maximum_charge,
        gas_limit: bounds.gas_limit,
        expiry_height: bounds.expiry_height,
    };
    let bytes =
        sponsor_wire::sponsor_signing_bytes(&authorization).map_err(|e| err(e.to_string()))?;
    let signature = keypair.sign(&bytes).map_err(|e| err(e.to_string()))?;
    let sponsored = SponsoredRecovery {
        recovery: signed.clone(),
        sponsor: authorization,
        signature,
    };
    sponsor_wire::encode(&sponsored).map_err(|e| err(e.to_string()))?;
    Ok(sponsored)
}
fn check_bounds(fee: &RecoveryFeeView, bounds: SponsorBounds, height: u64) -> Result<()> {
    if bounds.gas_limit < fee.minimum_gas || bounds.gas_limit > fee.max_transaction_gas {
        return Err(err(format!(
            "gas limit must be {} to {}",
            fee.minimum_gas, fee.max_transaction_gas
        )));
    }
    let floor = u128::from(bounds.gas_limit) * u128::from(fee.gas_price);
    if bounds.maximum_charge < floor || bounds.maximum_charge > fee.max_fee_cap {
        return Err(err(format!(
            "maximum charge must be {floor} to {} {}",
            fee.max_fee_cap, fee.denomination
        )));
    }
    if bounds.expiry_height <= height {
        return Err(err("the sponsor expiry must be above the view's height"));
    }
    Ok(())
}

/// The sponsor authorization's identity: the key of its receipt
/// (`recovery:v2:receipt:<hex>`).
pub fn authorization_id(sponsored: &SponsoredRecovery) -> Result<[u8; 32]> {
    let bytes =
        sponsor_wire::sponsor_signing_bytes(&sponsored.sponsor).map_err(|e| err(e.to_string()))?;
    Ok(Sha3_256::digest(bytes).into())
}

/// A signed operation's wire bytes, and back.
pub fn signed_bytes(signed: &SignedRecovery) -> Result<Vec<u8>> {
    recovery_wire::encode(signed).map_err(|e| err(e.to_string()))
}
pub fn decode_signed(bytes: &[u8]) -> Result<SignedRecovery> {
    recovery_wire::decode(bytes).map_err(|e| err(e.to_string()))
}
/// A sponsored envelope's wire bytes, and back.
pub fn envelope_bytes(sponsored: &SponsoredRecovery) -> Result<Vec<u8>> {
    sponsor_wire::encode(sponsored).map_err(|e| err(e.to_string()))
}
pub fn decode_envelope(bytes: &[u8]) -> Result<SponsoredRecovery> {
    sponsor_wire::decode(bytes).map_err(|e| err(e.to_string()))
}

/// The transaction bytes to submit: `{"kind":"recovery","envelope_base64":...}`.
pub fn transaction(sponsored: &SponsoredRecovery) -> Result<Vec<u8>> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let envelope = sponsor_wire::encode(sponsored).map_err(|e| err(e.to_string()))?;
    serde_json::to_vec(&serde_json::json!({
        "kind": "recovery",
        "envelope_base64": STANDARD.encode(envelope),
    }))
    .map_err(|e| err(e.to_string()))
}

/// Lowercase hexadecimal.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
/// Exactly 32 bytes from lowercase hexadecimal.
pub fn parse_hex32(value: &str) -> Result<[u8; 32]> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(err("expected a lowercase 64-hex ID"));
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[2 * i..2 * i + 2], 16).map_err(|e| err(e.to_string()))?;
    }
    Ok(out)
}

mod optional_hex32 {
    use serde::{de::Error, Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(
        value: &Option<[u8; 32]>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(bytes) => serializer.serialize_some(&super::hex(bytes)),
            None => serializer.serialize_none(),
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<[u8; 32]>, D::Error> {
        let Some(value) = Option::<String>::deserialize(deserializer)? else {
            return Ok(None);
        };
        super::parse_hex32(&value)
            .map(Some)
            .map_err(|e| D::Error::custom(e.0))
    }
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;
