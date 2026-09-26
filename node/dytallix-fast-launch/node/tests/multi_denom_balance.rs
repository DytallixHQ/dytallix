use std::collections::BTreeMap;
use std::sync::Arc;
use tempfile::tempdir;

use dytallix_fast_node::state::{AccountState, State};
use dytallix_fast_node::storage::state::Storage;

#[test]
fn test_account_state_multi_denomination() {
    let mut account = AccountState {
        balances: BTreeMap::new(),
        nonce: 0,
    };

    // Test setting and getting balances
    account.set_balance("udgt", 1000);
    account.set_balance("udrt", 2000);

    assert_eq!(account.balance_of("udgt"), 1000);
    assert_eq!(account.balance_of("udrt"), 2000);
    assert_eq!(account.balance_of("unknown"), 0);

    // Test legacy balance (should default to udgt)
    assert_eq!(account.legacy_balance(), 1000);

    // Test adding balance
    account.add_balance("udgt", 500);
    assert_eq!(account.balance_of("udgt"), 1500);

    // Test subtracting balance
    assert!(account.sub_balance("udgt", 300).is_ok());
    assert_eq!(account.balance_of("udgt"), 1200);

    // Test insufficient balance
    assert!(account.sub_balance("udgt", 1500).is_err());
    assert_eq!(account.balance_of("udgt"), 1200); // Should remain unchanged

    // Test zero balance removal
    account.set_balance("udrt", 0);
    assert!(!account.balances.contains_key("udrt"));
}

#[test]
fn test_storage_multi_denomination() {
    let temp_dir = tempdir().unwrap();
    let storage = Storage::open(temp_dir.path().to_path_buf()).unwrap();

    let addr = "dyt1test123";

    // Test setting and getting multi-denomination balances
    let mut balances = BTreeMap::new();
    balances.insert("udgt".to_string(), 1000);
    balances.insert("udrt".to_string(), 2000);

    storage.set_balances_db(addr, &balances).unwrap();
    let retrieved = storage.get_balances_db(addr);

    assert_eq!(retrieved.get("udgt"), Some(&1000));
    assert_eq!(retrieved.get("udrt"), Some(&2000));

    // Test legacy balance getter (should return udgt)
    assert_eq!(storage.get_balance_db(addr), 1000);

    // Test legacy balance setter (should migrate to multi-denom)
    let new_addr = "dyt1test456";
    storage.set_balance_db(new_addr, 5000).unwrap();

    let migrated_balances = storage.get_balances_db(new_addr);
    assert_eq!(migrated_balances.get("udgt"), Some(&5000));
    assert_eq!(storage.get_balance_db(new_addr), 5000);
}

#[test]
fn test_state_multi_denomination() {
    let temp_dir = tempdir().unwrap();
    let storage = Arc::new(Storage::open(temp_dir.path().to_path_buf()).unwrap());
    let mut state = State::new(storage);

    let addr1 = "dyt1alice";
    let addr2 = "dyt1bob";

    // Test crediting different denominations
    state.credit(addr1, "udgt", 1000);
    state.credit(addr1, "udrt", 2000);

    assert_eq!(state.balance_of(addr1, "udgt"), 1000);
    assert_eq!(state.balance_of(addr1, "udrt"), 2000);
    assert_eq!(state.legacy_balance_of(addr1), 1000);

    // Test multi-denomination transfer
    state.credit(addr2, "udgt", 500); // Give addr2 some udgt for fees

    // Transfer udrt from addr1 to addr2, pay fees in udgt
    let result = state.apply_transfer(addr1, addr2, "udrt", 1000, "udgt", 100);
    assert!(result.is_ok());

    // Check balances after transfer
    assert_eq!(state.balance_of(addr1, "udgt"), 900); // 1000 - 100 (fee)
    assert_eq!(state.balance_of(addr1, "udrt"), 1000); // 2000 - 1000 (transfer)
    assert_eq!(state.balance_of(addr2, "udgt"), 500); // unchanged (fees paid by sender)
    assert_eq!(state.balance_of(addr2, "udrt"), 1000); // received transfer

    // Test insufficient balance
    let result = state.apply_transfer(addr1, addr2, "udgt", 2000, "udgt", 0);
    assert!(result.is_err());

    // Test legacy methods
    state.credit_legacy(addr1, 500);
    assert_eq!(state.balance_of(addr1, "udgt"), 1400); // 900 + 500

    state.apply_transfer_legacy(addr1, addr2, 200, 50);
    assert_eq!(state.balance_of(addr1, "udgt"), 1200); // 1400 - 200
    assert_eq!(state.balance_of(addr1, "udrt"), 950); // 1000 - 50 (legacy fee)
    assert_eq!(state.balance_of(addr2, "udrt"), 1000); // unchanged
    assert_eq!(state.balance_of(addr2, "udgt"), 700); // 500 + 200
}

#[test]
fn test_balances_ordering_deterministic() {
    let mut account = AccountState {
        balances: BTreeMap::new(),
        nonce: 0,
    };

    // Add balances in different order
    account.set_balance("udrt", 1000);
    account.set_balance("udgt", 2000);
    account.set_balance("other", 3000);

    // BTreeMap should maintain deterministic ordering
    let keys: Vec<_> = account.balances.keys().collect();
    assert_eq!(keys, vec!["other", "udgt", "udrt"]); // Alphabetical order
}

#[test]
fn test_empty_balances() {
    let temp_dir = tempdir().unwrap();
    let storage = Arc::new(Storage::open(temp_dir.path().to_path_buf()).unwrap());
    let mut state = State::new(storage);

    let addr = "dyt1empty";

    // Test getting balances for non-existent account
    assert_eq!(state.balance_of(addr, "udgt"), 0);
    assert_eq!(state.balance_of(addr, "udrt"), 0);
    assert_eq!(state.legacy_balance_of(addr), 0);

    let balances = state.balances_of(addr);
    assert!(balances.is_empty());
}
