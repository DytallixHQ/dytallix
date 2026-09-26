use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Serialize, Deserialize};
use log::{info, debug, warn};
use chrono::{DateTime, Utc};
use serde_json;
use std::path::Path;

use crate::runtime::DytallixRuntime;
use crate::crypto::{PQCManager, PQCSignature};
use crate::types::{Transaction, Block, BlockHeader, AIRequestTransaction, AIServiceType, TransferTransaction}; // Import from types

// AI Service Integration
use std::collections::HashMap;
use tokio::time::Duration;
use reqwest::Client;
use anyhow::{Result, anyhow};
use serde::de::DeserializeOwned;
use serde_json::Value;

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
                        return Err(anyhow!(
                            "AI service error: {}", resp.status()
                        ));
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
    pub async fn request_analysis(&self, request: &AIAnalysisRequest) -> Result<SignedAIOracleResponse> {
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
    validators: Arc<RwLock<Vec<String>>>,
    is_validator: bool,
    ai_client: AIOracleClient,
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
                        info!("Available algorithms: {}, {}, {}",
                              store.dilithium.algorithm, store.falcon.algorithm, store.sphincs.algorithm);
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
        Ok(Self {
            runtime,
            pqc_manager,
            current_block: Arc::new(RwLock::new(None)),
            validators: Arc::new(RwLock::new(Vec::new())),
            is_validator: true, // For development
            ai_client,
        })
    }

    fn generate_and_store_keys(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;

        let (d_pk, d_sk) = dilithium5::keypair();
        let (f_pk, f_sk) = falcon1024::keypair();
        let (s_pk, s_sk) = sphincssha2128ssimple::keypair();

        let store = NodeKeyStore {
            dilithium: StoredKeyPair {
                algorithm: "Dilithium5".to_string(),
                public_key: d_pk.as_bytes().to_vec(),
                secret_key: d_sk.as_bytes().to_vec(),
            },
            falcon: StoredKeyPair {
                algorithm: "Falcon1024".to_string(),
                public_key: f_pk.as_bytes().to_vec(),
                secret_key: f_sk.as_bytes().to_vec(),
            },
            sphincs: StoredKeyPair {
                algorithm: "SphincsSha256128s".to_string(),
                public_key: s_pk.as_bytes().to_vec(),
                secret_key: s_sk.as_bytes().to_vec(),
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
    pub async fn discover_ai_services(&self) -> Result<Vec<AIServiceInfo>, Box<dyn std::error::Error>> {
        let services = self.ai_client.discover_services().await?;
        info!("Discovered {} AI services", services.len());
        for service in &services {
            debug!("AI Service: {} - Type: {:?} - Availability: {:.2}",
                   service.service_id, service.service_type, service.availability_score);
        }
        Ok(services)
    }

    /// Request AI analysis for a transaction or data
    pub async fn request_ai_analysis(
        &self,
        service_type: AIServiceType,
        data: HashMap<String, Value>
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
            warn!("AI analysis confidence score below threshold: {}", response.confidence_score);
        }

        Ok(response)
    }

    async fn start_validator_loop(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Starting validator loop...");

        let runtime = Arc::clone(&self.runtime);
        let pqc_manager = Arc::clone(&self.pqc_manager);
        let current_block = Arc::clone(&self.current_block);

        tokio::spawn(async move {
            let mut block_number = 0u64;

            loop {
                debug!("Validator tick - producing block #{}", block_number);

                // Create a sample transaction for demonstration
                let mut sample_tx = crate::types::TransferTransaction {
                    hash: String::new(), // Will be calculated
                    from: "dyt1genesis".to_string(),
                    to: format!("dyt1addr{}", block_number % 5), // Rotate between addresses
                    amount: 100 + (block_number * 10), // Variable amounts
                    fee: 1,
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
                match Self::create_block_proposal(&runtime, &pqc_manager, &current_block, transactions, block_number).await {
                    Ok(block) => {
                        info!("✅ Successfully created block #{} with {} transactions",
                              block.header.number, block.transactions.len());

                        // Validate the block
                        match Self::validate_block_static(&runtime, &pqc_manager, &block).await {
                            Ok(true) => {
                                info!("✅ Block #{} validation successful", block.header.number);

                                // Apply block to state
                                if let Err(e) = Self::apply_block_to_state(&runtime, &block).await {
                                    log::error!("Failed to apply block to state: {}", e);
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
                                log::error!("❌ Error validating block #{}: {}", block.header.number, e);
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to create block proposal: {}", e);
                    }
                }

                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
            }
        });

        Ok(())
    }

    pub async fn propose_block(&self, transactions: Vec<Transaction>) -> Result<Block, String> {
        Self::create_block_proposal(&self.runtime, &self.pqc_manager, &self.current_block, transactions, 0).await
    }

    pub async fn validate_block(&self, block: &Block) -> Result<bool, String> {
        Self::validate_block_static(&self.runtime, &self.pqc_manager, block).await
    }

    fn calculate_merkle_root(&self, transactions: &[Transaction]) -> String {
        Self::calculate_merkle_root_static(transactions)
    }

    // Static helper methods for use in async tasks
    async fn create_block_proposal(
        runtime: &Arc<DytallixRuntime>,
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

        };

        let mut block = Block {
            header,
            transactions,
        };

        // Sign the block with PQC signature
        let block_hash = Self::calculate_block_hash_static(&block.header);
        let signature = pqc_manager.sign_message(block_hash.as_bytes())
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
        let is_valid = pqc_manager.verify_signature(
            block_hash.as_bytes(),
            &crate::crypto::PQCSignature {
                signature: block.header.signature.signature.data.clone(),
                algorithm: format!("{:?}", block.header.signature.signature.algorithm),
                nonce: 0,
                timestamp: 0,
            },
            &block.header.signature.public_key,
        ).map_err(|e| e.to_string())?;

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

    async fn validate_transaction_static(
        runtime: &Arc<DytallixRuntime>,
        pqc_manager: &Arc<PQCManager>,
        tx: &Transaction,
    ) -> Result<bool, String> {
        // Validate signature for any transaction type
        if !Self::validate_any_transaction_signature(pqc_manager, tx)? {
            return Ok(false);
        }

        match tx {
            Transaction::Transfer(transfer_tx) => {
                // Basic validation
                if transfer_tx.amount == 0 {
                    return Ok(false);
                }

                // Check AI risk score if present
                if let Some(risk_score) = transfer_tx.ai_risk_score {
                    if risk_score > 0.8 {
                        info!("Transaction rejected due to high AI risk score: {}", risk_score);
                        return Ok(false);
                    }
                }

                // Check balance for transfers (skip for genesis)
                if transfer_tx.from != "dyt1genesis" {
                    let balance = runtime.get_balance(&transfer_tx.from).await
                        .map_err(|e| e.to_string())?;
                    if balance < transfer_tx.amount {
                        info!("Transaction rejected due to insufficient balance: {} < {}", balance, transfer_tx.amount);
                        return Ok(false);
                    }
                }

                if !Self::validate_transaction_signature_static(pqc_manager, tx)? {
                    return Ok(false);
                }
                Ok(true)
            }
            Transaction::Deploy(_) => {
                // TODO: Implement contract deployment validation
                Ok(true)
            }
            Transaction::Call(_) => {
                // TODO: Implement contract call validation
                Ok(true)
            }
            Transaction::Stake(_) => {
                // TODO: Implement staking validation
                Ok(true)
            }
            Transaction::AIRequest(ai_request_tx) => {
                // Validate AI request transaction
                if ai_request_tx.service_type == AIServiceType::Unknown {
                    return Ok(false);
                }

                // Check for required fields based on service type
                match ai_request_tx.service_type {
                    AIServiceType::KYC | AIServiceType::AML => {
                        if ai_request_tx.payload.get("identity").is_none() {
                            return Ok(false);
                        }
                    },
                    AIServiceType::CreditAssessment => {
                        if ai_request_tx.payload.get("social_security_number").is_none() {
                            return Ok(false);
                        }
                    },
                    _ => {}
                }

                // Check AI risk score if present
                if let Some(risk_score) = ai_request_tx.ai_risk_score {
                    if risk_score > 0.8 {
                        info!("AI request rejected due to high risk score: {}", risk_score);
                        return Ok(false);
                    }
                }

                Ok(true)
            }
        }
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
                        let sender_balance = runtime.get_balance(&transfer_tx.from).await.unwrap_or(0);
                        if sender_balance >= transfer_tx.amount {
                            runtime.set_balance(&transfer_tx.from, sender_balance - transfer_tx.amount).await
                                .map_err(|e| e.to_string())?;
                            runtime.increment_nonce(&transfer_tx.from).await
                                .map_err(|e| e.to_string())?;
                        }
                    }

                    // Add to recipient
                    let recipient_balance = runtime.get_balance(&transfer_tx.to).await.unwrap_or(0);
                    runtime.set_balance(&transfer_tx.to, recipient_balance + transfer_tx.amount).await
                        .map_err(|e| e.to_string())?;

                    info!("Applied transfer: {} -> {} ({})", transfer_tx.from, transfer_tx.to, transfer_tx.amount);
                }
                Transaction::Deploy(deploy_tx) => {
                    // TODO: Deploy smart contract
                    info!("Applied contract deployment: {}", deploy_tx.hash);
                }
                Transaction::Call(call_tx) => {
                    // TODO: Execute smart contract call
                    info!("Applied contract call: {}", call_tx.hash);
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
        runtime.save_state().await.map_err(|e| e.to_string())?;

        Ok(())
    }

    fn format_transfer_transaction_message(tx: &TransferTransaction) -> Vec<u8> {
        format!(
            "{}:{}:{}:{}:{}:{}",
            tx.from, tx.to, tx.amount, tx.fee, tx.nonce, tx.timestamp
        )
        .into_bytes()
    }

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
                            algorithm: format!(
                                "{:?}",
                                transfer_tx.signature.signature.algorithm
                            ),
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
}
