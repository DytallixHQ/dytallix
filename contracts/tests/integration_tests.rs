/*
Integration tests for the Dytallix Smart Contract Runtime

These tests demonstrate the full functionality of the advanced WASM runtime
including contract deployment, execution, state management, and AI integration.
*/

use dytallix_contracts::runtime::*;
use std::sync::Arc;

// Mock AI Analyzer for testing
struct MockAIAnalyzer;

impl ContractAIAnalyzer for MockAIAnalyzer {
    fn analyze_deployment(
        &self,
        _contract: &ContractDeployment,
    ) -> Result<AIAnalysisResult, String> {
        Ok(AIAnalysisResult {
            security_score: 0.85,
            gas_efficiency: 0.9,
            compliance_flags: vec!["secure".to_string()],
            risk_assessment: "Low risk".to_string(),
        })
    }

    fn analyze_execution(
        &self,
        _call: &ContractCall,
        _result: &ExecutionResult,
    ) -> Result<AIAnalysisResult, String> {
        Ok(AIAnalysisResult {
            security_score: 0.8,
            gas_efficiency: 0.85,
            compliance_flags: vec![],
            risk_assessment: "Normal execution".to_string(),
        })
    }

    fn validate_state_change(&self, _change: &StateChange) -> Result<bool, String> {
        Ok(true)
    }
}

#[tokio::test]
async fn test_complete_contract_lifecycle() {
    // Create runtime with AI analyzer
    let mut runtime = ContractRuntime::new(1_000_000, 16).unwrap();
    runtime.set_ai_analyzer(Arc::new(MockAIAnalyzer));

    // Create a minimal WASM contract (just magic + version for testing)
    let contract_code = vec![
        0x00, 0x61, 0x73, 0x6d, // WASM magic number
        0x01, 0x00, 0x00, 0x00, // WASM version
    ];

    // Deploy contract
    let deployment = ContractDeployment {
        address: "dyt1test_contract_123".to_string(),
        code: contract_code,
        initial_state: vec![1, 2, 3, 4], // Some initial state
        gas_limit: 100_000,
        deployer: "dyt1deployer_123".to_string(),
        timestamp: 1234567890,
        ai_audit_score: Some(0.85),
    };

    let deploy_result = runtime.deploy_contract(deployment.clone()).await;
    if let Err(ref e) = deploy_result {
        println!("Deployment failed: {e:?}");
    }
    assert!(deploy_result.is_ok());
    let contract_address = deploy_result.unwrap();
    assert_eq!(contract_address, "dyt1test_contract_123");

    // Verify contract info
    let contract_info = runtime.get_contract_info(&contract_address);
    assert!(contract_info.is_some());
    let info = contract_info.unwrap();
    assert_eq!(info.deployer, "dyt1deployer_123");
    assert_eq!(info.gas_limit, 100_000);

    // Test state persistence
    let state_data = runtime.persist_contract_state(&contract_address).unwrap();
    assert!(!state_data.is_empty());

    // Test state restoration
    runtime
        .restore_contract_state(&contract_address, &state_data)
        .unwrap();

    // Test gas estimation
    let test_call = ContractCall {
        contract_address: contract_address.clone(),
        caller: "dyt1caller_123".to_string(),
        method: "transfer".to_string(),
        input_data: vec![1, 2, 3],
        gas_limit: 50_000,
        value: 100,
        timestamp: 1234567891,
    };

    let estimated_gas = runtime.estimate_gas(&test_call).unwrap();
    assert!(estimated_gas > 0);
    assert!(estimated_gas < test_call.gas_limit);

    // Test contract statistics
    let stats = runtime.get_contract_statistics(&contract_address);
    assert!(stats.is_some());
    let (call_count, last_called, memory_usage) = stats.unwrap();
    assert_eq!(call_count, 0); // No successful calls yet
    assert_eq!(last_called, deployment.timestamp);
    assert_eq!(memory_usage, 0);
}

#[tokio::test]
async fn test_gas_limits_and_metering() {
    let runtime = ContractRuntime::new(10_000, 16).unwrap(); // Very low gas limit

    let deployment = ContractDeployment {
        address: "dyt1gas_test".to_string(),
        code: vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00],
        initial_state: Vec::new(),
        gas_limit: 20_000, // Exceeds runtime limit
        deployer: "dyt1deployer".to_string(),
        timestamp: 1234567890,
        ai_audit_score: Some(0.9),
    };

    // Should fail due to gas limit
    let result = runtime.deploy_contract(deployment).await;
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(matches!(error.code, ErrorCode::InvalidInput));
    assert!(error.message.contains("Gas limit exceeds maximum"));
}

#[tokio::test]
async fn test_ai_validation_failure() {
    // Create analyzer that always rejects contracts
    struct RejectingAnalyzer;
    impl ContractAIAnalyzer for RejectingAnalyzer {
        fn analyze_deployment(
            &self,
            _contract: &ContractDeployment,
        ) -> Result<AIAnalysisResult, String> {
            Ok(AIAnalysisResult {
                security_score: 0.3, // Low score should cause rejection
                gas_efficiency: 0.5,
                compliance_flags: vec!["suspicious".to_string()],
                risk_assessment: "High risk".to_string(),
            })
        }

        fn analyze_execution(
            &self,
            _call: &ContractCall,
            _result: &ExecutionResult,
        ) -> Result<AIAnalysisResult, String> {
            Ok(AIAnalysisResult {
                security_score: 0.3,
                gas_efficiency: 0.5,
                compliance_flags: vec![],
                risk_assessment: "High risk".to_string(),
            })
        }

        fn validate_state_change(&self, _change: &StateChange) -> Result<bool, String> {
            Ok(false)
        }
    }

    let mut runtime = ContractRuntime::new(1_000_000, 16).unwrap();
    runtime.set_ai_analyzer(Arc::new(RejectingAnalyzer));

    let deployment = ContractDeployment {
        address: "dyt1malicious_contract".to_string(),
        code: vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00],
        initial_state: Vec::new(),
        gas_limit: 100_000,
        deployer: "dyt1attacker".to_string(),
        timestamp: 1234567890,
        ai_audit_score: Some(0.3),
    };

    let result = runtime.deploy_contract(deployment).await;
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(matches!(error.code, ErrorCode::AIValidationFailed));
    assert!(error.message.contains("AI security score too low"));
}

#[tokio::test]
async fn test_invalid_wasm_contract() {
    let runtime = ContractRuntime::new(1_000_000, 16).unwrap();

    // Invalid WASM (missing magic number)
    let invalid_code = vec![0xFF, 0xFF, 0xFF, 0xFF, 0x01, 0x00, 0x00, 0x00];

    let deployment = ContractDeployment {
        address: "dyt1invalid_contract".to_string(),
        code: invalid_code,
        initial_state: Vec::new(),
        gas_limit: 100_000,
        deployer: "dyt1deployer".to_string(),
        timestamp: 1234567890,
        ai_audit_score: Some(0.9),
    };

    let result = runtime.deploy_contract(deployment).await;
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(matches!(error.code, ErrorCode::InvalidContract));
    assert!(error.message.contains("Invalid WASM magic number"));
}

#[tokio::test]
async fn test_contract_storage_operations() {
    let runtime = ContractRuntime::new(1_000_000, 16).unwrap();
    let contract_address = "dyt1storage_test".to_string();

    // Deploy a simple contract
    let deployment = ContractDeployment {
        address: contract_address.clone(),
        code: vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00],
        initial_state: Vec::new(),
        gas_limit: 100_000,
        deployer: "dyt1deployer".to_string(),
        timestamp: 1234567890,
        ai_audit_score: Some(0.9),
    };

    runtime.deploy_contract(deployment).await.unwrap();

    // Test storage operations (this tests the internal storage, not WASM host functions)
    let key = b"test_key".to_vec();
    let _value = b"test_value".to_vec();

    // Initially, key should not exist
    let initial_value = runtime.get_contract_state(&contract_address, &key);
    assert!(initial_value.is_none());

    // After setting, we would need to test via host functions during contract execution
    // For now, we verify the storage structure is initialized
    let contracts = runtime.list_contracts();
    assert!(contracts.contains(&contract_address));
}

#[tokio::test]
async fn test_event_storage_and_retrieval() {
    let runtime = ContractRuntime::new(1_000_000, 16).unwrap();
    let contract_address = "dyt1event_test".to_string();

    // Deploy contract
    let deployment = ContractDeployment {
        address: contract_address.clone(),
        code: vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00],
        initial_state: Vec::new(),
        gas_limit: 100_000,
        deployer: "dyt1deployer".to_string(),
        timestamp: 1234567890,
        ai_audit_score: Some(0.9),
    };

    runtime.deploy_contract(deployment).await.unwrap();

    // Initially no events
    let events = runtime.get_events(Some(&contract_address));
    assert!(events.is_empty());

    let all_events = runtime.get_events(None);
    assert!(all_events.is_empty());

    // Events would be added during contract execution via host functions
    // The infrastructure is in place and tested via unit tests
}

#[tokio::test]
async fn test_concurrent_contract_operations() {
    use std::sync::Arc;
    use tokio::task;

    let runtime = Arc::new(ContractRuntime::new(1_000_000, 16).unwrap());
    let mut handles = vec![];

    // Deploy multiple contracts concurrently
    for i in 0..5 {
        let runtime_clone = Arc::clone(&runtime);
        let handle = task::spawn(async move {
            let deployment = ContractDeployment {
                address: format!("dyt1concurrent_test_{i}"),
                code: vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00],
                initial_state: Vec::new(),
                gas_limit: 100_000,
                deployer: format!("dyt1deployer_{i}"),
                timestamp: 1234567890 + i as u64,
                ai_audit_score: Some(0.9),
            };

            runtime_clone.deploy_contract(deployment).await
        });
        handles.push(handle);
    }

    // Wait for all deployments to complete
    let mut deployed_contracts = vec![];
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
        deployed_contracts.push(result.unwrap());
    }

    // Verify all contracts were deployed
    assert_eq!(deployed_contracts.len(), 5);
    let all_contracts = runtime.list_contracts();
    for contract_addr in &deployed_contracts {
        assert!(all_contracts.contains(contract_addr));
    }
}
