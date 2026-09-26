use dytallix_fast_node::crypto::PQC;
use dytallix_fast_node::types::tx::Tx;
use dytallix_fast_node::types::{Msg, SignedTx};

#[test]
fn tamper_signature_failure_is_detected() {
    // Generate real Dilithium5 keypair via ActivePQC (pqc-real feature is default)
    let (sk, pk) = dytallix_fast_node::crypto::ActivePQC::keypair();

    // Build a canonical transaction
    let tx = Tx::new(
        "dyt-test-chain",
        0,
        vec![Msg::Send {
            from: "dyt1senderdev000000".to_string(),
            to: "dyt1receiverdev000000".to_string(),
            denom: "DGT".to_string(),
            amount: 1000,
        }],
        1000,
        "pqc tamper test",
    )
    .expect("tx build");

    // Sign tx into SignedTx
    let stx = SignedTx::sign(tx.clone(), &sk, &pk).expect("sign");

    // Baseline: signature verifies
    stx.verify().expect("baseline verify ok");

    // Tamper: change amount in the message
    let mut tampered = stx.clone();
    if let Some(Msg::Send { amount, .. }) = tampered.tx.msgs.first_mut() {
        *amount += 1;
    }

    // Verification must now fail
    let err = tampered.verify().expect_err("tamper must fail");
    let msg = format!("{err}");
    assert!(
        msg.to_lowercase().contains("signature"),
        "unexpected error: {msg}"
    );
}
