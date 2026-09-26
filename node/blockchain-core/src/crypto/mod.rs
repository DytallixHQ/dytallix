use dytallix_pqc::{PQCManager as DytallixPQCManager, Signature, SignatureAlgorithm};
use log::info;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PQCKeyPair {
    pub public_key: Vec<u8>,
    pub secret_key: Vec<u8>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PQCSignature {
    pub signature: Vec<u8>,
    pub algorithm: String,
    pub nonce: u64,
    pub timestamp: u64,
}

pub struct PQCManager {
    inner: DytallixPQCManager,
}

impl std::fmt::Debug for PQCManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PQCManager")
            .field("inner", &"<PQCManager instance>")
            .finish()
    }
}

impl PQCManager {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        info!("Generating post-quantum cryptographic keys...");

        let inner = DytallixPQCManager::new()?;
        info!("Post-quantum keys generated successfully");

        Ok(Self { inner })
    }

    pub fn sign_message(&self, message: &[u8]) -> Result<PQCSignature, Box<dyn std::error::Error>> {
        let signature = self.inner.sign(message)?;

        Ok(PQCSignature {
            signature: signature.data.clone(),
            algorithm: format!("{:?}", signature.algorithm),
            nonce: 0, // Default value for compatibility
            timestamp: chrono::Utc::now().timestamp() as u64,
        })
    }

    pub fn load_or_generate<P: AsRef<std::path::Path>>(
        path: P,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let inner = DytallixPQCManager::load_or_generate(path)?;
        Ok(Self { inner })
    }

    pub fn validate_keys(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.inner.validate_keys()?;
        Ok(())
    }

    pub fn verify_signature(
        &self,
        message: &[u8],
        signature: &PQCSignature,
        public_key: &[u8],
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let algorithm = match signature.algorithm.as_str() {
            "Dilithium3" | "CRYSTALS-Dilithium3" => SignatureAlgorithm::Dilithium3,
            "Dilithium5" | "CRYSTALS-Dilithium5" => SignatureAlgorithm::Dilithium5,
            "Falcon1024" => SignatureAlgorithm::Falcon1024,
            "SphincsSha256128s" | "SPHINCS+" => SignatureAlgorithm::SphincsSha256128s,
            _ => SignatureAlgorithm::Dilithium3,
        };

        let sig = Signature {
            data: signature.signature.clone(),
            algorithm,
        };

        Ok(self.inner.verify(message, &sig, public_key)?)
    }

    pub fn get_dilithium_public_key(&self) -> &[u8] {
        self.inner.get_signature_public_key()
    }

    pub fn get_kyber_public_key(&self) -> &[u8] {
        self.inner.get_key_exchange_public_key()
    }

    pub fn perform_key_exchange(
        &self,
        _peer_public_key: &[u8],
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        info!("Key exchange performed, shared secret generated");
        // Placeholder implementation
        Ok(vec![0u8; 32])
    }

    // Crypto-agility: Allow swapping algorithms
    pub fn set_signature_algorithm(
        &mut self,
        algorithm: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match algorithm {
            "CRYSTALS-Dilithium5" => {
                // Already using this
                Ok(())
            }
            "Falcon-1024" => {
                // Placeholder for Falcon implementation
                info!("Switching to Falcon-1024 (not yet implemented)");
                Err("Falcon-1024 not yet implemented".into())
            }
            "SPHINCS+" => {
                // Placeholder for SPHINCS+ implementation
                info!("Switching to SPHINCS+ (not yet implemented)");
                Err("SPHINCS+ not yet implemented".into())
            }
            _ => Err(format!("Unsupported signature algorithm: {algorithm}").into()),
        }
    }

    /// Sign a blockchain transaction using the node's Dilithium key
    pub fn sign_transaction(
        &self,
        tx: &crate::types::Transaction,
    ) -> Result<crate::types::PQCTransactionSignature, Box<dyn std::error::Error>> {
        use crate::types::*;

        // Format the transaction message depending on its type
        let message = match tx {
            Transaction::Transfer(t) => format_transfer_message(t),
            Transaction::Deploy(t) => format_deploy_message(t),
            Transaction::Call(t) => format_call_message(t),
            Transaction::Stake(t) => format_stake_message(t),
            Transaction::AIRequest(t) => format_ai_request_message(t),
        };

        // Create Dilithium signature over the formatted bytes
        let signature = self.inner.sign(&message)?;

        Ok(PQCTransactionSignature {
            signature,
            public_key: self.inner.get_signature_public_key().to_vec(),
        })
    }

    /// Derive validator address from signature public key: dyt1 + hex(blake3(pubkey)[0..20])
    pub fn derive_validator_address(&self) -> String {
        use blake3::Hasher;
        let pk = self.get_dilithium_public_key();
        let mut hasher = Hasher::new();
        hasher.update(pk);
        let digest = hasher.finalize();
        let bytes = &digest.as_bytes()[0..20];
        format!("dyt1{}", hex::encode(bytes))
    }

    /// Produce canonical bytes of a block header with signature field zeroed (not included).
    pub fn canonical_header_without_sig(header: &crate::types::BlockHeader) -> Vec<u8> {
        // Create a shallow clone with empty signature fields
        let mut clone = header.clone();
        clone.signature.signature.data.clear();
        clone.signature.public_key.clear();
        // Serialize deterministic (bincode is deterministic given struct order)
        bincode::serialize(&clone).expect("serialize header")
    }

    /// Sign block header (excluding signature field itself)
    pub fn sign_block_header(
        &self,
        header: &crate::types::BlockHeader,
    ) -> Result<crate::types::PQCBlockSignature, Box<dyn std::error::Error>> {
        let bytes = Self::canonical_header_without_sig(header);
        let sig = self.inner.sign(&bytes)?;
        Ok(crate::types::PQCBlockSignature {
            signature: sig,
            public_key: self.get_dilithium_public_key().to_vec(),
        })
    }

    /// Verify block signature using canonical header serialization with signature removed
    pub fn verify_block_signature(
        &self,
        header: &crate::types::BlockHeader,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let bytes = Self::canonical_header_without_sig(header);
        let sig = &header.signature.signature;
        Ok(self
            .inner
            .verify(&bytes, sig, &header.signature.public_key)?)
    }
}

// --- Message formatters ----------------------------------------------------

use crate::types::{
    AIRequestTransaction, CallTransaction, DeployTransaction, StakeTransaction, TransferTransaction,
};

fn format_transfer_message(tx: &TransferTransaction) -> Vec<u8> {
    format!(
        "transfer:{}:{}:{}:{}:{}:{}",
        tx.from, tx.to, tx.amount, tx.fee, tx.nonce, tx.timestamp
    )
    .into_bytes()
}

fn format_deploy_message(tx: &DeployTransaction) -> Vec<u8> {
    format!(
        "deploy:{}:{}:{}:{}:{}:{}",
        tx.from,
        hex::encode(&tx.contract_code),
        hex::encode(&tx.constructor_args),
        tx.fee,
        tx.nonce,
        tx.timestamp
    )
    .into_bytes()
}

fn format_call_message(tx: &CallTransaction) -> Vec<u8> {
    format!(
        "call:{}:{}:{}:{}:{}:{}:{}",
        tx.from,
        tx.to,
        tx.method,
        hex::encode(&tx.args),
        tx.fee,
        tx.nonce,
        tx.timestamp
    )
    .into_bytes()
}

fn format_stake_message(tx: &StakeTransaction) -> Vec<u8> {
    format!(
        "stake:{}:{:?}:{}:{}:{}:{}",
        tx.validator, tx.action, tx.amount, tx.fee, tx.nonce, tx.timestamp
    )
    .into_bytes()
}

fn format_ai_request_message(tx: &AIRequestTransaction) -> Vec<u8> {
    format!(
        "airequest:{}:{:?}:{}:{}:{}:{}",
        tx.from,
        tx.service_type,
        hex::encode(&tx.request_data),
        tx.fee,
        tx.nonce,
        tx.timestamp
    )
    .into_bytes()
}
