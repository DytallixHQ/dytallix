use super::*;
use rand::{rngs::StdRng, Rng, SeedableRng};

fn storage() -> (tempfile::TempDir, Storage) {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("db")).unwrap();
    (dir, storage)
}
fn commit(storage: &Storage, update: &Update) {
    let mut batch = rocksdb::WriteBatch::default();
    for key in &update.deletes {
        batch.delete(key);
    }
    for (key, value) in &update.writes {
        batch.put(key, value);
    }
    storage.db.write(batch).unwrap();
}
fn tree_records(storage: &Storage) -> usize {
    storage
        .db
        .iterator(IteratorMode::From(PREFIX, Direction::Forward))
        .take_while(|e| e.as_ref().unwrap().0.starts_with(PREFIX))
        .count()
}

/// Incremental roots equal a root rebuilt from the full key set, through
/// random inserts, updates and deletions over many blocks.
#[test]
fn incremental_root_equals_a_rebuilt_reference_through_random_changes() {
    let (_dir, storage) = storage();
    let mut rng = StdRng::seed_from_u64(7);
    let mut state: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    let genesis: Vec<_> = (0..40u32)
        .map(|i| (format!("acct:{i}").into_bytes(), vec![i as u8; 3]))
        .collect();
    state.extend(genesis.iter().cloned());
    let update =
        super::update(&storage, 0, genesis.into_iter().map(|(k, v)| (k, Some(v)))).unwrap();
    commit(&storage, &update);
    assert_eq!(update.root, rebuilt_root(state.clone()).unwrap());
    let mut peak = 0;
    for version in 1..=120u64 {
        let mut changes = BTreeMap::new();
        for _ in 0..rng.gen_range(0..6) {
            let key = format!("acct:{}", rng.gen_range(0..60u32)).into_bytes();
            let value = if rng.gen_bool(0.25) {
                None
            } else {
                Some(vec![rng.gen(); rng.gen_range(1..8)])
            };
            changes.insert(key, value);
        }
        for (key, value) in &changes {
            match value {
                Some(value) => {
                    state.insert(key.clone(), value.clone());
                }
                None => {
                    state.remove(key);
                }
            }
        }
        let update = super::update(&storage, version, changes).unwrap();
        commit(&storage, &update);
        assert_eq!(update.root, root(&storage, version).unwrap());
        assert_eq!(
            update.root,
            rebuilt_root(state.clone()).unwrap(),
            "version {version}"
        );
        peak = peak.max(tree_records(&storage));
    }
    verify_stored(&storage, 120, &state).unwrap();
    // Only the latest version is kept: records stay near the live key count.
    assert!(peak < 4 * 60 + 60, "tree records peaked at {peak}");
}

#[test]
fn proofs_show_presence_and_absence_against_the_root() {
    let (_dir, storage) = storage();
    let entries = [
        (b"a".to_vec(), b"1".to_vec()),
        (b"b".to_vec(), b"2".to_vec()),
    ];
    let update = super::update(
        &storage,
        0,
        entries.iter().cloned().map(|(k, v)| (k, Some(v))),
    )
    .unwrap();
    commit(&storage, &update);
    let root = RootHash(update.root);
    let (value, proof) = prove(&storage, 0, b"a").unwrap();
    assert_eq!(value, Some(leaf_value(b"1")));
    proof
        .verify_existence(root, key_hash(b"a"), leaf_value(b"1"))
        .unwrap();
    assert!(proof
        .verify_existence(root, key_hash(b"a"), leaf_value(b"9"))
        .is_err());
    let (value, proof) = prove(&storage, 0, b"missing").unwrap();
    assert_eq!(value, None);
    proof
        .verify_nonexistence(root, key_hash(b"missing"))
        .unwrap();
    assert!(proof.verify_nonexistence(root, key_hash(b"a")).is_err());
}

#[test]
fn stored_tree_check_refuses_changed_or_extra_records() {
    let (_dir, storage) = storage();
    let state: BTreeMap<Vec<u8>, Vec<u8>> = (0..10u8).map(|i| (vec![b'k', i], vec![i])).collect();
    let update = super::update(
        &storage,
        0,
        state.clone().into_iter().map(|(k, v)| (k, Some(v))),
    )
    .unwrap();
    commit(&storage, &update);
    verify_stored(&storage, 0, &state).unwrap();
    // A state value the tree does not hold.
    let mut changed = state.clone();
    changed.insert(vec![b'k', 3], vec![99]);
    assert!(verify_stored(&storage, 0, &changed).is_err());
    // A key the tree holds but state does not.
    let mut missing = state.clone();
    missing.remove(&vec![b'k', 4]);
    assert!(verify_stored(&storage, 0, &missing).is_err());
    // A corrupted value record.
    let key = value_key(&key_hash(&[b'k', 5]));
    let mut record = storage.db.get(&key).unwrap().unwrap();
    *record.last_mut().unwrap() ^= 1;
    storage.db.put(&key, record).unwrap();
    assert!(verify_stored(&storage, 0, &state).is_err());
}

/// A tree rebuilt at a later version from the full key set, in storage
/// without tree records, has the incremental root and takes later updates.
#[test]
fn rebuilt_tree_at_a_later_version_matches_and_continues() {
    let (_dir, source) = storage();
    let mut state: BTreeMap<Vec<u8>, Vec<u8>> =
        (0..30u8).map(|i| (vec![b'k', i], vec![i; 2])).collect();
    commit(
        &source,
        &super::update(&source, 0, state.clone().into_iter().map(|(k, v)| (k, Some(v)))).unwrap(),
    );
    for version in 1..=9u64 {
        let key = vec![b'k', version as u8];
        state.insert(key.clone(), vec![0xee]);
        commit(
            &source,
            &super::update(&source, version, [(key, Some(vec![0xee]))]).unwrap(),
        );
    }
    let (_other, restored) = storage();
    let update = rebuild(&restored, 9, state.clone()).unwrap();
    assert_eq!(update.root, root(&source, 9).unwrap());
    commit(&restored, &update);
    verify_stored(&restored, 9, &state).unwrap();
    assert!(rebuild(&restored, 9, state.clone()).is_err());
    let change = [(vec![b'k', 3], None)];
    let next = super::update(&restored, 10, change.clone()).unwrap();
    assert_eq!(next.root, super::update(&source, 10, change).unwrap().root);
}
