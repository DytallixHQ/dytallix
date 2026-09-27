//! Authenticated consensus state (state model phase B, D3; design
//! `docs/architecture/state-root-v2.md`, approved 27 September 2026).
//!
//! A Jellyfish Merkle Tree (`jmt`, pinned) over SHA3-256 of each committed
//! state key. A leaf holds SHA3-256 of the value, so state is not stored
//! twice; a proof shows that hash. The tree version is the block height. Tree
//! records live under `merkle:`, outside the committed keys. Only the latest
//! version is kept: nodes a block replaces are deleted when it commits.
use crate::block_lifecycle::{Deletes, Writes};
use crate::storage::state::Storage;
use anyhow::{bail, ensure, Context, Result};
use jmt::storage::{LeafNode, Node, NodeKey, TreeReader};
use jmt::{proof::SparseMerkleProof, JellyfishMerkleTree, KeyHash, OwnedValue, RootHash, Version};
use rocksdb::{Direction, IteratorMode};
use sha3::{Digest, Sha3_256};
use std::collections::BTreeMap;

pub(crate) const PREFIX: &[u8] = b"merkle:";
const NODE: &[u8] = b"merkle:node:";
const VALUE: &[u8] = b"merkle:value:";
type Tree<'a, R> = JellyfishMerkleTree<'a, R, Sha3_256>;

fn node_key(key: &NodeKey) -> Result<Vec<u8>> {
    let mut out = NODE.to_vec();
    out.extend(borsh::to_vec(key)?);
    Ok(out)
}
fn value_key(hash: &KeyHash) -> Vec<u8> {
    let mut out = VALUE.to_vec();
    out.extend(hash.0);
    out
}
pub(crate) fn key_hash(key: &[u8]) -> KeyHash {
    KeyHash(Sha3_256::digest(key).into())
}
/// The leaf value for a state value: its SHA3-256.
pub(crate) fn leaf_value(value: &[u8]) -> OwnedValue {
    Sha3_256::digest(value).to_vec()
}

/// Reads committed tree records, with an optional staged node overlay.
struct Reader<'a> {
    storage: &'a Storage,
}
impl TreeReader for Reader<'_> {
    fn get_node_option(&self, key: &NodeKey) -> Result<Option<Node>> {
        self.storage
            .db
            .get(node_key(key)?)?
            .map(|bytes| Ok(borsh::from_slice(&bytes)?))
            .transpose()
    }
    fn get_value_option(&self, max_version: Version, hash: KeyHash) -> Result<Option<OwnedValue>> {
        let Some(bytes) = self.storage.db.get(value_key(&hash))? else {
            return Ok(None);
        };
        ensure!(bytes.len() > 8, "Invalid state tree value record");
        let version = u64::from_be_bytes(bytes[..8].try_into()?);
        // Only the latest version is kept; older versions are not served.
        ensure!(
            version <= max_version,
            "State tree value is newer than the requested version"
        );
        Ok(Some(bytes[8..].to_vec()))
    }
    fn get_rightmost_leaf(&self) -> Result<Option<(NodeKey, LeafNode)>> {
        bail!("State tree does not support restore from the rightmost leaf")
    }
}

/// An in-memory tree for rebuilding a root from a full key set.
#[derive(Default)]
struct Memory {
    nodes: BTreeMap<NodeKey, Node>,
    values: BTreeMap<KeyHash, OwnedValue>,
}
impl TreeReader for Memory {
    fn get_node_option(&self, key: &NodeKey) -> Result<Option<Node>> {
        Ok(self.nodes.get(key).cloned())
    }
    fn get_value_option(&self, _: Version, hash: KeyHash) -> Result<Option<OwnedValue>> {
        Ok(self.values.get(&hash).cloned())
    }
    fn get_rightmost_leaf(&self) -> Result<Option<(NodeKey, LeafNode)>> {
        bail!("State tree does not support restore from the rightmost leaf")
    }
}

/// One block's tree change: the new root and the tree records to write and
/// delete in the same storage transaction as the state.
pub(crate) struct Update {
    pub root: [u8; 32],
    pub writes: Writes,
    pub deletes: Deletes,
}

/// Apply `changes` (state key to new value, or None to delete) as `version`
/// over the committed tree. Reads only the paths the changes touch.
pub(crate) fn update(
    storage: &Storage,
    version: Version,
    changes: impl IntoIterator<Item = (Vec<u8>, Option<Vec<u8>>)>,
) -> Result<Update> {
    let reader = Reader { storage };
    let set: BTreeMap<KeyHash, Option<OwnedValue>> = changes
        .into_iter()
        .map(|(key, value)| (key_hash(&key), value.as_deref().map(leaf_value)))
        .collect();
    let (root, batch) = Tree::new(&reader).put_value_set(set, version)?;
    let mut writes = Writes::new();
    let mut deletes = Deletes::new();
    for (key, node) in batch.node_batch.nodes() {
        writes.insert(node_key(key)?, borsh::to_vec(node)?);
    }
    for ((value_version, hash), value) in batch.node_batch.values() {
        match value {
            Some(value) => {
                let mut record = value_version.to_be_bytes().to_vec();
                record.extend(value);
                writes.insert(value_key(hash), record);
            }
            None => {
                deletes.insert(value_key(hash));
            }
        }
    }
    for stale in &batch.stale_node_index_batch {
        let key = node_key(&stale.node_key)?;
        // A node replaced in this block is not needed by the latest version.
        if !writes.contains_key(&key) {
            deletes.insert(key);
        }
    }
    Ok(Update {
        root: root.0,
        writes,
        deletes,
    })
}

/// The committed root at `version`.
pub(crate) fn root(storage: &Storage, version: Version) -> Result<[u8; 32]> {
    let reader = Reader { storage };
    Ok(Tree::new(&reader)
        .get_root_hash_option(version)?
        .context("State tree root missing")?
        .0)
}

/// Rebuild a root from every committed state key, without stored tree
/// records: the reference for the complete check.
pub(crate) fn rebuilt_root(
    entries: impl IntoIterator<Item = (Vec<u8>, Vec<u8>)>,
) -> Result<[u8; 32]> {
    let memory = Memory::default();
    let set: BTreeMap<KeyHash, Option<OwnedValue>> = entries
        .into_iter()
        .map(|(key, value)| (key_hash(&key), Some(leaf_value(&value))))
        .collect();
    let (root, _) = Tree::new(&memory).put_value_set(set, 0)?;
    Ok(root.0)
}

/// Check the stored tree at `version` against every committed state entry:
/// each key's proof verifies against the root, and the tree holds no other
/// leaf. Cost O(state × depth); for the complete check only.
pub(crate) fn verify_stored(
    storage: &Storage,
    version: Version,
    entries: &BTreeMap<Vec<u8>, Vec<u8>>,
) -> Result<()> {
    let reader = Reader { storage };
    let tree = Tree::new(&reader);
    let root = RootHash(root(storage, version)?);
    for (key, value) in entries {
        let (stored, proof) = tree.get_with_proof(key_hash(key), version)?;
        let expected = leaf_value(value);
        ensure!(
            stored.as_ref() == Some(&expected),
            "State tree value differs from committed state"
        );
        proof
            .verify_existence(root, key_hash(key), &expected)
            .context("State tree proof does not verify")?;
    }
    let leaves = tree.get_leaf_count(version)?;
    ensure!(
        leaves == entries.len(),
        "State tree holds keys that committed state does not"
    );
    let mut stored = 0usize;
    for item in storage
        .db
        .iterator(IteratorMode::From(VALUE, Direction::Forward))
    {
        let (key, _) = item?;
        if !key.starts_with(VALUE) {
            break;
        }
        stored += 1;
    }
    ensure!(
        stored == entries.len(),
        "State tree value records differ from committed state"
    );
    Ok(())
}

/// A committed value with its proof against the root at `version`.
pub(crate) fn prove(
    storage: &Storage,
    version: Version,
    key: &[u8],
) -> Result<(Option<OwnedValue>, SparseMerkleProof<Sha3_256>)> {
    let reader = Reader { storage };
    Tree::new(&reader).get_with_proof(key_hash(key), version)
}

#[cfg(test)]
#[path = "state_tree_tests.rs"]
mod tests;
