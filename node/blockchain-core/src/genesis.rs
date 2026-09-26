//! Legacy core genesis template and checked allocation arithmetic.
//! Token precision and production genesis approval remain unresolved.

use crate::types::{Address, Amount, BlockNumber, Hash, Timestamp, ValidatorInfo};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// JSON amount codec. Raw JSON preserves integer literals without a float conversion.
/// Binary serializers retain the previous u128 representation.
pub(crate) mod serde_genesis_amount {
    use serde::de::Error;
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(value: &u128, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serializer.serialize_str(&value.to_string())
        } else {
            serializer.serialize_u128(*value)
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u128, D::Error> {
        if !deserializer.is_human_readable() {
            return u128::deserialize(deserializer);
        }
        let raw = Box::<serde_json::value::RawValue>::deserialize(deserializer)?;
        let text = raw.get();
        if text.starts_with('"') {
            let value: String = serde_json::from_str(text).map_err(D::Error::custom)?;
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(D::Error::custom("Amount must contain decimal digits only"));
            }
            value.parse().map_err(D::Error::custom)
        } else {
            // The typed integer parser rejects fractions, exponents, negatives and overflow.
            serde_json::from_str::<u128>(text).map_err(D::Error::custom)
        }
    }
}

/// Vesting schedule for token allocations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VestingSchedule {
    /// Total amount to be vested
    #[serde(with = "crate::genesis::serde_genesis_amount")]
    pub total_amount: Amount,
    /// Cliff period in seconds (tokens locked completely)
    pub cliff_duration: u64,
    /// Total vesting duration in seconds
    pub vesting_duration: u64,
    /// Start timestamp for vesting
    pub start_time: Timestamp,
}

impl VestingSchedule {
    /// Equal cliff and duration preserve an instantaneous unlock at the cliff.
    pub fn validate(&self) -> Result<(), String> {
        if self.vesting_duration < self.cliff_duration {
            return Err("Vesting duration must not precede cliff duration".into());
        }
        self.start_time
            .checked_add(self.vesting_duration)
            .ok_or("Vesting end timestamp exceeds u64")?;
        Ok(())
    }

    /// Floor of linear vesting after the cliff, in the allocation's units.
    pub fn _vested_amount(&self, current_time: Timestamp) -> Result<Amount, String> {
        self.validate()?;
        if current_time < self.start_time {
            return Ok(0);
        }
        let elapsed = current_time - self.start_time;
        if elapsed < self.cliff_duration {
            return Ok(0);
        }
        if elapsed >= self.vesting_duration {
            return Ok(self.total_amount);
        }
        let numerator = u128::from(elapsed - self.cliff_duration);
        let denominator = u128::from(self.vesting_duration - self.cliff_duration);
        // A = q*p+r. Here e<p and r<p<=u64::MAX, so r*e fits u128.
        let whole = (self.total_amount / denominator)
            .checked_mul(numerator)
            .ok_or("Vesting whole term exceeds u128")?;
        let remainder = (self.total_amount % denominator)
            .checked_mul(numerator)
            .ok_or("Vesting remainder term exceeds u128")?
            / denominator;
        whole
            .checked_add(remainder)
            .ok_or_else(|| "Vesting amount exceeds u128".into())
    }

    pub fn _locked_amount(&self, current_time: Timestamp) -> Result<Amount, String> {
        Ok(self.total_amount - self._vested_amount(current_time)?)
    }
}

/// DGT (Governance Token) allocation with vesting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DGTAllocation {
    /// Recipient address
    pub address: Address,
    /// Allocation amount
    #[serde(with = "crate::genesis::serde_genesis_amount")]
    pub amount: Amount,
    /// Vesting schedule (None = unlocked immediately)
    pub vesting: Option<VestingSchedule>,
}

/// Validate allocation identity, schedule domain, and total arithmetic.
/// This does not select a token decimal scale or approve an economic allocation.
pub(crate) fn validate_allocations(allocations: &[DGTAllocation]) -> Result<Amount, String> {
    let mut addresses = HashSet::new();
    let mut total = 0u128;
    for allocation in allocations {
        if allocation.address.trim().is_empty() || !addresses.insert(&allocation.address) {
            return Err("Allocation addresses must be nonempty and unique".into());
        }
        if let Some(schedule) = &allocation.vesting {
            schedule.validate()?;
            if schedule.total_amount != allocation.amount {
                return Err("Vesting total must equal its allocation".into());
            }
        }
        total = total
            .checked_add(allocation.amount)
            .ok_or("Allocation total exceeds u128")?;
    }
    Ok(total)
}

/// DRT emission configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DRTEmissionConfig {
    /// Annual inflation rate (5% = 500 basis points)
    pub annual_inflation_rate: u16,
    /// Initial supply (0 for DRT)
    #[serde(with = "crate::genesis::serde_genesis_amount")]
    pub initial_supply: Amount,
    /// Emission breakdown percentages (must sum to 100)
    pub emission_breakdown: EmissionBreakdown,
}

/// DRT emission distribution breakdown
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmissionBreakdown {
    /// Block rewards percentage (60%)
    pub block_rewards: u8,
    /// Staking rewards percentage (25%)
    pub staking_rewards: u8,
    /// AI module incentives percentage (10%)
    pub ai_module_incentives: u8,
    /// Bridge operations percentage (5%)
    pub bridge_operations: u8,
}

impl EmissionBreakdown {
    /// Validate that percentages sum to 100
    pub fn _is_valid(&self) -> bool {
        u16::from(self.block_rewards)
            + u16::from(self.staking_rewards)
            + u16::from(self.ai_module_incentives)
            + u16::from(self.bridge_operations)
            == 100
    }
}

/// Burn rules configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurnRulesConfig {
    /// Percentage of transaction fees to burn (100% = all fees burned)
    pub transaction_fee_burn_rate: u8,
    /// Percentage of AI service fees to burn (50%)
    pub ai_service_fee_burn_rate: u8,
    /// Percentage of bridge fees to burn (75%)
    pub bridge_fee_burn_rate: u8,
}

/// Governance parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceConfig {
    /// Minimum DGT tokens required to create a proposal
    #[serde(with = "crate::genesis::serde_genesis_amount")]
    pub proposal_threshold: Amount,
    /// Voting period in blocks
    pub voting_period: BlockNumber,
    /// Minimum quorum for proposal to pass (basis points)
    pub quorum_threshold: u16,
    /// Percentage required for proposal to pass (basis points)
    pub pass_threshold: u16,
}

/// Staking parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakingConfig {
    /// Minimum stake required to become a validator
    #[serde(with = "crate::genesis::serde_genesis_amount")]
    pub minimum_validator_stake: Amount,
    /// Maximum number of validators
    pub max_validators: u32,
    /// Slash percentage for double signing (basis points)
    pub double_sign_slash_rate: u16,
    /// Slash percentage for downtime (basis points)
    pub downtime_slash_rate: u16,
    /// Blocks to consider validator offline
    pub offline_threshold: BlockNumber,
    /// Emission rate per block (in uDRT)
    #[serde(with = "crate::genesis::serde_genesis_amount")]
    pub emission_per_block: u128,
}

impl StakingConfig {
    /// Convert to staking module parameters
    pub fn to_staking_params(&self) -> crate::staking::StakingParams {
        // Use defaults for fields not represented in StakingConfig
        let defaults = crate::staking::StakingParams::default();
        crate::staking::StakingParams {
            max_validators: self.max_validators,
            min_self_stake: self.minimum_validator_stake,
            slash_double_sign: self.double_sign_slash_rate,
            slash_downtime: self.downtime_slash_rate,
            emission_per_block: self.emission_per_block,
            // Newly required fields
            downtime_threshold: self.offline_threshold,
            signed_blocks_window: defaults.signed_blocks_window,
            min_signed_per_window: defaults.min_signed_per_window,
        }
    }
}

/// Network metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Network name
    pub name: String,
    /// Chain ID
    pub chain_id: String,
    /// Genesis timestamp
    pub genesis_time: DateTime<Utc>,
}

/// Complete genesis configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisConfig {
    /// Network configuration
    pub network: NetworkConfig,
    /// DGT token allocations
    pub dgt_allocations: Vec<DGTAllocation>,
    /// DRT emission configuration
    pub drt_emission: DRTEmissionConfig,
    /// Burn rules
    pub burn_rules: BurnRulesConfig,
    /// Initial validator set
    pub validators: Vec<ValidatorInfo>,
    /// Governance parameters
    pub governance: GovernanceConfig,
    /// Staking parameters
    pub staking: StakingConfig,
    /// Genesis block hash (calculated)
    pub genesis_hash: Option<Hash>,
}

impl GenesisConfig {
    /// Legacy template only. The historical name does not establish mainnet readiness.
    pub fn mainnet() -> Self {
        Self::_mainnet()
    }

    /// Import from JSON string
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        Self::_from_json(json)
    }

    /// Legacy template only. The historical name does not establish mainnet readiness.
    pub fn _mainnet() -> Self {
        let genesis_time = DateTime::parse_from_rfc3339("2025-08-03T19:00:26.000000000Z")
            .unwrap()
            .with_timezone(&Utc);

        // DGT allocations with vesting schedules
        let dgt_allocations = vec![
            // Community Treasury - historical allocation, unlocked
            DGTAllocation {
                address: "0xCommunityTreasury".to_string(),
                amount: 400_000_000_000_000_000, // Historical raw units; precision unresolved
                vesting: None,
            },
            // Staking Rewards - historical allocation, 4-year linear vesting
            DGTAllocation {
                address: "0xStakingRewards".to_string(),
                amount: 250_000_000_000_000_000, // Historical raw units
                vesting: Some(VestingSchedule {
                    total_amount: 250_000_000_000_000_000,
                    cliff_duration: 0, // No cliff for staking rewards
                    vesting_duration: 4 * 365 * 24 * 60 * 60, // 4 years in seconds
                    start_time: genesis_time.timestamp() as u64,
                }),
            },
            // Dev Team - historical allocation, 1-year cliff + 3-year linear vesting
            DGTAllocation {
                address: "0xDevTeam".to_string(),
                amount: 150_000_000_000_000_000, // Historical raw units
                vesting: Some(VestingSchedule {
                    total_amount: 150_000_000_000_000_000,
                    cliff_duration: 365 * 24 * 60 * 60, // 1 year cliff
                    vesting_duration: 4 * 365 * 24 * 60 * 60, // Total 4 years (1 cliff + 3 vesting)
                    start_time: genesis_time.timestamp() as u64,
                }),
            },
            // Validators - historical allocation, 6-month cliff + 2-year linear vesting
            DGTAllocation {
                address: "0xValidators".to_string(),
                amount: 100_000_000_000_000_000, // Historical raw units
                vesting: Some(VestingSchedule {
                    total_amount: 100_000_000_000_000_000,
                    cliff_duration: 6 * 30 * 24 * 60 * 60, // 6 months cliff (approx)
                    vesting_duration: (6 + 24) * 30 * 24 * 60 * 60, // Total 2.5 years (6m cliff + 2y vesting)
                    start_time: genesis_time.timestamp() as u64,
                }),
            },
            // Ecosystem Fund - historical allocation, 5-year linear vesting
            DGTAllocation {
                address: "0xEcosystemFund".to_string(),
                amount: 100_000_000_000_000_000, // Historical raw units
                vesting: Some(VestingSchedule {
                    total_amount: 100_000_000_000_000_000,
                    cliff_duration: 0, // No cliff for ecosystem fund
                    vesting_duration: 5 * 365 * 24 * 60 * 60, // 5 years in seconds
                    start_time: genesis_time.timestamp() as u64,
                }),
            },
        ];

        // DRT emission configuration (~5% annual inflation)
        let drt_emission = DRTEmissionConfig {
            annual_inflation_rate: 500, // 5% in basis points
            initial_supply: 0,          // DRT starts with 0 supply
            emission_breakdown: EmissionBreakdown {
                block_rewards: 60,
                staking_rewards: 25,
                ai_module_incentives: 10,
                bridge_operations: 5,
            },
        };

        // Burn rules configuration
        let burn_rules = BurnRulesConfig {
            transaction_fee_burn_rate: 100, // 100% of transaction fees burned
            ai_service_fee_burn_rate: 50,   // 50% of AI service fees burned
            bridge_fee_burn_rate: 75,       // 75% of bridge fees burned
        };

        // Initial validator set (placeholder keys for now)
        let validators = vec![
            ValidatorInfo {
                address: "dyt1validator1000000000000000000000000000".to_string(),
                // Historical raw stake. Token precision remains unresolved.
                stake: 32_000_000_000_000u128, // Historical raw units
                public_key: vec![0u8; 32],     // Placeholder - would be real keys in production
                signature_algorithm: dytallix_pqc::SignatureAlgorithm::Dilithium5,
                active: true,
                commission: 500, // 5% commission
            },
            ValidatorInfo {
                address: "dyt1validator2000000000000000000000000000".to_string(),
                stake: 32_000_000_000_000u128,
                public_key: vec![1u8; 32],
                signature_algorithm: dytallix_pqc::SignatureAlgorithm::Dilithium5,
                active: true,
                commission: 500,
            },
            ValidatorInfo {
                address: "dyt1validator3000000000000000000000000000".to_string(),
                stake: 32_000_000_000_000u128,
                public_key: vec![2u8; 32],
                signature_algorithm: dytallix_pqc::SignatureAlgorithm::Dilithium5,
                active: true,
                commission: 500,
            },
        ];

        // Governance configuration
        let governance = GovernanceConfig {
            proposal_threshold: 1_000_000_000_000_000_000, // Historical raw-unit proposal threshold
            voting_period: 50400,                          // ~7 days assuming 12s block time
            quorum_threshold: 3333,                        // 33.33% quorum required
            pass_threshold: 5000,                          // 50% majority required
        };

        // Staking configuration
        let staking = StakingConfig {
            minimum_validator_stake: 32_000_000_000_000u128,
            max_validators: 100,
            double_sign_slash_rate: 500,   // 5% slash for double signing
            downtime_slash_rate: 100,      // 1% slash for downtime
            offline_threshold: 300,        // 300 blocks (~1 hour) to be considered offline
            emission_per_block: 1_000_000, // 1 DRT per block in uDRT
        };

        Self {
            network: NetworkConfig {
                name: "dytallix-mainnet".to_string(),
                chain_id: "dytallix-mainnet-1".to_string(),
                genesis_time,
            },
            dgt_allocations,
            drt_emission,
            burn_rules,
            validators,
            governance,
            staking,
            genesis_hash: None, // Will be calculated when genesis block is created
        }
    }

    /// Validate the genesis configuration
    pub fn _validate(&self) -> Result<(), String> {
        if self.network.genesis_time.timestamp() < 0 {
            return Err("Genesis timestamp must not precede the Unix epoch".into());
        }
        // Preserve the legacy raw-unit total until the unit contract is approved.
        let total_dgt = validate_allocations(&self.dgt_allocations)?;
        if total_dgt != 1_000_000_000_000_000_000 {
            return Err(format!(
                "Legacy core allocation total must be 1000000000000000000 raw units, got {total_dgt}"
            ));
        }

        // Validate emission breakdown
        if !self.drt_emission.emission_breakdown._is_valid() {
            return Err("DRT emission breakdown percentages must sum to 100".to_string());
        }

        // Validate burn rates are <= 100%
        if self.burn_rules.transaction_fee_burn_rate > 100
            || self.burn_rules.ai_service_fee_burn_rate > 100
            || self.burn_rules.bridge_fee_burn_rate > 100
        {
            return Err("Burn rates cannot exceed 100%".to_string());
        }

        // Validate governance parameters
        if self.governance.quorum_threshold > 10000 || self.governance.pass_threshold > 10000 {
            return Err(
                "Governance thresholds cannot exceed 100% (10000 basis points)".to_string(),
            );
        }

        // Validate staking parameters
        if self.staking.double_sign_slash_rate > 10000 || self.staking.downtime_slash_rate > 10000 {
            return Err("Slash rates cannot exceed 100% (10000 basis points)".to_string());
        }

        // Validate validators have minimum stake
        for validator in &self.validators {
            if validator.stake < self.staking.minimum_validator_stake {
                return Err(format!(
                    "Validator {} has insufficient stake",
                    validator.address
                ));
            }
        }

        Ok(())
    }

    /// Sum raw allocations without overflow. This does not select token precision.
    pub fn _total_dgt_supply(&self) -> Result<Amount, String> {
        validate_allocations(&self.dgt_allocations)
    }

    pub fn _get_vested_amount(
        &self,
        address: &Address,
        current_time: Timestamp,
    ) -> Result<Amount, String> {
        validate_allocations(&self.dgt_allocations)?;
        match self.dgt_allocations.iter().find(|a| &a.address == address) {
            Some(allocation) => match &allocation.vesting {
                Some(schedule) => schedule._vested_amount(current_time),
                None => Ok(allocation.amount),
            },
            None => Ok(0),
        }
    }

    pub fn _get_locked_amount(
        &self,
        address: &Address,
        current_time: Timestamp,
    ) -> Result<Amount, String> {
        validate_allocations(&self.dgt_allocations)?;
        match self.dgt_allocations.iter().find(|a| &a.address == address) {
            Some(allocation) => match &allocation.vesting {
                Some(schedule) => schedule._locked_amount(current_time),
                None => Ok(0),
            },
            None => Ok(0),
        }
    }

    /// Export to JSON string
    pub fn _to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Import from JSON string
    pub fn _from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genesis_amounts_serialize_as_strings() {
        let genesis = GenesisConfig::_mainnet();
        let json = genesis._to_json().unwrap();
        // Spot check one large number appears quoted
        assert!(json.contains("\"400000000000000000\""));
    }

    #[test]
    fn test_mainnet_genesis_validation() {
        let genesis = GenesisConfig::_mainnet();
        assert!(genesis._validate().is_ok());
    }

    #[test]
    fn test_dgt_total_supply() {
        let genesis = GenesisConfig::_mainnet();
        assert_eq!(
            genesis._total_dgt_supply().unwrap(),
            1_000_000_000_000_000_000
        );
    }

    #[test]
    fn test_emission_breakdown_validation() {
        let breakdown = EmissionBreakdown {
            block_rewards: 60,
            staking_rewards: 25,
            ai_module_incentives: 10,
            bridge_operations: 5,
        };
        assert!(breakdown._is_valid());

        let invalid_breakdown = EmissionBreakdown {
            block_rewards: 60,
            staking_rewards: 25,
            ai_module_incentives: 10,
            bridge_operations: 6, // Sum = 101%
        };
        assert!(!invalid_breakdown._is_valid());
    }

    #[test]
    fn test_vesting_schedule() {
        let start_time = 1722715226; // Genesis timestamp
        let vesting = VestingSchedule {
            total_amount: 1000,
            cliff_duration: 365 * 24 * 60 * 60,       // 1 year
            vesting_duration: 4 * 365 * 24 * 60 * 60, // 4 years total
            start_time,
        };

        // During cliff period
        let cliff_time = start_time + 6 * 30 * 24 * 60 * 60; // 6 months
        assert_eq!(vesting._vested_amount(cliff_time).unwrap(), 0);

        // After cliff, during vesting
        let mid_vesting_time = start_time + 2 * 365 * 24 * 60 * 60; // 2 years
        let vested = vesting._vested_amount(mid_vesting_time).unwrap();
        assert!(vested > 0 && vested < 1000);

        // After full vesting
        let end_time = start_time + 5 * 365 * 24 * 60 * 60; // 5 years
        assert_eq!(vesting._vested_amount(end_time).unwrap(), 1000);
    }

    #[test]
    fn test_genesis_serialization() {
        let genesis = GenesisConfig::_mainnet();
        let json = genesis._to_json().unwrap();
        let deserialized = GenesisConfig::_from_json(&json).unwrap();

        assert_eq!(genesis.network.name, deserialized.network.name);
        assert_eq!(
            genesis.dgt_allocations.len(),
            deserialized.dgt_allocations.len()
        );
    }

    #[test]
    fn generate_genesis_json() {
        let directory = crate::test_support::TestDirectory::new();
        let path = directory.path().join("genesis.json");
        let genesis = GenesisConfig::_mainnet();
        genesis._validate().unwrap();
        std::fs::write(&path, genesis._to_json().unwrap()).unwrap();
        let loaded = GenesisConfig::_from_json(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(
            loaded._total_dgt_supply().unwrap(),
            genesis._total_dgt_supply().unwrap()
        );
    }

    #[test]
    fn vesting_full_width_values_and_boundaries_are_exact() {
        let schedule = VestingSchedule {
            total_amount: u128::MAX,
            start_time: 10,
            cliff_duration: 2,
            vesting_duration: 12,
        };
        for (time, expected) in [
            (0, 0),
            (10, 0),
            (11, 0),
            (12, 0),
            (13, u128::MAX / 10),
            (17, u128::MAX / 2),
            (22, u128::MAX),
            (u64::MAX, u128::MAX),
        ] {
            let vested = schedule._vested_amount(time).unwrap();
            assert_eq!(vested, expected);
            assert_eq!(
                vested.checked_add(schedule._locked_amount(time).unwrap()),
                Some(u128::MAX)
            );
        }
    }

    #[test]
    fn zero_length_vesting_preserves_instant_unlock_semantics() {
        for cliff in [0, 2] {
            let schedule = VestingSchedule {
                total_amount: 10,
                start_time: 5,
                cliff_duration: cliff,
                vesting_duration: cliff,
            };
            assert_eq!(schedule._vested_amount(4 + cliff).unwrap(), 0);
            assert_eq!(schedule._vested_amount(5 + cliff).unwrap(), 10);
        }
        let schedule = VestingSchedule {
            total_amount: u128::MAX,
            start_time: 0,
            cliff_duration: 0,
            vesting_duration: u64::MAX,
        };
        assert_eq!(
            schedule._vested_amount(u64::MAX - 1).unwrap(),
            u128::MAX - (u128::from(u64::MAX) + 2)
        );
        assert_eq!(schedule._vested_amount(u64::MAX).unwrap(), u128::MAX);
    }

    #[test]
    fn vesting_rejects_invalid_duration_and_timestamp_domains() {
        for (start, cliff, duration) in [(0, 3, 2), (u64::MAX, 0, 1)] {
            let schedule = VestingSchedule {
                total_amount: 10,
                start_time: start,
                cliff_duration: cliff,
                vesting_duration: duration,
            };
            for time in [0, u64::MAX] {
                assert!(schedule._vested_amount(time).is_err());
                assert!(schedule._locked_amount(time).is_err());
            }
        }
    }

    #[test]
    fn vesting_is_monotone_and_conserves_every_small_allocation() {
        for amount in 0..64u128 {
            for duration in 1..16u64 {
                for cliff in 0..duration {
                    let schedule = VestingSchedule {
                        total_amount: amount,
                        start_time: 3,
                        cliff_duration: cliff,
                        vesting_duration: duration,
                    };
                    let mut previous = 0;
                    for time in 0..duration + 5 {
                        let vested = schedule._vested_amount(time).unwrap();
                        let expected = if time < 3 + cliff {
                            0
                        } else if time >= 3 + duration {
                            amount
                        } else {
                            amount * u128::from(time - 3 - cliff) / u128::from(duration - cliff)
                        };
                        assert_eq!(vested, expected);
                        assert!(vested >= previous && vested <= amount);
                        assert_eq!(vested + schedule._locked_amount(time).unwrap(), amount);
                        previous = vested;
                    }
                }
            }
        }
    }

    #[test]
    fn allocation_validation_rejects_duplicates_mismatches_and_overflow() {
        let mut config = GenesisConfig::mainnet();
        config.dgt_allocations[1].address = config.dgt_allocations[0].address.clone();
        assert!(config._validate().unwrap_err().contains("unique"));
        let mut config = GenesisConfig::mainnet();
        config.dgt_allocations[1]
            .vesting
            .as_mut()
            .unwrap()
            .total_amount += 1;
        assert!(config._validate().unwrap_err().contains("Vesting total"));
        let mut config = GenesisConfig::mainnet();
        config.dgt_allocations[0].amount = u128::MAX;
        assert!(config._total_dgt_supply().is_err());
        assert!(config._validate().is_err());
        let percentages = EmissionBreakdown {
            block_rewards: 255,
            staking_rewards: 255,
            ai_module_incentives: 255,
            bridge_operations: 103,
        };
        assert!(!percentages._is_valid());
    }

    #[test]
    fn genesis_amounts_roundtrip_exact_strings_and_legacy_small_integers() {
        let value = DGTAllocation {
            address: "dyt1fixture".into(),
            amount: u128::MAX,
            vesting: None,
        };
        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(json["amount"], u128::MAX.to_string());
        let restored: DGTAllocation = serde_json::from_value(json).unwrap();
        assert_eq!(restored.amount, u128::MAX);
        let old: DGTAllocation =
            serde_json::from_str(r#"{"address":"dyt1fixture","amount":42,"vesting":null}"#)
                .unwrap();
        assert_eq!(old.amount, 42);
        for amount in [
            "-1",
            "1.5",
            "null",
            "340282366920938463463374607431768211456",
        ] {
            let json = format!(r#"{{"address":"dyt1fixture","amount":{amount},"vesting":null}}"#);
            assert!(serde_json::from_str::<DGTAllocation>(&json).is_err());
        }
        let json = serde_json::to_value(GenesisConfig::mainnet()).unwrap();
        for pointer in [
            "/dgt_allocations/0/amount",
            "/dgt_allocations/1/vesting/total_amount",
            "/drt_emission/initial_supply",
            "/governance/proposal_threshold",
            "/staking/minimum_validator_stake",
            "/staking/emission_per_block",
            "/validators/0/stake",
        ] {
            assert!(json.pointer(pointer).unwrap().is_string(), "{pointer}");
        }
    }
    #[test]
    fn legacy_full_width_integer_literals_remain_exact() {
        for amount in [u128::from(u64::MAX), u128::from(u64::MAX) + 1, u128::MAX] {
            let json = format!(
                r#"{{"minimum_validator_stake":{amount},"max_validators":1,"double_sign_slash_rate":0,"downtime_slash_rate":0,"offline_threshold":1,"emission_per_block":{amount}}}"#
            );
            let config: StakingConfig = serde_json::from_str(&json).unwrap();
            assert_eq!(config.minimum_validator_stake, amount);
            assert_eq!(config.emission_per_block, amount);
        }
    }

    #[test]
    fn genesis_binary_amount_encoding_remains_compatible() {
        let allocation = DGTAllocation {
            address: "dyt1fixture".into(),
            amount: u128::MAX,
            vesting: None,
        };
        let encoded = bincode::serialize(&allocation).unwrap();
        let old_shape = (
            allocation.address.clone(),
            allocation.amount,
            Option::<VestingSchedule>::None,
        );
        assert_eq!(encoded, bincode::serialize(&old_shape).unwrap());
        assert_eq!(
            bincode::deserialize::<DGTAllocation>(&encoded)
                .unwrap()
                .amount,
            u128::MAX
        );
    }
}
