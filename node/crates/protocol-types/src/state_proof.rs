//! State proofs for clients (clients v1, decision 3). A committed value's
//! Jellyfish Merkle proof leads to the state root, and the root with the
//! head's anchor to the application hash, which the caller must trust by
//! other means (their own node, or a verified header). The tree is `jmt`
//! 0.12.0 over SHA3-256: a key's path is SHA3-256 of the key, and a leaf
//! commits to SHA3-256 of SHA3-256 of the value. This follows `jmt` without
//! depending on it; the node's tests check both agree.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use sha3::{Digest, Sha3_256};

pub const TREE: &str = "jmt-0.12.0/sha3-256";
pub const VIEW_VERSION: u16 = 1;
const LEAF_DOMAIN: &[u8] = b"JMT::LeafNode";
const INTERNAL_DOMAIN: &[u8] = b"JMT::IntrnalNode";
const PLACEHOLDER: [u8; 32] = *b"SPARSE_MERKLE_PLACEHOLDER_HASH__";
const APP_DOMAIN: &[u8] = b"dytallix-cometbft-app-v1";
const GENESIS_DOMAIN: &[u8] = b"dytallix-cometbft-genesis-v1";
const MAX_SIBLINGS: usize = 256;

/// The committed block anchor. With the state root it forms the application
/// hash; the chain and clients share this definition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Anchor {
    pub version: u32,
    pub height: u64,
    pub engine_hash: String,
    pub parent_engine_hash: String,
    pub time_seconds: i64,
    pub time_nanos: i32,
    pub input_digest: String,
    pub result_digest: String,
    pub prior_app_hash: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeafNode {
    pub key_hash: [u8; 32],
    pub value_hash: [u8; 32],
}
impl LeafNode {
    pub fn hash(&self) -> [u8; 32] {
        let mut hasher = Sha3_256::new();
        hasher.update(LEAF_DOMAIN);
        hasher.update(self.key_hash);
        hasher.update(self.value_hash);
        hasher.finalize().into()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InternalNode {
    pub left_child: [u8; 32],
    pub right_child: [u8; 32],
}
impl InternalNode {
    pub fn hash(&self) -> [u8; 32] {
        internal(self.left_child, self.right_child)
    }
}
fn internal(left: [u8; 32], right: [u8; 32]) -> [u8; 32] {
    let mut hasher = Sha3_256::new();
    hasher.update(INTERNAL_DOMAIN);
    hasher.update(left);
    hasher.update(right);
    hasher.finalize().into()
}
/// A sibling on the path, with its node type so a prover cannot forge it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProofNode {
    Null,
    Internal(InternalNode),
    Leaf(LeafNode),
}
impl ProofNode {
    pub fn hash(&self) -> [u8; 32] {
        match self {
            Self::Null => PLACEHOLDER,
            Self::Internal(node) => node.hash(),
            Self::Leaf(node) => node.hash(),
        }
    }
}
/// `jmt::proof::SparseMerkleProof` as the node serializes it. Siblings run
/// from the bottom of the path to the root.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SparseMerkleProof {
    pub leaf: Option<LeafNode>,
    pub siblings: Vec<ProofNode>,
    pub phantom_hasher: (),
}

pub fn key_hash(key: &[u8]) -> [u8; 32] {
    Sha3_256::digest(key).into()
}
/// The hash a leaf commits to for `value`: SHA3-256 of the stored SHA3-256.
pub fn value_hash(value: &[u8]) -> [u8; 32] {
    Sha3_256::digest(Sha3_256::digest(value)).into()
}
fn bit(hash: &[u8; 32], index: usize) -> bool {
    (hash[index / 8] >> (7 - index % 8)) & 1 == 1
}
fn common_prefix_bits(a: &[u8; 32], b: &[u8; 32]) -> usize {
    (0..256).take_while(|&i| bit(a, i) == bit(b, i)).count()
}

impl SparseMerkleProof {
    /// Check that `key` has `value` (or, with `None`, is absent) under `root`.
    pub fn verify(&self, root: &[u8; 32], key: &[u8], value: Option<&[u8]>) -> Result<()> {
        let key = key_hash(key);
        ensure!(
            self.siblings.len() <= MAX_SIBLINGS,
            "State proof has more than 256 siblings"
        );
        match (value, &self.leaf) {
            (Some(value), Some(leaf)) => ensure!(
                leaf.key_hash == key && leaf.value_hash == value_hash(value),
                "State proof leaf differs from the key and value"
            ),
            (Some(_), None) => anyhow::bail!("Expected an inclusion proof"),
            // Another key's leaf: the only key in the subtree this key would enter.
            (None, Some(leaf)) => ensure!(
                leaf.key_hash != key
                    && common_prefix_bits(&key, &leaf.key_hash) >= self.siblings.len(),
                "State proof does not show the key absent"
            ),
            (None, None) => {}
        }
        let mut hash = self.leaf.as_ref().map_or(PLACEHOLDER, LeafNode::hash);
        let depth = self.siblings.len();
        for (level, sibling) in self.siblings.iter().enumerate() {
            hash = if bit(&key, depth - 1 - level) {
                internal(sibling.hash(), hash)
            } else {
                internal(hash, sibling.hash())
            };
        }
        ensure!(hash == *root, "State proof does not lead to the state root");
        Ok(())
    }
}

/// The application hash of `state_root` at `height`: at genesis from the
/// root alone, afterwards from the root and that height's anchor.
pub fn app_hash(height: u64, state_root: &[u8; 32], anchor: Option<&Anchor>) -> Result<[u8; 32]> {
    let root = hex::encode(state_root);
    let mut hasher = Sha256::new();
    match anchor {
        None => {
            ensure!(height == 0, "Only the genesis state has no anchor");
            hasher.update(GENESIS_DOMAIN);
            hasher.update(serde_json::to_vec(&root)?);
        }
        Some(anchor) => {
            ensure!(
                height > 0 && anchor.height == height,
                "Anchor height differs from the state height"
            );
            hasher.update(APP_DOMAIN);
            hasher.update(serde_json::to_vec(&(root.as_str(), anchor))?);
        }
    }
    Ok(hasher.finalize().into())
}

/// The node's `/state/proof/{key}` response. `value` is base64 (decoded by
/// the caller); hashes are lowercase hexadecimal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateProofView {
    pub version: u16,
    pub tree: String,
    pub height: u64,
    pub key: String,
    pub value: Option<String>,
    pub leaf: Option<String>,
    pub state_root: String,
    pub anchor: Option<Anchor>,
    pub app_hash: String,
    pub proof: SparseMerkleProof,
}

fn hex32(value: &str) -> Result<[u8; 32]> {
    ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "Expected a lowercase 32-byte hexadecimal hash"
    );
    Ok(hex::decode(value)?.try_into().expect("32 bytes"))
}

impl StateProofView {
    /// Verify `value` (decoded from `self.value`) for `self.key` against
    /// `trusted_app_hash`, the application hash of `self.height` from a
    /// source the caller trusts. The node's own reported hash must agree.
    pub fn verify(&self, value: Option<&[u8]>, trusted_app_hash: &[u8; 32]) -> Result<()> {
        ensure!(
            self.version == VIEW_VERSION && self.tree == TREE,
            "Unsupported state proof view"
        );
        let key = hex::decode(&self.key).context("State proof key is not hexadecimal")?;
        ensure!(
            hex::encode(&key) == self.key && !key.is_empty(),
            "State proof key is not canonical"
        );
        ensure!(
            self.value.is_some() == value.is_some()
                && self.leaf == value.map(|v| hex::encode(Sha3_256::digest(v))),
            "State proof value and leaf differ"
        );
        let root = hex32(&self.state_root)?;
        self.proof.verify(&root, &key, value)?;
        let computed = app_hash(self.height, &root, self.anchor.as_ref())?;
        ensure!(
            computed == hex32(&self.app_hash)?,
            "State proof's application hash differs from its root and anchor"
        );
        ensure!(
            computed == *trusted_app_hash,
            "State proof does not match the trusted application hash"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor(height: u64) -> Anchor {
        Anchor {
            version: 1,
            height,
            engine_hash: "aa".repeat(32),
            parent_engine_hash: "bb".repeat(32),
            time_seconds: 1_790_000_000,
            time_nanos: 5,
            input_digest: "cc".repeat(32),
            result_digest: "dd".repeat(32),
            prior_app_hash: "ee".repeat(32),
        }
    }

    #[test]
    fn a_two_leaf_tree_proves_both_keys_and_an_absent_one() {
        // Choose keys whose hashes differ in the first bit.
        let mut keys: Vec<Vec<u8>> = Vec::new();
        for i in 0u32.. {
            let key = format!("acct:{i}").into_bytes();
            if keys
                .iter()
                .all(|k| bit(&key_hash(k), 0) != bit(&key_hash(&key), 0))
            {
                keys.push(key);
            }
            if keys.len() == 2 {
                break;
            }
        }
        keys.sort_by_key(|k| bit(&key_hash(k), 0));
        let leaves: Vec<LeafNode> = keys
            .iter()
            .map(|k| LeafNode {
                key_hash: key_hash(k),
                value_hash: value_hash(k),
            })
            .collect();
        let root = internal(leaves[0].hash(), leaves[1].hash());
        for (index, key) in keys.iter().enumerate() {
            let proof = SparseMerkleProof {
                leaf: Some(leaves[index]),
                siblings: vec![ProofNode::Leaf(leaves[1 - index])],
                phantom_hasher: (),
            };
            proof.verify(&root, key, Some(key)).unwrap();
            assert!(proof.verify(&root, key, Some(b"other")).is_err());
            assert!(proof.verify(&[0; 32], key, Some(key)).is_err());
        }
        // An absent key whose path passes through the first leaf.
        let absent = (0u32..)
            .map(|i| format!("absent:{i}").into_bytes())
            .find(|k| bit(&key_hash(k), 0) == bit(&leaves[0].key_hash, 0))
            .unwrap();
        let proof = SparseMerkleProof {
            leaf: Some(leaves[0]),
            siblings: vec![ProofNode::Leaf(leaves[1])],
            phantom_hasher: (),
        };
        proof.verify(&root, &absent, None).unwrap();
        assert!(proof.verify(&root, &keys[0], None).is_err());
        let json = serde_json::to_value(&proof).unwrap();
        assert_eq!(json["phantom_hasher"], serde_json::Value::Null);
        assert!(json["siblings"][0]["Leaf"].is_object());
        assert_eq!(
            serde_json::from_value::<SparseMerkleProof>(json).unwrap(),
            proof
        );
    }

    #[test]
    fn application_hashes_bind_the_root_and_the_anchor_height() {
        let root = [3; 32];
        let genesis = app_hash(0, &root, None).unwrap();
        assert_ne!(genesis, app_hash(0, &[4; 32], None).unwrap());
        assert!(app_hash(1, &root, None).is_err());
        let later = app_hash(5, &root, Some(&anchor(5))).unwrap();
        assert!(app_hash(6, &root, Some(&anchor(5))).is_err());
        assert!(app_hash(0, &root, Some(&anchor(0))).is_err());
        let mut moved = anchor(5);
        moved.time_nanos += 1;
        assert_ne!(later, app_hash(5, &root, Some(&moved)).unwrap());
    }
}
