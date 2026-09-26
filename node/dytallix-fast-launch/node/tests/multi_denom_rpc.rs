use serde_json::json;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

use dytallix_fast_node::mempool::Mempool;
use dytallix_fast_node::metrics::Metrics;
use dytallix_fast_node::rpc::RpcContext;
use dytallix_fast_node::runtime::emission::EmissionEngine;
use dytallix_fast_node::runtime::fee_burn::FeeBurnEngine;
use dytallix_fast_node::runtime::governance::GovernanceModule;
use dytallix_fast_node::runtime::staking::StakingModule;
use dytallix_fast_node::state::State;
use dytallix_fast_node::storage::blocks::TpsWindow;
use dytallix_fast_node::storage::state::Storage;
use dytallix_fast_node::ws::server::WsHub;

// Mock the RPC endpoint behavior for testing
async fn test_balance_endpoint() {
    let temp_dir = tempdir().unwrap();
    let storage = Arc::new(Storage::open(temp_dir.path().to_path_buf()).unwrap());
    let mut state = State::new(storage.clone());

    let addr = "dyt1test123";

    // Set up multi-denomination balances
    state.credit(addr, "udgt", 1_000_000); // 1 DGT
    state.credit(addr, "udrt", 2_000_000); // 2 DRT

    // Create RPC context
    let mempool = Arc::new(Mutex::new(Mempool::new()));
    let state_mutex = Arc::new(Mutex::new(state));
    let ws = WsHub::new();
    let tps = Arc::new(Mutex::new(TpsWindow::new(60)));
    let emission = Arc::new(Mutex::new(EmissionEngine::new(
        storage.clone(),
        state_mutex.clone(),
    )));
    let staking = Arc::new(Mutex::new(StakingModule::new(storage.clone())));
    let governance = Arc::new(Mutex::new(GovernanceModule::new(
        storage.clone(),
        state_mutex.clone(),
        staking.clone(),
    )));
    let metrics = Arc::new(Metrics::new().expect("metrics"));

    let ctx = RpcContext {
        storage,
        mempool,
        state: state_mutex,
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
    };

    // Test multi-denomination response (no specific denom requested)
    {
        let mut state = ctx.state.lock().unwrap();
        let balances = state.balances_of(addr);
        let legacy_balance = state.legacy_balance_of(addr);

        // Simulate the RPC response formatting
        let formatted_balances: std::collections::HashMap<String, serde_json::Value> = balances
            .iter()
            .map(|(denom, amount)| {
                let denom_info = match denom.as_str() {
                    "udgt" => json!({
                        "balance": amount.to_string(),
                        "formatted": format!("{} DGT", amount / 1_000_000),
                        "type": "governance",
                        "description": "Governance token for voting and staking"
                    }),
                    "udrt" => json!({
                        "balance": amount.to_string(),
                        "formatted": format!("{} DRT", amount / 1_000_000),
                        "type": "reward",
                        "description": "Reward token for transaction fees and staking rewards"
                    }),
                    _ => json!({
                        "balance": amount.to_string(),
                        "type": "unknown"
                    }),
                };
                (denom.clone(), denom_info)
            })
            .collect();

        let response = json!({
            "address": addr,
            "balances": formatted_balances,
            "legacy_balance": legacy_balance.to_string()
        });

        // Verify response structure
        assert_eq!(response["address"], addr);
        assert_eq!(response["legacy_balance"], "1000000");

        let balances_obj = &response["balances"];
        assert_eq!(balances_obj["udgt"]["balance"], "1000000");
        assert_eq!(balances_obj["udgt"]["formatted"], "1 DGT");
        assert_eq!(balances_obj["udgt"]["type"], "governance");

        assert_eq!(balances_obj["udrt"]["balance"], "2000000");
        assert_eq!(balances_obj["udrt"]["formatted"], "2 DRT");
        assert_eq!(balances_obj["udrt"]["type"], "reward");
    }

    // Test specific denomination query
    {
        let mut state = ctx.state.lock().unwrap();
        let bal = state.balance_of(addr, "udgt");

        let response = json!({
            "address": addr,
            "denom": "udgt",
            "balance": bal.to_string()
        });

        assert_eq!(response["address"], addr);
        assert_eq!(response["denom"], "udgt");
        assert_eq!(response["balance"], "1000000");
    }
}

#[test]
fn test_denomination_validation() {
    // Test the denomination validation logic that would be used in transaction validation
    let denoms = vec!["DGT", "DRT", "dgt", "drt"];

    for denom in denoms {
        let up = denom.to_ascii_uppercase();
        assert!(up == "DGT" || up == "DRT", "Invalid denom: {denom}");

        // Test conversion to micro denominations
        let micro_denom = match up.as_str() {
            "DGT" => "udgt",
            "DRT" => "udrt",
            _ => panic!("Unexpected denom"),
        };

        assert!(micro_denom.starts_with("u"));
    }
}

#[test]
fn test_balance_operations_with_real_amounts() {
    let temp_dir = tempdir().unwrap();
    let storage = Arc::new(Storage::open(temp_dir.path().to_path_buf()).unwrap());
    let mut state = State::new(storage);

    let addr = "dyt1realtest";

    // Test with realistic token amounts (using micro denomination)
    let dgt_amount = 10_000_000; // 10 DGT
    let drt_amount = 100_000_000; // 100 DRT

    state.credit(addr, "udgt", dgt_amount);
    state.credit(addr, "udrt", drt_amount);

    assert_eq!(state.balance_of(addr, "udgt"), dgt_amount);
    assert_eq!(state.balance_of(addr, "udrt"), drt_amount);

    // Test transfer with fees
    let to_addr = "dyt1recipient";
    let transfer_amount = 5_000_000; // 5 DRT
    let fee_amount = 100_000; // 0.1 DGT fee

    // Give recipient some initial DGT for testing
    state.credit(to_addr, "udgt", 1_000_000);

    let result = state.apply_transfer(addr, to_addr, "udrt", transfer_amount, "udgt", fee_amount);
    assert!(result.is_ok());

    // Verify balances after transfer
    assert_eq!(state.balance_of(addr, "udgt"), dgt_amount - fee_amount);
    assert_eq!(state.balance_of(addr, "udrt"), drt_amount - transfer_amount);
    assert_eq!(state.balance_of(to_addr, "udrt"), transfer_amount);
    assert_eq!(state.balance_of(to_addr, "udgt"), 1_000_000); // Unchanged
}

#[tokio::test]
async fn test_rpc_balance_endpoint() {
    test_balance_endpoint().await;
}
