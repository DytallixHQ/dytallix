//! Explicit CometBFT JSON-RPC client for the local ordinary-v2 integration.
//! No URL default, legacy fallback, redirect, automatic submission, or consensus
//! proof is supplied. A successful CheckTx result is not a committed receipt.
use crate::ordinary_v2::{
    self, error, AccountView, Error, FeeProfile, ProfileView, ReceiptView, Result, SignedOrdinary,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::Duration;

pub struct CometClient {
    endpoint: reqwest::Url,
    http: reqwest::Client,
    max_response_bytes: usize,
}
/// Admission result only. The engine hash and ordinary transaction ID differ.
#[derive(Clone, Debug, Serialize)]
pub struct CheckTxResponse {
    pub code: u32,
    pub log: String,
    pub codespace: String,
    pub transaction_id: String,
    pub engine_hash: Option<String>,
    pub submitted: bool,
}
impl CheckTxResponse {
    pub fn admitted(&self) -> bool {
        self.code == 0
    }
}
impl CometClient {
    /// Configure one endpoint and a strict decoded response limit. Requests have
    /// a 30-second timeout. Redirects and URL credentials are rejected.
    pub fn new(endpoint: &str, max_response_bytes: usize) -> Result<Self> {
        let endpoint = reqwest::Url::parse(endpoint).map_err(error)?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || max_response_bytes == 0
        {
            return Err(Error("explicit HTTP RPC endpoint and positive response bound required; URL credentials/query/fragment are forbidden".into()));
        }
        #[cfg(any(feature = "ordinary-http-only", feature = "strict-local-mldsa65"))]
        {
            if endpoint.scheme() != "http" {
                return Err(Error(
                    "HTTPS is unsupported in ordinary-http-only; no downgrade is permitted".into(),
                ));
            }
            // Literal loopback avoids DNS changes and external plaintext traffic.
            let host = endpoint
                .host_str()
                .unwrap()
                .trim_start_matches('[')
                .trim_end_matches(']');
            if !host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
            {
                return Err(Error("ordinary-http-only requires a literal loopback address; production endpoints are unsupported".into()));
            }
        }
        let builder = reqwest::Client::builder();
        #[cfg(any(feature = "ordinary-http-only", feature = "strict-local-mldsa65"))]
        let builder = builder.no_proxy();
        let http = builder
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(error)?;
        Ok(Self {
            endpoint,
            http,
            max_response_bytes,
        })
    }
    async fn call<T: DeserializeOwned>(&self, method: &str, params: Value) -> Result<T> {
        let request = json!({"jsonrpc":"2.0","id":"ordinary-v2","method":method,"params":params});
        let mut response = self
            .http
            .post(self.endpoint.clone())
            .json(&request)
            .send()
            .await
            .map_err(error)?;
        if !response.status().is_success() {
            return Err(Error(format!("RPC HTTP status {}", response.status())));
        }
        if response
            .content_length()
            .is_some_and(|n| n > self.max_response_bytes as u64)
        {
            return Err(Error("RPC response exceeds bound".into()));
        }
        let mut raw = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(error)? {
            if chunk.len() > self.max_response_bytes - raw.len() {
                return Err(Error("RPC response exceeds bound".into()));
            }
            raw.extend_from_slice(&chunk);
        }
        let rpc: RpcResponse<T> = serde_json::from_slice(&raw).map_err(error)?;
        if rpc.jsonrpc != "2.0" || rpc.id != "ordinary-v2" {
            return Err(Error("RPC version or response ID differs".into()));
        }
        match (rpc.result, rpc.error) {
            (Some(result), None) => Ok(result),
            (None, Some(e)) => Err(Error(format!("RPC error {}: {}", e.code, e.message))),
            _ => Err(Error(
                "RPC response requires exactly one result or error".into(),
            )),
        }
    }
    async fn query<T: DeserializeOwned>(&self, path: &str) -> Result<(u64, T)> {
        let result: QueryResult = self
            .call(
                "abci_query",
                json!({"path":path,"data":"","height":"0","prove":false}),
            )
            .await?;
        let r = result.response;
        if r.code != 0 {
            return Err(Error(format!("ABCI query {}: {}", r.code, r.log)));
        }
        let height =
            dytallix_protocol_types::ordinary::parse_decimal_u64(&r.height).map_err(error)?;
        let encoded = r
            .value
            .ok_or_else(|| Error("ABCI query value missing".into()))?;
        let bytes = STANDARD.decode(&encoded).map_err(error)?;
        if STANDARD.encode(&bytes) != encoded {
            return Err(Error("ABCI query value is not canonical base64".into()));
        }
        Ok((height, serde_json::from_slice(&bytes).map_err(error)?))
    }
    pub async fn query_profile(&self) -> Result<ProfileView> {
        let (height, view): (u64, ProfileView) = self.query("/ordinary/profile").await?;
        if view.version != 1
            || view.context.height != height
            || view.enabled != view.config.is_some()
        {
            return Err(Error("inconsistent profile query view".into()));
        }
        if let Some(config) = &view.config {
            config.fee_profile.validate().map_err(error)?;
        }
        Ok(view)
    }
    pub async fn query_account(&self, id: &[u8; 32]) -> Result<Option<AccountView>> {
        let (height, view): (u64, Option<AccountView>) = self
            .query(&format!("/ordinary/account/{}", hex(id)))
            .await?;
        if let Some(account) = &view {
            if account.version != 1
                || account.context.height != height
                || account.account_id != *id
                || account.domain.account_id != *id
            {
                return Err(Error("inconsistent account query view".into()));
            }
        }
        Ok(view)
    }
    /// A receipt is only reported committed state. Call validate_receipt to bind
    /// it to a signed envelope, and verify its context through caller trust.
    pub async fn query_receipt(&self, id: &[u8; 32]) -> Result<Option<ReceiptView>> {
        let (height, view): (u64, Option<ReceiptView>) = self
            .query(&format!("/ordinary/receipt/{}", hex(id)))
            .await?;
        if let Some(receipt) = &view {
            if receipt.version != 1
                || receipt.context.height != height
                || receipt.transaction_id != *id
            {
                return Err(Error("inconsistent receipt query view".into()));
            }
        }
        Ok(view)
    }
    pub async fn check_tx(
        &self,
        signed: &SignedOrdinary,
        profile: &FeeProfile,
        max_transport_bytes: usize,
    ) -> Result<CheckTxResponse> {
        let tx = submission(signed, profile, max_transport_bytes)?;
        let result: CheckResult = self
            .call("check_tx", json!({"tx":STANDARD.encode(tx)}))
            .await?;
        Ok(CheckTxResponse {
            code: result.code,
            log: result.log,
            codespace: result.codespace,
            transaction_id: hex(&ordinary_v2::transaction_id(&signed.body, &profile.limits)?),
            engine_hash: None,
            submitted: false,
        })
    }
    /// Explicit broadcast. A zero code only reports CheckTx admission.
    /// This method does not wait for commit or retry an uncertain submission.
    pub async fn submit_sync(
        &self,
        signed: &SignedOrdinary,
        profile: &FeeProfile,
        max_transport_bytes: usize,
    ) -> Result<CheckTxResponse> {
        let tx = submission(signed, profile, max_transport_bytes)?;
        let expected_hash = hex(&Sha256::digest(&tx));
        let result: BroadcastResult = self
            .call("broadcast_tx_sync", json!({"tx":STANDARD.encode(tx)}))
            .await?;
        if result.hash.len() != 64
            || !result.hash.bytes().all(|b| b.is_ascii_hexdigit())
            || result.hash.to_ascii_lowercase() != expected_hash
        {
            return Err(Error("invalid Comet transaction hash".into()));
        }
        Ok(CheckTxResponse {
            code: result.code,
            log: result.log,
            codespace: result.codespace,
            transaction_id: hex(&ordinary_v2::transaction_id(&signed.body, &profile.limits)?),
            engine_hash: Some(result.hash),
            submitted: true,
        })
    }
}
fn submission(signed: &SignedOrdinary, profile: &FeeProfile, bound: usize) -> Result<Vec<u8>> {
    profile
        .validate_request(signed.body.gas_limit, signed.body.maximum_fee)
        .map_err(error)?;
    if signed.body.fee_profile_digest != ordinary_v2::profile_digest(profile)?
        || signed.body.fee_profile_version != profile.version
        || signed.body.ordinary_fee_contract_version != profile.ordinary_fee_contract_version
        || signed.body.fee_denomination != profile.denomination
    {
        return Err(Error("submission profile differs from signed body".into()));
    }
    ordinary_v2::verify_signature(signed, &profile.limits)?;
    ordinary_v2::encode_transport(signed, &profile.limits, bound)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RpcResponse<T> {
    jsonrpc: String,
    id: String,
    result: Option<T>,
    error: Option<RpcError>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RpcError {
    code: i64,
    message: String,
    #[serde(rename = "data")]
    _data: Option<Value>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryResult {
    response: QueryResponse,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryResponse {
    // Pinned CometBFT overrides protobuf JSON to emit default result fields.
    // Require status fields; an omitted code must never imply admission.
    code: u32,
    log: String,
    height: String,
    value: Option<String>,
    #[serde(rename = "info")]
    _info: Option<String>,
    #[serde(rename = "index")]
    _index: Option<String>,
    #[serde(rename = "key")]
    _key: Option<String>,
    #[serde(rename = "proofOps", alias = "proof_ops")]
    _proof: Option<Value>,
    #[serde(rename = "codespace")]
    _codespace: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckResult {
    code: u32,
    log: String,
    codespace: String,
    #[serde(rename = "data")]
    _data: Option<String>,
    #[serde(rename = "info")]
    _info: Option<String>,
    #[serde(rename = "gas_wanted")]
    _gas_wanted: Option<String>,
    #[serde(rename = "gas_used")]
    _gas_used: Option<String>,
    #[serde(rename = "events")]
    _events: Option<Vec<Value>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BroadcastResult {
    code: u32,
    log: String,
    codespace: String,
    hash: String,
    #[serde(rename = "data")]
    _data: Option<String>,
}

#[cfg(all(
    test,
    any(feature = "ordinary-http-only", feature = "strict-local-mldsa65")
))]
mod local_endpoint_tests {
    use super::*;

    #[test]
    fn local_profile_rejects_https_external_dns_and_ambiguous_urls() {
        for endpoint in [
            "https://127.0.0.1:26657",
            "http://example.com:26657",
            "http://localhost:26657",
            "http://192.0.2.1:26657",
            "http://0.0.0.0:26657",
            "http://user@127.0.0.1:26657",
            "http://127.0.0.1:26657?x=1",
            "http://127.0.0.1:26657#x",
            "ftp://127.0.0.1",
        ] {
            assert!(
                CometClient::new(endpoint, 1024).is_err(),
                "accepted {endpoint}"
            );
        }
        let error = CometClient::new("https://127.0.0.1", 1024).err().unwrap();
        assert!(error.to_string().contains("HTTPS is unsupported"));
    }

    #[test]
    fn local_profile_accepts_literal_loopback_with_positive_bound() {
        for endpoint in ["http://127.0.0.1:26657", "http://[::1]:26657"] {
            assert!(CometClient::new(endpoint, 1024).is_ok());
            assert!(CometClient::new(endpoint, 0).is_err());
        }
    }
}
