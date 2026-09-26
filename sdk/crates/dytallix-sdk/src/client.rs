//! Asynchronous HTTP client for Dytallix node APIs.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use reqwest::Url;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use tokio::time::sleep;

use crate::error::SdkError;
use crate::transaction::{SignedTransaction, Transaction};
use crate::{
    AccountState, Balance, Block, BlockId, ChainStatus, Delegation, FeeEstimate,
    TransactionReceipt, TransactionStatus, Validator,
};
use dytallix_core::address::DAddr;

const DEFAULT_PUBLIC_MIN_GAS_PRICE: u64 = 1_000;
const PUBLIC_TESTNET_ENDPOINT: &str = "https://dytallix.com";
const LOCAL_NODE_ENDPOINT: &str = "http://localhost:3030";
const CAPABILITIES_ENDPOINT_PATH: &str = "/api/capabilities";
const EMBEDDED_PUBLIC_CAPABILITIES_JSON: &str =
    include_str!("../../../docs/public-capabilities.json");

/// Indicates whether a capabilities document came from a live node or from the
/// SDK's embedded fallback manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilitiesSource {
    /// The document was fetched successfully from a live node endpoint.
    LiveNode,
    /// The live endpoint was unavailable, so the SDK used its embedded manifest.
    EmbeddedFallback,
}

impl CapabilitiesSource {
    /// Returns a stable human-readable label for this source.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LiveNode => "live-node",
            Self::EmbeddedFallback => "embedded-fallback",
        }
    }
}

/// Asynchronous client for interacting with Dytallix nodes.
#[derive(Debug, Clone)]
pub struct DytallixClient {
    endpoint: String,
    http: reqwest::Client,
    capabilities_cache: Arc<tokio::sync::Mutex<Option<CachedCapabilities>>>,
}

impl DytallixClient {
    /// Creates a new client targeting the provided node endpoint.
    pub async fn new(endpoint: &str) -> Result<Self, SdkError> {
        let normalized = normalize_endpoint(endpoint)?;
        let http = reqwest::Client::builder()
            .build()
            .map_err(|err| SdkError::Network(err.to_string()))?;

        Ok(Self {
            endpoint: normalized,
            http,
            capabilities_cache: Arc::new(tokio::sync::Mutex::new(None)),
        })
    }

    /// Creates a client for the canonical Dytallix testnet node.
    pub async fn testnet() -> Result<Self, SdkError> {
        Self::new(PUBLIC_TESTNET_ENDPOINT).await
    }

    /// Creates a client for a local Dytallix node.
    pub async fn local() -> Result<Self, SdkError> {
        Self::new(LOCAL_NODE_ENDPOINT).await
    }

    /// Fetches the current account state for the provided address.
    pub async fn get_account(&self, address: &DAddr) -> Result<AccountState, SdkError> {
        let account: AccountResponse = self.get_json(&format!("/account/{address}")).await?;

        Ok(AccountState {
            address: address.clone(),
            pubkey_hash: [0; 32],
            balance: account.balances,
            nonce: account.nonce,
            key_scheme: crate::KeyScheme::MlDsa65,
        })
    }

    /// Fetches the current token balances for the provided address.
    pub async fn get_balance(&self, address: &DAddr) -> Result<Balance, SdkError> {
        let balance: BalanceResponse = self.get_json(&format!("/balance/{address}")).await?;
        Ok(balance.balances)
    }

    /// Fetches a block by number, hash, or chain-relative identifier.
    pub async fn get_block(&self, id: BlockId) -> Result<Block, SdkError> {
        let path = match id {
            BlockId::Number(number) => format!("/block/{number}"),
            BlockId::Hash(hash) => format!("/block/{hash}"),
            BlockId::Latest | BlockId::Finalized => "/block/latest".to_owned(),
        };

        let block: BlockResponse = self.get_json(&path).await?;
        Ok(block.into())
    }

    /// Fetches a transaction receipt by hash.
    pub async fn get_transaction(&self, hash: &str) -> Result<TransactionReceipt, SdkError> {
        let receipt: TransactionReceiptResponse = self.get_json(&format!("/tx/{hash}")).await?;
        Ok(receipt.into())
    }

    /// Fetches the current chain status.
    pub async fn get_chain_status(&self) -> Result<ChainStatus, SdkError> {
        let status: ChainStatusResponse = self.get_json("/status").await?;
        Ok(status.into())
    }

    /// Fetches the machine-readable public capabilities document.
    ///
    /// Compatible nodes should serve this from `/api/capabilities`. When that
    /// endpoint is unavailable, the SDK falls back to its embedded manifest.
    pub async fn get_capabilities(&self) -> Result<serde_json::Value, SdkError> {
        self.get_capabilities_with_source()
            .await
            .map(|(document, _)| document)
    }

    /// Fetches the machine-readable public capabilities document together with
    /// the source that supplied it.
    pub async fn get_capabilities_with_source(
        &self,
    ) -> Result<(serde_json::Value, CapabilitiesSource), SdkError> {
        self.capabilities_json_value().await
    }

    /// Submits a signed transaction to the node and returns its receipt.
    pub async fn submit_transaction(
        &self,
        tx: &SignedTransaction,
    ) -> Result<TransactionReceipt, SdkError> {
        let url = self.url(transaction_submit_path(&self.endpoint))?;
        let tx_hash = tx.hash();
        let response = self
            .http
            .post(url.clone())
            .json(&SubmitTransactionBody { signed_tx: tx })
            .send()
            .await
            .map_err(|err| SdkError::NodeUnavailable {
                endpoint: url.to_string(),
                reason: err.to_string(),
            })?;

        if response.status().is_success() {
            let submitted: SubmittedTransaction =
                response.json().await.map_err(serialization_error)?;
            Ok(TransactionReceipt {
                hash: submitted.hash,
                block: 0,
                status: map_transaction_status(&submitted.status, None),
                fee: tx.fee_breakdown().unwrap_or_else(|| tx.tx.fee_estimate()),
            })
        } else {
            let reason = response
                .text()
                .await
                .unwrap_or_else(|_| "request rejected".to_owned());
            if let Some(receipt) = self.lookup_submitted_transaction(&tx_hash).await {
                return Ok(receipt);
            }
            Err(SdkError::TransactionRejected(reason))
        }
    }

    /// Requests a fee simulation for an unsigned transaction.
    pub async fn simulate_transaction(&self, tx: &Transaction) -> Result<FeeEstimate, SdkError> {
        let gas = self.get_network_gas_params().await?;
        Ok(tx.fee_estimate_with_gas_price(gas.min_gas_price))
    }

    /// Fetches the active validator set.
    pub async fn get_validators(&self) -> Result<Vec<Validator>, SdkError> {
        if self.uses_public_testnet_gateway()
            && self.route_is_direct_node_only("GET /v1/validators").await?
        {
            return Err(
                self.legacy_public_read_unavailable("/v1/validators", "validator-set reads")
            );
        }
        self.get_json("/v1/validators").await
    }

    /// Fetches delegations for the provided delegator address.
    pub async fn get_delegations(&self, address: &DAddr) -> Result<Vec<Delegation>, SdkError> {
        if self.uses_public_testnet_gateway()
            && self
                .route_is_direct_node_only("GET /v1/delegations/:address")
                .await?
        {
            return Err(self.legacy_public_read_unavailable(
                &format!("/v1/delegations/{address}"),
                "delegation reads",
            ));
        }
        self.get_json(&format!("/v1/delegations/{address}")).await
    }

    /// Returns the advertised feature state from the runtime capabilities endpoint,
    /// or the embedded SDK manifest when the runtime endpoint is unavailable.
    pub async fn public_feature_state(
        &self,
        feature_key: &str,
    ) -> Result<Option<String>, SdkError> {
        let document = self.capabilities_document().await?;
        Ok(document.features.get(feature_key).cloned())
    }

    /// Resolves a GET path against the active endpoint.
    ///
    /// When the client targets the default public website gateway, this uses
    /// the runtime capabilities document when available and the embedded SDK
    /// manifest otherwise.
    pub async fn resolve_read_path(&self, path: &str) -> Result<String, SdkError> {
        self.resolve_get_path(path).await
    }

    async fn get_json<T>(&self, path: &str) -> Result<T, SdkError>
    where
        T: DeserializeOwned,
    {
        let resolved_path = self.resolve_read_path(path).await?;
        let url = self.url(&resolved_path)?;
        let response =
            self.http
                .get(url.clone())
                .send()
                .await
                .map_err(|err| SdkError::NodeUnavailable {
                    endpoint: url.to_string(),
                    reason: err.to_string(),
                })?;

        if response.status().is_success() {
            response.json().await.map_err(serialization_error)
        } else {
            let reason = response
                .text()
                .await
                .unwrap_or_else(|_| "request failed".to_owned());
            Err(SdkError::NodeUnavailable {
                endpoint: url.to_string(),
                reason,
            })
        }
    }

    fn url(&self, path: &str) -> Result<Url, SdkError> {
        let joined = format!(
            "{}{}",
            self.endpoint,
            public_gateway_path(&self.endpoint, path)
        );
        Url::parse(&joined).map_err(|err| SdkError::Network(err.to_string()))
    }

    async fn get_network_gas_params(&self) -> Result<NetworkGasParams, SdkError> {
        let status: ChainStatusResponse = self.get_json("/status").await?;
        let gas = status.gas.unwrap_or_default();
        Ok(NetworkGasParams {
            min_gas_price: gas.min_gas_price.max(DEFAULT_PUBLIC_MIN_GAS_PRICE),
        })
    }

    async fn capabilities_document(&self) -> Result<CapabilityDocument, SdkError> {
        Ok(self.cached_capabilities().await?.document)
    }

    async fn capabilities_json_value(
        &self,
    ) -> Result<(serde_json::Value, CapabilitiesSource), SdkError> {
        let cached = self.cached_capabilities().await?;
        Ok((cached.json, cached.source))
    }

    async fn cached_capabilities(&self) -> Result<CachedCapabilities, SdkError> {
        if let Some(cached) = self.capabilities_cache.lock().await.clone() {
            return Ok(cached);
        }

        let loaded = match self.try_get_capabilities_payload().await {
            Ok(cached) => cached,
            Err(_) => embedded_capabilities_payload()?,
        };

        let mut guard = self.capabilities_cache.lock().await;
        *guard = Some(loaded.clone());
        Ok(loaded)
    }

    async fn try_get_capabilities_payload(&self) -> Result<CachedCapabilities, SdkError> {
        let url = Url::parse(&format!("{}{}", self.endpoint, CAPABILITIES_ENDPOINT_PATH))
            .map_err(|err| SdkError::Network(err.to_string()))?;
        let response =
            self.http
                .get(url.clone())
                .send()
                .await
                .map_err(|err| SdkError::NodeUnavailable {
                    endpoint: url.to_string(),
                    reason: err.to_string(),
                })?;

        if response.status().is_success() {
            let json: serde_json::Value = response.json().await.map_err(serialization_error)?;
            let document: CapabilityDocument = serde_json::from_value(json.clone())
                .map_err(|err| SdkError::Serialization(err.to_string()))?;
            Ok(CachedCapabilities {
                document,
                json,
                source: CapabilitiesSource::LiveNode,
            })
        } else {
            let reason = response
                .text()
                .await
                .unwrap_or_else(|_| "request failed".to_owned());
            Err(SdkError::NodeUnavailable {
                endpoint: url.to_string(),
                reason,
            })
        }
    }

    async fn route_is_direct_node_only(&self, route: &str) -> Result<bool, SdkError> {
        let document = self.capabilities_document().await?;
        Ok(document
            .direct_node_only_routes()
            .iter()
            .any(|candidate| candidate == route))
    }

    async fn resolve_get_path(&self, path: &str) -> Result<String, SdkError> {
        if !self.uses_public_testnet_gateway() {
            return Ok(path.to_owned());
        }

        let document = self.capabilities_document().await?;
        Ok(resolve_public_gateway_get_path(path, &document))
    }

    fn uses_public_testnet_gateway(&self) -> bool {
        self.endpoint == PUBLIC_TESTNET_ENDPOINT
    }

    fn legacy_public_read_unavailable(&self, path: &str, feature: &str) -> SdkError {
        SdkError::NodeUnavailable {
            endpoint: format!("{}{}", self.endpoint, path),
            reason: format!(
                "{feature} are not exposed as public JSON routes on the website gateway. Check `{CAPABILITIES_ENDPOINT_PATH}` on a compatible node, connect the SDK to a direct node endpoint that serves `{path}`, or use the documented public routes at https://dytallix.com/docs."
            ),
        }
    }

    async fn lookup_submitted_transaction(&self, hash: &str) -> Option<TransactionReceipt> {
        for _ in 0..3 {
            sleep(Duration::from_millis(750)).await;
            if let Ok(receipt) = self.get_transaction(hash).await {
                return Some(receipt);
            }
        }
        None
    }
}

fn normalize_endpoint(endpoint: &str) -> Result<String, SdkError> {
    let parsed = Url::parse(endpoint).map_err(|err| SdkError::Network(err.to_string()))?;
    let mut normalized = parsed.to_string();
    while normalized.ends_with('/') {
        normalized.pop();
    }
    Ok(normalized)
}

fn public_gateway_path(endpoint: &str, path: &str) -> String {
    if endpoint != PUBLIC_TESTNET_ENDPOINT {
        return path.to_owned();
    }

    if path.starts_with("/api/") || path.starts_with("/contracts/") {
        return path.to_owned();
    }

    let is_blockchain_root_read = matches!(path, "/status" | "/blocks" | "/transactions")
        || path.starts_with("/account/")
        || path.starts_with("/balance/")
        || path.starts_with("/block/")
        || path.starts_with("/tx/")
        || path.starts_with("/transactions/");

    if is_blockchain_root_read {
        format!("/api/blockchain{path}")
    } else {
        path.to_owned()
    }
}

fn resolve_public_gateway_get_path(path: &str, document: &CapabilityDocument) -> String {
    if path.starts_with("/api/") {
        return path.to_owned();
    }

    if document.supports_get_path(path) {
        return path.to_owned();
    }

    let prefixed = format!("/api/blockchain{path}");
    if document.supports_get_path(&prefixed) {
        return prefixed;
    }

    public_gateway_path(PUBLIC_TESTNET_ENDPOINT, path)
}

fn embedded_capabilities_document() -> Result<CapabilityDocument, SdkError> {
    serde_json::from_str(EMBEDDED_PUBLIC_CAPABILITIES_JSON)
        .map_err(|err| SdkError::Serialization(err.to_string()))
}

fn embedded_capabilities_json_value() -> Result<serde_json::Value, SdkError> {
    serde_json::from_str(EMBEDDED_PUBLIC_CAPABILITIES_JSON)
        .map_err(|err| SdkError::Serialization(err.to_string()))
}

fn embedded_capabilities_payload() -> Result<CachedCapabilities, SdkError> {
    Ok(CachedCapabilities {
        document: embedded_capabilities_document()?,
        json: embedded_capabilities_json_value()?,
        source: CapabilitiesSource::EmbeddedFallback,
    })
}

fn transaction_submit_path(endpoint: &str) -> &'static str {
    if endpoint == PUBLIC_TESTNET_ENDPOINT {
        "/api/blockchain/submit"
    } else {
        "/submit"
    }
}

fn serialization_error(err: reqwest::Error) -> SdkError {
    SdkError::Serialization(err.to_string())
}

fn map_transaction_status(status: &str, error: Option<String>) -> TransactionStatus {
    match status.to_ascii_lowercase().as_str() {
        "pending" => TransactionStatus::Pending,
        "success" | "confirmed" => TransactionStatus::Confirmed,
        _ => TransactionStatus::Failed(
            error.unwrap_or_else(|| format!("transaction failed with status `{status}`")),
        ),
    }
}

#[derive(Debug, serde::Deserialize)]
struct AccountResponse {
    #[serde(default = "default_balance", deserialize_with = "deserialize_balances")]
    balances: Balance,
    #[serde(default)]
    nonce: u64,
}

#[derive(Debug, serde::Deserialize)]
struct BalanceResponse {
    #[serde(default = "default_balance", deserialize_with = "deserialize_balances")]
    balances: Balance,
}

#[derive(Debug, serde::Deserialize)]
struct ChainStatusResponse {
    chain_id: String,
    latest_height: u64,
    #[serde(default)]
    epoch: u64,
    #[serde(default)]
    slot: u64,
    #[serde(default)]
    gas: Option<ChainGasResponse>,
}

impl From<ChainStatusResponse> for ChainStatus {
    fn from(value: ChainStatusResponse) -> Self {
        Self {
            block_height: value.latest_height,
            epoch: value.epoch,
            slot: value.slot,
            finalized_checkpoint: value.chain_id,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
struct ChainGasResponse {
    #[serde(default)]
    min_gas_price: u64,
}

#[derive(Debug, Clone, Copy)]
struct NetworkGasParams {
    min_gas_price: u64,
}

#[derive(Debug, Clone)]
struct CachedCapabilities {
    document: CapabilityDocument,
    json: serde_json::Value,
    source: CapabilitiesSource,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct CapabilityDocument {
    #[serde(default)]
    features: BTreeMap<String, String>,
    #[serde(default, rename = "publicRoutes")]
    public_routes: CapabilityPublicRoutes,
    #[serde(default, rename = "publicNode")]
    public_node: CapabilityPublicNode,
}

impl CapabilityDocument {
    fn direct_node_only_routes(&self) -> Vec<String> {
        let mut routes = self.public_routes.direct_node_only.clone();
        routes.extend(self.public_node.direct_node_only_routes.clone());
        routes
    }

    fn supports_get_path(&self, path: &str) -> bool {
        self.public_routes
            .supported
            .iter()
            .chain(self.public_node.supported_routes.iter())
            .filter_map(|route| route.strip_prefix("GET "))
            .any(|template| route_template_matches(template, path))
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct CapabilityPublicRoutes {
    #[serde(default, rename = "directNodeOnly")]
    direct_node_only: Vec<String>,
    #[serde(default)]
    supported: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct CapabilityPublicNode {
    #[serde(default, rename = "directNodeOnlyRoutes")]
    direct_node_only_routes: Vec<String>,
    #[serde(default, rename = "supportedRoutes")]
    supported_routes: Vec<String>,
}

fn route_template_matches(template: &str, path: &str) -> bool {
    let template_segments: Vec<&str> = template.trim_start_matches('/').split('/').collect();
    let path_segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();

    if template_segments.len() != path_segments.len() {
        return false;
    }

    template_segments
        .iter()
        .zip(path_segments.iter())
        .all(|(template_segment, path_segment)| {
            if template_segment.starts_with(':') {
                !path_segment.is_empty()
            } else {
                template_segment == path_segment
            }
        })
}

#[derive(Debug, serde::Serialize)]
struct SubmitTransactionBody<'a> {
    signed_tx: &'a SignedTransaction,
}

#[derive(Debug, serde::Deserialize)]
struct SubmittedTransaction {
    hash: String,
    status: String,
}

#[derive(Debug, serde::Deserialize)]
struct TransactionReceiptResponse {
    #[serde(alias = "hash", rename = "tx_hash")]
    hash: String,
    #[serde(
        rename = "block_height",
        default,
        deserialize_with = "deserialize_u64_or_default"
    )]
    block: u64,
    status: String,
    #[serde(default)]
    error: Option<String>,
    #[serde(default, deserialize_with = "deserialize_u128_string")]
    fee: u128,
}

impl From<TransactionReceiptResponse> for TransactionReceipt {
    fn from(value: TransactionReceiptResponse) -> Self {
        Self {
            hash: value.hash,
            block: value.block,
            status: map_transaction_status(&value.status, value.error),
            fee: FeeEstimate {
                c_gas: 0,
                c_gas_cost_drt: 0,
                b_gas: 0,
                b_gas_cost_drt: value.fee,
                total_cost_drt: value.fee,
            },
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct BlockResponse {
    #[serde(alias = "number", alias = "height")]
    number: u64,
    hash: String,
    #[serde(default, alias = "parent", alias = "parent_hash")]
    parent_hash: String,
    #[serde(default)]
    proposer: Option<DAddr>,
    #[serde(default)]
    slot: u64,
    #[serde(default)]
    epoch: u64,
    #[serde(default)]
    c_gas_used: u64,
    #[serde(default)]
    b_gas_used: u64,
    #[serde(default)]
    timestamp: u64,
    #[serde(default)]
    txs: Vec<serde_json::Value>,
}

impl From<BlockResponse> for Block {
    fn from(value: BlockResponse) -> Self {
        Self {
            number: value.number,
            hash: value.hash,
            parent_hash: value.parent_hash,
            proposer: value.proposer.unwrap_or_else(unknown_block_proposer),
            slot: value.slot,
            epoch: value.epoch,
            tx_count: value.txs.len(),
            c_gas_used: value.c_gas_used,
            b_gas_used: value.b_gas_used,
            timestamp: value.timestamp,
        }
    }
}

fn unknown_block_proposer() -> DAddr {
    DAddr::from_public_key(&[0u8; 1_952]).expect("fixed placeholder proposer key is valid")
}

fn deserialize_balances<'de, D>(deserializer: D) -> Result<Balance, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let balances =
        <BTreeMap<String, serde_json::Value> as serde::Deserialize>::deserialize(deserializer)?;
    Ok(Balance {
        dgt: decode_micro_balance(&balances, "udgt"),
        drt: decode_micro_balance(&balances, "udrt"),
    })
}

fn default_balance() -> Balance {
    Balance { dgt: 0, drt: 0 }
}

fn decode_micro_balance(balances: &BTreeMap<String, serde_json::Value>, denom: &str) -> u128 {
    match balances.get(denom) {
        Some(serde_json::Value::Number(number)) => {
            number.as_u64().map(u128::from).unwrap_or(0) / 1_000_000
        }
        Some(serde_json::Value::String(value)) => value.parse::<u128>().unwrap_or(0) / 1_000_000,
        Some(serde_json::Value::Object(value)) => {
            value
                .get("balance")
                .and_then(|amount| match amount {
                    serde_json::Value::Number(number) => number.as_u64().map(u128::from),
                    serde_json::Value::String(raw) => raw.parse::<u128>().ok(),
                    _ => None,
                })
                .unwrap_or(0)
                / 1_000_000
        }
        _ => 0,
    }
}

fn deserialize_u64_or_default<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Number(number) => number.as_u64().unwrap_or_default(),
        serde_json::Value::String(raw) => raw.parse::<u64>().unwrap_or_default(),
        _ => 0,
    })
}

fn deserialize_u128_string<'de, D>(deserializer: D) -> Result<u128, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Number(number) => number.as_u64().map(u128::from).unwrap_or_default(),
        serde_json::Value::String(raw) => raw.parse::<u128>().unwrap_or_default(),
        _ => 0,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        embedded_capabilities_document, normalize_endpoint, public_gateway_path,
        resolve_public_gateway_get_path, route_template_matches, transaction_submit_path,
        BlockResponse, CapabilitiesSource, ChainStatusResponse, DytallixClient,
        LOCAL_NODE_ENDPOINT, PUBLIC_TESTNET_ENDPOINT,
    };
    use crate::error::SdkError;
    use dytallix_core::address::DAddr;
    use dytallix_core::keypair::DytallixKeypair;

    #[test]
    fn normalize_endpoint_trims_trailing_slash() {
        assert_eq!(
            normalize_endpoint("https://dytallix.com/").unwrap(),
            PUBLIC_TESTNET_ENDPOINT
        );
        assert_eq!(
            normalize_endpoint("http://localhost:3030///").unwrap(),
            LOCAL_NODE_ENDPOINT
        );
    }

    #[test]
    fn public_gateway_paths_are_prefixed() {
        assert_eq!(
            public_gateway_path(PUBLIC_TESTNET_ENDPOINT, "/balance/demo"),
            "/api/blockchain/balance/demo"
        );
        assert_eq!(
            public_gateway_path(PUBLIC_TESTNET_ENDPOINT, "/status"),
            "/api/blockchain/status"
        );
        assert_eq!(
            public_gateway_path(PUBLIC_TESTNET_ENDPOINT, "/api/blockchain/submit"),
            "/api/blockchain/submit"
        );
        assert_eq!(
            public_gateway_path(PUBLIC_TESTNET_ENDPOINT, "/api/capabilities"),
            "/api/capabilities"
        );
        assert_eq!(
            public_gateway_path(PUBLIC_TESTNET_ENDPOINT, "/contracts/deploy"),
            "/contracts/deploy"
        );
        assert_eq!(
            public_gateway_path(LOCAL_NODE_ENDPOINT, "/status"),
            "/status"
        );
    }

    #[test]
    fn route_template_matching_handles_placeholder_segments() {
        assert!(route_template_matches(
            "/api/blockchain/balance/:address",
            "/api/blockchain/balance/dytallix1demo"
        ));
        assert!(route_template_matches(
            "/api/blockchain/block/:number|hash|latest|finalized",
            "/api/blockchain/block/latest"
        ));
        assert!(!route_template_matches(
            "/api/blockchain/block/:number|hash|latest|finalized",
            "/api/blockchain/block/latest/extra"
        ));
    }

    #[test]
    fn embedded_capabilities_manifest_exposes_runtime_contract_and_direct_routes() {
        let document = embedded_capabilities_document().unwrap();
        assert_eq!(
            document.features.get("stakingWrites").map(String::as_str),
            Some("hidden")
        );
        assert!(document
            .direct_node_only_routes()
            .iter()
            .any(|route| route == "GET /api/capabilities"));
        assert!(document
            .direct_node_only_routes()
            .iter()
            .any(|route| route == "GET /v1/validators"));
        assert_eq!(
            resolve_public_gateway_get_path("/status", &document),
            "/api/blockchain/status"
        );
        assert_eq!(
            resolve_public_gateway_get_path("/balance/demo", &document),
            "/api/blockchain/balance/demo"
        );
        assert_eq!(
            resolve_public_gateway_get_path("/api/capabilities", &document),
            "/api/capabilities"
        );
    }

    #[tokio::test]
    async fn capabilities_api_falls_back_to_embedded_manifest() {
        let client = DytallixClient::new("http://127.0.0.1:9").await.unwrap();
        let (capabilities, source) = client.get_capabilities_with_source().await.unwrap();

        assert_eq!(source, CapabilitiesSource::EmbeddedFallback);
        assert_eq!(
            capabilities["canonicalStatement"].as_str(),
            Some("Keypair, faucet, transfer, and basic contract lifecycle are available for experimentation on the public testnet. Staking, governance, and some advanced or operator paths are not yet production-complete.")
        );
        assert_eq!(
            capabilities["features"]["governanceWrites"].as_str(),
            Some("hidden")
        );
        assert!(capabilities["publicRoutes"]["directNodeOnly"]
            .as_array()
            .unwrap()
            .iter()
            .any(|route| route.as_str() == Some("GET /api/capabilities")));
    }

    #[test]
    fn submit_path_uses_direct_node_route_when_needed() {
        assert_eq!(
            transaction_submit_path(PUBLIC_TESTNET_ENDPOINT),
            "/api/blockchain/submit"
        );
        assert_eq!(transaction_submit_path(LOCAL_NODE_ENDPOINT), "/submit");
        assert_eq!(transaction_submit_path("http://127.0.0.1:43030"), "/submit");
    }

    #[test]
    fn block_response_maps_minimal_node_payload() {
        let parsed: BlockResponse = serde_json::from_str(
            r#"{
                "asset_hashes": [],
                "hash": "0xabc",
                "height": 42,
                "parent": "0xdef",
                "timestamp": 1776025359,
                "txs": []
            }"#,
        )
        .unwrap();

        let block: crate::Block = parsed.into();
        assert_eq!(block.number, 42);
        assert_eq!(block.hash, "0xabc");
        assert_eq!(block.parent_hash, "0xdef");
        assert_eq!(block.timestamp, 1776025359);
        assert_eq!(block.tx_count, 0);
        assert_eq!(block.slot, 0);
        assert_eq!(block.epoch, 0);
    }

    #[test]
    fn chain_status_response_maps_epoch_and_slot() {
        let parsed: ChainStatusResponse = serde_json::from_str(
            r#"{
                "chain_id": "dyt-local-1",
                "latest_height": 303461,
                "epoch": 52,
                "slot": 3940
            }"#,
        )
        .unwrap();

        let status: crate::ChainStatus = parsed.into();
        assert_eq!(status.block_height, 303461);
        assert_eq!(status.epoch, 52);
        assert_eq!(status.slot, 3940);
        assert_eq!(status.finalized_checkpoint, "dyt-local-1");
    }

    #[tokio::test]
    async fn public_gateway_legacy_reads_fail_fast() {
        let client = DytallixClient::testnet().await.unwrap();
        let address = DAddr::from_public_key(DytallixKeypair::generate().public_key()).unwrap();

        let validators = client.get_validators().await.unwrap_err();
        let delegations = client.get_delegations(&address).await.unwrap_err();

        match validators {
            SdkError::NodeUnavailable { endpoint, reason } => {
                assert!(endpoint.ends_with("/v1/validators"));
                assert!(reason.contains("not exposed as public JSON routes"));
            }
            other => panic!("unexpected validator error: {other:?}"),
        }

        match delegations {
            SdkError::NodeUnavailable { endpoint, reason } => {
                assert!(endpoint.contains("/v1/delegations/"));
                assert!(reason.contains("direct node endpoint"));
            }
            other => panic!("unexpected delegation error: {other:?}"),
        }
    }
}
