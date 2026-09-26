use serde::{Deserialize, Serialize};
use std::time::Duration;

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

impl AIResponseError {
    /// Create a new AI response error
    pub fn new(code: String, message: String, category: ErrorCategory, retryable: bool) -> Self {
        Self {
            code,
            message,
            details: None,
            category,
            retryable,
        }
    }

    /// Create a validation error
    pub fn validation(message: String) -> Self {
        Self::new(
            "VALIDATION_ERROR".to_string(),
            message,
            ErrorCategory::ValidationError,
            false,
        )
    }

    /// Create a processing error
    pub fn processing(message: String) -> Self {
        Self::new(
            "PROCESSING_ERROR".to_string(),
            message,
            ErrorCategory::ProcessingError,
            false,
        )
    }

    /// Create a network error
    pub fn network(message: String) -> Self {
        Self::new(
            "NETWORK_ERROR".to_string(),
            message,
            ErrorCategory::NetworkError,
            true,
        )
    }

    /// Create an authentication error
    pub fn authentication(message: String) -> Self {
        Self::new(
            "AUTH_ERROR".to_string(),
            message,
            ErrorCategory::AuthenticationError,
            false,
        )
    }

    /// Create a rate limit error
    pub fn rate_limit(message: String) -> Self {
        Self::new(
            "RATE_LIMIT_ERROR".to_string(),
            message,
            ErrorCategory::RateLimitError,
            true,
        )
    }

    /// Create a service error
    pub fn service(message: String) -> Self {
        Self::new(
            "SERVICE_ERROR".to_string(),
            message,
            ErrorCategory::ServiceError,
            false,
        )
    }

    /// Create an unknown error
    pub fn unknown(message: String) -> Self {
        Self::new(
            "UNKNOWN_ERROR".to_string(),
            message,
            ErrorCategory::UnknownError,
            false,
        )
    }

    /// Create a timeout error
    pub fn timeout(message: String) -> Self {
        Self::new(
            "TIMEOUT".to_string(),
            message,
            ErrorCategory::NetworkError,
            true,
        )
    }

    /// Add details to the error
    pub fn with_details(mut self, details: String) -> Self {
        self.details = Some(details);
        self
    }
}

/// Comprehensive error types for AI Oracle operations
#[derive(Debug, thiserror::Error)]
pub enum AIOracleError {
    #[error("Network error: {message}")]
    Network {
        message: String,
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("Request timeout after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },

    #[error("HTTP error: {status} - {message}")]
    Http { status: u16, message: String },

    #[error("Serialization error: {message}")]
    Serialization { message: String },

    #[error("Authentication error: {message}")]
    Authentication { message: String },

    #[error("Rate limit exceeded: {message}")]
    RateLimit {
        message: String,
        retry_after: Option<Duration>,
    },

    #[error("Service unavailable: {message}")]
    ServiceUnavailable { message: String },

    #[error("Configuration error: {message}")]
    Configuration { message: String },

    #[error("Validation error: {message}")]
    Validation { message: String },

    #[error("Circuit breaker open: {message}")]
    CircuitBreaker { message: String },

    #[error("Max retries exceeded: {attempts} attempts failed")]
    MaxRetriesExceeded {
        attempts: u32,
        last_error: Box<AIOracleError>,
    },

    #[error("Service error: {code} - {message}")]
    Service {
        code: String,
        message: String,
        details: Option<String>,
    },

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
                *status >= 500
                    || *status == 408
                    || *status == 429
                    || *status == 502
                    || *status == 503
                    || *status == 504
            }
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

    /// Get the error category for this error
    pub fn category(&self) -> ErrorCategory {
        match self {
            AIOracleError::Network { .. } => ErrorCategory::NetworkError,
            AIOracleError::Timeout { .. } => ErrorCategory::NetworkError,
            AIOracleError::Http { .. } => ErrorCategory::NetworkError,
            AIOracleError::Serialization { .. } => ErrorCategory::ProcessingError,
            AIOracleError::Authentication { .. } => ErrorCategory::AuthenticationError,
            AIOracleError::RateLimit { .. } => ErrorCategory::RateLimitError,
            AIOracleError::ServiceUnavailable { .. } => ErrorCategory::ServiceError,
            AIOracleError::Configuration { .. } => ErrorCategory::ValidationError,
            AIOracleError::Validation { .. } => ErrorCategory::ValidationError,
            AIOracleError::CircuitBreaker { .. } => ErrorCategory::ServiceError,
            AIOracleError::MaxRetriesExceeded { .. } => ErrorCategory::ServiceError,
            AIOracleError::Service { .. } => ErrorCategory::ServiceError,
            AIOracleError::Unknown { .. } => ErrorCategory::UnknownError,
        }
    }

    /// Create a network error
    pub fn network(message: String) -> Self {
        AIOracleError::Network {
            message,
            source: None,
        }
    }

    /// Create a network error with source
    pub fn network_with_source(
        message: String,
        source: Box<dyn std::error::Error + Send + Sync>,
    ) -> Self {
        AIOracleError::Network {
            message,
            source: Some(source),
        }
    }

    /// Create a timeout error
    pub fn timeout(timeout_ms: u64) -> Self {
        AIOracleError::Timeout { timeout_ms }
    }

    /// Create an HTTP error
    pub fn http(status: u16, message: String) -> Self {
        AIOracleError::Http { status, message }
    }

    /// Create a serialization error
    pub fn serialization(message: String) -> Self {
        AIOracleError::Serialization { message }
    }

    /// Create an authentication error
    pub fn authentication(message: String) -> Self {
        AIOracleError::Authentication { message }
    }

    /// Create a rate limit error
    pub fn rate_limit(message: String, retry_after: Option<Duration>) -> Self {
        AIOracleError::RateLimit {
            message,
            retry_after,
        }
    }

    /// Create a service unavailable error
    pub fn service_unavailable(message: String) -> Self {
        AIOracleError::ServiceUnavailable { message }
    }

    /// Create a configuration error
    pub fn configuration(message: String) -> Self {
        AIOracleError::Configuration { message }
    }

    /// Create a validation error
    pub fn validation(message: String) -> Self {
        AIOracleError::Validation { message }
    }

    /// Create a circuit breaker error
    pub fn circuit_breaker(message: String) -> Self {
        AIOracleError::CircuitBreaker { message }
    }

    /// Create a max retries exceeded error
    pub fn max_retries_exceeded(attempts: u32, last_error: AIOracleError) -> Self {
        AIOracleError::MaxRetriesExceeded {
            attempts,
            last_error: Box::new(last_error),
        }
    }

    /// Create a service error
    pub fn service(code: String, message: String, details: Option<String>) -> Self {
        AIOracleError::Service {
            code,
            message,
            details,
        }
    }

    /// Create an unknown error
    pub fn unknown(message: String) -> Self {
        AIOracleError::Unknown { message }
    }
}

/// Convert from AIResponseError to AIOracleError
impl From<AIResponseError> for AIOracleError {
    fn from(error: AIResponseError) -> Self {
        match error.category {
            ErrorCategory::ValidationError => AIOracleError::validation(error.message),
            ErrorCategory::ProcessingError => AIOracleError::serialization(error.message),
            ErrorCategory::NetworkError => AIOracleError::network(error.message),
            ErrorCategory::AuthenticationError => AIOracleError::authentication(error.message),
            ErrorCategory::RateLimitError => AIOracleError::rate_limit(error.message, None),
            ErrorCategory::ServiceError => {
                AIOracleError::service(error.code, error.message, error.details)
            }
            ErrorCategory::UnknownError => AIOracleError::unknown(error.message),
        }
    }
}

/// Convert from AIOracleError to AIResponseError
impl From<AIOracleError> for AIResponseError {
    fn from(error: AIOracleError) -> Self {
        let category = error.category();
        let retryable = error.is_retryable();

        match error {
            AIOracleError::Network { message, .. } => {
                AIResponseError::new("NETWORK_ERROR".to_string(), message, category, retryable)
            }
            AIOracleError::Timeout { timeout_ms } => AIResponseError::new(
                "TIMEOUT".to_string(),
                format!("Request timeout after {timeout_ms}ms"),
                category,
                retryable,
            ),
            AIOracleError::Http { status, message } => {
                AIResponseError::new(format!("HTTP_{status}"), message, category, retryable)
            }
            AIOracleError::Serialization { message } => AIResponseError::new(
                "SERIALIZATION_ERROR".to_string(),
                message,
                category,
                retryable,
            ),
            AIOracleError::Authentication { message } => {
                AIResponseError::new("AUTH_ERROR".to_string(), message, category, retryable)
            }
            AIOracleError::RateLimit { message, .. } => {
                AIResponseError::new("RATE_LIMIT_ERROR".to_string(), message, category, retryable)
            }
            AIOracleError::ServiceUnavailable { message } => AIResponseError::new(
                "SERVICE_UNAVAILABLE".to_string(),
                message,
                category,
                retryable,
            ),
            AIOracleError::Configuration { message } => {
                AIResponseError::new("CONFIG_ERROR".to_string(), message, category, retryable)
            }
            AIOracleError::Validation { message } => {
                AIResponseError::new("VALIDATION_ERROR".to_string(), message, category, retryable)
            }
            AIOracleError::CircuitBreaker { message } => AIResponseError::new(
                "CIRCUIT_BREAKER_OPEN".to_string(),
                message,
                category,
                retryable,
            ),
            AIOracleError::MaxRetriesExceeded { attempts, .. } => AIResponseError::new(
                "MAX_RETRIES_EXCEEDED".to_string(),
                format!("Max retries exceeded: {attempts} attempts failed"),
                category,
                retryable,
            ),
            AIOracleError::Service {
                code,
                message,
                details,
            } => AIResponseError::new(code, message, category, retryable)
                .with_details(details.unwrap_or_default()),
            AIOracleError::Unknown { message } => {
                AIResponseError::new("UNKNOWN_ERROR".to_string(), message, category, retryable)
            }
        }
    }
}
