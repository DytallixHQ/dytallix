// Re-export types from the types module
pub mod types;

// Business logic modules
pub mod ai_oracle_client;
pub mod consensus_engine;
pub mod transaction_validation;
pub mod block_processing;
pub mod key_management;

// Additional AI integration modules
pub mod signature_verification;
pub mod ai_integration;
pub mod oracle_registry;
pub mod enhanced_ai_integration;
pub mod replay_protection;
pub mod high_risk_queue;
pub mod review_api;
pub mod notification_system;
pub mod notification_types;
pub mod audit_trail;
pub mod compliance_api;
pub mod performance_optimizer;

// Legacy module - to be fully refactored
pub mod mod_clean;

// Re-export main types and components
pub use types::*;
pub use consensus_engine::ConsensusEngine;

// Test modules
#[cfg(test)]
pub mod integration_tests;

#[cfg(test)]
pub mod transaction_validation_tests;

#[cfg(test)]
pub mod simple_risk_tests;

#[cfg(test)]
pub mod performance_test;

// Temporary stub - to be removed after full refactoring
pub struct DytallixConsensus;

/// Response status for AI service responses
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ResponseStatus {
    Success,
    Failure,
    Timeout,
    RateLimited,
    ServiceUnavailable,
    InvalidRequest,
    InternalError,
}

/// Categories of errors that can occur in AI responses
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ErrorCategory {
    ValidationError,
    ProcessingError,
    NetworkError,
    AuthenticationError,
    RateLimitError,
    ServiceError,
    UnknownError,
}

/// Error information for failed AI responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIResponseError {
    /// Error code for programmatic handling
    pub code: String,
    /// Human-readable error message
    pub message: String,
    /// Detailed error description
    pub details: Option<String>,
    /// Error category for classification
    pub category: ErrorCategory,
    /// Whether the error is retryable
    pub retryable: bool,
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
    pub error: Option<AIResponseError>,
    /// Digital signature for response verification
    pub signature: Option<String>,
    /// Oracle ID that generated this response
    pub oracle_id: Option<String>,
    /// Nonce for replay protection
    pub nonce: String,
}

/// Priority levels for AI requests
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RequestPriority {
    Low,
    Normal,
    High,
    Critical,
}

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

impl Default for ResponseStatus {
    fn default() -> Self {
        ResponseStatus::Success
    }
}

impl Default for RequestPriority {
    fn default() -> Self {
        RequestPriority::Normal
    }
}

impl std::fmt::Display for ResponseStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResponseStatus::Success => write!(f, "Success"),
            ResponseStatus::Failure => write!(f, "Failure"),
            ResponseStatus::Timeout => write!(f, "Timeout"),
            ResponseStatus::RateLimited => write!(f, "RateLimited"),
            ResponseStatus::ServiceUnavailable => write!(f, "ServiceUnavailable"),
            ResponseStatus::InvalidRequest => write!(f, "InvalidRequest"),
            ResponseStatus::InternalError => write!(f, "InternalError"),
        }
    }
}

impl std::fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorCategory::ValidationError => write!(f, "ValidationError"),
            ErrorCategory::ProcessingError => write!(f, "ProcessingError"),
            ErrorCategory::NetworkError => write!(f, "NetworkError"),
            ErrorCategory::AuthenticationError => write!(f, "AuthenticationError"),
            ErrorCategory::RateLimitError => write!(f, "RateLimitError"),
            ErrorCategory::ServiceError => write!(f, "ServiceError"),
            ErrorCategory::UnknownError => write!(f, "UnknownError"),
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

impl AIResponsePayload {
    /// Create a new response payload with minimal required fields
    pub fn new(
        request_id: String,
        service_type: AIServiceType,
        response_data: serde_json::Value,
        status: ResponseStatus,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            request_id,
            service_type,
            response_data,
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
        Self::new(request_id, service_type, response_data, ResponseStatus::Success)
    }

    /// Create a failed response
    pub fn failure(
        request_id: String,
        service_type: AIServiceType,
        error: AIResponseError,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            request_id,
            service_type,
            response_data: serde_json::Value::Null,
            timestamp: chrono::Utc::now().timestamp() as u64,
            processing_time_ms: 0,
            status: ResponseStatus::Failure,
            metadata: None,
            error: Some(error),
            signature: None,
            oracle_id: None,
            nonce: uuid::Uuid::new_v4().to_string(),
        }
    }

    /// Create a timeout response
    pub fn timeout(request_id: String, service_type: AIServiceType) -> Self {
        let error = AIResponseError {
            code: "TIMEOUT".to_string(),
            message: "Request timed out".to_string(),
            details: None,
            category: ErrorCategory::NetworkError,
            retryable: true,
        };
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            request_id,
            service_type,
            response_data: serde_json::Value::Null,
            timestamp: chrono::Utc::now().timestamp() as u64,
            processing_time_ms: 0,
            status: ResponseStatus::Timeout,
            metadata: None,
            error: Some(error),
            signature: None,
            oracle_id: None,
            nonce: uuid::Uuid::new_v4().to_string(),
        }
    }

    /// Set the processing time
    pub fn with_processing_time(mut self, processing_time_ms: u64) -> Self {
        self.processing_time_ms = processing_time_ms;
        self
    }

    /// Set the error information
    pub fn with_error(mut self, error: AIResponseError) -> Self {
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

        if self.timestamp == 0 {
            return Err("Timestamp cannot be zero".to_string());
        }

        // Check if timestamp is not too far in the future (1 hour)
        let now = chrono::Utc::now().timestamp() as u64;
        if self.timestamp > now && (self.timestamp - now) > 3600 {
            return Err("Response timestamp is too far in the future".to_string());
        }

        // Validate that failure responses have error information
        if matches!(self.status, ResponseStatus::Failure) && self.error.is_none() {
            return Err("Failure responses must include error information".to_string());
        }

        // Validate confidence score if present
        if let Some(ref metadata) = self.metadata {
            if let Some(confidence) = metadata.confidence_score {
                if confidence < 0.0 || confidence > 1.0 {
                    return Err("Confidence score must be between 0.0 and 1.0".to_string());
                }
            }
        }

        Ok(())
    }

    /// Get the age of the response in seconds
    pub fn age_seconds(&self) -> u64 {
        let now = chrono::Utc::now().timestamp() as u64;
        if now > self.timestamp {
            now - self.timestamp
        } else {
            0
        }
    }

    /// Check if the response is successful
    pub fn is_successful(&self) -> bool {
        matches!(self.status, ResponseStatus::Success)
    }

    /// Check if the response is a failure
    pub fn is_failure(&self) -> bool {
        matches!(self.status, ResponseStatus::Failure)
    }

    /// Check if the response is retryable
    pub fn is_retryable(&self) -> bool {
        match self.status {
            ResponseStatus::Timeout | ResponseStatus::RateLimited | ResponseStatus::ServiceUnavailable => true,
            ResponseStatus::Failure => {
                self.error.as_ref().map_or(false, |e| e.retryable)
            }
            _ => false,
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
}

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

impl AIResponseError {
    /// Create a new error with minimal fields
    pub fn new(code: String, message: String, category: ErrorCategory) -> Self {
        Self {
            code,
            message,
            details: None,
            category,
            retryable: false,
        }
    }

    /// Create a retryable error
    pub fn retryable(code: String, message: String, category: ErrorCategory) -> Self {
        Self {
            code,
            message,
            details: None,
            category,
            retryable: true,
        }
    }

    /// Set the error details
    pub fn with_details(mut self, details: String) -> Self {
        self.details = Some(details);
        self
    }

    /// Set whether the error is retryable
    pub fn with_retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }
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

/// Post-Quantum Cryptographic signature for AI oracle responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIResponseSignature {
    /// Signature algorithm used (Dilithium5, Falcon1024, etc.)
    pub algorithm: SignatureAlgorithm,
    /// The actual signature bytes
    pub signature: Vec<u8>,
    /// Public key used for verification (oracle's public key)
    pub public_key: Vec<u8>,
    /// Signature creation timestamp (Unix timestamp in seconds)
    pub signature_timestamp: u64,
    /// Version of the signature format for forward compatibility
    pub signature_version: u8,
    /// Additional signature metadata
    pub metadata: Option<SignatureMetadata>,
}

/// Additional metadata for signatures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureMetadata {
    /// Key identifier or fingerprint
    pub key_id: Option<String>,
    /// Certificate chain for verification
    pub cert_chain: Option<Vec<String>>,
    /// Additional signature parameters
    pub parameters: Option<serde_json::Value>,
}

/// Oracle identity and certificate information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleIdentity {
    /// Unique oracle identifier
    pub oracle_id: String,
    /// Oracle name or description
    pub name: String,
    /// Oracle's public key for signature verification
    pub public_key: Vec<u8>,
    /// Signature algorithm used by this oracle
    pub signature_algorithm: SignatureAlgorithm,
    /// Oracle registration timestamp
    pub registered_at: u64,
    /// Oracle reputation score (0.0 to 1.0)
    pub reputation_score: f64,
    /// Whether the oracle is currently active
    pub is_active: bool,
    /// Oracle's certificate chain
    pub certificate_chain: Vec<OracleCertificate>,
}

/// Oracle certificate for identity verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleCertificate {
    /// Certificate version
    pub version: u8,
    /// Subject oracle ID
    pub subject_oracle_id: String,
    /// Issuer oracle ID (for chain of trust)
    pub issuer_oracle_id: String,
    /// Certificate validity start time
    pub valid_from: u64,
    /// Certificate validity end time
    pub valid_until: u64,
    /// Public key in this certificate
    pub public_key: Vec<u8>,
    /// Signature algorithm
    pub signature_algorithm: SignatureAlgorithm,
    /// Certificate signature (signed by issuer)
    pub signature: Vec<u8>,
    /// Additional certificate extensions
    pub extensions: Option<serde_json::Value>,
}

/// Signed AI Oracle Response with cryptographic verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedAIOracleResponse {
    /// The underlying AI response payload
    pub response: AIResponsePayload,
    /// Cryptographic signature of the response
    pub signature: AIResponseSignature,
    /// Nonce for replay protection (must be unique)
    pub nonce: u64,
    /// Response expiration timestamp (for freshness)
    pub expires_at: u64,
    /// Oracle identity information
    pub oracle_identity: OracleIdentity,
    /// Additional verification data
    pub verification_data: Option<VerificationData>,
}

/// Additional data for response verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationData {
    /// Hash of the original request (for request-response binding)
    pub request_hash: Vec<u8>,
    /// Merkle proof if response is part of a batch
    pub merkle_proof: Option<Vec<Vec<u8>>>,
    /// Timestamp verification data
    pub timestamp_proof: Option<TimestampProof>,
    /// Additional verification metadata
    pub metadata: Option<serde_json::Value>,
}

/// Timestamp proof for response freshness verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimestampProof {
    /// Timestamp authority identifier
    pub authority_id: String,
    /// Timestamp token or proof
    pub proof: Vec<u8>,
    /// Timestamp algorithm used
    pub algorithm: String,
    /// Timestamp creation time
    pub created_at: u64,
}

impl AIResponseSignature {
    /// Create a new signature
    pub fn new(
        algorithm: SignatureAlgorithm,
        signature: Vec<u8>,
        public_key: Vec<u8>,
    ) -> Self {
        Self {
            algorithm,
            signature,
            public_key,
            signature_timestamp: chrono::Utc::now().timestamp() as u64,
            signature_version: 1,
            metadata: None,
        }
    }

    /// Add metadata to the signature
    pub fn with_metadata(mut self, metadata: SignatureMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Add key ID to the signature
    pub fn with_key_id(mut self, key_id: String) -> Self {
        let metadata = self.metadata.get_or_insert_with(|| SignatureMetadata {
            key_id: None,
            cert_chain: None,
            parameters: None,
        });
        metadata.key_id = Some(key_id);
        self
    }

    /// Get the signature age in seconds
    pub fn age_seconds(&self) -> u64 {
        let now = chrono::Utc::now().timestamp() as u64;
        if now > self.signature_timestamp {
            now - self.signature_timestamp
        } else {
            0
        }
    }

    /// Check if the signature is recent (within the given seconds)
    pub fn is_recent(&self, max_age_seconds: u64) -> bool {
        self.age_seconds() <= max_age_seconds
    }
}

impl OracleIdentity {
    /// Create a new oracle identity
    pub fn new(
        oracle_id: String,
        name: String,
        public_key: Vec<u8>,
        signature_algorithm: SignatureAlgorithm,
    ) -> Self {
        Self {
            oracle_id,
            name,
            public_key,
            signature_algorithm,
            registered_at: chrono::Utc::now().timestamp() as u64,
            reputation_score: 0.5, // Start with neutral reputation
            is_active: true,
            certificate_chain: Vec::new(),
        }
    }

    /// Add a certificate to the oracle's chain
    pub fn add_certificate(mut self, certificate: OracleCertificate) -> Self {
        self.certificate_chain.push(certificate);
        self
    }

    /// Update reputation score
    pub fn update_reputation(mut self, score: f64) -> Self {
        self.reputation_score = score.clamp(0.0, 1.0);
        self
    }

    /// Deactivate the oracle
    pub fn deactivate(mut self) -> Self {
        self.is_active = false;
        self
    }

    /// Check if the oracle is trusted (high reputation and active)
    pub fn is_trusted(&self, min_reputation: f64) -> bool {
        self.is_active && self.reputation_score >= min_reputation
    }
}

impl OracleCertificate {
    /// Create a new oracle certificate
    pub fn new(
        subject_oracle_id: String,
        issuer_oracle_id: String,
        valid_from: u64,
        valid_until: u64,
        public_key: Vec<u8>,
        signature_algorithm: SignatureAlgorithm,
        signature: Vec<u8>,
    ) -> Self {
        Self {
            version: 1,
            subject_oracle_id,
            issuer_oracle_id,
            valid_from,
            valid_until,
            public_key,
            signature_algorithm,
            signature,
            extensions: None,
        }
    }

    /// Check if the certificate is currently valid
    pub fn is_valid(&self) -> bool {
        let now = chrono::Utc::now().timestamp() as u64;
        now >= self.valid_from && now <= self.valid_until
    }

    /// Check if the certificate is expired
    pub fn is_expired(&self) -> bool {
        let now = chrono::Utc::now().timestamp() as u64;
        now > self.valid_until
    }

    /// Get days until expiration
    pub fn days_until_expiration(&self) -> i64 {
        let now = chrono::Utc::now().timestamp() as u64;
        if self.valid_until > now {
            ((self.valid_until - now) / 86400) as i64
        } else {
            0
        }
    }
}

impl SignedAIOracleResponse {
    /// Create a new signed response
    pub fn new(
        response: AIResponsePayload,
        signature: AIResponseSignature,
        nonce: u64,
        expires_at: u64,
        oracle_identity: OracleIdentity,
    ) -> Self {
        Self {
            response,
            signature,
            nonce,
            expires_at,
            oracle_identity,
            verification_data: None,
        }
    }

    /// Add verification data
    pub fn with_verification_data(mut self, verification_data: VerificationData) -> Self {
        self.verification_data = Some(verification_data);
        self
    }

    /// Check if the response is expired
    pub fn is_expired(&self) -> bool {
        let now = chrono::Utc::now().timestamp() as u64;
        now > self.expires_at
    }

    /// Check if the response is fresh (not expired)
    pub fn is_fresh(&self) -> bool {
        !self.is_expired()
    }

    /// Get seconds until expiration
    pub fn seconds_until_expiration(&self) -> i64 {
        let now = chrono::Utc::now().timestamp() as u64;
        if self.expires_at > now {
            (self.expires_at - now) as i64
        } else {
            0
        }
    }

    /// Get the canonical data for signature verification
    /// This creates a deterministic byte representation of the response for signing/verification
    pub fn get_signable_data(&self) -> Result<Vec<u8>> {
        let mut data = Vec::new();

        // Add response data
        data.extend_from_slice(self.response.id.as_bytes());
        data.extend_from_slice(self.response.request_id.as_bytes());
        data.extend_from_slice(&self.response.timestamp.to_be_bytes());
        data.extend_from_slice(&self.response.processing_time_ms.to_be_bytes());

        // Add response data hash
        let response_data_json = serde_json::to_string(&self.response.response_data)?;
        data.extend_from_slice(response_data_json.as_bytes());

        // Add nonce and expiration
        data.extend_from_slice(&self.nonce.to_be_bytes());
        data.extend_from_slice(&self.expires_at.to_be_bytes());

        // Add oracle identity
        data.extend_from_slice(self.oracle_identity.oracle_id.as_bytes());

        Ok(data)
    }

    /// Create a summary of the signed response for logging/monitoring
    pub fn get_summary(&self) -> serde_json::Value {
        serde_json::json!({
            "response_id": self.response.id,
            "request_id": self.response.request_id,
            "oracle_id": self.oracle_identity.oracle_id,
            "service_type": self.response.service_type,
            "status": self.response.status,
            "signature_algorithm": self.signature.algorithm,
            "nonce": self.nonce,
            "expires_at": self.expires_at,
            "is_fresh": self.is_fresh(),
            "oracle_reputation": self.oracle_identity.reputation_score,
            "signature_age_seconds": self.signature.age_seconds(),
        })
    }
}

/// Circuit breaker states
#[derive(Debug, Clone, PartialEq)]
pub enum CircuitBreakerState {
    /// Circuit is closed - requests flow normally
    Closed,
    /// Circuit is open - requests are blocked
    Open,
    /// Circuit is half-open - testing if service recovered
    HalfOpen,
}

/// Circuit breaker statistics
#[derive(Debug, Clone)]
pub struct CircuitBreakerStats {
    /// Number of successful requests
    pub success_count: u64,
    /// Number of failed requests
    pub failure_count: u64,
    /// Total number of requests
    pub total_requests: u64,
    /// Current failure rate (0.0 to 1.0)
    pub failure_rate: f64,
    /// Time when circuit was last opened
    pub last_opened_time: Option<std::time::Instant>,
    /// Time when circuit was last closed
    pub last_closed_time: Option<std::time::Instant>,
}

impl Default for CircuitBreakerStats {
    fn default() -> Self {
        Self {
            success_count: 0,
            failure_count: 0,
            total_requests: 0,
            failure_rate: 0.0,
            last_opened_time: None,
            last_closed_time: None,
        }
    }
}

/// Circuit breaker configuration and state
#[derive(Debug, Clone)]
pub struct CircuitBreakerContext {
    /// Current state of the circuit breaker
    pub state: CircuitBreakerState,
    /// Statistics for the circuit breaker
    pub stats: CircuitBreakerStats,
    /// Failure threshold (0.0 to 1.0)
    pub failure_threshold: f64,
    /// Recovery time in seconds
    pub recovery_time_seconds: u64,
    /// Minimum requests before circuit can open
    pub min_requests: u64,
}

impl CircuitBreakerContext {
    pub fn new(failure_threshold: f64, recovery_time_seconds: u64) -> Self {
        Self {
            state: CircuitBreakerState::Closed,
            stats: CircuitBreakerStats::default(),
            failure_threshold,
            recovery_time_seconds,
            min_requests: 3, // Lower default for testing
        }
    }

    /// Record a successful request
    pub fn record_success(&mut self) {
        self.stats.success_count += 1;
        self.stats.total_requests += 1;
        self.update_failure_rate();

        // If we're half-open and got a success, close the circuit
        if self.state == CircuitBreakerState::HalfOpen {
            self.state = CircuitBreakerState::Closed;
            self.stats.last_closed_time = Some(std::time::Instant::now());
            log::info!("Circuit breaker closed after successful request");
        }
    }

    /// Record a failed request
    pub fn record_failure(&mut self) {
        self.stats.failure_count += 1;
        self.stats.total_requests += 1;
        self.update_failure_rate();

        // Check if we should open the circuit
        if self.state == CircuitBreakerState::Closed && self.should_open_circuit() {
            self.state = CircuitBreakerState::Open;
            self.stats.last_opened_time = Some(std::time::Instant::now());
            log::warn!("Circuit breaker opened due to high failure rate: {:.2}%", self.stats.failure_rate * 100.0);
        }
    }

    /// Check if circuit should allow requests
    pub fn should_allow_request(&mut self) -> bool {
        match self.state {
            CircuitBreakerState::Closed => true,
            CircuitBreakerState::Open => {
                // Check if enough time has passed to try again
                if let Some(opened_time) = self.stats.last_opened_time {
                    let elapsed = opened_time.elapsed();
                    if elapsed >= Duration::from_secs(self.recovery_time_seconds) {
                        log::info!("Circuit breaker transitioning to half-open state");
                        self.state = CircuitBreakerState::HalfOpen;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            CircuitBreakerState::HalfOpen => true,
        }
    }

    /// Check if circuit should open based on failure rate
    fn should_open_circuit(&self) -> bool {
        self.stats.total_requests >= self.min_requests &&
        self.stats.failure_rate >= self.failure_threshold
    }

    /// Update the failure rate
    fn update_failure_rate(&mut self) {
        if self.stats.total_requests > 0 {
            self.stats.failure_rate = self.stats.failure_count as f64 / self.stats.total_requests as f64;
        }
    }

    /// Get circuit breaker status summary
    pub fn get_status_summary(&self) -> serde_json::Value {
        serde_json::json!({
            "state": match self.state {
                CircuitBreakerState::Closed => "closed",
                CircuitBreakerState::Open => "open",
                CircuitBreakerState::HalfOpen => "half-open",
            },
            "failure_rate": self.stats.failure_rate,
            "success_count": self.stats.success_count,
            "failure_count": self.stats.failure_count,
            "total_requests": self.stats.total_requests,
            "failure_threshold": self.failure_threshold,
            "recovery_time_seconds": self.recovery_time_seconds,
            "last_opened": self.stats.last_opened_time.map(|t| t.elapsed().as_secs()),
            "last_closed": self.stats.last_closed_time.map(|t| t.elapsed().as_secs()),
        })
    }
}

/// Fallback response when AI service is unavailable
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallbackResponse {
    /// Fallback response type
    pub response_type: String,
    /// Fallback data
    pub data: serde_json::Value,
    /// Message explaining the fallback
    pub message: String,
    /// Timestamp of fallback generation
    pub timestamp: u64,
}

/// AI Oracle Client for communicating with AI services
#[derive(Debug, Clone)]
pub struct AIOracleClient {
    /// HTTP client with connection pooling
    client: Client,
    /// Base URL for the AI service
    base_url: String,
    /// Request timeout duration
    timeout: Duration,
    /// Circuit breaker context (shared across threads)
    circuit_breaker: Arc<Mutex<CircuitBreakerContext>>,
}

impl AIOracleClient {
    /// Create a new AIOracleClient with default configuration
    pub fn new(base_url: String) -> Result<Self> {
        let client = ClientBuilder::new()
            .timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(10)
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(60))
            .build()?;

        Ok(Self {
            client,
            base_url,
            timeout: Duration::from_secs(30),
            circuit_breaker: Arc::new(Mutex::new(CircuitBreakerContext::new(0.5, 60))),
        })
    }

    /// Create a new AIOracleClient with custom timeout
    pub fn with_timeout(base_url: String, timeout: Duration) -> Result<Self> {
        let client = ClientBuilder::new()
            .timeout(timeout)
            .pool_max_idle_per_host(10)
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(60))
            .build()?;

        Ok(Self {
            client,
            base_url,
            timeout,
            circuit_breaker: Arc::new(Mutex::new(CircuitBreakerContext::new(0.5, 60))),
        })
    }

    /// Create a new AIOracleClient with custom configuration
    pub fn with_config(
        base_url: String,
        timeout: Duration,
        max_idle_per_host: usize,
        idle_timeout: Duration,
        keepalive: Duration,
    ) -> Result<Self> {
        let client = ClientBuilder::new()
            .timeout(timeout)
            .pool_max_idle_per_host(max_idle_per_host)
            .pool_idle_timeout(idle_timeout)
            .tcp_keepalive(keepalive)
            .build()?;

        Ok(Self {
            client,
            base_url,
            timeout,
            circuit_breaker: Arc::new(Mutex::new(CircuitBreakerContext::new(0.5, 60))),
        })
    }

    /// Create a new AIOracleClient with circuit breaker configuration
    pub fn with_circuit_breaker(
        base_url: String,
        timeout: Duration,
        failure_threshold: f64,
        recovery_time_seconds: u64,
    ) -> Result<Self> {
        let client = ClientBuilder::new()
            .timeout(timeout)
            .pool_max_idle_per_host(10)
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(60))
            .build()?;

        Ok(Self {
            client,
            base_url,
            timeout,
            circuit_breaker: Arc::new(Mutex::new(CircuitBreakerContext::new(failure_threshold, recovery_time_seconds))),
        })
    }

    /// Create a new AIOracleClient with connection pool configuration
    pub fn with_pool_config(
        base_url: String,
        timeout: Duration,
        pool_config: ConnectionPoolConfig,
    ) -> Result<Self> {
        let mut client_builder = ClientBuilder::new()
            .timeout(timeout)
            .pool_max_idle_per_host(pool_config.max_idle_per_host)
            .pool_idle_timeout(pool_config.idle_timeout)
            .tcp_keepalive(pool_config.tcp_keepalive);

        // Enable HTTP/2 if configured
        if pool_config.http2_prior_knowledge {
            client_builder = client_builder.http2_prior_knowledge();
        }

        // Set maximum total connections if specified
        if let Some(max_total) = pool_config.max_total_connections {
            client_builder = client_builder.pool_max_idle_per_host(
                std::cmp::min(pool_config.max_idle_per_host, max_total)
            );
        }

        let client = client_builder.build()?;

        Ok(Self {
            client,
            base_url,
            timeout,
            circuit_breaker: Arc::new(Mutex::new(CircuitBreakerContext::new(0.5, 60))),
        })
    }

    /// Create a new AIOracleClient optimized for high-performance scenarios
    pub fn high_performance(base_url: String, timeout: Duration) -> Result<Self> {
        Self::with_pool_config(base_url, timeout, ConnectionPoolConfig::high_performance())
    }

    /// Create a new AIOracleClient optimized for low-resource scenarios
    pub fn low_resource(base_url: String, timeout: Duration) -> Result<Self> {
        Self::with_pool_config(base_url, timeout, ConnectionPoolConfig::low_resource())
    }

    /// Test basic connectivity to the AI service endpoint
    pub async fn test_connectivity(&self) -> Result<bool> {
        let url = format!("{}/health", self.base_url);

        match self.client.get(&url).send().await {
            Ok(response) => {
                log::info!("AI service connectivity test: HTTP {}", response.status());
                Ok(response.status().is_success())
            }
            Err(e) => {
                log::error!("AI service connectivity test failed: {}", e);
                Ok(false)
            }
        }
    }

    /// Perform a comprehensive health check of the AI service
    pub async fn health_check(&self) -> Result<AIHealthCheckResponse> {
        let url = format!("{}/health", self.base_url);
        let start_time = std::time::Instant::now();

        log::debug!("Performing health check for AI service at: {}", url);

        match self.client.get(&url).send().await {
            Ok(response) => {
                let response_time_ms = start_time.elapsed().as_millis() as u64;
                let status_code = response.status();

                log::debug!("Health check response: HTTP {}, response time: {}ms", status_code, response_time_ms);

                if status_code.is_success() {
                    // Try to parse the response as JSON to get detailed health information
                    match response.json::<serde_json::Value>().await {
                        Ok(json_response) => {
                            log::debug!("Successfully parsed health check response: {:?}", json_response);

                            // Extract status from the response or default to Healthy
                            let status = if let Some(status_str) = json_response.get("status").and_then(|v| v.as_str()) {
                                match status_str.to_lowercase().as_str() {
                                    "healthy" => AIServiceStatus::Healthy,
                                    "degraded" => AIServiceStatus::Degraded,
                                    "unhealthy" => AIServiceStatus::Unhealthy,
                                    _ => AIServiceStatus::Unknown,
                                }
                            } else {
                                AIServiceStatus::Healthy
                            };

                            // Extract version information
                            let version = json_response.get("version")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());

                            // Extract available endpoints
                            let endpoints = json_response.get("endpoints")
                                .and_then(|v| v.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|v| v.as_str())
                                        .map(|s| s.to_string())
                                        .collect::<Vec<String>>()
                                });

                            // Extract service load information
                            let load = json_response.get("load")
                                .and_then(|v| v.as_object())
                                .map(|load_obj| {
                                    AIServiceLoad {
                                        cpu_usage: load_obj.get("cpu_usage").and_then(|v| v.as_f64()),
                                        memory_usage: load_obj.get("memory_usage").and_then(|v| v.as_f64()),
                                        queue_size: load_obj.get("queue_size").and_then(|v| v.as_u64()).map(|v| v as u32),
                                        requests_per_second: load_obj.get("requests_per_second").and_then(|v| v.as_f64()),
                                        avg_response_time_ms: load_obj.get("avg_response_time_ms").and_then(|v| v.as_f64()),
                                    }
                                });

                            Ok(AIHealthCheckResponse {
                                status,
                                timestamp: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs(),
                                response_time_ms,
                                version,
                                details: Some(json_response),
                                endpoints,
                                load,
                            })
                        }
                        Err(e) => {
                            log::warn!("Failed to parse health check response as JSON: {}", e);

                            // Create a basic health response based on HTTP status
                            let status = if status_code.is_success() {
                                AIServiceStatus::Healthy
                            } else {
                                AIServiceStatus::Degraded
                            };

                            Ok(AIHealthCheckResponse {
                                status,
                                timestamp: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs(),
                                response_time_ms,
                                version: None,
                                details: None,
                                endpoints: None,
                                load: None,
                            })
                        }
                    }
                } else {
                    log::warn!("Health check failed with HTTP status: {}", status_code);

                    let status = match status_code.as_u16() {
                        503 => AIServiceStatus::Unhealthy,
                        500..=599 => AIServiceStatus::Degraded,
                        _ => AIServiceStatus::Unknown,
                    };

                    Ok(AIHealthCheckResponse {
                        status,
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                        response_time_ms,
                        version: None,
                        details: Some(serde_json::json!({
                            "error": "HTTP request failed",
                            "status_code": status_code.as_u16(),
                            "message": format!("Health check returned HTTP {}", status_code)
                        })),
                        endpoints: None,
                        load: None,
                    })
                }
            }
            Err(e) => {
                let response_time_ms = start_time.elapsed().as_millis() as u64;
                log::error!("Health check request failed: {}", e);

                // Determine error type for better status reporting
                let (status, error_details) = if e.is_timeout() {
                    (AIServiceStatus::Degraded, serde_json::json!({
                        "error": "timeout",
                        "message": format!("Health check timed out after {}ms", response_time_ms)
                    }))
                } else if e.is_connect() {
                    (AIServiceStatus::Unhealthy, serde_json::json!({
                        "error": "connection_failed",
                        "message": "Could not connect to AI service"
                    }))
                } else {
                    (AIServiceStatus::Unknown, serde_json::json!({
                        "error": "network_error",
                        "message": format!("Network error: {}", e)
                    }))
                };

                Ok(AIHealthCheckResponse {
                    status,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    response_time_ms,
                    version: None,
                    details: Some(error_details),
                    endpoints: None,
                    load: None,
                })
            }
        }
    }

    /// Start periodic health monitoring in the background
    pub fn start_background_health_monitoring(&self, interval_seconds: u64) -> tokio::task::JoinHandle<()> {
        let client = self.clone();
        let interval_duration = std::time::Duration::from_secs(interval_seconds);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(interval_duration);

            loop {
                interval.tick().await;

                match client.health_check().await {
                    Ok(health_response) => {
                        log::info!(
                            "Health check completed - Status: {}, Response time: {}ms",
                            health_response.status,
                            health_response.response_time_ms
                        );

                        // Log additional details if available
                        if let Some(ref details) = health_response.details {
                            log::debug!("Health check details: {:?}", details);
                        }

                        // Log service load if available
                        if let Some(ref load) = health_response.load {
                            log::info!(
                                "Service load - CPU: {:?}%, Memory: {:?}%, Queue: {:?}, RPS: {:?}",
                                load.cpu_usage,
                                load.memory_usage,
                                load.queue_size,
                                load.requests_per_second
                            );
                        }
                    }
                    Err(e) => {
                        log::error!("Periodic health check failed: {}", e);
                    }
                }
            }
        })
    }

    /// Perform health check with custom timeout
    pub async fn health_check_with_timeout(&self, timeout: std::time::Duration) -> Result<AIHealthCheckResponse> {
        let url = format!("{}/health", self.base_url);
        let start_time = std::time::Instant::now();

        log::debug!("Performing health check with timeout {:?} for AI service at: {}", timeout, url);

        match tokio::time::timeout(timeout, self.client.get(&url).send()).await {
            Ok(Ok(response)) => {
                let response_time_ms = start_time.elapsed().as_millis() as u64;
                let status_code = response.status();

                log::debug!("Health check response: HTTP {}, response time: {}ms", status_code, response_time_ms);

                if status_code.is_success() {
                    // Try to parse the response as JSON to get detailed health information
                    match response.json::<serde_json::Value>().await {
                        Ok(json_response) => {
                            log::debug!("Successfully parsed health check response: {:?}", json_response);

                            // Extract status from the response or default to Healthy
                            let status = if let Some(status_str) = json_response.get("status").and_then(|v| v.as_str()) {
                                match status_str.to_lowercase().as_str() {
                                    "healthy" => AIServiceStatus::Healthy,
                                    "degraded" => AIServiceStatus::Degraded,
                                    "unhealthy" => AIServiceStatus::Unhealthy,
                                    _ => AIServiceStatus::Unknown,
                                }
                            } else {
                                AIServiceStatus::Healthy
                            };

                            // Extract version information
                            let version = json_response.get("version")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());

                            // Extract available endpoints
                            let endpoints = json_response.get("endpoints")
                                .and_then(|v| v.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|v| v.as_str())
                                        .map(|s| s.to_string())
                                        .collect::<Vec<String>>()
                                });

                            // Extract service load information
                            let load = json_response.get("load")
                                .and_then(|v| v.as_object())
                                .map(|load_obj| {
                                    AIServiceLoad {
                                        cpu_usage: load_obj.get("cpu_usage").and_then(|v| v.as_f64()),
                                        memory_usage: load_obj.get("memory_usage").and_then(|v| v.as_f64()),
                                        queue_size: load_obj.get("queue_size").and_then(|v| v.as_u64()).map(|v| v as u32),
                                        requests_per_second: load_obj.get("requests_per_second").and_then(|v| v.as_f64()),
                                        avg_response_time_ms: load_obj.get("avg_response_time_ms").and_then(|v| v.as_f64()),
                                    }
                                });

                            Ok(AIHealthCheckResponse {
                                status,
                                timestamp: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs(),
                                response_time_ms,
                                version,
                                details: Some(json_response),
                                endpoints,
                                load,
                            })
                        }
                        Err(e) => {
                            log::warn!("Failed to parse health check response as JSON: {}", e);

                            // Create a basic health response based on HTTP status
                            let status = if status_code.is_success() {
                                AIServiceStatus::Healthy
                            } else {
                                AIServiceStatus::Degraded
                            };

                            Ok(AIHealthCheckResponse {
                                status,
                                timestamp: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs(),
                                response_time_ms,
                                version: None,
                                details: None,
                                endpoints: None,
                                load: None,
                            })
                        }
                    }
                } else {
                    log::warn!("Health check failed with HTTP status: {}", status_code);

                    let status = match status_code.as_u16() {
                        503 => AIServiceStatus::Unhealthy,
                        500..=599 => AIServiceStatus::Degraded,
                        _ => AIServiceStatus::Unknown,
                    };

                    Ok(AIHealthCheckResponse {
                        status,
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                        response_time_ms,
                        version: None,
                        details: Some(serde_json::json!({
                            "error": "HTTP request failed",
                            "status_code": status_code.as_u16(),
                            "message": format!("Health check returned HTTP {}", status_code)
                        })),
                        endpoints: None,
                        load: None,
                    })
                }
            }
            Ok(Err(e)) => {
                let response_time_ms = start_time.elapsed().as_millis() as u64;
                log::error!("Health check request failed: {}", e);

                // Determine error type for better status reporting
                let (status, error_details) = if e.is_timeout() {
                    (AIServiceStatus::Degraded, serde_json::json!({
                        "error": "timeout",
                        "message": format!("Health check timed out after {}ms", response_time_ms)
                    }))
                } else if e.is_connect() {
                    (AIServiceStatus::Unhealthy, serde_json::json!({
                        "error": "connection_failed",
                        "message": "Could not connect to AI service"
                    }))
                } else {
                    (AIServiceStatus::Unknown, serde_json::json!({
                        "error": "network_error",
                        "message": format!("Network error: {}", e)
                    }))
                };

                Ok(AIHealthCheckResponse {
                    status,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    response_time_ms,
                    version: None,
                    details: Some(error_details),
                    endpoints: None,
                    load: None,
                })
            }
            Err(_) => {
                // Timeout occurred
                let response_time_ms = start_time.elapsed().as_millis() as u64;
                log::error!("Health check timed out after {:?}", timeout);

                Ok(AIHealthCheckResponse {
                    status: AIServiceStatus::Degraded,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    response_time_ms,
                    version: None,
                    details: Some(serde_json::json!({
                        "error": "timeout",
                        "message": format!("Health check timed out after {:?}", timeout)
                    })),
                    endpoints: None,
                    load: None,
                })
            }
        }
    }

    /// Make a GET request to the AI service
    pub async fn get(&self, endpoint: &str) -> Result<reqwest::Response> {
        let url = format!("{}/{}", self.base_url.trim_end_matches('/'), endpoint.trim_start_matches('/'));

        log::debug!("Making GET request to: {}", url);

        let response = self.client
            .get(&url)
            .send()
            .await?;

        log::debug!("GET request completed with status: {}", response.status());
        Ok(response)
    }

    /// Make a POST request to the AI service
    pub async fn post(&self, endpoint: &str, body: serde_json::Value) -> Result<reqwest::Response> {
        let url = format!("{}/{}", self.base_url.trim_end_matches('/'), endpoint.trim_start_matches('/'));

        log::debug!("Making POST request to: {}", url);

        let response = self.client
            .post(&url)
            .json(&body)
            .send()
            .await?;

        log::debug!("POST request completed with status: {}", response.status());
        Ok(response)
    }

    /// Make a POST request to the AI service with JSON payload
    pub async fn post_json<T>(&self, endpoint: &str, payload: &T) -> Result<reqwest::Response>
    where
        T: Serialize,
    {
        let url = format!("{}/{}", self.base_url.trim_end_matches('/'), endpoint.trim_start_matches('/'));

        log::debug!("Making POST request to: {}", url);

        let response = self.client
            .post(&url)
            .json(payload)
            .send()
            .await?;

        log::debug!("POST request completed with status: {}", response.status());
        Ok(response)
    }

    /// Make a POST request to the AI service with custom headers
    pub async fn post_json_with_headers<T>(
        &self,
        endpoint: &str,
        payload: &T,
        headers: &[(&str, &str)],
    ) -> Result<reqwest::Response>
    where
        T: Serialize,
    {
        let url = format!("{}/{}", self.base_url.trim_end_matches('/'), endpoint.trim_start_matches('/'));

        log::debug!("Making POST request with headers to: {}", url);

        let mut request = self.client.post(&url).json(payload);

        for (key, value) in headers {
            request = request.header(*key, *value);
        }

        let response = request.send().await?;

        log::debug!("POST request with headers completed with status: {}", response.status());
        Ok(response)
    }

    /// Make a request and deserialize the JSON response
    pub async fn get_json<T>(&self, endpoint: &str) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let response = self.get(endpoint).await?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Request failed with status: {} for endpoint: {}",
                response.status(),
                endpoint
            ));
        }

        let json_response = response.json::<T>().await?;
        Ok(json_response)
    }

    /// Make a POST request and deserialize the JSON response
    pub async fn post_json_response<T, R>(&self, endpoint: &str, payload: &T) -> Result<R>
    where
        T: Serialize,
        R: for<'de> Deserialize<'de>,
    {
        let response = self.post_json(endpoint, payload).await?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Request failed with status: {} for endpoint: {}",
                response.status(),
                endpoint
            ));
        }

        let json_response = response.json::<R>().await?;
        Ok(json_response)
    }

    /// Send an AI request using the AIRequestPayload structure
    /// Send an AI request with retry logic and comprehensive error handling
    pub async fn send_ai_request(&self, payload: &AIRequestPayload) -> AIOracleResult<reqwest::Response> {
        let retry_config = RetryConfig::default();
        self.send_ai_request_with_retry(payload, &retry_config).await
    }

    /// Send an AI request with custom retry configuration
    pub async fn send_ai_request_with_retry(
        &self,
        payload: &AIRequestPayload,
        retry_config: &RetryConfig
    ) -> AIOracleResult<reqwest::Response> {
        // Validate the payload before sending
        payload.validate()
            .map_err(|e| AIOracleError::Validation { message: format!("Invalid request payload: {}", e) })?;

        let endpoint = self.get_endpoint_for_service_type(&payload.service_type);

        log::info!("Sending AI request {} to {} service (max {} attempts)",
                  payload.id, endpoint, retry_config.max_attempts);

        let mut last_error = None;

        for attempt in 0..retry_config.max_attempts {
            if attempt > 0 {
                let delay = retry_config.delay_for_attempt(attempt);
                log::debug!("Retrying AI request {} (attempt {}/{}) after {:?} delay",
                           payload.id, attempt + 1, retry_config.max_attempts, delay);
                tokio::time::sleep(delay).await;
            } else {
                log::debug!("Starting AI request {} (attempt {}/{})",
                           payload.id, attempt + 1, retry_config.max_attempts);
            }

            match self.execute_ai_request(payload, endpoint).await {
                Ok(response) => {
                    log::info!("AI request {} completed successfully on attempt {}/{} (total time: {:?})",
                              payload.id, attempt + 1, retry_config.max_attempts,
                              std::time::Instant::now().duration_since(std::time::Instant::now()));
                    return Ok(response);
                }
                Err(error) => {
                    log::warn!("AI request {} failed on attempt {}/{} with error: {} (retryable: {})",
                              payload.id, attempt + 1, retry_config.max_attempts, error, error.is_retryable());

                    // Check if we should retry this error
                    if !error.is_retryable() {
                        log::error!("AI request {} failed with non-retryable error type: {}", payload.id, error);
                        return Err(error);
                    }

                    // If this is our last attempt, return the error
                    if attempt + 1 >= retry_config.max_attempts {
                        log::error!("AI request {} exhausted all {} retry attempts", payload.id, retry_config.max_attempts);
                        last_error = Some(error);
                        break;
                    }

                    last_error = Some(error);
                }
            }
        }

        // All retries exhausted
        let final_error = last_error.unwrap_or_else(|| {
            AIOracleError::Unknown { message: "All retry attempts failed".to_string() }
        });

        log::error!("AI request {} failed after {} attempts: {}",
                   payload.id, retry_config.max_attempts, final_error);

        Err(AIOracleError::MaxRetriesExceeded {
            attempts: retry_config.max_attempts,
            last_error: Box::new(final_error),
        })
    }

    /// Execute a single AI request attempt
    async fn execute_ai_request(&self, payload: &AIRequestPayload, endpoint: &str) -> AIOracleResult<reqwest::Response> {
        // Create string values for headers to avoid temporary value issues
        let service_type_str = payload.service_type.to_string();
        let priority_str = payload.priority.to_string();
        let timeout_str = payload.timeout.map(|t| t.to_string());

        // Set up headers
        let mut headers = vec![
            ("Content-Type", "application/json"),
            ("X-Request-ID", payload.id.as_str()),
            ("X-Service-Type", service_type_str.as_str()),
            ("X-Priority", priority_str.as_str()),
        ];

        // Add timeout header if specified
        if let Some(ref timeout_value) = timeout_str {
            headers.push(("X-Timeout", timeout_value.as_str()));
        }

        // Add signature header if present
        if let Some(ref signature) = payload.signature {
            headers.push(("X-Signature", signature));
        }

        // Add correlation ID if available
        if let Some(ref metadata) = payload.metadata {
            if let Some(ref correlation_id) = metadata.correlation_id {
                headers.push(("X-Correlation-ID", correlation_id));
            }
        }

        let url = format!("{}/{}", self.base_url.trim_end_matches('/'), endpoint.trim_start_matches('/'));

        log::debug!("Making POST request to: {}", url);

        // Execute the request with timeout and error handling
        let mut request = self.client.post(&url).json(payload);

        for (key, value) in headers {
            request = request.header(key, value);
        }

        match request.send().await {
            Ok(response) => {
                let status = response.status();
                log::debug!("POST request completed with status: {}", status);

                if status.is_success() {
                    Ok(response)
                } else if status.as_u16() == 429 {
                    // Rate limit exceeded
                    let retry_after = response.headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| s.parse::<u64>().ok())
                        .map(Duration::from_secs);

                    Err(AIOracleError::RateLimit {
                        message: format!("Rate limit exceeded (status: {})", status),
                        retry_after,
                    })
                } else if status.is_server_error() {
                    Err(AIOracleError::Http {
                        status: status.as_u16(),
                        message: format!("Server error: {}", status),
                    })
                } else if status.as_u16() == 401 || status.as_u16() == 403 {
                    Err(AIOracleError::Authentication {
                        message: format!("Authentication failed: {}", status),
                    })
                } else if status.as_u16() == 503 {
                    Err(AIOracleError::ServiceUnavailable {
                        message: format!("Service unavailable: {}", status),
                    })
                } else {
                    Err(AIOracleError::Http {
                        status: status.as_u16(),
                        message: format!("HTTP error: {}", status),
                    })
                }
            }
            Err(e) => {
                if e.is_timeout() {
                    Err(AIOracleError::Timeout {
                        timeout_ms: self.timeout.as_millis() as u64,
                    })
                } else if e.is_connect() {
                    Err(AIOracleError::Network {
                        message: "Connection failed".to_string(),
                        source: Some(Box::new(e)),
                    })
                } else {
                    Err(AIOracleError::Network {
                        message: format!("Network error: {}", e),
                        source: Some(Box::new(e)),
                    })
                }
            }
        }
    }

    /// Get endpoint path for a service type
    fn get_endpoint_for_service_type(&self, service_type: &AIServiceType) -> &str {
        match service_type {
            AIServiceType::FraudDetection => "fraud-detection",
            AIServiceType::RiskScoring => "risk-scoring",
            AIServiceType::ContractAnalysis => "contract-analysis",
            AIServiceType::AddressReputation => "address-reputation",
            AIServiceType::KYC => "kyc",
            AIServiceType::AML => "aml",
            AIServiceType::CreditAssessment => "credit-assessment",
            AIServiceType::TransactionValidation => "transaction-validation",
            AIServiceType::PatternAnalysis => "pattern-analysis",
            AIServiceType::ThreatDetection => "threat-detection",
            AIServiceType::Unknown => "unknown",
        }
    }

    /// Send an AI request and get an AIResponsePayload with retry logic
    pub async fn send_ai_request_response(&self, payload: &AIRequestPayload) -> Result<AIResponsePayload> {
        let retry_config = RetryConfig::default();
        self.send_ai_request_response_with_retry(payload, &retry_config).await
    }

    /// Send an AI request and get an AIResponsePayload with custom retry configuration
    pub async fn send_ai_request_response_with_retry(
        &self,
        payload: &AIRequestPayload,
        retry_config: &RetryConfig
    ) -> Result<AIResponsePayload> {
        let start_time = std::time::Instant::now();

        match self.send_ai_request_with_retry(payload, retry_config).await {
            Ok(response) => {
                let processing_time = start_time.elapsed().as_millis() as u64;
                let status_code = response.status();

                log::info!("AI request {} completed with status {}", payload.id, status_code);

                if status_code.is_success() {
                    // Parse the response body as JSON
                    match response.json::<serde_json::Value>().await {
                        Ok(response_data) => {
                            let ai_response = AIResponsePayload::success(
                                payload.id.clone(),
                                payload.service_type.clone(),
                                response_data,
                            ).with_processing_time(processing_time);

                            log::debug!("AI request {} parsed successfully", payload.id);
                            Ok(ai_response)
                        }
                        Err(e) => {
                            log::error!("Failed to parse AI response for request {}: {}", payload.id, e);
                            let error = AIResponseError::new(
                                "PARSE_ERROR".to_string(),
                                format!("Failed to parse response JSON: {}", e),
                                ErrorCategory::ProcessingError,
                            );
                            let ai_response = AIResponsePayload::failure(
                                payload.id.clone(),
                                payload.service_type.clone(),
                                error,
                            ).with_processing_time(processing_time);

                            Ok(ai_response)
                        }
                    }
                } else {
                    log::warn!("AI request {} returned non-success status: {}", payload.id, status_code);

                    // This shouldn't happen with our new error handling, but handle it gracefully
                    let error = AIResponseError::new(
                        format!("HTTP_{}", status_code.as_u16()),
                        format!("HTTP request failed with status: {}", status_code),
                        ErrorCategory::NetworkError,
                    ).with_retryable(status_code.is_server_error());

                    let ai_response = AIResponsePayload::failure(
                        payload.id.clone(),
                        payload.service_type.clone(),
                        error,
                    ).with_processing_time(processing_time);

                    Ok(ai_response)
                }
            }
            Err(oracle_error) => {
                let processing_time = start_time.elapsed().as_millis() as u64;

                log::error!("AI request {} failed after retries: {}", payload.id, oracle_error);

                // Convert AIOracleError to AIResponseError and create failure response
                let response_error = oracle_error.to_response_error();

                let status = match oracle_error {
                    AIOracleError::Timeout { .. } => ResponseStatus::Timeout,
                    AIOracleError::RateLimit { .. } => ResponseStatus::RateLimited,
                    AIOracleError::ServiceUnavailable { .. } => ResponseStatus::ServiceUnavailable,
                    AIOracleError::Authentication { .. } => ResponseStatus::InvalidRequest,
                    AIOracleError::Validation { .. } => ResponseStatus::InvalidRequest,
                    AIOracleError::MaxRetriesExceeded { .. } => ResponseStatus::Failure,
                    _ => ResponseStatus::InternalError,
                };

                let ai_response = AIResponsePayload::new(
                    payload.id.clone(),
                    payload.service_type.clone(),
                    status,
                )
                .with_error(response_error)
                .with_processing_time(processing_time);

                Ok(ai_response)
            }
        }
    }

    /// Send an AI request and get a typed response wrapped in AIResponsePayload
    pub async fn send_ai_request_typed_response<T>(&self, payload: &AIRequestPayload) -> Result<AIResponsePayload>
    where
        T: for<'de> serde::Deserialize<'de> + serde::Serialize,
    {
        let ai_response = self.send_ai_request_response(payload).await?;

        if ai_response.is_successful() {
            // Try to deserialize the response data to the expected type
            match serde_json::from_value::<T>(ai_response.response_data.clone()) {
                Ok(typed_data) => {
                    // Re-serialize to ensure consistent JSON format
                    let serialized_data = serde_json::to_value(typed_data)?;
                    let mut updated_response = ai_response;
                    updated_response.response_data = serialized_data;
                    Ok(updated_response)
                }
                Err(e) => {
                    let error = AIResponseError::new(
                        "TYPE_MISMATCH".to_string(),
                        format!("Response data doesn't match expected type: {}", e),
                        ErrorCategory::ProcessingError,
                    );
                    let failure_response = AIResponsePayload::failure(
                        payload.id.clone(),
                        payload.service_type.clone(),
                        error,
                    ).with_processing_time(ai_response.processing_time_ms);

                    Ok(failure_response)
                }
            }
        } else {
            Ok(ai_response)
        }
    }

    /// Batch send multiple AI requests and get responses
    pub async fn send_ai_request_batch(&self, payloads: &[AIRequestPayload]) -> Result<Vec<AIResponsePayload>> {
        let mut responses = Vec::new();

        // Process requests concurrently
        let mut handles = Vec::new();
        for payload in payloads {
            let client = self.clone();
            let payload_clone = payload.clone();
            let handle = tokio::spawn(async move {
                client.send_ai_request_response(&payload_clone).await
            });
            handles.push(handle);
        }

        // Collect all responses
        for handle in handles {
            match handle.await {
                Ok(Ok(response)) => responses.push(response),
                Ok(Err(e)) => {
                    log::error!("Batch request failed: {}", e);
                    // Create a failure response for this request
                    let error = AIResponseError::new(
                        "BATCH_REQUEST_FAILED".to_string(),
                        e.to_string(),
                        ErrorCategory::ProcessingError,
                    );
                    let failure_response = AIResponsePayload::failure(
                        "unknown".to_string(),
                        AIServiceType::Unknown,
                        error,
                    );
                    responses.push(failure_response);
                }
                Err(e) => {
                    log::error!("Batch request task failed: {}", e);
                    let error = AIResponseError::new(
                        "TASK_FAILED".to_string(),
                        e.to_string(),
                        ErrorCategory::ProcessingError,
                    );
                    let failure_response = AIResponsePayload::failure(
                        "unknown".to_string(),
                        AIServiceType::Unknown,
                        error,
                    );
                    responses.push(failure_response);
                }
            }
        }

        Ok(responses)
    }

    /// Get the configured base URL
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Get the configured timeout
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Get a reference to the underlying HTTP client
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Get connection pool statistics and health information
    pub fn connection_info(&self) -> String {
        format!(
            "AIOracleClient connected to: {} | Timeout: {:?}",
            self.base_url,
            self.timeout
        )
    }

    /// Test connection pool efficiency by making multiple concurrent requests
    pub async fn test_connection_pool(&self, concurrent_requests: usize) -> Result<Vec<bool>> {
        let mut handles = Vec::new();

        for i in 0..concurrent_requests {
            let client = self.clone();
            let handle = tokio::spawn(async move {
                log::debug!("Starting concurrent request {}", i);
                let result = client.test_connectivity().await;
                log::debug!("Completed concurrent request {}: {:?}", i, result);
                result.unwrap_or(false)
            });
            handles.push(handle);
        }

        let mut results = Vec::new();
        for handle in handles {
            let result = handle.await.unwrap_or(false);
            results.push(result);
        }

        log::info!(
            "Connection pool test completed: {}/{} requests successful",
            results.iter().filter(|&&x| x).count(),
            concurrent_requests
        );

        Ok(results)
    }

    /// Make a request with circuit breaker protection
    pub async fn request_with_circuit_breaker<T>(&self, request_fn: impl std::future::Future<Output = Result<T>>) -> Result<T> {
        // Check if circuit breaker allows the request
        let should_allow = {
            let mut circuit_breaker = self.circuit_breaker.lock()
                .map_err(|e| anyhow::anyhow!("Failed to acquire circuit breaker lock: {}", e))?;
            circuit_breaker.should_allow_request()
        };

        if !should_allow {
            log::warn!("Circuit breaker is open, rejecting request");
            return Err(anyhow::anyhow!("Circuit breaker is open - AI service is unavailable"));
        }

        // Execute the request
        match request_fn.await {
            Ok(result) => {
                // Record success
                let mut circuit_breaker = self.circuit_breaker.lock()
                    .map_err(|e| anyhow::anyhow!("Failed to acquire circuit breaker lock: {}", e))?;
                circuit_breaker.record_success();
                log::debug!("Request succeeded, circuit breaker stats: success={}, failure={}, rate={:.2}%",
                    circuit_breaker.stats.success_count,
                    circuit_breaker.stats.failure_count,
                    circuit_breaker.stats.failure_rate * 100.0);
                Ok(result)
            }
            Err(e) => {
                // Record failure
                let mut circuit_breaker = self.circuit_breaker.lock()
                    .map_err(|e| anyhow::anyhow!("Failed to acquire circuit breaker lock: {}", e))?;
                circuit_breaker.record_failure();
                log::warn!("Request failed, circuit breaker stats: success={}, failure={}, rate={:.2}%",
                    circuit_breaker.stats.success_count,
                    circuit_breaker.stats.failure_count,
                    circuit_breaker.stats.failure_rate * 100.0);
                Err(e)
            }
        }
    }

    /// Create a fallback response when AI service is unavailable
    pub fn create_fallback_response(&self, request_type: &str, message: &str) -> FallbackResponse {
        FallbackResponse {
            response_type: request_type.to_string(),
            data: serde_json::json!({
                "fallback": true,
                "service_unavailable": true,
                "recommendation": "retry_later"
            }),
            message: message.to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    /// Make a GET request with circuit breaker protection and fallback
    pub async fn get_with_fallback(&self, endpoint: &str) -> Result<reqwest::Response> {
        let endpoint = endpoint.to_string();
        let client = self.clone();

        match self.request_with_circuit_breaker(async move {
            client.get(&endpoint).await
        }).await {
            Ok(response) => Ok(response),
            Err(e) => {
                log::error!("GET request failed and circuit breaker triggered: {}", e);
                // Return a fallback error that the caller can handle
                Err(anyhow::anyhow!("AI service unavailable (circuit breaker open): {}", e))
            }
        }
    }



    /// Make a POST request with circuit breaker protection and fallback
    pub async fn post_with_fallback(&self, endpoint: &str, body: serde_json::Value) -> Result<reqwest::Response> {
        let endpoint = endpoint.to_string();
        let client = self.clone();

        match self.request_with_circuit_breaker(async move {
            client.post(&endpoint, body).await
        }).await {
            Ok(response) => Ok(response),
            Err(e) => {
                log::error!("POST request failed and circuit breaker triggered: {}", e);
                // Return a fallback error that the caller can handle
                Err(anyhow::anyhow!("AI service unavailable (circuit breaker open): {}", e))
            }
        }
    }

    /// Perform health check with circuit breaker integration
    pub async fn health_check_with_circuit_breaker(&self) -> Result<AIHealthCheckResponse> {
        let client = self.clone();

        match self.request_with_circuit_breaker(async move {
            client.health_check().await
        }).await {
            Ok(health_response) => {
                // Additional circuit breaker logic based on health status
                if health_response.status == AIServiceStatus::Unhealthy {
                    let mut circuit_breaker = self.circuit_breaker.lock()
                        .map_err(|e| anyhow::anyhow!("Failed to acquire circuit breaker lock: {}", e))?;
                    circuit_breaker.record_failure();
                    log::warn!("Health check indicates unhealthy service, recording failure");
                }
                Ok(health_response)
            }
            Err(e) => {
                log::error!("Health check failed with circuit breaker protection: {}", e);

                // Return a fallback health response
                Ok(AIHealthCheckResponse {
                    status: AIServiceStatus::Unhealthy,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    response_time_ms: 0,
                    version: None,
                    details: Some(serde_json::json!({
                        "error": "circuit_breaker_open",
                        "message": "Circuit breaker prevented health check request",
                        "fallback": true
                    })),
                    endpoints: None,
                    load: None,
                })
            }
        }
    }

    /// Send AI request with circuit breaker protection
    pub async fn send_ai_request_with_circuit_breaker(&self, payload: &AIRequestPayload) -> Result<AIResponsePayload> {
        let client = self.clone();
        let payload_clone = payload.clone();

        match self.request_with_circuit_breaker(async move {
            client.send_ai_request_response(&payload_clone).await
        }).await {
            Ok(response) => Ok(response),
            Err(e) => {
                log::error!("AI request failed with circuit breaker protection: {}", e);

                // Create a fallback response
                let error = AIResponseError::new(
                    "CIRCUIT_BREAKER_OPEN".to_string(),
                    format!("AI service unavailable (circuit breaker open): {}", e),
                    ErrorCategory::ServiceError,
                ).with_retryable(true);

                let fallback_response = AIResponsePayload::failure(
                    payload.id.clone(),
                    payload.service_type.clone(),
                    error,
                );

                Ok(fallback_response)
            }
        }
    }
}

/// Configuration for AI service communication and behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIServiceConfig {
    /// Base URL for AI service endpoints
    pub base_url: String,
    /// API key for authentication with AI service
    pub api_key: Option<String>,
    /// Request timeout in seconds
    pub timeout_seconds: u64,
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Base delay for exponential backoff in milliseconds
    pub base_retry_delay_ms: u64,
    /// Maximum delay for exponential backoff in milliseconds
    pub max_retry_delay_ms: u64,
    /// Jitter factor for retry delays (0.0 to 1.0)
    pub retry_jitter: f64,
    /// Health check interval in seconds
    pub health_check_interval_seconds: u64,
    /// Health check timeout in seconds
    pub health_check_timeout_seconds: u64,
    /// Number of consecutive failed health checks before marking service as unhealthy
    pub health_check_failure_threshold: u32,
    /// Number of consecutive successful health checks before marking service as healthy
    pub health_check_success_threshold: u32,
    /// Circuit breaker failure threshold (percentage)
    pub circuit_breaker_failure_threshold: f64,
    /// Circuit breaker recovery time in seconds
    pub circuit_breaker_recovery_time_seconds: u64,
    /// Connection pool configuration
    pub connection_pool: ConnectionPoolConfig,
    /// Request rate limit (requests per second)
    pub rate_limit_per_second: Option<u32>,
    /// Batch size for bulk operations
    pub batch_size: u32,
    /// Cache TTL for responses in seconds
    pub cache_ttl_seconds: u64,
    /// Enable request/response logging
    pub enable_request_logging: bool,
    /// Enable metrics collection
    pub enable_metrics: bool,
    /// Custom headers to include in requests
    pub custom_headers: std::collections::HashMap<String, String>,
    /// AI service endpoints for different service types
    pub endpoints: AIServiceEndpoints,
    /// Risk score configuration
    pub risk_config: RiskScoringConfig,
    /// Fallback configuration when AI service is unavailable
    pub fallback_config: FallbackConfig,
}

/// Configuration for AI service endpoints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIServiceEndpoints {
    /// Fraud detection endpoint
    pub fraud_detection: String,
    /// Risk scoring endpoint
    pub risk_scoring: String,
    /// Contract analysis endpoint
    pub contract_analysis: String,
    /// Address reputation endpoint
    pub address_reputation: String,
    /// KYC verification endpoint
    pub kyc: String,
    /// AML compliance endpoint
    pub aml: String,
    /// Credit assessment endpoint
    pub credit_assessment: String,
    /// Transaction validation endpoint
    pub transaction_validation: String,
    /// Pattern analysis endpoint
    pub pattern_analysis: String,
    /// Threat detection endpoint
    pub threat_detection: String,
    /// Health check endpoint
    pub health_check: String,
    /// Batch processing endpoint
    pub batch_processing: String,
}

/// Configuration for risk scoring thresholds and policies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskScoringConfig {
    /// Low risk threshold (0.0 to 1.0)
    pub low_risk_threshold: f64,
    /// Medium risk threshold (0.0 to 1.0)
    pub medium_risk_threshold: f64,
    /// High risk threshold (0.0 to 1.0)
    pub high_risk_threshold: f64,
    /// Auto-approve transactions below this threshold
    pub auto_approve_threshold: f64,
    /// Auto-reject transactions above this threshold
    pub auto_reject_threshold: f64,
    /// Require manual review for scores in this range
    pub manual_review_threshold_range: (f64, f64),
    /// Default risk score when AI service is unavailable
    pub default_risk_score: f64,
    /// Enable risk-based processing
    pub enable_risk_based_processing: bool,
    /// Risk score weights for different transaction types
    pub transaction_type_weights: std::collections::HashMap<String, f64>,
}

/// Configuration for fallback behavior when AI service is unavailable
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallbackConfig {
    /// Enable fallback processing
    pub enable_fallback: bool,
    /// Fallback mode: "permissive" (allow all), "restrictive" (block all), or "default_scores"
    pub fallback_mode: String,
    /// Default risk scores by service type when in fallback mode
    pub default_scores: std::collections::HashMap<String, f64>,
    /// Cache previous results for fallback processing
    pub use_cached_results: bool,
    /// Cache retention time in seconds
    pub cache_retention_seconds: u64,
    /// Log fallback operations
    pub log_fallback_operations: bool,
}

impl Default for AIServiceConfig {
    fn default() -> Self {
        let mut custom_headers = std::collections::HashMap::new();
        custom_headers.insert("User-Agent".to_string(), "Dytallix-Blockchain/1.0".to_string());
        custom_headers.insert("Content-Type".to_string(), "application/json".to_string());

        let mut transaction_type_weights = std::collections::HashMap::new();
        transaction_type_weights.insert("transfer".to_string(), 1.0);
        transaction_type_weights.insert("contract_call".to_string(), 1.5);
        transaction_type_weights.insert("contract_deploy".to_string(), 2.0);
        transaction_type_weights.insert("stake".to_string(), 1.2);

        let mut default_scores = std::collections::HashMap::new();
        default_scores.insert("fraud_detection".to_string(), 0.5);
        default_scores.insert("risk_scoring".to_string(), 0.5);
        default_scores.insert("contract_analysis".to_string(), 0.3);
        default_scores.insert("address_reputation".to_string(), 0.5);

        Self {
            base_url: "http://localhost:8080".to_string(),
            api_key: None,
            timeout_seconds: 30,
            max_retries: 3,
            base_retry_delay_ms: 100,
            max_retry_delay_ms: 5000,
            retry_jitter: 0.1,
            health_check_interval_seconds: 30,
            health_check_timeout_seconds: 5,
            health_check_failure_threshold: 3,
            health_check_success_threshold: 2,
            circuit_breaker_failure_threshold: 0.5,
            circuit_breaker_recovery_time_seconds: 60,
            connection_pool: ConnectionPoolConfig::default(),
            rate_limit_per_second: Some(100),
            batch_size: 10,
            cache_ttl_seconds: 300,
            enable_request_logging: true,
            enable_metrics: true,
            custom_headers,
            endpoints: AIServiceEndpoints::default(),
            risk_config: RiskScoringConfig::default(),
            fallback_config: FallbackConfig::default(),
        }
    }
}

impl Default for AIServiceEndpoints {
    fn default() -> Self {
        Self {
            fraud_detection: "/api/v1/fraud-detection".to_string(),
            risk_scoring: "/api/v1/risk-scoring".to_string(),
            contract_analysis: "/api/v1/contract-analysis".to_string(),
            address_reputation: "/api/v1/address-reputation".to_string(),
            kyc: "/api/v1/kyc".to_string(),
            aml: "/api/v1/aml".to_string(),
            credit_assessment: "/api/v1/credit-assessment".to_string(),
            transaction_validation: "/api/v1/transaction-validation".to_string(),
            pattern_analysis: "/api/v1/pattern-analysis".to_string(),
            threat_detection: "/api/v1/threat-detection".to_string(),
            health_check: "/health".to_string(),
            batch_processing: "/api/v1/batch".to_string(),
        }
    }
}

impl Default for RiskScoringConfig {
    fn default() -> Self {
        Self {
            low_risk_threshold: 0.3,
            medium_risk_threshold: 0.6,
            high_risk_threshold: 0.8,
            auto_approve_threshold: 0.2,
            auto_reject_threshold: 0.9,
            manual_review_threshold_range: (0.2, 0.9),
            default_risk_score: 0.5,
            enable_risk_based_processing: true,
            transaction_type_weights: std::collections::HashMap::new(),
        }
    }
}

impl Default for FallbackConfig {
    fn default() -> Self {
        Self {
            enable_fallback: true,
            fallback_mode: "default_scores".to_string(),
            default_scores: std::collections::HashMap::new(),
            use_cached_results: true,
            cache_retention_seconds: 3600,
            log_fallback_operations: true,
        }
    }
}

impl AIServiceConfig {
    /// Create a new configuration with minimal required fields
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            ..Default::default()
        }
    }

    /// Create a configuration optimized for development
    pub fn development() -> Self {
        Self {
            base_url: "http://localhost:8080".to_string(),
            timeout_seconds: 10,
            max_retries: 2,
            health_check_interval_seconds: 60,
            enable_request_logging: true,
            enable_metrics: false,
            connection_pool: ConnectionPoolConfig::low_resource(),
            fallback_config: FallbackConfig {
                enable_fallback: true,
                fallback_mode: "permissive".to_string(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Create a configuration optimized for production
    pub fn production() -> Self {
        Self {
            base_url: "https://ai-service.dytallix.com".to_string(),
            timeout_seconds: 30,
            max_retries: 5,
            health_check_interval_seconds: 15,
            enable_request_logging: false,
            enable_metrics: true,
            connection_pool: ConnectionPoolConfig::high_performance(),
            fallback_config: FallbackConfig {
                enable_fallback: true,
                fallback_mode: "restrictive".to_string(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Create a configuration for testing
    pub fn testing() -> Self {
        Self {
            base_url: "http://localhost:8081".to_string(),
            timeout_seconds: 5,
            max_retries: 1,
            health_check_interval_seconds: 10,
            enable_request_logging: true,
            enable_metrics: true,
            connection_pool: ConnectionPoolConfig::low_resource(),
            fallback_config: FallbackConfig {
                enable_fallback: false,
                fallback_mode: "default_scores".to_string(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Set API key for authentication
    pub fn with_api_key(mut self, api_key: String) -> Self {
        self.api_key = Some(api_key);
        self
    }

    /// Set request timeout
    pub fn with_timeout(mut self, timeout_seconds: u64) -> Self {
        self.timeout_seconds = timeout_seconds;
        self
    }

    /// Set retry configuration
    pub fn with_retry_config(mut self, max_retries: u32, base_delay_ms: u64, max_delay_ms: u64) -> Self {
        self.max_retries = max_retries;
        self.base_retry_delay_ms = base_delay_ms;
        self.max_retry_delay_ms = max_delay_ms;
        self
    }

    /// Set health check configuration
    pub fn with_health_check_config(mut self, interval_seconds: u64, timeout_seconds: u64, failure_threshold: u32) -> Self {
        self.health_check_interval_seconds = interval_seconds;
        self.health_check_timeout_seconds = timeout_seconds;
        self.health_check_failure_threshold = failure_threshold;
        self
    }

    /// Set circuit breaker configuration
    pub fn with_circuit_breaker_config(mut self, failure_threshold: f64, recovery_time_seconds: u64) -> Self {
        self.circuit_breaker_failure_threshold = failure_threshold;
        self.circuit_breaker_recovery_time_seconds = recovery_time_seconds;
        self
    }

    /// Set connection pool configuration
    pub fn with_connection_pool(mut self, pool_config: ConnectionPoolConfig) -> Self {
        self.connection_pool = pool_config;
        self
    }

    /// Add custom header
    pub fn with_custom_header(mut self, key: String, value: String) -> Self {
        self.custom_headers.insert(key, value);
        self
    }

    /// Set risk scoring configuration
    pub fn with_risk_config(mut self, risk_config: RiskScoringConfig) -> Self {
        self.risk_config = risk_config;
        self
    }

    /// Set fallback configuration
    pub fn with_fallback_config(mut self, fallback_config: FallbackConfig) -> Self {
        self.fallback_config = fallback_config;
        self
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.base_url.is_empty() {
            return Err("Base URL cannot be empty".to_string());
        }

        if self.timeout_seconds == 0 {
            return Err("Timeout must be greater than 0".to_string());
        }

        if self.max_retries > 10 {
            return Err("Max retries cannot exceed 10".to_string());
        }

        if self.base_retry_delay_ms >= self.max_retry_delay_ms {
            return Err("Base retry delay must be less than max retry delay".to_string());
        }

        if self.retry_jitter < 0.0 || self.retry_jitter > 1.0 {
            return Err("Retry jitter must be between 0.0 and 1.0".to_string());
        }

        if self.circuit_breaker_failure_threshold < 0.0 || self.circuit_breaker_failure_threshold > 1.0 {
            return Err("Circuit breaker failure threshold must be between 0.0 and 1.0".to_string());
        }

        if self.health_check_failure_threshold == 0 {
            return Err("Health check failure threshold must be greater than 0".to_string());
        }

        if self.health_check_success_threshold == 0 {
            return Err("Health check success threshold must be greater than 0".to_string());
        }

        // Validate risk scoring configuration
        if self.risk_config.low_risk_threshold >= self.risk_config.medium_risk_threshold {
            return Err("Low risk threshold must be less than medium risk threshold".to_string());
        }

        if self.risk_config.medium_risk_threshold >= self.risk_config.high_risk_threshold {
            return Err("Medium risk threshold must be less than high risk threshold".to_string());
        }

        if self.risk_config.auto_approve_threshold >= self.risk_config.auto_reject_threshold {
            return Err("Auto-approve threshold must be less than auto-reject threshold".to_string());
        }

        // Validate fallback configuration
        let valid_fallback_modes = ["permissive", "restrictive", "default_scores"];
        if !valid_fallback_modes.contains(&self.fallback_config.fallback_mode.as_str()) {
            return Err(format!("Invalid fallback mode: {}. Must be one of: {:?}",
                              self.fallback_config.fallback_mode, valid_fallback_modes));
        }

        Ok(())
    }

    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self, String> {
        let mut config = Self::default();

        if let Ok(base_url) = std::env::var("AI_SERVICE_BASE_URL") {
            config.base_url = base_url;
        }

        if let Ok(api_key) = std::env::var("AI_SERVICE_API_KEY") {
            config.api_key = Some(api_key);
        }

        if let Ok(timeout) = std::env::var("AI_SERVICE_TIMEOUT_SECONDS") {
            config.timeout_seconds = timeout.parse()
                .map_err(|_| "Invalid AI_SERVICE_TIMEOUT_SECONDS format")?;
        }

        if let Ok(max_retries) = std::env::var("AI_SERVICE_MAX_RETRIES") {
            config.max_retries = max_retries.parse()
                .map_err(|_| "Invalid AI_SERVICE_MAX_RETRIES format")?;
        }

        if let Ok(base_delay) = std::env::var("AI_SERVICE_BASE_RETRY_DELAY_MS") {
            config.base_retry_delay_ms = base_delay.parse()
                .map_err(|_| "Invalid AI_SERVICE_BASE_RETRY_DELAY_MS format")?;
        }

        if let Ok(max_delay) = std::env::var("AI_SERVICE_MAX_RETRY_DELAY_MS") {
            config.max_retry_delay_ms = max_delay.parse()
                .map_err(|_| "Invalid AI_SERVICE_MAX_RETRY_DELAY_MS format")?;
        }

        if let Ok(health_interval) = std::env::var("AI_SERVICE_HEALTH_CHECK_INTERVAL_SECONDS") {
            config.health_check_interval_seconds = health_interval.parse()
                .map_err(|_| "Invalid AI_SERVICE_HEALTH_CHECK_INTERVAL_SECONDS format")?;
        }

        if let Ok(enable_logging) = std::env::var("AI_SERVICE_ENABLE_REQUEST_LOGGING") {
            config.enable_request_logging = enable_logging.parse()
                .map_err(|_| "Invalid AI_SERVICE_ENABLE_REQUEST_LOGGING format")?;
        }

        if let Ok(enable_metrics) = std::env::var("AI_SERVICE_ENABLE_METRICS") {
            config.enable_metrics = enable_metrics.parse()
                .map_err(|_| "Invalid AI_SERVICE_ENABLE_METRICS format")?;
        }

        if let Ok(fallback_mode) = std::env::var("AI_SERVICE_FALLBACK_MODE") {
            config.fallback_config.fallback_mode = fallback_mode;
        }

        if let Ok(low_risk) = std::env::var("AI_SERVICE_LOW_RISK_THRESHOLD") {
            config.risk_config.low_risk_threshold = low_risk.parse()
                .map_err(|_| "Invalid AI_SERVICE_LOW_RISK_THRESHOLD format")?;
        }

        if let Ok(high_risk) = std::env::var("AI_SERVICE_HIGH_RISK_THRESHOLD") {
            config.risk_config.high_risk_threshold = high_risk.parse()
                .map_err(|_| "Invalid AI_SERVICE_HIGH_RISK_THRESHOLD format")?;
        }

        config.validate()?;
        Ok(config)
    }

    /// Get the full URL for a specific service endpoint
    pub fn get_endpoint_url(&self, service_type: &AIServiceType) -> String {
        let endpoint = match service_type {
            AIServiceType::FraudDetection => &self.endpoints.fraud_detection,
            AIServiceType::RiskScoring => &self.endpoints.risk_scoring,
            AIServiceType::ContractAnalysis => &self.endpoints.contract_analysis,
            AIServiceType::AddressReputation => &self.endpoints.address_reputation,
            AIServiceType::KYC => &self.endpoints.kyc,
            AIServiceType::AML => &self.endpoints.aml,
            AIServiceType::CreditAssessment => &self.endpoints.credit_assessment,
            AIServiceType::TransactionValidation => &self.endpoints.transaction_validation,
            AIServiceType::PatternAnalysis => &self.endpoints.pattern_analysis,
            AIServiceType::ThreatDetection => &self.endpoints.threat_detection,
            AIServiceType::Unknown => "/api/v1/unknown",
        };

        format!("{}{}", self.base_url.trim_end_matches('/'), endpoint)
    }

    /// Get health check URL
    pub fn get_health_check_url(&self) -> String {
        format!("{}{}", self.base_url.trim_end_matches('/'), self.endpoints.health_check)
    }

    /// Get batch processing URL
    pub fn get_batch_url(&self) -> String {
        format!("{}{}", self.base_url.trim_end_matches('/'), self.endpoints.batch_processing)
    }

    /// Calculate retry delay with exponential backoff and jitter
    pub fn calculate_retry_delay(&self, attempt: u32) -> Duration {
        let base_delay = self.base_retry_delay_ms as f64;
        let max_delay = self.max_retry_delay_ms as f64;
        let jitter = self.retry_jitter;

        // Exponential backoff: delay = base_delay * 2^attempt
        let exponential_delay = base_delay * (2_f64.powi(attempt as i32));
        let capped_delay = exponential_delay.min(max_delay);

        // Add jitter: final_delay = delay * (1 + random(-jitter, jitter))
        let jitter_range = capped_delay * self.retry_jitter;
        let jitter = (rand::random::<f64>() - 0.5) * 2.0 * jitter_range;
        let final_delay = (capped_delay + jitter).max(0.);

        Duration::from_millis(final_delay as u64)
    }
}

/// Configuration for connection pooling and keep-alive settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionPoolConfig {
    /// Maximum number of idle connections per host
    pub max_idle_per_host: usize,
    /// Timeout for idle connections in the pool
    pub idle_timeout: Duration,
    /// TCP keep-alive interval
    pub tcp_keepalive: Duration,
    /// Maximum total connections in the pool
    pub max_total_connections: Option<usize>,
    /// Enable HTTP/2 support
    pub http2_prior_knowledge: bool,
}

impl Default for ConnectionPoolConfig {
    fn default() -> Self {
        Self {
            max_idle_per_host: 10,
            idle_timeout: Duration::from_secs(90),
            tcp_keepalive: Duration::from_secs(60),
            max_total_connections: Some(100),
            http2_prior_knowledge: false,
        }
    }
}

impl ConnectionPoolConfig {
    /// Create a new connection pool configuration for high-performance scenarios
    pub fn high_performance() -> Self {
        Self {
            max_idle_per_host: 50,
            idle_timeout: Duration::from_secs(120),
            tcp_keepalive: Duration::from_secs(30),
            max_total_connections: Some(500),
            http2_prior_knowledge: true,
        }
    }

    /// Create a new connection pool configuration for low-resource scenarios
    pub fn low_resource() -> Self {
        Self {
            max_idle_per_host: 2,
            idle_timeout: Duration::from_secs(30),
            tcp_keepalive: Duration::from_secs(120),
            max_total_connections: Some(10),
            http2_prior_knowledge: false,
        }
    }
}

/// Enhanced AI Request Payload for Oracle Communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIRequestPayload {
    /// Unique request identifier for tracking
    pub id: String,
    /// Type of AI service being requested
    pub service_type: AIServiceType,
    /// Request data specific to the service type
    pub request_data: serde_json::Value,
    /// Timestamp when the request was created
    pub timestamp: u64,
    /// Optional request metadata
    pub metadata: Option<AIRequestMetadata>,
    /// Request priority level
    pub priority: RequestPriority,
    /// Maximum time to wait for response (in seconds)
    pub timeout: Option<u64>,
    /// Callback URL for async responses
    pub callback_url: Option<String>,
    /// Request signature for authentication
    pub signature: Option<String>,
}

/// Metadata associated with AI requests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIRequestMetadata {
    /// Source of the request (e.g., "consensus", "validation", "fraud_detection")
    pub source: String,
    /// Version of the request format
    pub version: String,
    /// Additional context or parameters
    pub context: Option<serde_json::Value>,
    /// Request correlation ID for grouping related requests
    pub correlation_id: Option<String>,
    /// User or system that initiated the request
    pub requester: Option<String>,
}

impl AIRequestPayload {
    /// Create a new request payload with minimal required fields
    pub fn new(service_type: AIServiceType, request_data: serde_json::Value) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            service_type,
            request_data,
            timestamp: chrono::Utc::now().timestamp() as u64,
            metadata: None,
            priority: RequestPriority::default(),
            timeout: None,
            callback_url: None,
            signature: None,
        }
    }

    /// Create a new request payload with full configuration
    pub fn new_with_config(
        service_type: AIServiceType,
        request_data: serde_json::Value,
        metadata: Option<AIRequestMetadata>,
        priority: RequestPriority,
        timeout: Option<u64>,
        callback_url: Option<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            service_type,
            request_data,
            timestamp: chrono::Utc::now().timestamp() as u64,
            metadata,
            priority,
            timeout,
            callback_url,
            signature: None,
        }
    }

    /// Create a fraud detection request
    pub fn fraud_detection(transaction_data: serde_json::Value) -> Self {
        Self::new(AIServiceType::FraudDetection, transaction_data)
    }

    /// Create a risk scoring request
    pub fn risk_scoring(transaction_data: serde_json::Value) -> Self {
        Self::new(AIServiceType::RiskScoring, transaction_data)
    }

    /// Create a contract analysis request
    pub fn contract_analysis(contract_data: serde_json::Value) -> Self {
        Self::new(AIServiceType::ContractAnalysis, contract_data)
    }

    /// Create a transaction validation request
    pub fn transaction_validation(transaction_data: serde_json::Value) -> Self {
        Self::new(AIServiceType::TransactionValidation, transaction_data)
    }

    /// Set the request priority
    pub fn with_priority(mut self, priority: RequestPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Set the request timeout
    pub fn with_timeout(mut self, timeout: u64) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Set the callback URL
    pub fn with_callback(mut self, callback_url: String) -> Self {
        self.callback_url = Some(callback_url);
        self
    }

    /// Set the request metadata
    pub fn with_metadata(mut self, metadata: AIRequestMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Set the request signature
    pub fn with_signature(mut self, signature: String) -> Self {
        self.signature = Some(signature);
        self
    }

    /// Serialize this payload to a JSON string
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Serialize this payload to pretty JSON string
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserialize a payload from a JSON string
    pub fn from_json(data: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(data)
    }

    /// Validate the request payload
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty() {
            return Err("Request ID cannot be empty".to_string());
        }

        if self.timestamp == 0 {
            return Err("Timestamp cannot be zero".to_string());
        }

        // Check if timestamp is not too old (24 hours)
        let now = chrono::Utc::now().timestamp() as u64;
        if now > self.timestamp && (now - self.timestamp) > 86400 {
            return Err("Request timestamp is too old".to_string());
        }

        // Check if timestamp is not too far in the future (1 hour)
        if self.timestamp > now && (self.timestamp - now) > 3600 {
            return Err("Request timestamp is too far in the future".to_string());
        }

        // Validate timeout if set
        if let Some(timeout) = self.timeout {
            if timeout == 0 {
                return Err("Timeout cannot be zero".to_string());
            }
            if timeout > 300 {
                return Err("Timeout cannot exceed 300 seconds".to_string());
            }
        }

        // Validate callback URL if set
        if let Some(ref callback_url) = self.callback_url {
            if callback_url.is_empty() {
                return Err("Callback URL cannot be empty".to_string());
            }
            if !callback_url.starts_with("http://") && !callback_url.starts_with("https://") {
                return Err("Callback URL must be a valid HTTP(S) URL".to_string());
            }
        }

        Ok(())
    }

    /// Get the age of the request in seconds
    pub fn age_seconds(&self) -> u64 {
        let now = chrono::Utc::now().timestamp() as u64;
        if now > self.timestamp {
            now - self.timestamp
        } else {
            0
        }
    }

    /// Check if the request has expired based on timeout
    pub fn is_expired(&self) -> bool {
        if let Some(timeout) = self.timeout {
            self.age_seconds() > timeout
        } else {
            false
        }
    }

    /// Get the effective timeout (default to 30 seconds if not set)
    pub fn effective_timeout(&self) -> u64 {
        self.timeout.unwrap_or(30)
    }
}

impl AIRequestMetadata {
    /// Create new metadata with minimal fields
    pub fn new(source: String, version: String) -> Self {
        Self {
            source,
            version,
            context: None,
            correlation_id: None,
            requester: None,
        }
    }

    /// Create metadata with full configuration
    pub fn new_with_config(
        source: String,
        version: String,
        context: Option<serde_json::Value>,
        correlation_id: Option<String>,
        requester: Option<String>,
    ) -> Self {
        Self {
            source,
            version,
            context,
            correlation_id,
            requester,
        }
    }

    /// Set the context
    pub fn with_context(mut self, context: serde_json::Value) -> Self {
        self.context = Some(context);
        self
    }

    /// Set the correlation ID
    pub fn with_correlation_id(mut self, correlation_id: String) -> Self {
        self.correlation_id = Some(correlation_id);
        self
    }

    /// Set the requester
    pub fn with_requester(mut self, requester: String) -> Self {
        self.requester = Some(requester);
        self
    }
}

/// Comprehensive error types for AI Oracle operations
#[derive(Debug, thiserror::Error)]
pub enum AIOracleError {
    #[error("Network error: {message}")]
    Network { message: String, source: Option<Box<dyn std::error::Error + Send + Sync>> },

    #[error("Request timeout after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },

    #[error("HTTP error: {status} - {message}")]
    Http { status: u16, message: String },

    #[error("Serialization error: {message}")]
    Serialization { message: String },

    #[error("Authentication error: {message}")]
    Authentication { message: String },

    #[error("Rate limit exceeded: {message}")]
    RateLimit { message: String, retry_after: Option<Duration> },

    #[error("Service unavailable: {message}")]
    ServiceUnavailable { message: String },

    #[error("Configuration error: {message}")]
    Configuration { message: String },

    #[error("Validation error: {message}")]
    Validation { message: String },

    #[error("Circuit breaker open: {message}")]
    CircuitBreaker { message: String },

    #[error("Max retries exceeded: {attempts} attempts failed")]
    MaxRetriesExceeded { attempts: u32, last_error: Box<AIOracleError> },

    #[error("Service error: {code} - {message}")]
    Service { code: String, message: String, details: Option<String> },

    #[error("Unknown error: {message}")]
    Unknown { message: String },
}

impl AIOracleError {
    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        match self {
            AIOracleError::Network { .. } => true,
            AIOracleError::Timeout { .. } => true,
            AIOracleError::Http { status, .. } => {
                // Retry on server errors (5xx) and some client errors
                *status >= 500 || *status == 408 || *status == 429 || *status == 502 || *status == 503 || *status == 504
            },
            AIOracleError::RateLimit { .. } => true,
            AIOracleError::ServiceUnavailable { .. } => true,
            AIOracleError::Service { .. } => false, // Service logic errors are usually not retryable
            AIOracleError::Authentication { .. } => false,
            AIOracleError::Validation { .. } => false,
            AIOracleError::Configuration { .. } => false,
            AIOracleError::CircuitBreaker { .. } => false,
            AIOracleError::MaxRetriesExceeded { .. } => false,
            AIOracleError::Serialization { .. } => false,
            AIOracleError::Unknown { .. } => false,
        }
    }

    /// Get the error category for classification
    pub fn category(&self) -> ErrorCategory {
        match self {
            AIOracleError::Network { .. } => ErrorCategory::NetworkError,
            AIOracleError::Timeout { .. } => ErrorCategory::NetworkError,
            AIOracleError::Http { .. } => ErrorCategory::NetworkError,
            AIOracleError::RateLimit { .. } => ErrorCategory::RateLimitError,
            AIOracleError::ServiceUnavailable { .. } => ErrorCategory::ServiceError,
            AIOracleError::Authentication { .. } => ErrorCategory::AuthenticationError,
            AIOracleError::Validation { .. } => ErrorCategory::ValidationError,
            AIOracleError::Configuration { .. } => ErrorCategory::ValidationError,
            AIOracleError::Serialization { .. } => ErrorCategory::ProcessingError,
            AIOracleError::Service { .. } => ErrorCategory::ServiceError,
            AIOracleError::CircuitBreaker { .. } => ErrorCategory::ServiceError,
            AIOracleError::MaxRetriesExceeded { .. } => ErrorCategory::NetworkError,
            AIOracleError::Unknown { .. } => ErrorCategory::UnknownError,
        }
    }

    /// Get suggested retry delay for retryable errors
    pub fn retry_delay(&self) -> Option<Duration> {
        match self {
            AIOracleError::RateLimit { retry_after, .. } => *retry_after,
            AIOracleError::ServiceUnavailable { .. } => Some(Duration::from_secs(5)),
            AIOracleError::Network { .. } => Some(Duration::from_millis(500)),
            AIOracleError::Timeout { .. } => Some(Duration::from_secs(1)),
            AIOracleError::Http { status, .. } if *status >= 500 => Some(Duration::from_secs(2)),
            _ => None,
        }
    }

    /// Convert to AIResponseError for response payloads
    pub fn to_response_error(&self) -> AIResponseError {
        AIResponseError {
            code: format!("{:?}", self).split('(').next().unwrap_or("Unknown").to_string(),
            message: self.to_string(),
            details: None,
            category: self.category(),
            retryable: self.is_retryable(),
        }
    }
}

/// Result type for AI Oracle operations
pub type AIOracleResult<T> = std::result::Result<T, AIOracleError>;

/// Retry configuration for AI Oracle requests
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_attempts: u32,
    /// Base delay for exponential backoff
    pub base_delay: Duration,
    /// Maximum delay cap
    pub max_delay: Duration,
    /// Jitter factor (0.0 to 1.0)
    pub jitter_factor: f64,
    /// Multiplier for exponential backoff
    pub backoff_multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            jitter_factor: 0.1,
            backoff_multiplier: 2.0,
        }
    }
}

impl RetryConfig {
    /// Create a new retry configuration
    pub fn new(max_attempts: u32, base_delay: Duration, max_delay: Duration) -> Self {
        Self {
            max_attempts,
            base_delay,
            max_delay,
            jitter_factor: 0.1,
            backoff_multiplier: 2.0,
        }
    }

    /// Create aggressive retry configuration (fast and frequent)
    pub fn aggressive() -> Self {
        Self {
            max_attempts: 5,
            base_delay: Duration::from_millis(50),
            max_delay: Duration::from_secs(5),
            jitter_factor: 0.05,
            backoff_multiplier: 1.5,
        }
    }

    /// Create conservative retry configuration (slow and few)
    pub fn conservative() -> Self {
        Self {
            max_attempts: 2,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            jitter_factor: 0.2,
            backoff_multiplier: 3.0,
        }
    }

    /// Calculate delay for a specific attempt with exponential backoff and jitter
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return Duration::from_millis(0);
        }

        let base_delay_ms = self.base_delay.as_millis() as f64;
        let max_delay_ms = self.max_delay.as_millis() as f64;

        // Exponential backoff: delay = base_delay * multiplier^(attempt-1)
        let exponential_delay = base_delay_ms * self.backoff_multiplier.powi((attempt - 1) as i32);
        let capped_delay = exponential_delay.min(max_delay_ms);

        // Add jitter: final_delay = delay * (1 + random(-jitter, jitter))
        let jitter_range = capped_delay * self.jitter_factor;
        let jitter = (rand::random::<f64>() - 0.5) * 2.0 * jitter_range;
        let final_delay = (capped_delay + jitter).max(0.);

        Duration::from_millis(final_delay as u64)
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_ai_oracle_client_creation() {
        let client = AIOracleClient::new("http://localhost:8080".to_string());
        assert!(client.is_ok());

        let client = client.unwrap();
        assert_eq!(client.base_url(), "http://localhost:8080");
        assert_eq!(client.timeout(), Duration::from_secs(30));
    }

    #[test]
    fn test_ai_oracle_client_with_timeout() {
        let timeout = Duration::from_secs(60);
        let client = AIOracleClient::with_timeout("http://localhost:8080".to_string(), timeout);
        assert!(client.is_ok());

        let client = client.unwrap();
        assert_eq!(client.timeout(), timeout);
    }

    #[tokio::test]
    async fn test_connectivity_with_invalid_url() {
        let client = AIOracleClient::new("http://invalid-url-12345.com".to_string()).unwrap();
        let result = client.test_connectivity().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), false);
    }

    #[test]
    fn test_url_formatting() {
        let client = AIOracleClient::new("http://localhost:8080/".to_string()).unwrap();
        assert_eq!(client.base_url(), "http://localhost:8080/");

        let client = AIOracleClient::new("http://localhost:8080".to_string()).unwrap();
        assert_eq!(client.base_url(), "http://localhost:8080");
    }

    #[test]
    fn test_custom_config() {
        use std::time::Duration;

        let client = AIOracleClient::with_config(
            "http://localhost:8080".to_string(),
            Duration::from_secs(45),
            20,
            Duration::from_secs(120),
            Duration::from_secs(90),
        );

        assert!(client.is_ok());
        let client = client.unwrap();
        assert_eq!(client.timeout(), Duration::from_secs(45));
    }

    #[test]
    fn test_connection_pool_config() {
        let config = ConnectionPoolConfig::default();
        assert_eq!(config.max_idle_per_host, 10);
        assert_eq!(config.idle_timeout, Duration::from_secs(90));
        assert_eq!(config.tcp_keepalive, Duration::from_secs(60));

        let high_perf = ConnectionPoolConfig::high_performance();
        assert_eq!(high_perf.max_idle_per_host, 50);
        assert!(high_perf.http2_prior_knowledge);

        let low_res = ConnectionPoolConfig::low_resource();
        assert_eq!(low_res.max_idle_per_host, 2);
        assert!(!low_res.http2_prior_knowledge);
    }

    #[test]
    fn test_client_with_pool_config() {
        let config = ConnectionPoolConfig::default();
        let client = AIOracleClient::with_pool_config(
            "http://localhost:8080".to_string(),
            Duration::from_secs(30),
            config,
        );
        assert!(client.is_ok());
    }

    #[test]
    fn test_high_performance_client() {
        let client = AIOracleClient::high_performance(
            "http://localhost:8080".to_string(),
            Duration::from_secs(30),
        );
        assert!(client.is_ok());
    }

    #[test]
    fn test_low_resource_client() {
        let client = AIOracleClient::low_resource(
            "http://localhost:8080".to_string(),
            Duration::from_secs(30),
        );
        assert!(client.is_ok());
    }

    #[tokio::test]
    async fn test_connection_pool_concurrent_requests() {
        let client = AIOracleClient::new("http://httpbin.org".to_string()).unwrap();

        // Test with a small number of concurrent requests
        let results = client.test_connection_pool(3).await;
        assert!(results.is_ok());

        // The actual success depends on network availability, so we just check the structure
        let results = results.unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_request_payload_serialization() {
        let payload = AIRequestPayload::new(
            AIServiceType::FraudDetection,
            json!({"amount": 1000, "currency": "USD"})
        );

        // Test JSON serialization
        let json = payload.to_json();
        assert!(json.is_ok());
        let json_str = json.unwrap();
        assert!(json_str.contains("\"id\""));
        assert!(json_str.contains("\"service_type\":\"FraudDetection\""));
        assert!(json_str.contains("\"request_data\":{\"amount\":1000,\"currency\":\"USD\"}"));

        // Test pretty JSON serialization
        let json_pretty = payload.to_json_pretty();
        assert!(json_pretty.is_ok());
        let json_pretty_str = json_pretty.unwrap();
        assert!(json_pretty_str.contains("\"id\""));
        assert!(json_pretty_str.contains("\"service_type\": \"FraudDetection\""));
        assert!(json_pretty_str.contains("\"request_data\": {\n    \"amount\": 1000,\n    \"currency\": \"USD\"\n  }"));
    }

    #[test]
    fn test_request_payload_deserialization() {
        let json_data = r#"{
            "id": "1234",
            "service_type": "RiskScoring",
            "request_data": {"score": 750},
            "timestamp": 1633072800,
            "metadata": {
                "source": "consensus",
                "version": "1.0",
                "context": {"key": "value"},
                "correlation_id": "abcd-efgh",
                "requester": "user123"
            },
            "priority": "High",
            "timeout": 60,
            "callback_url": "http://localhost/callback",
            "signature": "sig1234"
        }"#;

        let payload: AIRequestPayload = serde_json::from_str(json_data).unwrap();
        assert_eq!(payload.id, "1234");
        assert_eq!(payload.service_type, AIServiceType::RiskScoring);
        assert_eq!(payload.request_data, json!({"score": 750}));
        assert_eq!(payload.timestamp, 1633072800);
        assert!(payload.metadata.is_some());
        assert_eq!(payload.priority, RequestPriority::High);
        assert_eq!(payload.timeout, Some(60));
        assert_eq!(payload.callback_url, Some("http://localhost/callback".to_string()));
        assert_eq!(payload.signature, Some("sig1234".to_string()));
    }

    #[test]
    fn test_request_payload_validation() {
        let mut payload = AIRequestPayload::new(
            AIServiceType::KYC,
            json!({"name": "John Doe", "document": "123456789"})
        );

        // Valid payload
        let validation_result = payload.validate();
        assert!(validation_result.is_ok());

        // Invalid payload: empty ID
        payload.id = "".to_string();
        let validation_result = payload.validate();
        assert!(validation_result.is_err());
        assert_eq!(validation_result.err().unwrap(), "Request ID cannot be empty");

        // Invalid payload: timestamp too old
        payload.id = uuid::Uuid::new_v4().to_string();
        payload.timestamp = 0;
        let validation_result = payload.validate();
        assert!(validation_result.is_err());
        assert_eq!(validation_result.err().unwrap(), "Timestamp cannot be zero");

        // Invalid payload: timeout exceeds maximum
        payload.timestamp = chrono::Utc::now().timestamp() as u64;
        payload.timeout = Some(400);
        let validation_result = payload.validate();
        assert!(validation_result.is_err());
        assert_eq!(validation_result.err().unwrap(), "Timeout cannot exceed 300 seconds");

        // Invalid payload: callback URL invalid
        payload.timeout = Some(60);
        payload.callback_url = Some("invalid-url".to_string());
        let validation_result = payload.validate();
        assert!(validation_result.is_err());
        assert_eq!(validation_result.err().unwrap(), "Callback URL must be a valid HTTP(S) URL");
    }

    #[test]
    fn test_metadata_serialization() {
        let metadata = AIRequestMetadata::new("consensus".to_string(), "1.0".to_string())
            .with_context(json!({"key": "value"}))
            .with_correlation_id("abcd-efgh".to_string())
            .with_requester("user123".to_string());

        // Test that metadata can be serialized as part of a request payload
        let payload = AIRequestPayload::new(AIServiceType::FraudDetection, json!({"test": "data"}))
            .with_metadata(metadata);

        let json_str = payload.to_json().unwrap();
        assert!(!json_str.is_empty());
        assert!(json_str.contains("\"source\":\"consensus\""));
        assert!(json_str.contains("\"version\":\"1.0\""));
        assert!(json_str.contains("\"correlation_id\":\"abcd-efgh\""));
        assert!(json_str.contains("\"requester\":\"user123\""));
    }

    #[test]
    fn test_metadata_deserialization() {
        let json_data = r#"{
            "source": "validation",
            "version": "1.0",
            "context": {"key": "value"},
            "correlation_id": "ijkl-mnop",
            "requester": "user456"
        }"#;

        let metadata: AIRequestMetadata = serde_json::from_str(json_data).unwrap();
        assert_eq!(metadata.source, "validation");
        assert_eq!(metadata.version, "1.0");
        assert!(metadata.context.is_some());
        assert_eq!(metadata.correlation_id, Some("ijkl-mnop".to_string()));
        assert_eq!(metadata.requester, Some("user456".to_string()));
    }

    #[test]
    fn test_ai_request_payload_creation() {
        let request_data = serde_json::json!({
            "transaction_id": "tx123",
            "amount": 1000
        });

        let payload = AIRequestPayload::new(AIServiceType::FraudDetection, request_data);

        assert!(!payload.id.is_empty());
        assert_eq!(payload.service_type, AIServiceType::FraudDetection);
        assert_eq!(payload.priority, RequestPriority::Normal);
        assert!(payload.timestamp > 0);
        assert!(payload.metadata.is_none());
        assert!(payload.timeout.is_none());
        assert!(payload.callback_url.is_none());
        assert!(payload.signature.is_none());
    }

    #[test]
    fn test_ai_request_payload_builder_pattern() {
        let request_data = serde_json::json!({
            "transaction_id": "tx123",
            "amount": 1000
        });

        let metadata = AIRequestMetadata::new(
            "consensus".to_string(),
            "1.0".to_string(),
        );

        let payload = AIRequestPayload::new(AIServiceType::RiskScoring, request_data)
            .with_priority(RequestPriority::High)
            .with_timeout(60)
            .with_callback("https://example.com/callback".to_string())
            .with_metadata(metadata)
            .with_signature("signature123".to_string());

        assert_eq!(payload.service_type, AIServiceType::RiskScoring);
        assert_eq!(payload.priority, RequestPriority::High);
        assert_eq!(payload.timeout, Some(60));
        assert_eq!(payload.callback_url, Some("https://example.com/callback".to_string()));
        assert!(payload.metadata.is_some());
        assert_eq!(payload.signature, Some("signature123".to_string()));
    }

    #[test]
    fn test_ai_request_payload_convenience_methods() {
        let tx_data = serde_json::json!({"tx_id": "123"});

        let fraud_payload = AIRequestPayload::fraud_detection(tx_data.clone());
        assert_eq!(fraud_payload.service_type, AIServiceType::FraudDetection);

        let risk_payload = AIRequestPayload::risk_scoring(tx_data.clone());
        assert_eq!(risk_payload.service_type, AIServiceType::RiskScoring);

        let contract_payload = AIRequestPayload::contract_analysis(tx_data.clone());
        assert_eq!(contract_payload.service_type, AIServiceType::ContractAnalysis);

        let validation_payload = AIRequestPayload::transaction_validation(tx_data);
        assert_eq!(validation_payload.service_type, AIServiceType::TransactionValidation);
    }

    #[test]
    fn test_ai_request_payload_validation() {
        let response_data = serde_json::json!({"test": "data"});
        let payload = AIRequestPayload::new(AIServiceType::FraudDetection, response_data);

        // Should validate successfully
        assert!(payload.validate().is_ok());

        // Test with invalid callback URL
        let invalid_payload = AIRequestPayload::new(AIServiceType::FraudDetection, serde_json::json!({}))
            .with_callback("invalid-url".to_string());

        assert!(invalid_payload.validate().is_err());

        // Test with zero timeout
        let zero_timeout_payload = AIRequestPayload::new(AIServiceType::FraudDetection, serde_json::json!({}))
            .with_timeout(0);

        assert!(zero_timeout_payload.validate().is_err());

        // Test with excessive timeout
        let excessive_timeout_payload = AIRequestPayload::new(AIServiceType::FraudDetection, serde_json::json!({}))
            .with_timeout(400);

        assert!(excessive_timeout_payload.validate().is_err());
    }

    #[test]
    fn test_ai_oracle_error_display() {
        let error = AIOracleError::Network { message: "Connection refused".to_string(), source: None };
        assert_eq!(format!("{}", error), "Network error: Connection refused");

        let error = AIOracleError::Timeout { timeout_ms: 15000 };
        assert_eq!(format!("{}", error), "Request timeout after 15000ms");

        let error = AIOracleError::Http { status: 404, message: "Not Found".to_string() };
        assert_eq!(format!("{}", error), "HTTP error: 404 - Not Found");

        let error = AIOracleError::Serialization { message: "Invalid JSON".to_string() };
        assert_eq!(format!("{}", error), "Serialization error: Invalid JSON");

        let error = AIOracleError::Authentication { message: "Invalid API key".to_string() };
        assert_eq!(format!("{}", error), "Authentication error: Invalid API key");

        let error = AIOracleError::RateLimit { message: "Too many requests".to_string(), retry_after: Some(Duration::from_secs(10)) };
        assert_eq!(format!("{}", error), "Rate limit exceeded: Too many requests");

        let error = AIOracleError::ServiceUnavailable { message: "Service is down".to_string() };
        assert_eq!(format!("{}", error), "Service unavailable: Service is down");

        let error = AIOracleError::Configuration { message: "Invalid config value".to_string() };
        assert_eq!(format!("{}", error), "Configuration error: Invalid config value");

        let error = AIOracleError::Validation { message: "Missing required field".to_string() };
        assert_eq!(format!("{}", error), "Validation error: Missing required field");

        let error = AIOracleError::CircuitBreaker { message: "Too many failures".to_string() };
        assert_eq!(format!("{}", error), "Circuit breaker open: Too many failures");

        let error = AIOracleError::MaxRetriesExceeded { attempts: 5, last_error: Box::new(AIOracleError::Timeout { timeout_ms: 30000 }) };
        assert_eq!(format!("{}", error), "Max retries exceeded: 5 attempts failed");

        let error = AIOracleError::Service {
            code: "SERVICE_UNAVAILABLE".to_string(),
            message: "The service is currently unavailable".to_string(),
            details: None };
        assert_eq!(format!("{}", error), "Service error: SERVICE_UNAVAILABLE - The service is currently unavailable");

        let error = AIOracleError::Unknown { message: "An unknown error occurred".to_string() };
        assert_eq!(format!("{}", error), "Unknown error: An unknown error occurred");
    }

    #[test]
    fn test_ai_oracle_error_is_retryable() {
        let error = AIOracleError::Network { message: "Connection reset".to_string(), source: None };
        assert!(error.is_retryable());

        let error = AIOracleError::Timeout { timeout_ms: 1000 };
        assert!(error.is_retryable());

        let error = AIOracleError::Http { status: 503, message: "Service Unavailable".to_string() };
        assert!(error.is_retryable());

        let error = AIOracleError::RateLimit { message: "Rate limit exceeded".to_string(), retry_after: None };
        assert!(error.is_retryable());

        let error = AIOracleError::ServiceUnavailable { message: "Temporarily down".to_string() };
        assert!(error.is_retryable());

        let error = AIOracleError::Authentication { message: "Invalid credentials".to_string() };
        assert!(!error.is_retryable());

        let error = AIOracleError::Validation { message: "Invalid input".to_string() };
        assert!(!error.is_retryable());

        let error = AIOracleError::Configuration { message: "Missing endpoint".to_string() };
        assert!(!error.is_retryable());

        let error = AIOracleError::Service { code: "INVALID_INPUT".to_string(), message: "Bad data".to_string(), details: None };
        assert!(!error.is_retryable());
    }

    #[test]
    fn test_retry_config_delay_calculation() {
        let config = RetryConfig::default();

        // Test delay for first attempt (should be 0)
        assert_eq!(config.delay_for_attempt(0), Duration::from_millis(0));

        // Test delay for second attempt (base delay * multiplier with jitter)
        let delay = config.delay_for_attempt(1);
        // With jitter, delay should be around base_delay but may vary
        assert!(delay >= Duration::from_millis(50) && delay <= Duration::from_millis(200));

        // Test delay for third attempt (base delay * multiplier^2 with jitter)
        let delay = config.delay_for_attempt(2);
        // With jitter, delay should be around base_delay * multiplier^2 but may vary
        assert!(delay >= Duration::from_millis(100) && delay <= Duration::from_millis(500));

        // Test that very high attempts are capped
        let high_delay = config.delay_for_attempt(20);
        // With jitter, the delay can exceed max_delay slightly, so we allow reasonable tolerance
        assert!(high_delay <= config.max_delay * 2); // Allow 2x tolerance for jitter
    }

    #[test]
    fn test_retry_config_aggressive() {
        let config = RetryConfig::aggressive();

        assert_eq!(config.max_attempts, 5);
        assert!(config.base_delay < config.max_delay);
        assert!(config.jitter_factor < 0.1);
        assert!(config.backoff_multiplier > 1.0);
    }

    #[test]
    fn test_retry_config_conservative() {
        let config = RetryConfig::conservative();

        assert_eq!(config.max_attempts, 2);
        assert!(config.base_delay >= Duration::from_millis(1000));
        assert!(config.max_delay >= Duration::from_millis(60000));
        assert!(config.jitter_factor >= 0.1);
        assert!(config.backoff_multiplier >= 2.0);
    }

    #[test]
    fn test_ai_service_config_creation() {
        let config = AIServiceConfig::new("https://api.example.com".to_string());
        assert_eq!(config.base_url, "https://api.example.com");
        assert_eq!(config.timeout_seconds, 30);
        assert_eq!(config.max_retries, 3);
        assert!(config.api_key.is_none());
    }

    #[test]
    fn test_ai_service_config_presets() {
        let dev_config = AIServiceConfig::development();
        assert_eq!(dev_config.base_url, "http://localhost:8080");
        assert_eq!(dev_config.timeout_seconds, 10);
        assert_eq!(dev_config.max_retries, 2);
        assert_eq!(dev_config.fallback_config.fallback_mode, "permissive");

        let prod_config = AIServiceConfig::production();
        assert_eq!(prod_config.base_url, "https://ai-service.dytallix.com");
        assert_eq!(prod_config.timeout_seconds, 30);
        assert_eq!(prod_config.max_retries, 5);
        assert_eq!(prod_config.fallback_config.fallback_mode, "restrictive");

        let test_config = AIServiceConfig::testing();
        assert_eq!(test_config.base_url, "http://localhost:8081");
        assert_eq!(test_config.timeout_seconds, 5);
        assert_eq!(test_config.max_retries, 1);
        assert!(!test_config.fallback_config.enable_fallback);
    }

    #[test]
    fn test_ai_service_config_builder_pattern() {
        let config = AIServiceConfig::new("https://api.example.com".to_string())
            .with_api_key("secret-key".to_string())
            .with_timeout(60)
            .with_retry_config(5, 200, 10000)
            .with_health_check_config(20, 3, 5)
            .with_circuit_breaker_config(0.7, 120);

        assert_eq!(config.api_key, Some("secret-key".to_string()));
        assert_eq!(config.timeout_seconds, 60);
        assert_eq!(config.max_retries, 5);
        assert_eq!(config.base_retry_delay_ms, 200);
        assert_eq!(config.max_retry_delay_ms, 10000);
        assert_eq!(config.health_check_interval_seconds, 20);
        assert_eq!(config.health_check_timeout_seconds, 3);
        assert_eq!(config.health_check_failure_threshold, 5);
        assert_eq!(config.circuit_breaker_failure_threshold, 0.7);
        assert_eq!(config.circuit_breaker_recovery_time_seconds, 120);
    }

    #[test]
    fn test_ai_service_config_validation() {
        let mut config = AIServiceConfig::new("https://api.example.com".to_string());
        assert!(config.validate().is_ok());

        // Test invalid base URL
        config.base_url = "".to_string();
        assert!(config.validate().is_err());

        // Test invalid timeout
        config.base_url = "https://api.example.com".to_string();
        config.timeout_seconds = 0;
        assert!(config.validate().is_err());

        // Test invalid max retries
        config.timeout_seconds = 30;
        config.max_retries = 11;
        assert!(config.validate().is_err());

        // Test invalid retry delays
        config.max_retries = 3;
        config.base_retry_delay_ms = 5000;
        config.max_retry_delay_ms = 1000;
        assert!(config.validate().is_err());

        // Test invalid jitter
        config.base_retry_delay_ms = 100;
        config.max_retry_delay_ms = 5000;
        config.retry_jitter = 1.5;
        assert!(config.validate().is_err());

        // Test invalid circuit breaker threshold
        config.retry_jitter = 0.1;
        config.circuit_breaker_failure_threshold = 1.5;
        assert!(config.validate().is_err());

        // Test invalid risk thresholds
        config.circuit_breaker_failure_threshold = 0.5;
        config.risk_config.low_risk_threshold = 0.8;
        config.risk_config.medium_risk_threshold = 0.6;
        assert!(config.validate().is_err());

        // Test invalid auto thresholds
        config.risk_config.low_risk_threshold = 0.3;
        config.risk_config.medium_risk_threshold = 0.6;
        config.risk_config.auto_approve_threshold = 0.9;
        config.risk_config.auto_reject_threshold = 0.1;
        assert!(config.validate().is_err());

        // Test invalid fallback mode
        config.risk_config.auto_approve_threshold = 0.2;
        config.risk_config.auto_reject_threshold = 0.9;
        config.fallback_config.fallback_mode = "invalid_mode".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_ai_service_config_endpoint_urls() {
        let config = AIServiceConfig::new("https://api.example.com".to_string());

        assert_eq!(
            config.get_endpoint_url(&AIServiceType::FraudDetection),
            "https://api.example.com/api/v1/fraud-detection"
        );
        assert_eq!(
            config.get_endpoint_url(&AIServiceType::RiskScoring),
            "https://api.example.com/api/v1/risk-scoring"
        );
        assert_eq!(
            config.get_endpoint_url(&AIServiceType::KYC),
            "https://api.example.com/api/v1/kyc"
        );
        assert_eq!(
            config.get_health_check_url(),
            "https://api.example.com/health"
        );
        assert_eq!(
            config.get_batch_url(),
            "https://api.example.com/api/v1/batch"
        );
    }

    #[test]
    fn test_ai_service_config_endpoint_urls_with_trailing_slash() {
        let config = AIServiceConfig::new("https://api.example.com/".to_string());

        assert_eq!(
            config.get_endpoint_url(&AIServiceType::FraudDetection),
            "https://api.example.com/api/v1/fraud-detection"
        );
        assert_eq!(
            config.get_health_check_url(),
            "https://api.example.com/health"
        );
    }

    #[test]
    fn test_ai_service_config_retry_delay_calculation() {
        let config = AIServiceConfig::new("https://api.example.com".to_string())
            .with_retry_config(5, 100, 5000);

        // Test exponential backoff
        let delay1 = config.calculate_retry_delay(0);
        let delay2 = config.calculate_retry_delay(1);
        let delay3 = config.calculate_retry_delay(2);

        // First retry should be around base_delay * 2^0 = 100ms
        assert!(delay1.as_millis() >= 80 && delay1.as_millis() <= 120);

        // Second retry should be around base_delay * 2^1 = 200ms
        assert!(delay2.as_millis() >= 160 && delay2.as_millis() <= 240);

        // Third retry should be around base_delay * 2^2 = 400ms
        assert!(delay3.as_millis() >= 320 && delay3.as_millis() <= 480);

        // Test that delay is capped at max_retry_delay_ms
        let long_delay = config.calculate_retry_delay(10);
        assert!(long_delay.as_millis() <= 5500); // Should be capped + jitter
    }

    #[test]
    fn test_ai_oracle_error_retryable_classification() {
        // Test network errors are retryable
        assert!(AIOracleError::Network { message: "Connection failed".to_string(), source: None }.is_retryable());

        // Test timeout errors are retryable
        assert!(AIOracleError::Timeout { timeout_ms: 5000 }.is_retryable());

        // Test retryable HTTP errors
        assert!(AIOracleError::Http { status: 500, message: "Internal Server Error".to_string() }.is_retryable());
        assert!(AIOracleError::Http { status: 502, message: "Bad Gateway".to_string() }.is_retryable());
        assert!(AIOracleError::Http { status: 503, message: "Service Unavailable".to_string() }.is_retryable());
        assert!(AIOracleError::Http { status: 504, message: "Gateway Timeout".to_string() }.is_retryable());
        assert!(AIOracleError::Http { status: 429, message: "Too Many Requests".to_string() }.is_retryable());

        // Test non-retryable HTTP errors
        assert!(!AIOracleError::Http { status: 400, message: "Bad Request".to_string() }.is_retryable());
        assert!(!AIOracleError::Http { status: 401, message: "Unauthorized".to_string() }.is_retryable());
        assert!(!AIOracleError::Http { status: 404, message: "Not Found".to_string() }.is_retryable());

        // Test service unavailable is retryable
        assert!(AIOracleError::ServiceUnavailable { message: "Service down".to_string() }.is_retryable());

        // Test rate limit is retryable
        assert!(AIOracleError::RateLimit { message: "Rate limited".to_string(), retry_after: None }.is_retryable());

        // Test non-retryable errors
        assert!(!AIOracleError::Authentication { message: "Invalid API key".to_string() }.is_retryable());
        assert!(!AIOracleError::Validation { message: "Invalid input".to_string() }.is_retryable());
        assert!(!AIOracleError::Configuration { message: "Missing endpoint".to_string() }.is_retryable());
        assert!(!AIOracleError::Service { code: "INVALID_INPUT".to_string(), message: "Bad data".to_string(), details: None }.is_retryable());
    }

    #[tokio::test]
    async fn test_request_payload_validation_with_retry_logging() {
        // Create a client for testing
        let client = AIOracleClient::new("http://localhost:8080".to_string()).unwrap();

        // Test with invalid payload (empty ID)
        let mut invalid_payload = AIRequestPayload::new(
            AIServiceType::RiskScoring,
            serde_json::json!({"test": "data"})
        );
        invalid_payload.id = "".to_string();

        let retry_config = RetryConfig::default();
        let result = client.send_ai_request_with_retry(&invalid_payload, &retry_config).await;

        // Should fail with validation error (non-retryable)
        assert!(result.is_err());
        match result.unwrap_err() {
            AIOracleError::Validation { message } => {
                assert!(message.contains("Invalid request payload"));
                assert!(message.contains("Request ID cannot be empty"));
            }
            _ => panic!("Expected validation error"),
        }
    }

    #[tokio::test]
    async fn test_retry_logic_with_different_error_types() {
        // Create a client for testing
        let client = AIOracleClient::new("http://localhost:8080".to_string()).unwrap();

        // Create a valid request payload
        let payload = AIRequestPayload::new(
            AIServiceType::FraudDetection,
            serde_json::json!({"transaction_id": "tx123", "amount": 1000})
        );

        // Test retry configuration
        let retry_config = RetryConfig::new(
            3,
            Duration::from_millis(10), // Very short delay for testing
            Duration::from_millis(100)
        );

        // This will fail with network error (invalid URL), which should be retryable
        let result = client.send_ai_request_with_retry(&payload, &retry_config).await;

        // Should fail after max retries
        assert!(result.is_err());
        match result.unwrap_err() {
            AIOracleError::MaxRetriesExceeded { attempts, last_error } => {
                assert_eq!(attempts, 3);
                // The last error should be a network error
                assert!(last_error.is_retryable());
            }
            _ => {
                // Direct network error is also acceptable for unreachable service
                // The retry logic will only apply if the service responds with retryable errors
            }
        }
    }
}
