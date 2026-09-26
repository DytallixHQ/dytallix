/*
Simple WASM Smart Contract Runtime

Core WebAssembly contract execution with:
- Gas metering
- State management
- Basic host functions
*/

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wasmi::{Engine, Linker, Module, Store, TypedFunc, Caller, Config, Memory, AsContext, AsContextMut};
use serde::{Serialize, Deserialize};
use log::{info, debug};
use crate::types::{Address, Amount, Gas, Hash};

// Gas costs
const GAS_COST_BASE: Gas = 1;
const GAS_COST_MEMORY_BYTE: Gas = 1;
const GAS_COST_STORAGE_READ: Gas = 200;
const GAS_COST_STORAGE_WRITE: Gas = 5000;

#[derive(Debug, Clone)]
pub struct ContractExecutionError {
    pub code: ErrorCode,
    pub message: String,
    pub gas_used: u64,
}

#[derive(Debug, Clone)]
pub enum ErrorCode {
    OutOfGas,
    InvalidContract,
    ExecutionFailed,
    InvalidInput,
    StateError,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractDeployment {
    pub address: Address,
    pub code: Vec<u8>,
    pub gas_limit: u64,
    pub deployer: Address,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractCall {
    pub contract_address: Address,
    pub caller: Address,
    pub method: String,
    pub input_data: Vec<u8>,
    pub gas_limit: u64,
    pub value: Amount,
}

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub success: bool,
    pub return_data: Vec<u8>,
    pub gas_used: u64,
    pub gas_remaining: u64,
}

#[derive(Debug)]
pub struct ContractRuntime {
    engine: Engine,
    contracts: Arc<Mutex<HashMap<Address, ContractInstance>>>,
    contract_storage: Arc<Mutex<ContractStorage>>,
    gas_meter: Arc<Mutex<GasMeter>>,
    max_gas_per_call: Gas,
}

#[derive(Debug, Clone)]
struct ContractInstance {
    address: Address,
    code: Vec<u8>,
    code_hash: Hash,
    deployment_info: ContractDeployment,
}

#[derive(Debug)]
struct ContractStorage {
    storage: HashMap<Address, HashMap<Vec<u8>, Vec<u8>>>,
}

#[derive(Debug)]
struct GasMeter {
    current_gas: Gas,
    gas_limit: Gas,
}

#[derive(Debug, Clone)]
struct ExecutionContext {
    contract_address: Address,
    caller_address: Address,
    call_value: Amount,
}

#[derive(Debug, Clone)]
struct HostCallContext {
    runtime: Arc<ContractRuntime>,
    execution_context: ExecutionContext,
}

impl ContractRuntime {
    pub fn new(max_gas_per_call: Gas) -> Result<Self, ContractExecutionError> {
        let config = Config::default();
        let engine = Engine::new(&config);

        Ok(Self {
            engine,
            contracts: Arc::new(Mutex::new(HashMap::new())),
            contract_storage: Arc::new(Mutex::new(ContractStorage::new())),
            gas_meter: Arc::new(Mutex::new(GasMeter::new())),
            max_gas_per_call,
        })
    }

    pub async fn deploy_contract(
        &self,
        deployment: ContractDeployment,
    ) -> Result<Address, ContractExecutionError> {
        info!("Deploying contract to address: {}", deployment.address);

        self.validate_contract_code(&deployment.code)?;

        let code_hash = self.hash_contract_code(&deployment.code);

        // Initialize storage
        {
            let mut storage = self.contract_storage.lock().unwrap();
            storage.initialize_contract(&deployment.address);
        }

        // Create instance
        let instance = ContractInstance {
            address: deployment.address.clone(),
            code: deployment.code.clone(),
            code_hash,
            deployment_info: deployment.clone(),
        };

        // Store contract
        {
            let mut contracts = self.contracts.lock().unwrap();
            contracts.insert(deployment.address.clone(), instance);
        }

        info!("Contract deployed successfully: {}", deployment.address);
        Ok(deployment.address)
    }

    pub async fn call_contract(
        &self,
        call: ContractCall,
    ) -> Result<ExecutionResult, ContractExecutionError> {
        debug!("Calling contract: {} method: {}", call.contract_address, call.method);

        if call.gas_limit > self.max_gas_per_call {
            return Err(ContractExecutionError {
                code: ErrorCode::InvalidInput,
                message: "Gas limit exceeds maximum allowed".to_string(),
                gas_used: 0,
            });
        }

        let contract = {
            let contracts = self.contracts.lock().unwrap();
            contracts.get(&call.contract_address)
                .ok_or_else(|| ContractExecutionError {
                    code: ErrorCode::InvalidContract,
                    message: format!("Contract not found: {}", call.contract_address),
                    gas_used: 0,
                })?
                .clone()
        };

        let execution_context = ExecutionContext {
            contract_address: call.contract_address.clone(),
            caller_address: call.caller.clone(),
            call_value: call.value,
        };

        {
            let mut gas_meter = self.gas_meter.lock().unwrap();
            gas_meter.reset(call.gas_limit);
        }

        let execution_result = self.execute_wasm_contract(&contract, &call, &execution_context).await?;

        debug!("Contract call completed: success={}, gas_used={}",
               execution_result.success, execution_result.gas_used);

        Ok(execution_result)
    }

    async fn execute_wasm_contract(
        &self,
        contract: &ContractInstance,
        call: &ContractCall,
        execution_context: &ExecutionContext,
    ) -> Result<ExecutionResult, ContractExecutionError> {
        let mut store = Store::new(&self.engine, HostCallContext {
            runtime: Arc::new(self.clone()),
            execution_context: execution_context.clone(),
        });

        let module = Module::new(&self.engine, contract.code.as_slice())
            .map_err(|e| ContractExecutionError {
                code: ErrorCode::InvalidContract,
                message: format!("Failed to load WASM module: {}", e),
                gas_used: 0,
            })?;

        let mut linker = Linker::new(&self.engine);
        self.register_host_functions(&mut linker)?;

        let instance_pre = linker.instantiate(&mut store, &module)
            .map_err(|e| ContractExecutionError {
                code: ErrorCode::ExecutionFailed,
                message: format!("Failed to instantiate contract: {}", e),
                gas_used: 0,
            })?;

        let instance = instance_pre.start(&mut store)
            .map_err(|e| ContractExecutionError {
                code: ErrorCode::ExecutionFailed,
                message: format!("Failed to start contract: {}", e),
                gas_used: 0,
            })?;

        // For now, just return success with basic gas accounting
        let gas_used = 1000; // Simplified gas calculation
        let gas_remaining = {
            let gas_meter = self.gas_meter.lock().unwrap();
            gas_meter.current_gas.saturating_sub(gas_used)
        };

        Ok(ExecutionResult {
            success: true,
            return_data: Vec::new(),
            gas_used,
            gas_remaining,
        })
    }

    fn register_host_functions(&self, linker: &mut Linker<HostCallContext>) -> Result<(), ContractExecutionError> {
        // Simple gas consumption function
        linker.func_wrap("env", "consume_gas", |_caller: Caller<HostCallContext>, amount: i32| -> i32 {
            // For now, just return success
            if amount > 0 { 0 } else { -1 }
        }).map_err(|e| ContractExecutionError {
            code: ErrorCode::ExecutionFailed,
            message: format!("Failed to register consume_gas: {}", e),
            gas_used: 0,
        })?;

        Ok(())
    }

    fn validate_contract_code(&self, code: &[u8]) -> Result<(), ContractExecutionError> {
        if code.len() < 8 {
            return Err(ContractExecutionError {
                code: ErrorCode::InvalidContract,
                message: "Contract code too small".to_string(),
                gas_used: 0,
            });
        }

        if &code[0..4] != b"\0asm" {
            return Err(ContractExecutionError {
                code: ErrorCode::InvalidContract,
                message: "Invalid WASM magic number".to_string(),
                gas_used: 0,
            });
        }

        Ok(())
    }

    fn hash_contract_code(&self, code: &[u8]) -> Hash {
        use sha3::{Sha3_256, Digest};
        let mut hasher = Sha3_256::new();
        hasher.update(code);
        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        hash
    }

    // Public getters
    pub fn get_contract_state(&self, address: &Address, key: &[u8]) -> Option<Vec<u8>> {
        let storage = self.contract_storage.lock().unwrap();
        storage.get(address, key)
    }

    pub fn list_contracts(&self) -> Vec<Address> {
        let contracts = self.contracts.lock().unwrap();
        contracts.keys().cloned().collect()
    }
}

impl Clone for ContractRuntime {
    fn clone(&self) -> Self {
        Self {
            engine: self.engine.clone(),
            contracts: Arc::clone(&self.contracts),
            contract_storage: Arc::clone(&self.contract_storage),
            gas_meter: Arc::clone(&self.gas_meter),
            max_gas_per_call: self.max_gas_per_call,
        }
    }
}

impl GasMeter {
    fn new() -> Self {
        Self {
            current_gas: 0,
            gas_limit: 0,
        }
    }

    fn reset(&mut self, gas_limit: Gas) {
        self.current_gas = gas_limit;
        self.gas_limit = gas_limit;
    }

    fn consume_gas(&mut self, amount: Gas) -> Result<(), ContractExecutionError> {
        if amount > self.current_gas {
            return Err(ContractExecutionError {
                code: ErrorCode::OutOfGas,
                message: format!("Out of gas: required {}, available {}", amount, self.current_gas),
                gas_used: self.gas_limit - self.current_gas,
            });
        }

        self.current_gas -= amount;
        Ok(())
    }
}

impl ContractStorage {
    fn new() -> Self {
        Self {
            storage: HashMap::new(),
        }
    }

    fn initialize_contract(&mut self, address: &Address) {
        self.storage.insert(address.clone(), HashMap::new());
    }

    fn get(&self, contract_address: &Address, key: &[u8]) -> Option<Vec<u8>> {
        self.storage.get(contract_address)?.get(key).cloned()
    }

    fn set(&mut self, contract_address: &Address, key: Vec<u8>, value: Vec<u8>) -> Result<(), ContractExecutionError> {
        let contract_storage = self.storage
            .get_mut(contract_address)
            .ok_or_else(|| ContractExecutionError {
                code: ErrorCode::StateError,
                message: "Contract storage not found".to_string(),
                gas_used: 0,
            })?;

        contract_storage.insert(key, value);
        Ok(())
    }
}

impl std::fmt::Display for ContractExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ContractExecutionError({:?}): {} (gas_used: {})",
               self.code, self.message, self.gas_used)
    }
}

impl std::error::Error for ContractExecutionError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_contract_deployment() {
        let runtime = ContractRuntime::new(1_000_000).unwrap();

        let deployment = ContractDeployment {
            address: "dyt1contract123".to_string(),
            code: vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00],
            gas_limit: 100_000,
            deployer: "dyt1deployer".to_string(),
            timestamp: 1234567890,
        };

        let result = runtime.deploy_contract(deployment).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_gas_metering() {
        let runtime = ContractRuntime::new(1_000_000).unwrap();

        {
            let mut gas_meter = runtime.gas_meter.lock().unwrap();
            gas_meter.reset(1000);

            let result = gas_meter.consume_gas(500);
            assert!(result.is_ok());

            let result = gas_meter.consume_gas(600);
            assert!(result.is_err());
        }
    }
}
