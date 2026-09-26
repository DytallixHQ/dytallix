pub mod algorithm_registry;
pub mod cosmos_bridge;
pub mod gas_optimizer;
pub mod governance;
#[doc(hidden)]
pub mod oracle_simple;
pub mod runtime;
pub mod security;
pub mod staking;
pub mod storage_optimizer;
pub mod tokenomics;
pub mod types;

mod cosmos_bridge_optimized;

// Re-export the dependency-light oracle implementation under the canonical public name.
pub use oracle_simple as oracle;

use scale::{Decode, Encode};

// Re-export common types
pub use algorithm_registry::*;
pub use governance::*;
pub use staking::*;
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
