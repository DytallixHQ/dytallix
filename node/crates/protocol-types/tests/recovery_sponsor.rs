//! Codec tests use synthetic key/signature bytes; they do not prove authority.
use dytallix_protocol_types::recovery::*;
use dytallix_protocol_types::recovery_sponsor::*;
use dytallix_protocol_types::recovery_wire::{
    self, RecoveryOperation, RecoverySignature, SignatureRole, SignedRecovery,
};
use std::collections::BTreeMap;
fn profile() -> FeeProfile {
    FeeProfile {
        version: 1,
        activation_height: 10,
        denomination: "udrt".into(),
        gas_price: 2,
        minimum_gas: 3,
        max_transaction_gas: 10000,
        max_block_gas: 100000,
        max_block_recovery_bytes: 1000000,
        max_block_recovery_signatures: 100,
        max_fee_cap: 100000,
        max_pending_accounts: 100,
        max_due_expiry_events_per_height: 10,
        mandatory_expiry_gas_budget: 1000,
        expiry_event_gas_cost: 10,
        action_costs: [1, 2, 3, 4, 5, 6, 7, 8, 9],
        wire_byte_cost: 1,
        read_byte_cost: 2,
        write_byte_cost: 3,
        signature_costs: BTreeMap::from([("mldsa65".into(), 50), ("mldsa87".into(), 70)]),
    }
}
fn fixture() -> SponsoredRecovery {
    let key = KeyIdentity {
        algorithm: "mldsa65".into(),
        public_key: vec![7; 1952],
    };
    let recovery = SignedRecovery {
        operation: RecoveryOperation {
            domain: RecoveryDomain {
                network: 1,
                chain_id: "local".into(),
                genesis_digest: [1; 32],
                account_id: [2; 32],
            },
            action: Action {
                submission_expiry: 100,
                kind: ActionKind::Finalize {
                    recovery: RecoveryAuthorization {
                        policy_version: 3,
                        sequence: 4,
                    },
                    request_id: [5; 32],
                },
            },
        },
        signatures: vec![RecoverySignature {
            role: SignatureRole::Operation,
            key: key.clone(),
            signature: vec![0x77; 3309],
        }],
    };
    let sponsor = SponsorAuthorization {
        domain: recovery.operation.domain.clone(),
        recovery_version: 1,
        operation_id: operation_id(&recovery.operation).unwrap(),
        signer_manifest_digest: signer_manifest_digest(&recovery).unwrap(),
        sponsor_account_id: [9; 32],
        sponsor_generation: 6,
        sponsor_nonce: 7,
        sponsor_key: key,
        fee_profile_version: 1,
        fee_profile_digest: profile_digest(&profile()).unwrap(),
        denomination: "udrt".into(),
        maximum_charge: 20000,
        gas_limit: 10000,
        expiry_height: 99,
    };
    SponsoredRecovery {
        recovery,
        sponsor,
        signature: vec![0x88; 3309],
    }
}
#[test]
fn canonical_golden_vectors_and_roundtrips() {
    // Expected values come from an independent explicit Python big-endian encoder.
    let value = fixture();
    assert_eq!(
        hex::encode(operation_id(&value.recovery.operation).unwrap()),
        "e7de6f5995902c6a5b202ef18b5d2cb6a70b8ab07cb7e5b394de1c2e1cd3f207"
    );
    assert_eq!(
        hex::encode(signer_manifest_digest(&value.recovery).unwrap()),
        "b535cdc6ff681dfa6cf3260235a9ba23a06356d5972e6090a5a5ad108ef6ae31"
    );
    assert_eq!(
        hex::encode(profile_digest(&profile()).unwrap()),
        "b0e6d635cc34cf5475f208a105e4d26da8b5fa666327ec3ead850ac7ebea9b19"
    );
    assert_eq!(
        hex::encode(authorization_id(&value.sponsor).unwrap()),
        "e2269b160d682d5cd27109af54f407c28deb067a7f3811576b1f5e07407b44e8"
    );
    assert_eq!(
        hex::encode(envelope_hash(&value).unwrap()),
        "0c09747037907551aa0450ac98baf00d95e37e0f0298c2452c7de98b7ef8bcc3"
    );
    let bytes = encode(&value).unwrap();
    assert_eq!(bytes.len(), 11018);
    assert_eq!(decode(&bytes).unwrap(), value);
    let bytes = sponsor_signing_bytes(&value.sponsor).unwrap();
    assert_eq!(bytes.len(), 2247);
    assert_eq!(decode_sponsor(&bytes).unwrap(), value.sponsor);
    let bytes = profile_bytes(&profile()).unwrap();
    assert_eq!(bytes.len(), 265);
    assert_eq!(decode_profile(&bytes).unwrap(), profile());
}
#[test]
fn randomized_signatures_do_not_change_intent_manifest_or_authorization() {
    let first = fixture();
    let mut second = first.clone();
    second.signature[0] ^= 1;
    second.recovery.signatures[0].signature[0] ^= 1;
    assert_eq!(
        operation_id(&first.recovery.operation),
        operation_id(&second.recovery.operation)
    );
    assert_eq!(
        signer_manifest_digest(&first.recovery),
        signer_manifest_digest(&second.recovery)
    );
    assert_eq!(
        authorization_id(&first.sponsor),
        authorization_id(&second.sponsor)
    );
    assert_ne!(
        envelope_hash(&first).unwrap(),
        envelope_hash(&second).unwrap()
    );
}
#[test]
fn exact_sponsor_fields_are_committed() {
    let original = fixture().sponsor;
    let expected = authorization_id(&original).unwrap();
    let mut variants = Vec::new();
    macro_rules! change {
        ($field:ident,$value:expr) => {{
            let mut s = original.clone();
            s.$field = $value;
            variants.push(s);
        }};
    }
    change!(operation_id, [8; 32]);
    change!(signer_manifest_digest, [8; 32]);
    change!(sponsor_account_id, [8; 32]);
    change!(sponsor_generation, 8);
    change!(sponsor_nonce, 8);
    change!(fee_profile_version, 8);
    change!(fee_profile_digest, [8; 32]);
    change!(maximum_charge, 20001);
    change!(gas_limit, 9999);
    change!(expiry_height, 98);
    let mut s = original.clone();
    s.domain.network = 2;
    variants.push(s);
    let mut s = original.clone();
    s.domain.chain_id = "other".into();
    variants.push(s);
    let mut s = original.clone();
    s.domain.genesis_digest = [8; 32];
    variants.push(s);
    let mut s = original.clone();
    s.domain.account_id = [8; 32];
    variants.push(s);
    let mut s = original.clone();
    s.sponsor_key.public_key[0] ^= 1;
    variants.push(s);
    for s in variants {
        assert_ne!(authorization_id(&s).unwrap(), expected);
    }
}
#[test]
fn envelope_requires_exact_intent_domain_and_signer_manifest() {
    let original = fixture();
    let mut value = original.clone();
    value.sponsor.domain.account_id[0] ^= 1;
    assert!(encode(&value).is_err());
    let mut value = original.clone();
    value.sponsor.operation_id[0] ^= 1;
    assert!(encode(&value).is_err());
    let mut value = original.clone();
    value.sponsor.signer_manifest_digest[0] ^= 1;
    assert!(encode(&value).is_err());
    let mut value = original.clone();
    value.recovery.signatures[0].role = SignatureRole::Possession;
    assert!(encode(&value).is_err());
    let mut value = original;
    value.recovery.operation.action.submission_expiry += 1;
    assert!(encode(&value).is_err());
}
#[test]
fn bounded_decoders_reject_truncation_trailing_bytes_and_lengths() {
    let bytes = encode(&fixture()).unwrap();
    for end in [
        0,
        1,
        WIRE_PREFIX.len(),
        WIRE_PREFIX.len() + 2,
        bytes.len() - 1,
    ] {
        assert!(decode(&bytes[..end]).is_err());
    }
    let mut altered = bytes.clone();
    altered.push(0);
    assert!(decode(&altered).is_err());
    let mut altered = bytes;
    let offset = WIRE_PREFIX.len() + 2;
    altered[offset..offset + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(decode(&altered).is_err());
    assert!(decode(&vec![0; MAX_WIRE_BYTES + 1]).is_err());
    assert!(decode_sponsor(&vec![0; MAX_SPONSOR_BYTES + 1]).is_err());
    let mut bytes = profile_bytes(&profile()).unwrap();
    bytes.push(0);
    assert!(decode_profile(&bytes).is_err());
}
#[test]
fn exact_algorithms_versions_denominations_and_signature_lengths() {
    let original = fixture();
    for name in ["dilithium3", "dilithium5", "mock-blake3", "MLDSA65", ""] {
        let mut value = original.clone();
        value.sponsor.sponsor_key.algorithm = name.into();
        assert!(encode(&value).is_err());
    }
    let mut value = original.clone();
    value.sponsor.recovery_version = 2;
    assert!(encode(&value).is_err());
    for denomination in ["UDRT", "udgt", "u drt", ""] {
        let mut value = original.clone();
        value.sponsor.denomination = denomination.into();
        assert!(encode(&value).is_err());
    }
    let mut value = original.clone();
    value.signature.pop();
    assert!(encode(&value).is_err());
    let mut value = original;
    value.sponsor.sponsor_key.public_key.pop();
    assert!(encode(&value).is_err());
}
#[test]
fn mldsa87_roundtrip_and_signer_order_checks() {
    let mut value = fixture();
    value.sponsor.sponsor_key = KeyIdentity {
        algorithm: "mldsa87".into(),
        public_key: vec![9; 2592],
    };
    value.signature = vec![8; 4627];
    assert_eq!(decode(&encode(&value).unwrap()).unwrap(), value);
    let duplicate = value.recovery.signatures[0].clone();
    value.recovery.signatures.push(duplicate);
    assert!(signer_manifest_digest(&value.recovery).is_err());
}
#[test]
fn profile_requires_explicit_consistent_capacity_and_exact_cost_codes() {
    let mut p = profile();
    p.gas_price = 0;
    assert!(p.validate().is_err());
    let mut p = profile();
    p.minimum_gas = p.max_transaction_gas + 1;
    assert!(p.validate().is_err());
    let mut p = profile();
    p.max_block_gas = p.max_transaction_gas - 1;
    assert!(p.validate().is_err());
    let mut p = profile();
    p.max_pending_accounts = 0;
    assert!(p.validate().is_err());
    let mut p = profile();
    p.signature_costs.clear();
    assert!(p.validate().is_err());
    let mut p = profile();
    p.signature_costs = BTreeMap::from([("dilithium3".into(), 1)]);
    assert!(p.validate().is_err());
    let mut p = profile();
    p.action_costs = [0; 9];
    p.signature_costs.insert("mldsa65".into(), 0);
    p.minimum_gas = 0;
    assert!(p.validate().is_ok());
    let bytes = profile_bytes(&profile()).unwrap();
    let tail = bytes.len() - 19;
    let mut changed = bytes.clone();
    changed[tail + 1] = 2;
    assert!(decode_profile(&changed).is_err());
    let mut changed = bytes;
    changed[tail + 10] = 3;
    assert!(decode_profile(&changed).is_err());
    let mut json = serde_json::to_value(profile()).unwrap();
    json.as_object_mut().unwrap().remove("wire_byte_cost");
    assert!(serde_json::from_value::<FeeProfile>(json).is_err());
    let mut json = serde_json::to_value(profile()).unwrap();
    json["unknown"] = true.into();
    assert!(serde_json::from_value::<FeeProfile>(json).is_err());
}
#[test]
fn operation_helper_preserves_existing_signing_message_and_rejects_spend() {
    let mut value = fixture();
    let key = &value.sponsor.sponsor_key;
    let original =
        recovery_wire::signing_bytes(&value.recovery.operation, SignatureRole::Operation, key)
            .unwrap();
    let bytes = recovery_wire::operation_bytes(&value.recovery.operation).unwrap();
    assert_eq!(
        &original[recovery_wire::OPERATION_PREFIX.len() + 2..],
        bytes
    );
    value.recovery.operation.action.kind = ActionKind::Spend {
        active: ActiveAuthorization {
            generation: 1,
            nonce: 1,
        },
    };
    assert!(operation_id(&value.recovery.operation).is_err());
}

// RF-T01 independent vectors use the contract's fixed field offsets. The hex and
// SHA3 constants were calculated outside the Rust codec with Python struct/hashlib.
// No production encoder or digest helper constructs an expected byte sequence.
fn independent_profile_vector() -> Vec<u8> {
    hex::decode("445954414c4c49582f5245434f564552592d4645452d50524f46494c450000010000000000000001000000000000000a00047564727400000000000000020000000000000003000000000000271000000000000186a000000000000f42400000000000000064000000000000000000000000000186a00000000000000064000000000000000a00000000000003e8000000000000000a00000000000000010000000000000002000000000000000300000000000000040000000000000005000000000000000600000000000000070000000000000008000000000000000900000000000000010000000000000002000000000000000302010000000000000032020000000000000046").unwrap()
}
fn independent_sponsor_vector() -> Vec<u8> {
    let mut bytes = hex::decode("445954414c4c49582f5245434f564552592d53504f4e534f520000010100056c6f63616c010101010101010101010101010101010101010101010101010101010101010102020202020202020202020202020202020202020202020202020202020202020001e7de6f5995902c6a5b202ef18b5d2cb6a70b8ab07cb7e5b394de1c2e1cd3f207b535cdc6ff681dfa6cf3260235a9ba23a06356d5972e6090a5a5ad108ef6ae310909090909090909090909090909090909090909090909090909090909090909000000000000000600000000000000070107a0").unwrap();
    bytes.extend_from_slice(&[7; 1952]);
    bytes.extend_from_slice(&hex::decode("0000000000000001b0e6d635cc34cf5475f208a105e4d26da8b5fa666327ec3ead850ac7ebea9b1900047564727400000000000000000000000000004e2000000000000027100000000000000063").unwrap());
    bytes
}
#[test]
fn every_fee_profile_field_has_independent_canonical_bytes_and_hash() {
    let original = independent_profile_vector();
    assert_eq!(profile_bytes(&profile()).unwrap(), original);
    let cases: &[(&str, fn(&mut FeeProfile), usize, usize, &str, &str)] = &[
        (
            "version",
            |p| {
                p.version = 2;
            },
            32,
            8,
            "0000000000000002",
            "7993e30b8beac70f61c6837ffc502dc36411c7d37a15cf9c8a325dfcc473a495",
        ),
        (
            "activation_height",
            |p| {
                p.activation_height = 11;
            },
            40,
            8,
            "000000000000000b",
            "05a2a9edb9caced4abefd964cdd7da0d9f378e4da4b19501a6e3536c7f626222",
        ),
        (
            "gas_price",
            |p| {
                p.gas_price = 3;
            },
            54,
            8,
            "0000000000000003",
            "da646962bab66430aadb868f151d65df82f1f7035222f4d036c2dfcf3c70e8d2",
        ),
        (
            "minimum_gas",
            |p| {
                p.minimum_gas = 4;
            },
            62,
            8,
            "0000000000000004",
            "fa743c1e2f82fb5ec82034823598dbfbc1d03538be448adabb9400e1c21f4cf7",
        ),
        (
            "max_transaction_gas",
            |p| {
                p.max_transaction_gas = 10001;
            },
            70,
            8,
            "0000000000002711",
            "55c17f2dcb20374db20a06ca382223f56e86544a691560e85a65b99099a84e7f",
        ),
        (
            "max_block_gas",
            |p| {
                p.max_block_gas = 100001;
            },
            78,
            8,
            "00000000000186a1",
            "ecbc0fb61be53735e71371db5f981363a18320190b29299f01499d35141e0ca3",
        ),
        (
            "max_block_recovery_bytes",
            |p| {
                p.max_block_recovery_bytes = 1000001;
            },
            86,
            8,
            "00000000000f4241",
            "042da871c66d54d35839d540b4da27d62f9158f0a0e546df18968e38fb37b0c4",
        ),
        (
            "max_block_recovery_signatures",
            |p| {
                p.max_block_recovery_signatures = 101;
            },
            94,
            8,
            "0000000000000065",
            "266dc4f60674da660f2631d45e6d6954936ea73db1995aa17b5b38a51f4c196d",
        ),
        (
            "max_fee_cap",
            |p| {
                p.max_fee_cap = 170141183460469231731687303715884105737;
            },
            102,
            16,
            "80000000000000000000000000000009",
            "15acce1ef8bc021829df9a32af133336ad59a85453fbff294f3ee7e5eecfbd34",
        ),
        (
            "max_pending_accounts",
            |p| {
                p.max_pending_accounts = 101;
            },
            118,
            8,
            "0000000000000065",
            "3cd4dccc6c634f90b1fa215f9c396cc4266bb8bbe00caa4d080481b9cb7a0c97",
        ),
        (
            "max_due_expiry_events_per_height",
            |p| {
                p.max_due_expiry_events_per_height = 11;
            },
            126,
            8,
            "000000000000000b",
            "2e8d95608928bdcca26c6bb4cc51c63da75f022e1fffb709738e33682faf59c2",
        ),
        (
            "mandatory_expiry_gas_budget",
            |p| {
                p.mandatory_expiry_gas_budget = 1001;
            },
            134,
            8,
            "00000000000003e9",
            "734dc571f8440e61f3aba8971186f1b7028aeddedeb401b4528da2498c48b7f5",
        ),
        (
            "expiry_event_gas_cost",
            |p| {
                p.expiry_event_gas_cost = 11;
            },
            142,
            8,
            "000000000000000b",
            "20b20313c0f0d5c771905eed3d703d16f710e3e9c1433f1880689830e79f959b",
        ),
        (
            "action_costs[0]",
            |p| {
                p.action_costs[0] = 2;
            },
            150,
            8,
            "0000000000000002",
            "326c07554b781a8f40dd8366f6c411d5de494795e92a3791ab75ab1835591c84",
        ),
        (
            "action_costs[1]",
            |p| {
                p.action_costs[1] = 3;
            },
            158,
            8,
            "0000000000000003",
            "d69ca22b767a43d18b7dd80b36305a72080279a1af19236270b92d7347d1dc71",
        ),
        (
            "action_costs[2]",
            |p| {
                p.action_costs[2] = 4;
            },
            166,
            8,
            "0000000000000004",
            "531d07d62c3ca96f789a4d4216500a16a2ce8aa21fb3dc1a8d230d22ddccbe09",
        ),
        (
            "action_costs[3]",
            |p| {
                p.action_costs[3] = 5;
            },
            174,
            8,
            "0000000000000005",
            "f11b48bc131d80814f63580a755f7d2f4b0f885c9fb69376d970733dba9dfadf",
        ),
        (
            "action_costs[4]",
            |p| {
                p.action_costs[4] = 6;
            },
            182,
            8,
            "0000000000000006",
            "94d1701ae249c4c1e49dc84581270ed52830f230ba4b92b28904af477c0c0109",
        ),
        (
            "action_costs[5]",
            |p| {
                p.action_costs[5] = 7;
            },
            190,
            8,
            "0000000000000007",
            "bc11629f8a9c9a551c336c9435d1a8fefe51c7d3ab639fcd5fc2adc70f682bf5",
        ),
        (
            "action_costs[6]",
            |p| {
                p.action_costs[6] = 8;
            },
            198,
            8,
            "0000000000000008",
            "cd9078f92e7429d860aca96216afb3b308a12b8b2c4386f4f313893f9d1467fa",
        ),
        (
            "action_costs[7]",
            |p| {
                p.action_costs[7] = 9;
            },
            206,
            8,
            "0000000000000009",
            "c455d8c0328ebb8284d38e29dcd2e3ae7816f30ce0d08e51a4949bd02a42fe51",
        ),
        (
            "action_costs[8]",
            |p| {
                p.action_costs[8] = 10;
            },
            214,
            8,
            "000000000000000a",
            "a1f42af779035fdd4d15f46af5458663b90a407393d383471971eb04a5f859c4",
        ),
        (
            "wire_byte_cost",
            |p| {
                p.wire_byte_cost = 2;
            },
            222,
            8,
            "0000000000000002",
            "162eec92736fc227869e6d238f5347a38b1ae6580cc8eb5c4b763459e183d01b",
        ),
        (
            "read_byte_cost",
            |p| {
                p.read_byte_cost = 3;
            },
            230,
            8,
            "0000000000000003",
            "34895bebecad4890b19be9b146590393799c08daf1beb440e7bd2d67903d1786",
        ),
        (
            "write_byte_cost",
            |p| {
                p.write_byte_cost = 4;
            },
            238,
            8,
            "0000000000000004",
            "13bcc7f985a6b8d5e294ee161f20739107d30aefa57693fb5a62d5c4a0af210f",
        ),
        (
            "mldsa65_cost",
            |p| {
                p.signature_costs.insert("mldsa65".into(), 51);
            },
            248,
            8,
            "0000000000000033",
            "15061d35d29bd9756a5f7e29a9ab60140d67ce05e2af479f414ada452776a3ea",
        ),
        (
            "mldsa87_cost",
            |p| {
                p.signature_costs.insert("mldsa87".into(), 71);
            },
            257,
            8,
            "0000000000000047",
            "556f84b61caff30f2961103fb7d380d1edfe0b9e52cb8b4dd635329b2c454d0e",
        ),
    ];
    for (name, change, offset, length, replacement, digest) in cases {
        let mut value = profile();
        change(&mut value);
        let mut expected = original.clone();
        expected.splice(*offset..offset + length, hex::decode(replacement).unwrap());
        assert_eq!(profile_bytes(&value).unwrap(), expected, "{name}");
        assert_eq!(
            hex::encode(profile_digest(&value).unwrap()),
            *digest,
            "{name}"
        );
        assert_eq!(decode_profile(&expected).unwrap(), value, "{name}");
    }
}
#[test]
fn every_sponsor_field_has_independent_canonical_bytes_and_hash() {
    let original = independent_sponsor_vector();
    assert_eq!(sponsor_signing_bytes(&fixture().sponsor).unwrap(), original);
    let cases: &[(
        &str,
        fn(&mut SponsorAuthorization),
        usize,
        usize,
        &str,
        &str,
    )] = &[
        (
            "domain.network",
            |p| {
                p.domain.network = 2;
            },
            28,
            1,
            "02",
            "15302bd33f02d565ddf3199f6bbbea65e4a91e4c6a79ae820440fa74dd891434",
        ),
        (
            "domain.chain_id",
            |p| {
                p.domain.chain_id = "different-chain".into();
            },
            29,
            7,
            "000f646966666572656e742d636861696e",
            "cdfd1ad9a6ea84fa46c4a28fb838b05bbffbf38f447d7e909c48ab3dce2a9378",
        ),
        (
            "domain.genesis_digest",
            |p| {
                p.domain.genesis_digest = [8; 32];
            },
            36,
            32,
            "0808080808080808080808080808080808080808080808080808080808080808",
            "ce4fd36da4972881ac2a1679120e80bebc0deacfbb7f53fd6921db31a74d0911",
        ),
        (
            "domain.account_id",
            |p| {
                p.domain.account_id = [8; 32];
            },
            68,
            32,
            "0808080808080808080808080808080808080808080808080808080808080808",
            "4c4f9207ec04679e657a881b95b02ee56690d63f859d1ebd3afa1c8acde35c60",
        ),
        (
            "operation_id",
            |p| {
                p.operation_id = [8; 32];
            },
            102,
            32,
            "0808080808080808080808080808080808080808080808080808080808080808",
            "ecfff2c10ec7d8c6886f5beb561e914cba20131b7511a17d4e6da9818105b5e8",
        ),
        (
            "signer_manifest_digest",
            |p| {
                p.signer_manifest_digest = [8; 32];
            },
            134,
            32,
            "0808080808080808080808080808080808080808080808080808080808080808",
            "f14334562acc369bb37dbbea7bb78384f65fb6aa369ebd75f95496267e83f985",
        ),
        (
            "sponsor_account_id",
            |p| {
                p.sponsor_account_id = [8; 32];
            },
            166,
            32,
            "0808080808080808080808080808080808080808080808080808080808080808",
            "363f6c089b4c84c166abcab915c8d8fd9835594909fc5fb196b92208f2b2c961",
        ),
        (
            "sponsor_generation",
            |p| {
                p.sponsor_generation = 7;
            },
            198,
            8,
            "0000000000000007",
            "6d8dbfd0172e93cca3e69a0c58e92610a9e95c8982a9ada87c4d62c41c831388",
        ),
        (
            "sponsor_nonce",
            |p| {
                p.sponsor_nonce = 8;
            },
            206,
            8,
            "0000000000000008",
            "b90ad3714c2a4a7d856cf8ba542a25158c8257eb157d3878dd7fe489929b0200",
        ),
        (
            "fee_profile_version",
            |p| {
                p.fee_profile_version = 2;
            },
            2169,
            8,
            "0000000000000002",
            "28ca5254fa81f8e3f55a045f35815d83e33f37b22cb83ac5c1b0369c3f655d51",
        ),
        (
            "fee_profile_digest",
            |p| {
                p.fee_profile_digest = [8; 32];
            },
            2177,
            32,
            "0808080808080808080808080808080808080808080808080808080808080808",
            "8aea14a0523d5ab980e0eef0beed45586a68b3e5f7a12ee1b829249c113e3dee",
        ),
        (
            "maximum_charge",
            |p| {
                p.maximum_charge = 170141183460469231731687303715884105738;
            },
            2215,
            16,
            "8000000000000000000000000000000a",
            "9249b780d92f9ed5652833b84830f720b8c637b1b72f376b4bd5a02591255eba",
        ),
        (
            "gas_limit",
            |p| {
                p.gas_limit = 10001;
            },
            2231,
            8,
            "0000000000002711",
            "986b792c7a35d88dcdc86a20bf1e99df1e2ef83a30fc44dfb54746f1ae9a2c87",
        ),
        (
            "expiry_height",
            |p| {
                p.expiry_height = 100;
            },
            2239,
            8,
            "0000000000000064",
            "11bc62fe5d6a535a5956e990dc71e00ff8893fb67b7e848d4b9912ddcea57920",
        ),
    ];
    for (name, change, offset, length, replacement, digest) in cases {
        let mut value = fixture().sponsor;
        change(&mut value);
        let mut expected = original.clone();
        expected.splice(*offset..offset + length, hex::decode(replacement).unwrap());
        assert_eq!(sponsor_signing_bytes(&value).unwrap(), expected, "{name}");
        assert_eq!(
            hex::encode(authorization_id(&value).unwrap()),
            *digest,
            "{name}"
        );
        assert_eq!(decode_sponsor(&expected).unwrap(), value, "{name}");
    }
    // Algorithm code, key length and bytes change together to a valid ML-DSA-87 key.
    let mut value = fixture().sponsor;
    value.sponsor_key = KeyIdentity {
        algorithm: "mldsa87".into(),
        public_key: vec![8; 2592],
    };
    let mut expected = original.clone();
    let mut key_bytes = vec![2, 10, 32];
    key_bytes.extend_from_slice(&[8; 2592]);
    expected.splice(214..2169, key_bytes);
    assert_eq!(sponsor_signing_bytes(&value).unwrap(), expected);
    assert_eq!(
        hex::encode(authorization_id(&value).unwrap()),
        "3863e0391194cf174dd9e81c2b3d484a8de86cc09fb5e9f7530ab024b5b25ded"
    );
    assert_eq!(decode_sponsor(&expected).unwrap(), value);
    // Changing key bytes without changing its algorithm also changes authorization.
    let mut value = fixture().sponsor;
    value.sponsor_key.public_key[0] = 8;
    let mut expected = original;
    expected[217] = 8;
    assert_eq!(sponsor_signing_bytes(&value).unwrap(), expected);
    assert_eq!(
        hex::encode(authorization_id(&value).unwrap()),
        "92de3a2975c8f2947e5bc119b91dba2f0f66722f634cb701534d0a81a9a29b71"
    );
}

#[test]
fn fee_profile_cost_map_order_has_one_canonical_encoding() {
    let canonical = independent_profile_vector();
    let mut reverse_insert = profile();
    reverse_insert.signature_costs.clear();
    reverse_insert.signature_costs.insert("mldsa87".into(), 70);
    reverse_insert.signature_costs.insert("mldsa65".into(), 50);
    assert_eq!(profile_bytes(&reverse_insert).unwrap(), canonical);
    assert_eq!(
        profile_digest(&reverse_insert).unwrap(),
        profile_digest(&profile()).unwrap()
    );
    // Reordering encoded entries is rejected. The decoder must not normalize it.
    let mut reordered = canonical.clone();
    reordered[247..256].copy_from_slice(&canonical[256..265]);
    reordered[256..265].copy_from_slice(&canonical[247..256]);
    assert!(decode_profile(&reordered).is_err());
    for unknown in [0, 3, 255] {
        let mut bytes = canonical.clone();
        bytes[247] = unknown;
        assert!(decode_profile(&bytes).is_err());
    }
    let mut duplicate = canonical.clone();
    duplicate[256] = 1;
    assert!(decode_profile(&duplicate).is_err());
    for count in [0, 3, 255] {
        let mut bytes = canonical.clone();
        bytes[246] = count;
        assert!(decode_profile(&bytes).is_err());
    }
    for (retained, expected, digest) in [
        ("mldsa65", "445954414c4c49582f5245434f564552592d4645452d50524f46494c450000010000000000000001000000000000000a00047564727400000000000000020000000000000003000000000000271000000000000186a000000000000f42400000000000000064000000000000000000000000000186a00000000000000064000000000000000a00000000000003e8000000000000000a00000000000000010000000000000002000000000000000300000000000000040000000000000005000000000000000600000000000000070000000000000008000000000000000900000000000000010000000000000002000000000000000301010000000000000032", "e0c0f4427077c2e8783c94beb7b71f8b24174a71f6db5dacbc008bcc27d3b44c"),
        ("mldsa87", "445954414c4c49582f5245434f564552592d4645452d50524f46494c450000010000000000000001000000000000000a00047564727400000000000000020000000000000003000000000000271000000000000186a000000000000f42400000000000000064000000000000000000000000000186a00000000000000064000000000000000a00000000000003e8000000000000000a00000000000000010000000000000002000000000000000300000000000000040000000000000005000000000000000600000000000000070000000000000008000000000000000900000000000000010000000000000002000000000000000301020000000000000046", "a55e79567760c24310e7cb338400677eb7e6584e473e53359a25b8a15822c31d"),
    ] {
        let mut value = profile(); value.signature_costs.retain(|name,_| name == retained);
        let expected = hex::decode(expected).unwrap();
        assert_eq!(profile_bytes(&value).unwrap(), expected);
        assert_eq!(hex::encode(profile_digest(&value).unwrap()), digest);
        assert_eq!(decode_profile(&expected).unwrap(), value);
    }
}

#[test]
fn fixed_version_and_denomination_fields_reject_unselected_values() {
    let sponsor = independent_sponsor_vector();
    let profile = independent_profile_vector();
    for version in [0u16, 2, u16::MAX] {
        let mut bytes = sponsor.clone();
        bytes[100..102].copy_from_slice(&version.to_be_bytes());
        assert!(decode_sponsor(&bytes).is_err());
        let mut value = fixture().sponsor;
        value.recovery_version = version;
        assert!(sponsor_signing_bytes(&value).is_err());
        let mut bytes = sponsor.clone();
        bytes[SPONSOR_PREFIX.len()..SPONSOR_PREFIX.len() + 2]
            .copy_from_slice(&version.to_be_bytes());
        assert!(decode_sponsor(&bytes).is_err());
        let mut bytes = profile.clone();
        bytes[PROFILE_PREFIX.len()..PROFILE_PREFIX.len() + 2]
            .copy_from_slice(&version.to_be_bytes());
        assert!(decode_profile(&bytes).is_err());
    }
    for denomination in ["", "UDRT", "udgt", "u drt", "udrtx", "abcdefghijklmnopq"] {
        let mut value = fixture().sponsor;
        value.denomination = denomination.into();
        assert!(sponsor_signing_bytes(&value).is_err());
        let mut value = crate::profile();
        value.denomination = denomination.into();
        assert!(profile_bytes(&value).is_err());
    }
    // A syntactically valid four-byte alternate denomination is still unselected.
    let mut bytes = sponsor;
    bytes[2211..2215].copy_from_slice(b"udgt");
    assert!(decode_sponsor(&bytes).is_err());
    let mut bytes = profile;
    bytes[50..54].copy_from_slice(b"udgt");
    assert!(decode_profile(&bytes).is_err());
}

#[test]
fn every_required_profile_and_sponsor_field_rejects_omission_or_null() {
    let profile_json = serde_json::to_value(profile()).unwrap();
    for field in profile_json.as_object().unwrap().keys() {
        let mut missing = profile_json.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<FeeProfile>(missing).is_err(),
            "missing {field}"
        );
        let mut null = profile_json.clone();
        null[field] = serde_json::Value::Null;
        assert!(
            serde_json::from_value::<FeeProfile>(null).is_err(),
            "null {field}"
        );
    }
    let sponsor_json = serde_json::to_value(fixture().sponsor).unwrap();
    for field in sponsor_json.as_object().unwrap().keys() {
        let mut missing = sponsor_json.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<SponsorAuthorization>(missing).is_err(),
            "missing {field}"
        );
        let mut null = sponsor_json.clone();
        null[field] = serde_json::Value::Null;
        assert!(
            serde_json::from_value::<SponsorAuthorization>(null).is_err(),
            "null {field}"
        );
    }
    for container in ["domain", "sponsor_key"] {
        for field in sponsor_json[container].as_object().unwrap().keys() {
            let mut missing = sponsor_json.clone();
            missing[container].as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<SponsorAuthorization>(missing).is_err(),
                "missing {container}.{field}"
            );
        }
        let mut unknown = sponsor_json.clone();
        unknown[container]["unknown"] = true.into();
        assert!(
            serde_json::from_value::<SponsorAuthorization>(unknown).is_err(),
            "unknown {container} field"
        );
    }
    for length in [0, 8, 10] {
        let mut value = profile_json.clone();
        value["action_costs"] = serde_json::json!(vec![0; length]);
        assert!(
            serde_json::from_value::<FeeProfile>(value).is_err(),
            "action count {length}"
        );
    }
}

#[test]
fn required_profile_capacities_reject_zero_and_preserve_explicit_zero_costs() {
    let cases: &[(&str, fn(&mut FeeProfile))] = &[
        ("gas_price", |p| p.gas_price = 0),
        ("max_transaction_gas", |p| p.max_transaction_gas = 0),
        ("max_block_gas", |p| p.max_block_gas = 0),
        ("max_block_recovery_bytes", |p| {
            p.max_block_recovery_bytes = 0
        }),
        ("max_block_recovery_signatures", |p| {
            p.max_block_recovery_signatures = 0
        }),
        ("max_fee_cap", |p| p.max_fee_cap = 0),
        ("max_pending_accounts", |p| p.max_pending_accounts = 0),
        ("max_due_expiry_events_per_height", |p| {
            p.max_due_expiry_events_per_height = 0
        }),
        ("mandatory_expiry_gas_budget", |p| {
            p.mandatory_expiry_gas_budget = 0
        }),
    ];
    for (name, change) in cases {
        let mut value = profile();
        change(&mut value);
        assert!(value.validate().is_err(), "{name}");
        assert!(profile_bytes(&value).is_err(), "{name}");
    }
    let mut value = profile();
    value.minimum_gas = 0;
    value.action_costs = [0; 9];
    value.expiry_event_gas_cost = 0;
    value.wire_byte_cost = 0;
    value.read_byte_cost = 0;
    value.write_byte_cost = 0;
    value.signature_costs = BTreeMap::from([("mldsa65".into(), 0), ("mldsa87".into(), 0)]);
    assert_eq!(
        decode_profile(&profile_bytes(&value).unwrap()).unwrap(),
        value
    );
    value.minimum_gas = value.max_transaction_gas;
    value.max_block_gas = value.max_transaction_gas;
    assert!(value.validate().is_ok());
    value.minimum_gas += 1;
    assert!(value.validate().is_err());
    value.minimum_gas -= 1;
    value.max_block_gas -= 1;
    assert!(value.validate().is_err());
}

#[test]
fn integer_width_overflow_and_negative_json_values_reject_without_fallback() {
    // These are codec bounds, not proof of fee-meter or settlement arithmetic.
    let base_profile = serde_json::to_string(&profile()).unwrap();
    for (field, original, invalid) in [
        ("gas_price", "2", "18446744073709551616"),
        (
            "max_fee_cap",
            "100000",
            "340282366920938463463374607431768211456",
        ),
        ("gas_price", "2", "-1"),
        ("max_fee_cap", "100000", "-1"),
    ] {
        let needle = format!("\"{field}\":{original}");
        assert!(base_profile.contains(&needle));
        let value = base_profile.replacen(&needle, &format!("\"{field}\":{invalid}"), 1);
        assert!(
            serde_json::from_str::<FeeProfile>(&value).is_err(),
            "{field}={invalid}"
        );
    }
    let base_sponsor = serde_json::to_string(&fixture().sponsor).unwrap();
    for (field, original, invalid) in [
        ("sponsor_nonce", "7", "18446744073709551616"),
        (
            "maximum_charge",
            "20000",
            "340282366920938463463374607431768211456",
        ),
        ("sponsor_nonce", "7", "-1"),
        ("maximum_charge", "20000", "-1"),
        ("recovery_version", "1", "65536"),
    ] {
        let needle = format!("\"{field}\":{original}");
        assert!(base_sponsor.contains(&needle));
        let value = base_sponsor.replacen(&needle, &format!("\"{field}\":{invalid}"), 1);
        assert!(
            serde_json::from_str::<SponsorAuthorization>(&value).is_err(),
            "{field}={invalid}"
        );
    }
}

#[test]
fn maximum_integer_widths_have_exact_vectors_without_truncation() {
    let mut value = profile();
    value.version = u64::MAX;
    value.activation_height = u64::MAX;
    value.gas_price = u64::MAX;
    value.minimum_gas = u64::MAX;
    value.max_transaction_gas = u64::MAX;
    value.max_block_gas = u64::MAX;
    value.max_block_recovery_bytes = u64::MAX;
    value.max_block_recovery_signatures = u64::MAX;
    value.max_fee_cap = u128::MAX;
    value.max_pending_accounts = u64::MAX;
    value.max_due_expiry_events_per_height = u64::MAX;
    value.mandatory_expiry_gas_budget = u64::MAX;
    value.expiry_event_gas_cost = u64::MAX;
    value.action_costs = [u64::MAX; 9];
    value.wire_byte_cost = u64::MAX;
    value.read_byte_cost = u64::MAX;
    value.write_byte_cost = u64::MAX;
    value.signature_costs =
        BTreeMap::from([("mldsa65".into(), u64::MAX), ("mldsa87".into(), u64::MAX)]);
    let expected = hex::decode("445954414c4c49582f5245434f564552592d4645452d50524f46494c45000001ffffffffffffffffffffffffffffffff000475647274ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff0201ffffffffffffffff02ffffffffffffffff").unwrap();
    assert_eq!(profile_bytes(&value).unwrap(), expected);
    assert_eq!(
        hex::encode(profile_digest(&value).unwrap()),
        "0a3ac4ab4db34b2e670edb3d296881c91dc9b0e494376e07e58e1d2875d70858"
    );
    assert_eq!(decode_profile(&expected).unwrap(), value);
    // Syntactic counters may reach MAX; execution separately rejects exhausted nonces.
    let mut value = fixture().sponsor;
    value.sponsor_generation = u64::MAX;
    value.sponsor_nonce = u64::MAX;
    value.fee_profile_version = u64::MAX;
    value.maximum_charge = u128::MAX;
    value.gas_limit = u64::MAX;
    value.expiry_height = u64::MAX;
    let mut expected = independent_sponsor_vector();
    expected[198..214].fill(255);
    expected[2169..2177].fill(255);
    expected[2215..2247].fill(255);
    assert_eq!(sponsor_signing_bytes(&value).unwrap(), expected);
    assert_eq!(decode_sponsor(&expected).unwrap(), value);
}
