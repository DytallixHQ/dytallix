//! Development-contract regressions. These cases do not approve mainnet voting policy.
#[path = "support/stake_fixture.rs"]
mod stake_fixture;
use stake_fixture::StakeFixture;

use dytallix_fast_node::{
    runtime::{governance::*, staking::StakingModule},
    state::State,
    storage::state::Storage,
};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

type Fixture = (
    GovernanceModule,
    Arc<Mutex<State>>,
    Arc<Mutex<StakingModule>>,
    Arc<Storage>,
);
fn fixture(path: &Path) -> Fixture {
    let storage = Arc::new(Storage::open(path.to_path_buf()).unwrap());
    let state = Arc::new(Mutex::new(State::new(storage.clone())));
    let staking = Arc::new(Mutex::new(StakingModule::new(storage.clone())));
    let governance = GovernanceModule::new(storage.clone(), state.clone(), staking.clone());
    (governance, state, staking, storage)
}
fn open_proposal(gov: &mut GovernanceModule, state: &Arc<Mutex<State>>) -> u64 {
    state
        .lock()
        .unwrap()
        .set_balance("depositor", "udgt", 2_000_000_000);
    let id = gov
        .submit_proposal(
            10,
            "Test".into(),
            "Stored vote regression".into(),
            ProposalType::ParameterChange {
                key: "gas_limit".into(),
                value: "50000".into(),
            },
        )
        .unwrap();
    gov.deposit(10, "depositor", id, 1_000_000_000, "udgt")
        .unwrap();
    id
}
fn store_vote(storage: &Storage, id: u64, voter: &str, weight: u128, option: VoteOption) {
    let vote = Vote {
        proposal_id: id,
        voter: voter.into(),
        option,
        weight,
    };
    storage
        .db
        .put(
            format!("gov:vote:{id}:{voter}"),
            bincode::serialize(&vote).unwrap(),
        )
        .unwrap();
}

#[test]
fn stored_votes_survive_empty_account_cache_and_database_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("node.db");
    let id;
    {
        let (mut gov, state, staking, _storage) = fixture(&path);
        staking.lock().unwrap().seed_total_stake(1_000_000);
        staking
            .lock()
            .unwrap()
            .seed_delegator_stake("voter", 700_000);
        id = open_proposal(&mut gov, &state);
        gov.vote(11, "voter", id, VoteOption::Yes).unwrap();
        assert!(!state.lock().unwrap().accounts.contains_key("voter"));
        assert_eq!(gov.tally(id).unwrap().yes, 700_000);
        assert!(gov.vote(12, "voter", id, VoteOption::No).is_err());
        state.lock().unwrap().accounts.clear();
        // Existing votes retain their cast-time weight. This is not an opening snapshot.
        staking
            .lock()
            .unwrap()
            .seed_delegator_stake("voter", 800_000);
        assert_eq!(gov.get_proposal_votes(id).unwrap().len(), 1);
        assert_eq!(gov.tally(id).unwrap().yes, 700_000);
        assert!(state.lock().unwrap().accounts.is_empty());
    }
    let (mut gov, state, _staking, _storage) = fixture(&path);
    assert!(state.lock().unwrap().accounts.is_empty());
    assert_eq!(gov.tally(id).unwrap().yes, 700_000);
    assert_eq!(gov.tally(id).unwrap().total_voting_power, 700_000);
    assert!(gov.vote(12, "voter", id, VoteOption::No).is_err());
    gov.end_block(311).unwrap();
    assert_eq!(
        gov.get_proposal(id).unwrap().unwrap().status,
        ProposalStatus::Passed
    );
    assert!(state.lock().unwrap().accounts.is_empty());
}

#[test]
fn vote_scan_is_proposal_scoped_and_has_deterministic_key_order() {
    let dir = tempfile::tempdir().unwrap();
    let (gov, state, _staking, storage) = fixture(&dir.path().join("node.db"));
    store_vote(&storage, 1, "z", 3, VoteOption::No);
    store_vote(&storage, 10, "other", 900, VoteOption::Yes);
    store_vote(&storage, 1, "a:part", 7, VoteOption::Yes);
    storage
        .db
        .put("gov:vote:1;unrelated", b"unrelated")
        .unwrap();
    let votes = gov.get_proposal_votes(1).unwrap();
    assert_eq!(
        votes.iter().map(|v| v.voter.as_str()).collect::<Vec<_>>(),
        vec!["a:part", "z"]
    );
    assert_eq!(gov.tally(1).unwrap().total_voting_power, 10);
    assert_eq!(gov.tally(10).unwrap().total_voting_power, 900);
    assert!(gov.get_proposal_votes(2).unwrap().is_empty());
    assert!(state.lock().unwrap().accounts.is_empty());
}

#[test]
fn invalid_stored_vote_stops_tally_without_proposal_transition() {
    let dir = tempfile::tempdir().unwrap();
    let (mut gov, state, _staking, storage) = fixture(&dir.path().join("node.db"));
    let id = open_proposal(&mut gov, &state);
    let key = format!("gov:vote:{id}:voter");
    let wrong_proposal = Vote {
        proposal_id: id + 1,
        voter: "voter".into(),
        option: VoteOption::Yes,
        weight: 1,
    };
    let wrong_voter = Vote {
        proposal_id: id,
        voter: "different".into(),
        option: VoteOption::Yes,
        weight: 1,
    };
    for bytes in [
        b"invalid record".to_vec(),
        bincode::serialize(&wrong_proposal).unwrap(),
        bincode::serialize(&wrong_voter).unwrap(),
    ] {
        storage.db.put(&key, bytes).unwrap();
        gov.clear_events();
        assert!(gov.get_proposal_votes(id).is_err());
        assert!(gov.tally(id).is_err());
        assert!(gov.end_block(311).is_err());
        let proposal = gov.get_proposal(id).unwrap().unwrap();
        assert_eq!(proposal.status, ProposalStatus::VotingPeriod);
        assert!(proposal.tally.is_none());
        assert!(gov.get_events().is_empty());
    }
}

#[test]
fn tally_reports_option_and_combined_weight_overflow() {
    let dir = tempfile::tempdir().unwrap();
    let (gov, _state, _staking, storage) = fixture(&dir.path().join("node.db"));
    store_vote(&storage, 1, "a", u128::MAX, VoteOption::Yes);
    store_vote(&storage, 1, "b", 1, VoteOption::Yes);
    assert_eq!(gov.tally(1).unwrap_err(), "Vote option total exceeds u128");
    store_vote(&storage, 1, "b", 1, VoteOption::No);
    assert_eq!(gov.tally(1).unwrap_err(), "Total voting power exceeds u128");
    store_vote(&storage, 1, "a", u128::MAX - 1, VoteOption::Yes);
    assert_eq!(gov.tally(1).unwrap().total_voting_power, u128::MAX);
}

#[test]
fn current_development_threshold_boundaries_remain_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let (gov, _state, staking, _storage) = fixture(&dir.path().join("node.db"));
    staking.lock().unwrap().seed_total_stake(10_000);
    let tally = |yes, no, veto, abstain| TallyResult {
        yes,
        no,
        no_with_veto: veto,
        abstain,
        total_voting_power: yes + no + veto + abstain,
    };
    // These are current defaults and integer arithmetic, not approved mainnet terms.
    assert_eq!(gov.get_config().quorum, 6700);
    assert_eq!(gov.get_config().threshold, 5000);
    assert_eq!(gov.get_config().veto_threshold, 3333);
    assert!(!gov.proposal_passes(&tally(0, 0, 0, 0)).unwrap());
    assert!(!gov.proposal_passes(&tally(6699, 0, 0, 0)).unwrap());
    assert!(gov.proposal_passes(&tally(6700, 0, 0, 0)).unwrap());
    assert!(!gov.proposal_passes(&tally(4999, 5001, 0, 0)).unwrap());
    assert!(gov.proposal_passes(&tally(5000, 5000, 0, 0)).unwrap());
    assert!(gov.proposal_passes(&tally(6668, 0, 3332, 0)).unwrap());
    assert!(!gov.proposal_passes(&tally(6667, 0, 3333, 0)).unwrap());
    assert!(!gov.proposal_passes(&tally(0, 0, 0, 10_000)).unwrap());
}
