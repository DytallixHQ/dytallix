//! Oracle risk ingestion for Dytallix blockchain runtime
//!
//! This module provides secure, deterministic ingestion of AI risk scores
//! from external oracles with optional cryptographic signature verification.

use serde::{Deserialize, Serialize};

/// AI risk record for deterministic storage
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AiRiskRecord {
    pub tx_hash: String,
    pub score_str: String, // Original score string - preserved for determinism
    pub model_id: String,
    pub ingested_at: u64, // Unix timestamp
    pub source: String,   // Oracle source identifier
}
