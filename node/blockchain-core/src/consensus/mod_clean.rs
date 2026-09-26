use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid;

use crate::crypto::PQCManager;
use crate::runtime::DytallixRuntime;
use crate::types::{AIServiceType, Block, BlockHeader, Transaction, TransferTransaction}; // Import from types

// Helper function to convert gas (u64) to tokens (u128)
fn gas_to_tokens(gas: u64) -> u128 {
    gas as u128
}

// AI Service Integration
use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::HashMap;
use tokio::time::Duration;

// Import AI integration modules
use crate::consensus::ai_integration;
// Remove duplicate import - using local definitions

/// AI Oracle Response with signed validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedAIOracleResponse {
    pub oracle_id: String,
    pub timestamp: u64,
    pub request_hash: String,
    pub ai_result: AIAnalysisResult,
    pub signature: AIResponseSignature,
    pub confidence_score: f64,
}

/// AI Analysis Result structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIAnalysisResult {
    pub service_type: AIServiceType,
    pub risk_score: f64,
    pub fraud_probability: f64,
    pub reputation_score: u32,
    pub compliance_flags: Vec<String>,
    pub recommendations: Vec<String>,
    pub metadata: HashMap<String, String>,
}

/// AI Response Signature (PQC-based)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIResponseSignature {
    pub algorithm: String,
    pub signature_data: Vec<u8>,
    pub public_key: Vec<u8>,
    pub certificate_chain: Vec<Vec<u8>>,
}

/// AI Service Information for discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIServiceInfo {
    pub service_id: String,
    pub service_type: AIServiceType,
    pub endpoint: String,
    pub capabilities: Vec<String>,
    pub supported_algorithms: Vec<String>,
    pub max_request_size: u64,
    pub average_response_time_ms: u64,
    pub availability_score: f64,
}

/// AI Analysis Request structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIAnalysisRequest {
    pub request_id: String,
    pub service_type: AIServiceType,
    pub data: HashMap<String, Value>,
    pub requester_id: String,
    pub timestamp: u64,
    pub priority: u8, // 1-10, where 10 is highest priority
}

/// AI Service Configuration
#[derive(Debug, Clone)]
pub struct AIServiceConfig {
    pub endpoint: String,
    pub api_key: Option<String>,
    pub timeout_seconds: u64,
    pub max_retries: u32,
    pub risk_threshold: f64,
}

impl Default for AIServiceConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:8888".to_string(),
            api_key: None,
            timeout_seconds: 30,
            max_retries: 3,
            risk_threshold: 0.8,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredKeyPair {
    algorithm: String,
    public_key: Vec<u8>,
    secret_key: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NodeKeyStore {
    dilithium: StoredKeyPair,
    falcon: StoredKeyPair,
    sphincs: StoredKeyPair,
}

/// HTTP client for communicating with external AI services
#[derive(Debug, Clone)]
pub struct AIOracleClient {
    client: Client,
    config: AIServiceConfig,
}

impl AIOracleClient {
    pub fn new(config: AIServiceConfig) -> Self {
        let client = Client::builder()
            .pool_max_idle_per_host(8)
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Some(Duration::from_secs(60)))
            .timeout(Duration::from_secs(config.timeout_seconds))
            .build()
            .expect("failed to build http client");

        Self { client, config }
    }

    pub async fn post<P: Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        path: &str,
        payload: &P,
    ) -> Result<R> {
        let url = format!(
            "{}/{}",
            self.config.endpoint.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        let mut attempts = 0;
        loop {
            attempts += 1;
            match self.client.post(&url).json(payload).send().await {
                Ok(resp) => {
                    if resp.status().is_success() {
                        let json = resp.json::<R>().await?;
                        return Ok(json);
                    } else if attempts >= self.config.max_retries {
                        return Err(anyhow!("AI service error: {}", resp.status()));
                    }
                }
                Err(e) => {
                    if attempts >= self.config.max_retries {
                        return Err(anyhow!("Request error: {}", e));
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    /// Health check endpoint for AI services
    pub async fn health_check(&self) -> Result<bool> {
        let url = format!("{}/health", self.config.endpoint.trim_end_matches('/'));

        match self.client.get(&url).send().await {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(_) => Ok(false),
        }
    }

    /// Service discovery - get available AI services and their capabilities
    pub async fn discover_services(&self) -> Result<Vec<AIServiceInfo>> {
        let url = format!("{}/services", self.config.endpoint.trim_end_matches('/'));

        match self.client.get(&url).send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    let services = resp.json::<Vec<AIServiceInfo>>().await?;
                    Ok(services)
                } else {
                    Err(anyhow!("Service discovery failed: {}", resp.status()))
                }
            }
            Err(e) => Err(anyhow!("Service discovery error: {}", e)),
        }
    }

    /// Submit AI analysis request and get signed response
    pub async fn request_analysis(
        &self,
        request: &AIAnalysisRequest,
    ) -> Result<SignedAIOracleResponse> {
        self.post("analyze", request).await
    }

    /// Get current configuration
    pub fn get_config(&self) -> &AIServiceConfig {
        &self.config
    }

    /// Update timeout configuration
    pub fn set_timeout(&mut self, timeout_seconds: u64) {
        self.config.timeout_seconds = timeout_seconds;
    }
}

#[derive(Debug)]
pub struct ConsensusEngine {
    runtime: Arc<DytallixRuntime>,
    pqc_manager: Arc<PQCManager>,
    current_block: Arc<RwLock<Option<Block>>>,
    is_validator: bool,
    ai_client: AIOracleClient,
    ai_integration: Option<Arc<ai_integration::AIIntegrationManager>>,
    high_risk_queue: Arc<crate::consensus::high_risk_queue::HighRiskQueue>,
    audit_trail: Arc<crate::consensus::audit_trail::AuditTrailManager>,
    performance_optimizer: Arc<crate::consensus::performance_optimizer::PerformanceOptimizer>,
}

impl ConsensusEngine {
    pub fn new(
        runtime: Arc<DytallixRuntime>,
        pqc_manager: Arc<PQCManager>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Check for existing PQC keys
        let key_dir = Path::new("./data");
        let key_file = key_dir.join("pqc_keys.json");

        if key_file.exists() {
            match std::fs::read_to_string(&key_file) {
                Ok(data) => {
                    if let Ok(store) = serde_json::from_str::<NodeKeyStore>(&data) {
                        info!("Loaded PQC keys from {}", key_file.display());
                        info!(
                            "Available algorithms: {}, {}, {}",
                            store.dilithium.algorithm,
                            store.falcon.algorithm,
                            store.sphincs.algorithm
                        );
                    } else {
                        warn!("Failed to parse PQC key store, generating new keys");
                        Self::generate_and_store_keys(&key_file)?;
                    }
                }
                Err(_) => {
                    warn!("Unable to read PQC key store, generating new keys");
                    Self::generate_and_store_keys(&key_file)?;
                }
            }
        } else {
            Self::generate_and_store_keys(&key_file)?;
        }

        let ai_client = AIOracleClient::new(AIServiceConfig::default());

        // Initialize AI integration with default configuration
        let ai_integration = match ai_integration::AIIntegrationManager::new_sync(
            ai_integration::AIIntegrationConfig::default(),
        ) {
            Ok(manager) => {
                info!("AI integration initialized successfully");
                Some(Arc::new(manager))
            }
            Err(e) => {
                warn!("Failed to initialize AI integration: {e}");
                warn!("Continuing without AI integration - transactions will use basic validation only");
                None
            }
        };

        // Initialize high-risk transaction queue
        let high_risk_queue = Arc::new(crate::consensus::high_risk_queue::HighRiskQueue::new(
            crate::consensus::high_risk_queue::HighRiskQueueConfig::default(),
        ));

        // Initialize audit trail manager
        let audit_trail = Arc::new(crate::consensus::audit_trail::AuditTrailManager::new(
            crate::consensus::audit_trail::AuditConfig::default(),
        ));

        // Initialize performance optimizer
        let performance_optimizer = Arc::new(
            crate::consensus::performance_optimizer::PerformanceOptimizer::new(
                crate::consensus::performance_optimizer::PerformanceConfig::default(),
            ),
        );

        Ok(Self {
            runtime,
            pqc_manager,
            current_block: Arc::new(RwLock::new(None)),
            is_validator: false,
            ai_client,
            ai_integration,
            high_risk_queue,
            audit_trail,
            performance_optimizer,
        })
    }

    fn generate_and_store_keys(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;

        // Generate placeholder keys for testing
        let (d_pk, d_sk) = (vec![0u8; 32], vec![0u8; 32]);
        let (f_pk, f_sk) = (vec![0u8; 32], vec![0u8; 32]);
        let (s_pk, s_sk) = (vec![0u8; 32], vec![0u8; 32]);

        let store = NodeKeyStore {
            dilithium: StoredKeyPair {
                algorithm: "Dilithium5".to_string(),
                public_key: d_pk,
                secret_key: d_sk,
            },
            falcon: StoredKeyPair {
                algorithm: "Falcon1024".to_string(),
                public_key: f_pk,
                secret_key: f_sk,
            },
            sphincs: StoredKeyPair {
                algorithm: "SphincsSha256128s".to_string(),
                public_key: s_pk,
                secret_key: s_sk,
            },
        };

        let json = serde_json::to_string_pretty(&store)?;
        std::fs::write(path, json)?;
        info!("Generated PQC keys at {}", path.display());
        Ok(())
    }

    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Starting consensus engine...");

        if self.is_validator {
            self.start_validator_loop().await?;
        }

        Ok(())
    }

    pub async fn stop(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Stopping consensus engine...");
        Ok(())
    }

    /// Check AI service health and connectivity
    pub async fn check_ai_service_health(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let health_status = self.ai_client.health_check().await?;
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
    ) -> Result<Vec<AIServiceInfo>, Box<dyn std::error::Error>> {
        let services = self.ai_client.discover_services().await?;
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
    ) -> Result<SignedAIOracleResponse, Box<dyn std::error::Error>> {
        let request = AIAnalysisRequest {
            request_id: format!("req_{}", chrono::Utc::now().timestamp_millis()),
            service_type,
            data,
            requester_id: "consensus_engine".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            priority: 5, // Medium priority
        };

        let response = self.ai_client.request_analysis(&request).await?;

        // Validate response signature (in production, this would verify PQC signature)
        if response.confidence_score < self.ai_client.get_config().risk_threshold {
            warn!(
                "AI analysis confidence score below threshold: {}",
                response.confidence_score
            );
        }

        Ok(response)
    }

    async fn start_validator_loop(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Starting validator loop...");

        let runtime = Arc::clone(&self.runtime);
        let pqc_manager = Arc::clone(&self.pqc_manager);
        let current_block = Arc::clone(&self.current_block); // fixed field name

        tokio::spawn(async move {
            let mut block_number = 0u64;

            loop {
                debug!("Validator tick - producing block #{block_number}");

                // Create a sample transaction for demonstration
                let mut sample_tx = crate::types::TransferTransaction {
                    hash: String::new(), // Will be calculated
                    from: "dyt1genesis".to_string(),
                    to: format!("dyt1addr{}", block_number % 5), // Rotate between addresses
                    amount: 100u128 + (block_number as u128 * 10), // Variable amounts as u128
                    fee: 1u128,
                    nonce: block_number,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    signature: crate::types::PQCTransactionSignature {
                        signature: dytallix_pqc::Signature {
                            data: Vec::new(),
                            algorithm: dytallix_pqc::SignatureAlgorithm::Dilithium5,
                        },
                        public_key: Vec::new(),
                    },
                    ai_risk_score: Some(0.1), // Low risk
                };

                // Calculate hash
                sample_tx.hash = sample_tx.calculate_hash();

                // Sign the transaction
                if let Ok(sig) = pqc_manager.sign_message(&sample_tx.signing_message()) {
                    sample_tx.signature = crate::types::PQCTransactionSignature {
                        signature: dytallix_pqc::Signature {
                            data: sig.signature,
                            algorithm: dytallix_pqc::SignatureAlgorithm::Dilithium5,
                        },
                        public_key: pqc_manager.get_dilithium_public_key().to_vec(),
                    };
                }

                let mut transaction = Transaction::Transfer(sample_tx);
                transaction
                    .sign_transaction(&pqc_manager)
                    .expect("failed to sign sample transaction");
                let transactions = vec![transaction];

                // Create block proposal
                match Self::create_block_proposal(
                    &runtime,
                    &pqc_manager,
                    &current_block,
                    transactions,
                    block_number,
                )
                .await
                {
                    Ok(block) => {
                        info!(
                            "✅ Successfully created block #{} with {} transactions",
                            block.header.number,
                            block.transactions.len()
                        );

                        // Validate the block
                        match Self::validate_block_static(&runtime, &pqc_manager, &block).await {
                            Ok(true) => {
                                info!("✅ Block #{} validation successful", block.header.number);

                                // Apply block to state
                                if let Err(e) = Self::apply_block_to_state(&runtime, &block).await {
                                    log::error!("Failed to apply block to state: {e}");
                                } else {
                                    // Update current block
                                    let mut current = current_block.write().await;
                                    *current = Some(block);
                                    block_number += 1;
                                }
                            }
                            Ok(false) => {
                                log::error!("❌ Block #{} validation failed", block.header.number);
                            }
                            Err(e) => {
                                log::error!(
                                    "❌ Error validating block #{}: {}",
                                    block.header.number,
                                    e
                                );
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to create block proposal: {e}");
                    }
                }

                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
            }
        });

        Ok(())
    }

    pub async fn propose_block(&self, transactions: Vec<Transaction>) -> Result<Block, String> {
        Self::create_block_proposal(
            &self.runtime,
            &self.pqc_manager,
            &self.current_block, // fixed field name
            transactions,
            0,
        )
        .await
    }

    pub async fn validate_block(&self, block: &Block) -> Result<bool, String> {
        Self::validate_block_static(&self.runtime, &self.pqc_manager, block).await
    }

    // Removed unused instance method calculate_merkle_root; use static instead

    /// Validate a block with AI-enhanced transaction validation
    pub async fn validate_block_with_ai(&self, block: &Block) -> Result<bool, String> {
        Self::validate_block_with_ai_static(
            &self.runtime,
            &self.pqc_manager,
            block,
            self.ai_integration.as_deref(),
        )
        .await
    }

    /// Validate a single transaction with AI enhancement
    pub async fn validate_transaction_with_ai(&self, tx: &Transaction) -> Result<bool, String> {
        Self::validate_transaction_with_ai_static(
            &self.runtime,
            &self.pqc_manager,
            tx,
            self.ai_integration.as_deref(),
        )
        .await
    }

    /// Check if AI integration is available
    pub fn has_ai_integration(&self) -> bool {
        self.ai_integration.is_some()
    }

    /// Get AI integration statistics (if available)
    pub async fn get_ai_integration_stats(&self) -> Option<serde_json::Value> {
        if let Some(ai_manager) = &self.ai_integration {
            let stats = ai_manager.get_statistics().await;
            Some(serde_json::json!({
                "total_requests": stats.total_requests,
                "successful_verifications": stats.successful_verifications,
                "failed_verifications": stats.failed_verifications,
                "ai_verification_required": ai_manager.is_ai_verification_required(),
                "cache_enabled": true // AI integration always has caching enabled
            }))
        } else {
            None
        }
    }

    // Static helper methods for use in async tasks
    async fn create_block_proposal(
        _runtime: &Arc<DytallixRuntime>,
        pqc_manager: &Arc<PQCManager>,
        current_block: &Arc<RwLock<Option<Block>>>,
        transactions: Vec<Transaction>,
        block_number: u64,
    ) -> Result<Block, String> {
        let previous_block = current_block.read().await;
        let parent_hash = match &*previous_block {
            Some(block) => Self::calculate_block_hash_static(&block.header),
            None => "0".repeat(64), // Genesis block
        };
        drop(previous_block);

        let transactions_root = Self::calculate_merkle_root_static(&transactions);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs();

        // Create a placeholder signature (to be replaced)
        let placeholder_signature = crate::types::PQCBlockSignature {
            signature: dytallix_pqc::Signature {
                data: Vec::new(),
                algorithm: dytallix_pqc::SignatureAlgorithm::Dilithium5,
            },
            public_key: Vec::new(),
        };

        let header = BlockHeader {
            number: block_number,
            parent_hash,
            transactions_root,
            state_root: "0".repeat(64), // TODO: Calculate actual state root
            timestamp,
            validator: "dyt1validator".to_string(), // TODO: Use actual validator address
            signature: placeholder_signature.clone(),
            nonce: 0, // TODO: Implement proper nonce for PoW if needed
        };

        let mut block = Block {
            header,
            transactions,
        };

        // Sign the block with PQC signature
        let block_hash = Self::calculate_block_hash_static(&block.header);
        let signature = pqc_manager
            .sign_message(block_hash.as_bytes())
            .map_err(|e| e.to_string())?;

        // Update the block header with real signature
        block.header.signature = crate::types::PQCBlockSignature {
            signature: dytallix_pqc::Signature {
                data: signature.signature,
                algorithm: dytallix_pqc::SignatureAlgorithm::Dilithium5,
            },
            public_key: pqc_manager.get_dilithium_public_key().to_vec(),
        };

        Ok(block)
    }

    async fn validate_block_static(
        runtime: &Arc<DytallixRuntime>,
        pqc_manager: &Arc<PQCManager>,
        block: &Block,
    ) -> Result<bool, String> {
        // Validate block structure
        if block.transactions.is_empty() {
            return Ok(false);
        }

        // Validate PQC signature
        let block_hash = Self::calculate_block_hash_static(&block.header);
        let is_valid = pqc_manager
            .verify_signature(
                block_hash.as_bytes(),
                &crate::crypto::PQCSignature {
                    signature: block.header.signature.signature.data.clone(),
                    algorithm: format!("{:?}", block.header.signature.signature.algorithm),
                    nonce: 0,
                    timestamp: 0,
                },
                &block.header.signature.public_key,
            )
            .map_err(|e| e.to_string())?;

        if !is_valid {
            return Ok(false);
        }

        // Validate transactions
        for tx in &block.transactions {
            if !Self::validate_transaction_static(runtime, pqc_manager, tx).await? {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Enhanced block validation with AI integration
    async fn validate_block_with_ai_static(
        runtime: &Arc<DytallixRuntime>,
        pqc_manager: &Arc<PQCManager>,
        block: &Block,
        ai_integration: Option<&ai_integration::AIIntegrationManager>,
    ) -> Result<bool, String> {
        // Validate block structure
        if block.transactions.is_empty() {
            return Ok(false);
        }

        // Validate PQC signature
        let block_hash = Self::calculate_block_hash_static(&block.header);
        let is_valid = pqc_manager
            .verify_signature(
                block_hash.as_bytes(),
                &crate::crypto::PQCSignature {
                    signature: block.header.signature.signature.data.clone(),
                    algorithm: format!("{:?}", block.header.signature.signature.algorithm),
                    nonce: 0,
                    timestamp: 0,
                },
                &block.header.signature.public_key,
            )
            .map_err(|e| e.to_string())?;

        if !is_valid {
            return Ok(false);
        }

        // Validate transactions with AI enhancement
        for tx in &block.transactions {
            if !Self::validate_transaction_with_ai_static(runtime, pqc_manager, tx, ai_integration)
                .await?
            {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Enhanced transaction validation with AI integration
    async fn validate_transaction_static(
        runtime: &Arc<DytallixRuntime>,
        pqc_manager: &Arc<PQCManager>,
        tx: &Transaction,
    ) -> Result<bool, String> {
        Self::validate_transaction_with_ai_static(runtime, pqc_manager, tx, None).await
    }

    /// Transaction validation with AI integration and configuration
    async fn validate_transaction_with_ai_static(
        runtime: &Arc<DytallixRuntime>,
        pqc_manager: &Arc<PQCManager>,
        tx: &Transaction,
        ai_integration: Option<&ai_integration::AIIntegrationManager>,
    ) -> Result<bool, String> {
        // Step 1: Basic signature validation
        if !Self::validate_any_transaction_signature(pqc_manager, tx)? {
            return Ok(false);
        }

        // Step 2: Transaction-specific validation
        let basic_validation_result = match tx {
            Transaction::Transfer(transfer_tx) => {
                Self::validate_transfer_transaction_basic(runtime, transfer_tx).await?
            }
            Transaction::Deploy(deploy_tx) => {
                Self::validate_deploy_transaction_basic(runtime, deploy_tx).await?
            }
            Transaction::Call(call_tx) => {
                Self::validate_call_transaction_basic(runtime, call_tx).await?
            }
            Transaction::Stake(stake_tx) => {
                Self::validate_stake_transaction_basic(runtime, stake_tx).await?
            }
            Transaction::AIRequest(ai_request_tx) => {
                Self::validate_ai_request_transaction_basic(runtime, ai_request_tx).await?
            }
        };

        // If basic validation fails, no need to proceed with AI analysis
        if !basic_validation_result {
            return Ok(false);
        }

        // Step 3: AI-enhanced validation (if AI integration is available)
        if let Some(ai_manager) = ai_integration {
            match Self::perform_ai_transaction_analysis(ai_manager, tx).await {
                Ok(ai_result) => {
                    match ai_result {
                        ai_integration::AIVerificationResult::Verified {
                            risk_score,
                            processing_decision,
                            fraud_probability,
                            confidence,
                            ..
                        } => {
                            // Apply risk-based processing decision
                            match processing_decision {
                                ai_integration::RiskProcessingDecision::AutoApprove => {
                                    info!("Transaction auto-approved by AI: risk score {:.3}, fraud prob {:.3}, confidence {:.3}",
                                          risk_score.unwrap_or(0.0), fraud_probability.unwrap_or(0.0), confidence.unwrap_or(0.0));
                                    Ok(true)
                                }
                                ai_integration::RiskProcessingDecision::RequireReview {
                                    reason,
                                } => {
                                    warn!("Transaction flagged for manual review: {} (risk: {:.3}, fraud: {:.3})",
                                          reason, risk_score.unwrap_or(0.0), fraud_probability.unwrap_or(0.0));

                                    // Add to high-risk queue for manual review
                                    // This is a placeholder - in practice, we'd need to pass the high-risk queue reference
                                    // and transaction hash to this static method, or restructure the validation flow
                                    warn!("Transaction should be queued for manual review - implementing queue integration");
                                    Ok(false) // Still reject for now until we restructure the validation flow
                                }
                                ai_integration::RiskProcessingDecision::AutoReject { reason } => {
                                    warn!("Transaction auto-rejected by AI: {} (risk: {:.3}, fraud: {:.3})",
                                          reason, risk_score.unwrap_or(0.0), fraud_probability.unwrap_or(0.0));
                                    Ok(false)
                                }
                            }
                        }
                        ai_integration::AIVerificationResult::Failed { error, .. } => {
                            warn!("AI transaction validation failed: {error}");
                            // Check if AI verification is required
                            if ai_manager.is_ai_verification_required() {
                                return Ok(false);
                            }
                            // Otherwise, proceed with basic validation result
                            info!("AI validation failed but not required, proceeding with basic validation");
                            Ok(true)
                        }
                        ai_integration::AIVerificationResult::Unavailable {
                            fallback_allowed,
                            ..
                        } => {
                            if !fallback_allowed && ai_manager.is_ai_verification_required() {
                                warn!("AI service unavailable and verification required, rejecting transaction");
                                return Ok(false);
                            }
                            info!("AI service unavailable but fallback allowed, proceeding with basic validation");
                            Ok(true)
                        }
                        ai_integration::AIVerificationResult::Skipped { reason } => {
                            info!("AI verification skipped: {reason}");
                            Ok(true)
                        }
                    }
                }
                Err(e) => {
                    warn!("AI analysis error: {e}");
                    // If AI analysis fails and AI verification is required, reject
                    if ai_manager.is_ai_verification_required() {
                        return Ok(false);
                    }
                    // Otherwise, proceed with basic validation
                    Ok(true)
                }
            }
        } else {
            // No AI integration available, proceed with basic validation
            Ok(true)
        }
    }

    /// Perform AI analysis on a transaction
    async fn perform_ai_transaction_analysis(
        ai_manager: &ai_integration::AIIntegrationManager,
        tx: &Transaction,
    ) -> Result<ai_integration::AIVerificationResult, String> {
        // Convert transaction to JSON for AI analysis
        let transaction_data = match Self::transaction_to_ai_data(tx) {
            Ok(data) => data,
            Err(e) => {
                return Err(format!(
                    "Failed to serialize transaction for AI analysis: {e}"
                ));
            }
        };

        // Request AI analysis
        match ai_manager
            .validate_transaction_with_ai(transaction_data)
            .await
        {
            Ok(result) => Ok(result),
            Err(e) => Err(format!("AI validation request failed: {e}")),
        }
    }

    /// Convert transaction to JSON format for AI analysis
    fn transaction_to_ai_data(tx: &Transaction) -> Result<serde_json::Value, String> {
        match tx {
            Transaction::Transfer(transfer_tx) => Ok(serde_json::json!({
                "transaction_type": "transfer",
                "from": transfer_tx.from,
                "to": transfer_tx.to,
                "amount": transfer_tx.amount,
                "fee": transfer_tx.fee,
                "nonce": transfer_tx.nonce,
                "timestamp": transfer_tx.timestamp,
                "hash": transfer_tx.hash,
                "existing_risk_score": transfer_tx.ai_risk_score
            })),
            Transaction::Deploy(deploy_tx) => Ok(serde_json::json!({
                "transaction_type": "contract_deploy",
                "deployer": deploy_tx.from,
                "contract_address": hex::encode(&deploy_tx.contract_code[..std::cmp::min(20, deploy_tx.contract_code.len())]),
                "code_size": deploy_tx.contract_code.len(),
                "initial_balance": deploy_tx.constructor_args.len(),
                "gas_limit": 1000000u64,
                "fee": deploy_tx.fee,
                "nonce": deploy_tx.nonce,
                "timestamp": deploy_tx.timestamp,
                "hash": deploy_tx.hash
            })),
            Transaction::Call(call_tx) => Ok(serde_json::json!({
                "transaction_type": "contract_call",
                "caller": call_tx.from,
                "contract_address": call_tx.to,
                "function_name": call_tx.method,
                "input_size": call_tx.args.len(),
                "gas_limit": 1000000u64,
                "fee": call_tx.fee,
                "nonce": call_tx.nonce,
                "timestamp": call_tx.timestamp,
                "hash": call_tx.hash
            })),
            Transaction::Stake(stake_tx) => Ok(serde_json::json!({
                "transaction_type": "stake",
                "validator": stake_tx.validator,
                "amount": stake_tx.amount,
                "action": format!("{:?}", stake_tx.action),
                "fee": stake_tx.fee,
                "nonce": stake_tx.nonce,
                "timestamp": stake_tx.timestamp,
                "hash": stake_tx.hash
            })),
            Transaction::AIRequest(ai_tx) => Ok(serde_json::json!({
                "transaction_type": "ai_request",
                "requester": ai_tx.from,
                "service_type": format!("{:?}", ai_tx.service_type),
                "payload": ai_tx.payload,
                "fee": ai_tx.fee,
                "nonce": ai_tx.nonce,
                "timestamp": ai_tx.timestamp,
                "hash": ai_tx.hash,
                "existing_risk_score": ai_tx.ai_risk_score
            })),
        }
    }

    /// Basic validation for transfer transactions
    async fn validate_transfer_transaction_basic(
        runtime: &Arc<DytallixRuntime>,
        transfer_tx: &crate::types::TransferTransaction,
    ) -> Result<bool, String> {
        // Basic validation
        if transfer_tx.amount == 0 {
            return Ok(false);
        }

        // Check balance for transfers (skip for genesis)
        if transfer_tx.from != "dyt1genesis" {
            let balance = runtime
                .get_balance(&transfer_tx.from)
                .await
                .map_err(|e| e.to_string())?;
            if balance < transfer_tx.amount {
                info!(
                    "Transaction rejected due to insufficient balance: {} < {}",
                    balance, transfer_tx.amount
                );
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Basic validation for contract deployment transactions
    async fn validate_deploy_transaction_basic(
        runtime: &Arc<DytallixRuntime>,
        deploy_tx: &crate::types::DeployTransaction,
    ) -> Result<bool, String> {
        // Check if deployer has sufficient balance for gas
        let deployer_balance = runtime
            .get_balance(&deploy_tx.from)
            .await
            .map_err(|e| e.to_string())?;

        let gas_cost = gas_to_tokens(deploy_tx.gas_limit * deploy_tx.gas_price);
        if deployer_balance < gas_cost {
            return Ok(false);
        }

        // Validate contract code is not empty
        if deploy_tx.contract_code.is_empty() {
            return Ok(false);
        }

        // Check contract code size (max 64KB)
        if deploy_tx.contract_code.len() > 65536 {
            return Ok(false);
        }

        // Generate contract address (simplified)
        let contract_address = format!("dyt1contract{}", &deploy_tx.hash[..8]);

        // Check if contract already exists at this address
        if let Ok(Some(_)) = runtime.get_contract(&contract_address).await {
            return Ok(false);
        }

        Ok(true)
    }

    /// Basic validation for contract call transactions
    async fn validate_call_transaction_basic(
        runtime: &Arc<DytallixRuntime>,
        call_tx: &crate::types::CallTransaction,
    ) -> Result<bool, String> {
        // Check if caller has sufficient balance for gas + value
        let caller_balance = runtime
            .get_balance(&call_tx.from)
            .await
            .map_err(|e| e.to_string())?;

        let gas_cost = gas_to_tokens(call_tx.gas_limit * call_tx.gas_price);
        let total_cost = gas_cost + call_tx.value;

        if caller_balance < total_cost {
            return Ok(false);
        }

        // Validate contract exists at the specified address
        if let Ok(None) = runtime.get_contract(&call_tx.to).await {
            return Ok(false);
        }

        // Validate method name is not empty
        if call_tx.method.trim().is_empty() {
            return Ok(false);
        }

        // Validate contract address format
        if !call_tx.to.starts_with("dyt1") {
            return Ok(false);
        }

        Ok(true)
    }

    /// Basic validation for staking transactions
    async fn validate_stake_transaction_basic(
        _runtime: &Arc<DytallixRuntime>,
        _stake_tx: &crate::types::StakeTransaction,
    ) -> Result<bool, String> {
        // TODO: Implement staking validation
        // For now, just allow all staking operations
        Ok(true)
    }

    /// Basic validation for AI request transactions
    async fn validate_ai_request_transaction_basic(
        _runtime: &Arc<DytallixRuntime>,
        ai_request_tx: &crate::types::AIRequestTransaction,
    ) -> Result<bool, String> {
        // Validate AI request transaction
        if ai_request_tx.service_type == crate::types::AIServiceType::Unknown {
            return Ok(false);
        }

        // Check for required fields based on service type
        match ai_request_tx.service_type {
            crate::types::AIServiceType::Kyc | crate::types::AIServiceType::Aml => {
                if ai_request_tx.payload.get("identity").is_none() {
                    return Ok(false);
                }
            }
            crate::types::AIServiceType::CreditAssessment => {
                if ai_request_tx
                    .payload
                    .get("social_security_number")
                    .is_none()
                {
                    return Ok(false);
                }
            }
            _ => {}
        }

        Ok(true)
    }

    async fn apply_block_to_state(
        runtime: &Arc<DytallixRuntime>,
        block: &Block,
    ) -> Result<(), String> {
        info!("Applying block #{} to state", block.header.number);

        for tx in &block.transactions {
            match tx {
                Transaction::Transfer(transfer_tx) => {
                    // Apply transfer transaction to state
                    if transfer_tx.from != "dyt1genesis" {
                        // Deduct from sender (skip for genesis)
                        let sender_balance =
                            runtime.get_balance(&transfer_tx.from).await.unwrap_or(0);
                        if sender_balance >= transfer_tx.amount {
                            runtime
                                .set_balance(&transfer_tx.from, sender_balance - transfer_tx.amount)
                                .await
                                .map_err(|e| e.to_string())?;
                            runtime
                                .increment_nonce(&transfer_tx.from)
                                .await
                                .map_err(|e| e.to_string())?;
                        }
                    }

                    // Add to recipient
                    let recipient_balance = runtime.get_balance(&transfer_tx.to).await.unwrap_or(0);
                    runtime
                        .set_balance(&transfer_tx.to, recipient_balance + transfer_tx.amount)
                        .await
                        .map_err(|e| e.to_string())?;

                    info!(
                        "Applied transfer: {} -> {} ({})",
                        transfer_tx.from, transfer_tx.to, transfer_tx.amount
                    );
                }
                Transaction::Deploy(deploy_tx) => {
                    // Generate contract address (simplified)
                    let contract_address = format!("dyt1contract{}", &deploy_tx.hash[..8]);

                    // Deploy smart contract using the runtime
                    match runtime
                        .deploy_contract_full(
                            &contract_address,
                            deploy_tx.contract_code.clone(),
                            &deploy_tx.from,
                            deploy_tx.gas_limit,
                            deploy_tx.constructor_args.clone(),
                        )
                        .await
                    {
                        Ok(deployed_address) => {
                            info!(
                                "Successfully deployed contract: {} at address: {}",
                                deploy_tx.hash, deployed_address
                            );
                        }
                        Err(e) => {
                            error!("Failed to deploy contract {}: {}", deploy_tx.hash, e);
                            return Err(format!("Contract deployment failed: {e}"));
                        }
                    }
                }
                Transaction::Call(call_tx) => {
                    // Execute smart contract call using the runtime
                    match runtime
                        .call_contract_method(
                            &call_tx.to, // Use 'to' field as contract address
                            &call_tx.from,
                            &call_tx.method,
                            &call_tx.args,
                            call_tx.gas_limit,
                            call_tx.value,
                        )
                        .await
                    {
                        Ok(execution_result) => {
                            if execution_result.success {
                                info!(
                                    "Successfully executed contract call: {}, gas used: {}",
                                    call_tx.hash, execution_result.gas_used
                                );

                                // Log contract events
                                for event in execution_result.events {
                                    info!("Contract event: {} - {:?}", call_tx.to, event);
                                }
                            } else {
                                warn!(
                                    "Contract call failed: {}, gas used: {}",
                                    call_tx.hash, execution_result.gas_used
                                );
                            }
                        }
                        Err(e) => {
                            error!("Failed to execute contract call {}: {}", call_tx.hash, e);
                            return Err(format!("Contract execution failed: {e}"));
                        }
                    }
                }
                Transaction::Stake(stake_tx) => {
                    // TODO: Process staking transaction
                    info!("Applied staking transaction: {}", stake_tx.hash);
                }
                Transaction::AIRequest(ai_tx) => {
                    // TODO: Process AI service request
                    info!("Applied AI request: {}", ai_tx.hash);
                }
            }
        }

        // Save state to storage
        runtime._save_state().await.map_err(|e| e.to_string())?;

        Ok(())
    }

    #[allow(dead_code)]
    fn format_transfer_transaction_message(tx: &TransferTransaction) -> Vec<u8> {
        format!(
            "{}:{}:{}:{}:{}:{}",
            tx.from, tx.to, tx.amount, tx.fee, tx.nonce, tx.timestamp
        )
        .into_bytes()
    }

    #[allow(dead_code)]
    fn validate_transaction_signature_static(
        pqc_manager: &Arc<PQCManager>,
        tx: &Transaction,
    ) -> Result<bool, String> {
        match tx {
            Transaction::Transfer(transfer_tx) => {
                let message = Self::format_transfer_transaction_message(transfer_tx);

                pqc_manager
                    .verify_signature(
                        &message,
                        &crate::crypto::PQCSignature {
                            signature: transfer_tx.signature.signature.data.clone(),
                            algorithm: format!("{:?}", transfer_tx.signature.signature.algorithm),
                            nonce: 0, // TODO: Use proper nonce
                            timestamp: chrono::Utc::now().timestamp() as u64,
                        },
                        &transfer_tx.signature.public_key,
                    )
                    .map_err(|e| e.to_string())
            }
            _ => Ok(true),
        }
    }

    fn calculate_block_hash_static(header: &BlockHeader) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        header.number.hash(&mut hasher);
        header.parent_hash.hash(&mut hasher);
        header.transactions_root.hash(&mut hasher);
        header.state_root.hash(&mut hasher);
        header.timestamp.hash(&mut hasher);
        header.validator.hash(&mut hasher);
        header.nonce.hash(&mut hasher);

        format!("{:x}", hasher.finish())
    }

    fn validate_any_transaction_signature(
        pqc_manager: &PQCManager,
        tx: &Transaction,
    ) -> Result<bool, String> {
        let message = tx.signing_message();
        let sig = tx.signature();

        let pqc_sig = crate::crypto::PQCSignature {
            signature: sig.signature.data.clone(),
            algorithm: format!("{:?}", sig.signature.algorithm),
            nonce: 0, // TODO: Use proper nonce
            timestamp: chrono::Utc::now().timestamp() as u64,
        };

        pqc_manager
            .verify_signature(&message, &pqc_sig, &sig.public_key)
            .map_err(|e| e.to_string())
    }

    fn calculate_merkle_root_static(transactions: &[Transaction]) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        for tx in transactions {
            // Hash the transaction based on its type
            match tx {
                Transaction::Transfer(transfer_tx) => {
                    transfer_tx.from.hash(&mut hasher);
                    transfer_tx.to.hash(&mut hasher);
                    transfer_tx.amount.hash(&mut hasher);
                }
                Transaction::Deploy(deploy_tx) => {
                    deploy_tx.hash.hash(&mut hasher);
                }
                Transaction::Call(call_tx) => {
                    call_tx.hash.hash(&mut hasher);
                }
                Transaction::Stake(stake_tx) => {
                    stake_tx.hash.hash(&mut hasher);
                }
                Transaction::AIRequest(ai_tx) => {
                    ai_tx.hash.hash(&mut hasher);
                }
            }
        }

        format!("{:x}", hasher.finish())
    }

    /// Calculate transaction hash for tracking
    fn calculate_transaction_hash(&self, tx: &Transaction) -> String {
        // For now, use the hash field from the transaction
        // In production, this should use the actual transaction hash field or a standardized method
        match tx {
            Transaction::Transfer(transfer_tx) => transfer_tx.hash.clone(),
            Transaction::Deploy(deploy_tx) => deploy_tx.hash.clone(),
            Transaction::Call(call_tx) => call_tx.hash.clone(),
            Transaction::Stake(stake_tx) => stake_tx.hash.clone(),
            Transaction::AIRequest(ai_tx) => ai_tx.hash.clone(),
        }
    }

    // High-Risk Queue Management Methods

    /// Validate a single transaction with high-risk queue integration and audit trail
    pub async fn validate_transaction_with_queue(&self, tx: &Transaction) -> Result<bool, String> {
        // First perform basic validation
        let basic_valid =
            Self::validate_transaction_static(&self.runtime, &self.pqc_manager, tx).await?;
        if !basic_valid {
            return Ok(false);
        }

        // If AI integration is available, check for high-risk flagging
        if let Some(ai_manager) = &self.ai_integration {
            // Convert transaction to AI data format
            let transaction_data = match Self::transaction_to_ai_data(tx) {
                Ok(data) => data,
                Err(e) => {
                    warn!("Failed to serialize transaction for AI analysis: {e}");
                    return Ok(true); // Proceed with basic validation if AI data conversion fails
                }
            };

            let ai_result = ai_manager
                .validate_transaction_with_ai(transaction_data)
                .await;

            match ai_result {
                Ok(ai_integration::AIVerificationResult::Verified {
                    risk_score,
                    processing_decision,
                    fraud_probability,
                    confidence,
                    oracle_id,
                    response_id,
                    ..
                }) => {
                    let tx_hash = self.calculate_transaction_hash(tx);

                    // Determine risk priority based on risk score and fraud probability
                    let risk_priority =
                        if let (Some(risk), Some(fraud)) = (risk_score, fraud_probability) {
                            if risk > 0.8 || fraud > 0.7 {
                                crate::consensus::notification_types::ReviewPriority::Critical
                            } else if risk > 0.6 || fraud > 0.5 {
                                crate::consensus::notification_types::ReviewPriority::High
                            } else if risk > 0.4 || fraud > 0.3 {
                                crate::consensus::notification_types::ReviewPriority::Medium
                            } else {
                                crate::consensus::notification_types::ReviewPriority::Low
                            }
                        } else {
                            crate::consensus::notification_types::ReviewPriority::Medium
                        };

                    // Record audit trail entry for AI decision
                    let audit_result = self
                        .audit_trail
                        .record_ai_decision(crate::consensus::audit_trail::RecordAiDecisionArgs {
                            transaction: tx,
                            transaction_hash: tx_hash.clone(),
                            ai_result: ai_integration::AIVerificationResult::Verified {
                                risk_score,
                                processing_decision: processing_decision.clone(),
                                fraud_probability,
                                confidence,
                                oracle_id: oracle_id.clone(),
                                response_id: response_id.clone(),
                            },
                            risk_decision: processing_decision.clone(),
                            risk_priority,
                            oracle_id: oracle_id.clone(),
                            request_id: response_id.clone(),
                            block_number: None, // Block number not available during validation
                        })
                        .await;

                    if let Err(e) = audit_result {
                        warn!("Failed to record audit trail entry: {e}");
                    }

                    match processing_decision {
                        ai_integration::RiskProcessingDecision::AutoApprove => {
                            info!("Transaction auto-approved by AI: risk score {:.3}, fraud prob {:.3}, confidence {:.3}",
                                  risk_score.unwrap_or(0.0), fraud_probability.unwrap_or(0.0), confidence.unwrap_or(0.0));
                            Ok(true)
                        }
                        ai_integration::RiskProcessingDecision::RequireReview { ref reason } => {
                            // Create AI result for queue
                            let verified_result = ai_integration::AIVerificationResult::Verified {
                                risk_score,
                                processing_decision: processing_decision.clone(),
                                fraud_probability,
                                confidence,
                                oracle_id: oracle_id.clone(),
                                response_id: response_id.clone(),
                            };

                            // Add to high-risk queue
                            match self
                                .high_risk_queue
                                .enqueue_transaction(
                                    tx.clone(),
                                    tx_hash.clone(),
                                    verified_result,
                                    processing_decision.clone(),
                                )
                                .await
                            {
                                Ok(queue_id) => {
                                    info!(
                                        "Transaction {tx_hash} queued for manual review (queue ID: {queue_id}): {reason}"
                                    );
                                    // Return false for now - transaction will be processed after manual approval
                                    Ok(false)
                                }
                                Err(e) => {
                                    warn!("Failed to queue transaction for review: {e}");
                                    // If queueing fails, reject the transaction
                                    Ok(false)
                                }
                            }
                        }
                        ai_integration::RiskProcessingDecision::AutoReject { reason } => {
                            warn!(
                                "Transaction auto-rejected by AI: {} (risk: {:.3}, fraud: {:.3})",
                                reason,
                                risk_score.unwrap_or(0.0),
                                fraud_probability.unwrap_or(0.0)
                            );
                            Ok(false)
                        }
                    }
                }
                Ok(ai_integration::AIVerificationResult::Failed {
                    error,
                    oracle_id,
                    response_id,
                }) => {
                    warn!("AI transaction validation failed: {error}");

                    // Record audit trail entry for failed AI decision
                    let tx_hash = self.calculate_transaction_hash(tx);
                    let audit_result = self
                        .audit_trail
                        .record_ai_decision(crate::consensus::audit_trail::RecordAiDecisionArgs {
                            transaction: tx,
                            transaction_hash: tx_hash,
                            ai_result: ai_integration::AIVerificationResult::Failed {
                                error: error.clone(),
                                oracle_id: oracle_id.clone(),
                                response_id: response_id.clone(),
                            },
                            risk_decision: ai_integration::RiskProcessingDecision::AutoReject {
                                reason: format!("AI validation failed: {error}"),
                            },
                            risk_priority:
                                crate::consensus::notification_types::ReviewPriority::High,
                            oracle_id: oracle_id.clone().unwrap_or_else(|| "unknown".to_string()),
                            request_id: response_id
                                .clone()
                                .unwrap_or_else(|| "unknown".to_string()),
                            block_number: None,
                        })
                        .await;

                    if let Err(e) = audit_result {
                        warn!("Failed to record audit trail entry for failed AI validation: {e}");
                    }

                    if ai_manager.is_ai_verification_required() {
                        return Ok(false);
                    }
                    Ok(true)
                }
                Ok(ai_integration::AIVerificationResult::Unavailable {
                    error,
                    fallback_allowed,
                }) => {
                    // Record audit trail entry for unavailable AI service
                    let tx_hash = self.calculate_transaction_hash(tx);
                    let audit_result = self
                        .audit_trail
                        .record_ai_decision(crate::consensus::audit_trail::RecordAiDecisionArgs {
                            transaction: tx,
                            transaction_hash: tx_hash,
                            ai_result: ai_integration::AIVerificationResult::Unavailable {
                                error: error.clone(),
                                fallback_allowed,
                            },
                            risk_decision: if fallback_allowed {
                                ai_integration::RiskProcessingDecision::AutoApprove
                            } else {
                                ai_integration::RiskProcessingDecision::AutoReject {
                                    reason: "AI service unavailable and fallback not allowed"
                                        .to_string(),
                                }
                            },
                            risk_priority:
                                crate::consensus::notification_types::ReviewPriority::Medium,
                            oracle_id: "unavailable".to_string(),
                            request_id: uuid::Uuid::new_v4().to_string(),
                            block_number: None,
                        })
                        .await;

                    if let Err(e) = audit_result {
                        warn!("Failed to record audit trail entry for unavailable AI service: {e}");
                    }

                    if !fallback_allowed && ai_manager.is_ai_verification_required() {
                        warn!("AI service unavailable and verification required, rejecting transaction");
                        return Ok(false);
                    }
                    Ok(true)
                }
                Ok(ai_integration::AIVerificationResult::Skipped { reason }) => {
                    info!("AI verification skipped: {reason}");

                    // Record audit trail entry for skipped AI verification
                    let tx_hash = self.calculate_transaction_hash(tx);
                    let audit_result = self
                        .audit_trail
                        .record_ai_decision(crate::consensus::audit_trail::RecordAiDecisionArgs {
                            transaction: tx,
                            transaction_hash: tx_hash,
                            ai_result: ai_integration::AIVerificationResult::Skipped {
                                reason: reason.clone(),
                            },
                            risk_decision: ai_integration::RiskProcessingDecision::AutoApprove,
                            risk_priority:
                                crate::consensus::notification_types::ReviewPriority::Low,
                            oracle_id: "skipped".to_string(),
                            request_id: uuid::Uuid::new_v4().to_string(),
                            block_number: None,
                        })
                        .await;

                    if let Err(e) = audit_result {
                        warn!(
                            "Failed to record audit trail entry for skipped AI verification: {e}"
                        );
                    }

                    Ok(true)
                }
                Err(e) => {
                    warn!("AI analysis error: {e}");

                    // Record audit trail entry for AI analysis error
                    let tx_hash = self.calculate_transaction_hash(tx);
                    let audit_result = self
                        .audit_trail
                        .record_ai_decision(crate::consensus::audit_trail::RecordAiDecisionArgs {
                            transaction: tx,
                            transaction_hash: tx_hash,
                            ai_result: ai_integration::AIVerificationResult::Failed {
                                error: format!("Analysis error: {e}"),
                                oracle_id: None,
                                response_id: None,
                            },
                            risk_decision: ai_integration::RiskProcessingDecision::AutoReject {
                                reason: format!("AI analysis error: {e}"),
                            },
                            risk_priority:
                                crate::consensus::notification_types::ReviewPriority::High,
                            oracle_id: "error".to_string(),
                            request_id: uuid::Uuid::new_v4().to_string(),
                            block_number: None,
                        })
                        .await;

                    if let Err(e) = audit_result {
                        warn!("Failed to record audit trail entry for AI analysis error: {e}");
                    }

                    // If AI analysis fails and AI verification is required, reject
                    if ai_manager.is_ai_verification_required() {
                        return Ok(false);
                    }
                    // Otherwise, proceed with basic validation
                    Ok(true)
                }
            }
        } else {
            // No AI integration, transaction passed basic validation
            Ok(true)
        }
    }

    /// Enhanced transaction validation with performance optimization
    pub async fn validate_transaction_optimized(&self, tx: &Transaction) -> Result<bool, String> {
        let tx_hash = self.calculate_transaction_hash(tx);
        let start_time = std::time::Instant::now();

        // First perform basic validation
        let basic_valid =
            Self::validate_transaction_static(&self.runtime, &self.pqc_manager, tx).await?;
        if !basic_valid {
            return Ok(false);
        }

        // Check cache first
        if let Some(cached_result) = self.performance_optimizer.get_cached_result(&tx_hash).await {
            debug!(
                "Using cached AI result for transaction {}",
                hex::encode(&tx_hash)
            );

            // Record cache hit in metrics
            self.performance_optimizer
                .record_request_metrics(start_time.elapsed().as_millis() as u64, true)
                .await;

            return self.process_ai_result(tx, &tx_hash, cached_result).await;
        }

        // Check if we should use fallback validation
        if !self.performance_optimizer.is_service_healthy().await {
            warn!(
                "AI service unhealthy, using fallback validation for transaction {}",
                hex::encode(&tx_hash)
            );

            let fallback_result = self
                .performance_optimizer
                .fallback_validation(&tx_hash, tx)
                .await
                .map_err(|e| format!("Fallback validation failed: {e}"))?;

            // Cache fallback result
            let _ = self
                .performance_optimizer
                .cache_result(&tx_hash, &fallback_result)
                .await;

            // Record audit trail for fallback decision
            let _ = self
                .record_fallback_audit_trail(tx, &tx_hash, &fallback_result)
                .await;

            // Record audit trail for fallback decision
            let decision = match fallback_result {
                ai_integration::AIVerificationResult::Verified {
                    ref processing_decision,
                    ..
                } => processing_decision.clone(),
                _ => ai_integration::RiskProcessingDecision::AutoApprove,
            };
            if let Err(e) = self
                .audit_trail
                .record_ai_decision(crate::consensus::audit_trail::RecordAiDecisionArgs {
                    transaction: tx,
                    transaction_hash: tx_hash.clone(),
                    ai_result: fallback_result.clone(),
                    risk_decision: decision,
                    risk_priority: crate::consensus::notification_types::ReviewPriority::Medium,
                    oracle_id: "fallback_engine".to_string(),
                    request_id: "fallback_request".to_string(),
                    block_number: None,
                })
                .await
            {
                warn!("Failed to record fallback audit trail: {e}");
            }

            self.performance_optimizer
                .record_request_metrics(start_time.elapsed().as_millis() as u64, true)
                .await;

            return self.process_ai_result(tx, &tx_hash, fallback_result).await;
        }

        // Check if we should use graceful degradation
        if self.performance_optimizer.should_degrade().await {
            warn!(
                "System under load, using degraded AI validation for transaction {}",
                hex::encode(&tx_hash)
            );

            // Use simplified AI validation or batching
            if self.performance_optimizer.config.enable_batching {
                // Add to batch for later processing
                match self
                    .performance_optimizer
                    .add_to_batch(tx_hash.clone(), tx.clone())
                    .await
                {
                    Ok(batch_id) => {
                        info!(
                            "Transaction {} added to batch {} due to system load",
                            hex::encode(&tx_hash),
                            batch_id
                        );
                        // For now, auto-approve batched transactions (they'll be processed later)
                        return Ok(true);
                    }
                    Err(e) => {
                        warn!("Failed to add transaction to batch: {e}");
                        // Fall through to regular processing
                    }
                }
            }
        }

        // Regular AI validation with concurrency limiting
        let _permit = self
            .performance_optimizer
            .acquire_request_permit()
            .await
            .map_err(|e| format!("Failed to acquire request permit: {e}"))?;

        let ai_result = if let Some(ai_manager) = &self.ai_integration {
            // Convert transaction to AI data format
            let transaction_data = match Self::transaction_to_ai_data(tx) {
                Ok(data) => data,
                Err(e) => {
                    warn!("Failed to serialize transaction for AI analysis: {e}");

                    self.performance_optimizer
                        .record_request_metrics(start_time.elapsed().as_millis() as u64, false)
                        .await;

                    // Use fallback if serialization fails
                    let fallback_result = self
                        .performance_optimizer
                        .fallback_validation(&tx_hash, tx)
                        .await
                        .map_err(|e| {
                            format!("Fallback validation failed after serialization error: {e}")
                        })?;

                    return self.process_ai_result(tx, &tx_hash, fallback_result).await;
                }
            };

            // Perform AI validation with timeout monitoring
            let ai_validation_start = std::time::Instant::now();
            let ai_result = ai_manager
                .validate_transaction_with_ai(transaction_data)
                .await;
            let ai_duration = ai_validation_start.elapsed();

            // Check for timeout
            if ai_duration.as_secs() > self.performance_optimizer.config.fallback_timeout_threshold
            {
                self.performance_optimizer.record_timeout().await;

                // Activate fallback mode if too many timeouts
                let metrics = self.performance_optimizer.get_metrics().await;
                if metrics.timeout_count > 5 && metrics.total_requests > 0 {
                    let timeout_rate = metrics.timeout_count as f64 / metrics.total_requests as f64;
                    if timeout_rate > 0.2 {
                        // 20% timeout rate
                        warn!(
                            "High timeout rate detected ({}%), activating fallback mode",
                            timeout_rate * 100.0
                        );
                        let _ = self
                            .performance_optimizer
                            .activate_fallback(
                                crate::consensus::performance_optimizer::FallbackMode::PatternBased,
                            )
                            .await;
                    }
                }
            }

            ai_result
        } else {
            // No AI integration available, use fallback
            self.performance_optimizer
                .fallback_validation(&tx_hash, tx)
                .await
                .map_err(|e| anyhow::anyhow!("Fallback validation failed: {}", e))
        };

        // Release the permit
        self.performance_optimizer.release_request_permit().await;

        match ai_result {
            Ok(result) => {
                // Cache the result
                let _ = self
                    .performance_optimizer
                    .cache_result(&tx_hash, &result)
                    .await;

                // Record successful request metrics
                self.performance_optimizer
                    .record_request_metrics(start_time.elapsed().as_millis() as u64, true)
                    .await;

                // Record audit trail
                let _ = self.record_ai_audit_trail(tx, &tx_hash, &result).await;

                self.process_ai_result(tx, &tx_hash, result).await
            }
            Err(e) => {
                warn!(
                    "AI validation failed for transaction {}: {}",
                    hex::encode(&tx_hash),
                    e
                );

                // Record failed request metrics
                self.performance_optimizer
                    .record_request_metrics(start_time.elapsed().as_millis() as u64, false)
                    .await;

                // Use fallback validation
                let fallback_result = self
                    .performance_optimizer
                    .fallback_validation(&tx_hash, tx)
                    .await
                    .map_err(|e| format!("AI validation failed and fallback also failed: {e}"))?;

                // Cache fallback result
                let _ = self
                    .performance_optimizer
                    .cache_result(&tx_hash, &fallback_result)
                    .await;

                // Record audit trail for fallback
                let _ = self
                    .record_fallback_audit_trail(tx, &tx_hash, &fallback_result)
                    .await;

                self.process_ai_result(tx, &tx_hash, fallback_result).await
            }
        }
    }

    /// Process AI result and determine transaction acceptance
    async fn process_ai_result(
        &self,
        tx: &Transaction,
        tx_hash: &str,
        ai_result: ai_integration::AIVerificationResult,
    ) -> Result<bool, String> {
        match ai_result {
            ai_integration::AIVerificationResult::Verified {
                ref processing_decision,
                ..
            } => {
                match processing_decision {
                    ai_integration::RiskProcessingDecision::AutoApprove => {
                        debug!("Transaction {} auto-approved by AI", hex::encode(tx_hash));
                        Ok(true)
                    }
                    ai_integration::RiskProcessingDecision::RequireReview { ref reason } => {
                        // Queue the transaction for manual review using the high-risk queue
                        match self
                            .high_risk_queue
                            .enqueue_transaction(
                                tx.clone(),
                                tx_hash.to_string(),
                                ai_result.clone(),
                                processing_decision.clone(),
                            )
                            .await
                        {
                            Ok(queue_id) => {
                                info!(
                                    "Transaction {tx_hash} queued for manual review (queue ID: {queue_id}): {reason}"
                                );
                                // Transaction will be processed after manual approval
                                Ok(false)
                            }
                            Err(e) => {
                                warn!("Failed to queue transaction for review: {e}");
                                // Reject if queueing fails
                                Ok(false)
                            }
                        }
                    }
                    ai_integration::RiskProcessingDecision::AutoReject { reason } => {
                        warn!(
                            "Transaction {} auto-rejected by AI: {}",
                            hex::encode(tx_hash),
                            reason
                        );
                        Ok(false)
                    }
                }
            }
            ai_integration::AIVerificationResult::Failed { error, .. } => {
                warn!(
                    "AI verification failed for transaction {}: {}",
                    hex::encode(tx_hash),
                    error
                );
                Ok(false)
            }
            ai_integration::AIVerificationResult::Unavailable {
                error,
                fallback_allowed,
            } => {
                if fallback_allowed {
                    warn!(
                        "AI service unavailable but fallback allowed for transaction {}",
                        hex::encode(tx_hash)
                    );
                    Ok(true)
                } else {
                    warn!(
                        "AI service unavailable and fallback not allowed for transaction {}: {}",
                        hex::encode(tx_hash),
                        error
                    );
                    Ok(false)
                }
            }
            ai_integration::AIVerificationResult::Skipped { reason } => {
                info!(
                    "AI verification skipped for transaction {}: {}",
                    hex::encode(tx_hash),
                    reason
                );
                Ok(true)
            }
        }
    }

    /// Record audit trail for AI decisions
    async fn record_ai_audit_trail(
        &self,
        tx: &Transaction,
        tx_hash: &str,
        ai_result: &ai_integration::AIVerificationResult,
    ) -> Result<(), String> {
        // Determine priority and decision based on AI result
        let (priority, decision) = match ai_result {
            ai_integration::AIVerificationResult::Verified {
                processing_decision,
                risk_score,
                ..
            } => {
                let priority = if let Some(score) = risk_score {
                    if *score > 0.8 {
                        crate::consensus::notification_types::ReviewPriority::Critical
                    } else if *score > 0.6 {
                        crate::consensus::notification_types::ReviewPriority::High
                    } else if *score > 0.4 {
                        crate::consensus::notification_types::ReviewPriority::Medium
                    } else {
                        crate::consensus::notification_types::ReviewPriority::Low
                    }
                } else {
                    crate::consensus::notification_types::ReviewPriority::Medium
                };
                (priority, processing_decision.clone())
            }
            _ => (
                crate::consensus::notification_types::ReviewPriority::Medium,
                ai_integration::RiskProcessingDecision::AutoApprove,
            ),
        };

        let oracle_id = match ai_result {
            ai_integration::AIVerificationResult::Verified { oracle_id, .. } => oracle_id.clone(),
            _ => "unknown".to_string(),
        };

        let response_id = match ai_result {
            ai_integration::AIVerificationResult::Verified { response_id, .. } => {
                response_id.clone()
            }
            _ => uuid::Uuid::new_v4().to_string(),
        };

        let audit_result = self
            .audit_trail
            .record_ai_decision(crate::consensus::audit_trail::RecordAiDecisionArgs {
                transaction: tx,
                transaction_hash: self.calculate_transaction_hash(tx),
                ai_result: ai_result.clone(),
                risk_decision: decision.clone(),
                risk_priority: priority,
                oracle_id: oracle_id.clone(),
                request_id: response_id.clone(),
                block_number: None,
            })
            .await;

        match audit_result {
            Ok(audit_id) => {
                debug!("Recorded audit trail entry {audit_id} for transaction {tx_hash}");
                Ok(())
            }
            Err(e) => {
                warn!("Failed to record audit trail for transaction {tx_hash}: {e}");
                Err(format!("Audit trail recording failed: {e}"))
            }
        }
    }

    /// Record audit trail for fallback decisions
    async fn record_fallback_audit_trail(
        &self,
        tx: &Transaction,
        tx_hash: &str,
        fallback_result: &ai_integration::AIVerificationResult,
    ) -> Result<(), String> {
        // Similar to regular audit trail but mark as fallback
        self.record_ai_audit_trail(tx, tx_hash, fallback_result)
            .await
    }

    /// Get performance optimizer reference
    pub fn get_performance_optimizer(
        &self,
    ) -> &Arc<crate::consensus::performance_optimizer::PerformanceOptimizer> {
        &self.performance_optimizer // fixed field name
    }

    /// Process pending batches of transactions
    pub async fn process_pending_batches(&self) -> Result<usize, String> {
        let mut processed_count = 0;

        while let Some(batch) = self.performance_optimizer.get_next_batch().await {
            info!(
                "Processing batch {} with {} transactions",
                batch.batch_id,
                batch.transactions.len()
            );

            for (tx_hash, transaction) in batch.transactions {
                match self.validate_transaction_optimized(&transaction).await {
                    Ok(valid) => {
                        if valid {
                            debug!(
                                "Batch transaction {} validated successfully",
                                hex::encode(&tx_hash)
                            );
                        } else {
                            debug!(
                                "Batch transaction {} validation failed",
                                hex::encode(&tx_hash)
                            );
                        }
                        processed_count += 1;
                    }
                    Err(e) => {
                        warn!(
                            "Error validating batch transaction {}: {}",
                            hex::encode(&tx_hash),
                            e
                        );
                    }
                }
            }
        }

        if processed_count > 0 {
            info!("Processed {processed_count} transactions from batches");
        }

        Ok(processed_count)
    }

    /// Cleanup performance optimizer caches and metrics
    pub async fn cleanup_performance_optimizer(&self) -> Result<(), String> {
        // Cleanup expired cache entries
        match self.performance_optimizer.cleanup_cache().await {
            Ok(removed) => {
                if removed > 0 {
                    info!("Performance optimizer cleaned up {removed} cache entries");
                }
            }
            Err(e) => {
                warn!("Failed to cleanup performance optimizer cache: {e}");
            }
        }

        Ok(())
    }

    /// Get current performance metrics
    pub async fn get_performance_metrics(
        &self,
    ) -> crate::consensus::performance_optimizer::PerformanceMetrics {
        self.performance_optimizer.get_metrics().await
    }
}
