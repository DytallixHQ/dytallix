use super::*;
use crate::runtime::governance_candidate::tests::example;

fn rules(quorum: u16, approval: u16, veto: u16) -> BallotRules {
    BallotRules {
        quorum_bps: quorum,
        approval_bps: approval,
        veto_bps: veto,
        ..example().ballot
    }
}

#[test]
fn thresholds_round_up_and_abstentions_count_only_for_quorum() {
    assert_eq!(required_weight(3, 5_000).unwrap(), 2);
    assert_eq!(required_weight(4, 5_000).unwrap(), 2);
    assert_eq!(required_weight(u128::MAX, 10_000).unwrap(), u128::MAX);
    assert_eq!(required_weight(0, 5_000).unwrap(), 0);
    assert!(required_weight(1, 10_001).is_err());
    let rules = rules(5_000, 5_000, 3_334);
    let tally = |yes, no, veto, abstain| Tally {
        yes,
        no,
        no_with_veto: veto,
        abstain,
        votes: 0,
    };
    // Quorum reached only with abstentions; approval uses decisive weight.
    assert!(tally(10, 0, 0, 40).passes(100, &rules).unwrap());
    assert!(!tally(10, 0, 0, 39).passes(100, &rules).unwrap());
    assert!(!tally(0, 0, 0, 100).passes(100, &rules).unwrap());
    assert!(tally(5, 5, 0, 40).passes(100, &rules).unwrap());
    assert!(!tally(4, 6, 0, 40).passes(100, &rules).unwrap());
    // Veto at or above its rounded-up threshold rejects.
    assert!(!tally(6, 0, 4, 40).passes(100, &rules).unwrap());
    assert!(tally(7, 0, 3, 40).passes(100, &rules).unwrap());
    // An empty electorate never passes.
    assert!(!tally(0, 0, 0, 0).passes(0, &rules).unwrap());
}

#[test]
fn a_proposal_record_is_consistent_with_its_phase_and_window() {
    let candidate = example();
    let (deposit, ballot) = (candidate.deposit.clone(), candidate.ballot.clone());
    let data = vec![1, 2];
    let mut proposal = Proposal {
        id: 1,
        proposer: [1; 32],
        action_class: 1,
        action_digest: dytallix_protocol_types::ordinary_v3::governance_action_digest(1, &data)
            .unwrap(),
        action_data: data,
        admitted_height: 5,
        deposits: BTreeMap::from([([2; 32], 3)]),
        deposited: 3,
        phase: Phase::Collecting { close_height: 6 },
    };
    proposal.validate(&deposit, &ballot).unwrap();
    assert_eq!(proposal.due_height().unwrap(), 6);
    assert_eq!(proposal.held(), 3);
    proposal.deposited = 4;
    assert!(proposal.validate(&deposit, &ballot).is_err());
    proposal.deposited = 3;
    proposal.phase = Phase::Collecting { close_height: 7 };
    assert!(proposal.validate(&deposit, &ballot).is_err());
    proposal.phase = Phase::Voting {
        snapshot_height: 5,
        end_height: 7,
        tally: Tally::default(),
    };
    proposal.validate(&deposit, &ballot).unwrap();
    assert_eq!(proposal.due_height().unwrap(), 8);
    proposal.phase = Phase::Passed {
        execute_at: 9,
        tally: Tally::default(),
    };
    proposal.validate(&deposit, &ballot).unwrap();
    proposal.phase = Phase::Finished {
        outcome: Outcome::Executed,
        height: 9,
    };
    assert_eq!((proposal.held(), proposal.due_height().unwrap()), (0, 10));
    proposal.action_data.push(3);
    assert!(proposal.validate(&deposit, &ballot).is_err());
}

#[test]
fn header_counters_and_encoding_are_strict() {
    let header = Header::genesis("chain".into(), [1; 32]).unwrap();
    assert_eq!(Header::decode(&header.encode().unwrap()).unwrap(), header);
    let mut bytes = header.encode().unwrap();
    bytes.push(0);
    assert!(Header::decode(&bytes).is_err());
    assert!(Header::genesis("chain".into(), [0; 32]).is_err());
    let mut ahead = header.clone();
    ahead.proposals = 1;
    assert!(ahead.encode().is_err());
}
