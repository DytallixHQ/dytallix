//! Public ordinary-v2 browser codec. No private keys, signing, RNG or network I/O.
//! Byte consistency does not establish signature validity or consensus inclusion.
mod strict_json;
mod validation;
use base64::{engine::general_purpose::STANDARD, Engine};
use dytallix_protocol_types::{
    address::{AccountAddress, AddressNetwork, OriginKeyAlgorithm},
    ordinary::{self as wire, Limits, OrdinaryTransaction, SignedOrdinary},
    ordinary_client::{AccountView, ProfileView, ReceiptView},
    ordinary_fees::{self, FeeProfile},
    recovery::KeyIdentity,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use validation::{Anchor, Intent, SigningContext};
use wasm_bindgen::prelude::*;
type Result<T> = std::result::Result<T, String>;
pub const MAX_BINARY_BYTES: usize = 1_048_576;
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn need(ok: bool, message: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(message.into())
    }
}
fn limits(input: &str) -> Result<Limits> {
    let mut value = strict_json::value(input)?;
    let lifetime = value
        .get("max_expiry_lifetime")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "max_expiry_lifetime requires a decimal string".to_owned())?;
    let parsed = wire::parse_decimal_u64(lifetime).map_err(err)?;
    value["max_expiry_lifetime"] = json!(parsed);
    let l: Limits = serde_json::from_value(value).map_err(err)?;
    bounded_limits(&l)?;
    Ok(l)
}
fn bounded_limits(l: &Limits) -> Result<()> {
    l.validate().map_err(err)?;
    need(
        l.max_wire_bytes as usize <= MAX_BINARY_BYTES,
        "selected wire limit exceeds browser bound",
    )
}
fn profile(input: &str) -> Result<FeeProfile> {
    let p: FeeProfile = strict_json::parse(input)?;
    p.validate().map_err(err)?;
    bounded_limits(&p.limits)?;
    Ok(p)
}
fn view(input: &str) -> Result<ProfileView> {
    let p: ProfileView = strict_json::parse(input)?;
    need(
        p.version == 1 && p.enabled,
        "ordinary profile view is unsupported or disabled",
    )?;
    let config = p
        .config
        .as_ref()
        .ok_or_else(|| "ordinary profile missing".to_owned())?;
    need(
        config.version == 1
            && config.max_transport_bytes > 0
            && config.max_transport_bytes <= MAX_BINARY_BYTES as u64,
        "invalid browser transport bound",
    )?;
    config.fee_profile.validate().map_err(err)?;
    bounded_limits(&config.fee_profile.limits)?;
    Ok(p)
}
fn network(n: u8) -> Result<AddressNetwork> {
    match n {
        1 => Ok(AddressNetwork::Mainnet),
        2 => Ok(AddressNetwork::Testnet),
        3 => Ok(AddressNetwork::Development),
        _ => Err("unsupported account network".into()),
    }
}
fn hex32(value: &str) -> Result<[u8; 32]> {
    need(
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "expected lowercase 64-hex account ID",
    )?;
    hex::decode(value)
        .map_err(err)?
        .try_into()
        .map_err(|_| "invalid account ID size".into())
}

/// Generic duplicate-key validation. Typed exports also reject unknown fields.
#[wasm_bindgen]
pub fn parse_json(input: &str) -> Result<String> {
    strict_json::emit(&strict_json::value(input)?)
}

/// Capture public authority only after comparison with caller-selected anchors.
#[wasm_bindgen]
pub fn context(
    anchor_json: &str,
    profile_view_json: &str,
    account_view_json: &str,
    current_key_json: &str,
) -> Result<String> {
    let anchor: Anchor = strict_json::parse(anchor_json)?;
    let p = view(profile_view_json)?;
    let a: AccountView = strict_json::parse(account_view_json)?;
    let key: KeyIdentity = strict_json::parse(current_key_json)?;
    let c = SigningContext {
        domain: anchor.domain.clone(),
        current_key: key.clone(),
        authorization_generation: a.authorization_generation,
        spending_nonce: a.spending_nonce,
        committed: anchor.committed.clone(),
        profile_digest: anchor.profile_digest,
        protected: a.protected,
    };
    validation::check_anchor(&c, &anchor, &key).map_err(err)?;
    validation::validate_views(&c, &p, &a).map_err(err)?;
    strict_json::emit(&c)
}

#[wasm_bindgen]
pub fn prepare(
    context_json: &str,
    profile_view_json: &str,
    account_view_json: &str,
    anchor_json: &str,
    intent_json: &str,
    current_key_json: &str,
) -> Result<String> {
    let c: SigningContext = strict_json::parse(context_json)?;
    let p = view(profile_view_json)?;
    let a: AccountView = strict_json::parse(account_view_json)?;
    let anchor: Anchor = strict_json::parse(anchor_json)?;
    let intent: Intent = strict_json::parse(intent_json)?;
    let key: KeyIdentity = strict_json::parse(current_key_json)?;
    validation::check_anchor(&c, &anchor, &key).map_err(err)?;
    let fee = validation::validate_views(&c, &p, &a).map_err(err)?;
    let body = validation::body(&fee, &c, intent).map_err(err)?;
    strict_json::emit(
        &json!({"signing_bytes":wire::signing_bytes(&body,&fee.limits).map_err(err)?,"transaction_id":hex::encode(wire::transaction_id(&body,&fee.limits).map_err(err)?),"key_id":hex::encode(wire::key_id(&body.key).map_err(err)?),"profile_digest":hex::encode(body.fee_profile_digest),"body":body,"signature_verified":false}),
    )
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Transport {
    #[serde(rename = "type")]
    kind: String,
    envelope_base64: String,
}
fn inspect(signed: &SignedOrdinary, p: &ProfileView) -> Result<String> {
    let config = p
        .config
        .as_ref()
        .ok_or_else(|| "ordinary profile missing".to_owned())?;
    let fee = &config.fee_profile;
    validation::check_profile_binding(&signed.body, fee).map_err(err)?;
    need(
        signed.body.domain.chain_id == p.context.chain_id
            && signed.body.domain.genesis_digest == p.context.genesis_digest,
        "signed domain differs from profile view",
    )?;
    let signing_bytes = wire::signing_bytes(&signed.body, &fee.limits).map_err(err)?;
    let envelope = wire::encode(signed, &fee.limits).map_err(err)?;
    let transport = serde_json::to_vec(&Transport {
        kind: "ordinary_v2".into(),
        envelope_base64: STANDARD.encode(&envelope),
    })
    .map_err(err)?;
    need(
        transport.len() <= config.max_transport_bytes as usize
            && transport.len() <= MAX_BINARY_BYTES,
        "ordinary transport exceeds bound",
    )?;
    strict_json::emit(
        &json!({"body":signed.body,"signed":signed,"signing_bytes":signing_bytes,"transaction_id":hex::encode(wire::transaction_id(&signed.body,&fee.limits).map_err(err)?),"key_id":hex::encode(wire::key_id(&signed.body.key).map_err(err)?),"envelope_hash":hex::encode(wire::envelope_hash(signed,&fee.limits).map_err(err)?),"comet_hash":hex::encode(Sha256::digest(&transport)),"envelope_bytes":envelope,"transport_bytes":transport,"signature_verified":false}),
    )
}

/// The browser must verify the supplied signature with Noble before submission.
#[wasm_bindgen]
pub fn attach_signature(
    body_json: &str,
    profile_view_json: &str,
    signature: Vec<u8>,
) -> Result<String> {
    need(
        signature.len() <= 4627,
        "signature exceeds supported length",
    )?;
    let body: OrdinaryTransaction = strict_json::parse(body_json)?;
    let p = view(profile_view_json)?;
    inspect(&SignedOrdinary { body, signature }, &p)
}
/// Public signed-file inspection. No secret import and no signature assertion.
#[wasm_bindgen]
pub fn inspect_signed(signed_json: &str, profile_view_json: &str) -> Result<String> {
    let signed: SignedOrdinary = strict_json::parse(signed_json)?;
    let p = view(profile_view_json)?;
    inspect(&signed, &p)
}

#[wasm_bindgen]
pub fn signing_bytes(body_json: &str, limits_json: &str) -> Result<Vec<u8>> {
    let l = limits(limits_json)?;
    let body: OrdinaryTransaction = strict_json::parse(body_json)?;
    wire::signing_bytes(&body, &l).map_err(err)
}
#[wasm_bindgen]
pub fn encode_signed(signed_json: &str, limits_json: &str) -> Result<Vec<u8>> {
    let l = limits(limits_json)?;
    let signed: SignedOrdinary = strict_json::parse(signed_json)?;
    wire::encode(&signed, &l).map_err(err)
}
#[wasm_bindgen]
pub fn decode_envelope(bytes: Vec<u8>, limits_json: &str) -> Result<String> {
    need(
        bytes.len() <= MAX_BINARY_BYTES,
        "envelope exceeds browser bound",
    )?;
    let l = limits(limits_json)?;
    let signed = wire::decode(&bytes, &l).map_err(err)?;
    strict_json::emit(&signed)
}
#[wasm_bindgen]
pub fn decode_transport(transport_json: &str, profile_view_json: &str) -> Result<String> {
    let p = view(profile_view_json)?;
    let config = p.config.as_ref().unwrap();
    need(
        transport_json.len() <= config.max_transport_bytes as usize,
        "transport exceeds selected bound",
    )?;
    let t: Transport = strict_json::parse(transport_json)?;
    need(t.kind == "ordinary_v2", "wrong transport type")?;
    need(
        serde_json::to_string(&t).map_err(err)? == transport_json,
        "transport JSON must match its exact canonical bytes",
    )?;
    need(
        t.envelope_base64.len() as u64
            <= u64::from(config.fee_profile.limits.max_wire_bytes).div_ceil(3) * 4,
        "encoded envelope exceeds bound",
    )?;
    let bytes = STANDARD.decode(&t.envelope_base64).map_err(err)?;
    need(
        STANDARD.encode(&bytes) == t.envelope_base64,
        "noncanonical envelope Base64",
    )?;
    let signed = wire::decode(&bytes, &config.fee_profile.limits).map_err(err)?;
    inspect(&signed, &p)
}

/// Receipt consistency only. A signature and consensus proof are not checked.
#[wasm_bindgen]
pub fn validate_receipt(
    receipt_json: &str,
    signed_json: &str,
    fee_profile_json: &str,
) -> Result<String> {
    let p = profile(fee_profile_json)?;
    let receipt: ReceiptView = strict_json::parse(receipt_json)?;
    let signed: SignedOrdinary = strict_json::parse(signed_json)?;
    validation::validate_receipt(&receipt, &signed, &p).map_err(err)?;
    strict_json::emit(
        &json!({"consistent":true,"transaction_id":hex::encode(receipt.transaction_id),"signature_verified":false,"consensus_verified":false}),
    )
}
#[wasm_bindgen]
pub fn profile_digest(fee_profile_json: &str) -> Result<String> {
    Ok(hex::encode(
        ordinary_fees::profile_digest(&profile(fee_profile_json)?).map_err(err)?,
    ))
}
#[wasm_bindgen]
pub fn profile_bytes(fee_profile_json: &str) -> Result<Vec<u8>> {
    ordinary_fees::profile_bytes(&profile(fee_profile_json)?).map_err(err)
}
#[wasm_bindgen]
pub fn decode_profile(bytes: Vec<u8>) -> Result<String> {
    need(bytes.len() <= 512, "profile exceeds protocol bound")?;
    let p = ordinary_fees::decode_profile(&bytes).map_err(err)?;
    bounded_limits(&p.limits)?;
    strict_json::emit(&p)
}
#[wasm_bindgen]
pub fn account_address(network_code: u8, account_id_hex: &str) -> Result<String> {
    Ok(AccountAddress::from_account_id(network(network_code)?, hex32(account_id_hex)?).encode())
}
#[wasm_bindgen]
pub fn decode_address(network_code: u8, address: &str) -> Result<String> {
    need(address.len() <= 128, "address exceeds browser bound")?;
    Ok(hex::encode(
        AccountAddress::decode(network(network_code)?, address)
            .map_err(err)?
            .account_id(),
    ))
}
#[wasm_bindgen]
pub fn origin_address(
    network_code: u8,
    chain_id: &str,
    algorithm: &str,
    public_key: Vec<u8>,
) -> Result<String> {
    need(
        chain_id.len() <= 128 && public_key.len() <= 2592,
        "origin input exceeds bounds",
    )?;
    let scheme = match algorithm {
        "mldsa65" => OriginKeyAlgorithm::MlDsa65,
        "mldsa87" => OriginKeyAlgorithm::MlDsa87,
        _ => return Err("ordinary origin requires exact ML-DSA identifier".into()),
    };
    let a = AccountAddress::from_origin_key(network(network_code)?, chain_id, scheme, &public_key)
        .map_err(err)?;
    strict_json::emit(&json!({"address":a.encode(),"account_id":hex::encode(a.account_id())}))
}
#[wasm_bindgen]
pub fn parse_token_units(value: &str) -> Result<String> {
    need(value.len() <= 64, "token amount exceeds bound")?;
    Ok(validation::parse_token_units(value)
        .map_err(err)?
        .to_string())
}
#[cfg(test)]
mod tests;
