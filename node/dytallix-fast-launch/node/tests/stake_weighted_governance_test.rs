#[path = "support/stake_fixture.rs"]
mod stake_fixture;
use stake_fixture::StakeFixture;

use dytallix_fast_node::runtime::governance::*;
use dytallix_fast_node::runtime::staking::StakingModule;
use dytallix_fast_node::state::State;
use dytallix_fast_node::storage::state::Storage;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

fn setup_test_governance_with_staking() -> (
    GovernanceModule,
    Arc<Mutex<State>>,
    Arc<Mutex<StakingModule>>,
    TempDir,
) {
    let temp_dir = TempDir::new().unwrap();
    let storage = Arc::new(Storage::open(temp_dir.path().join("test.db")).unwrap());
    let state = Arc::new(Mutex::new(State::new(storage.clone())));
    let staking_module = StakingModule::new(storage.clone());
    let staking_arc = Arc::new(Mutex::new(staking_module));

    let governance = GovernanceModule::new(storage, state.clone(), staking_arc.clone());

    (governance, state, staking_arc, temp_dir)
}

#[test]
fn test_stake_weighted_voting_power() {
    let (governance, _state, staking, _temp_dir) = setup_test_governance_with_staking();

    // Setup staking state
    staking
        .lock()
        .unwrap()
        .seed_delegator_stake("voter1", 500_000_000_000); // 500 DGT
    staking
        .lock()
        .unwrap()
        .seed_delegator_stake("voter2", 1_000_000_000_000); // 1000 DGT
    staking.lock().unwrap().seed_total_stake(1_500_000_000_000); // Total 1500 DGT

    // Test voting power calculation
    let power1 = governance.voting_power("voter1").unwrap();
    let power2 = governance.voting_power("voter2").unwrap();
    let total_power = governance.total_voting_power().unwrap();

    assert_eq!(power1, 500_000_000_000);
    assert_eq!(power2, 1_000_000_000_000);
    assert_eq!(total_power, 1_500_000_000_000);
}

#[test]
fn test_stake_weighted_tally_quorum_not_met() {
    let (mut governance, state, staking, _temp_dir) = setup_test_governance_with_staking();

    // Setup staking: total 1T DGT staked
    staking.lock().unwrap().seed_total_stake(1_000_000_000_000);
    staking
        .lock()
        .unwrap()
        .seed_delegator_stake("voter1", 200_000_000_000); // 20% of total stake

    // Use the development default of 67% quorum.
    let proposal_id = governance
        .submit_proposal(
            100,
            "Test Proposal".to_string(),
            "Test Description".to_string(),
            ProposalType::ParameterChange {
                key: "gas_limit".to_string(),
                value: "50000".to_string(),
            },
        )
        .unwrap();

    // Add deposit to reach voting period
    {
        let mut state = state.lock().unwrap();
        let mut account = state.get_account("depositor");
        account.add_balance("udgt", 2_000_000_000); // 2000 DGT
        state.accounts.insert("depositor".to_string(), account);
    }
    governance
        .deposit(100, "depositor", proposal_id, 1_000_000_000, "udgt")
        .unwrap();

    // Cast a vote with insufficient participation (20% < 67% quorum).
    governance
        .vote(150, "voter1", proposal_id, VoteOption::Yes)
        .unwrap();

    // Process end of voting period
    governance.end_block(500).unwrap(); // After voting end

    // Check proposal was rejected due to quorum not met
    let proposal = governance.get_proposal(proposal_id).unwrap().unwrap();
    assert_eq!(proposal.status, ProposalStatus::Rejected);

    let tally = proposal.tally.unwrap();
    assert_eq!(tally.yes, 200_000_000_000);
    assert_eq!(tally.total_voting_power, 200_000_000_000);

    // Should be rejected due to insufficient participation
    assert!(!governance.proposal_passes(&tally).unwrap());
}

#[test]
fn test_stake_weighted_tally_veto_triggered() {
    let (mut governance, state, staking, _temp_dir) = setup_test_governance_with_staking();

    // Setup staking: total 1T DGT staked
    staking.lock().unwrap().seed_total_stake(1_000_000_000_000);
    staking
        .lock()
        .unwrap()
        .seed_delegator_stake("voter1", 400_000_000_000); // 40% yes votes
    staking
        .lock()
        .unwrap()
        .seed_delegator_stake("voter2", 500_000_000_000); // 50% veto votes (> 33.33% threshold)

    let proposal_id = governance
        .submit_proposal(
            100,
            "Test Proposal".to_string(),
            "Test Description".to_string(),
            ProposalType::ParameterChange {
                key: "gas_limit".to_string(),
                value: "50000".to_string(),
            },
        )
        .unwrap();

    // Add deposit to reach voting period
    {
        let mut state = state.lock().unwrap();
        let mut account = state.get_account("depositor");
        account.add_balance("udgt", 2_000_000_000);
        state.accounts.insert("depositor".to_string(), account);
    }
    governance
        .deposit(100, "depositor", proposal_id, 1_000_000_000, "udgt")
        .unwrap();

    // Cast votes: sufficient participation but veto triggered
    governance
        .vote(150, "voter1", proposal_id, VoteOption::Yes)
        .unwrap();
    governance
        .vote(150, "voter2", proposal_id, VoteOption::NoWithVeto)
        .unwrap();

    // Process end of voting period
    governance.end_block(500).unwrap();

    // Check proposal was rejected due to veto
    let proposal = governance.get_proposal(proposal_id).unwrap().unwrap();
    assert_eq!(proposal.status, ProposalStatus::Rejected);

    let tally = proposal.tally.unwrap();
    assert_eq!(tally.yes, 400_000_000_000);
    assert_eq!(tally.no_with_veto, 500_000_000_000);
    assert_eq!(tally.total_voting_power, 900_000_000_000);

    // Should be rejected due to veto threshold exceeded
    assert!(!governance.proposal_passes(&tally).unwrap());
}

#[test]
fn test_stake_weighted_tally_successful_pass() {
    let (mut governance, state, staking, _temp_dir) = setup_test_governance_with_staking();

    // Setup staking: total 1T DGT staked
    staking.lock().unwrap().seed_total_stake(1_000_000_000_000);
    staking
        .lock()
        .unwrap()
        .seed_delegator_stake("voter1", 600_000_000_000); // 60% yes votes
    staking
        .lock()
        .unwrap()
        .seed_delegator_stake("voter2", 200_000_000_000); // 20% no votes
    staking
        .lock()
        .unwrap()
        .seed_delegator_stake("voter3", 100_000_000_000); // 10% abstain

    let proposal_id = governance
        .submit_proposal(
            100,
            "Test Proposal".to_string(),
            "Test Description".to_string(),
            ProposalType::ParameterChange {
                key: "gas_limit".to_string(),
                value: "50000".to_string(),
            },
        )
        .unwrap();

    // Add deposit to reach voting period
    {
        let mut state = state.lock().unwrap();
        let mut account = state.get_account("depositor");
        account.add_balance("udgt", 2_000_000_000);
        state.accounts.insert("depositor".to_string(), account);
    }
    governance
        .deposit(100, "depositor", proposal_id, 1_000_000_000, "udgt")
        .unwrap();

    // Cast votes: sufficient participation and yes votes > 50%
    governance
        .vote(150, "voter1", proposal_id, VoteOption::Yes)
        .unwrap();
    governance
        .vote(150, "voter2", proposal_id, VoteOption::No)
        .unwrap();
    governance
        .vote(150, "voter3", proposal_id, VoteOption::Abstain)
        .unwrap();

    // Tally first, then execute on the next end-block hook.
    governance.end_block(500).unwrap();

    let proposal = governance.get_proposal(proposal_id).unwrap().unwrap();
    assert_eq!(proposal.status, ProposalStatus::Passed);
    assert_eq!(governance.get_config().gas_limit, 2_000);
    governance.end_block(501).unwrap();

    // Check proposal passed and was executed
    let proposal = governance.get_proposal(proposal_id).unwrap().unwrap();
    assert_eq!(proposal.status, ProposalStatus::Executed);

    let tally = proposal.tally.unwrap();
    assert_eq!(tally.yes, 600_000_000_000);
    assert_eq!(tally.no, 200_000_000_000);
    assert_eq!(tally.abstain, 100_000_000_000);
    assert_eq!(tally.total_voting_power, 900_000_000_000);

    // Should pass: 90% participation > 67% quorum, 75% yes > 50% threshold.
    assert!(governance.proposal_passes(&tally).unwrap());

    // Verify parameter was changed
    assert_eq!(governance.get_config().gas_limit, 50000);
}

#[test]
fn test_parameter_change_events() {
    let (mut governance, state, staking, _temp_dir) = setup_test_governance_with_staking();

    // Give the voter 70% of total stake, above the unchanged 67% quorum.
    staking.lock().unwrap().seed_total_stake(1_000_000_000_000);
    staking
        .lock()
        .unwrap()
        .seed_delegator_stake("voter1", 700_000_000_000);

    let proposal_id = governance
        .submit_proposal(
            100,
            "Change Gas Limit".to_string(),
            "Update gas limit parameter".to_string(),
            ProposalType::ParameterChange {
                key: "gas_limit".to_string(),
                value: "75000".to_string(),
            },
        )
        .unwrap();

    // Add deposit and vote
    {
        let mut state = state.lock().unwrap();
        let mut account = state.get_account("depositor");
        account.add_balance("udgt", 2_000_000_000);
        state.accounts.insert("depositor".to_string(), account);
    }
    governance
        .deposit(100, "depositor", proposal_id, 1_000_000_000, "udgt")
        .unwrap();
    governance
        .vote(150, "voter1", proposal_id, VoteOption::Yes)
        .unwrap();

    // Clear existing events
    governance.clear_events();

    // The first hook stores the tally. The next hook executes the proposal.
    governance.end_block(500).unwrap();
    let proposal = governance.get_proposal(proposal_id).unwrap().unwrap();
    assert_eq!(proposal.status, ProposalStatus::Passed);
    assert_eq!(governance.get_config().gas_limit, 2_000);
    assert!(!governance
        .get_events()
        .iter()
        .any(|event| matches!(event, GovernanceEvent::ParameterChanged { .. })));

    governance.end_block(501).unwrap();
    let proposal = governance.get_proposal(proposal_id).unwrap().unwrap();
    assert_eq!(proposal.status, ProposalStatus::Executed);
    assert_eq!(governance.get_config().gas_limit, 75_000);

    // Additional hooks must not execute the proposal again.
    governance.end_block(501).unwrap();
    governance.end_block(502).unwrap();
    assert_eq!(
        state
            .lock()
            .unwrap()
            .snapshot_account("depositor")
            .balance_of("udgt"),
        2_000_000_000,
        "Repeated hooks must not refund the deposit twice"
    );

    // Check events include parameter change
    let events = governance.get_events();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, GovernanceEvent::ParameterChanged { .. }))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, GovernanceEvent::ProposalExecuted { id } if *id == proposal_id))
            .count(),
        1
    );
    let param_change_event = events
        .iter()
        .find(|event| matches!(event, GovernanceEvent::ParameterChanged { .. }));

    assert!(param_change_event.is_some());
    if let Some(GovernanceEvent::ParameterChanged {
        key,
        old_value,
        new_value,
    }) = param_change_event
    {
        assert_eq!(key, "gas_limit");
        assert_eq!(old_value, "2000"); // Default value
        assert_eq!(new_value, "75000");
    }
}

#[test]
fn test_governable_parameters_validation() {
    let (governance, _state, _staking, _temp_dir) = setup_test_governance_with_staking();

    // Test allowed parameters
    let allowed_params = governance.get_governable_parameters();
    assert!(allowed_params.contains(&"gas_limit".to_string()));
    assert!(allowed_params.contains(&"consensus.max_gas_per_block".to_string()));

    // Test parameter value retrieval
    assert_eq!(governance.get_config().gas_limit, 2_000);
}

#[test]
fn test_stake_weighted_vs_token_balance_difference() {
    let (mut governance, state, staking, _temp_dir) = setup_test_governance_with_staking();

    // Setup: user has DGT balance but no staking power
    {
        let mut state = state.lock().unwrap();
        let mut account = state.get_account("rich_user");
        account.add_balance("udgt", 1_000_000_000_000); // 1M DGT balance
        state.accounts.insert("rich_user".to_string(), account);
    }

    // User has no staking power (not delegated)
    staking.lock().unwrap().seed_total_stake(100_000_000_000); // 100 DGT total staked by others

    // Test voting power (should be 0 despite large DGT balance)
    let voting_power = governance.voting_power("rich_user").unwrap();
    assert_eq!(voting_power, 0); // No staking = no voting power

    // Create proposal and try to vote
    let proposal_id = governance
        .submit_proposal(
            100,
            "Test Proposal".to_string(),
            "Test Description".to_string(),
            ProposalType::ParameterChange {
                key: "gas_limit".to_string(),
                value: "50000".to_string(),
            },
        )
        .unwrap();

    // Add deposit to reach voting period
    governance
        .deposit(100, "rich_user", proposal_id, 1_000_000_000, "udgt")
        .unwrap();

    // Vote should succeed but have zero weight
    governance
        .vote(150, "rich_user", proposal_id, VoteOption::Yes)
        .unwrap();

    // Tally should show zero voting power
    let tally = governance.tally(proposal_id).unwrap();
    assert_eq!(tally.yes, 0); // Zero weight despite voting yes
    assert_eq!(tally.total_voting_power, 0);
}
