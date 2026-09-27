//! Explicit ordinary-v3 governance transactions (clients v1, E04 gap 8).
//!
//! A v3 transaction carries exactly one governance action (proposal, deposit
//! or vote) and is signed against the v3 fee profile a node reports at
//! `/ordinary/profile_v3`. Account identity comes from the same views and
//! [`SigningContext`] as ordinary-v2, under the same trust rule: matching
//! views do not authenticate the endpoint or its state. Once the chain admits
//! a v3 transaction every governance rule failure is paid: the fee and nonce
//! apply and the action's effects are discarded. The chain keeps no
//! per-transaction v3 receipt yet (E04 gap 12); the spent nonce is the only
//! committed evidence.
use crate::ordinary_v2::{
    self, error, AccountView, Error, OrdinarySigner, ProfileView, Result, SigningContext,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use dytallix_core::{keypair::KeyScheme, signature::verify_for_scheme};
pub use dytallix_protocol_types::{
    governance_action::{
        FeeValues, ParameterChange, RegistryChange, CLASS_PARAMETER_CHANGE,
        CLASS_VALIDATOR_REGISTRY,
    },
    ordinary_client::GovernanceProfileView,
    ordinary_fees_v3::FeeProfileV3,
    ordinary_v3::{Action, OrdinaryTransaction, SignedOrdinary, V3Limits, VoteChoice},
};
use dytallix_protocol_types::{
    ordinary_fees_v3,
    ordinary_v3::{self as wire, Denomination},
};
use serde::{Deserialize, Serialize};

pub const TRANSPORT_TYPE: &str = "ordinary_v3";

fn need(ok: bool, message: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error(message.into()))
    }
}

/// Compare the account, ordinary and governance views against caller-selected
/// state and return the v3 profile to sign against. The account identity
/// checks are ordinary-v2's; the v2 profile's digest stays in the context.
pub fn validate_views(
    expected: &SigningContext,
    profile: &ProfileView,
    governance: &GovernanceProfileView,
    account: &AccountView,
) -> Result<FeeProfileV3> {
    ordinary_v2::validate_identity_views(expected, profile, account)?;
    need(
        governance.version == 1,
        "unsupported governance view version",
    )?;
    need(
        governance.enabled && governance.next_proposal_id > 0,
        "governance execution is disabled",
    )?;
    need(
        governance.context == expected.committed,
        "governance context differs from expected committed state",
    )?;
    let fee_profile = governance
        .fee_profile
        .as_ref()
        .ok_or_else(|| Error("governance fee profile is missing".into()))?;
    validate_context(fee_profile, expected)?;
    Ok(fee_profile.clone())
}

fn validate_context(profile: &FeeProfileV3, context: &SigningContext) -> Result<()> {
    profile.validate().map_err(error)?;
    need(
        context.committed.chain_id == context.domain.chain_id
            && context.committed.genesis_digest == context.domain.genesis_digest,
        "committed chain or genesis differs",
    )?;
    need(
        profile
            .base
            .limits
            .allowed_algorithms
            .contains(&context.current_key.algorithm),
        "current algorithm is outside the governance profile",
    )?;
    wire::key_id(&context.current_key).map_err(error)?;
    let target_height = next_height(context)?;
    need(
        target_height >= profile.activation_height,
        "governance fee profile is not active at the next height",
    )?;
    need(!context.protected, "account is protected")?;
    need(
        context.spending_nonce < u64::MAX,
        "spending nonce is exhausted",
    )
}
fn next_height(context: &SigningContext) -> Result<u64> {
    context
        .committed
        .height
        .checked_add(1)
        .ok_or_else(|| Error("committed height is exhausted".into()))
}

/// A proposal action with its digest. `proposal_id` must be the view's
/// `next_proposal_id`; another ID is a paid rule failure.
pub fn proposal(proposal_id: u64, action_class: u16, action_data: Vec<u8>) -> Result<Action> {
    let action_digest =
        wire::governance_action_digest(action_class, &action_data).map_err(error)?;
    Ok(Action::GovernanceProposal {
        proposal_id,
        action_class,
        action_data,
        action_digest,
    })
}
pub fn parameter_change(proposal_id: u64, change: &ParameterChange) -> Result<Action> {
    proposal(proposal_id, CLASS_PARAMETER_CHANGE, change.action_data())
}
pub fn registry_change(proposal_id: u64, change: &RegistryChange) -> Result<Action> {
    proposal(proposal_id, CLASS_VALIDATOR_REGISTRY, change.action_data())
}
pub fn deposit(proposal_id: u64, amount_udgt: u128) -> Action {
    Action::GovernanceDeposit {
        proposal_id,
        amount_udgt,
    }
}
pub fn vote(proposal_id: u64, choice: VoteChoice) -> Action {
    Action::GovernanceVote {
        proposal_id,
        choice,
    }
}

/// Immutable v3 body whose fields were checked against explicit expected state.
#[derive(Clone, Debug)]
pub struct PreparedGovernance {
    body: OrdinaryTransaction,
    profile: FeeProfileV3,
    context: SigningContext,
}
pub fn prepare(
    profile: &FeeProfileV3,
    context: &SigningContext,
    action: Action,
    memo: String,
    expiry_height: u64,
    gas_limit: u64,
    maximum_fee: u128,
) -> Result<PreparedGovernance> {
    PreparedGovernance::from_body(
        OrdinaryTransaction {
            domain: context.domain.clone(),
            authorization_generation: context.authorization_generation,
            spending_nonce: context.spending_nonce,
            key: context.current_key.clone(),
            expiry_height,
            ordinary_fee_contract_version: ordinary_fees_v3::ORDINARY_FEE_CONTRACT_VERSION,
            fee_profile_version: profile.version,
            fee_profile_digest: profile_digest(profile)?,
            fee_denomination: Denomination::Udrt,
            maximum_fee,
            gas_limit,
            memo,
            actions: vec![action],
        },
        profile,
        context,
    )
}
impl PreparedGovernance {
    pub fn from_body(
        body: OrdinaryTransaction,
        profile: &FeeProfileV3,
        context: &SigningContext,
    ) -> Result<Self> {
        validate_context(profile, context)?;
        need(
            body.domain == context.domain
                && body.key == context.current_key
                && body.authorization_generation == context.authorization_generation
                && body.spending_nonce == context.spending_nonce,
            "transaction authority differs from expected state",
        )?;
        let target_height = next_height(context)?;
        // One governance action, the exact profile, its activation and the fee cap.
        profile
            .validate_signed_request(&body, target_height)
            .map_err(error)?;
        let lifetime = body
            .expiry_height
            .checked_sub(target_height)
            .ok_or_else(|| Error("transaction has expired".into()))?;
        need(
            lifetime > 0 && lifetime <= profile.limits().max_expiry_lifetime,
            "transaction expiry is outside next-height lifetime",
        )?;
        Ok(Self {
            body,
            profile: profile.clone(),
            context: context.clone(),
        })
    }
    pub fn body(&self) -> &OrdinaryTransaction {
        &self.body
    }
    pub fn signing_bytes(&self) -> Result<Vec<u8>> {
        wire::signing_bytes(&self.body, &self.profile.limits()).map_err(error)
    }
    pub fn sign(&self, signer: &impl OrdinarySigner) -> Result<SignedOrdinary> {
        // Recheck the stored expected state. Live-state refresh remains caller work.
        Self::from_body(self.body.clone(), &self.profile, &self.context)?;
        need(
            signer.algorithm() == self.body.key.algorithm
                && signer.public_key() == self.body.key.public_key,
            "signer differs from current account key",
        )?;
        let signature = signer.sign_message(&self.signing_bytes()?)?;
        let signed = SignedOrdinary {
            body: self.body.clone(),
            signature,
        };
        verify_signature(&signed, &self.profile.limits())?;
        Ok(signed)
    }
}

pub fn verify_signature(signed: &SignedOrdinary, limits: &V3Limits) -> Result<()> {
    wire::encode(signed, limits).map_err(error)?;
    let scheme = match signed.body.key.algorithm.as_str() {
        "mldsa65" => KeyScheme::MlDsa65,
        "mldsa87" => KeyScheme::MlDsa87,
        _ => return Err(Error("unsupported ordinary algorithm".into())),
    };
    let bytes = wire::signing_bytes(&signed.body, limits).map_err(error)?;
    let valid = verify_for_scheme(
        scheme,
        &signed.body.key.public_key,
        &bytes,
        &signed.signature,
    )
    .map_err(error)?;
    need(valid, "ordinary signature is invalid")
}
pub fn transaction_id(body: &OrdinaryTransaction, limits: &V3Limits) -> Result<[u8; 32]> {
    wire::transaction_id(body, limits).map_err(error)
}
pub fn envelope_hash(signed: &SignedOrdinary, limits: &V3Limits) -> Result<[u8; 32]> {
    wire::envelope_hash(signed, limits).map_err(error)
}
pub fn profile_digest(profile: &FeeProfileV3) -> Result<[u8; 32]> {
    ordinary_fees_v3::profile_digest(profile).map_err(error)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Transport {
    #[serde(rename = "type")]
    kind: String,
    envelope_base64: String,
}
/// The node's v3 transport. Its bound is the ordinary configuration's
/// `max_transport_bytes`, as for v2.
pub fn encode_transport(
    signed: &SignedOrdinary,
    limits: &V3Limits,
    max_transport_bytes: usize,
) -> Result<Vec<u8>> {
    need(max_transport_bytes > 0, "explicit transport bound required")?;
    let bytes = wire::encode(signed, limits).map_err(error)?;
    let raw = serde_json::to_vec(&Transport {
        kind: TRANSPORT_TYPE.into(),
        envelope_base64: STANDARD.encode(bytes),
    })
    .map_err(error)?;
    need(
        raw.len() <= max_transport_bytes,
        "ordinary transport exceeds bound",
    )?;
    Ok(raw)
}
pub fn decode_transport(
    raw: &[u8],
    limits: &V3Limits,
    max_transport_bytes: usize,
) -> Result<SignedOrdinary> {
    limits.validate().map_err(error)?;
    need(
        max_transport_bytes > 0 && raw.len() <= max_transport_bytes,
        "ordinary transport exceeds explicit bound",
    )?;
    let t: Transport = serde_json::from_slice(raw).map_err(error)?;
    need(t.kind == TRANSPORT_TYPE, "wrong ordinary transport type")?;
    need(
        t.envelope_base64.len() as u64 <= u64::from(limits.max_wire_bytes).div_ceil(3) * 4,
        "encoded envelope exceeds bound",
    )?;
    let bytes = STANDARD.decode(&t.envelope_base64).map_err(error)?;
    need(
        STANDARD.encode(&bytes) == t.envelope_base64,
        "noncanonical envelope base64",
    )?;
    wire::decode(&bytes, limits).map_err(error)
}
