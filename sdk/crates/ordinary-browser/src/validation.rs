//! Public validation adapted from SDK ordinary_v2. No signature verification.
use dytallix_protocol_types::{
    address::{AccountAddress, AddressNetwork},
    ordinary::{Action, OrdinaryTransaction, SignedOrdinary},
    ordinary_client::{AccountView, CommittedContext, ProfileView, ReceiptOutcome, ReceiptView},
    ordinary_fees::FeeProfile,
    recovery::{KeyIdentity, RecoveryDomain},
};
use dytallix_protocol_types::{ordinary as wire, ordinary_fees};
use serde::{Deserialize, Serialize};
#[derive(Debug, thiserror::Error)]
#[error("ordinary-v2: {0}")]
pub struct Error(pub String);
pub type Result<T> = std::result::Result<T, Error>;
pub(crate) fn error(e: impl std::fmt::Display) -> Error {
    Error(e.to_string())
}
fn need(ok: bool, message: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error(message.into()))
    }
}

/// Explicit expected state selected by the caller. This is not a proof object.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SigningContext {
    pub domain: RecoveryDomain,
    pub current_key: KeyIdentity,
    #[serde(with = "decimal_u64")]
    pub authorization_generation: u64,
    #[serde(with = "decimal_u64")]
    pub spending_nonce: u64,
    pub committed: CommittedContext,
    pub profile_digest: [u8; 32],
    pub protected: bool,
}

/// Compare both RPC views against caller-selected state before using their profile.
/// Successful comparison does not authenticate the endpoint or its state.
pub fn validate_views(
    expected: &SigningContext,
    profile: &ProfileView,
    account: &AccountView,
) -> Result<FeeProfile> {
    let fee_profile = validate_identity_views(expected, profile, account)?;
    validate_context(&fee_profile, expected)?;
    Ok(fee_profile)
}

/// Check exact identity and profile metadata without asserting signability.
/// Protected accounts, exhausted nonces, and later activation can still have
/// valid identity records. This function does not authenticate reported state.
pub fn validate_identity_views(
    expected: &SigningContext,
    profile: &ProfileView,
    account: &AccountView,
) -> Result<FeeProfile> {
    need(
        profile.version == 1 && account.version == 1,
        "unsupported query view version",
    )?;
    need(profile.enabled, "ordinary execution is disabled")?;
    let config = profile
        .config
        .as_ref()
        .ok_or_else(|| Error("ordinary profile is missing".into()))?;
    need(
        config.version == 1 && config.max_transport_bytes > 0,
        "invalid public ordinary configuration",
    )?;
    need(
        profile.context == expected.committed && account.context == expected.committed,
        "query contexts differ from expected committed state",
    )?;
    need(
        account.domain.network == expected.domain.network
            && account.domain.chain_id == expected.domain.chain_id
            && account.domain.genesis_digest == expected.domain.genesis_digest
            && account.domain.account_id == expected.domain.account_id
            && account.account_id == expected.domain.account_id,
        "account domain differs",
    )?;
    need(
        account.current_key == expected.current_key
            && account.authorization_generation == expected.authorization_generation
            && account.spending_nonce == expected.spending_nonce
            && account.protected == expected.protected,
        "account authority differs",
    )?;
    need(
        account.profile_digest == expected.profile_digest,
        "account profile digest differs",
    )?;
    let address = AccountAddress::decode(network(expected.domain.network)?, &account.address)
        .map_err(error)?;
    need(
        address.account_id() == &expected.domain.account_id,
        "account address differs",
    )?;
    validate_identity_context(&config.fee_profile, expected)?;
    Ok(config.fee_profile.clone())
}

fn network(code: u8) -> Result<AddressNetwork> {
    match code {
        1 => Ok(AddressNetwork::Mainnet),
        2 => Ok(AddressNetwork::Testnet),
        3 => Ok(AddressNetwork::Development),
        _ => Err(Error("unsupported network".into())),
    }
}
fn validate_identity_context(profile: &FeeProfile, context: &SigningContext) -> Result<()> {
    profile.validate().map_err(error)?;
    network(context.domain.network)?;
    need(
        context.committed.chain_id == context.domain.chain_id
            && context.committed.genesis_digest == context.domain.genesis_digest,
        "committed chain or genesis differs",
    )?;
    need(
        ordinary_fees::profile_digest(profile).map_err(error)? == context.profile_digest,
        "fee profile digest differs",
    )?;
    need(
        profile
            .limits
            .allowed_algorithms
            .contains(&context.current_key.algorithm),
        "current algorithm is outside account role",
    )?;
    wire::key_id(&context.current_key).map_err(error)?;
    Ok(())
}
fn validate_context(profile: &FeeProfile, context: &SigningContext) -> Result<()> {
    validate_identity_context(profile, context)?;
    let target_height = context
        .committed
        .height
        .checked_add(1)
        .ok_or_else(|| Error("committed height is exhausted".into()))?;
    need(
        target_height >= profile.activation_height,
        "fee profile is not active at the next height",
    )?;
    need(!context.protected, "account is protected")?;
    need(
        context.spending_nonce < u64::MAX,
        "spending nonce is exhausted",
    )?;
    Ok(())
}

/// Parse an unsigned token amount into six-place base units with checked arithmetic.
/// Integer parts are canonical; fractional parts contain one to six digits.
pub fn parse_token_units(value: &str) -> Result<u128> {
    let (whole, fraction) = match value.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (value, None),
    };
    let whole = wire::parse_decimal_u128(whole).map_err(error)?;
    let fraction = match fraction {
        None => 0,
        Some(text) => {
            need(
                !text.is_empty() && text.len() <= 6 && text.bytes().all(|b| b.is_ascii_digit()),
                "token fraction requires one to six decimal digits",
            )?;
            let n: u128 = text.parse().map_err(error)?;
            n.checked_mul(10u128.pow(6 - text.len() as u32))
                .ok_or_else(|| Error("token fraction overflow".into()))?
        }
    };
    whole
        .checked_mul(1_000_000)
        .and_then(|n| n.checked_add(fraction))
        .ok_or_else(|| Error("token amount overflow".into()))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Anchor {
    pub domain: RecoveryDomain,
    pub committed: CommittedContext,
    pub profile_digest: [u8; 32],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub actions: Vec<Action>,
    pub memo: String,
    #[serde(with = "decimal_u64")]
    pub expiry_height: u64,
    #[serde(with = "decimal_u64")]
    pub gas_limit: u64,
    #[serde(with = "decimal_input_u128")]
    pub maximum_fee: u128,
}
pub fn check_anchor(
    context: &SigningContext,
    anchor: &Anchor,
    current_key: &KeyIdentity,
) -> Result<()> {
    need(
        context.domain == anchor.domain
            && context.committed == anchor.committed
            && context.profile_digest == anchor.profile_digest,
        "context differs from caller-selected anchor",
    )?;
    need(
        context.current_key == *current_key,
        "current public key differs from captured authority",
    )
}
pub fn body(
    profile: &FeeProfile,
    context: &SigningContext,
    intent: Intent,
) -> Result<OrdinaryTransaction> {
    validate_context(profile, context)?;
    let body = OrdinaryTransaction {
        domain: context.domain.clone(),
        authorization_generation: context.authorization_generation,
        spending_nonce: context.spending_nonce,
        key: context.current_key.clone(),
        expiry_height: intent.expiry_height,
        ordinary_fee_contract_version: profile.ordinary_fee_contract_version,
        fee_profile_version: profile.version,
        fee_profile_digest: context.profile_digest,
        fee_denomination: profile.denomination,
        maximum_fee: intent.maximum_fee,
        gas_limit: intent.gas_limit,
        memo: intent.memo,
        actions: intent.actions,
    };
    check_body(&body, profile, context)?;
    Ok(body)
}
pub fn check_body(
    body: &OrdinaryTransaction,
    profile: &FeeProfile,
    context: &SigningContext,
) -> Result<()> {
    validate_context(profile, context)?;
    need(
        body.domain == context.domain
            && body.key == context.current_key
            && body.authorization_generation == context.authorization_generation
            && body.spending_nonce == context.spending_nonce,
        "transaction authority differs from expected state",
    )?;
    check_profile_binding(body, profile)?;
    let next = context
        .committed
        .height
        .checked_add(1)
        .ok_or_else(|| error("committed height exhausted"))?;
    let life = body
        .expiry_height
        .checked_sub(next)
        .ok_or_else(|| error("transaction expired"))?;
    need(
        life > 0 && life <= profile.limits.max_expiry_lifetime,
        "transaction expiry is outside next-height lifetime",
    )?;
    wire::signing_bytes(body, &profile.limits).map_err(error)?;
    Ok(())
}
pub fn check_profile_binding(body: &OrdinaryTransaction, profile: &FeeProfile) -> Result<()> {
    profile.validate().map_err(error)?;
    need(
        body.ordinary_fee_contract_version == profile.ordinary_fee_contract_version
            && body.fee_profile_version == profile.version
            && body.fee_profile_digest == ordinary_fees::profile_digest(profile).map_err(error)?
            && body.fee_denomination == profile.denomination,
        "transaction fee profile differs",
    )?;
    profile
        .validate_request(body.gas_limit, body.maximum_fee)
        .map_err(error)
}
mod decimal_input_u128 {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &u128, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u128, D::Error> {
        let s = String::deserialize(d)?;
        dytallix_protocol_types::ordinary::parse_decimal_u128(&s).map_err(serde::de::Error::custom)
    }
}
pub fn validate_receipt(
    receipt: &ReceiptView,
    signed: &SignedOrdinary,
    profile: &FeeProfile,
) -> Result<()> {
    let body = &signed.body;
    // Public codec consistency only. The browser must verify with Noble.
    wire::encode(signed, &profile.limits).map_err(error)?;
    profile
        .validate_request(body.gas_limit, body.maximum_fee)
        .map_err(error)?;
    let digest = ordinary_fees::profile_digest(profile).map_err(error)?;
    need(
        receipt.version == 1
            && receipt.transaction_id
                == wire::transaction_id(body, &profile.limits).map_err(error)?
            && receipt.envelope_hash
                == wire::envelope_hash(signed, &profile.limits).map_err(error)?
            && receipt.actor == body.domain.account_id,
        "receipt identity differs",
    )?;
    need(
        receipt.context.chain_id == body.domain.chain_id
            && receipt.context.genesis_digest == body.domain.genesis_digest
            && receipt.block_height > 0
            && receipt.block_height <= receipt.context.height
            && receipt.block_height >= profile.activation_height
            && receipt.block_height < body.expiry_height,
        "receipt chain or height differs",
    )?;
    need(
        receipt.contract_version == profile.ordinary_fee_contract_version
            && body.ordinary_fee_contract_version == profile.ordinary_fee_contract_version
            && receipt.profile_version == profile.version
            && body.fee_profile_version == profile.version
            && receipt.profile_digest == digest
            && body.fee_profile_digest == digest
            && body.fee_denomination == profile.denomination,
        "receipt profile differs",
    )?;
    need(
        receipt.gas_limit == body.gas_limit
            && receipt.reserved_cap == body.maximum_fee
            && receipt.gas_used <= body.gas_limit
            && receipt.metadata_gas == profile.receipt_metadata_cost
            && receipt.nonce_before == body.spending_nonce
            && Some(receipt.nonce_after) == body.spending_nonce.checked_add(1),
        "receipt accounting fields differ",
    )?;
    let wire_len = u64::try_from(wire::encode(signed, &profile.limits).map_err(error)?.len())
        .map_err(error)?;
    let validation_floor = wire_len
        .checked_mul(profile.wire_byte_cost)
        .and_then(|n| n.checked_add(profile.transaction_overhead))
        .and_then(|n| n.checked_add(profile.signature_costs[&body.key.algorithm]))
        .and_then(|n| n.checked_add(profile.receipt_metadata_cost))
        .ok_or_else(|| Error("receipt validation gas overflow".into()))?;
    need(
        receipt.gas_used >= validation_floor,
        "receipt omits mandatory validation or metadata gas",
    )?;
    let expected_charge = if receipt.outcome == ReceiptOutcome::OutOfGas {
        need(
            receipt.gas_used == body.gas_limit,
            "out-of-gas receipt must consume its limit",
        )?;
        u128::from(body.gas_limit).checked_mul(u128::from(profile.gas_price))
    } else {
        u128::from(receipt.gas_used.max(profile.minimum_gas))
            .checked_mul(u128::from(profile.gas_price))
    }
    .ok_or_else(|| Error("receipt charge overflow".into()))?;
    need(
        receipt.charge == expected_charge
            && receipt.charge.checked_add(receipt.released_cap) == Some(receipt.reserved_cap),
        "receipt fee conservation differs",
    )?;
    match receipt.outcome {
        ReceiptOutcome::Success => need(
            receipt.failing_action.is_none()
                && receipt.failure_phase.is_none()
                && receipt.rule_class.is_none()
                && receipt.rule_code.is_none(),
            "success receipt contains failure fields",
        )?,
        ReceiptOutcome::OutOfGas => {
            valid_failure_index(receipt, body)?;
            need(
                receipt.rule_code.as_deref() == Some("OUT_OF_GAS")
                    && receipt.rule_class.is_none()
                    && matches!(
                        receipt.failure_phase.as_deref(),
                        Some("action" | "ACTION" | "WRITE")
                    ),
                "invalid out-of-gas classification",
            )?;
        }
        ReceiptOutcome::ApplicationFailure => {
            valid_failure_index(receipt, body)?;
            need(
                matches!(
                    receipt.failure_phase.as_deref(),
                    Some("action" | "APPLICATION")
                ) && matches!(
                    (receipt.rule_class.as_deref(), receipt.rule_code.as_deref()),
                    (
                        Some("ACTION_CAPACITY"),
                        Some("SEND_RECIPIENT_CAPACITY" | "LIFECYCLE_CAPACITY")
                    ) | (
                        Some("ACTION_STATE_PRECONDITION"),
                        Some(
                            "DMS_INACTIVITY_DELAY"
                                | "UNBOND_ALREADY_RELEASED"
                                | "UNBOND_PENALTIES_PENDING"
                                | "UNBOND_NOT_MATURE"
                                | "BOND_TARGET_UNAVAILABLE"
                                | "UNBOND_PRINCIPAL_UNAVAILABLE"
                                | "VALIDATOR_ALREADY_REGISTERED"
                                | "VALIDATOR_TARGET_UNAVAILABLE"
                                | "VALIDATOR_EXPOSURE_BARRED"
                                | "CONSENSUS_KEY_ALREADY_USED"
                                | "BOND_PRINCIPAL_ZERO"
                                | "VALIDATOR_SELF_BOND_MINIMUM"
                                | "EXIT_PENDING_ADDITIONS"
                                | "VALIDATOR_SET_EMPTY"
                        )
                    )
                ),
                "invalid application failure classification",
            )?;
        }
    }
    Ok(())
}
fn valid_failure_index(receipt: &ReceiptView, body: &OrdinaryTransaction) -> Result<()> {
    need(
        receipt
            .failing_action
            .is_some_and(|index| usize::from(index) < body.actions.len()),
        "receipt failure requires an action inside the body",
    )
}
mod decimal_u64 {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &u64, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<u64, D::Error> {
        let s = String::deserialize(d)?;
        dytallix_protocol_types::ordinary::parse_decimal_u64(&s).map_err(serde::de::Error::custom)
    }
}
