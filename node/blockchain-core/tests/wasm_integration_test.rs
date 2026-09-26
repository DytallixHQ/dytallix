#[path = "support/temp_directory.rs"]
mod test_support;
use dytallix_node::consensus::ConsensusEngine;
use dytallix_node::crypto::PQCManager;
use dytallix_node::runtime::DytallixRuntime;
use dytallix_node::storage::StorageManager;
use dytallix_node::types::{
    CallTransaction, DeployTransaction, PQCTransactionSignature, Transaction,
};
use dytallix_pqc::{Signature, SignatureAlgorithm};
use std::sync::Arc;
use test_support::TestDirectory;

#[tokio::test]
async fn test_wasm_contract_integration() {
    // Initialize components
    let directory = TestDirectory::new();
    let storage = Arc::new(
        StorageManager::open(directory.path(), "contract-test", None)
            .await
            .unwrap(),
    );
    let runtime = Arc::new(DytallixRuntime::new(storage.clone()).unwrap());
    let pqc_manager = Arc::new(PQCManager::new().unwrap());

    // Create consensus engine
    let consensus_engine = ConsensusEngine::new_with_key_path(
        runtime,
        pqc_manager,
        &directory.path().join("pqc_keys.json"),
    )
    .await
    .unwrap();

    // Create a simple WASM contract bytecode (mock)
    let wasm_code = vec![
        0x00, 0x61, 0x73, 0x6d, // WASM magic number
        0x01, 0x00, 0x00,
        0x00, // WASM version
              // ... more WASM bytecode would go here
    ];
    // Create deployment transaction
    let deploy_tx = DeployTransaction {
        from: "deployer123".to_string(),
        contract_code: wasm_code.clone(),
        constructor_args: vec![],
        gas_limit: 1000000,
        timestamp: 1234567890,
        signature: PQCTransactionSignature {
            signature: Signature {
                data: vec![1, 2, 3, 4],
                algorithm: SignatureAlgorithm::Dilithium5,
            },
            public_key: vec![5, 6, 7, 8],
        },
        hash: "deploy_hash".to_string(),
        fee: 100,
        gas_price: 1,
        nonce: 1,
    };

    // Test contract deployment - create a block with deploy transaction
    let deploy_block = consensus_engine
        .propose_block(vec![Transaction::Deploy(deploy_tx.clone())])
        .await;
    assert!(deploy_block.is_ok());

    // Create contract call transaction
    let call_tx = CallTransaction {
        from: "caller123".to_string(),
        to: "contract_address".to_string(),
        method: "test_method".to_string(),
        args: vec![1, 2, 3, 4],
        value: 0,
        gas_limit: 500000,
        timestamp: 1234567891,
        signature: PQCTransactionSignature {
            signature: Signature {
                data: vec![9, 10, 11, 12],
                algorithm: SignatureAlgorithm::Dilithium5,
            },
            public_key: vec![13, 14, 15, 16],
        },
        hash: "call_hash".to_string(),
        fee: 50,
        gas_price: 1,
        nonce: 2,
    };

    // Test contract call - create a block with call transaction
    let call_block = consensus_engine
        .propose_block(vec![Transaction::Call(call_tx.clone())])
        .await;
    assert!(call_block.is_ok());

    println!("WASM contract integration test passed!");
}
