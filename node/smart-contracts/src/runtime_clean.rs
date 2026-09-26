/*
WASM Smart Contract Execution Engine

Production-ready WebAssembly contract runtime with:
- Secure sandboxed execution
- Gas metering and limits
- State management
- AI integration hooks
- Post-quantum cryptography support
*/

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wasmi::{Engine, Linker, Module, Store, TypedFunc, Caller, Config, Memory, MemoryType, Limits};
use serde::{Serialize, Deserialize};
use log::{info, debug, warn, error};
use crate::types::{Address, Amount, Gas, Hash};

// Gas costs for different operations
const GAS_COST_BASE: Gas = 1;
const GAS_COST_MEMORY_BYTE: Gas = 1;
const GAS_COST_STORAGE_READ: Gas = 200;
const GAS_COST_STORAGE_WRITE: Gas = 5000;
const GAS_COST_EVENT_EMIT: Gas = 375;
const GAS_COST_EXTERNAL_CALL: Gas = 700;

// Resource limits
const MAX_MEMORY_PAGES: u32 = 256; // 16MB max memory
const MAX_STORAGE_KEY_SIZE: usize = 128;
const MAX_STORAGE_VALUE_SIZE: usize = 16384; // 16KB
const MAX_EVENT_DATA_SIZE: usize = 1024;

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
    PermissionDenied,
    AIValidationFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractDeployment {
    pub address: Address,
    pub code: Vec<u8>,
    pub initial_state: Vec<u8>,
    pub gas_limit: u64,
    pub deployer: Address,
    pub timestamp: u64,
    pub ai_audit_score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractCall {
    pub contract_address: Address,
    pub caller: Address,
    pub method: String,
    pub input_data: Vec<u8>,
    pub gas_limit: u64,
    pub value: Amount,
    pub timestamp: u64,
}

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub success: bool,
    pub return_data: Vec<u8>,
    pub gas_used: u64,
    pub gas_remaining: u64,
    pub state_changes: Vec<StateChange>,
    pub events: Vec<ContractEvent>,
    pub ai_analysis: Option<AIAnalysisResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateChange {
    pub contract_address: Address,
    pub key: Vec<u8>,
    pub old_value: Option<Vec<u8>>,
    pub new_value: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractEvent {
    pub contract_address: Address,
    pub topic: String,
    pub data: Vec<u8>,
    pub timestamp: u64,
}

#[derive(Debug, Clone)]
pub struct AIAnalysisResult {
    pub security_score: f64,
    pub gas_efficiency: f64,
    pub compliance_flags: Vec<String>,
    pub risk_assessment: String,
}

pub struct ContractRuntime {
    engine: Engine,
    contracts: Arc<Mutex<HashMap<Address, ContractInstance>>>,
    contract_storage: Arc<Mutex<ContractStorage>>,
    gas_meter: Arc<Mutex<GasMeter>>,
    ai_analyzer: Option<Arc<dyn ContractAIAnalyzer + Send + Sync>>,
    max_gas_per_call: Gas,
    max_memory_pages: u32,
    execution_context: Arc<Mutex<Option<ExecutionContext>>>,
    state_tracker: Arc<Mutex<StateTracker>>,
    event_storage: Arc<Mutex<Vec<ContractEvent>>>,
}

#[derive(Debug, Clone)]
struct ContractInstance {
    address: Address,
    code: Vec<u8>,
    code_hash: Hash,
    deployment_info: ContractDeployment,
    call_count: u64,
    last_called: u64,
    memory_usage: u32,
}

struct ContractStorage {
    storage: HashMap<Address, HashMap<Vec<u8>, Vec<u8>>>,
    storage_usage: HashMap<Address, usize>,
}

struct GasMeter {
    current_gas: Gas,
    gas_limit: Gas,
    gas_price: u64,
    operations_count: u64,
}

#[derive(Debug, Clone)]
struct ExecutionContext {
    contract_address: Address,
    caller_address: Address,
    call_value: Amount,
    block_timestamp: u64,
    block_number: u64,
    gas_limit: Gas,
    memory_pages: u32,
    stack_depth: u32,
}

#[derive(Debug, Clone)]
struct HostCallContext {
    runtime: Arc<ContractRuntime>,
    execution_context: ExecutionContext,
}

#[derive(Debug)]
struct StateTracker {
    changes: Vec<StateChange>,
    pending_events: Vec<ContractEvent>,
}

pub trait ContractAIAnalyzer {
    fn analyze_deployment(&self, contract: &ContractDeployment) -> Result<AIAnalysisResult, String>;
    fn analyze_execution(&self, call: &ContractCall, result: &ExecutionResult) -> Result<AIAnalysisResult, String>;
    fn validate_state_change(&self, change: &StateChange) -> Result<bool, String>;
}

impl ContractRuntime {
    pub fn new(max_gas_per_call: Gas, max_memory_pages: u32) -> Result<Self, ContractExecutionError> {
        let mut config = Config::default();
        config.wasm_multi_memory(false);
        config.wasm_bulk_memory(true);
        config.wasm_reference_types(false);
        config.wasm_simd(false);
        config.wasm_threads(false);
        config.wasm_tail_call(false);

        let engine = Engine::new(&config);

        Ok(Self {
            engine,
            contracts: Arc::new(Mutex::new(HashMap::new())),
            contract_storage: Arc::new(Mutex::new(ContractStorage::new())),
            gas_meter: Arc::new(Mutex::new(GasMeter::new())),
            ai_analyzer: None,
            max_gas_per_call,
            max_memory_pages: max_memory_pages.min(MAX_MEMORY_PAGES),
            execution_context: Arc::new(Mutex::new(None)),
            state_tracker: Arc::new(Mutex::new(StateTracker::new())),
            event_storage: Arc::new(Mutex::new(Vec::new())),
        })
    }

    pub fn set_ai_analyzer(&mut self, analyzer: Arc<dyn ContractAIAnalyzer + Send + Sync>) {
        self.ai_analyzer = Some(analyzer);
    }

    pub async fn deploy_contract(
        &self,
        deployment: ContractDeployment,
    ) -> Result<Address, ContractExecutionError> {
        info!("Deploying contract to address: {}", deployment.address);

        self.validate_contract_code(&deployment.code)?;

        if deployment.gas_limit > self.max_gas_per_call {
            return Err(ContractExecutionError {
                code: ErrorCode::InvalidInput,
                message: "Gas limit exceeds maximum allowed".to_string(),
                gas_used: 0,
            });
        }

        let code_hash = self.hash_contract_code(&deployment.code);

        // AI analysis
        if let Some(analyzer) = &self.ai_analyzer {
            match analyzer.analyze_deployment(&deployment) {
                Ok(analysis) => {
                    if analysis.security_score < 0.7 {
                        return Err(ContractExecutionError {
                            code: ErrorCode::AIValidationFailed,
                            message: format!("AI security score too low: {}", analysis.security_score),
                            gas_used: 0,
                        });
                    }
                }
                Err(e) => {
                    return Err(ContractExecutionError {
                        code: ErrorCode::AIValidationFailed,
                        message: format!("AI analysis failed: {}", e),
                        gas_used: 0,
                    });
                }
            }
        }

        // Initialize storage
        {
            let mut storage = self.contract_storage.lock().unwrap();
            storage.initialize_contract(&deployment.address, &deployment.initial_state)?;
        }

        // Create instance
        let instance = ContractInstance {
            address: deployment.address.clone(),
            code: deployment.code.clone(),
            code_hash,
            deployment_info: deployment.clone(),
            call_count: 0,
            last_called: deployment.timestamp,
            memory_usage: 0,
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
            block_timestamp: call.timestamp,
            block_number: call.timestamp / 12,
            gas_limit: call.gas_limit,
            memory_pages: 0,
            stack_depth: 0,
        };

        {
            let mut context = self.execution_context.lock().unwrap();
            *context = Some(execution_context.clone());
        }

        {
            let mut gas_meter = self.gas_meter.lock().unwrap();
            gas_meter.reset(call.gas_limit);
        }

        // Clear state tracker
        {
            let mut tracker = self.state_tracker.lock().unwrap();
            tracker.clear();
        }

        let execution_result = self.execute_wasm_contract(&contract, &call, &execution_context).await?;

        {
            let mut context = self.execution_context.lock().unwrap();
            *context = None;
        }

        if execution_result.success {
            let mut contracts = self.contracts.lock().unwrap();
            if let Some(contract) = contracts.get_mut(&call.contract_address) {
                contract.call_count += 1;
                contract.last_called = call.timestamp;
            }
        }

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

        let module = Module::new(&self.engine, &contract.code)
            .map_err(|e| ContractExecutionError {
                code: ErrorCode::InvalidContract,
                message: format!("Failed to load WASM module: {}", e),
                gas_used: 0,
            })?;

        let mut linker = Linker::new(&self.engine);
        self.register_host_functions(&mut linker)?;

        let instance = linker.instantiate(&mut store, &module)
            .map_err(|e| ContractExecutionError {
                code: ErrorCode::ExecutionFailed,
                message: format!("Failed to instantiate contract: {}", e),
                gas_used: 0,
            })?;

        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| ContractExecutionError {
                code: ErrorCode::ExecutionFailed,
                message: "Contract memory not found".to_string(),
                gas_used: 0,
            })?;

        let func_name = format!("contract_{}", call.method);
        let func: TypedFunc<(i32, i32), i32> = instance
            .get_typed_func(&mut store, &func_name)
            .map_err(|e| ContractExecutionError {
                code: ErrorCode::ExecutionFailed,
                message: format!("Function '{}' not found: {}", func_name, e),
                gas_used: 0,
            })?;

        let input_ptr = self.allocate_memory(&mut store, &memory, &call.input_data)?;
        let input_len = call.input_data.len() as i32;

        {
            let mut gas_meter = self.gas_meter.lock().unwrap();
            gas_meter.consume_gas(GAS_COST_BASE * 100)?;
        }

        let gas_before = {
            let gas_meter = self.gas_meter.lock().unwrap();
            gas_meter.current_gas
        };

        let result = func.call(&mut store, (input_ptr, input_len))
            .map_err(|e| {
                let gas_after = {
                    let gas_meter = self.gas_meter.lock().unwrap();
                    gas_meter.current_gas
                };
                ContractExecutionError {
                    code: ErrorCode::ExecutionFailed,
                    message: format!("Contract execution failed: {}", e),
                    gas_used: gas_before - gas_after,
                }
            })?;

        let gas_after = {
            let gas_meter = self.gas_meter.lock().unwrap();
            gas_meter.current_gas
        };

        let gas_used = gas_before - gas_after;

        let return_data = if result > 0 {
            self.read_memory(&mut store, &memory, result, 1024)?
        } else {
            Vec::new()
        };

        let state_changes = self.collect_state_changes(&call.contract_address)?;
        let events = self.collect_events(&call.contract_address)?;

        Ok(ExecutionResult {
            success: true,
            return_data,
            gas_used,
            gas_remaining: gas_after,
            state_changes,
            events,
            ai_analysis: None,
        })
    }

    fn register_host_functions(&self, linker: &mut Linker<HostCallContext>) -> Result<(), ContractExecutionError> {
        // Gas consumption
        linker.func_wrap("env", "consume_gas", |caller: Caller<HostCallContext>, amount: i32| -> Result<(), wasmi::Error> {
            let gas_amount = amount as Gas;
            let runtime = &caller.data().runtime;

            let mut gas_meter = runtime.gas_meter.lock().unwrap();
            gas_meter.consume_gas(gas_amount)
                .map_err(|_| wasmi::Error::new("Out of gas"))?;

            Ok(())
        }).map_err(|e| ContractExecutionError {
            code: ErrorCode::ExecutionFailed,
            message: format!("Failed to register consume_gas: {}", e),
            gas_used: 0,
        })?;

        // Storage read
        linker.func_wrap("env", "storage_get",
            |caller: Caller<HostCallContext>, key_ptr: i32, key_len: i32, value_ptr: i32, value_len: i32| -> i32 {
                let runtime = &caller.data().runtime;
                let context = &caller.data().execution_context;

                {
                    let mut gas_meter = runtime.gas_meter.lock().unwrap();
                    if gas_meter.consume_gas(GAS_COST_STORAGE_READ).is_err() {
                        return -1;
                    }
                }

                let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                    Some(mem) => mem,
                    None => return -2,
                };

                if key_len as usize > MAX_STORAGE_KEY_SIZE {
                    return -3;
                }

                let key = match runtime.read_memory_slice(&caller, &memory, key_ptr, key_len as usize) {
                    Ok(k) => k,
                    Err(_) => return -4,
                };

                let storage = runtime.contract_storage.lock().unwrap();
                match storage.get(&context.contract_address, &key) {
                    Some(value) => {
                        let copy_len = std::cmp::min(value.len(), value_len as usize);
                        if runtime.write_memory_slice(&caller, &memory, value_ptr, &value[..copy_len]).is_ok() {
                            copy_len as i32
                        } else {
                            -5
                        }
                    }
                    None => 0,
                }
            }
        ).map_err(|e| ContractExecutionError {
            code: ErrorCode::ExecutionFailed,
            message: format!("Failed to register storage_get: {}", e),
            gas_used: 0,
        })?;

        // Storage write
        linker.func_wrap("env", "storage_set",
            |caller: Caller<HostCallContext>, key_ptr: i32, key_len: i32, value_ptr: i32, value_len: i32| -> i32 {
                let runtime = &caller.data().runtime;
                let context = &caller.data().execution_context;

                if key_len as usize > MAX_STORAGE_KEY_SIZE || value_len as usize > MAX_STORAGE_VALUE_SIZE {
                    return -1;
                }

                {
                    let mut gas_meter = runtime.gas_meter.lock().unwrap();
                    if gas_meter.consume_gas(GAS_COST_STORAGE_WRITE).is_err() {
                        return -2;
                    }
                }

                let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                    Some(mem) => mem,
                    None => return -3,
                };

                let key = match runtime.read_memory_slice(&caller, &memory, key_ptr, key_len as usize) {
                    Ok(k) => k,
                    Err(_) => return -4,
                };

                let value = match runtime.read_memory_slice(&caller, &memory, value_ptr, value_len as usize) {
                    Ok(v) => v,
                    Err(_) => return -5,
                };

                // Track state change
                {
                    let old_value = {
                        let storage = runtime.contract_storage.lock().unwrap();
                        storage.get(&context.contract_address, &key)
                    };

                    let state_change = StateChange {
                        contract_address: context.contract_address.clone(),
                        key: key.clone(),
                        old_value,
                        new_value: value.clone(),
                    };

                    let mut tracker = runtime.state_tracker.lock().unwrap();
                    tracker.record_state_change(state_change);
                }

                let mut storage = runtime.contract_storage.lock().unwrap();
                match storage.set(&context.contract_address, key, value) {
                    Ok(_) => 0,
                    Err(_) => -6,
                }
            }
        ).map_err(|e| ContractExecutionError {
            code: ErrorCode::ExecutionFailed,
            message: format!("Failed to register storage_set: {}", e),
            gas_used: 0,
        })?;

        // Event emission
        linker.func_wrap("env", "emit_event",
            |caller: Caller<HostCallContext>, topic_ptr: i32, topic_len: i32, data_ptr: i32, data_len: i32| -> i32 {
                let runtime = &caller.data().runtime;
                let context = &caller.data().execution_context;

                if data_len as usize > MAX_EVENT_DATA_SIZE {
                    return -1;
                }

                {
                    let mut gas_meter = runtime.gas_meter.lock().unwrap();
                    if gas_meter.consume_gas(GAS_COST_EVENT_EMIT).is_err() {
                        return -2;
                    }
                }

                let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                    Some(mem) => mem,
                    None => return -3,
                };

                let topic = match runtime.read_memory_slice(&caller, &memory, topic_ptr, topic_len as usize) {
                    Ok(t) => String::from_utf8_lossy(&t).to_string(),
                    Err(_) => return -4,
                };

                let data = match runtime.read_memory_slice(&caller, &memory, data_ptr, data_len as usize) {
                    Ok(d) => d,
                    Err(_) => return -5,
                };

                let event = ContractEvent {
                    contract_address: context.contract_address.clone(),
                    topic,
                    data,
                    timestamp: context.block_timestamp,
                };

                {
                    let mut tracker = runtime.state_tracker.lock().unwrap();
                    tracker.emit_event(event.clone());
                }

                {
                    let mut events = runtime.event_storage.lock().unwrap();
                    events.push(event);
                }

                0
            }
        ).map_err(|e| ContractExecutionError {
            code: ErrorCode::ExecutionFailed,
            message: format!("Failed to register emit_event: {}", e),
            gas_used: 0,
        })?;

        // Block information
        linker.func_wrap("env", "block_timestamp", |caller: Caller<HostCallContext>| -> u64 {
            caller.data().execution_context.block_timestamp
        }).map_err(|e| ContractExecutionError {
            code: ErrorCode::ExecutionFailed,
            message: format!("Failed to register block_timestamp: {}", e),
            gas_used: 0,
        })?;

        linker.func_wrap("env", "block_number", |caller: Caller<HostCallContext>| -> u64 {
            caller.data().execution_context.block_number
        }).map_err(|e| ContractExecutionError {
            code: ErrorCode::ExecutionFailed,
            message: format!("Failed to register block_number: {}", e),
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

        if &code[4..8] != b"\x01\x00\x00\x00" {
            return Err(ContractExecutionError {
                code: ErrorCode::InvalidContract,
                message: "Unsupported WASM version".to_string(),
                gas_used: 0,
            });
        }

        match Module::new(&self.engine, code) {
            Ok(_) => Ok(()),
            Err(e) => Err(ContractExecutionError {
                code: ErrorCode::InvalidContract,
                message: format!("WASM validation failed: {}", e),
                gas_used: 0,
            })
        }
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

    fn allocate_memory(&self, store: &mut Store<HostCallContext>, memory: &Memory, data: &[u8]) -> Result<i32, ContractExecutionError> {
        let offset = 1024;

        if data.len() > 65536 {
            return Err(ContractExecutionError {
                code: ErrorCode::InvalidInput,
                message: "Input data too large".to_string(),
                gas_used: 0,
            });
        }

        {
            let mut gas_meter = self.gas_meter.lock().unwrap();
            gas_meter.consume_gas(GAS_COST_MEMORY_BYTE * data.len() as Gas)?;
        }

        memory.write(store, offset, data)
            .map_err(|e| ContractExecutionError {
                code: ErrorCode::ExecutionFailed,
                message: format!("Failed to write to memory: {}", e),
                gas_used: 0,
            })?;

        Ok(offset as i32)
    }

    fn read_memory(&self, store: &mut Store<HostCallContext>, memory: &Memory, ptr: i32, max_len: usize) -> Result<Vec<u8>, ContractExecutionError> {
        if ptr < 0 {
            return Ok(Vec::new());
        }

        let mut len_bytes = [0u8; 4];
        memory.read(store, ptr as usize, &mut len_bytes)
            .map_err(|e| ContractExecutionError {
                code: ErrorCode::ExecutionFailed,
                message: format!("Failed to read length from memory: {}", e),
                gas_used: 0,
            })?;

        let len = u32::from_le_bytes(len_bytes) as usize;

        if len == 0 {
            return Ok(Vec::new());
        }

        if len > max_len {
            return Err(ContractExecutionError {
                code: ErrorCode::ExecutionFailed,
                message: "Return data too large".to_string(),
                gas_used: 0,
            });
        }

        {
            let mut gas_meter = self.gas_meter.lock().unwrap();
            gas_meter.consume_gas(GAS_COST_MEMORY_BYTE * len as Gas)?;
        }

        let mut data = vec![0u8; len];
        memory.read(store, (ptr as usize) + 4, &mut data)
            .map_err(|e| ContractExecutionError {
                code: ErrorCode::ExecutionFailed,
                message: format!("Failed to read data from memory: {}", e),
                gas_used: 0,
            })?;

        Ok(data)
    }

    fn read_memory_slice(&self, caller: &Caller<HostCallContext>, memory: &Memory, ptr: i32, len: usize) -> Result<Vec<u8>, ContractExecutionError> {
        if ptr < 0 || len == 0 {
            return Ok(Vec::new());
        }

        let mut data = vec![0u8; len];
        memory.read(caller.as_context(), ptr as usize, &mut data)
            .map_err(|e| ContractExecutionError {
                code: ErrorCode::ExecutionFailed,
                message: format!("Failed to read memory slice: {}", e),
                gas_used: 0,
            })?;

        Ok(data)
    }

    fn write_memory_slice(&self, caller: &Caller<HostCallContext>, memory: &Memory, ptr: i32, data: &[u8]) -> Result<(), ContractExecutionError> {
        if ptr < 0 {
            return Err(ContractExecutionError {
                code: ErrorCode::ExecutionFailed,
                message: "Invalid memory pointer".to_string(),
                gas_used: 0,
            });
        }

        memory.write(caller.as_context_mut(), ptr as usize, data)
            .map_err(|e| ContractExecutionError {
                code: ErrorCode::ExecutionFailed,
                message: format!("Failed to write memory slice: {}", e),
                gas_used: 0,
            })?;

        Ok(())
    }

    fn collect_state_changes(&self, _contract_address: &Address) -> Result<Vec<StateChange>, ContractExecutionError> {
        let tracker = self.state_tracker.lock().unwrap();
        Ok(tracker.get_changes())
    }

    fn collect_events(&self, _contract_address: &Address) -> Result<Vec<ContractEvent>, ContractExecutionError> {
        let tracker = self.state_tracker.lock().unwrap();
        Ok(tracker.get_events())
    }

    // State persistence methods
    pub fn persist_contract_state(&self, address: &Address) -> Result<Vec<u8>, ContractExecutionError> {
        let storage = self.contract_storage.lock().unwrap();

        if let Some(contract_storage) = storage.storage.get(address) {
            bincode::serialize(contract_storage)
                .map_err(|e| ContractExecutionError {
                    code: ErrorCode::StateError,
                    message: format!("Failed to serialize state: {}", e),
                    gas_used: 0,
                })
        } else {
            Ok(Vec::new())
        }
    }

    pub fn restore_contract_state(&self, address: &Address, state_data: &[u8]) -> Result<(), ContractExecutionError> {
        if state_data.is_empty() {
            return Ok(());
        }

        let state: HashMap<Vec<u8>, Vec<u8>> = bincode::deserialize(state_data)
            .map_err(|e| ContractExecutionError {
                code: ErrorCode::StateError,
                message: format!("Failed to deserialize state: {}", e),
                gas_used: 0,
            })?;

        let mut storage = self.contract_storage.lock().unwrap();
        storage.storage.insert(address.clone(), state);

        Ok(())
    }

    // Gas estimation
    pub fn estimate_gas(&self, call: &ContractCall) -> Result<Gas, ContractExecutionError> {
        let mut estimated_gas = GAS_COST_BASE * 1000;
        estimated_gas += GAS_COST_MEMORY_BYTE * call.input_data.len() as Gas;

        match call.method.as_str() {
            "transfer" | "approve" => estimated_gas += 21000,
            "transferFrom" => estimated_gas += 25000,
            _ => estimated_gas += 50000,
        }

        estimated_gas += GAS_COST_STORAGE_READ * 5;
        estimated_gas += GAS_COST_STORAGE_WRITE * 2;

        Ok(estimated_gas)
    }

    // Public getters
    pub fn get_contract_state(&self, address: &Address, key: &[u8]) -> Option<Vec<u8>> {
        let storage = self.contract_storage.lock().unwrap();
        storage.get(address, key)
    }

    pub fn get_contract_info(&self, address: &Address) -> Option<ContractDeployment> {
        let contracts = self.contracts.lock().unwrap();
        contracts.get(address).map(|contract| contract.deployment_info.clone())
    }

    pub fn get_contract_statistics(&self, address: &Address) -> Option<(u64, u64, u32)> {
        let contracts = self.contracts.lock().unwrap();
        contracts.get(address).map(|contract| (
            contract.call_count,
            contract.last_called,
            contract.memory_usage
        ))
    }

    pub fn list_contracts(&self) -> Vec<Address> {
        let contracts = self.contracts.lock().unwrap();
        contracts.keys().cloned().collect()
    }

    pub fn get_events(&self, contract_address: Option<&Address>) -> Vec<ContractEvent> {
        let events = self.event_storage.lock().unwrap();
        match contract_address {
            Some(addr) => events.iter()
                .filter(|e| &e.contract_address == addr)
                .cloned()
                .collect(),
            None => events.clone(),
        }
    }
}

impl Clone for ContractRuntime {
    fn clone(&self) -> Self {
        Self {
            engine: self.engine.clone(),
            contracts: Arc::clone(&self.contracts),
            contract_storage: Arc::clone(&self.contract_storage),
            gas_meter: Arc::clone(&self.gas_meter),
            ai_analyzer: self.ai_analyzer.clone(),
            max_gas_per_call: self.max_gas_per_call,
            max_memory_pages: self.max_memory_pages,
            execution_context: Arc::clone(&self.execution_context),
            state_tracker: Arc::clone(&self.state_tracker),
            event_storage: Arc::clone(&self.event_storage),
        }
    }
}

impl GasMeter {
    fn new() -> Self {
        Self {
            current_gas: 0,
            gas_limit: 0,
            gas_price: 1,
            operations_count: 0,
        }
    }

    fn reset(&mut self, gas_limit: Gas) {
        self.current_gas = gas_limit;
        self.gas_limit = gas_limit;
        self.operations_count = 0;
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
        self.operations_count += 1;

        if self.operations_count > 1_000_000 {
            return Err(ContractExecutionError {
                code: ErrorCode::ExecutionFailed,
                message: "Operation limit exceeded".to_string(),
                gas_used: self.gas_limit - self.current_gas,
            });
        }

        Ok(())
    }

    fn remaining_gas(&self) -> Gas {
        self.current_gas
    }

    fn gas_used(&self) -> Gas {
        self.gas_limit - self.current_gas
    }
}

impl ContractStorage {
    fn new() -> Self {
        Self {
            storage: HashMap::new(),
            storage_usage: HashMap::new(),
        }
    }

    fn initialize_contract(&mut self, address: &Address, _initial_state: &[u8]) -> Result<(), ContractExecutionError> {
        self.storage.insert(address.clone(), HashMap::new());
        self.storage_usage.insert(address.clone(), 0);
        Ok(())
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

        let old_size = contract_storage.get(&key).map(|v| v.len()).unwrap_or(0);
        let new_size = value.len();
        let size_change = new_size as i64 - old_size as i64;

        let current_usage = self.storage_usage.get_mut(contract_address).unwrap();
        let new_usage = (*current_usage as i64 + size_change) as usize;

        if new_usage > 1024 * 1024 {
            return Err(ContractExecutionError {
                code: ErrorCode::StateError,
                message: "Storage limit exceeded".to_string(),
                gas_used: 0,
            });
        }

        contract_storage.insert(key, value);
        *current_usage = new_usage;

        Ok(())
    }
}

impl StateTracker {
    fn new() -> Self {
        Self {
            changes: Vec::new(),
            pending_events: Vec::new(),
        }
    }

    fn record_state_change(&mut self, change: StateChange) {
        self.changes.push(change);
    }

    fn emit_event(&mut self, event: ContractEvent) {
        self.pending_events.push(event);
    }

    fn get_changes(&self) -> Vec<StateChange> {
        self.changes.clone()
    }

    fn get_events(&self) -> Vec<ContractEvent> {
        self.pending_events.clone()
    }

    fn clear(&mut self) {
        self.changes.clear();
        self.pending_events.clear();
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
        let runtime = ContractRuntime::new(1_000_000, 16).unwrap();

        let deployment = ContractDeployment {
            address: "dyt1contract123".to_string(),
            code: vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00],
            initial_state: Vec::new(),
            gas_limit: 100_000,
            deployer: "dyt1deployer".to_string(),
            timestamp: 1234567890,
            ai_audit_score: Some(0.85),
        };

        let result = runtime.deploy_contract(deployment).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_gas_metering() {
        let runtime = ContractRuntime::new(1_000_000, 16).unwrap();

        {
            let mut gas_meter = runtime.gas_meter.lock().unwrap();
            gas_meter.reset(1000);

            assert_eq!(gas_meter.remaining_gas(), 1000);

            let result = gas_meter.consume_gas(500);
            assert!(result.is_ok());
            assert_eq!(gas_meter.remaining_gas(), 500);

            let result = gas_meter.consume_gas(600);
            assert!(result.is_err());
        }
    }

    #[tokio::test]
    async fn test_contract_storage() {
        let runtime = ContractRuntime::new(1_000_000, 16).unwrap();

        let address = "test_contract".to_string();
        let key = b"test_key".to_vec();
        let value = b"test_value".to_vec();

        {
            let mut storage = runtime.contract_storage.lock().unwrap();
            storage.initialize_contract(&address, &[]).unwrap();
            storage.set(&address, key.clone(), value.clone()).unwrap();

            let retrieved = storage.get(&address, &key);
            assert_eq!(retrieved, Some(value));
        }
    }
}
