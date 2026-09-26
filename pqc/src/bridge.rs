//! Bridge-specific PQC cryptography implementation
//!
//! Extends the PQC crypto library with bridge-specific functionality for cross-chain operations.

use crate::{KeyPair, PQCError, PQCManager, Signature, SignatureAlgorithm};
use blake3::Hasher;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sha3::Keccak256;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Bridge-specific signature that includes chain and payload information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeSignature {
    pub signature: Signature,
    pub chain_id: String,
    pub payload_hash: Vec<u8>,
    pub timestamp: u64,
    pub validator_id: String,
    /// SECURITY FIX: CV-002 - Added nonce for replay attack protection
    pub nonce: u64,
    /// SECURITY ENHANCEMENT: Added sequence number for ordering validation
    pub sequence: u64,
}

/// Cross-chain payload formats for different blockchain types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CrossChainPayload {
    EthereumTransaction {
        to: String,
        value: u64,
        data: Vec<u8>,
        gas_limit: u64,
        gas_price: u64,
        nonce: u64,
    },
    CosmosIBCPacket {
        sequence: u64,
        source_port: String,
        source_channel: String,
        dest_port: String,
        dest_channel: String,
        data: Vec<u8>,
        timeout_height: u64,
        timeout_timestamp: u64,
    },
    GenericBridgePayload {
        asset_id: String,
        amount: u64,
        source_chain: String,
        dest_chain: String,
        source_address: String,
        dest_address: String,
        metadata: HashMap<String, String>,
    },
}

/// Enhanced payload structure for replay protection
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EnhancedPayload {
    payload: CrossChainPayload,
    chain_id: String,
    validator_id: String,
    nonce: u64,
    sequence: u64,
    timestamp: u64,
}

/// Multi-signature validation result
#[derive(Debug, Clone)]
pub struct MultiSigValidationResult {
    pub valid_signatures: usize,
    pub required_signatures: usize,
    pub validator_results: HashMap<String, bool>,
    pub consensus_reached: bool,
}

/// Bridge PQC Manager that extends the base PQC functionality
#[derive(Clone)]
pub struct BridgePQCManager {
    pqc_manager: PQCManager,
    validator_keys: HashMap<String, (Vec<u8>, SignatureAlgorithm)>, // validator_id -> (public_key, algorithm)
    chain_configs: HashMap<String, ChainConfig>,
    min_signatures: usize,
    // Generated signers remain private and are selected by the registered public key.
    local_signers: std::sync::Arc<std::sync::Mutex<HashMap<Vec<u8>, KeyPair>>>,
    /// SECURITY FIX: CV-002 - Added nonce tracking for replay protection
    nonce_counter: std::sync::Arc<std::sync::atomic::AtomicU64>,
    /// SECURITY ENHANCEMENT: Track used nonces to prevent replay attacks
    used_nonces: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<(String, String, u64)>>>,
    /// SECURITY ENHANCEMENT: Sequence tracking for ordering validation
    sequence_counter: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

/// Configuration for different blockchain chains
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    pub chain_id: String,
    pub signature_format: SignatureFormat,
    pub hash_algorithm: HashAlgorithm,
    pub address_format: AddressFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SignatureFormat {
    Raw,
    DER,
    Ethereum,
    Cosmos,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HashAlgorithm {
    Blake3,
    SHA256,
    Keccak256,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AddressFormat {
    Ethereum,
    CosmosBase32,
    PolkadotSS58,
}

impl BridgePQCManager {
    /// Create a new BridgePQCManager with default settings
    pub fn new() -> Result<Self, PQCError> {
        let pqc_manager = PQCManager::new()?;

        let mut chain_configs = HashMap::new();

        // Add default chain configurations
        chain_configs.insert(
            "ethereum".to_string(),
            ChainConfig {
                chain_id: "ethereum".to_string(),
                signature_format: SignatureFormat::Ethereum,
                hash_algorithm: HashAlgorithm::Keccak256,
                address_format: AddressFormat::Ethereum,
            },
        );

        chain_configs.insert(
            "cosmos".to_string(),
            ChainConfig {
                chain_id: "cosmos".to_string(),
                signature_format: SignatureFormat::Cosmos,
                hash_algorithm: HashAlgorithm::SHA256,
                address_format: AddressFormat::CosmosBase32,
            },
        );

        chain_configs.insert(
            "polkadot".to_string(),
            ChainConfig {
                chain_id: "polkadot".to_string(),
                signature_format: SignatureFormat::Raw,
                hash_algorithm: HashAlgorithm::Blake3,
                address_format: AddressFormat::PolkadotSS58,
            },
        );

        Ok(Self {
            pqc_manager,
            validator_keys: HashMap::new(),
            chain_configs,
            local_signers: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
            min_signatures: 3, // Default 3-of-N multi-sig
            // SECURITY FIX: Initialize nonce and sequence counters for replay protection
            nonce_counter: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(1)),
            used_nonces: std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::HashSet::new(),
            )),
            sequence_counter: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(1)),
        })
    }

    /// Load or create a BridgePQCManager with persistent storage
    pub fn load_or_create<P: AsRef<std::path::Path>>(path: P) -> Result<Self, PQCError> {
        let pqc_manager = PQCManager::load_or_generate(&path)?;

        let mut bridge_manager = Self {
            pqc_manager,
            validator_keys: HashMap::new(),
            chain_configs: HashMap::new(),
            local_signers: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
            min_signatures: 3,
            // SECURITY FIX: Initialize security counters
            nonce_counter: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(1)),
            used_nonces: std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::HashSet::new(),
            )),
            sequence_counter: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(1)),
        };

        // Initialize default chain configs
        bridge_manager.initialize_default_chains();

        Ok(bridge_manager)
    }

    /// Initialize default chain configurations
    fn initialize_default_chains(&mut self) {
        self.chain_configs.insert(
            "ethereum".to_string(),
            ChainConfig {
                chain_id: "ethereum".to_string(),
                signature_format: SignatureFormat::Ethereum,
                hash_algorithm: HashAlgorithm::Keccak256,
                address_format: AddressFormat::Ethereum,
            },
        );

        self.chain_configs.insert(
            "cosmos".to_string(),
            ChainConfig {
                chain_id: "cosmos".to_string(),
                signature_format: SignatureFormat::Cosmos,
                hash_algorithm: HashAlgorithm::SHA256,
                address_format: AddressFormat::CosmosBase32,
            },
        );
    }

    /// Add a validator's public key for multi-signature verification
    pub fn add_validator(
        &mut self,
        validator_id: String,
        public_key: Vec<u8>,
        algorithm: SignatureAlgorithm,
    ) {
        self.validator_keys
            .insert(validator_id, (public_key, algorithm));
    }

    /// Set minimum required signatures for consensus
    pub fn set_min_signatures(&mut self, min_signatures: usize) {
        self.min_signatures = min_signatures;
    }

    /// Sign a cross-chain payload for bridge operations
    ///
    /// SECURITY VULNERABILITIES:
    /// - CV-002: Timestamp-based replay attacks - weak timestamp validation
    /// - Missing nonce-based replay protection
    /// - No validation of payload integrity before signing
    ///
    /// ATTACK VECTORS:
    /// - Replay attacks: Signatures can be replayed within large time windows
    /// - Payload manipulation: No canonical serialization enforcement
    /// - Cross-chain confusion: Same signature valid across different chains
    ///
    /// CRITICAL SECURITY REQUIREMENTS:
    /// - Implement nonce-based ordering to prevent replay attacks
    /// - Add strict timestamp validation window (< 5 minutes)
    /// - Enforce canonical payload serialization
    /// - Include chain-specific context in signature
    ///
    /// IMPLEMENTATION NOTE: Replay protection & payload validation partially implemented; further enhancements (nonce persistence, chain domain separation extension) tracked in security roadmap.
    pub fn sign_bridge_payload(
        &self,
        payload: &CrossChainPayload,
        chain_id: &str,
        validator_id: &str,
    ) -> Result<BridgeSignature, PQCError> {
        // SECURITY FIX: CV-002 - Generate unique nonce for replay protection
        let nonce = self
            .nonce_counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let sequence = self
            .sequence_counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // SECURITY ENHANCEMENT: Create enhanced payload hash including nonce and sequence
        let enhanced_payload = EnhancedPayload {
            payload: payload.clone(),
            chain_id: chain_id.to_string(),
            validator_id: validator_id.to_string(),
            nonce,
            sequence,
            timestamp,
        };

        // Serialize enhanced payload and calculate hash based on chain configuration
        let payload_hash = self.calculate_enhanced_payload_hash(&enhanced_payload)?;

        // Select only the signer bound to the registered validator identity.
        let (public_key, algorithm) = self
            .validator_keys
            .get(validator_id)
            .ok_or_else(|| PQCError::InvalidKey("Validator is not registered".into()))?;
        let signer = self
            .local_signers
            .lock()
            .map_err(|_| PQCError::InvalidKey("Local signer state is unavailable".into()))?
            .get(public_key)
            .cloned();
        let signature = if let Some(keypair) = signer {
            if keypair.algorithm != *algorithm {
                return Err(PQCError::InvalidKey(
                    "Registered signer algorithm does not match".into(),
                ));
            }
            PQCManager::from_keypair(keypair, self.pqc_manager.get_key_exchange_keypair())
                .sign(&payload_hash)?
        } else if self.pqc_manager.get_signature_public_key() == public_key.as_slice()
            && self.pqc_manager.get_signature_algorithm() == algorithm
        {
            self.pqc_manager.sign(&payload_hash)?
        } else {
            return Err(PQCError::InvalidKey(
                "No local signer matches the registered validator".into(),
            ));
        };

        Ok(BridgeSignature {
            signature,
            chain_id: chain_id.to_string(),
            payload_hash,
            timestamp,
            validator_id: validator_id.to_string(),
            nonce,
            sequence,
        })
    }

    /// Verify a bridge signature from a specific validator
    ///
    /// SECURITY VULNERABILITIES:
    /// - CV-003: Algorithm downgrade attacks - no security level validation
    /// - BR-002: Insufficient payload hash validation
    /// - Missing timestamp validation for signature freshness
    ///
    /// ATTACK VECTORS:
    /// - Force use of weaker algorithms by manipulating signature algorithm field
    /// - Payload hash manipulation if serialization is not deterministic
    /// - Stale signature acceptance due to missing timeout validation
    /// - Validator impersonation if key validation is insufficient
    ///
    /// CRITICAL SECURITY GAPS:
    /// - No algorithm security hierarchy enforcement
    /// - Missing validator authorization checks
    /// - No signature freshness validation
    /// - Insufficient error information leakage protection
    ///
    /// REQUIRED MITIGATIONS:
    /// - Implement algorithm security level validation
    /// - Add strict timestamp window checking
    /// - Validate validator authorization status
    /// - Ensure constant-time verification to prevent timing attacks
    fn validate_bridge_signature(
        &self,
        bridge_sig: &BridgeSignature,
        payload: &CrossChainPayload,
    ) -> Result<bool, PQCError> {
        let (public_key, algorithm) = self
            .validator_keys
            .get(&bridge_sig.validator_id)
            .ok_or_else(|| {
                PQCError::InvalidKey(format!("Unknown validator: {}", bridge_sig.validator_id))
            })?;

        // SECURITY ENHANCEMENT: Strict timestamp validation (5-minute window)
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let time_diff = current_time.abs_diff(bridge_sig.timestamp);

        if time_diff > 300 {
            // 5 minutes
            return Ok(false); // Signature too old or from future
        }

        // Reconstruct enhanced payload for verification
        let enhanced_payload = EnhancedPayload {
            payload: payload.clone(),
            chain_id: bridge_sig.chain_id.clone(),
            validator_id: bridge_sig.validator_id.clone(),
            nonce: bridge_sig.nonce,
            sequence: bridge_sig.sequence,
            timestamp: bridge_sig.timestamp,
        };

        // Verify enhanced payload hash matches
        let expected_hash = self.calculate_enhanced_payload_hash(&enhanced_payload)?;
        if expected_hash != bridge_sig.payload_hash {
            return Ok(false);
        }

        if *algorithm != bridge_sig.signature.algorithm {
            return Ok(false);
        }

        // SECURITY FIX: CV-003 - Validate algorithm security level
        if !self.validate_algorithm_security_level(&bridge_sig.signature.algorithm, payload) {
            return Ok(false); // Algorithm security level insufficient for payload
        }

        if !self
            .pqc_manager
            .verify(&bridge_sig.payload_hash, &bridge_sig.signature, public_key)?
        {
            return Ok(false);
        }
        Ok(true)
    }

    /// Verify and consume one authenticated nonce.
    pub fn verify_bridge_signature(
        &self,
        bridge_sig: &BridgeSignature,
        payload: &CrossChainPayload,
    ) -> Result<bool, PQCError> {
        if !self.validate_bridge_signature(bridge_sig, payload)? {
            return Ok(false);
        }
        let mut used = self
            .used_nonces
            .lock()
            .map_err(|_| PQCError::InvalidSignature("Nonce state is unavailable".into()))?;
        Ok(used.insert((
            bridge_sig.chain_id.clone(),
            bridge_sig.validator_id.clone(),
            bridge_sig.nonce,
        )))
    }

    /// Verify multiple signatures for multi-sig consensus
    ///
    /// SECURITY VULNERABILITIES:
    /// - BR-001: Weak multi-signature validation
    /// - No prevention of signature reuse across different payloads
    /// - Missing validator identity verification beyond key lookup
    /// - No timeout validation for signature freshness
    ///
    /// ATTACK VECTORS:
    /// - Signature replay: Same signatures reused for different payloads
    /// - Validator impersonation: Compromised validators not detected
    /// - Consensus manipulation: Malicious validators can affect threshold
    /// - Time-based attacks: Old signatures accepted without freshness check
    ///
    /// CRITICAL SECURITY REQUIREMENTS:
    /// - Each signature must include unique payload binding
    /// - Implement validator authorization verification
    /// - Add signature freshness validation
    /// - Prevent signature reuse across different operations
    /// - Implement byzantine fault tolerance considerations
    ///
    /// IMPLEMENTATION NOTE: Signature uniqueness & BFT threshold extensions planned; current model enforces nonce + timestamp freshness; additional byzantine mitigation tracked in roadmap.
    pub fn verify_multi_signature(
        &self,
        signatures: &[BridgeSignature],
        payload: &CrossChainPayload,
    ) -> Result<MultiSigValidationResult, PQCError> {
        let mut validator_results = HashMap::new();
        let mut candidates = HashMap::new();
        for signature in signatures {
            let valid = self.validate_bridge_signature(signature, payload)?;
            // Count each validator once. Keep a valid signature if another entry is invalid.
            let result = validator_results
                .entry(signature.validator_id.clone())
                .or_insert(false);
            *result |= valid;
            if valid {
                candidates.entry(signature.validator_id.clone()).or_insert((
                    signature.chain_id.clone(),
                    signature.validator_id.clone(),
                    signature.nonce,
                ));
            }
        }
        let mut used = self
            .used_nonces
            .lock()
            .map_err(|_| PQCError::InvalidSignature("Nonce state is unavailable".into()))?;
        candidates.retain(|validator, nonce| {
            if used.contains(nonce) {
                validator_results.insert(validator.clone(), false);
                false
            } else {
                true
            }
        });
        let valid_signatures = candidates.len();
        let consensus_reached = valid_signatures >= self.min_signatures;
        // Validation errors and incomplete batches do not commit partial nonce state.
        if consensus_reached {
            used.extend(candidates.into_values());
        }

        Ok(MultiSigValidationResult {
            valid_signatures,
            required_signatures: self.min_signatures,
            validator_results,
            consensus_reached,
        })
    }

    /// Calculate payload hash based on chain configuration
    #[allow(dead_code)]
    fn calculate_payload_hash(
        &self,
        payload: &CrossChainPayload,
        chain_id: &str,
    ) -> Result<Vec<u8>, PQCError> {
        let serialized = serde_json::to_vec(payload)
            .map_err(|e| PQCError::InvalidKey(format!("Payload serialization error: {e}")))?;

        let chain_config = self
            .chain_configs
            .get(chain_id)
            .ok_or_else(|| PQCError::UnsupportedAlgorithm(format!("Unknown chain: {chain_id}")))?;

        match chain_config.hash_algorithm {
            HashAlgorithm::Blake3 => Ok(blake3::hash(&serialized).as_bytes().to_vec()),
            HashAlgorithm::SHA256 => {
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(&serialized);
                Ok(hasher.finalize().to_vec())
            }
            HashAlgorithm::Keccak256 => {
                // For now, use SHA256 as placeholder - in production would use keccak256
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(&serialized);
                Ok(hasher.finalize().to_vec())
            }
        }
    }

    #[allow(dead_code)]
    fn calculate_enhanced_payload_hash(
        &self,
        enhanced_payload: &EnhancedPayload,
    ) -> Result<Vec<u8>, PQCError> {
        let serialized = serde_json::to_vec(enhanced_payload).map_err(|e| {
            PQCError::InvalidKey(format!("Enhanced payload serialization error: {e}"))
        })?;
        let chain_config = self
            .chain_configs
            .get(&enhanced_payload.chain_id)
            .ok_or_else(|| {
                PQCError::UnsupportedAlgorithm(format!(
                    "Unknown chain: {}",
                    enhanced_payload.chain_id
                ))
            })?;
        match chain_config.hash_algorithm {
            HashAlgorithm::Blake3 => {
                let mut hasher = Hasher::new();
                hasher.update(&serialized);
                Ok(hasher.finalize().as_bytes().to_vec())
            }
            HashAlgorithm::SHA256 => {
                let mut hasher = Sha256::new();
                hasher.update(&serialized);
                Ok(hasher.finalize().to_vec())
            }
            HashAlgorithm::Keccak256 => {
                let mut hasher = Keccak256::new();
                hasher.update(&serialized);
                Ok(hasher.finalize().to_vec())
            }
        }
    }

    /// SECURITY FIX: CV-003 - Validate algorithm security level for payload requirements
    fn validate_algorithm_security_level(
        &self,
        algorithm: &SignatureAlgorithm,
        payload: &CrossChainPayload,
    ) -> bool {
        let required_security_level = self.determine_required_security_level(payload);
        let algorithm_security_level = self.get_algorithm_security_level(algorithm);

        // Algorithm must meet or exceed required security level
        algorithm_security_level >= required_security_level
    }

    /// Determine required security level based on payload characteristics
    fn determine_required_security_level(&self, payload: &CrossChainPayload) -> u8 {
        match payload {
            CrossChainPayload::GenericBridgePayload { amount, .. } => {
                // Higher security for higher value transfers
                if *amount > 10_000_000 {
                    // > 10M units
                    5 // Highest security level
                } else if *amount > 1_000_000 {
                    // > 1M units
                    3 // Medium security level
                } else {
                    1 // Basic security level
                }
            }
            CrossChainPayload::EthereumTransaction { value, .. } => {
                // High security for Ethereum transactions due to gas costs
                if *value > 1000 {
                    // > 1000 Wei (example threshold)
                    5
                } else {
                    3
                }
            }
            CrossChainPayload::CosmosIBCPacket { .. } => {
                // Medium security for IBC packets
                3
            }
        }
    }

    /// Get security level of PQC algorithm (NIST security levels)
    fn get_algorithm_security_level(&self, algorithm: &SignatureAlgorithm) -> u8 {
        match algorithm {
            SignatureAlgorithm::Dilithium3 => 3, // Medium-high security (192-bit)
            SignatureAlgorithm::Dilithium5 => 5, // Highest security (256-bit)
            SignatureAlgorithm::Falcon1024 => 5, // Highest security (256-bit)
            SignatureAlgorithm::SphincsSha256128s => 1, // Conservative but lower performance
        }
    }

    /// Format signature for specific chain requirements
    pub fn format_signature_for_chain(
        &self,
        signature: &BridgeSignature,
    ) -> Result<Vec<u8>, PQCError> {
        let chain_config = self.chain_configs.get(&signature.chain_id).ok_or_else(|| {
            PQCError::UnsupportedAlgorithm(format!("Unknown chain: {}", signature.chain_id))
        })?;

        match chain_config.signature_format {
            SignatureFormat::Raw => Ok(signature.signature.data.clone()),
            SignatureFormat::DER => {
                // In production, would implement DER encoding
                Ok(signature.signature.data.clone())
            }
            SignatureFormat::Ethereum => {
                // In production, would implement Ethereum-specific signature format
                Ok(signature.signature.data.clone())
            }
            SignatureFormat::Cosmos => {
                // In production, would implement Cosmos-specific signature format
                Ok(signature.signature.data.clone())
            }
        }
    }

    /// Get supported chains
    pub fn get_supported_chains(&self) -> Vec<String> {
        self.chain_configs.keys().cloned().collect()
    }

    /// Get validator public keys
    pub fn get_validator_public_key(&self, validator_id: &str) -> Option<&Vec<u8>> {
        self.validator_keys.get(validator_id).map(|(key, _)| key)
    }

    /// Get the underlying PQC manager for direct operations
    pub fn get_pqc_manager(&self) -> &PQCManager {
        &self.pqc_manager
    }

    /// Get the underlying PQC manager for mutable operations
    pub fn get_pqc_manager_mut(&mut self) -> &mut PQCManager {
        &mut self.pqc_manager
    }

    /// Generate a new validator keypair
    pub fn generate_validator_keypair(
        &self,
        algorithm: &SignatureAlgorithm,
    ) -> Result<KeyPair, PQCError> {
        let keypair = self.pqc_manager.generate_keypair(algorithm)?;
        self.local_signers
            .lock()
            .map_err(|_| PQCError::InvalidKey("Local signer state is unavailable".into()))?
            .insert(keypair.public_key.clone(), keypair.clone());
        Ok(keypair)
    }

    /// Add a new chain configuration
    pub fn add_chain_config(&mut self, chain_id: String, config: ChainConfig) {
        self.chain_configs.insert(chain_id, config);
    }

    /// Get chain configuration
    pub fn get_chain_config(&self, chain_id: &str) -> Option<&ChainConfig> {
        self.chain_configs.get(chain_id)
    }
}

impl Default for BridgePQCManager {
    fn default() -> Self {
        Self::new().expect("Failed to create BridgePQCManager")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bridge_signature_creation_and_verification() {
        let mut bridge_manager = BridgePQCManager::new().unwrap();

        // Add a validator
        let validator_keypair = bridge_manager
            .generate_validator_keypair(&SignatureAlgorithm::Dilithium5)
            .unwrap();
        bridge_manager.add_validator(
            "validator_1".to_string(),
            validator_keypair.public_key.clone(),
            SignatureAlgorithm::Dilithium5,
        );

        // Create a test payload
        let payload = CrossChainPayload::GenericBridgePayload {
            asset_id: "USDC".to_string(),
            amount: 1000000,
            source_chain: "ethereum".to_string(),
            dest_chain: "cosmos".to_string(),
            source_address: "0x742d35Cc6634C0532925a3b8D1EbA4F00b7C8000".to_string(),
            dest_address: "cosmos1abc123...".to_string(),
            metadata: HashMap::new(),
        };

        // Sign the payload (would use validator's private key in production)
        let bridge_signature = bridge_manager
            .sign_bridge_payload(&payload, "ethereum", "validator_1")
            .unwrap();

        // Verify the signature
        let is_valid = bridge_manager
            .verify_bridge_signature(&bridge_signature, &payload)
            .unwrap();
        assert!(is_valid);
    }

    #[test]
    fn test_multi_signature_validation() {
        let mut bridge_manager = BridgePQCManager::new().unwrap();
        bridge_manager.set_min_signatures(2);

        // Add multiple validators
        for i in 1..=3 {
            let validator_keypair = bridge_manager
                .generate_validator_keypair(&SignatureAlgorithm::Dilithium5)
                .unwrap();
            bridge_manager.add_validator(
                format!("validator_{i}"),
                validator_keypair.public_key.clone(),
                SignatureAlgorithm::Dilithium5,
            );
        }

        let payload = CrossChainPayload::EthereumTransaction {
            to: "0x742d35Cc6634C0532925a3b8D1EbA4F00b7C8000".to_string(),
            value: 1000000,
            data: vec![],
            gas_limit: 21000,
            gas_price: 20000000000,
            nonce: 1,
        };

        // Create signatures from multiple validators
        let mut signatures = Vec::new();
        for i in 1..=3 {
            let bridge_signature = bridge_manager
                .sign_bridge_payload(&payload, "ethereum", &format!("validator_{i}"))
                .unwrap();
            signatures.push(bridge_signature);
        }

        // Verify multi-signature
        let result = bridge_manager
            .verify_multi_signature(&signatures, &payload)
            .unwrap();
        assert!(result.consensus_reached);
        assert_eq!(result.valid_signatures, 3);
        assert_eq!(result.required_signatures, 2);
    }

    #[test]
    fn test_cross_chain_payload_formats() {
        let bridge_manager = BridgePQCManager::new().unwrap();

        // Test Ethereum transaction format
        let eth_payload = CrossChainPayload::EthereumTransaction {
            to: "0x742d35Cc6634C0532925a3b8D1EbA4F00b7C8000".to_string(),
            value: 1000000,
            data: vec![0x12, 0x34],
            gas_limit: 21000,
            gas_price: 20000000000,
            nonce: 1,
        };

        let eth_hash = bridge_manager
            .calculate_payload_hash(&eth_payload, "ethereum")
            .unwrap();
        assert!(!eth_hash.is_empty());

        // Test Cosmos IBC packet format
        let cosmos_payload = CrossChainPayload::CosmosIBCPacket {
            sequence: 1,
            source_port: "transfer".to_string(),
            source_channel: "channel-0".to_string(),
            dest_port: "transfer".to_string(),
            dest_channel: "channel-1".to_string(),
            data: vec![0x56, 0x78],
            timeout_height: 1000,
            timeout_timestamp: 1234567890,
        };

        let cosmos_hash = bridge_manager
            .calculate_payload_hash(&cosmos_payload, "cosmos")
            .unwrap();
        assert!(!cosmos_hash.is_empty());

        // Hashes should be different for different payloads
        assert_ne!(eth_hash, cosmos_hash);
    }

    #[test]
    fn test_invalid_signature_scenarios() {
        let mut bridge_manager = BridgePQCManager::new().unwrap();

        // Add a validator
        let validator_keypair = bridge_manager
            .generate_validator_keypair(&SignatureAlgorithm::Dilithium5)
            .unwrap();
        bridge_manager.add_validator(
            "validator_1".to_string(),
            validator_keypair.public_key.clone(),
            SignatureAlgorithm::Dilithium5,
        );

        let payload = CrossChainPayload::GenericBridgePayload {
            asset_id: "USDC".to_string(),
            amount: 1000000,
            source_chain: "ethereum".to_string(),
            dest_chain: "cosmos".to_string(),
            source_address: "0x742d35Cc6634C0532925a3b8D1EbA4F00b7C8000".to_string(),
            dest_address: "cosmos1abc123...".to_string(),
            metadata: HashMap::new(),
        };

        // Create a valid signature
        let mut bridge_signature = bridge_manager
            .sign_bridge_payload(&payload, "ethereum", "validator_1")
            .unwrap();

        // Test with tampered payload
        let tampered_payload = CrossChainPayload::GenericBridgePayload {
            asset_id: "USDC".to_string(),
            amount: 2000000, // Changed amount
            source_chain: "ethereum".to_string(),
            dest_chain: "cosmos".to_string(),
            source_address: "0x742d35Cc6634C0532925a3b8D1EbA4F00b7C8000".to_string(),
            dest_address: "cosmos1abc123...".to_string(),
            metadata: HashMap::new(),
        };

        let is_valid = bridge_manager
            .verify_bridge_signature(&bridge_signature, &tampered_payload)
            .unwrap();
        assert!(!is_valid);

        // Test with unknown validator
        bridge_signature.validator_id = "unknown_validator".to_string();
        let result = bridge_manager.verify_bridge_signature(&bridge_signature, &payload);
        assert!(result.is_err());
    }
}
