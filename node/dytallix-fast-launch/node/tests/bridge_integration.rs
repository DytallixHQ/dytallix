use axum::body::{self, Body}; // adjusted
use axum::http::StatusCode;
use axum::{routing::get, routing::post, Extension, Router};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use dytallix_fast_node::mempool::Mempool;
use dytallix_fast_node::rpc::RpcContext;
use dytallix_fast_node::runtime::bridge::ensure_bridge_validators;
use dytallix_fast_node::runtime::fee_burn::FeeBurnEngine;
use dytallix_fast_node::state::State;
use dytallix_fast_node::storage::blocks::TpsWindow;
use dytallix_fast_node::storage::bridge::BridgeValidator;
use dytallix_fast_node::storage::state::Storage;
use dytallix_fast_node::ws::server::WsHub;
use ed25519_dalek::{Keypair, PublicKey, SecretKey, Signer};
// removed rand to avoid rand_core conflicts
use serde_json::json;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tower::ServiceExt; // for oneshot // rand 0.7

fn deterministic_kp(tag: u8) -> Keypair {
    // simple fixed seed per tag; not cryptographically secure, fine for tests
    let mut seed = [0u8; 32];
    seed[0] = tag;
    let secret = SecretKey::from_bytes(&seed).unwrap();
    let public: PublicKey = (&secret).into();
    Keypair { secret, public }
}

fn test_router(ctx: RpcContext) -> Router {
    use dytallix_fast_node::rpc;
    Router::new()
        .route("/bridge/ingest", post(rpc::bridge_ingest))
        .route("/bridge/halt", post(rpc::bridge_halt))
        .route("/bridge/state", get(rpc::bridge_state))
        .layer(Extension(ctx))
}

fn build_ctx(num_validators: usize) -> (RpcContext, Vec<Keypair>) {
    let dir = tempdir().unwrap();
    let storage = Arc::new(Storage::open(dir.path().join("node.db")).unwrap());
    let mempool = Arc::new(Mutex::new(Mempool::new()));
    let state = Arc::new(Mutex::new(State::new(storage.clone())));
    let tps = Arc::new(Mutex::new(TpsWindow::new(60)));
    let ws = WsHub::new();
    use dytallix_fast_node::runtime::emission::EmissionEngine;
    let emission = Arc::new(Mutex::new(EmissionEngine::new(
        storage.clone(),
        state.clone(),
    )));
    use dytallix_fast_node::metrics::Metrics;
    use dytallix_fast_node::runtime::governance::GovernanceModule;
    use dytallix_fast_node::runtime::staking::StakingModule;
    let staking = Arc::new(Mutex::new(StakingModule::new(storage.clone())));
    let governance = Arc::new(Mutex::new(GovernanceModule::new(
        storage.clone(),
        state.clone(),
        staking.clone(),
    )));
    let metrics = Arc::new(Metrics::new().expect("metrics init"));
    let ctx = RpcContext {
        storage: storage.clone(),
        mempool,
        state,
        ws,
        tps,
        emission,
        governance,
        staking,
        metrics,
        fee_burn: Arc::new(Mutex::new(FeeBurnEngine::new())),
        features: dytallix_fast_node::rpc::FeatureFlags {
            governance: true,
            staking: true,
        },
        // Add minimal wasm contracts map required by RpcContext
        wasm_contracts: Arc::new(Mutex::new(std::collections::HashMap::new())),
        #[cfg(feature = "contracts")]
        wasm_runtime: Arc::new(dytallix_fast_node::runtime::wasm::WasmRuntime::new()),
        pending_assets: Arc::new(Mutex::new(Vec::new())),
        proposer_address: None,
        validator_public_key_b64: None,
        validator_algorithm: None,
        slots_per_epoch: 100,
    }; // added emission
       // gen validators
    let mut keypairs = vec![];
    let mut vals = vec![];
    for i in 0..num_validators as u8 {
        let kp = deterministic_kp(i);
        keypairs.push(kp);
    }
    for (i, kp) in keypairs.iter().enumerate() {
        vals.push(BridgeValidator {
            id: format!("v{i}"),
            pubkey: B64.encode(kp.public.as_bytes()),
        });
    }
    storage
        .db
        .put("bridge:validators", serde_json::to_vec(&vals).unwrap())
        .unwrap();
    (ctx, keypairs)
}

fn sign(_id: &str, kp: &Keypair, msg: &serde_json::Value) -> String {
    // removed unused
    let payload = format!(
        "{}:{}:{}:{}:{}:{}",
        msg["id"].as_str().unwrap(),
        msg["source_chain"].as_str().unwrap(),
        msg["dest_chain"].as_str().unwrap(),
        msg["asset"].as_str().unwrap(),
        msg["amount"].as_str().unwrap(),
        msg["recipient"].as_str().unwrap()
    );
    let sig = kp.sign(payload.as_bytes());
    B64.encode(sig.to_bytes())
}

#[tokio::test]
async fn bridge_ingest_flow() {
    let (ctx, kps) = build_ctx(4); // need ceil(2/3*4)=3
    ensure_bridge_validators(&ctx.storage.db).unwrap();
    let app = test_router(ctx.clone());

    let base_msg = json!({
        "id":"m1", "source_chain":"A", "dest_chain":"B", "asset":"dyt", "amount":"100", "recipient":"dest", "signatures":[], "signers":[]
    });

    // insufficient signatures (2 < 3)
    let mut msg_insufficient = base_msg.clone();
    let mut sigs = vec![];
    let mut signers = vec![];
    for (i, kp) in kps.iter().take(2).enumerate() {
        sigs.push(sign(&format!("v{i}"), kp, &base_msg));
        signers.push(format!("v{i}"));
    }
    msg_insufficient["signatures"] =
        serde_json::Value::Array(sigs.iter().map(|s| json!(s)).collect());
    msg_insufficient["signers"] =
        serde_json::Value::Array(signers.iter().map(|s| json!(s)).collect());

    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/bridge/ingest")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&msg_insufficient).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR); // rejected quorum

    // sufficient quorum (3 signatures)
    let mut msg_quorum = base_msg.clone();
    let mut sigs = vec![];
    let mut signers = vec![];
    for (i, kp) in kps.iter().take(3).enumerate() {
        sigs.push(sign(&format!("v{i}"), kp, &base_msg));
        signers.push(format!("v{i}"));
    }
    msg_quorum["signatures"] = serde_json::Value::Array(sigs.iter().map(|s| json!(s)).collect());
    msg_quorum["signers"] = serde_json::Value::Array(signers.iter().map(|s| json!(s)).collect());
    let resp2 = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/bridge/ingest")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&msg_quorum).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    // fetch state
    let state_resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri("/bridge/state")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(state_resp.status(), StatusCode::OK);
    let body = body::to_bytes(state_resp.into_body(), 2048).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["custody"]["dyt"].as_u64().unwrap(), 100);

    // halt bridge
    let halt_payload = json!({"action":"halt"});
    let resp3 = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/bridge/halt")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&halt_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp3.status(), StatusCode::OK);

    // attempt ingest while halted -> reject
    let resp4 = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/bridge/ingest")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&msg_quorum).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp4.status(), StatusCode::INTERNAL_SERVER_ERROR);

    // resume
    let resume_payload = json!({"action":"resume"});
    let resp5 = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/bridge/halt")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&resume_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp5.status(), StatusCode::OK);
}
