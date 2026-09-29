//! Explicit CometBFT JSON-RPC client for ordinary-v2 and ordinary-v3 transactions.
//! No URL default, legacy fallback, redirect, automatic submission, or consensus
//! proof is supplied. A successful CheckTx result is not a committed receipt.
//! Requests go to a loopback node over plain HTTP or to a pinned endpoint
//! over the post-quantum client channel ([`crate::transport`]); there is no
//! TLS (E04 gap 19).
use crate::ordinary_v2::{
    self, error, AccountView, ChainPin, Error, FeeProfile, KeyIdentity, ProfileView, ReceiptView,
    Result, SignedOrdinary, SigningContext,
};
use crate::ordinary_v3::{self, FeeProfileV3, GovernanceProfileView};
use base64::{engine::general_purpose::STANDARD, Engine};
/// Typed reads (interfaces v1, decision 2): the node's report, not proofs.
pub use dytallix_protocol_types::ordinary_client::{
    AccountSummaryView, BondView, ProposalPhaseView, ProposalView, TallyView, UnbondView,
    ValidatorEntryView, ValidatorSetView, VoteView,
};
use dytallix_protocol_types::{address::AccountAddress, state_proof::StateProofView};
/// State keys, record decoding and proof verification (clients v1, K-c, K-d).
pub use dytallix_protocol_types::{native_account, state_proof};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::time::Duration;

pub use crate::transport::Endpoint;
#[cfg(feature = "comet-rpc")]
pub use crate::transport::EndpointPin;

/// Reading a context retries this often when a block commits between queries.
const CONTEXT_ATTEMPTS: usize = 3;

/// An ordinary-v2 signing context read from one endpoint for one key.
#[derive(Clone, Debug)]
pub struct OrdinaryContext {
    pub context: SigningContext,
    pub fee_profile: FeeProfile,
    pub profile: ProfileView,
    /// The node reported no account record: this is the account's first spend.
    pub first_spend: bool,
}
/// An ordinary-v3 (governance) signing context read from one endpoint.
#[derive(Clone, Debug)]
pub struct GovernanceContext {
    pub context: SigningContext,
    pub fee_profile: FeeProfileV3,
    pub profile: ProfileView,
    pub governance: GovernanceProfileView,
}
fn transport_bound(profile: &ProfileView) -> Result<usize> {
    let config = profile
        .config
        .as_ref()
        .ok_or_else(|| Error("ordinary configuration is missing".into()))?;
    usize::try_from(config.max_transport_bytes).map_err(error)
}
impl OrdinaryContext {
    pub fn max_transport_bytes(&self) -> Result<usize> {
        transport_bound(&self.profile)
    }
}
impl GovernanceContext {
    pub fn max_transport_bytes(&self) -> Result<usize> {
        transport_bound(&self.profile)
    }
}
/// Reading the header after a state's height polls this often, once a second.
const HEADER_ATTEMPTS: u32 = 30;
const HEADER_INTERVAL: Duration = Duration::from_secs(1);

/// Where the trusted application hash of a verified read came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppHashSource {
    /// Supplied by the caller.
    Caller,
    /// The header of the next block, as this endpoint's engine reports it.
    /// Its signatures are not checked: trust in the endpoint is the caller's
    /// (their own node; clients v1, decision 3).
    NodeHeader,
}
/// A committed state value with its proof, as `/state/proof/{key}` reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateProof {
    pub view: StateProofView,
    pub value: Option<Vec<u8>>,
}
impl StateProof {
    /// Verify against the application hash of `view.height` from a source
    /// the caller trusts.
    pub fn verify(&self, trusted_app_hash: &[u8; 32]) -> Result<()> {
        self.view
            .verify(self.value.as_deref(), trusted_app_hash)
            .map_err(error)
    }
}
/// A state value verified to `app_hash` at `height`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedValue {
    pub height: u64,
    pub value: Option<Vec<u8>>,
    pub app_hash: [u8; 32],
    pub source: AppHashSource,
}
/// An account's liquid balances by denomination, verified; `None` when the
/// account has no record (never funded).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Balances {
    pub height: u64,
    pub balances: Option<BTreeMap<String, u128>>,
    pub app_hash: [u8; 32],
    pub source: AppHashSource,
}
/// A first spend's preconditions, verified (rule 4): the account is funded
/// and has no recovery record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirstSpendProof {
    pub height: u64,
    pub balances: BTreeMap<String, u128>,
    pub app_hash: [u8; 32],
    pub source: AppHashSource,
}

pub struct CometClient {
    endpoint: Endpoint,
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
    /// A node on this machine: `http://IP:PORT` with a literal loopback IP,
    /// and a strict decoded response limit. Requests have a 30-second
    /// timeout. HTTPS and remote HTTP are refused: reach a remote node
    /// through [`CometClient::channel`].
    pub fn new(endpoint: &str, max_response_bytes: usize) -> Result<Self> {
        Self::with_endpoint(Endpoint::loopback(endpoint)?, max_response_bytes)
    }
    /// A remote node through the client channel to the pinned endpoint. The
    /// pin's network is the endpoint's chain ID; the endpoint refuses a
    /// handshake for another.
    #[cfg(feature = "comet-rpc")]
    pub fn channel(pin: EndpointPin, max_response_bytes: usize) -> Result<Self> {
        Self::with_endpoint(Endpoint::Channel(pin), max_response_bytes)
    }
    pub fn with_endpoint(endpoint: Endpoint, max_response_bytes: usize) -> Result<Self> {
        if max_response_bytes == 0 {
            return Err(Error("a positive response bound is required".into()));
        }
        Ok(Self {
            endpoint,
            max_response_bytes,
        })
    }
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }
    async fn call<T: DeserializeOwned>(&self, method: &str, params: Value) -> Result<T> {
        let request = json!({"jsonrpc":"2.0","id":"ordinary-v2","method":method,"params":params});
        let body = serde_json::to_vec(&request).map_err(error)?;
        let raw = crate::transport::post(&self.endpoint, body, self.max_response_bytes).await?;
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
    /// The committed v3 fee profile and next proposal ID (`/ordinary/profile_v3`).
    pub async fn query_governance_profile(&self) -> Result<GovernanceProfileView> {
        let (height, view): (u64, GovernanceProfileView) =
            self.query("/ordinary/profile_v3").await?;
        if view.version != 1
            || view.context.height != height
            || view.enabled != view.fee_profile.is_some()
            || view.enabled != (view.next_proposal_id > 0)
        {
            return Err(Error("inconsistent governance profile query view".into()));
        }
        if let Some(profile) = &view.fee_profile {
            profile.validate().map_err(error)?;
        }
        Ok(view)
    }
    /// The ordinary-v2 context for `key` on `account_id`, checked against the
    /// pin (`ordinary_v2::context_from_views`); a first spend without a record.
    pub async fn signing_context(
        &self,
        pin: &ChainPin,
        account_id: &[u8; 32],
        key: &KeyIdentity,
    ) -> Result<OrdinaryContext> {
        for _ in 0..CONTEXT_ATTEMPTS {
            let profile = self.query_profile().await?;
            pin.check(&profile.context)?;
            let account = self.query_account(account_id).await?;
            if account
                .as_ref()
                .is_some_and(|a| a.context.height != profile.context.height)
            {
                continue;
            }
            let (context, fee_profile) =
                ordinary_v2::context_from_views(pin, &profile, account_id, account.as_ref(), key)?;
            return Ok(OrdinaryContext {
                context,
                fee_profile,
                profile,
                first_spend: account.is_none(),
            });
        }
        Err(Error(
            "the chain kept advancing between context queries; retry".into(),
        ))
    }
    /// The ordinary-v3 context for `key` on `account_id`, checked against the pin.
    pub async fn governance_context(
        &self,
        pin: &ChainPin,
        account_id: &[u8; 32],
        key: &KeyIdentity,
    ) -> Result<GovernanceContext> {
        for _ in 0..CONTEXT_ATTEMPTS {
            let profile = self.query_profile().await?;
            pin.check(&profile.context)?;
            let governance = self.query_governance_profile().await?;
            let account = self.query_account(account_id).await?;
            let height = profile.context.height;
            if governance.context.height != height
                || account.as_ref().is_some_and(|a| a.context.height != height)
            {
                continue;
            }
            let (context, fee_profile) = ordinary_v3::context_from_views(
                pin,
                &profile,
                &governance,
                account_id,
                account.as_ref(),
                key,
            )?;
            return Ok(GovernanceContext {
                context,
                fee_profile,
                profile,
                governance,
            });
        }
        Err(Error(
            "the chain kept advancing between context queries; retry".into(),
        ))
    }
    /// An account's balances, bonds, unbonding and claimable rewards
    /// (`/account/{address}`). The node's report; `query_balances` proves
    /// the balances.
    pub async fn query_account_summary(
        &self,
        pin: &ChainPin,
        address: &AccountAddress,
    ) -> Result<AccountSummaryView> {
        let encoded = address.encode();
        let (height, view): (u64, AccountSummaryView) =
            self.query(&format!("/account/{encoded}")).await?;
        pin.check(&view.context)?;
        if view.version != 1
            || view.context.height != height
            || view.address != encoded
            || view.account_id != *address.account_id()
        {
            return Err(Error("inconsistent account summary view".into()));
        }
        Ok(view)
    }
    /// The validator set of the next block (`/staking/validators`).
    pub async fn query_validators(&self, pin: &ChainPin) -> Result<ValidatorSetView> {
        let (height, view): (u64, ValidatorSetView) = self.query("/staking/validators").await?;
        pin.check(&view.context)?;
        if view.version != 1
            || view.context.height != height
            || Some(view.height) != height.checked_add(1)
            // An enabled set has validators; a disabled one has none.
            || view.enabled == view.validators.is_empty()
        {
            return Err(Error("inconsistent validator set view".into()));
        }
        Ok(view)
    }
    /// A governance proposal (`/governance/proposal/{id}`); `None` when it
    /// does not exist or governance is off.
    pub async fn query_proposal(&self, pin: &ChainPin, id: u64) -> Result<Option<ProposalView>> {
        let (height, view): (u64, Option<ProposalView>) =
            self.query(&format!("/governance/proposal/{id}")).await?;
        if let Some(proposal) = &view {
            pin.check(&proposal.context)?;
            if proposal.version != 1
                || proposal.context.height != height
                || proposal.proposal_id != id
            {
                return Err(Error("inconsistent proposal view".into()));
            }
        }
        Ok(view)
    }
    /// One account's vote (`/governance/vote/{id}/{account_id}`); `None` when
    /// the proposal does not exist or governance is off.
    pub async fn query_vote(
        &self,
        pin: &ChainPin,
        id: u64,
        voter: &[u8; 32],
    ) -> Result<Option<VoteView>> {
        let (height, view): (u64, Option<VoteView>) = self
            .query(&format!("/governance/vote/{id}/{}", hex(voter)))
            .await?;
        if let Some(vote) = &view {
            pin.check(&vote.context)?;
            if vote.version != 1
                || vote.context.height != height
                || vote.proposal_id != id
                || vote.voter != *voter
            {
                return Err(Error("inconsistent vote view".into()));
            }
        }
        Ok(view)
    }
    /// One committed state value with its proof (`/state/proof/{key}`).
    /// Call `StateProof::verify`, or use `query_verified`.
    pub async fn query_state_proof(&self, key: &[u8]) -> Result<StateProof> {
        let key_hex = hex(key);
        let (height, view): (u64, StateProofView) =
            self.query(&format!("/state/proof/{key_hex}")).await?;
        if view.key != key_hex || view.height != height {
            return Err(Error("inconsistent state proof query view".into()));
        }
        let value = match &view.value {
            None => None,
            Some(encoded) => {
                let bytes = STANDARD.decode(encoded).map_err(error)?;
                if STANDARD.encode(&bytes) != *encoded {
                    return Err(Error("state value is not canonical base64".into()));
                }
                Some(bytes)
            }
        };
        Ok(StateProof { view, value })
    }
    /// The application hash of state `height`: the one in the header of
    /// block `height + 1`, as this endpoint's engine reports it. Polls until
    /// that block commits. Header signatures are not checked.
    pub async fn header_app_hash(&self, pin: &ChainPin, height: u64) -> Result<[u8; 32]> {
        let block = height
            .checked_add(1)
            .ok_or_else(|| Error("state height is exhausted".into()))?;
        let mut attempt = 0;
        let result: Value = loop {
            attempt += 1;
            match self
                .call("commit", json!({"height": block.to_string()}))
                .await
            {
                Ok(result) => break result,
                // The next block may not have committed yet.
                Err(_) if attempt < HEADER_ATTEMPTS => tokio::time::sleep(HEADER_INTERVAL).await,
                Err(e) => return Err(e),
            }
        };
        let header = &result["signed_header"]["header"];
        // Comet encodes heights as decimal strings.
        let expected_height = block.to_string();
        if header["chain_id"] != pin.chain_id.as_str()
            || header["height"].as_str() != Some(expected_height.as_str())
        {
            return Err(Error("header is for another chain or height".into()));
        }
        header["app_hash"]
            .as_str()
            .and_then(hash32)
            .ok_or_else(|| Error("header application hash is not 32 hexadecimal bytes".into()))
    }
    /// A state value verified against `trusted` (the application hash of the
    /// node's current height from a source the caller trusts) or, without
    /// it, against this endpoint's next header.
    pub async fn query_verified(
        &self,
        pin: &ChainPin,
        key: &[u8],
        trusted: Option<[u8; 32]>,
    ) -> Result<VerifiedValue> {
        let proof = self.query_state_proof(key).await?;
        let (app_hash, source) = match trusted {
            Some(hash) => (hash, AppHashSource::Caller),
            None => (
                self.header_app_hash(pin, proof.view.height).await?,
                AppHashSource::NodeHeader,
            ),
        };
        proof.verify(&app_hash)?;
        Ok(VerifiedValue {
            height: proof.view.height,
            value: proof.value,
            app_hash,
            source,
        })
    }
    /// An account's liquid balances from its native record, verified.
    pub async fn query_balances(
        &self,
        pin: &ChainPin,
        address: &AccountAddress,
        trusted: Option<[u8; 32]>,
    ) -> Result<Balances> {
        let verified = self
            .query_verified(pin, &native_account::balances_key(address), trusted)
            .await?;
        let balances = verified
            .value
            .as_deref()
            .map(native_account::decode_balances)
            .transpose()
            .map_err(error)?;
        Ok(Balances {
            height: verified.height,
            balances,
            app_hash: verified.app_hash,
            source: verified.source,
        })
    }
    /// Prove a first spend's preconditions (rule 4) at one height against
    /// this endpoint's next header: a balance record and no recovery record.
    pub async fn prove_first_spend(
        &self,
        pin: &ChainPin,
        address: &AccountAddress,
    ) -> Result<FirstSpendProof> {
        for _ in 0..CONTEXT_ATTEMPTS {
            let record = self
                .query_state_proof(&native_account::recovery_account_key(address))
                .await?;
            let funds = self
                .query_state_proof(&native_account::balances_key(address))
                .await?;
            if record.view.height != funds.view.height {
                continue;
            }
            let app_hash = self.header_app_hash(pin, record.view.height).await?;
            record.verify(&app_hash)?;
            funds.verify(&app_hash)?;
            if record.value.is_some() {
                return Err(Error(
                    "the account has a recovery record; this is not a first spend".into(),
                ));
            }
            let balances = funds
                .value
                .as_deref()
                .map(native_account::decode_balances)
                .transpose()
                .map_err(error)?
                .ok_or_else(|| Error("the account has no balance record; fund it first".into()))?;
            return Ok(FirstSpendProof {
                height: record.view.height,
                balances,
                app_hash,
                source: AppHashSource::NodeHeader,
            });
        }
        Err(Error(
            "the chain kept advancing between proof queries; retry".into(),
        ))
    }
    /// Poll for a committed receipt; `None` when it is still absent.
    pub async fn wait_for_receipt(
        &self,
        id: &[u8; 32],
        attempts: u32,
        interval: Duration,
    ) -> Result<Option<ReceiptView>> {
        for attempt in 0..attempts {
            if let Some(receipt) = self.query_receipt(id).await? {
                return Ok(Some(receipt));
            }
            if attempt + 1 < attempts {
                tokio::time::sleep(interval).await;
            }
        }
        Ok(None)
    }
    /// Poll until the account's spending nonce passes `nonce`, the committed
    /// evidence for a v3 transaction (no v3 receipt yet, gap 12).
    pub async fn wait_for_spent_nonce(
        &self,
        id: &[u8; 32],
        nonce: u64,
        attempts: u32,
        interval: Duration,
    ) -> Result<Option<AccountView>> {
        for attempt in 0..attempts {
            if let Some(account) = self.query_account(id).await? {
                if account.spending_nonce > nonce {
                    return Ok(Some(account));
                }
            }
            if attempt + 1 < attempts {
                tokio::time::sleep(interval).await;
            }
        }
        Ok(None)
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
    /// An account's recovery state (E04 gap 17), to build and sponsor
    /// recovery actions from. The node's report, not a proof.
    pub async fn query_recovery_account(
        &self,
        id: &[u8; 32],
    ) -> Result<Option<crate::recovery::RecoveryAccountView>> {
        let (height, view): (u64, Option<crate::recovery::RecoveryAccountView>) = self
            .query(&format!("/recovery/account/{}", hex(id)))
            .await?;
        if let Some(account) = &view {
            if account.version != 1
                || account.context.height != height
                || account.domain.account_id != *id
            {
                return Err(Error("inconsistent recovery query view".into()));
            }
        }
        Ok(view)
    }
    /// CheckTx for a sponsored recovery transaction (`recovery::transaction`).
    pub async fn check_recovery_tx(
        &self,
        sponsored: &crate::recovery::SponsoredRecovery,
    ) -> Result<CheckTxResponse> {
        let tx = crate::recovery::transaction(sponsored)?;
        self.check(tx, &crate::recovery::authorization_id(sponsored)?)
            .await
    }
    /// Explicit broadcast of a sponsored recovery transaction. A zero code
    /// only reports CheckTx admission.
    pub async fn submit_recovery_sync(
        &self,
        sponsored: &crate::recovery::SponsoredRecovery,
    ) -> Result<CheckTxResponse> {
        let tx = crate::recovery::transaction(sponsored)?;
        self.broadcast(tx, &crate::recovery::authorization_id(sponsored)?)
            .await
    }
    /// The sponsor receipt of `authorization_id`
    /// (`recovery:v2:receipt:<hex>`), verified against the application
    /// hash. An absent value is not success; receipts are kept until the
    /// operation's submission expiry.
    pub async fn query_recovery_receipt(
        &self,
        pin: &ChainPin,
        authorization_id: &[u8; 32],
        trusted: Option<[u8; 32]>,
    ) -> Result<VerifiedValue> {
        let key = format!("recovery:v2:receipt:{}", hex(authorization_id));
        self.query_verified(pin, key.as_bytes(), trusted).await
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
        let id = ordinary_v2::transaction_id(&signed.body, &profile.limits)?;
        self.check(tx, &id).await
    }
    /// CheckTx for a v3 governance transaction; the bound is the ordinary
    /// configuration's `max_transport_bytes`.
    pub async fn check_governance_tx(
        &self,
        signed: &ordinary_v3::SignedOrdinary,
        profile: &FeeProfileV3,
        max_transport_bytes: usize,
    ) -> Result<CheckTxResponse> {
        let tx = governance_submission(signed, profile, max_transport_bytes)?;
        let id = ordinary_v3::transaction_id(&signed.body, &profile.limits())?;
        self.check(tx, &id).await
    }
    async fn check(&self, tx: Vec<u8>, id: &[u8; 32]) -> Result<CheckTxResponse> {
        let result: CheckResult = self
            .call("check_tx", json!({"tx":STANDARD.encode(tx)}))
            .await?;
        Ok(CheckTxResponse {
            code: result.code,
            log: result.log,
            codespace: result.codespace,
            transaction_id: hex(id),
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
        let id = ordinary_v2::transaction_id(&signed.body, &profile.limits)?;
        self.broadcast(tx, &id).await
    }
    /// Explicit v3 broadcast, as `submit_sync`. An admitted governance
    /// transaction whose rule then fails is still charged.
    pub async fn submit_governance_sync(
        &self,
        signed: &ordinary_v3::SignedOrdinary,
        profile: &FeeProfileV3,
        max_transport_bytes: usize,
    ) -> Result<CheckTxResponse> {
        let tx = governance_submission(signed, profile, max_transport_bytes)?;
        let id = ordinary_v3::transaction_id(&signed.body, &profile.limits())?;
        self.broadcast(tx, &id).await
    }
    async fn broadcast(&self, tx: Vec<u8>, id: &[u8; 32]) -> Result<CheckTxResponse> {
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
            transaction_id: hex(id),
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
fn governance_submission(
    signed: &ordinary_v3::SignedOrdinary,
    profile: &FeeProfileV3,
    bound: usize,
) -> Result<Vec<u8>> {
    // Binds the exact profile, one action, contract version and fee cap.
    // Activation was checked at preparation, so the profile's own height is passed.
    profile
        .validate_signed_request(&signed.body, profile.activation_height)
        .map_err(|_| Error("submission profile differs from signed body".into()))?;
    ordinary_v3::verify_signature(signed, &profile.limits())?;
    ordinary_v3::encode_transport(signed, &profile.limits(), bound)
}
/// 64 hexadecimal characters in either case (Comet headers use uppercase).
fn hash32(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0; 32];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(value.get(index * 2..index * 2 + 2)?, 16).ok()?;
    }
    Some(out)
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

#[cfg(test)]
mod local_endpoint_tests {
    use super::*;

    #[test]
    fn plain_http_reaches_only_a_literal_loopback_node() {
        for endpoint in [
            "https://127.0.0.1:26657",
            "http://example.com:26657",
            "http://localhost:26657",
            "http://192.0.2.1:26657",
            "http://0.0.0.0:26657",
            "http://user@127.0.0.1:26657",
            "http://127.0.0.1:26657?x=1",
            "http://127.0.0.1:26657#x",
            "http://127.0.0.1:26657/rpc",
            "http://127.0.0.1",
            "ftp://127.0.0.1",
        ] {
            assert!(
                CometClient::new(endpoint, 1024).is_err(),
                "accepted {endpoint}"
            );
        }
        let error = CometClient::new("https://127.0.0.1", 1024).err().unwrap();
        assert!(error.to_string().contains("TLS is not supported"));
        let error = CometClient::new("http://192.0.2.1:26657", 1024)
            .err()
            .unwrap();
        assert!(error.to_string().contains("only a node on this machine"));
    }

    #[test]
    fn a_literal_loopback_node_needs_a_positive_bound() {
        for endpoint in [
            "http://127.0.0.1:26657",
            "http://127.0.0.1:26657/",
            "http://[::1]:26657",
        ] {
            assert!(CometClient::new(endpoint, 1024).is_ok(), "{endpoint}");
            assert!(CometClient::new(endpoint, 0).is_err());
        }
    }
}
