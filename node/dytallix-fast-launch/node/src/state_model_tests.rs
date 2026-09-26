//! State model v2, phase A: per-call verification and state reads do not grow
//! with block history. These count work; they are not wall-clock benchmarks.
use super::*;

fn full_passes() -> usize {
    FULL_HISTORY_PASSES.with(|n| n.get())
}

fn digest_reads() -> usize {
    STATE_DIGEST_READS.with(|n| n.get())
}

/// The pre-v2 digest: a scan of every stored key filtered by `selected`.
fn full_scan_state_digest(storage: &Storage, writes: &Writes, governance_enabled: bool) -> String {
    let mut values = BTreeMap::new();
    for item in storage.db.iterator(IteratorMode::Start) {
        let (k, v) = item.unwrap();
        if selected(&k, governance_enabled) {
            values.insert(k.to_vec(), v.to_vec());
        }
    }
    for (k, v) in writes {
        if selected(k, governance_enabled) {
            values.insert(k.clone(), v.clone());
        }
    }
    digest(
        b"dytallix-cometbft-state-v1",
        &values.iter().collect::<Vec<_>>(),
    )
    .unwrap()
}

/// The Inputs fixture uses two-block epochs and at most eight recorded epochs,
/// so its chains end at height 18.
const EPOCH_BLOCKS: u64 = 2;

/// Epoch boundaries require the completed parent epoch's observation first.
fn commit_next(app: &mut ConsensusApplication, height: u64) {
    let mut txs = Vec::new();
    if height > 1 && (height - 1) % EPOCH_BLOCKS == 0 {
        let epoch = (height - 1) / EPOCH_BLOCKS - 1;
        txs.push(
            serde_json::to_vec(&WireTransaction::EpochObservation {
                observation: EpochObservation {
                    epoch,
                    utilization_ppm: 500000,
                    volatility_ppm: 0,
                    first_height: epoch * EPOCH_BLOCKS + 1,
                    last_height: (epoch + 1) * EPOCH_BLOCKS,
                    parent_hash: block(height - 1, vec![]).hash,
                },
            })
            .unwrap(),
        );
    }
    app.finalize_block(block(height, txs)).unwrap();
    app.commit().unwrap();
}

fn committed_chain(inputs: &Inputs, path: &std::path::Path, height: u64) -> ConsensusApplication {
    let mut app = inputs.initialized(path);
    app.finalize_block(block(1, vec![signed_wire(inputs.send())]))
        .unwrap();
    app.commit().unwrap();
    for h in 2..=height {
        commit_next(&mut app, h);
    }
    app
}

#[test]
fn prefix_state_digest_matches_full_scan_digest() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let app = committed_chain(&inputs, &dir.path().join("db"), 4);
    let mut writes = Writes::new();
    writes.insert(b"acct:extra".to_vec(), b"1".to_vec());
    writes.insert(b"tx:outside-state".to_vec(), b"2".to_vec());
    writes.insert(MODE_KEY.as_bytes().to_vec(), b"3".to_vec());
    for writes in [Writes::new(), writes] {
        assert_eq!(
            state_digest(&app.storage, &writes, false).unwrap(),
            full_scan_state_digest(&app.storage, &writes, false)
        );
    }
}

#[test]
fn state_digest_reads_only_state_keys() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let app = committed_chain(&inputs, &dir.path().join("db"), 16);
    let mut all_keys = 0;
    let mut prefixed_state_keys = 0;
    for item in app.storage.db.iterator(IteratorMode::Start) {
        let (k, _) = item.unwrap();
        all_keys += 1;
        if STATE_PREFIXES.iter().any(|prefix| k.starts_with(prefix)) {
            prefixed_state_keys += 1;
        }
    }
    let before = digest_reads();
    state_digest(&app.storage, &Writes::new(), false).unwrap();
    assert_eq!(digest_reads() - before, prefixed_state_keys);
    // Block records alone exceed the state keys this digest reads.
    assert!(all_keys >= prefixed_state_keys + 16);
}

#[test]
fn committed_operation_runs_no_complete_history_check_after_startup() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = committed_chain(&inputs, &dir.path().join("db"), 1);
    let before = full_passes();
    let _ = app.check_tx(&signed_wire(inputs.send()));
    for height in 2..=12 {
        commit_next(&mut app, height);
    }
    app.info().unwrap();
    app.query().unwrap();
    assert_eq!(full_passes() - before, 0);

    // A write outside commit forces one complete check, which refuses corruption.
    let saved = app.storage.db.get(record_key(1)).unwrap().unwrap();
    app.storage.db.put(record_key(1), b"{}").unwrap();
    let before = full_passes();
    assert!(app.info().is_err());
    assert_eq!(full_passes() - before, 1);
    app.storage.db.put(record_key(1), saved).unwrap();
    app.info().unwrap();
    app.info().unwrap();
    assert_eq!(full_passes() - before, 2);
}

#[test]
fn reopened_storage_repeats_the_complete_check() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    drop(committed_chain(&inputs, &path, 3));
    let before = full_passes();
    let app = inputs.open(&path);
    app.info().unwrap();
    assert!(full_passes() - before >= 1);
}

#[test]
fn block_inputs_are_not_capped_at_the_former_history_bound() {
    let inputs = Inputs::new();
    // The block helper derives time as height * 10 seconds, so stay within i64.
    for height in [100_000u64, 100_001, 10_000_000, 1_000_000_000_000] {
        input_limits(&inputs.config, &block(height, vec![])).unwrap();
    }
    assert!(input_limits(&inputs.config, &block(0, vec![])).is_err());
}

#[test]
fn supply_validation_reads_only_the_current_emission_event() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let app = committed_chain(&inputs, &dir.path().join("db"), 8);
    for height in 1..8u64 {
        let key = format!("emission:event:{height}");
        assert!(app.storage.db.get(&key).unwrap().is_some());
        app.storage.db.delete(key).unwrap();
    }
    crate::supply::validate_native(&app.storage, &Writes::new()).unwrap();
}
