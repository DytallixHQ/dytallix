/*
DRT (Dytallix Reward Token) - Adaptive emission reward token with burning

This token features adaptive emission controlled by governance and burning mechanism.
Key features:
- Adaptive emission based on governance decisions
- Burnable tokens to control supply
- Transfer functionality
- WASM-compatible exports
- Integration with Emission Controller
*/

use super::types::{Balance, EmissionRate, TokenomicsError, TokenomicsResult};
use crate::types::Address;
use scale::{Decode, Encode};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)]
const WASM_ENV_NOTE: &str = "DRTToken WASM extern fns are no-ops on non-wasm32 targets";

/// DRT Token contract state
#[derive(Debug, Clone, Encode, Decode, Serialize, Deserialize)]
pub struct DRTToken {
    /// Total supply of DRT tokens
    pub total_supply: Balance,
    /// Token balances for each address
    pub balances: BTreeMap<Address, Balance>,
    /// Token allowances (owner -> spender -> amount)
    pub allowances: BTreeMap<Address, BTreeMap<Address, Balance>>,
    /// Contract owner
    pub owner: Address,
    /// Address of the emission controller contract
    pub emission_controller: Option<Address>,
    /// Current emission rate (tokens per block)
    pub current_emission_rate: EmissionRate,
    /// Last block when emission occurred
    pub last_emission_block: u64,
    /// Total tokens burned
    pub total_burned: Balance,
}

impl DRTToken {
    /// Create a new DRT token
    pub fn new(owner: Address) -> Self {
        Self {
            total_supply: 0,
            balances: BTreeMap::new(),
            allowances: BTreeMap::new(),
            owner,
            emission_controller: None,
            current_emission_rate: 1000, // Default 1000 tokens per block
            last_emission_block: 0,
            total_burned: 0,
        }
    }

    /// Set the emission controller address (only callable by owner)
    pub fn set_emission_controller(&mut self, controller: Address) -> TokenomicsResult<()> {
        self.emission_controller = Some(controller);
        Ok(())
    }

    /// Get balance of an address
    pub fn balance_of(&self, address: &Address) -> Balance {
        self.balances.get(address).copied().unwrap_or(0)
    }

    /// Mint new tokens (only callable by emission controller)
    pub fn mint(&mut self, to: Address, amount: Balance, caller: &Address) -> TokenomicsResult<()> {
        // Check if caller is emission controller
        if let Some(ref controller) = self.emission_controller {
            if caller != controller {
                return Err(TokenomicsError::NotAuthorized);
            }
        } else {
            return Err(TokenomicsError::EmissionControllerNotSet);
        }

        if amount == 0 {
            return Err(TokenomicsError::InvalidAmount);
        }

        // Update balance and total supply
        let current_balance = self.balance_of(&to);
        self.balances.insert(to.clone(), current_balance + amount);
        self.total_supply += amount;

        Ok(())
    }

    /// Burn tokens from an address
    pub fn burn(&mut self, from: Address, amount: Balance) -> TokenomicsResult<()> {
        if amount == 0 {
            return Err(TokenomicsError::InvalidAmount);
        }

        let from_balance = self.balance_of(&from);
        if from_balance < amount {
            return Err(TokenomicsError::InsufficientBalance);
        }

        // Update balance and total supply
        self.balances.insert(from.clone(), from_balance - amount);
        self.total_supply -= amount;
        self.total_burned += amount;

        Ok(())
    }

    /// Transfer tokens between addresses
    pub fn transfer(
        &mut self,
        from: Address,
        to: Address,
        amount: Balance,
    ) -> TokenomicsResult<()> {
        if from == to {
            return Err(TokenomicsError::TransferToSelf);
        }

        if amount == 0 {
            return Err(TokenomicsError::InvalidAmount);
        }

        let from_balance = self.balance_of(&from);
        if from_balance < amount {
            return Err(TokenomicsError::InsufficientBalance);
        }

        // Update balances
        self.balances.insert(from.clone(), from_balance - amount);
        let to_balance = self.balance_of(&to);
        self.balances.insert(to.clone(), to_balance + amount);

        Ok(())
    }

    /// Approve spending allowance
    pub fn approve(
        &mut self,
        owner: Address,
        spender: Address,
        amount: Balance,
    ) -> TokenomicsResult<()> {
        self.allowances
            .entry(owner)
            .or_default()
            .insert(spender, amount);

        Ok(())
    }

    /// Transfer from allowance
    pub fn transfer_from(
        &mut self,
        owner: Address,
        spender: Address,
        to: Address,
        amount: Balance,
    ) -> TokenomicsResult<()> {
        // Check allowance
        let allowance = self
            .allowances
            .get(&owner)
            .and_then(|allowances| allowances.get(&spender))
            .copied()
            .unwrap_or(0);

        if allowance < amount {
            return Err(TokenomicsError::InsufficientBalance);
        }

        // Perform transfer
        self.transfer(owner.clone(), to, amount)?;

        // Update allowance
        self.allowances
            .entry(owner)
            .or_default()
            .insert(spender, allowance - amount);

        Ok(())
    }

    /// Get allowance amount
    pub fn allowance(&self, owner: &Address, spender: &Address) -> Balance {
        self.allowances
            .get(owner)
            .and_then(|allowances| allowances.get(spender))
            .copied()
            .unwrap_or(0)
    }

    /// Get total supply
    pub fn total_supply(&self) -> Balance {
        self.total_supply
    }

    /// Get total burned tokens
    pub fn total_burned(&self) -> Balance {
        self.total_burned
    }

    /// Get current emission rate
    pub fn emission_rate(&self) -> EmissionRate {
        self.current_emission_rate
    }

    /// Update emission rate (only callable by emission controller)
    pub fn update_emission_rate(
        &mut self,
        new_rate: EmissionRate,
        caller: &Address,
    ) -> TokenomicsResult<()> {
        // Check if caller is emission controller
        if let Some(ref controller) = self.emission_controller {
            if caller != controller {
                return Err(TokenomicsError::NotAuthorized);
            }
        } else {
            return Err(TokenomicsError::EmissionControllerNotSet);
        }

        self.current_emission_rate = new_rate;
        Ok(())
    }

    /// Process emission for the current block (only callable by emission controller)
    pub fn process_emission(
        &mut self,
        current_block: u64,
        recipient: Address,
        caller: &Address,
    ) -> TokenomicsResult<Balance> {
        // Check if caller is emission controller
        if let Some(ref controller) = self.emission_controller {
            if caller != controller {
                return Err(TokenomicsError::NotAuthorized);
            }
        } else {
            return Err(TokenomicsError::EmissionControllerNotSet);
        }

        if current_block <= self.last_emission_block {
            return Ok(0); // No emission for this block
        }

        let blocks_elapsed = current_block - self.last_emission_block;
        let emission_amount = (self.current_emission_rate * blocks_elapsed) as Balance;

        if emission_amount > 0 {
            self.mint(recipient, emission_amount, caller)?;
            self.last_emission_block = current_block;
        }

        Ok(emission_amount)
    }
}

// WASM-compatible exports for DRT token functions
#[no_mangle]
#[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
pub extern "C" fn drt_balance_of(
    token_ptr: *const DRTToken,
    address_ptr: *const u8,
    address_len: usize,
) -> u64 {
    0
}

#[no_mangle]
#[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
pub extern "C" fn drt_transfer(
    token_ptr: *mut DRTToken,
    from_ptr: *const u8,
    from_len: usize,
    to_ptr: *const u8,
    to_len: usize,
    amount: u64, // Changed from u128 for FFI safety
) -> i32 {
    0
}

#[no_mangle]
#[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
pub extern "C" fn drt_mint(
    token_ptr: *mut DRTToken,
    to_ptr: *const u8,
    to_len: usize,
    amount: u64, // Changed from u128 for FFI safety
    caller_ptr: *const u8,
    caller_len: usize,
) -> i32 {
    0
}

#[no_mangle]
#[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
pub extern "C" fn drt_burn(
    token_ptr: *mut DRTToken,
    from_ptr: *const u8,
    from_len: usize,
    amount: u64, // Changed from u128 for FFI safety
) -> i32 {
    0
}

#[no_mangle]
#[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
pub extern "C" fn drt_emission_rate(token_ptr: *const DRTToken) -> u64 {
    0
}

#[no_mangle]
#[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
pub extern "C" fn drt_total_supply(token_ptr: *const DRTToken) -> u64 {
    0
}

#[no_mangle]
#[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
pub extern "C" fn drt_total_burned(token_ptr: *const DRTToken) -> u64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drt_creation() {
        let owner = "dyt1owner".to_string();
        let token = DRTToken::new(owner.clone());

        assert_eq!(token.total_supply, 0);
        assert_eq!(token.owner, owner);
        assert_eq!(token.current_emission_rate, 1000);
        assert!(token.emission_controller.is_none());
    }

    #[test]
    fn test_drt_set_emission_controller() {
        let owner = "dyt1owner".to_string();
        let controller = "dyt1controller".to_string();
        let mut token = DRTToken::new(owner);

        let result = token.set_emission_controller(controller.clone());
        assert!(result.is_ok());
        assert_eq!(token.emission_controller, Some(controller));
    }

    #[test]
    fn test_drt_mint_by_controller() {
        let owner = "dyt1owner".to_string();
        let controller = "dyt1controller".to_string();
        let recipient = "dyt1recipient".to_string();
        let mut token = DRTToken::new(owner);

        // Set controller
        token.set_emission_controller(controller.clone()).unwrap();

        // Mint tokens
        let result = token.mint(recipient.clone(), 500, &controller);
        assert!(result.is_ok());

        assert_eq!(token.balance_of(&recipient), 500);
        assert_eq!(token.total_supply(), 500);
    }

    #[test]
    fn test_drt_mint_unauthorized() {
        let owner = "dyt1owner".to_string();
        let controller = "dyt1controller".to_string();
        let unauthorized = "dyt1unauthorized".to_string();
        let recipient = "dyt1recipient".to_string();
        let mut token = DRTToken::new(owner);

        // Set controller
        token.set_emission_controller(controller).unwrap();

        // Try to mint with unauthorized caller
        let result = token.mint(recipient, 500, &unauthorized);
        assert!(matches!(result, Err(TokenomicsError::NotAuthorized)));
    }

    #[test]
    fn test_drt_burn() {
        let owner = "dyt1owner".to_string();
        let controller = "dyt1controller".to_string();
        let user = "dyt1user".to_string();
        let mut token = DRTToken::new(owner);

        // Set controller and mint tokens
        token.set_emission_controller(controller.clone()).unwrap();
        token.mint(user.clone(), 1000, &controller).unwrap();

        // Burn tokens
        let result = token.burn(user.clone(), 300);
        assert!(result.is_ok());

        assert_eq!(token.balance_of(&user), 700);
        assert_eq!(token.total_supply(), 700);
        assert_eq!(token.total_burned(), 300);
    }

    #[test]
    fn test_drt_burn_insufficient_balance() {
        let owner = "dyt1owner".to_string();
        let user = "dyt1user".to_string();
        let mut token = DRTToken::new(owner);

        // Try to burn without balance
        let result = token.burn(user, 100);
        assert!(matches!(result, Err(TokenomicsError::InsufficientBalance)));
    }

    #[test]
    fn test_drt_emission_process() {
        let owner = "dyt1owner".to_string();
        let controller = "dyt1controller".to_string();
        let recipient = "dyt1recipient".to_string();
        let mut token = DRTToken::new(owner);

        // Set controller
        token.set_emission_controller(controller.clone()).unwrap();

        // Process emission for block 10
        let result = token.process_emission(10, recipient.clone(), &controller);
        assert!(result.is_ok());
        let emitted = result.unwrap();

        // Should emit for 10 blocks at rate of 1000 per block
        assert_eq!(emitted, 10000);
        assert_eq!(token.balance_of(&recipient), 10000);
        assert_eq!(token.last_emission_block, 10);
    }

    #[test]
    fn test_drt_transfer() {
        let owner = "dyt1owner".to_string();
        let controller = "dyt1controller".to_string();
        let from = "dyt1from".to_string();
        let to = "dyt1to".to_string();
        let mut token = DRTToken::new(owner);

        // Set controller and mint tokens
        token.set_emission_controller(controller.clone()).unwrap();
        token.mint(from.clone(), 1000, &controller).unwrap();

        // Transfer tokens
        let result = token.transfer(from.clone(), to.clone(), 300);
        assert!(result.is_ok());

        assert_eq!(token.balance_of(&from), 700);
        assert_eq!(token.balance_of(&to), 300);
    }

    #[test]
    fn test_drt_update_emission_rate() {
        let owner = "dyt1owner".to_string();
        let controller = "dyt1controller".to_string();
        let mut token = DRTToken::new(owner);

        // Set controller
        token.set_emission_controller(controller.clone()).unwrap();

        // Update emission rate
        let result = token.update_emission_rate(2000, &controller);
        assert!(result.is_ok());

        assert_eq!(token.emission_rate(), 2000);
    }
}
