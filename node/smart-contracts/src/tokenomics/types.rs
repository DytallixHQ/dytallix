/*
Common types for the Dytallix tokenomics system
*/

use crate::types::{Address, Amount};
use scale::{Decode, Encode};
use serde::{Deserialize, Serialize};

/// Token balance type
pub type Balance = Amount;

/// Emission rate (tokens per block)
pub type EmissionRate = u64;

/// Proposal ID for governance
pub type ProposalId = u64;

/// Token events
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub enum TokenEvent {
    Transfer {
        from: Address,
        to: Address,
        amount: Balance,
    },
    Mint {
        to: Address,
        amount: Balance,
    },
    Burn {
        from: Address,
        amount: Balance,
    },
    EmissionRateChanged {
        old_rate: EmissionRate,
        new_rate: EmissionRate,
    },
}

/// Errors for tokenomics operations
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub enum TokenomicsError {
    InsufficientBalance,
    NotAuthorized,
    InvalidAmount,
    InvalidAddress,
    EmissionControllerNotSet,
    ProposalNotFound,
    InvalidEmissionRate,
    TransferToSelf,
}

impl std::fmt::Display for TokenomicsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenomicsError::InsufficientBalance => write!(f, "Insufficient balance"),
            TokenomicsError::NotAuthorized => write!(f, "Not authorized"),
            TokenomicsError::InvalidAmount => write!(f, "Invalid amount"),
            TokenomicsError::InvalidAddress => write!(f, "Invalid address"),
            TokenomicsError::EmissionControllerNotSet => write!(f, "Emission controller not set"),
            TokenomicsError::ProposalNotFound => write!(f, "Proposal not found"),
            TokenomicsError::InvalidEmissionRate => write!(f, "Invalid emission rate"),
            TokenomicsError::TransferToSelf => write!(f, "Cannot transfer to self"),
        }
    }
}

impl std::error::Error for TokenomicsError {}

/// Emission parameters for governance proposals
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub struct EmissionParameters {
    pub base_emission_rate: EmissionRate,
    pub max_emission_rate: EmissionRate,
    pub min_emission_rate: EmissionRate,
    pub adjustment_factor: u32, // basis points (1/10000)
}

impl Default for EmissionParameters {
    fn default() -> Self {
        Self {
            base_emission_rate: 1000, // 1000 tokens per block
            max_emission_rate: 5000,  // Max 5000 tokens per block
            min_emission_rate: 100,   // Min 100 tokens per block
            adjustment_factor: 500,   // 5% adjustment factor
        }
    }
}

/// Governance proposal types for tokenomics
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub enum TokenomicsProposal {
    ChangeEmissionRate { new_rate: EmissionRate },
    UpdateEmissionParameters { new_params: EmissionParameters },
    MintDGT { to: Address, amount: Balance },
    BurnDRT { from: Address, amount: Balance },
}

/// Result type for tokenomics operations
pub type TokenomicsResult<T> = Result<T, TokenomicsError>;
