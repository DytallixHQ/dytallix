use blake3;
use std::collections::BTreeMap;

/// Compute deterministic state root from arbitrary key->value map.
/// Keys and values are raw byte slices. Caller should canonicalize higher-level structs.
pub fn commit<K: AsRef<[u8]>, V: AsRef<[u8]>>(pairs: &[(K, V)]) -> [u8; 32] {
    if pairs.is_empty() {
        return *blake3::hash(b"DYTALLIX/STATE/EMPTY").as_bytes();
    }
    // Insert into BTreeMap for lexicographic order of keys
    let mut map = BTreeMap::new();
    for (k, v) in pairs {
        map.insert(k.as_ref().to_vec(), v.as_ref().to_vec());
    }
    // Leaf hashes
    let mut leaves: Vec<[u8; 32]> = map
        .into_iter()
        .map(|(k, v)| {
            let mut hasher = blake3::Hasher::new();
            hasher.update(&k);
            hasher.update(&v);
            *hasher.finalize().as_bytes()
        })
        .collect();
    // Build binary Merkle
    while leaves.len() > 1 {
        let mut next = Vec::with_capacity(leaves.len().div_ceil(2));
        for chunk in leaves.chunks(2) {
            if chunk.len() == 1 {
                next.push(chunk[0]);
                continue;
            }
            let mut hasher = blake3::Hasher::new();
            hasher.update(&chunk[0]);
            hasher.update(&chunk[1]);
            next.push(*hasher.finalize().as_bytes());
        }
        leaves = next;
    }
    leaves[0]
}

pub fn commit_hex<K: AsRef<[u8]>, V: AsRef<[u8]>>(pairs: &[(K, V)]) -> String {
    hex::encode(commit(pairs))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty() {
        let r = commit::<&[u8], &[u8]>(&[]);
        assert_eq!(
            hex::encode(r),
            hex::encode(blake3::hash(b"DYTALLIX/STATE/EMPTY").as_bytes())
        );
    }
    #[test]
    fn determinism() {
        let a = commit(&[
            (b"b".as_ref(), b"2".as_ref()),
            (b"a".as_ref(), b"1".as_ref()),
        ]);
        let b = commit(&[
            (b"a".as_ref(), b"1".as_ref()),
            (b"b".as_ref(), b"2".as_ref()),
        ]);
        assert_eq!(a, b);
    }
}
