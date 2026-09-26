//! Manual component measurement. No timing gate; signed admission remains mandatory.
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use dytallix_fast_node::{
    crypto::{canonical_json, sha3_256, ActivePQC, PQC},
    mempool::Mempool,
    state::State,
    storage::{state::Storage, tx::Transaction},
};
use std::{sync::Arc, time::Instant};
use tempfile::TempDir;
#[allow(clippy::too_many_arguments)]
fn create_test_transaction_with_key(
    hash: &str,
    from: &str,
    to: &str,
    amount: u128,
    fee: u128,
    nonce: u64,
    gas_limit: u64,
    gas_price: u64,
    sk: &[u8],
    pk: &[u8],
) -> Transaction {
    let tx = Transaction::base(hash, from, to, amount, fee, nonce)
        .with_gas(gas_limit, gas_price)
        .with_pqc(B64.encode(pk), "dytallix-testnet", "");

    let canonical_tx = tx.canonical_fields();
    let tx_bytes = canonical_json(&canonical_tx).expect("serialize canonical tx");
    let tx_hash = sha3_256(&tx_bytes);
    let signature = ActivePQC::sign(sk, &tx_hash);
    tx.with_signature(B64.encode(&signature))
}

#[test]
#[ignore = "manual paired component measurement"]
fn profile_admission_components() {
    let total = Instant::now();
    let tmp = TempDir::new().unwrap();
    let storage = Arc::new(Storage::open(tmp.path().join("node.db")).unwrap());
    let mut state = State::new(storage);
    for i in 0..1000 {
        let name = format!("sender{i}");
        let mut account = state.get_account(&name);
        account.set_balance("udgt", 1_000_000_000);
        account.set_balance("udrt", 1_000_000_000);
        state.accounts.insert(name, account);
    }
    let setup = total.elapsed();
    let start = Instant::now();
    let (sk, pk) = ActivePQC::keypair();
    let keygen = start.elapsed();
    let start = Instant::now();
    let transactions: Vec<_> = (0..1000)
        .map(|i| {
            create_test_transaction_with_key(
                &format!("hash{i}"),
                &format!("sender{i}"),
                "receiver",
                1000,
                100,
                0,
                21000,
                1000 + (i % 100),
                &sk,
                &pk,
            )
        })
        .collect();
    let signing = start.elapsed();
    for count in [100, 1000] {
        let inputs = transactions[..count].to_vec();
        let mut queue = Mempool::new();
        let start = Instant::now();
        for tx in inputs {
            queue.add_transaction(&state, tx).expect("signed admission");
        }
        let verified = start.elapsed();
        assert_eq!(queue.len(), count);
        assert_eq!(queue.total_count(), count);
        let start = Instant::now();
        let selected = queue.take_snapshot(count.min(500));
        let snapshot = start.elapsed();
        assert_eq!(selected.len(), count.min(500));
        let inputs = transactions[..count].to_vec();
        let mut queue = Mempool::new();
        let start = Instant::now();
        for tx in inputs {
            queue
                .add_transaction_trusted(&state, tx)
                .expect("diagnostic trusted admission");
        }
        let trusted = start.elapsed();
        assert_eq!(queue.len(), count);
        assert_eq!(queue.total_count(), count);
        println!(
            "PROFILE count={count} verified_ms={:.3} trusted_diagnostic_ms={:.3} snapshot_ms={:.3}",
            verified.as_secs_f64() * 1000.0,
            trusted.as_secs_f64() * 1000.0,
            snapshot.as_secs_f64() * 1000.0
        );
    }
    println!(
        "PROFILE setup_ms={:.3} keygen_ms={:.3} signing_1000_ms={:.3}",
        setup.as_secs_f64() * 1000.0,
        keygen.as_secs_f64() * 1000.0,
        signing.as_secs_f64() * 1000.0
    );
}
