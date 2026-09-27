//! Approved signature algorithm identifiers. This module contains no
//! cryptographic implementation. ML-DSA-65 (FIPS 204) is the only approved
//! transaction signature algorithm; pre-standard and classical names are not
//! representable.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum SignatureAlgorithm {
    MlDsa65,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_mldsa65_is_representable() {
        let encoded = serde_json::to_string(&SignatureAlgorithm::MlDsa65).unwrap();
        assert_eq!(encoded, "\"MlDsa65\"");
        assert_eq!(
            serde_json::from_str::<SignatureAlgorithm>(&encoded).unwrap(),
            SignatureAlgorithm::MlDsa65
        );
        for legacy in ["Dilithium3", "Dilithium5", "Falcon1024", "SphincsSha256128s"] {
            assert!(serde_json::from_str::<SignatureAlgorithm>(&format!("\"{legacy}\"")).is_err());
        }
    }
}
