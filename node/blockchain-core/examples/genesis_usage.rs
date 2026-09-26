//! Core genesis mechanics example. This does not generate an approved mainnet genesis.
use dytallix_node::{
    genesis::GenesisConfig,
    genesis_integration::{GenesisBlockCreator, GenesisInitializer},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = GenesisConfig::mainnet();
    config._validate()?;
    let (block, accounts, validators) = GenesisInitializer::initialize_blockchain(config.clone())?;
    println!(
        "Legacy core fixture: block {}, {} accounts, {} validators",
        block.header.number,
        accounts.len(),
        validators.len()
    );
    println!(
        "Allocation total: {} raw units",
        config._total_dgt_supply()?
    );
    let timestamp = u64::try_from(config.network.genesis_time.timestamp())?;
    let creator = GenesisBlockCreator::new(config.clone());
    for allocation in &config.dgt_allocations {
        println!(
            "{}: {} vested, {} locked raw units at {}",
            allocation.address,
            creator.get_vested_amount_at(&allocation.address, timestamp)?,
            creator.get_locked_amount_at(&allocation.address, timestamp)?,
            timestamp
        );
    }
    println!("Token precision, funded stake, consensus, and launch approval remain unresolved.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_example_genesis_creation() {
        let config = GenesisConfig::mainnet();
        assert!(config._validate().is_ok());
        let block = GenesisBlockCreator::new(config)
            .create_genesis_block()
            .unwrap();
        assert_eq!(block.header.number, 0);
        assert_eq!(block.transactions.len(), 5);
    }
    #[test]
    fn test_vesting_edge_cases() {
        let config = GenesisConfig::mainnet();
        let allocation = config.dgt_allocations[0].clone();
        let creator = GenesisBlockCreator::new(config);
        assert_eq!(
            creator
                .get_vested_amount_at(&allocation.address, 0)
                .unwrap(),
            allocation.amount
        );
        assert_eq!(
            creator
                .get_vested_amount_at(&"dyt1unknown".into(), 0)
                .unwrap(),
            0
        );
    }
    #[test]
    fn test_burn_calculations() {
        let config = GenesisConfig::mainnet();
        let fee = 200u128;
        assert_eq!(
            fee * u128::from(config.burn_rules.transaction_fee_burn_rate) / 100,
            fee
        );
        assert_eq!(
            fee * u128::from(config.burn_rules.ai_service_fee_burn_rate) / 100,
            100
        );
    }
}
