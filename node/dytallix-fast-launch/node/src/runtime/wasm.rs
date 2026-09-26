/*
WASM Runtime Integration

Provides a minimal WASM runtime with gas metering and deterministic execution.
This module integrates the blockchain-core WASM engine with the node's runtime
to provide contract deployment and execution capabilities.
*/

use crate::gas::GasMeter;
use anyhow::{anyhow, Result};
use dytallix_node::wasm::{host_env::HostEnv, WasmEngine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

// Type aliases to match existing codebase
pub type Address = String;
pub type Hash = String;
pub type TxHash = String;

// Factor complex types to reduce clippy type_complexity
pub type ContractStateKey = (Address, String);
pub type ContractStateMap = HashMap<ContractStateKey, Vec<u8>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractDeployment {
    pub address: Address,
    pub code_hash: Hash,
    pub code: Vec<u8>,
    pub tx_hash: TxHash,
    pub gas_used: u64,
    pub deployed_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractExecution {
    pub contract_address: Address,
    pub method: String,
    pub args: Vec<u8>,
    pub result: Vec<u8>,
    pub gas_used: u64,
    pub tx_hash: TxHash,
    pub executed_at: u64,
    pub logs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractState {
    pub contract_address: Address,
    pub key: String,
    pub value: Vec<u8>,
}

/// Minimal WASM runtime for contract deployment and execution
#[derive(Debug)]
pub struct WasmRuntime {
    engine: WasmEngine,
    deployed_contracts: Arc<Mutex<HashMap<Address, ContractDeployment>>>,
    contract_state: Arc<Mutex<ContractStateMap>>,
    execution_history: Arc<Mutex<Vec<ContractExecution>>>,
}

impl Default for WasmRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl WasmRuntime {
    pub fn new() -> Self {
        // Updated HostEnv construction requires PQCManager from core
        let pqc = dytallix_node::crypto::PQCManager::new().expect("PQCManager");
        let host_env = HostEnv::with_pqc(Arc::new(pqc));
        let engine = WasmEngine::new_with_env(host_env);

        Self {
            engine,
            deployed_contracts: Arc::new(Mutex::new(HashMap::new())),
            contract_state: Arc::new(Mutex::new(HashMap::new())),
            execution_history: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Deploy a WASM contract
    pub fn deploy_contract(
        &self,
        wasm_bytes: &[u8],
        from: &str,
        gas_limit: u64,
        initial_state: Option<&[u8]>,
    ) -> Result<ContractDeployment> {
        let mut gas_meter = GasMeter::new(gas_limit);

        // Charge gas for deployment
        gas_meter.consume(50000, "contract_deploy_base")?;
        gas_meter.consume(wasm_bytes.len() as u64, "contract_deploy_per_byte")?;

        // Validate WASM module by attempting to instantiate
        let (_store, _instance) = self
            .engine
            .instantiate_with_fuel(wasm_bytes, gas_limit)
            .map_err(|e| anyhow!("Failed to validate WASM module: {}", e))?;

        // Calculate code hash
        let mut hasher = Sha256::new();
        hasher.update(wasm_bytes);
        let code_hash = hex::encode(hasher.finalize());

        // Generate contract address from code hash and deployer
        let address = self.generate_contract_address(&code_hash, from);

        // Generate transaction hash
        let tx_hash = self.generate_tx_hash(&address, "deploy");

        let deployment = ContractDeployment {
            address: address.clone(),
            code_hash,
            code: wasm_bytes.to_vec(),
            tx_hash,
            gas_used: gas_meter.gas_used(),
            deployed_at: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        };

        // Store contract
        let mut contracts = self.deployed_contracts.lock().unwrap();
        contracts.insert(address.clone(), deployment.clone());

        // Initialize contract state if provided
        if let Some(state_data) = initial_state {
            let mut state = self.contract_state.lock().unwrap();
            state.insert((address, "init".to_string()), state_data.to_vec());
        }

        Ok(deployment)
    }

    /// Execute a contract method
    pub fn execute_contract(
        &self,
        contract_address: &Address,
        method: &str,
        args: &[u8],
        gas_limit: u64,
    ) -> Result<ContractExecution> {
        self.execute_contract_internal(contract_address, method, args, gas_limit, true)
    }

    /// Execute a read-only contract query without recording an event.
    pub fn query_contract(
        &self,
        contract_address: &Address,
        method: &str,
        args: &[u8],
        gas_limit: u64,
    ) -> Result<ContractExecution> {
        self.execute_contract_internal(contract_address, method, args, gas_limit, false)
    }

    fn execute_contract_internal(
        &self,
        contract_address: &Address,
        method: &str,
        args: &[u8],
        gas_limit: u64,
        record_history: bool,
    ) -> Result<ContractExecution> {
        let mut gas_meter = GasMeter::new(gas_limit);

        // Charge base execution gas
        gas_meter.consume(25000, "contract_execute_base")?;

        // Get deployed contract
        let contracts = self.deployed_contracts.lock().unwrap();
        let contract = contracts
            .get(contract_address)
            .ok_or_else(|| anyhow!("Contract not found: {}", contract_address))?;

        // Instantiate contract with fuel
        let (mut store, instance) = self
            .engine
            .instantiate_with_fuel(&contract.code, gas_limit)
            .map_err(|e| anyhow!("Failed to instantiate contract: {}", e))?;

        // Set input in context
        let mut ctx = self.engine.env().context();
        ctx.input = args.to_vec();
        // Update context with current block info if available (placeholder for now)
        ctx.caller = "caller_placeholder".to_string();
        self.engine.set_context(ctx);

        // Execute the method
        let func = instance
            .get_func(&mut store, method)
            .ok_or_else(|| anyhow!("Method not found: {}", method))?;

        // Call the function (expecting 0 args, 0 results for void/standard ABI, or flexible)
        // We support functions returning nothing (void) or i32/i64 (status)
        let ty = func.ty(&store);
        let mut results = vec![wasmtime::Val::I32(0); ty.results().len()];

        if let Err(e) = func.call(&mut store, &[], &mut results) {
            let logs = self.engine.env().take_logs();
            return Err(anyhow!(
                "WASM execution failed: {}\nLogs:\n{}",
                e,
                logs.join("\n")
            ));
        }

        // Retrieve output from env
        let result = self.engine.env().take_output();

        let gas_used = gas_meter.gas_used();
        let tx_hash = self.generate_tx_hash(contract_address, method);

        let execution = ContractExecution {
            contract_address: contract_address.clone(),
            method: method.to_string(),
            args: args.to_vec(),
            result,
            gas_used,
            tx_hash,
            executed_at: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            logs: self.engine.env().take_logs(),
        };

        if record_history {
            let mut history = self.execution_history.lock().unwrap();
            history.push(execution.clone());
        }

        Ok(execution)
    }

    /// Get contract state by key
    pub fn get_contract_state(&self, contract_address: &Address, key: &str) -> Option<Vec<u8>> {
        let state = self.contract_state.lock().unwrap();
        state
            .get(&(contract_address.clone(), key.to_string()))
            .cloned()
    }

    /// Generate contract address from code hash and deployer
    fn generate_contract_address(&self, code_hash: &str, from: &str) -> Address {
        let mut hasher = Sha256::new();
        hasher.update(code_hash.as_bytes());
        hasher.update(from.as_bytes());
        hasher.update(b"contract");
        format!("0x{}", hex::encode(&hasher.finalize()[..20]))
    }

    /// Generate transaction hash
    fn generate_tx_hash(&self, address: &str, operation: &str) -> TxHash {
        let mut hasher = Sha256::new();
        hasher.update(address.as_bytes());
        hasher.update(operation.as_bytes());
        hasher.update(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                .to_le_bytes(),
        );
        format!("0x{}", hex::encode(&hasher.finalize()[..32]))
    }

    /// List all deployed contracts
    pub fn list_contracts(&self) -> Vec<ContractDeployment> {
        let contracts = self.deployed_contracts.lock().unwrap();
        contracts.values().cloned().collect()
    }

    /// Get contract deployment info
    pub fn get_contract(&self, address: &Address) -> Option<ContractDeployment> {
        let contracts = self.deployed_contracts.lock().unwrap();
        contracts.get(address).cloned()
    }

    /// Get execution history for a contract
    pub fn get_execution_history(
        &self,
        contract_address: Option<&Address>,
    ) -> Vec<ContractExecution> {
        let history = self.execution_history.lock().unwrap();
        match contract_address {
            Some(addr) => history
                .iter()
                .filter(|exec| &exec.contract_address == addr)
                .cloned()
                .collect(),
            None => history.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wasm_runtime_creation() {
        let runtime = WasmRuntime::new();
        assert!(runtime.list_contracts().is_empty());
    }

    #[test]
    fn test_contract_address_generation() {
        let runtime = WasmRuntime::new();
        let addr1 = runtime.generate_contract_address("hash1", "deployer1");
        let addr2 = runtime.generate_contract_address("hash1", "deployer2");
        let addr3 = runtime.generate_contract_address("hash2", "deployer1");

        assert_ne!(addr1, addr2);
        assert_ne!(addr1, addr3);
        assert_ne!(addr2, addr3);
    }
}
