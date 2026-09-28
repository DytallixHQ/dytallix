//! Per-transaction reconciliation of v3 governance (E04 gap 12).
use super::*;
use crate::storage::state::Storage;

const ACTOR: &str = "actor";
const OTHER: &str = "other";

/// A settlement with two loaded accounts, and the effects of a successful
/// deposit of 20 uDGT charged 30 uDRT.
fn pair() -> (tempfile::TempDir, Settlement, Settlement) {
    let dir = tempfile::tempdir().unwrap();
    let mut before = Settlement::new(Arc::new(Storage::open(dir.path().join("db")).unwrap()));
    let actor = before.account(ACTOR).unwrap();
    actor.set_balance("udrt", 1000);
    actor.set_balance("udgt", 500);
    actor.nonce = 4;
    let other = before.account(OTHER).unwrap();
    other.set_balance("udrt", 50);
    other.set_balance("udgt", 100);
    before.fee_total = Some(17);
    let mut after = before.clone();
    after.charge_sponsored(ACTOR, 30).unwrap();
    let actor = after.account(ACTOR).unwrap();
    actor.set_balance("udgt", 480);
    actor.nonce = 5;
    (dir, before, after)
}

fn delta(deposit: u128, held_after: u128) -> Delta<'static> {
    Delta {
        actor: ACTOR,
        charge: 30,
        deposit,
        nonce_before: 4,
        held_before: 7,
        held_after,
    }
}

fn refused(before: &Settlement, after: &Settlement, delta: &Delta<'_>, reason: &str) {
    match reconcile(before, after, delta) {
        Err(PlanError::Internal(message)) => assert!(message.contains(reason), "{message}"),
        other => panic!("expected refusal {reason:?}, got {other:?}"),
    }
}

#[test]
fn a_committed_deposit_reconciles_fee_escrow_and_nonce() {
    let (_dir, before, after) = pair();
    reconcile(&before, &after, &delta(20, 27)).unwrap();
}

#[test]
fn a_vote_or_failed_action_moves_only_the_fee() {
    let (_dir, before, mut after) = pair();
    after.account(ACTOR).unwrap().set_balance("udgt", 500);
    reconcile(&before, &after, &delta(0, 7)).unwrap();
    // An absent balance and a zero balance are the same.
    let mut empty = before.clone();
    empty.account(ACTOR).unwrap().set_balance("udgt", 0);
    let mut charged = empty.clone();
    charged.charge_sponsored(ACTOR, 30).unwrap();
    charged.account(ACTOR).unwrap().nonce = 5;
    reconcile(&empty, &charged, &delta(0, 7)).unwrap();
}

#[test]
fn fee_custody_and_burns_must_match_the_charge() {
    let (_dir, before, mut after) = pair();
    after.fee_total = Some(17 + 29);
    refused(&before, &after, &delta(20, 27), "fee custody");
    let (_dir, before, mut after) = pair();
    after.burned_total = Some(1);
    refused(&before, &after, &delta(20, 27), "fee custody");
}

#[test]
fn the_held_total_must_move_by_exactly_the_deposit() {
    let (_dir, before, after) = pair();
    refused(&before, &after, &delta(20, 26), "held deposits");
    refused(&before, &after, &delta(0, 27), "held deposits");
}

#[test]
fn only_the_actor_changes_and_only_by_its_charge_deposit_and_nonce() {
    let (_dir, before, mut after) = pair();
    after.account(OTHER).unwrap().set_balance("udgt", 99);
    refused(&before, &after, &delta(20, 27), "account delta");
    let (_dir, before, mut after) = pair();
    after.account(ACTOR).unwrap().set_balance("udgt", 479);
    refused(&before, &after, &delta(20, 27), "account delta");
    let (_dir, before, mut after) = pair();
    after.account(ACTOR).unwrap().set_balance("ucustom", 1);
    refused(&before, &after, &delta(20, 27), "account delta");
    let (_dir, before, mut after) = pair();
    after.account(ACTOR).unwrap().nonce = 6;
    refused(&before, &after, &delta(20, 27), "account delta");
    let (_dir, before, after) = pair();
    refused(
        &before,
        &after,
        &Delta { nonce_before: 3, ..delta(20, 27) },
        "nonce predecessor",
    );
    let (_dir, before, mut after) = pair();
    after.account("loaded-later").unwrap();
    refused(&before, &after, &delta(20, 27), "cannot change");
}

#[test]
fn staking_lifecycle_and_evidence_state_must_not_change() {
    let (_dir, before, mut after) = pair();
    after.evidence_records.insert(b"evidence".to_vec(), vec![1]);
    refused(&before, &after, &delta(20, 27), "staking or lifecycle");
    let (_dir, before, mut after) = pair();
    after.lifecycle.insert(b"lifecycle".to_vec(), vec![1]);
    refused(&before, &after, &delta(20, 27), "staking or lifecycle");
    let (_dir, before, mut after) = pair();
    after.reward_timestamp = Some(9);
    refused(&before, &after, &delta(20, 27), "staking or lifecycle");
}
