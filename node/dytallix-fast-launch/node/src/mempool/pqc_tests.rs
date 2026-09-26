//! Tests for PQC signature verification in mempool

#[cfg(test)]
mod tests {
    use crate::crypto::{canonical_json, sha3_256, ActivePQC, PQC};
    use crate::mempool::{verify_envelope, Mempool, RejectionReason};
    use crate::state::State;
    use crate::storage::state::Storage; // added
    use crate::storage::tx::Transaction;
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    use std::sync::Arc;
    use tempfile::TempDir; // added // added

    fn create_state() -> State {
        let temp_dir = TempDir::new().expect("tempdir");
        let storage = Arc::new(Storage::open(temp_dir.path().to_path_buf()).expect("open storage"));
        State::new(storage)
    }

    #[test]
    fn test_verify_envelope_valid_signature() {
        // Generate test keypair
        let (sk, pk) = ActivePQC::keypair();

        // Create test transaction
        let mut tx = Transaction::base(
            "test_hash",
            "dyt1alice",
            "dyt1bob",
            1_000_000, // amount
            1_000,     // fee
            42,        // nonce
        )
        .with_gas(21_000, 1_000)
        .with_pqc(B64.encode(&pk), "dytallix-testnet", "test memo");

        // Sign the transaction
        let canonical_tx = tx.canonical_fields();
        let tx_bytes = canonical_json(&canonical_tx).unwrap();
        let tx_hash = sha3_256(&tx_bytes);
        let signature = ActivePQC::sign(&sk, &tx_hash);

        // Set signature
        tx.signature = Some(B64.encode(&signature));

        // Verify signature
        assert!(
            verify_envelope(&tx),
            "Valid signature should pass verification"
        );
    }

    #[test]
    fn test_verify_envelope_invalid_signature() {
        // Generate test keypair
        let (sk, pk) = ActivePQC::keypair();

        // Create test transaction
        let mut tx = Transaction::base("test_hash", "dyt1alice", "dyt1bob", 1_000_000, 1_000, 42)
            .with_gas(21_000, 1_000)
            .with_pqc(B64.encode(&pk), "dytallix-testnet", "test memo");

        // Sign the transaction
        let canonical_tx = tx.canonical_fields();
        let tx_bytes = canonical_json(&canonical_tx).unwrap();
        let tx_hash = sha3_256(&tx_bytes);
        let mut signature = ActivePQC::sign(&sk, &tx_hash);

        // Tamper with signature (mutate one byte)
        if !signature.is_empty() {
            signature[0] ^= 0x01;
        }

        // Set tampered signature
        tx.signature = Some(B64.encode(&signature));

        // Verify signature
        assert!(
            !verify_envelope(&tx),
            "Tampered signature should fail verification"
        );
    }

    #[test]
    fn test_verify_envelope_missing_signature() {
        // Create test transaction without signature
        let tx = Transaction::base("test_hash", "dyt1alice", "dyt1bob", 1_000_000, 1_000, 42)
            .with_gas(21_000, 1_000)
            .with_pqc("dummy_pk", "dytallix-testnet", "test memo");

        // Verify should fail
        assert!(
            !verify_envelope(&tx),
            "Transaction without signature should fail verification"
        );
    }

    #[test]
    fn test_mempool_rejects_invalid_signature() {
        // Create test state and mempool
        let state = create_state();
        let mut mempool = Mempool::new();

        // Create transaction with invalid signature
        let tx = Transaction::base("test_hash", "dyt1alice", "dyt1bob", 1_000_000, 1_000, 42)
            .with_gas(21_000, 1_000)
            .with_pqc("invalid_public_key", "dytallix-testnet", "test memo")
            .with_signature("invalid_signature");

        // Attempt to add transaction should fail with InvalidSignature
        match mempool.add_transaction(&state, tx) {
            Err(RejectionReason::InvalidSignature) => {
                // Ensure display string matches expectation
                assert_eq!(
                    RejectionReason::InvalidSignature.to_string(),
                    "invalid signature"
                );
            }
            _ => panic!("Expected InvalidSignature rejection"),
        }
    }
}
