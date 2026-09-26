use axum::{
    routing::{get, post},
    Extension, Router,
};
use dytallix_fast_node::{
    mempool::Mempool, rpc, runtime::emission::EmissionEngine, runtime::fee_burn::FeeBurnEngine,
    runtime::governance::GovernanceModule, runtime::staking::StakingModule, state::State,
    storage::blocks::TpsWindow, storage::state::Storage, ws::server::WsHub,
};
use serde_json::json;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tower::ServiceExt;

fn app() -> (
    Router,
    dytallix_fast_node::rpc::RpcContext,
    tempfile::TempDir,
) {
    app_with_stake(0)
}
fn app_with_stake(
    stake: u128,
) -> (
    Router,
    dytallix_fast_node::rpc::RpcContext,
    tempfile::TempDir,
) {
    app_with_profile(stake, false)
}
fn app_with_profile(stake: u128, reward_v2: bool) -> (Router, rpc::RpcContext, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let mut storage = Storage::open(dir.path().join("node.db")).unwrap();
    let mut source = json!({"chain_id":"test", "accounts":[{"address":"alice","balances":{"udgt":"1000"}}],
        "staking":{"delegations":[{"delegator":"alice","amount_udgt":stake.to_string()}]}});
    if reward_v2 {
        assert_eq!(stake, 0, "This fixture has no eligible reward positions");
        source["staking"] = json!({"delegations":[]});
        source["accounts"][0]["vesting"] = json!({"kind":"unlocked"});
        source["reward_v2"] = json!({"version":2,"activation_height":1,"decimals":6,"profile":"development",
            "max_validators":4,"max_positions":8,"validators":[],"positions":[]});
    }
    dytallix_fast_node::genesis::initialize(
        &mut storage,
        "test",
        Some(&serde_json::to_vec(&source).unwrap()),
    )
    .unwrap();
    let storage = Arc::new(storage);
    let state = Arc::new(Mutex::new(State::new(storage.clone())));
    let mempool = Arc::new(Mutex::new(Mempool::new()));
    let tps = Arc::new(Mutex::new(TpsWindow::new(60)));
    let ws = WsHub::new();
    let emission = Arc::new(Mutex::new(EmissionEngine::new(
        storage.clone(),
        state.clone(),
    )));
    let staking = Arc::new(Mutex::new(StakingModule::new(storage.clone())));
    let governance = Arc::new(Mutex::new(GovernanceModule::new(
        storage.clone(),
        state.clone(),
        staking.clone(),
    )));
    let metrics = Arc::new(dytallix_fast_node::metrics::Metrics::new().expect("metrics"));
    let ctx = dytallix_fast_node::rpc::RpcContext {
        storage,
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
            governance: false,
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
    };
    let router = Router::new()
        .route("/stats", get(rpc::stats))
        .route("/api/supply/drt", get(rpc::drt_supply))
        .route("/api/supply/dgt", get(rpc::dgt_supply))
        .route("/api/staking/stats", get(rpc::staking_get_stats))
        .route("/balance/:addr", get(rpc::get_balance))
        .route("/emission/claim", post(rpc::emission_claim))
        .route("/api/staking/delegate", post(rpc::staking_delegate))
        .route("/api/staking/undelegate", post(rpc::staking_undelegate))
        .route("/api/staking/claim", post(rpc::staking_claim))
        .layer(Extension(ctx.clone()));
    (router, ctx, dir)
}

#[tokio::test]
async fn direct_claim_is_disabled_and_does_not_change_balances() {
    let (app, ctx, _dir) = app();
    // Seed an archived pool directly. The retired mutator is not an execution path.
    ctx.storage
        .db
        .put("emission:last_height", 3u64.to_be_bytes())
        .unwrap();
    ctx.storage
        .db
        .put(
            "emission:pool:block_rewards",
            bincode::serialize(&100u128).unwrap(),
        )
        .unwrap();

    // capture pre-claim pool for robust comparison across schedule changes
    let pre_claim_block_rewards = ctx.emission.lock().unwrap().pool_amount("block_rewards");

    // sanity: stats endpoint reachable
    let resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri("/stats")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // claim 5 from block_rewards to acct A
    let claim_body = json!({"pool":"block_rewards","amount":5,"to":"acctA"});
    let resp2 = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/emission/claim")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(claim_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp2.status(), axum::http::StatusCode::BAD_REQUEST);

    // The rejected direct claim must not credit the account.
    let bal_resp = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri("/balance/acctA?denom=udrt")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(bal_resp.into_body(), 1024)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["balance"].as_str().unwrap(), "0");

    // restart simulation: new context reading same storage
    let storage2 = ctx.storage.clone();
    let state2 = Arc::new(Mutex::new(State::new(storage2.clone()))); // lazy loads balance
    let engine2 = EmissionEngine::new(storage2.clone(), state2.clone());

    // engine2 should see previously advanced height (3)
    assert_eq!(engine2.last_accounted_height(), 3);

    // The rejected direct claim must leave its pool unchanged.
    let expected_after_claim = pre_claim_block_rewards;
    assert_eq!(engine2.pool_amount("block_rewards"), expected_after_claim);
}

#[tokio::test]
async fn enabled_staking_routes_cannot_write_outside_a_committed_block() {
    let (app, ctx, _dir) = app_with_profile(0, true);
    {
        let mut emission = ctx.emission.lock().unwrap();
        let mut staking = ctx.staking.lock().unwrap();
        let mut state = ctx.state.lock().unwrap();
        let mut burn = ctx.fee_burn.lock().unwrap();
        dytallix_fast_node::block_settlement::commit_reward_development_block(
            &mut state,
            &mut emission,
            &mut staking,
            &mut burn,
            true,
            &dytallix_fast_node::block_settlement::BlockRequest {
                height: 1,
                transactions: &[],
                assets: &[],
                timestamp: 10,
                empty_blocks: true,
            },
        )
        .unwrap();
    }
    let snapshot = || {
        ctx.storage
            .db
            .iterator(rocksdb::IteratorMode::Start)
            .map(|v| {
                let (k, v) = v.unwrap();
                (k.to_vec(), v.to_vec())
            })
            .collect::<Vec<_>>()
    };
    let before = snapshot();
    for route in ["delegate", "undelegate", "claim"] {
        let body = json!({"delegator_addr":"alice","validator_addr":"validator","amount_udgt":"10","address":"alice"});
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(format!("/api/staking/{route}"))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::NOT_IMPLEMENTED);
        assert_eq!(snapshot(), before);
        dytallix_fast_node::block_settlement::verify_recovery(&ctx.storage).unwrap();
    }
}

#[tokio::test]
async fn supply_route_returns_exact_committed_values() {
    let (app, ctx, _dir) = app_with_profile(0, true);
    {
        let mut emission = ctx.emission.lock().unwrap();
        emission.config.schedule =
            dytallix_fast_node::runtime::emission::EmissionSchedule::Static { per_block: 101 };
        dytallix_fast_node::block_settlement::commit_reward_development_block(
            &mut ctx.state.lock().unwrap(),
            &mut emission,
            &mut ctx.staking.lock().unwrap(),
            &mut ctx.fee_burn.lock().unwrap(),
            true,
            &dytallix_fast_node::block_settlement::BlockRequest {
                height: 1,
                transactions: &[],
                assets: &[],
                timestamp: 10,
                empty_blocks: true,
            },
        )
        .unwrap();
    }
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/supply/drt")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["genesis"], "0");
    assert_eq!(value["emitted"], "101");
    assert_eq!(value["total"], "101");
    assert_eq!(value["liquid"], "0");
    assert_eq!(value["burned"], "0");
    assert_eq!(value["height"], 1);
    assert_eq!(value["pools"]["staking_rewards"], "25");
    // V2 records the unallocated budget in its reserve, not the retired accumulator.
    use dytallix_fast_node::runtime::reward_runtime::{RewardState, REWARD_STATE_KEY};
    let rewards =
        RewardState::decode(&ctx.storage.db.get(REWARD_STATE_KEY).unwrap().unwrap()).unwrap();
    assert_eq!(rewards.inactive_reserve, 25);
    assert_eq!(rewards.rounding_reserve, 0);
    assert_eq!(rewards.total_budget, 25);
    assert_eq!(rewards.total_unpaid().unwrap(), 0);
    assert_eq!(ctx.staking.lock().unwrap().pending_staking_emission, 0);
}

#[tokio::test]
async fn supply_route_does_not_report_unmarked_legacy_emission_as_committed() {
    let (app, ctx, _dir) = app();
    // Unmarked historical bytes must remain unsuitable as committed supply evidence.
    ctx.storage
        .db
        .put("emission:last_height", 1u64.to_be_bytes())
        .unwrap();
    ctx.storage
        .db
        .put(
            "emission:circulating_supply",
            bincode::serialize(&1_000_000u128).unwrap(),
        )
        .unwrap();
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/supply/drt")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    );
    assert!(axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn dgt_supply_and_staking_stats_use_issued_supply() {
    let (app, ctx, _dir) = app_with_stake(400);
    // The denominator must be the 1000 issued units, not the 10^15-unit cap.
    for route in ["/api/supply/dgt", "/api/staking/stats"] {
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri(route)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        if route == "/api/supply/dgt" {
            assert_eq!(value["issued"], "1000");
            assert_eq!(value["liquid"], "600");
            assert_eq!(value["staked"], "400");
        } else {
            assert_eq!(value["total_supply"], "1000");
            assert_eq!(value["total_stake"], "400");
            assert_eq!(value["staking_ratio"], "40.00%");
            assert_eq!(value["staking_ratio_basis_points"], 4000);
            assert!(value["apy"].is_null());
            assert_eq!(value["apy_status"], "not_qualified");
        }
    }
    assert_eq!(ctx.staking.lock().unwrap().total_stake, 400);
}

#[tokio::test]
async fn dgt_supply_does_not_return_partial_totals() {
    let (app, ctx, _dir) = app();
    ctx.storage.db.delete("staking:total_stake").unwrap();
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/supply/dgt")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    );
    assert!(axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn staking_stats_read_funded_state_when_feature_is_off_and_cache_is_stale() {
    let (_app, mut ctx, _dir) = app_with_stake(400);
    ctx.features.staking = false;
    {
        let mut cache = ctx.staking.lock().unwrap();
        cache.total_stake = 999;
        cache.reward_index = 777;
        cache.pending_staking_emission = 888;
        cache.reward_rate_bps = 1;
    }
    let response = rpc::staking_get_stats(Extension(ctx)).await.unwrap().0;
    assert_eq!(response["total_stake"], "400");
    assert_eq!(response["total_supply"], "1000");
    assert_eq!(response["reward_index"], "0");
    assert_eq!(response["pending_emission"], "0");
    assert_eq!(response["reward_rate_bps"], 500);
    assert_eq!(response["staking_ratio"], "40.00%");
}
