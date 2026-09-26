use dytallix_fast_node::crypto::canonical_json;
/// Test to verify that canonical JSON serialization produces sorted keys
/// This ensures CLI (TypeScript) and node (Rust) produce identical hashes
use dytallix_fast_node::types::tx::{Msg, Tx};

#[test]
fn test_canonical_json_key_ordering() {
    // Create a test transaction identical to what the CLI sends
    let tx = Tx {
        chain_id: "dyt-local-1".to_string(),
        nonce: 0,
        msgs: vec![Msg::Send {
            from: "dytallix1cab745b5e9eda865279472e3f36949350da7982b".to_string(),
            to: "dytallix1test000000000000000000000000000".to_string(),
            denom: "DGT".to_string(),
            amount: 1000000,
        }],
        fee: 1000,
        memo: "".to_string(),
    };

    // Get canonical JSON bytes
    let canonical_bytes = canonical_json(&tx).expect("canonical_json should succeed");
    let canonical_str = String::from_utf8(canonical_bytes.clone()).expect("should be valid UTF-8");

    println!("Canonical JSON from Rust:");
    println!("{}", canonical_str);

    // Expected CLI output (with alphabetically sorted keys)
    let expected_cli = r#"{"chain_id":"dyt-local-1","fee":"1000","memo":"","msgs":[{"amount":"1000000","denom":"DGT","from":"dytallix1cab745b5e9eda865279472e3f36949350da7982b","to":"dytallix1test000000000000000000000000000","type":"send"}],"nonce":0}"#;

    println!("\nExpected CLI canonical JSON:");
    println!("{}", expected_cli);

    // Verify keys are sorted alphabetically at root level
    assert!(
        canonical_str.starts_with(r#"{"chain_id":"#),
        "Root keys should be alphabetically sorted, starting with 'chain_id'"
    );

    // Verify the complete structure matches expected CLI output
    assert_eq!(
        canonical_str, expected_cli,
        "Canonical JSON should exactly match CLI output with sorted keys"
    );

    // Verify hash matches expected
    use dytallix_fast_node::crypto::sha3_256;
    let hash = sha3_256(&canonical_bytes);
    let hash_hex = hex::encode(hash);

    println!("\nComputed hash: {}", hash_hex);

    // This is the hash the CLI computed for this exact transaction
    let expected_hash = "91505f577061ca4ecae26710435b8bc709943c5268ba03c865ff0b86c6bcc52b";

    assert_eq!(
        hash_hex, expected_hash,
        "Hash should match the CLI's computed hash for identical transaction"
    );
}

#[test]
fn test_nested_object_key_ordering() {
    // Test that nested objects (like msgs) also have sorted keys
    let tx = Tx {
        chain_id: "test-chain".to_string(),
        nonce: 5,
        msgs: vec![Msg::Send {
            from: "addr1".to_string(),
            to: "addr2".to_string(),
            denom: "DGT".to_string(),
            amount: 100,
        }],
        fee: 500,
        memo: "test".to_string(),
    };

    let canonical_bytes = canonical_json(&tx).expect("canonical_json should succeed");
    let canonical_str = String::from_utf8(canonical_bytes).expect("should be valid UTF-8");

    // Parse back to JSON to verify structure
    let parsed: serde_json::Value =
        serde_json::from_str(&canonical_str).expect("canonical JSON should be valid JSON");

    // Verify root level keys are sorted
    let root_keys: Vec<&str> = parsed
        .as_object()
        .unwrap()
        .keys()
        .map(|s| s.as_str())
        .collect();
    let mut sorted_root_keys = root_keys.clone();
    sorted_root_keys.sort();
    assert_eq!(
        root_keys, sorted_root_keys,
        "Root level keys should be sorted"
    );

    // Verify nested msg keys are sorted
    let msg = &parsed["msgs"][0];
    let msg_keys: Vec<&str> = msg
        .as_object()
        .unwrap()
        .keys()
        .map(|s| s.as_str())
        .collect();
    let mut sorted_msg_keys = msg_keys.clone();
    sorted_msg_keys.sort();
    assert_eq!(
        msg_keys, sorted_msg_keys,
        "Nested message keys should be sorted"
    );
}

#[test]
fn test_multiple_messages_canonical_json() {
    // Test transaction with multiple messages
    let tx = Tx {
        chain_id: "dyt-local-1".to_string(),
        nonce: 1,
        msgs: vec![
            Msg::Send {
                from: "addr1".to_string(),
                to: "addr2".to_string(),
                denom: "DGT".to_string(),
                amount: 100,
            },
            Msg::Send {
                from: "addr1".to_string(),
                to: "addr3".to_string(),
                denom: "DRT".to_string(),
                amount: 200,
            },
        ],
        fee: 1000,
        memo: "multi-send".to_string(),
    };

    let canonical_bytes = canonical_json(&tx).expect("canonical_json should succeed");
    let canonical_str = String::from_utf8(canonical_bytes).expect("should be valid UTF-8");

    // Verify both messages maintain sorted keys
    let parsed: serde_json::Value =
        serde_json::from_str(&canonical_str).expect("canonical JSON should be valid JSON");

    for (i, msg) in parsed["msgs"].as_array().unwrap().iter().enumerate() {
        let msg_keys: Vec<&str> = msg
            .as_object()
            .unwrap()
            .keys()
            .map(|s| s.as_str())
            .collect();
        let mut sorted_msg_keys = msg_keys.clone();
        sorted_msg_keys.sort();
        assert_eq!(
            msg_keys, sorted_msg_keys,
            "Message {} keys should be sorted",
            i
        );
    }
}
