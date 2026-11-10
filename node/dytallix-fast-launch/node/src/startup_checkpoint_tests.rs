//! Startup checkpoint v1 (T06): startup work after a checkpoint does not grow
//! with the history before it, one startup check runs per start, and
//! tampering before the checkpoint is still caught by the complete check.
//! These count work; they are not wall-clock benchmarks.
use super::*;
use serde_json::Value;

fn decodes() -> usize {
    BLOCK_DECODES.with(|n| n.get())
}

fn startup_checks() -> usize {
    STARTUP_CHECKS.with(|n| n.get())
}

/// An archive chain of `height` blocks; epoch boundaries (two-block epochs)
/// carry the derived observation.
fn archive_chain(inputs: &Inputs, path: &Path, height: u64) {
    let mut app = inputs.initialized(path).with_block_history(BlockHistory::Archive);
    for h in 1..=height {
        let parent = block(h - 1, vec![]).hash;
        let txs = derived_observation_wire(&app.storage, &app.config, 2, h, &parent)
            .unwrap()
            .into_iter()
            .collect();
        app.finalize_block(block(h, txs)).unwrap();
        app.commit().unwrap();
    }
}

fn reopen(inputs: &Inputs, path: &Path) -> Result<ConsensusApplication> {
    ConsensusApplication::open(path, inputs.config.clone(), inputs.genesis.clone())
}

fn checkpoint_at(path: &Path) -> Option<StartupCheckpoint> {
    read_checkpoint(&Storage::open_read_only(path.to_path_buf()).unwrap()).unwrap()
}

/// Move the stored checkpoint back to `height`, as a node whose checkpoint
/// lags its head (an older release committed the later blocks).
fn set_checkpoint(path: &Path, height: u64) {
    let storage = Storage::open(path.to_path_buf()).unwrap();
    let record = HistoryRead::new(&storage).block(height).unwrap();
    let mut checkpoint = read_checkpoint(&storage).unwrap().unwrap();
    checkpoint.height = height;
    checkpoint.complete_height = checkpoint.complete_height.min(height);
    checkpoint.engine_hash = record.head.anchor.engine_hash.clone();
    checkpoint.app_hash = record.head.app_hash.clone();
    checkpoint.controls.retain(|h| *h <= height);
    write_checkpoint(&storage, &checkpoint).unwrap();
}

fn edit_record(path: &Path, height: u64, edit: impl FnOnce(&mut Value)) {
    let storage = Storage::open(path.to_path_buf()).unwrap();
    let mut value: Value =
        serde_json::from_slice(&storage.db.get(record_key(height)).unwrap().unwrap()).unwrap();
    edit(&mut value);
    storage.db.put(record_key(height), serde_json::to_vec(&value).unwrap()).unwrap();
}

/// The complete check of a stopped node: the maintenance command.
fn complete_check(inputs: &Inputs, path: &Path) -> Result<(StoppedCheck, Option<u64>)> {
    write_startup_checkpoint(path, &inputs.config, &inputs.genesis)
}

#[test]
fn startup_reads_only_the_blocks_after_the_checkpoint_at_any_history_length() {
    let inputs = Inputs::new();
    let mut tails = Vec::new();
    for height in [20u64, 140] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        archive_chain(&inputs, &path, height);
        // Each commit moved the checkpoint to its block.
        let checkpoint = checkpoint_at(&path).unwrap();
        assert_eq!((checkpoint.height, checkpoint.app_hash.clone()),
            (height, read_head(&Storage::open_read_only(path.clone()).unwrap()).unwrap().unwrap().app_hash));
        let before = decodes();
        drop(reopen(&inputs, &path).unwrap());
        let at_head = decodes() - before;
        // Five blocks behind the head: those five, whatever came before.
        set_checkpoint(&path, height - 5);
        let before = decodes();
        drop(reopen(&inputs, &path).unwrap());
        let behind = decodes() - before;
        assert_eq!(checkpoint_at(&path).unwrap().height, height);
        tails.push((at_head, behind));
    }
    assert_eq!(tails[0], tails[1], "startup work grew with history before the checkpoint");
    // A block reads itself and at most its parent (epoch observations).
    assert!(tails[0].0 <= 3 && tails[0].1 <= tails[0].0 + 2 * 5, "{tails:?}");
}

#[test]
fn a_data_dir_without_a_checkpoint_gets_one_from_the_complete_check() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    archive_chain(&inputs, &path, 30);
    // An existing data directory, from before the checkpoint.
    Storage::open(path.clone()).unwrap().db.delete(CHECKPOINT_KEY).unwrap();
    assert!(checkpoint_at(&path).is_none());
    // The maintenance command runs the complete check over every block.
    let before = decodes();
    let (check, height) = complete_check(&inputs, &path).unwrap();
    assert!(matches!(check, StoppedCheck::Passed(ref info) if info.height == 30));
    assert_eq!(height, Some(30));
    assert!(decodes() - before >= 30);
    let checkpoint = checkpoint_at(&path).unwrap();
    assert_eq!((checkpoint.height, checkpoint.complete_height), (30, 30));
    // The next start reads only the checkpoint's own record.
    let before = decodes();
    drop(reopen(&inputs, &path).unwrap());
    assert!(decodes() - before <= 3);
    // Without the command, startup still runs the complete check (as before
    // the checkpoint), and writes one.
    Storage::open(path.clone()).unwrap().db.delete(CHECKPOINT_KEY).unwrap();
    let before = decodes();
    drop(reopen(&inputs, &path).unwrap());
    assert!(decodes() - before >= 30);
    assert_eq!(checkpoint_at(&path).unwrap().height, 30);
}

#[test]
fn the_maintenance_command_refuses_a_running_node_and_a_missing_database() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    archive_chain(&inputs, &path, 4);
    let running = reopen(&inputs, &path).unwrap();
    assert!(complete_check(&inputs, &path).is_err());
    drop(running);
    complete_check(&inputs, &path).unwrap();
    assert!(complete_check(&inputs, &dir.path().join("absent")).is_err());
    assert!(!dir.path().join("absent").exists());
}

#[test]
fn preflight_and_writable_open_run_one_startup_check() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    archive_chain(&inputs, &path, 12);
    set_checkpoint(&path, 8);
    let preflight = || {
        let storage = Storage::open_read_only(path.clone()).unwrap();
        let Startup::Verified(checkpoint) = check_stored_startup(
            &storage, &inputs.config, &inputs.genesis, None, None, false,
        )
        .unwrap() else {
            panic!("initialized");
        };
        PreflightStartup {
            fingerprint: startup_fingerprint(&storage, &path).unwrap(),
            checkpoint,
        }
    };
    let writable = |pre: &PreflightStartup| {
        let storage = Storage::open(path.clone()).unwrap();
        let before = startup_checks();
        assert!(writable_startup_check(
            &storage, &path, &inputs.config, &inputs.genesis, None, None, Some(pre),
        )
        .unwrap());
        assert!(storage.verified_at_current_sequence().unwrap());
        startup_checks() - before
    };
    // Unchanged between the two opens: the writable open checks nothing
    // again and writes the preflight's checkpoint.
    let pre = preflight();
    assert_eq!(pre.checkpoint.as_ref().unwrap().height, 12);
    assert_eq!(checkpoint_at(&path).unwrap().height, 8);
    assert_eq!(writable(&pre), 0);
    assert_eq!(checkpoint_at(&path).unwrap().height, 12);
    // A write between them (another writer) and the check runs again.
    let pre = preflight();
    Storage::open(path.clone()).unwrap().db.put(b"acct:other-writer", b"1").unwrap();
    let storage = Storage::open(path.clone()).unwrap();
    assert!(writable_startup_check(
        &storage, &path, &inputs.config, &inputs.genesis, None, None, Some(&pre),
    )
    .is_err());
}

#[test]
fn tampering_before_the_checkpoint_is_caught_by_the_complete_check_only() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    archive_chain(&inputs, &path, 16);
    // Record 5 still parses but no longer matches its own commitments.
    edit_record(&path, 5, |record| {
        let nanos = record["head"]["anchor"]["time_nanos"].as_u64().unwrap();
        record["head"]["anchor"]["time_nanos"] = (nanos + 1).into();
    });
    // Startup does not re-read it (documented: startup checkpoint v1).
    drop(reopen(&inputs, &path).unwrap());
    // The complete check does, and writes no checkpoint.
    let before = checkpoint_at(&path);
    assert!(complete_check(&inputs, &path).is_err());
    assert_eq!(checkpoint_at(&path), before);
}

#[test]
fn tampering_at_or_after_the_checkpoint_or_with_state_refuses_startup() {
    let inputs = Inputs::new();
    let fresh = |height: u64| {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        archive_chain(&inputs, &path, height);
        (dir, path)
    };
    // The checkpoint's own record.
    let (_dir, path) = fresh(12);
    set_checkpoint(&path, 9);
    edit_record(&path, 9, |record| {
        record["head"]["anchor"]["time_nanos"] = 7.into();
    });
    assert!(reopen(&inputs, &path).is_err());
    // A block after the checkpoint.
    let (_dir, path) = fresh(12);
    set_checkpoint(&path, 9);
    edit_record(&path, 11, |record| {
        record["head"]["anchor"]["time_nanos"] = 7.into();
    });
    assert!(reopen(&inputs, &path).is_err());
    // The checkpoint's bytes: another hash, a height above the head, an
    // unknown version, unreadable bytes.
    for edit in [
        serde_json::json!({"app_hash": "00".repeat(32)}),
        serde_json::json!({"height": 13}),
        serde_json::json!({"version": 2}),
    ] {
        let (_dir, path) = fresh(12);
        let storage = Storage::open(path.clone()).unwrap();
        let mut value: Value =
            serde_json::from_slice(&storage.db.get(CHECKPOINT_KEY).unwrap().unwrap()).unwrap();
        for (key, field) in edit.as_object().unwrap() {
            value[key] = field.clone();
        }
        storage.db.put(CHECKPOINT_KEY, serde_json::to_vec(&value).unwrap()).unwrap();
        drop(storage);
        assert!(reopen(&inputs, &path).is_err(), "{edit}");
    }
    let (_dir, path) = fresh(12);
    Storage::open(path.clone()).unwrap().db.put(CHECKPOINT_KEY, b"{").unwrap();
    let error = reopen(&inputs, &path).err().unwrap();
    assert!(format!("{error:#}").contains("--write-startup-checkpoint"), "{error:#}");
    // An account balance: the state-wide checks still run at startup.
    let (_dir, path) = fresh(12);
    let storage = Storage::open(path.clone()).unwrap();
    let (key, value) = storage
        .db
        .prefix_iterator(b"acct:")
        .map(|item| item.unwrap())
        .find(|(key, _)| key.starts_with(b"acct:"))
        .unwrap();
    let mut changed = value.to_vec();
    *changed.last_mut().unwrap() ^= 1;
    storage.db.put(&key, changed).unwrap();
    drop(storage);
    assert!(reopen(&inputs, &path).is_err());
}

#[test]
fn a_node_that_removed_records_ignores_the_checkpoint() {
    let inputs = Inputs::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    archive_chain(&inputs, &path, 12);
    let storage = Storage::open(path.clone()).unwrap();
    // As a window would: the checkpoint is no help once records go.
    storage.db.put(RETAINED_KEY, 2u64.to_be_bytes()).unwrap();
    assert!(extended_checkpoint(&storage, &read_head(&storage).unwrap().unwrap(), false)
        .unwrap()
        .is_none());
}
