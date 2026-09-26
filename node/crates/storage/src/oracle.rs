use rocksdb::DB;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AiRiskRecord {
    pub tx_hash: String,
    pub model_id: String,
    pub risk_score: f32,         // 0.0..1.0
    pub score_str: String,       // Original score string for determinism
    pub confidence: Option<f32>, // 0.0..1.0
    pub signature: Option<String>,
    pub oracle_pubkey: Option<String>,
    pub ingested_at: u64, // Unix timestamp
    pub source: String,   // Oracle source identifier
}

pub struct OracleStore<'a> {
    pub db: &'a DB,
}

impl<'a> OracleStore<'a> {
    fn key(tx_hash: &str) -> String {
        format!("oracle:ai:{tx_hash}")
    }

    pub fn put_ai_risk(&self, rec: &AiRiskRecord) -> anyhow::Result<()> {
        self.validate_record(rec)?;
        self.db
            .put(Self::key(&rec.tx_hash), serde_json::to_vec(rec)?)?;
        Ok(())
    }

    pub fn put_ai_risks_batch(&self, records: &[AiRiskRecord]) -> anyhow::Result<Vec<String>> {
        let mut failed_hashes = Vec::new();

        for record in records {
            if let Err(e) = self.put_ai_risk(record) {
                eprintln!("Failed to store record for {}: {}", record.tx_hash, e);
                failed_hashes.push(record.tx_hash.clone());
            }
        }

        Ok(failed_hashes)
    }

    pub fn get_ai_risk(&self, tx_hash: &str) -> Option<AiRiskRecord> {
        self.db
            .get(Self::key(tx_hash))
            .ok()
            .flatten()
            .and_then(|v| serde_json::from_slice(&v).ok())
    }

    fn validate_record(&self, rec: &AiRiskRecord) -> anyhow::Result<()> {
        // Validate tx_hash format (hex)
        if !rec.tx_hash.starts_with("0x") || rec.tx_hash.len() < 3 {
            return Err(anyhow::anyhow!("Invalid tx_hash format"));
        }

        // Validate risk_score range
        if !(0.0..=1.0).contains(&rec.risk_score) {
            return Err(anyhow::anyhow!("risk_score must be between 0.0 and 1.0"));
        }

        // Validate confidence range if present
        if let Some(confidence) = rec.confidence {
            if !(0.0..=1.0).contains(&confidence) {
                return Err(anyhow::anyhow!("confidence must be between 0.0 and 1.0"));
            }
        }

        // Validate model_id is not empty
        if rec.model_id.trim().is_empty() {
            return Err(anyhow::anyhow!("model_id cannot be empty"));
        }

        // Validate score_str is not empty
        if rec.score_str.trim().is_empty() {
            return Err(anyhow::anyhow!("score_str cannot be empty"));
        }

        // Validate source is not empty
        if rec.source.trim().is_empty() {
            return Err(anyhow::anyhow!("source cannot be empty"));
        }

        Ok(())
    }
}
