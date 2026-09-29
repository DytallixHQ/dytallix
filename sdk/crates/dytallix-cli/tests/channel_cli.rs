//! The CLI reaches a remote node only through the client channel (E04 gap
//! 19): `--endpoint` names the endpoint's pin file, and the request crosses
//! a real channel to an endpoint on this machine. TLS and remote plain HTTP
//! are refused before any connection.
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread::JoinHandle;

use dytallix_client_channel::{
    endpoint_offer, handshake_payload_len, EndpointPin, HandshakeType, Identity, MessageReader,
    Request, Response, Session, HANDSHAKE_HEADER_LEN, MAX_REQUEST_MESSAGE,
};
use serde_json::Value;

const CHAIN: &str = "channel-cli-test";

fn dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "dytallix-cli-channel-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dytallix"))
        .args(args)
        .env("HOME", home)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// One channel exchange as the node's adapter makes it. The endpoint answers
/// every JSON-RPC request with an ABCI query result whose value is `null`,
/// and returns the request it saw.
fn endpoint(seed: u8) -> (EndpointPin, JoinHandle<Option<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let identity = Identity::from_seed(&[seed; 32]);
    let pin = EndpointPin::new(
        CHAIN,
        &listener.local_addr().unwrap().to_string(),
        identity.public_key(),
    )
    .unwrap();
    let task = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut header = [0u8; HANDSHAKE_HEADER_LEN];
        stream.read_exact(&mut header).ok()?;
        let mut hello = vec![0; handshake_payload_len(&header, HandshakeType::Hello).ok()?];
        stream.read_exact(&mut hello).ok()?;
        let (pending, offer) = endpoint_offer(&identity, CHAIN, &hello).ok()?;
        stream.write_all(&offer).ok()?;
        let mut header = [0u8; HANDSHAKE_HEADER_LEN];
        stream.read_exact(&mut header).ok()?;
        let mut finish = vec![0; handshake_payload_len(&header, HandshakeType::Finish).ok()?];
        stream.read_exact(&mut finish).ok()?;
        let Session { mut sealer, opener } = pending.finish(&finish).ok()?;
        let mut reader = MessageReader::new(opener, MAX_REQUEST_MESSAGE);
        let request = loop {
            let mut bytes = vec![0; reader.wanted()];
            stream.read_exact(&mut bytes).ok()?;
            if let Some(message) = reader.feed(&bytes).ok()? {
                break Request::decode(&message).ok()?;
            }
        };
        let rpc: Value = serde_json::from_slice(&request.body).ok()?;
        let reply = serde_json::json!({"jsonrpc":"2.0","id":rpc["id"],"result":{"response":{
            "code":0,"log":"","height":"19","value":"bnVsbA=="}}});
        let response = Response {
            status: 200,
            content_type: "application/json".into(),
            cache_control: String::new(),
            body: reply.to_string().into_bytes(),
        };
        stream
            .write_all(&sealer.seal_message(&response.encode().unwrap()).unwrap())
            .ok()?;
        Some(rpc)
    });
    (pin, task)
}

fn write_pin(dir: &Path, name: &str, pin: &EndpointPin) -> String {
    let path = dir.join(name);
    std::fs::write(&path, pin.to_json()).unwrap();
    path.to_str().unwrap().to_owned()
}

#[test]
fn a_query_crosses_the_channel_named_by_a_pin_file() {
    let home = dir("query");
    let account = "ab".repeat(32);
    let output = home.join("account.json");
    let output = output.to_str().unwrap();

    let (pin, task) = endpoint(1);
    let pin_file = write_pin(&home, "pin.json", &pin);
    let result = run(
        &home,
        &[
            "ordinary",
            "query-account",
            "--endpoint",
            &pin_file,
            "--account-id",
            &account,
            "--output",
            output,
        ],
    );
    // The endpoint reported no account: the request reached it and its
    // answer was read.
    assert!(!result.status.success());
    assert!(
        text(&result).contains("account is absent from committed state"),
        "{}",
        text(&result)
    );
    let request = task
        .join()
        .unwrap()
        .expect("the endpoint served the request");
    assert_eq!(request["method"], "abci_query");
    assert_eq!(
        request["params"]["path"],
        format!("/ordinary/account/{account}")
    );

    // A pin with another key: the endpoint closes the channel.
    let (pin, task) = endpoint(1);
    let other = EndpointPin::new(
        CHAIN,
        &pin.address,
        Identity::from_seed(&[2; 32]).public_key(),
    )
    .unwrap();
    let other_file = write_pin(&home, "other.json", &other);
    let result = run(
        &home,
        &[
            "ordinary",
            "query-account",
            "--endpoint",
            &other_file,
            "--account-id",
            &account,
            "--output",
            output,
        ],
    );
    assert!(
        text(&result).contains("closed the channel"),
        "{}",
        text(&result)
    );
    assert!(task.join().unwrap().is_none());
    std::fs::remove_dir_all(&home).unwrap();
}

#[test]
fn tls_and_remote_plain_http_are_refused() {
    let home = dir("refused");
    let account = "ab".repeat(32);
    for (endpoint, expected) in [
        ("https://node.example:443", "TLS is not supported"),
        ("http://192.0.2.1:26657", "only a node on this machine"),
        ("/nonexistent/pin.json", "cannot read"),
    ] {
        let result = run(
            &home,
            &[
                "ordinary",
                "query-account",
                "--endpoint",
                endpoint,
                "--account-id",
                &account,
                "--output",
                "/dev/null",
            ],
        );
        assert!(!result.status.success());
        assert!(
            text(&result).contains(expected),
            "{endpoint}: {}",
            text(&result)
        );
    }
    std::fs::remove_dir_all(&home).unwrap();
}

#[test]
fn pin_chain_stores_the_endpoint_key() {
    let home = dir("pin-chain");
    let (pin, _task) = endpoint(3);
    let pin_file = write_pin(&home, "pin.json", &pin);
    let digest = "07".repeat(32);
    let result = run(
        &home,
        &[
            "config",
            "pin-chain",
            "--endpoint",
            &pin_file,
            "--network",
            "development",
            "--chain-id",
            CHAIN,
            "--genesis-digest",
            &digest,
            "--no-check",
        ],
    );
    assert!(result.status.success(), "{}", text(&result));
    assert!(
        text(&result).contains(&pin.fingerprint()),
        "{}",
        text(&result)
    );
    let stored: Value =
        serde_json::from_slice(&std::fs::read(home.join(".dytallix/chain.json")).unwrap()).unwrap();
    assert_eq!(stored["version"], 2);
    assert_eq!(stored["endpoint"], pin.address.as_str());
    assert!(stored["endpoint_key_base64"].as_str().is_some());

    // A pin for another chain is refused.
    let other = EndpointPin::new("another-chain", &pin.address, &pin.public_key).unwrap();
    let other_file = write_pin(&home, "other.json", &other);
    let result = run(
        &home,
        &[
            "config",
            "pin-chain",
            "--endpoint",
            &other_file,
            "--network",
            "development",
            "--chain-id",
            CHAIN,
            "--genesis-digest",
            &digest,
            "--no-check",
        ],
    );
    assert!(!result.status.success());
    assert!(
        text(&result).contains("not channel-cli-test"),
        "{}",
        text(&result)
    );
    std::fs::remove_dir_all(&home).unwrap();
}
