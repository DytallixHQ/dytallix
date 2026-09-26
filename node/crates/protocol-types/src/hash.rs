use serde::Serialize;
use sha3::{Digest, Sha3_256};

/// Serialize to canonical JSON with sorted keys for deterministic hashing.
/// This ensures both CLI and node produce identical JSON for the same transaction.
pub fn canonical_json<T: Serialize>(value: &T) -> serde_json::Result<Vec<u8>> {
    // First serialize to a serde_json::Value to normalize the structure
    let json_value = serde_json::to_value(value)?;

    // Recursively sort all object keys
    let sorted_value = sort_json_value(json_value);

    // Serialize to compact JSON (no extra whitespace)
    let sorted_string = serde_json::to_string(&sorted_value)?;
    Ok(sorted_string.into_bytes())
}

/// Recursively sort all keys in a JSON value to ensure canonical ordering
fn sort_json_value(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut sorted_map = serde_json::Map::new();
            let mut keys: Vec<_> = map.keys().cloned().collect();
            keys.sort();
            for key in keys {
                if let Some(val) = map.get(&key) {
                    sorted_map.insert(key, sort_json_value(val.clone()));
                }
            }
            serde_json::Value::Object(sorted_map)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(sort_json_value).collect())
        }
        other => other,
    }
}

pub fn sha3_256(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha3_256::new();
    h.update(bytes);
    let out = h.finalize();
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&out);
    arr
}
