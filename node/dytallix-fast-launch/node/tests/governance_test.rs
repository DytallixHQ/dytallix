#[path = "support/stake_fixture.rs"]
mod stake_fixture;
use stake_fixture::StakeFixture;

use dytallix_fast_node::runtime::governance::*;
use dytallix_fast_node::runtime::staking::StakingModule;
use dytallix_fast_node::state::State;
use dytallix_fast_node::storage::state::Storage;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

fn setup_test_governance() -> (
    GovernanceModule,
    Arc<Mutex<State>>,
    Arc<Mutex<StakingModule>>,
    TempDir,
) {
    let temp_dir = TempDir::new().unwrap();
    let storage = Arc::new(Storage::open(temp_dir.path().join("test.db")).unwrap());
    let state = Arc::new(Mutex::new(State::new(storage.clone())));
    let staking = Arc::new(Mutex::new(StakingModule::new(storage.clone())));

    let governance = GovernanceModule::new(storage, state.clone(), staking.clone());
    (governance, state, staking, temp_dir)
}

#[test]
fn test_governance_submit_proposal() {
    let (mut governance, _state, _staking, _temp_dir) = setup_test_governance();

    let proposal_id = governance
        .submit_proposal(
            100,
            "Test Gas Limit Increase".to_string(),
            "Increase gas limit to 500000".to_string(),
            ProposalType::ParameterChange {
                key: "gas_limit".to_string(),
                value: "500000".to_string(),
            },
        )
        .unwrap();

    assert_eq!(proposal_id, 1);

    let proposal = governance.get_proposal(proposal_id).unwrap().unwrap();
    assert_eq!(proposal.title, "Test Gas Limit Increase");
    assert_eq!(proposal.status, ProposalStatus::DepositPeriod);
    assert_eq!(proposal.submit_height, 100);
    assert_eq!(proposal.deposit_end_height, 400); // 100 + 300
    assert_eq!(proposal.voting_start_height, 400);
    assert_eq!(proposal.voting_end_height, 700); // 400 + 300
}

#[test]
fn test_governance_deposit_flow() {
    let (mut governance, state, _staking, _temp_dir) = setup_test_governance();

    // Setup account with DGT balance
    {
        let mut state = state.lock().unwrap();
        let mut account = state.get_account("depositor1");
        account.add_balance("udgt", 2_000_000_000); // 2000 DGT
        state.accounts.insert("depositor1".to_string(), account);
    }

    let proposal_id = governance
        .submit_proposal(
            100,
            "Test Proposal".to_string(),
            "Test Description".to_string(),
            ProposalType::ParameterChange {
                key: "gas_limit".to_string(),
                value: "500000".to_string(),
            },
        )
        .unwrap();

    // Deposit enough to meet minimum - should transition to voting
    governance
        .deposit(150, "depositor1", proposal_id, 1_000_000_000, "udgt")
        .unwrap();

    let proposal = governance.get_proposal(proposal_id).unwrap().unwrap();
    assert_eq!(proposal.status, ProposalStatus::VotingPeriod);
    assert_eq!(proposal.total_deposit, 1_000_000_000);

    // Check balance was deducted
    let mut state = state.lock().unwrap();
    let account = state.get_account("depositor1");
    assert_eq!(account.balance_of("udgt"), 1_000_000_000); // 2000 - 1000 = 1000 DGT remaining
}

#[test]
fn test_governance_voting_flow() {
    let (mut governance, state, staking, _temp_dir) = setup_test_governance();
    // Explicit synthetic bonded stake; liquid balances remain separate.
    {
        let mut staking = staking.lock().unwrap();
        staking.seed_delegator_stake("voter1", 500_000_000);
        staking.seed_delegator_stake("voter2", 300_000_000);
        staking.seed_delegator_stake("voter3", 200_000_000);
        staking.seed_total_stake(1_000_000_000);
    }

    // Setup voters with DGT balances
    {
        let mut state = state.lock().unwrap();

        let mut voter1 = state.get_account("voter1");
        // Increased to cover deposit of 1,000 DGT while retaining 500 DGT voting power
        voter1.add_balance("udgt", 1_500_000_000); // 1500 DGT
        state.accounts.insert("voter1".to_string(), voter1);

        let mut voter2 = state.get_account("voter2");
        voter2.add_balance("udgt", 300_000_000); // 300 DGT
        state.accounts.insert("voter2".to_string(), voter2);

        let mut voter3 = state.get_account("voter3");
        voter3.add_balance("udgt", 200_000_000); // 200 DGT
        state.accounts.insert("voter3".to_string(), voter3);
    }

    let proposal_id = governance
        .submit_proposal(
            100,
            "Test Proposal".to_string(),
            "Test Description".to_string(),
            ProposalType::ParameterChange {
                key: "gas_limit".to_string(),
                value: "500000".to_string(),
            },
        )
        .unwrap();

    // Deposit to transition into voting period
    governance
        .deposit(150, "voter1", proposal_id, 1_000_000_000, "udgt")
        .unwrap();

    // Cast votes within the voting window [150, 450]
    governance
        .vote(450, "voter1", proposal_id, VoteOption::Yes)
        .unwrap();
    governance
        .vote(300, "voter2", proposal_id, VoteOption::Yes)
        .unwrap();
    governance
        .vote(400, "voter3", proposal_id, VoteOption::No)
        .unwrap();

    // Tally votes
    let tally = governance.tally(proposal_id).unwrap();
    assert_eq!(tally.yes, 800_000_000); // 500 + 300 DGT
    assert_eq!(tally.no, 200_000_000); // 200 DGT
    assert_eq!(tally.abstain, 0);

    // Should pass with majority yes
    assert!(tally.yes > tally.no);
}

#[test]
fn test_governance_parameter_execution() {
    let (mut governance, state, staking, _temp_dir) = setup_test_governance();
    // Explicit synthetic bonded stake; liquid balances remain separate.
    {
        let mut staking = staking.lock().unwrap();
        staking.seed_delegator_stake("voterA", 600_000_000);
        staking.seed_delegator_stake("voterB", 500_000_000);
        staking.seed_total_stake(1_100_000_000);
    }

    let proposal_id = governance
        .submit_proposal(
            100,
            "Increase Gas Limit".to_string(),
            "Increase gas limit from default to 50000".to_string(),
            ProposalType::ParameterChange {
                key: "gas_limit".to_string(),
                value: "50000".to_string(),
            },
        )
        .unwrap();

    // Check initial gas limit
    assert_eq!(governance.get_config().gas_limit, 2_000);

    // Fund a depositor and voters
    {
        let mut st = state.lock().unwrap();
        let mut d = st.get_account("depositorZ");
        d.add_balance("udgt", 2_000_000_000);
        st.accounts.insert("depositorZ".to_string(), d);
        let mut v1 = st.get_account("voterA");
        v1.add_balance("udgt", 600_000_000);
        st.accounts.insert("voterA".to_string(), v1);
        let mut v2 = st.get_account("voterB");
        v2.add_balance("udgt", 500_000_000);
        st.accounts.insert("voterB".to_string(), v2);
    }
    // Deposit to reach voting
    governance
        .deposit(150, "depositorZ", proposal_id, 1_000_000_000, "udgt")
        .unwrap();
    // Cast votes (both within [150, 450])
    governance
        .vote(450, "voterA", proposal_id, VoteOption::Yes)
        .unwrap();
    governance
        .vote(449, "voterB", proposal_id, VoteOption::Yes)
        .unwrap();

    // First end_block after voting end will tally and mark as Passed
    governance.end_block(800).unwrap();
    let proposal_after_tally = governance.get_proposal(proposal_id).unwrap().unwrap();
    assert_eq!(proposal_after_tally.status, ProposalStatus::Passed);

    // Second end_block will execute passed proposals
    governance.end_block(801).unwrap();

    // Check gas limit was updated
    assert_eq!(governance.get_config().gas_limit, 50_000);

    let proposal = governance.get_proposal(proposal_id).unwrap().unwrap();
    assert_eq!(proposal.status, ProposalStatus::Executed);
}

#[test]
fn test_governance_end_block_processing() {
    let (mut governance, _state, _staking, _temp_dir) = setup_test_governance();

    let proposal_id = governance
        .submit_proposal(
            100,
            "Test Proposal".to_string(),
            "Test Description".to_string(),
            ProposalType::ParameterChange {
                key: "gas_limit".to_string(),
                value: "500000".to_string(),
            },
        )
        .unwrap();

    // Should be in deposit period
    let proposal = governance.get_proposal(proposal_id).unwrap().unwrap();
    assert_eq!(proposal.status, ProposalStatus::DepositPeriod);

    // Process end block after deposit period expires (height 401)
    governance.end_block(401).unwrap();

    // Should be marked as Failed due to insufficient deposits in deposit period
    let proposal = governance.get_proposal(proposal_id).unwrap().unwrap();
    assert_eq!(proposal.status, ProposalStatus::Failed);
}

#[test]
fn test_governance_config_defaults() {
    let config = GovernanceConfig::default();
    assert_eq!(config.min_deposit, 1_000_000_000); // 1000 DGT
    assert_eq!(config.deposit_period, 300);
    assert_eq!(config.voting_period, 300);
    assert_eq!(config.gas_limit, 2_000);
}
