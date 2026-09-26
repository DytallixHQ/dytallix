use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use dytallix_fast_node::storage::bridge::{verify_bridge_message, BridgeMessage, BridgeValidator};
use ed25519_dalek::{Keypair, PublicKey, SecretKey, Signer};

fn deterministic_kp(tag: u8) -> Keypair {
    let mut seed = [0u8; 32];
    seed[0] = tag;
    let secret = SecretKey::from_bytes(&seed).unwrap();
    let public: PublicKey = (&secret).into();
    Keypair { secret, public }
}

fn mk_validator(id: &str) -> (BridgeValidator, Keypair) {
    let kp = deterministic_kp(id.as_bytes().first().copied().unwrap_or(0));
    let pk_b64 = B64.encode(kp.public.as_bytes());
    (
        BridgeValidator {
            id: id.to_string(),
            pubkey: pk_b64,
        },
        kp,
    )
}

fn sign_payload(kp: &Keypair, msg: &BridgeMessage) -> String {
    let payload = format!(
        "{}:{}:{}:{}:{}:{}",
        msg.id, msg.source_chain, msg.dest_chain, msg.asset, msg.amount, msg.recipient
    );
    let sig = kp.sign(payload.as_bytes());
    B64.encode(sig.to_bytes())
}

#[test]
fn quorum_math_and_signature_validation() {
    // 5 validators -> need ceil(2/3*5) = 4
    let mut vals = vec![];
    let mut kps = vec![];
    for i in 0..5 {
        let (v, k) = mk_validator(&format!("v{i}"));
        vals.push(v);
        kps.push(k);
    }
    let mut msg = BridgeMessage {
        id: "m1".into(),
        source_chain: "A".into(),
        dest_chain: "B".into(),
        asset: "dyt".into(),
        amount: 100,
        recipient: "dest".into(),
        signatures: vec![],
        signers: vec![],
    };
    // Add only 3 signatures (below 4 quorum)
    for i in 0..3 {
        msg.signers.push(format!("v{i}"));
        msg.signatures.push(sign_payload(&kps[i as usize], &msg));
    }
    let err = verify_bridge_message(&msg, &vals).unwrap_err();
    assert!(err.starts_with("InsufficientQuorum"));
    // Add one more valid signer (v3)
    msg.signers.push("v3".into());
    msg.signatures.push(sign_payload(&kps[3], &msg));
    verify_bridge_message(&msg, &vals).expect("now meets quorum");
}

#[test]
fn duplicate_signers_not_counted() {
    // 3 validators -> need ceil(2/3*3)=2
    let mut vals = vec![];
    let mut kps = vec![];
    for i in 0..3 {
        let (v, k) = mk_validator(&format!("v{i}"));
        vals.push(v);
        kps.push(k);
    }
    let mut msg = BridgeMessage {
        id: "m2".into(),
        source_chain: "A".into(),
        dest_chain: "B".into(),
        asset: "dyt".into(),
        amount: 1,
        recipient: "dest".into(),
        signatures: vec![],
        signers: vec![],
    };
    // same signer repeated
    msg.signers.push("v0".into());
    msg.signatures.push(sign_payload(&kps[0], &msg));
    msg.signers.push("v0".into());
    msg.signatures.push(sign_payload(&kps[0], &msg));
    let err = verify_bridge_message(&msg, &vals).unwrap_err();
    assert!(err.starts_with("InsufficientQuorum"));
    // add second distinct signer
    msg.signers.push("v1".into());
    msg.signatures.push(sign_payload(&kps[1], &msg));
    verify_bridge_message(&msg, &vals).expect("quorum with two distinct signers");
}
