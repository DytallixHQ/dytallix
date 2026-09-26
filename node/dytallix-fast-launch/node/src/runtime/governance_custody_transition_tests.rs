use super::*;
use crate::runtime::{
    governance_deposit_stage::{DepositRules, DepositStage, DepositStageStatus},
    governance_escrow::{DepositEscrow, EscrowStatus, TerminalOutcome},
    governance_state::GOVERNANCE_STATE_VERSION,
};
use dytallix_protocol_types::ordinary_v3::governance_action_digest;

const OWNER: OwnerId = [7; 32];

fn admitted() -> GovernanceState {
    let genesis =
        GovernanceState::new(GOVERNANCE_STATE_VERSION, "custody-test".into(), [1; 32], 0).unwrap();
    let action = vec![9];
    let digest = governance_action_digest(17, &action).unwrap();
    let stage = DepositStage::new(
        DepositRules {
            deposit_period_blocks: 2,
            minimum_deposit_udgt: 10,
            max_action_bytes: 1,
        },
        1,
        17,
        action,
        digest,
        1,
    )
    .unwrap();
    genesis
        .plan_commit(
            1,
            BTreeMap::from([(
                1,
                ProposalRecord::new(stage, DepositEscrow::new(1, 2).unwrap(), None),
            )]),
        )
        .unwrap()
}

fn snapshots(balance: u128, spendable: u128) -> BTreeMap<OwnerId, AccountDgt> {
    BTreeMap::from([(
        OWNER,
        AccountDgt {
            balance_udgt: balance,
            spendable_udgt: spendable,
        },
    )])
}

#[test]
fn deposit_then_exact_height_rejection_refunds_once_and_preserves_dgt() {
    let parent = admitted();
    let deposit = plan_custody_block(
        &parent,
        2,
        &snapshots(20, 6),
        vec![Deposit {
            proposal_id: 1,
            owner: OWNER,
            amount_udgt: 5,
        }],
        &BTreeSet::new(),
    )
    .unwrap();
    assert_eq!(deposit.account_writes[0].balance_after_udgt, 15);
    assert_eq!(deposit.next_state.held_total_udgt().unwrap(), 5);
    assert_eq!(
        GovernanceState::decode(&deposit.state_bytes).unwrap(),
        deposit.next_state
    );
    let refund = plan_custody_block(
        &deposit.next_state,
        3,
        &snapshots(15, 1),
        vec![],
        &BTreeSet::from([1]),
    )
    .unwrap();
    assert_eq!(refund.account_writes[0].balance_after_udgt, 20);
    assert_eq!(refund.next_state.held_total_udgt().unwrap(), 0);
    let record = &refund.next_state.proposals()[&1];
    assert_eq!(record.stage().status(), DepositStageStatus::Rejected);
    assert_eq!(
        record.escrow().status(),
        EscrowStatus::Refunded(TerminalOutcome::Rejected)
    );
    assert!(plan_custody_block(
        &refund.next_state,
        4,
        &snapshots(20, 6),
        vec![],
        &BTreeSet::from([1]),
    )
    .is_err());
}

#[test]
fn insufficient_spendable_and_missing_refund_owner_leave_parent_unchanged() {
    let parent = admitted();
    let unchanged = parent.encode().unwrap();
    assert!(plan_custody_block(
        &parent,
        2,
        &snapshots(20, 4),
        vec![Deposit {
            proposal_id: 1,
            owner: OWNER,
            amount_udgt: 5
        }],
        &BTreeSet::new(),
    )
    .unwrap_err()
    .to_string()
    .contains("Insufficient spendable DGT"));
    assert_eq!(parent.encode().unwrap(), unchanged);

    let deposit = plan_custody_block(
        &parent,
        2,
        &snapshots(20, 5),
        vec![Deposit {
            proposal_id: 1,
            owner: OWNER,
            amount_udgt: 5,
        }],
        &BTreeSet::new(),
    )
    .unwrap();
    assert!(plan_custody_block(
        &deposit.next_state,
        3,
        &BTreeMap::new(),
        vec![],
        &BTreeSet::from([1]),
    )
    .is_err());
}

#[test]
fn repeated_owner_deposits_follow_finalized_order() {
    let parent = admitted();
    let accounts = snapshots(20, 6);
    let first = Deposit {
        proposal_id: 1,
        owner: OWNER,
        amount_udgt: 2,
    };
    let second = Deposit {
        proposal_id: 1,
        owner: OWNER,
        amount_udgt: 3,
    };
    let a =
        plan_custody_block(&parent, 2, &accounts, vec![first, second], &BTreeSet::new()).unwrap();
    assert_eq!(a.next_state.proposals()[&1].escrow().deposited_by(OWNER), 5);
    assert_eq!(a.account_writes[0].balance_after_udgt, 15);
    assert_eq!(a.account_writes[0].spendable_after_udgt, 1);
    assert_eq!(
        a.effects,
        vec![
            CustodyEffect::Deposit {
                proposal_id: 1,
                owner: OWNER,
                amount_udgt: 2
            },
            CustodyEffect::Deposit {
                proposal_id: 1,
                owner: OWNER,
                amount_udgt: 3
            },
        ]
    );
    let reversed =
        plan_custody_block(&parent, 2, &accounts, vec![second, first], &BTreeSet::new()).unwrap();
    assert_eq!(
        reversed.effects,
        vec![a.effects[1].clone(), a.effects[0].clone()]
    );
    assert_eq!(reversed.state_bytes, a.state_bytes);
    assert!(plan_custody_block(
        &parent,
        2,
        &snapshots(20, 4),
        vec![first, second],
        &BTreeSet::new(),
    )
    .is_err());
}

#[test]
fn close_requires_exact_height_and_below_minimum() {
    let parent = admitted();
    assert!(
        plan_custody_block(&parent, 2, &BTreeMap::new(), vec![], &BTreeSet::from([1])).is_err()
    );
    let full = plan_custody_block(
        &parent,
        2,
        &snapshots(20, 20),
        vec![Deposit {
            proposal_id: 1,
            owner: OWNER,
            amount_udgt: 10,
        }],
        &BTreeSet::new(),
    )
    .unwrap();
    assert!(plan_custody_block(
        &full.next_state,
        3,
        &snapshots(10, 10),
        vec![],
        &BTreeSet::from([1]),
    )
    .unwrap_err()
    .to_string()
    .contains("Voting rules and parent snapshot"));
    assert!(plan_custody_block(
        &full.next_state,
        3,
        &snapshots(10, 10),
        vec![],
        &BTreeSet::new(),
    )
    .is_err());
}
