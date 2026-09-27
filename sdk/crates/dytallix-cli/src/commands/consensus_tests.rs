//! The one-step flow against a fake Comet node: context, first spend,
//! signing, broadcast and the committed result (clients v1, K-c).
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use clap::Parser;
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
    /// Raw state values served at `/state/proof/{hex key}`.
    values: std::collections::BTreeMap<String, Value>,
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
            } else if let Some(key) = path.strip_prefix("/state/proof/") {
                let value = state.values.get(key).cloned().unwrap_or(Value::Null);
                query(
                    height,
                    &json!({"version":1,"tree":"jmt-0.12.0/sha3-256","height":height,
                    "key":key,"value":value,"leaf":null,"state_root":"00","anchor":null,
                    "app_hash":"00","proof":{}}),
                )
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
        values: Default::default(),
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
fn recipient_address() -> String {
    pin()
        .origin_address(&key_identity(&DytallixKeypair::generate()).unwrap())
        .unwrap()
        .encode()
}

#[tokio::test]
async fn a_first_spend_send_uses_the_origin_key_and_checks_its_receipt() {
    let key = DytallixKeypair::generate();
    let mut initial = state(None);
    initial.receipt = Some(success);
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
        endpoint: "http://127.0.0.1:26657".into(),
        network: Network::Development,
        chain_id: CHAIN.into(),
        genesis_digest: "07".repeat(32),
    };
    assert_eq!(config.pin().unwrap(), pin());
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
    #[cfg(feature = "legacy-network")]
    assert!(parse(&["legacy", "send", "addr", "5"]).is_ok());
}

#[tokio::test]
async fn balances_come_from_the_native_record_and_absence_is_explicit() {
    use dytallix_sdk::ordinary_v2::AccountAddress;
    let key = DytallixKeypair::generate();
    let address = pin().origin_address(&key_identity(&key).unwrap()).unwrap();
    let mut record = 2u64.to_le_bytes().to_vec();
    for (name, amount) in [("udgt", 1_500_000u128), ("udrt", 7u128)] {
        record.extend((name.len() as u64).to_le_bytes());
        record.extend(name.as_bytes());
        record.extend(amount.to_le_bytes());
    }
    let mut initial = state(None);
    let key_hex = bytes_to_hex(format!("acct:balances:{}", address.encode()).as_bytes());
    initial
        .values
        .insert(key_hex, STANDARD.encode(&record).into());
    let node = FakeNode::start(initial);
    let client = client(&node.url).unwrap();
    let reported = client.query_balances(&address).await.unwrap();
    assert_eq!(reported.height, 40);
    let balances = reported.balances.unwrap();
    assert_eq!((balances["udgt"], balances["udrt"]), (1_500_000, 7));
    let absent = AccountAddress::from_account_id(AddressNetwork::Development, [9; 32]);
    assert_eq!(client.query_balances(&absent).await.unwrap().balances, None);
}
