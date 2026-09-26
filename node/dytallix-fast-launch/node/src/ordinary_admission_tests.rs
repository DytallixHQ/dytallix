use super::*;
use crate::ordinary_reservations::{Asset, Denomination, EligibleLiquidity};
use std::collections::BTreeMap;
fn config() -> OrdinaryConfig {
    let vectors: serde_json::Value = serde_json::from_str(include_str!(
        "../../../crates/protocol-types/tests/fixtures/ordinary_fee_v1_vectors.json"
    ))
    .unwrap();
    OrdinaryConfig {
        version: 1,
        fee_profile: serde_json::from_value(vectors["profile"].clone()).unwrap(),
        origins: BTreeMap::new(),
        initial_grants: BTreeMap::new(),
        max_state_bytes: 100000,
        max_grants: 1,
        max_receipts: 1,
        max_retained_profiles: 1,
        max_transport_bytes: 100000,
        queue_max_entries: 2,
        queue_max_wire_bytes: 1000,
        queue_max_signature_work: 4,
    }
}
fn request(config: &OrdinaryConfig, id: u8, sponsor: bool) -> ReservationRequest {
    ReservationRequest {
        context_digest: ordinary_fees::profile_digest(&config.fee_profile).unwrap(),
        id: if sponsor {
            ReservationId::RecoverySponsorship([id; 32])
        } else {
            ReservationId::Ordinary([id; 32])
        },
        payer: [1; 32],
        nonce: 0,
        fee_cap_udrt: 10,
        action_debits: vec![],
        unrestricted_debits: vec![],
        wire_bytes: 100,
        signature_work: 1,
    }
}
fn liquidity(amount: u128) -> Eligibility {
    let total: EligibleLiquidity = BTreeMap::from([(
        Asset {
            owner: [1; 32],
            denomination: Denomination::Udrt,
        },
        amount,
    )]);
    Eligibility {
        unrestricted: total.clone(),
        total,
    }
}
#[test]
fn retained_same_head_shares_ordinary_and_sponsor_funds_and_exact_dedup() {
    let mut queue = OrdinaryAdmissionQueue::default();
    let config = config();
    let head = "a".repeat(64);
    assert_eq!(queue.len(), 0);
    assert!(queue
        .reserve(&request(&config, 1, false), &liquidity(20))
        .is_err());
    queue.reset(&head, &config).unwrap();
    assert_eq!(
        queue
            .reserve(&request(&config, 1, false), &liquidity(20))
            .unwrap(),
        ReservationStatus::Added
    );
    queue.reset(&head, &config).unwrap();
    assert_eq!(queue.len(), 1);
    assert_eq!(
        queue
            .reserve(&request(&config, 1, false), &liquidity(20))
            .unwrap(),
        ReservationStatus::AlreadyReserved
    );
    let before = queue.clone();
    assert!(queue
        .reserve(&request(&config, 2, false), &liquidity(20))
        .is_err());
    assert_eq!(queue, before);
    queue
        .reserve(&request(&config, 1, true), &liquidity(20))
        .unwrap();
    assert_eq!(queue.len(), 2);
    let mut another = request(&config, 2, true);
    another.nonce = 1;
    let before = queue.clone();
    assert!(queue.reserve(&another, &liquidity(20)).is_err());
    assert_eq!(queue, before);
}
#[test]
fn changed_committed_head_or_any_configuration_field_discards_stale_entries() {
    let mut queue = OrdinaryAdmissionQueue::default();
    let mut config = config();
    let head = "a".repeat(64);
    queue.reset(&head, &config).unwrap();
    queue
        .reserve(&request(&config, 1, false), &liquidity(20))
        .unwrap();
    queue.reset(&"b".repeat(64), &config).unwrap();
    assert_eq!(queue.len(), 0);
    queue
        .reserve(&request(&config, 2, false), &liquidity(20))
        .unwrap();
    config.max_receipts += 1; // Outside the fee profile: still a changed context.
    queue.reset(&"b".repeat(64), &config).unwrap();
    assert_eq!(queue.len(), 0);
    queue
        .reserve(&request(&config, 3, false), &liquidity(20))
        .unwrap();
    config.queue_max_entries += 1;
    queue.reset(&"b".repeat(64), &config).unwrap();
    assert_eq!(queue.len(), 0);
}
#[test]
fn invalid_reset_and_reservation_preserve_existing_queue() {
    let mut queue = OrdinaryAdmissionQueue::default();
    let config = config();
    let head = "a".repeat(64);
    queue.reset(&head, &config).unwrap();
    queue
        .reserve(&request(&config, 1, false), &liquidity(10))
        .unwrap();
    let before = queue.clone();
    for bad in ["", "not-a-head", &"A".repeat(64)] {
        assert!(queue.reset(bad, &config).is_err());
        assert_eq!(queue, before);
    }
    let mut invalid = config.clone();
    invalid.queue_max_entries = 0;
    assert!(queue.reset(&"b".repeat(64), &invalid).is_err());
    assert_eq!(queue, before);
    let mut invalid = config.clone();
    invalid.fee_profile.gas_price = 0;
    assert!(queue.reset(&"b".repeat(64), &invalid).is_err());
    assert_eq!(queue, before);
    assert!(queue
        .reserve(&request(&config, 1, true), &liquidity(10))
        .is_err());
    assert_eq!(queue, before);
}
#[test]
fn explicit_trusted_eviction_releases_once_and_new_admission_rechecks_funding() {
    let mut queue = OrdinaryAdmissionQueue::default();
    let config = config();
    queue.reset(&"a".repeat(64), &config).unwrap();
    let first = request(&config, 1, false);
    queue.reserve(&first, &liquidity(20)).unwrap();
    queue
        .reserve(&request(&config, 1, true), &liquidity(20))
        .unwrap();
    assert!(queue.evict(first.id).unwrap());
    assert!(!queue.evict(first.id).unwrap());
    assert_eq!(queue.len(), 1);
    let before = queue.clone();
    assert!(queue.reserve(&first, &liquidity(19)).is_err());
    assert_eq!(queue, before);
    queue.reserve(&first, &liquidity(20)).unwrap();
    assert_eq!(queue.len(), 2);
    queue.reset(&"b".repeat(64), &config).unwrap();
    assert!(!queue.evict(first.id).unwrap());
    assert_eq!(queue.len(), 0);
}
