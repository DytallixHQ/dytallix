use crate::rpc::{ApiError, RpcContext};
use axum::{
    extract::{Extension, Path, Query},
    Json,
};
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize, Default)]
pub struct ContractQueryParams {
    pub args: Option<String>,
    pub gas_limit: Option<u64>,
}

fn decode_hex_arg(raw: Option<&str>) -> Result<Vec<u8>, ApiError> {
    match raw {
        Some(value) if !value.is_empty() => hex::decode(value.trim_start_matches("0x"))
            .map_err(|_| ApiError::BadRequest("invalid hex args".to_string())),
        _ => Ok(Vec::new()),
    }
}

fn map_contract_error(err: anyhow::Error) -> ApiError {
    let message = err.to_string();
    if message.contains("Contract not found") {
        ApiError::NotFound
    } else if message.contains("Method not found") {
        ApiError::BadRequest(message)
    } else {
        ApiError::Internal
    }
}

#[cfg(feature = "contracts")]
pub async fn contracts_deploy(
    Extension(ctx): Extension<RpcContext>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::rpc::require_legacy_route(&ctx)?;
    let code_hex = payload
        .get("code")
        .and_then(|v| v.as_str())
        .ok_or(ApiError::BadRequest("missing code".to_string()))?;

    let code = hex::decode(code_hex.trim_start_matches("0x"))
        .map_err(|_| ApiError::BadRequest("invalid hex code".to_string()))?;

    let deployer = payload
        .get("deployer")
        .and_then(|v| v.as_str())
        .unwrap_or("dyt1genesis");

    let gas_limit = payload
        .get("gas_limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(1_000_000);

    let address = ctx
        .wasm_runtime
        .deploy_contract(&code, deployer, gas_limit, None)
        .map_err(map_contract_error)?;

    Ok(Json(json!({
        "address": address.address,
        "tx_hash": address.tx_hash,
        "gas_used": address.gas_used,
        "deployed_at": address.deployed_at,
    })))
}

#[cfg(feature = "contracts")]
pub async fn contracts_call(
    Extension(ctx): Extension<RpcContext>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::rpc::require_legacy_route(&ctx)?;
    let address = payload
        .get("address")
        .and_then(|v| v.as_str())
        .ok_or(ApiError::BadRequest("missing address".to_string()))?;

    let method = payload
        .get("method")
        .and_then(|v| v.as_str())
        .ok_or(ApiError::BadRequest("missing method".to_string()))?;

    let args_hex = payload.get("args").and_then(|v| v.as_str()).unwrap_or("");

    let args = hex::decode(args_hex.trim_start_matches("0x"))
        .map_err(|_| ApiError::BadRequest("invalid hex args".to_string()))?;

    let gas_limit = payload
        .get("gas_limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(1_000_000);

    let result = ctx
        .wasm_runtime
        .execute_contract(&address.to_string(), method, &args, gas_limit)
        .map_err(map_contract_error)?;

    Ok(Json(json!({
        "result": hex::encode(result.result),
        "gas_used": result.gas_used,
        "logs": result.logs,
        "tx_hash": result.tx_hash,
        "executed_at": result.executed_at,
    })))
}

#[cfg(feature = "contracts")]
pub async fn contracts_state(
    Extension(ctx): Extension<RpcContext>,
    Path((address, key)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    match ctx.wasm_runtime.get_contract_state(&address, &key) {
        Some(value) => Ok(Json(json!({
            "contract_address": address,
            "key": key,
            "value": hex::encode(value),
        }))),
        None => Err(ApiError::NotFound),
    }
}

#[cfg(feature = "contracts")]
pub async fn contract_info(
    Extension(ctx): Extension<RpcContext>,
    Path(address): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let deployment = ctx
        .wasm_runtime
        .get_contract(&address)
        .ok_or(ApiError::NotFound)?;

    Ok(Json(json!({
        "address": deployment.address,
        "code_hash": deployment.code_hash,
        "code_size": deployment.code.len(),
        "tx_hash": deployment.tx_hash,
        "gas_used": deployment.gas_used,
        "deployed_at": deployment.deployed_at,
    })))
}

#[cfg(feature = "contracts")]
pub async fn contract_events(
    Extension(ctx): Extension<RpcContext>,
    Path(address): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let events = ctx.wasm_runtime.get_execution_history(Some(&address));
    let events: Vec<_> = events
        .into_iter()
        .map(|event| {
            json!({
                "contract_address": event.contract_address,
                "method": event.method,
                "args": hex::encode(event.args),
                "result": hex::encode(event.result),
                "gas_used": event.gas_used,
                "tx_hash": event.tx_hash,
                "executed_at": event.executed_at,
                "logs": event.logs,
            })
        })
        .collect();

    Ok(Json(json!({
        "contract_address": address,
        "events": events,
    })))
}

#[cfg(feature = "contracts")]
pub async fn contract_query(
    Extension(ctx): Extension<RpcContext>,
    Path((address, method)): Path<(String, String)>,
    Query(params): Query<ContractQueryParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let args = decode_hex_arg(params.args.as_deref())?;
    let gas_limit = params.gas_limit.unwrap_or(1_000_000);
    let result = ctx
        .wasm_runtime
        .query_contract(&address, &method, &args, gas_limit)
        .map_err(map_contract_error)?;

    Ok(Json(json!({
        "contract_address": result.contract_address,
        "method": result.method,
        "result": hex::encode(result.result),
        "gas_used": result.gas_used,
        "logs": result.logs,
    })))
}
