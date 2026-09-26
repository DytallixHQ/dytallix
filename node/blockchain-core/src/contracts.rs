/*
Real WASM Smart Contract Runtime Integration

Replaces stub implementations with actual WASM runtime integration.
Provides proper contract deployment, instantiation, and execution
through the dytallix-contracts runtime.
*/

use anyhow::Result; // retain Result only
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

// Re-export types from dytallix-contracts
pub use dytallix_contracts::runtime::ContractRuntime as WasmRuntime;

/// Blockchain-integrated contract runtime wrapper
#[derive(Debug, Clone)]
pub struct ContractRuntime {
    inner: Arc<WasmRuntime>,
}

impl ContractRuntime {
    pub fn new(max_gas_per_call: u64, max_memory_pages: u64) -> Result<Self, String> {
        match WasmRuntime::new(max_gas_per_call, max_memory_pages as u32) {
            Ok(runtime) => Ok(Self {
                inner: Arc::new(runtime),
            }),
            Err(e) => Err(format!("Failed to create contract runtime: {e}")),
        }
    }

    pub async fn deploy_contract(&self, deployment: ContractDeployment) -> Result<String, String> {
        info!(
            "Deploying contract with {} bytes of code",
            deployment.code.len()
        );

        // Convert to dytallix-contracts types
        let wasm_deployment = dytallix_contracts::runtime::ContractDeployment {
            address: deployment.address.clone(),
            code: deployment.code,
            initial_state: deployment.initial_state,
            gas_limit: deployment.gas_limit,
            deployer: deployment.deployer,
            timestamp: deployment.timestamp,
            ai_audit_score: deployment.ai_audit_score,
        };

        match self.inner.deploy_contract(wasm_deployment).await {
            Ok(address) => {
                info!("Contract deployed successfully to address: {address}");
                Ok(address)
            }
            Err(e) => {
                error!("Contract deployment failed: {e:?}");
                Err(format!("Deployment failed: {}", e.message))
            }
        }
    }

    pub async fn call_contract(&self, call: ContractCall) -> Result<ExecutionResult, String> {
        info!(
            "Executing contract {} method: {}",
            call.contract_address, call.method
        );

        // Convert to dytallix-contracts types
        let wasm_call = dytallix_contracts::runtime::ContractCall {
            contract_address: call.contract_address.clone(),
            caller: call.caller.clone(),
            method: call.method.clone(),
            input_data: call.input_data,
            gas_limit: call.gas_limit,
            value: call.value as u128, // cast to expected u128
            timestamp: call.timestamp,
        };

        match self.inner.call_contract(wasm_call).await {
            Ok(result) => {
                info!(
                    "Contract execution completed. Gas used: {}",
                    result.gas_used
                );

                // Convert events to JSON values
                let events: Vec<serde_json::Value> = result
                    .events
                    .iter()
                    .map(|event| {
                        serde_json::json!({
                            "contract_address": event.contract_address,
                            "topic": event.topic,
                            "data": event.data,
                            "timestamp": event.timestamp,
                        })
                    })
                    .collect();

                // Transform state changes (best-effort: assumes fields key & value)
                let mut state_changes_map: HashMap<String, serde_json::Value> = HashMap::new();
                for sc in &result.state_changes {
                    // Convert binary key to hex for JSON readability
                    let key_hex = hex::encode(&sc.key);
                    let entry = serde_json::json!({
                        "old": sc.old_value.as_ref().map(hex::encode),
                        "new": hex::encode(&sc.new_value)
                    });
                    state_changes_map.insert(key_hex, entry);
                }

                Ok(ExecutionResult {
                    success: result.success,
                    return_value: result.return_data.clone(),
                    return_data: result.return_data,
                    gas_used: result.gas_used,
                    logs: Vec::new(), // upstream result has no logs field
                    events,
                    state_changes: state_changes_map,
                })
            }
            Err(e) => {
                warn!("Contract execution failed: {e:?}");
                Ok(ExecutionResult {
                    success: false,
                    return_value: Vec::new(),
                    return_data: Vec::new(),
                    gas_used: e.gas_used,
                    logs: vec![format!("Error: {}", e.message)],
                    events: Vec::new(),
                    state_changes: HashMap::new(),
                })
            }
        }
    }

    pub fn get_contract_info(&self, address: &str) -> Option<ContractInfo> {
        let addr = address.to_string();
        if let Some(deployment) = self.inner.get_contract_info(&addr) {
            Some(ContractInfo {
                address: deployment.address,
                code_hash: hex::encode(blake3::hash(&deployment.code).as_bytes()),
                deployer: deployment.deployer,
                gas_limit: deployment.gas_limit,
                timestamp: deployment.timestamp,
                ai_audit_score: deployment.ai_audit_score,
            })
        } else {
            None
        }
    }

    pub fn get_contract_storage(&self, address: &str, key: &[u8]) -> Option<Vec<u8>> {
        let addr = address.to_string();
        self.inner.get_contract_state(&addr, key)
    }

    pub fn list_contracts(&self) -> Vec<String> {
        self.inner.list_contracts()
    }
}

/// Contract deployment request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractDeployment {
    pub code: Vec<u8>,
    pub metadata: serde_json::Value,
    pub deployer: String,
    pub gas_limit: u64,
    pub address: String,
    pub initial_state: Vec<u8>,
    pub timestamp: u64,
    pub ai_audit_score: Option<f64>,
}

/// Contract function call request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractCall {
    pub contract_id: String,
    pub function: String,
    pub args: serde_json::Value,
    pub caller: String,
    pub gas_limit: u64,
    pub contract_address: String,
    pub method: String,
    pub input_data: Vec<u8>,
    pub value: u64,
    pub timestamp: u64,
}

/// Contract execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub success: bool,
    pub return_value: Vec<u8>,
    pub return_data: Vec<u8>,
    pub gas_used: u64,
    pub logs: Vec<String>,
    pub events: Vec<serde_json::Value>,
    pub state_changes: HashMap<String, serde_json::Value>,
}

/// Contract information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractInfo {
    pub address: String,
    pub code_hash: String,
    pub deployer: String,
    pub gas_limit: u64,
    pub timestamp: u64,
    pub ai_audit_score: Option<f64>,
}
