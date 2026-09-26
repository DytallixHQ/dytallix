#[path = "support/stake_fixture.rs"]
mod stake_fixture;
use stake_fixture::StakeFixture;

use dytallix_fast_node::runtime::governance::{
    GovernanceModule, ProposalStatus, ProposalType, VoteOption,
};
use dytallix_fast_node::runtime::staking::StakingModule;
use dytallix_fast_node::state::State;
use dytallix_fast_node::storage::state::Storage;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

#[test]
fn governance_manual_execute_test() {
    // Setup backed by temp storage
    let dir = tempdir().unwrap();
    let storage = Arc::new(Storage::open(dir.path().join("node.db")).unwrap());
    let state = Arc::new(Mutex::new(State::new(storage.clone())));
    let staking = Arc::new(Mutex::new(StakingModule::new(storage.clone())));
    let mut governance = GovernanceModule::new(storage.clone(), state.clone(), staking.clone());

    // Explicit bonded stake determines voting power; liquid balances are separate.
    {
        let mut staking = staking.lock().unwrap();
        staking.seed_delegator_stake("voter1", 500_000_000);
        staking.seed_delegator_stake("voter2", 500_000_000);
        staking.seed_total_stake(1_000_000_000);
    }

    // Fund accounts
    {
        let mut s = state.lock().unwrap();
        let mut depositor = s.get_account("depositor1");
        depositor.add_balance("udgt", 2_000_000_000);
        s.accounts.insert("depositor1".to_string(), depositor);

        let mut voter1 = s.get_account("voter1");
        voter1.add_balance("udgt", 500_000_000);
        s.accounts.insert("voter1".to_string(), voter1);

        let mut voter2 = s.get_account("voter2");
        voter2.add_balance("udgt", 500_000_000);
        s.accounts.insert("voter2".to_string(), voter2);
    }

    // Submit proposal
    let proposal_id = governance
        .submit_proposal(
            100,
            "Manual Execute Test".to_string(),
            "Test manual execution of parameter change".to_string(),
            ProposalType::ParameterChange {
                key: "gas_limit".to_string(),
                value: "75000".to_string(),
            },
        )
        .expect("Failed to submit proposal");

    // Deposit
    governance
        .deposit(150, "depositor1", proposal_id, 1_000_000_000, "udgt")
        .expect("Failed to deposit");

    // Vote
    governance
        .vote(200, "voter1", proposal_id, VoteOption::Yes)
        .expect("Failed to vote");

    governance
        .vote(300, "voter2", proposal_id, VoteOption::Yes)
        .expect("Failed to vote");

    // Get initial parameter value
    let initial_gas_limit = governance.get_config().gas_limit;
    assert_eq!(initial_gas_limit, 2_000);

    // Process end_block to move proposal to Passed status (but don't auto-execute)
    governance
        .end_block(701)
        .expect("Failed to process end_block");

    let proposal_after_tally = governance.get_proposal(proposal_id).unwrap().unwrap();
    assert_eq!(proposal_after_tally.status, ProposalStatus::Passed);

    // Verify parameter hasn't changed yet
    let current_gas_limit = governance.get_config().gas_limit;
    assert_eq!(current_gas_limit, 2_000);

    // Test manual execute - this is the new functionality
    governance
        .execute(proposal_id)
        .expect("Failed to manually execute proposal");

    // Verify proposal executed and parameter changed
    let new_gas_limit = governance.get_config().gas_limit;
    assert_eq!(new_gas_limit, 75_000);

    println!("✅ Manual execute test passed: gas_limit {initial_gas_limit} -> {new_gas_limit}");

    // Test that executing again fails
    let result = governance.execute(proposal_id);
    assert!(
        result.is_err(),
        "Should not be able to execute proposal twice"
    );

    // Test executing non-existent proposal fails
    let result = governance.execute(999);
    assert!(
        result.is_err(),
        "Should not be able to execute non-existent proposal"
    );
}

#[test]
fn governance_execute_not_passed_fails() {
    let dir = tempdir().unwrap();
    let storage = Arc::new(Storage::open(dir.path().join("node.db")).unwrap());
    let state = Arc::new(Mutex::new(State::new(storage.clone())));
    let staking = Arc::new(Mutex::new(StakingModule::new(storage.clone())));
    let mut governance = GovernanceModule::new(storage.clone(), state.clone(), staking);

    // Submit proposal but don't vote/pass it
    let proposal_id = governance
        .submit_proposal(
            100,
            "Test Proposal".to_string(),
            "Test".to_string(),
            ProposalType::ParameterChange {
                key: "gas_limit".to_string(),
                value: "30000".to_string(),
            },
        )
        .expect("Failed to submit proposal");

    // Try to execute proposal that hasn't passed
    let result = governance.execute(proposal_id);
    assert!(
        result.is_err(),
        "Should not be able to execute proposal that hasn't passed"
    );
    assert!(result.unwrap_err().contains("has not passed"));
}
