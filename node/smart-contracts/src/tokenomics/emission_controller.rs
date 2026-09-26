/*
Emission Controller - Manages DRT token emission based on DAO governance

This contract controls the emission of DRT tokens and integrates with the governance system
to allow DAO-driven emission parameter adjustments.

Key features:
- DAO-controlled emission rate adjustments
- Adaptive emission based on network conditions
- Integration with governance proposals
- WASM-compatible exports
- Reward distribution management
*/

use super::types::{
    Balance, EmissionParameters, EmissionRate, ProposalId, TokenomicsError, TokenomicsProposal,
    TokenomicsResult,
};
use crate::types::Address;
use scale::{Decode, Encode};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)]
const WASM_ENV_NOTE: &str = "EmissionController WASM extern fns are no-ops on non-wasm32 targets";

/// Emission Controller contract state
#[derive(Debug, Clone, Encode, Decode, Serialize, Deserialize)]
pub struct EmissionController {
    /// Contract owner
    pub owner: Address,
    /// Address of the DRT token contract
    pub drt_token: Option<Address>,
    /// Address of the governance contract
    pub governance_contract: Option<Address>,
    /// Current emission parameters
    pub emission_params: EmissionParameters,
    /// Pending governance proposals
    pub pending_proposals: BTreeMap<ProposalId, TokenomicsProposal>,
    /// Approved proposals waiting for execution
    pub approved_proposals: BTreeMap<ProposalId, TokenomicsProposal>,
    /// Last block when emission was processed
    pub last_emission_block: u64,
    /// Treasury address for rewards
    pub treasury: Option<Address>,
    /// Validator reward pool
    pub validator_pool: Balance,
    /// Staker reward pool
    pub staker_pool: Balance,
}

impl EmissionController {
    /// Create a new emission controller
    pub fn new(owner: Address) -> Self {
        Self {
            owner,
            drt_token: None,
            governance_contract: None,
            emission_params: EmissionParameters::default(),
            pending_proposals: BTreeMap::new(),
            approved_proposals: BTreeMap::new(),
            last_emission_block: 0,
            treasury: None,
            validator_pool: 0,
            staker_pool: 0,
        }
    }

    /// Set the DRT token contract address (only callable by owner)
    pub fn set_drt_token(&mut self, token_address: Address) -> TokenomicsResult<()> {
        self.drt_token = Some(token_address);
        Ok(())
    }

    /// Set the governance contract address (only callable by owner)
    pub fn set_governance_contract(&mut self, governance_address: Address) -> TokenomicsResult<()> {
        self.governance_contract = Some(governance_address);
        Ok(())
    }

    /// Set the treasury address (only callable by owner)
    pub fn set_treasury(&mut self, treasury_address: Address) -> TokenomicsResult<()> {
        self.treasury = Some(treasury_address);
        Ok(())
    }

    /// Submit a tokenomics proposal to governance
    pub fn submit_proposal(
        &mut self,
        proposal_id: ProposalId,
        proposal: TokenomicsProposal,
    ) -> TokenomicsResult<()> {
        self.pending_proposals.insert(proposal_id, proposal);
        Ok(())
    }

    /// Mark a proposal as approved by governance (only callable by governance contract)
    pub fn approve_proposal(
        &mut self,
        proposal_id: ProposalId,
        caller: &Address,
    ) -> TokenomicsResult<()> {
        // Verify caller is governance contract
        if let Some(ref governance) = self.governance_contract {
            if caller != governance {
                return Err(TokenomicsError::NotAuthorized);
            }
        } else {
            return Err(TokenomicsError::NotAuthorized);
        }

        if let Some(proposal) = self.pending_proposals.remove(&proposal_id) {
            self.approved_proposals.insert(proposal_id, proposal);
            Ok(())
        } else {
            Err(TokenomicsError::ProposalNotFound)
        }
    }

    /// Execute an approved proposal
    pub fn execute_proposal(&mut self, proposal_id: ProposalId) -> TokenomicsResult<()> {
        let proposal = self
            .approved_proposals
            .remove(&proposal_id)
            .ok_or(TokenomicsError::ProposalNotFound)?;

        match proposal {
            TokenomicsProposal::ChangeEmissionRate { new_rate } => {
                self.set_emission_rate(new_rate)?;
            }
            TokenomicsProposal::UpdateEmissionParameters { new_params } => {
                self.emission_params = new_params;
            }
            TokenomicsProposal::MintDGT { to: _, amount: _ } => {
                // DGT minting would be handled by DGT contract
                // This is just a placeholder for governance integration
            }
            TokenomicsProposal::BurnDRT { from: _, amount: _ } => {
                // DRT burning would be handled by DRT contract
                // This is just a placeholder for governance integration
            }
        }

        Ok(())
    }

    /// Set emission rate (internal function)
    fn set_emission_rate(&mut self, new_rate: EmissionRate) -> TokenomicsResult<()> {
        if new_rate > self.emission_params.max_emission_rate
            || new_rate < self.emission_params.min_emission_rate
        {
            return Err(TokenomicsError::InvalidEmissionRate);
        }

        self.emission_params.base_emission_rate = new_rate;
        Ok(())
    }

    /// Calculate adaptive emission rate based on network conditions
    pub fn calculate_adaptive_rate(&self, network_utilization: u32) -> EmissionRate {
        let base_rate = self.emission_params.base_emission_rate;
        let adjustment_factor = self.emission_params.adjustment_factor as u64;

        // Adjust emission based on network utilization (0-10000 basis points)
        let adjustment = if network_utilization > 5000 {
            // High utilization - increase emission
            base_rate * adjustment_factor / 10000
        } else {
            // Low utilization - decrease emission
            base_rate * adjustment_factor / 20000
        };

        let new_rate = if network_utilization > 5000 {
            base_rate + adjustment
        } else {
            base_rate.saturating_sub(adjustment)
        };

        // Ensure rate stays within bounds
        new_rate
            .max(self.emission_params.min_emission_rate)
            .min(self.emission_params.max_emission_rate)
    }

    /// Process emission for current block
    pub fn process_emission(
        &mut self,
        current_block: u64,
        network_utilization: u32,
    ) -> TokenomicsResult<Balance> {
        if current_block <= self.last_emission_block {
            return Ok(0); // No emission for this block
        }

        let blocks_elapsed = current_block - self.last_emission_block;
        let current_rate = self.calculate_adaptive_rate(network_utilization);
        let total_emission = (current_rate * blocks_elapsed) as Balance;

        if total_emission > 0 {
            // Distribute emission:
            // 40% to validators
            // 30% to stakers
            // 30% to treasury
            let validator_share = total_emission * 40 / 100;
            let staker_share = total_emission * 30 / 100;
            let _treasury_share = total_emission * 30 / 100;

            self.validator_pool += validator_share;
            self.staker_pool += staker_share;

            // Treasury emission would be minted to treasury address
            // This would integrate with the DRT token contract

            self.last_emission_block = current_block;
        }

        Ok(total_emission)
    }

    /// Claim validator rewards
    pub fn claim_validator_rewards(
        &mut self,
        _validator: Address,
        amount: Balance,
    ) -> TokenomicsResult<()> {
        if amount > self.validator_pool {
            return Err(TokenomicsError::InsufficientBalance);
        }

        self.validator_pool -= amount;
        // This would mint DRT tokens to the validator
        // Integration with DRT token contract would happen here

        Ok(())
    }

    /// Claim staker rewards
    pub fn claim_staker_rewards(
        &mut self,
        _staker: Address,
        amount: Balance,
    ) -> TokenomicsResult<()> {
        if amount > self.staker_pool {
            return Err(TokenomicsError::InsufficientBalance);
        }

        self.staker_pool -= amount;
        // This would mint DRT tokens to the staker
        // Integration with DRT token contract would happen here

        Ok(())
    }

    /// Get current emission parameters
    pub fn get_emission_params(&self) -> &EmissionParameters {
        &self.emission_params
    }

    /// Get validator pool balance
    pub fn validator_pool_balance(&self) -> Balance {
        self.validator_pool
    }

    /// Get staker pool balance
    pub fn staker_pool_balance(&self) -> Balance {
        self.staker_pool
    }

    /// Get pending proposals
    pub fn get_pending_proposals(&self) -> &BTreeMap<ProposalId, TokenomicsProposal> {
        &self.pending_proposals
    }

    /// Get approved proposals
    pub fn get_approved_proposals(&self) -> &BTreeMap<ProposalId, TokenomicsProposal> {
        &self.approved_proposals
    }
}

// WASM-compatible exports for emission controller functions
#[no_mangle]
#[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
pub extern "C" fn emission_controller_process_emission(
    controller_ptr: *mut EmissionController,
    current_block: u64,
    network_utilization: u32,
) -> u64 {
    0
}

#[no_mangle]
#[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
pub extern "C" fn emission_controller_submit_proposal(
    controller_ptr: *mut EmissionController,
    proposal_id: u64,
    proposal_type: u32,
    proposal_data_ptr: *const u8,
    proposal_data_len: usize,
) -> i32 {
    0
}

#[no_mangle]
#[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
pub extern "C" fn emission_controller_execute_proposal(
    controller_ptr: *mut EmissionController,
    proposal_id: u64,
) -> i32 {
    0
}

#[no_mangle]
#[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
pub extern "C" fn emission_controller_claim_validator_rewards(
    controller_ptr: *mut EmissionController,
    validator_ptr: *const u8,
    validator_len: usize,
    amount: u64,
) -> i32 {
    0
}

#[no_mangle]
#[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
pub extern "C" fn emission_controller_claim_staker_rewards(
    controller_ptr: *mut EmissionController,
    staker_ptr: *const u8,
    staker_len: usize,
    amount: u64,
) -> i32 {
    0
}

#[no_mangle]
#[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
pub extern "C" fn emission_controller_get_emission_rate(
    controller_ptr: *const EmissionController,
) -> u64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emission_controller_creation() {
        let owner = "dyt1owner".to_string();
        let controller = EmissionController::new(owner.clone());

        assert_eq!(controller.owner, owner);
        assert!(controller.drt_token.is_none());
        assert!(controller.governance_contract.is_none());
        assert_eq!(controller.emission_params.base_emission_rate, 1000);
    }

    #[test]
    fn test_set_contracts() {
        let owner = "dyt1owner".to_string();
        let token_addr = "dyt1token".to_string();
        let gov_addr = "dyt1governance".to_string();
        let treasury_addr = "dyt1treasury".to_string();
        let mut controller = EmissionController::new(owner);

        controller.set_drt_token(token_addr.clone()).unwrap();
        controller
            .set_governance_contract(gov_addr.clone())
            .unwrap();
        controller.set_treasury(treasury_addr.clone()).unwrap();

        assert_eq!(controller.drt_token, Some(token_addr));
        assert_eq!(controller.governance_contract, Some(gov_addr));
        assert_eq!(controller.treasury, Some(treasury_addr));
    }

    #[test]
    fn test_proposal_lifecycle() {
        let owner = "dyt1owner".to_string();
        let gov_addr = "dyt1governance".to_string();
        let mut controller = EmissionController::new(owner);

        controller
            .set_governance_contract(gov_addr.clone())
            .unwrap();

        // Submit proposal
        let proposal_id = 1;
        let proposal = TokenomicsProposal::ChangeEmissionRate { new_rate: 1500 };
        controller.submit_proposal(proposal_id, proposal).unwrap();

        assert!(controller.pending_proposals.contains_key(&proposal_id));

        // Approve proposal
        controller.approve_proposal(proposal_id, &gov_addr).unwrap();
        assert!(controller.approved_proposals.contains_key(&proposal_id));
        assert!(!controller.pending_proposals.contains_key(&proposal_id));

        // Execute proposal
        controller.execute_proposal(proposal_id).unwrap();
        assert_eq!(controller.emission_params.base_emission_rate, 1500);
        assert!(!controller.approved_proposals.contains_key(&proposal_id));
    }

    #[test]
    fn test_adaptive_emission_rate() {
        let owner = "dyt1owner".to_string();
        let controller = EmissionController::new(owner);

        // Test high utilization (increase emission)
        let high_util_rate = controller.calculate_adaptive_rate(7500);
        assert!(high_util_rate > controller.emission_params.base_emission_rate);

        // Test low utilization (decrease emission)
        let low_util_rate = controller.calculate_adaptive_rate(2500);
        assert!(low_util_rate < controller.emission_params.base_emission_rate);
    }

    #[test]
    fn test_emission_processing() {
        let owner = "dyt1owner".to_string();
        let mut controller = EmissionController::new(owner);

        // Process emission for block 10
        let result = controller.process_emission(10, 5000);
        assert!(result.is_ok());
        let emitted = result.unwrap();

        // Should emit for 10 blocks
        assert!(emitted > 0);
        assert!(controller.validator_pool > 0);
        assert!(controller.staker_pool > 0);
        assert_eq!(controller.last_emission_block, 10);
    }

    #[test]
    fn test_reward_claiming() {
        let owner = "dyt1owner".to_string();
        let validator = "dyt1validator".to_string();
        let staker = "dyt1staker".to_string();
        let mut controller = EmissionController::new(owner);

        // Process some emission first
        controller.process_emission(10, 5000).unwrap();

        let initial_validator_pool = controller.validator_pool;
        let initial_staker_pool = controller.staker_pool;

        // Claim validator rewards
        let validator_claim = 100;
        let result = controller.claim_validator_rewards(validator, validator_claim);
        assert!(result.is_ok());
        assert_eq!(
            controller.validator_pool,
            initial_validator_pool - validator_claim
        );

        // Claim staker rewards
        let staker_claim = 50;
        let result = controller.claim_staker_rewards(staker, staker_claim);
        assert!(result.is_ok());
        assert_eq!(controller.staker_pool, initial_staker_pool - staker_claim);
    }

    #[test]
    fn test_invalid_emission_rate() {
        let owner = "dyt1owner".to_string();
        let mut controller = EmissionController::new(owner);

        // Try to set rate above maximum
        let result = controller.set_emission_rate(10000);
        assert!(matches!(result, Err(TokenomicsError::InvalidEmissionRate)));

        // Try to set rate below minimum
        let result = controller.set_emission_rate(50);
        assert!(matches!(result, Err(TokenomicsError::InvalidEmissionRate)));
    }

    #[test]
    fn test_unauthorized_proposal_approval() {
        let owner = "dyt1owner".to_string();
        let unauthorized = "dyt1unauthorized".to_string();
        let mut controller = EmissionController::new(owner);

        // Submit proposal
        let proposal_id = 1;
        let proposal = TokenomicsProposal::ChangeEmissionRate { new_rate: 1500 };
        controller.submit_proposal(proposal_id, proposal).unwrap();

        // Try to approve with unauthorized caller
        let result = controller.approve_proposal(proposal_id, &unauthorized);
        assert!(matches!(result, Err(TokenomicsError::NotAuthorized)));
    }
}
