//! The browser companion (E04 gap 19, C-d): `dytallix gateway serve` relays
//! its own pages' JSON-RPC to the pinned chain and refuses every other page.
//! The tests run the real binary and speak raw HTTP as a browser would.
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Arc, Mutex};

use dytallix_client_channel::{
    endpoint_offer, handshake_payload_len, EndpointPin, HandshakeType, Identity, MessageReader,
    Request, Response, Session, HANDSHAKE_HEADER_LEN, MAX_REQUEST_MESSAGE,
};
use serde_json::{json, Value};

const CHAIN: &str = "gateway-cli-test";

fn dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "dytallix-cli-gateway-{label}-{}",
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

fn pin_chain(home: &Path, endpoint: &str) {
    let digest = "07".repeat(32);
    let result = run(
        home,
        &[
            "config",
            "pin-chain",
            "--endpoint",
            endpoint,
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
}

/// A loopback Comet node stand-in: it answers every JSON-RPC request with
/// the method it received, and records each request.
fn node() -> (String, Arc<Mutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let (_, _, body) = read_http(&mut stream, true);
            let rpc: Value = serde_json::from_slice(&body).unwrap();
            let reply =
                json!({"jsonrpc":"2.0","id":rpc["id"],"result":{"echo":rpc["method"]}}).to_string();
            record.lock().unwrap().push(rpc);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                reply.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    (url, seen)
}

/// Reads one HTTP message. For a request (`until_length`), stops after its
/// Content-Length body; for a response, reads to the end.
fn read_http(
    stream: &mut TcpStream,
    until_length: bool,
) -> (String, BTreeMap<String, String>, Vec<u8>) {
    let mut raw = Vec::new();
    let mut buffer = [0; 4096];
    let end = loop {
        if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break end;
        }
        let n = stream.read(&mut buffer).unwrap();
        assert!(n > 0, "connection closed before the headers");
        raw.extend_from_slice(&buffer[..n]);
    };
    let head = String::from_utf8(raw[..end].to_vec()).unwrap();
    let mut lines = head.split("\r\n");
    let first = lines.next().unwrap().to_owned();
    let headers: BTreeMap<String, String> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let mut body = raw[end + 4..].to_vec();
    if until_length {
        let length: usize = headers
            .get("content-length")
            .map_or(0, |v| v.parse().unwrap());
        while body.len() < length {
            let n = stream.read(&mut buffer).unwrap();
            assert!(n > 0);
            body.extend_from_slice(&buffer[..n]);
        }
    } else {
        stream.read_to_end(&mut body).unwrap();
    }
    (first, headers, body)
}

/// A running `dytallix gateway serve`, killed when dropped.
struct Gateway {
    child: Child,
    authority: String,
}
impl Gateway {
    fn start(home: &Path, extra: &[&str]) -> Gateway {
        let mut child = Command::new(env!("CARGO_BIN_EXE_dytallix"))
            .args(["gateway", "serve", "--listen", "127.0.0.1:0"])
            .args(extra)
            .env("HOME", home)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let ready: Value = serde_json::from_str(&line).expect("the gateway prints its URL");
        let url = ready["url"].as_str().unwrap();
        let authority = url
            .strip_prefix("http://")
            .unwrap()
            .trim_end_matches('/')
            .to_owned();
        Gateway { child, authority }
    }
    fn origin(&self) -> String {
        format!("http://{}", self.authority)
    }
    /// One request with the given Host and headers. Returns the status code,
    /// the headers and the body.
    fn send(
        &self,
        method: &str,
        path: &str,
        host: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> (u16, BTreeMap<String, String>, Vec<u8>) {
        let mut stream = TcpStream::connect(&self.authority).unwrap();
        let mut request =
            format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
        for (name, value) in headers {
            request.push_str(&format!("{name}: {value}\r\n"));
        }
        request.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
        stream.write_all(request.as_bytes()).unwrap();
        stream.write_all(body).unwrap();
        let (status, headers, body) = read_http(&mut stream, false);
        (
            status.split(' ').nth(1).unwrap().parse().unwrap(),
            headers,
            body,
        )
    }
    fn rpc(&self, headers: &[(&str, &str)], body: &[u8]) -> (u16, Vec<u8>) {
        let (status, _, body) = self.send("POST", "/rpc", &self.authority.clone(), headers, body);
        (status, body)
    }
}
impl Drop for Gateway {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

const STATUS: &[u8] = br#"{"jsonrpc":"2.0","id":1,"method":"status","params":{}}"#;

#[test]
fn the_gateway_relays_only_its_own_pages() {
    let home = dir("relay");
    let (url, seen) = node();
    pin_chain(&home, &url);
    let gateway = Gateway::start(&home, &[]);
    let origin = gateway.origin();
    let json = ("Content-Type", "application/json");

    // Its own page, and a local program without an Origin, are relayed.
    let (status, headers, body) = gateway.send(
        "POST",
        "/rpc",
        &gateway.authority,
        &[json, ("Origin", &origin), ("Sec-Fetch-Site", "same-origin")],
        STATUS,
    );
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let reply: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(reply["result"]["echo"], "status");
    assert!(headers["content-security-policy"].contains("default-src 'none'"));
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert_eq!(headers["cache-control"], "no-store");
    assert!(!headers.contains_key("access-control-allow-origin"));
    assert_eq!(gateway.rpc(&[json], STATUS).0, 200);

    // Another site's page, a rebound DNS name and a cross-site fetch are
    // refused before anything reaches the node.
    for refused in [
        gateway.rpc(&[json, ("Origin", "http://evil.example")], STATUS),
        gateway.rpc(&[json, ("Origin", "null")], STATUS),
        gateway.rpc(&[json, ("Sec-Fetch-Site", "cross-site")], STATUS),
        gateway.rpc(&[json, ("Sec-Fetch-Site", "same-site")], STATUS),
    ] {
        assert_eq!(refused.0, 403, "{}", String::from_utf8_lossy(&refused.1));
    }
    let port = gateway.authority.rsplit_once(':').unwrap().1;
    for host in [format!("evil.example:{port}"), format!("localhost:{port}")] {
        let (status, _, _) = gateway.send("POST", "/rpc", &host, &[json], STATUS);
        assert_eq!(status, 403, "{host}");
    }
    // A form post cannot be JSON-RPC, and the body must be JSON.
    assert_eq!(
        gateway.rpc(&[("Content-Type", "text/plain")], STATUS).0,
        415
    );
    assert_eq!(gateway.rpc(&[json], b"not json").0, 400);
    assert_eq!(gateway.rpc(&[json], b"3").0, 400);
    // No preflight is answered, so no other origin can send JSON.
    let (status, headers, _) = gateway.send(
        "OPTIONS",
        "/rpc",
        &gateway.authority,
        &[("Origin", &origin)],
        b"",
    );
    assert_eq!(status, 405);
    assert!(!headers.contains_key("access-control-allow-origin"));
    assert_eq!(seen.lock().unwrap().len(), 2);

    // The pinned chain, for the page; no bundle is served without one.
    let (status, _, body) = gateway.send("GET", "/chain", &gateway.authority, &[], b"");
    assert_eq!(status, 200);
    let chain: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(chain["chain_id"], CHAIN);
    assert_eq!(chain["network"], "development");
    assert_eq!(chain["endpoint"], url.as_str());
    assert_eq!(
        gateway.send("GET", "/", &gateway.authority, &[], b"").0,
        404
    );
    drop(gateway);
    std::fs::remove_dir_all(&home).unwrap();
}

#[test]
fn a_pinned_bundle_is_served_and_a_changed_one_is_refused() {
    let home = dir("bundle");
    let (url, _) = node();
    pin_chain(&home, &url);
    let bundle = home.join("wallet");
    std::fs::create_dir_all(bundle.join("pkg")).unwrap();
    std::fs::write(bundle.join("index.html"), b"<script src=app.js></script>").unwrap();
    std::fs::write(bundle.join("app.js"), b"fetch('/rpc')").unwrap();
    std::fs::write(bundle.join("pkg/wallet_bg.wasm"), b"\0asm").unwrap();
    let bundle = bundle.to_str().unwrap();

    let digest = run(&home, &["gateway", "bundle-digest", bundle]);
    assert!(digest.status.success(), "{}", text(&digest));
    let digest: Value = serde_json::from_slice(&digest.stdout).unwrap();
    assert_eq!(digest["files"], 3);
    let digest = digest["bundle_sha256"].as_str().unwrap().to_owned();

    let gateway = Gateway::start(&home, &["--bundle", bundle, "--bundle-sha256", &digest]);
    let get = |path: &str| gateway.send("GET", path, &gateway.authority, &[], b"");
    let (status, headers, body) = get("/");
    assert_eq!(
        (status, body.as_slice()),
        (200, &b"<script src=app.js></script>"[..])
    );
    assert_eq!(headers["content-type"], "text/html; charset=utf-8");
    assert!(headers["content-security-policy"].contains("script-src 'self' 'wasm-unsafe-eval'"));
    assert_eq!(headers["x-frame-options"], "DENY");
    assert_eq!(
        get("/app.js").1["content-type"],
        "text/javascript; charset=utf-8"
    );
    assert_eq!(
        get("/pkg/wallet_bg.wasm").1["content-type"],
        "application/wasm"
    );
    for missing in [
        "/missing.js",
        "/../index.html",
        "/pkg",
        "/%2e%2e/index.html",
    ] {
        assert_eq!(get(missing).0, 404, "{missing}");
    }
    drop(gateway);

    // A changed file changes the digest: nothing is served.
    std::fs::write(home.join("wallet/app.js"), b"steal()").unwrap();
    let refused = run(
        &home,
        &[
            "gateway",
            "serve",
            "--listen",
            "127.0.0.1:0",
            "--bundle",
            bundle,
            "--bundle-sha256",
            &digest,
        ],
    );
    assert!(!refused.status.success());
    assert!(
        text(&refused).contains("not the pinned"),
        "{}",
        text(&refused)
    );
    // A bundle needs its digest.
    let unpinned = run(
        &home,
        &[
            "gateway",
            "serve",
            "--listen",
            "127.0.0.1:0",
            "--bundle",
            bundle,
        ],
    );
    assert!(!unpinned.status.success());
    std::fs::remove_dir_all(&home).unwrap();
}

#[test]
fn the_gateway_listens_only_on_loopback() {
    let home = dir("listen");
    let (url, _) = node();
    pin_chain(&home, &url);
    for listen in ["0.0.0.0:0", "192.0.2.1:4173", "localhost:4173"] {
        let result = run(&home, &["gateway", "serve", "--listen", listen]);
        assert!(!result.status.success(), "{listen}");
    }
    std::fs::remove_dir_all(&home).unwrap();
}

/// One channel exchange as a node's adapter makes it, answering JSON-RPC
/// with the method it received.
fn channel_endpoint() -> EndpointPin {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let identity = Identity::from_seed(&[4; 32]);
    let pin = EndpointPin::new(
        CHAIN,
        &listener.local_addr().unwrap().to_string(),
        identity.public_key(),
    )
    .unwrap();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut header = [0u8; HANDSHAKE_HEADER_LEN];
        stream.read_exact(&mut header).unwrap();
        let mut hello = vec![0; handshake_payload_len(&header, HandshakeType::Hello).unwrap()];
        stream.read_exact(&mut hello).unwrap();
        let (pending, offer) = endpoint_offer(&identity, CHAIN, &hello).unwrap();
        stream.write_all(&offer).unwrap();
        stream.read_exact(&mut header).unwrap();
        let mut finish = vec![0; handshake_payload_len(&header, HandshakeType::Finish).unwrap()];
        stream.read_exact(&mut finish).unwrap();
        let Session { mut sealer, opener } = pending.finish(&finish).unwrap();
        let mut reader = MessageReader::new(opener, MAX_REQUEST_MESSAGE);
        let request = loop {
            let mut bytes = vec![0; reader.wanted()];
            stream.read_exact(&mut bytes).unwrap();
            if let Some(message) = reader.feed(&bytes).unwrap() {
                break Request::decode(&message).unwrap();
            }
        };
        let rpc: Value = serde_json::from_slice(&request.body).unwrap();
        let reply = json!({"jsonrpc":"2.0","id":rpc["id"],"result":{"over":"channel","echo":rpc["method"]}});
        let response = Response {
            status: 200,
            content_type: "application/json".into(),
            cache_control: String::new(),
            body: reply.to_string().into_bytes(),
        };
        stream
            .write_all(&sealer.seal_message(&response.encode().unwrap()).unwrap())
            .unwrap();
    });
    pin
}

#[test]
fn the_gateway_relays_over_the_client_channel() {
    let home = dir("channel");
    let pin = channel_endpoint();
    let pin_file = home.join("pin.json");
    std::fs::write(&pin_file, pin.to_json()).unwrap();
    pin_chain(&home, pin_file.to_str().unwrap());
    let gateway = Gateway::start(&home, &[]);
    let (status, body) = gateway.rpc(
        &[
            ("Content-Type", "application/json"),
            ("Origin", &gateway.origin()),
        ],
        STATUS,
    );
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let reply: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        (
            reply["result"]["over"].as_str(),
            reply["result"]["echo"].as_str()
        ),
        (Some("channel"), Some("status"))
    );
    let (_, _, chain) = gateway.send("GET", "/chain", &gateway.authority, &[], b"");
    let chain: Value = serde_json::from_slice(&chain).unwrap();
    assert!(chain["endpoint"]
        .as_str()
        .unwrap()
        .contains(&pin.fingerprint()));
    drop(gateway);
    std::fs::remove_dir_all(&home).unwrap();
}
