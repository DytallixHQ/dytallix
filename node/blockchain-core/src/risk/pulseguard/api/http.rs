use crate::risk::pulseguard::pqc::signer::PqcSigner;
use crate::risk::pulseguard::RiskScore;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue};
use axum::response::sse::{Event, Sse};
use axum::{
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use futures_util::stream::Stream;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct ApiState {
    pub signer: Arc<PqcSigner>,
    pub alert_tx: tokio::sync::broadcast::Sender<RiskScore>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScoreRequest {
    pub tx_hash: String,
    #[serde(default)]
    pub snapshot: bool,
    #[serde(default)]
    pub details: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScoreResponse {
    pub tx_hash: String,
    pub score: f32,
    pub confidence: f32,
    pub reasons: Vec<String>,
    pub explainability: Option<ExplainBlock>,
    pub p95_budget_ms: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExplainBlock {
    pub top_features: Vec<(String, f32)>,
    pub paths: Vec<serde_json::Value>,
}

pub async fn score_endpoint(
    State(state): State<ApiState>,
    Json(req): Json<ScoreRequest>,
) -> impl IntoResponse {
    // Placeholder deterministic scoring
    let mut score = (blake3::hash(req.tx_hash.as_bytes()).as_bytes()[0] as f32 / 255.0) * 100.0;
    let confidence = 0.75;
    let reasons = if score > 80.0 {
        vec!["high_velocity".into()]
    } else {
        vec!["baseline".into()]
    };
    if req.snapshot {
        score *= 0.99;
    }

    let explainability = if req.details {
        Some(ExplainBlock {
            top_features: vec![("velocity_1m".into(), 0.42), ("fanout_k3".into(), 0.25)],
            paths: vec![],
        })
    } else {
        None
    };

    let resp = ScoreResponse {
        tx_hash: req.tx_hash.clone(),
        score,
        confidence,
        reasons,
        explainability,
        p95_budget_ms: 100,
    };

    if score > 80.0 {
        let _ = state.alert_tx.send(RiskScore {
            tx_hash: resp.tx_hash.clone(),
            score: resp.score,
            confidence: resp.confidence,
            reasons: vec!["high_velocity".into()],
            top_features: vec![],
            paths: vec![],
            p95_budget_ms: 100,
            elapsed_ms: 0,
        });
    }

    let payload = serde_json::to_vec(&resp).unwrap();
    let sig = state.signer.sign(&payload).unwrap();
    let mut headers = HeaderMap::new();
    headers.insert("x-pqc-algo", HeaderValue::from_str(&sig.algo).unwrap());
    headers.insert(
        "x-pqc-sig",
        HeaderValue::from_str(&sig.signature_hex).unwrap_or(HeaderValue::from_static("invalid")),
    );
    headers.insert(
        "x-evidence-sha256",
        HeaderValue::from_str(&sig.sha256_hex).unwrap(),
    );
    (headers, Json(resp))
}

pub async fn stream_alerts(
    State(state): State<ApiState>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    use futures_util::stream::unfold;
    let rx = state.alert_tx.subscribe();
    let stream = unfold(rx, |mut rx| async {
        match rx.recv().await {
            Ok(alert) => {
                let json = serde_json::to_string(&alert).unwrap();
                Some((Ok(Event::default().json_data(json).unwrap()), rx))
            }
            Err(_) => None,
        }
    });
    Sse::new(stream)
}

pub fn router(state: ApiState) -> Router {
    Router::new()
        .route("/pulseguard/score", post(score_endpoint))
        .route("/pulseguard/stream", get(stream_alerts))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
}
