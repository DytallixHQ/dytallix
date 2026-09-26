//! Local authority-model tests. No test activates a fee contract or writes a chain.
use super::*;
use crate::recovery_fees::RecoveryAccount;
use dytallix_protocol_types::{
    ordinary::Denomination,
    recovery::{
        Guardian, KeyIdentity, RecoveryConfig, RecoveryDomain, RecoveryPolicy, RecoveryState,
        RecoveryStatus,
    },
    recovery_sponsor::FeeProfile,
};
fn key(byte: u8) -> KeyIdentity {
    KeyIdentity {
        algorithm: "mldsa65".into(),
        public_key: vec![byte; 1952],
    }
}
fn limits() -> Limits {
    Limits {
        max_wire_bytes: 262_144,
        max_actions: 32,
        max_identifier_bytes: 128,
        max_data_bytes: 4096,
        max_memo_bytes: 1024,
        max_consensus_key_bytes: 4096,
        max_proof_bytes: 8192,
        max_expiry_lifetime: 100,
        allowed_algorithms: BTreeSet::from(["mldsa65".into(), "mldsa87".into()]),
    }
}
fn account(id: u8) -> RecoveryAccount {
    let mut state = RecoveryState::new(
        RecoveryDomain {
            network: 3,
            chain_id: "ordinary-model".into(),
            genesis_digest: [7; 32],
            account_id: [id; 32],
        },
        RecoveryConfig {
            timing_version: 1,
            recovery_delay: 2,
            finalization_window: 3,
            policy_delay: 2,
            policy_window: 3,
            submission_lifetime: 100,
            algorithms: BTreeMap::from([("mldsa65".into(), 1952), ("mldsa87".into(), 2592)]),
        },
        key(id),
        0,
    )
    .unwrap();
    state.policy = Some(RecoveryPolicy {
        threshold: 2,
        guardians: (10..13)
            .map(|g| Guardian {
                key: key(g),
                control_group: format!("guardian-{g}"),
            })
            .collect(),
    });
    state.policy_version = 1;
    RecoveryAccount {
        address: AccountAddress::from_account_id(AddressNetwork::Development, [id; 32]).encode(),
        recovery: state,
        sponsor_nonce: 0,
    }
}
fn book() -> RecoveryBook {
    let profile = FeeProfile {
        version: 1,
        activation_height: 1,
        denomination: "udrt".into(),
        gas_price: 1,
        minimum_gas: 10,
        max_transaction_gas: 100_000,
        max_block_gas: 200_000,
        max_block_recovery_bytes: 262_144,
        max_block_recovery_signatures: 16,
        max_fee_cap: 100_000,
        max_pending_accounts: 2,
        max_due_expiry_events_per_height: 2,
        mandatory_expiry_gas_budget: 20,
        expiry_event_gas_cost: 10,
        action_costs: [10; 9],
        wire_byte_cost: 1,
        read_byte_cost: 1,
        write_byte_cost: 1,
        signature_costs: BTreeMap::from([("mldsa65".into(), 10), ("mldsa87".into(), 10)]),
    };
    let mut book = RecoveryBook::new(profile, vec![account(1), account(2)]).unwrap();
    for state in book.accounts.values_mut() {
        state.recovery = state.recovery.advance_height(10).unwrap();
    }
    book.last_height = 10;
    book.validate().unwrap();
    book
}
fn native_nonces(book: &RecoveryBook) -> BTreeMap<String, u64> {
    book.accounts
        .values()
        .map(|a| (a.address.clone(), a.recovery.spending_nonce))
        .collect()
}
fn grant(owner: u8, beneficiary: u8) -> DiscretionaryGrant {
    DiscretionaryGrant {
        version: 1,
        owner: [owner; 32],
        beneficiary: [beneficiary; 32],
        owner_generation: 0,
        period_blocks: 2,
        last_active_height: 0,
    }
}
fn grants() -> Grants {
    BTreeMap::from([
        (hex::encode([1; 32]), grant(1, 2)),
        (hex::encode([2; 32]), grant(2, 1)),
    ])
}
fn body(book: &RecoveryBook, id: u8, actions: Vec<Action>) -> OrdinaryTransaction {
    let state = &book.accounts[&hex::encode([id; 32])].recovery;
    OrdinaryTransaction {
        domain: state.domain.clone(),
        authorization_generation: state.active_generation,
        spending_nonce: state.spending_nonce,
        key: state.active_key.clone(),
        expiry_height: 50,
        ordinary_fee_contract_version: 1,
        fee_profile_version: 1,
        fee_profile_digest: [9; 32],
        fee_denomination: Denomination::Udrt,
        maximum_fee: 1000,
        gas_limit: 100,
        memo: String::new(),
        actions,
    }
}
fn actions() -> Vec<Action> {
    vec![
        Action::Send {
            recipient: [2; 32],
            denomination: Denomination::Udgt,
            amount: 1,
        },
        Action::Data {
            data: "data".into(),
        },
        Action::DmsRegister {
            beneficiary: [2; 32],
            period_blocks: 2,
        },
        Action::DmsPing,
        Action::DmsClaim {
            owner: [2; 32],
            expected_grant_generation: 0,
        },
        Action::RewardBond {
            validator_id: "validator".into(),
            amount_udgt: 1,
        },
        Action::RewardBeginUnbond {
            validator_id: "validator".into(),
            amount_udgt: 1,
        },
        Action::RewardClaim,
        Action::ValidatorRegister {
            validator_id: "validator".into(),
            consensus_key: vec![1],
            proof: vec![2],
            proof_expiry_height: 50,
            amount_udgt: 1,
        },
        Action::ValidatorRotateKey {
            validator_id: "validator".into(),
            consensus_key: vec![1],
            proof: vec![2],
            proof_expiry_height: 50,
        },
        Action::ValidatorExit {
            validator_id: "validator".into(),
        },
        Action::ValidatorWithdraw {
            unbond_id: "unbond".into(),
        },
    ]
}
#[test]
fn all_twelve_actions_identify_actor_fee_payer_and_actual_debit_owner() {
    let book = book();
    let native = native_nonces(&book);
    let grants = grants();
    let before = (book.clone(), native.clone(), grants.clone());
    for (index, action) in actions().into_iter().enumerate() {
        let result = prepare_body(
            &book,
            &native,
            &grants,
            10,
            &body(&book, 1, vec![action]),
            &limits(),
        )
        .unwrap();
        assert_eq!(result.actor, [1; 32]);
        assert_eq!(result.fee_payer, [1; 32]);
        let expected = if index == 4 {
            BTreeSet::from([[1; 32], [2; 32]])
        } else {
            BTreeSet::from([[1; 32]])
        };
        assert_eq!(result.action_owners[0].debit_owners, expected);
        assert_eq!(result.prospective_nonce.before, 0);
        assert_eq!(result.prospective_nonce.after, 1);
        assert!(!result.is_executable());
        assert!(result.require_paid_execution().is_err());
    }
    assert_eq!((book, native, grants), before);
}
#[test]
fn protection_blocks_all_actor_paths_and_owner_claims_but_allows_incoming_reference() {
    let mut book = book();
    let native = native_nonces(&book);
    let grants = grants();
    book.accounts
        .get_mut(&hex::encode([1; 32]))
        .unwrap()
        .recovery
        .status = RecoveryStatus::RecoveryLocked;
    for action in actions() {
        assert!(prepare_body(
            &book,
            &native,
            &grants,
            10,
            &body(&book, 1, vec![action]),
            &limits()
        )
        .is_err());
    }
    book.accounts
        .get_mut(&hex::encode([1; 32]))
        .unwrap()
        .recovery
        .status = RecoveryStatus::Normal;
    book.accounts
        .get_mut(&hex::encode([2; 32]))
        .unwrap()
        .recovery
        .status = RecoveryStatus::RecoveryLocked;
    assert!(prepare_body(
        &book,
        &native,
        &grants,
        10,
        &body(
            &book,
            1,
            vec![Action::DmsClaim {
                owner: [2; 32],
                expected_grant_generation: 0
            }]
        ),
        &limits()
    )
    .is_err());
    assert!(prepare_body(
        &book,
        &native,
        &grants,
        10,
        &body(
            &book,
            1,
            vec![Action::Send {
                recipient: [2; 32],
                denomination: Denomination::Udrt,
                amount: 1
            }]
        ),
        &limits()
    )
    .is_ok());
}
#[test]
fn explicit_native_nonce_mirrors_are_required_and_never_repaired() {
    let mut book = book();
    let native = native_nonces(&book);
    let original = book.clone();
    let mut missing = native.clone();
    missing.remove(&book.accounts[&hex::encode([2; 32])].address);
    assert!(validate_nonce_mirrors(&book, &missing).is_err());
    book.accounts
        .get_mut(&hex::encode([1; 32]))
        .unwrap()
        .recovery
        .spending_nonce = 1;
    assert!(validate_nonce_mirrors(&book, &native).is_err());
    assert_eq!(native.values().copied().collect::<Vec<_>>(), vec![0, 0]);
    assert_eq!(
        book.accounts[&hex::encode([1; 32])].recovery.spending_nonce,
        1
    );
    let reconciled = native_nonces(&book);
    validate_nonce_mirrors(&book, &reconciled).unwrap();
    let mut alias = original;
    alias
        .accounts
        .get_mut(&hex::encode([1; 32]))
        .unwrap()
        .address = "legacy-address".into();
    assert!(validate_nonce_mirrors(&alias, &native).is_err());
}
#[test]
fn current_domain_key_generation_nonce_and_expiry_are_exact() {
    let book = book();
    let native = native_nonces(&book);
    let grants = grants();
    let original = body(&book, 1, vec![Action::Data { data: "x".into() }]);
    for case in 0..7 {
        let mut tx = original.clone();
        match case {
            0 => tx.domain.genesis_digest = [8; 32],
            1 => tx.key = key(3),
            2 => tx.authorization_generation = 1,
            3 => tx.spending_nonce = 1,
            4 => tx.expiry_height = 10,
            5 => tx.expiry_height = 111,
            _ => tx.domain.chain_id = "different".into(),
        }
        assert!(prepare_body(&book, &native, &grants, 10, &tx, &limits()).is_err());
    }
    assert!(prepare_body(&book, &native, &grants, 9, &original, &limits()).is_err());
    let mut restricted = limits();
    restricted.allowed_algorithms = BTreeSet::from(["mldsa87".into()]);
    assert!(prepare_body(&book, &native, &grants, 10, &original, &restricted).is_err());
}
#[test]
fn changed_generation_invalidates_grant_until_current_owner_registers_again() {
    let mut book = book();
    let native = native_nonces(&book);
    let grants = grants();
    book.accounts
        .get_mut(&hex::encode([2; 32]))
        .unwrap()
        .recovery
        .active_generation = 1;
    let claim = Action::DmsClaim {
        owner: [2; 32],
        expected_grant_generation: 0,
    };
    assert!(prepare_body(
        &book,
        &native,
        &grants,
        10,
        &body(&book, 1, vec![claim]),
        &limits()
    )
    .is_err());
    assert!(prepare_body(
        &book,
        &native,
        &grants,
        10,
        &body(&book, 2, vec![Action::DmsPing]),
        &limits()
    )
    .is_err());
    let registration = prepare_body(
        &book,
        &native,
        &grants,
        10,
        &body(
            &book,
            2,
            vec![Action::DmsRegister {
                beneficiary: [1; 32],
                period_blocks: 2,
            }],
        ),
        &limits(),
    )
    .unwrap();
    assert_eq!(
        registration.prospective_grants[&hex::encode([2; 32])].owner_generation,
        1
    );
    assert_eq!(grants[&hex::encode([2; 32])].owner_generation, 0);
    for a in book.accounts.values_mut() {
        a.recovery = a.recovery.advance_height(12).unwrap();
    }
    book.last_height = 12;
    let result = prepare_body(
        &book,
        &native,
        &registration.prospective_grants,
        12,
        &body(
            &book,
            1,
            vec![Action::DmsClaim {
                owner: [2; 32],
                expected_grant_generation: 1,
            }],
        ),
        &limits(),
    )
    .unwrap();
    assert!(!result.is_executable());
}
#[test]
fn ordered_grant_preparation_does_not_commit_nonce_or_modify_protocol_obligations() {
    let book = book();
    let native = native_nonces(&book);
    let grants = Grants::new();
    let result = prepare_body(
        &book,
        &native,
        &grants,
        10,
        &body(
            &book,
            1,
            vec![
                Action::DmsRegister {
                    beneficiary: [2; 32],
                    period_blocks: 5,
                },
                Action::DmsPing,
            ],
        ),
        &limits(),
    )
    .unwrap();
    assert_eq!(
        result.prospective_grants[&hex::encode([1; 32])].last_active_height,
        10
    );
    assert!(grants.is_empty());
    assert_eq!(
        book.accounts[&hex::encode([1; 32])].recovery.spending_nonce,
        0
    );
    assert!(result.require_paid_execution().is_err());
    let mut future = result.prospective_grants;
    future
        .get_mut(&hex::encode([1; 32]))
        .unwrap()
        .owner_generation = 1;
    assert!(validate_grants(&book, &future).is_err());
}
#[test]
fn exhausted_nonce_and_unmatured_or_wrong_beneficiary_claims_reject() {
    let mut book = book();
    let grants = grants();
    book.accounts
        .get_mut(&hex::encode([1; 32]))
        .unwrap()
        .recovery
        .spending_nonce = u64::MAX;
    let native = native_nonces(&book);
    assert!(prepare_body(
        &book,
        &native,
        &grants,
        10,
        &body(
            &book,
            1,
            vec![Action::Data {
                data: String::new()
            }]
        ),
        &limits()
    )
    .unwrap_err()
    .to_string()
    .contains("exhausted"));
    book.accounts
        .get_mut(&hex::encode([1; 32]))
        .unwrap()
        .recovery
        .spending_nonce = 0;
    let native = native_nonces(&book);
    let mut young = grants.clone();
    young
        .get_mut(&hex::encode([2; 32]))
        .unwrap()
        .last_active_height = 10;
    assert!(prepare_body(
        &book,
        &native,
        &young,
        10,
        &body(
            &book,
            1,
            vec![Action::DmsClaim {
                owner: [2; 32],
                expected_grant_generation: 0
            }]
        ),
        &limits()
    )
    .is_err());
    assert!(prepare_body(
        &book,
        &native,
        &grants,
        10,
        &body(
            &book,
            1,
            vec![Action::DmsClaim {
                owner: [1; 32],
                expected_grant_generation: 0
            }]
        ),
        &limits()
    )
    .is_err());
}
#[test]
fn real_signature_current_key_check_invalidates_cached_old_authority_without_paid_execution() {
    use crate::crypto::{ActivePQC, PQC};
    let mut book = book();
    let native = native_nonces(&book);
    let grants = grants();
    let (old_secret, old_public) = ActivePQC::keypair();
    let (new_secret, new_public) = ActivePQC::keypair();
    book.accounts
        .get_mut(&hex::encode([1; 32]))
        .unwrap()
        .recovery
        .active_key = KeyIdentity {
        algorithm: "mldsa65".into(),
        public_key: old_public,
    };
    let tx = body(
        &book,
        1,
        vec![Action::Data {
            data: "authorized".into(),
        }],
    );
    let signed = SignedOrdinary {
        signature: ActivePQC::sign(&old_secret, &wire::signing_bytes(&tx, &limits()).unwrap()),
        body: tx,
    };
    let verified = verify_signed(&signed, &limits()).unwrap();
    let checked = check_signed(&book, &native, &grants, 10, &signed, &limits()).unwrap();
    assert_eq!(checked.transaction_id, verified.transaction_id());
    assert!(!checked.is_executable());
    assert!(checked.require_paid_execution().is_err());
    let current = &mut book
        .accounts
        .get_mut(&hex::encode([1; 32]))
        .unwrap()
        .recovery;
    current.active_key = KeyIdentity {
        algorithm: "mldsa65".into(),
        public_key: new_public,
    };
    current.active_generation = 1;
    assert!(check_verified(&book, &native, &grants, 10, &verified, &limits()).is_err());
    let tx = body(
        &book,
        1,
        vec![Action::Data {
            data: "new-key".into(),
        }],
    );
    let signed = SignedOrdinary {
        signature: ActivePQC::sign(&new_secret, &wire::signing_bytes(&tx, &limits()).unwrap()),
        body: tx,
    };
    let current = check_signed(&book, &native, &grants, 10, &signed, &limits()).unwrap();
    assert_eq!(current.actor, [1; 32]);
    assert_eq!(current.prospective_nonce.before, 0);
    assert!(!current.is_executable());
}

#[test]
fn fee_preacceptance_defers_only_maturity_and_preserves_authority_rejections() {
    let book = book();
    let native = native_nonces(&book);
    let mut young = grants();
    young
        .get_mut(&hex::encode([2; 32]))
        .unwrap()
        .last_active_height = 10;
    let tx = body(
        &book,
        1,
        vec![Action::DmsClaim {
            owner: [2; 32],
            expected_grant_generation: 0,
        }],
    );
    assert!(prepare_body(&book, &native, &young, 10, &tx, &limits()).is_err());
    let result = prepare_body_phase(&book, &native, &young, 10, &tx, &limits(), true).unwrap();
    assert_eq!(
        result.deferred_application_checks,
        vec![DeferredApplicationCheck::DmsClaimMaturity {
            action_index: 0,
            deadline_height: 12
        }]
    );
    assert!(!result.is_executable());
    let mut stale = tx;
    stale.actions = vec![Action::DmsClaim {
        owner: [2; 32],
        expected_grant_generation: 1,
    }];
    assert!(prepare_body_phase(&book, &native, &young, 10, &stale, &limits(), true).is_err());
    assert_eq!(young[&hex::encode([2; 32])].last_active_height, 10);
}
