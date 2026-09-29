//! The one-step flow against a fake Comet node: context, first spend,
//! signing, broadcast and the committed result (clients v1, K-c).
use super::*;
use crate::commands::ordinary::client;
use base64::{engine::general_purpose::STANDARD, Engine};
use clap::Parser;
use dytallix_sdk::ordinary_client::native_account;
use dytallix_sdk::ordinary_client::state_proof::{
    app_hash, key_hash, value_hash, Anchor, LeafNode, SparseMerkleProof, StateProofView, TREE,
};
use dytallix_sdk::ordinary_v2::{
    AccountDomain, AccountView, CommittedContext, Denomination, FeeProfile, ProfileView,
    PublicOrdinaryConfig, ReceiptOutcome, ReceiptView, SignedOrdinary,
};
use dytallix_sdk::ordinary_v3::{FeeProfileV3, GovernanceProfileView, VoteChoice};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

const CHAIN: &str = "consensus-cli-fixture";

fn vectors() -> Value {
    serde_json::from_str(include_str!(
        "../../../../vendor/dytallix-protocol-types/tests/fixtures/ordinary_v3_vectors.json"
    ))
    .unwrap()
}
fn v3_profile() -> FeeProfileV3 {
    serde_json::from_value(vectors()["fee_profile"]["input"].clone()).unwrap()
}
fn fee_profile() -> FeeProfile {
    v3_profile().base
}
fn committed(height: u64) -> CommittedContext {
    CommittedContext {
        chain_id: CHAIN.into(),
        genesis_digest: [7; 32],
        height,
        app_hash: [height as u8; 32],
    }
}
fn pin() -> ChainPin {
    ChainPin {
        network: AddressNetwork::Development,
        chain_id: CHAIN.into(),
        genesis_digest: [7; 32],
    }
}

/// Node state; `after` applies once a transaction is broadcast.
struct State {
    height: u64,
    profile: ProfileView,
    governance: GovernanceProfileView,
    account: Option<AccountView>,
    receipt: Option<fn(&SignedOrdinary) -> ReceiptView>,
    broadcast: Vec<Vec<u8>>,
    check_code: u32,
    after_nonce: Option<u64>,
    /// The whole authenticated state: one key and value, so every proof
    /// is a single leaf and its root is that leaf's hash.
    tree: (Vec<u8>, Vec<u8>),
    /// Serve headers whose application hash differs from the state's.
    lying_header: bool,
    /// Typed views by query path; null when absent.
    views: std::collections::BTreeMap<String, Value>,
}
fn anchor(height: u64) -> Anchor {
    Anchor {
        version: 1,
        height,
        engine_hash: "aa".repeat(32),
        parent_engine_hash: "bb".repeat(32),
        time_seconds: 1_790_000_000,
        time_nanos: 0,
        input_digest: "cc".repeat(32),
        result_digest: "dd".repeat(32),
        prior_app_hash: "ee".repeat(32),
    }
}
fn leaf(state: &State) -> LeafNode {
    LeafNode {
        key_hash: key_hash(&state.tree.0),
        value_hash: value_hash(&state.tree.1),
    }
}
fn state_app_hash(state: &State, height: u64) -> [u8; 32] {
    app_hash(height, &leaf(state).hash(), Some(&anchor(height))).unwrap()
}
fn unhex(raw: &str) -> Vec<u8> {
    (0..raw.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&raw[i..i + 2], 16).unwrap())
        .collect()
}
struct FakeNode {
    url: String,
    state: Arc<Mutex<State>>,
}
impl FakeNode {
    fn start(state: State) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(state));
        let shared = state.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let request = read_request(&mut stream);
                let result = respond(&shared, &request);
                let body = json!({"jsonrpc":"2.0","id":request["id"],"result":result}).to_string();
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        Self { url, state }
    }
    fn session(&self, key: &DytallixKeypair) -> Session {
        let identity = key_identity(key).unwrap();
        let address = pin().origin_address(&identity).unwrap();
        Session {
            pin: pin(),
            client: client(&self.url).unwrap(),
            key: DytallixKeypair::from_keypair(key.scheme(), key.public_key(), key.private_key())
                .unwrap(),
            identity,
            address,
        }
    }
    fn broadcast(&self) -> Vec<Vec<u8>> {
        self.state.lock().unwrap().broadcast.clone()
    }
}
fn read_request(stream: &mut std::net::TcpStream) -> Value {
    let mut raw = Vec::new();
    let mut buf = [0; 8192];
    loop {
        let n = stream.read(&mut buf).unwrap();
        raw.extend_from_slice(&buf[..n]);
        if let Some(end) = raw.windows(4).position(|b| b == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&raw[..end]).to_ascii_lowercase();
            let length: usize = headers
                .lines()
                .find_map(|l| {
                    l.strip_prefix("content-length:")
                        .map(|n| n.trim().parse().unwrap())
                })
                .unwrap();
            while raw.len() < end + 4 + length {
                let n = stream.read(&mut buf).unwrap();
                raw.extend_from_slice(&buf[..n]);
            }
            return serde_json::from_slice(&raw[end + 4..end + 4 + length]).unwrap();
        }
    }
}
fn query(height: u64, value: &impl serde::Serialize) -> Value {
    json!({"response":{"code":0,"log":"","height":height.to_string(),
        "value":STANDARD.encode(serde_json::to_vec(value).unwrap())}})
}
fn respond(state: &Mutex<State>, request: &Value) -> Value {
    let mut state = state.lock().unwrap();
    let height = state.height;
    match request["method"].as_str().unwrap() {
        "abci_query" => {
            let path = request["params"]["path"].as_str().unwrap();
            if path == "/ordinary/profile" {
                query(height, &state.profile)
            } else if path == "/ordinary/profile_v3" {
                query(height, &state.governance)
            } else if path.starts_with("/ordinary/account/") {
                query(height, &state.account)
            } else if path.starts_with("/ordinary/receipt/") {
                let receipt = match (&state.receipt, state.broadcast.last()) {
                    (Some(build), Some(raw)) => {
                        let signed =
                            ordinary::decode_transport(raw, &fee_profile().limits, 200_000)
                                .unwrap();
                        Some(build(&signed))
                    }
                    _ => None,
                };
                query(height, &receipt)
            } else if let Some(view) = state.views.get(path) {
                query(height, view)
            } else if path.starts_with("/account/")
                || path.starts_with("/governance/")
                || path == "/staking/validators"
            {
                query(height, &Value::Null)
            } else if let Some(key) = path.strip_prefix("/state/proof/") {
                let present = unhex(key) == state.tree.0;
                let value = present.then(|| state.tree.1.clone());
                let view = StateProofView {
                    version: 1,
                    tree: TREE.into(),
                    height,
                    key: key.into(),
                    value: value.as_ref().map(|v| STANDARD.encode(v)),
                    // `key_hash` is SHA3-256 of its input.
                    leaf: value.as_ref().map(|v| bytes_to_hex(&key_hash(v))),
                    state_root: bytes_to_hex(&leaf(&state).hash()),
                    anchor: Some(anchor(height)),
                    app_hash: bytes_to_hex(&state_app_hash(&state, height)),
                    proof: SparseMerkleProof {
                        leaf: Some(leaf(&state)),
                        siblings: Vec::new(),
                        phantom_hasher: (),
                    },
                };
                query(height, &view)
            } else {
                panic!("unexpected query {path}")
            }
        }
        "broadcast_tx_sync" => {
            let tx = STANDARD
                .decode(request["params"]["tx"].as_str().unwrap())
                .unwrap();
            let hash = bytes_to_hex(&Sha256::digest(&tx));
            state.broadcast.push(tx);
            // The next block commits: every view moves to the new head.
            state.height += 1;
            let head = committed(state.height);
            state.profile.context = head.clone();
            state.governance.context = head.clone();
            let after_nonce = state.after_nonce;
            if let Some(account) = state.account.as_mut() {
                account.context = head;
                if let Some(nonce) = after_nonce {
                    account.spending_nonce = nonce;
                }
            }
            json!({"code":state.check_code,"log":if state.check_code == 0 {""} else {"refused"},
                "codespace":"","hash":hash,"data":""})
        }
        "commit" => {
            let block: u64 = request["params"]["height"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap();
            let mut hash = state_app_hash(&state, block - 1);
            if state.lying_header {
                hash[0] ^= 1;
            }
            json!({"signed_header":{"header":{"chain_id":CHAIN,"height":block.to_string(),
                "app_hash":bytes_to_hex(&hash).to_uppercase()},"commit":{}},"canonical":true})
        }
        other => panic!("unexpected method {other}"),
    }
}

fn profile_view(chain: &str) -> ProfileView {
    ProfileView {
        version: 1,
        enabled: true,
        context: CommittedContext {
            chain_id: chain.into(),
            ..committed(40)
        },
        config: Some(PublicOrdinaryConfig {
            version: 1,
            fee_profile: fee_profile(),
            max_state_bytes: 1_000_000,
            max_grants: 100,
            max_receipts: 100,
            max_retained_profiles: 10,
            max_transport_bytes: 200_000,
            queue_max_entries: 100,
            queue_max_wire_bytes: 1_000_000,
            queue_max_signature_work: 100,
        }),
    }
}
fn account_view(key: &DytallixKeypair, nonce: u64) -> AccountView {
    let address = pin().origin_address(&key_identity(key).unwrap()).unwrap();
    let id = *address.account_id();
    AccountView {
        version: 1,
        context: committed(40),
        domain: AccountDomain {
            network: 3,
            chain_id: CHAIN.into(),
            genesis_digest: [7; 32],
            account_id: id,
        },
        account_id: id,
        address: address.encode(),
        current_key: key_identity(key).unwrap(),
        authorization_generation: 1,
        spending_nonce: nonce,
        protected: false,
        profile_digest: ordinary::profile_digest(&fee_profile()).unwrap(),
    }
}
fn state(account: Option<AccountView>) -> State {
    State {
        height: 40,
        profile: profile_view(CHAIN),
        governance: GovernanceProfileView {
            version: 1,
            enabled: true,
            context: committed(40),
            fee_profile: Some(v3_profile()),
            next_proposal_id: 3,
        },
        account,
        receipt: None,
        broadcast: Vec::new(),
        check_code: 0,
        after_nonce: None,
        tree: (b"unrelated:key".to_vec(), b"x".to_vec()),
        lying_header: false,
        views: Default::default(),
    }
}
fn write(wait_seconds: u64) -> WriteArgs {
    WriteArgs {
        gas_limit: 10_000,
        maximum_fee_udrt: 20_000,
        expiry_blocks: 50,
        memo: String::new(),
        wait_seconds,
    }
}
/// A success receipt with the exact validation gas and fee arithmetic.
fn success(signed: &SignedOrdinary) -> ReceiptView {
    let p = fee_profile();
    let wire = ordinary::encode_transport(signed, &p.limits, 200_000).unwrap();
    let envelope_len = STANDARD
        .decode(
            serde_json::from_slice::<Value>(&wire).unwrap()["envelope_base64"]
                .as_str()
                .unwrap(),
        )
        .unwrap()
        .len() as u64;
    let gas_used = envelope_len * p.wire_byte_cost
        + p.transaction_overhead
        + p.signature_costs["mldsa65"]
        + p.receipt_metadata_cost;
    let charge = u128::from(gas_used) * u128::from(p.gas_price);
    ReceiptView {
        version: 1,
        context: committed(41),
        transaction_id: ordinary::transaction_id(&signed.body, &p.limits).unwrap(),
        envelope_hash: ordinary::envelope_hash(signed, &p.limits).unwrap(),
        actor: signed.body.domain.account_id,
        block_height: 41,
        block_index: 0,
        contract_version: p.ordinary_fee_contract_version,
        profile_version: p.version,
        profile_digest: ordinary::profile_digest(&p).unwrap(),
        outcome: ReceiptOutcome::Success,
        failing_action: None,
        failure_phase: None,
        rule_class: None,
        rule_code: None,
        gas_limit: signed.body.gas_limit,
        gas_used,
        metadata_gas: p.receipt_metadata_cost,
        reserved_cap: signed.body.maximum_fee,
        charge,
        released_cap: signed.body.maximum_fee - charge,
        nonce_before: signed.body.spending_nonce,
        nonce_after: signed.body.spending_nonce + 1,
    }
}
fn balance_record(udgt: u128, udrt: u128) -> Vec<u8> {
    let mut record = 2u64.to_le_bytes().to_vec();
    for (name, amount) in [("udgt", udgt), ("udrt", udrt)] {
        record.extend((name.len() as u64).to_le_bytes());
        record.extend(name.as_bytes());
        record.extend(amount.to_le_bytes());
    }
    record
}
fn recipient_address() -> String {
    pin()
        .origin_address(&key_identity(&DytallixKeypair::generate()).unwrap())
        .unwrap()
        .encode()
}

#[tokio::test]
async fn a_first_spend_send_uses_the_origin_key_and_checks_its_receipt() {
    let key = DytallixKeypair::generate();
    let address = pin().origin_address(&key_identity(&key).unwrap()).unwrap();
    let mut initial = state(None);
    initial.receipt = Some(success);
    // Funded, and no recovery record: the proofs the first spend needs.
    initial.tree = (
        native_account::balances_key(&address),
        balance_record(0, 50_000),
    );
    let node = FakeNode::start(initial);
    let session = node.session(&key);
    let to = recipient(&pin(), &recipient_address()).unwrap();
    let action = Action::Send {
        recipient: to,
        denomination: Denomination::Udrt,
        amount: 5,
    };
    let report = submit_ordinary(&session, vec![action.clone()], write(1))
        .await
        .unwrap();
    assert_eq!(report["first_spend"], true);
    assert_eq!(report["first_spend_proof"]["height"], 40);
    assert_eq!(
        report["first_spend_proof"]["app_hash_source"],
        "node_header"
    );
    assert_eq!(report["status"], "success");
    assert_eq!(report["committed"], true);
    let sent = node.broadcast();
    assert_eq!(sent.len(), 1);
    let signed = ordinary::decode_transport(&sent[0], &fee_profile().limits, 200_000).unwrap();
    ordinary::verify_signature(&signed, &fee_profile().limits).unwrap();
    let body = &signed.body;
    assert_eq!((body.authorization_generation, body.spending_nonce), (0, 0));
    assert_eq!(body.key, session.identity);
    assert_eq!(body.domain.account_id, *session.address.account_id());
    assert_eq!(
        (body.domain.network, body.domain.chain_id.as_str()),
        (3, CHAIN)
    );
    assert_eq!(body.expiry_height, 40 + 1 + 50);
    assert_eq!(body.actions, vec![action]);
}

#[tokio::test]
async fn a_node_on_another_chain_is_refused_before_anything_is_signed() {
    let key = DytallixKeypair::generate();
    let mut initial = state(Some(account_view(&key, 4)));
    initial.profile = profile_view("impostor");
    let node = FakeNode::start(initial);
    let error = submit_ordinary(&node.session(&key), vec![Action::RewardClaim], write(0))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("pinned"), "{error}");
    assert!(node.broadcast().is_empty());
}

#[tokio::test]
async fn a_registered_account_stakes_at_its_nonce_and_reports_pending_commitment() {
    let key = DytallixKeypair::generate();
    let node = FakeNode::start(state(Some(account_view(&key, 4))));
    let action = Action::RewardBond {
        validator_id: "validator-1".into(),
        amount_udgt: 1_000_000,
    };
    let report = submit_ordinary(&node.session(&key), vec![action.clone()], write(1))
        .await
        .unwrap();
    assert_eq!(report["first_spend"], false);
    assert_eq!(report["status"], "not_yet_committed");
    let signed =
        ordinary::decode_transport(&node.broadcast()[0], &fee_profile().limits, 200_000).unwrap();
    assert_eq!(
        (
            signed.body.authorization_generation,
            signed.body.spending_nonce
        ),
        (1, 4)
    );
    assert_eq!(signed.body.actions, vec![action]);
    // A key that is not the account's current key signs nothing.
    let stranger = DytallixKeypair::generate();
    let mut session = node.session(&stranger);
    session.address = node.session(&key).address;
    assert!(
        submit_ordinary(&session, vec![Action::RewardClaim], write(0))
            .await
            .is_err()
    );
    assert_eq!(node.broadcast().len(), 1);
}

#[tokio::test]
async fn a_checktx_refusal_is_an_error_and_claims_nothing() {
    let key = DytallixKeypair::generate();
    let mut initial = state(Some(account_view(&key, 4)));
    initial.check_code = 7;
    let node = FakeNode::start(initial);
    let error = submit_ordinary(&node.session(&key), vec![Action::RewardClaim], write(1))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("refused"), "{error}");
}

#[tokio::test]
async fn a_governance_vote_waits_for_the_spent_nonce() {
    let key = DytallixKeypair::generate();
    let mut initial = state(Some(account_view(&key, 4)));
    initial.after_nonce = Some(5);
    let node = FakeNode::start(initial);
    let session = node.session(&key);
    let report = submit_governance(
        &session,
        |_| Ok(governance::vote(2, VoteChoice::NoWithVeto)),
        write(1),
    )
    .await
    .unwrap();
    assert_eq!(report["status"], "nonce_spent");
    assert_eq!(report["committed"], true);
    let signed =
        governance::decode_transport(&node.broadcast()[0], &v3_profile().limits(), 200_000)
            .unwrap();
    assert_eq!(signed.body.spending_nonce, 4);
    assert_eq!(
        signed.body.actions,
        vec![governance::vote(2, VoteChoice::NoWithVeto)]
    );
    // A proposal takes the node's next proposal ID.
    let mut taken = None;
    submit_governance(
        &session,
        |id| {
            taken = Some(id);
            Ok(governance::parameter_change(
                id,
                &governance::ParameterChange::MaxActive(16),
            )?)
        },
        write(0),
    )
    .await
    .unwrap();
    assert_eq!(taken, Some(3));
    // Governance needs an account record; a first spend must be ordinary v2.
    let fresh = FakeNode::start(state(None));
    let error = submit_governance(
        &fresh.session(&key),
        |_| Ok(governance::vote(2, VoteChoice::Yes)),
        write(0),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("first spend"), "{error}");
}

#[test]
fn chain_pins_expiry_and_amounts_are_checked() {
    let config = ChainConfig {
        version: CHAIN_CONFIG_VERSION,
        endpoint: "http://127.0.0.1:26657".into(),
        endpoint_key_base64: None,
        network: Network::Development,
        chain_id: CHAIN.into(),
        genesis_digest: "07".repeat(32),
    };
    assert_eq!(config.pin().unwrap(), pin());
    // A pin file written before interfaces v1 has no version: it is version 1.
    let mut legacy = serde_json::to_value(&config).unwrap();
    legacy.as_object_mut().unwrap().remove("version");
    let legacy: ChainConfig = serde_json::from_value(legacy).unwrap();
    assert_eq!(legacy.version, 1);
    assert_eq!(legacy.pin().unwrap(), pin());
    assert!(ChainConfig {
        version: 3,
        ..config.clone()
    }
    .pin()
    .is_err());
    let raw = serde_json::to_string(&config).unwrap();
    assert!(raw.contains("\"network\":\"development\""));
    assert_eq!(serde_json::from_str::<ChainConfig>(&raw).unwrap(), config);
    for digest in ["07".repeat(31), "0G".repeat(32), "AA".repeat(32)] {
        let bad = ChainConfig {
            genesis_digest: digest,
            ..config.clone()
        };
        assert!(bad.pin().is_err());
    }
    assert!(ChainConfig {
        chain_id: String::new(),
        ..config.clone()
    }
    .pin()
    .is_err());
    assert_eq!(expiry(40, 50, 1000).unwrap(), 91);
    assert!(expiry(40, 0, 1000).is_err());
    assert!(expiry(40, 1001, 1000).is_err());
    assert!(expiry(u64::MAX, 1, 1000).is_err());
    assert_eq!(tokens(1_500_000), "1.500000");
    assert_eq!(tokens(7), "0.000007");
    assert!(recipient(&pin(), "tdytallix1notonthisnetwork").is_err());
    let testnet = AccountAddress::from_account_id(AddressNetwork::Testnet, [1; 32]).encode();
    assert!(recipient(&pin(), &testnet).is_err());
}

#[test]
fn endpoints_are_loopback_or_a_channel_pin_for_the_chain() {
    use dytallix_sdk::ordinary_client::{Endpoint, EndpointPin};
    let dir = std::env::temp_dir().join(format!("dytallix-cli-endpoints-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let key = vec![7u8; 1952];
    let write_pin = |name: &str, chain: &str| {
        let path = dir.join(name);
        let pin = EndpointPin::new(chain, "node.example:26670", &key).unwrap();
        std::fs::write(&path, pin.to_json()).unwrap();
        path.to_str().unwrap().to_owned()
    };
    let pin_file = write_pin("pin.json", CHAIN);

    // Loopback HTTP needs no key; TLS and remote HTTP are refused.
    let local = ChainConfig::new(
        "http://127.0.0.1:26657/",
        Network::Development,
        CHAIN.into(),
        "07".repeat(32),
    )
    .unwrap();
    assert_eq!(local.endpoint, "http://127.0.0.1:26657");
    assert!(local.endpoint_key_base64.is_none());
    for bad in ["https://node.example:443", "http://192.0.2.1:26657"] {
        let error = ChainConfig::new(bad, Network::Development, CHAIN.into(), "07".repeat(32))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("TLS is not supported") || error.contains("only a node on this machine"),
            "{error}"
        );
    }

    // A pin file becomes a version 2 pin with the key inline.
    let remote = ChainConfig::new(
        &pin_file,
        Network::Development,
        CHAIN.into(),
        "07".repeat(32),
    )
    .unwrap();
    assert_eq!(remote.version, 2);
    assert_eq!(remote.endpoint, "node.example:26670");
    assert_eq!(
        remote.endpoint_key_base64.as_deref(),
        Some(STANDARD.encode(&key).as_str())
    );
    let raw = serde_json::to_string(&remote).unwrap();
    let reloaded: ChainConfig = serde_json::from_str(&raw).unwrap();
    match reloaded.endpoint().unwrap() {
        Endpoint::Channel(pin) => {
            assert_eq!((pin.network.as_str(), pin.public_key), (CHAIN, key.clone()))
        }
        other => panic!("{other:?}"),
    }
    // The loopback file does not serialize a key field at all.
    assert!(!serde_json::to_string(&local)
        .unwrap()
        .contains("endpoint_key"));

    // A pin for another chain is refused when pinning and as an override.
    let other = write_pin("other.json", "another-chain");
    assert!(ChainConfig::new(&other, Network::Development, CHAIN.into(), "07".repeat(32)).is_err());
    let error = local.client(Some(&other)).err().unwrap().to_string();
    assert!(error.contains("not the pinned chain"), "{error}");
    assert!(local.client(Some(&pin_file)).is_ok());

    // A version 1 file with a remote HTTP endpoint loads but cannot connect:
    // it must be pinned again with the endpoint's pin file.
    let old = ChainConfig {
        version: 1,
        endpoint: "http://192.0.2.1:26657".into(),
        ..local.clone()
    };
    old.pin().unwrap();
    let error = old.client(None).err().unwrap().to_string();
    assert!(error.contains("pin-chain --endpoint PIN_FILE"), "{error}");
    // Version 1 never carries a key, and a noncanonical key is refused.
    assert!(ChainConfig {
        version: 1,
        ..remote.clone()
    }
    .pin()
    .is_err());
    let noncanonical = ChainConfig {
        endpoint_key_base64: Some(format!("{}\n", STANDARD.encode(&key))),
        ..remote.clone()
    };
    assert!(noncanonical.endpoint().is_err());
    assert!(local.client(Some("/nonexistent/pin.json")).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn one_step_writes_require_an_explicit_gas_limit_and_fee_cap() {
    let parse = |args: &[&str]| crate::Cli::try_parse_from([&["dytallix"], args].concat());
    let to = recipient_address();
    let send = ["send", "--to", to.as_str(), "--amount", "1.5"];
    assert!(parse(&send).is_err());
    assert!(parse(&[&send[..], &["--gas-limit", "10000"]].concat()).is_err());
    let fee = ["--gas-limit", "10000", "--maximum-fee-udrt", "20000"];
    assert!(parse(&[&send[..], &fee].concat()).is_ok());
    assert!(parse(&[&send[..], &fee, &["--wallet", "a", "--key-file", "k"]].concat()).is_err());
    assert!(parse(
        &[
            &["stake", "bond", "--validator", "v1", "--amount", "10"][..],
            &fee
        ]
        .concat()
    )
    .is_ok());
    assert!(parse(
        &[
            &["stake", "unbond", "--validator", "v1", "--amount", "1"][..],
            &fee
        ]
        .concat()
    )
    .is_ok());
    assert!(parse(&[&["stake", "claim"][..], &fee].concat()).is_ok());
    assert!(parse(&["stake", "claim"]).is_err());
    assert!(parse(
        &[
            &[
                "governance",
                "vote",
                "--proposal-id",
                "3",
                "--choice",
                "no-with-veto"
            ][..],
            &fee
        ]
        .concat()
    )
    .is_ok());
    assert!(parse(
        &[
            &[
                "governance",
                "deposit",
                "--proposal-id",
                "3",
                "--amount",
                "5"
            ][..],
            &fee
        ]
        .concat()
    )
    .is_ok());
    assert!(parse(&[&["governance", "propose", "--max-active", "16"][..], &fee].concat()).is_ok());
    assert!(parse(&[&["governance", "propose"][..], &fee].concat()).is_err());
    assert!(parse(&["balance"]).is_ok());
    assert!(parse(&["stake", "status"]).is_ok());
    assert!(parse(&["stake", "status", to.as_str()]).is_ok());
    assert!(parse(&["stake", "validators"]).is_ok());
    assert!(parse(&["governance", "show", "--proposal-id", "3"]).is_ok());
    assert!(parse(&[
        "governance",
        "show",
        "--proposal-id",
        "3",
        "--voter",
        to.as_str()
    ])
    .is_ok());
    assert!(parse(&["governance", "show"]).is_err());
    assert!(parse(&["balance", to.as_str(), "--wallet", "a"]).is_err());
    let pin_chain = [
        "config",
        "pin-chain",
        "--endpoint",
        "http://127.0.0.1:26657",
        "--network",
        "development",
        "--chain-id",
        CHAIN,
        "--genesis-digest",
    ];
    let digest = "07".repeat(32);
    assert!(parse(&[&pin_chain[..], &[digest.as_str()]].concat()).is_ok());
    assert!(parse(&[
        "config",
        "pin-chain",
        "--endpoint",
        "x",
        "--network",
        "moon",
        "--chain-id",
        "c",
        "--genesis-digest",
        "d"
    ])
    .is_err());
    // The legacy testnet commands are gone (E04 gap 19).
    assert!(parse(&["legacy", "send", "addr", "5"]).is_err());
}

#[tokio::test]
async fn balances_are_verified_to_the_next_header_and_absence_is_explicit() {
    use dytallix_sdk::ordinary_client::AppHashSource;
    use dytallix_sdk::ordinary_v2::AccountAddress;
    let key = DytallixKeypair::generate();
    let address = pin().origin_address(&key_identity(&key).unwrap()).unwrap();
    let mut initial = state(None);
    initial.tree = (
        native_account::balances_key(&address),
        balance_record(1_500_000, 7),
    );
    let expected = state_app_hash(&initial, 40);
    let node = FakeNode::start(initial);
    let client = client(&node.url).unwrap();
    let reported = client.query_balances(&pin(), &address, None).await.unwrap();
    assert_eq!((reported.height, reported.app_hash), (40, expected));
    assert_eq!(reported.source, AppHashSource::NodeHeader);
    let balances = reported.balances.unwrap();
    assert_eq!((balances["udgt"], balances["udrt"]), (1_500_000, 7));
    let absent = AccountAddress::from_account_id(AddressNetwork::Development, [9; 32]);
    assert_eq!(
        client
            .query_balances(&pin(), &absent, None)
            .await
            .unwrap()
            .balances,
        None
    );
    // A caller's trusted hash replaces the header; a different one is refused.
    let trusted = client
        .query_balances(&pin(), &address, Some(expected))
        .await
        .unwrap();
    assert_eq!(trusted.source, AppHashSource::Caller);
    let mut other = expected;
    other[0] ^= 1;
    assert!(client
        .query_balances(&pin(), &address, Some(other))
        .await
        .is_err());
    // A header on another chain is refused.
    let mut elsewhere = pin();
    elsewhere.chain_id = "elsewhere".into();
    assert!(client
        .query_balances(&elsewhere, &address, None)
        .await
        .is_err());
    // So is a header whose application hash differs from the proof's.
    node.state.lock().unwrap().lying_header = true;
    let error = client
        .query_balances(&pin(), &address, None)
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("trusted application hash"),
        "{error}"
    );
}

#[tokio::test]
async fn a_first_spend_is_refused_when_proofs_show_a_record_or_no_funds() {
    let key = DytallixKeypair::generate();
    let address = pin().origin_address(&key_identity(&key).unwrap()).unwrap();
    // The node reports no account, but its state holds a recovery record.
    let mut recorded = state(None);
    recorded.tree = (
        native_account::recovery_account_key(&address),
        b"{}".to_vec(),
    );
    let node = FakeNode::start(recorded);
    let error = submit_ordinary(&node.session(&key), vec![Action::RewardClaim], write(0))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("not a first spend"), "{error}");
    assert!(node.broadcast().is_empty());
    // No balance record: nothing to pay the fee.
    let node = FakeNode::start(state(None));
    let error = submit_ordinary(&node.session(&key), vec![Action::RewardClaim], write(0))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("fund it first"), "{error}");
    assert!(node.broadcast().is_empty());
}

#[tokio::test]
async fn typed_reads_are_checked_against_the_pin_height_and_question() {
    use dytallix_sdk::ordinary_client::{
        AccountSummaryView, ProposalPhaseView, ProposalView, ValidatorEntryView, ValidatorSetView,
        VoteView,
    };
    let key = DytallixKeypair::generate();
    let address = pin().origin_address(&key_identity(&key).unwrap()).unwrap();
    let summary = AccountSummaryView {
        version: 1,
        context: committed(40),
        address: address.encode(),
        account_id: *address.account_id(),
        funded: true,
        liquid_udgt: 900,
        liquid_udrt: 7,
        nonce: 2,
        bonded_udgt: 100,
        bonds: Vec::new(),
        pending_bond_udgt: 0,
        unbonding_udgt: 0,
        unbonds: Vec::new(),
        claimable_rewards_udrt: 3,
    };
    let validators = ValidatorSetView {
        version: 1,
        enabled: true,
        context: committed(40),
        height: 41,
        validators: vec![ValidatorEntryView {
            validator_id: "validator-one".into(),
            owner: address.encode(),
            consensus_key_base64: "a2V5".into(),
            power_udgt: 100,
        }],
        max_active: 4,
        min_self_bond_udgt: 10,
    };
    let proposal = ProposalView {
        version: 1,
        context: committed(40),
        proposal_id: 3,
        proposer: *address.account_id(),
        action_class: 1,
        action_data: "00".into(),
        action_digest: [1; 32],
        admitted_height: 30,
        deposited_udgt: 5,
        depositors: 1,
        phase: ProposalPhaseView::Collecting { close_height: 45 },
        due_height: 45,
    };
    let vote = VoteView {
        version: 1,
        context: committed(40),
        proposal_id: 3,
        voter: *address.account_id(),
        choice: Some(VoteChoice::Yes),
    };
    let vote_path = format!("/governance/vote/3/{}", bytes_to_hex(address.account_id()));
    let serve = |summary: &AccountSummaryView,
                 validators: &ValidatorSetView,
                 proposal: &ProposalView,
                 vote: &VoteView| {
        let mut initial = state(None);
        initial.views.insert(
            format!("/account/{}", address.encode()),
            serde_json::to_value(summary).unwrap(),
        );
        initial.views.insert(
            "/staking/validators".into(),
            serde_json::to_value(validators).unwrap(),
        );
        initial.views.insert(
            "/governance/proposal/3".into(),
            serde_json::to_value(proposal).unwrap(),
        );
        initial
            .views
            .insert(vote_path.clone(), serde_json::to_value(vote).unwrap());
        FakeNode::start(initial)
    };
    let node = serve(&summary, &validators, &proposal, &vote);
    let client = client(&node.url).unwrap();
    assert_eq!(
        client
            .query_account_summary(&pin(), &address)
            .await
            .unwrap(),
        summary
    );
    assert_eq!(client.query_validators(&pin()).await.unwrap(), validators);
    assert_eq!(
        client.query_proposal(&pin(), 3).await.unwrap(),
        Some(proposal.clone())
    );
    assert_eq!(client.query_proposal(&pin(), 4).await.unwrap(), None);
    assert_eq!(
        client
            .query_vote(&pin(), 3, address.account_id())
            .await
            .unwrap(),
        Some(vote.clone())
    );
    // Another chain, another height, or an answer to a different question.
    let mut elsewhere = pin();
    elsewhere.chain_id = "elsewhere".into();
    assert!(client
        .query_account_summary(&elsewhere, &address)
        .await
        .is_err());
    assert!(client.query_validators(&elsewhere).await.is_err());
    let other = AccountAddress::from_account_id(AddressNetwork::Development, [5; 32]);
    let mismatched = [
        AccountSummaryView {
            address: other.encode(),
            ..summary.clone()
        },
        AccountSummaryView {
            context: committed(39),
            ..summary.clone()
        },
        AccountSummaryView {
            version: 2,
            ..summary.clone()
        },
    ];
    for bad in mismatched {
        let node = serve(&bad, &validators, &proposal, &vote);
        assert!(
            client_of(&node)
                .query_account_summary(&pin(), &address)
                .await
                .is_err(),
            "{bad:?}"
        );
    }
    let empty = ValidatorSetView {
        validators: Vec::new(),
        ..validators.clone()
    };
    let late = ValidatorSetView {
        height: 42,
        ..validators.clone()
    };
    for bad in [empty, late] {
        let node = serve(&summary, &bad, &proposal, &vote);
        assert!(client_of(&node).query_validators(&pin()).await.is_err());
    }
    let node = serve(
        &summary,
        &validators,
        &ProposalView {
            proposal_id: 9,
            ..proposal.clone()
        },
        &vote,
    );
    assert!(client_of(&node).query_proposal(&pin(), 3).await.is_err());
    let node = serve(
        &summary,
        &validators,
        &proposal,
        &VoteView {
            voter: [5; 32],
            ..vote.clone()
        },
    );
    assert!(client_of(&node)
        .query_vote(&pin(), 3, address.account_id())
        .await
        .is_err());
}
fn client_of(node: &FakeNode) -> dytallix_sdk::ordinary_client::CometClient {
    client(&node.url).unwrap()
}
