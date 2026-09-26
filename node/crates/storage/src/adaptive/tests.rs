use super::*;
use dytallix_adaptive_emission::Gains;

fn config() -> Config {
    Config {
        target_ppm: 700000,
        shock_threshold_ppm: 100000,
        volatility_threshold_ppm: 500000,
        window_samples: 3,
        integral_min: -500000,
        integral_max: 500000,
        soft: Gains {
            proportional: 200000,
            integral: 20000,
            derivative: 50000,
        },
        hard: Gains {
            proportional: 400000,
            integral: 40000,
            derivative: 100000,
        },
        base_udrt: 1400000,
        min_udrt: 500000,
        max_udrt: 2500000,
    }
}
fn observation(epoch: u64) -> Observation {
    Observation {
        epoch,
        utilization_ppm: if epoch % 2 == 0 { 700000 } else { 900000 },
        volatility_ppm: 1000000,
    }
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn independent_binary_vectors_match_checkpoint_and_command_record() {
    let dir = tempfile::tempdir().unwrap();
    let mut storage = Storage::open(dir.path().join("db")).unwrap();
    let mut journal = AdaptiveJournal::initialize(&mut storage, [7; 32], config()).unwrap();
    let expected: Vec<&str> = include_str!("journal-vectors.txt").lines().collect();
    assert_eq!(
        hex(&journal.storage.db.get(HEAD_KEY).unwrap().unwrap()),
        expected[0]
    );
    journal.advance(observation(0)).unwrap();
    assert_eq!(
        hex(&journal.storage.db.get(HEAD_KEY).unwrap().unwrap()),
        expected[1]
    );
    assert_eq!(
        hex(&journal.storage.db.get(event_key(0)).unwrap().unwrap()),
        expected[2]
    );
}

#[test]
fn restart_preserves_history_command_chain_and_unrelated_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut reference = Controller::new(config()).unwrap();
    let mut prior = None;
    for epoch in 0..12 {
        let mut storage = Storage::open(path.clone()).unwrap();
        if epoch == 0 {
            storage.db.put(b"unrelated", b"unchanged").unwrap();
        }
        let mut journal = if epoch == 0 {
            AdaptiveJournal::initialize(&mut storage, [7; 32], config()).unwrap()
        } else {
            AdaptiveJournal::open(&mut storage, [7; 32], config()).unwrap()
        };
        let input = observation(epoch);
        assert_eq!(
            journal.advance(input).unwrap(),
            reference.step(input).unwrap()
        );
        assert_eq!(journal.snapshot().unwrap(), reference.snapshot());
        assert_eq!(journal.verify_history(epoch + 1).unwrap(), epoch + 1);
        let record = journal.record(epoch).unwrap().unwrap();
        if let Some(previous) = prior {
            assert_eq!(record.before_digest, previous);
        }
        prior = Some(record.after_digest);
        assert_eq!(
            journal.storage.db.get(b"unrelated").unwrap().unwrap(),
            b"unchanged"
        );
    }
}

#[test]
fn history_audit_replays_commands_and_enforces_work_limit() {
    let dir = tempfile::tempdir().unwrap();
    let mut storage = Storage::open(dir.path().join("db")).unwrap();
    let mut journal = AdaptiveJournal::initialize(&mut storage, [7; 32], config()).unwrap();
    assert_eq!(journal.verify_history(0).unwrap(), 0);
    for epoch in 0..4 {
        journal.advance(observation(epoch)).unwrap();
    }
    assert!(matches!(
        journal.verify_history(3),
        Err(JournalError::AuditLimit)
    ));
    assert_eq!(journal.verify_history(4).unwrap(), 4);
    // A well-formed record must still reproduce the controller's amount.
    let mut record = journal.record(0).unwrap().unwrap();
    record.emission_udrt += 1;
    journal
        .storage
        .db
        .put(event_key(0), record.encode(&[7; 32]))
        .unwrap();
    assert!(matches!(
        journal.verify_history(4),
        Err(JournalError::InvalidRecord)
    ));
}

#[test]
fn history_audit_rejects_missing_and_unexpected_records() {
    let dir = tempfile::tempdir().unwrap();
    let mut storage = Storage::open(dir.path().join("db")).unwrap();
    let mut journal = AdaptiveJournal::initialize(&mut storage, [7; 32], config()).unwrap();
    for epoch in 0..3 {
        journal.advance(observation(epoch)).unwrap();
    }
    let first = journal.storage.db.get(event_key(0)).unwrap().unwrap();
    journal.storage.db.delete(event_key(0)).unwrap();
    assert!(matches!(
        journal.verify_history(3),
        Err(JournalError::InvalidRecord)
    ));
    journal.storage.db.put(event_key(0), &first).unwrap();
    journal.storage.db.put(event_key(99), &first).unwrap();
    assert!(matches!(
        journal.verify_history(3),
        Err(JournalError::InvalidRecord)
    ));
}

#[test]
fn invalid_observations_and_record_conflicts_do_not_write() {
    let dir = tempfile::tempdir().unwrap();
    let mut storage = Storage::open(dir.path().join("db")).unwrap();
    let mut journal = AdaptiveJournal::initialize(&mut storage, [7; 32], config()).unwrap();
    journal.advance(observation(0)).unwrap();
    let before = journal.storage.db.get(HEAD_KEY).unwrap().unwrap();
    for input in [
        observation(0),
        observation(2),
        Observation {
            utilization_ppm: 1000001,
            ..observation(1)
        },
    ] {
        assert!(journal.advance(input).is_err());
        assert_eq!(journal.storage.db.get(HEAD_KEY).unwrap().unwrap(), before);
        assert!(journal.record(1).unwrap().is_none());
    }
    journal
        .storage
        .db
        .put(event_key(1), b"conflicting record")
        .unwrap();
    assert!(matches!(
        journal.advance(observation(1)),
        Err(JournalError::RecordConflict)
    ));
    assert_eq!(journal.storage.db.get(HEAD_KEY).unwrap().unwrap(), before);
    assert_eq!(
        journal.storage.db.get(event_key(1)).unwrap().unwrap(),
        b"conflicting record"
    );
}

#[test]
fn commit_failure_before_write_leaves_both_records_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    {
        let mut storage = Storage::open(path.clone()).unwrap();
        let mut journal = AdaptiveJournal::initialize(&mut storage, [7; 32], config()).unwrap();
        let before = journal.storage.db.get(HEAD_KEY).unwrap().unwrap();
        let result =
            journal.advance_with_commit(observation(0), |_, _| Err(JournalError::InvalidRecord));
        assert!(result.is_err());
        assert_eq!(journal.storage.db.get(HEAD_KEY).unwrap().unwrap(), before);
        assert!(journal.record(0).unwrap().is_none());
    }
    let mut storage = Storage::open(path).unwrap();
    let mut journal = AdaptiveJournal::open(&mut storage, [7; 32], config()).unwrap();
    assert_eq!(journal.snapshot().unwrap().last_epoch, None);
    journal.advance(observation(0)).unwrap();
}

#[test]
fn lost_commit_acknowledgement_recovers_without_replaying_transition() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    {
        let mut storage = Storage::open(path.clone()).unwrap();
        let mut journal = AdaptiveJournal::initialize(&mut storage, [7; 32], config()).unwrap();
        let result = journal.advance_with_commit(observation(0), |db, batch| {
            write_sync(db, batch)?;
            Err(JournalError::InvalidRecord)
        });
        assert!(result.is_err());
    }
    let mut storage = Storage::open(path).unwrap();
    let mut journal = AdaptiveJournal::open(&mut storage, [7; 32], config()).unwrap();
    let before = journal.storage.db.get(HEAD_KEY).unwrap().unwrap();
    assert_eq!(journal.snapshot().unwrap().last_epoch, Some(0));
    assert!(journal.record(0).unwrap().is_some());
    assert!(matches!(
        journal.advance(observation(0)),
        Err(JournalError::Controller(
            dytallix_adaptive_emission::Error::UnexpectedEpoch
        ))
    ));
    assert_eq!(journal.storage.db.get(HEAD_KEY).unwrap().unwrap(), before);
    journal.advance(observation(1)).unwrap();
}

#[test]
fn missing_or_mismatched_state_never_initializes_implicitly() {
    let dir = tempfile::tempdir().unwrap();
    let mut storage = Storage::open(dir.path().join("db")).unwrap();
    assert!(matches!(
        AdaptiveJournal::open(&mut storage, [7; 32], config()),
        Err(JournalError::MissingCheckpoint)
    ));
    AdaptiveJournal::initialize(&mut storage, [7; 32], config()).unwrap();
    let before = storage.db.get(HEAD_KEY).unwrap().unwrap();
    assert!(matches!(
        AdaptiveJournal::initialize(&mut storage, [7; 32], config()),
        Err(JournalError::AlreadyInitialized)
    ));
    assert!(matches!(
        AdaptiveJournal::open(&mut storage, [8; 32], config()),
        Err(JournalError::BindingMismatch)
    ));
    let mut other = config();
    other.target_ppm += 1;
    assert!(matches!(
        AdaptiveJournal::open(&mut storage, [7; 32], other),
        Err(JournalError::ConfigMismatch)
    ));
    assert_eq!(storage.db.get(HEAD_KEY).unwrap().unwrap(), before);
}

#[test]
fn damaged_checkpoint_and_incomplete_tail_stop_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let mut storage = Storage::open(dir.path().join("db")).unwrap();
    {
        let mut journal = AdaptiveJournal::initialize(&mut storage, [7; 32], config()).unwrap();
        journal.advance(observation(0)).unwrap();
    }
    let valid_head = storage.db.get(HEAD_KEY).unwrap().unwrap();
    let mut damaged = valid_head.clone();
    damaged[45] ^= 1;
    storage.db.put(HEAD_KEY, &damaged).unwrap();
    assert!(matches!(
        AdaptiveJournal::open(&mut storage, [7; 32], config()),
        Err(JournalError::InvalidCheckpoint)
    ));
    assert_eq!(storage.db.get(HEAD_KEY).unwrap().unwrap(), damaged);
    storage.db.put(HEAD_KEY, &valid_head).unwrap();
    storage.db.delete(event_key(0)).unwrap();
    assert!(matches!(
        AdaptiveJournal::open(&mut storage, [7; 32], config()),
        Err(JournalError::InvalidRecord)
    ));
}

#[test]
fn orphaned_records_prevent_reset_and_damaged_records_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let mut storage = Storage::open(dir.path().join("db")).unwrap();
    {
        let mut journal = AdaptiveJournal::initialize(&mut storage, [7; 32], config()).unwrap();
        journal.advance(observation(0)).unwrap();
    }
    let record = storage.db.get(event_key(0)).unwrap().unwrap();
    for length in 0..record.len() {
        assert!(RecordedCommand::decode(&record[..length], &[7; 32]).is_err());
    }
    let mut bad = record.clone();
    bad.push(0);
    assert!(RecordedCommand::decode(&bad, &[7; 32]).is_err());
    let mut bad = record;
    bad[72] ^= 1;
    storage.db.put(event_key(0), bad).unwrap();
    assert!(AdaptiveJournal::open(&mut storage, [7; 32], config()).is_err());
    storage.db.delete(HEAD_KEY).unwrap();
    assert!(matches!(
        AdaptiveJournal::initialize(&mut storage, [7; 32], config()),
        Err(JournalError::AlreadyInitialized)
    ));
}

fn stored_bytes(storage: &Storage) -> Vec<(Vec<u8>, Vec<u8>)> {
    storage
        .db
        .iterator(IteratorMode::Start)
        .map(|entry| {
            let (key, value) = entry.unwrap();
            (key.to_vec(), value.to_vec())
        })
        .collect()
}

#[test]
fn prepared_updates_do_not_write_and_match_legacy_canonical_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let prepared_store = Storage::open(dir.path().join("prepared")).unwrap();
    let mut legacy_store = Storage::open(dir.path().join("legacy")).unwrap();
    let initial = prepare_initialize(&prepared_store, [7; 32], config()).unwrap();
    assert!(initial.command().is_none());
    assert!(initial.expected_head().is_none());
    assert_eq!(initial.writes().len(), 1);
    assert!(stored_bytes(&prepared_store).is_empty());
    {
        let guard = prepared_store.lock_execution().unwrap();
        let mut batch = WriteBatch::default();
        initial
            .append_checked(&prepared_store, &guard, &mut batch)
            .unwrap();
        // Appending a plan still changes no database bytes.
        assert!(stored_bytes(&prepared_store).is_empty());
        write_sync(&prepared_store.db, batch).unwrap();
    }
    let mut legacy = AdaptiveJournal::initialize(&mut legacy_store, [7; 32], config()).unwrap();
    assert_eq!(stored_bytes(&prepared_store), stored_bytes(legacy.storage));
    for epoch in 0..4 {
        let before = stored_bytes(&prepared_store);
        let plan =
            prepare_transition(&prepared_store, [7; 32], config(), observation(epoch)).unwrap();
        assert_eq!(
            plan.expected_head(),
            prepared_store.db.get(HEAD_KEY).unwrap().as_deref()
        );
        assert_eq!(plan.writes().len(), 2);
        assert_eq!(stored_bytes(&prepared_store), before);
        let command = plan.command().unwrap().clone();
        {
            let guard = prepared_store.lock_execution().unwrap();
            let mut discarded = WriteBatch::default();
            plan.append_checked(&prepared_store, &guard, &mut discarded)
                .unwrap();
            drop(discarded); // Simulated outer planner failure before the combined write.
            assert_eq!(stored_bytes(&prepared_store), before);
            let mut batch = WriteBatch::default();
            plan.append_checked(&prepared_store, &guard, &mut batch)
                .unwrap();
            write_sync(&prepared_store.db, batch).unwrap();
        }
        assert_eq!(command, legacy.advance(observation(epoch)).unwrap());
        assert_eq!(stored_bytes(&prepared_store), stored_bytes(legacy.storage));
    }
}

#[test]
fn stale_plan_does_not_append_to_an_existing_unrelated_batch() {
    let dir = tempfile::tempdir().unwrap();
    let mut storage = Storage::open(dir.path().join("db")).unwrap();
    AdaptiveJournal::initialize(&mut storage, [7; 32], config()).unwrap();
    let plan = prepare_transition(&storage, [7; 32], config(), observation(0)).unwrap();
    AdaptiveJournal::open(&mut storage, [7; 32], config())
        .unwrap()
        .advance(observation(0))
        .unwrap();
    let before = stored_bytes(&storage);
    let guard = storage.lock_execution().unwrap();
    let mut batch = WriteBatch::default();
    batch.put(b"unrelated", b"preserved");
    assert!(matches!(
        plan.append_checked(&storage, &guard, &mut batch),
        Err(JournalError::StalePreparation)
    ));
    assert_eq!(batch.len(), 1);
    write_sync(&storage.db, batch).unwrap();
    let mut expected = before;
    expected.push((b"unrelated".to_vec(), b"preserved".to_vec()));
    expected.sort();
    assert_eq!(stored_bytes(&storage), expected);
}

#[test]
fn prepared_initialization_rechecks_orphan_events_without_partial_append() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("db")).unwrap();
    let plan = prepare_initialize(&storage, [7; 32], config()).unwrap();
    let guard = storage.lock_execution().unwrap();
    storage.db.put(event_key(0), b"orphan").unwrap();
    let mut batch = WriteBatch::default();
    batch.put(b"unrelated", b"preserved");
    assert!(matches!(
        plan.append_checked(&storage, &guard, &mut batch),
        Err(JournalError::AlreadyInitialized)
    ));
    assert_eq!(batch.len(), 1);
    assert!(storage.db.get(HEAD_KEY).unwrap().is_none());
}

#[test]
fn prepared_transition_rechecks_previous_and_conflicting_event_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let mut storage = Storage::open(dir.path().join("db")).unwrap();
    AdaptiveJournal::initialize(&mut storage, [7; 32], config())
        .unwrap()
        .advance(observation(0))
        .unwrap();
    let plan = prepare_transition(&storage, [7; 32], config(), observation(1)).unwrap();
    let previous = storage.db.get(event_key(0)).unwrap().unwrap();
    let guard = storage.lock_execution().unwrap();
    let mut batch = WriteBatch::default();
    batch.put(b"unrelated", b"preserved");
    storage
        .db
        .put(event_key(0), b"changed predecessor")
        .unwrap();
    assert!(matches!(
        plan.append_checked(&storage, &guard, &mut batch),
        Err(JournalError::StalePreparation)
    ));
    assert_eq!(batch.len(), 1);
    storage.db.put(event_key(0), previous).unwrap();
    storage.db.put(event_key(1), b"conflict").unwrap();
    assert!(matches!(
        plan.append_checked(&storage, &guard, &mut batch),
        Err(JournalError::RecordConflict)
    ));
    assert_eq!(batch.len(), 1);
}

#[test]
fn legacy_initialization_and_advance_require_the_execution_lock() {
    let dir = tempfile::tempdir().unwrap();
    let mut initial = Storage::open(dir.path().join("initial")).unwrap();
    let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = initial.lock_execution().unwrap();
        panic!("Poison initialization execution lock");
    }));
    assert!(poisoned.is_err());
    assert!(matches!(
        AdaptiveJournal::initialize(&mut initial, [7; 32], config()),
        Err(JournalError::ExecutionLock)
    ));
    assert!(stored_bytes(&initial).is_empty());
    let mut storage = Storage::open(dir.path().join("advance")).unwrap();
    let mut journal = AdaptiveJournal::initialize(&mut storage, [7; 32], config()).unwrap();
    let before = stored_bytes(journal.storage);
    let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = journal.storage.lock_execution().unwrap();
        panic!("Poison transition execution lock");
    }));
    assert!(poisoned.is_err());
    assert!(matches!(
        journal.advance(observation(0)),
        Err(JournalError::ExecutionLock)
    ));
    assert_eq!(stored_bytes(journal.storage), before);
}

fn synthetic_view(epochs: u64) -> BTreeMap<Vec<u8>, Vec<u8>> {
    let mut controller = Controller::new(config()).unwrap();
    let (head, mut before_digest) = encode_head(&[7; 32], &controller);
    let mut values = BTreeMap::from([(HEAD_KEY.to_vec(), head)]);
    for epoch in 0..epochs {
        let input = observation(epoch);
        let command = controller.step(input).unwrap();
        let (head, after_digest) = encode_head(&[7; 32], &controller);
        let record = RecordedCommand {
            observation: input,
            for_epoch: command.for_epoch,
            emission_udrt: command.emission_udrt,
            before_digest,
            after_digest,
        };
        values.insert(event_key(epoch), record.encode(&[7; 32]));
        values.insert(HEAD_KEY.to_vec(), head);
        before_digest = after_digest;
    }
    values
}

#[test]
fn verified_view_replays_empty_and_multiple_epochs_and_ignores_unrelated_keys() {
    for epochs in [0, 1, 7] {
        let mut values = synthetic_view(epochs);
        values.insert(
            b"issuance:v1:state".to_vec(),
            b"unrelated issuance view".to_vec(),
        );
        values.insert(
            b"acct:balances:owner".to_vec(),
            b"unrelated account view".to_vec(),
        );
        let before = values.clone();
        let verified = verify_view(&values, [7; 32], &config(), epochs).unwrap();
        assert_eq!(verified.commands.len() as u64, epochs);
        assert_eq!(verified.snapshot.last_epoch, epochs.checked_sub(1));
        let mut reference = Controller::new(config()).unwrap();
        for (epoch, record) in verified.commands.iter().enumerate() {
            let expected = reference.step(observation(epoch as u64)).unwrap();
            assert_eq!(record.observation, observation(epoch as u64));
            assert_eq!(record.for_epoch, expected.for_epoch);
            assert_eq!(record.emission_udrt, expected.emission_udrt);
        }
        assert_eq!(verified.snapshot, reference.snapshot());
        assert_eq!(values, before);
    }
}

#[test]
fn verified_view_rejects_checkpoint_binding_config_checksum_and_replay_corruption() {
    let values = synthetic_view(2);
    assert!(matches!(
        verify_view(&values, [8; 32], &config(), 2),
        Err(JournalError::BindingMismatch)
    ));
    let mut wrong_config = config();
    wrong_config.base_udrt += 1;
    assert!(matches!(
        verify_view(&values, [7; 32], &wrong_config, 2),
        Err(JournalError::ConfigMismatch)
    ));
    let mut corrupt = values.clone();
    corrupt.get_mut(HEAD_KEY).unwrap()[0] ^= 1;
    assert!(matches!(
        verify_view(&corrupt, [7; 32], &config(), 2),
        Err(JournalError::InvalidCheckpoint)
    ));
    let mut corrupt = values.clone();
    corrupt.get_mut(&event_key(0)).unwrap()[72] ^= 1;
    assert!(matches!(
        verify_view(&corrupt, [7; 32], &config(), 2),
        Err(JournalError::InvalidRecord)
    ));
    // Recompute the event checksum: valid wire bytes still must replay exactly.
    let mut corrupt = values.clone();
    let mut event = RecordedCommand::decode(&corrupt[&event_key(0)], &[7; 32]).unwrap();
    event.emission_udrt += 1;
    corrupt.insert(event_key(0), event.encode(&[7; 32]));
    assert!(matches!(
        verify_view(&corrupt, [7; 32], &config(), 2),
        Err(JournalError::InvalidRecord)
    ));
    let mut corrupt = values;
    corrupt.remove(HEAD_KEY);
    assert!(matches!(
        verify_view(&corrupt, [7; 32], &config(), 2),
        Err(JournalError::MissingCheckpoint)
    ));
}

#[test]
fn verified_view_rejects_unknown_malformed_extra_and_skipped_adaptive_keys() {
    let values = synthetic_view(3);
    for key in [
        b"adaptive:v2:head".to_vec(),
        b"adaptive:v1:other".to_vec(),
        EVENT_PREFIX.to_vec(),
    ] {
        let mut invalid = values.clone();
        invalid.insert(key, b"unknown".to_vec());
        assert!(matches!(
            verify_view(&invalid, [7; 32], &config(), 10),
            Err(JournalError::InvalidRecord)
        ));
    }
    let mut extra = values.clone();
    extra.insert(event_key(9), values[&event_key(2)].clone());
    assert!(matches!(
        verify_view(&extra, [7; 32], &config(), 10),
        Err(JournalError::InvalidRecord)
    ));
    let mut gap = values.clone();
    let event = gap.remove(&event_key(1)).unwrap();
    gap.insert(event_key(9), event);
    assert!(matches!(
        verify_view(&gap, [7; 32], &config(), 10),
        Err(JournalError::InvalidRecord)
    ));
    let mut missing = values;
    missing.remove(&event_key(1));
    assert!(matches!(
        verify_view(&missing, [7; 32], &config(), 10),
        Err(JournalError::InvalidRecord)
    ));
}

#[test]
fn verified_view_enforces_count_bounds_before_decoding_commands() {
    let mut values = synthetic_view(3);
    values.insert(event_key(0), b"invalid command bytes".to_vec());
    assert!(matches!(
        verify_view(&values, [7; 32], &config(), 2),
        Err(JournalError::AuditLimit)
    ));
    // A checkpoint that claims excess history also rejects before replay, even
    // when the supplied event view is incomplete.
    values.retain(|key, _| key.as_slice() == HEAD_KEY);
    assert!(matches!(
        verify_view(&values, [7; 32], &config(), 2),
        Err(JournalError::AuditLimit)
    ));
    assert!(verify_view(&synthetic_view(0), [7; 32], &config(), 0).is_ok());
}

#[test]
fn verified_view_does_not_change_database_or_caller_values() {
    let dir = tempfile::tempdir().unwrap();
    let mut storage = Storage::open(dir.path().join("db")).unwrap();
    {
        let mut journal = AdaptiveJournal::initialize(&mut storage, [7; 32], config()).unwrap();
        for epoch in 0..3 {
            journal.advance(observation(epoch)).unwrap();
        }
    }
    storage.db.put(b"unrelated", b"unchanged").unwrap();
    let before = stored_bytes(&storage);
    let values: BTreeMap<_, _> = before.iter().cloned().collect();
    let verified = verify_view(&values, [7; 32], &config(), 3).unwrap();
    assert_eq!(verified.commands.len(), 3);
    assert_eq!(stored_bytes(&storage), before);
    assert_eq!(values.into_iter().collect::<Vec<_>>(), before);
    AdaptiveJournal::open(&mut storage, [7; 32], config())
        .unwrap()
        .verify_history(3)
        .unwrap();
}
