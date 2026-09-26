//! AI Service Type Definitions
//!
//! This module contains all type definitions related to AI service communication,
//! requests, responses, and related data structures.

use chrono;
use serde::{Deserialize, Serialize};
use uuid;

/// Service types for AI Oracle requests
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AIServiceType {
    FraudDetection,
    RiskScoring,
    ContractAnalysis,
    AddressReputation,
    KYC,
    AML,
    CreditAssessment,
    TransactionValidation,
    PatternAnalysis,
    ThreatDetection,
    Unknown,
}

/// Priority levels for AI requests
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum RequestPriority {
    Low,
    #[default]
    Normal,
    High,
    Critical,
}

/// Response status for AI service responses
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum ResponseStatus {
    #[default]
    Success,
    Failure,
    Timeout,
    RateLimited,
    ServiceUnavailable,
    InvalidRequest,
    InternalError,
}

/// Metadata associated with AI responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIResponseMetadata {
    /// AI model version used for processing
    pub model_version: String,
    /// Confidence score for the response (0.0 to 1.0)
    pub confidence_score: Option<f64>,
    /// Processing details and statistics
    pub processing_stats: Option<serde_json::Value>,
    /// Additional context or debug information
    pub context: Option<serde_json::Value>,
    /// Oracle reputation score at time of response
    pub oracle_reputation: Option<f64>,
    /// Response correlation ID for grouping related responses
    pub correlation_id: Option<String>,
}

/// Enhanced AI Response Payload for Oracle Communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIResponsePayload {
    /// Unique response identifier matching the request
    pub id: String,
    /// Original request ID for correlation
    pub request_id: String,
    /// Type of AI service that generated the response
    pub service_type: AIServiceType,
    /// Response data specific to the service type
    pub response_data: serde_json::Value,
    /// Timestamp when the response was generated
    pub timestamp: u64,
    /// Processing time in milliseconds
    pub processing_time_ms: u64,
    /// Response status indicating success or failure
    pub status: ResponseStatus,
    /// Optional response metadata
    pub metadata: Option<AIResponseMetadata>,
    /// Error information if the response indicates failure
    pub error: Option<super::error_types::AIResponseError>,
    /// Digital signature for response verification
    pub signature: Option<String>,
    /// Oracle ID that generated this response
    pub oracle_id: Option<String>,
    /// Nonce for replay protection
    pub nonce: String,
}

/// Enhanced AI Request Payload for Oracle Communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIRequestPayload {
    /// Unique request identifier
    pub id: String,
    /// Type of AI service requested
    pub service_type: AIServiceType,
    /// Request data specific to the service type
    pub request_data: serde_json::Value,
    /// Timestamp when the request was created
    pub timestamp: u64,
    /// Request priority level
    pub priority: RequestPriority,
    /// Timeout for the request in milliseconds
    pub timeout_ms: u64,
    /// Optional request metadata
    pub metadata: Option<AIRequestMetadata>,
    /// Callback URL for async responses (optional)
    pub callback_url: Option<String>,
    /// Client/requester identification
    pub requester_id: String,
    /// Request correlation ID for grouping related requests
    pub correlation_id: Option<String>,
    /// Nonce for request uniqueness
    pub nonce: String,
}

/// Metadata associated with AI requests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIRequestMetadata {
    /// Client version information
    pub client_version: Option<String>,
    /// Request source (blockchain, api, etc.)
    pub request_source: Option<String>,
    /// Additional context for the request
    pub context: Option<serde_json::Value>,
    /// Tags for categorizing requests
    pub tags: Option<Vec<String>>,
    /// Session ID for grouping related requests
    pub session_id: Option<String>,
}

/// AI service health status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AIServiceStatus {
    /// Service is fully operational
    Healthy,
    /// Service has some issues but is partially operational
    Degraded,
    /// Service is not operational
    Unhealthy,
    /// Service status is unknown
    Unknown,
}

/// Service load information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIServiceLoad {
    /// Current CPU usage percentage (0-100)
    pub cpu_usage: Option<f64>,
    /// Current memory usage percentage (0-100)
    pub memory_usage: Option<f64>,
    /// Current request queue size
    pub queue_size: Option<u32>,
    /// Requests per second
    pub requests_per_second: Option<f64>,
    /// Average response time in milliseconds
    pub avg_response_time_ms: Option<f64>,
}

/// Health check response from AI service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIHealthCheckResponse {
    /// Service status (healthy, unhealthy, degraded)
    pub status: AIServiceStatus,
    /// Timestamp of the health check
    pub timestamp: u64,
    /// Response time in milliseconds
    pub response_time_ms: u64,
    /// Service version information
    pub version: Option<String>,
    /// Additional health details
    pub details: Option<serde_json::Value>,
    /// Available service endpoints
    pub endpoints: Option<Vec<String>>,
    /// Current load or capacity information
    pub load: Option<AIServiceLoad>,
}

/// Fallback response when AI service is unavailable
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallbackResponse {
    /// Fallback reason
    pub reason: String,
    /// Default response data
    pub response_data: serde_json::Value,
    /// Fallback confidence score
    pub confidence_score: f64,
    /// Timestamp when fallback was triggered
    pub timestamp: u64,
}

// Default implementations

// Display implementations
impl std::fmt::Display for ResponseStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResponseStatus::Success => write!(f, "Success"),
            ResponseStatus::Failure => write!(f, "Failure"),
            ResponseStatus::Timeout => write!(f, "Timeout"),
            ResponseStatus::RateLimited => write!(f, "Rate Limited"),
            ResponseStatus::ServiceUnavailable => write!(f, "Service Unavailable"),
            ResponseStatus::InvalidRequest => write!(f, "Invalid Request"),
            ResponseStatus::InternalError => write!(f, "Internal Error"),
        }
    }
}

impl std::fmt::Display for RequestPriority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RequestPriority::Low => write!(f, "Low"),
            RequestPriority::Normal => write!(f, "Normal"),
            RequestPriority::High => write!(f, "High"),
            RequestPriority::Critical => write!(f, "Critical"),
        }
    }
}

impl std::fmt::Display for AIServiceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AIServiceType::FraudDetection => write!(f, "Fraud Detection"),
            AIServiceType::RiskScoring => write!(f, "Risk Scoring"),
            AIServiceType::ContractAnalysis => write!(f, "Contract Analysis"),
            AIServiceType::AddressReputation => write!(f, "Address Reputation"),
            AIServiceType::KYC => write!(f, "KYC"),
            AIServiceType::AML => write!(f, "AML"),
            AIServiceType::CreditAssessment => write!(f, "Credit Assessment"),
            AIServiceType::TransactionValidation => write!(f, "Transaction Validation"),
            AIServiceType::PatternAnalysis => write!(f, "Pattern Analysis"),
            AIServiceType::ThreatDetection => write!(f, "Threat Detection"),
            AIServiceType::Unknown => write!(f, "Unknown"),
        }
    }
}

impl std::fmt::Display for AIServiceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AIServiceStatus::Healthy => write!(f, "Healthy"),
            AIServiceStatus::Degraded => write!(f, "Degraded"),
            AIServiceStatus::Unhealthy => write!(f, "Unhealthy"),
            AIServiceStatus::Unknown => write!(f, "Unknown"),
        }
    }
}

// Implementation methods for AIResponsePayload
impl AIResponsePayload {
    /// Create a new response payload with minimal required fields
    pub fn new(request_id: String, service_type: AIServiceType, status: ResponseStatus) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            request_id,
            service_type,
            response_data: serde_json::Value::Null,
            timestamp: chrono::Utc::now().timestamp() as u64,
            processing_time_ms: 0,
            status,
            metadata: None,
            error: None,
            signature: None,
            oracle_id: None,
            nonce: uuid::Uuid::new_v4().to_string(),
        }
    }

    /// Create a successful response
    pub fn success(
        request_id: String,
        service_type: AIServiceType,
        response_data: serde_json::Value,
    ) -> Self {
        Self::new(request_id, service_type, ResponseStatus::Success)
            .with_response_data(response_data)
    }

    /// Create a failed response
    pub fn failure(
        request_id: String,
        service_type: AIServiceType,
        error: super::error_types::AIResponseError,
    ) -> Self {
        Self::new(request_id, service_type, ResponseStatus::Failure).with_error(error)
    }

    /// Create a timeout response
    pub fn timeout(request_id: String, service_type: AIServiceType) -> Self {
        let error = super::error_types::AIResponseError::new(
            "TIMEOUT".to_string(),
            "Request timed out".to_string(),
            super::error_types::ErrorCategory::NetworkError,
            true,
        );
        Self::new(request_id, service_type, ResponseStatus::Timeout).with_error(error)
    }

    /// Set the response data
    pub fn with_response_data(mut self, response_data: serde_json::Value) -> Self {
        self.response_data = response_data;
        self
    }

    /// Set the processing time
    pub fn with_processing_time(mut self, processing_time_ms: u64) -> Self {
        self.processing_time_ms = processing_time_ms;
        self
    }

    /// Set the error information
    pub fn with_error(mut self, error: super::error_types::AIResponseError) -> Self {
        self.error = Some(error);
        self
    }

    /// Set the response metadata
    pub fn with_metadata(mut self, metadata: AIResponseMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Set the response signature
    pub fn with_signature(mut self, signature: String) -> Self {
        self.signature = Some(signature);
        self
    }

    /// Set the oracle ID
    pub fn with_oracle_id(mut self, oracle_id: String) -> Self {
        self.oracle_id = Some(oracle_id);
        self
    }

    /// Serialize this response to a JSON string
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Serialize this response to pretty JSON string
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserialize a response from a JSON string
    pub fn from_json(data: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(data)
    }

    /// Validate the response payload
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty() {
            return Err("Response ID cannot be empty".to_string());
        }
        if self.request_id.is_empty() {
            return Err("Request ID cannot be empty".to_string());
        }
        if self.nonce.is_empty() {
            return Err("Nonce cannot be empty".to_string());
        }
        Ok(())
    }

    /// Get the age of the response in seconds
    pub fn age_seconds(&self) -> u64 {
        let now = chrono::Utc::now().timestamp() as u64;
        now.saturating_sub(self.timestamp)
    }

    /// Check if the response is fresh (within 5 minutes)
    pub fn is_fresh(&self) -> bool {
        self.age_seconds() <= 300 // 5 minutes
    }

    /// Check if the response is stale (older than 5 minutes)
    pub fn is_stale(&self) -> bool {
        !self.is_fresh()
    }

    /// Check if the response is successful
    pub fn is_successful(&self) -> bool {
        self.status == ResponseStatus::Success
    }

    /// Check if the response is a failure
    pub fn is_failure(&self) -> bool {
        matches!(
            self.status,
            ResponseStatus::Failure | ResponseStatus::InternalError
        )
    }

    /// Check if the response is retryable
    pub fn is_retryable(&self) -> bool {
        match &self.error {
            Some(error) => error.retryable,
            None => matches!(
                self.status,
                ResponseStatus::Timeout
                    | ResponseStatus::ServiceUnavailable
                    | ResponseStatus::RateLimited
            ),
        }
    }

    /// Get the error message if the response is a failure
    pub fn error_message(&self) -> Option<&str> {
        self.error.as_ref().map(|e| e.message.as_str())
    }

    /// Get the confidence score if available
    pub fn confidence_score(&self) -> Option<f64> {
        self.metadata.as_ref().and_then(|m| m.confidence_score)
    }

    /// Check if the response has timed out
    pub fn is_timeout(&self) -> bool {
        self.status == ResponseStatus::Timeout
    }

    /// Get the model version from metadata
    pub fn model_version(&self) -> Option<&str> {
        self.metadata.as_ref().map(|m| m.model_version.as_str())
    }

    /// Get the oracle reputation from metadata
    pub fn oracle_reputation(&self) -> Option<f64> {
        self.metadata.as_ref().and_then(|m| m.oracle_reputation)
    }
}

// Implementation methods for other types
impl AIResponseMetadata {
    /// Create new response metadata with minimal fields
    pub fn new(model_version: String) -> Self {
        Self {
            model_version,
            confidence_score: None,
            processing_stats: None,
            context: None,
            oracle_reputation: None,
            correlation_id: None,
        }
    }

    /// Set the confidence score
    pub fn with_confidence_score(mut self, score: f64) -> Self {
        self.confidence_score = Some(score);
        self
    }

    /// Set the processing statistics
    pub fn with_processing_stats(mut self, stats: serde_json::Value) -> Self {
        self.processing_stats = Some(stats);
        self
    }

    /// Set the context
    pub fn with_context(mut self, context: serde_json::Value) -> Self {
        self.context = Some(context);
        self
    }

    /// Set the oracle reputation
    pub fn with_oracle_reputation(mut self, reputation: f64) -> Self {
        self.oracle_reputation = Some(reputation);
        self
    }

    /// Set the correlation ID
    pub fn with_correlation_id(mut self, correlation_id: String) -> Self {
        self.correlation_id = Some(correlation_id);
        self
    }
}

impl AIRequestPayload {
    /// Create a new request payload
    pub fn new(service_type: AIServiceType, request_data: serde_json::Value) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            service_type,
            request_data,
            timestamp: chrono::Utc::now().timestamp() as u64,
            priority: RequestPriority::Normal,
            timeout_ms: 30000, // 30 seconds default
            metadata: None,
            callback_url: None,
            requester_id: "unknown".to_string(),
            correlation_id: None,
            nonce: uuid::Uuid::new_v4().to_string(),
        }
    }

    /// Serialize to JSON
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserialize from JSON
    pub fn from_json(data: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(data)
    }
}

impl Default for AIRequestMetadata {
    fn default() -> Self {
        Self::new()
    }
}

impl AIRequestMetadata {
    /// Create new request metadata
    pub fn new() -> Self {
        Self {
            client_version: None,
            request_source: None,
            context: None,
            tags: None,
            session_id: None,
        }
    }
}
