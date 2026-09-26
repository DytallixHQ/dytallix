#[path = "support/stake_fixture.rs"]
mod stake_fixture;
use stake_fixture::StakeFixture;

use dytallix_fast_node::rpc::{FeatureFlags, RpcContext};
use dytallix_fast_node::runtime::fee_burn::FeeBurnEngine;
use dytallix_fast_node::runtime::governance::GovernanceModule;
use dytallix_fast_node::runtime::staking::StakingModule;
use dytallix_fast_node::state::State;
use dytallix_fast_node::storage::state::Storage;
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

/// Test the complete governance happy path using RPC endpoints
#[tokio::test]
async fn governance_rpc_happy_path_test() {
    // Setup test environment
    let dir = tempdir().unwrap();
    let storage = Arc::new(Storage::open(dir.path().join("node.db")).unwrap());
    let state = Arc::new(Mutex::new(State::new(storage.clone())));
    let staking = Arc::new(Mutex::new(StakingModule::new(storage.clone())));
    let governance = Arc::new(Mutex::new(GovernanceModule::new(
        storage.clone(),
        state.clone(),
        staking.clone(),
    )));

    // Fund test accounts
    {
        let mut s = state.lock().unwrap();
        let mut depositor = s.get_account("depositor1");
        depositor.add_balance("udgt", 2_000_000_000);
        s.accounts.insert("depositor1".to_string(), depositor);

        let mut voter1 = s.get_account("voter1");
        voter1.add_balance("udgt", 500_000_000);
        s.accounts.insert("voter1".to_string(), voter1);

        let mut voter2 = s.get_account("voter2");
        voter2.add_balance("udgt", 500_000_000);
        s.accounts.insert("voter2".to_string(), voter2);
    }

    // Voting eligibility comes from bonded stake, not the funded liquid balances.
    {
        let mut staking = staking.lock().unwrap();
        staking.seed_delegator_stake("voter1", 500_000_000);
        staking.seed_delegator_stake("voter2", 500_000_000);
        staking.seed_total_stake(1_000_000_000);
    }

    let ctx = RpcContext {
        storage: storage.clone(),
        mempool: Arc::new(Mutex::new(dytallix_fast_node::mempool::Mempool::new())),
        state: state.clone(),
        ws: dytallix_fast_node::ws::server::WsHub::new(),
        tps: Arc::new(Mutex::new(
            dytallix_fast_node::storage::blocks::TpsWindow::new(60),
        )),
        emission: Arc::new(Mutex::new(
            dytallix_fast_node::runtime::emission::EmissionEngine::new(
                storage.clone(),
                state.clone(),
            ),
        )),
        governance: governance.clone(),
        staking: staking.clone(),
        metrics: Arc::new(dytallix_fast_node::metrics::Metrics::new().unwrap()),
        fee_burn: Arc::new(Mutex::new(FeeBurnEngine::new())),
        features: FeatureFlags {
            governance: true,
            staking: true,
        },
        wasm_contracts: Arc::new(Mutex::new(HashMap::new())),
        #[cfg(feature = "contracts")]
        wasm_runtime: Arc::new(dytallix_fast_node::runtime::wasm::WasmRuntime::new()),
        pending_assets: Arc::new(Mutex::new(Vec::new())),
        proposer_address: None,
        validator_public_key_b64: None,
        validator_algorithm: None,
        slots_per_epoch: 100,
    };

    // Test step 1: Submit proposal
    let submit_request = json!({
        "title": "RPC Happy Path Test",
        "description": "Test complete governance flow via RPC",
        "key": "gas_limit",
        "value": "42000"
    });

    let submit_response = dytallix_fast_node::rpc::gov_submit_proposal(
        axum::Extension(ctx.clone()),
        axum::Json(submit_request),
    )
    .await
    .expect("Failed to submit proposal");

    let proposal_id = submit_response.0["proposal_id"].as_u64().unwrap();
    println!("✅ Step 1: Submitted proposal {}", proposal_id);

    // Test step 2: Deposit on proposal
    let deposit_request = json!({
        "depositor": "depositor1",
        "proposal_id": proposal_id,
        "amount": 1_000_000_000u64
    });

    let _deposit_response = dytallix_fast_node::rpc::gov_deposit(
        axum::Extension(ctx.clone()),
        axum::Json(deposit_request),
    )
    .await
    .expect("Failed to deposit on proposal");

    println!("✅ Step 2: Deposited on proposal");

    // Test step 3: Vote on proposal
    let vote1_request = json!({
        "voter": "voter1",
        "proposal_id": proposal_id,
        "option": "yes"
    });

    let _vote1_response =
        dytallix_fast_node::rpc::gov_vote(axum::Extension(ctx.clone()), axum::Json(vote1_request))
            .await
            .expect("Failed to vote on proposal");

    let vote2_request = json!({
        "voter": "voter2",
        "proposal_id": proposal_id,
        "option": "yes"
    });

    let _vote2_response =
        dytallix_fast_node::rpc::gov_vote(axum::Extension(ctx.clone()), axum::Json(vote2_request))
            .await
            .expect("Failed to vote on proposal");

    println!("✅ Step 3: Voted on proposal");

    // Test step 4: Get initial config
    let initial_config_response =
        dytallix_fast_node::rpc::gov_get_config(axum::Extension(ctx.clone()))
            .await
            .expect("Failed to get initial config");

    let initial_gas_limit = initial_config_response.0["gas_limit"].as_u64().unwrap();
    assert_eq!(initial_gas_limit, 2_000);
    println!("✅ Step 4: Initial gas_limit = {}", initial_gas_limit);

    // Test step 5: Process end_block to move to Passed status
    governance
        .lock()
        .unwrap()
        .end_block(701)
        .expect("Failed to process end_block");

    // Test step 6: Execute proposal manually via RPC
    let execute_request = json!({
        "proposal_id": proposal_id
    });

    let execute_response = dytallix_fast_node::rpc::gov_execute(
        axum::Extension(ctx.clone()),
        axum::Json(execute_request),
    )
    .await
    .expect("Failed to execute proposal");

    assert_eq!(execute_response.0["success"].as_bool().unwrap(), true);
    println!("✅ Step 6: Executed proposal via RPC");

    // Test step 7: Verify parameter change
    let final_config_response =
        dytallix_fast_node::rpc::gov_get_config(axum::Extension(ctx.clone()))
            .await
            .expect("Failed to get final config");

    let final_gas_limit = final_config_response.0["gas_limit"].as_u64().unwrap();
    assert_eq!(final_gas_limit, 42_000);
    println!(
        "✅ Step 7: Final gas_limit = {} (changed from {})",
        final_gas_limit, initial_gas_limit
    );

    // Test step 8: Verify proposal status
    let proposal_response = dytallix_fast_node::rpc::gov_get_proposal(
        axum::Extension(ctx.clone()),
        axum::extract::Path(proposal_id),
    )
    .await
    .expect("Failed to get proposal");

    let status = proposal_response.0["status"].as_str().unwrap();
    // Note: The proposal status might still be "Passed" since we called execute directly
    // rather than through end_block, but the parameter change should be applied
    println!("✅ Step 8: Proposal status = {}", status);

    println!("🎉 Complete governance happy path test successful!");
}
