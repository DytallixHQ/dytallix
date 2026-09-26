//! AI Oracle Client Module
//!
//! This module provides the HTTP client for communicating with external AI services
//! and handles all AI Oracle-related operations including health checks, service discovery,
//! and analysis requests.

use anyhow::{anyhow, Result};
use chrono;
use log::{info, warn};
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use tokio::time::Duration;

use crate::consensus::http_circuit_breaker::{
    HttpCircuitBreaker, HttpCircuitBreakerConfig, HttpCircuitError, HttpCircuitStatus,
};
use crate::consensus::types::AIServiceType;
use crate::consensus::SignedAIOracleResponse;

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

/// AI Service Information for discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIServiceInfo {
    pub service_id: String,
    pub service_type: AIServiceType,
    pub endpoint: String,
    pub version: String,
    pub capabilities: Vec<String>,
    pub availability_score: f64,
    pub last_heartbeat: u64,
}

/// AI Analysis Request structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIAnalysisRequest {
    pub request_id: String,
    pub service_type: AIServiceType,
    pub data: HashMap<String, Value>,
    pub requester_id: String,
    pub timestamp: u64,
    pub priority: u8,
}

/// AI Service Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIServiceConfig {
    /// Base URL for AI Oracle services
    pub base_url: String,
    /// Timeout for AI requests in seconds
    pub timeout_seconds: u64,
    /// API key for authentication
    pub api_key: String,
    /// Risk threshold for AI analysis confidence
    pub risk_threshold: f64,
    /// Maximum retries for failed requests
    pub max_retries: u32,
    /// Retry delay in milliseconds
    pub retry_delay_ms: u64,
}

impl Default for AIServiceConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:8080".to_string(),
            timeout_seconds: 30,
            api_key: "default_api_key".to_string(),
            risk_threshold: 0.7,
            max_retries: 3,
            retry_delay_ms: 1000,
        }
    }
}

/// HTTP client for communicating with external AI services
#[derive(Debug)]
pub struct AIOracleClient {
    client: Client,
    config: AIServiceConfig,
    circuit: Option<HttpCircuitBreaker>,
}

impl AIOracleClient {
    /// Create new AI Oracle client with configuration
    pub fn new(config: AIServiceConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            config,
            circuit: None,
        }
    }

    /// Enable a per-client circuit for health and POST requests.
    /// Share this client through Arc to share its circuit between callers.
    pub fn with_circuit_breaker(
        config: AIServiceConfig,
        circuit: HttpCircuitBreakerConfig,
    ) -> Result<Self, HttpCircuitError> {
        let breaker = HttpCircuitBreaker::new(circuit)?;
        if config.max_retries == 0 || config.timeout_seconds == 0 {
            return Err(HttpCircuitError::InvalidConfig);
        }
        let mut client = Self::new(config);
        client.circuit = Some(breaker);
        Ok(client)
    }

    pub fn circuit_breaker_status(&self) -> Result<Option<HttpCircuitStatus>, HttpCircuitError> {
        self.circuit
            .as_ref()
            .map(HttpCircuitBreaker::status)
            .transpose()
    }

    /// Reset clears counters and ignores completions from requests admitted before reset.
    /// It does not cancel those requests or undo their remote effects.
    pub fn reset_circuit_breaker(&self) -> Result<(), HttpCircuitError> {
        if let Some(circuit) = &self.circuit {
            circuit.reset()?;
        }
        Ok(())
    }

    /// Generic POST request to AI service
    pub async fn post<P: Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        endpoint: &str,
        payload: &P,
    ) -> Result<R> {
        let permit = self
            .circuit
            .as_ref()
            .map(HttpCircuitBreaker::acquire)
            .transpose()?;
        let max_attempts = if permit.as_ref().is_some_and(|permit| permit.is_probe()) {
            1
        } else {
            self.config.max_retries
        };
        let result = self.post_attempts(endpoint, payload, max_attempts).await;
        if let Some(permit) = permit {
            permit.finish(result.is_ok());
        }
        result
    }

    async fn post_attempts<P: Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        endpoint: &str,
        payload: &P,
        max_attempts: u32,
    ) -> Result<R> {
        let url = format!("{}/{}", self.config.base_url, endpoint);
        let mut attempts = 0;

        while attempts < max_attempts {
            let response = self
                .client
                .post(&url)
                .timeout(self.timeout())
                .header("Authorization", format!("Bearer {}", self.config.api_key))
                .header("Content-Type", "application/json")
                .json(payload)
                .send()
                .await?;

            if response.status().is_success() {
                let result = response.json::<R>().await?;
                return Ok(result);
            } else if response.status().is_server_error() && attempts < max_attempts - 1 {
                attempts += 1;
                warn!("Server error on attempt {attempts}, retrying...");
                tokio::time::sleep(Duration::from_millis(self.config.retry_delay_ms)).await;
                continue;
            } else {
                return Err(anyhow!(
                    "HTTP error {}: {}",
                    response.status(),
                    response.text().await?
                ));
            }
        }

        Err(anyhow!("Max retries exceeded"))
    }

    /// Health check endpoint for AI services
    pub async fn health_check(&self) -> Result<bool> {
        let permit = self
            .circuit
            .as_ref()
            .map(HttpCircuitBreaker::acquire)
            .transpose()?;
        let url = format!("{}/health", self.config.base_url);
        let result = self.client.get(&url).timeout(self.timeout()).send().await;
        if let Some(permit) = permit {
            permit.finish(
                result
                    .as_ref()
                    .is_ok_and(|response| response.status().is_success()),
            );
        }
        Ok(result?.status().is_success())
    }

    /// Service discovery - get available AI services and their capabilities
    pub async fn discover_services(&self) -> Result<Vec<AIServiceInfo>> {
        let services: Vec<AIServiceInfo> = self.post("services", &serde_json::json!({})).await?;
        Ok(services)
    }

    /// Analysis transport requires an agreed service contract and signature verification.
    /// Until then, fail explicitly instead of constructing a fabricated signed result.
    pub async fn request_analysis(
        &self,
        _request: &AIAnalysisRequest,
    ) -> Result<SignedAIOracleResponse> {
        Err(anyhow!("AI analysis transport is not implemented"))
    }

    /// Get current configuration
    pub fn get_config(&self) -> &AIServiceConfig {
        &self.config
    }

    /// Update the timeout used by subsequent HTTP attempts.
    pub fn set_timeout(&mut self, timeout_seconds: u64) {
        self.config.timeout_seconds = timeout_seconds;
    }

    /// Request AI analysis for a transaction or data with retry logic
    pub async fn request_ai_analysis(
        &self,
        service_type: AIServiceType,
        data: HashMap<String, Value>,
    ) -> Result<SignedAIOracleResponse> {
        let request = AIAnalysisRequest {
            request_id: format!("req_{}", chrono::Utc::now().timestamp_millis()),
            service_type,
            data,
            requester_id: "consensus_engine".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            priority: 5, // Medium priority
        };

        let response = self.request_analysis(&request).await?;

        // Validate response confidence score from metadata
        if let Some(metadata) = &response.response.metadata {
            if let Some(confidence) = metadata.confidence_score {
                if confidence < self.config.risk_threshold {
                    warn!("AI analysis confidence score below threshold: {confidence}");
                }
            }
        }

        info!(
            "AI analysis completed: service_type={:?}, response_id={}",
            response.response.service_type, response.response.id
        );

        Ok(response)
    }

    /// Batch request multiple AI analyses
    pub async fn batch_request_analyses(
        &self,
        requests: Vec<AIAnalysisRequest>,
    ) -> Result<Vec<SignedAIOracleResponse>> {
        let batch_payload = serde_json::json!({
            "requests": requests
        });

        let responses: Vec<SignedAIOracleResponse> =
            self.post("batch_analyze", &batch_payload).await?;
        Ok(responses)
    }

    /// Get service statistics from AI Oracle
    pub async fn get_service_stats(&self) -> Result<HashMap<String, Value>> {
        let stats: HashMap<String, Value> = self.post("stats", &serde_json::json!({})).await?;
        Ok(stats)
    }

    /// Check if specific AI service is available
    pub async fn is_service_available(&self, service_type: AIServiceType) -> Result<bool> {
        let services = self.discover_services().await?;
        Ok(services
            .iter()
            .any(|s| s.service_type == service_type && s.availability_score > 0.5))
    }

    /// Get the best available service for a specific type
    pub async fn get_best_service(
        &self,
        service_type: AIServiceType,
    ) -> Result<Option<AIServiceInfo>> {
        let services = self.discover_services().await?;
        let best_service = services
            .into_iter()
            .filter(|s| s.service_type == service_type)
            .max_by(|a, b| {
                a.availability_score
                    .partial_cmp(&b.availability_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

        Ok(best_service)
    }

    pub fn base_url(&self) -> &str {
        &self.config.base_url
    }

    pub fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.config.timeout_seconds)
    }
}

impl Default for AIOracleClient {
    fn default() -> Self {
        Self::new(AIServiceConfig::default())
    }
}
