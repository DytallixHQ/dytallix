pub mod alerts;
pub mod api;
pub mod bench;
pub mod engine;
pub mod explain;
pub mod features;
pub mod graph;
pub mod ingest;
pub mod models;
pub mod ops;
pub mod pqc;
pub mod usecases; // processing pipeline engine

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskEvent {
    pub tx_hash: String,
    pub from: String,
    pub to: String,
    pub amount: u128,
    pub timestamp: u64,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureVector {
    pub version: u32,
    pub tx_hash: String,
    pub features: Vec<f32>,
    pub feature_names: Vec<String>,
    pub generated_at: u64,
}

impl FeatureVector {
    pub fn new(tx_hash: String, feature_names: Vec<String>, features: Vec<f32>) -> Self {
        Self {
            version: 1,
            tx_hash,
            feature_names,
            features,
            generated_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskScore {
    pub tx_hash: String,
    pub score: f32,      // 0-100
    pub confidence: f32, // 0-1
    pub reasons: Vec<String>,
    pub top_features: Vec<(String, f32)>,
    pub paths: Vec<serde_json::Value>,
    pub p95_budget_ms: u32,
    pub elapsed_ms: u32,
}

impl RiskScore {
    pub fn new(tx_hash: String) -> Self {
        Self {
            tx_hash,
            score: 0.0,
            confidence: 0.0,
            reasons: vec![],
            top_features: vec![],
            paths: vec![],
            p95_budget_ms: 100,
            elapsed_ms: 0,
        }
    }
}

pub fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
