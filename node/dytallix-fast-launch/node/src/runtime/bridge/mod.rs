use crate::rpc::{errors::ApiError, RpcContext};
use crate::storage::bridge::{verify_bridge_message, BridgeMessage, BridgeStore, BridgeValidator};
use axum::{Extension, Json};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
pub struct IngestBridgeMessage {
    pub id: String,
    pub source_chain: String,
    pub dest_chain: String,
    pub asset: String,
    pub amount: String, // accept as string -> parse u128
    pub recipient: String,
    pub signatures: Vec<String>,
    pub signers: Vec<String>,
}

#[axum::debug_handler]
pub async fn ingest(
    Extension(ctx): Extension<RpcContext>,
    Json(body): Json<IngestBridgeMessage>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = BridgeStore {
        db: &ctx.storage.db,
    };
    if store.is_halted() {
        return Err(ApiError::Internal);
    }
    if store.has_message(&body.id) {
        return Ok(Json(json!({"status":"duplicate"})));
    }
    let amount: u128 = body.amount.parse().map_err(|_| ApiError::Internal)?;
    let msg = BridgeMessage {
        id: body.id.clone(),
        source_chain: body.source_chain,
        dest_chain: body.dest_chain,
        asset: body.asset,
        amount,
        recipient: body.recipient,
        signatures: body.signatures,
        signers: body.signers,
    };
    let validators = store.get_validators();
    match verify_bridge_message(&msg, &validators) {
        Ok(_) => {
            store.put_pending(&msg).map_err(|_| ApiError::Internal)?;
            store
                .add_custody(&msg.asset, msg.amount)
                .map_err(|_| ApiError::Internal)?;
            store
                .mark_applied(&msg.id)
                .map_err(|_| ApiError::Internal)?; // immediate finalize for now
            Ok(Json(json!({"status":"accepted","id": msg.id })))
        }
        Err(e) => {
            eprintln!("bridge_ingest_reject id={} reason={}", msg.id, e);
            Err(ApiError::Internal)
        }
    }
}

#[derive(Deserialize)]
pub struct BridgeHaltToggle {
    pub action: String,
}

#[axum::debug_handler]
pub async fn halt_toggle(
    Extension(ctx): Extension<RpcContext>,
    Json(body): Json<BridgeHaltToggle>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = BridgeStore {
        db: &ctx.storage.db,
    };
    match body.action.as_str() {
        "halt" => {
            store.set_halted(true).map_err(|_| ApiError::Internal)?;
            Ok(Json(json!({"halted":true})))
        }
        "resume" => {
            store.set_halted(false).map_err(|_| ApiError::Internal)?;
            Ok(Json(json!({"halted":false})))
        }
        _ => Err(ApiError::Internal),
    }
}

pub async fn bridge_state(
    Extension(ctx): Extension<RpcContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = BridgeStore {
        db: &ctx.storage.db,
    };
    let st = store.build_debug_state();
    Ok(Json(
        serde_json::to_value(st).map_err(|_| ApiError::Internal)?,
    ))
}

// Initialization helper (called from main or tests) to set validators if empty
pub fn ensure_bridge_validators(db: &rocksdb::DB) -> anyhow::Result<()> {
    let store = BridgeStore { db };
    if store.get_validators().is_empty() {
        // Load from env BRIDGE_VALIDATORS json array of {id,pubkey}
        if let Ok(raw) = std::env::var("BRIDGE_VALIDATORS") {
            if let Ok(vals) = serde_json::from_str::<Vec<BridgeValidator>>(&raw) {
                store.set_validators(&vals)?;
            }
        }
    }
    Ok(())
}
