use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use dytallix_client_channel::{client_hello, MAX_RESPONSE_MESSAGE};
use std::os::unix::fs::PermissionsExt;
use tokio::net::{TcpStream, UnixListener};

const NETWORK: &str = "dytallix-adapter-test";

/// A private home (0700, canonical) with `data/`, removed on drop.
struct Home(PathBuf);
impl Home {
    fn new(label: &str) -> Home {
        let path = std::env::temp_dir().join(format!("dyt-ch-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("data")).unwrap();
        std::fs::create_dir_all(path.join("config")).unwrap();
        let path = path.canonicalize().unwrap();
        for dir in [path.clone(), path.join("data")] {
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        Home(path)
    }
    fn seed(&self, bytes: &[u8], mode: u32) -> PathBuf {
        let path = self.0.join(SEED_PATH);
        std::fs::write(&path, bytes).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// An engine stand-in on `data/rpc.sock`: it answers each request with the
/// fields the adapter sent.
fn engine_socket(home: &Home) -> PathBuf {
    let path = home.0.join("data/rpc.sock");
    let listener = UnixListener::bind(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let len = stream.read_u32().await.unwrap() as usize;
            let mut frame = vec![0; len];
            stream.read_exact(&mut frame).await.unwrap();
            let request: serde_json::Value = serde_json::from_slice(&frame).unwrap();
            let body = serde_json::to_vec(&request).unwrap();
            let reply = serde_json::to_vec(&serde_json::json!({
                "version": 1, "status": 200,
                "headers": {"Content-Type": "application/json", "Cache-Control": "no-store"},
                "body_base64": STANDARD.encode(body),
            }))
            .unwrap();
            stream.write_u32(reply.len() as u32).await.unwrap();
            stream.write_all(&reply).await.unwrap();
        }
    });
    path
}

/// Serves `exchange` on a loopback port, one task per connection.
async fn endpoint(identity: Identity, socket: PathBuf, limits: Limits) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let identity = Arc::new(identity);
    let socket = Arc::new(socket);
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let (identity, socket) = (identity.clone(), socket.clone());
            tokio::spawn(async move {
                let _ = exchange(&mut stream, &identity, NETWORK, &socket, &limits).await;
            });
        }
    });
    address
}

/// A client's whole exchange, as the SDK will make it.
async fn call(address: SocketAddr, key: &[u8], request: &Request) -> Result<Response, ()> {
    let mut stream = TcpStream::connect(address).await.map_err(|_| ())?;
    let (start, hello) = client_hello(NETWORK, key).map_err(|_| ())?;
    stream.write_all(&hello).await.map_err(|_| ())?;
    let offer = read_handshake(&mut stream, HandshakeType::Offer).await?;
    let (Session { mut sealer, opener }, mut out) = start.finish(&offer).map_err(|_| ())?;
    out.extend(sealer.seal_message(&request.encode().unwrap()).unwrap());
    stream.write_all(&out).await.map_err(|_| ())?;
    let mut reader = MessageReader::new(opener, MAX_RESPONSE_MESSAGE);
    loop {
        let bytes = read_vec(&mut stream, reader.wanted()).await?;
        if let Some(message) = reader.feed(&bytes).map_err(|_| ())? {
            return Response::decode(&message).map_err(|_| ());
        }
    }
}

fn get(path: &str, query: &str) -> Request {
    Request {
        method: Method::Get,
        path: path.into(),
        query: query.into(),
        body: vec![],
    }
}

fn error_of(response: &Response) -> String {
    let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    body["error"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn requests_cross_the_channel_to_the_engine() {
    let home = Home::new("cross");
    let socket = engine_socket(&home);
    let identity = Identity::from_seed(&[5; 32]);
    let key = identity.public_key().to_vec();
    let address = endpoint(identity, socket, Limits::CEILING).await;

    let response = call(address, &key, &get("/abci_query", "path=%22%2Fstatus%22"))
        .await
        .unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.content_type, "application/json");
    assert_eq!(response.cache_control, "no-store");
    let sent: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(sent["method"], "GET");
    assert_eq!(sent["path"], "/abci_query");
    assert_eq!(sent["query"], "path=%22%2Fstatus%22");
    assert_eq!(sent["body_base64"], "");
    // The engine sees the adapter as a local caller, never the client's own
    // address: its client socket refuses any caller that is not loopback.
    assert_eq!(sent["remote_addr"], "127.0.0.1:1");

    let post = Request {
        method: Method::Post,
        path: "/".into(),
        query: String::new(),
        body: br#"{"jsonrpc":"2.0","id":1,"method":"status"}"#.to_vec(),
    };
    let response = call(address, &key, &post).await.unwrap();
    let sent: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(sent["method"], "POST");
    assert_eq!(
        STANDARD
            .decode(sent["body_base64"].as_str().unwrap())
            .unwrap(),
        post.body
    );
}

#[tokio::test]
async fn a_client_pinning_another_key_gets_nothing() {
    let home = Home::new("pin");
    let socket = engine_socket(&home);
    let address = endpoint(Identity::from_seed(&[5; 32]), socket, Limits::CEILING).await;
    let other = Identity::from_seed(&[6; 32]);
    assert!(call(address, other.public_key(), &get("/status", ""))
        .await
        .is_err());
}

#[tokio::test]
async fn refused_requests_get_the_http_listeners_errors() {
    let home = Home::new("refused");
    let socket = engine_socket(&home);
    let identity = Identity::from_seed(&[5; 32]);
    let key = identity.public_key().to_vec();
    let mut limits = Limits::CEILING;
    limits.max_request_body = 8;
    let address = endpoint(identity, socket, limits).await;

    let response = call(address, &key, &get("/", "")).await.unwrap();
    assert_eq!(response.status, 501);
    let response = call(address, &key, &get("/a-b", "")).await.unwrap();
    assert_eq!(response.status, 400);
    assert_eq!(error_of(&response), "Unsupported request target");
    // A body over the lowered limit is refused before the engine.
    let large = Request {
        method: Method::Post,
        path: "/".into(),
        query: String::new(),
        body: vec![b'x'; 9],
    };
    let response = call(address, &key, &large).await.unwrap();
    assert_eq!(response.status, 413);
}

#[tokio::test]
async fn an_unavailable_engine_is_a_gateway_error() {
    let home = Home::new("down");
    let identity = Identity::from_seed(&[5; 32]);
    let key = identity.public_key().to_vec();
    let address = endpoint(identity, home.0.join("data/rpc.sock"), Limits::CEILING).await;
    let response = call(address, &key, &get("/status", "")).await.unwrap();
    assert_eq!(response.status, 502);
    assert_eq!(error_of(&response), "Engine RPC response failed");
}

#[test]
fn seed_files_are_owner_only_single_link_and_exact() {
    let home = Home::new("seed");
    let path = home.seed(&[7; 32], 0o600);
    assert_eq!(*read_seed(&path).unwrap(), [7; 32]);
    home.seed(&[7; 32], 0o640);
    assert!(read_seed(&path).is_err());
    home.seed(&[7; 31], 0o600);
    assert!(read_seed(&path).is_err());
    home.seed(&[7; 32], 0o600);
    let link = home.0.join("config/linked.bin");
    std::fs::hard_link(&path, &link).unwrap();
    assert!(read_seed(&path).is_err());
    std::fs::remove_file(&link).unwrap();
    let symlink = home.0.join("config/symlink.bin");
    std::os::unix::fs::symlink(&path, &symlink).unwrap();
    assert!(read_seed(&symlink).is_err());
    assert!(read_seed(&home.0.join("config/absent.bin")).is_err());
}

#[test]
fn channel_flags_are_checked_and_complete() {
    let set = |flag: &str, value: &str| ChannelArgs::default().set(flag, Some(value.into()));
    assert!(set("--channel-listen", "203.0.113.7:26670").is_ok());
    assert!(set("--channel-listen", "[2001:db8::7]:26670").is_ok());
    for bad in [
        "0.0.0.0:26670",
        "[::]:26670",
        "224.0.0.1:1",
        "203.0.113.7:0",
        "node:26670",
    ] {
        assert!(set("--channel-listen", bad).is_err(), "{bad}");
    }
    assert!(set("--channel-network", NETWORK).is_ok());
    assert!(set("--channel-network", "").is_err());
    assert!(set("--channel-network", "a b").is_err());
    assert!(set("--channel-network", &"n".repeat(65)).is_err());
    assert!(set("--max-channel-connections", "64").is_ok());
    assert!(set("--max-channel-connections", "65").is_err());
    assert!(set("--max-channel-connections", "0").is_err());
    assert!(set("--max-channel-connections-per-address", "5").is_err());
    assert!(ChannelArgs::default()
        .set("--channel-listen", None)
        .is_err());

    let home = Home::new("flags");
    // No channel flags: no listener, and no seed is read.
    assert!(ChannelArgs::default().finish(&home.0).unwrap().is_none());
    let mut partial = ChannelArgs::default();
    partial
        .set("--channel-listen", Some("127.0.0.1:1".into()))
        .unwrap();
    assert!(partial.finish(&home.0).is_err());
    let mut limits_only = ChannelArgs::default();
    limits_only
        .set("--max-channel-connections", Some("8".into()))
        .unwrap();
    assert!(limits_only.finish(&home.0).is_err());

    let complete = || {
        let mut args = ChannelArgs::default();
        args.set("--channel-listen", Some("127.0.0.1:1".into()))
            .unwrap();
        args.set("--channel-network", Some(NETWORK.into())).unwrap();
        args.set("--max-channel-connections-per-address", Some("2".into()))
            .unwrap();
        args
    };
    assert!(complete().finish(&home.0).is_err(), "no seed yet");
    home.seed(&[8; 32], 0o600);
    let config = complete().finish(&home.0).unwrap().unwrap();
    assert_eq!((config.max_connections, config.max_per_address), (64, 2));
    assert_eq!(
        config.fingerprint(),
        fingerprint(Identity::from_seed(&[8; 32]).public_key())
    );
    assert!(!format!("{config:?}").contains("[8"));

    // The peer transport's seed is a different role key.
    std::fs::write(home.0.join(PEER_SEED_PATH), [8; 32]).unwrap();
    assert!(complete().finish(&home.0).is_err());
}

#[test]
fn each_address_has_its_own_bound() {
    let slots = AddressSlots::new(2);
    let a: IpAddr = "198.51.100.1".parse().unwrap();
    let b: IpAddr = "198.51.100.2".parse().unwrap();
    let first = slots.acquire(a).unwrap();
    let _second = slots.acquire(a).unwrap();
    assert!(slots.acquire(a).is_none());
    let _other = slots.acquire(b).unwrap();
    drop(first);
    let _again = slots.acquire(a).unwrap();
    drop((_second, _other, _again));
    assert!(slots.counts.lock().unwrap().is_empty());
}
