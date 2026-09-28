use serde::{Deserialize, Serialize};

/// Configuration for a user's dead man switch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadManSwitchConfig {
    /// The address that can claim the funds
    pub beneficiary: String,
    /// The inactivity period in blocks after which funds can be claimed
    pub period_blocks: u64,
    /// The block height of the last activity (registration or ping)
    pub last_active_block: u64,
}
