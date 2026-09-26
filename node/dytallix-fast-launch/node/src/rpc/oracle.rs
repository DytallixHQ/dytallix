#![allow(unused)]
use crate::rpc::errors::ApiError;
use crate::rpc::RpcContext;
#[cfg(feature = "oracle")]
use crate::runtime::oracle::{OracleAiRiskBatchInput, OracleAiRiskInput};
#[cfg(feature = "oracle")]
use crate::storage::oracle::OracleStore;
use axum::{Extension, Json};
use serde_json::json;

#[cfg(feature = "oracle")]
#[axum::debug_handler]
pub async fn submit_ai_risk(
    Extension(ctx): Extension<RpcContext>,
    Json(inp): Json<OracleAiRiskInput>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::rpc::require_legacy_route(&ctx)?;
    // Delegate to the runtime module
    crate::runtime::oracle::post_ai_risk(Extension(ctx), Json(inp)).await
}
#[cfg(not(feature = "oracle"))]
pub async fn submit_ai_risk(
    _ctx: Extension<RpcContext>,
    _inp: Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::BadRequest("oracle feature disabled".into()))
}

#[cfg(feature = "oracle")]
#[axum::debug_handler]
pub async fn submit_ai_risk_batch(
    Extension(ctx): Extension<RpcContext>,
    Json(inp): Json<OracleAiRiskBatchInput>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::rpc::require_legacy_route(&ctx)?;
    // Delegate to the runtime module
    crate::runtime::oracle::post_ai_risk_batch(Extension(ctx), Json(inp)).await
}
#[cfg(not(feature = "oracle"))]
pub async fn submit_ai_risk_batch(
    _ctx: Extension<RpcContext>,
    _inp: Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::BadRequest("oracle feature disabled".into()))
}

#[cfg(feature = "oracle")]
#[axum::debug_handler]
pub async fn get_ai_risk_batch(
    Extension(ctx): Extension<RpcContext>,
    Json(tx_hashes): Json<Vec<String>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = OracleStore {
        db: &ctx.storage.db,
    };

    let mut results = Vec::new();
    let mut not_found = Vec::new();

    for tx_hash in tx_hashes {
        if let Some(risk_record) = store.get_ai_risk(&tx_hash) {
            results.push(json!({
                "tx_hash": tx_hash,
                "model_id": risk_record.model_id,
                "risk_score": risk_record.risk_score,
                "confidence": risk_record.confidence,
                "oracle_pubkey": risk_record.oracle_pubkey
            }));
        } else {
            not_found.push(tx_hash);
        }
    }

    Ok(Json(json!({
        "found": results,
        "not_found": not_found,
        "total_requested": results.len() + not_found.len(),
        "total_found": results.len()
    })))
}
#[cfg(not(feature = "oracle"))]
pub async fn get_ai_risk_batch(
    _ctx: Extension<RpcContext>,
    _tx_hashes: Json<Vec<String>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::BadRequest("oracle feature disabled".into()))
}

#[cfg(feature = "oracle")]
#[axum::debug_handler]
pub async fn oracle_stats(
    Extension(_ctx): Extension<RpcContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // For now, return basic stats. In a real implementation, we'd track more metrics
    let configured_pubkey = std::env::var("AI_ORACLE_PUBKEY").is_ok();

    Ok(Json(json!({
        "signature_verification_enabled": configured_pubkey,
        "supported_endpoints": [
            "/oracle/ai_risk",
            "/oracle/ai_risk_batch",
            "/oracle/ai_risk_query_batch",
            "/oracle/stats"
        ],
        "schema_version": "1.0",
        "validation_rules": {
            "risk_score_range": "0.0 to 1.0",
            "confidence_range": "0.0 to 1.0",
            "tx_hash_format": "hex string starting with 0x",
            "model_id_required": true
        }
    })))
}
#[cfg(not(feature = "oracle"))]
pub async fn oracle_stats(
    _ctx: Extension<RpcContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::BadRequest("oracle feature disabled".into()))
}
