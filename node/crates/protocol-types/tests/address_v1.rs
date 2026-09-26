use dytallix_protocol_types::address::{AccountAddress, AddressNetwork, OriginKeyAlgorithm};
use serde_json::Value;

fn network(name: &str) -> AddressNetwork {
    match name {
        "mainnet" => AddressNetwork::Mainnet,
        "testnet" => AddressNetwork::Testnet,
        "development" => AddressNetwork::Development,
        _ => panic!("unknown fixture network"),
    }
}
fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/address-v1.json")).unwrap()
}

#[test]
fn independent_encoding_vectors_match() {
    let fixture = fixture();
    let cases = fixture["encoding"].as_array().unwrap();
    assert_eq!(cases.len(), 12);
    for case in cases {
        let net = network(case["network"].as_str().unwrap());
        let id: [u8; 32] = hex::decode(case["account_id"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
        let address = AccountAddress::from_account_id(net, id);
        assert_eq!(address.encode(), case["address"].as_str().unwrap());
        assert_eq!(
            AccountAddress::decode(net, &address.encode()).unwrap(),
            address
        );
        assert_eq!(address.account_id(), &id);
    }
}
#[test]
fn independent_origin_vectors_match() {
    let fixture = fixture();
    let cases = fixture["origins"].as_array().unwrap();
    assert_eq!(cases.len(), 9);
    for case in cases {
        let algorithm = match case["algorithm"].as_str().unwrap() {
            "mldsa65" => OriginKeyAlgorithm::MlDsa65,
            "mldsa87" => OriginKeyAlgorithm::MlDsa87,
            "legacy_dilithium5" => OriginKeyAlgorithm::LegacyDilithium5,
            _ => panic!("unknown fixture algorithm"),
        };
        let key: Vec<u8> = (0..case["public_key_len"].as_u64().unwrap())
            .map(|i| (i % 251) as u8)
            .collect();
        let address = AccountAddress::from_origin_key(
            network(case["network"].as_str().unwrap()),
            case["chain_id"].as_str().unwrap(),
            algorithm,
            &key,
        )
        .unwrap();
        assert_eq!(
            hex::encode(address.account_id()),
            case["account_id"].as_str().unwrap()
        );
        assert_eq!(address.encode(), case["address"].as_str().unwrap());
    }
}
#[test]
fn unsupported_and_noncanonical_vectors_are_rejected() {
    let fixture = fixture();
    let cases = fixture["invalid"].as_array().unwrap();
    assert_eq!(cases.len(), 10);
    for case in cases {
        assert!(
            AccountAddress::decode(
                network(case["network"].as_str().unwrap()),
                case["address"].as_str().unwrap()
            )
            .is_err(),
            "{}",
            case["name"]
        );
    }
}
#[test]
fn origin_domain_separates_network_chain_and_algorithm() {
    let key = vec![42; 2592];
    let main = AccountAddress::from_origin_key(
        AddressNetwork::Mainnet,
        "chain",
        OriginKeyAlgorithm::MlDsa87,
        &key,
    )
    .unwrap();
    let test = AccountAddress::from_origin_key(
        AddressNetwork::Testnet,
        "chain",
        OriginKeyAlgorithm::MlDsa87,
        &key,
    )
    .unwrap();
    let chain = AccountAddress::from_origin_key(
        AddressNetwork::Mainnet,
        "other",
        OriginKeyAlgorithm::MlDsa87,
        &key,
    )
    .unwrap();
    let legacy = AccountAddress::from_origin_key(
        AddressNetwork::Mainnet,
        "chain",
        OriginKeyAlgorithm::LegacyDilithium5,
        &key,
    )
    .unwrap();
    assert_ne!(main.account_id(), test.account_id());
    assert_ne!(main.account_id(), chain.account_id());
    assert_ne!(main.account_id(), legacy.account_id());
    assert!(AccountAddress::decode(AddressNetwork::Testnet, &main.encode()).is_err());
}
#[test]
fn origin_requires_explicit_chain_and_key_length() {
    for chain in [String::new(), "a".repeat(256)] {
        assert!(AccountAddress::from_origin_key(
            AddressNetwork::Development,
            &chain,
            OriginKeyAlgorithm::MlDsa87,
            &[0; 2592]
        )
        .is_err());
    }
    for length in [0, 1, 2591, 2593] {
        assert!(AccountAddress::from_origin_key(
            AddressNetwork::Development,
            "chain",
            OriginKeyAlgorithm::MlDsa87,
            &vec![0; length]
        )
        .is_err());
    }
    assert!(AccountAddress::from_origin_key(
        AddressNetwork::Development,
        &"a".repeat(255),
        OriginKeyAlgorithm::MlDsa87,
        &[0; 2592]
    )
    .is_ok());
}
#[test]
fn whitespace_unicode_and_oversized_inputs_are_rejected() {
    let address = AccountAddress::from_account_id(AddressNetwork::Mainnet, [7; 32]).encode();
    for value in [
        format!(" {address}"),
        format!("{address}\n"),
        "é".repeat(35),
        "q".repeat(10_000),
    ] {
        assert!(AccountAddress::decode(AddressNetwork::Mainnet, &value).is_err());
    }
}
