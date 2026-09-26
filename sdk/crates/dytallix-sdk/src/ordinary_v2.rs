//! Explicit ordinary-v2 local integration. Legacy transaction APIs are unchanged.
//!
//! Callers must establish trust in the supplied committed context. RPC metadata
//! and matching hashes do not constitute a consensus proof. Preparation and
//! signing do not reserve funds, prove authority at execution, or submit work.
use base64::{engine::general_purpose::STANDARD, Engine};
use dytallix_core::{
    keypair::{DytallixKeypair, KeyScheme},
    signature::verify_for_scheme,
};
pub use dytallix_protocol_types::{
    address::{AccountAddress, AddressNetwork, OriginKeyAlgorithm},
    ordinary::{Action, Denomination, Limits, OrdinaryTransaction, SignedOrdinary},
    ordinary_client::{
        AccountView, CommittedContext, ProfileView, PublicOrdinaryConfig, ReceiptOutcome,
        ReceiptView,
    },
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
    #[cfg(feature = "strict-local-mldsa65")]
    need(
        context.current_key.algorithm == "mldsa65",
        "strict local profile requires exactly mldsa65",
    )?;
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

/// A cap quote bounds a future charge. It is not an execution fee estimate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FeeQuote {
    #[serde(with = "decimal_u64")]
    pub gas_limit: u64,
    #[serde(with = "decimal_u128")]
    pub required_cap: u128,
    #[serde(with = "decimal_u128")]
    pub minimum_charge: u128,
}
impl FeeQuote {
    pub fn from_profile(profile: &FeeProfile, gas_limit: u64) -> Result<Self> {
        let required_cap = u128::from(gas_limit)
            .checked_mul(u128::from(profile.gas_price))
            .ok_or_else(|| Error("fee cap overflow".into()))?;
        profile
            .validate_request(gas_limit, required_cap)
            .map_err(error)?;
        let minimum_charge = u128::from(profile.minimum_gas)
            .checked_mul(u128::from(profile.gas_price))
            .ok_or_else(|| Error("minimum charge overflow".into()))?;
        Ok(Self {
            gas_limit,
            required_cap,
            minimum_charge,
        })
    }
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

/// Immutable body whose fields were checked against explicit expected state.
#[derive(Clone, Debug)]
pub struct PreparedTransaction {
    body: OrdinaryTransaction,
    profile: FeeProfile,
    context: SigningContext,
}
#[allow(clippy::too_many_arguments)]
pub fn prepare(
    profile: &FeeProfile,
    context: &SigningContext,
    actions: Vec<Action>,
    memo: String,
    expiry_height: u64,
    gas_limit: u64,
    maximum_fee: u128,
) -> Result<PreparedTransaction> {
    PreparedTransaction::from_body(
        OrdinaryTransaction {
            domain: context.domain.clone(),
            authorization_generation: context.authorization_generation,
            spending_nonce: context.spending_nonce,
            key: context.current_key.clone(),
            expiry_height,
            ordinary_fee_contract_version: profile.ordinary_fee_contract_version,
            fee_profile_version: profile.version,
            fee_profile_digest: context.profile_digest,
            fee_denomination: Denomination::Udrt,
            maximum_fee,
            gas_limit,
            memo,
            actions,
        },
        profile,
        context,
    )
}
impl PreparedTransaction {
    pub fn from_body(
        body: OrdinaryTransaction,
        profile: &FeeProfile,
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
        need(
            body.ordinary_fee_contract_version == profile.ordinary_fee_contract_version
                && body.fee_profile_version == profile.version
                && body.fee_profile_digest == context.profile_digest
                && body.fee_denomination == profile.denomination,
            "transaction fee profile differs",
        )?;
        profile
            .validate_request(body.gas_limit, body.maximum_fee)
            .map_err(error)?;
        let target_height = context
            .committed
            .height
            .checked_add(1)
            .ok_or_else(|| Error("committed height is exhausted".into()))?;
        let lifetime = body
            .expiry_height
            .checked_sub(target_height)
            .ok_or_else(|| Error("transaction has expired".into()))?;
        need(
            lifetime > 0 && lifetime <= profile.limits.max_expiry_lifetime,
            "transaction expiry is outside next-height lifetime",
        )?;
        wire::signing_bytes(&body, &profile.limits).map_err(error)?;
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
        wire::signing_bytes(&self.body, &self.profile.limits).map_err(error)
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
        verify_signature(&signed, &self.profile.limits)?;
        Ok(signed)
    }
}
/// Sign the complete supplied bytes with pure ML-DSA and an empty FIPS context.
/// Hardware and remote signers can implement this trait. The returned signature
/// is checked before it leaves PreparedTransaction::sign.
pub trait OrdinarySigner {
    fn algorithm(&self) -> &str;
    fn public_key(&self) -> &[u8];
    fn sign_message(&self, signing_bytes: &[u8]) -> Result<Vec<u8>>;
}
pub struct KeypairSigner<'a>(&'a DytallixKeypair);
impl<'a> KeypairSigner<'a> {
    pub fn new(key: &'a DytallixKeypair) -> Result<Self> {
        key.scheme().require_available().map_err(error)?;
        need(
            matches!(key.scheme(), KeyScheme::MlDsa65 | KeyScheme::MlDsa87),
            "ordinary signing requires exact ML-DSA-65 or ML-DSA-87",
        )?;
        Ok(Self(key))
    }
}
impl OrdinarySigner for KeypairSigner<'_> {
    fn algorithm(&self) -> &str {
        match self.0.scheme() {
            KeyScheme::MlDsa65 => "mldsa65",
            KeyScheme::MlDsa87 => "mldsa87",
            KeyScheme::SlhDsa => unreachable!("constructor rejects SLH-DSA"),
        }
    }
    fn public_key(&self) -> &[u8] {
        self.0.public_key()
    }
    fn sign_message(&self, bytes: &[u8]) -> Result<Vec<u8>> {
        self.0.sign(bytes).map_err(error)
    }
}
pub struct MlDsa65Signer<'a>(KeypairSigner<'a>);
impl<'a> MlDsa65Signer<'a> {
    pub fn new(key: &'a DytallixKeypair) -> Result<Self> {
        key.scheme().require_available().map_err(error)?;
        need(
            key.scheme() == KeyScheme::MlDsa65,
            "ML-DSA-65 signer requires ML-DSA-65 key",
        )?;
        Ok(Self(KeypairSigner::new(key)?))
    }
}
impl OrdinarySigner for MlDsa65Signer<'_> {
    fn algorithm(&self) -> &str {
        self.0.algorithm()
    }
    fn public_key(&self) -> &[u8] {
        self.0.public_key()
    }
    fn sign_message(&self, bytes: &[u8]) -> Result<Vec<u8>> {
        self.0.sign_message(bytes)
    }
}
pub fn verify_signature(signed: &SignedOrdinary, limits: &Limits) -> Result<()> {
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
pub fn transaction_id(body: &OrdinaryTransaction, limits: &Limits) -> Result<[u8; 32]> {
    wire::transaction_id(body, limits).map_err(error)
}
pub fn envelope_hash(signed: &SignedOrdinary, limits: &Limits) -> Result<[u8; 32]> {
    wire::envelope_hash(signed, limits).map_err(error)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Transport {
    #[serde(rename = "type")]
    kind: String,
    envelope_base64: String,
}
pub fn encode_transport(
    signed: &SignedOrdinary,
    limits: &Limits,
    max_transport_bytes: usize,
) -> Result<Vec<u8>> {
    need(max_transport_bytes > 0, "explicit transport bound required")?;
    let bytes = wire::encode(signed, limits).map_err(error)?;
    let raw = serde_json::to_vec(&Transport {
        kind: "ordinary_v2".into(),
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
    limits: &Limits,
    max_transport_bytes: usize,
) -> Result<SignedOrdinary> {
    limits.validate().map_err(error)?;
    need(
        max_transport_bytes > 0 && raw.len() <= max_transport_bytes,
        "ordinary transport exceeds explicit bound",
    )?;
    let t: Transport = serde_json::from_slice(raw).map_err(error)?;
    need(t.kind == "ordinary_v2", "wrong ordinary transport type")?;
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

/// Check a reported committed receipt against the exact signed envelope and
/// profile. This checks consistency, not consensus inclusion or endpoint trust.
pub fn validate_receipt(
    receipt: &ReceiptView,
    signed: &SignedOrdinary,
    profile: &FeeProfile,
) -> Result<()> {
    let body = &signed.body;
    verify_signature(signed, &profile.limits)?;
    profile
        .validate_request(body.gas_limit, body.maximum_fee)
        .map_err(error)?;
    let digest = ordinary_fees::profile_digest(profile).map_err(error)?;
    need(
        receipt.version == 1
            && receipt.transaction_id == transaction_id(body, &profile.limits)?
            && receipt.envelope_hash == envelope_hash(signed, &profile.limits)?
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
pub fn profile_digest(profile: &FeeProfile) -> Result<[u8; 32]> {
    ordinary_fees::profile_digest(profile).map_err(error)
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
mod decimal_u128 {
    use serde::Serializer;
    pub fn serialize<S: Serializer>(v: &u128, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }
}
