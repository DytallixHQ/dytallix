use axum::{extract::Path, Extension, Json};
use rand::{rngs::StdRng, Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::crypto::PQC;
use crate::rpc::errors::ApiError;
use crate::rpc::RpcContext;
use crate::runtime::oracle::{apply_oracle_risk, current_timestamp};
use crate::storage::oracle::OracleStore;
use std::collections::VecDeque;
use std::sync::Mutex;

// Simple in-memory latency window for UI badge
static LAT_WIN: once_cell::sync::Lazy<Mutex<VecDeque<u64>>> =
    once_cell::sync::Lazy::new(|| Mutex::new(VecDeque::with_capacity(1024)));

// Request/response models matching the Python service
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TxInput {
    pub hash: String,
    #[serde(rename = "from")]
    pub from_addr: String,
    pub to: String,
    pub amount: u64,
    pub fee: u64,
    pub nonce: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AiScoreReq {
    pub tx: TxInput,
    pub model_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AiScoreResp {
    pub score: f32,
    pub tx_hash: String,
    pub model_id: Option<String>,
    pub ts: Option<u64>,
    pub signature: Option<String>,
}

/// Sign a client request payload using PQC (Dilithium) and return base64 signature and pk
type EphemeralKeypair = (Vec<u8>, Vec<u8>);
fn sign_client_request(payload: &str) -> (String, String) {
    // Thread-safe lazy init of ephemeral keypair
    static KEYPAIR: once_cell::sync::Lazy<std::sync::Mutex<Option<EphemeralKeypair>>> =
        once_cell::sync::Lazy::new(|| std::sync::Mutex::new(None));
    let mut guard = KEYPAIR.lock().unwrap();
    if guard.is_none() {
        *guard = Some(crate::crypto::ActivePQC::keypair());
    }
    let (sk, pk) = guard.as_ref().unwrap();
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    let sig = crate::crypto::ActivePQC::sign(sk, payload.as_bytes());
    (STANDARD.encode(sig), STANDARD.encode(pk))
}

/// POST /ai/score
/// Calls the external AI service, verifies oracle signature (if configured), persists risk.
#[axum::debug_handler]
pub async fn ai_score(
    Extension(ctx): Extension<RpcContext>,
    Json(inp): Json<AiScoreReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::rpc::require_legacy_route(&ctx)?;
    let t_start = std::time::Instant::now();
    let model_id = inp
        .model_id
        .clone()
        .unwrap_or_else(|| "risk-v1".to_string());

    // Prepare client-signed request context (PQC)
    let ts = current_timestamp();
    let payload = format!("{}:{}:{}", model_id, inp.tx.hash, ts);
    let (client_sig_b64, client_pk_b64) = sign_client_request(&payload);

    // Call external AI service with robust client: timeout=800ms, retries=1 with jitter
    let ai_url =
        std::env::var("AI_RISK_URL").unwrap_or_else(|_| "http://127.0.0.1:7000".to_string());
    let url = format!("{}/score", ai_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(800))
        .build()
        .map_err(|_| ApiError::Internal)?;

    let req_body = serde_json::json!({
        "hash": inp.tx.hash,
        "from": inp.tx.from_addr,
        "to": inp.tx.to,
        "amount": inp.tx.amount,
        "fee": inp.tx.fee,
        "nonce": inp.tx.nonce,
    });

    let mut attempts = 0;
    // track errors in logs only (no need to keep variable)
    let mut rng = StdRng::from_entropy();
    let resp: AiScoreResp = loop {
        attempts += 1;
        let res = client
            .post(&url)
            .query(&[("model_id", &model_id)])
            .header("x-dlx-client-ts", ts.to_string())
            .header("x-dlx-client-sig", &client_sig_b64)
            .header("x-dlx-client-pk", &client_pk_b64)
            .json(&req_body)
            .send()
            .await;
        match res {
            Ok(r) if r.status().is_success() => match r.json::<AiScoreResp>().await {
                Ok(parsed) => break parsed,
                Err(_e) => {}
            },
            Ok(r) => {
                let _ = r.status();
            }
            Err(e) => {
                tracing::warn!("ai_score request error: {}", e);
            }
        }
        if attempts >= 2 {
            return Err(ApiError::Internal);
        }
        // jitter 50-150ms before retry
        let jitter_ms: u64 = rng.gen_range(50..=150);
        tokio::time::sleep(Duration::from_millis(jitter_ms)).await;
    };

    // Verify oracle signature if configured (ed25519)
    if let Ok(pk_b64) = std::env::var("AI_ORACLE_PUBKEY") {
        if let (Some(sig_b64), Some(mid)) = (resp.signature.as_ref(), resp.model_id.as_ref()) {
            let payload = format!("{}:{}:{}", resp.tx_hash, resp.score, mid);
            if !crate::runtime::oracle::verify_sig(&payload, sig_b64, &pk_b64) {
                return Err(ApiError::Internal);
            }
        } else {
            // Missing signature or model id
            return Err(ApiError::Internal);
        }
    }

    // Persist into oracle store
    let store = OracleStore {
        db: &ctx.storage.db,
    };
    let ingested_at = current_timestamp();
    let source = std::env::var("DLX_ORACLE_MODEL_ID").unwrap_or_else(|_| model_id.clone());
    let score_str = resp.score.to_string();
    if let Err(_e) = apply_oracle_risk(
        &store,
        &resp.tx_hash,
        &score_str,
        resp.model_id.as_deref().unwrap_or(&model_id),
        ingested_at,
        &source,
    ) {
        return Err(ApiError::Internal);
    }

    // Record latency sample
    let ms = t_start.elapsed().as_millis();
    let mut w = LAT_WIN.lock().unwrap();
    let ms_u64 = (ms as u64).min(60_000);
    if w.len() >= 1000 {
        w.pop_front();
    }
    w.push_back(ms_u64);

    Ok(Json(serde_json::json!({
        "ok": true,
        "tx_hash": resp.tx_hash,
        "model_id": resp.model_id.unwrap_or(model_id),
        "score": resp.score,
        "ts": resp.ts,
    })))
}

/// GET /ai/risk/:hash -> return stored record if present
#[axum::debug_handler]
pub async fn ai_risk_get(
    Extension(ctx): Extension<RpcContext>,
    Path(hash): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = OracleStore {
        db: &ctx.storage.db,
    };
    if let Some(rec) = store.get_ai_risk(&hash) {
        return Ok(Json(serde_json::json!({
            "tx_hash": rec.tx_hash,
            "model_id": rec.model_id,
            "risk_score": rec.risk_score,
            "confidence": rec.confidence,
            "source": rec.source,
            "ingested_at": rec.ingested_at,
        })));
    }
    Err(ApiError::NotFound)
}

/// GET /ai/latency -> { avg_ms, p95_ms, samples }
#[axum::debug_handler]
pub async fn ai_latency() -> Result<Json<serde_json::Value>, ApiError> {
    let w = LAT_WIN.lock().unwrap();
    let mut xs: Vec<u64> = w.iter().copied().collect();
    if xs.is_empty() {
        return Ok(Json(
            serde_json::json!({"avg_ms": null, "p95_ms": null, "samples": 0}),
        ));
    }
    xs.sort_unstable();
    let n = xs.len();
    let sum: u128 = xs.iter().map(|&v| v as u128).sum();
    let avg = (sum as f64) / (n as f64);
    let idx = ((n as f64 - 1.0) * 0.95).round() as usize;
    let p95 = xs[idx];
    Ok(Json(
        serde_json::json!({"avg_ms": avg, "p95_ms": p95, "samples": n}),
    ))
}
