//! Cosmos (Osmosis) Bridge Module for Token Minting/Burning
//!
//! This module implements the Cosmos side of the cross-chain bridge,
//! handling token minting/burning with IBC integration and AI-enhanced validation.

use cosmwasm_std::{
    entry_point, to_json_binary, Addr, BankMsg, Binary, Coin, CosmosMsg, Deps, DepsMut, Env,
    MessageInfo, Response, StdError, StdResult, Uint128,
};

// When both feature flags are enabled (e.g. during `--all-features` clippy runs),
// we allow both modules to compile; runtime selection happens externally.
// Removed previous compile_error! to enable linting with all features.

use cw_storage_plus::{Item, Map};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Contract error types
#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),
    #[error("Unauthorized")]
    Unauthorized,
    #[error("Invalid bridge transaction")]
    InvalidBridgeTransaction,
    #[error("Bridge transaction already processed")]
    AlreadyProcessed,
    #[error("Insufficient balance")]
    InsufficientBalance,
    #[error("Token not supported")]
    TokenNotSupported,
    #[error("AI fraud detection triggered")]
    AIFraudDetected,
    #[error("Invalid IBC packet")]
    InvalidIBCPacket,
    #[error("Bridge paused")]
    BridgePaused,
    #[error("Amount below minimum")]
    AmountBelowMinimum,
    #[error("Amount above maximum")]
    AmountAboveMaximum,
}

/// Contract state
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct State {
    pub admin: Addr,
    pub ethereum_channel: String,
    pub validators: Vec<Addr>,
    pub min_validators: u32,
    pub bridge_fee: Uint128,
    pub min_bridge_amount: Uint128,
    pub max_bridge_amount: Uint128,
    pub is_paused: bool,
    pub ai_oracle: Addr,
    pub ai_confidence_threshold: u8,
}

/// Bridge transaction status
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum BridgeStatus {
    Pending,
    Minted,
    Burned,
    Completed,
    Failed,
    Expired,
}

/// Bridge transaction details
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BridgeTransaction {
    pub bridge_id: String,
    pub ethereum_tx_hash: String,
    pub token_denom: String,
    pub amount: Uint128,
    pub recipient: Addr,
    pub ethereum_sender: String,
    pub timestamp: u64,
    pub status: BridgeStatus,
    pub validator_confirmations: u32,
    pub ai_risk_score: u8,
}

/// Supported token configuration
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TokenConfig {
    pub denom: String,
    pub ethereum_address: String,
    pub decimals: u8,
    pub is_active: bool,
    pub mint_cap: Option<Uint128>,
    pub total_minted: Uint128,
    pub total_burned: Uint128,
}

/// IBC packet data for cross-chain communication
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct IBCBridgePacket {
    pub action: IBCAction,
    pub bridge_id: String,
    pub token_denom: String,
    pub amount: Uint128,
    pub recipient: String,
    pub sender: String,
    pub timestamp: u64,
}

/// IBC action types
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum IBCAction {
    Mint,
    Burn,
    Verify,
    Acknowledge,
}

/// Contract execute messages
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    /// Mint tokens after Ethereum lock confirmation
    MintTokens {
        bridge_id: String,
        ethereum_tx_hash: String,
        token_denom: String,
        amount: Uint128,
        recipient: String,
        ethereum_sender: String,
    },

    /// Burn tokens for Ethereum unlock
    BurnTokens {
        bridge_id: String,
        token_denom: String,
        amount: Uint128,
        ethereum_recipient: String,
    },

    /// Validator confirmation for bridge transaction
    ConfirmBridge {
        bridge_id: String,
        signature: String,
    },

    /// Complete bridge transaction
    CompleteBridge { bridge_id: String, success: bool },

    /// Add supported token
    AddSupportedToken {
        denom: String,
        ethereum_address: String,
        decimals: u8,
        mint_cap: Option<Uint128>,
    },

    /// Remove supported token
    RemoveSupportedToken { denom: String },

    /// Add validator
    AddValidator { validator: String },

    /// Remove validator
    RemoveValidator { validator: String },

    /// Update AI risk score
    UpdateAIRiskScore { bridge_id: String, risk_score: u8 },

    /// Pause bridge operations
    Pause {},

    /// Unpause bridge operations
    Unpause {},

    /// Update bridge parameters
    UpdateBridgeParams {
        bridge_fee: Option<Uint128>,
        min_bridge_amount: Option<Uint128>,
        max_bridge_amount: Option<Uint128>,
        min_validators: Option<u32>,
    },

    /// Emergency burn recovery
    EmergencyRecovery { bridge_id: String, reason: String },
}

/// Contract query messages
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    /// Get contract state
    GetState {},

    /// Get bridge transaction
    GetBridgeTransaction { bridge_id: String },

    /// Get supported token
    GetSupportedToken { denom: String },

    /// List all supported tokens
    ListSupportedTokens {},

    /// Get validator list
    GetValidators {},

    /// Get bridge statistics
    GetBridgeStats {},

    /// Get AI risk assessment
    GetAIRiskScore { bridge_id: String },
}

/// Contract instantiation message
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct InstantiateMsg {
    pub admin: String,
    pub ethereum_channel: String,
    pub validators: Vec<String>,
    pub min_validators: u32,
    pub bridge_fee: Uint128,
    pub ai_oracle: String,
}

/// Storage items
const STATE: Item<State> = Item::new("state");
const BRIDGE_TRANSACTIONS: Map<&str, BridgeTransaction> = Map::new("bridge_transactions");
const SUPPORTED_TOKENS: Map<&str, TokenConfig> = Map::new("supported_tokens");
const VALIDATOR_CONFIRMATIONS: Map<(&str, &str), bool> = Map::new("validator_confirmations");

/// Contract instantiation
#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let admin = deps.api.addr_validate(&msg.admin)?;
    let ai_oracle = deps.api.addr_validate(&msg.ai_oracle)?;

    let validators: Result<Vec<Addr>, _> = msg
        .validators
        .iter()
        .map(|v| deps.api.addr_validate(v))
        .collect();
    let validators = validators?;

    let state = State {
        admin,
        ethereum_channel: msg.ethereum_channel,
        validators,
        min_validators: msg.min_validators,
        bridge_fee: msg.bridge_fee,
        min_bridge_amount: Uint128::from(1000u128), // Default minimum
        max_bridge_amount: Uint128::from(1000000000u128), // Default maximum
        is_paused: false,
        ai_oracle,
        ai_confidence_threshold: 80,
    };

    STATE.save(deps.storage, &state)?;

    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("admin", info.sender))
}

/// Contract execution entry point
#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::MintTokens {
            bridge_id,
            ethereum_tx_hash,
            token_denom,
            amount,
            recipient,
            ethereum_sender,
        } => execute_mint_tokens(
            deps,
            env,
            info,
            bridge_id,
            ethereum_tx_hash,
            token_denom,
            amount,
            recipient,
            ethereum_sender,
        ),
        ExecuteMsg::BurnTokens {
            bridge_id,
            token_denom,
            amount,
            ethereum_recipient,
        } => execute_burn_tokens(
            deps,
            env,
            info,
            bridge_id,
            token_denom,
            amount,
            ethereum_recipient,
        ),
        ExecuteMsg::ConfirmBridge {
            bridge_id,
            signature,
        } => execute_confirm_bridge(deps, env, info, bridge_id, signature),
        ExecuteMsg::CompleteBridge { bridge_id, success } => {
            execute_complete_bridge(deps, env, info, bridge_id, success)
        }
        ExecuteMsg::AddSupportedToken {
            denom,
            ethereum_address,
            decimals,
            mint_cap,
        } => execute_add_supported_token(
            deps,
            env,
            info,
            denom,
            ethereum_address,
            decimals,
            mint_cap,
        ),
        ExecuteMsg::RemoveSupportedToken { denom } => {
            execute_remove_supported_token(deps, env, info, denom)
        }
        ExecuteMsg::AddValidator { validator } => execute_add_validator(deps, env, info, validator),
        ExecuteMsg::RemoveValidator { validator } => {
            execute_remove_validator(deps, env, info, validator)
        }
        ExecuteMsg::UpdateAIRiskScore {
            bridge_id,
            risk_score,
        } => execute_update_ai_risk_score(deps, env, info, bridge_id, risk_score),
        ExecuteMsg::Pause {} => execute_pause(deps, env, info),
        ExecuteMsg::Unpause {} => execute_unpause(deps, env, info),
        ExecuteMsg::UpdateBridgeParams {
            bridge_fee,
            min_bridge_amount,
            max_bridge_amount,
            min_validators,
        } => execute_update_bridge_params(
            deps,
            env,
            info,
            bridge_fee,
            min_bridge_amount,
            max_bridge_amount,
            min_validators,
        ),
        ExecuteMsg::EmergencyRecovery { bridge_id, reason } => {
            execute_emergency_recovery(deps, env, info, bridge_id, reason)
        }
    }
}

/// Mint tokens after Ethereum lock confirmation
// Allow many arguments as this function serves as an external entry point
// that needs to handle all bridge-related parameters
#[allow(clippy::too_many_arguments)]
pub fn execute_mint_tokens(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    bridge_id: String,
    ethereum_tx_hash: String,
    token_denom: String,
    amount: Uint128,
    recipient: String,
    ethereum_sender: String,
) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;

    // Check if bridge is paused
    if state.is_paused {
        return Err(ContractError::BridgePaused {});
    }

    // Validate sender is a validator
    if !state.validators.contains(&info.sender) {
        return Err(ContractError::Unauthorized {});
    }

    // Check if token is supported
    let token_config = SUPPORTED_TOKENS
        .may_load(deps.storage, &token_denom)?
        .ok_or(ContractError::TokenNotSupported {})?;

    if !token_config.is_active {
        return Err(ContractError::TokenNotSupported {});
    }

    // Validate amount
    if amount < state.min_bridge_amount {
        return Err(ContractError::AmountBelowMinimum {});
    }
    if amount > state.max_bridge_amount {
        return Err(ContractError::AmountAboveMaximum {});
    }

    // Check mint cap
    if let Some(mint_cap) = token_config.mint_cap {
        if token_config.total_minted + amount > mint_cap {
            return Err(ContractError::InsufficientBalance {});
        }
    }

    let recipient_addr = deps.api.addr_validate(&recipient)?;

    // Check if bridge transaction already exists
    if BRIDGE_TRANSACTIONS.has(deps.storage, &bridge_id) {
        return Err(ContractError::AlreadyProcessed {});
    }

    // Create bridge transaction
    let bridge_tx = BridgeTransaction {
        bridge_id: bridge_id.clone(),
        ethereum_tx_hash,
        token_denom: token_denom.clone(),
        amount,
        recipient: recipient_addr.clone(),
        ethereum_sender,
        timestamp: env.block.time.seconds(),
        status: BridgeStatus::Pending,
        validator_confirmations: 1,
        ai_risk_score: 0, // Will be updated by AI oracle
    };

    BRIDGE_TRANSACTIONS.save(deps.storage, &bridge_id, &bridge_tx)?;
    VALIDATOR_CONFIRMATIONS.save(deps.storage, (&bridge_id, info.sender.as_str()), &true)?;

    // Check if enough confirmations
    if bridge_tx.validator_confirmations >= state.min_validators {
        // Execute mint
        let mint_msg = execute_mint(&token_denom, amount, recipient_addr)?;

        // Update token config
        let mut updated_token_config = token_config;
        updated_token_config.total_minted += amount;
        SUPPORTED_TOKENS.save(deps.storage, &token_denom, &updated_token_config)?;

        // Update bridge transaction status
        let mut updated_bridge_tx = bridge_tx;
        updated_bridge_tx.status = BridgeStatus::Minted;
        BRIDGE_TRANSACTIONS.save(deps.storage, &bridge_id, &updated_bridge_tx)?;

        Ok(Response::new()
            .add_message(mint_msg)
            .add_attribute("method", "mint_tokens")
            .add_attribute("bridge_id", bridge_id)
            .add_attribute("amount", amount.to_string())
            .add_attribute("recipient", recipient))
    } else {
        Ok(Response::new()
            .add_attribute("method", "mint_tokens_pending")
            .add_attribute("bridge_id", bridge_id)
            .add_attribute(
                "confirmations",
                bridge_tx.validator_confirmations.to_string(),
            ))
    }
}

/// Burn tokens for Ethereum unlock
pub fn execute_burn_tokens(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    bridge_id: String,
    token_denom: String,
    amount: Uint128,
    ethereum_recipient: String,
) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;

    // Check if bridge is paused
    if state.is_paused {
        return Err(ContractError::BridgePaused {});
    }

    // Check if token is supported
    let token_config = SUPPORTED_TOKENS
        .may_load(deps.storage, &token_denom)?
        .ok_or(ContractError::TokenNotSupported {})?;

    if !token_config.is_active {
        return Err(ContractError::TokenNotSupported {});
    }

    // Validate amount
    if amount < state.min_bridge_amount {
        return Err(ContractError::AmountBelowMinimum {});
    }
    if amount > state.max_bridge_amount {
        return Err(ContractError::AmountAboveMaximum {});
    }

    // Check if bridge transaction already exists
    if BRIDGE_TRANSACTIONS.has(deps.storage, &bridge_id) {
        return Err(ContractError::AlreadyProcessed {});
    }

    // Create bridge transaction
    let bridge_tx = BridgeTransaction {
        bridge_id: bridge_id.clone(),
        ethereum_tx_hash: "".to_string(), // Will be set when completed
        token_denom: token_denom.clone(),
        amount,
        recipient: info.sender.clone(),
        ethereum_sender: ethereum_recipient.clone(),
        timestamp: env.block.time.seconds(),
        status: BridgeStatus::Pending,
        validator_confirmations: 0,
        ai_risk_score: 0,
    };

    BRIDGE_TRANSACTIONS.save(deps.storage, &bridge_id, &bridge_tx)?;

    // Execute burn
    let burn_msg = execute_burn(&token_denom, amount, info.sender.clone())?;

    // Update token config
    let mut updated_token_config = token_config;
    updated_token_config.total_burned += amount;
    SUPPORTED_TOKENS.save(deps.storage, &token_denom, &updated_token_config)?;

    // Update bridge transaction status
    let mut updated_bridge_tx = bridge_tx;
    updated_bridge_tx.status = BridgeStatus::Burned;
    BRIDGE_TRANSACTIONS.save(deps.storage, &bridge_id, &updated_bridge_tx)?;

    Ok(Response::new()
        .add_message(burn_msg)
        .add_attribute("method", "burn_tokens")
        .add_attribute("bridge_id", bridge_id)
        .add_attribute("amount", amount.to_string())
        .add_attribute("ethereum_recipient", ethereum_recipient))
}

/// Validator confirmation for bridge transaction
pub fn execute_confirm_bridge(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    bridge_id: String,
    _signature: String,
) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;

    // Validate sender is a validator
    if !state.validators.contains(&info.sender) {
        return Err(ContractError::Unauthorized {});
    }

    // Check if already confirmed by this validator
    if VALIDATOR_CONFIRMATIONS.has(deps.storage, (&bridge_id, info.sender.as_str())) {
        return Ok(Response::new().add_attribute("method", "already_confirmed"));
    }

    // Load bridge transaction
    let mut bridge_tx = BRIDGE_TRANSACTIONS
        .may_load(deps.storage, &bridge_id)?
        .ok_or(ContractError::InvalidBridgeTransaction {})?;

    // Record confirmation
    VALIDATOR_CONFIRMATIONS.save(deps.storage, (&bridge_id, info.sender.as_str()), &true)?;
    bridge_tx.validator_confirmations += 1;

    // Save updated transaction
    BRIDGE_TRANSACTIONS.save(deps.storage, &bridge_id, &bridge_tx)?;

    Ok(Response::new()
        .add_attribute("method", "confirm_bridge")
        .add_attribute("bridge_id", bridge_id)
        .add_attribute(
            "confirmations",
            bridge_tx.validator_confirmations.to_string(),
        ))
}

/// Complete bridge transaction
pub fn execute_complete_bridge(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    bridge_id: String,
    success: bool,
) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;

    // Validate sender is a validator
    if !state.validators.contains(&info.sender) {
        return Err(ContractError::Unauthorized {});
    }

    // Load bridge transaction
    let mut bridge_tx = BRIDGE_TRANSACTIONS
        .may_load(deps.storage, &bridge_id)?
        .ok_or(ContractError::InvalidBridgeTransaction {})?;

    // Update status
    bridge_tx.status = if success {
        BridgeStatus::Completed
    } else {
        BridgeStatus::Failed
    };

    BRIDGE_TRANSACTIONS.save(deps.storage, &bridge_id, &bridge_tx)?;

    Ok(Response::new()
        .add_attribute("method", "complete_bridge")
        .add_attribute("bridge_id", bridge_id)
        .add_attribute("success", success.to_string()))
}

/// Update AI risk score for a bridge transaction
pub fn execute_update_ai_risk_score(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    bridge_id: String,
    risk_score: u8,
) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;

    // Only AI oracle or admin may update risk score
    if info.sender != state.ai_oracle && info.sender != state.admin {
        return Err(ContractError::Unauthorized {});
    }

    let mut bridge_tx = BRIDGE_TRANSACTIONS
        .may_load(deps.storage, &bridge_id)?
        .ok_or(ContractError::InvalidBridgeTransaction {})?;

    bridge_tx.ai_risk_score = risk_score;

    // If risk score exceeds (or equals) threshold, mark as failed (if still pending/minted/burned)
    if risk_score >= state.ai_confidence_threshold {
        match bridge_tx.status {
            BridgeStatus::Pending | BridgeStatus::Minted | BridgeStatus::Burned => {
                bridge_tx.status = BridgeStatus::Failed;
            }
            _ => {}
        }
    }

    BRIDGE_TRANSACTIONS.save(deps.storage, &bridge_id, &bridge_tx)?;

    Ok(Response::new()
        .add_attribute("method", "update_ai_risk_score")
        .add_attribute("bridge_id", bridge_id)
        .add_attribute("risk_score", risk_score.to_string())
        .add_attribute(
            "threshold_triggered",
            (risk_score >= state.ai_confidence_threshold).to_string(),
        )
        .add_attribute("status", format!("{:?}", bridge_tx.status)))
}

/// Contract query entry point
#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetState {} => to_json_binary(&query_state(deps)?),
        QueryMsg::GetBridgeTransaction { bridge_id } => {
            to_json_binary(&query_bridge_transaction(deps, bridge_id)?)
        }
        QueryMsg::GetSupportedToken { denom } => {
            to_json_binary(&query_supported_token(deps, denom)?)
        }
        QueryMsg::ListSupportedTokens {} => to_json_binary(&query_list_supported_tokens(deps)?),
        QueryMsg::GetValidators {} => to_json_binary(&query_validators(deps)?),
        QueryMsg::GetBridgeStats {} => to_json_binary(&query_bridge_stats(deps)?),
        QueryMsg::GetAIRiskScore { bridge_id } => {
            to_json_binary(&query_ai_risk_score(deps, bridge_id)?)
        }
    }
}

/// Query contract state
pub fn query_state(deps: Deps) -> StdResult<State> {
    STATE.load(deps.storage)
}

/// Query bridge transaction
pub fn query_bridge_transaction(deps: Deps, bridge_id: String) -> StdResult<BridgeTransaction> {
    BRIDGE_TRANSACTIONS.load(deps.storage, &bridge_id)
}

/// Query supported token
pub fn query_supported_token(deps: Deps, denom: String) -> StdResult<TokenConfig> {
    SUPPORTED_TOKENS.load(deps.storage, &denom)
}

/// Query list of supported tokens
pub fn query_list_supported_tokens(deps: Deps) -> StdResult<Vec<TokenConfig>> {
    SUPPORTED_TOKENS
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .map(|item| item.map(|(_, token)| token))
        .collect()
}

/// Query validators
pub fn query_validators(deps: Deps) -> StdResult<Vec<Addr>> {
    let state = STATE.load(deps.storage)?;
    Ok(state.validators)
}

/// Query bridge statistics
pub fn query_bridge_stats(deps: Deps) -> StdResult<BridgeStats> {
    let tokens: Vec<TokenConfig> = SUPPORTED_TOKENS
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .map(|item| item.map(|(_, token)| token))
        .collect::<StdResult<Vec<_>>>()?;

    let total_minted = tokens.iter().map(|t| t.total_minted).sum::<Uint128>();
    let total_burned = tokens.iter().map(|t| t.total_burned).sum::<Uint128>();

    Ok(BridgeStats {
        total_tokens: tokens.len() as u32,
        total_minted,
        total_burned,
        active_tokens: tokens.iter().filter(|t| t.is_active).count() as u32,
    })
}

/// Query AI risk score
pub fn query_ai_risk_score(deps: Deps, bridge_id: String) -> StdResult<u8> {
    let bridge_tx = BRIDGE_TRANSACTIONS.load(deps.storage, &bridge_id)?;
    Ok(bridge_tx.ai_risk_score)
}

/// Bridge statistics
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BridgeStats {
    pub total_tokens: u32,
    pub total_minted: Uint128,
    pub total_burned: Uint128,
    pub active_tokens: u32,
}

// Helper functions

/// Create mint message
fn execute_mint(
    token_denom: &str,
    amount: Uint128,
    recipient: Addr,
) -> Result<CosmosMsg, ContractError> {
    Ok(CosmosMsg::Bank(BankMsg::Send {
        to_address: recipient.to_string(),
        amount: vec![Coin {
            denom: token_denom.to_string(),
            amount,
        }],
    }))
}

/// Create burn message
fn execute_burn(
    token_denom: &str,
    amount: Uint128,
    _sender: Addr,
) -> Result<CosmosMsg, ContractError> {
    // In practice, this would use a custom burn module or bank burn functionality
    Ok(CosmosMsg::Bank(BankMsg::Burn {
        amount: vec![Coin {
            denom: token_denom.to_string(),
            amount,
        }],
    }))
}

// Additional implementation functions would go here...

/// Placeholder implementations for remaining execute functions
pub fn execute_add_supported_token(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    denom: String,
    ethereum_address: String,
    decimals: u8,
    mint_cap: Option<Uint128>,
) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;

    // Only admin can add tokens
    if info.sender != state.admin {
        return Err(ContractError::Unauthorized {});
    }

    let token_config = TokenConfig {
        denom: denom.clone(),
        ethereum_address,
        decimals,
        is_active: true,
        mint_cap,
        total_minted: Uint128::zero(),
        total_burned: Uint128::zero(),
    };

    SUPPORTED_TOKENS.save(deps.storage, &denom, &token_config)?;

    Ok(Response::new()
        .add_attribute("method", "add_supported_token")
        .add_attribute("denom", denom))
}

pub fn execute_remove_supported_token(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    denom: String,
) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;
    if info.sender != state.admin {
        return Err(ContractError::Unauthorized {});
    }

    let mut token = SUPPORTED_TOKENS
        .may_load(deps.storage, &denom)?
        .ok_or(ContractError::TokenNotSupported {})?;

    if !token.is_active {
        return Ok(Response::new()
            .add_attribute("method", "remove_supported_token")
            .add_attribute("denom", denom)
            .add_attribute("status", "already_inactive"));
    }

    token.is_active = false;
    SUPPORTED_TOKENS.save(deps.storage, &token.denom, &token)?;

    Ok(Response::new()
        .add_attribute("method", "remove_supported_token")
        .add_attribute("denom", token.denom)
        .add_attribute("status", "deactivated"))
}

pub fn execute_add_validator(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    validator: String,
) -> Result<Response, ContractError> {
    let mut state = STATE.load(deps.storage)?;
    if info.sender != state.admin {
        return Err(ContractError::Unauthorized {});
    }

    let addr = deps.api.addr_validate(&validator)?;
    if state.validators.contains(&addr) {
        return Ok(Response::new()
            .add_attribute("method", "add_validator")
            .add_attribute("validator", validator)
            .add_attribute("status", "exists"));
    }
    state.validators.push(addr.clone());
    STATE.save(deps.storage, &state)?;

    Ok(Response::new()
        .add_attribute("method", "add_validator")
        .add_attribute("validator", addr)
        .add_attribute("total_validators", state.validators.len().to_string()))
}

pub fn execute_remove_validator(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    validator: String,
) -> Result<Response, ContractError> {
    let mut state = STATE.load(deps.storage)?;
    if info.sender != state.admin {
        return Err(ContractError::Unauthorized {});
    }

    let addr = deps.api.addr_validate(&validator)?;
    if let Some(pos) = state.validators.iter().position(|v| v == addr) {
        state.validators.remove(pos);
        // Ensure min_validators still <= current validator count (can't make it impossible)
        if state.min_validators as usize > state.validators.len() {
            // revert removal to avoid inconsistent state
            state.validators.insert(pos, addr.clone());
            return Err(ContractError::Unauthorized {}); // reuse; ideally define a specific error
        }
        STATE.save(deps.storage, &state)?;
        Ok(Response::new()
            .add_attribute("method", "remove_validator")
            .add_attribute("validator", validator)
            .add_attribute("total_validators", state.validators.len().to_string()))
    } else {
        Ok(Response::new()
            .add_attribute("method", "remove_validator")
            .add_attribute("validator", validator)
            .add_attribute("status", "not_found"))
    }
}

pub fn execute_pause(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let mut state = STATE.load(deps.storage)?;
    if info.sender != state.admin {
        return Err(ContractError::Unauthorized {});
    }
    if state.is_paused {
        return Ok(Response::new()
            .add_attribute("method", "pause")
            .add_attribute("status", "already_paused"));
    }
    state.is_paused = true;
    STATE.save(deps.storage, &state)?;
    Ok(Response::new()
        .add_attribute("method", "pause")
        .add_attribute("status", "paused"))
}

pub fn execute_unpause(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let mut state = STATE.load(deps.storage)?;
    if info.sender != state.admin {
        return Err(ContractError::Unauthorized {});
    }
    if !state.is_paused {
        return Ok(Response::new()
            .add_attribute("method", "unpause")
            .add_attribute("status", "already_active"));
    }
    state.is_paused = false;
    STATE.save(deps.storage, &state)?;
    Ok(Response::new()
        .add_attribute("method", "unpause")
        .add_attribute("status", "active"))
}

pub fn execute_update_bridge_params(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    bridge_fee: Option<Uint128>,
    min_bridge_amount: Option<Uint128>,
    max_bridge_amount: Option<Uint128>,
    min_validators: Option<u32>,
) -> Result<Response, ContractError> {
    let mut state = STATE.load(deps.storage)?;
    if info.sender != state.admin {
        return Err(ContractError::Unauthorized {});
    }

    if let Some(fee) = bridge_fee {
        state.bridge_fee = fee;
    }
    if let Some(min_amt) = min_bridge_amount {
        state.min_bridge_amount = min_amt;
    }
    if let Some(max_amt) = max_bridge_amount {
        state.max_bridge_amount = max_amt;
    }
    if state.min_bridge_amount > state.max_bridge_amount {
        return Err(ContractError::AmountAboveMaximum {}); // reuse as validation error (could define new error)
    }
    if let Some(min_val) = min_validators {
        // Ensure not zero and not more than available validators
        if min_val == 0 || (min_val as usize) > state.validators.len() {
            return Err(ContractError::Unauthorized {}); // reuse (better to create InvalidBridgeTransaction variant but already exists)
        }
        state.min_validators = min_val;
    }

    STATE.save(deps.storage, &state)?;
    Ok(Response::new()
        .add_attribute("method", "update_bridge_params")
        .add_attribute("bridge_fee", state.bridge_fee.to_string())
        .add_attribute("min_bridge_amount", state.min_bridge_amount.to_string())
        .add_attribute("max_bridge_amount", state.max_bridge_amount.to_string())
        .add_attribute("min_validators", state.min_validators.to_string()))
}

pub fn execute_emergency_recovery(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    bridge_id: String,
    reason: String,
) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;
    if info.sender != state.admin {
        return Err(ContractError::Unauthorized {});
    }
    let mut bridge_tx = BRIDGE_TRANSACTIONS
        .may_load(deps.storage, &bridge_id)?
        .ok_or(ContractError::InvalidBridgeTransaction {})?;

    match bridge_tx.status {
        BridgeStatus::Completed | BridgeStatus::Failed => {
            return Ok(Response::new()
                .add_attribute("method", "emergency_recovery")
                .add_attribute("bridge_id", bridge_id)
                .add_attribute("status", "no_action"))
        }
        _ => {
            bridge_tx.status = BridgeStatus::Failed;
            BRIDGE_TRANSACTIONS.save(deps.storage, &bridge_tx.bridge_id, &bridge_tx)?;
        }
    }

    Ok(Response::new()
        .add_attribute("method", "emergency_recovery")
        .add_attribute("bridge_id", bridge_tx.bridge_id)
        .add_attribute("reason", reason)
        .add_attribute("status", "marked_failed"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{coins, from_json};

    #[test]
    fn proper_initialization() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {
            admin: "admin".to_string(),
            ethereum_channel: "channel-0".to_string(),
            validators: vec!["validator1".to_string(), "validator2".to_string()],
            min_validators: 2,
            bridge_fee: Uint128::from(1000u128),
            ai_oracle: "ai_oracle".to_string(),
        };

        let info = mock_info("creator", &coins(1000, "earth"));
        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        assert_eq!(0, res.messages.len());
    }

    #[test]
    fn test_query_state() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {
            admin: "admin".to_string(),
            ethereum_channel: "channel-0".to_string(),
            validators: vec!["validator1".to_string()],
            min_validators: 1,
            bridge_fee: Uint128::from(1000u128),
            ai_oracle: "ai_oracle".to_string(),
        };

        let info = mock_info("creator", &coins(1000, "earth"));
        let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

        let res = query(deps.as_ref(), mock_env(), QueryMsg::GetState {}).unwrap();
        let state: State = from_json(&res).unwrap();
        assert_eq!(state.admin, "admin");
    }

    #[test]
    fn pause_unpause_flow() {
        let mut deps = mock_dependencies();
        let msg = InstantiateMsg {
            admin: "admin".to_string(),
            ethereum_channel: "channel-0".to_string(),
            validators: vec!["validator1".to_string()],
            min_validators: 1,
            bridge_fee: Uint128::from(1000u128),
            ai_oracle: "ai_oracle".to_string(),
        };
        let info = mock_info("creator", &[]);
        instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

        // Pause
        let pause_res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("admin", &[]),
            ExecuteMsg::Pause {},
        )
        .unwrap();
        assert!(pause_res.attributes.iter().any(|a| a.value == "paused"));

        // Unpause
        let unpause_res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("admin", &[]),
            ExecuteMsg::Unpause {},
        )
        .unwrap();
        assert!(unpause_res.attributes.iter().any(|a| a.value == "active"));
    }
}
