/*
Genesis Block Integration for Consensus Engine

Integrates genesis configuration with the consensus engine to initialize
the blockchain state with proper token allocations, validators, and parameters.
*/

use crate::genesis::GenesisConfig;
use crate::types::{
    AccountState, Address, Amount, Block, BlockHeader, Hash, Transaction, ValidatorInfo,
};
use chrono::Utc;
use sha3::{Digest, Sha3_256};
use std::collections::HashMap; // retained for future extensions

/// Genesis block creator for Dytallix blockchain
pub struct GenesisBlockCreator {
    config: GenesisConfig,
}

impl GenesisBlockCreator {
    /// Create a new genesis block creator with the given configuration
    pub fn new(config: GenesisConfig) -> Self {
        Self { config }
    }

    /// Create the genesis block
    pub fn create_genesis_block(&self) -> Result<Block, String> {
        // Validate the configuration first
        self.config._validate()?;

        // Create genesis transactions for DGT allocations
        let genesis_transactions = self.create_genesis_transactions()?;

        // Calculate the merkle root of transactions
        let transactions_root = BlockHeader::calculate_transactions_root(&genesis_transactions);

        // Create the genesis state root (initially empty state + allocations)
        let state_root = self.calculate_genesis_state_root()?;

        // Genesis timestamp
        let genesis_timestamp = self.config.network.genesis_time.timestamp() as u64;

        // Create genesis block header
        let header = BlockHeader {
            number: 0,                   // Genesis block is block 0
            parent_hash: "0".repeat(64), // No parent for genesis block
            transactions_root,
            state_root,
            timestamp: genesis_timestamp,
            validator: "genesis".to_string(), // Special genesis validator
            signature: crate::types::PQCBlockSignature {
                signature: dytallix_pqc::Signature {
                    data: vec![0u8; 32], // Genesis signature placeholder
                    algorithm: dytallix_pqc::SignatureAlgorithm::Dilithium5,
                },
                public_key: vec![0u8; 32], // Genesis public key placeholder
            },
            nonce: 0,
        };

        let genesis_block = Block {
            header,
            transactions: genesis_transactions,
        };

        Ok(genesis_block)
    }

    /// Create genesis transactions for initial DGT allocations
    fn create_genesis_transactions(&self) -> Result<Vec<Transaction>, String> {
        let mut transactions = Vec::new();

        // Create mint transactions for each DGT allocation
        for (index, allocation) in self.config.dgt_allocations.iter().enumerate() {
            let tx = Transaction::Transfer(crate::types::TransferTransaction {
                hash: format!("genesis_mint_{index}"),
                from: "genesis_mint".to_string(), // Special genesis minter address
                to: allocation.address.clone(),
                amount: allocation.amount,
                fee: 0, // No fees for genesis transactions
                nonce: index as u64,
                timestamp: self.config.network.genesis_time.timestamp() as u64,
                signature: crate::types::PQCTransactionSignature {
                    signature: dytallix_pqc::Signature {
                        data: vec![0u8; 32], // Genesis signature placeholder
                        algorithm: dytallix_pqc::SignatureAlgorithm::Dilithium5,
                    },
                    public_key: vec![0u8; 32], // Genesis public key placeholder
                },
                ai_risk_score: Some(0.0), // Genesis transactions are safe
            });
            transactions.push(tx);
        }

        Ok(transactions)
    }

    /// Calculate the genesis state root after applying all allocations
    fn calculate_genesis_state_root(&self) -> Result<Hash, String> {
        let mut state_data = String::new();

        // Add all DGT allocations to the state calculation
        for allocation in &self.config.dgt_allocations {
            state_data.push_str(&format!("{}:{}", allocation.address, allocation.amount));
        }

        // Add genesis configuration hash
        state_data.push_str(&format!("network:{}", self.config.network.chain_id));
        state_data.push_str(&format!("validators:{}", self.config.validators.len()));

        // Calculate SHA3-256 hash
        let mut hasher = Sha3_256::new();
        hasher.update(state_data.as_bytes());
        let hash = hasher.finalize();

        Ok(hex::encode(hash))
    }

    /// Initialize account states from genesis configuration
    pub fn create_genesis_accounts(&self) -> Result<HashMap<Address, AccountState>, String> {
        self.config._validate()?;
        let mut accounts = HashMap::new();

        // Create accounts for DGT allocations
        for allocation in &self.config.dgt_allocations {
            let account_state = AccountState {
                balance: allocation.amount,
                nonce: 0,
                code: None,
                storage: HashMap::new(),
                reputation_score: 1000, // Max reputation for genesis accounts
                last_ai_analysis: Some(self.config.network.genesis_time.timestamp() as u64),
            };
            accounts.insert(allocation.address.clone(), account_state);
        }

        // Create accounts for validators
        for validator in &self.config.validators {
            // Skip if validator account already exists (might be in DGT allocations)
            if !accounts.contains_key(&validator.address) {
                let account_state = AccountState {
                    balance: 0, // Validators might not have initial balance beyond stake
                    nonce: 0,
                    code: None,
                    storage: HashMap::new(),
                    reputation_score: 1000, // Max reputation for genesis validators
                    last_ai_analysis: Some(self.config.network.genesis_time.timestamp() as u64),
                };
                accounts.insert(validator.address.clone(), account_state);
            }
        }

        Ok(accounts)
    }

    /// Get the genesis configuration
    pub fn config(&self) -> &GenesisConfig {
        &self.config
    }

    pub fn get_vested_amount_at(
        &self,
        address: &Address,
        timestamp: u64,
    ) -> Result<Amount, String> {
        self.config._get_vested_amount(address, timestamp)
    }

    pub fn get_locked_amount_at(
        &self,
        address: &Address,
        timestamp: u64,
    ) -> Result<Amount, String> {
        self.config._get_locked_amount(address, timestamp)
    }

    /// Diagnostic wall-clock query. Consensus must supply its agreed timestamp.
    pub fn get_current_vested_amount(&self, address: &Address) -> Result<Amount, String> {
        let timestamp =
            u64::try_from(Utc::now().timestamp()).map_err(|_| "Clock precedes Unix epoch")?;
        self.get_vested_amount_at(address, timestamp)
    }

    /// Diagnostic wall-clock query. Consensus must supply its agreed timestamp.
    pub fn get_current_locked_amount(&self, address: &Address) -> Result<Amount, String> {
        let timestamp =
            u64::try_from(Utc::now().timestamp()).map_err(|_| "Clock precedes Unix epoch")?;
        self.get_locked_amount_at(address, timestamp)
    }

    /// Cumulative vested allocation check only. This does not check prior spending.
    pub fn can_transfer_at(
        &self,
        address: &Address,
        amount: Amount,
        timestamp: u64,
    ) -> Result<bool, String> {
        Ok(self.get_vested_amount_at(address, timestamp)? >= amount)
    }

    /// Diagnostic compatibility query, not transaction authorization.
    pub fn can_transfer(&self, address: &Address, amount: Amount) -> Result<bool, String> {
        Ok(self.get_current_vested_amount(address)? >= amount)
    }
}

/// Genesis initialization helper for the consensus engine
pub struct GenesisInitializer;

/// Result of blockchain initialization
pub type GenesisInitializationResult =
    Result<(Block, HashMap<Address, AccountState>, Vec<ValidatorInfo>), String>;

impl GenesisInitializer {
    /// Initialize the blockchain with genesis configuration
    pub fn initialize_blockchain(config: GenesisConfig) -> GenesisInitializationResult {
        // Create genesis block
        let creator = GenesisBlockCreator::new(config.clone());
        let genesis_block = creator.create_genesis_block()?;

        // Create initial account states
        let genesis_accounts = creator.create_genesis_accounts()?;

        // Get initial validators
        let validators = config.validators.clone();

        Ok((genesis_block, genesis_accounts, validators))
    }

    /// Validate genesis block against configuration
    pub fn validate_genesis_block(block: &Block, config: &GenesisConfig) -> Result<(), String> {
        config._validate()?;
        // Check if it's actually block 0
        if block.header.number != 0 {
            return Err("Genesis block must have number 0".to_string());
        }

        // Check timestamp matches configuration
        let expected_timestamp = config.network.genesis_time.timestamp() as u64;
        if block.header.timestamp != expected_timestamp {
            return Err(format!(
                "Genesis timestamp mismatch: expected {}, got {}",
                expected_timestamp, block.header.timestamp
            ));
        }

        // Check that we have the correct number of genesis transactions
        let expected_tx_count = config.dgt_allocations.len();
        if block.transactions.len() != expected_tx_count {
            return Err(format!(
                "Expected {} genesis transactions, got {}",
                expected_tx_count,
                block.transactions.len()
            ));
        }

        // Validate each transaction corresponds to a DGT allocation
        for (index, transaction) in block.transactions.iter().enumerate() {
            if let Transaction::Transfer(tx) = transaction {
                let allocation = &config.dgt_allocations[index];
                if tx.to != allocation.address || tx.amount != allocation.amount {
                    return Err(format!(
                        "Genesis transaction {index} does not match allocation"
                    ));
                }
            } else {
                return Err(format!("Genesis transaction {index} is not a transfer"));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genesis::{
        BurnRulesConfig, DGTAllocation, DRTEmissionConfig, EmissionBreakdown, GovernanceConfig,
        NetworkConfig, StakingConfig, VestingSchedule,
    };

    // Synthetic raw units satisfy the legacy core total. They define no DGT precision.
    const FIXTURE_TOTAL: Amount = 1_000_000_000_000_000_000;
    const FIXTURE_START: u64 = 1_700_000_000;
    const FIXTURE_STAKE: Amount = 1_000;
    const ECOSYSTEM: &str = "fixture-ecosystem";
    const TEAM: &str = "fixture-team";
    const PUBLIC: &str = "fixture-public";
    const PRIVATE: &str = "fixture-private";
    const RESERVE: &str = "fixture-reserve";
    const FIXTURE_ALLOCATIONS: [(&str, Amount); 5] = [
        (ECOSYSTEM, 300_000_000_000_000_000),
        (TEAM, 200_000_000_000_000_000),
        (PUBLIC, 150_000_000_000_000_000),
        (PRIVATE, 150_000_000_000_000_000),
        (RESERVE, 200_000_000_000_000_000),
    ];

    /// Independent fixture with approved bucket shares and synthetic policy inputs.
    /// See docs/mainnet/genesis-fixture-contract.md for its qualification limits.
    fn synthetic_genesis() -> GenesisConfig {
        GenesisConfig {
            network: NetworkConfig {
                name: "genesis-integration-fixture".into(),
                chain_id: "genesis-integration-fixture-1".into(),
                genesis_time: chrono::DateTime::from_timestamp(FIXTURE_START as i64, 0).unwrap(),
            },
            dgt_allocations: FIXTURE_ALLOCATIONS
                .iter()
                .map(|(address, amount)| DGTAllocation {
                    address: (*address).into(),
                    amount: *amount,
                    vesting: match *address {
                        TEAM => Some(VestingSchedule {
                            total_amount: *amount,
                            start_time: FIXTURE_START,
                            cliff_duration: 10,
                            vesting_duration: 30,
                        }),
                        ECOSYSTEM => Some(VestingSchedule {
                            total_amount: *amount,
                            start_time: FIXTURE_START,
                            cliff_duration: 0,
                            vesting_duration: 30,
                        }),
                        _ => None,
                    },
                })
                .collect(),
            // This legacy four-field split is a test input, not approved DRT policy.
            drt_emission: DRTEmissionConfig {
                annual_inflation_rate: 0,
                initial_supply: 0,
                emission_breakdown: EmissionBreakdown {
                    block_rewards: 60,
                    staking_rewards: 25,
                    ai_module_incentives: 10,
                    bridge_operations: 5,
                },
            },
            burn_rules: BurnRulesConfig {
                transaction_fee_burn_rate: 0,
                ai_service_fee_burn_rate: 0,
                bridge_fee_burn_rate: 0,
            },
            validators: vec![ValidatorInfo {
                address: RESERVE.into(),
                stake: FIXTURE_STAKE,
                public_key: vec![7; 32],
                signature_algorithm: dytallix_pqc::SignatureAlgorithm::Dilithium5,
                active: true,
                commission: 0,
            }],
            governance: GovernanceConfig {
                proposal_threshold: 1,
                voting_period: 10,
                quorum_threshold: 5_000,
                pass_threshold: 5_000,
            },
            staking: StakingConfig {
                minimum_validator_stake: FIXTURE_STAKE,
                max_validators: 1,
                double_sign_slash_rate: 0,
                downtime_slash_rate: 0,
                offline_threshold: 10,
                emission_per_block: 0,
            },
            genesis_hash: None,
        }
    }

    #[test]
    fn test_genesis_block_creation() {
        let config = synthetic_genesis();
        let creator = GenesisBlockCreator::new(config.clone());

        let genesis_block = creator.create_genesis_block().unwrap();

        // Verify genesis block properties
        assert_eq!(genesis_block.header.number, 0);
        assert_eq!(genesis_block.header.parent_hash, "0".repeat(64));
        assert_eq!(
            genesis_block.transactions.len(),
            config.dgt_allocations.len()
        );

        // Verify all allocations are present as transactions
        for (index, allocation) in config.dgt_allocations.iter().enumerate() {
            if let Transaction::Transfer(tx) = &genesis_block.transactions[index] {
                assert_eq!(tx.to, allocation.address);
                assert_eq!(tx.amount, allocation.amount);
                assert_eq!(tx.fee, 0);
            } else {
                panic!("Expected transfer transaction");
            }
        }
    }

    #[test]
    fn test_genesis_accounts_creation() {
        let config = synthetic_genesis();
        let creator = GenesisBlockCreator::new(config.clone());

        let accounts = creator.create_genesis_accounts().unwrap();

        // Should have accounts for all allocations and validators
        assert!(accounts.len() >= config.dgt_allocations.len());

        // Check each allocation has correct balance
        for allocation in &config.dgt_allocations {
            let account = accounts.get(&allocation.address).unwrap();
            assert_eq!(account.balance, allocation.amount);
            assert_eq!(account.nonce, 0);
            assert_eq!(account.reputation_score, 1000);
        }
    }

    #[test]
    fn test_vesting_calculations() {
        let creator = GenesisBlockCreator::new(synthetic_genesis());
        let team = TEAM.to_string();
        let total = 200_000_000_000_000_000;
        // Fixed offsets cover before start, both cliff edges, an interior point,
        // and both final-release edges. Expected values do not call schedule code.
        for (timestamp, expected) in [
            (FIXTURE_START - 1, 0),
            (FIXTURE_START, 0),
            (FIXTURE_START + 9, 0),
            (FIXTURE_START + 10, 0),
            (FIXTURE_START + 11, 10_000_000_000_000_000),
            (FIXTURE_START + 20, 100_000_000_000_000_000),
            (FIXTURE_START + 29, 190_000_000_000_000_000),
            (FIXTURE_START + 30, total),
            (FIXTURE_START + 31, total),
        ] {
            let vested = creator.get_vested_amount_at(&team, timestamp).unwrap();
            let locked = creator.get_locked_amount_at(&team, timestamp).unwrap();
            assert_eq!(vested, expected, "timestamp {timestamp}");
            assert_eq!(locked, total - expected, "timestamp {timestamp}");
            assert_eq!(vested.checked_add(locked), Some(total));
        }
        assert_eq!(
            creator
                .get_vested_amount_at(&PUBLIC.to_string(), FIXTURE_START)
                .unwrap(),
            150_000_000_000_000_000
        );
        assert_eq!(
            creator
                .get_locked_amount_at(&PUBLIC.to_string(), FIXTURE_START)
                .unwrap(),
            0
        );
    }

    #[test]
    fn test_transfer_permissions() {
        let creator = GenesisBlockCreator::new(synthetic_genesis());
        // can_transfer_at checks cumulative vested allocation only. It does not
        // spend funds or authorize runtime transfers against prior spending.
        for (address, timestamp, available) in [
            (PUBLIC, FIXTURE_START, 150_000_000_000_000_000),
            (TEAM, FIXTURE_START + 9, 0),
            (TEAM, FIXTURE_START + 10, 0),
            (TEAM, FIXTURE_START + 11, 10_000_000_000_000_000),
            (TEAM, FIXTURE_START + 20, 100_000_000_000_000_000),
            (TEAM, FIXTURE_START + 30, 200_000_000_000_000_000),
            (ECOSYSTEM, FIXTURE_START, 0),
            (ECOSYSTEM, FIXTURE_START + 10, 100_000_000_000_000_000),
            ("fixture-unknown", FIXTURE_START + 30, 0),
        ] {
            let address = address.to_string();
            assert!(creator
                .can_transfer_at(&address, available, timestamp)
                .unwrap());
            assert!(!creator
                .can_transfer_at(&address, available + 1, timestamp)
                .unwrap());
        }
    }

    #[test]
    fn test_genesis_validation() {
        let config = synthetic_genesis();
        let creator = GenesisBlockCreator::new(config.clone());
        let genesis_block = creator.create_genesis_block().unwrap();

        // Should validate successfully
        GenesisInitializer::validate_genesis_block(&genesis_block, &config).unwrap();

        // Test with wrong block number
        let mut invalid_block = genesis_block.clone();
        invalid_block.header.number = 1;
        assert!(GenesisInitializer::validate_genesis_block(&invalid_block, &config).is_err());
    }

    #[test]
    fn test_blockchain_initialization() {
        let config = synthetic_genesis();
        let (genesis_block, accounts, validators) =
            GenesisInitializer::initialize_blockchain(config.clone()).unwrap();

        assert_eq!(genesis_block.header.number, 0);
        assert_eq!(genesis_block.header.timestamp, FIXTURE_START);
        assert_eq!(accounts.len(), 5);
        assert_eq!(validators.len(), 1);
        assert_eq!(validators[0].address, RESERVE);
        assert_eq!(validators[0].stake, FIXTURE_STAKE);
        for (address, expected) in FIXTURE_ALLOCATIONS {
            assert_eq!(accounts[address].balance, expected);
        }
        assert_eq!(
            accounts
                .values()
                .map(|account| account.balance)
                .sum::<Amount>(),
            FIXTURE_TOTAL
        );
        assert_eq!(
            config
                .dgt_allocations
                .iter()
                .map(|allocation| allocation.amount * 100 / FIXTURE_TOTAL)
                .collect::<Vec<_>>(),
            vec![30, 20, 15, 15, 20]
        );

        // Allocation accounts precede staking. Runtime initialization must debit
        // the fixture validator's funded allocation once and preserve total DGT.
        let state = crate::runtime::RuntimeState::from_genesis(&config).unwrap();
        assert_eq!(state.total_supply, FIXTURE_TOTAL);
        assert_eq!(state.staking.total_stake, FIXTURE_STAKE);
        assert_eq!(
            state.balances[RESERVE],
            200_000_000_000_000_000 - FIXTURE_STAKE
        );
        assert_eq!(state.balances.len(), 5);
        assert!(!state.balances.contains_key("dyt1genesis"));
        assert!(state.drt_balances.is_empty());
        assert_eq!(
            state
                .balances
                .values()
                .sum::<Amount>()
                .checked_add(state.staking.total_stake),
            Some(FIXTURE_TOTAL)
        );

        // JSON import preserves raw amounts and schedule times. Repeated
        // initialization from the imported fixture must produce the same balances.
        let imported = GenesisConfig::from_json(&config._to_json().unwrap()).unwrap();
        let restored = crate::runtime::RuntimeState::from_genesis(&imported).unwrap();
        assert_eq!(restored.balances, state.balances);
        assert_eq!(restored.total_supply, state.total_supply);
        assert_eq!(restored.staking.total_stake, state.staking.total_stake);
        let imported_creator = GenesisBlockCreator::new(imported);
        assert_eq!(
            imported_creator
                .get_vested_amount_at(&TEAM.to_string(), FIXTURE_START + 20)
                .unwrap(),
            100_000_000_000_000_000
        );
    }
    #[test]
    fn explicit_vesting_time_checks_cliff_linear_interval_and_end() {
        let config = synthetic_genesis();
        let allocation = &config.dgt_allocations[1];
        let schedule = allocation.vesting.as_ref().unwrap().clone();
        let address = allocation.address.clone();
        let total = allocation.amount;
        let creator = GenesisBlockCreator::new(config);
        let cliff = schedule.start_time + schedule.cliff_duration;
        assert_eq!(creator.get_vested_amount_at(&address, cliff).unwrap(), 0);
        assert!(!creator.can_transfer_at(&address, 1, cliff).unwrap());
        let half = cliff + (schedule.vesting_duration - schedule.cliff_duration) / 2;
        assert_eq!(
            creator.get_vested_amount_at(&address, half).unwrap(),
            total / 2
        );
        assert_eq!(
            creator.get_locked_amount_at(&address, half).unwrap(),
            total - total / 2
        );
        assert!(creator
            .can_transfer_at(
                &address,
                total,
                schedule.start_time + schedule.vesting_duration
            )
            .unwrap());
    }

    #[test]
    fn direct_account_creation_rejects_invalid_allocations_and_dates() {
        let mut config = synthetic_genesis();
        config.dgt_allocations[1].address = config.dgt_allocations[0].address.clone();
        assert!(crate::runtime::RuntimeState::from_genesis(&config).is_err());
        let creator = GenesisBlockCreator::new(config);
        assert!(creator.create_genesis_accounts().is_err());
        assert!(creator.create_genesis_block().is_err());
        let mut config = synthetic_genesis();
        config.network.genesis_time = chrono::DateTime::from_timestamp(-1, 0).unwrap();
        assert!(GenesisBlockCreator::new(config)
            .create_genesis_accounts()
            .is_err());
    }
}
