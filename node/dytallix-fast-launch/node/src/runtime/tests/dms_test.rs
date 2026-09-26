use crate::runtime::dead_man_switch::DeadManSwitchModule;
use crate::storage::state::Storage;
use std::sync::Arc;
use tempfile::tempdir;

#[test]
fn test_dms_flow() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(Storage::open(dir.path().join("test.db")).unwrap());
    let dms = DeadManSwitchModule::new(storage);

    let owner = "owner_addr";
    let beneficiary = "beneficiary_addr";
    let period = 100;
    let current_block = 1000;

    // Register
    dms.register(owner, beneficiary, period, current_block)
        .unwrap();

    // Check config
    let config = dms.load_config(owner).unwrap();
    assert_eq!(config.beneficiary, beneficiary);
    assert_eq!(config.period_blocks, period);
    assert_eq!(config.last_active_block, current_block);

    // Validate claim too early
    let claim_block = current_block + period - 1;
    let res = dms.validate_claim(owner, beneficiary, claim_block);
    assert!(res.is_err());

    // Validate claim success
    let claim_block = current_block + period + 1;
    let res = dms.validate_claim(owner, beneficiary, claim_block);
    assert!(res.is_ok());
    assert_eq!(res.unwrap(), beneficiary);

    // Ping
    let ping_block = current_block + 50;
    dms.ping(owner, ping_block).unwrap();

    // Check config updated
    let config = dms.load_config(owner).unwrap();
    assert_eq!(config.last_active_block, ping_block);

    // Validate claim after ping (should fail now)
    let claim_block = current_block + period + 1; // 1101
                                                  // New deadline is ping_block + period = 1050 + 100 = 1150
    let res = dms.validate_claim(owner, beneficiary, claim_block);
    assert!(res.is_err()); // 1101 < 1150

    // Validate claim after new deadline
    let claim_block = ping_block + period + 1;
    let res = dms.validate_claim(owner, beneficiary, claim_block);
    assert!(res.is_ok());
}
