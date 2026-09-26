//! Transaction building, signing, and fee estimation for Dytallix.

use base64::Engine as _;
use dytallix_core::address::DAddr;
use dytallix_core::keypair::DytallixKeypair;
use serde::Serialize;
use sha3::{Digest, Sha3_256};

#[cfg(feature = "network")]
use crate::client::DytallixClient;
use crate::error::SdkError;
use crate::FeeEstimate;
use crate::Token;

const DEFAULT_CHAIN_ID: &str = "dyt-local-1";
const DEFAULT_MIN_GAS_PRICE_MICRO: u64 = 1_000;
const STORAGE_HASH_HEX_LEN: u64 = 66;
const LEGACY_TX_SIZE_OVERHEAD_BYTES: u64 = 64;
const TRANSFER_BASE_GAS: u64 = 500;
const PER_BYTE_GAS: u64 = 2;
const PER_ADDITIONAL_SIGNATURE_GAS: u64 = 700;
const TX_OVERHEAD_GAS: u64 = 1;
const KV_READ_GAS: u64 = 40;
const KV_WRITE_GAS: u64 = 120;
const MICROS_PER_TOKEN: u128 = 1_000_000;

mod as_str_u128 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &u128, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u128, D::Error> {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

/// Builder for Dytallix transactions.
#[derive(Debug, Clone, Default)]
pub struct TransactionBuilder {
    from: Option<DAddr>,
    to: Option<DAddr>,
    amount: Option<u128>,
    token: Option<Token>,
    c_gas: Option<u64>,
    b_gas: Option<u64>,
    nonce: Option<u64>,
    data: Option<Vec<u8>>,
    chain_id: Option<String>,
    fee_micro: Option<u128>,
    memo: Option<String>,
}

impl TransactionBuilder {
    /// Creates a new empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the sender address.
    pub fn from(mut self, address: DAddr) -> Self {
        self.from = Some(address);
        self
    }

    /// Sets the recipient address.
    pub fn to(mut self, address: DAddr) -> Self {
        self.to = Some(address);
        self
    }

    /// Sets the token amount and token type for the transfer.
    pub fn amount(mut self, value: u128, token: Token) -> Self {
        self.amount = Some(value);
        self.token = Some(token);
        self
    }

    /// Sets the compute and bandwidth gas limits.
    pub fn gas_limit(mut self, c_gas: u64, b_gas: u64) -> Self {
        self.c_gas = Some(c_gas);
        self.b_gas = Some(b_gas);
        self
    }

    /// Sets the sender nonce.
    pub fn nonce(mut self, nonce: u64) -> Self {
        self.nonce = Some(nonce);
        self
    }

    /// Sets arbitrary transaction payload bytes.
    pub fn data(mut self, bytes: Vec<u8>) -> Self {
        self.data = Some(bytes);
        self
    }

    /// Overrides the chain ID used in the signed payload.
    pub fn chain_id(mut self, chain_id: impl Into<String>) -> Self {
        self.chain_id = Some(chain_id.into());
        self
    }

    /// Overrides the flat micro-denominated network fee.
    pub fn fee_micro(mut self, fee_micro: u128) -> Self {
        self.fee_micro = Some(fee_micro);
        self
    }

    /// Sets an optional memo field.
    pub fn memo(mut self, memo: impl Into<String>) -> Self {
        self.memo = Some(memo.into());
        self
    }

    /// Builds the final transaction after validation.
    pub fn build(self) -> Result<Transaction, SdkError> {
        let from = self
            .from
            .ok_or_else(|| SdkError::TransactionRejected("missing from address".to_owned()))?;
        let to = self
            .to
            .ok_or_else(|| SdkError::TransactionRejected("missing to address".to_owned()))?;
        let amount = self.amount.unwrap_or(0);
        let token = self.token.unwrap_or(Token::DRT);
        let data = self.data.unwrap_or_default();
        let chain_id = self.chain_id.unwrap_or_else(|| DEFAULT_CHAIN_ID.to_owned());

        let message = if amount > 0 {
            Message::Send {
                from: from.to_string(),
                to: to.to_string(),
                denom: token.micro_denom().to_owned(),
                amount: amount.saturating_mul(MICROS_PER_TOKEN),
            }
        } else if !data.is_empty() {
            Message::Data {
                from: from.to_string(),
                data: String::from_utf8_lossy(&data).into_owned(),
            }
        } else {
            return Err(SdkError::TransactionRejected(
                "transaction must include a positive token amount or non-empty data payload"
                    .to_owned(),
            ));
        };

        let messages = vec![message];
        let (intrinsic_gas, execution_gas) = estimate_gas_components(&messages);
        let fee_micro = self.fee_micro.unwrap_or_else(|| {
            u128::from(intrinsic_gas.saturating_add(execution_gas))
                .saturating_mul(u128::from(DEFAULT_MIN_GAS_PRICE_MICRO))
        });

        Ok(Transaction {
            chain_id,
            nonce: self.nonce.unwrap_or(0),
            msgs: messages,
            fee: fee_micro,
            memo: self.memo.unwrap_or_default(),
            c_gas_limit: self.c_gas.unwrap_or(intrinsic_gas),
            b_gas_limit: self.b_gas.unwrap_or(execution_gas),
        })
    }
}

/// An unsigned Dytallix transaction in the live node wire format.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Transaction {
    /// The expected chain identifier.
    pub chain_id: String,
    /// The sender nonce.
    pub nonce: u64,
    /// The transaction messages.
    pub msgs: Vec<Message>,
    /// The fee encoded in micro-units.
    #[serde(with = "as_str_u128")]
    pub fee: u128,
    /// Optional memo field.
    pub memo: String,
    /// Local fee breakdown metadata used by the SDK.
    #[serde(skip)]
    pub c_gas_limit: u64,
    /// Local fee breakdown metadata used by the SDK.
    #[serde(skip)]
    pub b_gas_limit: u64,
}

impl Transaction {
    /// Returns a copy of the transaction with an explicit fee override.
    pub fn with_fee_micro(mut self, fee_micro: u128) -> Self {
        self.fee = fee_micro;
        self
    }

    /// Signs the transaction with the provided Dytallix keypair.
    pub fn sign(self, keypair: &DytallixKeypair) -> Result<SignedTransaction, SdkError> {
        keypair.scheme().require_production_operational()?;
        let canonical = canonical_json(&self)?;
        let hash_bytes = sha3_256(&canonical);
        let signature = keypair.sign(&hash_bytes)?;
        let tx_hash = encode_prefixed_hex(&hash_bytes);

        Ok(SignedTransaction {
            tx: self.clone(),
            signature: base64::engine::general_purpose::STANDARD.encode(signature),
            public_key: base64::engine::general_purpose::STANDARD.encode(keypair.public_key()),
            algorithm: "mldsa65".to_owned(),
            version: 1,
            fee: Some(self.fee_estimate()),
            tx_hash,
        })
    }

    /// Returns the deterministic flat fee used by the public node.
    pub fn fee_estimate(&self) -> FeeEstimate {
        let total_gas = self.c_gas_limit.saturating_add(self.b_gas_limit);
        if total_gas == 0 {
            return FeeEstimate {
                c_gas: 0,
                c_gas_cost_drt: 0,
                b_gas: 0,
                b_gas_cost_drt: 0,
                total_cost_drt: 0,
            };
        }

        let gas_price_micro = self.fee / u128::from(total_gas);
        let c_gas_cost = u128::from(self.c_gas_limit).saturating_mul(gas_price_micro);
        let mut b_gas_cost = u128::from(self.b_gas_limit).saturating_mul(gas_price_micro);
        let billed_total = c_gas_cost.saturating_add(b_gas_cost);
        if billed_total < self.fee {
            b_gas_cost = b_gas_cost.saturating_add(self.fee - billed_total);
        }

        FeeEstimate {
            c_gas: self.c_gas_limit,
            c_gas_cost_drt: c_gas_cost,
            b_gas: self.b_gas_limit,
            b_gas_cost_drt: b_gas_cost,
            total_cost_drt: self.fee,
        }
    }

    /// Returns the deterministic fee estimate for a specific gas price.
    #[cfg(feature = "network")]
    pub(crate) fn fee_estimate_with_gas_price(&self, gas_price_micro: u64) -> FeeEstimate {
        let intrinsic_gas = self.c_gas_limit;
        let execution_gas = self.b_gas_limit;
        let c_gas_cost = u128::from(intrinsic_gas).saturating_mul(u128::from(gas_price_micro));
        let b_gas_cost = u128::from(execution_gas).saturating_mul(u128::from(gas_price_micro));

        FeeEstimate {
            c_gas: intrinsic_gas,
            c_gas_cost_drt: c_gas_cost,
            b_gas: execution_gas,
            b_gas_cost_drt: b_gas_cost,
            total_cost_drt: c_gas_cost.saturating_add(b_gas_cost),
        }
    }

    /// Requests a fee estimate for this transaction from the provided client.
    #[cfg(feature = "network")]
    pub async fn estimate_fee(&self, client: &DytallixClient) -> Result<FeeEstimate, SdkError> {
        client.simulate_transaction(self).await
    }

    /// Returns a copy of the transaction repriced with the live node's fee schedule.
    #[cfg(feature = "network")]
    pub async fn with_estimated_fee(
        self,
        client: &DytallixClient,
    ) -> Result<(Self, FeeEstimate), SdkError> {
        let fee = self.estimate_fee(client).await?;
        Ok((self.with_fee_micro(fee.total_cost_drt), fee))
    }
}

/// A signed Dytallix transaction ready for submission.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SignedTransaction {
    /// The unsigned transaction payload.
    pub tx: Transaction,
    /// The detached signature bytes encoded as base64.
    pub signature: String,
    /// The raw public key encoded as base64.
    pub public_key: String,
    /// The signature algorithm identifier accepted by the node.
    pub algorithm: String,
    /// Protocol version for the signed envelope.
    pub version: u32,
    /// Optional fee estimate captured alongside the transaction.
    #[serde(skip)]
    pub fee: Option<FeeEstimate>,
    /// The canonical transaction hash.
    #[serde(skip)]
    pub tx_hash: String,
}

impl SignedTransaction {
    /// Returns the optional fee breakdown attached to the signed transaction.
    pub fn fee_breakdown(&self) -> Option<FeeEstimate> {
        self.fee.clone()
    }

    /// Returns the canonical transaction hash.
    pub fn hash(&self) -> String {
        self.tx_hash.clone()
    }
}

/// Supported transaction message types exposed by the live node.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    /// A token transfer.
    Send {
        from: String,
        to: String,
        denom: String,
        #[serde(with = "as_str_u128")]
        amount: u128,
    },
    /// An arbitrary data payload anchored on-chain.
    Data { from: String, data: String },
    /// A WASM contract deployment message.
    ContractDeploy {
        from: String,
        code: String,
        gas_limit: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        initial_state: Option<String>,
    },
    /// A contract call message.
    ContractCall {
        from: String,
        address: String,
        method: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        args: Option<String>,
        gas_limit: u64,
    },
}

impl Token {
    fn micro_denom(self) -> &'static str {
        match self {
            Token::DGT => "udgt",
            Token::DRT => "udrt",
        }
    }
}

/// Returns the default compute and bandwidth gas limits for the provided messages.
pub fn estimate_default_gas_limits(messages: &[Message]) -> (u64, u64) {
    estimate_gas_components(messages)
}

fn estimate_gas_components(messages: &[Message]) -> (u64, u64) {
    let (from, to) = legacy_storage_addresses(messages);
    let tx_size = STORAGE_HASH_HEX_LEN
        .saturating_add(from.len() as u64)
        .saturating_add(to.len() as u64)
        .saturating_add(LEGACY_TX_SIZE_OVERHEAD_BYTES);
    let intrinsic_gas = TRANSFER_BASE_GAS
        .saturating_add(PER_BYTE_GAS.saturating_mul(tx_size))
        .saturating_add(PER_ADDITIONAL_SIGNATURE_GAS.saturating_mul(messages.len() as u64));
    let execution_gas = TX_OVERHEAD_GAS.saturating_add(
        messages
            .iter()
            .map(message_execution_gas)
            .fold(0u64, u64::saturating_add),
    );
    (intrinsic_gas, execution_gas)
}

fn legacy_storage_addresses(messages: &[Message]) -> (&str, &str) {
    let Some(first_sender) = messages.first().map(Message::sender_address) else {
        return ("", "");
    };

    let mut first_to = first_sender;
    for message in messages {
        if first_to == first_sender {
            if let Some(send_to) = message.send_recipient() {
                first_to = send_to;
            }
        }
    }

    (first_sender, first_to)
}

fn message_execution_gas(message: &Message) -> u64 {
    match message {
        Message::Send { .. } => KV_READ_GAS
            .saturating_add(KV_READ_GAS)
            .saturating_add(KV_WRITE_GAS)
            .saturating_add(KV_WRITE_GAS),
        Message::Data { data, .. } => data.len() as u64,
        Message::ContractDeploy { gas_limit, .. } | Message::ContractCall { gas_limit, .. } => {
            *gas_limit
        }
    }
}

impl Message {
    fn sender_address(&self) -> &str {
        match self {
            Self::Send { from, .. } => from,
            Self::Data { from, .. } => from,
            Self::ContractDeploy { from, .. } => from,
            Self::ContractCall { from, .. } => from,
        }
    }

    fn send_recipient(&self) -> Option<&str> {
        match self {
            Self::Send { to, .. } => Some(to),
            Self::Data { .. } | Self::ContractDeploy { .. } => None,
            Self::ContractCall { address, .. } => Some(address),
        }
    }
}

fn canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>, SdkError> {
    let json_value =
        serde_json::to_value(value).map_err(|err| SdkError::Serialization(err.to_string()))?;
    let sorted = sort_json_value(json_value);
    serde_json::to_vec(&sorted).map_err(|err| SdkError::Serialization(err.to_string()))
}

fn sort_json_value(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut sorted_map = serde_json::Map::new();
            let mut keys = map.keys().cloned().collect::<Vec<_>>();
            keys.sort();

            for key in keys {
                if let Some(inner) = map.get(&key) {
                    sorted_map.insert(key, sort_json_value(inner.clone()));
                }
            }

            serde_json::Value::Object(sorted_map)
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(sort_json_value).collect())
        }
        other => other,
    }
}

fn sha3_256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha3_256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut output = [0u8; 32];
    output.copy_from_slice(&digest);
    output
}

fn encode_prefixed_hex(bytes: &[u8]) -> String {
    format!("0x{}", encode_hex(bytes))
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}
