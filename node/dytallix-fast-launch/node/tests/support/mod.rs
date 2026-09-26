use base64::{engine::general_purpose::STANDARD as B64, Engine};
use dytallix_fast_node::crypto::{canonical_json, sha3_256, ActivePQC, PQC};
use dytallix_fast_node::storage::tx::Transaction;

/// Sign a test envelope with the selected production backend.
pub fn sign_transaction(tx: Transaction) -> Transaction {
    let (sk, pk) = ActivePQC::keypair();
    let tx = tx.with_pqc(B64.encode(&pk), "dytallix-testnet", "integration fixture");
    let bytes = canonical_json(&tx.canonical_fields()).expect("canonical transaction");
    let signature = ActivePQC::sign(&sk, &sha3_256(&bytes));
    tx.with_signature(B64.encode(signature))
}
