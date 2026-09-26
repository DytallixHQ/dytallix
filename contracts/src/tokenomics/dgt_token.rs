/*
DGT (Dytallix Governance Token) - Fixed supply governance token

This token has a fixed supply and is used for governance voting.
Key features:
- Fixed total supply
- Transfer functionality
- Governance voting weight based on balance
- WASM-compatible exports
*/

use super::types::{Balance, TokenomicsError, TokenomicsResult};
use crate::types::Address;
use scale::{Decode, Encode};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)]
const WASM_ENV_NOTE: &str = "DGTToken WASM extern fns are no-ops on non-wasm32 targets";

/// DGT Token contract state
#[derive(Debug, Clone, Encode, Decode, Serialize, Deserialize)]
pub struct DGTToken {
    /// Total supply of DGT tokens
    pub total_supply: Balance,
    /// Token balances for each address
    pub balances: BTreeMap<Address, Balance>,
    /// Token allowances (owner -> spender -> amount)
    pub allowances: BTreeMap<Address, BTreeMap<Address, Balance>>,
    /// Contract owner (can mint initial supply)
    pub owner: Address,
    /// Whether initial minting is complete
    pub initial_mint_complete: bool,
}

impl DGTToken {
    /// Create a new DGT token with zero supply
    pub fn new(owner: Address) -> Self {
        Self {
            total_supply: 0,
            balances: BTreeMap::new(),
            allowances: BTreeMap::new(),
            owner,
            initial_mint_complete: false,
        }
    }

    /// Mint initial supply (only callable by owner, only once)
    pub fn mint_initial_supply(&mut self, to: Address, amount: Balance) -> TokenomicsResult<()> {
        if self.initial_mint_complete {
            return Err(TokenomicsError::NotAuthorized);
        }

        self.balances.insert(to.clone(), amount);
        self.total_supply = amount;
        self.initial_mint_complete = true;

        Ok(())
    }

    /// Get balance of an address
    pub fn balance_of(&self, address: &Address) -> Balance {
        self.balances.get(address).copied().unwrap_or(0)
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

    /// Get voting power (same as balance for DGT)
    pub fn voting_power(&self, address: &Address) -> Balance {
        self.balance_of(address)
    }
}

// WASM-compatible exports for DGT token functions
#[no_mangle]
#[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
pub extern "C" fn dgt_balance_of(
    token_ptr: *const DGTToken,
    address_ptr: *const u8,
    address_len: usize,
) -> u64 {
    0
}

#[no_mangle]
#[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
pub extern "C" fn dgt_transfer(
    token_ptr: *mut DGTToken,
    from_ptr: *const u8,
    from_len: usize,
    to_ptr: *const u8,
    to_len: usize,
    amount: u64,
) -> i32 {
    0
}

#[no_mangle]
#[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
pub extern "C" fn dgt_approve(
    token_ptr: *mut DGTToken,
    owner_ptr: *const u8,
    owner_len: usize,
    spender_ptr: *const u8,
    spender_len: usize,
    amount: u64,
) -> i32 {
    0
}

#[no_mangle]
#[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
pub extern "C" fn dgt_total_supply(token_ptr: *const DGTToken) -> u64 {
    0
}

#[no_mangle]
#[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
pub extern "C" fn dgt_voting_power(
    token_ptr: *const DGTToken,
    address_ptr: *const u8,
    address_len: usize,
) -> u64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dgt_creation() {
        let owner = "dyt1owner".to_string();
        let token = DGTToken::new(owner.clone());

        assert_eq!(token.total_supply, 0);
        assert_eq!(token.owner, owner);
        assert!(!token.initial_mint_complete);
    }

    #[test]
    fn test_dgt_initial_mint() {
        let owner = "dyt1owner".to_string();
        let recipient = "dyt1recipient".to_string();
        let mut token = DGTToken::new(owner.clone());

        let amount = 1_000_000;
        let result = token.mint_initial_supply(recipient.clone(), amount);
        assert!(result.is_ok());

        assert_eq!(token.total_supply(), amount);
        assert_eq!(token.balance_of(&recipient), amount);
        assert!(token.initial_mint_complete);
    }

    #[test]
    fn test_dgt_transfer() {
        let owner = "dyt1owner".to_string();
        let from = "dyt1from".to_string();
        let to = "dyt1to".to_string();
        let mut token = DGTToken::new(owner);

        // Mint initial supply to 'from'
        token.mint_initial_supply(from.clone(), 1000).unwrap();

        // Transfer tokens
        let result = token.transfer(from.clone(), to.clone(), 300);
        assert!(result.is_ok());

        assert_eq!(token.balance_of(&from), 700);
        assert_eq!(token.balance_of(&to), 300);
    }

    #[test]
    fn test_dgt_insufficient_balance() {
        let owner = "dyt1owner".to_string();
        let from = "dyt1from".to_string();
        let to = "dyt1to".to_string();
        let mut token = DGTToken::new(owner);

        // Try to transfer without balance
        let result = token.transfer(from, to, 100);
        assert!(matches!(result, Err(TokenomicsError::InsufficientBalance)));
    }

    #[test]
    fn test_dgt_allowance_system() {
        let owner = "dyt1owner".to_string();
        let spender = "dyt1spender".to_string();
        let recipient = "dyt1recipient".to_string();
        let mut token = DGTToken::new(owner.clone());

        // Mint initial supply
        token.mint_initial_supply(owner.clone(), 1000).unwrap();

        // Approve allowance
        token.approve(owner.clone(), spender.clone(), 200).unwrap();
        assert_eq!(token.allowance(&owner, &spender), 200);

        // Transfer from allowance
        let result = token.transfer_from(owner.clone(), spender.clone(), recipient.clone(), 150);
        assert!(result.is_ok());

        assert_eq!(token.balance_of(&owner), 850);
        assert_eq!(token.balance_of(&recipient), 150);
        assert_eq!(token.allowance(&owner, &spender), 50);
    }

    #[test]
    fn test_dgt_voting_power() {
        let owner = "dyt1owner".to_string();
        let voter = "dyt1voter".to_string();
        let mut token = DGTToken::new(owner);

        // Mint tokens
        token.mint_initial_supply(voter.clone(), 500).unwrap();

        // Voting power should equal balance
        assert_eq!(token.voting_power(&voter), 500);
    }
}
