use dytallix_contracts::runtime::{ContractCall, ContractDeployment, ContractRuntime};

#[tokio::test]
async fn wasm_host_state_events_and_restore() {
    let runtime = ContractRuntime::new(1_000_000, 16).unwrap();
    let deployment = ContractDeployment {
        address: "dyt1host_roundtrip".into(),
        code: wat::parse_str(include_str!("fixtures/state_roundtrip.wat")).unwrap(),
        initial_state: Vec::new(),
        gas_limit: 100_000,
        deployer: "dyt1deployer".into(),
        timestamp: 1,
        ai_audit_score: None,
    };
    let address = runtime.deploy_contract(deployment.clone()).await.unwrap();
    let result = runtime
        .call_contract(ContractCall {
            contract_address: address.clone(),
            caller: "dyt1caller".into(),
            method: "update".into(),
            input_data: Vec::new(),
            gas_limit: 50_000,
            value: 0,
            timestamp: 2,
        })
        .await
        .unwrap();

    assert!(result.success);
    assert!(result.gas_used > 0 && result.gas_used < 50_000);
    assert_eq!(
        runtime.get_contract_state(&address, b"count"),
        Some(b"42".to_vec())
    );
    assert_eq!(result.state_changes.len(), 1);
    assert_eq!(result.state_changes[0].key, b"count");
    assert_eq!(result.state_changes[0].new_value, b"42");
    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].topic, "updated");
    assert_eq!(result.events[0].data, b"42");
    assert_eq!(runtime.get_contract_statistics(&address).unwrap().0, 1);

    // This verifies the runtime snapshot API. Node database recovery is separate.
    let snapshot = runtime.persist_contract_state(&address).unwrap();
    let restored = ContractRuntime::new(1_000_000, 16).unwrap();
    restored.deploy_contract(deployment).await.unwrap();
    restored
        .restore_contract_state(&address, &snapshot)
        .unwrap();
    assert_eq!(
        restored.get_contract_state(&address, b"count"),
        Some(b"42".to_vec())
    );
    assert_eq!(restored.persist_contract_state(&address).unwrap(), snapshot);
}
