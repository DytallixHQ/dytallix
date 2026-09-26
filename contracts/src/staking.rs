use crate::types::{Address, Amount, BlockNumber};
use scale::{Decode, Encode};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const REWARD_SCALE: u128 = 1_000_000_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub struct StakingConfig {
    pub min_self_bond: Amount,
    pub min_delegation: Amount,
    pub max_validators: u32,
}

impl Default for StakingConfig {
    fn default() -> Self {
        Self {
            min_self_bond: 1_000_000,
            min_delegation: 100_000,
            max_validators: 100,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub enum ValidatorStatus {
    Active,
    Jailed,
    Tombstoned,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub struct ValidatorProfile {
    pub operator: Address,
    pub commission_bps: u16,
    pub self_bond: Amount,
    pub delegated_stake: Amount,
    pub status: ValidatorStatus,
    pub metadata: Option<String>,
    pub created_at: BlockNumber,
    pub slash_count: u32,
}

impl ValidatorProfile {
    pub fn total_stake(&self) -> Amount {
        self.self_bond + self.delegated_stake
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub struct DelegationPosition {
    pub delegator: Address,
    pub validator: Address,
    pub stake: Amount,
    pub accrued_rewards: Amount,
    pub reward_index_snapshot: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub struct StakingStats {
    pub validator_count: u32,
    pub delegation_count: u32,
    pub total_stake: Amount,
    pub reward_index: u128,
    pub pending_rewards: Amount,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StakingError {
    #[error("validator already exists")]
    ValidatorAlreadyExists,
    #[error("validator not found")]
    ValidatorNotFound,
    #[error("validator is not active")]
    ValidatorNotActive,
    #[error("max validators reached")]
    MaxValidatorsReached,
    #[error("delegation amount below minimum")]
    DelegationTooSmall,
    #[error("self bond below minimum")]
    SelfBondTooSmall,
    #[error("delegation not found")]
    DelegationNotFound,
    #[error("insufficient delegated balance")]
    InsufficientDelegation,
    #[error("invalid slash percentage")]
    InvalidSlashRate,
    #[error("commission rate exceeds 100%")]
    InvalidCommission,
}

pub type StakingResult<T> = Result<T, StakingError>;

#[derive(Debug, Clone, Encode, Decode, Serialize, Deserialize)]
pub struct StakingContract {
    pub config: StakingConfig,
    pub validators: BTreeMap<Address, ValidatorProfile>,
    pub delegations: BTreeMap<(Address, Address), DelegationPosition>,
    pub total_stake: Amount,
    pub reward_index: u128,
    pub reward_index_residual: u128,
    pub pending_rewards: Amount,
}

impl StakingContract {
    pub fn new(config: StakingConfig) -> Self {
        Self {
            config,
            validators: BTreeMap::new(),
            delegations: BTreeMap::new(),
            total_stake: 0,
            reward_index: 0,
            reward_index_residual: 0,
            pending_rewards: 0,
        }
    }

    pub fn register_validator(
        &mut self,
        operator: Address,
        self_bond: Amount,
        commission_bps: u16,
        metadata: Option<String>,
        current_block: BlockNumber,
    ) -> StakingResult<()> {
        if self.validators.contains_key(&operator) {
            return Err(StakingError::ValidatorAlreadyExists);
        }
        if self.validators.len() as u32 >= self.config.max_validators {
            return Err(StakingError::MaxValidatorsReached);
        }
        if self_bond < self.config.min_self_bond {
            return Err(StakingError::SelfBondTooSmall);
        }
        // Commission is expressed in basis points; reject anything above 100%.
        if commission_bps > 10_000 {
            return Err(StakingError::InvalidCommission);
        }

        self.validators.insert(
            operator.clone(),
            ValidatorProfile {
                operator: operator.clone(),
                commission_bps,
                self_bond,
                delegated_stake: 0,
                status: ValidatorStatus::Active,
                metadata,
                created_at: current_block,
                slash_count: 0,
            },
        );

        self.delegations.insert(
            (operator.clone(), operator.clone()),
            DelegationPosition {
                delegator: operator.clone(),
                validator: operator.clone(),
                stake: self_bond,
                accrued_rewards: 0,
                reward_index_snapshot: self.reward_index,
            },
        );
        self.total_stake += self_bond;

        Ok(())
    }

    pub fn set_validator_status(
        &mut self,
        validator: &Address,
        status: ValidatorStatus,
    ) -> StakingResult<()> {
        let profile = self
            .validators
            .get_mut(validator)
            .ok_or(StakingError::ValidatorNotFound)?;
        profile.status = status;
        Ok(())
    }

    pub fn delegate(
        &mut self,
        delegator: Address,
        validator: Address,
        amount: Amount,
    ) -> StakingResult<()> {
        if amount < self.config.min_delegation {
            return Err(StakingError::DelegationTooSmall);
        }

        let profile = self
            .validators
            .get(&validator)
            .ok_or(StakingError::ValidatorNotFound)?;
        if profile.status != ValidatorStatus::Active {
            return Err(StakingError::ValidatorNotActive);
        }

        let key = (delegator.clone(), validator.clone());
        self.settle_position(&key);
        let position = self
            .delegations
            .entry(key)
            .or_insert_with(|| DelegationPosition {
                delegator: delegator.clone(),
                validator: validator.clone(),
                stake: 0,
                accrued_rewards: 0,
                reward_index_snapshot: self.reward_index,
            });
        position.stake += amount;

        self.total_stake += amount;
        self.recompute_validator_totals(&validator)?;

        Ok(())
    }

    pub fn undelegate(
        &mut self,
        delegator: &Address,
        validator: &Address,
        amount: Amount,
    ) -> StakingResult<Amount> {
        let key = (delegator.clone(), validator.clone());
        self.settle_position(&key);

        let current_stake = self
            .delegations
            .get(&key)
            .ok_or(StakingError::DelegationNotFound)?
            .stake;
        if current_stake < amount {
            return Err(StakingError::InsufficientDelegation);
        }

        if delegator == validator {
            let remaining = current_stake - amount;
            if remaining < self.config.min_self_bond {
                return Err(StakingError::SelfBondTooSmall);
            }
        }

        let remove_position = {
            let position = self
                .delegations
                .get_mut(&key)
                .ok_or(StakingError::DelegationNotFound)?;
            position.stake -= amount;
            position.stake == 0 && delegator != validator
        };

        if remove_position {
            self.delegations.remove(&key);
        }

        self.total_stake -= amount;
        self.recompute_validator_totals(validator)?;

        Ok(amount)
    }

    pub fn apply_reward_emission(&mut self, amount: Amount) {
        if self.total_stake == 0 {
            self.pending_rewards = self.pending_rewards.saturating_add(amount);
            return;
        }

        let scaled_amount = amount
            .saturating_add(self.pending_rewards)
            .saturating_mul(REWARD_SCALE);
        let numerator = self.reward_index_residual.saturating_add(scaled_amount);
        self.reward_index += numerator / self.total_stake;
        self.reward_index_residual = numerator % self.total_stake;
        self.pending_rewards = 0;
    }

    pub fn claim_rewards(
        &mut self,
        delegator: &Address,
        validator: &Address,
    ) -> StakingResult<Amount> {
        let key = (delegator.clone(), validator.clone());
        self.settle_position(&key);

        let position = self
            .delegations
            .get_mut(&key)
            .ok_or(StakingError::DelegationNotFound)?;
        let claimed = position.accrued_rewards;
        position.accrued_rewards = 0;
        Ok(claimed)
    }

    pub fn slash_validator(
        &mut self,
        validator: &Address,
        slash_bps: u16,
    ) -> StakingResult<Amount> {
        if slash_bps == 0 || slash_bps > 10_000 {
            return Err(StakingError::InvalidSlashRate);
        }
        if !self.validators.contains_key(validator) {
            return Err(StakingError::ValidatorNotFound);
        }

        let affected: Vec<_> = self
            .delegations
            .keys()
            .filter(|(_, position_validator)| position_validator == validator)
            .cloned()
            .collect();

        let mut total_slashed = 0;
        for key in affected {
            self.settle_position(&key);
            if let Some(position) = self.delegations.get_mut(&key) {
                let slashed = position.stake * slash_bps as u128 / 10_000;
                position.stake -= slashed;
                total_slashed += slashed;
            }
        }

        self.total_stake = self.total_stake.saturating_sub(total_slashed);
        self.recompute_validator_totals(validator)?;

        if let Some(profile) = self.validators.get_mut(validator) {
            profile.slash_count += 1;
            if profile.self_bond < self.config.min_self_bond {
                profile.status = ValidatorStatus::Jailed;
            }
        }

        Ok(total_slashed)
    }

    pub fn validator_voting_power(&self, validator: &Address) -> Amount {
        self.validators
            .get(validator)
            .map(ValidatorProfile::total_stake)
            .unwrap_or(0)
    }

    pub fn validator(&self, validator: &Address) -> Option<&ValidatorProfile> {
        self.validators.get(validator)
    }

    pub fn delegation(
        &self,
        delegator: &Address,
        validator: &Address,
    ) -> Option<&DelegationPosition> {
        self.delegations
            .get(&(delegator.clone(), validator.clone()))
    }

    pub fn stats(&self) -> StakingStats {
        StakingStats {
            validator_count: self.validators.len() as u32,
            delegation_count: self.delegations.len() as u32,
            total_stake: self.total_stake,
            reward_index: self.reward_index,
            pending_rewards: self.pending_rewards,
        }
    }

    fn settle_position(&mut self, key: &(Address, Address)) {
        if let Some(position) = self.delegations.get_mut(key) {
            if position.stake > 0 && self.reward_index > position.reward_index_snapshot {
                let delta = self.reward_index - position.reward_index_snapshot;
                let accrued = position.stake.saturating_mul(delta) / REWARD_SCALE;
                position.accrued_rewards = position.accrued_rewards.saturating_add(accrued);
            }
            position.reward_index_snapshot = self.reward_index;
        }
    }

    fn recompute_validator_totals(&mut self, validator: &Address) -> StakingResult<()> {
        let profile = self
            .validators
            .get_mut(validator)
            .ok_or(StakingError::ValidatorNotFound)?;

        let mut self_bond = 0;
        let mut delegated = 0;
        for ((delegator, position_validator), position) in &self.delegations {
            if position_validator == validator {
                if delegator == validator {
                    self_bond += position.stake;
                } else {
                    delegated += position.stake;
                }
            }
        }

        profile.self_bond = self_bond;
        profile.delegated_stake = delegated;
        Ok(())
    }
}

impl Default for StakingContract {
    fn default() -> Self {
        Self::new(StakingConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reward_index_distribution_is_o1_for_delegators() {
        let mut staking = StakingContract::default();
        staking
            .register_validator("validator1".into(), 1_000_000, 500, None, 1)
            .unwrap();
        staking
            .delegate("alice".into(), "validator1".into(), 500_000)
            .unwrap();
        staking
            .delegate("bob".into(), "validator1".into(), 500_000)
            .unwrap();

        staking.apply_reward_emission(200_000);

        let alice_rewards = staking
            .claim_rewards(&"alice".to_string(), &"validator1".to_string())
            .unwrap();
        let bob_rewards = staking
            .claim_rewards(&"bob".to_string(), &"validator1".to_string())
            .unwrap();

        assert_eq!(alice_rewards, bob_rewards);
        assert!(alice_rewards > 0);
    }

    #[test]
    fn slashing_updates_validator_power() {
        let mut staking = StakingContract::default();
        staking
            .register_validator("validator1".into(), 1_000_000, 500, None, 1)
            .unwrap();
        staking
            .delegate("alice".into(), "validator1".into(), 500_000)
            .unwrap();

        let slashed = staking
            .slash_validator(&"validator1".to_string(), 500)
            .unwrap();
        assert!(slashed > 0);
        assert_eq!(
            staking.validator_voting_power(&"validator1".to_string()),
            1_425_000
        );
    }
}
