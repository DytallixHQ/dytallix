use crate::crypto::{canonical_json, sha3_256, verify, ActivePQC, PQCAlgorithm, PQC};
use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

// Keep the existing node import path for downstream callers.
use dytallix_protocol_types::TRANSACTION_FORMAT_VERSION;
pub use dytallix_protocol_types::{Msg, Tx};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SignedTx {
    pub tx: Tx,
    pub public_key: String, // base64
    pub signature: String,  // base64
    pub algorithm: String,  // ActivePQC::ALG
    pub version: u32,       // 1
}

impl SignedTx {
    pub fn sign(tx: Tx, sk: &[u8], pk: &[u8]) -> Result<Self> {
        let bytes = canonical_json(&tx)?;
        let hash = sha3_256(&bytes);
        let sig = {
            use fips204::{
                ml_dsa_65,
                traits::{SerDes, Signer, Verifier},
            };
            let encoded: [u8; ml_dsa_65::SK_LEN] = sk.try_into().map_err(|_| {
                anyhow!("Signing requires an ML-DSA-65 private key; explicit migration required")
            })?;
            let key = ml_dsa_65::PrivateKey::try_from_bytes(encoded)
                .map_err(|_| anyhow!("Invalid ML-DSA-65 private key"))?;
            let encoded_public: [u8; ml_dsa_65::PK_LEN] = pk
                .try_into()
                .map_err(|_| anyhow!("Invalid ML-DSA-65 public key length"))?;
            let public = ml_dsa_65::PublicKey::try_from_bytes(encoded_public)
                .map_err(|_| anyhow!("Invalid ML-DSA-65 public key"))?;
            let signature = key
                .try_sign(&hash, &[])
                .map_err(|_| anyhow!("ML-DSA-65 signing failed"))?;
            anyhow::ensure!(
                public.verify(&hash, &signature, &[]),
                "Signing key does not authenticate under the supplied ML-DSA-65 public key"
            );
            signature.to_vec()
        };
        Ok(Self {
            tx,
            public_key: B64.encode(pk),
            signature: B64.encode(sig),
            algorithm: ActivePQC::ALG.to_string(),
            version: TRANSACTION_FORMAT_VERSION,
        })
    }

    pub fn verify(&self) -> Result<()> {
        // Parse the algorithm from the string
        let algorithm = PQCAlgorithm::from_str(&self.algorithm)
            .map_err(|e| anyhow!("invalid algorithm '{}': {}", self.algorithm, e))?;

        if self.version != TRANSACTION_FORMAT_VERSION {
            return Err(anyhow!(
                "unsupported version: expected {}, got {}",
                TRANSACTION_FORMAT_VERSION,
                self.version
            ));
        }
        let bytes = canonical_json(&self.tx)?;
        let hash = sha3_256(&bytes);

        let sig = B64
            .decode(&self.signature)
            .map_err(|e| anyhow!("invalid signature encoding: {}", e))?;
        let pk = B64
            .decode(&self.public_key)
            .map_err(|e| anyhow!("invalid public key encoding: {}", e))?;

        // Use the new multi-algorithm verification
        match verify(&pk, &hash, &sig, algorithm) {
            Ok(()) => {
                Ok(())
            }
            Err(e) => {
                Err(anyhow!("signature verification failed: {}", e))
            }
        }
    }

    pub fn tx_hash(&self) -> Result<String> {
        self.tx.tx_hash()
    }

    pub fn first_from_address(&self) -> Option<&str> {
        self.tx.msgs.first().map(|m| m.sender())
    }
}

// Validation helpers
#[derive(Debug, Clone, serde::Serialize)]
pub enum ValidationError {
    InvalidChainId {
        expected: String,
        got: String,
    },
    InvalidNonce {
        expected: u64,
        got: u64,
    },
    InvalidSignature,
    InsufficientFunds {
        denom: String,
        required: u128,
        available: u128,
    },
    DuplicateTransaction,
    MempoolFull,
    Internal(String),
}

impl ValidationError {
    pub fn http_status(&self) -> u16 {
        match self {
            ValidationError::InvalidChainId { .. }
            | ValidationError::InvalidSignature
            | ValidationError::Internal(_) => 422, // Unprocessable Entity
            ValidationError::InvalidNonce { .. } | ValidationError::DuplicateTransaction => 409, // Conflict
            ValidationError::InsufficientFunds { .. } => 422,
            ValidationError::MempoolFull => 503, // Service Unavailable
        }
    }

    pub fn error_code(&self) -> &'static str {
        match self {
            ValidationError::InvalidChainId { .. } => "INVALID_CHAIN_ID",
            ValidationError::InvalidNonce { .. } => "INVALID_NONCE",
            ValidationError::InvalidSignature => "INVALID_SIGNATURE",
            ValidationError::InsufficientFunds { .. } => "INSUFFICIENT_FUNDS",
            ValidationError::DuplicateTransaction => "DUPLICATE_TRANSACTION",
            ValidationError::MempoolFull => "MEMPOOL_FULL",
            ValidationError::Internal(_) => "INTERNAL_ERROR",
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        match self {
            ValidationError::InvalidChainId { expected, got } => {
                serde_json::json!({
                    "error": self.error_code(),
                    "message": format!("Invalid chain_id: expected {}, got {}", expected, got),
                    "expected": expected,
                    "got": got
                })
            }
            ValidationError::InvalidNonce { expected, got } => {
                serde_json::json!({
                    "error": self.error_code(),
                    "message": format!("Invalid nonce: expected {}, got {}", expected, got),
                    "expected": expected,
                    "got": got
                })
            }
            ValidationError::InvalidSignature => {
                serde_json::json!({
                    "error": self.error_code(),
                    "message": "Invalid signature"
                })
            }
            ValidationError::InsufficientFunds {
                denom,
                required,
                available,
            } => {
                serde_json::json!({
                    "error": self.error_code(),
                    "message": format!(
                        "Insufficient funds for {}: required {}, available {}",
                        denom, required, available
                    ),
                    "denom": denom,
                    "required": required.to_string(),
                    "available": available.to_string()
                })
            }
            ValidationError::DuplicateTransaction => {
                serde_json::json!({
                    "error": self.error_code(),
                    "message": "Transaction already exists"
                })
            }
            ValidationError::MempoolFull => {
                serde_json::json!({
                    "error": self.error_code(),
                    "message": "Mempool is full"
                })
            }
            ValidationError::Internal(msg) => {
                serde_json::json!({
                    "error": self.error_code(),
                    "message": msg
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_error_http_status() {
        assert_eq!(
            ValidationError::InvalidChainId {
                expected: "test".into(),
                got: "wrong".into()
            }
            .http_status(),
            422
        );
        assert_eq!(
            ValidationError::InvalidNonce {
                expected: 1,
                got: 2
            }
            .http_status(),
            409
        );
        assert_eq!(ValidationError::InvalidSignature.http_status(), 422);
        assert_eq!(
            ValidationError::InsufficientFunds {
                denom: "udgt".into(),
                required: 100,
                available: 50
            }
            .http_status(),
            422
        );
        assert_eq!(ValidationError::DuplicateTransaction.http_status(), 409);
        assert_eq!(ValidationError::MempoolFull.http_status(), 503);
    }

    #[test]
    fn test_validation_error_json() {
        let err = ValidationError::InvalidNonce {
            expected: 5,
            got: 3,
        };
        let json = err.to_json();

        assert_eq!(json["error"], "INVALID_NONCE");
        assert_eq!(json["expected"], 5);
        assert_eq!(json["got"], 3);

        let insufficient = ValidationError::InsufficientFunds {
            denom: "udrt".into(),
            required: 1_000_000,
            available: 500_000,
        };
        let json = insufficient.to_json();
        assert_eq!(json["error"], "INSUFFICIENT_FUNDS");
        assert_eq!(json["denom"], "udrt");
        assert_eq!(json["required"], "1000000");
        assert_eq!(json["available"], "500000");
    }

    #[test]
    fn test_msg_validation() {
        let valid_msg = Msg::Send {
            from: "alice".into(),
            to: "bob".into(),
            denom: "DGT".into(),
            amount: 100,
        };
        assert!(valid_msg.validate().is_ok());

        let zero_amount = Msg::Send {
            from: "alice".into(),
            to: "bob".into(),
            denom: "DGT".into(),
            amount: 0,
        };
        assert!(zero_amount.validate().is_err());
    }

    #[test]
    fn test_serde_u128_string() {
        let msg = Msg::Send {
            from: "alice".into(),
            to: "bob".into(),
            denom: "DGT".into(),
            amount: 1000000000000000000u128,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"1000000000000000000\""));

        let parsed: Msg = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg);
    }
}

#[cfg(test)]
mod operational_signing_tests {
    use super::*;
    use fips204::{
        ml_dsa_65,
        traits::{KeyGen, SerDes},
    };

    fn transaction() -> Tx {
        Tx {
            chain_id: "signing-profile-test".into(),
            nonce: 0,
            msgs: vec![],
            fee: 0,
            memo: String::new(),
        }
    }

    #[test]
    fn signing_rejects_mismatched_and_legacy_keys_without_translation() {
        let (public, private) = ml_dsa_65::KG::keygen_from_seed(&[61; 32]);
        let (other, _) = ml_dsa_65::KG::keygen_from_seed(&[62; 32]);
        let private = private.into_bytes();
        let public = public.into_bytes();
        SignedTx::sign(transaction(), &private, &public)
            .unwrap()
            .verify()
            .unwrap();
        assert!(SignedTx::sign(transaction(), &private, &other.into_bytes()).is_err());
        assert!(SignedTx::sign(transaction(), b"bad key", &public).is_err());
    }

    #[test]
    fn selected_consensus_rejects_development_algorithm_without_fallback() {
        let (public, private) = ml_dsa_65::KG::keygen_from_seed(&[63; 32]);
        let mut envelope =
            SignedTx::sign(transaction(), &private.into_bytes(), &public.into_bytes()).unwrap();
        envelope.verify().unwrap();
        for algorithm in [
            "mldsa87",
            "dilithium5",
            "falcon1024",
            "sphincs_sha2_128s_simple",
        ] {
            envelope.algorithm = algorithm.into();
            assert!(
                envelope.verify().is_err(),
                "unsupported algorithm {algorithm} accepted"
            );
        }
    }

    #[test]
    fn signing_rejects_inconsistent_imported_secret_without_public_derivation() {
        let (public, private) = ml_dsa_65::KG::keygen_from_seed(&[64; 32]);
        let mut encoded = private.into_bytes();
        // tr binds the serialized public key to the signing operation.
        encoded[64] ^= 1;
        assert!(SignedTx::sign(transaction(), &encoded, &public.into_bytes()).is_err());
    }
}
