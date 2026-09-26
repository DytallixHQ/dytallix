//! Legacy algorithm identifiers. This module contains no cryptographic implementation.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum SignatureAlgorithm {
    Dilithium3,
    Dilithium5,
    Falcon1024,
    SphincsSha256128s,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_metadata_serialization_is_unchanged() {
        for (value, name) in [
            (SignatureAlgorithm::Dilithium3, "Dilithium3"),
            (SignatureAlgorithm::Dilithium5, "Dilithium5"),
            (SignatureAlgorithm::Falcon1024, "Falcon1024"),
            (SignatureAlgorithm::SphincsSha256128s, "SphincsSha256128s"),
        ] {
            let encoded = serde_json::to_string(&value).unwrap();
            assert_eq!(encoded, format!("\"{name}\""));
            assert_eq!(
                serde_json::from_str::<SignatureAlgorithm>(&encoded).unwrap(),
                value
            );
        }
    }
}
