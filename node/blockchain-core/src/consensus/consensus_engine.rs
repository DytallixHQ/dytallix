//! Consensus Engine Module
//!
//! This module contains the main ConsensusEngine struct that coordinates
//! all consensus-related operations including block processing, transaction
//! validation, and AI integration.

use anyhow::Result;
use log::{debug, error, info, warn};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::consensus::ai_oracle_client::{AIOracleClient, AIServiceConfig};
use crate::consensus::types::AIServiceType;
use crate::crypto::PQCManager;
use crate::runtime::DytallixRuntime;
use crate::storage::{ContractState, StorageManager};
use crate::types::{Block, CallTransaction, DeployTransaction, Transaction};
// Temporarily disabled due to smart contracts compilation issues
// use dytallix_contracts::runtime::{ContractRuntime, ContractDeployment, ContractCall};
use crate::consensus::ai_integration::{AIIntegrationConfig, AIIntegrationManager};
use crate::consensus::audit_trail::{AuditConfig, AuditTrailManager};
use crate::consensus::block_processing::BlockProcessor;
use crate::consensus::high_risk_queue::{HighRiskQueue, HighRiskQueueConfig};
use crate::consensus::key_management::{KeyManager, NodeKeyStore};
use crate::consensus::performance_optimizer::{PerformanceConfig, PerformanceOptimizer};
use crate::consensus::transaction_validation::TransactionValidator;
use crate::contracts::{ContractCall, ContractDeployment, ContractRuntime};

/// Contract execution result
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub success: bool,
    pub gas_used: u64,
    pub output: Vec<u8>,
    pub error: Option<String>,
}

impl ExecutionResult {
    pub fn success() -> Self {
        Self {
            success: true,
            gas_used: 0,
            output: Vec::new(),
            error: None,
        }
    }

    pub fn failure(error: String) -> Self {
        Self {
            success: false,
            gas_used: 0,
            output: Vec::new(),
            error: Some(error),
        }
    }
}

/// Consensus engine error
#[derive(Debug, thiserror::Error)]
pub enum ConsensusError {
    #[error("Storage error: {0}")]
    Storage(#[from] Box<dyn std::error::Error>),
    #[error("Contract not found: {0}")]
    ContractNotFound(String),
    #[error("Execution error: {0}")]
    Execution(String),
    #[error("Invalid transaction: {0}")]
    InvalidTransaction(String),
}

/// Main Consensus Engine
#[derive(Debug)]
pub struct ConsensusEngine {
    _runtime: Arc<DytallixRuntime>,
    _pqc_manager: Arc<PQCManager>,
    _current_block: Arc<RwLock<Option<Block>>>, // prefixed underscore
    _validators: Arc<RwLock<Vec<String>>>,      // underscore
    is_validator: bool,

    // Core components
    _ai_client: Arc<AIOracleClient>, // underscore
    ai_integration: Option<Arc<AIIntegrationManager>>,
    transaction_validator: Arc<TransactionValidator>,
    block_processor: Arc<BlockProcessor>,
    key_manager: Arc<RwLock<KeyManager>>,

    // Supporting services
    _high_risk_queue: Arc<HighRiskQueue>, // underscore
    _audit_trail: Arc<AuditTrailManager>, // underscore
    _performance_optimizer: Arc<PerformanceOptimizer>, // underscore

    // WASM contract runtime
    wasm_runtime: Arc<ContractRuntime>,
}

impl ConsensusEngine {
    /// Create a new consensus engine
    pub async fn new(
        runtime: Arc<DytallixRuntime>,
        pqc_manager: Arc<PQCManager>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Self::new_with_key_path(runtime, pqc_manager, Path::new("./data/pqc_keys.json")).await
    }

    /// Select the key-store path explicitly. Key generation and validation are unchanged.
    pub async fn new_with_key_path(
        runtime: Arc<DytallixRuntime>,
        pqc_manager: Arc<PQCManager>,
        key_file: &Path,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut key_manager =
            KeyManager::new(key_file.to_string_lossy().to_string(), pqc_manager.clone());

        // Initialize keys
        if let Err(e) = key_manager.initialize() {
            error!("Failed to initialize key management: {e}");
            return Err(e.into());
        }

        let key_manager = Arc::new(RwLock::new(key_manager));

        // Initialize AI client
        let ai_config = AIServiceConfig::default();
        let ai_client = Arc::new(AIOracleClient::new(ai_config));

        // Initialize supporting services
        let high_risk_queue = Arc::new(HighRiskQueue::new(HighRiskQueueConfig::default()));
        let audit_trail = Arc::new(AuditTrailManager::new(AuditConfig::default()));
        let performance_optimizer =
            Arc::new(PerformanceOptimizer::new(PerformanceConfig::default()));

        // Initialize AI integration (optional)
        let ai_integration = match AIIntegrationManager::new(AIIntegrationConfig::default()).await {
            Ok(manager) => Some(Arc::new(manager)),
            Err(e) => {
                warn!("AI integration not available: {e}");
                None
            }
        };

        // Initialize transaction validator
        let policy_manager = Arc::new(crate::policy::PolicyManager::default());
        let transaction_validator = Arc::new(TransactionValidator::new(
            ai_client.clone(),
            ai_integration.clone(),
            high_risk_queue.clone(),
            audit_trail.clone(),
            performance_optimizer.clone(),
            policy_manager.clone(),
        ));

        // Initialize block processor
        let current_block = Arc::new(RwLock::new(None));
        let block_processor = Arc::new(BlockProcessor::new(
            current_block.clone(),
            transaction_validator.clone(),
            ai_client.clone(),
            ai_integration.clone(),
            runtime.clone(),
        ));

        // Initialize WASM runtime
        let wasm_runtime = Arc::new(
            ContractRuntime::new(
                1_000_000, // Max gas per call
                256,       // Max memory pages
            )
            .map_err(|e| format!("Failed to initialize WASM runtime: {e:?}"))?,
        );

        Ok(Self {
            _runtime: runtime,
            _pqc_manager: pqc_manager,
            _current_block: current_block,
            _validators: Arc::new(RwLock::new(Vec::new())),
            is_validator: false,
            _ai_client: ai_client,
            ai_integration,
            transaction_validator,
            block_processor,
            key_manager,
            _high_risk_queue: high_risk_queue,
            _audit_trail: audit_trail,
            _performance_optimizer: performance_optimizer,
            wasm_runtime,
        })
    }

    /// Start the consensus engine
    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Starting consensus engine...");

        // Check and rotate keys if needed
        {
            let mut key_manager = self.key_manager.write().await;
            if let Err(e) = key_manager.rotate_keys_if_needed() {
                warn!("Failed to rotate keys: {e}");
            }
        }

        // Start supporting services
        // Note: These services are ready to use upon instantiation
        info!("High-risk queue ready");
        info!("Audit trail ready");
        info!("Performance optimizer ready");

        // Check AI service health
        if let Err(e) = self.check_ai_service_health().await {
            warn!("AI service health check failed: {e}");
        }

        info!("Consensus engine started successfully");
        Ok(())
    }

    /// Stop the consensus engine
    pub async fn stop(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Stopping consensus engine...");

        // Stop supporting services
        info!("Shutting down high-risk queue");
        info!("Shutting down audit trail");
        info!("Shutting down performance optimizer");

        info!("Consensus engine stopped");
        Ok(())
    }

    /// Check AI service health and connectivity
    pub async fn check_ai_service_health(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let health_status = self._ai_client.health_check().await?;
        if health_status {
            info!("AI service is healthy and responsive");
        } else {
            warn!("AI service health check failed");
        }
        Ok(health_status)
    }

    /// Discover available AI services
    pub async fn discover_ai_services(
        &self,
    ) -> Result<Vec<crate::consensus::ai_oracle_client::AIServiceInfo>, Box<dyn std::error::Error>>
    {
        let services = self._ai_client.discover_services().await?;
        info!("Discovered {} AI services", services.len());
        for service in &services {
            debug!(
                "AI Service: {} - Type: {:?} - Availability: {:.2}",
                service.service_id, service.service_type, service.availability_score
            );
        }
        Ok(services)
    }

    /// Request AI analysis for a transaction or data
    pub async fn request_ai_analysis(
        &self,
        service_type: AIServiceType,
        data: HashMap<String, Value>,
    ) -> Result<crate::consensus::SignedAIOracleResponse, Box<dyn std::error::Error>> {
        let response = self
            ._ai_client
            .request_ai_analysis(service_type, data)
            .await?;

        // Validate response confidence score from metadata
        if let Some(metadata) = &response.response.metadata {
            if let Some(confidence) = metadata.confidence_score {
                if confidence < self._ai_client.get_config().risk_threshold {
                    warn!("AI analysis confidence score below threshold: {confidence}");
                }
            }
        }

        info!(
            "AI analysis completed: service_type={:?}, response_id={}",
            response.response.service_type, response.response.id
        );

        Ok(response)
    }

    /// Propose a block with the given transactions
    pub async fn propose_block(&self, transactions: Vec<Transaction>) -> Result<Block, String> {
        self.block_processor
            .propose_block(transactions)
            .await
            .map_err(|e| e.to_string())
    }

    /// Validate a block
    pub async fn validate_block(&self, block: &Block) -> Result<bool, String> {
        match self.block_processor.validate_block(block).await {
            Ok(result) => Ok(result.is_valid),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Validate a block with AI-enhanced validation
    pub async fn validate_block_with_ai(&self, block: &Block) -> Result<bool, String> {
        self.validate_block(block).await
    }

    /// Validate a single transaction with AI enhancement
    pub async fn validate_transaction_with_ai(&self, tx: &Transaction) -> Result<bool, String> {
        match self.transaction_validator.validate_transaction(tx).await {
            Ok(result) => Ok(result.is_valid),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Check if AI integration is available
    pub fn has_ai_integration(&self) -> bool {
        self.ai_integration.is_some()
    }

    /// Get AI integration statistics (if available)
    pub async fn get_ai_integration_stats(&self) -> Option<Value> {
        self.transaction_validator.get_ai_integration_stats().await
    }

    /// Validate transaction with queue management
    pub async fn validate_transaction_with_queue(&self, tx: &Transaction) -> Result<bool, String> {
        match self
            .transaction_validator
            .validate_transaction_with_queue(tx)
            .await
        {
            Ok(result) => Ok(result.is_valid),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Validate transaction with optimized performance
    pub async fn validate_transaction_optimized(&self, tx: &Transaction) -> Result<bool, String> {
        match self
            .transaction_validator
            .validate_transaction_optimized(tx)
            .await
        {
            Ok(result) => Ok(result.is_valid),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Get comprehensive consensus engine statistics
    pub async fn get_stats(&self) -> HashMap<String, Value> {
        let mut stats = HashMap::new();

        // Block processing stats
        let block_stats = self.block_processor.get_stats().await;
        stats.insert(
            "block_processing".to_string(),
            serde_json::to_value(block_stats).unwrap_or_default(),
        );

        // Transaction validation stats
        let validation_stats = self.transaction_validator.get_validation_stats().await;
        stats.insert(
            "transaction_validation".to_string(),
            serde_json::to_value(validation_stats).unwrap_or_default(),
        );

        // AI integration stats
        if let Some(ai_stats) = self.get_ai_integration_stats().await {
            stats.insert("ai_integration".to_string(), ai_stats);
        }

        // Key management stats
        {
            let key_manager = self.key_manager.read().await;
            if let Some(key_store) = key_manager.get_key_store() {
                let mut key_stats = HashMap::new();
                key_stats.insert(
                    "node_id".to_string(),
                    Value::String(key_store.node_id.clone()),
                );
                key_stats.insert(
                    "created_at".to_string(),
                    Value::Number(key_store.created_at.into()),
                );
                key_stats.insert(
                    "version".to_string(),
                    Value::String(key_store.version.clone()),
                );
                key_stats.insert(
                    "needs_rotation".to_string(),
                    Value::Bool(key_manager.needs_key_rotation()),
                );

                stats.insert(
                    "key_management".to_string(),
                    serde_json::to_value(key_stats).unwrap_or_default(),
                );
            }
        }

        // Service health
        let mut health_stats = HashMap::new();
        health_stats.insert(
            "ai_integration_available".to_string(),
            Value::Bool(self.has_ai_integration()),
        );
        health_stats.insert(
            "block_processor_available".to_string(),
            Value::Bool(self.block_processor.has_ai_validation()),
        );
        health_stats.insert(
            "transaction_validator_available".to_string(),
            Value::Bool(self.transaction_validator.has_ai_validation()),
        );

        stats.insert(
            "service_health".to_string(),
            serde_json::to_value(health_stats).unwrap_or_default(),
        );

        stats
    }

    /// Get node key store information
    pub async fn get_key_store(&self) -> Option<NodeKeyStore> {
        let key_manager = self.key_manager.read().await;
        key_manager.get_key_store().cloned()
    }

    /// Get current block
    pub async fn get_current_block(&self) -> Option<Block> {
        self.block_processor.get_current_block().await
    }

    /// Set current block
    pub async fn set_current_block(&self, block: Block) {
        self.block_processor.set_current_block(block).await
    }

    /// Check if this node is a validator
    pub fn is_validator(&self) -> bool {
        self.is_validator
    }

    /// Set validator status
    pub fn set_validator_status(&mut self, is_validator: bool) {
        self.is_validator = is_validator;
    }

    /// Get validator list
    pub async fn get_validators(&self) -> Vec<String> {
        self._validators.read().await.clone()
    }

    /// Add validator
    pub async fn add_validator(&self, validator: String) {
        let mut validators = self._validators.write().await;
        if !validators.contains(&validator) {
            validators.push(validator);
        }
    }

    /// Remove validator
    pub async fn remove_validator(&self, validator: &str) {
        let mut validators = self._validators.write().await;
        validators.retain(|v| v != validator);
    }

    /// Get AI client reference
    pub fn get_ai_client(&self) -> Arc<AIOracleClient> {
        self._ai_client.clone()
    }

    /// Get transaction validator reference
    pub fn get_transaction_validator(&self) -> Arc<TransactionValidator> {
        self.transaction_validator.clone()
    }

    /// Get block processor reference
    pub fn get_block_processor(&self) -> Arc<BlockProcessor> {
        self.block_processor.clone()
    }

    /// Process contract transaction
    pub async fn process_contract_transaction(
        &self,
        tx: &Transaction,
        storage: &StorageManager,
    ) -> Result<ExecutionResult, ConsensusError> {
        match tx {
            Transaction::Deploy(deploy_tx) => self.execute_deployment(deploy_tx, storage).await,
            Transaction::Call(call_tx) => self.execute_contract_call(call_tx, storage).await,
            _ => Ok(ExecutionResult::success()),
        }
    }

    /// Execute contract deployment
    async fn execute_deployment(
        &self,
        deploy_tx: &DeployTransaction,
        storage: &StorageManager,
    ) -> Result<ExecutionResult, ConsensusError> {
        info!("Executing contract deployment from {}", deploy_tx.from);

        // Generate contract address (simplified - in production use proper derivation)
        let contract_address = format!("contract_{}", deploy_tx.hash);

        // Check if contract already exists
        if storage._contract_exists(&contract_address).await? {
            return Ok(ExecutionResult::failure(
                "Contract already exists".to_string(),
            ));
        }

        // Prepare WASM contract deployment
        let deployment = ContractDeployment {
            address: contract_address.clone(),
            code: deploy_tx.contract_code.clone(),
            initial_state: deploy_tx.constructor_args.clone(),
            gas_limit: deploy_tx.gas_limit,
            deployer: deploy_tx.from.clone(),
            timestamp: deploy_tx.timestamp,
            ai_audit_score: None,
            metadata: serde_json::json!({}),
        };

        // Deploy contract to WASM runtime
        let deployed_address = match self.wasm_runtime.deploy_contract(deployment).await {
            Ok(addr) => addr,
            Err(e) => {
                error!("WASM contract deployment failed: {e:?}");
                return Ok(ExecutionResult::failure(format!(
                    "Contract deployment failed: {e:?}"
                )));
            }
        };

        // Create contract state for blockchain storage
        let contract_state = ContractState::_new(
            deploy_tx.contract_code.clone(),
            deploy_tx.from.clone(),
            0, // TODO: Get actual block number
            deploy_tx.timestamp,
        );

        // Store contract state in blockchain storage
        storage
            ._store_contract(&contract_address, &contract_state)
            .await?;

        info!("Contract deployed successfully at {deployed_address}");

        Ok(ExecutionResult {
            success: true,
            gas_used: 1000, // TODO: Calculate actual gas used from WASM runtime
            output: deployed_address.as_bytes().to_vec(),
            error: None,
        })
    }

    /// Execute contract call
    async fn execute_contract_call(
        &self,
        call_tx: &CallTransaction,
        storage: &StorageManager,
    ) -> Result<ExecutionResult, ConsensusError> {
        info!(
            "Executing contract call from {} to {}",
            call_tx.from, call_tx.to
        );

        // Check if contract exists
        if !storage._contract_exists(&call_tx.to).await? {
            return Ok(ExecutionResult::failure(format!(
                "Contract not found: {}",
                call_tx.to
            )));
        }

        // Get contract state
        let mut contract_state = storage
            ._get_contract(&call_tx.to)
            .await?
            .ok_or_else(|| ConsensusError::ContractNotFound(call_tx.to.clone()))?;

        // Prepare WASM contract call
        let call = ContractCall {
            contract_address: call_tx.to.clone(),
            caller: call_tx.from.clone(),
            method: call_tx.method.clone(),
            input_data: call_tx.args.clone(),
            gas_limit: call_tx.gas_limit,
            value: 0, // TODO: Add value transfer support
            timestamp: call_tx.timestamp,
            contract_id: call_tx.to.clone(),
            function: call_tx.method.clone(),
            args: serde_json::json!({}),
        };

        // Execute contract call in WASM runtime
        let execution_result = match self.wasm_runtime.call_contract(call).await {
            Ok(result) => result,
            Err(e) => {
                error!("WASM contract call failed: {e:?}");
                return Ok(ExecutionResult::failure(format!(
                    "Contract call failed: {e:?}"
                )));
            }
        };

        // Update contract state
        contract_state._increment_calls();
        contract_state._update_timestamp(call_tx.timestamp);

        // Apply state changes from WASM execution
        for (key, value) in &execution_result.state_changes {
            let key_bytes = key.as_bytes().to_vec();
            let value_bytes = serde_json::to_vec(value).unwrap_or_default();
            contract_state._set_storage(key_bytes, value_bytes);
        }

        // Store updated state
        storage
            ._store_contract(&call_tx.to, &contract_state)
            .await?;

        info!(
            "Contract call executed successfully: gas_used={}, success={}",
            execution_result.gas_used, execution_result.success
        );

        Ok(ExecutionResult {
            success: execution_result.success,
            gas_used: execution_result.gas_used,
            output: execution_result.return_value,
            error: if execution_result.success {
                None
            } else {
                Some("Contract execution failed".to_string())
            },
        })
    }

    /// Get contract state
    pub async fn get_contract_state(
        &self,
        contract_address: &str,
        storage: &StorageManager,
    ) -> Result<Option<ContractState>, ConsensusError> {
        Ok(storage._get_contract(contract_address).await?)
    }

    /// Deploy contract (convenience method)
    pub async fn deploy_contract(
        &self,
        deploy_tx: &DeployTransaction,
        storage: &StorageManager,
    ) -> Result<String, ConsensusError> {
        let result = self.execute_deployment(deploy_tx, storage).await?;

        if result.success {
            let contract_address = String::from_utf8(result.output)
                .map_err(|e| ConsensusError::Execution(e.to_string()))?;
            Ok(contract_address)
        } else {
            Err(ConsensusError::Execution(
                result
                    .error
                    .unwrap_or_else(|| "Unknown deployment error".to_string()),
            ))
        }
    }

    /// Call contract method (convenience method)
    pub async fn call_contract(
        &self,
        call_tx: &CallTransaction,
        storage: &StorageManager,
    ) -> Result<Vec<u8>, ConsensusError> {
        let result = self.execute_contract_call(call_tx, storage).await?;

        if result.success {
            Ok(result.output)
        } else {
            Err(ConsensusError::Execution(
                result
                    .error
                    .unwrap_or_else(|| "Unknown call error".to_string()),
            ))
        }
    }
}
