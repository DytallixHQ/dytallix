use crate::crypto::PQCManager;
use crate::types::Amount as Tokens;
use base64::Engine as _;
use chrono; // for block timestamps
use futures_util::{SinkExt, StreamExt};
use log::{error, info}; // removed unused warn import
use once_cell::sync::Lazy;
use rand; // for block hash generation
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast;
use warp::reply::Reply;
use warp::Filter; // ensure accessible

// Replace direct dytallix_contracts runtime imports with wrapper exposed via crate::contracts
use crate::contracts::{ContractCall, ContractDeployment, ContractRuntime};

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
    message: String,
}

#[derive(Debug, Serialize, Clone)]
struct BlockInfo {
    number: u64,
    hash: String,
    parent_hash: String,
    timestamp: u64,
    transactions: Vec<String>,
    size: u64,
    gas_used: u64,
    gas_limit: u64,
}

#[derive(Debug, Deserialize)]
struct TransferRequest {
    from: String,
    to: String,
    #[serde(with = "crate::types::serde_string_or_number")]
    amount: Tokens,
    #[serde(with = "crate::types::serde_opt_u128_string")]
    fee: Option<Tokens>,
    nonce: Option<u64>,
    signature: Option<TransferSignature>,
}

#[derive(Debug, Deserialize)]
struct TransferSignature {
    algorithm: String,
    public_key: String,
    data: String,
}

#[derive(Debug, Serialize)]
struct ApiResponse<T> {
    success: bool,
    data: Option<T>,
    error: Option<String>,
}

#[cfg_attr(not(feature = "api-websocket"), allow(dead_code))]
#[derive(Debug, Serialize)]
struct BlockchainStats {
    // removed underscore - struct is intended for API responses
    current_block: u64,
    total_transactions: usize,
    network_peers: usize,
    mempool_size: usize,
}

#[cfg_attr(not(feature = "api-websocket"), allow(dead_code))]
#[derive(Debug, Serialize)]
struct PeerInfo {
    // removed underscore
    id: String,
    address: String,
    status: String,
    last_seen: u64,
    block_height: u64,
    protocol_version: String,
}

#[cfg_attr(not(feature = "api-websocket"), allow(dead_code))]
#[derive(Debug, Serialize)]
struct SystemStatus {
    // removed underscore
    version: String,
    uptime: u64,
    block_height: u64,
    peer_count: usize,
    mempool_size: usize,
    sync_status: String,
    chain_id: String,
}

#[derive(Debug, Serialize, Clone)]
struct WebSocketMessage {
    message_type: String,
    timestamp: u64,
    data: serde_json::Value,
}

impl WebSocketMessage {
    fn new_block(block: &BlockInfo) -> Self {
        Self {
            message_type: "new_block".to_string(),
            timestamp: chrono::Utc::now().timestamp() as u64,
            data: serde_json::to_value(block).unwrap_or_default(),
        }
    }

    #[allow(dead_code)]
    fn new_transaction(tx: &TransactionDetails) -> Self {
        Self {
            message_type: "new_transaction".to_string(),
            timestamp: chrono::Utc::now().timestamp() as u64,
            data: serde_json::to_value(tx).unwrap_or_default(),
        }
    }

    #[allow(dead_code)]
    fn status_update(status: &SystemStatus) -> Self {
        Self {
            message_type: "status_update".to_string(),
            timestamp: chrono::Utc::now().timestamp() as u64,
            data: serde_json::to_value(status).unwrap_or_default(),
        }
    }
}

#[cfg_attr(not(feature = "api-websocket"), allow(dead_code))]
#[derive(Debug, Serialize)]
struct TransactionResponse {
    // removed underscore
    hash: String,
    status: String,
    block_number: Option<u64>,
}

#[cfg_attr(not(feature = "api-websocket"), allow(dead_code))]
#[derive(Debug, Serialize)]
struct TransactionDetails {
    // removed underscore
    hash: String,
    from: String,
    to: String,
    amount: u128,
    fee: u128,
    nonce: u64,
    status: String,
    block_number: Option<u64>,
    timestamp: u64,
    confirmations: u64,
}

#[cfg_attr(not(feature = "staking"), allow(dead_code))]
#[derive(Debug, Deserialize)]
struct _StakingRegisterRequest {
    // underscore
    #[allow(dead_code)]
    address: String,
    #[allow(dead_code)]
    consensus_pubkey: String,
    #[allow(dead_code)]
    commission_rate: u16,
}

#[cfg_attr(not(feature = "staking"), allow(dead_code))]
#[derive(Debug, Deserialize)]
struct _StakingDelegateRequest {
    // underscore
    #[allow(dead_code)]
    delegator: String,
    #[allow(dead_code)]
    validator: String,
    #[allow(dead_code)]
    amount: u128,
}

#[cfg_attr(not(feature = "staking"), allow(dead_code))]
#[derive(Debug, Deserialize)]
struct _StakingClaimRequest {
    // underscore
    #[allow(dead_code)]
    delegator: String,
    #[allow(dead_code)]
    validator: String,
}

#[cfg_attr(not(feature = "staking"), allow(dead_code))]
#[derive(Debug, Serialize)]
struct ValidatorResponse {
    address: String,
    total_stake: u128,
    status: String,
    commission_rate: u16,
    self_stake: u128,
}

#[cfg_attr(not(feature = "staking"), allow(dead_code))]
#[derive(Debug, Serialize)]
struct _DelegationResponse {
    // underscore
    delegator_address: String,
    validator_address: String,
    stake_amount: u128,
    pending_rewards: u128,
}

#[derive(Debug, Serialize)]
struct StakingStatsResponse {
    total_stake: u128,
    total_validators: u32,
    active_validators: u32,
}

impl<T> ApiResponse<T> {
    fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    #[allow(dead_code)]
    fn error(message: String) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message),
        }
    }
}

// Address validation regex (dyt1 + 10+ lowercase alphanumerics)
static ADDRESS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^dyt1[0-9a-z]{10,}$").unwrap());
const MIN_FEE: Tokens = 1;
const MAX_TX_BODY: usize = 8192;

// Removed runtime_mocks function - block production is now enabled by default

// Temporarily implementing basic API server for testing
pub async fn start_api_server() -> Result<(), Box<dyn std::error::Error>> {
    info!("Initializing Dytallix API server...");
    // Create broadcast channel for WebSocket messages
    let (ws_tx, _) = broadcast::channel::<WebSocketMessage>(1000);
    let ws_tx = Arc::new(ws_tx);

    // Shared state placeholders (TODO: wire real storage & pools)
    let storage = Arc::new(crate::storage::StorageManager::new().await?);
    let tx_pool = Arc::new(crate::types::TransactionPool::new(10_000));

    // Start block production - create blocks automatically every 10 seconds
    let ws_tx_clone = ws_tx.clone();
    let storage_clone = storage.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(10));
        loop {
            interval.tick().await;

            // Get current height and create next block
            let current_height = storage_clone.get_height().unwrap_or(0);
            let next_height = current_height + 1;

            // Create a new block
            let block = crate::types::Block {
                header: crate::types::BlockHeader {
                    number: next_height,
                    parent_hash: if current_height == 0 {
                        "0x0000000000000000000000000000000000000000000000000000000000000000"
                            .to_string()
                    } else {
                        format!("0x{:064x}", rand::random::<u64>())
                    },
                    transactions_root: format!("0x{:064x}", rand::random::<u64>()),
                    state_root: format!("0x{:064x}", rand::random::<u64>()),
                    timestamp: chrono::Utc::now().timestamp() as u64,
                    validator: "dyt1validator000000000000000000000000000000000000".to_string(),
                    signature: crate::types::PQCBlockSignature {
                        signature: dytallix_pqc::Signature {
                            data: vec![], // Empty signature for now
                            algorithm: dytallix_pqc::SignatureAlgorithm::Dilithium3,
                        },
                        public_key: vec![],
                    },
                    nonce: 0,
                },
                transactions: vec![], // Empty block for now
            };

            // Store the block
            if let Err(e) = storage_clone._store_block(&block) {
                error!("Failed to store block {}: {}", next_height, e);
                continue;
            }

            info!("Created and stored block #{}", next_height);

            // Send WebSocket notification
            let block_info = BlockInfo {
                number: next_height,
                hash: block.hash(),
                parent_hash: block.header.parent_hash.clone(),
                timestamp: block.header.timestamp,
                transactions: vec![],
                size: 0,
                gas_used: 0,        // Default value since we don't have gas tracking yet
                gas_limit: 1000000, // Default value
            };
            let _ = ws_tx_clone.send(WebSocketMessage::new_block(&block_info));
        }
    });

    // WebSocket endpoint
    let ws_tx_root = ws_tx.clone();
    let websocket = warp::path("ws")
        .and(warp::ws())
        .and(warp::any().map(move || ws_tx_root.clone()))
        .map(
            |ws: warp::ws::Ws, ws_tx: Arc<broadcast::Sender<WebSocketMessage>>| {
                ws.on_upgrade(move |websocket| handle_websocket(websocket, ws_tx))
            },
        );

    // Peers route (returns empty list until networking integrated)
    let peers = warp::path("peers")
        .and(warp::get())
        .map(|| {
            warp::reply::with_status(
                warp::reply::json(&ApiResponse::success(Vec::<String>::new())),
                warp::http::StatusCode::OK,
            )
            .into_response()
        })
        .boxed();

    // Health
    let health = warp::path("health")
        .and(warp::get())
        .map(|| {
            warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"status":"ok","service":"dytallix-node"})),
                warp::http::StatusCode::OK,
            )
            .into_response()
        })
        .boxed();

    // Balance (state-backed)
    let storage_balance = storage.clone();
    let balance = warp::path("balance")
        .and(warp::path::param::<String>())
        .and(warp::get())
        .and(warp::any().map(move || storage_balance.clone()))
        .and_then(
            |address: String, storage: Arc<crate::storage::StorageManager>| async move {
                if !ADDRESS_RE.is_match(&address) {
                    return Ok::<_, warp::Rejection>(
                        warp::reply::with_status(
                            warp::reply::json(&ApiResponse::<()> {
                                success: false,
                                data: None,
                                error: Some("invalid_address".into()),
                            }),
                            warp::http::StatusCode::BAD_REQUEST,
                        )
                        .into_response(),
                    );
                }
                match storage.get_address_balance(&address).await {
                    Ok(bal) => Ok(warp::reply::with_status(
                        warp::reply::json(&ApiResponse::success(bal)),
                        warp::http::StatusCode::OK,
                    )
                    .into_response()),
                    Err(e) => {
                        error!("balance error: {e}");
                        Ok(warp::reply::with_status(
                            warp::reply::json(&ApiResponse::<()> {
                                success: false,
                                data: None,
                                error: Some("internal_error".into()),
                            }),
                            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                        )
                        .into_response())
                    }
                }
            },
        )
        .boxed();

    // Submit TX (transfer only for MV(T))
    let storage_submit = storage.clone();
    let tx_pool_submit = tx_pool.clone();
    let ws_tx_submit = ws_tx.clone();
    let submit_tx = warp::path("submit")
        .and(warp::post())
        .and(warp::header::optional::<String>("content-length"))
        .and(warp::body::bytes())
        .and(warp::any().map(move || {
            (
                storage_submit.clone(),
                tx_pool_submit.clone(),
                ws_tx_submit.clone(),
            )
        }))
        .and_then(
            |content_length: Option<String>,
             body: bytes::Bytes,
             ctx: (
                Arc<crate::storage::StorageManager>,
                Arc<crate::types::TransactionPool>,
                Arc<broadcast::Sender<WebSocketMessage>>,
            )| async move {
                if let Some(len_str) = content_length {
                    if let Ok(len) = len_str.parse::<usize>() {
                        if len > MAX_TX_BODY {
                            return Ok::<_, warp::Rejection>(
                                warp::reply::with_status(
                                    warp::reply::json(&ApiResponse::<()> {
                                        success: false,
                                        data: None,
                                        error: Some("invalid_body".into()),
                                    }),
                                    warp::http::StatusCode::PAYLOAD_TOO_LARGE,
                                )
                                .into_response(),
                            );
                        }
                    }
                }
                if body.len() > MAX_TX_BODY {
                    return Ok(warp::reply::with_status(
                        warp::reply::json(&ApiResponse::<()> {
                            success: false,
                            data: None,
                            error: Some("invalid_body".into()),
                        }),
                        warp::http::StatusCode::PAYLOAD_TOO_LARGE,
                    )
                    .into_response());
                }
                let parsed: serde_json::Value = match serde_json::from_slice(&body) {
                    Ok(v) => v,
                    Err(_) => {
                        return Ok(warp::reply::with_status(
                            warp::reply::json(&ApiResponse::<()> {
                                success: false,
                                data: None,
                                error: Some("invalid_body".into()),
                            }),
                            warp::http::StatusCode::BAD_REQUEST,
                        )
                        .into_response())
                    }
                };
                if parsed.get("type").and_then(|v| v.as_str()) != Some("transfer") {
                    return Ok(warp::reply::with_status(
                        warp::reply::json(&ApiResponse::<()> {
                            success: false,
                            data: None,
                            error: Some("invalid_body".into()),
                        }),
                        warp::http::StatusCode::BAD_REQUEST,
                    )
                    .into_response());
                }
                let req: TransferRequest = match serde_json::from_value(parsed.clone()) {
                    Ok(r) => r,
                    Err(_) => {
                        return Ok(warp::reply::with_status(
                            warp::reply::json(&ApiResponse::<()> {
                                success: false,
                                data: None,
                                error: Some("invalid_body".into()),
                            }),
                            warp::http::StatusCode::BAD_REQUEST,
                        )
                        .into_response())
                    }
                };
                if !ADDRESS_RE.is_match(&req.from) || !ADDRESS_RE.is_match(&req.to) {
                    return Ok(warp::reply::with_status(
                        warp::reply::json(&ApiResponse::<()> {
                            success: false,
                            data: None,
                            error: Some("invalid_address".into()),
                        }),
                        warp::http::StatusCode::BAD_REQUEST,
                    )
                    .into_response());
                }
                if req.amount == 0 || req.fee.unwrap_or(0) < MIN_FEE {
                    return Ok(warp::reply::with_status(
                        warp::reply::json(&ApiResponse::<()> {
                            success: false,
                            data: None,
                            error: Some("invalid_body".into()),
                        }),
                        warp::http::StatusCode::BAD_REQUEST,
                    )
                    .into_response());
                }
                // Balance & nonce check via storage
                let (storage, pool, ws_tx) = ctx;
                let sender_balance = storage.get_address_balance(&req.from).await.unwrap_or(0);
                let sender_nonce = storage.get_address_nonce(&req.from).await.unwrap_or(0);
                // Nonce rule
                let effective_nonce = match req.nonce {
                    None => sender_nonce,
                    Some(n) => {
                        if n != sender_nonce {
                            return Ok(warp::reply::with_status(
                                warp::reply::json(&ApiResponse::<()> {
                                    success: false,
                                    data: None,
                                    error: Some(format!(
                                        "invalid_nonce:expected:{sender_nonce}:got:{n}"
                                    )),
                                }),
                                warp::http::StatusCode::UNPROCESSABLE_ENTITY,
                            )
                            .into_response());
                        }
                        n
                    }
                };
                if sender_balance < req.amount + req.fee.unwrap_or(MIN_FEE) {
                    return Ok(warp::reply::with_status(
                        warp::reply::json(&ApiResponse::<()> {
                            success: false,
                            data: None,
                            error: Some("insufficient_balance".into()),
                        }),
                        warp::http::StatusCode::BAD_REQUEST,
                    )
                    .into_response());
                }
                // Build transaction
                let tx = crate::types::TransferTransaction::new(
                    req.from.clone(),
                    req.to.clone(),
                    req.amount,
                    req.fee.unwrap_or(MIN_FEE),
                    effective_nonce,
                );
                // Always enforce signature presence and validity
                let sig = match req.signature {
                    Some(s) => s,
                    None => {
                        return Ok::<_, warp::Rejection>(
                            warp::reply::with_status(
                                warp::reply::json(&ApiResponse::<()> {
                                    success: false,
                                    data: None,
                                    error: Some("signature_required".into()),
                                }),
                                warp::http::StatusCode::BAD_REQUEST,
                            )
                            .into_response(),
                        )
                    }
                };
                let sig_bytes = match hex::decode(sig.data) {
                    Ok(b) => b,
                    Err(_) => {
                        return Ok::<_, warp::Rejection>(
                            warp::reply::with_status(
                                warp::reply::json(&ApiResponse::<()> {
                                    success: false,
                                    data: None,
                                    error: Some("signature_invalid".into()),
                                }),
                                warp::http::StatusCode::BAD_REQUEST,
                            )
                            .into_response(),
                        )
                    }
                };
                let pk_bytes = match hex::decode(sig.public_key) {
                    Ok(b) => b,
                    Err(_) => {
                        return Ok(warp::reply::with_status(
                            warp::reply::json(&ApiResponse::<()> {
                                success: false,
                                data: None,
                                error: Some("signature_invalid".into()),
                            }),
                            warp::http::StatusCode::BAD_REQUEST,
                        )
                        .into_response())
                    }
                };
                let pqc = PQCManager::new().map_err(|_| ()).unwrap();
                let sig_wrapper = crate::crypto::PQCSignature {
                    signature: sig_bytes.clone(),
                    algorithm: sig.algorithm.clone(),
                    nonce: 0,
                    timestamp: 0,
                };
                match pqc.verify_signature(tx.signing_message().as_slice(), &sig_wrapper, &pk_bytes)
                {
                    Ok(valid) if valid => {}
                    _ => {
                        return Ok(warp::reply::with_status(
                            warp::reply::json(&ApiResponse::<()> {
                                success: false,
                                data: None,
                                error: Some("signature_invalid".into()),
                            }),
                            warp::http::StatusCode::BAD_REQUEST,
                        )
                        .into_response());
                    }
                }
                let hash = tx.hash.clone();
                // Add to mempool
                if let Err(e) = pool
                    .add_transaction(crate::types::Transaction::Transfer(tx.clone()))
                    .await
                {
                    return Ok(warp::reply::with_status(
                        warp::reply::json(&ApiResponse::<()> {
                            success: false,
                            data: None,
                            error: Some(match e.as_str() {
                                "Transaction already in pool" => "duplicate_tx".into(),
                                _ => "mempool_error".into(),
                            }),
                        }),
                        warp::http::StatusCode::CONFLICT,
                    )
                    .into_response());
                }
                if ws_tx.receiver_count() > 0 {
                    let _ = ws_tx.send(WebSocketMessage {
                        message_type: "new_transaction".into(),
                        timestamp: chrono::Utc::now().timestamp() as u64,
                        data: serde_json::json!({"hash": hash}),
                    });
                }
                // Persist pending transaction (optional to allow /tx lookup before inclusion)
                if let Err(e) = storage
                    .store_transaction(&crate::types::Transaction::Transfer(tx.clone()))
                    .await
                {
                    error!("store tx err: {e}");
                }
                Ok::<_, warp::Rejection>(
                    warp::reply::with_status(
                        warp::reply::json(&ApiResponse::success(
                            serde_json::json!({"hash": hash, "status":"pending"}),
                        )),
                        warp::http::StatusCode::OK,
                    )
                    .into_response(),
                )
            },
        )
        .boxed();

    // Blocks list (persistent)
    let storage_blocks = storage.clone();
    let blocks = {
        let base = warp::path("blocks").and(warp::get());
        // Accept optional raw query string; if absent supply empty string
        let with_query = base.and(
            warp::query::raw()
                .map(Some)
                .or(warp::any().map(|| None))
                .unify(),
        );
        with_query
            .and(warp::any().map(move || storage_blocks.clone()))
            .and_then(|query_opt: Option<String>, storage: Arc<crate::storage::StorageManager>| async move {
                let query = query_opt.unwrap_or_default();
                let qs: Vec<(String,String)> = url::form_urlencoded::parse(query.as_bytes()).into_owned().collect();
                let mut limit = 10usize; let mut from: Option<u64> = None;
                for (k,v) in qs { if k=="limit" { if let Ok(l)= v.parse::<usize>() { limit = l.min(100);} } else if k=="from" { if let Ok(h)= v.parse::<u64>() { from = Some(h); } } }
                match storage.list_blocks_desc(limit, from).await { Ok(list) => { let out: Vec<_> = list.into_iter().map(|b| serde_json::json!({"number": b.header.number, "hash": b.hash(), "parent_hash": b.header.parent_hash, "timestamp": b.header.timestamp, "tx_count": b.transactions.len()})).collect(); Ok::<_, warp::Rejection>(warp::reply::with_status(warp::reply::json(&ApiResponse::success(out)), warp::http::StatusCode::OK).into_response()) }, Err(e)=> { error!("blocks err: {e}"); Ok(warp::reply::with_status(warp::reply::json(&ApiResponse::<()> { success:false, data:None, error:Some("internal_error".into()) }), warp::http::StatusCode::INTERNAL_SERVER_ERROR).into_response()) } }
            })
            .map(|reply: warp::reply::Response| reply) // identity
            .boxed()
    };

    // Single block (by number or hash or latest)
    let storage_block = storage.clone();
    let get_block = warp::path("block")
        .and(warp::path::param::<String>())
        .and(warp::get())
        .and(warp::any().map(move || storage_block.clone()))
        .and_then(|id: String, storage: Arc<crate::storage::StorageManager>| async move {
            let res = if id == "latest" { let h = storage.get_height().unwrap_or(0); storage.get_block_by_height(h).await } else if id.starts_with("0x") { storage.get_block_by_hash(&id).await } else if let Ok(num) = id.parse::<u64>() { storage.get_block_by_height(num).await } else { Ok(None) };
            match res { Ok(Some(block)) => { let resp = serde_json::json!({"number": block.header.number, "hash": block.hash(), "parent_hash": block.header.parent_hash, "timestamp": block.header.timestamp, "transactions": block.transactions.iter().map(|t| t.hash()).collect::<Vec<_>>()}); Ok::<_, warp::Rejection>(warp::reply::with_status(warp::reply::json(&ApiResponse::success(resp)), warp::http::StatusCode::OK).into_response()) }, Ok(None)=> Ok(warp::reply::with_status(warp::reply::json(&ApiResponse::<()> { success:false, data:None, error:Some("not_found".into()) }), warp::http::StatusCode::NOT_FOUND).into_response()), Err(e)=> { error!("block err: {e}"); Ok(warp::reply::with_status(warp::reply::json(&ApiResponse::<()> { success:false, data:None, error:Some("internal_error".into()) }), warp::http::StatusCode::INTERNAL_SERVER_ERROR).into_response()) } }
        })
        .boxed();

    // Stats (augment with height & mempool size)
    let storage_stats = storage.clone();
    let tx_pool_stats = tx_pool.clone();
    let stats = warp::path("stats")
        .and(warp::get())
        .and(warp::any().map(move || (tx_pool_stats.clone(), storage_stats.clone())))
        .and_then(|ctx: (Arc<crate::types::TransactionPool>, Arc<crate::storage::StorageManager>)| async move {
            let (pool, storage) = ctx; let pool_stats = pool.get_stats().await; let height = storage.get_height().unwrap_or(0); Ok::<_, warp::Rejection>(warp::reply::with_status(warp::reply::json(&ApiResponse::success(serde_json::json!({"height": height, "mempool_size": pool_stats.total_transactions, "fee_levels": pool_stats.fee_levels, "max_mempool": pool_stats.max_size}))), warp::http::StatusCode::OK).into_response())
        })
        .boxed();

    // Status endpoint (for blockchain status checks)
    let storage_status = storage.clone();
    let tx_pool_status = tx_pool.clone();
    let status = warp::path("status")
        .and(warp::get())
        .and(warp::any().map(move || (tx_pool_status.clone(), storage_status.clone())))
        .and_then(
            |ctx: (
                Arc<crate::types::TransactionPool>,
                Arc<crate::storage::StorageManager>,
            )| async move {
                let (pool, storage) = ctx;
                let height = storage.get_height().unwrap_or(0);
                let pool_stats = pool.get_stats().await;
                Ok::<_, warp::Rejection>(
                    warp::reply::with_status(
                        warp::reply::json(&serde_json::json!({
                            "status": "ok",
                            "service": "dytallix-node",
                            "chain_id": "dyt-local-1",
                            "latest_height": height,
                            "height": height,
                            "block_height": height,
                            "mempool_size": pool_stats.total_transactions,
                            "timestamp": chrono::Utc::now().timestamp()
                        })),
                        warp::http::StatusCode::OK,
                    )
                    .into_response(),
                )
            },
        )
        .boxed();

    // Get transaction
    let storage_tx = storage.clone();
    let tx_pool_lookup = tx_pool.clone();
    let get_tx = warp::path("tx")
        .and(warp::path::param::<String>())
        .and(warp::get())
        .and(warp::any().map(move || (storage_tx.clone(), tx_pool_lookup.clone())))
        .and_then(|hash: String, ctx: (Arc<crate::storage::StorageManager>, Arc<crate::types::TransactionPool>)| async move {
            let (storage, _pool) = ctx; // renamed pool to _pool to silence unused variable warning
            if let Ok(Some(r)) = storage.get_receipt(&hash).await {
                return Ok::<_, warp::Rejection>(warp::reply::with_status(warp::reply::json(&serde_json::json!({
                    "tx_hash": r.tx_hash,
                    "status": match r.status { crate::types::TxStatus::Pending => "pending", crate::types::TxStatus::Success => "success", crate::types::TxStatus::Failed => "failed" },
                    "block_number": r.block_number,
                    "gas_used": r.gas_used.to_string(),
                    "fee_paid": r.fee_paid.to_string(),
                    "timestamp": r.timestamp,
                    "index": r.index,
                    "error": r.error
                })), warp::http::StatusCode::OK).into_response());
            }
            if let Ok(Some(_tx)) = storage.get_transaction_by_hash(&hash).await {
                return Ok(warp::reply::with_status(warp::reply::json(&serde_json::json!({"tx_hash": hash, "status": "pending"})), warp::http::StatusCode::OK).into_response());
            }
            Ok(warp::reply::with_status(warp::reply::json(&ErrorResponse { error: "NotFound".into(), message: "Transaction not found".into() }), warp::http::StatusCode::NOT_FOUND).into_response())
        })
        .boxed();

    // Initialize Dytallix Runtime for staking operations
    let _runtime = Arc::new(crate::runtime::DytallixRuntime::new(storage.clone())?); // unused runtime reserved for staking and future expansion

    // Contract RPC endpoint
    let rpc_runtime = _runtime.clone();
    let contract_rpc = warp::path("rpc")
        .and(warp::post())
        .and(warp::body::json())
        .and(warp::any().map(move || (storage.clone(), tx_pool.clone(), rpc_runtime.clone())))
        .and_then(
            |request: serde_json::Value,
             ctx: (
                Arc<crate::storage::StorageManager>,
                Arc<crate::types::TransactionPool>,
                Arc<crate::runtime::DytallixRuntime>,
            )| async move {
                if let Some(method) = request.get("method").and_then(|m| m.as_str()) {
                    let (storage, tx_pool, runtime) = ctx;
                    let result = match method {
                        // Contract methods
                        "contract_deploy" => {
                            if let Some(params) = request
                                .get("params")
                                .and_then(|p| p.as_array())
                                .and_then(|arr| arr.first())
                            {
                                handle_contract_deploy(
                                    params.clone(),
                                    (storage.clone(), tx_pool.clone()),
                                )
                                .await
                            } else {
                                serde_json::json!({"error": "Invalid parameters"})
                            }
                        }
                        "contract_instantiate" => {
                            if let Some(params) = request
                                .get("params")
                                .and_then(|p| p.as_array())
                                .and_then(|arr| arr.first())
                            {
                                handle_contract_instantiate(
                                    params.clone(),
                                    (storage.clone(), tx_pool.clone()),
                                )
                                .await
                            } else {
                                serde_json::json!({"error": "Invalid parameters"})
                            }
                        }
                        "contract_execute" => {
                            if let Some(params) = request
                                .get("params")
                                .and_then(|p| p.as_array())
                                .and_then(|arr| arr.first())
                            {
                                handle_contract_execute(
                                    params.clone(),
                                    (storage.clone(), tx_pool.clone()),
                                )
                                .await
                            } else {
                                serde_json::json!({"error": "Invalid parameters"})
                            }
                        }
                        "contract_get_code" => {
                            if let Some(params) = request
                                .get("params")
                                .and_then(|p| p.as_array())
                                .and_then(|arr| arr.first())
                            {
                                handle_contract_get_code(
                                    params.clone(),
                                    (storage.clone(), tx_pool.clone()),
                                )
                                .await
                            } else {
                                serde_json::json!({"error": "Invalid parameters"})
                            }
                        }
                        "contract_get_instance" => {
                            if let Some(params) = request
                                .get("params")
                                .and_then(|p| p.as_array())
                                .and_then(|arr| arr.first())
                            {
                                handle_contract_get_instance(
                                    params.clone(),
                                    (storage.clone(), tx_pool.clone()),
                                )
                                .await
                            } else {
                                serde_json::json!({"error": "Invalid parameters"})
                            }
                        }
                        "contract_get_storage" => {
                            if let Some(params) = request
                                .get("params")
                                .and_then(|p| p.as_array())
                                .and_then(|arr| arr.first())
                            {
                                handle_contract_get_storage(
                                    params.clone(),
                                    (storage.clone(), tx_pool.clone()),
                                )
                                .await
                            } else {
                                serde_json::json!({"error": "Invalid parameters"})
                            }
                        }
                        "contract_list" => {
                            handle_contract_list((storage.clone(), tx_pool.clone())).await
                        }

                        // WASM-specific endpoints (aliases for contract methods)
                        "wasm_deploy" => {
                            if let Some(params) = request
                                .get("params")
                                .and_then(|p| p.as_array())
                                .and_then(|arr| arr.first())
                            {
                                handle_wasm_deploy(
                                    params.clone(),
                                    (storage.clone(), tx_pool.clone()),
                                )
                                .await
                            } else {
                                serde_json::json!({"error": "Invalid parameters"})
                            }
                        }
                        "wasm_execute" => {
                            if let Some(params) = request
                                .get("params")
                                .and_then(|p| p.as_array())
                                .and_then(|arr| arr.first())
                            {
                                handle_wasm_execute(
                                    params.clone(),
                                    (storage.clone(), tx_pool.clone()),
                                )
                                .await
                            } else {
                                serde_json::json!({"error": "Invalid parameters"})
                            }
                        }

                        // Staking methods
                        "staking_register_validator" => {
                            if let Some(params) = request.get("params").and_then(|p| p.as_array()) {
                                if params.len() >= 3 {
                                    handle_staking_register_validator(params, runtime.clone()).await
                                } else {
                                    serde_json::json!({"error": "Invalid parameters"})
                                }
                            } else {
                                serde_json::json!({"error": "Invalid parameters"})
                            }
                        }
                        "staking_delegate" => {
                            if let Some(params) = request.get("params").and_then(|p| p.as_array()) {
                                if params.len() >= 3 {
                                    handle_staking_delegate(params, runtime.clone()).await
                                } else {
                                    serde_json::json!({"error": "Invalid parameters"})
                                }
                            } else {
                                serde_json::json!({"error": "Invalid parameters"})
                            }
                        }
                        "staking_claim_rewards" => {
                            if let Some(params) = request.get("params").and_then(|p| p.as_array()) {
                                if params.len() >= 2 {
                                    handle_staking_claim_rewards(params, runtime.clone()).await
                                } else {
                                    serde_json::json!({"error": "Invalid parameters"})
                                }
                            } else {
                                serde_json::json!({"error": "Invalid parameters"})
                            }
                        }
                        "staking_claim_all_rewards" => {
                            if let Some(params) = request.get("params").and_then(|p| p.as_array()) {
                                if !params.is_empty() {
                                    handle_staking_claim_all_rewards(params, runtime.clone()).await
                                } else {
                                    serde_json::json!({"error": "Invalid parameters"})
                                }
                            } else {
                                serde_json::json!({"error": "Invalid parameters"})
                            }
                        }
                        "staking_sync_accrued" => {
                            if let Some(params) = request.get("params").and_then(|p| p.as_array()) {
                                if params.len() >= 2 {
                                    handle_staking_sync_accrued(params, runtime.clone()).await
                                } else {
                                    serde_json::json!({"error": "Invalid parameters"})
                                }
                            } else {
                                serde_json::json!({"error": "Invalid parameters"})
                            }
                        }
                        "staking_get_accrued" => {
                            if let Some(params) = request.get("params").and_then(|p| p.as_array()) {
                                if params.len() >= 2 {
                                    handle_staking_get_accrued(params, runtime.clone()).await
                                } else {
                                    serde_json::json!({"error": "Invalid parameters"})
                                }
                            } else {
                                serde_json::json!({"error": "Invalid parameters"})
                            }
                        }
                        "staking_get_validator" => {
                            if let Some(params) = request.get("params").and_then(|p| p.as_array()) {
                                if !params.is_empty() {
                                    handle_staking_get_validator(params, runtime.clone()).await
                                } else {
                                    serde_json::json!({"error": "Invalid parameters"})
                                }
                            } else {
                                serde_json::json!({"error": "Invalid parameters"})
                            }
                        }
                        "staking_get_validators" => {
                            handle_staking_get_validators(runtime.clone()).await
                        }
                        "staking_get_delegations" => {
                            if let Some(params) = request.get("params").and_then(|p| p.as_array()) {
                                if !params.is_empty() {
                                    handle_staking_get_validators(runtime.clone()).await
                                } else {
                                    serde_json::json!({"error": "Invalid parameters"})
                                }
                            } else {
                                serde_json::json!({"error": "Invalid parameters"})
                            }
                        }
                        "staking_get_stats" => handle_staking_get_stats(runtime.clone()).await,

                        // Asset Registry methods
                        "asset_register" => {
                            if let Some(params) = request.get("params").and_then(|p| p.as_array()) {
                                if params.len() >= 2 {
                                    handle_asset_register(params, runtime.clone()).await
                                } else {
                                    serde_json::json!({"error": "Invalid parameters"})
                                }
                            } else {
                                serde_json::json!({"error": "Invalid parameters"})
                            }
                        }
                        "asset_verify" => {
                            if let Some(params) = request.get("params").and_then(|p| p.as_array()) {
                                if !params.is_empty() {
                                    handle_asset_verify(params, runtime.clone()).await
                                } else {
                                    serde_json::json!({"error": "Invalid parameters"})
                                }
                            } else {
                                serde_json::json!({"error": "Invalid parameters"})
                            }
                        }
                        "asset_get" => {
                            if let Some(params) = request.get("params").and_then(|p| p.as_array()) {
                                if !params.is_empty() {
                                    handle_asset_get(params, runtime.clone()).await
                                } else {
                                    serde_json::json!({"error": "Invalid parameters"})
                                }
                            } else {
                                serde_json::json!({"error": "Invalid parameters"})
                            }
                        }

                        // Default case for unknown methods
                        _ => {
                            serde_json::json!({"error": "Method not found"})
                        }
                    };

                    Ok::<_, warp::Rejection>(
                        warp::reply::json(&serde_json::json!({
                            "jsonrpc": "2.0",
                            "result": result,
                            "id": request.get("id").unwrap_or(&serde_json::json!(1))
                        }))
                        .into_response(),
                    )
                } else {
                    Ok(warp::reply::json(&serde_json::json!({
                        "jsonrpc": "2.0",
                        "error": {"code": -32600, "message": "Invalid Request"},
                        "id": request.get("id").unwrap_or(&serde_json::json!(1))
                    }))
                    .into_response())
                }
            },
        )
        .boxed();

    // Staking endpoints
    let staking_runtime = _runtime.clone();
    let staking_stats = warp::path!("staking" / "stats")
        .and(warp::get())
        .and(warp::any().map(move || staking_runtime.clone()))
        .and_then(|runtime: Arc<crate::runtime::DytallixRuntime>| async move {
            let (total_stake, total_validators, active_validators) =
                runtime.get_staking_stats().await;
            let response = StakingStatsResponse {
                total_stake,
                total_validators,
                active_validators,
            };
            Ok::<_, warp::Rejection>(
                warp::reply::with_status(
                    warp::reply::json(&ApiResponse::success(response)),
                    warp::http::StatusCode::OK,
                )
                .into_response(),
            )
        })
        .boxed();

    let staking_validators_runtime = _runtime.clone();
    let staking_validators = warp::path!("staking" / "validators")
        .and(warp::get())
        .and(warp::any().map(move || staking_validators_runtime.clone()))
        .and_then(|runtime: Arc<crate::runtime::DytallixRuntime>| async move {
            let validators = runtime.get_active_validators().await;
            let response: Vec<ValidatorResponse> = validators
                .iter()
                .map(|v| ValidatorResponse {
                    address: v.address.clone(),
                    total_stake: v.total_stake,
                    status: format!("{:?}", v.status),
                    commission_rate: v.commission_rate,
                    self_stake: v.self_stake,
                })
                .collect();
            Ok::<_, warp::Rejection>(
                warp::reply::with_status(
                    warp::reply::json(&ApiResponse::success(response)),
                    warp::http::StatusCode::OK,
                )
                .into_response(),
            )
        })
        .boxed();

    // GET /staking/rewards/{delegator} - comprehensive reward information
    let staking_rewards_runtime = _runtime.clone();
    let staking_rewards = warp::path!("staking" / "rewards" / String)
        .and(warp::get())
        .and(warp::any().map(move || staking_rewards_runtime.clone()))
        .and_then(
            |delegator: String, runtime: Arc<crate::runtime::DytallixRuntime>| async move {
                let summary = runtime.get_delegator_rewards_summary(&delegator).await;
                Ok::<_, warp::Rejection>(
                    warp::reply::with_status(
                        warp::reply::json(&summary),
                        warp::http::StatusCode::OK,
                    )
                    .into_response(),
                )
            },
        )
        .boxed();

    // POST /staking/claim - claim rewards
    let staking_claim_runtime = _runtime.clone();
    let staking_claim =
        warp::path!("staking" / "claim")
            .and(warp::post())
            .and(warp::body::json())
            .and(warp::any().map(move || staking_claim_runtime.clone()))
            .and_then(
                |claim_request: serde_json::Value,
                 runtime: Arc<crate::runtime::DytallixRuntime>| async move {
                    if let Some(delegator) = claim_request.get("delegator").and_then(|v| v.as_str())
                    {
                        let result = if let Some(validator) =
                            claim_request.get("validator").and_then(|v| v.as_str())
                        {
                            // Claim from specific validator
                            runtime
                                .claim_rewards(&delegator.to_string(), &validator.to_string())
                                .await
                        } else {
                            // Claim from all validators
                            runtime.claim_all_rewards(&delegator.to_string()).await
                        };

                        match result {
                            Ok(claimed) => {
                                let new_balance = runtime.get_drt_balance(delegator).await;
                                let response = serde_json::json!({
                                    "delegator": delegator,
                                    "claimed": claimed.to_string(),
                                    "new_balance": new_balance.to_string(),
                                    "height": runtime.get_current_height().await.unwrap_or(0)
                                });
                                Ok::<_, warp::Rejection>(
                                    warp::reply::with_status(
                                        warp::reply::json(&response),
                                        warp::http::StatusCode::OK,
                                    )
                                    .into_response(),
                                )
                            }
                            Err(e) => {
                                let response = serde_json::json!({
                                    "error": format!("Failed to claim rewards: {}", e)
                                });
                                Ok::<_, warp::Rejection>(
                                    warp::reply::with_status(
                                        warp::reply::json(&response),
                                        warp::http::StatusCode::BAD_REQUEST,
                                    )
                                    .into_response(),
                                )
                            }
                        }
                    } else {
                        let response = serde_json::json!({
                            "error": "Missing required field: delegator"
                        });
                        Ok::<_, warp::Rejection>(
                            warp::reply::with_status(
                                warp::reply::json(&response),
                                warp::http::StatusCode::BAD_REQUEST,
                            )
                            .into_response(),
                        )
                    }
                },
            )
            .boxed();

    // Asset Registry Routes
    let asset_register_runtime = _runtime.clone();
    let asset_register = warp::path!("asset" / "register")
        .and(warp::post())
        .and(warp::body::json())
        .and(warp::any().map(move || asset_register_runtime.clone()))
        .and_then(
            |request: serde_json::Value, runtime: Arc<crate::runtime::DytallixRuntime>| async move {
                if let Some(params) = request.get("params").and_then(|p| p.as_array()) {
                    if params.len() >= 2 {
                        let result = handle_asset_register(params, runtime.clone()).await;
                        Ok::<_, warp::Rejection>(warp::reply::json(&result).into_response())
                    } else {
                        let error = serde_json::json!({"error": "Invalid parameters"});
                        Ok::<_, warp::Rejection>(warp::reply::json(&error).into_response())
                    }
                } else {
                    let error = serde_json::json!({"error": "Invalid parameters"});
                    Ok::<_, warp::Rejection>(warp::reply::json(&error).into_response())
                }
            },
        )
        .boxed();

    // REST alias to match integration tests: POST /api/quantum/register
    // Accepts { assetHash|asset_hash, uri, metadata }
    let api_quantum_register_runtime = _runtime.clone();
    let api_quantum_register = warp::path!("api" / "quantum" / "register")
        .and(warp::post())
        .and(warp::body::json())
        .and(warp::any().map(move || api_quantum_register_runtime.clone()))
        .and_then(|request: serde_json::Value, runtime: Arc<crate::runtime::DytallixRuntime>| async move {
            let asset_hash = request
                .get("assetHash")
                .or_else(|| request.get("asset_hash"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let uri = request.get("uri").and_then(|v| v.as_str()).map(|s| s.to_string());
            let metadata = request.get("metadata").cloned().unwrap_or_else(|| serde_json::json!({}));

            match (asset_hash, uri) {
                (Some(asset_hash), Some(uri)) => {
                    let params = vec![serde_json::json!(asset_hash), serde_json::json!(uri), metadata];
                    let result = handle_asset_register(&params, runtime.clone()).await;
                    Ok::<_, warp::Rejection>(warp::reply::json(&result).into_response())
                }
                _ => {
                    let error = serde_json::json!({"error": "Missing required fields: assetHash/asset_hash and uri"});
                    Ok::<_, warp::Rejection>(warp::reply::with_status(
                        warp::reply::json(&error),
                        warp::http::StatusCode::BAD_REQUEST,
                    )
                    .into_response())
                }
            }
        })
        .boxed();

    let asset_verify_runtime = _runtime.clone();
    let asset_verify = warp::path!("asset" / "verify")
        .and(warp::post())
        .and(warp::body::json())
        .and(warp::any().map(move || asset_verify_runtime.clone()))
        .and_then(
            |request: serde_json::Value, runtime: Arc<crate::runtime::DytallixRuntime>| async move {
                if let Some(params) = request.get("params").and_then(|p| p.as_array()) {
                    if !params.is_empty() {
                        let result = handle_asset_verify(params, runtime.clone()).await;
                        Ok::<_, warp::Rejection>(warp::reply::json(&result).into_response())
                    } else {
                        let error = serde_json::json!({"error": "Invalid parameters"});
                        Ok::<_, warp::Rejection>(warp::reply::json(&error).into_response())
                    }
                } else {
                    let error = serde_json::json!({"error": "Invalid parameters"});
                    Ok::<_, warp::Rejection>(warp::reply::json(&error).into_response())
                }
            },
        )
        .boxed();

    let asset_get_runtime = _runtime.clone();
    let asset_get = warp::path!("asset" / "get")
        .and(warp::post())
        .and(warp::body::json())
        .and(warp::any().map(move || asset_get_runtime.clone()))
        .and_then(
            |request: serde_json::Value, runtime: Arc<crate::runtime::DytallixRuntime>| async move {
                if let Some(params) = request.get("params").and_then(|p| p.as_array()) {
                    if !params.is_empty() {
                        let result = handle_asset_get(params, runtime.clone()).await;
                        Ok::<_, warp::Rejection>(warp::reply::json(&result).into_response())
                    } else {
                        let error = serde_json::json!({"error": "Invalid parameters"});
                        Ok::<_, warp::Rejection>(warp::reply::json(&error).into_response())
                    }
                } else {
                    let error = serde_json::json!({"error": "Invalid parameters"});
                    Ok::<_, warp::Rejection>(warp::reply::json(&error).into_response())
                }
            },
        )
        .boxed();

    // CORS
    let cors = {
        let origin = std::env::var("FRONTEND_ORIGIN").ok();
        let mut c = warp::cors()
            .allow_headers(vec!["content-type"])
            .allow_methods(vec!["GET", "POST", "OPTIONS"]);
        if let Some(o) = origin {
            c = c.allow_origin(o.as_str());
        } else {
            c = c.allow_any_origin();
        }
        c
    };

    let json_routes = health
        .or(balance)
        .or(submit_tx)
        .or(get_tx)
        .or(blocks)
        .or(get_block)
        .or(peers)
        .or(stats)
        .or(status)
        .or(staking_stats)
        .or(staking_validators)
        .or(staking_rewards)
        .or(staking_claim)
        .or(contract_rpc)
        .or(asset_register)
        .or(api_quantum_register)
        .or(asset_verify)
        .or(asset_get)
        .with(cors)
        .with(warp::log("api"));

    let routes = json_routes.or(websocket);

    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "3003".to_string())
        .parse::<u16>()
        .unwrap_or(3003);

    info!(
        "API server listening on 0.0.0.0:{} (block production: enabled)",
        port
    );
    warp::serve(routes).run(([0, 0, 0, 0], port)).await;
    Ok(())
}

async fn handle_websocket(
    websocket: warp::ws::WebSocket,
    ws_tx: Arc<broadcast::Sender<WebSocketMessage>>,
) {
    info!("New WebSocket connection established");

    let (mut ws_sink, mut ws_stream) = websocket.split();
    let mut ws_rx = ws_tx.subscribe();

    // Handle incoming messages from client
    let ws_tx_clone = ws_tx.clone();
    let incoming_task = tokio::spawn(async move {
        while let Some(result) = ws_stream.next().await {
            match result {
                Ok(msg) => {
                    if msg.is_text() {
                        if let Ok(text) = msg.to_str() {
                            info!("Received WebSocket message: {text}");

                            // Handle subscription requests
                            if text.contains("subscribe") {
                                let response = WebSocketMessage {
                                    message_type: "subscription_confirmed".to_string(),
                                    timestamp: chrono::Utc::now().timestamp() as u64,
                                    data: serde_json::json!({"status": "subscribed"}),
                                };
                                let _ = ws_tx_clone.send(response);
                            }
                        }
                    } else if msg.is_close() {
                        info!("WebSocket connection closed by client");
                        break;
                    }
                }
                Err(e) => {
                    error!("WebSocket error: {e}");
                    break;
                }
            }
        }
    });

    // Handle outgoing messages to client
    let outgoing_task = tokio::spawn(async move {
        while let Ok(message) = ws_rx.recv().await {
            let json_msg = serde_json::to_string(&message).unwrap_or_default();
            if let Err(e) = ws_sink.send(warp::ws::Message::text(json_msg)).await {
                error!("Failed to send WebSocket message: {e}");
                break;
            }
        }
    });

    // Wait for either task to complete
    tokio::select! {
        _ = incoming_task => info!("WebSocket incoming task completed"),
        _ = outgoing_task => info!("WebSocket outgoing task completed"),
    }

    info!("WebSocket connection closed");
}

// Contract RPC handler functions
async fn handle_contract_deploy(
    params: serde_json::Value,
    ctx: (
        Arc<crate::storage::StorageManager>,
        Arc<crate::types::TransactionPool>,
    ),
) -> serde_json::Value {
    let (_storage, _tx_pool) = ctx;

    // Extract deployment parameters
    if let Some(code_hex) = params.get("code").and_then(|c| c.as_str()) {
        if let Ok(code) = hex::decode(code_hex) {
            // Validate WASM code
            if code.len() < 8 || &code[0..4] != b"\x00asm" {
                return serde_json::json!({"error": "Invalid WASM code"});
            }

            // Create runtime and deploy contract
            if let Ok(runtime) = ContractRuntime::new(1_000_000, 16) {
                let deployment = ContractDeployment {
                    code: code.clone(),
                    metadata: serde_json::json!({}),
                    deployer: params
                        .get("from")
                        .and_then(|f| f.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    gas_limit: params
                        .get("gas_limit")
                        .and_then(|g| g.as_u64())
                        .unwrap_or(100_000),
                    address: generate_contract_address(blake3::hash(&code).as_bytes()),
                    initial_state: params
                        .get("initial_state")
                        .and_then(|s| serde_json::to_vec(s).ok())
                        .unwrap_or_default(),
                    timestamp: chrono::Utc::now().timestamp() as u64,
                    ai_audit_score: None,
                };

                match runtime.deploy_contract(deployment).await {
                    Ok(address) => {
                        let code_hash = blake3::hash(&code);
                        return serde_json::json!({
                            "success": true,
                            "address": address,
                            "code_hash": hex::encode(code_hash.as_bytes()),
                            "gas_used": 50000 // TODO: Get actual gas used
                        });
                    }
                    Err(e) => {
                        return serde_json::json!({
                            "error": format!("Deployment failed: {}", e)
                        });
                    }
                }
            } else {
                return serde_json::json!({"error": "Failed to create contract runtime"});
            }
        }
    }

    serde_json::json!({"error": "Invalid deployment parameters"})
}

async fn handle_contract_instantiate(
    params: serde_json::Value,
    ctx: (
        Arc<crate::storage::StorageManager>,
        Arc<crate::types::TransactionPool>,
    ),
) -> serde_json::Value {
    let (_storage, _tx_pool) = ctx;

    // For MVP, we don't distinguish between deployment and instantiation
    // In a full implementation, this would create instances from deployed code
    if let Some(code_hash) = params.get("code_hash").and_then(|h| h.as_str()) {
        let instance_address = generate_instance_address(code_hash);

        return serde_json::json!({
            "success": true,
            "instance_address": instance_address,
            "gas_used": 30000
        });
    }

    serde_json::json!({"error": "Invalid instantiation parameters"})
}

async fn handle_contract_execute(
    params: serde_json::Value,
    ctx: (
        Arc<crate::storage::StorageManager>,
        Arc<crate::types::TransactionPool>,
    ),
) -> serde_json::Value {
    // unify
    let (_storage, _tx_pool) = ctx;

    if let Some(contract_address) = params.get("contract_address").and_then(|a| a.as_str()) {
        if let Ok(runtime) = ContractRuntime::new(1_000_000, 16) {
            // Extract args raw JSON for both args/args_json fields
            let args_json = params.get("args").cloned().unwrap_or(serde_json::json!({}));
            let method_name = params
                .get("function")
                .and_then(|f| f.as_str())
                .unwrap_or("execute")
                .to_string();
            let call = ContractCall {
                // include all required fields from wrapper struct
                contract_id: contract_address.to_string(), // use address as id for now
                function: method_name.clone(),
                args: args_json.clone(),
                contract_address: contract_address.to_string(),
                method: method_name,
                caller: params
                    .get("from")
                    .and_then(|f| f.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                input_data: serde_json::to_vec(&args_json).unwrap_or_default(),
                gas_limit: params
                    .get("gas_limit")
                    .and_then(|g| g.as_u64())
                    .unwrap_or(100_000),
                value: params.get("value").and_then(|v| v.as_u64()).unwrap_or(0),
                timestamp: chrono::Utc::now().timestamp() as u64,
            };

            return match runtime.call_contract(call).await {
                Ok(result) => serde_json::json!({
                    "success": result.success,
                    "return_value": hex::encode(&result.return_data), // updated field
                    "gas_used": result.gas_used,
                    "events": result.events
                }),
                Err(e) => serde_json::json!({
                    "error": format!("Execution failed: {}", e)
                }),
            };
        }
    }

    serde_json::json!({"error": "Invalid execution parameters"})
}

async fn handle_contract_get_code(
    params: serde_json::Value,
    ctx: (
        Arc<crate::storage::StorageManager>,
        Arc<crate::types::TransactionPool>,
    ),
) -> serde_json::Value {
    let (_storage, _tx_pool) = ctx;

    if let Some(hash) = params.get("hash").and_then(|h| h.as_str()) {
        // TODO: Implement proper code storage and retrieval
        return serde_json::json!({
            "hash": hash,
            "size": 1024, // Mock size
            "deployed_at": chrono::Utc::now().timestamp()
        });
    }

    serde_json::json!({"error": "Code not found"})
}

async fn handle_contract_get_instance(
    params: serde_json::Value,
    ctx: (
        Arc<crate::storage::StorageManager>,
        Arc<crate::types::TransactionPool>,
    ),
) -> serde_json::Value {
    // unify return type
    let (_storage, _tx_pool) = ctx;

    if let Some(address) = params.get("address").and_then(|a| a.as_str()) {
        if let Ok(runtime) = ContractRuntime::new(1_000_000, 16) {
            let addr = address.to_string();
            if let Some(info) = runtime.get_contract_info(&addr) {
                return serde_json::json!({
                    "address": info.address,
                    "deployer": info.deployer,
                    "gas_limit": info.gas_limit,
                    "deployed_at": info.timestamp
                });
            }
        }

        return serde_json::json!({"error": "Instance not found"});
    }

    serde_json::json!({"error": "Invalid address parameter"})
}

async fn handle_contract_get_storage(
    params: serde_json::Value,
    ctx: (
        Arc<crate::storage::StorageManager>,
        Arc<crate::types::TransactionPool>,
    ),
) -> serde_json::Value {
    // fixed return type path
    let (_storage, _tx_pool) = ctx;

    if let Some(contract_address) = params.get("contract_address").and_then(|a| a.as_str()) {
        if let Some(key_hex) = params.get("key").and_then(|k| k.as_str()) {
            if let Ok(key) = hex::decode(key_hex) {
                if let Ok(runtime) = ContractRuntime::new(1_000_000, 16) {
                    let addr = contract_address.to_string();
                    if let Some(value) = runtime.get_contract_storage(&addr, &key) {
                        // corrected method
                        return serde_json::json!({
                            "value": hex::encode(&value)
                        });
                    }
                }
                return serde_json::json!({"error": "Storage key not found"});
            }
        }
    }

    serde_json::json!({"error": "Invalid storage parameters"})
}

async fn handle_contract_list(
    ctx: (
        Arc<crate::storage::StorageManager>,
        Arc<crate::types::TransactionPool>,
    ),
) -> serde_json::Value {
    let (_storage, _tx_pool) = ctx;

    if let Ok(runtime) = ContractRuntime::new(1_000_000, 16) {
        let contracts = runtime.list_contracts();
        let contract_list: Vec<serde_json::Value> = contracts
            .iter()
            .filter_map(|addr| {
                runtime.get_contract_info(addr).map(|info| {
                    serde_json::json!({
                        "address": info.address,
                        "deployer": info.deployer,
                        "gas_limit": info.gas_limit,
                        "timestamp": info.timestamp
                    })
                })
            })
            .collect();

        return serde_json::json!(contract_list);
    }

    serde_json::json!([])
}

// WASM-specific handlers (aliases for contract handlers with different parameter formats)
async fn handle_wasm_deploy(
    params: serde_json::Value,
    ctx: (
        Arc<crate::storage::StorageManager>,
        Arc<crate::types::TransactionPool>,
    ),
) -> serde_json::Value {
    // unify
    let (_storage, _tx_pool) = ctx;
    if let Some(code_base64) = params.get("code_base64").and_then(|c| c.as_str()) {
        if let Ok(code) = base64::engine::general_purpose::STANDARD.decode(code_base64) {
            if code.len() < 8 || &code[0..4] != b"\x00asm" {
                return serde_json::json!({"error": "Invalid WASM code"});
            }
            if let Ok(runtime) = ContractRuntime::new(1_000_000, 16) {
                let deployment = ContractDeployment {
                    code: code.clone(),
                    metadata: serde_json::json!({}),
                    deployer: "wasm_deployer".to_string(),
                    gas_limit: params
                        .get("gas_limit")
                        .and_then(|g| g.as_u64())
                        .unwrap_or(500_000),
                    address: generate_contract_address(blake3::hash(&code).as_bytes()),
                    initial_state: vec![],
                    timestamp: chrono::Utc::now().timestamp() as u64,
                    ai_audit_score: None,
                };
                match runtime.deploy_contract(deployment).await {
                    Ok(address) => {
                        let code_hash = blake3::hash(&code);
                        return serde_json::json!({
                            "address": address,
                            "code_hash": hex::encode(code_hash.as_bytes()),
                            "gas_used": 50000
                        });
                    }
                    Err(e) => {
                        return serde_json::json!({"error": format!("Deployment failed: {}", e)});
                    }
                }
            } else {
                return serde_json::json!({"error": "Failed to create contract runtime"});
            }
        } else {
            return serde_json::json!({"error": "Failed to decode base64 WASM code"});
        }
    }
    serde_json::json!({"error": "Invalid WASM deployment parameters"})
}

async fn handle_wasm_execute(
    params: serde_json::Value,
    ctx: (
        Arc<crate::storage::StorageManager>,
        Arc<crate::types::TransactionPool>,
    ),
) -> serde_json::Value {
    // unify
    let (_storage, _tx_pool) = ctx;

    if let (Some(address), Some(method)) = (
        params.get("address").and_then(|a| a.as_str()),
        params.get("method").and_then(|m| m.as_str()),
    ) {
        if let Ok(runtime) = ContractRuntime::new(1_000_000, 16) {
            let args_json = params
                .get("args_json")
                .cloned()
                .unwrap_or(serde_json::json!({}));
            let method_name = method.to_string();
            let call = ContractCall {
                contract_id: address.to_string(),
                function: method_name.clone(),
                args: args_json.clone(),
                contract_address: address.to_string(),
                method: method_name,
                caller: "wasm_caller".to_string(),
                input_data: serde_json::to_vec(&args_json).unwrap_or_default(),
                gas_limit: params
                    .get("gas_limit")
                    .and_then(|g| g.as_u64())
                    .unwrap_or(20_000),
                value: 0,
                timestamp: chrono::Utc::now().timestamp() as u64,
            };

            return match runtime.call_contract(call).await {
                Ok(result) => serde_json::json!({
                    "result_json": result.return_data,
                    "gas_used": result.gas_used,
                    "height": 1
                }),
                Err(e) => serde_json::json!({"error": format!("Execution failed: {}", e)}),
            };
        } else {
            return serde_json::json!({"error": "Failed to create contract runtime"});
        }
    }

    serde_json::json!({"error": "Invalid WASM execution parameters"})
}

// Helper functions for address generation
fn generate_contract_address(code_hash: &[u8]) -> String {
    let hash = blake3::hash(code_hash);
    format!("contract_{}", hex::encode(&hash.as_bytes()[..16]))
}

fn generate_instance_address(code_hash: &str) -> String {
    let timestamp = chrono::Utc::now().timestamp();
    let input = format!("{code_hash}_{timestamp}");
    let hash = blake3::hash(input.as_bytes());
    format!("instance_{}", hex::encode(&hash.as_bytes()[..16]))
}

// Staking RPC handler functions
async fn handle_staking_register_validator(
    params: &[serde_json::Value],
    runtime: Arc<crate::runtime::DytallixRuntime>,
) -> serde_json::Value {
    if let (Some(address), Some(pubkey), Some(commission)) =
        (params[0].as_str(), params[1].as_str(), params[2].as_u64())
    {
        if let Ok(pubkey_bytes) = hex::decode(pubkey) {
            match runtime
                .register_validator(address.to_string(), pubkey_bytes, commission as u16)
                .await
            {
                Ok(_) => serde_json::json!({"success": true}),
                Err(e) => serde_json::json!({"error": format!("Registration failed: {}", e)}),
            }
        } else {
            serde_json::json!({"error": "Invalid public key format"})
        }
    } else {
        serde_json::json!({"error": "Invalid parameters"})
    }
}

async fn handle_staking_delegate(
    params: &[serde_json::Value],
    runtime: Arc<crate::runtime::DytallixRuntime>,
) -> serde_json::Value {
    if let (Some(delegator), Some(validator), Some(amount)) = (
        params[0].as_str(),
        params[1].as_str(),
        params[2]
            .as_u64()
            .or_else(|| params[2].as_str().and_then(|s| s.parse().ok())),
    ) {
        match runtime
            .delegate(delegator.to_string(), validator.to_string(), amount as u128)
            .await
        {
            Ok(_) => serde_json::json!({"success": true}),
            Err(e) => serde_json::json!({"error": format!("Delegation failed: {}", e)}),
        }
    } else {
        serde_json::json!({"error": "Invalid parameters"})
    }
}

async fn handle_staking_claim_rewards(
    params: &[serde_json::Value],
    runtime: Arc<crate::runtime::DytallixRuntime>,
) -> serde_json::Value {
    if let (Some(delegator), Some(validator)) = (params[0].as_str(), params[1].as_str()) {
        match runtime
            .claim_rewards(&delegator.to_string(), &validator.to_string())
            .await
        {
            Ok(rewards) => serde_json::json!(rewards),
            Err(e) => serde_json::json!({"error": format!("Failed to claim rewards: {}", e)}),
        }
    } else {
        serde_json::json!({"error": "Invalid parameters"})
    }
}

async fn handle_staking_claim_all_rewards(
    params: &[serde_json::Value],
    runtime: Arc<crate::runtime::DytallixRuntime>,
) -> serde_json::Value {
    if let Some(delegator) = params[0].as_str() {
        match runtime.claim_all_rewards(&delegator.to_string()).await {
            Ok(total_claimed) => serde_json::json!({"total_claimed": total_claimed}),
            Err(e) => serde_json::json!({"error": format!("Failed to claim all rewards: {}", e)}),
        }
    } else {
        serde_json::json!({"error": "Invalid parameters"})
    }
}

async fn handle_staking_sync_accrued(
    params: &[serde_json::Value],
    runtime: Arc<crate::runtime::DytallixRuntime>,
) -> serde_json::Value {
    if let (Some(delegator), Some(validator)) = (params[0].as_str(), params[1].as_str()) {
        match runtime
            .sync_and_get_accrued_rewards(&delegator.to_string(), &validator.to_string())
            .await
        {
            Ok(accrued) => serde_json::json!({"accrued": accrued}),
            Err(e) => {
                serde_json::json!({"error": format!("Failed to sync accrued rewards: {}", e)})
            }
        }
    } else {
        serde_json::json!({"error": "Invalid parameters"})
    }
}

async fn handle_staking_get_accrued(
    params: &[serde_json::Value],
    runtime: Arc<crate::runtime::DytallixRuntime>,
) -> serde_json::Value {
    if let (Some(delegator), Some(validator)) = (params[0].as_str(), params[1].as_str()) {
        match runtime
            .get_accrued_rewards(&delegator.to_string(), &validator.to_string())
            .await
        {
            Ok(accrued) => serde_json::json!({"accrued": accrued}),
            Err(e) => serde_json::json!({"error": format!("Failed to get accrued rewards: {}", e)}),
        }
    } else {
        serde_json::json!({"error": "Invalid parameters"})
    }
}

async fn handle_staking_get_validator(
    params: &[serde_json::Value],
    runtime: Arc<crate::runtime::DytallixRuntime>,
) -> serde_json::Value {
    if let Some(address) = params[0].as_str() {
        match runtime.get_validator(&address.to_string()).await {
            Some(validator) => serde_json::json!({
                "address": validator.address,
                "total_stake": validator.total_stake.to_string(),
                "status": format!("{:?}", validator.status),
                "commission_rate": validator.commission_rate,
                "self_stake": validator.self_stake.to_string(),
                "missed_blocks": validator.missed_blocks,
                "slash_count": validator.slash_count,
                "total_slashed": validator.total_slashed.to_string(),
            }),
            None => serde_json::json!(null),
        }
    } else {
        serde_json::json!({"error": "Invalid address parameter"})
    }
}

async fn handle_staking_get_validators(
    _runtime: Arc<crate::runtime::DytallixRuntime>,
) -> serde_json::Value {
    let validators = _runtime.get_active_validators().await;
    let result: Vec<serde_json::Value> = validators
        .iter()
        .map(|v| {
            serde_json::json!({
                "address": v.address,
                "total_stake": v.total_stake.to_string(),
                "status": format!("{:?}", v.status),
                "commission_rate": v.commission_rate,
                "self_stake": v.self_stake.to_string(),
            })
        })
        .collect();
    serde_json::json!(result)
}

async fn handle_staking_get_stats(
    runtime: Arc<crate::runtime::DytallixRuntime>,
) -> serde_json::Value {
    let (total_stake, total_validators, active_validators) = runtime.get_staking_stats().await;
    serde_json::json!({
        "total_stake": total_stake.to_string(),
        "total_validators": total_validators,
        "active_validators": active_validators,
    })
}

// Asset registry request and response types
#[derive(Debug, Deserialize)]
struct AssetRegistryRequest {
    asset_hash: String,
    uri: String,
    metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct AssetRegistryResponse {
    success: bool,
    asset_id: String,
    tx_hash: String,
    block_height: Option<u64>,
    timestamp: u64,
}

#[derive(Debug, Serialize)]
struct AssetVerificationResponse {
    verified: bool,
    asset_id: Option<String>,
    tx_hash: Option<String>,
    block_height: Option<u64>,
    timestamp: Option<u64>,
    metadata: Option<serde_json::Value>,
}

// Asset Registry handler functions
async fn handle_asset_register(
    params: &[serde_json::Value],
    runtime: Arc<crate::runtime::DytallixRuntime>,
) -> serde_json::Value {
    if params.len() < 2 {
        return serde_json::json!({"error": "Invalid parameters - expected [asset_hash, metadata] or [asset_hash, uri, metadata]"});
    }

    let asset_hash = match params[0].as_str() {
        Some(h) => h,
        None => return serde_json::json!({"error": "Invalid asset_hash parameter"}),
    };

    // Support both legacy and current forms:
    // - [asset_hash, metadata] (metadata can be JSON string or JSON value)
    // - [asset_hash, uri, metadata]
    let (uri, metadata) = if params.len() >= 3 {
        let uri = match params[1].as_str() {
            Some(u) => u.to_string(),
            None => "".to_string(),
        };

        let metadata_value = match &params[2] {
            serde_json::Value::String(s) => serde_json::from_str::<serde_json::Value>(s)
                .unwrap_or_else(|_| serde_json::json!({})),
            other => other.clone(),
        };

        (Some(uri), metadata_value)
    } else {
        match &params[1] {
            serde_json::Value::String(s) => {
                // If it isn't JSON, treat it as a URI and use empty metadata.
                match serde_json::from_str::<serde_json::Value>(s) {
                    Ok(m) => (None, m),
                    Err(_) => (Some(s.to_string()), serde_json::json!({})),
                }
            }
            other => (None, other.clone()),
        }
    };

    info!(
        "[AssetRegistry] Registering asset: {} uri={:?} metadata={}",
        asset_hash, uri, metadata
    );

    // Generate a unique asset ID based on the hash (handle short hashes)
    let hash_len = asset_hash.len().min(16);
    let asset_id = format!("asset_{}", &asset_hash[..hash_len]);

    // Get current blockchain state
    let block_height = runtime.get_current_height().await.unwrap_or(0);
    let timestamp = chrono::Utc::now().timestamp() as u64;

    // Create the asset record to store on-chain
    let asset_record = serde_json::json!({
        "asset_id": asset_id,
        "asset_hash": asset_hash,
        "uri": uri,
        "metadata": metadata,
        "block_height": block_height,
        "timestamp": timestamp,
        "registered_at": chrono::Utc::now().to_rfc3339()
    });

    // Store the asset record in the runtime's state
    // This creates a permanent on-chain record
    let storage_key = format!("asset:{}", asset_hash);
    if let Err(e) = runtime
        .store_data(&storage_key, &asset_record.to_string())
        .await
    {
        error!("[AssetRegistry] Failed to store asset on-chain: {}", e);
        return serde_json::json!({"error": format!("Failed to store asset: {}", e)});
    }

    // Generate transaction hash for the registration
    let tx_content = format!("{}:{:?}:{}:{}", asset_hash, uri, metadata, timestamp);
    let tx_hash = format!("0x{}", blake3::hash(tx_content.as_bytes()).to_hex());

    info!(
        "[AssetRegistry] ✅ Asset registered on-chain: {} at block {}, tx: {}",
        asset_id, block_height, tx_hash
    );

    serde_json::json!({
        "success": true,
        "asset_id": asset_id,
        "tx_hash": tx_hash,
        "block_height": block_height,
        "timestamp": timestamp,
        "metadata": metadata,
        "note": "Asset successfully anchored to Dytallix blockchain"
    })
}

async fn handle_asset_verify(
    params: &[serde_json::Value],
    runtime: Arc<crate::runtime::DytallixRuntime>,
) -> serde_json::Value {
    if params.is_empty() {
        return serde_json::json!({"error": "Invalid parameters - expected asset_hash"});
    }

    let asset_hash = match params[0].as_str() {
        Some(h) => h,
        None => return serde_json::json!({"error": "Invalid asset_hash parameter"}),
    };

    info!("[AssetRegistry] Verifying asset: {}", asset_hash);

    // Look up the asset in storage
    let storage_key = format!("asset:{}", asset_hash);
    match runtime.get_data(&storage_key).await {
        Ok(Some(data)) => {
            // Asset found on-chain
            match serde_json::from_str::<serde_json::Value>(&data) {
                Ok(asset_record) => {
                    info!("[AssetRegistry] ✅ Asset verified on-chain: {}", asset_hash);
                    serde_json::json!({
                        "verified": true,
                        "asset_id": asset_record["asset_id"],
                        "asset_hash": asset_hash,
                        "tx_hash": format!("0x{}", blake3::hash(format!("verify_{}", asset_hash).as_bytes()).to_hex()),
                        "block_height": asset_record["block_height"],
                        "timestamp": asset_record["timestamp"],
                        "metadata": asset_record["metadata"].clone(),
                        "registered_at": asset_record["registered_at"],
                        "note": "Asset verified on Dytallix blockchain"
                    })
                }
                Err(e) => {
                    error!("[AssetRegistry] Failed to parse stored asset data: {}", e);
                    serde_json::json!({"error": "Stored asset data is corrupted"})
                }
            }
        }
        Ok(None) => {
            info!("[AssetRegistry] Asset not found: {}", asset_hash);
            serde_json::json!({
                "verified": false,
                "error": "Asset not found on blockchain",
                "asset_hash": asset_hash
            })
        }
        Err(e) => {
            error!("[AssetRegistry] Storage query failed: {}", e);
            serde_json::json!({"error": format!("Storage query failed: {}", e)})
        }
    }
}

async fn handle_asset_get(
    params: &[serde_json::Value],
    _runtime: Arc<crate::runtime::DytallixRuntime>,
) -> serde_json::Value {
    if let Some(asset_id) = params[0].as_str() {
        // In a real implementation, this would query the blockchain for asset details
        // For now, we'll return mock data
        serde_json::json!({
            "asset_id": asset_id,
            "asset_hash": format!("{}{}", asset_id.replace("asset_", ""), "0".repeat(48)),
            "uri": format!("dytallix://{}.enc", asset_id),
            "metadata": {
                "filename": "example.txt",
                "mime": "text/plain",
                "size": 1024
            },
            "tx_hash": format!("0x{}", blake3::hash(format!("tx_{}", asset_id).as_bytes()).to_hex()),
            "block_height": 123456,
            "timestamp": chrono::Utc::now().timestamp(),
            "status": "registered"
        })
    } else {
        serde_json::json!({"error": "Invalid parameters - expected asset_id"})
    }
}
