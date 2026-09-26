//! Counts actual canonical block decodes. These are not wall-clock benchmarks.
use super::*;

#[test]
fn unconfigured_governance_records_cannot_escape_application_commitment() {
    let f = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let app = f.initialized(dir.path());
    verify_recovery(&app.storage).unwrap();
    for key in [
        b"gov:proposal:1".as_slice(),
        b"governance:v1:ballot:00000000000000000001",
    ] {
        app.storage.db.put(key, b"unapproved").unwrap();
        assert!(state_digest(&app.storage, &Writes::new(), false).is_err());
        assert!(verify_recovery(&app.storage).is_err());
        assert!(app.info().is_err());
        app.storage.db.delete(key).unwrap();
    }
    let mut writes = Writes::new();
    writes.insert(
        b"governance:v1:ballot:00000000000000000001".to_vec(),
        b"unapproved".to_vec(),
    );
    assert!(state_digest(&app.storage, &writes, false).is_err());
    verify_recovery(&app.storage).unwrap();
}

#[test]
fn configured_digest_selects_only_the_governance_state_key() {
    let f = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let app = f.initialized(dir.path());
    let base = state_digest(&app.storage, &Writes::new(), false).unwrap();
    let genesis_digest = hex::decode(&app.config.app_state_sha256)
        .unwrap()
        .try_into()
        .unwrap();
    let governance = GovernanceState::new(
        GOVERNANCE_STATE_VERSION,
        app.config.chain_id.clone(),
        genesis_digest,
        0,
    )
    .unwrap();
    let mut writes = Writes::new();
    writes.insert(GOVERNANCE_STATE_KEY.as_bytes().to_vec(), governance.encode().unwrap());
    assert!(state_digest(&app.storage, &writes, false).is_err());
    assert_ne!(state_digest(&app.storage, &writes, true).unwrap(), base);
    writes.insert(b"governance:v1:other".to_vec(), b"forbidden".to_vec());
    assert!(state_digest(&app.storage, &writes, true).is_err());
    writes.remove(b"governance:v1:other".as_slice());
    writes.insert(b"gov:proposal:1".to_vec(), b"forbidden".to_vec());
    assert!(state_digest(&app.storage, &writes, true).is_err());
    writes.remove(b"gov:proposal:1".as_slice());
    let expected = state_digest(&app.storage, &writes, true).unwrap();
    app.storage
        .db
        .put(GOVERNANCE_STATE_KEY, &writes[GOVERNANCE_STATE_KEY.as_bytes()])
        .unwrap();
    assert_eq!(
        state_digest(&app.storage, &Writes::new(), true).unwrap(),
        expected
    );
    assert!(verify_recovery(&app.storage).is_err());
    app.storage.db.delete(GOVERNANCE_STATE_KEY).unwrap();
}

#[test]
fn anchored_query_validates_once_and_preserves_values_and_corruption_refusal() {
    let f = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = f.initialized(dir.path());
    commit(&mut app, 1, vec![]);
    let before = RECOVERY_PASSES.with(|n| n.get());
    let info = app.info().unwrap();
    let expected = app.query().unwrap();
    assert_eq!(RECOVERY_PASSES.with(|n| n.get()) - before, 2);
    let before = RECOVERY_PASSES.with(|n| n.get());
    let actual = app
        .query_at(QueryRequest::Status, info.height as i64)
        .unwrap();
    assert_eq!(actual, (info.clone(), expected));
    assert_eq!(RECOVERY_PASSES.with(|n| n.get()) - before, 1);
    for request in [
        QueryRequest::OrdinaryProfile,
        QueryRequest::OrdinaryAccount(&hex::encode(f.active.id())),
        QueryRequest::OrdinaryReceipt(&"11".repeat(32)),
        QueryRequest::EmergencyReceipt(&"22".repeat(32)),
    ] {
        let expected = match request {
            QueryRequest::OrdinaryProfile => app.query_ordinary_profile().unwrap(),
            QueryRequest::OrdinaryAccount(id) => app.query_ordinary_account(id).unwrap(),
            QueryRequest::OrdinaryReceipt(id) => app.query_ordinary_receipt(id).unwrap(),
            QueryRequest::EmergencyReceipt(id) => app.query_emergency_receipt(id).unwrap(),
            _ => unreachable!(),
        };
        let before = RECOVERY_PASSES.with(|n| n.get());
        assert_eq!(app.query_at(request, 0).unwrap(), (info.clone(), expected));
        assert_eq!(RECOVERY_PASSES.with(|n| n.get()) - before, 1);
    }
    assert!(app.query_at(QueryRequest::Status, -1).is_err());
    assert!(app.query_at(QueryRequest::Status, 2).is_err());
    let saved = app.storage.db.get(record_key(1)).unwrap().unwrap();
    app.storage.db.put(record_key(1), b"{}").unwrap();
    assert!(app.query_at(QueryRequest::Status, 0).is_err());
    app.storage.db.put(record_key(1), saved).unwrap();
    assert_eq!(app.query_at(QueryRequest::Status, 0).unwrap().0, info);
}

#[test]
#[ignore = "requires explicit disposable SLH signing helper"]
fn bounded_history_cache_and_linear_trace_match_full_prefix_recovery() {
    let mut f = Fixture::new();
    let root = setup(&mut f);
    let dir = tempfile::tempdir().unwrap();
    let mut app = start(&root, &f, dir.path());
    let freeze = emergency_control(&root, &app, emergency::Action::Freeze);
    commit(&mut app, 1, vec![freeze]);
    let resume = emergency_control(&root, &app, emergency::Action::Resume);
    commit(&mut app, 2, vec![resume]);
    let admit = handover_control(
        &root,
        &app,
        handover::Action::Admit {
            plan: release_plan(),
        },
    );
    commit(&mut app, 3, vec![admit]);
    let state = handover_state(&app.storage, &app.config).unwrap().unwrap();
    let pending = state.pending().unwrap();
    let cancel = handover_control(
        &root,
        &app,
        handover::Action::Cancel {
            plan_sha256: pending.plan.sha256().unwrap(),
            admission_receipt_sha256: pending.admission_receipt_sha256.clone(),
        },
    );
    commit(&mut app, 4, vec![cancel]);
    let mut measurements = Vec::new();
    for height in [4u64, 8] {
        while app.info().unwrap().height < height {
            let next = app.info().unwrap().height + 1;
            commit(&mut app, next, vec![]);
        }
        let before = data(&app);
        let cached = HistoryRead::new(&app.storage);
        let cached_trace = verify_recovery_with(&cached).unwrap();
        let streaming = HistoryRead::with_budget(&app.storage, 0);
        let streaming_trace = verify_recovery_with(&streaming).unwrap();
        assert_eq!(cached_trace.records, streaming_trace.records);
        assert_eq!(cached_trace.states, streaming_trace.states);
        assert_eq!(cached.block_decodes.get(), height as usize);
        assert!(streaming.block_decodes.get() >= 4 * height as usize);
        assert_eq!(cached.emergency_steps.get(), 2);
        assert_eq!(streaming.emergency_steps.get(), 2);
        assert!(cached.cached_bytes.get() <= HISTORY_CACHE_BYTES);
        assert_eq!(streaming.cached_bytes.get(), 0);
        let policy = app.config.emergency.as_ref().unwrap();
        let mut prefix_steps = 0;
        for h in 1..=height {
            let end = cached_trace
                .records
                .partition_point(|(_, context)| context.height < h);
            let expected =
                emergency::recover_recorded(policy, &cached_trace.records[..end]).unwrap();
            prefix_steps += end;
            assert_eq!(expected, cached_trace.states[end]);
        }
        assert!(prefix_steps > cached.emergency_steps.get());
        for h in 0..=height {
            assert_eq!(recorded_release_at(&cached, &app.config, h).unwrap(), None);
        }
        assert_eq!(data(&app), before);
        measurements.push((
            height,
            cached.block_decodes.get(),
            streaming.block_decodes.get(),
            cached.emergency_steps.get(),
            prefix_steps,
        ));
    }
    assert_eq!(measurements[1].1, measurements[0].1 * 2);
    println!("history counts (height, cached decodes, streaming decodes, trace steps, repeated prefix steps): {measurements:?}");
    // A later operation has a fresh cache and must see a changed historical key.
    let key = record_key(2);
    let saved = app.storage.db.get(&key).unwrap().unwrap();
    app.storage.db.put(&key, b"{}").unwrap();
    assert!(verify_recovery_with(&HistoryRead::new(&app.storage)).is_err());
    assert!(verify_recovery_with(&HistoryRead::with_budget(&app.storage, 0)).is_err());
    app.storage.db.put(&key, saved).unwrap();
    verify_recovery(&app.storage).unwrap();
}
