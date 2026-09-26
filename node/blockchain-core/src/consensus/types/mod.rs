//! Type definitions for the consensus module
//!
//! This module organizes all type definitions used throughout the consensus system
//! into logical groups for better maintainability and modularity.

pub mod ai_types;
pub mod config_types;
pub mod error_types;
pub mod oracle_types;

// Re-export all types for convenience
pub use ai_types::*;
pub use config_types::*;
pub use error_types::*;
pub use oracle_types::*;
