use log::{debug, info};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
// Enable smart contracts integration now that the crate compiles
use crate::crypto::PQCManager; // added
use crate::genesis;
use crate::staking::{
    Delegation, DelegatorRewardsSummary, DelegatorValidatorRewards, StakingError, StakingState,
    Validator,
};
use crate::storage::StorageManager;
use crate::types::Amount as Tokens;
use crate::types::{Address, BlockNumber};
use crate::types::{Transaction, TxReceipt, TxStatus};
use crate::wasm::host_env::{HostEnv, HostExecutionContext}; // keep host env
use crate::wasm::WasmEngine; // updated simplified import
use dytallix_contracts::runtime::{
    ContractCall, ContractDeployment, ContractRuntime, ExecutionResult,
}; // added // import module to simplify path references

pub mod oracle;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeState {
    pub balances: HashMap<String, u128>,
    pub contracts: HashMap<String, Vec<u8>>, // Will be deprecated in favor of contract runtime
    pub nonces: HashMap<String, u64>,
    pub total_supply: u128,
    pub last_block_number: u64,
    pub last_block_timestamp: u64,
    /// DRT token balances (separate from DGT balances)
    pub drt_balances: HashMap<String, u128>,
    /// Staking state for validators and delegations
    pub staking: StakingState,
}

impl Default for RuntimeState {
    fn default() -> Self {
        let mut balances = HashMap::new();
        balances.insert("dyt1genesis".to_string(), 1_000_000_000_000u128); // 1 trillion tokens

        Self {
            balances,
            contracts: HashMap::new(),
            nonces: HashMap::new(),
            total_supply: 1_000_000_000_000u128,
            last_block_number: 0,
            last_block_timestamp: 0,
            drt_balances: HashMap::new(),
            staking: StakingState::new(),
        }
    }
}

impl RuntimeState {
    /// Initialize runtime state with genesis configuration
    pub fn from_genesis(genesis: &genesis::GenesisConfig) -> Result<Self, String> {
        genesis._validate()?;
        if genesis.drt_emission.initial_supply != 0 {
            return Err("Nonzero initial DRT supply requires an explicit allocation map".into());
        }
        let total_supply = genesis::validate_allocations(&genesis.dgt_allocations)?;
        let mut state = Self {
            balances: HashMap::new(),
            contracts: HashMap::new(),
            nonces: HashMap::new(),
            total_supply,
            last_block_number: 0,
            last_block_timestamp: 0,
            drt_balances: HashMap::new(),
            staking: StakingState::new(),
        };
        state.staking.params = genesis.staking.to_staking_params();
        for allocation in &genesis.dgt_allocations {
            state
                .balances
                .insert(allocation.address.clone(), allocation.amount);
        }
        for validator_info in &genesis.validators {
            state
                .staking
                .register_validator(
                    validator_info.address.clone(),
                    validator_info.public_key.clone(),
                    validator_info.commission,
                )
                .map_err(|error| format!("Genesis validator registration failed: {error}"))?;
            if validator_info.stake > 0 {
                let balance = state
                    .balances
                    .get_mut(&validator_info.address)
                    .ok_or("Genesis validator stake has no funding allocation")?;
                *balance = balance
                    .checked_sub(validator_info.stake)
                    .ok_or("Genesis validator stake exceeds its funding allocation")?;
                // Each stake is debited from a unique allocation. Their sum cannot
                // exceed the checked allocation total or overflow the staking state.
                state
                    .staking
                    .delegate(
                        validator_info.address.clone(),
                        validator_info.address.clone(),
                        validator_info.stake,
                    )
                    .map_err(|error| format!("Genesis self-delegation failed: {error}"))?;
            }
        }
        let liquid = state
            .balances
            .values()
            .try_fold(0u128, |sum, amount| sum.checked_add(*amount))
            .ok_or("Genesis liquid total exceeds u128")?;
        if liquid.checked_add(state.staking.total_stake) != Some(state.total_supply) {
            return Err("Genesis DGT conservation check failed".into());
        }

        Ok(state)
    }
}

#[allow(dead_code)]
pub struct DytallixRuntime {
    state: Arc<RwLock<RuntimeState>>,
    storage: Arc<StorageManager>,
    contract_runtime: Arc<ContractRuntime>,
    // WASM engine & env (single reusable engine with shared HostEnv)
    wasm_engine: Arc<WasmEngine>,
    _pqc_manager: Arc<PQCManager>, // underscore
}

impl std::fmt::Debug for DytallixRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DytallixRuntime")
            .field("state", &"<state>")
            .field("storage", &"<storage>")
            .field("contract_runtime", &"<contract_runtime>")
            .finish()
    }
}

impl DytallixRuntime {
    pub fn new(storage: Arc<StorageManager>) -> Result<Self, Box<dyn std::error::Error>> {
        let pqc_manager = Arc::new(PQCManager::new()?);
        let host_env = HostEnv::with_pqc(pqc_manager.clone());
        let wasm_engine = Arc::new(WasmEngine::new_with_env(host_env));
        Self::new_with_genesis_inner(storage, None, wasm_engine, pqc_manager)
    }

    fn new_with_genesis_inner(
        storage: Arc<StorageManager>,
        genesis: Option<&genesis::GenesisConfig>, // updated path
        wasm_engine: Arc<WasmEngine>,
        pqc_manager: Arc<PQCManager>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Initialize contract runtime with reasonable limits
        let contract_runtime = Arc::new(ContractRuntime::new(
            10_000_000, // 10M gas limit per call
            256,        // 256 pages (16MB) memory limit
        )?);
        let initial_state = match genesis {
            Some(g) => RuntimeState::from_genesis(g)?,
            None => RuntimeState::default(),
        };
        Ok(Self {
            state: Arc::new(RwLock::new(initial_state)),
            storage,
            contract_runtime,
            wasm_engine,
            _pqc_manager: pqc_manager,
        })
    }

    pub fn new_with_genesis(
        storage: Arc<StorageManager>,
        genesis: Option<&genesis::GenesisConfig>, // updated path
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let pqc_manager = Arc::new(PQCManager::new()?);
        let host_env = HostEnv::with_pqc(pqc_manager.clone());
        let wasm_engine = Arc::new(WasmEngine::new_with_env(host_env));
        Self::new_with_genesis_inner(storage, genesis, wasm_engine, pqc_manager)
    }

    pub async fn get_balance(&self, address: &str) -> Result<u128, Box<dyn std::error::Error>> {
        let state = self.state.read().await;
        Ok(state.balances.get(address).copied().unwrap_or(0))
    }

    pub async fn set_balance(
        &self,
        address: &str,
        amount: u128,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut state = self.state.write().await;
        state.balances.insert(address.to_string(), amount);
        debug!("Set balance for {address}: {amount}");
        Ok(())
    }

    pub async fn transfer(
        &self,
        from: &str,
        to: &str,
        amount: u128,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut state = self.state.write().await;

        let from_balance = state.balances.get(from).copied().unwrap_or(0);
        if from_balance < amount {
            return Err("Insufficient balance".into());
        }

        let to_balance = state.balances.get(to).copied().unwrap_or(0);

        state
            .balances
            .insert(from.to_string(), from_balance - amount);
        state.balances.insert(to.to_string(), to_balance + amount);

        info!("Transfer: {from} -> {to} amount: {amount}");
        Ok(())
    }

    pub async fn get_nonce(&self, address: &str) -> Result<u64, Box<dyn std::error::Error>> {
        let state = self.state.read().await;
        Ok(state.nonces.get(address).copied().unwrap_or(0))
    }

    #[allow(dead_code)]
    pub async fn increment_nonce(&self, address: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut state = self.state.write().await;
        let current_nonce = state.nonces.get(address).copied().unwrap_or(0);
        state.nonces.insert(address.to_string(), current_nonce + 1);
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn deploy_contract(
        &self,
        address: &str,
        code: Vec<u8>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("Deploying contract at address: {address}");

        // Create deployment info
        let deployment = ContractDeployment {
            address: address.to_string(),
            code: code.clone(),
            initial_state: Vec::new(),
            gas_limit: 1_000_000,                // 1M gas for deployment
            deployer: "dyt1genesis".to_string(), // TODO: Get from transaction context
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            ai_audit_score: None,
        };

        // Deploy to contract runtime
        let deployed_address = self
            .contract_runtime
            .deploy_contract(deployment)
            .await
            .map_err(|e| format!("Contract deployment failed: {e}"))?;

        // Also store in legacy state for backward compatibility
        let mut state = self.state.write().await;
        state.contracts.insert(address.to_string(), code);

        info!("Contract deployed successfully at address: {deployed_address}");
        Ok(())
    }

    pub async fn get_contract(
        &self,
        address: &str,
    ) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
        let state = self.state.read().await;
        Ok(state.contracts.get(address).cloned())
    }

    #[allow(dead_code)]
    pub async fn execute_contract(
        &self,
        address: &str,
        input: &[u8],
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        debug!(
            "Executing contract at {} with {} bytes input",
            address,
            input.len()
        );

        // Create contract call
        let contract_call = ContractCall {
            contract_address: address.to_string(),
            caller: "dyt1genesis".to_string(), // TODO: Get from transaction context
            method: "execute".to_string(),     // TODO: Parse method from input
            input_data: input.to_vec(),
            gas_limit: 500_000, // 500K gas for execution
            value: 0,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };

        // Execute contract call
        let execution_result = self
            .contract_runtime
            .call_contract(contract_call)
            .await
            .map_err(|e| format!("Contract execution failed: {e}"))?;

        if execution_result.success {
            debug!(
                "Contract execution successful, gas used: {}",
                execution_result.gas_used
            );

            // Log events if any
            for event in &execution_result.events {
                info!("Contract event: {event:?}");
            }

            Ok(execution_result.return_data)
        } else {
            Err(format!(
                "Contract execution failed, gas used: {}",
                execution_result.gas_used
            )
            .into())
        }
    }

    /// Execute a contract call with specific method and parameters
    pub async fn call_contract_method(
        &self,
        address: &str,
        caller: &str,
        method: &str,
        input_data: &[u8],
        gas_limit: u64,
        value: u128,
    ) -> Result<ExecutionResult, Box<dyn std::error::Error>> {
        debug!("Calling contract method {method} at {address} from {caller}");

        let contract_call = ContractCall {
            contract_address: address.to_string(),
            caller: caller.to_string(),
            method: method.to_string(),
            input_data: input_data.to_vec(),
            gas_limit,
            value,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };

        let execution_result = self
            .contract_runtime
            .call_contract(contract_call)
            .await
            .map_err(|e| format!("Contract call failed: {e}"))?;

        Ok(execution_result)
    }

    /// Get contract runtime reference for advanced operations
    pub fn get_contract_runtime(&self) -> Arc<ContractRuntime> {
        self.contract_runtime.clone()
    }

    /// Deploy a contract with full deployment configuration
    pub async fn deploy_contract_full(
        &self,
        address: &str,
        code: Vec<u8>,
        deployer: &str,
        gas_limit: u64,
        initial_state: Vec<u8>,
    ) -> Result<String, Box<dyn std::error::Error>> {
        info!("Deploying contract at address: {address} from deployer: {deployer}");

        let deployment = ContractDeployment {
            address: address.to_string(),
            code: code.clone(),
            initial_state,
            gas_limit,
            deployer: deployer.to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            ai_audit_score: None,
        };

        let deployed_address = self
            .contract_runtime
            .deploy_contract(deployment)
            .await
            .map_err(|e| format!("Contract deployment failed: {e}"))?;

        // Update legacy state
        let mut state = self.state.write().await;
        state.contracts.insert(address.to_string(), code);

        info!("Contract deployed successfully at address: {deployed_address}");
        Ok(deployed_address)
    }

    // Staking-related methods

    /// Register a new validator
    pub async fn register_validator(
        &self,
        address: Address,
        consensus_pubkey: Vec<u8>,
        commission_rate: u16,
    ) -> Result<(), StakingError> {
        let mut state = self.state.write().await;
        state
            .staking
            .register_validator(address, consensus_pubkey, commission_rate)
    }

    /// Delegate DGT tokens to a validator
    pub async fn delegate(
        &self,
        delegator: Address,
        validator: Address,
        amount: Tokens,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Check if delegator has sufficient DGT balance
        let dgt_balance = self.get_balance(&delegator).await?;

        if dgt_balance < amount {
            return Err(Box::new(StakingError::InsufficientFunds));
        }

        let mut state = self.state.write().await;

        // Lock DGT tokens by reducing balance
        let current_balance = state.balances.get(&delegator).copied().unwrap_or(0);
        if current_balance < amount {
            return Err(Box::new(StakingError::InsufficientFunds));
        }
        state
            .balances
            .insert(delegator.clone(), current_balance - amount);

        // Create delegation
        state.staking.delegate(delegator, validator, amount)?;

        debug!("Delegated {amount} uDGT");
        Ok(())
    }

    /// Get active validators
    pub async fn get_active_validators(&self) -> Vec<Validator> {
        let state = self.state.read().await;
        state
            .staking
            .get_active_validators()
            .into_iter()
            .cloned()
            .collect()
    }

    /// Get validator info
    pub async fn get_validator(&self, address: &Address) -> Option<Validator> {
        let state = self.state.read().await;
        state.staking.validators.get(address).cloned()
    }

    /// Get delegation info
    pub async fn get_delegation(
        &self,
        delegator: &Address,
        validator: &Address,
    ) -> Option<Delegation> {
        let state = self.state.read().await;
        let delegation_key = format!("{delegator}:{validator}");
        state.staking.delegations.get(&delegation_key).cloned()
    }

    /// Calculate pending rewards for a delegation
    pub async fn calculate_pending_rewards(
        &self,
        delegator: &Address,
        validator: &Address,
    ) -> Result<u128, StakingError> {
        let state = self.state.read().await;
        state
            .staking
            .calculate_pending_rewards(delegator, validator)
    }

    /// Sync delegation rewards and return current accrued amount
    pub async fn sync_and_get_accrued_rewards(
        &self,
        delegator: &Address,
        validator: &Address,
    ) -> Result<u128, StakingError> {
        let mut state = self.state.write().await;
        let (_, total_accrued) = state
            .staking
            .sync_delegation_rewards(delegator, validator)?;
        Ok(total_accrued)
    }

    /// Get current accrued rewards without recomputation
    pub async fn get_accrued_rewards(
        &self,
        delegator: &Address,
        validator: &Address,
    ) -> Result<u128, StakingError> {
        let state = self.state.read().await;
        let delegation_key = format!("{delegator}:{validator}");
        let delegation = state
            .staking
            .delegations
            .get(&delegation_key)
            .ok_or(StakingError::DelegationNotFound)?;
        Ok(delegation.accrued_rewards)
    }

    /// Claim rewards for a delegation
    pub async fn claim_rewards(
        &self,
        delegator: &Address,
        validator: &Address,
    ) -> Result<u128, StakingError> {
        let mut state = self.state.write().await;
        let rewards = state.staking.claim_rewards(delegator, validator)?;

        if rewards > 0 {
            // Credit DRT tokens to delegator
            let current_drt = state.drt_balances.get(delegator).copied().unwrap_or(0);
            state
                .drt_balances
                .insert(delegator.clone(), current_drt + rewards);
            debug!("Credited {rewards} uDRT rewards to {delegator}");
        }

        Ok(rewards)
    }

    /// Claim rewards for all delegations of a delegator
    pub async fn claim_all_rewards(&self, delegator: &Address) -> Result<u128, StakingError> {
        let mut state = self.state.write().await;
        let total_rewards = state.staking.claim_all_rewards(delegator)?;

        if total_rewards > 0 {
            // Credit DRT tokens to delegator
            let current_drt = state.drt_balances.get(delegator).copied().unwrap_or(0);
            state
                .drt_balances
                .insert(delegator.clone(), current_drt + total_rewards);
            debug!("Credited {total_rewards} uDRT total rewards to {delegator}");
        }

        Ok(total_rewards)
    }

    /// Get comprehensive delegator reward information
    pub async fn get_delegator_rewards_summary(
        &self,
        delegator: &Address,
    ) -> DelegatorRewardsSummary {
        let state = self.state.read().await;
        state.staking.get_delegator_rewards_summary(delegator)
    }

    /// Get delegator rewards for a specific validator
    pub async fn _get_delegator_validator_rewards(
        &self,
        delegator: &Address,
        validator: &Address,
    ) -> Result<DelegatorValidatorRewards, StakingError> {
        let state = self.state.read().await;
        state
            .staking
            ._get_delegator_validator_rewards(delegator, validator)
    }

    /// Process block rewards (called during block processing)
    pub async fn process_block_rewards(
        &self,
        block_height: BlockNumber,
    ) -> Result<(), StakingError> {
        let mut state = self.state.write().await;
        state.staking.process_block_rewards(block_height)
    }

    /// Get DRT balance for an address
    pub async fn get_drt_balance(&self, address: &str) -> u128 {
        let state = self.state.read().await;
        state.drt_balances.get(address).copied().unwrap_or(0)
    }

    /// Get current block height
    pub async fn get_current_height(&self) -> Result<BlockNumber, Box<dyn std::error::Error>> {
        let state = self.state.read().await;
        Ok(state.staking.current_height)
    }

    /// Store arbitrary data in the blockchain state (for asset registry, etc.)
    pub async fn store_data(
        &self,
        key: &str,
        value: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let storage_key = format!("data:{}", key);
        self.storage
            ._put(storage_key.as_bytes(), value.as_bytes())
            .await?;
        Ok(())
    }

    /// Get arbitrary data from the blockchain state
    pub async fn get_data(&self, key: &str) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let storage_key = format!("data:{}", key);
        match self.storage._get(storage_key.as_bytes()).await? {
            Some(bytes) => {
                let value = String::from_utf8(bytes)?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// Get staking statistics
    pub async fn get_staking_stats(&self) -> (u128, u32, u32) {
        let state = self.state.read().await;
        let total_stake = state.staking.total_stake;
        let total_validators = state.staking.validators.len() as u32;
        let active_validators = state.staking.get_active_validators().len() as u32;
        (total_stake, total_validators, active_validators)
    }

    /// Apply external emission to staking system (called by emission engine)
    pub async fn _apply_staking_emission(&self, amount: u128) -> Result<(), StakingError> {
        let mut state = self.state.write().await;
        state.staking._apply_external_emission(amount);
        Ok(())
    }

    /// Get reward statistics for emission validation
    pub async fn _get_reward_stats(&self) -> (u128, u128) {
        let state = self.state.read().await;
        state.staking._get_reward_stats()
    }

    pub async fn _save_state(&self) -> Result<(), Box<dyn std::error::Error>> {
        let state = self.state.read().await;

        // Serialize and save state to storage
        let state_json = serde_json::to_string(&*state)?;
        self.storage
            ._put("runtime_state".as_bytes(), state_json.as_bytes())
            .await?;

        info!("Runtime state saved to storage");
        Ok(())
    }

    pub async fn _load_state(&self) -> Result<(), Box<dyn std::error::Error>> {
        match self.storage._get("runtime_state".as_bytes()).await? {
            Some(state_data) => {
                let state_json = String::from_utf8(state_data)?;
                let loaded_state: RuntimeState = serde_json::from_str(&state_json)?;

                let mut state = self.state.write().await;
                *state = loaded_state;

                info!("Runtime state loaded from storage");
                Ok(())
            }
            None => {
                info!("No previous state found, starting with fresh state");
                Ok(())
            }
        }
    }

    /// Execute a single transaction (Deploy / Call / Transfer currently)
    #[allow(dead_code)]
    pub async fn execute_tx(
        &self,
        tx: &Transaction,
        block_height: u64,
        block_time: i64,
        tx_index: usize,
    ) -> TxReceipt {
        use Transaction::*;
        let mut status = TxStatus::Success;
        let gas_used: u64 = 0; // not mut per clippy warning
        let mut error: Option<String> = None;
        let mut contract_address: Option<String> = None;
        let mut return_data: Option<Vec<u8>> = None;
        let env = self.wasm_engine.env();

        // Basic nonce/auth enforcement for account based txs
        let from = tx.from().clone();
        let expected_nonce = self.storage.get_address_nonce(&from).await.unwrap_or(0);
        if tx.nonce() != expected_nonce {
            status = TxStatus::Failed;
            error = Some("nonce_mismatch".into());
        }

        if status == TxStatus::Success {
            match tx {
                Transfer(t) => {
                    if let Err(e) = self.storage._apply_transfer(t) {
                        status = TxStatus::Failed;
                        error = Some(e);
                    }
                }
                Deploy(d) => {
                    // Set context
                    self.wasm_engine.set_context(HostExecutionContext {
                        block_height,
                        block_time,
                        caller: d.from.clone(),
                        deployer: d.from.clone(),
                        input: Vec::new(),
                    });
                    // Execute deployment via legacy runtime for now (placeholder for WASMExecutor integration)
                    if let Err(e) = self
                        .deploy_contract(&format!("contract_{}", d.hash), d.contract_code.clone())
                        .await
                    {
                        status = TxStatus::Failed;
                        error = Some(format!("deploy_failed:{e}"));
                    } else {
                        contract_address = Some(format!("contract_{}", d.hash));
                    }
                }
                Call(c) => {
                    self.wasm_engine.set_context(HostExecutionContext {
                        block_height,
                        block_time,
                        caller: c.from.clone(),
                        deployer: c.from.clone(),
                        input: c.args.clone(),
                    });
                    match self.execute_contract(&c.to, &c.args).await {
                        Ok(data) => {
                            return_data = Some(data);
                        }
                        Err(e) => {
                            status = TxStatus::Failed;
                            error = Some(format!("call_failed:{e}"));
                        }
                    }
                }
                _ => {}
            }
        }

        // Increment nonce on success for account-based txs
        if status == TxStatus::Success {
            let _ = self.increment_nonce(&from).await;
        }

        let logs = env.take_logs();
        TxReceipt {
            tx_hash: tx.hash(),
            block_number: block_height,
            status,
            gas_used,
            fee_paid: tx.fee(),
            timestamp: block_time as u64,
            index: tx_index as u32,
            error,
            contract_address,
            logs,
            return_data,
        }
    }

    /// Execute a vector of transactions, returning receipts and total gas
    pub async fn execute_block_txs(
        &self,
        txs: &[Transaction],
        block_height: u64,
        block_time: i64,
    ) -> (Vec<TxReceipt>, u64) {
        let mut receipts = Vec::with_capacity(txs.len());
        let mut gas_sum = 0u64; // mutable because we accumulate
        for (i, tx) in txs.iter().enumerate() {
            let rcpt = self.execute_tx(tx, block_height, block_time, i).await;
            gas_sum += rcpt.gas_used;
            receipts.push(rcpt);
        }
        (receipts, gas_sum)
    }
}

// Remove temporary staking stubs (Validator, Delegation, etc.) now that real staking module is used

#[cfg(test)]
mod genesis_funding_tests {
    use super::*;
    fn funded() -> genesis::GenesisConfig {
        let mut genesis = genesis::GenesisConfig::mainnet();
        genesis.validators.truncate(1);
        genesis.validators[0].address = genesis.dgt_allocations[0].address.clone();
        genesis
    }
    #[test]
    fn supply_equals_liquid_plus_funded_stake_without_default_seed() {
        let genesis = funded();
        let state = RuntimeState::from_genesis(&genesis).unwrap();
        let stake = genesis.validators[0].stake;
        assert!(!state.balances.contains_key("dyt1genesis"));
        assert_eq!(
            state.balances[&genesis.validators[0].address],
            genesis.dgt_allocations[0].amount - stake
        );
        assert_eq!(state.staking.total_stake, stake);
        assert_eq!(
            state.total_supply,
            genesis.dgt_allocations.iter().map(|a| a.amount).sum()
        );
        assert_eq!(
            state.balances.values().sum::<u128>() + stake,
            state.total_supply
        );
    }
    #[test]
    fn unfunded_historical_validator_template_is_rejected() {
        assert!(
            RuntimeState::from_genesis(&genesis::GenesisConfig::mainnet())
                .unwrap_err()
                .contains("no funding allocation")
        );
    }
    #[test]
    fn insufficient_funding_and_duplicate_validators_are_rejected() {
        let mut genesis = funded();
        genesis.validators[0].stake = genesis.dgt_allocations[0].amount + 1;
        assert!(RuntimeState::from_genesis(&genesis)
            .unwrap_err()
            .contains("exceeds its funding"));
        let mut genesis = funded();
        genesis.validators.push(genesis.validators[0].clone());
        assert!(RuntimeState::from_genesis(&genesis)
            .unwrap_err()
            .contains("registration failed"));
    }
    #[test]
    fn nonzero_drt_without_allocations_is_rejected() {
        let mut genesis = funded();
        genesis.drt_emission.initial_supply = 1;
        assert!(RuntimeState::from_genesis(&genesis)
            .unwrap_err()
            .contains("explicit allocation map"));
    }
    #[test]
    fn zero_stake_preserves_full_allocation() {
        let mut genesis = funded();
        genesis.validators[0].stake = 0;
        assert!(RuntimeState::from_genesis(&genesis)
            .unwrap_err()
            .contains("insufficient stake"));
        genesis.staking.minimum_validator_stake = 0;
        let state = RuntimeState::from_genesis(&genesis).unwrap();
        assert_eq!(state.staking.total_stake, 0);
        assert_eq!(state.balances.values().sum::<u128>(), state.total_supply);
    }
}
