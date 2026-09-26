use dytallix_contracts::staking::{StakingConfig, StakingContract, StakingError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistributionReceipt {
    pub staking_rewards: u128,
    pub treasury_rewards: u128,
    pub ecosystem_rewards: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardSplitter {
    pub staking: StakingContract,
    pub treasury_address: String,
    pub treasury_balance: u128,
    pub ecosystem_address: String,
    pub ecosystem_balance: u128,
    pub staking_bps: u16,
    pub treasury_bps: u16,
    pub ecosystem_bps: u16,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RewardSplitterError {
    #[error("distribution basis points must sum to 10000")]
    InvalidDistribution,
    #[error(transparent)]
    Staking(#[from] StakingError),
}

impl RewardSplitter {
    pub fn new(
        treasury_address: impl Into<String>,
        ecosystem_address: impl Into<String>,
        staking_bps: u16,
        treasury_bps: u16,
        ecosystem_bps: u16,
    ) -> Result<Self, RewardSplitterError> {
        if staking_bps as u32 + treasury_bps as u32 + ecosystem_bps as u32 != 10_000 {
            return Err(RewardSplitterError::InvalidDistribution);
        }

        Ok(Self {
            staking: StakingContract::new(StakingConfig::default()),
            treasury_address: treasury_address.into(),
            treasury_balance: 0,
            ecosystem_address: ecosystem_address.into(),
            ecosystem_balance: 0,
            staking_bps,
            treasury_bps,
            ecosystem_bps,
        })
    }

    pub fn register_validator(
        &mut self,
        validator: impl Into<String>,
        self_bond: u128,
        block: u64,
    ) -> Result<(), RewardSplitterError> {
        self.staking
            .register_validator(validator.into(), self_bond, 500, None, block)?;
        Ok(())
    }

    pub fn delegate(
        &mut self,
        delegator: impl Into<String>,
        validator: impl Into<String>,
        amount: u128,
    ) -> Result<(), RewardSplitterError> {
        self.staking
            .delegate(delegator.into(), validator.into(), amount)?;
        Ok(())
    }

    pub fn distribute_epoch_rewards(
        &mut self,
        total_emission: u128,
    ) -> Result<DistributionReceipt, RewardSplitterError> {
        let staking_rewards = total_emission * self.staking_bps as u128 / 10_000;
        let treasury_rewards = total_emission * self.treasury_bps as u128 / 10_000;
        let ecosystem_rewards = total_emission.saturating_sub(staking_rewards + treasury_rewards);

        self.staking.apply_reward_emission(staking_rewards);
        self.treasury_balance += treasury_rewards;
        self.ecosystem_balance += ecosystem_rewards;

        Ok(DistributionReceipt {
            staking_rewards,
            treasury_rewards,
            ecosystem_rewards,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_emissions_and_accrues_staking_rewards() {
        let mut splitter =
            RewardSplitter::new("treasury", "ecosystem", 6_000, 2_000, 2_000).unwrap();
        splitter
            .register_validator("validator1", 1_000_000, 1)
            .unwrap();
        splitter.delegate("alice", "validator1", 500_000).unwrap();

        let receipt = splitter.distribute_epoch_rewards(1_000_000).unwrap();
        assert_eq!(receipt.staking_rewards, 600_000);
        assert_eq!(splitter.treasury_balance, 200_000);
        assert_eq!(splitter.ecosystem_balance, 200_000);

        let alice_rewards = splitter
            .staking
            .claim_rewards(&"alice".to_string(), &"validator1".to_string())
            .unwrap();
        assert!(alice_rewards > 0);
    }
}
