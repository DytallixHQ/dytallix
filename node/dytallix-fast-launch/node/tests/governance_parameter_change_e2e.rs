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
fn governance_parameter_change_e2e() {
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

    // Fund liquid balances for deposits; stake above supplies voting power
    {
        let mut s = state.lock().unwrap();
        // depositor1: 2,000 DGT
        let mut depositor = s.get_account("depositor1");
        depositor.add_balance("udgt", 2_000_000_000);
        s.accounts.insert("depositor1".to_string(), depositor);

        // voter1: 500 DGT
        let mut voter1 = s.get_account("voter1");
        voter1.add_balance("udgt", 500_000_000);
        s.accounts.insert("voter1".to_string(), voter1);

        // voter2: 500 DGT
        let mut voter2 = s.get_account("voter2");
        voter2.add_balance("udgt", 500_000_000);
        s.accounts.insert("voter2".to_string(), voter2);
    }

    // Test proposal submission
    let proposal_id = governance
        .submit_proposal(
            100, // height
            "Gas Limit Increase".to_string(),
            "Increase gas limit from 21,000 to 50,000 for better UX".to_string(),
            ProposalType::ParameterChange {
                key: "gas_limit".to_string(),
                value: "50000".to_string(),
            },
        )
        .expect("Failed to submit proposal");

    assert_eq!(proposal_id, 1);

    // Deposit enough to meet minimum requirement
    governance
        .deposit(
            150, // height
            "depositor1",
            proposal_id,
            1_000_000_000, // 1000 DGT
            "udgt",
        )
        .expect("Failed to deposit");

    // Vote on proposal within the configured voting period
    governance
        .vote(
            200, // height inside voting window (start at 150, end at 450)
            "voter1",
            proposal_id,
            VoteOption::Yes,
        )
        .expect("Failed to vote");

    governance
        .vote(
            300, // height inside voting window
            "voter2",
            proposal_id,
            VoteOption::Yes,
        )
        .expect("Failed to vote");

    // Get initial parameter value
    let initial_gas_limit = governance.get_config().gas_limit;
    assert_eq!(initial_gas_limit, 2_000);

    // Process proposal at end of voting period - first call will tally and mark as Passed
    governance
        .end_block(701)
        .expect("Failed to process end_block");

    let proposal_after_tally = governance.get_proposal(proposal_id).unwrap().unwrap();
    assert_eq!(proposal_after_tally.status, ProposalStatus::Passed);

    // Second end_block will execute passed proposals
    governance
        .end_block(702)
        .expect("Failed to process end_block for execution");

    // Verify proposal executed and parameter changed
    let proposal = governance.get_proposal(proposal_id).unwrap().unwrap();
    assert_eq!(proposal.status, ProposalStatus::Executed);

    let new_gas_limit = governance.get_config().gas_limit;
    assert_eq!(new_gas_limit, 50_000);

    println!("✅ Parameter change test passed: gas_limit {initial_gas_limit} -> {new_gas_limit}");
}
