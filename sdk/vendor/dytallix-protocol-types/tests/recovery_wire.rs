//! Binary-format tests use synthetic key/signature bytes, not signature evidence.
use dytallix_protocol_types::recovery::*;
use dytallix_protocol_types::recovery_wire::*;
use dytallix_protocol_types::sha3_256;

fn key(byte: u8) -> KeyIdentity {
    KeyIdentity {
        algorithm: "mldsa65".into(),
        public_key: vec![byte; 1952],
    }
}
fn key87(byte: u8) -> KeyIdentity {
    KeyIdentity {
        algorithm: "mldsa87".into(),
        public_key: vec![byte; 2592],
    }
}
fn signature(role: SignatureRole, key: KeyIdentity) -> RecoverySignature {
    let size = if key.algorithm == "mldsa65" {
        3309
    } else {
        4627
    };
    RecoverySignature {
        role,
        key,
        signature: vec![0x77; size],
    }
}
fn active() -> ActiveAuthorization {
    ActiveAuthorization {
        generation: 8,
        nonce: 9,
    }
}
fn auth() -> RecoveryAuthorization {
    RecoveryAuthorization {
        policy_version: 5,
        sequence: 6,
    }
}
fn policy_auth() -> PolicyAuthorization {
    PolicyAuthorization {
        policy_version: 5,
        sequence: 7,
    }
}
fn policy() -> RecoveryPolicy {
    RecoveryPolicy {
        threshold: 2,
        guardians: (1..=3)
            .map(|n| Guardian {
                key: key(n),
                control_group: format!("g{n}"),
            })
            .collect(),
    }
}
fn operation(kind: ActionKind) -> RecoveryOperation {
    RecoveryOperation {
        domain: RecoveryDomain {
            network: 3,
            chain_id: "fixture".into(),
            genesis_digest: [0x11; 32],
            account_id: [0x22; 32],
        },
        action: Action {
            submission_expiry: 123,
            kind,
        },
    }
}
fn unsigned() -> SignedRecovery {
    SignedRecovery {
        operation: operation(ActionKind::Cancel {
            recovery: auth(),
            request_id: [0x33; 32],
        }),
        signatures: vec![],
    }
}
fn action_offset() -> usize {
    WIRE_PREFIX.len() + 2 + 1 + 2 + 7 + 64 + 8
}

#[test]
fn golden_cancel_wire_has_exact_bytes_and_independent_digest() {
    // Independently assembled with fixed big-endian fields, not this codec.
    let expected = hex::decode(concat!(
        "445954414c4c49582f5245434f564552592d5749524500000103000766697874757265",
        "1111111111111111111111111111111111111111111111111111111111111111",
        "2222222222222222222222222222222222222222222222222222222222222222",
        "000000000000007b0500000000000000050000000000000006",
        "333333333333333333333333333333333333333333333333333333333333333300"
    ))
    .unwrap();
    let encoded = encode(&unsigned()).unwrap();
    assert_eq!(encoded, expected);
    assert_eq!(encoded.len(), 157);
    assert_eq!(
        hex::encode(sha3_256(&encoded)),
        "7f72dc779e9236ecfe37a3f27e3a24feae12d7f367a48ae246fa068f16bf1e43"
    );
    assert_eq!(decode(&expected).unwrap(), unsigned());
}

#[test]
fn golden_signing_domains_share_operation_bytes_and_bind_possession_key() {
    let op = unsigned().operation;
    let spending = signing_bytes(&op, SignatureRole::Operation, &key(0x44)).unwrap();
    let possession = signing_bytes(&op, SignatureRole::Possession, &key(0x44)).unwrap();
    assert_eq!(spending.len(), 161);
    assert_eq!(possession.len(), 2117);
    assert_eq!(
        hex::encode(sha3_256(&spending)),
        "b393b7197197770c477a5b36826f336859268d02a4cb7c2e5e5bcc55fde6266a"
    );
    assert_eq!(
        hex::encode(sha3_256(&possession)),
        "97020465a36025b6ddefe2c9c9c1bf1be320e24ab88d701ef9b5f4546980a2a3"
    );
    assert_ne!(spending, possession);
    assert_eq!(
        spending,
        signing_bytes(&op, SignatureRole::Operation, &key(0x45)).unwrap()
    );
    assert_eq!(
        spending,
        signing_bytes(&op, SignatureRole::Operation, &key87(0x45)).unwrap()
    );
    assert_ne!(
        possession,
        signing_bytes(&op, SignatureRole::Possession, &key(0x45)).unwrap()
    );
    let mut changed = op.clone();
    changed.domain.genesis_digest[0] ^= 1;
    assert_ne!(
        spending,
        signing_bytes(&changed, SignatureRole::Operation, &key(0x44)).unwrap()
    );
    changed = op.clone();
    changed.action.submission_expiry += 1;
    assert_ne!(
        spending,
        signing_bytes(&changed, SignatureRole::Operation, &key(0x44)).unwrap()
    );
}

#[test]
fn all_supported_actions_round_trip_with_both_algorithms_and_roles() {
    let kinds = vec![
        ActionKind::Enroll {
            active: active(),
            policy: policy(),
        },
        ActionKind::Rotate {
            active: active(),
            replacement: key87(5),
        },
        ActionKind::Start {
            recovery: auth(),
            request_id: [3; 32],
            replacement: key(4),
            timing_version: 7,
        },
        ActionKind::Finalize {
            recovery: auth(),
            request_id: [3; 32],
        },
        ActionKind::Cancel {
            recovery: auth(),
            request_id: [3; 32],
        },
        ActionKind::Resume {
            recovery: auth(),
            active_key: key87(5),
        },
        ActionKind::StagePolicy {
            active: active(),
            authorization: policy_auth(),
            update_id: [4; 32],
            policy: policy(),
            timing_version: 7,
        },
        ActionKind::ActivatePolicy {
            active: active(),
            authorization: policy_auth(),
            update_id: [4; 32],
        },
        ActionKind::CancelPolicy {
            authorization: policy_auth(),
            update_id: [4; 32],
        },
    ];
    for (index, kind) in kinds.into_iter().enumerate() {
        let value = SignedRecovery {
            operation: operation(kind),
            signatures: vec![
                signature(SignatureRole::Operation, key(1)),
                signature(SignatureRole::Operation, key87(2)),
                signature(SignatureRole::Possession, key(1)),
            ],
        };
        let bytes = encode(&value).unwrap();
        assert_eq!(bytes[action_offset()], index as u8 + 1);
        assert_eq!(decode(&bytes).unwrap(), value);
        assert_eq!(encode(&decode(&bytes).unwrap()).unwrap(), bytes);
    }
}

#[test]
fn spend_placeholder_has_no_network_encoding_or_signing_message() {
    let value = SignedRecovery {
        operation: operation(ActionKind::Spend { active: active() }),
        signatures: vec![],
    };
    assert!(encode(&value).is_err());
    assert!(signing_bytes(&value.operation, SignatureRole::Operation, &key(1)).is_err());
}

#[test]
fn every_truncated_prefix_and_trailing_bytes_are_rejected() {
    let bytes = encode(&unsigned()).unwrap();
    for length in 0..bytes.len() {
        assert!(decode(&bytes[..length]).is_err(), "length {length}");
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(decode(&trailing).is_err());
    assert!(decode(&vec![0; MAX_WIRE_BYTES + 1]).is_err());
}

#[test]
fn unknown_prefix_version_network_and_action_are_rejected() {
    let bytes = encode(&unsigned()).unwrap();
    for index in [
        0,
        WIRE_PREFIX.len() + 1,
        WIRE_PREFIX.len() + 2,
        action_offset(),
    ] {
        let mut changed = bytes.clone();
        changed[index] = 0xff;
        assert!(decode(&changed).is_err());
    }
}

#[test]
fn text_bounds_are_bytes_and_no_unicode_normalization_occurs() {
    let mut value = unsigned();
    for invalid in [String::new(), "x".repeat(129), "é".repeat(65)] {
        value.operation.domain.chain_id = invalid;
        assert!(encode(&value).is_err());
    }
    value.operation.domain.chain_id = "é".repeat(64);
    assert_eq!(decode(&encode(&value).unwrap()).unwrap(), value);
    let composed = signing_bytes(&value.operation, SignatureRole::Operation, &key(1)).unwrap();
    value.operation.domain.chain_id = "e\u{301}".repeat(32);
    assert_ne!(
        composed,
        signing_bytes(&value.operation, SignatureRole::Operation, &key(1)).unwrap()
    );
    let mut bytes = encode(&unsigned()).unwrap();
    let length_offset = WIRE_PREFIX.len() + 2 + 1;
    bytes[length_offset..length_offset + 2].copy_from_slice(&129u16.to_be_bytes());
    assert!(decode(&bytes).is_err());
    bytes = encode(&unsigned()).unwrap();
    bytes[length_offset + 2] = 0xff;
    assert!(decode(&bytes).is_err());
}

#[test]
fn guardian_order_duplicates_count_and_control_groups_are_checked() {
    let valid = policy();
    let mut cases = Vec::new();
    let mut p = valid.clone();
    p.guardians.swap(0, 1);
    cases.push(p);
    let mut p = valid.clone();
    p.guardians[1] = p.guardians[0].clone();
    cases.push(p);
    let mut p = valid.clone();
    p.guardians[1].control_group = p.guardians[0].control_group.clone();
    cases.push(p);
    let mut p = valid.clone();
    p.guardians.push(Guardian {
        key: key(4),
        control_group: "g4".into(),
    });
    cases.push(p);
    let mut p = valid.clone();
    p.guardians[0].control_group.clear();
    cases.push(p);
    let mut p = valid.clone();
    p.guardians[0].control_group = "x".repeat(129);
    cases.push(p);
    for p in cases {
        let value = SignedRecovery {
            operation: operation(ActionKind::Enroll {
                active: active(),
                policy: p,
            }),
            signatures: vec![],
        };
        assert!(encode(&value).is_err());
    }
    let value = SignedRecovery {
        operation: operation(ActionKind::Enroll {
            active: active(),
            policy: valid,
        }),
        signatures: vec![],
    };
    let mut bytes = encode(&value).unwrap();
    let start = action_offset() + 1 + 16 + 2 + 1;
    let size = 1 + 2 + 1952 + 2 + 2;
    let first = bytes[start..start + size].to_vec();
    let second = bytes[start + size..start + 2 * size].to_vec();
    bytes[start..start + size].copy_from_slice(&second);
    bytes[start + size..start + 2 * size].copy_from_slice(&first);
    assert!(decode(&bytes).is_err());
    let mut count = encode(&value).unwrap();
    count[action_offset() + 1 + 16 + 2] = 4;
    assert!(decode(&count).is_err());
}

#[test]
fn signature_order_duplicates_and_count_are_checked_on_encode_and_decode() {
    let mut value = unsigned();
    value.signatures = vec![
        signature(SignatureRole::Operation, key(1)),
        signature(SignatureRole::Operation, key(2)),
    ];
    let canonical = encode(&value).unwrap();
    value.signatures.swap(0, 1);
    assert!(encode(&value).is_err());
    value.signatures[1] = value.signatures[0].clone();
    assert!(encode(&value).is_err());
    value.signatures = (1..=8)
        .map(|n| signature(SignatureRole::Operation, key(n)))
        .collect();
    assert!(encode(&value).is_err());
    let start = encode(&unsigned()).unwrap().len();
    let size = 1 + 1 + 2 + 1952 + 2 + 3309;
    let mut reordered = canonical.clone();
    reordered[start..start + size].copy_from_slice(&canonical[start + size..start + 2 * size]);
    reordered[start + size..start + 2 * size].copy_from_slice(&canonical[start..start + size]);
    assert!(decode(&reordered).is_err());
    let mut duplicate = canonical.clone();
    duplicate[start + size..start + 2 * size].copy_from_slice(&canonical[start..start + size]);
    assert!(decode(&duplicate).is_err());
    let mut too_many = encode(&unsigned()).unwrap();
    *too_many.last_mut().unwrap() = 8;
    assert!(decode(&too_many).is_err());
}

#[test]
fn exact_algorithm_key_and_signature_lengths_are_required() {
    for bad in ["dilithium5", "ml_dsa_65", "MLDSA65", "legacy_dilithium5"] {
        let mut k = key(1);
        k.algorithm = bad.into();
        assert!(signing_bytes(&unsigned().operation, SignatureRole::Operation, &k).is_err());
    }
    let mut value = unsigned();
    value
        .signatures
        .push(signature(SignatureRole::Operation, key(1)));
    let canonical = encode(&value).unwrap();
    value.signatures[0].key.public_key.pop();
    assert!(encode(&value).is_err());
    value.signatures[0] = signature(SignatureRole::Operation, key(1));
    value.signatures[0].signature.pop();
    assert!(encode(&value).is_err());
    let start = encode(&unsigned()).unwrap().len();
    for index in [start, start + 1, start + 2, start + 1 + 1 + 2 + 1952] {
        let mut changed = canonical.clone();
        changed[index] = 0xff;
        assert!(decode(&changed).is_err());
    }
}

#[test]
fn maximum_signature_count_and_mixed_algorithm_guardians_round_trip() {
    let mut p = policy();
    p.guardians[2].key = key87(3);
    let value = SignedRecovery {
        operation: operation(ActionKind::Enroll {
            active: active(),
            policy: p,
        }),
        signatures: (1..=7)
            .map(|n| signature(SignatureRole::Operation, key87(n)))
            .collect(),
    };
    let bytes = encode(&value).unwrap();
    assert!(bytes.len() < MAX_WIRE_BYTES);
    assert_eq!(decode(&bytes).unwrap(), value);
}

#[test]
fn codec_is_not_the_threshold_or_signature_verifier() {
    let mut p = policy();
    p.threshold = 1;
    let value = SignedRecovery {
        operation: operation(ActionKind::Enroll {
            active: active(),
            policy: p,
        }),
        signatures: vec![],
    };
    assert_eq!(decode(&encode(&value).unwrap()).unwrap(), value);
    // State validation rejects this threshold; binary parsing alone grants no authority.
}
