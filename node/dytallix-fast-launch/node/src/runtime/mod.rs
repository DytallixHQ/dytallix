#[cfg(feature = "legacy-services")]
pub mod bridge;
pub mod dead_man_switch;
pub mod emission;
pub mod fee_burn;
pub mod governance;
pub mod governance_ballot;
pub mod governance_candidate;
pub mod governance_custody_transition;
pub mod governance_deposit_stage;
pub mod governance_escrow;
pub mod governance_ordered_admission;
pub mod governance_state;
pub mod issuance_timing;
#[cfg(feature = "oracle")]
pub mod oracle;
pub mod penalty_custody;
pub mod reward_allocation;
pub mod reward_runtime;
pub mod staking;
pub mod validator_lifecycle;
#[cfg(feature = "contracts")]
pub mod wasm;

#[cfg(test)]
pub mod tests;

// The modules are feature-gated, meaning they will only be compiled
// and included in the project if the corresponding feature is enabled.
// This allows for optional functionality and reduces the binary size
// for users who do not need the extra features.
