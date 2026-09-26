//! Error types for the Dytallix SDK crate.

/// Errors produced by the Dytallix SDK.
#[derive(Debug, thiserror::Error)]
pub enum SdkError {
    /// Wraps a core cryptography error.
    #[error(transparent)]
    Core(#[from] dytallix_core::DytallixError),
    /// A token-specific balance check failed.
    #[error("Insufficient balance: need {required} {token}, have {available} {token}")]
    InsufficientBalance {
        /// The token whose balance was insufficient.
        token: crate::Token,
        /// The required token amount.
        required: u128,
        /// The available token amount.
        available: u128,
    },
    /// Provided gas did not satisfy the required gas amount.
    #[error("Insufficient gas: need {required} units, provided {provided}")]
    InsufficientGas {
        /// The required gas units.
        required: u64,
        /// The provided gas units.
        provided: u64,
    },
    /// The faucet endpoint rate-limited a request.
    #[error("Faucet rate limited: retry after {retry_after_seconds}s")]
    FaucetRateLimited {
        /// Seconds until the next allowed faucet request.
        retry_after_seconds: u64,
    },
    /// The faucet endpoint could not serve the request.
    #[error("Faucet unavailable at {endpoint}: {reason}")]
    FaucetUnavailable {
        /// The faucet endpoint that failed.
        endpoint: String,
        /// The reported failure reason.
        reason: String,
    },
    /// The node endpoint could not serve the request.
    #[error("Node unavailable at {endpoint}: {reason}")]
    NodeUnavailable {
        /// The node endpoint that failed.
        endpoint: String,
        /// The reported failure reason.
        reason: String,
    },
    /// A transaction was rejected by the local builder or remote node.
    #[error("Transaction rejected: {0}")]
    TransactionRejected(String),
    /// A contract deployment attempt failed.
    #[error("Contract deployment failed: {0}")]
    ContractDeployFailed(String),
    /// The requested keystore file did not exist.
    #[error("Keystore not found at {0}")]
    KeystoreNotFound(std::path::PathBuf),
    /// The keystore file contents were malformed or inconsistent.
    #[error("Keystore corrupt: {0}")]
    KeystoreCorrupt(String),
    /// The target network does not match the expected network.
    #[error("Network mismatch: {0}")]
    NetworkMismatch(String),
    /// Generic network-layer failure.
    #[error("Network error: {0}")]
    Network(String),
    /// Wraps a standard I/O error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// Serialization or deserialization failed.
    #[error("Serialization error: {0}")]
    Serialization(String),
}
