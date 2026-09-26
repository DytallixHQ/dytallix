//! Version registry for retained chain upgrade implementations.
//! Existing history always uses its original implementation and wire format.
use anyhow::{ensure, Result};
use sha2::{Digest, Sha256};

#[path = "upgrade/v1/upgrade.rs"]
pub mod v1;
pub use v1::*;

const REGISTRY: &[u8] = include_bytes!("upgrade/registry.json");
const V1_SOURCE: &[u8] = include_bytes!("upgrade/v1/upgrade.rs");
const V1_SHA256: &str = "57bf05eda1f4676ceabc6b8b58e71e513970feb340a99d62ab75d6d1dafe31eb";

/// Exact registry identity for candidate manifests. Registry entries are compiled.
pub fn registry_sha256() -> String {
    hex::encode(Sha256::digest(REGISTRY))
}

/// Reject a changed retained implementation rather than silently changing history.
pub fn validate_registry() -> Result<()> {
    ensure!(hex::encode(Sha256::digest(V1_SOURCE)) == V1_SHA256,
        "Retained upgrade v1 implementation changed");
    ensure!(v1::migration_sha256() == V1_SHA256,
        "Retained upgrade v1 digest changed");
    let expected = serde_json::json!({"schema":1,"migrations":[{
        "id":v1::MIGRATION_ID,"version":1,"implementation_sha256":V1_SHA256,
        "source_schema":0,"target_schema":1
    }]});
    ensure!(serde_json::from_slice::<serde_json::Value>(REGISTRY)? == expected,
        "Compiled migration registry differs from retained implementation");
    Ok(())
}

#[cfg(test)]
mod registry_tests {
    use super::*;
    #[test]
    fn retained_v1_implementation_and_registry_match() {
        validate_registry().unwrap();
        assert_eq!(v1::migration_sha256(), V1_SHA256);
        assert_eq!(registry_sha256().len(), 64);
    }
}
