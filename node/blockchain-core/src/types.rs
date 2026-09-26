// Core blockchain types for Dytallix
// Post-Quantum Cryptography Enhanced Blockchain

use dytallix_pqc::{Signature, SignatureAlgorithm};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha3::{Digest, Sha3_256};
use std::fmt;
use uuid::Uuid;

/// Canonical type for all monetary amounts, balances, stakes, and fees.
/// Uses u128 to handle large values without overflow.
pub type Tokens = u128;

/// Canonical type for all gas-related fields and counters.
/// Uses u64 as gas amounts are bounded and for FFI safety with WASM.
pub type Gas = u64;

/// Dytallix address format (dyt1...)
pub type Address = String;

/// Block hash (32 bytes as hex string)
pub type Hash = String;

/// Transaction hash (32 bytes as hex string)
pub type TxHash = String;

/// Block number
pub type BlockNumber = u64;

/// Amount in smallest unit (like satoshis) - now uses canonical Tokens type
pub type Amount = Tokens;

/// Transaction fee in smallest unit - now uses canonical Tokens type
pub type Fee = Tokens;

/// Account balance in smallest unit - now uses canonical Tokens type
pub type Balance = Tokens;

/// Staking amount in smallest unit - now uses canonical Tokens type
pub type Stake = Tokens;

/// Unix timestamp (seconds since epoch)
pub type Timestamp = u64;

/// Serde module for serializing u128 values as decimal strings in JSON.
/// This prevents precision loss in JavaScript and other JSON consumers.
pub mod serde_u128_string {
    use serde::de::Error;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &u128, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u128, D::Error> {
        let s = String::deserialize(d)?;
        s.parse::<u128>().map_err(D::Error::custom)
    }
}

/// Serde module for optional u128 values serialized as decimal strings.
pub mod serde_opt_u128_string {
    use serde::de::Error;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &Option<u128>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(val) => s.serialize_str(&val.to_string()),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u128>, D::Error> {
        let opt: Option<String> = Option::deserialize(d)?;
        match opt {
            Some(s) => s.parse::<u128>().map(Some).map_err(D::Error::custom),
            None => Ok(None),
        }
    }
}

/// Serde module for accepting both string and number input (for API compatibility).
/// Always serializes as string to avoid precision issues.
pub mod serde_string_or_number {
    use serde::de::{self, Visitor};
    use serde::{Deserializer, Serializer};
    use std::fmt;

    pub fn serialize<S: Serializer>(v: &u128, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u128, D::Error> {
        struct U128Visitor;

        impl<'de> Visitor<'de> for U128Visitor {
            type Value = u128;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string or number representing u128")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                v.parse::<u128>().map_err(de::Error::custom)
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(v as u128)
            }

            fn visit_u128<E: de::Error>(self, v: u128) -> Result<Self::Value, E> {
                Ok(v)
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                if v >= 0 {
                    Ok(v as u128)
                } else {
                    Err(de::Error::custom("negative numbers not allowed"))
                }
            }
        }

        d.deserialize_any(U128Visitor)
    }
}

/// Dytallix Block Structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
}

/// Block Header with PQC Validator Signature
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    /// Block number in the chain
    pub number: BlockNumber,

    /// Hash of the previous block
    pub parent_hash: Hash,

    /// Merkle root of all transactions in this block
    pub transactions_root: Hash,

    /// State root after applying all transactions
    pub state_root: Hash,

    /// Block timestamp
    pub timestamp: Timestamp,

    /// Address of the validator who produced this block
    pub validator: Address,

    /// Post-quantum signature of the block hash by validator
    pub signature: PQCBlockSignature,

    /// Nonce for PoW (if hybrid consensus)
    pub nonce: u64,
}

/// Post-Quantum Signature for Block Validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PQCBlockSignature {
    /// The signature data
    pub signature: Signature,

    /// Public key of the signer (validator)
    pub public_key: Vec<u8>,
}

/// Dytallix Transaction Types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Transaction {
    /// Simple transfer between accounts
    Transfer(TransferTransaction),

    /// Smart contract deployment
    Deploy(DeployTransaction),

    /// Smart contract call
    Call(CallTransaction),

    /// Validator staking transaction
    Stake(StakeTransaction),

    /// AI service request transaction
    AIRequest(AIRequestTransaction),
}

/// Simple Transfer Transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferTransaction {
    /// Transaction hash
    pub hash: TxHash,

    /// Sender address
    pub from: Address,

    /// Recipient address
    pub to: Address,

    /// Amount to transfer
    #[serde(with = "crate::types::serde_u128_string")]
    pub amount: Amount,

    /// Transaction fee
    #[serde(with = "crate::types::serde_u128_string")]
    pub fee: Fee,

    /// Transaction nonce (to prevent replay attacks)
    pub nonce: u64,

    /// Timestamp when transaction was created
    pub timestamp: Timestamp,

    /// Post-quantum signature by sender
    pub signature: PQCTransactionSignature,

    /// AI-calculated risk score (0.0 = low risk, 1.0 = high risk)
    pub ai_risk_score: Option<f64>,
}

/// Smart Contract Deployment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployTransaction {
    pub hash: TxHash,
    pub from: Address,
    pub contract_code: Vec<u8>,
    pub constructor_args: Vec<u8>,
    pub gas_limit: Gas,
    pub gas_price: Gas,
    #[serde(with = "crate::types::serde_u128_string")]
    pub fee: Fee,
    pub nonce: u64,
    pub timestamp: Timestamp,
    pub signature: PQCTransactionSignature,
}

impl DeployTransaction {
    /// Calculate hash of deployment data
    pub fn calculate_hash(&self) -> TxHash {
        let data = format!(
            "{}:{}:{}:{}:{}:{}:{}",
            self.from,
            hex::encode(&self.contract_code),
            hex::encode(&self.constructor_args),
            self.gas_limit,
            self.gas_price,
            self.nonce,
            self.timestamp
        );
        let mut hasher = Sha3_256::new();
        hasher.update(data.as_bytes());
        hex::encode(hasher.finalize())
    }

    /// Format signing message
    pub fn signing_message(&self) -> Vec<u8> {
        self.calculate_hash().as_bytes().to_vec()
    }
}

/// Smart Contract Call
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallTransaction {
    pub hash: TxHash,
    pub from: Address,
    pub to: Address, // Contract address
    pub method: String,
    pub args: Vec<u8>,
    #[serde(with = "crate::types::serde_u128_string")]
    pub value: Amount,
    pub gas_limit: Gas,
    pub gas_price: Gas,
    #[serde(with = "crate::types::serde_u128_string")]
    pub fee: Fee,
    pub nonce: u64,
    pub timestamp: Timestamp,
    pub signature: PQCTransactionSignature,
}

impl CallTransaction {
    /// Calculate hash of call transaction data
    pub fn calculate_hash(&self) -> TxHash {
        let data = format!(
            "{}:{}:{}:{}:{}:{}:{}:{}:{}",
            self.from,
            self.to,
            self.method,
            hex::encode(&self.args),
            self.value,
            self.gas_limit,
            self.gas_price,
            self.nonce,
            self.timestamp
        );
        let mut hasher = Sha3_256::new();
        hasher.update(data.as_bytes());
        hex::encode(hasher.finalize())
    }

    /// Format signing message
    pub fn signing_message(&self) -> Vec<u8> {
        self.calculate_hash().as_bytes().to_vec()
    }
}

/// Validator Staking Transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakeTransaction {
    pub hash: TxHash,
    pub validator: Address,
    #[serde(with = "crate::types::serde_u128_string")]
    pub amount: Stake,
    pub action: StakeAction,
    #[serde(with = "crate::types::serde_u128_string")]
    pub fee: Fee,
    pub nonce: u64,
    pub timestamp: Timestamp,
    pub signature: PQCTransactionSignature,
}

impl StakeTransaction {
    /// Calculate hash of staking transaction data
    pub fn calculate_hash(&self) -> TxHash {
        let data = format!(
            "{}:{}:{:?}:{}:{}:{}",
            self.validator, self.amount, self.action, self.fee, self.nonce, self.timestamp
        );
        let mut hasher = Sha3_256::new();
        hasher.update(data.as_bytes());
        hex::encode(hasher.finalize())
    }

    /// Format signing message
    pub fn signing_message(&self) -> Vec<u8> {
        self.calculate_hash().as_bytes().to_vec()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StakeAction {
    Stake,
    Unstake,
    Delegate { to: Address },
    Undelegate,
}

/// AI Service Request Transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIRequestTransaction {
    pub hash: TxHash,
    pub from: Address,
    pub service_type: AIServiceType,
    pub request_data: Vec<u8>,
    pub payload: serde_json::Value, // Added for compatibility
    pub ai_risk_score: Option<f64>, // Added for risk scoring
    pub ai_response: Option<serde_json::Value>, // Added for AI response storage
    #[serde(with = "crate::types::serde_u128_string")]
    pub fee: Fee,
    pub nonce: u64,
    pub timestamp: Timestamp,
    pub signature: PQCTransactionSignature,
}

impl AIRequestTransaction {
    /// Calculate hash of AI request data
    pub fn calculate_hash(&self) -> TxHash {
        let data = format!(
            "{}:{:?}:{}:{}:{}:{}",
            self.from,
            self.service_type,
            hex::encode(&self.request_data),
            self.fee,
            self.nonce,
            self.timestamp
        );
        let mut hasher = Sha3_256::new();
        hasher.update(data.as_bytes());
        hex::encode(hasher.finalize())
    }

    /// Format signing message
    pub fn signing_message(&self) -> Vec<u8> {
        self.calculate_hash().as_bytes().to_vec()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AIServiceType {
    FraudDetection,
    RiskScoring,
    ContractAnalysis,
    AddressReputation,
    Kyc,
    Aml,
    CreditAssessment,
    Unknown,
}

/// Payload sent to external AI services
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIRequestPayload {
    /// Unique request identifier
    pub id: String,
    /// Type of AI service being requested
    pub service_type: AIServiceType,
    /// Arbitrary request data encoded as JSON
    pub request_data: Value,
    /// Timestamp of the request
    pub timestamp: Timestamp,
}

impl AIRequestPayload {
    /// Create a new request payload with a random ID and current timestamp
    pub fn _new(service_type: AIServiceType, request_data: Value) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            service_type,
            request_data,
            timestamp: chrono::Utc::now().timestamp() as u64,
        }
    }

    /// Serialize this payload to a JSON string
    pub fn _to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserialize a payload from a JSON string
    pub fn _from_json(data: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(data)
    }
}

/// Payload returned from external AI services
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIResponsePayload {
    /// ID of the corresponding request
    pub id: String,
    /// Whether the request succeeded
    pub success: bool,
    /// Result data provided by the AI service
    pub result_data: Value,
    /// Optional error message
    pub error: Option<String>,
    /// Timestamp of the response
    pub timestamp: Timestamp,
}

impl AIResponsePayload {
    /// Serialize this payload to a JSON string
    pub fn _to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserialize a payload from a JSON string
    pub fn _from_json(data: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(data)
    }
}

/// Post-Quantum Signature for Transactions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PQCTransactionSignature {
    /// The signature data
    pub signature: Signature,

    /// Public key of the signer
    pub public_key: Vec<u8>,
}

/// Account State
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AccountState {
    /// Account balance
    #[serde(with = "crate::types::serde_u128_string")]
    pub balance: Balance,

    /// Transaction nonce
    pub nonce: u64,

    /// Smart contract code (if this is a contract account)
    pub code: Option<Vec<u8>>,

    /// Contract storage (if this is a contract account)
    pub storage: std::collections::HashMap<String, Vec<u8>>,

    /// AI reputation score (0-1000)
    pub reputation_score: u16,

    /// Last AI analysis timestamp
    pub last_ai_analysis: Option<Timestamp>,
}

/// Validator Information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorInfo {
    /// Validator address
    pub address: Address,

    /// Staked amount
    #[serde(with = "crate::types::serde_u128_string")]
    pub stake: Stake,

    /// Public key for block signing
    pub public_key: Vec<u8>,

    /// Signature algorithm used
    pub signature_algorithm: SignatureAlgorithm,

    /// Is currently active
    pub active: bool,

    /// Commission rate (basis points)
    pub commission: u16,
}

/// Transaction execution status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TxStatus {
    Pending,
    Success,
    Failed,
}

/// Transaction receipt (persisted once included in a block)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TxReceipt {
    /// Hash of the transaction this receipt corresponds to
    pub tx_hash: TxHash,
    /// Block number that included the transaction
    pub block_number: BlockNumber,
    /// Execution status
    pub status: TxStatus,
    /// Gas actually used - now uses canonical Gas type (u64)
    pub gas_used: Gas,
    /// Fee actually paid (can differ from quoted fee in future)
    #[serde(with = "crate::types::serde_u128_string")]
    pub fee_paid: Amount,
    /// Inclusion timestamp (block timestamp)
    pub timestamp: Timestamp,
    /// Transaction index within block
    pub index: u32,
    /// Optional execution error (present if status == failed)
    pub error: Option<String>,
    /// Optional contract address (for deploy or call)
    pub contract_address: Option<String>,
    /// Execution logs captured from WASM host env
    pub logs: Vec<String>,
    /// Optional return data from contract call
    pub return_data: Option<Vec<u8>>,
}

/// Transaction Pool for managing pending transactions
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug)]
pub struct TransactionPool {
    /// Pending transactions organized by fee (highest fee first)
    pending: Arc<RwLock<BTreeMap<u128, Vec<Transaction>>>>,
    /// Transaction lookup by hash
    lookup: Arc<RwLock<HashMap<TxHash, Transaction>>>,
    /// Maximum pool size
    max_size: usize,
}

impl TransactionPool {
    pub fn new(max_size: usize) -> Self {
        Self {
            pending: Arc::new(RwLock::new(BTreeMap::new())),
            lookup: Arc::new(RwLock::new(HashMap::new())),
            max_size,
        }
    }

    /// Add a transaction to the pool
    pub async fn add_transaction(&self, tx: Transaction) -> Result<TxHash, String> {
        let tx_hash = tx.hash();
        let fee = tx.fee();

        // Check if transaction already exists
        {
            let lookup = self.lookup.read().await;
            if lookup.contains_key(&tx_hash) {
                return Err("Transaction already in pool".to_string());
            }
        }

        // Add to pending transactions
        {
            let mut pending = self.pending.write().await;
            let mut lookup = self.lookup.write().await;

            // Check pool size limit
            if lookup.len() >= self.max_size {
                // Remove lowest fee transaction
                if let Some((lowest_fee, txs)) = pending.iter_mut().next() {
                    if let Some(removed_tx) = txs.pop() {
                        lookup.remove(&removed_tx.hash());
                    }
                    if txs.is_empty() {
                        let lowest_fee = *lowest_fee;
                        pending.remove(&lowest_fee);
                    }
                }
            }

            pending.entry(fee).or_insert_with(Vec::new).push(tx.clone());
            lookup.insert(tx_hash.clone(), tx);
        }

        Ok(tx_hash)
    }

    /// Get transactions with highest fees for block creation
    pub async fn _get_pending_transactions(&self, max_count: usize) -> Vec<Transaction> {
        let pending = self.pending.read().await;
        let mut transactions = Vec::new();

        // Iterate from highest fee to lowest
        for (_, txs) in pending.iter().rev() {
            for tx in txs {
                if transactions.len() >= max_count {
                    break;
                }
                transactions.push(tx.clone());
            }
            if transactions.len() >= max_count {
                break;
            }
        }

        transactions
    }

    /// Remove transactions that have been included in a block
    pub async fn _remove_transactions(&self, tx_hashes: &[TxHash]) {
        let mut pending = self.pending.write().await;
        let mut lookup = self.lookup.write().await;

        for tx_hash in tx_hashes {
            if let Some(tx) = lookup.remove(tx_hash) {
                let fee = tx.fee();
                if let Some(txs) = pending.get_mut(&fee) {
                    txs.retain(|t| t.hash() != *tx_hash);
                    if txs.is_empty() {
                        pending.remove(&fee);
                    }
                }
            }
        }
    }

    /// Get current pool statistics
    pub async fn get_stats(&self) -> PoolStats {
        let lookup = self.lookup.read().await;
        let pending = self.pending.read().await;

        PoolStats {
            total_transactions: lookup.len(),
            fee_levels: pending.len(),
            max_size: self.max_size,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PoolStats {
    pub total_transactions: usize,
    pub fee_levels: usize,
    pub max_size: usize,
}

impl fmt::Display for PoolStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Pool: {}/{} transactions, {} fee levels",
            self.total_transactions, self.max_size, self.fee_levels
        )
    }
}

/// Node operational status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NodeStatus {
    Starting,
    Running,
    Syncing,
    Stopping,
    Stopped,
    Error(String),
}

// Implementation methods
impl Block {
    /// Calculate the hash of this block
    pub fn hash(&self) -> Hash {
        let header_bytes = bincode::serialize(&self.header).unwrap();
        let mut hasher = Sha3_256::new();
        hasher.update(&header_bytes);
        let hash = hasher.finalize();
        hex::encode(hash)
    }

    /// Verify all transactions in this block
    pub fn verify_transactions(&self) -> bool {
        // Basic checks: we have transactions
        if self.transactions.is_empty() {
            return false;
        }

        // Verify each transaction's signature
        for tx in &self.transactions {
            if !tx.verify_signature() {
                return false;
            }
        }

        true
    }

    // Verify all transactions in this block with AI signature verification
    // TODO: Re-enable after fixing import paths
    /*
    pub async fn verify_transactions_with_ai(&self, ai_integration: &AIIntegrationManager) -> Result<bool, Box<dyn std::error::Error>> {
        // First run basic verification
        if !self.verify_transactions() {
            return Ok(false);
        }

        // If AI verification is not required, skip it
        if !ai_integration.is_ai_verification_required() {
            return Ok(true);
        }

        // Verify AI responses for AI request transactions
        for tx in &self.transactions {
            if let Transaction::AIRequest(ai_tx) = tx {
                if let Some(ref response) = ai_tx.ai_response {
                    // Try to parse the response as a signed AI oracle response
                    if let Ok(signed_response) = serde_json::from_value::<SignedAIOracleResponse>(response.clone()) {
                        let verification_result = ai_integration.verify_ai_response(&signed_response, None).await;
                        match verification_result {
                            AIVerificationResult::Verified { .. } => continue,
                            _ => return Ok(false),
                        }
                    } else {
                        // If it's not a signed response, skip verification for now
                        continue;
                    }
                }
            }
        }

        Ok(true)
    }
    */
}

impl BlockHeader {
    /// Calculate merkle root of transactions
    pub fn calculate_transactions_root(transactions: &[Transaction]) -> Hash {
        if transactions.is_empty() {
            return "0".repeat(64);
        }

        let tx_hashes: Vec<String> = transactions.iter().map(|tx| tx.hash()).collect();

        Self::merkle_root(&tx_hashes)
    }

    /// Simple merkle root calculation
    fn merkle_root(hashes: &[String]) -> Hash {
        if hashes.is_empty() {
            return "0".repeat(64);
        }

        if hashes.len() == 1 {
            return hashes[0].clone();
        }

        let mut level = hashes.to_vec();
        while level.len() > 1 {
            let mut next_level = Vec::new();

            for chunk in level.chunks(2) {
                let mut hasher = Sha3_256::new();
                hasher.update(chunk[0].as_bytes());
                if chunk.len() > 1 {
                    hasher.update(chunk[1].as_bytes());
                } else {
                    hasher.update(chunk[0].as_bytes()); // Duplicate if odd
                }
                let hash = hasher.finalize();
                next_level.push(hex::encode(hash));
            }

            level = next_level;
        }

        level[0].clone()
    }
}

impl Transaction {
    /// Get the hash of this transaction
    pub fn hash(&self) -> TxHash {
        match self {
            Transaction::Transfer(tx) => tx.hash.clone(),
            Transaction::Deploy(tx) => tx.hash.clone(),
            Transaction::Call(tx) => tx.hash.clone(),
            Transaction::Stake(tx) => tx.hash.clone(),
            Transaction::AIRequest(tx) => tx.hash.clone(),
        }
    }

    /// Get the sender address
    pub fn from(&self) -> &Address {
        match self {
            Transaction::Transfer(tx) => &tx.from,
            Transaction::Deploy(tx) => &tx.from,
            Transaction::Call(tx) => &tx.from,
            Transaction::Stake(tx) => &tx.validator,
            Transaction::AIRequest(tx) => &tx.from,
        }
    }

    /// Get the transaction fee
    pub fn fee(&self) -> Fee {
        match self {
            Transaction::Transfer(tx) => tx.fee,
            Transaction::Deploy(tx) => tx.fee,
            Transaction::Call(tx) => tx.fee,
            Transaction::Stake(tx) => tx.fee,
            Transaction::AIRequest(tx) => tx.fee,
        }
    }

    /// Get the transaction nonce
    pub fn nonce(&self) -> u64 {
        match self {
            Transaction::Transfer(tx) => tx.nonce,
            Transaction::Deploy(tx) => tx.nonce,
            Transaction::Call(tx) => tx.nonce,
            Transaction::Stake(tx) => tx.nonce,
            Transaction::AIRequest(tx) => tx.nonce,
        }
    }

    /// Get a reference to the embedded signature
    pub fn signature(&self) -> &PQCTransactionSignature {
        match self {
            Transaction::Transfer(tx) => &tx.signature,
            Transaction::Deploy(tx) => &tx.signature,
            Transaction::Call(tx) => &tx.signature,
            Transaction::Stake(tx) => &tx.signature,
            Transaction::AIRequest(tx) => &tx.signature,
        }
    }

    /// Build the signing message for this transaction
    pub fn signing_message(&self) -> Vec<u8> {
        match self {
            Transaction::Transfer(tx) => tx.signing_message(),
            Transaction::Deploy(tx) => tx.signing_message(),
            Transaction::Call(tx) => tx.signing_message(),
            Transaction::Stake(tx) => tx.signing_message(),
            Transaction::AIRequest(tx) => tx.signing_message(),
        }
    }

    /// Verify the transaction signature
    pub fn verify_signature(&self) -> bool {
        // Get the transaction signature
        let signature = self.signature();

        // Build the signing message
        let message = self.signing_message();

        // Create a temporary PQC manager to verify the signature
        // In a real implementation, this would use a shared PQC manager instance
        match dytallix_pqc::PQCManager::new() {
            Ok(pqc_manager) => pqc_manager
                .verify(&message, &signature.signature, &signature.public_key)
                .unwrap_or_default(),
            Err(_) => false,
        }
    }
}

impl TransferTransaction {
    /// Create a new transfer transaction
    pub fn new(from: Address, to: Address, amount: Amount, fee: Fee, nonce: u64) -> Self {
        let timestamp = chrono::Utc::now().timestamp() as u64;

        // Create transaction without signature first
        let mut tx = Self {
            hash: String::new(),
            from,
            to,
            amount,
            fee,
            nonce,
            timestamp,
            signature: PQCTransactionSignature {
                signature: Signature {
                    data: Vec::new(),
                    algorithm: SignatureAlgorithm::Dilithium3,
                },
                public_key: Vec::new(),
            },
            ai_risk_score: None, // Will be calculated later
        };

        // Calculate hash
        tx.hash = tx.calculate_hash();
        tx
    }

    /// Calculate hash of transaction data (without signature)
    pub fn calculate_hash(&self) -> TxHash {
        let data = format!(
            "{}:{}:{}:{}:{}:{}",
            self.from, self.to, self.amount, self.fee, self.nonce, self.timestamp
        );
        let mut hasher = Sha3_256::new();
        hasher.update(data.as_bytes());
        let hash = hasher.finalize();
        hex::encode(hash)
    }

    /// Format the signing message for this transaction
    pub fn signing_message(&self) -> Vec<u8> {
        self.calculate_hash().as_bytes().to_vec()
    }
}

impl fmt::Display for Block {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Block #{} with {} transactions",
            self.header.number,
            self.transactions.len()
        )
    }
}

impl fmt::Display for Transaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Transaction::Transfer(tx) => {
                write!(f, "Transfer: {} -> {} ({})", tx.from, tx.to, tx.amount)
            }
            Transaction::Deploy(tx) => {
                write!(f, "Deploy contract by {}", tx.from)
            }
            Transaction::Call(tx) => {
                write!(f, "Call {} by {}", tx.to, tx.from)
            }
            Transaction::Stake(tx) => {
                write!(f, "Stake {:?}: {} ({})", tx.action, tx.validator, tx.amount)
            }
            Transaction::AIRequest(tx) => {
                write!(f, "AI {:?} request by {}", tx.service_type, tx.from)
            }
        }
    }
}

impl TransferTransaction {
    /// Generate signing bytes for this transfer transaction
    pub fn signing_bytes(&self) -> Vec<u8> {
        format!(
            "transfer:{}:{}:{}:{}:{}:{}",
            self.from, self.to, self.amount, self.fee, self.nonce, self.timestamp
        )
        .into_bytes()
    }
}

impl DeployTransaction {
    /// Generate signing bytes for this deploy transaction
    pub fn signing_bytes(&self) -> Vec<u8> {
        format!(
            "deploy:{}:{}:{}:{}:{}:{}",
            self.from,
            hex::encode(&self.contract_code),
            hex::encode(&self.constructor_args),
            self.fee,
            self.nonce,
            self.timestamp
        )
        .into_bytes()
    }
}

impl CallTransaction {
    /// Generate signing bytes for this call transaction
    pub fn signing_bytes(&self) -> Vec<u8> {
        format!(
            "call:{}:{}:{}:{}:{}:{}:{}",
            self.from,
            self.to,
            self.method,
            hex::encode(&self.args),
            self.fee,
            self.nonce,
            self.timestamp
        )
        .into_bytes()
    }
}

impl StakeTransaction {
    /// Generate signing bytes for this stake transaction
    pub fn signing_bytes(&self) -> Vec<u8> {
        let action_str = match &self.action {
            StakeAction::Stake => "Stake".to_string(),
            StakeAction::Unstake => "Unstake".to_string(),
            StakeAction::Delegate { to } => format!("Delegate:{to}"),
            StakeAction::Undelegate => "Undelegate".to_string(),
        };
        format!(
            "stake:{}:{}:{}:{}:{}:{}",
            self.validator, self.amount, action_str, self.fee, self.nonce, self.timestamp
        )
        .into_bytes()
    }
}

impl AIRequestTransaction {
    /// Generate signing bytes for this AI request transaction
    pub fn signing_bytes(&self) -> Vec<u8> {
        format!(
            "ai_request:{}:{:?}:{}:{}:{}:{}",
            self.from,
            self.service_type,
            hex::encode(&self.request_data),
            self.fee,
            self.nonce,
            self.timestamp
        )
        .into_bytes()
    }
}

impl Transaction {
    /// Get canonical signing bytes for a transaction
    pub fn signing_bytes(&self) -> Vec<u8> {
        match self {
            Transaction::Transfer(tx) => tx.signing_bytes(),
            Transaction::Deploy(tx) => tx.signing_bytes(),
            Transaction::Call(tx) => tx.signing_bytes(),
            Transaction::Stake(tx) => tx.signing_bytes(),
            Transaction::AIRequest(tx) => tx.signing_bytes(),
        }
    }

    /// Sign the transaction using the provided PQC manager
    pub fn sign_transaction(&mut self, pqc: &crate::crypto::PQCManager) -> Result<(), String> {
        let message = self.signing_bytes();
        let sig = pqc.sign_message(&message).map_err(|e| e.to_string())?;

        let signature = crate::types::PQCTransactionSignature {
            signature: dytallix_pqc::Signature {
                data: sig.signature,
                algorithm: dytallix_pqc::SignatureAlgorithm::Dilithium3,
            },
            public_key: pqc.get_dilithium_public_key().to_vec(),
        };

        match self {
            Transaction::Transfer(tx) => tx.signature = signature,
            Transaction::Deploy(tx) => tx.signature = signature,
            Transaction::Call(tx) => tx.signature = signature,
            Transaction::Stake(tx) => tx.signature = signature,
            Transaction::AIRequest(tx) => tx.signature = signature,
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_amount_serde_roundtrip() {
        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        struct Wrapper {
            #[serde(with = "crate::types::serde_u128_string")]
            amount: Amount,
        }
        let w = Wrapper {
            amount: 12_345_678_901_234_567_890_123_456u128,
        }; // Use full u128 value
        let json = serde_json::to_string(&w).unwrap();
        assert!(json.contains("12345678901234567890123456"));
        let de: Wrapper = serde_json::from_str(&json).unwrap();
        assert_eq!(de, w);
    }

    #[test]
    fn test_receipt_serde_roundtrip() {
        let r = TxReceipt {
            tx_hash: "0xabc".into(),
            block_number: 5,
            status: TxStatus::Success,
            gas_used: 1,
            fee_paid: 10,
            timestamp: 111,
            index: 0,
            error: None,
            contract_address: Some("0xcontract".into()),
            logs: vec!["log1".into(), "log2".into()],
            return_data: Some(vec![1, 2, 3]),
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: TxReceipt = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }
}
