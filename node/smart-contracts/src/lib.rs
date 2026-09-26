pub mod cosmos_bridge;
pub mod cosmos_bridge_optimized;
pub mod gas_optimizer;
pub mod oracle_simple;
pub mod runtime;
pub mod security;
pub mod storage_optimizer;
pub mod tokenomics;
pub mod types;

// Re-export oracle_simple as oracle for compatibility
pub use oracle_simple as oracle;

use scale::{Decode, Encode};

// Re-export common types
pub use types::*;

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub enum ContractError {
    NotAuthorized,
    InvalidState,
    InsufficientFunds,
    AIFraudDetected,
    Timeout,
    InvalidSignature,
    OracleError,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct AIAnalysisResult {
    pub risk_score: u8, // 0-100
    pub is_fraudulent: bool,
    pub confidence: u8,   // 0-100
    pub factors: Vec<u8>, // Encoded risk factors
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct PQCSignature {
    pub algorithm: u8,       // Algorithm identifier
    pub signature: Vec<u8>,  // Signature bytes
    pub public_key: Vec<u8>, // Signer's public key
}

pub type Result<T> = core::result::Result<T, ContractError>;
