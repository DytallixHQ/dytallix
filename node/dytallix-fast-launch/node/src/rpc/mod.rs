use crate::rpc::errors::ApiError;
use crate::runtime::bridge;
use crate::runtime::emission::EmissionEngine;
use crate::runtime::fee_burn::FeeBurnEngine;
use crate::runtime::governance::{GovernanceModule, ProposalType};
#[cfg(feature = "oracle")]
use crate::runtime::oracle::current_timestamp;
use crate::runtime::staking::StakingModule;
#[cfg(not(feature = "oracle"))]
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
#[cfg(feature = "oracle")]
use crate::storage::oracle::OracleStore;
use crate::types::{SignedTx, ValidationError};
use crate::{
    mempool::Mempool,
    state::State,
    storage::blocks::TpsWindow,
    storage::{state::Storage, tx::Transaction},
    ws::server::WsHub,
};
use axum::{
    extract::{Path, Query},
    Extension, Json,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(feature = "oracle")]
pub mod ai;
#[cfg(feature = "contracts")]
pub mod contracts;
pub mod errors; // restored errors module export
#[cfg(feature = "contracts")]
pub use contracts::{
    contract_events, contract_info, contract_query, contracts_call, contracts_deploy,
    contracts_state,
};
#[cfg(feature = "oracle")]
pub mod oracle;

/// GET /account/:addr - Return account details including nonce and balances
pub async fn get_account(
    Extension(ctx): Extension<RpcContext>,
    Path(addr): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut state = ctx.state.lock().unwrap();
    let nonce = state.nonce_of(&addr);
    let balances = state.balances_of(&addr);
    Ok(Json(json!({
        "address": addr,
        "nonce": nonce,
        "balances": balances
    })))
}

#[derive(Clone)]
pub struct RpcContext {
    pub storage: Arc<Storage>,
    pub mempool: Arc<Mutex<Mempool>>,
    pub state: Arc<Mutex<State>>,
    pub ws: WsHub,
    pub tps: Arc<Mutex<TpsWindow>>,
    pub emission: Arc<Mutex<EmissionEngine>>,
    pub governance: Arc<Mutex<GovernanceModule>>,
    pub staking: Arc<Mutex<StakingModule>>,
    pub fee_burn: Arc<Mutex<FeeBurnEngine>>,
    pub metrics: Arc<crate::metrics::Metrics>,
    pub features: FeatureFlags,
    /// Minimal in-memory WASM contract state for JSON-RPC facade (legacy)
    pub wasm_contracts: Arc<Mutex<HashMap<String, u64>>>, // address -> counter
    /// WASM runtime for contract deployment and execution
    #[cfg(feature = "contracts")]
    pub wasm_runtime: Arc<crate::runtime::wasm::WasmRuntime>,
    /// Pending asset hashes to be included in the next block
    pub pending_assets: Arc<Mutex<Vec<String>>>,
    pub proposer_address: Option<String>,
    pub validator_public_key_b64: Option<String>,
    pub validator_algorithm: Option<String>,
    pub slots_per_epoch: u64,
}

#[derive(Clone, Copy)]
pub struct FeatureFlags {
    pub governance: bool,
    pub staking: bool,
}

const PUBLIC_CAPABILITIES_JSON: &str = include_str!("../../../../docs/public-capabilities.json");

fn public_capabilities_document() -> serde_json::Value {
    serde_json::from_str(PUBLIC_CAPABILITIES_JSON)
        .expect("node public capabilities manifest must be valid JSON")
}

/// Reject before parsing action arguments, calling external services, or changing state.
pub(crate) fn require_legacy_route(ctx: &RpcContext) -> Result<(), ApiError> {
    crate::genesis::reject_qualified_state(&ctx.storage)
        .map_err(|error| ApiError::BadRequest(error.to_string()))
}

fn governance_write_unavailable() -> ApiError {
    ApiError::NotImplemented(
        "Governance write routes are intentionally disabled on the public alpha node until proposal, deposit, vote, and execution semantics are production-complete. Public governance reads remain available.".to_string(),
    )
}

fn staking_write_unavailable() -> ApiError {
    ApiError::NotImplemented(
        "Staking write routes are intentionally disabled on the public alpha node until delegation, undelegation, and reward-claim semantics are production-complete. Public staking reads remain available.".to_string(),
    )
}

fn public_validator_entries(ctx: &RpcContext, total_stake: u128) -> Vec<serde_json::Value> {
    let Some(address) = ctx.proposer_address.as_ref() else {
        return Vec::new();
    };

    vec![json!({
        "address": address,
        "moniker": "Current Proposer",
        "voting_power": total_stake.to_string(),
        "voting_power_formatted": format!("{:.2} DGT", (total_stake as f64) / 1_000_000.0),
        "commission": null,
        "status": if ctx.features.staking { "active" } else { "operator-preview" },
        "uptime": null,
        "delegator_count": 0,
        "pqc_enabled": ctx.validator_public_key_b64.is_some(),
        "public_key": ctx.validator_public_key_b64,
        "algorithm": ctx.validator_algorithm,
        "source": "validator_secrets"
    })]
}

fn epoch_and_slot(height: u64, slots_per_epoch: u64) -> (u64, u64) {
    let slots_per_epoch = slots_per_epoch.max(1);
    if height == 0 {
        return (0, 0);
    }

    let zero_based_height = height.saturating_sub(1);
    (
        zero_based_height / slots_per_epoch,
        zero_based_height % slots_per_epoch,
    )
}

fn estimate_transaction_size(tx: &Transaction) -> usize {
    tx.hash.len() + tx.from.len() + tx.to.len() + 64
}

fn block_gas_breakdown(
    block: &crate::storage::blocks::Block,
    storage: &Storage,
) -> (u64, u64, u64) {
    let gas_schedule = crate::gas::GasSchedule::default();
    let mut total_gas_used = 0u64;
    let mut bandwidth_gas_used = 0u64;

    for tx in &block.txs {
        if let Some(receipt) = storage.get_receipt(&tx.hash) {
            total_gas_used = total_gas_used.saturating_add(receipt.gas_used);
            let tx_bandwidth = gas_schedule
                .per_byte
                .saturating_mul(estimate_transaction_size(tx) as u64);
            bandwidth_gas_used = bandwidth_gas_used.saturating_add(tx_bandwidth);
        }
    }

    bandwidth_gas_used = bandwidth_gas_used.min(total_gas_used);
    let compute_gas_used = total_gas_used.saturating_sub(bandwidth_gas_used);
    (compute_gas_used, bandwidth_gas_used, total_gas_used)
}

fn block_response(
    block: &crate::storage::blocks::Block,
    ctx: &RpcContext,
    txs: Vec<serde_json::Value>,
) -> serde_json::Value {
    let (epoch, slot) = epoch_and_slot(block.header.height, ctx.slots_per_epoch);
    let (c_gas_used, b_gas_used, gas_used) = block_gas_breakdown(block, &ctx.storage);

    json!({
        "hash": block.hash,
        "height": block.header.height,
        "parent": block.header.parent,
        "timestamp": block.header.timestamp,
        "txs": txs,
        "asset_hashes": block.header.asset_hashes,
        "proposer": ctx.proposer_address,
        "epoch": epoch,
        "slot": slot,
        "gas_used": gas_used,
        "c_gas_used": c_gas_used,
        "b_gas_used": b_gas_used,
    })
}

#[derive(Deserialize)]
pub struct SubmitTx {
    pub signed_tx: SignedTx,
}

fn current_public_gas_price(ctx: &RpcContext) -> u64 {
    ctx.mempool.lock().unwrap().config().min_gas_price
}

fn current_default_gas_limit(ctx: &RpcContext) -> u64 {
    std::env::var("DYTALLIX_DEFAULT_GAS_LIMIT")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or_else(|| {
            let gov = ctx.governance.lock().unwrap();
            gov.get_config().gas_limit
        })
}

fn validate_signed_tx(
    signed_tx: &SignedTx,
    expected_chain_id: &str,
    expected_nonce: u64,
    _account_state: &crate::state::AccountState,
) -> Result<(), ValidationError> {
    // Verify signature. This is always enforced — there is intentionally no
    // runtime/env-var escape hatch, since one would let a misconfigured node
    // accept forged or unsigned transactions.
    if signed_tx.verify().is_err() {
        return Err(ValidationError::InvalidSignature);
    }

    // Validate transaction
    if let Err(e) = signed_tx.tx.validate(expected_chain_id) {
        if signed_tx.tx.chain_id != expected_chain_id {
            return Err(ValidationError::InvalidChainId {
                expected: expected_chain_id.to_string(),
                got: signed_tx.tx.chain_id.clone(),
            });
        }
        // If chain IDs match but validation failed, it's another error (e.g. zero amount)
        return Err(ValidationError::Internal(e.to_string()));
    }

    // Check nonce
    if signed_tx.tx.nonce < expected_nonce {
        return Err(ValidationError::InvalidNonce {
            expected: expected_nonce,
            got: signed_tx.tx.nonce,
        });
    }

    Ok(())
}

fn append_submit_log(entry: &serde_json::Value) {
    let dir = std::path::PathBuf::from("launch-evidence/tx");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("submit_demo.log");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let line = serde_json::to_string(entry).unwrap_or_else(|_| "{}".to_string());
        let _ = writeln!(f, "{line}");
    }
}

fn validate_reward_mode_submission(
    storage: &Storage,
    transaction: &Transaction,
    original: &crate::storage::transaction_record::SignedEnvelope,
) -> anyhow::Result<()> {
    if storage
        .db
        .get(crate::runtime::reward_runtime::REWARD_STATE_KEY)?
        .is_some()
    {
        let record = crate::storage::transaction_record::TransactionRecord::new(
            transaction.clone(),
            Some(original.clone()),
        )?;
        crate::signed_transaction::verify_reward_mode_record(
            &record,
            storage.get_chain_id().as_deref(),
        )?;
    }
    Ok(())
}

#[axum::debug_handler]
pub async fn submit(
    ctx: Extension<RpcContext>,
    Json(body): Json<SubmitTx>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_legacy_route(&ctx)?;
    let signed_tx = body.signed_tx;

    // Precompute basic info for logging
    let base_log = |accepted: bool, detail: serde_json::Value| {
        json!({
            "ts": current_timestamp(),
            "accepted": accepted,
            "detail": detail
        })
    };

    // Get chain ID and first sender address
    let chain_id = ctx.storage.get_chain_id().unwrap_or_default();
    let from = signed_tx.first_from_address().ok_or_else(|| {
        let err = ApiError::Validation(ValidationError::Internal(
            "no sender address found".to_string(),
        ));
        append_submit_log(&base_log(
            false,
            json!({
                "reason": "no_sender",
                "chain_id": chain_id,
            }),
        ));
        err
    })?;

    // Get current nonce and account state
    let mut state = ctx.state.lock().unwrap();
    let current_nonce = state.nonce_of(from);
    let account_state = state.get_account(from);
    drop(state);

    // Validate the signed transaction
    if let Err(ve) = validate_signed_tx(&signed_tx, &chain_id, current_nonce, &account_state) {
        let detail = match &ve {
            ValidationError::InvalidChainId { expected, got } => json!({
                "error": "INVALID_CHAIN_ID", "expected": expected, "got": got, "from": from, "nonce": signed_tx.tx.nonce
            }),
            ValidationError::InvalidNonce { expected, got } => json!({
                "error": "INVALID_NONCE", "expected": expected, "got": got, "from": from, "nonce": signed_tx.tx.nonce
            }),
            ValidationError::InvalidSignature => json!({
                "error": "INVALID_SIGNATURE", "from": from, "nonce": signed_tx.tx.nonce
            }),
            ValidationError::InsufficientFunds {
                denom,
                required,
                available,
            } => json!({
                "error": "INSUFFICIENT_FUNDS",
                "denom": denom,
                "required": required.to_string(),
                "available": available.to_string(),
                "from": from,
                "nonce": signed_tx.tx.nonce
            }),
            ValidationError::DuplicateTransaction => json!({
                "error": "DUPLICATE_TRANSACTION", "from": from, "nonce": signed_tx.tx.nonce
            }),
            ValidationError::MempoolFull => json!({
                "error": "MEMPOOL_FULL", "from": from, "nonce": signed_tx.tx.nonce
            }),
            ValidationError::Internal(msg) => json!({
                "error": "INTERNAL_ERROR", "message": msg, "from": from, "nonce": signed_tx.tx.nonce
            }),
        };
        append_submit_log(&base_log(false, detail));
        return Err(ApiError::from(ve));
    }

    // Generate transaction hash
    let tx_hash = signed_tx.tx_hash().map_err(|_| {
        let err = ApiError::Validation(ValidationError::Internal(
            "failed to generate tx hash".to_string(),
        ));
        append_submit_log(&base_log(false, json!({
            "error": "INTERNAL_ERROR", "message": "tx_hash_failed", "from": from, "nonce": signed_tx.tx.nonce
        })));
        err
    })?;

    // Check for duplicate transaction
    {
        let mempool = ctx.mempool.lock().unwrap();
        if mempool.contains(&tx_hash) {
            append_submit_log(&base_log(
                false,
                json!({
                    "error": "DUPLICATE_TRANSACTION", "tx_hash": tx_hash, "from": from
                }),
            ));
            return Err(ApiError::Validation(ValidationError::DuplicateTransaction));
        }
        if mempool.is_full() {
            append_submit_log(&base_log(
                false,
                json!({
                    "error": "MEMPOOL_FULL", "tx_hash": tx_hash
                }),
            ));
            return Err(ApiError::Validation(ValidationError::MempoolFull));
        }
    }

    let min_gas_price = current_public_gas_price(&ctx);
    let legacy_tx =
        crate::signed_transaction::normalize(&signed_tx, min_gas_price).map_err(|error| {
            if let crate::signed_transaction::NormalizationError::Fee(reason) = &error {
                append_submit_log(&base_log(
                    false,
                    json!({
                        "error": "INVALID_FEE",
                        "message": reason,
                        "tx_hash": tx_hash,
                        "fee": signed_tx.tx.fee.to_string(),
                        "min_gas_price": min_gas_price
                    }),
                ));
            }
            ApiError::BadRequest(error.to_string())
        })?;

    let original: crate::storage::transaction_record::SignedEnvelope =
        serde_json::from_value(serde_json::to_value(&signed_tx).map_err(|_| ApiError::Internal)?)
            .map_err(|_| ApiError::Internal)?;
    validate_reward_mode_submission(&ctx.storage, &legacy_tx, &original)
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;

    // Add to mempool (mempool will perform full validation including gas cost)
    let state_snapshot = {
        // Clone so we can drop the lock before touching the mempool
        ctx.state.lock().unwrap().clone()
    };
    {
        let mut live_mempool = ctx.mempool.lock().unwrap();
        let mut mempool = live_mempool.clone();
        // Gas parameters already set above
        // Use add_transaction directly to capture detailed rejection reasons
        if let Err(reason) = mempool.add_transaction_trusted(&state_snapshot, legacy_tx.clone()) {
            let (api_err, code) = match reason {
                crate::mempool::RejectionReason::InvalidSignature => {
                    (ApiError::InvalidSignature, "INVALID_SIGNATURE")
                }
                crate::mempool::RejectionReason::NonceGap { expected, got } => {
                    let e = ApiError::InvalidNonce { expected, got };
                    append_submit_log(&base_log(
                        false,
                        json!({
                            "error": "INVALID_NONCE", "expected": expected, "got": got, "tx_hash": tx_hash
                        }),
                    ));
                    return Err(e);
                }
                crate::mempool::RejectionReason::InsufficientFunds {
                    denom,
                    required,
                    available,
                } => {
                    let ve = ValidationError::InsufficientFunds {
                        denom: denom.clone(),
                        required,
                        available,
                    };
                    append_submit_log(&base_log(
                        false,
                        json!({
                            "error": "INSUFFICIENT_FUNDS",
                            "denom": denom,
                            "required": required.to_string(),
                            "available": available.to_string(),
                            "tx_hash": tx_hash
                        }),
                    ));
                    return Err(ApiError::from(ve));
                }
                crate::mempool::RejectionReason::UnderpricedGas { .. } => (
                    ApiError::BadRequest("underpriced gas".to_string()),
                    "UNDERPRICED_GAS",
                ),
                crate::mempool::RejectionReason::OversizedTx { .. } => (
                    ApiError::BadRequest("oversized transaction".to_string()),
                    "OVERSIZED_TX",
                ),
                crate::mempool::RejectionReason::Duplicate(_) => {
                    (ApiError::DuplicateTx, "DUPLICATE_TRANSACTION")
                }
                crate::mempool::RejectionReason::PolicyViolation(msg) => (
                    ApiError::BadRequest(format!("policy violation: {msg}")),
                    "POLICY_VIOLATION",
                ),
                crate::mempool::RejectionReason::InternalError(_) => {
                    (ApiError::Internal, "INTERNAL_ERROR")
                }
            };
            append_submit_log(&base_log(
                false,
                json!({
                    "error": code, "tx_hash": tx_hash
                }),
            ));
            return Err(api_err);
        }
        if let Err(error) = ctx
            .storage
            .put_pending_signed_transaction(&legacy_tx, Some(original))
        {
            tracing::error!(%error, "Pending transaction persistence failed");
            return Err(ApiError::Internal);
        }
        *live_mempool = mempool;
    }

    // Broadcast to websocket
    ctx.ws.broadcast_json(&json!({
        "type": "new_transaction",
        "hash": tx_hash
    }));

    // Success evidence log
    append_submit_log(&base_log(
        true,
        json!({
            "tx_hash": tx_hash,
            "from": from,
            "nonce": signed_tx.tx.nonce,
            "fee": signed_tx.tx.fee.to_string(),
            "chain_id": chain_id
        }),
    ));

    Ok(Json(json!({
        "hash": tx_hash,
        "status": "pending"
    })))
}

// Remove legacy helper function
// impl SignedTx { fn msgs_first_from(&self) { self.tx.msgs.get(0).and_then(|v| v.get("from")).and_then(|f| f.as_str()).map(|s| s.to_string()) } }

#[derive(Deserialize)]
pub struct BlocksQuery {
    pub offset: Option<u64>,
    pub limit: Option<u64>,
}

pub async fn list_blocks(
    Query(q): Query<BlocksQuery>,
    ctx: axum::Extension<RpcContext>,
) -> Json<serde_json::Value> {
    let limit = q.limit.unwrap_or(10).min(1000); // Increased from 100 to 1000 for better transaction history
    let height = ctx.storage.height();
    let mut blocks = vec![];
    let mut h = q.offset.unwrap_or(height);
    while h > 0 && blocks.len() < limit as usize {
        if let Some(b) = ctx.storage.get_block_by_height(h) {
            // Explicitly serialize transactions to ensure full objects are returned
            let tx_objects: Vec<serde_json::Value> = b
                .txs
                .iter()
                .map(|tx| {
                    json!({
                        "hash": tx.hash,
                        "from": tx.from,
                        "to": tx.to,
                        "amount": tx.amount.to_string(),
                        "fee": tx.fee.to_string(),
                        "nonce": tx.nonce,
                        "denom": tx.denom,
                        "signature": tx.signature,
                        "gas_limit": tx.gas_limit,
                        "gas_price": tx.gas_price,
                        "public_key": tx.public_key,
                        "chain_id": tx.chain_id,
                        "memo": tx.memo,
                    })
                })
                .collect();

            blocks.push(block_response(&b, &ctx, tx_objects));
        }
        if h == 0 {
            break;
        }
        h -= 1;
    }
    Json(json!({"blocks": blocks}))
}

/// GET /api/anchored-assets - Return all blocks with anchored assets (no limit)
pub async fn list_anchored_assets(ctx: axum::Extension<RpcContext>) -> Json<serde_json::Value> {
    let height = ctx.storage.height();
    let mut anchored_blocks = vec![];

    // Scan all blocks from genesis to current height
    for h in 1..=height {
        if let Some(b) = ctx.storage.get_block_by_height(h) {
            // Only include blocks that have anchored assets
            if !b.header.asset_hashes.is_empty() {
                anchored_blocks.push(json!({
                    "height": b.header.height,
                    "hash": b.hash,
                    "timestamp": b.header.timestamp,
                    "asset_hashes": b.header.asset_hashes,
                    "txs": b.txs.len()
                }));
            }
        }
    }

    Json(json!({
        "blocks": anchored_blocks,
        "total": anchored_blocks.len()
    }))
}

/// GET /transactions - Return recent transactions from blocks
#[derive(Deserialize)]
pub struct TransactionsQuery {
    pub offset: Option<u64>,
    pub limit: Option<u64>,
}

pub async fn list_transactions(
    Query(q): Query<TransactionsQuery>,
    ctx: axum::Extension<RpcContext>,
) -> Json<serde_json::Value> {
    let limit = q.limit.unwrap_or(50).min(1000);
    let height = ctx.storage.height();
    let mut transactions = vec![];
    let mut h = q.offset.unwrap_or(height);

    // Scan blocks from most recent backwards to find transactions
    while h > 0 && transactions.len() < limit as usize {
        if let Some(b) = ctx.storage.get_block_by_height(h) {
            for tx in b.txs.iter().rev() {
                if transactions.len() >= limit as usize {
                    break;
                }
                transactions.push(json!({
                    "hash": tx.hash,
                    "from": tx.from,
                    "to": tx.to,
                    "amount": tx.amount.to_string(),
                    "fee": tx.fee.to_string(),
                    "nonce": tx.nonce,
                    "denom": tx.denom,
                    "block_height": b.header.height,
                    "timestamp": b.header.timestamp,
                    "status": "confirmed",
                }));
            }
        }
        if h == 0 {
            break;
        }
        h -= 1;
    }

    Json(json!({
        "transactions": transactions,
        "total": transactions.len()
    }))
}

pub async fn get_block(
    Path(id): Path<String>,
    ctx: axum::Extension<RpcContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let block = if id.starts_with("0x") {
        ctx.storage.get_block_by_hash(id.clone())
    } else if id == "latest" {
        ctx.storage.get_block_by_height(ctx.storage.height())
    } else {
        id.parse::<u64>()
            .ok()
            .and_then(|h| ctx.storage.get_block_by_height(h))
    };
    if let Some(b) = block {
        let txs = b
            .txs
            .iter()
            .map(|tx| serde_json::to_value(tx).unwrap_or_else(|_| json!({"hash": tx.hash})))
            .collect();
        let obj = block_response(&b, &ctx, txs);
        Ok(Json(obj))
    } else {
        Err(ApiError::NotFound)
    }
}

// New: getTransactionReceipt endpoint (hash path param) returning full receipt metadata
#[axum::debug_handler]
pub async fn get_transaction_receipt(
    Path(hash): Path<String>,
    ctx: axum::Extension<RpcContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if let Some(r) = ctx.storage.get_receipt(&hash) {
        let mut v = serde_json::to_value(&r).unwrap();
        // The active backend cannot identify historical signatures.
        if let Some(envelope) = ctx
            .storage
            .get_transaction_record(&hash)
            .map_err(|_| ApiError::Internal)?
            .and_then(|record| record.signed_envelope)
        {
            v["pqc_algorithm"] = serde_json::json!(envelope.algorithm);
        }
        return Ok(Json(v));
    }
    Err(ApiError::NotFound)
}

pub async fn get_balance(
    Path(addr): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
    ctx: axum::Extension<RpcContext>,
) -> Json<serde_json::Value> {
    let mut state = ctx.state.lock().unwrap();

    // Check if specific denomination is requested
    if let Some(denom) = params.get("denom") {
        let bal = state.balance_of(&addr, denom);
        return Json(json!({
            "address": addr,
            "denom": denom,
            "balance": bal.to_string()
        }));
    }

    // Return all balances for the address
    let balances = state.balances_of(&addr);
    let legacy_balance = state.legacy_balance_of(&addr);

    // Format balances for multi-denomination response
    let formatted_balances: std::collections::HashMap<String, serde_json::Value> = balances
        .iter()
        .map(|(denom, amount)| {
            let denom_info = match denom.as_str() {
                "udgt" => json!({
                    "balance": amount.to_string(),
                    "formatted": format!("{} DGT", amount / 1_000_000), // Assuming 6 decimal places
                    "type": "governance",
                    "description": "Governance token for voting and staking"
                }),
                "udrt" => json!({
                    "balance": amount.to_string(),
                    "formatted": format!("{} DRT", amount / 1_000_000), // Assuming 6 decimal places
                    "type": "reward",
                    "description": "Reward token for transaction fees and staking rewards"
                }),
                _ => json!({
                    "balance": amount.to_string(),
                    "type": "unknown"
                }),
            };
            (denom.clone(), denom_info)
        })
        .collect();

    Json(json!({
        "address": addr,
        "balances": formatted_balances,
        "legacy_balance": legacy_balance.to_string() // For backward compatibility
    }))
}

pub async fn get_tx(
    Path(hash): Path<String>,
    ctx: axum::Extension<RpcContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if let Some(r) = ctx.storage.get_receipt(&hash) {
        let v = {
            #[cfg(feature = "oracle")]
            let mut tmp = serde_json::to_value(r).unwrap();
            #[cfg(not(feature = "oracle"))]
            let tmp = serde_json::to_value(r).unwrap();
            #[cfg(feature = "oracle")]
            {
                let store = OracleStore {
                    db: &ctx.storage.db,
                };
                if let Some(ai) = store.get_ai_risk(&hash) {
                    tmp["ai_risk_score"] = serde_json::json!(ai.risk_score);
                    tmp["ai_model_id"] = serde_json::json!(ai.model_id);
                    if let Some(confidence) = ai.confidence {
                        tmp["ai_confidence"] = serde_json::json!(confidence);
                    }
                }
            }
            tmp
        };
        return Ok(Json(v));
    }
    if ctx.mempool.lock().unwrap().contains(&hash) {
        let base = {
            #[cfg(feature = "oracle")]
            let mut tmp = serde_json::json!({"status":"pending","hash": hash });
            #[cfg(not(feature = "oracle"))]
            let tmp = serde_json::json!({"status":"pending","hash": hash });
            #[cfg(feature = "oracle")]
            {
                let store = OracleStore {
                    db: &ctx.storage.db,
                };
                if let Some(ai) = store.get_ai_risk(&hash) {
                    tmp["ai_risk_score"] = serde_json::json!(ai.risk_score);
                    tmp["ai_model_id"] = serde_json::json!(ai.model_id);
                    if let Some(confidence) = ai.confidence {
                        tmp["ai_confidence"] = serde_json::json!(confidence);
                    }
                }
            }
            tmp
        };
        return Ok(Json(base));
    }
    Err(ApiError::NotFound)
}

fn timed_stats_response(
    ctx: &RpcContext,
    rolling_tps: f64,
) -> Result<Option<Json<serde_json::Value>>, ApiError> {
    if let Some(supply) =
        crate::supply::inspect_timed_native(&ctx.storage).map_err(|_| ApiError::Internal)?
    {
        return Ok(Some(Json(json!({
            "height": supply.drt.height,
            "mempool_size": ctx.mempool.lock().unwrap().len(),
            "rolling_tps": rolling_tps,
            "chain_id": ctx.storage.get_chain_id(),
            "emission_denomination": "udrt",
            "emission_pools": supply.drt.pools.iter()
                .map(|(name, amount)| (name.clone(), amount.to_string())).collect::<std::collections::BTreeMap<_,_>>(),
            "issuance_timing": supply.drt.issuance_timing,
            "staking": {
                "total_stake": supply.dgt.staked.to_string(),
                "pending_bonded": supply.dgt.pending_bonded.to_string(),
                "unbonding": supply.dgt.unbonding.to_string(),
                "stake_denomination": "udgt",
                "reward_liabilities": supply.drt.staking_reward_liabilities,
            },
        }))));
    }
    Ok(None)
}

pub async fn stats(ctx: axum::Extension<RpcContext>) -> Result<Json<serde_json::Value>, ApiError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let rolling_tps = { ctx.tps.lock().unwrap().rolling_tps(now) };
    let chain_id = ctx.storage.get_chain_id();
    if let Some(response) = timed_stats_response(&ctx, rolling_tps)? {
        return Ok(response);
    }
    let em_snap = ctx.emission.lock().unwrap().snapshot();
    Ok(Json(
        json!({"height": ctx.storage.height(), "mempool_size": ctx.mempool.lock().unwrap().len(), "rolling_tps": rolling_tps, "chain_id": chain_id, "emission_pools": em_snap.pools }),
    ))
}

/// GET /api/capabilities - Machine-readable public contract for compatible node deployments.
pub async fn public_capabilities() -> Json<serde_json::Value> {
    Json(public_capabilities_document())
}

/// GET /status - Node status endpoint for health check and basic info
pub async fn status(ctx: axum::Extension<RpcContext>) -> Json<serde_json::Value> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let latest_height = ctx.storage.height();
    let (epoch, slot) = epoch_and_slot(latest_height, ctx.slots_per_epoch);
    let mempool_size = ctx.mempool.lock().unwrap().len();
    let chain_id = ctx.storage.get_chain_id();
    let min_gas_price = current_public_gas_price(&ctx);
    let default_gas_limit = current_default_gas_limit(&ctx);
    let gas_schedule = crate::gas::GasSchedule::default();

    // For this implementation, we assume the node is never syncing (single node setup)
    // In a real network, this would check sync status against peers
    let syncing = false;

    let production_state = crate::production_control::PRODUCTION.state();
    Json(json!({
        "status": production_state.health(),
        "production_state": production_state.name(),
        "latest_height": latest_height,
        "syncing": syncing,
        "mempool_size": mempool_size,
        "chain_id": chain_id,
        "capabilities_endpoint": "/api/capabilities",
        "epoch": epoch,
        "slot": slot,
        "gas": {
            "version": crate::gas::GAS_TABLE_VERSION,
            "fee_denom": "udrt",
            "min_gas_price": min_gas_price,
            "default_gas_limit": default_gas_limit,
            "default_signed_fee": u128::from(min_gas_price).saturating_mul(u128::from(default_gas_limit)),
            "transfer_base": gas_schedule.transfer_base,
            "per_byte": gas_schedule.per_byte,
            "per_additional_signature": gas_schedule.per_additional_signature,
            "per_kv_read": gas_schedule.per_kv_read,
            "per_kv_write": gas_schedule.per_kv_write
        },
        "validator": {
            "proposer": ctx.proposer_address,
            "public_key": ctx.validator_public_key_b64,
            "algorithm": ctx.validator_algorithm,
        },
        "timestamp": now
    }))
}

/// GET /health - Health check endpoint (alias for /status)
pub async fn health(ctx: axum::Extension<RpcContext>) -> Json<serde_json::Value> {
    status(ctx).await
}

/// GET /genesis - Return genesis configuration
pub async fn get_genesis() -> Result<Json<serde_json::Value>, ApiError> {
    // Read genesis.json file
    let genesis_content =
        std::fs::read_to_string("genesis.json").map_err(|_| ApiError::Internal)?;

    let mut genesis: serde_json::Value =
        serde_json::from_str(&genesis_content).map_err(|_| ApiError::Internal)?;

    // Compute genesis hash
    let genesis_bytes = serde_json::to_vec(&genesis).unwrap_or_default();
    let hash = blake3::hash(&genesis_bytes);

    // Add computed hash to metadata
    if let Some(metadata) = genesis.get_mut("metadata") {
        if let Some(obj) = metadata.as_object_mut() {
            obj.insert(
                "genesis_hash".to_string(),
                json!(format!("0x{}", hash.to_hex())),
            );
        }
    }

    Ok(Json(genesis))
}

/// GET /genesis/hash - Return just the genesis hash
pub async fn get_genesis_hash() -> Result<Json<serde_json::Value>, ApiError> {
    let genesis_content =
        std::fs::read_to_string("genesis.json").map_err(|_| ApiError::Internal)?;

    let genesis: serde_json::Value =
        serde_json::from_str(&genesis_content).map_err(|_| ApiError::Internal)?;

    let genesis_bytes = serde_json::to_vec(&genesis).unwrap_or_default();
    let hash = blake3::hash(&genesis_bytes);

    Ok(Json(json!({
        "genesis_hash": format!("0x{}", hash.to_hex()),
        "chain_id": genesis.get("chain_id").and_then(|v| v.as_str()).unwrap_or("unknown"),
        "genesis_time": genesis.get("genesis_time").and_then(|v| v.as_str()).unwrap_or("unknown")
    })))
}

pub async fn peers() -> Json<serde_json::Value> {
    Json(json!([]))
}

fn production_control_response(
    control: &crate::production_control::ProductionControl,
    paused: bool,
) -> (axum::http::StatusCode, Json<serde_json::Value>) {
    match control.set_paused(paused) {
        Ok(()) => (
            axum::http::StatusCode::OK,
            Json(json!({"ok":true,"paused":paused})),
        ),
        Err(_) => (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(
                json!({"ok":false,"paused":true,"error":"Production failed; explicit recovery and restart required"}),
            ),
        ),
    }
}
/// POST /ops/pause - pause block production. A failed producer remains failed.
pub async fn ops_pause() -> (axum::http::StatusCode, Json<serde_json::Value>) {
    production_control_response(&crate::production_control::PRODUCTION, true)
}
/// POST /ops/resume - resume only an operational producer.
pub async fn ops_resume() -> (axum::http::StatusCode, Json<serde_json::Value>) {
    production_control_response(&crate::production_control::PRODUCTION, false)
}

/// GET /metrics - Prometheus metrics exposition (text/plain)
#[cfg(feature = "metrics")]
pub async fn metrics_export(
    Extension(ctx): Extension<RpcContext>,
) -> Result<(axum::http::StatusCode, String), ApiError> {
    use prometheus::TextEncoder;
    let enc = TextEncoder::new();
    let families = ctx.metrics.gather();
    match enc.encode_to_string(&families) {
        Ok(body) => Ok((axum::http::StatusCode::OK, body)),
        Err(_) => Err(ApiError::Internal),
    }
}

/// GET /metrics - Not implemented when metrics feature is disabled
#[cfg(not(feature = "metrics"))]
pub async fn metrics_export() -> Result<(axum::http::StatusCode, String), ApiError> {
    Ok((
        axum::http::StatusCode::NOT_IMPLEMENTED,
        "# metrics feature not compiled; rebuild with --features metrics".to_string(),
    ))
}

// Bridge endpoints
pub async fn bridge_ingest(
    Extension(ctx): Extension<RpcContext>,
    Json(body): Json<bridge::IngestBridgeMessage>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_legacy_route(&ctx)?;
    bridge::ingest(Extension(ctx), Json(body)).await
}
pub async fn bridge_halt(
    Extension(ctx): Extension<RpcContext>,
    Json(body): Json<bridge::BridgeHaltToggle>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_legacy_route(&ctx)?;
    bridge::halt_toggle(Extension(ctx), Json(body)).await
}
pub async fn bridge_state(
    Extension(ctx): Extension<RpcContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    bridge::bridge_state(Extension(ctx)).await
}

pub async fn emission_claim(
    Extension(ctx): Extension<RpcContext>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let _ = (ctx, body);
    Err(ApiError::BadRequest(
        "Emission claims require a signed block transaction; direct claims are disabled".into(),
    ))
}

pub async fn gov_submit_proposal(
    Extension(ctx): Extension<RpcContext>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_legacy_route(&ctx)?;
    if !ctx.features.governance {
        return Err(governance_write_unavailable());
    }
    use crate::runtime::governance::ProposalType;

    let title = body
        .get("title")
        .and_then(|v| v.as_str())
        .ok_or(ApiError::Internal)?;
    let description = body
        .get("description")
        .and_then(|v| v.as_str())
        .ok_or(ApiError::Internal)?;
    let key = body
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or(ApiError::Internal)?;
    let value = body
        .get("value")
        .and_then(|v| v.as_str())
        .ok_or(ApiError::Internal)?;
    let height = ctx.storage.height();
    let proposal_type = ProposalType::ParameterChange {
        key: key.to_string(),
        value: value.to_string(),
    };
    match ctx.governance.lock().unwrap().submit_proposal(
        height,
        title.to_string(),
        description.to_string(),
        proposal_type,
    ) {
        Ok(proposal_id) => Ok(Json(json!({"proposal_id": proposal_id}))),
        Err(_) => Err(ApiError::Internal),
    }
}

pub async fn gov_deposit(
    Extension(ctx): Extension<RpcContext>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_legacy_route(&ctx)?;
    if !ctx.features.governance {
        return Err(governance_write_unavailable());
    }
    let depositor = body
        .get("depositor")
        .and_then(|v| v.as_str())
        .ok_or(ApiError::Internal)?;
    let proposal_id = body
        .get("proposal_id")
        .and_then(|v| v.as_u64())
        .ok_or(ApiError::Internal)?;
    let amount = body
        .get("amount")
        .and_then(|v| v.as_u64())
        .ok_or(ApiError::Internal)? as u128;
    let height = ctx.storage.height();

    match ctx
        .governance
        .lock()
        .unwrap()
        .deposit(height, depositor, proposal_id, amount, "udgt")
    {
        Ok(()) => Ok(Json(json!({"success": true}))),
        Err(e) => {
            eprintln!("Governance deposit error: {e}");
            Err(ApiError::Internal)
        }
    }
}

pub async fn gov_vote(
    Extension(ctx): Extension<RpcContext>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_legacy_route(&ctx)?;
    if !ctx.features.governance {
        return Err(governance_write_unavailable());
    }
    use crate::runtime::governance::VoteOption;

    let voter = body
        .get("voter")
        .and_then(|v| v.as_str())
        .ok_or(ApiError::Internal)?;
    let proposal_id = body
        .get("proposal_id")
        .and_then(|v| v.as_u64())
        .ok_or(ApiError::Internal)?;
    let option_str = body
        .get("option")
        .and_then(|v| v.as_str())
        .ok_or(ApiError::Internal)?;
    let height = ctx.storage.height();

    let option = match option_str {
        "yes" => VoteOption::Yes,
        "no" => VoteOption::No,
        "no_with_veto" => VoteOption::NoWithVeto,
        "abstain" => VoteOption::Abstain,
        _ => return Err(ApiError::Internal),
    };

    match ctx
        .governance
        .lock()
        .unwrap()
        .vote(height, voter, proposal_id, option)
    {
        Ok(()) => Ok(Json(json!({"success": true}))),
        Err(e) => {
            eprintln!("Governance vote error: {e}");
            Err(ApiError::Internal)
        }
    }
}

pub async fn gov_get_proposal(
    Extension(ctx): Extension<RpcContext>,
    Path(proposal_id): Path<u64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    match ctx.governance.lock().unwrap().get_proposal(proposal_id) {
        Ok(Some(proposal)) => Ok(Json(serde_json::to_value(proposal).unwrap())),
        Ok(None) => Err(ApiError::Internal),
        Err(e) => {
            eprintln!("Governance get proposal error: {e}");
            Err(ApiError::Internal)
        }
    }
}

pub async fn gov_tally(
    Extension(ctx): Extension<RpcContext>,
    Path(proposal_id): Path<u64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    match ctx.governance.lock().unwrap().tally(proposal_id) {
        Ok(tally) => Ok(Json(serde_json::to_value(tally).unwrap())),
        Err(e) => {
            eprintln!("Governance tally error: {e}");
            Err(ApiError::Internal)
        }
    }
}

pub async fn gov_execute(
    Extension(ctx): Extension<RpcContext>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_legacy_route(&ctx)?;
    if !ctx.features.governance {
        return Err(governance_write_unavailable());
    }

    let proposal_id = body
        .get("proposal_id")
        .and_then(|v| v.as_u64())
        .ok_or(ApiError::Internal)?;

    match ctx.governance.lock().unwrap().execute(proposal_id) {
        Ok(()) => Ok(Json(json!({"success": true, "proposal_id": proposal_id}))),
        Err(e) => {
            eprintln!("Governance execute error: {e}");
            Err(ApiError::BadRequest(e))
        }
    }
}
// Runtime flags control behavior; queries remain available regardless of compile features

pub async fn gov_get_config(
    Extension(ctx): Extension<RpcContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = {
        let governance = ctx.governance.lock().unwrap();
        governance.get_config().clone()
    };
    Ok(Json(serde_json::to_value(config).unwrap()))
}

/// GET /api/governance/proposals - List all governance proposals
pub async fn gov_list_proposals(
    Extension(ctx): Extension<RpcContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let governance = ctx.governance.lock().unwrap();

    match governance.get_all_proposals() {
        Ok(proposals) => {
            let mut proposal_list = Vec::new();

            for proposal in proposals {
                // Get current tally for each proposal
                let current_tally = governance.tally(proposal.id).ok();
                let total_voting_power = governance.total_voting_power().unwrap_or(1);

                let participating_voting_power = current_tally
                    .as_ref()
                    .map(|t| t.total_voting_power)
                    .unwrap_or(0);

                let quorum_met = if total_voting_power > 0 {
                    let quorum_required =
                        (total_voting_power * governance.get_config().quorum) / 10000;
                    participating_voting_power >= quorum_required
                } else {
                    false
                };

                let proposal_summary = json!({
                    "id": proposal.id,
                    "type": match &proposal.proposal_type {
                        ProposalType::ParameterChange { key, .. } => format!("ParameterChange({key})")
                    },
                    "title": proposal.title,
                    "status": proposal.status,
                    "submit_time": proposal.submit_height, // In a real impl, convert to timestamp
                    "deposit_end": proposal.deposit_end_height,
                    "voting_end": proposal.voting_end_height,
                    "current_tally": current_tally.as_ref().map(|tally| json!({
                        "yes": tally.yes.to_string(),
                        "no": tally.no.to_string(),
                        "abstain": tally.abstain.to_string(),
                        "no_with_veto": tally.no_with_veto.to_string(),
                        "total_voting_power": total_voting_power.to_string(),
                        "participating_voting_power": participating_voting_power.to_string(),
                        "quorum_met": quorum_met
                    }))
                });

                proposal_list.push(proposal_summary);
            }

            Ok(Json(json!({
                "proposals": proposal_list
            })))
        }
        Err(e) => {
            eprintln!("Failed to get proposals: {e}");
            Err(ApiError::Internal)
        }
    }
}

/// GET /api/governance/proposals/{id}/votes - Get votes for a specific proposal
pub async fn gov_get_proposal_votes(
    Extension(ctx): Extension<RpcContext>,
    Path(proposal_id): Path<u64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let governance = ctx.governance.lock().unwrap();

    match governance.get_proposal_votes(proposal_id) {
        Ok(votes) => {
            let vote_list: Vec<_> = votes
                .into_iter()
                .map(|vote| {
                    json!({
                        "voter": vote.voter,
                        "option": vote.option,
                        "voting_power": vote.weight.to_string(),
                        "timestamp": null // TODO: add timestamp to Vote struct
                    })
                })
                .collect();

            Ok(Json(json!({
                "proposal_id": proposal_id,
                "votes": vote_list
            })))
        }
        Err(e) => {
            eprintln!("Failed to get proposal votes: {e}");
            Err(ApiError::Internal)
        }
    }
}

/// GET /api/governance/voting-power/{address} - Get voting power for specific address
pub async fn gov_get_voting_power(
    Extension(ctx): Extension<RpcContext>,
    Path(address): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let governance = ctx.governance.lock().unwrap();

    match governance.voting_power(&address) {
        Ok(voting_power) => Ok(Json(json!({
            "address": address,
            "voting_power": voting_power.to_string()
        }))),
        Err(e) => {
            eprintln!("Failed to get voting power for {address}: {e}");
            Err(ApiError::Internal)
        }
    }
}

/// GET /api/governance/total-voting-power - Get total voting power
pub async fn gov_get_total_voting_power(
    Extension(ctx): Extension<RpcContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let governance = ctx.governance.lock().unwrap();

    match governance.total_voting_power() {
        Ok(total_power) => {
            let active_power = governance.active_set_voting_power().unwrap_or(total_power);

            Ok(Json(json!({
                "total_voting_power": total_power.to_string(),
                "active_set_voting_power": active_power.to_string()
            })))
        }
        Err(e) => {
            eprintln!("Failed to get total voting power: {e}");
            Err(ApiError::Internal)
        }
    }
}

/// GET /api/contracts - List minimal in-memory contracts (address + counter)
pub async fn list_contracts(
    Extension(ctx): Extension<RpcContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    #[cfg(feature = "contracts")]
    {
        let contracts = ctx.wasm_runtime.list_contracts();
        let items: Vec<_> = contracts
            .into_iter()
            .map(|deployment| {
                json!({
                    "address": deployment.address,
                    "code_hash": deployment.code_hash,
                    "code_size": deployment.code.len(),
                    "tx_hash": deployment.tx_hash,
                    "gas_used": deployment.gas_used,
                    "deployed_at": deployment.deployed_at,
                })
            })
            .collect();
        return Ok(Json(json!({ "contracts": items })));
    }

    #[cfg(not(feature = "contracts"))]
    {
        let map = ctx.wasm_contracts.lock().unwrap();
        let mut items: Vec<serde_json::Value> = Vec::new();
        for (addr, counter) in map.iter() {
            items.push(json!({"address": addr, "counter": counter}));
        }
        Ok(Json(json!({"contracts": items})))
    }
}

// Rewards API endpoints

#[derive(Deserialize)]
pub struct RewardsQuery {
    pub limit: Option<u32>,
    pub start_height: Option<u64>,
}

/// GET /api/rewards - Get recent emission events with pagination (staking optional)
pub async fn get_rewards(
    Extension(ctx): Extension<RpcContext>,
    Query(params): Query<RewardsQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let limit = params.limit.unwrap_or(50).min(500);
    let current_height = ctx.storage.height();
    let start_height = params.start_height.unwrap_or(current_height);
    let mut events = Vec::new();
    let emission = ctx.emission.lock().unwrap();
    for height in (1..=start_height.min(current_height))
        .rev()
        .take(limit as usize)
    {
        if let Some(event) = emission.get_event(height) {
            if ctx.features.staking {
                let formatted_event = json!({
                    "reward_profile": "legacy-historical",
                    "reward_mutation": "retired",
                    "height": event.height,
                    "timestamp": event.timestamp,
                    "total_emitted": event.total_emitted.to_string(),
                    "pools": {
                        "block_rewards": event.pools.get("block_rewards").unwrap_or(&0).to_string(),
                        "staking_rewards": event.pools.get("staking_rewards").unwrap_or(&0).to_string(),
                        "ai_module_incentives": event.pools.get("ai_module_incentives").unwrap_or(&0).to_string(),
                        "bridge_operations": event.pools.get("bridge_operations").unwrap_or(&0).to_string(),
                    },
                    "reward_index_after": event.reward_index_after.map(|v| v.to_string()),
                    "circulating_supply": event.circulating_supply.to_string(),
                });
                events.push(formatted_event);
            } else {
                events.push(json!({
                    "reward_profile": "legacy-historical",
                    "reward_mutation": "retired",
                    "height": event.height,
                    "timestamp": event.timestamp,
                    "total_emitted": event.total_emitted.to_string(),
                    "circulating_supply": event.circulating_supply.to_string(),
                }));
            }
        }
    }
    // Staking stats: real values if staking enabled; otherwise zeros
    let (total_stake, reward_index, pending_emission) = if ctx.features.staking {
        ctx.staking.lock().unwrap().get_stats()
    } else {
        (0, 0, 0)
    };
    Ok(Json(json!({
        "reward_profile": "legacy-historical",
        "reward_mutation": "retired",
        "events": events,
        "pagination": {"limit": limit, "start_height": start_height, "total_available": current_height},
        "staking_stats": {
            "total_stake": total_stake.to_string(),
            "reward_index": reward_index.to_string(),
            "pending_emission": pending_emission.to_string(),
        }
    })))
}

/// GET /api/rewards/:height - Get emission event for specific height
pub async fn get_rewards_by_height(
    Extension(ctx): Extension<RpcContext>,
    Path(height): Path<u64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let emission = ctx.emission.lock().unwrap();

    match emission.get_event(height) {
        Some(event) => {
            // Format numbers as strings to prevent JS precision issues
            let formatted_event = json!({
                "reward_profile": "legacy-historical",
                    "reward_mutation": "retired",
                    "height": event.height,
                "timestamp": event.timestamp,
                "total_emitted": event.total_emitted.to_string(),
                "pools": {
                    "block_rewards": event.pools.get("block_rewards").unwrap_or(&0).to_string(),
                    "staking_rewards": event.pools.get("staking_rewards").unwrap_or(&0).to_string(),
                    "ai_module_incentives": event.pools.get("ai_module_incentives").unwrap_or(&0).to_string(),
                    "bridge_operations": event.pools.get("bridge_operations").unwrap_or(&0).to_string(),
                },
                "reward_index_after": event.reward_index_after.map(|v| v.to_string()),
                "circulating_supply": event.circulating_supply.to_string(),
            });
            Ok(Json(formatted_event))
        }
        None => Err(ApiError::Internal), // Could return 404 instead
    }
}

/// POST /dev/faucet - Development-only faucet to credit balances directly
/// Body: { "address": string, "udgt": u64 (optional), "udrt": u64 (optional) }
pub async fn dev_faucet(
    Extension(ctx): Extension<RpcContext>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_legacy_route(&ctx)?;
    let addr = payload
        .get("address")
        .and_then(|v| v.as_str())
        .ok_or(ApiError::BadRequest("missing address".to_string()))?;
    let udgt = payload
        .get("udgt")
        .and_then(|v| v.as_u64())
        .unwrap_or(1_000_000_000); // Default 1,000 DGT
    let udrt = payload
        .get("udrt")
        .and_then(|v| v.as_u64())
        .unwrap_or(10_000_000_000); // Default 10,000 DRT
    let dgt_credited = {
        let mut st = ctx.state.lock().unwrap();
        // DRT (the fee/reward token) is uncapped and minted freely.
        st.credit(addr, "udrt", udrt as u128);
        // DGT is fixed-supply: route through the capped mint path. Once the 1B
        // cap is reached (it is, after genesis), this mints nothing — DGT is not
        // faucet-distributed; it is allocated at genesis. DRT still dispenses.
        st.mint_dgt(addr, udgt as u128).unwrap_or(0)
    };
    Ok(Json(serde_json::json!({
        "success": true,
        "address": addr,
        "credited": {"udgt": dgt_credited.to_string(), "udrt": udrt.to_string()}
    })))
}

/// Enhanced stats endpoint with emission data (staking optional)
pub async fn stats_with_emission(
    Extension(ctx): Extension<RpcContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Get base stats
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let rolling_tps = { ctx.tps.lock().unwrap().rolling_tps(now) };
    let chain_id = ctx.storage.get_chain_id();
    if let Some(response) = timed_stats_response(&ctx, rolling_tps)? {
        return Ok(response);
    }
    let em_snap = ctx.emission.lock().unwrap().snapshot();

    // Get latest emission event
    let current_height = ctx.storage.height();
    let latest_emission_event = ctx.emission.lock().unwrap().get_event(current_height);

    // Get staking stats
    let (total_stake, reward_index, pending_emission) = if ctx.features.staking {
        ctx.staking.lock().unwrap().get_stats()
    } else {
        (0, 0, 0)
    };

    Ok(Json(json!({
        "height": ctx.storage.height(),
        "mempool_size": ctx.mempool.lock().unwrap().len(),
        "rolling_tps": rolling_tps,
        "chain_id": chain_id,
        "emission_profile": "legacy-historical",
        "legacy_reward_mutation": "retired",
        "emission_pools": em_snap.pools,
        "latest_emission": latest_emission_event.map(|event| json!({
            "height": event.height,
            "total_emitted": event.total_emitted.to_string(),
            "circulating_supply": event.circulating_supply.to_string(),
        })),
        "staking": {"total_stake": total_stake.to_string(), "reward_index": reward_index.to_string(), "pending_emission": pending_emission.to_string()},
    })))
}

/// POST /api/staking/claim - Claim staking rewards for an address
pub async fn staking_claim(
    Extension(ctx): Extension<RpcContext>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let _ = (ctx, body);
    Err(staking_write_unavailable())
}

/// GET /api/staking/accrued/:address - Get accrued rewards for an address
pub async fn staking_get_accrued(
    Extension(ctx): Extension<RpcContext>,
    Path(address): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if let Some(owner) = crate::supply::inspect_reward_owner(&ctx.storage, &address)
        .map_err(|_| ApiError::Internal)?
    {
        return Ok(Json(json!({
            "address": address,
            "accrued_rewards": owner.unpaid.to_string(),
            "reward_index": "0",
            "reward_version": 2,
            "reward_denomination": "udrt",
        })));
    }
    let (accrued, reward_index) = if ctx.features.staking {
        let staking = ctx.staking.lock().unwrap();
        (staking.get_accrued_rewards(&address), staking.get_stats().1)
    } else {
        (0, 0)
    };
    Ok(Json(json!({
        "reward_profile": "legacy-historical",
        "reward_mutation": "retired",
        "address": address,
        "accrued_rewards": accrued.to_string(),
        "reward_index": reward_index.to_string(),
    })))
}

/// GET /api/staking/balance/:delegator - Get staking balance for a delegator
pub async fn staking_get_balance(
    Extension(ctx): Extension<RpcContext>,
    Path(delegator): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if let Some(owner) = crate::supply::inspect_reward_owner(&ctx.storage, &delegator)
        .map_err(|_| ApiError::Internal)?
    {
        return Ok(Json(json!({
            "delegator": delegator,
            "staked": owner.bonded.to_string(),
            "pending_bonded": owner.pending_bonded.to_string(),
            "unbonding": owner.unbonding.to_string(),
            "liquid": owner.liquid.to_string(),
            "rewards": owner.unpaid.to_string(),
            "reward_version": 2,
            "stake_denomination": "udgt",
            "reward_denomination": "udrt",
        })));
    }
    let (staked, rewards) = if ctx.features.staking {
        let staking = ctx.staking.lock().unwrap();
        (
            staking.get_total_stake(&delegator),
            staking.get_accrued_rewards(&delegator),
        )
    } else {
        (0, 0)
    };

    let liquid = if let Ok(state) = ctx.state.lock() {
        state.get_balance(&delegator, "udgt")
    } else {
        0
    };

    Ok(Json(json!({
        "reward_profile": "legacy-historical",
        "reward_mutation": "retired",
        "delegator": delegator,
        "staked": staked.to_string(),
        "liquid": liquid.to_string(),
        "rewards": rewards.to_string()
    })))
}

/// POST /api/staking/delegate - Delegate tokens to a validator
#[axum::debug_handler]
pub async fn staking_delegate(
    Extension(ctx): Extension<RpcContext>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let _ = (ctx, payload);
    Err(staking_write_unavailable())
}

/// POST /api/staking/undelegate - Undelegate tokens from a validator
#[axum::debug_handler]
pub async fn staking_undelegate(
    Extension(ctx): Extension<RpcContext>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let _ = (ctx, payload);
    Err(staking_write_unavailable())
}

/// GET /api/staking/stats - Get overall staking statistics
pub async fn staking_get_stats(
    Extension(ctx): Extension<RpcContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let supply = crate::supply::inspect_native(&ctx.storage).map_err(|error| {
        tracing::error!(%error, "Native accounting unavailable");
        ApiError::Internal
    })?;
    let reward_index = supply.staking_reward_index;
    let pending_emission = supply.pending_staking_emission;
    let reward_rate_bps = supply.reward_rate_bps;
    let total_stake = supply.dgt.staked;
    let ratio_bps = supply
        .dgt
        .stake_basis_points()
        .map_err(|_| ApiError::Internal)?;
    let validator_count = public_validator_entries(&ctx, total_stake).len();
    Ok(Json(json!({
        "height": supply.dgt.height,
        "total_stake": total_stake.to_string(),
        "total_stake_formatted": format!("{:.2} DGT", (total_stake as f64) / 1_000_000.0),
        "total_supply": supply.dgt.issued.to_string(),
        "supply_cap": crate::state::DGT_MAX_SUPPLY.to_string(),
        "legacy_reward_fields_status": "historical-read-only",
        "reward_index": reward_index.to_string(),
        "pending_emission": pending_emission.to_string(),
        "staking_ratio": format!("{}.{:02}%", ratio_bps / 100, ratio_bps % 100),
        "staking_ratio_basis_points": ratio_bps,
        "apy": null,
        "apy_status": "not_qualified",
        "reward_rate_bps": reward_rate_bps,
        "total_validators": validator_count,
        "active_validators": validator_count
    })))
}

/// GET /api/staking/validators - Get list of validators with stats
pub async fn staking_get_validators(
    Extension(ctx): Extension<RpcContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (total_stake, reward_index, _) = if ctx.features.staking {
        ctx.staking.lock().unwrap().get_stats()
    } else {
        (0, 0, 0)
    };

    let validators = public_validator_entries(&ctx, total_stake);

    Ok(Json(json!({
        "validators": validators,
        "total_validators": validators.len(),
        "active_validators": validators.len(),
        "total_voting_power": total_stake.to_string(),
        "legacy_reward_fields_status": "historical-read-only",
        "reward_index": reward_index.to_string()
    })))
}

/// GET /transactions/pending - List pending transactions in mempool
pub async fn get_pending_transactions(
    Extension(ctx): Extension<RpcContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mempool = ctx.mempool.lock().unwrap();
    let pending_txs = mempool.take_snapshot(1000); // Get up to 1000 pending transactions

    let tx_list: Vec<serde_json::Value> = pending_txs
        .iter()
        .map(|tx| {
            json!({
                "tx_hash": tx.hash,
                "from": tx.from,
                "to": tx.to,
                "amount": tx.amount.to_string(),
                "fee": tx.fee.to_string(),
                "nonce": tx.nonce,
                "gas_limit": tx.gas_limit,
                "gas_price": tx.gas_price
            })
        })
        .collect();

    Ok(Json(json!({
        "pending_transactions": tx_list,
        "count": tx_list.len()
    })))
}

/// GET /params/staking_reward_rate -> plain decimal string (e.g. "0.0500")
pub async fn params_staking_reward_rate(
    Extension(ctx): Extension<RpcContext>,
) -> Result<axum::response::Response, ApiError> {
    let staking = ctx.staking.lock().unwrap();
    let bps = staking.get_reward_rate_bps();
    let frac = (bps as f64) / 10_000.0;
    Ok(axum::response::IntoResponse::into_response((
        [
            ("x-dytallix-reward-profile", "legacy-historical"),
            ("x-dytallix-reward-mutation", "retired"),
        ],
        format!("{frac:.4}"),
    )))
}

/// JSON-RPC 2.0 endpoint stub (minimal WASM contract support)
pub async fn json_rpc(
    Extension(_ctx): Extension<RpcContext>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let method = body
        .get("method")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::BadRequest("missing method".to_string()))?;

    // Stub implementation for MVP
    Ok(Json(json!({
        "jsonrpc": "2.0",
        "id": body.get("id").cloned().unwrap_or(json!(1)),
        "result": {
            "status": "not_implemented",
            "message": format!("Method {} not yet implemented", method)
        }
    })))
}

/// POST /asset/register - Asset registration stub
pub async fn asset_register(
    Extension(ctx): Extension<RpcContext>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_legacy_route(&ctx)?;
    // Extract asset hash and metadata from request
    let params = body.get("params").and_then(|v| v.as_array());

    if params.is_none() {
        return Err(ApiError::BadRequest("Missing params array".to_string()));
    }

    let params = params.unwrap();
    if params.len() < 2 {
        return Err(ApiError::BadRequest(
            "Expected params: [asset_hash, metadata]".to_string(),
        ));
    }

    let asset_hash = params[0].as_str().unwrap_or("unknown");
    let metadata_str = params[1].as_str().unwrap_or("{}");

    // Get current block height for the transaction
    let current_height = ctx.storage.height();
    let next_height = current_height + 1;

    // Generate a transaction hash for this asset registration
    let tx_hash = format!(
        "dytallix_anchor_{}",
        blake3::hash(asset_hash.as_bytes()).to_hex()
    );

    // Add asset hash to pending assets for next block
    ctx.pending_assets
        .lock()
        .unwrap()
        .push(asset_hash.to_string());

    // Log the asset registration
    eprintln!(
        "[Asset Registry] Registered asset: {} at block {}",
        asset_hash, next_height
    );
    eprintln!("[Asset Registry] Metadata: {}", metadata_str);

    // Return success response with transaction details
    Ok(Json(json!({
        "success": true,
        "tx_hash": tx_hash,
        "block_height": next_height,
        "asset_hash": asset_hash,
        "timestamp": SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        "message": "Asset registered successfully"
    })))
}

/// POST /asset/verify - Verify that an asset hash is actually anchored on chain.
pub async fn asset_verify(
    Extension(ctx): Extension<RpcContext>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Extract asset hash from request
    let params = body.get("params").and_then(|v| v.as_array());

    if params.is_none() {
        return Err(ApiError::BadRequest("Missing params array".to_string()));
    }

    let params = params.unwrap();
    if params.is_empty() {
        return Err(ApiError::BadRequest(
            "Expected params: [asset_hash]".to_string(),
        ));
    }

    let asset_hash = params[0].as_str().unwrap_or("unknown");
    let current_height = ctx.storage.height();

    // Verify against the actual on-chain registry instead of trusting the
    // caller: scan committed block headers for the anchored asset hash.
    let mut anchored_height: Option<u64> = None;
    for h in 1..=current_height {
        if let Some(b) = ctx.storage.get_block_by_height(h) {
            if b.header.asset_hashes.iter().any(|a| a == asset_hash) {
                anchored_height = Some(b.header.height);
                break;
            }
        }
    }

    // Also accept assets registered in this session but not yet sealed into a block.
    let pending = ctx
        .pending_assets
        .lock()
        .unwrap()
        .iter()
        .any(|a| a == asset_hash);

    let verified = anchored_height.is_some() || pending;
    Ok(Json(json!({
        "verified": verified,
        "asset_hash": asset_hash,
        "block_height": anchored_height,
        "pending": pending && anchored_height.is_none(),
        "message": if verified {
            "Asset found on chain"
        } else {
            "Asset not found on chain"
        }
    })))
}

/// POST /asset/get - Asset retrieval stub
pub async fn asset_get(
    Extension(_ctx): Extension<RpcContext>,
    Json(_body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(Json(json!({
        "status": "not_implemented",
        "message": "Asset retrieval not yet implemented"
    })))
}

/// POST /faucet - Testnet faucet to credit tokens to an address
/// This is an administrative faucet helper that directly credits testnet funds.
/// Request body: { "address": "dytallix1...", "dgt_amount": 10, "drt_amount": 100 }
/// Amount inputs are whole-token grants. The handler converts them to micro-units internally.
/// Success responses expose `funded` in whole tokens and keep `credited` for micro-unit detail.
pub async fn faucet(
    Extension(ctx): Extension<RpcContext>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_legacy_route(&ctx)?;
    // Parse request
    let address = body
        .get("address")
        .and_then(|v| v.as_str())
        .ok_or(ApiError::BadRequest("address is required".to_string()))?;

    // Validate address format
    if !address.starts_with("dyt") && !address.starts_with("dytallix") {
        return Err(ApiError::BadRequest("Invalid address format".to_string()));
    }

    // Get amounts (default: 10 DGT, 100 DRT)
    let dgt_amount = body
        .get("dgt_amount")
        .and_then(|v| v.as_u64())
        .map(|v| v as u128 * 1_000_000) // Convert whole units to micro-units
        .unwrap_or(10_000_000); // Default 10 DGT

    let drt_amount = body
        .get("drt_amount")
        .and_then(|v| v.as_u64())
        .map(|v| v as u128 * 1_000_000) // Convert whole units to micro-units
        .unwrap_or(100_000_000); // Default 100 DRT

    // Credit the tokens
    {
        let mut state = ctx.state.lock().unwrap();
        if dgt_amount > 0 {
            // DGT is fixed-supply; route through the capped mint path (mints
            // nothing once the genesis-allocated cap is reached).
            let _ = state.mint_dgt(address, dgt_amount);
        }
        if drt_amount > 0 {
            state.credit(address, "udrt", drt_amount);
        }
    }

    // Log the faucet distribution
    eprintln!(
        "[FAUCET] Credited {} udgt and {} udrt to {}",
        dgt_amount, drt_amount, address
    );

    // Return success response
    Ok(Json(json!({
        "success": true,
        "address": address,
        "funded": {
            "dgt": dgt_amount / 1_000_000,
            "drt": drt_amount / 1_000_000
        },
        "message": "Tokens sent successfully",
        "credited": {
            "dgt": {
                "amount": dgt_amount,
                "denom": "udgt",
                "formatted": format!("{} DGT", dgt_amount / 1_000_000)
            },
            "drt": {
                "amount": drt_amount,
                "denom": "udrt",
                "formatted": format!("{} DRT", drt_amount / 1_000_000)
            }
        },
        "timestamp": current_timestamp()
    })))
}

#[cfg(test)]
mod public_contract_tests {
    use super::public_capabilities_document;

    #[test]
    fn capabilities_document_advertises_direct_contract_endpoint() {
        let document = public_capabilities_document();
        assert_eq!(
            document["publicNode"]["contractEndpoint"]["path"].as_str(),
            Some("GET /api/capabilities")
        );
        assert!(document["publicNode"]["directNodeOnlyRoutes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|route| route.as_str() == Some("GET /v1/validators")));
        assert_eq!(
            document["validatorDiscovery"]["publicShape"].as_str(),
            Some("daddr-compatible-addresses-only")
        );
    }
}

/// Committed DRT custody totals. Errors do not return a partial supply estimate.
pub async fn drt_supply(
    Extension(ctx): Extension<RpcContext>,
) -> Result<Json<crate::supply::SupplyResponse>, axum::http::StatusCode> {
    crate::supply::inspect(&ctx.storage)
        .map(|s| Json(s.response()))
        .map_err(|error| {
            tracing::error!(%error, "DRT accounting unavailable");
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        })
}

/// Committed DGT issuance, liquid balances, and funded stake.
pub async fn dgt_supply(
    Extension(ctx): Extension<RpcContext>,
) -> Result<Json<crate::supply::DgtSupplyResponse>, axum::http::StatusCode> {
    crate::supply::inspect_native(&ctx.storage)
        .map(|s| Json(s.dgt.response()))
        .map_err(|error| {
            tracing::error!(%error, "Native accounting unavailable");
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        })
}

/// Stored normalized transaction and original parsed signed envelope.
pub async fn get_transaction_record(
    Path(hash): Path<String>,
    Extension(ctx): Extension<RpcContext>,
) -> Result<Json<crate::storage::transaction_record::TransactionRecord>, ApiError> {
    ctx.storage
        .get_transaction_record(&hash)
        .map_err(|error| {
            tracing::error!(%error, "Transaction record unavailable");
            ApiError::Internal
        })?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

#[cfg(test)]
mod production_control_tests {
    use super::*;
    #[test]
    fn resume_reports_unavailable_after_producer_failure() {
        let control = crate::production_control::ProductionControl::new();
        assert_eq!(
            production_control_response(&control, true).0,
            axum::http::StatusCode::OK
        );
        assert_eq!(
            production_control_response(&control, false).0,
            axum::http::StatusCode::OK
        );
        control.fail();
        let (status, body) = production_control_response(&control, false);
        assert_eq!(status, axum::http::StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body.0["ok"], false);
        assert_eq!(body.0["paused"], true);
    }
}

#[cfg(all(test, any(feature = "pqc-fips204", feature = "pqc-real")))]
mod reward_admission_tests {
    use super::*;
    use crate::crypto::{ActivePQC, PQC};
    use crate::types::tx::{Msg, Tx};

    fn ordinary_send(sender_override: Option<&str>) -> SignedTx {
        let (sk, pk) = ActivePQC::keypair();
        #[cfg(feature = "pqc-fips204")]
        let algorithm = crate::addr::OriginKeyAlgorithm::MlDsa65;
        #[cfg(feature = "pqc-real")]
        let algorithm = crate::addr::OriginKeyAlgorithm::LegacyDilithium5;
        let sender = crate::addr::initial_address(
            crate::addr::AddressNetwork::Development,
            "admission-local",
            algorithm,
            &pk,
        )
        .unwrap();
        SignedTx::sign(
            Tx {
                chain_id: "admission-local".into(),
                nonce: 1,
                fee: 21_000_000,
                memo: String::new(),
                msgs: vec![Msg::Send {
                    from: sender_override.unwrap_or(&sender).into(),
                    to: "receiver".into(),
                    denom: "udgt".into(),
                    amount: 1,
                }],
            },
            &sk,
            &pk,
        )
        .unwrap()
    }
    #[test]
    fn reward_admission_applies_block_origin_rules_to_ordinary_sends() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path().join("db")).unwrap();
        storage.db.put("meta:chain_id", b"admission-local").unwrap();
        let authorized = ordinary_send(None);
        let unauthorized = ordinary_send(authorized.first_from_address());
        let check = |signed: &SignedTx| {
            let tx = crate::signed_transaction::normalize(signed, 1_000).unwrap();
            let original = serde_json::from_value(serde_json::to_value(signed).unwrap()).unwrap();
            validate_reward_mode_submission(&storage, &tx, &original)
        };
        check(&unauthorized).unwrap(); // Legacy admission remains unchanged.
        storage
            .db
            .put(crate::runtime::reward_runtime::REWARD_STATE_KEY, b"present")
            .unwrap();
        check(&authorized).unwrap();
        assert!(check(&unauthorized)
            .unwrap_err()
            .to_string()
            .contains("signing origin key"));
        assert!(!storage
            .db
            .prefix_iterator(b"tx:")
            .any(|item| item.unwrap().0.starts_with(b"tx:")));
    }
}
