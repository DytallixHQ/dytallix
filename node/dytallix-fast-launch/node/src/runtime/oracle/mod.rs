use crate::rpc::errors::ApiError;
use crate::rpc::RpcContext;
use crate::storage::oracle::{AiRiskRecord, OracleStore};
use axum::{Extension, Json};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine; // for new decode API
use fips204::ml_dsa_65;
use fips204::traits::{SerDes, Verifier};
use serde::Deserialize;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Deserialize)]
pub struct OracleAiRiskInput {
    pub tx_hash: String,
    pub model_id: String,
    pub risk_score: f32,
    pub confidence: Option<f32>,
    pub signature: Option<String>,
}

#[derive(Deserialize)]
pub struct OracleAiRiskBatchInput {
    pub records: Vec<OracleAiRiskInput>,
}

/// Verify ML-DSA-65 signature for oracle data
///
/// # Arguments
/// * `payload` - The data that was signed
/// * `signature` - Base64-encoded signature
/// * `pubkey` - Base64-encoded public key
///
/// # Returns
/// `true` if signature is valid, `false` otherwise
pub fn verify_sig(payload: &str, signature: &str, pubkey: &str) -> bool {
    if let (Ok(pk_bytes), Ok(sig_bytes)) = (B64.decode(pubkey), B64.decode(signature)) {
        // Reject keys outside the approved operational parameter set.
        if pk_bytes.len() != ml_dsa_65::PK_LEN {
            return false;
        }

        let mut pk_arr = [0u8; ml_dsa_65::PK_LEN];
        pk_arr.copy_from_slice(&pk_bytes);

        if let Ok(pk) = ml_dsa_65::PublicKey::try_from_bytes(pk_arr) {
            let ctx = b"dytallix-oracle";

            // Accept the exact detached signature encoding.
            if sig_bytes.len() != ml_dsa_65::SIG_LEN {
                return false;
            }
            let mut sig_arr = [0u8; ml_dsa_65::SIG_LEN];
            sig_arr.copy_from_slice(&sig_bytes);

            // The oracle domain remains separate from transaction signatures.
            return pk.verify(payload.as_bytes(), &sig_arr, ctx);
        }
    }
    false
}

/// Apply oracle risk assessment to transaction
///
/// # Arguments
/// * `store` - Oracle store for persistence
/// * `tx_hash` - Transaction hash (hex format with 0x prefix)
/// * `score_str` - Original score string (preserved for determinism)
/// * `model_id` - AI model identifier
/// * `ingested_at` - Unix timestamp of ingestion
/// * `source` - Oracle source identifier
///
/// # Returns
/// `Ok(())` on success, error on failure
pub fn apply_oracle_risk(
    store: &OracleStore,
    tx_hash: &str,
    score_str: &str,
    model_id: &str,
    ingested_at: u64,
    source: &str,
) -> anyhow::Result<()> {
    // Validate inputs
    if !tx_hash.starts_with("0x") || tx_hash.len() < 3 {
        return Err(anyhow::anyhow!("Invalid transaction hash format"));
    }

    if score_str.trim().is_empty() {
        return Err(anyhow::anyhow!("Score string cannot be empty"));
    }

    if model_id.trim().is_empty() {
        return Err(anyhow::anyhow!("Model ID cannot be empty"));
    }

    if source.trim().is_empty() {
        return Err(anyhow::anyhow!("Source cannot be empty"));
    }

    // Parse score for storage (maintain original string)
    let risk_score = score_str
        .parse::<f32>()
        .map_err(|_| anyhow::anyhow!("Invalid score format"))?;

    if !(0.0..=1.0).contains(&risk_score) {
        return Err(anyhow::anyhow!("Score must be between 0.0 and 1.0"));
    }

    let record = AiRiskRecord {
        tx_hash: tx_hash.to_string(),
        model_id: model_id.to_string(),
        risk_score,
        score_str: score_str.to_string(), // Preserve original for determinism
        confidence: None,
        signature: None,
        oracle_pubkey: None,
        ingested_at,
        source: source.to_string(),
    };

    store.put_ai_risk(&record)
}

/// Get oracle risk assessment for transaction
///
/// # Arguments
/// * `store` - Oracle store for lookup
/// * `tx_hash` - Transaction hash to lookup
///
/// # Returns
/// `Some(AiRiskRecord)` if found, `None` otherwise
pub fn get_oracle_risk(store: &OracleStore, tx_hash: &str) -> Option<AiRiskRecord> {
    store.get_ai_risk(tx_hash)
}

/// Get current Unix timestamp
pub fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[axum::debug_handler]
pub async fn post_ai_risk(
    Extension(ctx): Extension<RpcContext>,
    Json(inp): Json<OracleAiRiskInput>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let start_time = std::time::SystemTime::now();

    if !(0.0..=1.0).contains(&inp.risk_score) {
        ctx.metrics.record_oracle_submission("error");
        return Err(ApiError::Internal);
    }

    let configured_pk = std::env::var("AI_ORACLE_PUBKEY").ok();
    if let Some(pk) = configured_pk.as_ref() {
        let sig = inp.signature.as_ref().ok_or_else(|| {
            ctx.metrics.record_oracle_submission("error");
            ApiError::Internal
        })?;
        let payload = format!("{}:{}:{}", inp.tx_hash, inp.risk_score, inp.model_id);
        if !verify_sig(&payload, sig, pk) {
            ctx.metrics.record_oracle_submission("error");
            return Err(ApiError::Internal);
        }
    }

    let store = OracleStore {
        db: &ctx.storage.db,
    };

    let ingested_at = current_timestamp();
    let source = std::env::var("DLX_ORACLE_MODEL_ID").unwrap_or_else(|_| "risk-v1".to_string());

    let score_str = inp.risk_score.to_string();

    // Use the new apply_oracle_risk function
    if let Err(e) = apply_oracle_risk(
        &store,
        &inp.tx_hash,
        &score_str,
        &inp.model_id,
        ingested_at,
        &source,
    ) {
        eprintln!("Failed to apply oracle risk: {e}");
        ctx.metrics.record_oracle_submission("error");
        return Err(ApiError::Internal);
    }

    // Record oracle metrics
    if let Ok(latency) = start_time.elapsed() {
        ctx.metrics.record_oracle_update(latency);
        ctx.metrics.record_oracle_submission("ok");
    }

    Ok(Json(serde_json::json!({"ok": true})))
}

#[axum::debug_handler]
pub async fn post_ai_risk_batch(
    Extension(ctx): Extension<RpcContext>,
    Json(inp): Json<OracleAiRiskBatchInput>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let start_time = std::time::SystemTime::now();

    let configured_pk = std::env::var("AI_ORACLE_PUBKEY").ok();
    let store = OracleStore {
        db: &ctx.storage.db,
    };

    let mut records = Vec::new();
    let mut errors = Vec::new();
    let ingested_at = current_timestamp();
    let source = std::env::var("DLX_ORACLE_MODEL_ID").unwrap_or_else(|_| "risk-v1".to_string());

    for (idx, risk_input) in inp.records.iter().enumerate() {
        // Validate each record
        if !(0.0..=1.0).contains(&risk_input.risk_score) {
            errors.push(format!("Record {idx}: risk_score out of range"));
            continue;
        }

        // Verify signature if required
        if let Some(pk) = configured_pk.as_ref() {
            if let Some(sig) = risk_input.signature.as_ref() {
                let payload = format!(
                    "{}:{}:{}",
                    risk_input.tx_hash, risk_input.risk_score, risk_input.model_id
                );
                if !verify_sig(&payload, sig, pk) {
                    errors.push(format!("Record {idx}: invalid signature"));
                    continue;
                }
            } else {
                errors.push(format!("Record {idx}: missing required signature"));
                continue;
            }
        }

        let score_str = risk_input.risk_score.to_string();

        // Apply oracle risk for each record
        if let Err(e) = apply_oracle_risk(
            &store,
            &risk_input.tx_hash,
            &score_str,
            &risk_input.model_id,
            ingested_at,
            &source,
        ) {
            errors.push(format!("Record {idx}: {e}"));
            continue;
        }

        records.push(risk_input.tx_hash.clone());
    }

    // Record oracle metrics for batch processing
    if let Ok(latency) = start_time.elapsed() {
        ctx.metrics.record_oracle_update(latency);
        if errors.is_empty() {
            ctx.metrics.record_oracle_submission("ok");
        } else {
            ctx.metrics.record_oracle_submission("error");
        }
    }

    Ok(Json(serde_json::json!({
        "processed": records.len(),
        "failed": errors.len(),
        "failed_hashes": Vec::<String>::new(), // For compatibility
        "validation_errors": errors
    })))
}
