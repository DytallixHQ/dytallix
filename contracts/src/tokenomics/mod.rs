/*!
Dytallix Tokenomics Module

Implements a dual-token system:
- DGT (Dytallix Governance Token): Fixed supply governance token
- DRT (Dytallix Reward Token): Adaptive emission reward token with burning capability
- Emission Controller: Manages DRT emission rates based on DAO governance

All contracts are WASM-compatible and integrate with the existing governance system.
*/

pub mod dgt_token;
pub mod drt_token;
pub mod emission_controller;
pub mod types;

pub use dgt_token::DGTToken;
pub use drt_token::DRTToken;
pub use emission_controller::EmissionController;
pub use types::*;

// Re-export for WASM compatibility
pub use dgt_token::*;
pub use drt_token::*;
pub use emission_controller::*;
