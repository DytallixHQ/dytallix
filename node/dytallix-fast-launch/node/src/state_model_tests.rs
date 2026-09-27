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
    hex::encode(crate::state_tree::rebuilt_root(values).unwrap())
}

/// The Inputs fixture uses two-block epochs and at most eight recorded epochs,
/// so its chains end at height 18.
const EPOCH_BLOCKS: u64 = 2;

/// Epoch boundaries carry the observation derived from committed blocks first.
fn commit_next(app: &mut ConsensusApplication, height: u64) {
    let parent = block(height - 1, vec![]).hash;
    let txs = derived_observation_wire(&app.storage, &app.config, EPOCH_BLOCKS, height, &parent)
        .unwrap()
        .into_iter()
        .collect();
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
            reference_state_digest(&app.storage, &writes, false).unwrap(),
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
    reference_state_digest(&app.storage, &Writes::new(), false).unwrap();
    assert_eq!(digest_reads() - before, prefixed_state_keys);
    // Block records alone exceed the state keys this digest reads.
    assert!(all_keys >= prefixed_state_keys + 16);
}

/// Phase B: committing a block reads the state tree along its changed paths,
/// not the committed entries, so block cost does not grow with state.
#[test]
fn committed_blocks_read_no_state_entries_for_the_digest() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = committed_chain(&inputs, &dir.path().join("db"), 2);
    let before = digest_reads();
    for height in 3..=12 {
        commit_next(&mut app, height);
    }
    assert_eq!(digest_reads() - before, 0);
    // The committed root still equals a rebuild from every entry.
    let head = read_head(&app.storage).unwrap().unwrap();
    assert_eq!(
        head.state_digest,
        reference_state_digest(&app.storage, &Writes::new(), false).unwrap()
    );
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
    // Committed state keeps only the parent's and the current event.
    for height in 1..=8u64 {
        let key = format!("emission:event:{height}");
        assert_eq!(app.storage.db.get(&key).unwrap().is_some(), height >= 7);
    }
    app.storage.db.delete("emission:event:7").unwrap();
    crate::supply::validate_native(&app.storage, &Writes::new()).unwrap();
}

#[test]
fn staged_deletions_match_physically_deleted_state() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let app = committed_chain(&inputs, &dir.path().join("db"), 4);
    let deleted: Deletes = ["emission:event:1", "emission:event:2"]
        .iter()
        .map(|k| k.as_bytes().to_vec())
        .collect();
    let staged = reference_state_digest_with(&app.storage, &Writes::new(), &deleted, false).unwrap();
    for key in &deleted {
        app.storage.db.delete(key).unwrap();
    }
    assert_eq!(staged, reference_state_digest(&app.storage, &Writes::new(), false).unwrap());
}

#[test]
fn a_block_cannot_write_and_delete_the_same_key() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let app = committed_chain(&inputs, &dir.path().join("db"), 1);
    let key = b"acct:overlap".to_vec();
    let writes = Writes::from([(key.clone(), b"1".to_vec())]);
    let deletes = Deletes::from([key]);
    assert!(reference_state_digest_with(&app.storage, &writes, &deletes, false).is_err());
}

#[test]
fn commit_applies_staged_deletions_atomically() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = committed_chain(&inputs, &dir.path().join("db"), 1);
    // A key outside the state commitment, so deleting it keeps the head valid.
    app.storage.db.put(b"scratch:b1a", b"x").unwrap();
    app.finalize_block(block(2, vec![])).unwrap();
    app.pending
        .as_mut()
        .unwrap()
        .deletes
        .insert(b"scratch:b1a".to_vec());
    app.commit().unwrap();
    assert!(app.storage.db.get(b"scratch:b1a").unwrap().is_none());
    assert_eq!(app.info().unwrap().height, 2);
}

/// The issuance journal keeps a window of `max_recorded_epochs` (8 here)
/// instead of stopping the chain (P01, 27 September 2026).
#[test]
fn issuance_runs_past_the_journal_window_and_keeps_only_the_window() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let app = committed_chain(&inputs, &dir.path().join("db"), 40);
    let count = |prefix: &[u8]| {
        app.storage
            .db
            .iterator(IteratorMode::From(prefix, Direction::Forward))
            .take_while(|e| e.as_ref().unwrap().0.starts_with(prefix))
            .count()
    };
    let state = timing(&app);
    assert_eq!(state.active_epoch, 19);
    assert_eq!(count(b"adaptive:v1:event:"), 8);
    assert_eq!(count(b"issuance:v1:observation:"), 8);
    assert!(app.storage.db.get(b"adaptive:v1:base").unwrap().is_some());
    assert!(state.pruned_issued.total().unwrap() > 0);
    crate::runtime::issuance_timing::verify_stored(&app.storage).unwrap();
    drop(app);
    // A restart replays the window from its checkpoint in the complete check.
    let reopened = inputs.open(&dir.path().join("db"));
    verify_recovery(&reopened.storage).unwrap();
    assert_eq!(timing(&reopened), state);
}

/// Heights of the block records the node holds.
fn held(app: &ConsensusApplication) -> Vec<u64> {
    app.storage
        .db
        .iterator(IteratorMode::From(BLOCK_PREFIX.as_bytes(), Direction::Forward))
        .map(|item| item.unwrap().0)
        .take_while(|key| key.starts_with(BLOCK_PREFIX.as_bytes()))
        .map(|key| height_of_record_key(&key).unwrap())
        .collect()
}

/// State sync v1, rule 2: a node keeps the retained window (two epochs of two
/// blocks here), block 1 because it holds a legacy transaction, and the
/// parent's and current emission events; it tells the engine the window
/// start and restarts from the window.
#[test]
fn a_pruned_node_keeps_the_window_and_restarts_from_it() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let app = committed_chain(&inputs, &path, 40);
    assert_eq!(held(&app), [1, 37, 38, 39, 40]);
    assert_eq!(retained_from(&app.storage).unwrap(), 37);
    assert_eq!(app.retain_height().unwrap(), 37);
    let events = app
        .storage
        .db
        .iterator(IteratorMode::From(b"emission:event:", Direction::Forward))
        .take_while(|e| e.as_ref().unwrap().0.starts_with(b"emission:event:"))
        .count();
    assert_eq!(events, 2);
    drop(app);
    let mut app = inputs.open(&path);
    verify_recovery(&app.storage).unwrap();
    commit_next(&mut app, 41);
    assert_eq!(held(&app), [1, 38, 39, 40, 41]);
}

/// Rule 1: the startup check decodes the same block records at any height:
/// the window and the pinned records, not the chain from genesis.
#[test]
fn startup_check_work_does_not_grow_with_height() {
    let inputs = Inputs::new();
    let decodes = |height| {
        let dir = tempfile::tempdir().unwrap();
        let app = committed_chain(&inputs, &dir.path().join("db"), height);
        let history = HistoryRead::new(&app.storage);
        let before = full_passes();
        verify_recovery_with(&history).unwrap();
        assert_eq!(full_passes() - before, 1);
        history.block_decodes.get()
    };
    let short = decodes(20);
    assert_eq!(short, decodes(60));
    assert!(short <= 5, "decoded {short} block records");
}

/// Rule 2: an archive node keeps every record and tells the engine to keep
/// every block, with the same committed state as a pruned node.
#[test]
fn an_archive_node_keeps_every_record_with_the_same_state() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("archive");
    // ML-DSA signatures are randomized: both chains carry the same bytes.
    let send = signed_wire(inputs.send());
    let chain = |path: &std::path::Path, history| {
        let mut app = inputs.initialized(path).with_block_history(history);
        app.finalize_block(block(1, vec![send.clone()])).unwrap();
        app.commit().unwrap();
        for height in 2..=12 {
            commit_next(&mut app, height);
        }
        app
    };
    let app = chain(&path, BlockHistory::Archive);
    assert_eq!(held(&app), (1..=12).collect::<Vec<_>>());
    assert_eq!(app.retain_height().unwrap(), 0);
    let pruned = chain(&dir.path().join("pruned"), BlockHistory::Window);
    assert_eq!(held(&pruned), [1, 9, 10, 11, 12]);
    assert_eq!(app.info().unwrap(), pruned.info().unwrap());
    drop(app);
    verify_recovery(&inputs.open(&path).storage).unwrap();
}

/// Rule 1: the startup check refuses a gap in the window, a window start that
/// leaves an unpinned record below it, and one below the records held.
#[test]
fn startup_check_refuses_a_gap_or_a_moved_window_start() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let app = committed_chain(&inputs, &dir.path().join("db"), 20);
    assert_eq!(held(&app), [1, 17, 18, 19, 20]);
    let check = || verify_recovery_with(&HistoryRead::new(&app.storage)).map(|_| ());
    check().unwrap();
    let db = &app.storage.db;
    let record = db.get(record_key(18)).unwrap().unwrap();
    db.delete(record_key(18)).unwrap();
    assert!(check().is_err());
    db.put(record_key(18), &record).unwrap();
    db.put(RETAINED_KEY, 18u64.to_be_bytes()).unwrap();
    let error = check().unwrap_err().to_string();
    assert!(error.contains("not pinned"), "{error}");
    db.put(RETAINED_KEY, 16u64.to_be_bytes()).unwrap();
    assert!(check().is_err());
    db.put(RETAINED_KEY, 17u64.to_be_bytes()).unwrap();
    check().unwrap();
}

/// Rule 2: a record a replay reads outlives the window: one with a recorded
/// control or accepted legacy transactions, or a named emergency anchor. A
/// control that a freeze refused does not pin its block.
#[test]
fn records_that_replays_read_are_pinned() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let app = committed_chain(&inputs, &dir.path().join("db"), 4);
    let record_at = |height| -> BlockRecord {
        decode(&app.storage.db.get(record_key(height)).unwrap().unwrap()).unwrap()
    };
    let none = BTreeSet::new();
    assert!(pinned(1, &record_at(1), &none));
    let mut record = record_at(3);
    assert!(!pinned(3, &record, &none));
    assert!(pinned(3, &record, &BTreeSet::from([3])));
    for (result, kept) in [
        (emergency_result(), true),
        (upgrade_result(), true),
        (handover_result(), true),
        (TxResult::invalid(EMERGENCY_FROZEN), false),
    ] {
        record.result.tx_results[0] = result;
        assert_eq!(pinned(3, &record, &none), kept);
    }
}

fn snapshot_app(
    inputs: &Inputs,
    dir: &std::path::Path,
    interval: u64,
) -> ConsensusApplication {
    let mut app = inputs
        .initialized(&dir.join("db"))
        .with_snapshots(crate::snapshot::SnapshotConfig {
            dir: dir.join("snapshots"),
            interval,
            keep: 2,
        })
        .unwrap();
    app.finalize_block(block(1, vec![signed_wire(inputs.send())]))
        .unwrap();
    app.commit().unwrap();
    app
}

/// State sync v1, rule 3: at every interval the chain writes a snapshot of
/// its database as committed at that height, without the state tree's
/// records, and keeps the latest ones.
#[test]
fn committed_chain_writes_snapshots_of_its_committed_database() {
    use crate::snapshot::{chunk_path, decode_entries, snapshot_dir, Metadata, METADATA_FILE};
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = snapshot_app(&inputs, dir.path(), 4);
    let mut written = BTreeMap::new();
    for height in 2..=12 {
        commit_next(&mut app, height);
        if let Some(metadata) = app.wait_for_snapshot().unwrap() {
            written.insert(metadata.height, app.info().unwrap().app_hash);
        }
    }
    assert_eq!(written.keys().copied().collect::<Vec<_>>(), [4, 8, 12]);
    let root = dir.path().join("snapshots");
    assert_eq!(crate::snapshot::published(&root).unwrap(), [8, 12]);
    let read = |height| {
        let target = snapshot_dir(&root, height);
        let metadata =
            Metadata::decode(&std::fs::read(target.join(METADATA_FILE)).unwrap()).unwrap();
        let mut stream = Vec::new();
        for index in 0..metadata.chunks.len() {
            stream.extend(std::fs::read(chunk_path(&target, index)).unwrap());
        }
        (metadata, decode_entries(&stream).unwrap())
    };
    let (earlier, _) = read(8);
    assert_eq!((earlier.height, &earlier.app_hash), (8, &written[&8]));
    let (metadata, entries) = read(12);
    let head = read_head(&app.storage).unwrap().unwrap();
    assert_eq!(metadata.chain_id, app.config.chain_id);
    assert_eq!(
        (metadata.height, &metadata.app_hash, &metadata.state_digest),
        (12, &head.app_hash, &head.state_digest)
    );
    assert_eq!(metadata.retained_from, retained_from(&app.storage).unwrap());
    let expected: Vec<_> = app
        .storage
        .db
        .iterator(IteratorMode::Start)
        .map(|item| {
            let (key, value) = item.unwrap();
            (key.to_vec(), value.to_vec())
        })
        .filter(|(key, _)| !key.starts_with(crate::state_tree::PREFIX))
        .collect();
    assert_eq!(entries, expected);
    assert!(entries.iter().any(|(key, _)| key == HEAD_KEY.as_bytes()));
}

/// Rule 3: a snapshot that cannot be written never affects the chain; the
/// next due height writes one.
#[test]
fn a_snapshot_failure_does_not_affect_commit() {
    use std::os::unix::fs::PermissionsExt;
    // Directory permissions do not bind root.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = snapshot_app(&inputs, dir.path(), 2);
    let root = dir.path().join("snapshots");
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o500)).unwrap();
    commit_next(&mut app, 2);
    assert_eq!(app.wait_for_snapshot().unwrap(), None);
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    commit_next(&mut app, 3);
    commit_next(&mut app, 4);
    assert_eq!(app.wait_for_snapshot().unwrap().unwrap().height, 4);
    assert_eq!(app.info().unwrap().height, 4);
}

/// A chain at height 12 with its snapshot at 12: the metadata bytes and
/// chunks.
fn snapshotted_chain(
    inputs: &Inputs,
    dir: &std::path::Path,
) -> (ConsensusApplication, Vec<u8>, Vec<Vec<u8>>) {
    use crate::snapshot::{chunk_path, snapshot_dir, Metadata, METADATA_FILE};
    let mut app = snapshot_app(inputs, dir, 12);
    for height in 2..=12 {
        commit_next(&mut app, height);
    }
    app.wait_for_snapshot().unwrap().unwrap();
    let target = snapshot_dir(&dir.join("snapshots"), 12);
    let raw = std::fs::read(target.join(METADATA_FILE)).unwrap();
    let chunks = (0..Metadata::decode(&raw).unwrap().chunks.len())
        .map(|index| std::fs::read(chunk_path(&target, index)).unwrap())
        .collect();
    (app, raw, chunks)
}

fn offer(app: &mut ConsensusApplication, raw: &[u8], chunks: usize, trusted: &str) -> SnapshotOffer {
    let hash = crate::snapshot::Metadata::decode(raw).unwrap().hash().unwrap();
    app.offer_snapshot(12, 1, u32::try_from(chunks).unwrap(), &hash, raw, trusted)
        .unwrap()
}

/// State sync v1, rule 4: a snapshot of a live chain restores on an empty
/// node to the same application hash; the restored node passes the startup
/// check, restarts, and continues the chain in step with the source.
#[test]
fn a_snapshot_restores_on_an_empty_node_and_the_chain_continues() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let (mut source, raw, chunks) = snapshotted_chain(&inputs, &dir.path().join("source"));
    let trusted = source.info().unwrap().app_hash;
    let target_path = dir.path().join("target");
    let mut target = inputs.open(&target_path);
    assert_eq!(offer(&mut target, &raw, chunks.len(), &trusted), SnapshotOffer::Accept);
    for (index, chunk) in chunks.iter().enumerate() {
        assert_eq!(
            target
                .apply_snapshot_chunk(u32::try_from(index).unwrap(), chunk)
                .unwrap(),
            SnapshotChunk::Accept
        );
    }
    assert_eq!(target.info().unwrap(), source.info().unwrap());
    assert!(!beside(&target_path, ".restore").exists());
    assert!(!beside(&target_path, ".replaced").exists());
    // A node with state takes no snapshot.
    assert_eq!(offer(&mut source, &raw, chunks.len(), &trusted), SnapshotOffer::Abort);
    drop(target);
    let mut target = inputs.open(&target_path);
    verify_recovery(&target.storage).unwrap();
    for height in 13..=16 {
        commit_next(&mut source, height);
        commit_next(&mut target, height);
        assert_eq!(target.info().unwrap(), source.info().unwrap());
    }
}

/// Rule 4: an offer must match the trusted application hash, format and
/// metadata hash; a chunk that differs from its listed hash is fetched
/// again; chunks out of order, or a stream whose rebuilt state does not give
/// the trusted hash, reject the snapshot and leave the node empty; an
/// interrupted restore is discarded on restart.
#[test]
fn restore_refuses_untrusted_or_altered_snapshots() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let (source, raw, chunks) = snapshotted_chain(&inputs, &dir.path().join("source"));
    let trusted = source.info().unwrap().app_hash;
    let path = dir.path().join("target");
    let mut target = inputs.open(&path);
    let hash = crate::snapshot::Metadata::decode(&raw).unwrap().hash().unwrap();
    let count = u32::try_from(chunks.len()).unwrap();
    assert_eq!(offer(&mut target, &raw, chunks.len(), &"0".repeat(64)), SnapshotOffer::Reject);
    assert_eq!(
        target.offer_snapshot(12, 2, count, &hash, &raw, &trusted).unwrap(),
        SnapshotOffer::RejectFormat
    );
    assert_eq!(
        target.offer_snapshot(12, 1, count, &[0; 32], &raw, &trusted).unwrap(),
        SnapshotOffer::Reject
    );
    assert_eq!(
        target.offer_snapshot(11, 1, count, &hash, &raw, &trusted).unwrap(),
        SnapshotOffer::Reject
    );
    // A corrupted chunk is fetched again; a chunk out of order rejects.
    assert_eq!(offer(&mut target, &raw, chunks.len(), &trusted), SnapshotOffer::Accept);
    let mut corrupted = chunks[0].clone();
    corrupted[0] ^= 1;
    assert_eq!(target.apply_snapshot_chunk(0, &corrupted).unwrap(), SnapshotChunk::Retry);
    assert_eq!(
        target.apply_snapshot_chunk(count, &chunks[0]).unwrap(),
        SnapshotChunk::RejectSnapshot
    );
    assert!(!beside(&path, ".restore").exists());
    assert_eq!(target.apply_snapshot_chunk(0, &chunks[0]).unwrap(), SnapshotChunk::Abort);
    // Altered state with consistent chunk hashes: the rebuilt root differs.
    let mut stream: Vec<u8> = chunks.concat();
    let entries = crate::snapshot::decode_entries(&stream).unwrap();
    let (key, value) = entries
        .iter()
        .find(|(key, _)| key.starts_with(b"acct:"))
        .unwrap();
    let at = stream
        .windows(key.len() + 4 + value.len())
        .position(|w| w.starts_with(key) && w.ends_with(value))
        .unwrap();
    let last = at + key.len() + 4 + value.len() - 1;
    stream[last] ^= 1;
    let mut altered = crate::snapshot::Metadata::decode(&raw).unwrap();
    let altered_chunks: Vec<Vec<u8>> = stream
        .chunks(crate::snapshot::CHUNK_BYTES)
        .map(<[u8]>::to_vec)
        .collect();
    altered.chunks = altered_chunks
        .iter()
        .map(|chunk| hex::encode(<sha3::Sha3_256 as sha3::Digest>::digest(chunk)))
        .collect();
    let altered_raw = altered.encode().unwrap();
    assert_eq!(offer(&mut target, &altered_raw, altered_chunks.len(), &trusted), SnapshotOffer::Accept);
    let mut last_result = None;
    for (index, chunk) in altered_chunks.iter().enumerate() {
        last_result = Some(
            target
                .apply_snapshot_chunk(u32::try_from(index).unwrap(), chunk)
                .unwrap(),
        );
    }
    assert_eq!(last_result, Some(SnapshotChunk::RejectSnapshot));
    assert!(!beside(&path, ".restore").exists());
    assert!(target.storage.db.iterator(IteratorMode::Start).next().is_none());
    // An interrupted restore leaves a staging database that restart removes.
    assert_eq!(offer(&mut target, &raw, chunks.len(), &trusted), SnapshotOffer::Accept);
    assert!(beside(&path, ".restore").exists());
    drop(target);
    let target = inputs.open(&path);
    assert!(!beside(&path, ".restore").exists());
    assert!(target.storage.db.iterator(IteratorMode::Start).next().is_none());
}

fn account_scans() -> usize {
    crate::supply::ACCOUNT_SCANS.with(|n| n.get())
}
/// Every account record, summed.
fn scanned_totals(app: &ConsensusApplication) -> crate::supply::AccountTotals {
    let maps: Vec<BTreeMap<String, u128>> = app
        .storage
        .db
        .iterator(IteratorMode::From(b"acct:balances:", Direction::Forward))
        .map(|item| item.unwrap())
        .take_while(|(key, _)| key.starts_with(b"acct:balances:"))
        .map(|(_, raw)| bincode::deserialize(&raw).unwrap())
        .collect();
    crate::supply::AccountTotals::sum(&maps).unwrap()
}
fn stored_totals(app: &ConsensusApplication) -> crate::supply::AccountTotals {
    let raw = app
        .storage
        .db
        .get(crate::supply::ACCOUNT_TOTALS_KEY)
        .unwrap()
        .unwrap();
    crate::supply::AccountTotals::decode(&raw).unwrap()
}

/// E04 gap 5: after the startup check, blocks read no account record for
/// the supply check; they use the running account totals.
#[test]
fn committed_blocks_read_no_account_records_for_supply() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let mut app = committed_chain(&inputs, &dir.path().join("db"), 2);
    let before = account_scans();
    for height in 3..=12 {
        commit_next(&mut app, height);
    }
    app.query().unwrap();
    assert_eq!(account_scans() - before, 0);
    drop(app);
    // A restart's complete check reads every account once.
    let before = account_scans();
    verify_recovery(&inputs.open(&dir.path().join("db")).storage).unwrap();
    assert_eq!(account_scans() - before, 1);
}

/// The running totals follow a transfer that creates a record, and both the
/// complete check and a block's check refuse totals that do not match.
#[test]
fn account_totals_follow_balances_and_are_audited() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let app = committed_chain(&inputs, &dir.path().join("db"), 4);
    assert!(app.storage.db.get(b"acct:balances:recipient").unwrap().is_some());
    assert_eq!(stored_totals(&app), scanned_totals(&app));
    let check = || verify_recovery_with(&HistoryRead::new(&app.storage)).map(|_| ());
    check().unwrap();
    let key = crate::supply::ACCOUNT_TOTALS_KEY;
    let good = app.storage.db.get(key).unwrap().unwrap();
    let mut wrong = stored_totals(&app);
    wrong.udrt += 1;
    app.storage.db.put(key, wrong.encode().unwrap()).unwrap();
    let error = check().unwrap_err().to_string();
    assert!(error.contains("Account totals differ"), "{error}");
    app.storage.db.put(key, &good).unwrap();
    check().unwrap();
    // A block's balance change must come with the totals it leaves.
    let mut balances: BTreeMap<String, u128> = BTreeMap::new();
    balances.insert("udrt".into(), 7);
    let mut writes = Writes::new();
    writes.insert(b"acct:balances:new".to_vec(), bincode::serialize(&balances).unwrap());
    let error = crate::supply::validate_native_block(&app.storage, &writes, &Deletes::new())
        .unwrap_err()
        .to_string();
    assert!(error.contains("block's balance changes"), "{error}");
}
