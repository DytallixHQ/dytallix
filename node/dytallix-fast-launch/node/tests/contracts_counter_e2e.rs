#![cfg(feature = "contracts")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, Query};
use axum::{Extension, Json};
use dytallix_fast_node::mempool::Mempool;
use dytallix_fast_node::rpc::contracts::ContractQueryParams;
use dytallix_fast_node::rpc::{self, FeatureFlags, RpcContext};
use dytallix_fast_node::runtime::emission::EmissionEngine;
use dytallix_fast_node::runtime::fee_burn::FeeBurnEngine;
use dytallix_fast_node::runtime::governance::GovernanceModule;
use dytallix_fast_node::runtime::staking::StakingModule;
use dytallix_fast_node::state::State;
use dytallix_fast_node::storage::blocks::TpsWindow;
use dytallix_fast_node::storage::state::Storage;
use dytallix_fast_node::ws::server::WsHub;
use serde_json::json;
use tempfile::tempdir;

const PING_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, // magic
    0x01, 0x00, 0x00, 0x00, // version
    0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7f, // type section
    0x03, 0x02, 0x01, 0x00, // function section
    0x07, 0x08, 0x01, 0x04, 0x70, 0x69, 0x6e, 0x67, 0x00, 0x00, // export section
    0x0a, 0x06, 0x01, 0x04, 0x00, 0x41, 0x07, 0x0b, // code section
];

fn build_ctx() -> RpcContext {
    let dir = tempdir().unwrap();
    let storage = Arc::new(Storage::open(dir.path().join("node.db")).unwrap());
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
    let metrics = Arc::new(dytallix_fast_node::metrics::Metrics::new().unwrap());

    RpcContext {
        storage,
        mempool,
        state,
        ws,
        tps,
        emission,
        governance,
        staking,
        fee_burn: Arc::new(Mutex::new(FeeBurnEngine::new())),
        metrics,
        features: FeatureFlags {
            governance: true,
            staking: true,
        },
        wasm_contracts: Arc::new(Mutex::new(HashMap::new())),
        wasm_runtime: Arc::new(dytallix_fast_node::runtime::wasm::WasmRuntime::new()),
        pending_assets: Arc::new(Mutex::new(Vec::new())),
        proposer_address: None,
        validator_public_key_b64: None,
        validator_algorithm: None,
        slots_per_epoch: 100,
    }
}

#[tokio::test]
async fn contracts_counter_e2e() {
    let ctx = build_ctx();

    let deploy = rpc::contracts_deploy(
        Extension(ctx.clone()),
        Json(json!({
            "deployer": "dyt1contracttester",
            "code": hex::encode(PING_WASM),
            "gas_limit": 1_000_000u64,
        })),
    )
    .await
    .expect("contract deploy should succeed")
    .0;

    let address = deploy["address"]
        .as_str()
        .expect("deploy should return contract address")
        .to_string();
    assert!(deploy["tx_hash"].as_str().is_some());
    assert_eq!(address.len(), 42, "contract address should be 20-byte hex");

    let info = rpc::contract_info(Extension(ctx.clone()), Path(address.clone()))
        .await
        .expect("contract info should succeed")
        .0;
    assert_eq!(info["address"], address);
    assert_eq!(info["code_size"].as_u64(), Some(PING_WASM.len() as u64));

    let initial_events = rpc::contract_events(Extension(ctx.clone()), Path(address.clone()))
        .await
        .expect("initial contract events should succeed")
        .0;
    assert_eq!(
        initial_events["events"].as_array().map(Vec::len),
        Some(0),
        "deploy should not create execution history entries"
    );

    let query = rpc::contract_query(
        Extension(ctx.clone()),
        Path((address.clone(), "ping".to_string())),
        Query(ContractQueryParams::default()),
    )
    .await
    .expect("contract query should succeed")
    .0;
    assert_eq!(query["contract_address"], address);
    assert_eq!(query["method"], "ping");

    let after_query_events = rpc::contract_events(Extension(ctx.clone()), Path(address.clone()))
        .await
        .expect("post-query contract events should succeed")
        .0;
    assert_eq!(
        after_query_events["events"].as_array().map(Vec::len),
        Some(0),
        "read-only queries should not record execution history"
    );

    let call = rpc::contracts_call(
        Extension(ctx.clone()),
        Json(json!({
            "address": address,
            "method": "ping",
            "args": "",
            "gas_limit": 1_000_000u64,
        })),
    )
    .await
    .expect("contract call should succeed")
    .0;
    assert_eq!(call["result"], "");
    assert!(call["tx_hash"].as_str().is_some());

    let events = rpc::contract_events(Extension(ctx.clone()), Path(address.clone()))
        .await
        .expect("contract events should succeed")
        .0;
    let events = events["events"]
        .as_array()
        .expect("events should be an array");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["method"], "ping");
    assert!(events[0]["tx_hash"].as_str().is_some());
}
