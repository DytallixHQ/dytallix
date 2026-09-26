use super::*;
use crate::state::State;
use crate::storage::state::Storage; // added
use crate::storage::tx::Transaction;
use std::sync::Arc; // added
use tempfile::TempDir; // added

#[cfg(test)]
mod mempool_gas_tests {
    use super::*;

    #[allow(dead_code)]
    fn create_mock_state() -> State {
        // Create temporary storage for state
        let temp_dir = TempDir::new().expect("failed to create temp dir for state test");
        let storage =
            Arc::new(Storage::open(temp_dir.path().to_path_buf()).expect("failed to open storage"));
        State::new(storage)
    }

    fn create_test_transaction(gas_limit: u64, gas_price: u64) -> Transaction {
        Transaction::new(
            "test_hash".to_string(),
            "dytallix1test_from".to_string(),
            "dytallix1test_to".to_string(),
            1000,
            10,
            1,
            Some("test_signature".to_string()),
        )
        .with_gas(gas_limit, gas_price)
    }

    #[test]
    fn test_gas_validation_success() {
        let tx = create_test_transaction(25000, 1000);
        let result = validate_gas(&tx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_gas_validation_zero_price() {
        let tx = create_test_transaction(25000, 0);
        let result = validate_gas(&tx);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("gas price cannot be zero"));
    }

    #[test]
    fn test_gas_validation_low_limit() {
        // Create transaction with very low gas limit
        let tx = create_test_transaction(100, 1000);
        let result = validate_gas(&tx);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("GasValidationError"));
    }

    #[test]
    fn test_estimate_tx_size() {
        let tx = create_test_transaction(25000, 1000);
        let size = estimate_tx_size(&tx);

        // Should be reasonable size for a transaction
        assert!(size > 50);
        assert!(size < 1000);
    }

    #[test]
    fn test_basic_validate_with_gas() {
        // This test would require a proper State implementation
        // For now, just test the gas validation part
        let tx = create_test_transaction(25000, 1000);
        let result = validate_gas(&tx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_legacy_transaction_compatibility() {
        // Test transaction without gas fields (backward compatibility)
        let legacy_tx = Transaction::new(
            "legacy_hash".to_string(),
            "dytallix1legacy_from".to_string(),
            "dytallix1legacy_to".to_string(),
            1000,
            10,
            1,
            Some("legacy_signature".to_string()),
        );

        // Should not trigger gas validation (gas_limit = 0)
        assert_eq!(legacy_tx.gas_limit, 0);
        assert_eq!(legacy_tx.gas_price, 0);
    }

    #[test]
    fn test_deterministic_gas_validation() {
        let tx1 = create_test_transaction(25000, 1000);
        let tx2 = create_test_transaction(25000, 1000);

        let result1 = validate_gas(&tx1);
        let result2 = validate_gas(&tx2);

        // Same inputs should produce same results
        assert_eq!(result1.is_ok(), result2.is_ok());
    }
}
