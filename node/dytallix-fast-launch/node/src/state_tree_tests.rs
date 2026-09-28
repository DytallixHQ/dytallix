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

/// Clients verify proofs with `protocol-types::state_proof`, which shares no
/// code with `jmt` (clients v1, K-d). Both must accept the same proofs and
/// refuse the same tampering, for present and absent keys in random trees.
#[test]
fn client_verifier_agrees_with_jmt_on_random_trees() {
    use dytallix_protocol_types::state_proof::SparseMerkleProof as ClientProof;
    let mut rng = StdRng::seed_from_u64(11);
    for size in [1usize, 2, 3, 17, 200] {
        let (_dir, storage) = storage();
        let entries: BTreeMap<Vec<u8>, Vec<u8>> = (0..size)
            .map(|i| {
                let value: Vec<u8> = (0..rng.gen_range(0..40)).map(|_| rng.gen()).collect();
                (format!("acct:{i}:{}", rng.gen::<u32>()).into_bytes(), value)
            })
            .collect();
        let update = super::update(
            &storage,
            0,
            entries.iter().map(|(k, v)| (k.clone(), Some(v.clone()))),
        )
        .unwrap();
        commit(&storage, &update);
        let root = update.root;
        let absent: Vec<Vec<u8>> = (0..20).map(|i| format!("absent:{i}").into_bytes()).collect();
        let cases = entries
            .iter()
            .map(|(k, v)| (k.clone(), Some(v.clone())))
            .chain(absent.into_iter().map(|k| (k, None)));
        for (key, value) in cases {
            let (_, proof) = prove(&storage, 0, &key).unwrap();
            let json = serde_json::to_value(&proof).unwrap();
            let client: ClientProof = serde_json::from_value(json.clone()).unwrap();
            assert_eq!(serde_json::to_value(&client).unwrap(), json);
            let jmt_ok = |p: &SparseMerkleProof<Sha3_256>, v: Option<&Vec<u8>>| {
                p.verify(RootHash(root), key_hash(&key), v.map(|v| leaf_value(v)))
                    .is_ok()
            };
            assert!(jmt_ok(&proof, value.as_ref()));
            client.verify(&root, &key, value.as_deref()).unwrap();
            // A wrong value, a wrong presence claim and a wrong root are refused by both.
            let wrong = Some(b"not the value".to_vec());
            let claims = [wrong.clone(), if value.is_some() { None } else { wrong }];
            for claim in claims {
                assert_eq!(
                    jmt_ok(&proof, claim.as_ref()),
                    client.verify(&root, &key, claim.as_deref()).is_ok()
                );
                assert!(client.verify(&root, &key, claim.as_deref()).is_err());
            }
            assert!(client.verify(&[0; 32], &key, value.as_deref()).is_err());
            // Tampering with any sibling hash changes the root.
            for index in 0..client.siblings.len() {
                let mut tampered = json.clone();
                let sibling = &mut tampered["siblings"][index];
                let hash = match sibling {
                    serde_json::Value::String(_) => continue,
                    _ => {
                        let (_, node) = sibling.as_object_mut().unwrap().iter_mut().next().unwrap();
                        node.as_object_mut().unwrap().values_mut().next().unwrap()
                    }
                };
                hash[0] = (hash[0].as_u64().unwrap() ^ 1).into();
                let jmt_proof: SparseMerkleProof<Sha3_256> =
                    serde_json::from_value(tampered.clone()).unwrap();
                let client_proof: ClientProof = serde_json::from_value(tampered).unwrap();
                assert!(!jmt_ok(&jmt_proof, value.as_ref()));
                assert!(client_proof.verify(&root, &key, value.as_deref()).is_err());
            }
        }
    }
}
