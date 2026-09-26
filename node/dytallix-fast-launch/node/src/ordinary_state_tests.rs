//! Durable local state tests use explicit synthetic genesis records.
use super::*;
use crate::{ordinary_authority::DiscretionaryGrant, recovery_fees::RecoveryAccount};
use dytallix_protocol_types::{
    recovery::{RecoveryConfig, RecoveryDomain, RecoveryState},
    recovery_sponsor,
};
struct Fixture {
    config: OrdinaryConfig,
    lifecycle: LifecycleConfig,
    book: RecoveryBook,
    nonces: BTreeMap<String, u64>,
}
fn fixture() -> Fixture {
    let chain = "ordinary-state-local";
    let mut accounts = Vec::new();
    let mut origins = BTreeMap::new();
    for value in [1u8, 2] {
        let key = KeyIdentity {
            algorithm: "mldsa65".into(),
            public_key: vec![value; 1952],
        };
        let address = AccountAddress::from_origin_key(
            AddressNetwork::Development,
            chain,
            OriginKeyAlgorithm::MlDsa65,
            &key.public_key,
        )
        .unwrap();
        origins.insert(hex::encode(address.account_id()), key.clone());
        let recovery = RecoveryState::new(
            RecoveryDomain {
                network: 3,
                chain_id: chain.into(),
                genesis_digest: [7; 32],
                account_id: *address.account_id(),
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
            key,
            0,
        )
        .unwrap();
        accounts.push(RecoveryAccount {
            address: address.encode(),
            recovery,
            sponsor_nonce: 0,
        });
    }
    let lifecycle = LifecycleConfig {
        version: 1,
        profile: "cometbft-lifecycle-local-qualification".into(),
        chain_id: chain.into(),
        approved_operators: BTreeMap::from([("validator-a".into(), accounts[0].address.clone())]),
        min_self_bond: 10,
        max_active: 4,
        evidence_max_age_blocks: 3,
        evidence_max_age_seconds: 3,
        processing_margin_blocks: 2,
        processing_margin_seconds: 2,
    };
    let book = RecoveryBook::new(
        recovery_sponsor::FeeProfile {
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
        },
        accounts,
    )
    .unwrap();
    let vectors: serde_json::Value = serde_json::from_str(include_str!(
        "../../../crates/protocol-types/tests/fixtures/ordinary_fee_v1_vectors.json"
    ))
    .unwrap();
    let mut fee_profile: FeeProfile = serde_json::from_value(vectors["profile"].clone()).unwrap();
    fee_profile.validator_proof_profile_digest = validator_profile_digest(&lifecycle).unwrap();
    fee_profile.validator_proof_costs = BTreeMap::from([("mldsa65".into(), 400)]);
    let config = OrdinaryConfig {
        version: 1,
        fee_profile,
        origins,
        initial_grants: BTreeMap::new(),
        max_state_bytes: 4_000_000,
        max_grants: 2,
        max_receipts: 100,
        max_retained_profiles: 1,
        max_transport_bytes: 200_000,
        queue_max_entries: 20,
        queue_max_wire_bytes: 1_000_000,
        queue_max_signature_work: 100,
    };
    let nonces = book
        .accounts
        .values()
        .map(|a| (a.address.clone(), a.recovery.spending_nonce))
        .collect();
    Fixture {
        config,
        lifecycle,
        book,
        nonces,
    }
}
fn state(f: &Fixture) -> OrdinaryState {
    OrdinaryState::genesis(f.config.clone(), &f.lifecycle, &f.book, &f.nonces).unwrap()
}
fn advance(f: &mut Fixture, height: u64) {
    for a in f.book.accounts.values_mut() {
        a.recovery = a.recovery.advance_height(height).unwrap();
    }
    f.book.last_height = height;
}
fn grant(f: &Fixture) -> DiscretionaryGrant {
    let values: Vec<_> = f.book.accounts.values().collect();
    DiscretionaryGrant {
        version: 1,
        owner: values[0].recovery.domain.account_id,
        beneficiary: values[1].recovery.domain.account_id,
        owner_generation: 0,
        period_blocks: 10,
        last_active_height: 0,
    }
}
#[test]
fn fresh_state_roundtrip_preserves_explicit_origins_and_current_authority() {
    let mut f = fixture();
    let original = state(&f);
    let bytes = original.encode().unwrap();
    assert_eq!(
        OrdinaryState::decode(&bytes, &f.config, &f.lifecycle, &f.book, &f.nonces).unwrap(),
        original
    );
    let original_id = f.book.accounts.keys().next().unwrap().clone();
    let account = f.book.accounts.get_mut(&original_id).unwrap();
    account.recovery.active_key = KeyIdentity {
        algorithm: "mldsa87".into(),
        public_key: vec![8; 2592],
    };
    account.recovery.active_generation = 1;
    // The fixed origin remains valid when a later current key differs.
    original.validate(&f.lifecycle, &f.book, &f.nonces).unwrap();
    assert_eq!(
        f.book.accounts[&original_id].recovery.domain.account_id,
        hex::decode(&original_id).unwrap().as_slice()
    );
    let mut changed = f.config.clone();
    changed.origins.get_mut(&original_id).unwrap().public_key[0] ^= 1;
    assert!(changed.validate(&f.lifecycle, &f.book).is_err());
}
#[test]
fn genesis_requires_exact_origins_nonce_mirrors_and_fresh_state() {
    let f = fixture();
    let mut missing = f.config.clone();
    missing.origins.pop_first();
    assert!(OrdinaryState::genesis(missing, &f.lifecycle, &f.book, &f.nonces).is_err());
    let mut missing = f.nonces.clone();
    missing.pop_first();
    assert!(OrdinaryState::genesis(f.config.clone(), &f.lifecycle, &f.book, &missing).is_err());
    let mut wrong = f.nonces.clone();
    *wrong.values_mut().next().unwrap() = 1;
    assert!(OrdinaryState::genesis(f.config.clone(), &f.lifecycle, &f.book, &wrong).is_err());
    let mut extra = f.nonces.clone();
    extra.insert("unregistered".into(), 0);
    assert!(OrdinaryState::genesis(f.config.clone(), &f.lifecycle, &f.book, &extra).is_err());
    let mut older = fixture();
    advance(&mut older, 1);
    assert!(
        OrdinaryState::genesis(older.config, &older.lifecycle, &older.book, &older.nonces).is_err()
    );
}
#[test]
fn lifecycle_role_profile_is_exact_and_does_not_expand_with_account_algorithms() {
    let f = fixture();
    assert!(f.config.validate(&f.lifecycle, &f.book).is_ok());
    let mut role = f.lifecycle.clone();
    role.processing_margin_blocks += 1;
    assert_ne!(
        validator_profile_digest(&role).unwrap(),
        validator_profile_digest(&f.lifecycle).unwrap()
    );
    assert!(f.config.validate(&role, &f.book).is_err());
    let mut role = f.lifecycle.clone();
    role.min_self_bond += 1;
    assert_ne!(
        validator_profile_digest(&role).unwrap(),
        validator_profile_digest(&f.lifecycle).unwrap()
    );
    let mut widened = f.config.clone();
    widened
        .fee_profile
        .validator_proof_costs
        .insert("mldsa87".into(), 100);
    assert!(widened.validate(&f.lifecycle, &f.book).is_err());
    let mut unknown = f.lifecycle.clone();
    *unknown.approved_operators.values_mut().next().unwrap() = "unregistered".into();
    let mut p = f.config.clone();
    p.fee_profile.validator_proof_profile_digest = validator_profile_digest(&unknown).unwrap();
    assert!(p.validate(&unknown, &f.book).is_err());
}
#[test]
fn versioned_grants_persist_stale_generation_without_automatic_reactivation() {
    let mut f = fixture();
    let g = grant(&f);
    f.config
        .initial_grants
        .insert(hex::encode(g.owner), g.clone());
    let mut s = state(&f);
    f.book
        .accounts
        .get_mut(&hex::encode(g.owner))
        .unwrap()
        .recovery
        .active_generation = 1;
    s.validate(&f.lifecycle, &f.book, &f.nonces).unwrap();
    assert_eq!(s.grants[&hex::encode(g.owner)].owner_generation, 0);
    s.grants
        .get_mut(&hex::encode(g.owner))
        .unwrap()
        .owner_generation = 2;
    assert!(s.validate(&f.lifecycle, &f.book, &f.nonces).is_err());
    s.grants
        .get_mut(&hex::encode(g.owner))
        .unwrap()
        .owner_generation = 0;
    s.grants
        .get_mut(&hex::encode(g.owner))
        .unwrap()
        .last_active_height = 1;
    assert!(s.validate(&f.lifecycle, &f.book, &f.nonces).is_err());
}
#[test]
fn state_decode_rejects_unknown_duplicate_noncanonical_and_rebound_configuration() {
    let f = fixture();
    let s = state(&f);
    let bytes = s.encode().unwrap();
    let mut expected = f.config.clone();
    expected.queue_max_entries += 1;
    assert!(OrdinaryState::decode(&bytes, &expected, &f.lifecycle, &f.book, &f.nonces).is_err());
    let mut view = serde_json::to_value(&s).unwrap();
    view["unknown"] = true.into();
    assert!(OrdinaryState::decode(
        &serde_json::to_vec(&view).unwrap(),
        &f.config,
        &f.lifecycle,
        &f.book,
        &f.nonces
    )
    .is_err());
    let json = String::from_utf8(bytes.clone()).unwrap();
    let duplicate = json.replacen("\"version\":1", "\"version\":1,\"version\":2", 1);
    assert!(OrdinaryState::decode(
        duplicate.as_bytes(),
        &f.config,
        &f.lifecycle,
        &f.book,
        &f.nonces
    )
    .is_err());
    let mut padded = bytes;
    padded.push(b' ');
    assert!(OrdinaryState::decode(&padded, &f.config, &f.lifecycle, &f.book, &f.nonces).is_err());
    let mut s = s;
    s.version = 2;
    assert!(s.encode().is_err());
}
#[test]
fn append_stages_one_complete_record_and_restart_requires_matching_nonce_state() {
    let f = fixture();
    let s = state(&f);
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::open(temp.path().join("db")).unwrap();
    assert_absent(&storage).unwrap();
    assert!(read_native_nonces(&storage, &f.book).is_err());
    assert!(load(&storage, &f.config, &f.lifecycle, &f.book, &f.nonces).is_err());
    let mut writes = Writes::new();
    append_writes(&s, &mut writes).unwrap();
    assert!(storage.db.get(STATE_KEY).unwrap().is_none());
    assert_eq!(writes.len(), 1);
    append_writes(&s, &mut writes).unwrap();
    let mut conflict = Writes::from([(STATE_KEY.as_bytes().to_vec(), b"different".to_vec())]);
    let before = conflict.clone();
    assert!(append_writes(&s, &mut conflict).is_err());
    assert_eq!(conflict, before);
    let mut batch = rocksdb::WriteBatch::default();
    for (key, value) in writes {
        batch.put(key, value);
    }
    for (address, nonce) in &f.nonces {
        batch.put(
            format!("acct:nonce:{address}"),
            bincode::serialize(nonce).unwrap(),
        );
    }
    let mut options = rocksdb::WriteOptions::default();
    options.set_sync(true);
    storage.db.write_opt(batch, &options).unwrap();
    drop(storage);
    let storage = Storage::open(temp.path().join("db")).unwrap();
    let nonces = read_native_nonces(&storage, &f.book).unwrap();
    assert_eq!(
        load(&storage, &f.config, &f.lifecycle, &f.book, &nonces).unwrap(),
        s
    );
    assert!(assert_absent(&storage).is_err());
    let address = f.nonces.keys().next().unwrap();
    storage
        .db
        .put(
            format!("acct:nonce:{address}"),
            bincode::serialize(&1u64).unwrap(),
        )
        .unwrap();
    let mismatched = read_native_nonces(&storage, &f.book).unwrap();
    assert!(load(&storage, &f.config, &f.lifecycle, &f.book, &mismatched).is_err());
}
#[test]
fn combined_store_rejects_legacy_grants_or_unknown_ordinary_namespaces() {
    let f = fixture();
    let s = state(&f);
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::open(temp.path().join("db")).unwrap();
    storage.db.put("dms:config:legacy", b"legacy").unwrap();
    assert!(assert_absent(&storage).is_err());
    storage.db.put(STATE_KEY, s.encode().unwrap()).unwrap();
    assert!(load(&storage, &f.config, &f.lifecycle, &f.book, &f.nonces).is_err());
    storage.db.delete("dms:config:legacy").unwrap();
    storage.db.put("ordinary:v2:unknown", b"unknown").unwrap();
    assert!(load(&storage, &f.config, &f.lifecycle, &f.book, &f.nonces).is_err());
}
#[test]
fn retained_receipts_bind_profile_height_actor_nonce_and_fee_conservation() {
    let mut f = fixture();
    advance(&mut f, 20);
    let actor = f.book.accounts.keys().next().unwrap().clone();
    f.book
        .accounts
        .get_mut(&actor)
        .unwrap()
        .recovery
        .spending_nonce = 1;
    let address = f.book.accounts[&actor].address.clone();
    f.nonces.insert(address, 1);
    let digest = ordinary_fees::profile_digest(&f.config.fee_profile).unwrap();
    let actor_id = f.book.accounts[&actor].recovery.domain.account_id;
    let receipt = serde_json::json!({"version":1,"transaction_id":vec![1;32],"envelope_hash":vec![2;32],"actor":actor_id,"block_height":20,"block_index":0,"contract_version":1,"profile_version":f.config.fee_profile.version,"profile_digest":digest,"outcome":"Success","failing_action":null,"failure_phase":null,"rule_class":null,"rule_code":null,"gas_limit":1000,"gas_used":200,"metadata_gas":100,"reserved_cap":2000,"charge":400,"released_cap":1600,"nonce_before":0,"nonce_after":1});
    let history = serde_json::json!({"receipts":{(hex::encode([1;32])):receipt},"profiles":{(hex::encode(digest)):serde_json::to_value(&f.config.fee_profile).unwrap()}});
    let mut s = OrdinaryState {
        version: 1,
        config: f.config.clone(),
        grants: Grants::new(),
        history: serde_json::from_value(history.clone()).unwrap(),
        last_height: 20,
    };
    s.validate(&f.lifecycle, &f.book, &f.nonces).unwrap();
    for (field, value) in [
        ("block_height", serde_json::json!(21)),
        ("nonce_after", serde_json::json!(2)),
        ("charge", serde_json::json!(401)),
        ("profile_version", serde_json::json!(99)),
    ] {
        let mut changed = history.clone();
        changed["receipts"][hex::encode([1; 32])][field] = value;
        s.history = serde_json::from_value(changed).unwrap();
        assert!(
            s.validate(&f.lifecycle, &f.book, &f.nonces).is_err(),
            "{field}"
        );
    }
}

#[test]
fn combined_profile_rejects_reachable_replacement_algorithms_outside_ordinary_role() {
    let mut f = fixture();
    // The current keys are ML-DSA-65, but the recovery map also permits a future
    // ML-DSA-87 replacement. Checking only current keys would miss this mismatch.
    f.config
        .fee_profile
        .limits
        .allowed_algorithms
        .remove("mldsa87");
    f.config.fee_profile.signature_costs.remove("mldsa87");
    f.config.fee_profile.validate().unwrap();
    assert!(f
        .book
        .accounts
        .values()
        .all(|a| a.recovery.active_key.algorithm == "mldsa65"));
    assert!(f.config.validate(&f.lifecycle, &f.book).is_err());
    assert!(OrdinaryState::genesis(f.config.clone(), &f.lifecycle, &f.book, &f.nonces).is_err());
    // Explicitly narrowing the reachable replacement set resolves the mismatch.
    // The validator proof role, guardian thresholds and signer checks are unchanged.
    for account in f.book.accounts.values_mut() {
        account.recovery.config.algorithms.remove("mldsa87");
    }
    f.book.validate().unwrap();
    let state = OrdinaryState::genesis(f.config.clone(), &f.lifecycle, &f.book, &f.nonces).unwrap();
    let bytes = state.encode().unwrap();
    assert_eq!(
        OrdinaryState::decode(&bytes, &f.config, &f.lifecycle, &f.book, &f.nonces).unwrap(),
        state
    );
    // A later expansion of the recovery map cannot bypass the same restart check.
    f.book
        .accounts
        .values_mut()
        .next()
        .unwrap()
        .recovery
        .config
        .algorithms
        .insert("mldsa87".into(), 2592);
    assert!(OrdinaryState::decode(&bytes, &f.config, &f.lifecycle, &f.book, &f.nonces).is_err());
}
